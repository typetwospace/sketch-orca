// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/MutablePriorityQueue.hpp lines 1-453 whole, of OrcaSlicer
// v2.4.2, translated on 2026-11-13 by index-1. Copyright the OrcaSlicer authors, the BambuStudio
// authors (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU Affero General
// Public License, version 3 (AGPL-3.0; LICENSE); its SkipHeapAddressing was taken there from
// rollbear/prio_queue (prio_queue.hpp), copyright Björn Fahller 2015, under the Boost Software
// License, version 1.0 (BSL-1.0; licenses/BSL-1.0.txt).

//! `MutablePriorityQueue` and `MutableSkipHeapPriorityQueue`: binary min-heaps whose items learn
//! their heap address through an index setter, so that a caller can `remove` or `update` an item
//! by its address.
//!
//! - [`MutablePriorityQueue`] is a plain binary heap from slot 0. [`MutableSkipHeapPriorityQueue`]
//!   is a binary heap of binary miniheaps of `B` slots each ([`SkipHeapAddressing`]), whose slot 0
//!   is padding (a `T::default()` that is never an item), so the root is at 1.
//! - The heaps sift exactly as the C++ does: on the way up a parent is swapped with its child
//!   unless `less(parent, child)`, so an item equal to its parent moves up past it; on the way
//!   down the second child is taken unless `less(first, second)`, and the walk stops at
//!   `less(parent, child)`. The index setter is called on each moved item in the C++'s order (the
//!   item going down, then the one going up).
//! - The engine's release build defines `NDEBUG`: a popped, removed or cleared item's index is
//!   set to `usize::MAX` only when `RESET` (the C++'s `ResetIndexWhenRemoved`) is true; otherwise
//!   it keeps its last address. Dropping a queue clears it, as the C++ destructor does.
//! - Where the C++ reads outside the heap (an empty queue's `top` or `pop`, an index past the
//!   end, a skip heap's padding slot), the queue stops with [`crate::stop::NotPorted`]
//!   (`parity/sop1/index.md`, IXNP).
//!
//! `T` is a copyable item handle (the C++ requires a trivially copyable `T`; the engine stores
//! pointers, which become indices or references here). The index setter is `FnMut(&mut T, usize)`
//! and the predicate `FnMut(&T, &T) -> bool`, "less goes first".

use crate::stop::not_ported;

/// `InvalidQueueID`: `std::numeric_limits<size_t>::max()`, the index of an item not in a queue.
pub const INVALID_QUEUE_ID: usize = usize::MAX;

const TOP_POP: &str = "MutablePriorityQueue::top, MutablePriorityQueue::pop";
const TOP_POP_EMPTY: &str =
    "the queue is empty: it reads the heap's first slot, which does not exist";
const AT: &str =
    "MutablePriorityQueue::remove, MutablePriorityQueue::update, MutablePriorityQueue::operator[]";
const AT_PAST_END: &str = "the index is size() or more";
const SKIP_TOP_POP: &str = "MutableSkipHeapPriorityQueue::top, MutableSkipHeapPriorityQueue::pop";
const SKIP_TOP_POP_EMPTY: &str = "the queue is empty: it reads heap slot 1, which does not exist";
const SKIP_AT: &str = "MutableSkipHeapPriorityQueue::remove, MutableSkipHeapPriorityQueue::update, MutableSkipHeapPriorityQueue::operator[]";
const SKIP_AT_PADDING: &str = "the address is a padding slot (a multiple of the block size)";
const SKIP_AT_PAST_END: &str = "the address is at or past the heap's end";

/// `MutablePriorityQueue<T, IndexSetter, LessPredicate, ResetIndexWhenRemoved>`.
pub struct MutablePriorityQueue<T, S, L, const RESET: bool>
where
    T: Copy,
    S: FnMut(&mut T, usize),
    L: FnMut(&T, &T) -> bool,
{
    heap: Vec<T>,
    index_setter: S,
    less_predicate: L,
}

/// `make_mutable_priority_queue<T, ResetIndexWhenRemoved>(index_setter, less_predicate)`.
pub fn make_mutable_priority_queue<T, const RESET: bool, S, L>(
    index_setter: S,
    less_predicate: L,
) -> MutablePriorityQueue<T, S, L, RESET>
where
    T: Copy,
    S: FnMut(&mut T, usize),
    L: FnMut(&T, &T) -> bool,
{
    MutablePriorityQueue {
        heap: Vec::new(),
        index_setter,
        less_predicate,
    }
}

impl<T, S, L, const RESET: bool> MutablePriorityQueue<T, S, L, RESET>
where
    T: Copy,
    S: FnMut(&mut T, usize),
    L: FnMut(&T, &T) -> bool,
{
    /// `MutablePriorityQueue(index_setter, less_predicate)`.
    pub fn new(index_setter: S, less_predicate: L) -> Self {
        make_mutable_priority_queue::<T, RESET, S, L>(index_setter, less_predicate)
    }

    /// `clear()`: with `RESET`, every item's index set to `usize::MAX` first.
    pub fn clear(&mut self) {
        // Only mark as removed from the queue in release mode, if configured so.
        if RESET {
            for idx in 0..self.heap.len() {
                // Mark as removed from the queue.
                (self.index_setter)(&mut self.heap[idx], INVALID_QUEUE_ID);
            }
        }
        self.heap.clear();
    }

    /// `reserve(cnt)`.
    pub fn reserve(&mut self, cnt: usize) {
        self.heap.reserve(cnt);
    }

    /// `push(item)`: appended, told its address, sifted up.
    pub fn push(&mut self, item: T) {
        let idx = self.heap.len();
        self.heap.push(item);
        (self.index_setter)(&mut self.heap[idx], idx);
        self.update_heap_up(0, idx);
    }

    /// `pop()`: the top removed (its index reset with `RESET`), the last item moved to the top and
    /// sifted down.
    pub fn pop(&mut self) {
        if self.heap.is_empty() {
            not_ported(TOP_POP, TOP_POP_EMPTY);
        }
        // Only mark as removed from the queue in release mode, if configured so.
        if RESET {
            (self.index_setter)(&mut self.heap[0], INVALID_QUEUE_ID);
        }
        if self.heap.len() > 1 {
            let last = self.heap.len() - 1;
            self.heap[0] = self.heap[last];
            self.heap.pop();
            (self.index_setter)(&mut self.heap[0], 0);
            let bottom = self.heap.len() - 1;
            self.update_heap_down(0, bottom);
        } else {
            self.heap.clear();
        }
    }

    /// `top()`.
    pub fn top(&mut self) -> &mut T {
        if self.heap.is_empty() {
            not_ported(TOP_POP, TOP_POP_EMPTY);
        }
        &mut self.heap[0]
    }

    /// `remove(idx)`: the item at `idx` removed (its index reset with `RESET`), the last item moved
    /// there and sifted down, then up.
    pub fn remove(&mut self, idx: usize) {
        if idx >= self.heap.len() {
            not_ported(AT, AT_PAST_END);
        }
        // Only mark as removed from the queue in release mode, if configured so.
        if RESET {
            (self.index_setter)(&mut self.heap[idx], INVALID_QUEUE_ID);
        }
        if idx + 1 == self.heap.len() {
            self.heap.pop();
            return;
        }
        let last = self.heap.len() - 1;
        self.heap[idx] = self.heap[last];
        (self.index_setter)(&mut self.heap[idx], idx);
        self.heap.pop();
        let bottom = self.heap.len() - 1;
        self.update_heap_down(idx, bottom);
        self.update_heap_up(0, idx);
    }

    /// `update(idx)`: the item at `idx` (a copy taken first) removed and pushed again.
    pub fn update(&mut self, idx: usize) {
        if idx >= self.heap.len() {
            not_ported(AT, AT_PAST_END);
        }
        let item = self.heap[idx];
        self.remove(idx);
        self.push(item);
    }

    /// `size()`.
    #[must_use]
    pub fn size(&self) -> usize {
        self.heap.len()
    }

    /// `empty()`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }

    /// `operator[](idx)`.
    #[must_use]
    pub fn get(&self, idx: usize) -> &T {
        if idx >= self.heap.len() {
            not_ported(AT, AT_PAST_END);
        }
        &self.heap[idx]
    }

    /// `operator[](idx)`, for writing.
    pub fn get_mut(&mut self, idx: usize) -> &mut T {
        if idx >= self.heap.len() {
            not_ported(AT, AT_PAST_END);
        }
        &mut self.heap[idx]
    }

    /// `begin()` to `end()`: the heap's items in slot order.
    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        &self.heap
    }

    /// `update_heap_up(top, bottom)`: the item at `bottom` swapped with its parent while the
    /// parent is not less than it, up to `top`.
    fn update_heap_up(&mut self, top: usize, bottom: usize) {
        let mut child_idx = bottom;
        loop {
            let parent_idx = child_idx.wrapping_sub(1) >> 1;
            if child_idx == 0 || parent_idx < top {
                break;
            }
            // Switch nodes.
            if !(self.less_predicate)(&self.heap[parent_idx], &self.heap[child_idx]) {
                let mut tmp = self.heap[parent_idx];
                (self.index_setter)(&mut tmp, child_idx);
                (self.index_setter)(&mut self.heap[child_idx], parent_idx);
                self.heap[parent_idx] = self.heap[child_idx];
                self.heap[child_idx] = tmp;
            }
            // Shift up.
            child_idx = parent_idx;
        }
    }

    /// `update_heap_down(top, bottom)`: the item at `top` swapped with its lesser child (the
    /// second unless the first is less) until it is less than that child, down to `bottom`.
    fn update_heap_down(&mut self, top: usize, bottom: usize) {
        let mut parent_idx = top;
        loop {
            let mut child_idx = (parent_idx << 1) + 1;
            if child_idx > bottom {
                break;
            }
            let child2_idx = child_idx + 1;
            if child2_idx <= bottom
                && !(self.less_predicate)(&self.heap[child_idx], &self.heap[child2_idx])
            {
                child_idx = child2_idx;
            }
            if (self.less_predicate)(&self.heap[parent_idx], &self.heap[child_idx]) {
                return;
            }
            // Switch nodes.
            let mut tmp = self.heap[parent_idx];
            (self.index_setter)(&mut tmp, child_idx);
            (self.index_setter)(&mut self.heap[child_idx], parent_idx);
            self.heap[parent_idx] = self.heap[child_idx];
            self.heap[child_idx] = tmp;
            // Shift down.
            parent_idx = child_idx;
        }
    }
}

impl<T, S, L, const RESET: bool> Drop for MutablePriorityQueue<T, S, L, RESET>
where
    T: Copy,
    S: FnMut(&mut T, usize),
    L: FnMut(&T, &T) -> bool,
{
    /// `~MutablePriorityQueue()`: `clear()`.
    fn drop(&mut self) {
        self.clear();
    }
}

/// `SkipHeapAddressing<blocking>`: the addresses of a binary heap of binary miniheaps of `B` slots
/// (a power of two), the first slot of each a padding slot. A block's leaves are its second half;
/// a leaf's children are the roots of two blocks of the next level.
pub struct SkipHeapAddressing<const B: usize>;

impl<const B: usize> SkipHeapAddressing<B> {
    /// `block_size`.
    pub const BLOCK_SIZE: usize = B;
    /// `block_mask`.
    pub const BLOCK_MASK: usize = B - 1;
    /// `static_assert((block_size & block_mask) == 0U, "block size must be 2^n for some integer
    /// n")`.
    const POWER_OF_TWO: () = assert!(
        B & (B - 1) == 0,
        "block size must be 2^n for some integer n"
    );

    /// `child_of(node_no)`: the first child of a slot.
    #[must_use]
    pub fn child_of(node_no: usize) -> usize {
        let () = Self::POWER_OF_TWO;
        if !Self::is_block_leaf(node_no) {
            // If not a leaf, then it is sufficient to just traverse down inside a miniheap.
            return node_no.wrapping_add(Self::block_offset(node_no));
        }
        // Otherwise skip to a root of a child miniheap.
        Self::block_base(node_no)
            .wrapping_add(1)
            .wrapping_add(Self::child_no(node_no).wrapping_mul(2))
            .wrapping_mul(B)
            .wrapping_add(1)
    }

    /// `parent_of(node_no)`: a slot's parent (meaningless for slot 1, the root, which the heap
    /// never asks for before it stops).
    #[must_use]
    pub fn parent_of(node_no: usize) -> usize {
        let () = Self::POWER_OF_TWO;
        let node_root = Self::block_base(node_no);
        if !Self::is_block_root(node_no) {
            // If not a block (miniheap) root, then it is sufficient to just traverse up inside a
            // miniheap.
            return node_root.wrapping_add(Self::block_offset(node_no) / 2);
        }
        // Otherwise skipping from a root of one miniheap into a leaf of another miniheap. The
        // address of a parent miniheap block: one miniheap branches at (block_size / 2) leaves to
        // (block_size) miniheaps.
        let parent_base = Self::block_base((node_root / B).wrapping_sub(1));
        // Index of a leaf of a parent miniheap, which is a parent of node_no.
        let child = (node_no.wrapping_sub(B) / B).wrapping_sub(parent_base) / 2;
        // The address of a parent miniheap, plus the address of a leaf of a parent miniheap.
        parent_base.wrapping_add(B / 2).wrapping_add(child)
    }

    /// `is_block_leaf(node_no)`: leaves are stored inside the second half of a block.
    #[must_use]
    pub fn is_block_leaf(node_no: usize) -> bool {
        (node_no & (B >> 1)) != 0
    }

    /// `is_padding(node_no)`: unused space, to make the addressing quick.
    #[must_use]
    pub fn is_padding(node_no: usize) -> bool {
        Self::block_offset(node_no) == 0
    }

    /// `is_block_root(node_no)`: the root of a block (of a miniheap).
    #[must_use]
    pub fn is_block_root(node_no: usize) -> bool {
        Self::block_offset(node_no) == 1
    }

    /// `block_offset(node_no)`: the offset inside a block.
    #[must_use]
    pub fn block_offset(node_no: usize) -> usize {
        node_no & Self::BLOCK_MASK
    }

    /// `block_base(node_no)`: the base address of a block.
    #[must_use]
    pub fn block_base(node_no: usize) -> usize {
        node_no & !Self::BLOCK_MASK
    }

    /// `child_no(node_no)`: the index of a leaf in its block.
    #[must_use]
    pub fn child_no(node_no: usize) -> usize {
        node_no & (Self::BLOCK_MASK >> 1)
    }
}

/// `MutableSkipHeapPriorityQueue<T, IndexSetter, LessPredicate, blocking, ResetIndexWhenRemoved>`.
pub struct MutableSkipHeapPriorityQueue<T, S, L, const B: usize, const RESET: bool>
where
    T: Copy + Default,
    S: FnMut(&mut T, usize),
    L: FnMut(&T, &T) -> bool,
{
    heap: Vec<T>,
    index_setter: S,
    less_predicate: L,
}

/// `make_miniheap_mutable_priority_queue<T, BlockSize, ResetIndexWhenRemoved>(index_setter,
/// less_predicate)`.
pub fn make_miniheap_mutable_priority_queue<T, const B: usize, const RESET: bool, S, L>(
    index_setter: S,
    less_predicate: L,
) -> MutableSkipHeapPriorityQueue<T, S, L, B, RESET>
where
    T: Copy + Default,
    S: FnMut(&mut T, usize),
    L: FnMut(&T, &T) -> bool,
{
    let () = SkipHeapAddressing::<B>::POWER_OF_TWO;
    MutableSkipHeapPriorityQueue {
        heap: Vec::new(),
        index_setter,
        less_predicate,
    }
}

impl<T, S, L, const B: usize, const RESET: bool> MutableSkipHeapPriorityQueue<T, S, L, B, RESET>
where
    T: Copy + Default,
    S: FnMut(&mut T, usize),
    L: FnMut(&T, &T) -> bool,
{
    /// `MutableSkipHeapPriorityQueue(index_setter, less_predicate)`.
    pub fn new(index_setter: S, less_predicate: L) -> Self {
        make_miniheap_mutable_priority_queue::<T, B, RESET, S, L>(index_setter, less_predicate)
    }

    /// `clear()`: with `RESET`, every item's index (not the padding's) set to `usize::MAX` first.
    pub fn clear(&mut self) {
        // Only mark as removed from the queue in release mode, if configured so.
        if RESET {
            for idx in 0..self.heap.len() {
                // Mark as removed from the queue.
                if !SkipHeapAddressing::<B>::is_padding(idx) {
                    (self.index_setter)(&mut self.heap[idx], INVALID_QUEUE_ID);
                }
            }
        }
        self.heap.clear();
    }

    /// `reserve(cnt)`: one unused slot per miniheap besides the items.
    pub fn reserve(&mut self, cnt: usize) {
        self.heap.reserve(cnt + (cnt + (B - 1)) / (B - 1));
    }

    /// `push(item)`: a padding slot first where the next slot is one, then the item appended, told
    /// its address and sifted up.
    pub fn push(&mut self, item: T) {
        if SkipHeapAddressing::<B>::is_padding(self.heap.len()) {
            self.heap.push(T::default());
        }
        let idx = self.heap.len();
        self.heap.push(item);
        (self.index_setter)(&mut self.heap[idx], idx);
        self.update_heap_up(1, idx);
    }

    /// `pop()`: the top (slot 1) removed (its index reset with `RESET`), the last item moved there
    /// and sifted down.
    pub fn pop(&mut self) {
        if self.heap.is_empty() {
            not_ported(SKIP_TOP_POP, SKIP_TOP_POP_EMPTY);
        }
        // Only mark as removed from the queue in release mode, if configured so.
        if RESET {
            (self.index_setter)(&mut self.heap[1], INVALID_QUEUE_ID);
        }
        // The zeroth slot is padding, so a non-empty queue has at least two slots.
        if self.heap.len() > 2 {
            let last = self.heap.len() - 1;
            self.heap[1] = self.heap[last];
            self.pop_back();
            (self.index_setter)(&mut self.heap[1], 1);
            let bottom = self.heap.len() - 1;
            self.update_heap_down(1, bottom);
        } else {
            self.heap.clear();
        }
    }

    /// `top()`: slot 1.
    pub fn top(&mut self) -> &mut T {
        if self.heap.is_empty() {
            not_ported(SKIP_TOP_POP, SKIP_TOP_POP_EMPTY);
        }
        &mut self.heap[1]
    }

    /// The stop of an address the C++ cannot read: past the end, or a padding slot.
    fn check_address(&self, idx: usize) {
        if idx >= self.heap.len() {
            not_ported(SKIP_AT, SKIP_AT_PAST_END);
        }
        if SkipHeapAddressing::<B>::is_padding(idx) {
            not_ported(SKIP_AT, SKIP_AT_PADDING);
        }
    }

    /// `remove(idx)`: the item at `idx` removed (its index reset with `RESET`), the last item moved
    /// there and sifted down, then up.
    pub fn remove(&mut self, idx: usize) {
        self.check_address(idx);
        // Only mark as removed from the queue in release mode, if configured so.
        if RESET {
            (self.index_setter)(&mut self.heap[idx], INVALID_QUEUE_ID);
        }
        if idx + 1 == self.heap.len() {
            self.pop_back();
            return;
        }
        let last = self.heap.len() - 1;
        self.heap[idx] = self.heap[last];
        (self.index_setter)(&mut self.heap[idx], idx);
        self.pop_back();
        let bottom = self.heap.len() - 1;
        self.update_heap_down(idx, bottom);
        self.update_heap_up(1, idx);
    }

    /// `update(idx)`: the item at `idx` (a copy taken first) removed and pushed again.
    pub fn update(&mut self, idx: usize) {
        self.check_address(idx);
        let item = self.heap[idx];
        self.remove(idx);
        self.push(item);
    }

    /// `size()`: the slots less one padding slot per miniheap.
    #[must_use]
    pub fn size(&self) -> usize {
        // `m_heap.size() - (m_heap.size() + block_size - 1) / block_size`.
        self.heap.len() - self.heap.len().div_ceil(B)
    }

    /// `empty()`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }

    /// `operator[](idx)`.
    #[must_use]
    pub fn get(&self, idx: usize) -> &T {
        self.check_address(idx);
        &self.heap[idx]
    }

    /// `operator[](idx)`, for writing.
    pub fn get_mut(&mut self, idx: usize) -> &mut T {
        self.check_address(idx);
        &mut self.heap[idx]
    }

    /// The heap's slots, padding included (the C++'s private `m_heap`).
    #[must_use]
    pub fn slots(&self) -> &[T] {
        &self.heap
    }

    /// `pop_back()`: the last slot dropped, and the padding slot before it when it is one.
    fn pop_back(&mut self) {
        self.heap.pop();
        if SkipHeapAddressing::<B>::is_padding(self.heap.len() - 1) {
            self.heap.pop();
        }
    }

    /// `update_heap_up(top, bottom)`, through [`SkipHeapAddressing::parent_of`].
    fn update_heap_up(&mut self, top: usize, bottom: usize) {
        let mut child_idx = bottom;
        loop {
            let parent_idx = SkipHeapAddressing::<B>::parent_of(child_idx);
            if child_idx == 1 || parent_idx < top {
                break;
            }
            // Switch nodes.
            if !(self.less_predicate)(&self.heap[parent_idx], &self.heap[child_idx]) {
                let mut tmp = self.heap[parent_idx];
                (self.index_setter)(&mut tmp, child_idx);
                (self.index_setter)(&mut self.heap[child_idx], parent_idx);
                self.heap[parent_idx] = self.heap[child_idx];
                self.heap[child_idx] = tmp;
            }
            // Shift up.
            child_idx = parent_idx;
        }
    }

    /// `update_heap_down(top, bottom)`, through [`SkipHeapAddressing::child_of`]; a leaf's second
    /// child is a block further on, an inner slot's the next slot.
    fn update_heap_down(&mut self, top: usize, bottom: usize) {
        let mut parent_idx = top;
        loop {
            let mut child_idx = SkipHeapAddressing::<B>::child_of(parent_idx);
            if child_idx > bottom {
                break;
            }
            let child2_idx = child_idx
                + if SkipHeapAddressing::<B>::is_block_leaf(parent_idx) {
                    B
                } else {
                    1
                };
            if child2_idx <= bottom
                && !(self.less_predicate)(&self.heap[child_idx], &self.heap[child2_idx])
            {
                child_idx = child2_idx;
            }
            if (self.less_predicate)(&self.heap[parent_idx], &self.heap[child_idx]) {
                return;
            }
            // Switch nodes.
            let mut tmp = self.heap[parent_idx];
            (self.index_setter)(&mut tmp, child_idx);
            (self.index_setter)(&mut self.heap[child_idx], parent_idx);
            self.heap[parent_idx] = self.heap[child_idx];
            self.heap[child_idx] = tmp;
            // Shift down.
            parent_idx = child_idx;
        }
    }
}

impl<T, S, L, const B: usize, const RESET: bool> Drop
    for MutableSkipHeapPriorityQueue<T, S, L, B, RESET>
where
    T: Copy + Default,
    S: FnMut(&mut T, usize),
    L: FnMut(&T, &T) -> bool,
{
    /// `~MutableSkipHeapPriorityQueue()`: `clear()`.
    fn drop(&mut self) {
        self.clear();
    }
}
