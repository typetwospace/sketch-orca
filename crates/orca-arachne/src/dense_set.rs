// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries ankerl::unordered_dense code: the order of ankerl::unordered_dense::set (its values
// vector, `emplace` and `do_erase`, deps_src/ankerl/unordered_dense.h lines 1053-1083) of
// unordered_dense v4.5.0 as OrcaSlicer v2.4.2 vendors it, translated on 2026-11-14 by
// orca-o3-arachne-4. Copyright (c) 2022-2024 Martin Leitner-Ankerl, under the MIT License
// (NOTICE).

//! `ankerl::unordered_dense::set<K>` (unordered_dense v4.5.0), the order its elements keep, for
//! `SkeletalTrapezoidation::connectJunctions`' quad starts and `WallToolPaths::getRegionOrder`'s
//! pairs. It belongs in `orca-stl`, which has MSVC's unordered containers but not ankerl's; it is
//! translated here until `orca-stl` takes it.
//!
//! The set keeps its elements in one vector, in the order they were added, and a bucket array of
//! indexes into it; iteration (`begin` to `end`) is the vector. `emplace` of a new element pushes
//! it at the back; erasing an element moves the last element into its place and pops the back
//! (`do_erase`). So the order depends on the calls alone, never on the hash or the bucket count:
//! here the index is a [`BTreeMap`] from each element to its place, which only finds them.
//!
//! Left out, since neither caller uses them: the maps, the segmented vector, the hash and the
//! bucket array themselves (their order never shows), `extract`, `replace` and the allocators.

use std::collections::BTreeMap;

/// `ankerl::unordered_dense::set<K>`: unique elements in their vector's order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DenseSet<K> {
    /// `m_values`.
    values: Vec<K>,
    /// Each element's index in `values` (the bucket array's job).
    index: BTreeMap<K, usize>,
}

impl<K> Default for DenseSet<K> {
    fn default() -> Self {
        Self {
            values: Vec::new(),
            index: BTreeMap::new(),
        }
    }
}

impl<K: Ord + Copy> DenseSet<K> {
    /// `set()`: empty. `set(bucket_count)` is the same, since the bucket count never changes the
    /// order.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            values: Vec::new(),
            index: BTreeMap::new(),
        }
    }

    /// `size()`.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// `empty()`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// `emplace(key)`: the key at the back when it is new; returns whether it was added.
    pub fn emplace(&mut self, key: K) -> bool {
        if self.index.contains_key(&key) {
            return false;
        }
        self.index.insert(key, self.values.len());
        self.values.push(key);
        true
    }

    /// `erase(key)`: the last element moved into the key's place, then the back popped
    /// (`do_erase`); returns the number of elements erased (0 or 1).
    pub fn erase(&mut self, key: &K) -> usize {
        let Some(value_idx_to_remove) = self.index.remove(key) else {
            return 0;
        };
        let last = self.values.len() - 1;
        if value_idx_to_remove != last {
            // no luck, we'll have to replace the value with the last one and update the index
            // accordingly
            let moved = self.values[last];
            self.values[value_idx_to_remove] = moved;
            self.index.insert(moved, value_idx_to_remove);
        }
        self.values.pop();
        1
    }

    /// `contains(key)`, `count(key) > 0`.
    #[must_use]
    pub fn contains(&self, key: &K) -> bool {
        self.index.contains_key(key)
    }

    /// `*begin()`: the first element, `None` when the set is empty.
    #[must_use]
    pub fn first(&self) -> Option<&K> {
        self.values.first()
    }

    /// The elements, `begin` to `end` (`values()`).
    #[must_use]
    pub fn values(&self) -> &[K] {
        &self.values
    }

    /// The elements in their order.
    pub fn iter(&self) -> core::slice::Iter<'_, K> {
        self.values.iter()
    }
}

impl<'a, K> IntoIterator for &'a DenseSet<K> {
    type Item = &'a K;
    type IntoIter = core::slice::Iter<'a, K>;

    fn into_iter(self) -> Self::IntoIter {
        self.values.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::DenseSet;

    #[test]
    fn erase_moves_the_last_element_into_the_hole() {
        let mut s = DenseSet::new();
        for k in [5, 3, 9, 1, 7] {
            assert!(s.emplace(k));
        }
        assert!(!s.emplace(9));
        assert_eq!(s.values(), &[5, 3, 9, 1, 7]);
        assert_eq!(s.erase(&3), 1);
        assert_eq!(s.values(), &[5, 7, 9, 1]);
        assert_eq!(s.erase(&3), 0);
        assert_eq!(s.erase(&1), 1);
        assert_eq!(s.values(), &[5, 7, 9]);
        assert_eq!(s.erase(&5), 1);
        assert_eq!(s.first(), Some(&9));
        assert!(s.contains(&7) && !s.contains(&5));
        assert!(s.emplace(5));
        assert_eq!(s.values(), &[9, 7, 5]);
        assert_eq!(s.len(), 3);
    }
}
