// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Microsoft STL code: the growth, reserve, insert, erase, assignment and reverse of
// std::vector and std::reverse in MSVC 14.42's <vector>, <xutility> and <xmemory> (the element
// moves they make, not their storage), translated on 2026-11-13 (orca-o3-layer-1). Copyright
// Microsoft Corporation, under the Apache License, version 2.0, with the LLVM exception
// (Apache-2.0 WITH LLVM-exception; licenses/).

//! What MSVC's `std::vector` does to an extrusion path's `inset_idx` when it moves the path.
//!
//! `ExtrusionEntity::inset_idx` starts at -1, and the copy and move constructors of
//! `ExtrusionPath` (and of its subclasses, which call them) do not copy it, while its copy and
//! move assignments leave the target's. So a path keeps its `inset_idx` only where it stays in
//! place: a vector that reallocates copies every path into new storage (the move constructor is
//! not `noexcept`), and they all come out -1; `erase` and `insert` shift paths by assignment, so
//! the indices stay with the slots while the paths move; `std::reverse` swaps by a move
//! construction and two assignments, so the indices stay with the slots too.
//!
//! These functions make those moves on a Rust `Vec` with the C++'s indices: [`push`] and
//! [`insert`] grow the capacity as MSVC does (`_Calculate_growth`: half again, or the size
//! needed), [`reserve`] to exactly what is asked, and Rust's own `clone` of a vector allocates
//! exactly its length, as MSVC's copy does; a vector the code builds only through these keeps the
//! C++'s capacity, so a reallocation happens where it happens in the engine. Domain A sets no
//! path's `inset_idx` (the classic perimeter generator sets a loop's), so every path index is -1
//! there and these only keep the record exact.

/// An element whose `inset_idx` the vector's moves change: `ExtrusionPath` and its subclasses.
pub trait InsetSlot {
    /// `inset_idx`, to read or set.
    fn inset_slot(&mut self) -> &mut i32;
    /// The copy assignment (`operator=`), which leaves this element's `inset_idx`.
    fn assign_keeping_inset(&mut self, rhs: &Self);
}

/// `_Calculate_growth(new_size)`: the capacity half again larger, or `new_size` when that is not
/// enough.
fn growth(capacity: usize, new_size: usize) -> usize {
    let geometric = capacity + capacity / 2;
    if geometric < new_size {
        new_size
    } else {
        geometric
    }
}

/// A reallocation to `capacity`: every element copied into new storage, so every `inset_idx` -1.
fn reallocate<T: InsetSlot>(v: &mut Vec<T>, capacity: usize) {
    for e in v.iter_mut() {
        *e.inset_slot() = -1;
    }
    let len = v.len();
    v.reserve_exact(capacity - len);
}

/// The indices of the slots.
fn insets<T: InsetSlot>(v: &mut [T]) -> Vec<i32> {
    v.iter_mut().map(|e| *e.inset_slot()).collect()
}

/// Each slot given its index back, from the first.
fn restore<T: InsetSlot>(v: &mut [T], insets: &[i32]) {
    for (e, &i) in v.iter_mut().zip(insets) {
        *e.inset_slot() = i;
    }
}

/// `push_back` and `emplace_back`: a reallocation first when the vector is full; the new element
/// is made by its copy or move constructor (or another constructor), so its index is -1.
pub fn push<T: InsetSlot>(v: &mut Vec<T>, mut x: T) {
    if v.len() == v.capacity() {
        let capacity = growth(v.capacity(), v.len() + 1);
        reallocate(v, capacity);
    }
    *x.inset_slot() = -1;
    v.push(x);
}

/// `reserve(n)`: a reallocation to exactly `n` when the capacity is less.
pub fn reserve<T: InsetSlot>(v: &mut Vec<T>, n: usize) {
    if n > v.capacity() {
        reallocate(v, n);
    }
}

/// `insert(begin() + i, x)`: in a full vector a reallocation (every index -1, `x` copied in with
/// -1); at the end a push (-1); else the elements from `i` shift up by assignment, the last into
/// a new slot made by a move construction (index -1), and `x` is assigned into slot `i` (which
/// keeps its index).
pub fn insert<T: InsetSlot>(v: &mut Vec<T>, i: usize, mut x: T) {
    if v.len() == v.capacity() {
        let capacity = growth(v.capacity(), v.len() + 1);
        reallocate(v, capacity);
        *x.inset_slot() = -1;
        v.insert(i, x);
    } else if i == v.len() {
        *x.inset_slot() = -1;
        v.push(x);
    } else {
        let old = insets(v);
        v.insert(i, x);
        restore(v, &old);
        let last = v.len() - 1;
        *v[last].inset_slot() = -1;
    }
}

/// `insert(end(), first, last)` of a range: a reallocation first (to the growth of the size
/// needed) when the unused capacity is too small; each element copied in (index -1).
pub fn extend_from_slice<T: InsetSlot + Clone>(v: &mut Vec<T>, src: &[T]) {
    if src.len() > v.capacity() - v.len() {
        let capacity = growth(v.capacity(), v.len() + src.len());
        reallocate(v, capacity);
    }
    for x in src {
        let mut copy = x.clone();
        *copy.inset_slot() = -1;
        v.push(copy);
    }
}

/// `erase(begin() + i)`: the elements after `i` shift down by assignment, so the slots keep their
/// indices; the last slot goes.
pub fn erase<T: InsetSlot>(v: &mut Vec<T>, i: usize) {
    let old = insets(v);
    v.remove(i);
    restore(v, &old);
}

/// `std::reverse` of the elements: swapped by a move construction and two assignments, so the
/// slots keep their indices.
pub fn reverse<T: InsetSlot>(v: &mut [T]) {
    let old = insets(v);
    v.reverse();
    restore(v, &old);
}

/// `dst = src` (`operator=(const vector&)`): into a vector too small, every element copied anew
/// (index -1, capacity grown as `_Clear_and_reserve_geometric`); else the first elements assigned
/// (keeping their indices), the rest copied (-1) or dropped.
pub fn assign<T: InsetSlot + Clone>(dst: &mut Vec<T>, src: &[T]) {
    if src.len() > dst.capacity() {
        let capacity = growth(dst.capacity(), src.len());
        *dst = Vec::with_capacity(capacity);
        dst.extend(src.iter().cloned());
        return;
    }
    let common = dst.len().min(src.len());
    for (d, s) in dst.iter_mut().zip(&src[..common]) {
        d.assign_keeping_inset(s);
    }
    if src.len() > dst.len() {
        dst.extend(src[common..].iter().cloned());
    } else {
        dst.truncate(src.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct E {
        v: i32,
        inset: i32,
    }

    impl InsetSlot for E {
        fn inset_slot(&mut self) -> &mut i32 {
            &mut self.inset
        }
        fn assign_keeping_inset(&mut self, rhs: &Self) {
            self.v = rhs.v;
        }
    }

    fn e(v: i32, inset: i32) -> E {
        E { v, inset }
    }

    #[test]
    fn growth_is_half_again() {
        let mut v: Vec<E> = Vec::new();
        let mut caps = Vec::new();
        for i in 0..7 {
            push(&mut v, e(i, i));
            // The pushed copy comes in with -1; the index is set after, as the engine does.
            assert_eq!(v[v.len() - 1].inset, -1);
            let last = v.len() - 1;
            v[last].inset = i;
            caps.push(v.capacity());
        }
        assert_eq!(caps, vec![1, 2, 3, 4, 6, 6, 9]);
        // The reallocation at the seventh push reset the six before it.
        assert_eq!(
            v.iter().map(|x| x.inset).collect::<Vec<_>>(),
            vec![-1, -1, -1, -1, -1, -1, 6]
        );
    }

    #[test]
    fn reverse_keeps_the_slots_indices() {
        let mut v = vec![e(1, 10), e(2, 20), e(3, 30)];
        reverse(&mut v);
        assert_eq!(v, vec![e(3, 10), e(2, 20), e(1, 30)]);
    }

    #[test]
    fn insert_with_room_shifts_by_assignment() {
        let mut v = Vec::with_capacity(4);
        v.extend([e(1, 10), e(2, 20), e(3, 30)]);
        insert(&mut v, 1, e(9, -1));
        assert_eq!(v, vec![e(1, 10), e(9, 20), e(2, 30), e(3, -1)]);
    }

    #[test]
    fn erase_keeps_the_slots_indices() {
        let mut v = vec![e(1, 10), e(2, 20), e(3, 30)];
        erase(&mut v, 0);
        assert_eq!(v, vec![e(2, 10), e(3, 20)]);
    }
}
