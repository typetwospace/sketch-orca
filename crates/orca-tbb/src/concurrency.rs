// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Intel oneTBB code: this file is translated from src/tbb/misc_ex.cpp
// (initialize_hardware_concurrency_info and AvailableHwConcurrency on Windows), src/tbb/
// governor.h (default_num_threads), src/tbb/arena.cpp (task_arena_impl::max_concurrency),
// src/tbb/misc.h (ThreadStackSize) and the scheduling of src/tbb/task_dispatcher.h
// (local_wait_for_all: a thread runs its own pool's newest task first and stops when the wait
// context it waits on is released; a cancelled context's tasks are not executed) of oneTBB
// 2021.5.0 (the tag v2021.5.0 of uxlfoundation/oneTBB), on 2026-11-06. Its copyright holder is
// Intel Corporation, and it is under Apache-2.0 (licenses/Apache-2.0.txt).

//! How many threads oneTBB sees, the arenas the algorithms run in, and the task machinery under
//! them.
//!
//! [`hardware_concurrency`] is what oneTBB's `AvailableHwConcurrency` reads on Windows: the
//! processors of the process's affinity mask, or, when the mask holds every processor of the
//! machine's group, the active processors of every processor group (up to 64 groups).
//! [`default_concurrency`] (`tbb::info::default_concurrency()`) is that number, and
//! [`max_concurrency`] (`tbb::this_task_arena::max_concurrency()`) is the concurrency of the
//! arena the calling thread is in, or the default outside any arena.
//!
//! An [`Arena`] is `tbb::task_arena` with concurrency P, in one of two modes:
//!
//! - [`Arena::new`] runs its algorithms on a rayon pool of P threads. The partitioners split as
//!   they do for P (see [`crate::partitioner`]), whichever thread takes which part, so a loop's
//!   leaves and a deterministic reduction's tree are the same as on one thread; only the order
//!   in which independent parts run differs.
//! - [`Arena::serial`] runs them on the calling thread, as oneTBB runs an arena of concurrency
//!   P that has one thread (`task_arena(P, P)` under `global_control(max_allowed_parallelism,
//!   1)`, which is how the parity oracle runs the C++ engine). A spawned task goes on the
//!   thread's last-in-first-out pool, a wait runs the newest task of the pool until the tasks it
//!   waits for are done, and a cancelled group's tasks are dropped unrun; so every body, nested
//!   loop, task group and `parallel_invoke` runs in the order the oracle runs it.
//!
//! Outside any arena the algorithms run on rayon's global pool with the default concurrency.
//!
//! A panic in a task is oneTBB's exception: the first one cancels the task's group (the loop, the
//! reduction, the `parallel_invoke` or the task group, and every group started inside its tasks),
//! the group's remaining tasks are dropped, and the wait resumes the panic.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::mem;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

/// `ThreadStackSize` on 64-bit Windows: the stack of oneTBB's worker threads.
const WORKER_STACK_SIZE: usize = 4 * 1024 * 1024;

/// How long a thread that waits for a task group's tasks, with nothing of its own to run,
/// sleeps before it looks for work again. The group's last task wakes it sooner.
const IDLE_WAIT: Duration = Duration::from_millis(1);

/// `AvailableHwConcurrency()`: the number of processors oneTBB uses by default, read once.
pub fn hardware_concurrency() -> usize {
    static COUNT: OnceLock<usize> = OnceLock::new();
    *COUNT.get_or_init(available_hw_concurrency)
}

/// `initialize_hardware_concurrency_info` on Windows: the processors in the process's affinity
/// mask; when the mask holds all of `dwNumberOfProcessors` and the machine has more than one
/// processor group, the active processors of every group instead (`MaxProcessorGroups` is 64).
#[cfg(windows)]
fn available_hw_concurrency() -> usize {
    use windows_sys::Win32::System::SystemInformation::{GetNativeSystemInfo, SYSTEM_INFO};
    use windows_sys::Win32::System::Threading::{
        GetActiveProcessorCount, GetActiveProcessorGroupCount, GetCurrentProcess,
        GetProcessAffinityMask,
    };
    const MAX_PROCESSOR_GROUPS: u16 = 64;

    let mut info = SYSTEM_INFO::default();
    // SAFETY: GetNativeSystemInfo fills in the structure it is given.
    unsafe { GetNativeSystemInfo(&raw mut info) };
    let (mut process_mask, mut system_mask) = (0_usize, 0_usize);
    // SAFETY: the current process's pseudo-handle, and the two masks to fill in.
    let read = unsafe {
        GetProcessAffinityMask(
            GetCurrentProcess(),
            &raw mut process_mask,
            &raw mut system_mask,
        )
    };
    let processors = info.dwNumberOfProcessors as usize;
    // oneTBB does not look at the result; a mask that could not be read is taken as all of
    // the processors.
    let in_mask = if read == 0 {
        processors
    } else {
        process_mask.count_ones() as usize
    };
    let mut total = in_mask;
    if in_mask == processors {
        // SAFETY: no arguments.
        let groups = unsafe { GetActiveProcessorGroupCount() }.min(MAX_PROCESSOR_GROUPS);
        if groups > 1 {
            total = 0;
            for group in 0..groups {
                // SAFETY: a group number below the number of active groups.
                total += unsafe { GetActiveProcessorCount(group) } as usize;
            }
        }
    }
    if total < 1 { 1 } else { total }
}

/// The processors the standard library sees, where oneTBB's Windows reading does not apply.
#[cfg(not(windows))]
fn available_hw_concurrency() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZero::get)
}

/// `tbb::info::default_concurrency()`: [`hardware_concurrency`], as `governor::default_num_threads`
/// is when no NUMA node or core type is asked for.
pub fn default_concurrency() -> usize {
    hardware_concurrency()
}

/// `tbb::this_task_arena::max_concurrency()`: the concurrency of the arena the calling thread
/// runs in, or [`default_concurrency`] outside any arena. The partitioners split for this number.
pub fn max_concurrency() -> usize {
    context().concurrency
}

/// The arena a thread runs in: its concurrency, and whether it runs one thread's way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Context {
    pub(crate) concurrency: usize,
    pub(crate) serial: bool,
}

/// A task of a serial arena: its job and the group it counts in.
struct Task {
    wait: Arc<WaitState>,
    job: Box<dyn FnOnce()>,
}

thread_local! {
    /// The arena this thread runs in; none outside any arena.
    static CONTEXT: Cell<Option<Context>> = const { Cell::new(None) };
    /// A serial arena's task pool: spawned tasks, the newest last.
    static STACK: RefCell<Vec<Task>> = const { RefCell::new(Vec::new()) };
    /// The groups of the tasks this thread is running, the innermost last.
    static RUNNING: RefCell<Vec<Arc<WaitState>>> = const { RefCell::new(Vec::new()) };
}

/// The calling thread's arena.
pub(crate) fn context() -> Context {
    CONTEXT.get().unwrap_or_else(|| Context {
        concurrency: default_concurrency(),
        serial: false,
    })
}

/// `tbb::task_arena`: an arena of concurrency P that algorithms run in through
/// [`Arena::execute`].
pub struct Arena {
    concurrency: usize,
    pool: Option<rayon::ThreadPool>,
}

impl Arena {
    /// An arena of concurrency `concurrency` on its own rayon pool of that many threads, with
    /// oneTBB's 4 MiB worker stacks.
    ///
    /// # Errors
    ///
    /// If rayon cannot start the threads.
    ///
    /// # Panics
    ///
    /// If `concurrency` is 0.
    pub fn new(concurrency: usize) -> Result<Self, rayon::ThreadPoolBuildError> {
        assert!(
            concurrency > 0,
            "orca-tbb: an arena runs at least one thread"
        );
        let context = Context {
            concurrency,
            serial: false,
        };
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(concurrency)
            .stack_size(WORKER_STACK_SIZE)
            .thread_name(|i| format!("orca-tbb-{i}"))
            .start_handler(move |_| CONTEXT.set(Some(context)))
            .build()?;
        Ok(Arena {
            concurrency,
            pool: Some(pool),
        })
    }

    /// An arena of concurrency `concurrency` run by the calling thread alone, task for task as
    /// oneTBB runs such an arena with one thread.
    ///
    /// # Panics
    ///
    /// If `concurrency` is 0.
    pub fn serial(concurrency: usize) -> Self {
        assert!(
            concurrency > 0,
            "orca-tbb: an arena runs at least one thread"
        );
        Arena {
            concurrency,
            pool: None,
        }
    }

    /// The arena's concurrency, its `max_concurrency()`.
    pub fn concurrency(&self) -> usize {
        self.concurrency
    }

    /// Whether the arena runs on the calling thread alone.
    pub fn is_serial(&self) -> bool {
        self.pool.is_none()
    }

    /// `task_arena::execute`: runs `f` in the arena and returns what it returns. The
    /// algorithms `f` calls split for the arena's concurrency and run in the arena's mode.
    pub fn execute<T: Send>(&self, f: impl FnOnce() -> T + Send) -> T {
        match &self.pool {
            Some(pool) => pool.install(f),
            None => {
                let _entered = SerialEntry::enter(self.concurrency);
                f()
            }
        }
    }
}

/// The calling thread's state outside a serial arena it has entered, put back when it leaves.
struct SerialEntry {
    context: Option<Context>,
    stack: Vec<Task>,
    running: Vec<Arc<WaitState>>,
}

impl SerialEntry {
    fn enter(concurrency: usize) -> Self {
        SerialEntry {
            context: CONTEXT.replace(Some(Context {
                concurrency,
                serial: true,
            })),
            stack: STACK.take(),
            running: RUNNING.take(),
        }
    }
}

impl Drop for SerialEntry {
    fn drop(&mut self) {
        // Every wait in the arena has finished; a task still here was given to a task group of
        // another arena from inside this one, and runs before the thread leaves, as it would in
        // the arena it was spawned in.
        while let Some(task) = STACK.with_borrow_mut(Vec::pop) {
            run_task(&task.wait, task.job);
        }
        STACK.set(mem::take(&mut self.stack));
        RUNNING.set(mem::take(&mut self.running));
        CONTEXT.set(self.context);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A group of tasks waited for together, with its cancellation: oneTBB's `wait_context` and
/// `task_group_context` in one.
pub(crate) struct WaitState {
    /// Tasks spawned (or started) and not yet finished.
    pending: AtomicUsize,
    /// `cancel_group_execution()` was called on this group.
    cancelled: AtomicBool,
    /// The panic that cancelled the group, resumed by its wait.
    panic: Mutex<Option<Box<dyn Any + Send>>>,
    /// The group of the task that started this one: its cancellation is this group's too, as a
    /// oneTBB context is bound to the context of the task that creates it.
    parent: Option<Arc<WaitState>>,
    lock: Mutex<()>,
    done: Condvar,
}

impl WaitState {
    /// A new group, bound to the group of the task the calling thread is running.
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(WaitState {
            pending: AtomicUsize::new(0),
            cancelled: AtomicBool::new(false),
            panic: Mutex::new(None),
            parent: RUNNING.with_borrow(|running| running.last().cloned()),
            lock: Mutex::new(()),
            done: Condvar::new(),
        })
    }

    /// `wait_context::reserve()`.
    pub(crate) fn reserve(&self) {
        self.pending.fetch_add(1, Ordering::AcqRel);
    }

    /// `wait_context::release()`.
    pub(crate) fn release(&self) {
        if self.pending.fetch_sub(1, Ordering::AcqRel) == 1 {
            let _guard = lock(&self.lock);
            self.done.notify_all();
        }
    }

    fn pending(&self) -> usize {
        self.pending.load(Ordering::Acquire)
    }

    /// `cancel_group_execution()`: true if this call cancelled the group.
    pub(crate) fn cancel(&self) -> bool {
        !self.is_cancelled() && !self.cancelled.swap(true, Ordering::AcqRel)
    }

    /// `is_group_execution_cancelled()`: this group or a group it is bound to was cancelled.
    pub(crate) fn is_cancelled(&self) -> bool {
        let mut group = Some(self);
        while let Some(g) = group {
            if g.cancelled.load(Ordering::Acquire) {
                return true;
            }
            group = g.parent.as_deref();
        }
        false
    }

    /// A task panicked: the panic cancels the group and is kept for its wait, unless the group
    /// was cancelled already, as oneTBB keeps the exception of the task that cancelled the group.
    fn fail(&self, payload: Box<dyn Any + Send>) {
        if self.cancel() {
            *lock(&self.panic) = Some(payload);
        }
    }

    /// The end of a wait: whether the group was cancelled, and the panic that cancelled it.
    /// Resets the group for reuse, as `task_group::wait` resets its context.
    pub(crate) fn reset(&self) -> (bool, Option<Box<dyn Any + Send>>) {
        let cancelled = self.is_cancelled();
        let panic = lock(&self.panic).take();
        self.cancelled.store(false, Ordering::Release);
        (cancelled, panic)
    }

    /// The end of an algorithm's wait: resumes the panic that cancelled the group, if any.
    pub(crate) fn finish(&self) {
        if let (_, Some(payload)) = self.reset() {
            panic::resume_unwind(payload);
        }
    }
}

/// Runs a task of `wait`'s group: its job, unless the group is cancelled (oneTBB then calls
/// the task's `cancel` instead of `execute`); a panic cancels the group. Then the task counts
/// as finished. The job is consumed or dropped before that.
pub(crate) fn run_task(wait: &Arc<WaitState>, job: impl FnOnce()) {
    if wait.is_cancelled() {
        drop(job);
    } else {
        RUNNING.with_borrow_mut(|running| running.push(Arc::clone(wait)));
        let outcome = panic::catch_unwind(AssertUnwindSafe(job));
        RUNNING.with_borrow_mut(Vec::pop);
        if let Err(payload) = outcome {
            wait.fail(payload);
        }
    }
    wait.release();
}

/// Runs one leaf of a loop of `wait`'s group on a parallel arena, unless the group is
/// cancelled; a panic cancels the group. False if the leaf did not run.
pub(crate) fn run_leaf(wait: &Arc<WaitState>, leaf: impl FnOnce()) -> bool {
    if wait.is_cancelled() {
        return false;
    }
    RUNNING.with_borrow_mut(|running| running.push(Arc::clone(wait)));
    let outcome = panic::catch_unwind(AssertUnwindSafe(leaf));
    RUNNING.with_borrow_mut(Vec::pop);
    if let Err(payload) = outcome {
        wait.fail(payload);
    }
    true
}

/// Panics if the calling thread runs a task of `wait`'s own group: oneTBB would wait forever
/// for the task that waits.
fn refuse_own_wait(wait: &Arc<WaitState>) {
    let own = RUNNING.with_borrow(|running| running.iter().any(|g| Arc::ptr_eq(g, wait)));
    assert!(
        !own,
        "orca-tbb: a task waited for its own task group, which never finishes"
    );
}

/// `spawn(task)` in a serial arena: puts `job` on the calling thread's task pool as a task of
/// `wait`'s group.
///
/// # Safety
///
/// The caller waits for `wait` (with [`wait_serial`]) on this thread before anything `job`
/// borrows goes away, on return and on unwinding alike.
pub(crate) unsafe fn spawn_serial<'a>(wait: &Arc<WaitState>, job: Box<dyn FnOnce() + 'a>) {
    // SAFETY: only the lifetime changes. The job runs or is dropped within the caller's wait,
    // which ends before 'a does.
    let job = unsafe { mem::transmute::<Box<dyn FnOnce() + 'a>, Box<dyn FnOnce()>>(job) };
    wait.reserve();
    STACK.with_borrow_mut(|stack| {
        stack.push(Task {
            wait: Arc::clone(wait),
            job,
        })
    });
}

/// `wait(wait_context)` in a serial arena: runs the newest task of the calling thread's pool,
/// whatever group it belongs to, until `wait`'s group has no task left.
pub(crate) fn wait_serial(wait: &Arc<WaitState>) {
    refuse_own_wait(wait);
    while wait.pending() > 0 {
        let task = STACK
            .with_borrow_mut(Vec::pop)
            .expect("orca-tbb: a serial arena waits for a task that is not in its pool");
        run_task(&task.wait, task.job);
    }
}

/// `execute_and_wait(task)` in a serial arena: runs `job` first, as a task of `wait`'s group,
/// then waits for the group.
pub(crate) fn run_and_wait_serial(wait: &Arc<WaitState>, job: impl FnOnce()) {
    wait.reserve();
    run_task(wait, job);
    wait_serial(wait);
}

/// `spawn(task)` in a parallel arena: hands `job` to the current rayon pool as a task of
/// `wait`'s group.
///
/// # Safety
///
/// The caller waits for `wait` (with [`wait_parallel`]) before anything `job` borrows goes
/// away, on return and on unwinding alike.
pub(crate) unsafe fn spawn_parallel<'a>(wait: &Arc<WaitState>, job: Box<dyn FnOnce() + Send + 'a>) {
    // SAFETY: only the lifetime changes. The job runs or is dropped before its task is
    // released, and the caller's wait ends only after that, before 'a does.
    let job =
        unsafe { mem::transmute::<Box<dyn FnOnce() + Send + 'a>, Box<dyn FnOnce() + Send>>(job) };
    wait.reserve();
    let wait = Arc::clone(wait);
    rayon::spawn(move || run_task(&wait, job));
}

/// `wait(wait_context)` in a parallel arena: helps the pool with its work until `wait`'s group
/// has no task left, and sleeps when there is nothing to help with.
pub(crate) fn wait_parallel(wait: &Arc<WaitState>) {
    refuse_own_wait(wait);
    while wait.pending() > 0 {
        if rayon::yield_now() == Some(rayon::Yield::Executed) {
            continue;
        }
        let guard = lock(&wait.lock);
        if wait.pending() == 0 {
            break;
        }
        drop(
            wait.done
                .wait_timeout(guard, IDLE_WAIT)
                .unwrap_or_else(PoisonError::into_inner),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_concurrency_is_the_hardware_concurrency() {
        assert!(hardware_concurrency() >= 1);
        assert_eq!(default_concurrency(), hardware_concurrency());
    }

    /// oneTBB asserts that the processor groups add up to every active processor when the
    /// process may run on all of them.
    #[cfg(windows)]
    #[test]
    fn the_groups_add_up_to_every_active_processor() {
        use windows_sys::Win32::System::Threading::{
            ALL_PROCESSOR_GROUPS, GetActiveProcessorCount, GetCurrentProcess,
            GetProcessAffinityMask,
        };
        let (mut process_mask, mut system_mask) = (0_usize, 0_usize);
        // SAFETY: the current process's pseudo-handle, and the two masks to fill in.
        let read = unsafe {
            GetProcessAffinityMask(
                GetCurrentProcess(),
                &raw mut process_mask,
                &raw mut system_mask,
            )
        };
        assert_ne!(read, 0);
        if process_mask == system_mask {
            // SAFETY: ALL_PROCESSOR_GROUPS asks for the processors of every group.
            let all = unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) } as usize;
            assert_eq!(hardware_concurrency(), all);
        } else {
            assert_eq!(hardware_concurrency(), process_mask.count_ones() as usize);
        }
    }

    #[test]
    fn an_arena_reports_its_concurrency() {
        assert_eq!(Arena::serial(3).execute(max_concurrency), 3);
        assert_eq!(
            Arena::new(2).expect("a pool of 2").execute(max_concurrency),
            2
        );
        assert_eq!(max_concurrency(), default_concurrency());
        // An arena inside an arena, and back.
        let outer = Arena::serial(5);
        let inner = Arena::serial(7);
        let seen = outer.execute(|| {
            (
                max_concurrency(),
                inner.execute(max_concurrency),
                max_concurrency(),
            )
        });
        assert_eq!(seen, (5, 7, 5));
    }

    #[test]
    fn a_cancelled_group_cancels_the_groups_bound_to_it() {
        let outer = WaitState::new();
        RUNNING.with_borrow_mut(|r| r.push(Arc::clone(&outer)));
        let inner = WaitState::new();
        RUNNING.with_borrow_mut(Vec::pop);
        assert!(!inner.is_cancelled());
        assert!(outer.cancel());
        assert!(!outer.cancel());
        assert!(inner.is_cancelled());
        // A panic in a group already cancelled is not kept.
        inner.fail(Box::new(1_u8));
        assert!(inner.reset().1.is_none());
        assert!(outer.reset().0);
        assert!(!outer.is_cancelled());
    }
}
