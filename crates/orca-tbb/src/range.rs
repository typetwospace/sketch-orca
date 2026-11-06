// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Intel oneTBB code: this file is translated from include/oneapi/tbb/blocked_range.h
// (blocked_range, with its even split and its proportional split), include/oneapi/tbb/
// blocked_range2d.h (blocked_range2d) and include/oneapi/tbb/detail/_range_common.h (the Range
// requirements and proportional_split) of oneTBB 2021.5.0 (the tag v2021.5.0 of
// uxlfoundation/oneTBB, formerly oneapi-src/oneTBB), split for split, on 2026-11-06. Its
// copyright holder is Intel Corporation, and it is under Apache-2.0 (licenses/Apache-2.0.txt).

//! oneTBB's ranges: the [`Range`] requirements, [`BlockedRange`] and [`BlockedRange2d`].
//!
//! A range splits in place, as the C++ splitting constructor `R(R& r, split)` does: the first
//! part stays in `self` and the second part is returned.
//!
//! The arithmetic of a range's values is the C++ arithmetic of the value type, conversions
//! included, because the split points decide which iterations run together. For `int` (`i32`,
//! also C++'s `long` on Windows), `end - begin` wraps, `size()` sign-extends it to `size_t`, and
//! the midpoint `begin + (end - begin) / 2u` divides as `unsigned`; for `long long` (`i64`,
//! `isize`) the division by `2u` is signed; for the unsigned types everything wraps. The
//! proportional split computes its cut in `float`, as oneTBB does.

use std::fmt;

/// What oneTBB's algorithms ask of a range (`tbb_range`): `empty()`, `is_divisible()` and the
/// splitting constructor, and optionally the proportional splitting constructor.
pub trait Range: Sized {
    /// Whether the range has the proportional splitting constructor `R(R&, proportional_split&)`.
    ///
    /// The static partitioner splits such a range in proportion to the threads each part gets.
    /// A range without it is split evenly, and each part split off gets one thread
    /// (`proportion_helper` answers `proportional_split(1, 1)`).
    const PROPORTIONAL: bool = false;

    /// `empty()`.
    fn is_empty(&self) -> bool;

    /// `is_divisible()`.
    fn is_divisible(&self) -> bool;

    /// `R(r, split)`: keeps the first part in `self` and returns the second.
    fn split(&mut self) -> Self;

    /// `R(r, proportional_split(left, right))`: keeps the first part in `self` and returns the
    /// second, about `right / (left + right)` of the range. Called only when
    /// [`Range::PROPORTIONAL`] is true; the default splits evenly.
    fn split_proportional(&mut self, left: usize, right: usize) -> Self {
        let _ = (left, right);
        self.split()
    }
}

/// A value type of [`BlockedRange`]: an integer, with the C++ arithmetic `blocked_range<Value>`
/// does on it.
///
/// Implemented for `i32` (C++ `int`, and `long` on Windows), `u32`, `i64` (`long long`), `u64`,
/// `isize` (`ptrdiff_t`) and `usize` (`size_t`).
pub trait BlockedValue: Copy + Ord + Send + Sync + fmt::Debug + 'static {
    /// The value 0.
    const ZERO: Self;
    /// The value 1.
    const ONE: Self;

    /// `size_type(end - begin)`: the difference in the type, converted to `size_t`.
    fn distance(begin: Self, end: Self) -> usize;

    /// `begin + (end - begin) / 2u`, the even split point.
    fn midpoint(begin: Self, end: Self) -> Self;

    /// `Value(end - n)` for a `size_t` `n`: the proportional split point.
    fn back_off(end: Self, n: usize) -> Self;

    /// `(last - first - 1) / step + 1`: the number of iterations of the integer form of
    /// `parallel_for`.
    fn iteration_count(first: Self, last: Self, step: Self) -> Self;

    /// `first + i * step`: the value of iteration `i` of the integer form of `parallel_for`.
    fn nth(first: Self, i: Self, step: Self) -> Self;

    /// `a + b`, wrapping as the C++ addition does.
    fn plus(a: Self, b: Self) -> Self;
}

macro_rules! blocked_value {
    ($t:ty, |$db:ident, $de:ident| $distance:expr, |$mb:ident, $me:ident| $midpoint:expr, |$be:ident, $bn:ident| $back_off:expr) => {
        impl BlockedValue for $t {
            const ZERO: Self = 0;
            const ONE: Self = 1;
            fn distance($db: Self, $de: Self) -> usize {
                $distance
            }
            fn midpoint($mb: Self, $me: Self) -> Self {
                $midpoint
            }
            fn back_off($be: Self, $bn: usize) -> Self {
                $back_off
            }
            fn iteration_count(first: Self, last: Self, step: Self) -> Self {
                last.wrapping_sub(first)
                    .wrapping_sub(1)
                    .wrapping_div(step)
                    .wrapping_add(1)
            }
            fn nth(first: Self, i: Self, step: Self) -> Self {
                first.wrapping_add(i.wrapping_mul(step))
            }
            fn plus(a: Self, b: Self) -> Self {
                a.wrapping_add(b)
            }
        }
    };
}

// int: `end - begin` wraps in int and is sign-extended to size_t; `/ 2u` converts it to unsigned.
blocked_value!(
    i32,
    |b, e| e.wrapping_sub(b) as usize,
    |b, e| b.wrapping_add((e.wrapping_sub(b) as u32 / 2) as i32),
    |e, n| (e as usize).wrapping_sub(n) as i32
);
// long long and ptrdiff_t: `/ 2u` converts 2u to long long, so the division is signed.
blocked_value!(
    i64,
    |b, e| e.wrapping_sub(b) as usize,
    |b, e| b.wrapping_add(e.wrapping_sub(b) / 2),
    |e, n| (e as usize).wrapping_sub(n) as i64
);
blocked_value!(
    isize,
    |b, e| e.wrapping_sub(b) as usize,
    |b, e| b.wrapping_add(e.wrapping_sub(b) / 2),
    |e, n| (e as usize).wrapping_sub(n) as isize
);
blocked_value!(
    u32,
    |b, e| e.wrapping_sub(b) as usize,
    |b, e| b.wrapping_add(e.wrapping_sub(b) / 2),
    |e, n| (e as usize).wrapping_sub(n) as u32
);
blocked_value!(
    u64,
    |b, e| e.wrapping_sub(b) as usize,
    |b, e| b.wrapping_add(e.wrapping_sub(b) / 2),
    |e, n| (e as usize).wrapping_sub(n) as u64
);
blocked_value!(
    usize,
    |b, e| e.wrapping_sub(b),
    |b, e| b.wrapping_add(e.wrapping_sub(b) / 2),
    |e, n| e.wrapping_sub(n)
);

/// `tbb::blocked_range<Value>`: the half-open interval `[begin, end)`, divisible while it holds
/// more than `grainsize` values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockedRange<T> {
    begin: T,
    end: T,
    grainsize: usize,
}

impl<T: BlockedValue> BlockedRange<T> {
    /// `blocked_range(begin, end, grainsize)`.
    ///
    /// # Panics
    ///
    /// If `grainsize` is 0, which oneTBB asserts against (a range of grain 0 never stops
    /// splitting).
    pub fn new(begin: T, end: T, grainsize: usize) -> Self {
        assert!(
            grainsize > 0,
            "orca-tbb: a blocked_range's grainsize must be positive"
        );
        BlockedRange {
            begin,
            end,
            grainsize,
        }
    }

    /// `begin()`.
    pub fn begin(&self) -> T {
        self.begin
    }

    /// `end()`.
    pub fn end(&self) -> T {
        self.end
    }

    /// `size()`: `size_t(end - begin)`.
    pub fn size(&self) -> usize {
        T::distance(self.begin, self.end)
    }

    /// `grainsize()`.
    pub fn grainsize(&self) -> usize {
        self.grainsize
    }

    /// `do_split(r, split)`: ends `self` at the midpoint and returns it.
    fn cut_even(&mut self) -> T {
        let middle = T::midpoint(self.begin, self.end);
        self.end = middle;
        middle
    }

    /// `do_split(r, proportional_split)`: the right part is
    /// `size_t(float(size) * float(right) / float(left + right) + 0.5f)` values; ends `self`
    /// where it starts and returns that point.
    fn cut_proportional(&mut self, left: usize, right: usize) -> T {
        let right_part =
            (self.size() as f32 * right as f32 / (left + right) as f32 + 0.5_f32) as usize;
        self.end = T::back_off(self.end, right_part);
        self.end
    }
}

impl<T: BlockedValue> Range for BlockedRange<T> {
    const PROPORTIONAL: bool = true;

    /// `!(begin < end)`.
    fn is_empty(&self) -> bool {
        self.begin >= self.end
    }

    /// `grainsize < size()`.
    fn is_divisible(&self) -> bool {
        self.grainsize < self.size()
    }

    fn split(&mut self) -> Self {
        let end = self.end;
        let begin = self.cut_even();
        BlockedRange {
            begin,
            end,
            grainsize: self.grainsize,
        }
    }

    fn split_proportional(&mut self, left: usize, right: usize) -> Self {
        let end = self.end;
        let begin = self.cut_proportional(left, right);
        BlockedRange {
            begin,
            end,
            grainsize: self.grainsize,
        }
    }
}

/// `tbb::blocked_range2d<RowValue, ColValue>`: rows times columns, split along the dimension
/// that is longer relative to its grain size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockedRange2d<R, C = R> {
    rows: BlockedRange<R>,
    cols: BlockedRange<C>,
}

impl<R: BlockedValue, C: BlockedValue> BlockedRange2d<R, C> {
    /// `blocked_range2d(row_begin, row_end, row_grainsize, col_begin, col_end, col_grainsize)`.
    ///
    /// # Panics
    ///
    /// If a grain size is 0.
    pub fn new(
        row_begin: R,
        row_end: R,
        row_grainsize: usize,
        col_begin: C,
        col_end: C,
        col_grainsize: usize,
    ) -> Self {
        BlockedRange2d {
            rows: BlockedRange::new(row_begin, row_end, row_grainsize),
            cols: BlockedRange::new(col_begin, col_end, col_grainsize),
        }
    }

    /// `blocked_range2d(row_range, col_range)`.
    pub fn from_ranges(rows: BlockedRange<R>, cols: BlockedRange<C>) -> Self {
        BlockedRange2d { rows, cols }
    }

    /// `rows()`.
    pub fn rows(&self) -> BlockedRange<R> {
        self.rows
    }

    /// `cols()`.
    pub fn cols(&self) -> BlockedRange<C> {
        self.cols
    }

    /// `do_split`'s choice: the columns when
    /// `rows.size() * double(cols.grainsize()) < cols.size() * double(rows.grainsize())`.
    fn splits_columns(&self) -> bool {
        (self.rows.size() as f64) * (self.cols.grainsize as f64)
            < (self.cols.size() as f64) * (self.rows.grainsize as f64)
    }
}

impl<R: BlockedValue, C: BlockedValue> Range for BlockedRange2d<R, C> {
    const PROPORTIONAL: bool = true;

    /// Empty when either dimension is.
    fn is_empty(&self) -> bool {
        self.rows.is_empty() || self.cols.is_empty()
    }

    /// Divisible when either dimension is.
    fn is_divisible(&self) -> bool {
        self.rows.is_divisible() || self.cols.is_divisible()
    }

    fn split(&mut self) -> Self {
        let mut right = *self;
        if self.splits_columns() {
            right.cols.begin = self.cols.cut_even();
        } else {
            right.rows.begin = self.rows.cut_even();
        }
        right
    }

    fn split_proportional(&mut self, left: usize, right_share: usize) -> Self {
        let mut right = *self;
        if self.splits_columns() {
            right.cols.begin = self.cols.cut_proportional(left, right_share);
        } else {
            right.rows.begin = self.rows.cut_proportional(left, right_share);
        }
        right
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_int_range_past_int_max_splits_at_the_true_midpoint() {
        // end - begin overflows int; the division as unsigned still finds the middle.
        let mut r = BlockedRange::new(-7_i32, i32::MAX, 1);
        assert_eq!(r.size(), (i32::MAX.wrapping_add(7)) as usize);
        let right = r.split();
        assert_eq!((r.begin(), r.end()), (-7, 1_073_741_820));
        assert_eq!((right.begin(), right.end()), (1_073_741_820, i32::MAX));
    }

    #[test]
    fn a_long_long_range_divides_its_length_as_signed() {
        let mut r = BlockedRange::new(-5_i64, 6, 1);
        let right = r.split();
        assert_eq!((r.end(), right.begin()), (0, 0));
        let mut r = BlockedRange::new(3_usize, 10, 2);
        let right = r.split();
        assert_eq!(
            (r.begin(), r.end(), right.begin(), right.end()),
            (3, 6, 6, 10)
        );
    }

    #[test]
    fn a_proportional_split_rounds_in_float() {
        // 100 * 2 / 3 + 0.5 = 67.17 in float: 67 to the right.
        let mut r = BlockedRange::new(0_i64, 100, 1);
        let right = r.split_proportional(1, 2);
        assert_eq!((r.end(), right.begin(), right.end()), (33, 33, 100));
        // 2^24 + 1 values are 2^24 in float.
        let mut r = BlockedRange::new(0_u64, (1 << 24) + 1, 1);
        let right = r.split_proportional(1, 1);
        assert_eq!(right.size(), 1 << 23);
        assert_eq!(r.size(), (1 << 23) + 1);
    }

    #[test]
    fn empty_and_divisible_follow_the_grain() {
        assert!(BlockedRange::new(3_i32, 3, 1).is_empty());
        assert!(BlockedRange::new(4_i32, 3, 1).is_empty());
        assert!(!BlockedRange::new(0_u32, 5, 5).is_divisible());
        assert!(BlockedRange::new(0_u32, 6, 5).is_divisible());
    }

    #[test]
    fn a_2d_range_splits_the_longer_dimension_relative_to_its_grain() {
        let mut r = BlockedRange2d::new(0_i32, 10, 1, 0_i32, 100, 1);
        let right = r.split();
        assert_eq!(r.cols().end(), 50);
        assert_eq!(right.cols().begin(), 50);
        assert_eq!(r.rows(), right.rows());
        // Rows of grain 1 against columns of grain 20: the rows are longer in grains.
        let mut r = BlockedRange2d::new(0_i32, 10, 1, 0_i32, 100, 20);
        let right = r.split();
        assert_eq!((r.rows().end(), right.rows().begin()), (5, 5));
        assert!(BlockedRange2d::new(0_i32, 0, 1, 0_i32, 5, 1).is_empty());
    }

    #[test]
    fn the_integer_form_counts_its_iterations_as_cpp_does() {
        assert_eq!(i32::iteration_count(3, 100, 7), 14);
        assert_eq!(i32::iteration_count(-20, 20, 40), 1);
        assert_eq!(i32::nth(3, 13, 7), 94);
        assert_eq!(u32::iteration_count(0, 10, 3), 4);
    }
}
