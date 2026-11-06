// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Microsoft STL code: this file is translated from stl/inc/algorithm
// (_Optimistic_temporary_buffer, _Get_temporary_buffer, _Uninitialized_merge_move, _Merge_move,
// _Uninitialized_chunked_merge_unchecked2, _Chunked_merge_unchecked,
// _Insertion_sort_isort_max_chunks, _Buffered_merge_sort_unchecked, _Stable_sort_unchecked,
// stable_sort, _Buffered_rotate_unchecked, _Rotate_one_right, _Rotate_one_left,
// _Inplace_merge_buffer_left, _Inplace_merge_buffer_right, the _Buffered_inplace_merge functions
// and inplace_merge) of microsoft/STL at the tag vs-2022-17.12 (commit
// 91e425587a0b9b5ac0d1005844c661fee2e2813b), the std::stable_sort and std::inplace_merge of MSVC
// 14.42, merge for merge, on 2026-11-06. Its copyright holder is Microsoft Corporation, and it is
// under Apache-2.0 WITH LLVM-exception (licenses/Apache-2.0.txt and licenses/LLVM-exception.txt).

//! `std::stable_sort` and `std::inplace_merge`, as MSVC's STL merges.
//!
//! [`stable_sort_by`] insertion-sorts ranges of at most [`ISORT_MAX`] elements. A longer range
//! asks for a buffer of half its length (rounded up) the way `_Optimistic_temporary_buffer`
//! does: up to 4096 bytes' worth it always gets, beyond that what the allocator gives, halving
//! the request on failure, and if that is no more than 4096 bytes' worth it falls back to those.
//! When a half fits the buffer, each half is merge-sorted through it in chunks of 32, 64, 128
//! and up; otherwise the halves are sorted recursively. The halves are then merged in place:
//! through the buffer from the left or the right when one side fits, otherwise by dividing at a
//! `lower_bound` or `upper_bound` and rotating. The comparator sees the same pairs in the same
//! order as MSVC's, for every buffer size ([`stable_sort_by_with_capacity`] fixes the size, as
//! the tests do to reach the in-place paths).
//!
//! While elements sit in the buffer, a panicking comparator aborts the process: the slice would
//! otherwise be left with holes. The insertion sorts and the searches unwind normally.

use std::mem::{self, ManuallyDrop, MaybeUninit};
use std::ptr;
use std::slice;

use crate::select::{lower_bound_by, upper_bound_by};
use crate::sort::{ISORT_MAX, insertion_sort};

/// Aborts the process if dropped while unwinding: armed while the slice has elements out in the
/// buffer, disarmed with `mem::forget` once they are all back.
struct AbortOnUnwind;

impl Drop for AbortOnUnwind {
    fn drop(&mut self) {
        std::process::abort();
    }
}

/// The scratch space `_Optimistic_temporary_buffer` gives: `capacity` elements, uninitialised.
struct TempBuffer<T> {
    storage: Vec<MaybeUninit<T>>,
    capacity: usize,
}

impl<T> TempBuffer<T> {
    /// `_Optimistic_count`: 4096 bytes' worth of elements, at least one.
    fn optimistic_count() -> usize {
        match mem::size_of::<T>() {
            0 => usize::MAX,
            size => crate::max(4096 / size, 1),
        }
    }

    /// Exactly `capacity` elements (the stack space C++ uses, or a forced size).
    fn exact(capacity: usize) -> Self {
        let mut storage: Vec<MaybeUninit<T>> = Vec::with_capacity(capacity);
        // SAFETY: uninitialised elements are valid MaybeUninit values, and the capacity is there.
        unsafe { storage.set_len(capacity) };
        Self { storage, capacity }
    }

    /// `_Optimistic_temporary_buffer(requested)`.
    fn optimistic(requested: usize) -> Self {
        let optimistic = Self::optimistic_count();
        if requested <= optimistic {
            return Self::exact(requested);
        }
        // _Get_temporary_buffer: halve the request until the allocator gives it.
        let mut storage: Vec<MaybeUninit<T>> = Vec::new();
        let mut count = requested.min(isize::MAX as usize);
        let mut got = 0;
        if count <= usize::MAX / crate::max(mem::size_of::<T>(), 1) {
            while 0 < count {
                if storage.try_reserve_exact(count).is_ok() {
                    got = count;
                    break;
                }
                count /= 2;
            }
        }
        if got > optimistic {
            // SAFETY: the reserved capacity holds `got` uninitialised elements.
            unsafe { storage.set_len(got) };
            Self {
                storage,
                capacity: got,
            }
        } else {
            Self::exact(optimistic)
        }
    }

    fn ptr(&mut self) -> *mut T {
        self.storage.as_mut_ptr().cast()
    }
}

/// `std::stable_sort(first, last)` with `std::less<>`.
pub fn stable_sort<T: PartialOrd>(v: &mut [T]) {
    stable_sort_by(v, |a, b| a < b);
}

/// `std::stable_sort` by a key compared with `<`.
pub fn stable_sort_by_key<T, K: PartialOrd, F: FnMut(&T) -> K>(v: &mut [T], mut key: F) {
    stable_sort_by(v, |a, b| key(a) < key(b));
}

/// `std::stable_sort(first, last, pred)`.
pub fn stable_sort_by<T, F: FnMut(&T, &T) -> bool>(v: &mut [T], mut less: F) {
    let count = v.len();
    if count <= ISORT_MAX {
        insertion_sort(v, &mut less);
        return;
    }
    let mut buffer = TempBuffer::<T>::optimistic(count - count / 2);
    let capacity = buffer.capacity;
    // SAFETY: the buffer holds `capacity` uninitialised elements and outlives the call.
    unsafe { stable_sort_unchecked(v.as_mut_ptr(), count, buffer.ptr(), capacity, &mut less) }
}

/// `_Stable_sort_unchecked(first, last, count, temp, capacity, pred)` with a buffer of exactly
/// `capacity` elements: what `stable_sort` does when the buffer it gets has that size. With
/// `capacity` 0 every merge is in place.
pub fn stable_sort_by_with_capacity<T, F: FnMut(&T, &T) -> bool>(
    v: &mut [T],
    capacity: usize,
    mut less: F,
) {
    let mut buffer = TempBuffer::<T>::exact(capacity);
    // SAFETY: as in stable_sort_by.
    unsafe { stable_sort_unchecked(v.as_mut_ptr(), v.len(), buffer.ptr(), capacity, &mut less) }
}

/// `std::inplace_merge(first, mid, last)` with `std::less<>`.
pub fn inplace_merge<T: PartialOrd>(v: &mut [T], mid: usize) {
    inplace_merge_by(v, mid, |a, b| a < b);
}

/// `std::inplace_merge(first, mid, last, pred)`: merges the sorted runs `v[..mid]` and
/// `v[mid..]`, keeping the left run's elements first among equivalents.
///
/// # Panics
/// If `mid > v.len()`.
pub fn inplace_merge_by<T, F: FnMut(&T, &T) -> bool>(v: &mut [T], mid: usize, mut less: F) {
    assert!(mid <= v.len(), "inplace_merge: mid is past the end");
    let mut first = 0usize;
    let mut last = v.len();
    if mid == last {
        return;
    }
    loop {
        if first == mid {
            return;
        }
        if less(&v[mid], &v[first]) {
            break;
        }
        first += 1;
    }
    let highest = mid - 1;
    loop {
        last -= 1;
        if mid == last {
            // SAFETY: [first, last + 1) is inside the slice and last + 1 - mid == 1.
            unsafe { rotate_one_right(v.as_mut_ptr().add(first), mid - first) };
            return;
        }
        if less(&v[last], &v[highest]) {
            break;
        }
    }
    last += 1;
    let count1 = mid - first;
    if count1 == 1 {
        // SAFETY: [first, last) is inside the slice and mid - first == 1.
        unsafe { rotate_one_left(v.as_mut_ptr().add(first), last - first) };
        return;
    }
    let count2 = last - mid;
    let mut buffer = TempBuffer::<T>::optimistic(count1.min(count2));
    let capacity = buffer.capacity;
    let base = v.as_mut_ptr();
    // SAFETY: the three positions are inside the slice, in order.
    unsafe {
        buffered_inplace_merge_impl(
            base.add(first),
            base.add(mid),
            base.add(last),
            count1,
            count2,
            buffer.ptr(),
            capacity,
            &mut less,
        )
    }
}

/// # Safety
/// `first` points at `count` elements; `temp` at room for `capacity`.
unsafe fn stable_sort_unchecked<T, F: FnMut(&T, &T) -> bool>(
    first: *mut T,
    count: usize,
    temp: *mut T,
    capacity: usize,
    less: &mut F,
) {
    // SAFETY: every range below is a part of [first, first + count).
    unsafe {
        if count <= ISORT_MAX {
            insertion_sort(slice::from_raw_parts_mut(first, count), less);
        } else {
            let half_count = count >> 1;
            let half_count_ceil = count - half_count;
            let mid = first.add(half_count_ceil);
            if half_count_ceil <= capacity {
                buffered_merge_sort(first, half_count_ceil, temp, less);
                buffered_merge_sort(mid, half_count, temp, less);
            } else {
                stable_sort_unchecked(first, half_count_ceil, temp, capacity, less);
                stable_sort_unchecked(mid, half_count, temp, capacity, less);
            }
            buffered_inplace_merge(
                first,
                mid,
                first.add(count),
                half_count_ceil,
                half_count,
                temp,
                capacity,
                less,
            );
        }
    }
}

/// `_Insertion_sort_isort_max_chunks`, then the chunked merges to the buffer and back, doubling
/// the chunk each pass. `count` must fit the buffer.
///
/// # Safety
/// `first` points at `count` elements; `temp` at room for `count`.
unsafe fn buffered_merge_sort<T, F: FnMut(&T, &T) -> bool>(
    first: *mut T,
    count: usize,
    temp: *mut T,
    less: &mut F,
) {
    // SAFETY: the chunks are parts of [first, first + count); the merges move every element to
    // the other buffer and back, so on return each is in [first, first + count) once.
    unsafe {
        let mut f = first;
        let mut c = count;
        while ISORT_MAX < c {
            insertion_sort(slice::from_raw_parts_mut(f, ISORT_MAX), less);
            f = f.add(ISORT_MAX);
            c -= ISORT_MAX;
        }
        insertion_sort(slice::from_raw_parts_mut(f, c), less);
        if count <= ISORT_MAX {
            return;
        }
        let guard = AbortOnUnwind;
        chunked_merge(first, count, temp, ISORT_MAX, less);
        let mut chunk = ISORT_MAX;
        loop {
            chunk <<= 1;
            chunked_merge(temp, count, first, chunk, less);
            chunk <<= 1;
            if count <= chunk {
                mem::forget(guard);
                return;
            }
            chunked_merge(first, count, temp, chunk, less);
        }
    }
}

/// `_Chunked_merge_unchecked` (and `_Uninitialized_chunked_merge_unchecked2` with the chunk 32):
/// merges adjacent chunks of `src` pairwise into `dest`, moving a last odd chunk as it is.
///
/// # Safety
/// `src` and `dest` point at `count` elements each, in different buffers.
unsafe fn chunked_merge<T, F: FnMut(&T, &T) -> bool>(
    src: *mut T,
    count: usize,
    dest: *mut T,
    chunk: usize,
    less: &mut F,
) {
    // SAFETY: all offsets stay below `count` in both buffers.
    unsafe {
        let mut first = src;
        let mut left = count;
        let mut out = dest;
        while chunk < left {
            left -= chunk;
            let mid1 = first.add(chunk);
            let chunk2 = chunk.min(left);
            left -= chunk2;
            let mid2 = mid1.add(chunk2);
            out = merge_move(first, mid1, mid2, out, less);
            first = mid2;
        }
        ptr::copy_nonoverlapping(first, out, left);
    }
}

/// `_Merge_move`: merges `[first, mid)` and `[mid, last)` into `dest`, taking from the right
/// only when its element goes before the left's; returns the end of the output.
///
/// # Safety
/// Both runs are non-empty; `dest` has room for both and does not overlap them.
unsafe fn merge_move<T, F: FnMut(&T, &T) -> bool>(
    mut first: *mut T,
    mid: *mut T,
    last: *mut T,
    mut dest: *mut T,
    less: &mut F,
) -> *mut T {
    // SAFETY: first < mid and next < last whenever they are read.
    unsafe {
        let mut next = mid;
        loop {
            if less(&*next, &*first) {
                ptr::copy_nonoverlapping(next, dest, 1);
                dest = dest.add(1);
                next = next.add(1);
                if next == last {
                    let n = mid.offset_from(first) as usize;
                    ptr::copy_nonoverlapping(first, dest, n);
                    return dest.add(n);
                }
            } else {
                ptr::copy_nonoverlapping(first, dest, 1);
                dest = dest.add(1);
                first = first.add(1);
                if first == mid {
                    let n = last.offset_from(next) as usize;
                    ptr::copy_nonoverlapping(next, dest, n);
                    return dest.add(n);
                }
            }
        }
    }
}

/// `_Rotate_one_right`: the element at `first + mid_offset`, the last of the range, moves to the
/// front and the `mid_offset` before it move up one place.
///
/// # Safety
/// `first` points at `mid_offset + 1` elements.
unsafe fn rotate_one_right<T>(first: *mut T, mid_offset: usize) {
    // SAFETY: the range is [first, first + mid_offset + 1).
    unsafe {
        let temp = ManuallyDrop::new(ptr::read(first.add(mid_offset)));
        ptr::copy(first, first.add(1), mid_offset);
        ptr::copy_nonoverlapping(&*temp as *const T, first, 1);
    }
}

/// `_Rotate_one_left`: the first of `len` elements moves to the back.
///
/// # Safety
/// `first` points at `len >= 1` elements.
unsafe fn rotate_one_left<T>(first: *mut T, len: usize) {
    // SAFETY: the range is [first, first + len).
    unsafe {
        let temp = ManuallyDrop::new(ptr::read(first));
        ptr::copy(first.add(1), first, len - 1);
        ptr::copy_nonoverlapping(&*temp as *const T, first.add(len - 1), 1);
    }
}

/// `_Buffered_inplace_merge_unchecked`: trims the elements already in place at both ends, then
/// merges what is left.
///
/// # Safety
/// `first <= mid <= last` in one slice, with `count1` and `count2` their distances; `temp` has
/// room for `capacity` elements.
#[allow(clippy::too_many_arguments)]
unsafe fn buffered_inplace_merge<T, F: FnMut(&T, &T) -> bool>(
    mut first: *mut T,
    mid: *mut T,
    mut last: *mut T,
    mut count1: usize,
    mut count2: usize,
    temp: *mut T,
    capacity: usize,
    less: &mut F,
) {
    // SAFETY: the pointers move inside [first, last) as MSVC's iterators do.
    unsafe {
        if mid == last {
            return;
        }
        loop {
            if first == mid {
                return;
            }
            if less(&*mid, &*first) {
                break;
            }
            first = first.add(1);
            count1 -= 1;
        }
        let highest = mid.sub(1);
        loop {
            last = last.sub(1);
            count2 -= 1;
            if mid == last {
                rotate_one_right(first, mid.offset_from(first) as usize);
                return;
            }
            if less(&*last, &*highest) {
                break;
            }
        }
        last = last.add(1);
        count2 += 1;
        if count1 == 1 {
            rotate_one_left(first, last.offset_from(first) as usize);
            return;
        }
        buffered_inplace_merge_impl(first, mid, last, count1, count2, temp, capacity, less);
    }
}

/// `_Buffered_inplace_merge_unchecked_impl`: through the buffer from the left if the left run is
/// the shorter and fits, else from the right if the right run fits, else divide and conquer.
///
/// # Safety
/// As [`buffered_inplace_merge`], with MSVC's invariants established: `*mid` goes before
/// `*first`, `*(last - 1)` before `*(mid - 1)`, and `count1 >= 2`.
#[allow(clippy::too_many_arguments)]
unsafe fn buffered_inplace_merge_impl<T, F: FnMut(&T, &T) -> bool>(
    first: *mut T,
    mid: *mut T,
    last: *mut T,
    count1: usize,
    count2: usize,
    temp: *mut T,
    capacity: usize,
    less: &mut F,
) {
    // SAFETY: forwarded.
    unsafe {
        if count1 <= count2 && count1 <= capacity {
            inplace_merge_buffer_left(first, mid, last, temp, less);
        } else if count2 <= capacity {
            inplace_merge_buffer_right(first, mid, last, temp, less);
        } else {
            buffered_inplace_merge_divide_and_conquer(
                first, mid, last, count1, count2, temp, capacity, less,
            );
        }
    }
}

/// `_Inplace_merge_buffer_left`: the left run goes to the buffer and is merged back from the
/// front; its highest element is never compared, as it goes last.
///
/// # Safety
/// As [`buffered_inplace_merge_impl`], with room for the left run in `temp`.
unsafe fn inplace_merge_buffer_left<T, F: FnMut(&T, &T) -> bool>(
    mut first: *mut T,
    mut mid: *mut T,
    last: *mut T,
    temp: *mut T,
    less: &mut F,
) {
    // SAFETY: the left run is moved out and every element moved back exactly once.
    unsafe {
        let guard = AbortOnUnwind;
        let n1 = mid.offset_from(first) as usize;
        ptr::copy_nonoverlapping(first, temp, n1);
        let mut left_first = temp;
        let left_end = temp.add(n1);
        let left_last = left_end.sub(1);
        ptr::copy_nonoverlapping(mid, first, 1);
        first = first.add(1);
        mid = mid.add(1);
        loop {
            if less(&*mid, &*left_first) {
                ptr::copy_nonoverlapping(mid, first, 1);
                first = first.add(1);
                mid = mid.add(1);
                if mid == last {
                    ptr::copy_nonoverlapping(
                        left_first,
                        first,
                        left_end.offset_from(left_first) as usize,
                    );
                    break;
                }
            } else {
                ptr::copy_nonoverlapping(left_first, first, 1);
                first = first.add(1);
                left_first = left_first.add(1);
                if left_first == left_last {
                    let n = last.offset_from(mid) as usize;
                    ptr::copy(mid, first, n);
                    ptr::copy_nonoverlapping(left_last, first.add(n), 1);
                    break;
                }
            }
        }
        mem::forget(guard);
    }
}

/// `_Inplace_merge_buffer_right`: the right run goes to the buffer and is merged back from the
/// end; its lowest element is never compared, as it goes first.
///
/// # Safety
/// As [`buffered_inplace_merge_impl`], with room for the right run in `temp`.
unsafe fn inplace_merge_buffer_right<T, F: FnMut(&T, &T) -> bool>(
    first: *mut T,
    mut mid: *mut T,
    mut last: *mut T,
    temp: *mut T,
    less: &mut F,
) {
    // SAFETY: the right run is moved out and every element moved back exactly once.
    unsafe {
        let guard = AbortOnUnwind;
        let n2 = last.offset_from(mid) as usize;
        ptr::copy_nonoverlapping(mid, temp, n2);
        last = last.sub(1);
        mid = mid.sub(1);
        ptr::copy_nonoverlapping(mid, last, 1);
        let right_first = temp;
        let mut right_last = temp.add(n2 - 1);
        mid = mid.sub(1);
        loop {
            if less(&*right_last, &*mid) {
                last = last.sub(1);
                ptr::copy_nonoverlapping(mid, last, 1);
                if first == mid {
                    last = last.sub(1);
                    ptr::copy_nonoverlapping(right_last, last, 1);
                    let n = right_last.offset_from(right_first) as usize;
                    ptr::copy_nonoverlapping(right_first, last.sub(n), n);
                    break;
                }
                mid = mid.sub(1);
            } else {
                last = last.sub(1);
                ptr::copy_nonoverlapping(right_last, last, 1);
                right_last = right_last.sub(1);
                if right_first == right_last {
                    last = last.sub(1);
                    ptr::copy_nonoverlapping(mid, last, 1);
                    let n = mid.offset_from(first) as usize;
                    ptr::copy(first, last.sub(n), n);
                    ptr::copy_nonoverlapping(right_first, first, 1);
                    break;
                }
            }
        }
        mem::forget(guard);
    }
}

/// `_Buffered_inplace_merge_divide_and_conquer` and its `...2`: splits the longer run in half,
/// finds the matching place in the other with `lower_bound` or `upper_bound`, rotates the middle
/// and merges both sides.
///
/// # Safety
/// As [`buffered_inplace_merge_impl`].
#[allow(clippy::too_many_arguments)]
unsafe fn buffered_inplace_merge_divide_and_conquer<T, F: FnMut(&T, &T) -> bool>(
    first: *mut T,
    mid: *mut T,
    last: *mut T,
    count1: usize,
    count2: usize,
    temp: *mut T,
    capacity: usize,
    less: &mut F,
) {
    // SAFETY: firstn is in [first, mid] and lastn in [mid, last]; the searches read the slice
    // while it is whole.
    unsafe {
        let (firstn, lastn, count1n, count2n);
        if count1 <= count2 {
            count1n = count1 >> 1;
            firstn = first.add(count1n);
            let right = slice::from_raw_parts(mid, count2);
            count2n = lower_bound_by(right, &*firstn, &mut *less);
            lastn = mid.add(count2n);
        } else {
            count2n = count2 >> 1;
            lastn = mid.add(count2n);
            let left = slice::from_raw_parts(first, count1);
            count1n = upper_bound_by(left, &*lastn, &mut *less);
            firstn = first.add(count1n);
        }
        let midn = buffered_rotate(
            firstn,
            mid,
            lastn,
            count1 - count1n,
            count2n,
            temp,
            capacity,
        );
        buffered_inplace_merge(first, firstn, midn, count1n, count2n, temp, capacity, less);
        buffered_inplace_merge(
            midn,
            lastn,
            last,
            count1 - count1n,
            count2 - count2n,
            temp,
            capacity,
            less,
        );
    }
}

/// `_Buffered_rotate_unchecked`: swaps `[first, mid)` and `[mid, last)` through the buffer when
/// the shorter fits, else in place; returns where the old `first` element now is.
///
/// # Safety
/// `count1` and `count2` are the runs' lengths; `temp` has room for `capacity` elements.
unsafe fn buffered_rotate<T>(
    first: *mut T,
    mid: *mut T,
    last: *mut T,
    count1: usize,
    count2: usize,
    temp: *mut T,
    capacity: usize,
) -> *mut T {
    // SAFETY: moves only, inside [first, last) and the buffer; no comparator runs.
    unsafe {
        if count1 == 0 {
            return last;
        }
        if count2 == 0 {
            return first;
        }
        if count1 <= count2 && count1 <= capacity {
            ptr::copy_nonoverlapping(first, temp, count1);
            ptr::copy(mid, first, count2);
            let new_mid = first.add(count2);
            ptr::copy_nonoverlapping(temp, new_mid, count1);
            return new_mid;
        }
        if count2 <= capacity {
            ptr::copy_nonoverlapping(mid, temp, count2);
            ptr::copy(first, last.sub(count1), count1);
            ptr::copy_nonoverlapping(temp, first, count2);
            return first.add(count2);
        }
        slice::from_raw_parts_mut(first, count1 + count2).rotate_left(count1);
        first.add(count2)
    }
}
