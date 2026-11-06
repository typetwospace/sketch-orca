// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Intel oneTBB code: this file is translated from include/oneapi/tbb/parallel_for.h
// (start_for::run, execute and offer_work, parallel_for over a range, parallel_for_impl and
// parallel_for_body_wrapper for the integer forms) and include/oneapi/tbb/parallel_for_each.h
// (for_each_root_task for random-access iterators and parallel_for_body_wrapper, which run
// parallel_for over blocked_range<size_t>) of oneTBB 2021.5.0 (the tag v2021.5.0 of
// uxlfoundation/oneTBB), on 2026-11-06. Its copyright holder is Intel Corporation, and it is
// under Apache-2.0 (licenses/Apache-2.0.txt).

//! `tbb::parallel_for` and `tbb::parallel_for_each`.
//!
//! The body runs once on each leaf of the partitioner's split tree (see
//! [`crate::partitioner`]). In a serial arena the leaves run left to right on the calling
//! thread, interleaved with whatever else is on its task pool exactly as oneTBB interleaves
//! them; in a parallel arena they run on the arena's rayon pool in any order.

use std::sync::Arc;

use crate::concurrency::{WaitState, context, run_and_wait_serial, run_leaf, spawn_serial};
use crate::partitioner::{PartTask, Partitioner};
use crate::range::{BlockedRange, BlockedValue, Range};

/// `parallel_for(range, body, partitioner)`: runs `body` on each leaf of `range`. Nothing runs
/// for an empty range.
///
/// # Panics
///
/// Resumes the first panic of `body`, after the leaves that had started have finished; the
/// leaves that had not started are skipped.
pub fn parallel_for<R, F>(range: R, partitioner: Partitioner, body: F)
where
    R: Range + Send,
    F: Fn(R) + Sync,
{
    if range.is_empty() {
        return;
    }
    let context = context();
    let root = PartTask::new(range, partitioner, context.concurrency);
    let wait = WaitState::new();
    if context.serial {
        run_and_wait_serial(&wait, || for_serial(root, &body, &wait));
    } else {
        for_parallel(root, &body, &wait);
    }
    wait.finish();
}

/// `start_for::execute` in a serial arena: split, spawning each right part, then run the leaves.
fn for_serial<'a, R: Range + 'a, F: Fn(R)>(
    mut task: PartTask<R>,
    body: &'a F,
    wait: &'a Arc<WaitState>,
) {
    task.start();
    while task.wants_split() {
        let child = task.split_child();
        // SAFETY: parallel_for waits for `wait` before `body` and `wait` go away.
        unsafe { spawn_serial(wait, Box::new(move || for_serial(child, body, wait))) };
    }
    for leaf in task.into_leaves() {
        body(leaf);
        if wait.is_cancelled() {
            break;
        }
    }
}

/// `start_for::execute` in a parallel arena: the same splits, with each right part handed to
/// the rayon pool and the leaves run here.
fn for_parallel<R: Range + Send, F: Fn(R) + Sync>(
    mut task: PartTask<R>,
    body: &F,
    wait: &Arc<WaitState>,
) {
    task.start();
    let mut children = Vec::new();
    while task.wants_split() {
        children.push(task.split_child());
    }
    let run_own = |task: PartTask<R>| {
        for leaf in task.into_leaves() {
            if !run_leaf(wait, || body(leaf)) {
                break;
            }
        }
    };
    if children.is_empty() {
        run_own(task);
    } else {
        rayon::scope(|s| {
            for child in children {
                s.spawn(move |_| for_parallel(child, body, wait));
            }
            run_own(task);
        });
    }
}

/// `parallel_for(first, last, f, partitioner)`: `f(i)` for each `i` in `[first, last)`.
///
/// # Panics
///
/// As [`parallel_for`].
pub fn parallel_for_index<I, F>(first: I, last: I, partitioner: Partitioner, f: F)
where
    I: BlockedValue,
    F: Fn(I) + Sync,
{
    parallel_for_index_step(first, last, I::ONE, partitioner, f);
}

/// `parallel_for(first, last, step, f, partitioner)`: `f(first + k * step)` for each `k` from
/// 0 while the value is below `last`, as `parallel_for` over
/// `blocked_range(0, (last - first - 1) / step + 1)` with a grain of 1.
///
/// # Panics
///
/// If `step` is not positive (oneTBB throws `std::invalid_argument`), and as [`parallel_for`].
pub fn parallel_for_index_step<I, F>(first: I, last: I, step: I, partitioner: Partitioner, f: F)
where
    I: BlockedValue,
    F: Fn(I) + Sync,
{
    assert!(
        step > I::ZERO,
        "orca-tbb: parallel_for's step must be positive"
    );
    if first < last {
        let end = I::iteration_count(first, last, step);
        parallel_for(
            BlockedRange::new(I::ZERO, end, 1),
            partitioner,
            |r: BlockedRange<I>| {
                let mut k = I::nth(first, r.begin(), step);
                let mut i = r.begin();
                while i < r.end() {
                    f(k);
                    i = I::plus(i, I::ONE);
                    k = I::plus(k, step);
                }
            },
        );
    }
}

/// `parallel_for_each(first, last, body)` over a slice (a random-access range): `parallel_for`
/// over `blocked_range<size_t>(0, n)` with the auto partitioner, `body` on each item of a leaf
/// in order.
///
/// # Panics
///
/// As [`parallel_for`].
pub fn parallel_for_each<T, F>(items: &[T], body: F)
where
    T: Sync,
    F: Fn(&T) + Sync,
{
    parallel_for(
        BlockedRange::new(0, items.len(), 1),
        Partitioner::Auto,
        |r: BlockedRange<usize>| {
            for item in &items[r.begin()..r.end()] {
                body(item);
            }
        },
    );
}

/// A slice's start, shared by the leaves of a loop that each take their own part of it.
struct SliceStart<T>(*mut T);

// SAFETY: the leaves of a range are disjoint, so the tasks that hold a SliceStart reach
// disjoint items; sending them across threads is sending the items, which are Send.
unsafe impl<T: Send> Sync for SliceStart<T> {}

impl<T> SliceStart<T> {
    fn get(&self) -> *mut T {
        self.0
    }
}

/// `parallel_for_each(first, last, body)` over a mutable slice: as [`parallel_for_each`], with
/// `body` given each item to change.
///
/// # Panics
///
/// As [`parallel_for`].
pub fn parallel_for_each_mut<T, F>(items: &mut [T], body: F)
where
    T: Send,
    F: Fn(&mut T) + Sync,
{
    let len = items.len();
    let start = SliceStart(items.as_mut_ptr());
    parallel_for(
        BlockedRange::new(0, len, 1),
        Partitioner::Auto,
        |r: BlockedRange<usize>| {
            // SAFETY: the leaves are disjoint parts of 0..len, and `items` stays borrowed mutably
            // until every leaf has run, so each leaf is the only one to reach its items.
            let part =
                unsafe { std::slice::from_raw_parts_mut(start.get().add(r.begin()), r.size()) };
            for item in part {
                body(item);
            }
        },
    );
}
