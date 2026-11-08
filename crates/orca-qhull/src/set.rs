// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Qhull's sets (`qset_r.c`, `qset_r.h`), as the hull's code uses them.
//!
//! Carries Qhull code: the set operations of Qhull 2015.2's `qset_r.c` and the iteration macros
//! of `qset_r.h`, translated into Rust on 2026-11-07, Copyright (c) 1993-2015 C.B. Barber and The
//! Geometry Center, University of Minnesota, under Qhull's licence (`licenses/Qhull.txt`); the
//! copyright holders are named in `NOTICE`.
//!
//! A Qhull set is a counted array of pointers ended by a NULL, or a NULL pointer for no set. Here
//! it is an `Option<Vec<u32>>` of arena indices: `None` is the NULL set, and [`NIL`] is a NULL
//! element, which Qhull stores inside a set on purpose (`SETfirst_(set)= NULL` ends a `FOREACH`
//! early while the size still counts it). So the operations keep C's two notions of length:
//!
//! - the size, `qh_setsize`, is the vector's length, NULL elements included;
//! - `FOREACH` stops at the first NULL element ([`elem`] gives [`NIL`] past the end, C's
//!   terminating NULL), and so do the searches that walk to the terminator (`qh_setin`,
//!   `qh_setdel`, `qh_setreplace`).
//!
//! The memory side of Qhull's sets (`qh_setlarger`, the temporary-set stack, `qh_setfree`'s free
//! lists) has no effect on a result and is left out: a temporary set is a local vector.

/// A NULL pointer, as an element of a set or a link of a list.
pub(crate) const NIL: u32 = u32::MAX;

/// A Qhull set: `None` is a NULL `setT *`.
pub(crate) type Set = Option<Vec<u32>>;

/// `qh_setsize`: the number of elements, NULL elements included; 0 for no set.
#[inline]
pub(crate) fn size(set: &Set) -> usize {
    set.as_ref().map_or(0, Vec::len)
}

/// `SETelem_(set, i)` as `FOREACH` reads it: the element, or C's terminating NULL at or past the
/// end (and for no set, where `FOREACH` does not start).
#[inline]
pub(crate) fn elem(set: &Set, i: usize) -> u32 {
    match set {
        Some(v) => v.get(i).copied().unwrap_or(NIL),
        None => NIL,
    }
}

/// `SETfirst_(set)`: the first element, or NULL.
#[inline]
pub(crate) fn first(set: &Set) -> u32 {
    elem(set, 0)
}

/// `SETsecond_(set)`: the second element, or NULL.
#[inline]
pub(crate) fn second(set: &Set) -> u32 {
    elem(set, 1)
}

/// `SETelem_(set, i)= value` for an element inside the set.
///
/// # Panics
///
/// Past the end, where C would write into the set's spare room (the callers never do).
#[inline]
pub(crate) fn set_elem(set: &mut Set, i: usize, value: u32) {
    let v = set.as_mut().expect("SETelem_ of a NULL set");
    v[i] = value;
}

/// `qh_setappend`: appends a non-NULL element, making the set if there is none; a NULL element is
/// ignored (and no set made).
#[inline]
pub(crate) fn append(set: &mut Set, value: u32) {
    if value == NIL {
        return;
    }
    set.get_or_insert_with(Vec::new).push(value);
}

/// `qh_setappend2ndlast`: puts the element before the last one.
///
/// # Panics
///
/// For an empty set, where the C reads and writes before the array (its callers never pass one).
pub(crate) fn append_2nd_last(set: &mut Set, value: u32) {
    let v = set.get_or_insert_with(Vec::new);
    assert!(!v.is_empty(), "qh_setappend2ndlast of an empty set");
    let at = v.len() - 1;
    v.insert(at, value);
}

/// `qh_setappend_set`: appends every element of `from` (NULLs included); a NULL `from` changes
/// nothing, and an empty one still makes `set` if there is none.
pub(crate) fn append_set(set: &mut Set, from: &Set) {
    let Some(from) = from else {
        return;
    };
    set.get_or_insert_with(Vec::new).extend_from_slice(from);
}

/// `qh_setaddnth`: inserts the element at `nth`, making the set if there is none.
///
/// # Panics
///
/// For `nth` past the end (Qhull's error 6171, which no caller reaches).
pub(crate) fn add_nth(set: &mut Set, nth: usize, value: u32) {
    let v = set.get_or_insert_with(Vec::new);
    assert!(nth <= v.len(), "qh_setaddnth past the end");
    v.insert(nth, value);
}

/// `qh_setin`: whether the element is in the set before its first NULL.
pub(crate) fn contains(set: &Set, value: u32) -> bool {
    match set {
        Some(v) => v.iter().take_while(|&&e| e != NIL).any(|&e| e == value),
        None => false,
    }
}

/// `qh_setindex`: the index of the element among all `size` elements, or -1.
pub(crate) fn index(set: &Set, value: u32) -> i32 {
    match set {
        Some(v) => v
            .iter()
            .position(|&e| e == value)
            .map_or(-1, |i| i32::try_from(i).expect("a set fits an int")),
        None => -1,
    }
}

/// `qh_setlast`: the last element (NULL if it is one), or NULL for an empty or no set.
pub(crate) fn last(set: &Set) -> u32 {
    match set {
        Some(v) => v.last().copied().unwrap_or(NIL),
        None => NIL,
    }
}

/// `qh_setdel`: removes the first occurrence before the first NULL, moving the last element into
/// its place; returns whether it was there.
pub(crate) fn del(set: &mut Set, value: u32) -> bool {
    let Some(v) = set else {
        return false;
    };
    for i in 0..v.len() {
        let e = v[i];
        if e == NIL {
            return false;
        }
        if e == value {
            v.swap_remove(i);
            return true;
        }
    }
    false
}

/// `qh_setdelnth`: removes the element at `nth`, moving the last element into its place.
///
/// # Panics
///
/// For `nth` past the end (Qhull's error 6174).
pub(crate) fn del_nth(set: &mut Set, nth: usize) -> u32 {
    let v = set.as_mut().expect("qh_setdelnth of a NULL set");
    v.swap_remove(nth)
}

/// The shift of `qh_setdelnthsorted` and `qh_setdelsorted` from `at`: each later element moves
/// down one, up to and including the first NULL (C copies until it has copied one), and the size
/// drops by one.
fn shift_down(v: &mut Vec<u32>, at: usize) {
    let n = v.len();
    let mut k = at;
    loop {
        let next = if k + 1 < n { v[k + 1] } else { NIL };
        v[k] = next;
        k += 1;
        if next == NIL {
            break;
        }
    }
    v.truncate(n - 1);
}

/// `qh_setdelnthsorted`: removes the element at `nth`, keeping the order; returns it.
///
/// # Panics
///
/// For `nth` past the end (Qhull's error 6174).
pub(crate) fn del_nth_sorted(set: &mut Set, nth: usize) -> u32 {
    let v = set.as_mut().expect("qh_setdelnthsorted of a NULL set");
    assert!(nth < v.len(), "qh_setdelnthsorted past the end");
    let e = v[nth];
    shift_down(v, nth);
    e
}

/// `qh_setdelsorted`: removes the first occurrence before the first NULL, keeping the order;
/// returns whether it was there.
pub(crate) fn del_sorted(set: &mut Set, value: u32) -> bool {
    let Some(v) = set else {
        return false;
    };
    let Some(at) = v
        .iter()
        .take_while(|&&e| e != NIL)
        .position(|&e| e == value)
    else {
        return false;
    };
    shift_down(v, at);
    true
}

/// `qh_setdellast`: removes and returns the last element; NULL for no set, an empty set or a set
/// whose first element is NULL.
pub(crate) fn del_last(set: &mut Set) -> u32 {
    match set {
        Some(v) if v.first().is_some_and(|&e| e != NIL) => v.pop().unwrap_or(NIL),
        _ => NIL,
    }
}

/// `qh_setcompact`: removes the NULL elements.
pub(crate) fn compact(set: &mut Set) {
    if let Some(v) = set {
        v.retain(|&e| e != NIL);
    }
}

/// `qh_settruncate`: keeps the first `n` elements.
///
/// # Panics
///
/// For `n` past the end, where C would take in the set's spare room (the callers never do).
pub(crate) fn truncate(set: &mut Set, n: usize) {
    let v = set.as_mut().expect("qh_settruncate of a NULL set");
    assert!(n <= v.len(), "qh_settruncate past the end");
    v.truncate(n);
}

/// `qh_setreplace`: replaces the first occurrence before the first NULL.
///
/// # Errors
///
/// Qhull's error 6177 (an internal error) when the element is not there.
pub(crate) fn replace(set: &mut Set, old: u32, new: u32) -> Result<(), u32> {
    if let Some(v) = set {
        for e in v.iter_mut() {
            if *e == NIL {
                break;
            }
            if *e == old {
                *e = new;
                return Ok(());
            }
        }
    }
    Err(6177)
}

/// `qh_setequal`: the same elements in the same order (NULL and empty sets are equal).
pub(crate) fn equal(a: &Set, b: &Set) -> bool {
    let a = a.as_deref().unwrap_or(&[]);
    let b = b.as_deref().unwrap_or(&[]);
    a == b
}

/// `qh_setequal_except`: equal once `skip_a` is left out of `a` and one element of `b`, which
/// `skip_b` names or, if it is NULL, the first that differs, is left out of `b`.
pub(crate) fn equal_except(a: &Set, skip_a: u32, b: &Set, mut skip_b: u32) -> bool {
    let (mut ia, mut ib) = (0usize, 0usize);
    let mut skip = 0;
    loop {
        if elem(a, ia) == skip_a {
            skip += 1;
            ia += 1;
        }
        if skip_b != NIL {
            if elem(b, ib) == skip_b {
                skip += 1;
                ib += 1;
            }
        } else if elem(a, ia) != elem(b, ib) {
            skip += 1;
            skip_b = elem(b, ib);
            ib += 1;
            if skip_b == NIL {
                return false;
            }
        }
        if elem(a, ia) == NIL {
            break;
        }
        let (ea, eb) = (elem(a, ia), elem(b, ib));
        ia += 1;
        ib += 1;
        if ea != eb {
            return false;
        }
    }
    skip == 2 && elem(b, ib) == NIL
}

/// `qh_setnew_delnthsorted(set, size, nth, prepend)`: a new set of the first `size` elements of
/// `set` but the one at `nth`, after `prepend` NULL elements (`qh_setnew` clears only the first
/// slot, and Qhull prepends at most one).
pub(crate) fn new_del_nth_sorted(set: &Set, size: usize, nth: usize, prepend: usize) -> Set {
    assert!(
        prepend <= 1,
        "qh_setnew_delnthsorted prepends at most one element"
    );
    assert!(nth < size, "qh_setnew_delnthsorted past the end");
    let mut v = Vec::with_capacity(size - 1 + prepend);
    v.extend(core::iter::repeat_n(NIL, prepend));
    v.extend((0..nth).map(|i| elem(set, i)));
    v.extend((nth + 1..size).map(|i| elem(set, i)));
    Some(v)
}

/// `qh_setunique`: appends the element unless it is in the set; returns whether it did.
pub(crate) fn unique(set: &mut Set, value: u32) -> bool {
    if contains(set, value) {
        false
    } else {
        append(set, value);
        true
    }
}

/// `qh_setnew` followed by `qh_setzero(set, 0, n)`: `n` NULL elements.
pub(crate) fn zeroed(n: usize) -> Set {
    Some(vec![NIL; n])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreach_stops_at_a_null_element_while_the_size_counts_it() {
        let s: Set = Some(vec![4, NIL, 5]);
        assert_eq!(size(&s), 3);
        assert!(!contains(&s, 5));
        assert_eq!(index(&s, 5), 2);
        assert_eq!(elem(&s, 3), NIL);
    }

    #[test]
    fn deletions_follow_qset_r() {
        let mut s: Set = Some(vec![1, 2, 3, 4]);
        assert!(del(&mut s, 2));
        assert_eq!(s, Some(vec![1, 4, 3]));
        let mut s: Set = Some(vec![1, 2, 3, 4]);
        assert!(del_sorted(&mut s, 2));
        assert_eq!(s, Some(vec![1, 3, 4]));
        let mut s: Set = Some(vec![1, 2, NIL, 4]);
        assert_eq!(del_nth_sorted(&mut s, 0), 1);
        assert_eq!(s, Some(vec![2, NIL, NIL]));
        let mut s: Set = Some(vec![NIL, 2]);
        assert_eq!(del_last(&mut s), NIL);
        assert_eq!(s, Some(vec![NIL, 2]));
    }

    #[test]
    fn equal_except_skips_one_element_of_each() {
        let a: Set = Some(vec![9, 7, 5]);
        let b: Set = Some(vec![9, 6, 5]);
        assert!(equal_except(&a, 7, &b, NIL));
        assert!(equal_except(&a, 7, &b, 6));
        assert!(!equal_except(&a, 9, &b, NIL));
    }

    #[test]
    fn new_del_nth_sorted_prepends_a_null() {
        let s: Set = Some(vec![3, 2, 1, 0]);
        assert_eq!(new_del_nth_sorted(&s, 4, 1, 0), Some(vec![3, 1, 0]));
        assert_eq!(new_del_nth_sorted(&s, 3, 2, 1), Some(vec![NIL, 3, 2]));
    }
}
