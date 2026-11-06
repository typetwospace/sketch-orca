// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Intel oneTBB code: this file is translated from include/oneapi/tbb/parallel_invoke.h
// (invoke_helper, parallel_invoke_impl, invoke_recursive_separation, invoke_subroot_task and
// function_invoker) and include/oneapi/tbb/task_group.h (task_group_base and task_group: run,
// run_and_wait, wait with its context reset, cancel, and the destructor's cancel-and-wait) of
// oneTBB 2021.5.0 (the tag v2021.5.0 of uxlfoundation/oneTBB), on 2026-11-06. Its copyright
// holder is Intel Corporation, and it is under Apache-2.0 (licenses/Apache-2.0.txt).

//! `tbb::parallel_invoke` and `tbb::task_group`.
//!
//! In a serial arena both run in oneTBB's one-thread order. `parallel_invoke(f0, ..., fn-1)`
//! puts `f0, f1, f2` in a subroot task while more than three functions are left, spawns it, and
//! goes on with the rest; with three or fewer left it spawns all but the last and runs the last
//! itself. A subroot spawns its third function, then its second, and runs its first itself.
//! So four functions run as 3, 0, 1, 2 and seven as 6, 3, 4, 5, 0, 1, 2 ([`invoke_order`]). A
//! task group's `run` spawns its task, and `wait` runs the newest spawned task first.
//!
//! In a parallel arena the functions and the tasks run on the arena's rayon pool in any order.

use std::marker::PhantomData;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::thread::{self, ThreadId};

use crate::concurrency::{
    WaitState, context, run_and_wait_serial, run_leaf, run_task, spawn_parallel, spawn_serial,
    wait_parallel, wait_serial,
};

/// A function of [`parallel_invoke()`].
pub type InvokeFn<'a> = Box<dyn FnOnce() + Send + 'a>;

/// Boxes a function for [`parallel_invoke()`].
pub fn invoke_fn<'a>(f: impl FnOnce() + Send + 'a) -> InvokeFn<'a> {
    Box::new(f)
}

/// `parallel_invoke(f0, f1, ...)`: runs every function and returns when all have run.
///
/// oneTBB takes two functions or more; here one runs by itself and none runs nothing.
/// [`parallel_invoke!`](crate::parallel_invoke!) takes the functions as arguments.
///
/// # Panics
///
/// Resumes the first panic of a function, after the functions that had started have finished;
/// the functions that had not started are skipped.
pub fn parallel_invoke(functions: Vec<InvokeFn<'_>>) {
    let wait = WaitState::new();
    if context().serial {
        invoke_serial(functions, &wait);
    } else {
        rayon::scope(|s| {
            for f in functions {
                let wait = &wait;
                s.spawn(move |_| {
                    run_leaf(wait, f);
                });
            }
        });
    }
    wait.finish();
}

/// `invoke_recursive_separation` on one thread.
fn invoke_serial(functions: Vec<InvokeFn<'_>>, wait: &Arc<WaitState>) {
    let mut functions = functions.into_iter();
    while functions.len() > 3 {
        let (Some(f1), Some(f2), Some(f3)) = (functions.next(), functions.next(), functions.next())
        else {
            break;
        };
        // invoke_subroot_task::execute: spawn the third, spawn the second, run the first.
        let subroot = move || {
            // SAFETY: invoke_serial's caller waits for `wait` before the functions go away.
            unsafe {
                spawn_serial(wait, f3);
                spawn_serial(wait, f2);
            }
            f1();
        };
        // SAFETY: as above.
        unsafe { spawn_serial(wait, Box::new(subroot)) };
    }
    let mut rest: Vec<InvokeFn<'_>> = functions.collect();
    let Some(last) = rest.pop() else { return };
    for f in rest {
        // SAFETY: as above; run_and_wait_serial below is that wait.
        unsafe { spawn_serial(wait, f) };
    }
    run_and_wait_serial(wait, last);
}

/// The order in which [`parallel_invoke()`] runs `n` functions in a serial arena, by their
/// position in the call.
pub fn invoke_order(n: usize) -> Vec<usize> {
    let order = std::sync::Mutex::new(Vec::with_capacity(n));
    crate::concurrency::Arena::serial(1).execute(|| {
        let order = &order;
        parallel_invoke(
            (0..n)
                .map(|i| {
                    invoke_fn(move || {
                        order
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .push(i);
                    })
                })
                .collect(),
        );
    });
    order
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// `parallel_invoke!(f0, f1, ...)`: [`parallel_invoke()`] with the functions as arguments.
#[macro_export]
macro_rules! parallel_invoke {
    ($($function:expr),+ $(,)?) => {
        $crate::parallel_invoke(::std::vec![$($crate::invoke_fn($function)),+])
    };
}

/// What `task_group::wait` returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskGroupStatus {
    /// `complete`: every task ran.
    Complete,
    /// `canceled`: the group was cancelled, and the tasks that had not started were skipped.
    Canceled,
}

/// `tbb::task_group`, alive for the call of [`task_group`] that made it. Tasks may borrow what
/// outlives that call.
pub struct TaskGroup<'scope> {
    wait: Arc<WaitState>,
    /// The thread of a serial arena's group, whose tasks wait on that thread's pool.
    owner: Option<ThreadId>,
    scope: PhantomData<&'scope mut &'scope ()>,
}

/// Runs `f` with a new task group and waits for the group's tasks before returning, as oneTBB
/// requires a `wait()` before a `task_group` goes away. If `f` panics, the group is cancelled
/// and waited for, and the panic goes on, as the destructor of a `task_group` does while the
/// stack unwinds.
///
/// # Panics
///
/// Resumes the panic of `f`, or else the first panic of a task the final wait finds.
pub fn task_group<'scope, T>(f: impl FnOnce(&TaskGroup<'scope>) -> T) -> T {
    let group = TaskGroup {
        wait: WaitState::new(),
        owner: context().serial.then(|| thread::current().id()),
        scope: PhantomData,
    };
    let outcome = panic::catch_unwind(AssertUnwindSafe(|| f(&group)));
    if outcome.is_err() {
        group.wait.cancel();
    }
    group.wait_all();
    let (_, task_panic) = group.wait.reset();
    match outcome {
        Ok(value) => {
            if let Some(payload) = task_panic {
                panic::resume_unwind(payload);
            }
            value
        }
        Err(payload) => panic::resume_unwind(payload),
    }
}

impl<'scope> TaskGroup<'scope> {
    /// `run(f)`: spawns `task`, which gets the group to run more tasks in.
    ///
    /// # Panics
    ///
    /// If the group belongs to a serial arena and the calling thread is not that arena's.
    pub fn run<F>(&self, task: F)
    where
        F: FnOnce(&TaskGroup<'scope>) + Send + 'scope,
    {
        let wait = Arc::clone(&self.wait);
        let owner = self.owner;
        let job = move || {
            task(&TaskGroup {
                wait,
                owner,
                scope: PhantomData,
            })
        };
        match self.owner {
            Some(owner) => {
                assert_eq!(
                    owner,
                    thread::current().id(),
                    "orca-tbb: a serial arena's task group takes tasks on its own thread only"
                );
                // SAFETY: task_group waits for the group on this thread before 'scope ends.
                unsafe { spawn_serial(&self.wait, Box::new(job)) };
            }
            // SAFETY: task_group waits for the group before 'scope ends.
            None => unsafe { spawn_parallel(&self.wait, Box::new(job)) },
        }
    }

    /// `run_and_wait(f)`: runs `task` on the calling thread as a task of the group, then
    /// [`wait`](Self::wait)s.
    ///
    /// # Panics
    ///
    /// As [`wait`](Self::wait).
    pub fn run_and_wait<F>(&self, task: F) -> TaskGroupStatus
    where
        F: FnOnce(&TaskGroup<'scope>),
    {
        if self.owner.is_some() {
            run_and_wait_serial(&self.wait, || task(self));
        } else {
            self.wait.reserve();
            run_task(&self.wait, || task(self));
            wait_parallel(&self.wait);
        }
        self.end_wait()
    }

    /// `wait()`: runs or waits for every task of the group, then resets the group so it can
    /// take more tasks.
    ///
    /// # Panics
    ///
    /// Resumes the first panic of a task. Panics if called from a task of the group itself,
    /// which oneTBB would wait for forever.
    pub fn wait(&self) -> TaskGroupStatus {
        self.wait_all();
        self.end_wait()
    }

    /// `cancel()`: the tasks of the group, and of the groups its tasks start, that have not
    /// started are skipped, and the running loops stop between leaves.
    pub fn cancel(&self) {
        self.wait.cancel();
    }

    /// `is_canceling()`.
    pub fn is_canceling(&self) -> bool {
        self.wait.is_cancelled()
    }

    fn wait_all(&self) {
        if self.owner.is_some() {
            wait_serial(&self.wait);
        } else {
            wait_parallel(&self.wait);
        }
    }

    fn end_wait(&self) -> TaskGroupStatus {
        let (cancelled, task_panic) = self.wait.reset();
        if let Some(payload) = task_panic {
            panic::resume_unwind(payload);
        }
        if cancelled {
            TaskGroupStatus::Canceled
        } else {
            TaskGroupStatus::Complete
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_invoke_order_is_oneapis_recursive_separation() {
        assert_eq!(invoke_order(0), Vec::<usize>::new());
        assert_eq!(invoke_order(1), [0]);
        assert_eq!(invoke_order(2), [1, 0]);
        assert_eq!(invoke_order(3), [2, 1, 0]);
        assert_eq!(invoke_order(4), [3, 0, 1, 2]);
        assert_eq!(invoke_order(7), [6, 3, 4, 5, 0, 1, 2]);
        assert_eq!(invoke_order(10), [9, 6, 7, 8, 3, 4, 5, 0, 1, 2]);
    }
}
