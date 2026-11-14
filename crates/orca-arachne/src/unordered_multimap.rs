// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Microsoft STL code: this file is translated from stl/inc/xhash (_Hash's constructor,
// emplace for _Multi, max_load_factor, rehash, reserve, _Equal_range, _Find_last,
// _Insert_new_node_before, _Check_rehash_required_1, _Rehash_for_1, _Min_load_factor_buckets,
// _Desired_grow_bucket_count and _Forced_rehash) and stl/inc/unordered_map (unordered_multimap) of
// MSVC 14.42 (microsoft/STL at the tag vs-2022-17.12, as Visual Studio 2022's 14.42.34433
// headers hold it), on 2026-11-14 by arachne-2. Its copyright holder is Microsoft Corporation, and
// it is under Apache-2.0 WITH LLVM-exception (licenses/Apache-2.0.txt and
// licenses/LLVM-exception.txt).

//! `std::unordered_multimap<K, V, Hash>` as MSVC builds it, for the sparse grids
//! ([`crate::sparse_grid`]). It belongs in `orca-stl`, whose `UnorderedMap` and `UnorderedSet`
//! cover the unique containers only (its README: "`unordered_multimap` and `unordered_multiset`
//! are not covered"); it is translated here, the parts the grids call, until `orca-stl` takes it.
//!
//! The grids iterate the map (`begin` to `end`), and Arachne lets that order reach its output
//! (`parity/sop1/arachne.md`: ANSP and ANSL record it), so the order has to be MSVC's. MSVC keeps
//! every element in one doubly linked list; the bucket array holds, per bucket, the first and the
//! last list node of that bucket's run. `emplace` grows the table first when `size + 1` would pass
//! `max_load_factor * bucket_count` (to 8 times the buckets below 512, else to the next power of
//! two that holds them), then puts the new element after the last element of its key in its
//! bucket's run, or at the front of the run when the key is new, or at the end of the list when
//! the bucket is empty. Rehashing walks the list in order and moves each element after the last
//! element of its key in its new bucket, or to the front of the run. `equal_range` walks the
//! bucket from its first node to the first element of the key and takes the run of equal keys.
//!
//! The key equality is `std::equal_to<K>` (`==`); the hash is a function, as a C++ hash functor
//! is. Left out, since no grid calls them: erasure, `find`, `count`, `bucket`, copies and
//! assignment, hints and node handles.

/// The list head (`_Myhead`), which is also `end()` and the empty-bucket marker.
const HEAD: usize = 0;
/// `_Min_buckets`.
const MIN_BUCKETS: usize = 8;
/// `1 << _Floor_of_log_2(_Vec.max_size() >> 1)` for 8-byte bucket iterators.
const MAX_STORAGE_BUCKETS: usize = 1 << 59;

#[derive(Clone, Copy, Debug)]
struct Link {
    prev: usize,
    next: usize,
}

/// `std::unordered_multimap<K, V, Hash>` with MSVC's layout and order.
pub struct UnorderedMultimap<K, V> {
    /// The list links, by node; node 0 is the head.
    links: Vec<Link>,
    /// The elements, by node; node 0 holds `None`.
    slots: Vec<Option<(K, V)>>,
    len: usize,
    /// `_Vec`: per bucket, its first and last node, or `HEAD` twice when empty.
    buckets: Vec<[usize; 2]>,
    mask: usize,
    maxidx: usize,
    max_load_factor: f32,
    hasher: fn(&K) -> u64,
}

impl<K: PartialEq + core::fmt::Debug, V: core::fmt::Debug> core::fmt::Debug
    for UnorderedMultimap<K, V>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<K: PartialEq, V> UnorderedMultimap<K, V> {
    /// `std::unordered_multimap<K, V, Hash>()`: empty, 8 buckets, maximum load factor 1, hashing
    /// with `hasher`.
    #[must_use]
    pub fn new(hasher: fn(&K) -> u64) -> Self {
        Self {
            links: vec![Link {
                prev: HEAD,
                next: HEAD,
            }],
            slots: vec![None],
            len: 0,
            buckets: vec![[HEAD, HEAD]; MIN_BUCKETS],
            mask: MIN_BUCKETS - 1,
            maxidx: MIN_BUCKETS,
            max_load_factor: 1.0,
            hasher,
        }
    }

    /// `size()`.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// `empty()`.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// `bucket_count()`.
    #[must_use]
    pub const fn bucket_count(&self) -> usize {
        self.maxidx
    }

    /// `max_load_factor()`.
    #[must_use]
    pub const fn max_load_factor(&self) -> f32 {
        self.max_load_factor
    }

    /// `max_load_factor(f)`: sets it and nothing else; the table grows by it from the next
    /// insertion on.
    pub fn set_max_load_factor(&mut self, f: f32) {
        debug_assert!(!f.is_nan() && f > 0.0, "invalid hash load factor");
        self.max_load_factor = f;
    }

    /// `begin()` to `end()`: the elements in the list's order.
    #[must_use]
    pub fn iter(&self) -> Iter<'_, K, V> {
        Iter {
            map: self,
            at: self.links[HEAD].next,
            end: HEAD,
        }
    }

    fn key(&self, node: usize) -> &K {
        &self.entry(node).0
    }

    fn entry(&self, node: usize) -> &(K, V) {
        self.slots[node]
            .as_ref()
            .expect("a list node holds an element")
    }

    fn hash(&self, k: &K) -> u64 {
        (self.hasher)(k)
    }

    /// The bucket of a hash: `_Hashval & _Mask`.
    #[allow(clippy::cast_possible_truncation)]
    const fn bucket_of(&self, hash: u64) -> usize {
        hash as usize & self.mask
    }

    /// `_Min_load_factor_buckets(n)`: `size_t(ceilf(float(n) / max_load_factor()))`.
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    fn min_load_factor_buckets(&self, n: usize) -> usize {
        orca_crt::f32_to_u64(orca_crt::ceilf(n as f32 / self.max_load_factor)) as usize
    }

    /// `_Desired_grow_bucket_count(n)`.
    fn desired_grow_bucket_count(&self, n: usize) -> usize {
        let old = self.maxidx;
        let req = orca_stl::max(MIN_BUCKETS, self.min_load_factor_buckets(n));
        if old >= req {
            return old;
        }
        if old < 512 && old * 8 >= req {
            return old * 8;
        }
        req
    }

    /// `_Check_rehash_required_1()`: `max_load_factor() < float(size + 1) / float(bucket_count())`.
    #[allow(clippy::cast_precision_loss)]
    fn check_rehash_required_1(&self) -> bool {
        self.max_load_factor < (self.len + 1) as f32 / self.maxidx as f32
    }

    /// `rehash(n)`: at least `n` buckets and at least `size / max_load_factor`, rounded up to a
    /// power of two; nothing when the table has that many already.
    pub fn rehash(&mut self, n: usize) {
        let n = orca_stl::max(self.min_load_factor_buckets(self.len), n);
        if n <= self.maxidx {
            return;
        }
        self.forced_rehash(n);
    }

    /// `reserve(n)`: `rehash(_Min_load_factor_buckets(n))`.
    pub fn reserve(&mut self, n: usize) {
        self.rehash(self.min_load_factor_buckets(n));
    }

    /// `_Unchecked_splice(before, node, node->_Next)`: moves `node` in front of `before`.
    fn splice(&mut self, before: usize, node: usize) {
        let Link { prev, next } = self.links[node];
        self.links[prev].next = next;
        self.links[next].prev = prev;
        let after = self.links[before].prev;
        self.links[node] = Link {
            prev: after,
            next: before,
        };
        self.links[after].next = node;
        self.links[before].prev = node;
    }

    /// `_Forced_rehash(buckets)`: `buckets` rounded up to a power of two, every element moved in
    /// list order after the last element of its key in its new bucket, or to the front of the
    /// bucket's run.
    fn forced_rehash(&mut self, buckets: usize) {
        assert!(buckets <= MAX_STORAGE_BUCKETS, "invalid hash bucket count");
        let buckets = buckets.next_power_of_two();
        self.buckets.clear();
        self.buckets.resize(buckets, [HEAD, HEAD]);
        self.mask = buckets - 1;
        self.maxidx = buckets;

        let mut inserted = self.links[HEAD].next;
        while inserted != HEAD {
            let next_inserted = self.links[inserted].next;
            let b = self.bucket_of(self.hash(self.key(inserted)));
            let [lo, hi] = self.buckets[b];
            if lo == HEAD {
                self.buckets[b] = [inserted, inserted];
                inserted = next_inserted;
                continue;
            }
            let mut before = hi;
            if self.key(inserted) == self.key(before) {
                before = self.links[before].next;
                if before != inserted {
                    self.splice(before, inserted);
                }
                self.buckets[b][1] = inserted;
                inserted = next_inserted;
                continue;
            }
            loop {
                if self.buckets[b][0] == before {
                    self.splice(before, inserted);
                    self.buckets[b][0] = inserted;
                    break;
                }
                before = self.links[before].prev;
                if self.key(inserted) == self.key(before) {
                    let after_equal = self.links[before].next;
                    self.splice(after_equal, inserted);
                    break;
                }
            }
            inserted = next_inserted;
        }
    }

    /// `_Find_last(key, hash)._Insert_before`: walks the key's bucket backwards from its last node;
    /// after the last element of the key, before the bucket's first node when the key is not
    /// there, the end of the list when the bucket is empty.
    fn find_last_insert_before(&self, key: &K, hash: u64) -> usize {
        let b = self.bucket_of(hash);
        let [lo, hi] = self.buckets[b];
        let mut at = hi;
        if at == HEAD {
            return HEAD;
        }
        loop {
            if key == self.key(at) {
                return self.links[at].next;
            }
            if at == lo {
                return at;
            }
            at = self.links[at].prev;
        }
    }

    /// `_Insert_new_node_before(hash, before, node)`: links `node` before `before` and moves its
    /// bucket's bounds.
    fn insert_new_node_before(&mut self, hash: u64, before: usize, node: usize) {
        let after = self.links[before].prev;
        self.len += 1;
        self.links[node] = Link {
            prev: after,
            next: before,
        };
        self.links[after].next = node;
        self.links[before].prev = node;
        let b = self.bucket_of(hash);
        let [lo, hi] = self.buckets[b];
        if lo == HEAD {
            self.buckets[b] = [node, node];
        } else if lo == before {
            self.buckets[b][0] = node;
        } else if hi == after {
            self.buckets[b][1] = node;
        }
    }

    /// `emplace(k, v)` of a multimap: the node is made, the table grows if the new size needs it,
    /// and the node goes after the last element of its key (`_Find_last`).
    pub fn emplace(&mut self, k: K, v: V) {
        let hash = self.hash(&k);
        self.slots.push(Some((k, v)));
        self.links.push(Link {
            prev: HEAD,
            next: HEAD,
        });
        let node = self.slots.len() - 1;
        if self.check_rehash_required_1() {
            self.forced_rehash(self.desired_grow_bucket_count(self.len + 1));
        }
        let before = self.find_last_insert_before(self.key(node), hash);
        self.insert_new_node_before(hash, before, node);
    }

    /// `equal_range(k)`: the elements whose key is `k`, in the list's order (`_Equal_range`).
    #[must_use]
    pub fn equal_range<'a>(&'a self, k: &K) -> Iter<'a, K, V> {
        let b = self.bucket_of(self.hash(k));
        let [lo, hi] = self.buckets[b];
        let none = Iter {
            map: self,
            at: HEAD,
            end: HEAD,
        };
        let mut at = lo;
        if at == HEAD {
            return none;
        }
        while self.key(at) != k {
            if at == hi {
                return none;
            }
            at = self.links[at].next;
        }
        let first = at;
        loop {
            let at_bucket_end = at == hi;
            at = self.links[at].next;
            if at_bucket_end || self.key(at) != k {
                break;
            }
        }
        Iter {
            map: self,
            at: first,
            end: at,
        }
    }
}

/// The elements of an [`UnorderedMultimap`] from one list node up to another.
pub struct Iter<'a, K, V> {
    map: &'a UnorderedMultimap<K, V>,
    at: usize,
    end: usize,
}

impl<'a, K, V> Iterator for Iter<'a, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        if self.at == self.end {
            return None;
        }
        let node = self.at;
        self.at = self.map.links[node].next;
        let (k, v) = self.map.slots[node]
            .as_ref()
            .expect("a list node holds an element");
        Some((k, v))
    }
}

#[cfg(test)]
mod tests {
    use super::UnorderedMultimap;

    fn h(k: &u64) -> u64 {
        *k
    }

    #[test]
    fn equal_keys_stay_together_in_insertion_order() {
        let mut m: UnorderedMultimap<u64, u32> = UnorderedMultimap::new(h);
        // 1 and 9 share bucket 1 of 8; a new key goes to the front of its bucket's run, an equal
        // key after the last of its key.
        m.emplace(1, 0);
        m.emplace(9, 1);
        m.emplace(1, 2);
        m.emplace(3, 3);
        let order: Vec<(u64, u32)> = m.iter().map(|(k, v)| (*k, *v)).collect();
        assert_eq!(order, vec![(9, 1), (1, 0), (1, 2), (3, 3)]);
        let ones: Vec<u32> = m.equal_range(&1).map(|(_, v)| *v).collect();
        assert_eq!(ones, vec![0, 2]);
        assert_eq!(m.equal_range(&17).count(), 0);
        assert_eq!(m.equal_range(&4).count(), 0);
        assert_eq!(m.bucket_count(), 8);
    }

    #[test]
    fn the_table_grows_eightfold_then_by_powers_of_two() {
        let mut m: UnorderedMultimap<u64, u64> = UnorderedMultimap::new(h);
        for i in 0..9 {
            m.emplace(i, i);
        }
        assert_eq!(m.bucket_count(), 64);
        for i in 9..65 {
            m.emplace(i % 7, i);
        }
        assert_eq!(m.bucket_count(), 512);
        assert_eq!(m.len(), 65);
        let mut r: UnorderedMultimap<u64, u64> = UnorderedMultimap::new(h);
        r.reserve(20);
        assert_eq!(r.bucket_count(), 32);
        r.set_max_load_factor(0.5);
        r.rehash(3);
        assert_eq!(r.bucket_count(), 32);
    }
}
