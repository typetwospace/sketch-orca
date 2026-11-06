// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! oneTBB 2021.5.0's concurrency and partitioners as fixed split trees, run on rayon.
//!
//! oneTBB 2021.5.0's `max_concurrency` and its partitioners, kept as fixed split trees and join
//! orders, and run on rayon 1.12.0 (Windows: `windows-sys` for the processor count, with processor
//! groups). The split tree depends on the concurrency P and on nothing else, so a result never
//! depends on how threads are scheduled.
//!
//! The C++ engine runs its loops through oneTBB, whose auto partitioner splits further whenever
//! a thread steals work, so the leaves of a loop, and the order in which a reduction meets
//! them, change from run to run. The parity oracle pins them by running the engine with one
//! thread in an arena of concurrency P. This crate reproduces that run: each partitioner splits
//! as one thread in an arena of concurrency P splits, and the leaves then run on a rayon pool
//! ([`Arena::new`]) or, task for task in oneTBB's order, on the calling thread
//! ([`Arena::serial`]). Each module names the oneTBB code it follows.
//!
//! | oneTBB | here |
//! |---|---|
//! | `tbb::info::default_concurrency`, `this_task_arena::max_concurrency`, `task_arena` | [`default_concurrency`], [`max_concurrency`], [`Arena`] |
//! | `blocked_range`, `blocked_range2d`, the Range requirements | [`BlockedRange`], [`BlockedRange2d`], [`Range`] |
//! | `auto_partitioner`, `simple_partitioner`, `static_partitioner` | [`Partitioner`], [`DeterministicPartitioner`], [`leaves`] |
//! | `parallel_for` over a range, over integers, `parallel_for_each` | [`parallel_for()`], [`parallel_for_index`], [`parallel_for_index_step`], [`parallel_for_each`], [`parallel_for_each_mut`] |
//! | `parallel_reduce`, `parallel_deterministic_reduce` | [`parallel_reduce()`], [`parallel_reduce_fn`], [`parallel_deterministic_reduce`], [`parallel_deterministic_reduce_fn`], [`ReduceBody`] |
//! | `parallel_invoke`, `task_group` | [`parallel_invoke()`], [`parallel_invoke!`], [`task_group()`], [`TaskGroup`] |

pub mod concurrency;
pub mod invoke;
pub mod parallel_for;
pub mod parallel_reduce;
pub mod partitioner;
pub mod range;

pub use concurrency::{Arena, default_concurrency, hardware_concurrency, max_concurrency};
pub use invoke::{
    InvokeFn, TaskGroup, TaskGroupStatus, invoke_fn, invoke_order, parallel_invoke, task_group,
};
pub use parallel_for::{
    parallel_for, parallel_for_each, parallel_for_each_mut, parallel_for_index,
    parallel_for_index_step,
};
pub use parallel_reduce::{
    ReduceBody, parallel_deterministic_reduce, parallel_deterministic_reduce_fn, parallel_reduce,
    parallel_reduce_fn,
};
pub use partitioner::{DeterministicPartitioner, Partitioner, leaves};
pub use range::{BlockedRange, BlockedRange2d, BlockedValue, Range};
pub use rayon::ThreadPoolBuildError;

/// The crate's name, as Cargo has it.
pub const NAME: &str = "orca-tbb";

#[cfg(test)]
mod tests {
    use super::NAME;

    #[test]
    fn name_is_the_package_name() {
        assert_eq!(NAME, env!("CARGO_PKG_NAME"));
    }
}
