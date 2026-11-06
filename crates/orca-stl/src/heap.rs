// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Microsoft STL code: this file is translated from stl/inc/__msvc_heap_algorithms.hpp
// (_Push_heap_by_index, push_heap, _Pop_heap_hole_by_index, _Pop_heap_hole_unchecked,
// _Pop_heap_unchecked, pop_heap, _Make_heap_unchecked, make_heap), stl/inc/algorithm
// (_Is_heap_until_unchecked, is_heap_until, is_heap, _Sort_heap_unchecked, sort_heap) and
// stl/inc/queue (priority_queue's push, pop, top and its constructors) of microsoft/STL at the tag
// vs-2022-17.12 (commit 91e425587a0b9b5ac0d1005844c661fee2e2813b), the heaps of MSVC 14.42, step
// for step, on 2026-11-06. Its copyright holder is Microsoft Corporation, and it is under
// Apache-2.0 WITH LLVM-exception (licenses/Apache-2.0.txt and licenses/LLVM-exception.txt).

//! The heap functions and `std::priority_queue`, as MSVC's STL moves holes through them.
//!
//! A heap here is C++'s: a max-heap under the comparator `less(a, b)`, "a goes before b", whose
//! first element is the one no other goes after. [`push_heap_by`], [`pop_heap_by`],
//! [`make_heap_by`] and [`sort_heap_by`] compare the same pairs in the same order as MSVC's,
//! so ties come out of a heap in MSVC's order. [`PriorityQueue`] is `std::priority_queue` over a
//! `Vec`: `push` is `push_back` and `push_heap`, `pop` is `pop_heap` and `pop_back`.
//!
//! `std::collections::BinaryHeap` pops ties in another order; `clippy.toml` sends the engine here.

use std::mem::ManuallyDrop;
use std::ptr;

use crate::sort::Hole;

/// `_Push_heap_by_index`: moves the hole at `hole` up towards `top` while its parent goes before
/// the held value, then (when `guard` drops) drops the value into it.
///
/// # Safety
/// `base` points at a slice of more than `hole` elements whose slot `hole` is the guard's hole.
unsafe fn push_heap_by_index<T, F: FnMut(&T, &T) -> bool>(
    base: *mut T,
    guard: &mut Hole<T>,
    mut hole: usize,
    top: usize,
    less: &mut F,
) {
    // SAFETY: every index read is a parent of the hole, inside the slice and not the hole.
    unsafe {
        while top < hole {
            let idx = (hole - 1) >> 1;
            if !less(&*base.add(idx), &guard.value) {
                break;
            }
            ptr::copy_nonoverlapping(base.add(idx), base.add(hole), 1);
            hole = idx;
            guard.dest = base.add(hole);
        }
    }
}

/// `_Pop_heap_hole_by_index`: moves the hole at `hole` down to the larger child until it is a
/// leaf of the heap `[0, bottom)`, then pushes the held value up from there.
///
/// # Safety
/// `base` points at a slice of at least `bottom >= 1` elements whose slot `hole` is the guard's
/// hole.
unsafe fn pop_heap_hole_by_index<T, F: FnMut(&T, &T) -> bool>(
    base: *mut T,
    guard: &mut Hole<T>,
    mut hole: usize,
    bottom: usize,
    less: &mut F,
) {
    let top = hole;
    let mut idx = hole;
    let max_sequence_non_leaf = (bottom - 1) >> 1;
    // SAFETY: the children read are below `bottom` and after the hole.
    unsafe {
        while idx < max_sequence_non_leaf {
            idx = 2 * idx + 2;
            if less(&*base.add(idx), &*base.add(idx - 1)) {
                idx -= 1;
            }
            ptr::copy_nonoverlapping(base.add(idx), base.add(hole), 1);
            hole = idx;
            guard.dest = base.add(hole);
        }
        if idx == max_sequence_non_leaf && bottom.is_multiple_of(2) {
            ptr::copy_nonoverlapping(base.add(bottom - 1), base.add(hole), 1);
            hole = bottom - 1;
            guard.dest = base.add(hole);
        }
        push_heap_by_index(base, guard, hole, top, less);
    }
}

/// `_Pop_heap_hole_unchecked`: moves the top of the heap `[0, bottom)` to `dest` (outside the
/// heap, or its last place) and re-heaps with `value`, which was taken from `dest`.
///
/// # Safety
/// `bottom >= 1`; `dest` is `bottom` or after it, inside the slice at `base`.
pub(crate) unsafe fn pop_heap_hole<T, F: FnMut(&T, &T) -> bool>(
    base: *mut T,
    bottom: usize,
    dest: usize,
    less: &mut F,
) {
    // SAFETY: the value at `dest` is held by the guard while the top moves into its slot, and the
    // top's slot becomes the hole.
    unsafe {
        let mut guard = Hole {
            value: ManuallyDrop::new(ptr::read(base.add(dest))),
            dest: base.add(dest),
        };
        ptr::copy_nonoverlapping(base, base.add(dest), 1);
        guard.dest = base;
        pop_heap_hole_by_index(base, &mut guard, 0, bottom, less);
    }
}

/// `push_heap`: the last element joins the heap formed by the ones before it.
pub(crate) fn push_heap_unchecked<T, F: FnMut(&T, &T) -> bool>(v: &mut [T], less: &mut F) {
    let count = v.len();
    if 2 <= count {
        let base = v.as_mut_ptr();
        // SAFETY: the last element is held by the guard while its slot is the hole.
        unsafe {
            let mut guard = Hole {
                value: ManuallyDrop::new(ptr::read(base.add(count - 1))),
                dest: base.add(count - 1),
            };
            push_heap_by_index(base, &mut guard, count - 1, 0, less);
        }
    }
}

/// `_Pop_heap_unchecked`: moves the top to the last place and re-heaps the rest.
pub(crate) fn pop_heap_unchecked<T, F: FnMut(&T, &T) -> bool>(v: &mut [T], less: &mut F) {
    let count = v.len();
    if 2 <= count {
        // SAFETY: bottom = count - 1 >= 1 and dest = count - 1 is the last place.
        unsafe { pop_heap_hole(v.as_mut_ptr(), count - 1, count - 1, less) }
    }
}

/// `_Make_heap_unchecked`: from the last parent down to the root, each value is popped into
/// place.
pub(crate) fn make_heap_unchecked<T, F: FnMut(&T, &T) -> bool>(v: &mut [T], less: &mut F) {
    let bottom = v.len();
    let base = v.as_mut_ptr();
    let mut hole = bottom >> 1;
    while hole > 0 {
        hole -= 1;
        // SAFETY: hole < bottom / 2 < bottom; its value is held by the guard.
        unsafe {
            let mut guard = Hole {
                value: ManuallyDrop::new(ptr::read(base.add(hole))),
                dest: base.add(hole),
            };
            pop_heap_hole_by_index(base, &mut guard, hole, bottom, less);
        }
    }
}

/// `_Sort_heap_unchecked`: pops the heap until one element is left.
pub(crate) fn sort_heap_unchecked<T, F: FnMut(&T, &T) -> bool>(v: &mut [T], less: &mut F) {
    let mut last = v.len();
    while last >= 2 {
        pop_heap_unchecked(&mut v[..last], less);
        last -= 1;
    }
}

/// `_Is_heap_until_unchecked`: the length of the longest prefix that is a heap.
pub(crate) fn is_heap_until_unchecked<T, F: FnMut(&T, &T) -> bool>(v: &[T], less: &mut F) -> usize {
    let size = v.len();
    for off in 1..size {
        if less(&v[(off - 1) >> 1], &v[off]) {
            return off;
        }
    }
    size
}

/// `std::push_heap(first, last)` with `std::less<>`.
pub fn push_heap<T: PartialOrd>(v: &mut [T]) {
    push_heap_unchecked(v, &mut |a: &T, b: &T| a < b);
}

/// `std::push_heap(first, last, pred)`.
pub fn push_heap_by<T, F: FnMut(&T, &T) -> bool>(v: &mut [T], mut less: F) {
    push_heap_unchecked(v, &mut less);
}

/// `std::pop_heap(first, last)` with `std::less<>`.
pub fn pop_heap<T: PartialOrd>(v: &mut [T]) {
    pop_heap_unchecked(v, &mut |a: &T, b: &T| a < b);
}

/// `std::pop_heap(first, last, pred)`.
pub fn pop_heap_by<T, F: FnMut(&T, &T) -> bool>(v: &mut [T], mut less: F) {
    pop_heap_unchecked(v, &mut less);
}

/// `std::make_heap(first, last)` with `std::less<>`.
pub fn make_heap<T: PartialOrd>(v: &mut [T]) {
    make_heap_unchecked(v, &mut |a: &T, b: &T| a < b);
}

/// `std::make_heap(first, last, pred)`.
pub fn make_heap_by<T, F: FnMut(&T, &T) -> bool>(v: &mut [T], mut less: F) {
    make_heap_unchecked(v, &mut less);
}

/// `std::sort_heap(first, last)` with `std::less<>`.
pub fn sort_heap<T: PartialOrd>(v: &mut [T]) {
    sort_heap_unchecked(v, &mut |a: &T, b: &T| a < b);
}

/// `std::sort_heap(first, last, pred)`.
pub fn sort_heap_by<T, F: FnMut(&T, &T) -> bool>(v: &mut [T], mut less: F) {
    sort_heap_unchecked(v, &mut less);
}

/// `std::is_heap_until(first, last)` with `std::less<>`, as an index.
pub fn is_heap_until<T: PartialOrd>(v: &[T]) -> usize {
    is_heap_until_unchecked(v, &mut |a: &T, b: &T| a < b)
}

/// `std::is_heap_until(first, last, pred)`, as an index.
pub fn is_heap_until_by<T, F: FnMut(&T, &T) -> bool>(v: &[T], mut less: F) -> usize {
    is_heap_until_unchecked(v, &mut less)
}

/// `std::is_heap(first, last)` with `std::less<>`.
pub fn is_heap<T: PartialOrd>(v: &[T]) -> bool {
    is_heap_until(v) == v.len()
}

/// `std::is_heap(first, last, pred)`.
pub fn is_heap_by<T, F: FnMut(&T, &T) -> bool>(v: &[T], less: F) -> bool {
    is_heap_until_by(v, less) == v.len()
}

/// `std::less<>` as a function pointer: [`PriorityQueue`]'s default comparator.
pub fn less<T: PartialOrd>(a: &T, b: &T) -> bool {
    a < b
}

/// `std::priority_queue<T, std::vector<T>, Compare>`: a max-heap under `less` over a `Vec`.
///
/// The default comparator is [`less`] (`std::less<T>`), so [`PriorityQueue::top`] is a greatest
/// element; `|a, b| a > b` (`std::greater<T>`) gives a min-queue. Among equivalent elements,
/// `top` and `pop` follow MSVC's heap moves, not insertion order.
#[derive(Clone, Debug)]
pub struct PriorityQueue<T, F = fn(&T, &T) -> bool> {
    c: Vec<T>,
    comp: F,
}

impl<T: PartialOrd> PriorityQueue<T> {
    /// `priority_queue()`: empty, ordered by `std::less<T>`.
    pub fn new() -> Self {
        Self {
            c: Vec::new(),
            comp: less::<T>,
        }
    }

    /// `priority_queue(first, last)`: the elements in order, then `make_heap`.
    pub fn from_vec(c: Vec<T>) -> Self {
        Self::from_vec_with(c, less::<T>)
    }
}

impl<T: PartialOrd> Default for PriorityQueue<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, F: FnMut(&T, &T) -> bool> PriorityQueue<T, F> {
    /// `priority_queue(pred)`: empty, ordered by `comp`.
    pub fn with_compare(comp: F) -> Self {
        Self {
            c: Vec::new(),
            comp,
        }
    }

    /// `priority_queue(pred, std::move(container))` and `priority_queue(first, last, pred)`: the
    /// container as it is, then `make_heap`.
    pub fn from_vec_with(c: Vec<T>, mut comp: F) -> Self {
        let mut c = c;
        make_heap_unchecked(&mut c, &mut comp);
        Self { c, comp }
    }

    /// `push` (and `emplace`): `push_back`, then `push_heap`.
    pub fn push(&mut self, value: T) {
        self.c.push(value);
        push_heap_unchecked(&mut self.c, &mut self.comp);
    }

    /// `pop`: `pop_heap`, then `pop_back`; returns the element that was on top.
    pub fn pop(&mut self) -> Option<T> {
        pop_heap_unchecked(&mut self.c, &mut self.comp);
        self.c.pop()
    }

    /// `top`: the first element of the heap.
    pub fn top(&self) -> Option<&T> {
        self.c.first()
    }

    /// `size`.
    pub fn len(&self) -> usize {
        self.c.len()
    }

    /// `empty`.
    pub fn is_empty(&self) -> bool {
        self.c.is_empty()
    }

    /// The underlying container in heap order (the protected member `c`).
    pub fn as_slice(&self) -> &[T] {
        &self.c
    }

    /// The underlying container, in heap order.
    pub fn into_vec(self) -> Vec<T> {
        self.c
    }
}
