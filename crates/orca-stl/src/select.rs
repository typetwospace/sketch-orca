// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Microsoft STL code: this file is translated from stl/inc/algorithm (nth_element,
// partial_sort, upper_bound, equal_range, binary_search and unique) and stl/inc/xutility
// (lower_bound) of microsoft/STL at the tag vs-2022-17.12 (commit
// 91e425587a0b9b5ac0d1005844c661fee2e2813b), the selections and searches of MSVC 14.42, comparison
// for comparison, on 2026-11-06. Its copyright holder is Microsoft Corporation, and it is under
// Apache-2.0 WITH LLVM-exception (licenses/Apache-2.0.txt and licenses/LLVM-exception.txt).

//! Selections and searches on slices: `nth_element`, `partial_sort`, `lower_bound`,
//! `upper_bound`, `equal_range`, `binary_search` and `unique`, as MSVC's STL runs them.
//!
//! Positions are indices into the slice where C++ returns iterators. The comparators are C++'s:
//! `less(a, b)` is "a goes before b", and `unique`'s `eq(a, b)` is "b repeats a".

use crate::heap;
use crate::sort::{ISORT_MAX, insertion_sort, partition_by_median_guess};

/// `std::nth_element(first, nth, last)` with `std::less<>`.
pub fn nth_element<T: PartialOrd>(v: &mut [T], nth: usize) {
    nth_element_by(v, nth, |a, b| a < b);
}

/// `std::nth_element(first, nth, last, pred)`: partitions around guessed medians until the part
/// holding `nth` has at most [`ISORT_MAX`] elements (or `nth` falls among the pivot's equals),
/// then insertion-sorts that part. `nth == v.len()` does nothing, as `nth == last` does.
///
/// # Panics
/// If `nth > v.len()` (C++'s precondition).
pub fn nth_element_by<T, F: FnMut(&T, &T) -> bool>(v: &mut [T], nth: usize, mut less: F) {
    assert!(nth <= v.len(), "nth_element: nth is past the end");
    let mut first = 0usize;
    let mut last = v.len();
    if nth == last {
        return;
    }
    while ISORT_MAX < last - first {
        let (mid_first, mid_last) = partition_by_median_guess(&mut v[first..last], &mut less);
        let (mid_first, mid_last) = (first + mid_first, first + mid_last);
        if mid_last <= nth {
            first = mid_last;
        } else if mid_first <= nth {
            return;
        } else {
            last = mid_first;
        }
    }
    insertion_sort(&mut v[first..last], &mut less);
}

/// `std::partial_sort(first, mid, last)` with `std::less<>`.
pub fn partial_sort<T: PartialOrd>(v: &mut [T], mid: usize) {
    partial_sort_by(v, mid, |a, b| a < b);
}

/// `std::partial_sort(first, mid, last, pred)`: `make_heap` on `[0, mid)`, each later element
/// that goes before the top replaces it through a pop, then `sort_heap`.
///
/// # Panics
/// If `mid > v.len()`.
pub fn partial_sort_by<T, F: FnMut(&T, &T) -> bool>(v: &mut [T], mid: usize, mut less: F) {
    assert!(mid <= v.len(), "partial_sort: mid is past the end");
    if mid == 0 {
        return;
    }
    heap::make_heap_unchecked(&mut v[..mid], &mut less);
    for next in mid..v.len() {
        if less(&v[next], &v[0]) {
            // SAFETY: the heap is [0, mid) with mid >= 1, and next >= mid is inside the slice.
            unsafe { heap::pop_heap_hole(v.as_mut_ptr(), mid, next, &mut less) }
        }
    }
    heap::sort_heap_unchecked(&mut v[..mid], &mut less);
}

/// `std::lower_bound(first, last, val)` with `std::less<>`: the first index whose element does
/// not go before `val`.
pub fn lower_bound<T: PartialOrd>(v: &[T], val: &T) -> usize {
    lower_bound_by(v, val, |e, x| e < x)
}

/// `std::lower_bound(first, last, val, pred)`, with `pred(element, val)`.
pub fn lower_bound_by<T, U: ?Sized, F: FnMut(&T, &U) -> bool>(
    v: &[T],
    val: &U,
    mut less: F,
) -> usize {
    let mut first = 0usize;
    let mut count = v.len();
    while 0 < count {
        let count2 = count / 2;
        let mid = first + count2;
        if less(&v[mid], val) {
            first = mid + 1;
            count -= count2 + 1;
        } else {
            count = count2;
        }
    }
    first
}

/// `std::upper_bound(first, last, val)` with `std::less<>`: the first index whose element `val`
/// goes before.
pub fn upper_bound<T: PartialOrd>(v: &[T], val: &T) -> usize {
    upper_bound_by(v, val, |x, e| x < e)
}

/// `std::upper_bound(first, last, val, pred)`, with `pred(val, element)`.
pub fn upper_bound_by<T, U: ?Sized, F: FnMut(&U, &T) -> bool>(
    v: &[T],
    val: &U,
    mut less: F,
) -> usize {
    let mut first = 0usize;
    let mut count = v.len();
    while 0 < count {
        let count2 = count / 2;
        let mid = first + count2;
        if less(val, &v[mid]) {
            count = count2;
        } else {
            first = mid + 1;
            count -= count2 + 1;
        }
    }
    first
}

/// `std::equal_range(first, last, val)` with `std::less<>`.
pub fn equal_range<T: PartialOrd>(v: &[T], val: &T) -> (usize, usize) {
    equal_range_by(v, val, |a, b| a < b)
}

/// `std::equal_range(first, last, val, pred)`: bisects until the middle is equivalent to `val`,
/// then a `lower_bound` before it and an `upper_bound` after it.
pub fn equal_range_by<T, F: FnMut(&T, &T) -> bool>(
    v: &[T],
    val: &T,
    mut less: F,
) -> (usize, usize) {
    let mut first = 0usize;
    let mut count = v.len();
    loop {
        if count == 0 {
            return (first, first);
        }
        let count2 = count / 2;
        let mid = first + count2;
        if less(&v[mid], val) {
            first = mid + 1;
            count -= count2 + 1;
        } else if less(val, &v[mid]) {
            count = count2;
        } else {
            let lo = first + lower_bound_by(&v[first..mid], val, &mut less);
            let end = first + count;
            let hi = mid + 1 + upper_bound_by(&v[mid + 1..end], val, &mut less);
            return (lo, hi);
        }
    }
}

/// `std::binary_search(first, last, val)` with `std::less<>`.
pub fn binary_search<T: PartialOrd>(v: &[T], val: &T) -> bool {
    binary_search_by(v, val, |a, b| a < b)
}

/// `std::binary_search(first, last, val, pred)`: `lower_bound`, then `!pred(val, *it)`.
pub fn binary_search_by<T, F: FnMut(&T, &T) -> bool>(v: &[T], val: &T, mut less: F) -> bool {
    let i = lower_bound_by(v, val, &mut less);
    i != v.len() && !less(val, &v[i])
}

/// `std::unique(first, last)` with `==`: see [`unique_by`].
pub fn unique<T: PartialEq>(v: &mut [T]) -> usize {
    unique_by(v, |a, b| a == b)
}

/// `std::unique(first, last, pred)`: returns the length of the kept prefix. `eq(kept, next)` is
/// called with the last kept element and the next one, in MSVC's order. The elements after the
/// prefix are the removed ones in an unspecified order (C++ leaves them moved-from).
pub fn unique_by<T, F: FnMut(&T, &T) -> bool>(v: &mut [T], mut eq: F) -> usize {
    let last = v.len();
    if last == 0 {
        return 0;
    }
    let mut firstb = 0usize;
    let mut first = 0usize;
    loop {
        first += 1;
        if first == last {
            return last;
        }
        if eq(&v[firstb], &v[first]) {
            loop {
                first += 1;
                if first == last {
                    return firstb + 1;
                }
                if !eq(&v[firstb], &v[first]) {
                    firstb += 1;
                    v.swap(firstb, first);
                }
            }
        }
        firstb = first;
    }
}

/// `v.erase(std::unique(v.begin(), v.end()), v.end())`.
pub fn unique_vec<T: PartialEq>(v: &mut Vec<T>) {
    let n = unique(v);
    v.truncate(n);
}

/// `v.erase(std::unique(v.begin(), v.end(), pred), v.end())`.
pub fn unique_vec_by<T, F: FnMut(&T, &T) -> bool>(v: &mut Vec<T>, eq: F) {
    let n = unique_by(v, eq);
    v.truncate(n);
}
