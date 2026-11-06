// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Microsoft STL code: this file is translated from stl/inc/algorithm (_Insertion_sort_unchecked,
// _Med3_unchecked, _Guess_median_unchecked, _Partition_by_median_guess_unchecked, _Sort_unchecked
// and sort) of microsoft/STL at the tag vs-2022-17.12 (commit
// 91e425587a0b9b5ac0d1005844c661fee2e2813b), the std::sort of MSVC 14.42, branch for branch and
// comparison for comparison, on 2026-11-06. Its copyright holder is Microsoft Corporation, and it is
// under Apache-2.0 WITH LLVM-exception (licenses/Apache-2.0.txt and licenses/LLVM-exception.txt).

//! `std::sort`: MSVC's introsort.
//!
//! [`sort_by`] partitions around a median of three (Tukey's ninther above 40 elements) with the
//! fat partition that gathers the elements equal to the pivot, loops on the larger part and
//! recurses into the smaller one, gives up partitioning after about 1.5 log2(n) levels for
//! `make_heap` and `sort_heap`, and insertion-sorts every part of at most [`ISORT_MAX`] elements.
//! The comparator sees the same pairs in the same order as MSVC's: element for element, the
//! result with an inconsistent comparator, NaNs or ties is the C++ program's.
//!
//! The comparator is a strict weak ordering written as C++'s `pred(a, b)`, "a goes before b":
//! `|a, b| a < b` sorts ascending, as `std::less<>` does.

use std::mem::ManuallyDrop;
use std::ptr;

use crate::heap;

/// `_ISORT_MAX`: ranges of at most this many elements are insertion-sorted, in `sort`,
/// `nth_element` and `stable_sort` alike.
pub const ISORT_MAX: usize = 32;

/// Holds a value taken out of a slice and writes it back into the current hole when dropped: on
/// return the hole is the value's place, and if the comparator panics the slice is still whole.
pub(crate) struct Hole<T> {
    pub(crate) value: ManuallyDrop<T>,
    pub(crate) dest: *mut T,
}

impl<T> Drop for Hole<T> {
    fn drop(&mut self) {
        // SAFETY: dest is the one slot of the slice whose value has been moved out (into `value` or
        // into another slot), so writing `value` there leaves every element exactly once.
        unsafe { ptr::copy_nonoverlapping(&*self.value as *const T, self.dest, 1) }
    }
}

/// `std::sort(first, last)` with `std::less<>`: ascending by `<`.
pub fn sort<T: PartialOrd>(v: &mut [T]) {
    sort_by(v, |a, b| a < b);
}

/// `std::sort(first, last, pred)`: MSVC's introsort with the comparator `less(a, b)`, "a goes
/// before b".
pub fn sort_by<T, F: FnMut(&T, &T) -> bool>(v: &mut [T], mut less: F) {
    let n = v.len();
    sort_unchecked(v, n as isize, &mut less);
}

/// `std::sort` by a key compared with `<`, as `std::sort` with a comparator that compares keys.
pub fn sort_by_key<T, K: PartialOrd, F: FnMut(&T) -> K>(v: &mut [T], mut key: F) {
    sort_by(v, |a, b| key(a) < key(b));
}

/// `_Sort_unchecked`: the introsort loop on `[first, last)` with `ideal` divisions left.
pub(crate) fn sort_unchecked<T, F: FnMut(&T, &T) -> bool>(
    v: &mut [T],
    mut ideal: isize,
    less: &mut F,
) {
    let mut first = 0usize;
    let mut last = v.len();
    loop {
        if last - first <= ISORT_MAX {
            insertion_sort(&mut v[first..last], less);
            return;
        }
        if ideal <= 0 {
            heap::make_heap_unchecked(&mut v[first..last], less);
            heap::sort_heap_unchecked(&mut v[first..last], less);
            return;
        }
        let (mid_first, mid_last) = partition_by_median_guess(&mut v[first..last], less);
        let (mid_first, mid_last) = (first + mid_first, first + mid_last);
        ideal = (ideal >> 1) + (ideal >> 2);
        if mid_first - first < last - mid_last {
            sort_unchecked(&mut v[first..mid_first], ideal, less);
            first = mid_last;
        } else {
            sort_unchecked(&mut v[mid_last..last], ideal, less);
            last = mid_first;
        }
    }
}

/// `_Insertion_sort_unchecked`: each element in turn is compared with the first; if it goes
/// before it, everything moves up one place; otherwise it walks down from its own place, the
/// last comparison being the one with the first element again (MSVC's loop is unguarded there,
/// relying on that comparison; here a comparator that answers it inconsistently stops at the
/// first place instead of reading before the slice).
pub(crate) fn insertion_sort<T, F: FnMut(&T, &T) -> bool>(v: &mut [T], less: &mut F) {
    let n = v.len();
    if n < 2 {
        return;
    }
    let base = v.as_mut_ptr();
    for mid in 1..n {
        // SAFETY: indices stay in [0, n); the value at `mid` is held by `hole` while its slot is
        // the hole, and `hole` writes it into whichever slot is the hole when it drops.
        unsafe {
            let mut hole = Hole {
                value: ManuallyDrop::new(ptr::read(base.add(mid))),
                dest: base.add(mid),
            };
            if less(&hole.value, &*base) {
                ptr::copy(base, base.add(1), mid);
                hole.dest = base;
            } else {
                let mut h = mid;
                loop {
                    let prev = h - 1;
                    if !less(&hole.value, &*base.add(prev)) {
                        break;
                    }
                    ptr::copy_nonoverlapping(base.add(prev), base.add(h), 1);
                    h = prev;
                    hole.dest = base.add(h);
                    if h == 0 {
                        break;
                    }
                }
            }
        }
    }
}

/// `_Med3_unchecked`: sorts the elements at `a`, `b` and `c` so the median is at `b`.
fn med3<T, F: FnMut(&T, &T) -> bool>(v: &mut [T], a: usize, b: usize, c: usize, less: &mut F) {
    if less(&v[b], &v[a]) {
        v.swap(b, a);
    }
    if less(&v[c], &v[b]) {
        v.swap(c, b);
        if less(&v[b], &v[a]) {
            v.swap(b, a);
        }
    }
}

/// `_Guess_median_unchecked` on the inclusive range `[first, last]`: a median of three, or
/// Tukey's ninther when the range spans more than 40 places.
fn guess_median<T, F: FnMut(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    mid: usize,
    last: usize,
    less: &mut F,
) {
    let count = last - first;
    if 40 < count {
        let step = (count + 1) >> 3;
        let two_step = step << 1;
        med3(v, first, first + step, first + two_step, less);
        med3(v, mid - step, mid, mid + step, less);
        med3(v, last - two_step, last - step, last, less);
        med3(v, first + step, mid, last - step, less);
    } else {
        med3(v, first, mid, last, less);
    }
}

/// `_Partition_by_median_guess_unchecked`: partitions `v` around a guessed median and returns the
/// range `[pfirst, plast)` of the elements equivalent to it, with the smaller elements before
/// and the larger ones after.
pub(crate) fn partition_by_median_guess<T, F: FnMut(&T, &T) -> bool>(
    v: &mut [T],
    less: &mut F,
) -> (usize, usize) {
    let first = 0usize;
    let last = v.len();
    let mid = first + ((last - first) >> 1);
    guess_median(v, first, mid, last - 1, less);
    let mut pfirst = mid;
    let mut plast = pfirst + 1;

    while first < pfirst && !less(&v[pfirst - 1], &v[pfirst]) && !less(&v[pfirst], &v[pfirst - 1]) {
        pfirst -= 1;
    }
    while plast < last && !less(&v[plast], &v[pfirst]) && !less(&v[pfirst], &v[plast]) {
        plast += 1;
    }

    let mut gfirst = plast;
    let mut glast = pfirst;

    loop {
        while gfirst < last {
            if less(&v[pfirst], &v[gfirst]) {
                // continue
            } else if less(&v[gfirst], &v[pfirst]) {
                break;
            } else if plast != gfirst {
                v.swap(plast, gfirst);
                plast += 1;
            } else {
                plast += 1;
            }
            gfirst += 1;
        }

        while first < glast {
            let glast_prev = glast - 1;
            if less(&v[glast_prev], &v[pfirst]) {
                // continue
            } else if less(&v[pfirst], &v[glast_prev]) {
                break;
            } else {
                pfirst -= 1;
                if pfirst != glast_prev {
                    v.swap(pfirst, glast_prev);
                }
            }
            glast -= 1;
        }

        if glast == first && gfirst == last {
            return (pfirst, plast);
        }

        if glast == first {
            // no room at the bottom: rotate the pivot upward
            if plast != gfirst {
                v.swap(pfirst, plast);
            }
            plast += 1;
            v.swap(pfirst, gfirst);
            pfirst += 1;
            gfirst += 1;
        } else if gfirst == last {
            // no room at the top: rotate the pivot downward
            glast -= 1;
            pfirst -= 1;
            if glast != pfirst {
                v.swap(glast, pfirst);
            }
            plast -= 1;
            v.swap(pfirst, plast);
        } else {
            glast -= 1;
            v.swap(gfirst, glast);
            gfirst += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insertion_sort_compares_with_the_first_element_last() {
        // [6, 9]: 9 is compared with the first element (6), then walks down: again with 6.
        let mut v = [6, 9];
        let mut calls = Vec::new();
        insertion_sort(&mut v, &mut |a: &i32, b: &i32| {
            calls.push((*a, *b));
            a < b
        });
        assert_eq!(calls, [(9, 6), (9, 6)]);
        assert_eq!(v, [6, 9]);
    }

    #[test]
    fn a_panicking_comparator_leaves_every_element() {
        let mut v: Vec<String> = (0..40).rev().map(|i| i.to_string()).collect();
        let mut count = 0;
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            sort_by(&mut v, |a, b| {
                count += 1;
                assert!(count < 60, "stop");
                a < b
            })
        }));
        assert!(r.is_err());
        let mut seen: Vec<i32> = v.iter().map(|s| s.parse().unwrap()).collect();
        crate::sort(&mut seen);
        assert_eq!(seen, (0..40).collect::<Vec<_>>());
    }
}
