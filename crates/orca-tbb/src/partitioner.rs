// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Intel oneTBB code: this file is translated from include/oneapi/tbb/partitioner.h
// (partition_type_base::execute, adaptive_mode, proportional_mode, proportion_helper,
// linear_affinity_mode, dynamic_grainsize_mode with its work balance, the range pool
// range_vector, auto_partition_type, simple_partition_type and static_partition_type, and the
// constants __TBB_INITIAL_CHUNKS, __TBB_RANGE_POOL_CAPACITY and __TBB_INIT_DEPTH) of oneTBB
// 2021.5.0 (the tag v2021.5.0 of uxlfoundation/oneTBB), decision for decision as one thread
// takes them, on 2026-11-06. Its copyright holder is Intel Corporation, and it is under
// Apache-2.0 (licenses/Apache-2.0.txt).

//! oneTBB's `auto_partitioner`, `simple_partitioner` and `static_partitioner`, as the split tree
//! each gives for an arena of concurrency P.
//!
//! A task of a oneTBB loop first splits its range while its partitioner allows, handing each
//! right part to a new task (`offer_work`, which spawns it), and then runs what is left (the
//! work balance). The split tree here is the one a single thread produces in an arena of
//! concurrency P, which is how the parity oracle runs the C++ engine: no task is ever stolen,
//! so `check_being_stolen` and `check_for_demand` never fire, and the tree depends on the range,
//! the partitioner and P only. With several threads oneTBB's auto partitioner splits further
//! when work is stolen; here it does not, so the leaves are the same whatever the threads do.
//!
//! - The simple partitioner splits while the range is divisible, so every leaf is at most the
//!   grain size.
//! - The static partitioner starts with a divisor of P and splits off a part for `divisor / 2`
//!   of the threads at a time, in proportion when the range can split in proportion, until each
//!   task has one thread: P leaves at most, and no work balance.
//! - The auto partitioner starts with a divisor of 2P (`__TBB_INITIAL_CHUNKS` chunks per thread)
//!   and halves it at each split; a task whose divisor has run out splits once more (lowering
//!   its depth budget, which starts at `__TBB_INIT_DEPTH`), and then runs its range through the
//!   range pool: a stack of up to `__TBB_RANGE_POOL_CAPACITY` ranges that splits the back range
//!   until it is too deep or not divisible, runs it and pops it.
//!
//! On one thread a task's spawned parts wait on a last-in-first-out pool, so the leaves run left
//! to right: a task's own leaves first, then the part split off last, and so on.

use crate::range::Range;

/// `__TBB_INITIAL_CHUNKS`: the auto partitioner's chunks per thread.
const INITIAL_CHUNKS: usize = 2;
/// `__TBB_RANGE_POOL_CAPACITY`: the ranges the auto partitioner's range pool holds.
const RANGE_POOL_CAPACITY: usize = 8;
/// `__TBB_INIT_DEPTH`: the auto partitioner's initial depth budget.
const INIT_DEPTH: u8 = 5;

/// The partitioner of a loop or a reduction: `tbb::auto_partitioner` (oneTBB's default),
/// `tbb::simple_partitioner` or `tbb::static_partitioner`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Partitioner {
    /// `auto_partitioner`, the default of `parallel_for` and `parallel_reduce`.
    #[default]
    Auto,
    /// `simple_partitioner`: split down to the grain size.
    Simple,
    /// `static_partitioner`: one part per thread.
    Static,
}

/// The partitioners `parallel_deterministic_reduce` takes: `simple_partitioner` (its default)
/// and `static_partitioner`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeterministicPartitioner {
    /// `simple_partitioner`, the default of `parallel_deterministic_reduce`.
    #[default]
    Simple,
    /// `static_partitioner`.
    Static,
}

impl From<DeterministicPartitioner> for Partitioner {
    fn from(p: DeterministicPartitioner) -> Self {
        match p {
            DeterministicPartitioner::Simple => Partitioner::Simple,
            DeterministicPartitioner::Static => Partitioner::Static,
        }
    }
}

/// A task's partition state (`task_partition_type`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// `auto_partition_type`: `my_divisor` and `my_max_depth`.
    Auto { divisor: usize, max_depth: u8 },
    /// `simple_partition_type`.
    Simple,
    /// `static_partition_type`: `my_divisor`, the threads this task's range is for.
    Static { divisor: usize },
}

impl Partitioner {
    /// The root task's state for an arena of concurrency `p`. `adaptive_mode` starts from
    /// `get_initial_auto_partitioner_divisor() / 4`, which is `4 * max_concurrency() / 4`.
    fn initial(self, p: usize) -> State {
        match self {
            Partitioner::Auto => State::Auto {
                divisor: p * INITIAL_CHUNKS,
                max_depth: INIT_DEPTH,
            },
            Partitioner::Simple => State::Simple,
            Partitioner::Static => State::Static { divisor: p },
        }
    }
}

/// One task of a loop: its range and its partition state (`start_for`, `start_reduce` and
/// `start_deterministic_reduce` without their bodies).
pub(crate) struct PartTask<R> {
    range: R,
    state: State,
}

impl<R: Range> PartTask<R> {
    /// The root task over `range` for an arena of concurrency `p`.
    pub(crate) fn new(range: R, partitioner: Partitioner, p: usize) -> Self {
        PartTask {
            range,
            state: partitioner.initial(p),
        }
    }

    /// `check_being_stolen` at the start of `execute`: never stolen on one thread, so only
    /// `dynamic_grainsize_mode`'s reset of a spent divisor to 1 remains.
    pub(crate) fn start(&mut self) {
        if let State::Auto { divisor, .. } = &mut self.state
            && *divisor == 0
        {
            *divisor = 1;
        }
    }

    /// The condition of `partition_type_base::execute`'s split loop,
    /// `range.is_divisible() && self().is_divisible()`, with the auto partitioner's side effect.
    pub(crate) fn wants_split(&mut self) -> bool {
        if !self.range.is_divisible() {
            return false;
        }
        match &mut self.state {
            State::Simple => true,
            State::Static { divisor } => *divisor > 1,
            State::Auto { divisor, max_depth } => {
                if *divisor > 1 {
                    true
                } else if *divisor != 0 && *max_depth != 0 {
                    // Split once more for the local pool, lowering the depth budget.
                    *max_depth -= 1;
                    *divisor = 0;
                    true
                } else {
                    false
                }
            }
        }
    }

    /// `offer_work`'s right child: splits the range and the partition state, and returns the
    /// task for the second part, which the caller spawns.
    pub(crate) fn split_child(&mut self) -> PartTask<R> {
        match &mut self.state {
            State::Simple => PartTask {
                range: self.range.split(),
                state: State::Simple,
            },
            State::Auto { divisor, max_depth } => {
                let range = self.range.split();
                // adaptive_mode::do_split: src.my_divisor /= 2u, and the child takes it.
                *divisor /= 2;
                PartTask {
                    range,
                    state: State::Auto {
                        divisor: *divisor,
                        max_depth: *max_depth,
                    },
                }
            }
            State::Static { divisor } => {
                // proportion_helper: left = n - n / 2, right = n / 2 for a range that splits in
                // proportion, and (1, 1) with an even split for one that does not.
                let (left, right) = if R::PROPORTIONAL {
                    let right = *divisor / 2;
                    (*divisor - right, right)
                } else {
                    (1, 1)
                };
                let range = if R::PROPORTIONAL {
                    self.range.split_proportional(left, right)
                } else {
                    self.range.split()
                };
                // proportional_mode::do_split: the child takes right() threads (factor 1).
                *divisor -= right;
                PartTask {
                    range,
                    state: State::Static { divisor: right },
                }
            }
        }
    }

    /// The work balance after the splits: the leaves this task runs itself, in order.
    pub(crate) fn into_leaves(self) -> Leaves<R> {
        match self.state {
            State::Auto { max_depth, .. } if self.range.is_divisible() && max_depth != 0 => {
                Leaves::Pool {
                    pool: vec![(self.range, 0)],
                    max_depth,
                }
            }
            _ => Leaves::One(Some(self.range)),
        }
    }
}

/// The leaves a task runs itself: its whole range, or what the auto partitioner's range pool
/// (`range_vector`) hands out.
pub(crate) enum Leaves<R> {
    /// `run_body(range)`.
    One(Option<R>),
    /// The range pool: ranges with their depth, the back one last.
    Pool { pool: Vec<(R, u8)>, max_depth: u8 },
}

impl<R: Range> Iterator for Leaves<R> {
    type Item = R;

    fn next(&mut self) -> Option<R> {
        match self {
            Leaves::One(range) => range.take(),
            Leaves::Pool { pool, max_depth } => {
                // split_to_fill: split the back range into its two halves, the first half at the
                // back, while the pool has room and the back range is shallow and divisible.
                while pool.len() < RANGE_POOL_CAPACITY {
                    match pool.last() {
                        Some((back, depth)) if *depth < *max_depth && back.is_divisible() => {}
                        _ => break,
                    }
                    let (mut first, depth) = pool.pop()?;
                    let second = first.split();
                    pool.push((second, depth + 1));
                    pool.push((first, depth + 1));
                }
                // run_body(range_pool.back()); range_pool.pop_back().
                pool.pop().map(|(range, _)| range)
            }
        }
    }
}

/// Every leaf of a task's subtree in the order one thread runs them: the task's own leaves,
/// then its spawned parts from the last spawned to the first.
pub(crate) fn walk<R: Range>(mut task: PartTask<R>, visit: &mut impl FnMut(R)) {
    task.start();
    let mut children = Vec::new();
    while task.wants_split() {
        children.push(task.split_child());
    }
    for leaf in task.into_leaves() {
        visit(leaf);
    }
    for child in children.into_iter().rev() {
        walk(child, visit);
    }
}

/// The leaves `partitioner` cuts `range` into in an arena of concurrency `concurrency`, in the
/// order one thread runs them (which is left to right). An empty range has none, as
/// `start_for::run` runs nothing for it.
pub fn leaves<R: Range>(range: R, partitioner: Partitioner, concurrency: usize) -> Vec<R> {
    let mut out = Vec::new();
    if !range.is_empty() {
        walk(
            PartTask::new(range, partitioner, concurrency),
            &mut |leaf| out.push(leaf),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::range::BlockedRange;

    fn bounds(leaves: &[BlockedRange<i64>]) -> Vec<(i64, i64)> {
        leaves.iter().map(|r| (r.begin(), r.end())).collect()
    }

    #[test]
    fn the_simple_partitioner_splits_down_to_the_grain() {
        let l = leaves(BlockedRange::new(0_i64, 10, 3), Partitioner::Simple, 4);
        assert_eq!(bounds(&l), [(0, 2), (2, 5), (5, 7), (7, 10)]);
    }

    #[test]
    fn the_static_partitioner_gives_each_thread_one_part() {
        let l = leaves(BlockedRange::new(0_i64, 100, 1), Partitioner::Static, 3);
        assert_eq!(bounds(&l), [(0, 33), (33, 67), (67, 100)]);
        // Fewer values than threads: one leaf per value.
        let l = leaves(BlockedRange::new(0_i64, 2, 1), Partitioner::Static, 4);
        assert_eq!(bounds(&l), [(0, 1), (1, 2)]);
    }

    /// A range without the proportional split: halves only.
    struct Halves(BlockedRange<i64>);

    impl Range for Halves {
        fn is_empty(&self) -> bool {
            self.0.is_empty()
        }
        fn is_divisible(&self) -> bool {
            self.0.is_divisible()
        }
        fn split(&mut self) -> Self {
            Halves(self.0.split())
        }
    }

    #[test]
    fn the_static_partitioner_peels_one_thread_at_a_time_off_a_range_that_only_halves() {
        let l = leaves(
            Halves(BlockedRange::new(0_i64, 64, 1)),
            Partitioner::Static,
            4,
        );
        let l: Vec<_> = l.into_iter().map(|h| h.0).collect();
        assert_eq!(bounds(&l), [(0, 8), (8, 16), (16, 32), (32, 64)]);
    }

    #[test]
    fn the_auto_partitioner_makes_chunks_and_fills_its_range_pool() {
        // One thread in an arena of 1: two chunks, each split once more and run through a range
        // pool four levels deep.
        let l = leaves(BlockedRange::new(0_i64, 1 << 20, 1), Partitioner::Auto, 1);
        assert_eq!(l.len(), 4 * 16);
        assert!(l.iter().all(|r| r.size() == (1 << 20) / 64));
        assert!(bounds(&l).windows(2).all(|w| w[0].1 == w[1].0));
    }

    #[test]
    fn an_empty_range_has_no_leaves() {
        assert!(leaves(BlockedRange::new(5_i64, 5, 1), Partitioner::Auto, 8).is_empty());
    }
}
