// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Microsoft STL code: this file is translated from stl/inc/xhash (_Hash: emplace,
// _Try_emplace, _Insert_or_assign, erase, clear, rehash, reserve, the copy constructor and copy
// assignment, _Find_last, _Insert_new_node_before, _Check_rehash_required_1, _Rehash_for_1,
// _Erase_bucket, _Min_load_factor_buckets, _Desired_grow_bucket_count and _Forced_rehash),
// stl/inc/unordered_map, stl/inc/unordered_set, stl/inc/type_traits (std::hash and the FNV-1a
// helpers) and stl/inc/xstring (std::hash of strings) of microsoft/STL at the tag vs-2022-17.12
// (commit 91e425587a0b9b5ac0d1005844c661fee2e2813b), MSVC 14.42's unordered containers with their
// bucket layout, growth and iteration order, on 2026-11-06. Its copyright holder is Microsoft
// Corporation, and it is under Apache-2.0 WITH LLVM-exception (licenses/Apache-2.0.txt and
// licenses/LLVM-exception.txt).

//! `std::unordered_map` and `std::unordered_set` as MSVC builds them, and `std::hash`.
//!
//! The engine iterates its hash containers and lets the order reach its output, so the order has
//! to be MSVC's. MSVC keeps every element in one doubly linked list; the bucket array holds, per
//! bucket, the first and last list node of that bucket's run, and a new key goes to the front of
//! its bucket's run (or starts a run at the list's end). The table starts with 8 buckets, grows
//! when `size + 1` would pass `max_load_factor * bucket_count` (to 8 times the buckets below 512,
//! else to the next power of two that holds them), and rehashing walks the list in order and
//! moves each element to the front of its new bucket's run. [`UnorderedMap`] and
//! [`UnorderedSet`] do exactly that, on a slab of nodes, so their iteration order, their
//! `bucket_count` and their `bucket` and `bucket_size` answers are MSVC's.
//!
//! [`StdHash`] is `std::hash` on 64-bit Windows: FNV-1a over the bytes of the value (integers,
//! `bool`, characters and pointers as stored, `float` and `double` with `-0.0` taken as `0.0`,
//! strings over their bytes). A custom hasher or key equality is any `Fn(&K) -> u64` or
//! `Fn(&K, &K) -> bool`, as a C++ functor would be.
//!
//! C++ assignment `a = b` is [`Clone::clone_from`] here (MSVC's copy assignment rehashes into
//! `a`'s own bucket count), and the copy constructor is [`Clone::clone`].

use std::borrow::Borrow;
use std::fmt;
use std::marker::PhantomData;

/// `_FNV_offset_basis` on 64-bit targets.
pub const FNV_OFFSET_BASIS: u64 = 14_695_981_039_346_656_037;
/// `_FNV_prime` on 64-bit targets.
pub const FNV_PRIME: u64 = 1_099_511_628_211;

/// `_Fnv1a_append_bytes`: folds `bytes` into the partial FNV-1a hash `val`.
#[inline]
pub fn fnv1a_append_bytes(mut val: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        val ^= u64::from(b);
        val = val.wrapping_mul(FNV_PRIME);
    }
    val
}

/// `_Hash_array_representation` of bytes: FNV-1a from the offset basis.
#[inline]
pub fn hash_bytes(bytes: &[u8]) -> u64 {
    fnv1a_append_bytes(FNV_OFFSET_BASIS, bytes)
}

/// A type `std::hash` hashes: its value as MSVC's `std::hash<T>` gives it.
pub trait StdHashable {
    /// `std::hash<T>()(*self)`.
    fn std_hash(&self) -> u64;
}

macro_rules! std_hash_int {
    ($($t:ty),*) => {$(
        impl StdHashable for $t {
            #[inline]
            fn std_hash(&self) -> u64 {
                hash_bytes(&self.to_le_bytes())
            }
        }
    )*};
}
std_hash_int!(u8, i8, u16, i16, u32, i32, u64, i64, usize, isize);

impl StdHashable for bool {
    #[inline]
    fn std_hash(&self) -> u64 {
        hash_bytes(&[u8::from(*self)])
    }
}

/// A Rust `char` is hashed as C++'s `char32_t`.
impl StdHashable for char {
    #[inline]
    fn std_hash(&self) -> u64 {
        hash_bytes(&u32::from(*self).to_le_bytes())
    }
}

impl StdHashable for f32 {
    #[inline]
    fn std_hash(&self) -> u64 {
        let v = if *self == 0.0 { 0.0f32 } else { *self }; // map -0 to 0
        hash_bytes(&v.to_bits().to_le_bytes())
    }
}

impl StdHashable for f64 {
    #[inline]
    fn std_hash(&self) -> u64 {
        let v = if *self == 0.0 { 0.0f64 } else { *self }; // map -0 to 0
        hash_bytes(&v.to_bits().to_le_bytes())
    }
}

/// `std::hash<std::string_view>`: the bytes.
impl StdHashable for str {
    #[inline]
    fn std_hash(&self) -> u64 {
        hash_bytes(self.as_bytes())
    }
}

/// `std::hash<std::string>`: the bytes.
impl StdHashable for String {
    #[inline]
    fn std_hash(&self) -> u64 {
        hash_bytes(self.as_bytes())
    }
}

/// `std::hash<T*>`: the address.
impl<T: ?Sized> StdHashable for *const T {
    #[inline]
    fn std_hash(&self) -> u64 {
        hash_bytes(&(self.cast::<u8>() as usize).to_le_bytes())
    }
}

/// `std::hash<T*>`: the address.
impl<T: ?Sized> StdHashable for *mut T {
    #[inline]
    fn std_hash(&self) -> u64 {
        hash_bytes(&(self.cast::<u8>() as usize).to_le_bytes())
    }
}

impl<T: StdHashable + ?Sized> StdHashable for &T {
    #[inline]
    fn std_hash(&self) -> u64 {
        (**self).std_hash()
    }
}

/// `std::hash<T>()(value)`.
#[inline]
pub fn std_hash<T: StdHashable + ?Sized>(value: &T) -> u64 {
    value.std_hash()
}

/// A hasher, as the `Hash` parameter of a C++ unordered container.
pub trait HashFn<K: ?Sized> {
    /// The hash of `key` (`size_t`).
    fn hash_key(&self, key: &K) -> u64;
}

/// A key equality, as the `KeyEqual` parameter of a C++ unordered container.
pub trait KeyEq<K: ?Sized> {
    /// Whether `a` and `b` are the same key.
    fn equal(&self, a: &K, b: &K) -> bool;
}

/// `std::hash<K>`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StdHash;

impl<K: StdHashable + ?Sized> HashFn<K> for StdHash {
    #[inline]
    fn hash_key(&self, key: &K) -> u64 {
        key.std_hash()
    }
}

impl<K: ?Sized, F: Fn(&K) -> u64> HashFn<K> for F {
    #[inline]
    fn hash_key(&self, key: &K) -> u64 {
        self(key)
    }
}

/// `std::equal_to<K>`: `a == b`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EqualTo;

impl<K: PartialEq + ?Sized> KeyEq<K> for EqualTo {
    #[inline]
    fn equal(&self, a: &K, b: &K) -> bool {
        a == b
    }
}

impl<K: ?Sized, F: Fn(&K, &K) -> bool> KeyEq<K> for F {
    #[inline]
    fn equal(&self, a: &K, b: &K) -> bool {
        self(a, b)
    }
}

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

/// `std::unordered_map<K, V, H, E>` with MSVC's layout and order.
pub struct UnorderedMap<K, V, H = StdHash, E = EqualTo> {
    /// The list links, by node; node 0 is the head.
    links: Vec<Link>,
    /// The elements, by node; node 0 and the free nodes hold `None`.
    slots: Vec<Option<(K, V)>>,
    free: Vec<usize>,
    len: usize,
    /// `_Vec`: per bucket, its first and last node, or `HEAD` twice when empty.
    buckets: Vec<[usize; 2]>,
    mask: usize,
    maxidx: usize,
    max_load_factor: f32,
    hasher: H,
    key_eq: E,
}

/// Clears the map if a hasher, a key equality or a clone panics halfway through a rehash or an
/// assignment, as `_Clear_guard` does.
struct ClearGuard<'a, K, V, H, E> {
    map: &'a mut UnorderedMap<K, V, H, E>,
    armed: bool,
}

impl<K, V, H, E> Drop for ClearGuard<'_, K, V, H, E> {
    fn drop(&mut self) {
        if self.armed {
            self.map.clear();
        }
    }
}

impl<K, V> UnorderedMap<K, V> {
    /// `std::unordered_map<K, V>()`: empty, 8 buckets, maximum load factor 1.
    pub fn new() -> Self {
        Self::with_hasher(StdHash, EqualTo)
    }
}

impl<K: StdHashable + PartialEq, V> UnorderedMap<K, V> {
    /// `std::unordered_map<K, V>(buckets)`: empty, then `rehash(buckets)`.
    pub fn with_buckets(buckets: usize) -> Self {
        let mut m = Self::new();
        m.rehash(buckets);
        m
    }
}

impl<K, V, H: Default, E: Default> Default for UnorderedMap<K, V, H, E> {
    fn default() -> Self {
        Self::with_hasher(H::default(), E::default())
    }
}

impl<K, V, H, E> UnorderedMap<K, V, H, E> {
    /// An empty map with the given hasher and key equality: 8 buckets, maximum load factor 1.
    pub fn with_hasher(hasher: H, key_eq: E) -> Self {
        UnorderedMap {
            links: vec![Link {
                prev: HEAD,
                next: HEAD,
            }],
            slots: vec![None],
            free: Vec::new(),
            len: 0,
            buckets: vec![[HEAD, HEAD]; MIN_BUCKETS],
            mask: MIN_BUCKETS - 1,
            maxidx: MIN_BUCKETS,
            max_load_factor: 1.0,
            hasher,
            key_eq,
        }
    }

    /// `size()`.
    pub fn len(&self) -> usize {
        self.len
    }

    /// `empty()`.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// `bucket_count()`.
    pub fn bucket_count(&self) -> usize {
        self.maxidx
    }

    /// `load_factor()`: `float(size) / float(bucket_count)`.
    pub fn load_factor(&self) -> f32 {
        self.len as f32 / self.maxidx as f32
    }

    /// `max_load_factor()`.
    pub fn max_load_factor(&self) -> f32 {
        self.max_load_factor
    }

    /// `max_load_factor(f)`: sets it and nothing else; the table grows by it from the next
    /// insertion on.
    pub fn set_max_load_factor(&mut self, f: f32) {
        debug_assert!(!f.is_nan() && f > 0.0, "invalid hash load factor");
        self.max_load_factor = f;
    }

    /// `hash_function()`.
    pub fn hasher(&self) -> &H {
        &self.hasher
    }

    /// `key_eq()`.
    pub fn key_eq(&self) -> &E {
        &self.key_eq
    }

    /// `bucket_size(b)`: the length of bucket `b`'s run.
    pub fn bucket_size(&self, b: usize) -> usize {
        let [lo, hi] = self.buckets[b];
        if lo == HEAD {
            return 0;
        }
        let mut n = 1;
        let mut at = lo;
        while at != hi {
            at = self.links[at].next;
            n += 1;
        }
        n
    }

    /// `begin(b)` to `end(b)`: the elements of bucket `b`, in list order.
    pub fn bucket_iter(&self, b: usize) -> Iter<'_, K, V> {
        let [lo, hi] = self.buckets[b];
        let end = if hi == HEAD {
            HEAD
        } else {
            self.links[hi].next
        };
        Iter {
            links: &self.links,
            slots: &self.slots,
            cur: lo,
            end,
            remaining: self.bucket_size(b),
        }
    }

    /// The elements in iteration order.
    pub fn iter(&self) -> Iter<'_, K, V> {
        Iter {
            links: &self.links,
            slots: &self.slots,
            cur: self.links[HEAD].next,
            end: HEAD,
            remaining: self.len,
        }
    }

    /// The elements in iteration order, the values mutable.
    pub fn iter_mut(&mut self) -> IterMut<'_, K, V> {
        let cur = self.links[HEAD].next;
        IterMut {
            links: &self.links,
            slots: self.slots.as_mut_ptr(),
            cur,
            remaining: self.len,
            _marker: PhantomData,
        }
    }

    /// The keys in iteration order.
    pub fn keys(&self) -> impl Iterator<Item = &K> + '_ {
        self.iter().map(|(k, _)| k)
    }

    /// The values in iteration order.
    pub fn values(&self) -> impl Iterator<Item = &V> + '_ {
        self.iter().map(|(_, v)| v)
    }

    /// The values in iteration order, mutable.
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> + '_ {
        self.iter_mut().map(|(_, v)| v)
    }

    /// `clear()`: removes every element and keeps the bucket count.
    pub fn clear(&mut self) {
        if self.len == 0 {
            return;
        }
        self.slots.truncate(1);
        self.links.truncate(1);
        self.links[HEAD] = Link {
            prev: HEAD,
            next: HEAD,
        };
        self.free.clear();
        self.len = 0;
        for b in &mut self.buckets {
            *b = [HEAD, HEAD];
        }
    }

    fn entry(&self, node: usize) -> &(K, V) {
        self.slots[node]
            .as_ref()
            .expect("a list node holds an element")
    }

    fn key(&self, node: usize) -> &K {
        &self.entry(node).0
    }

    fn alloc(&mut self, k: K, v: V) -> usize {
        if let Some(i) = self.free.pop() {
            self.slots[i] = Some((k, v));
            i
        } else {
            self.slots.push(Some((k, v)));
            self.links.push(Link {
                prev: HEAD,
                next: HEAD,
            });
            self.slots.len() - 1
        }
    }

    /// `_List._Unchecked_erase`: unlinks `node` and frees it.
    fn unlink(&mut self, node: usize) -> (K, V) {
        let Link { prev, next } = self.links[node];
        self.links[prev].next = next;
        self.links[next].prev = prev;
        self.len -= 1;
        self.free.push(node);
        self.slots[node]
            .take()
            .expect("a list node holds an element")
    }

    /// Appends an unlinked `node` at the end of the list (no bucket bookkeeping).
    fn push_back(&mut self, node: usize) {
        let last = self.links[HEAD].prev;
        self.links[node] = Link {
            prev: last,
            next: HEAD,
        };
        self.links[last].next = node;
        self.links[HEAD].prev = node;
        self.len += 1;
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

    /// `_Erase_bucket`.
    fn erase_bucket(&mut self, node: usize, b: usize) {
        let [lo, hi] = self.buckets[b];
        if hi == node {
            if lo == node {
                self.buckets[b] = [HEAD, HEAD];
            } else {
                self.buckets[b][1] = self.links[node].prev;
            }
        } else if lo == node {
            self.buckets[b][0] = self.links[node].next;
        }
    }

    /// `_Insert_new_node_before`.
    fn insert_new_node_before(&mut self, hash: u64, before: usize, node: usize) {
        let after = self.links[before].prev;
        self.len += 1;
        self.links[node] = Link {
            prev: after,
            next: before,
        };
        self.links[after].next = node;
        self.links[before].prev = node;
        let b = hash as usize & self.mask;
        let [lo, hi] = self.buckets[b];
        if lo == HEAD {
            self.buckets[b] = [node, node];
        } else if lo == before {
            self.buckets[b][0] = node;
        } else if hi == after {
            self.buckets[b][1] = node;
        }
    }

    /// `_Check_rehash_required_1`.
    fn check_rehash_required_1(&self) -> bool {
        self.max_load_factor < (self.len + 1) as f32 / self.maxidx as f32
    }

    /// `_Min_load_factor_buckets`: `ceilf(float(n) / max_load_factor)`.
    fn min_load_factor_buckets(&self, n: usize) -> usize {
        (n as f32 / self.max_load_factor).ceil() as usize
    }

    /// `_Desired_grow_bucket_count`.
    fn desired_grow_bucket_count(&self, n: usize) -> usize {
        let old = self.maxidx;
        let req = crate::max(MIN_BUCKETS, self.min_load_factor_buckets(n));
        if old >= req {
            return old;
        }
        if old < 512 && old * 8 >= req {
            return old * 8;
        }
        req
    }
}

impl<K, V, H: HashFn<K>, E: KeyEq<K>> UnorderedMap<K, V, H, E> {
    /// `std::unordered_map<K, V, H, E>(buckets, hasher, key_eq)`: empty, then `rehash(buckets)`.
    pub fn with_buckets_and_hasher(buckets: usize, hasher: H, key_eq: E) -> Self {
        let mut m = Self::with_hasher(hasher, key_eq);
        m.rehash(buckets);
        m
    }

    /// `_Find_last`: walks `key`'s bucket backwards; returns where a new element goes and the
    /// equal element, if there is one.
    fn find_last<Q: ?Sized>(&self, key: &Q, hash: u64) -> (usize, Option<usize>)
    where
        K: Borrow<Q>,
        E: KeyEq<Q>,
    {
        let b = hash as usize & self.mask;
        let [lo, hi] = self.buckets[b];
        let mut at = hi;
        if at == HEAD {
            return (HEAD, None);
        }
        loop {
            if self.key_eq.equal(key, self.key(at).borrow()) {
                return (self.links[at].next, Some(at));
            }
            if at == lo {
                return (at, None);
            }
            at = self.links[at].prev;
        }
    }

    /// `_Forced_rehash`: `buckets` rounded up to a power of two, every element moved in list
    /// order to its new bucket.
    fn forced_rehash(&mut self, buckets: usize) {
        assert!(buckets <= MAX_STORAGE_BUCKETS, "invalid hash bucket count");
        let buckets = buckets.next_power_of_two();
        if self.buckets.len() < buckets {
            self.buckets.resize(buckets, [HEAD, HEAD]);
        }
        for b in &mut self.buckets {
            *b = [HEAD, HEAD];
        }
        self.mask = buckets - 1;
        self.maxidx = buckets;

        let mut guard = ClearGuard {
            map: self,
            armed: true,
        };
        let m = &mut *guard.map;
        let mut inserted = m.links[HEAD].next;
        while inserted != HEAD {
            let next_inserted = m.links[inserted].next;
            let b = m.hasher.hash_key(m.key(inserted)) as usize & m.mask;
            let [lo, hi] = m.buckets[b];
            if lo == HEAD {
                m.buckets[b] = [inserted, inserted];
                inserted = next_inserted;
                continue;
            }
            let mut before = hi;
            if m.key_eq.equal(m.key(inserted), m.key(before)) {
                before = m.links[before].next;
                if before != inserted {
                    m.splice(before, inserted);
                }
                m.buckets[b][1] = inserted;
                inserted = next_inserted;
                continue;
            }
            loop {
                if m.buckets[b][0] == before {
                    m.splice(before, inserted);
                    m.buckets[b][0] = inserted;
                    break;
                }
                before = m.links[before].prev;
                if m.key_eq.equal(m.key(inserted), m.key(before)) {
                    let after_equal = m.links[before].next;
                    m.splice(after_equal, inserted);
                    break;
                }
            }
            inserted = next_inserted;
        }
        guard.armed = false;
    }

    /// `emplace` and `try_emplace`: finds `k`; if absent makes the value, grows the table if the
    /// new size needs it and links the new node at the end of its bucket's run.
    fn emplace_with<F: FnOnce() -> V>(&mut self, k: K, make: F) -> (usize, bool) {
        let hash = self.hasher.hash_key(&k);
        let (mut before, dup) = self.find_last(&k, hash);
        if let Some(d) = dup {
            return (d, false);
        }
        let node = self.alloc(k, make());
        if self.check_rehash_required_1() {
            let grow = self.desired_grow_bucket_count(self.len + 1);
            self.forced_rehash(grow);
            before = self.find_last(self.key(node), hash).0;
        }
        self.insert_new_node_before(hash, before, node);
        (node, true)
    }

    /// `insert({k, v})` and `emplace(k, v)`: adds the element unless the key is there; returns
    /// whether it did (the value is dropped if not).
    pub fn insert(&mut self, k: K, v: V) -> bool {
        self.emplace_with(k, || v).1
    }

    /// `insert_or_assign(k, v)`: adds the element, or assigns the value of the one there; returns
    /// whether it added.
    pub fn insert_or_assign(&mut self, k: K, v: V) -> bool {
        let mut v = Some(v);
        let (node, inserted) = self.emplace_with(k, || v.take().expect("taken once"));
        if let Some(v) = v {
            self.slots[node]
                .as_mut()
                .expect("a list node holds an element")
                .1 = v;
        }
        inserted
    }

    /// `try_emplace(k, make())`: the value of `k`, added with `make()` if it is not there.
    pub fn get_or_insert_with<F: FnOnce() -> V>(&mut self, k: K, make: F) -> &mut V {
        let (node, _) = self.emplace_with(k, make);
        &mut self.slots[node]
            .as_mut()
            .expect("a list node holds an element")
            .1
    }

    /// `operator[](k)`: the value of `k`, added value-initialised if it is not there.
    pub fn get_or_insert_default(&mut self, k: K) -> &mut V
    where
        V: Default,
    {
        self.get_or_insert_with(k, V::default)
    }

    /// `find(k)`: the value of `k`.
    pub fn get<Q: ?Sized>(&self, k: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        H: HashFn<Q>,
        E: KeyEq<Q>,
    {
        self.get_key_value(k).map(|(_, v)| v)
    }

    /// `find(k)`: the stored key and value of `k`.
    pub fn get_key_value<Q: ?Sized>(&self, k: &Q) -> Option<(&K, &V)>
    where
        K: Borrow<Q>,
        H: HashFn<Q>,
        E: KeyEq<Q>,
    {
        let hash = self.hasher.hash_key(k);
        let node = self.find_last(k, hash).1?;
        let (k, v) = self.entry(node);
        Some((k, v))
    }

    /// `find(k)`: the value of `k`, mutable.
    pub fn get_mut<Q: ?Sized>(&mut self, k: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        H: HashFn<Q>,
        E: KeyEq<Q>,
    {
        let hash = self.hasher.hash_key(k);
        let node = self.find_last(k, hash).1?;
        self.slots[node].as_mut().map(|(_, v)| v)
    }

    /// `contains(k)`.
    pub fn contains_key<Q: ?Sized>(&self, k: &Q) -> bool
    where
        K: Borrow<Q>,
        H: HashFn<Q>,
        E: KeyEq<Q>,
    {
        let hash = self.hasher.hash_key(k);
        self.find_last(k, hash).1.is_some()
    }

    /// `count(k)`: 0 or 1.
    pub fn count<Q: ?Sized>(&self, k: &Q) -> usize
    where
        K: Borrow<Q>,
        H: HashFn<Q>,
        E: KeyEq<Q>,
    {
        usize::from(self.contains_key(k))
    }

    /// `bucket(k)`: `hash(k) & (bucket_count - 1)`.
    pub fn bucket<Q: ?Sized>(&self, k: &Q) -> usize
    where
        H: HashFn<Q>,
    {
        self.hasher.hash_key(k) as usize & self.mask
    }

    /// `erase(k)`: removes the element of `k` and returns its value.
    pub fn remove<Q: ?Sized>(&mut self, k: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        H: HashFn<Q>,
        E: KeyEq<Q>,
    {
        self.remove_entry(k).map(|(_, v)| v)
    }

    /// `erase(k)`: removes the element of `k` and returns it.
    pub fn remove_entry<Q: ?Sized>(&mut self, k: &Q) -> Option<(K, V)>
    where
        K: Borrow<Q>,
        H: HashFn<Q>,
        E: KeyEq<Q>,
    {
        let hash = self.hasher.hash_key(k);
        let node = self.find_last(k, hash).1?;
        self.erase_bucket(node, hash as usize & self.mask);
        Some(self.unlink(node))
    }

    /// `std::erase_if` and the `it = erase(it)` loop: walks the elements in order and erases each
    /// one `keep` rejects.
    pub fn retain<F: FnMut(&K, &mut V) -> bool>(&mut self, mut keep: F) {
        let mut node = self.links[HEAD].next;
        while node != HEAD {
            let next = self.links[node].next;
            let kept = {
                let (k, v) = self.slots[node]
                    .as_mut()
                    .expect("a list node holds an element");
                keep(k, v)
            };
            if !kept {
                let b = self.hasher.hash_key(self.key(node)) as usize & self.mask;
                self.erase_bucket(node, b);
                self.unlink(node);
            }
            node = next;
        }
    }

    /// `rehash(n)`: at least `n` buckets and at least `size / max_load_factor`, rounded up to a
    /// power of two; nothing if the table has that many already.
    pub fn rehash(&mut self, n: usize) {
        let n = crate::max(n, self.min_load_factor_buckets(self.len));
        if n <= self.maxidx {
            return;
        }
        self.forced_rehash(n);
    }

    /// `reserve(n)`: `rehash(ceilf(n / max_load_factor))`.
    pub fn reserve(&mut self, n: usize) {
        self.rehash(self.min_load_factor_buckets(n));
    }
}

/// The copy constructor: the same bucket count and maximum load factor, the elements inserted
/// in the source's order.
impl<K: Clone, V: Clone, H: HashFn<K> + Clone, E: KeyEq<K> + Clone> Clone
    for UnorderedMap<K, V, H, E>
{
    fn clone(&self) -> Self {
        let mut m = UnorderedMap {
            links: vec![Link {
                prev: HEAD,
                next: HEAD,
            }],
            slots: vec![None],
            free: Vec::new(),
            len: 0,
            buckets: vec![[HEAD, HEAD]; self.buckets.len()],
            mask: self.mask,
            maxidx: self.maxidx,
            max_load_factor: self.max_load_factor,
            hasher: self.hasher.clone(),
            key_eq: self.key_eq.clone(),
        };
        for (k, v) in self.iter() {
            m.emplace_with(k.clone(), || v.clone());
        }
        m
    }

    /// Copy assignment: the source's hasher, key equality and maximum load factor, its elements
    /// in its order, then a rehash into this map's own bucket count (grown if they need more).
    fn clone_from(&mut self, source: &Self) {
        self.hasher = source.hasher.clone();
        self.key_eq = source.key_eq.clone();
        self.max_load_factor = source.max_load_factor;
        self.clear();
        let mut guard = ClearGuard {
            map: self,
            armed: true,
        };
        let m = &mut *guard.map;
        for (k, v) in source.iter() {
            let node = m.alloc(k.clone(), v.clone());
            m.push_back(node);
        }
        let grow = m.desired_grow_bucket_count(m.len);
        m.forced_rehash(grow);
        guard.armed = false;
    }
}

/// `operator==`: the same size, and every element's key found in `other` with an equal value.
impl<K, V: PartialEq, H: HashFn<K>, E: KeyEq<K>> PartialEq for UnorderedMap<K, V, H, E> {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len && self.iter().all(|(k, v)| other.get(k) == Some(v))
    }
}

impl<K: fmt::Debug, V: fmt::Debug, H, E> fmt::Debug for UnorderedMap<K, V, H, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

/// The range constructor: an empty map, then `insert` of each element in order.
impl<K, V, H: HashFn<K> + Default, E: KeyEq<K> + Default> FromIterator<(K, V)>
    for UnorderedMap<K, V, H, E>
{
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let mut m = Self::default();
        m.extend(iter);
        m
    }
}

/// `insert(first, last)`: `insert` of each element in order.
impl<K, V, H: HashFn<K>, E: KeyEq<K>> Extend<(K, V)> for UnorderedMap<K, V, H, E> {
    fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, iter: I) {
        for (k, v) in iter {
            self.insert(k, v);
        }
    }
}

/// An iterator over the elements of an [`UnorderedMap`] (or of one bucket), in list order.
pub struct Iter<'a, K, V> {
    links: &'a [Link],
    slots: &'a [Option<(K, V)>],
    cur: usize,
    end: usize,
    remaining: usize,
}

impl<'a, K, V> Iterator for Iter<'a, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        if self.cur == self.end {
            return None;
        }
        let node = self.cur;
        self.cur = self.links[node].next;
        self.remaining -= 1;
        let (k, v) = self.slots[node].as_ref()?;
        Some((k, v))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<K, V> ExactSizeIterator for Iter<'_, K, V> {}

impl<K, V> Clone for Iter<'_, K, V> {
    fn clone(&self) -> Self {
        Iter {
            links: self.links,
            slots: self.slots,
            cur: self.cur,
            end: self.end,
            remaining: self.remaining,
        }
    }
}

/// An iterator over the elements of an [`UnorderedMap`] in list order, the values mutable.
pub struct IterMut<'a, K, V> {
    links: &'a [Link],
    slots: *mut Option<(K, V)>,
    cur: usize,
    remaining: usize,
    _marker: PhantomData<&'a mut (K, V)>,
}

impl<'a, K, V> Iterator for IterMut<'a, K, V> {
    type Item = (&'a K, &'a mut V);

    fn next(&mut self) -> Option<Self::Item> {
        if self.cur == HEAD {
            return None;
        }
        let node = self.cur;
        self.cur = self.links[node].next;
        self.remaining -= 1;
        // SAFETY: `slots` points at the map's slot vector, which the iterator's lifetime borrows
        // mutably and nothing else touches; the list visits each node once, so each slot is lent
        // out once, and `node` is a list node, so it is inside the vector.
        let slot = unsafe { &mut *self.slots.add(node) };
        let (k, v) = slot.as_mut()?;
        Some((&*k, v))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<K, V> ExactSizeIterator for IterMut<'_, K, V> {}

/// The elements of an [`UnorderedMap`] by value, in list order.
pub struct IntoIter<K, V> {
    inner: std::vec::IntoIter<(K, V)>,
}

impl<K, V> Iterator for IntoIter<K, V> {
    type Item = (K, V);

    fn next(&mut self) -> Option<(K, V)> {
        self.inner.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<K, V> ExactSizeIterator for IntoIter<K, V> {}

impl<K, V, H, E> IntoIterator for UnorderedMap<K, V, H, E> {
    type Item = (K, V);
    type IntoIter = IntoIter<K, V>;

    fn into_iter(mut self) -> IntoIter<K, V> {
        let mut out = Vec::with_capacity(self.len);
        let mut node = self.links[HEAD].next;
        while node != HEAD {
            let next = self.links[node].next;
            if let Some(e) = self.slots[node].take() {
                out.push(e);
            }
            node = next;
        }
        IntoIter {
            inner: out.into_iter(),
        }
    }
}

impl<'a, K, V, H, E> IntoIterator for &'a UnorderedMap<K, V, H, E> {
    type Item = (&'a K, &'a V);
    type IntoIter = Iter<'a, K, V>;

    fn into_iter(self) -> Iter<'a, K, V> {
        self.iter()
    }
}

impl<'a, K, V, H, E> IntoIterator for &'a mut UnorderedMap<K, V, H, E> {
    type Item = (&'a K, &'a mut V);
    type IntoIter = IterMut<'a, K, V>;

    fn into_iter(self) -> IterMut<'a, K, V> {
        self.iter_mut()
    }
}

/// `std::unordered_set<K, H, E>` with MSVC's layout and order: an [`UnorderedMap`] of `()`.
pub struct UnorderedSet<K, H = StdHash, E = EqualTo> {
    map: UnorderedMap<K, (), H, E>,
}

impl<K> UnorderedSet<K> {
    /// `std::unordered_set<K>()`: empty, 8 buckets, maximum load factor 1.
    pub fn new() -> Self {
        UnorderedSet {
            map: UnorderedMap::new(),
        }
    }
}

impl<K: StdHashable + PartialEq> UnorderedSet<K> {
    /// `std::unordered_set<K>(buckets)`.
    pub fn with_buckets(buckets: usize) -> Self {
        UnorderedSet {
            map: UnorderedMap::with_buckets(buckets),
        }
    }
}

impl<K, H: Default, E: Default> Default for UnorderedSet<K, H, E> {
    fn default() -> Self {
        UnorderedSet {
            map: UnorderedMap::default(),
        }
    }
}

impl<K, H, E> UnorderedSet<K, H, E> {
    /// An empty set with the given hasher and key equality.
    pub fn with_hasher(hasher: H, key_eq: E) -> Self {
        UnorderedSet {
            map: UnorderedMap::with_hasher(hasher, key_eq),
        }
    }

    /// `size()`.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// `empty()`.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// `bucket_count()`.
    pub fn bucket_count(&self) -> usize {
        self.map.bucket_count()
    }

    /// `bucket_size(b)`.
    pub fn bucket_size(&self, b: usize) -> usize {
        self.map.bucket_size(b)
    }

    /// `begin(b)` to `end(b)`.
    pub fn bucket_iter(&self, b: usize) -> SetIter<'_, K> {
        SetIter {
            inner: self.map.bucket_iter(b),
        }
    }

    /// `load_factor()`.
    pub fn load_factor(&self) -> f32 {
        self.map.load_factor()
    }

    /// `max_load_factor()`.
    pub fn max_load_factor(&self) -> f32 {
        self.map.max_load_factor()
    }

    /// `max_load_factor(f)`.
    pub fn set_max_load_factor(&mut self, f: f32) {
        self.map.set_max_load_factor(f);
    }

    /// The elements in iteration order.
    pub fn iter(&self) -> SetIter<'_, K> {
        SetIter {
            inner: self.map.iter(),
        }
    }

    /// `clear()`.
    pub fn clear(&mut self) {
        self.map.clear();
    }
}

impl<K, H: HashFn<K>, E: KeyEq<K>> UnorderedSet<K, H, E> {
    /// `std::unordered_set<K, H, E>(buckets, hasher, key_eq)`.
    pub fn with_buckets_and_hasher(buckets: usize, hasher: H, key_eq: E) -> Self {
        UnorderedSet {
            map: UnorderedMap::with_buckets_and_hasher(buckets, hasher, key_eq),
        }
    }

    /// `insert(k)`: whether it added `k`.
    pub fn insert(&mut self, k: K) -> bool {
        self.map.insert(k, ())
    }

    /// `find(k)`: the stored key equal to `k`.
    pub fn get<Q: ?Sized>(&self, k: &Q) -> Option<&K>
    where
        K: Borrow<Q>,
        H: HashFn<Q>,
        E: KeyEq<Q>,
    {
        self.map.get_key_value(k).map(|(k, _)| k)
    }

    /// `contains(k)`.
    pub fn contains<Q: ?Sized>(&self, k: &Q) -> bool
    where
        K: Borrow<Q>,
        H: HashFn<Q>,
        E: KeyEq<Q>,
    {
        self.map.contains_key(k)
    }

    /// `count(k)`.
    pub fn count<Q: ?Sized>(&self, k: &Q) -> usize
    where
        K: Borrow<Q>,
        H: HashFn<Q>,
        E: KeyEq<Q>,
    {
        self.map.count(k)
    }

    /// `bucket(k)`.
    pub fn bucket<Q: ?Sized>(&self, k: &Q) -> usize
    where
        H: HashFn<Q>,
    {
        self.map.bucket(k)
    }

    /// `erase(k)`: whether it removed `k`.
    pub fn remove<Q: ?Sized>(&mut self, k: &Q) -> bool
    where
        K: Borrow<Q>,
        H: HashFn<Q>,
        E: KeyEq<Q>,
    {
        self.map.remove_entry(k).is_some()
    }

    /// `erase(k)`: removes `k` and returns the stored key.
    pub fn take<Q: ?Sized>(&mut self, k: &Q) -> Option<K>
    where
        K: Borrow<Q>,
        H: HashFn<Q>,
        E: KeyEq<Q>,
    {
        self.map.remove_entry(k).map(|(k, _)| k)
    }

    /// `std::erase_if`: erases, in order, each element `keep` rejects.
    pub fn retain<F: FnMut(&K) -> bool>(&mut self, mut keep: F) {
        self.map.retain(|k, _| keep(k));
    }

    /// `rehash(n)`.
    pub fn rehash(&mut self, n: usize) {
        self.map.rehash(n);
    }

    /// `reserve(n)`.
    pub fn reserve(&mut self, n: usize) {
        self.map.reserve(n);
    }
}

/// The copy constructor.
impl<K: Clone, H: HashFn<K> + Clone, E: KeyEq<K> + Clone> Clone for UnorderedSet<K, H, E> {
    fn clone(&self) -> Self {
        UnorderedSet {
            map: self.map.clone(),
        }
    }

    /// Copy assignment.
    fn clone_from(&mut self, source: &Self) {
        self.map.clone_from(&source.map);
    }
}

/// `operator==`: the same size, and every element found in `other`.
impl<K, H: HashFn<K>, E: KeyEq<K>> PartialEq for UnorderedSet<K, H, E> {
    fn eq(&self, other: &Self) -> bool {
        self.map == other.map
    }
}

impl<K: fmt::Debug, H, E> fmt::Debug for UnorderedSet<K, H, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

/// The range constructor.
impl<K, H: HashFn<K> + Default, E: KeyEq<K> + Default> FromIterator<K> for UnorderedSet<K, H, E> {
    fn from_iter<I: IntoIterator<Item = K>>(iter: I) -> Self {
        let mut s = Self::default();
        s.extend(iter);
        s
    }
}

/// `insert(first, last)`.
impl<K, H: HashFn<K>, E: KeyEq<K>> Extend<K> for UnorderedSet<K, H, E> {
    fn extend<I: IntoIterator<Item = K>>(&mut self, iter: I) {
        for k in iter {
            self.insert(k);
        }
    }
}

/// An iterator over the elements of an [`UnorderedSet`] (or of one bucket), in list order.
pub struct SetIter<'a, K> {
    inner: Iter<'a, K, ()>,
}

impl<'a, K> Iterator for SetIter<'a, K> {
    type Item = &'a K;

    fn next(&mut self) -> Option<&'a K> {
        self.inner.next().map(|(k, _)| k)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<K> ExactSizeIterator for SetIter<'_, K> {}

impl<K> Clone for SetIter<'_, K> {
    fn clone(&self) -> Self {
        SetIter {
            inner: self.inner.clone(),
        }
    }
}

impl<'a, K, H, E> IntoIterator for &'a UnorderedSet<K, H, E> {
    type Item = &'a K;
    type IntoIter = SetIter<'a, K>;

    fn into_iter(self) -> SetIter<'a, K> {
        self.iter()
    }
}

/// The elements of an [`UnorderedSet`] by value, in list order.
pub struct SetIntoIter<K> {
    inner: IntoIter<K, ()>,
}

impl<K> Iterator for SetIntoIter<K> {
    type Item = K;

    fn next(&mut self) -> Option<K> {
        self.inner.next().map(|(k, _)| k)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<K> ExactSizeIterator for SetIntoIter<K> {}

impl<K, H, E> IntoIterator for UnorderedSet<K, H, E> {
    type Item = K;
    type IntoIter = SetIntoIter<K>;

    fn into_iter(self) -> SetIntoIter<K> {
        SetIntoIter {
            inner: self.map.into_iter(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_key_goes_to_the_front_of_its_bucket_run() {
        // 8 buckets, a hash that is the key: 1, 9 and 17 share bucket 1, 2 has bucket 2.
        let mut m = UnorderedMap::with_hasher(|k: &u64| *k, EqualTo);
        for k in [1u64, 2, 9, 17] {
            m.insert(k, ());
        }
        let order: Vec<u64> = m.keys().copied().collect();
        assert_eq!(order, [17, 9, 1, 2]);
        assert_eq!(m.bucket_size(1), 3);
        assert_eq!(
            m.bucket_iter(1).map(|(k, _)| *k).collect::<Vec<_>>(),
            [17, 9, 1]
        );
    }

    #[test]
    fn a_rehash_reverses_each_run() {
        // Even keys hash to 0, odd ones to 1. After 0..8 the list is 6 4 2 0 7 5 3 1; the ninth
        // key needs 64 buckets, and the rehash walks the list in order, putting each element at
        // the front of its run: 0 2 4 6 1 3 5 7. Then 8 goes to the front of bucket 0.
        let mut m = UnorderedMap::with_hasher(|k: &u64| *k % 2, EqualTo);
        for k in 0u64..8 {
            m.insert(k, ());
        }
        assert_eq!(
            m.keys().copied().collect::<Vec<_>>(),
            [6, 4, 2, 0, 7, 5, 3, 1]
        );
        m.insert(8, ());
        assert_eq!(m.bucket_count(), 64);
        assert_eq!(
            m.keys().copied().collect::<Vec<_>>(),
            [8, 0, 2, 4, 6, 1, 3, 5, 7]
        );
    }
}
