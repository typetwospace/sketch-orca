// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The standard library algorithms and containers of the C++ engine, with their exact order of operations.
//!
//! The algorithms and containers of the C++ standard library as Microsoft's implementation runs
//! them: `sort`, `stable_sort`, `nth_element`, `partial_sort`, the heap functions, `min` and `max`
//! with their NaN behaviour, the unordered containers in MSVC's iteration order, and `mt19937` with
//! its distributions.
//!
//! The order of the comparator calls and the order of the iteration are part of the result of a
//! slice, so they are reproduced, not approximated.
//!
//! The reference is MSVC 14.42 (Visual Studio 2022 17.12), whose headers come from microsoft/STL
//! at the tag `vs-2022-17.12`; each module names the functions it follows. A comparator is
//! written as C++'s `pred(a, b)`, "a goes before b", and positions are slice indices where C++
//! returns iterators.
//!
//! | C++ | here |
//! |---|---|
//! | `std::sort` | [`sort()`], [`sort_by`], [`sort_by_key`] |
//! | `std::stable_sort`, `std::inplace_merge` | [`stable_sort()`], [`stable_sort_by`], [`inplace_merge_by`] |
//! | `std::nth_element`, `std::partial_sort` | [`nth_element_by`], [`partial_sort_by`] |
//! | `std::lower_bound`, `upper_bound`, `equal_range`, `binary_search` | [`lower_bound_by`], [`upper_bound_by`], [`equal_range_by`], [`binary_search_by`] |
//! | `std::unique` | [`unique_by`], [`unique_vec_by`] |
//! | `std::push_heap` and the other heap functions, `std::priority_queue` | [`heap`], [`PriorityQueue`] |
//! | `std::min`, `std::max`, `std::clamp`, `std::max_element` and the other element searches | [`minmax`] |
//! | `std::hash`, `std::unordered_map`, `std::unordered_set` | [`unordered`] |
//! | `std::mt19937`, `std::uniform_int_distribution`, `std::shuffle` and the rest of `<random>` used | [`random`] |

pub mod heap;
pub mod minmax;
pub mod random;
pub mod select;
pub mod sort;
pub mod stable_sort;
pub mod unordered;

pub use heap::{
    PriorityQueue, is_heap, is_heap_by, is_heap_until, is_heap_until_by, make_heap, make_heap_by,
    pop_heap, pop_heap_by, push_heap, push_heap_by, sort_heap, sort_heap_by,
};
pub use minmax::{
    clamp, clamp_by, max, max_by, max_element, max_element_by, max_element_f32, max_element_f64,
    min, min_by, min_element, min_element_by, min_element_f32, min_element_f64, minmax_element,
    minmax_element_by, minmax_element_f32, minmax_element_f64,
};
pub use random::{
    DefaultRandomEngine, Mt19937, Mt19937_64, SeedSeq, UniformIntDistribution,
    UniformRealDistribution, Urbg, generate_canonical_f32, generate_canonical_f64, shuffle,
};
pub use select::{
    binary_search, binary_search_by, equal_range, equal_range_by, lower_bound, lower_bound_by,
    nth_element, nth_element_by, partial_sort, partial_sort_by, unique, unique_by, unique_vec,
    unique_vec_by, upper_bound, upper_bound_by,
};
pub use sort::{ISORT_MAX, sort, sort_by, sort_by_key};
pub use stable_sort::{
    inplace_merge, inplace_merge_by, stable_sort, stable_sort_by, stable_sort_by_key,
    stable_sort_by_with_capacity,
};
pub use unordered::{
    EqualTo, HashFn, KeyEq, StdHash, StdHashable, UnorderedMap, UnorderedSet, std_hash,
};

/// The crate's name, as Cargo has it.
pub const NAME: &str = "orca-stl";

#[cfg(test)]
mod tests {
    use super::NAME;

    #[test]
    fn name_is_the_package_name() {
        assert_eq!(NAME, env!("CARGO_PKG_NAME"));
    }
}
