// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Boost code: Boost.Polygon of Boost 1.84.0, translated on 2026-11-13 by orca-o3-voronoi-1
// from boost/polygon/detail/voronoi_ctypes.hpp, lines 1-643 (ulp_comparison,
// extended_exponent_fpt, extended_int, type_converter_fpt, type_converter_efpt and
// voronoi_ctype_traits<int32>). Copyright Andrii Sydorchuk 2010-2012, under the Boost Software
// License, version 1.0 (BSL-1.0; licenses/BSL-1.0.txt).

//! Boost.Polygon's Voronoi coordinate types (`voronoi_ctypes.hpp`), as MSVC builds them for
//! `voronoi_ctype_traits<int32>`, the only traits OrcaSlicer instantiates (the default
//! `voronoi_builder<int32>`).
//!
//! - [`UlpComparison`] is `ulp_comparison<double>`: two doubles are equal when their bits, read as
//!   sign-magnitude integers, differ by at most `maxUlps`. A NaN compares by its bits too.
//! - [`ExtendedExponentFpt`] is `extended_exponent_fpt<double>`: a mantissa in [0.5, 1) (or 0)
//!   from `frexp` and an `int` exponent, so the exponent never overflows a double. Sums and
//!   differences drop the smaller operand when the exponents differ by more than 54; zero keeps an
//!   exponent (`efpt(0, 5)` holds 0 and 5). `sqrt` makes the exponent even and halves it with an
//!   arithmetic shift. Every `frexp` and `ldexp` is the static UCRT's (`orca-crt`).
//! - [`ExtendedInt`] is `extended_int<N>`: a count whose sign is the number's and whose size is
//!   the number of 32-bit chunks in use, lowest first. It keeps the C++'s unnormalised values: a
//!   difference checks only its top chunk for zero, so it may keep leading zero chunks in its
//!   count, and the comparisons and the vector constructor take the count as it is. A sum or a
//!   product stops at N chunks and drops the carry. The C++ leaves the chunks past the count
//!   unset; here they are zero or what an earlier value left, and nothing reads them.
//! - [`TypeConverterFpt`] and [`TypeConverterEfpt`] are the converters to `double` and to the
//!   extended-exponent double; [`ToFpt`] holds the C++'s overloads of the first.
//! - [`Sign`] and [`GetSqrt`] are the free functions `is_pos`, `is_neg`, `is_zero` and `get_sqrt`
//!   that the predicates call on every numeric type; module `robust_fpt` adds the generic template
//!   (`that > 0`, `std::sqrt`) for the plain numbers and the overloads of `robust_fpt`.
//!
//! Signed overflow, which the C++ leaves undefined and MSVC wraps, wraps here too (the exponent
//! sums of [`ExtendedExponentFpt`]; negating `INT32_MIN` or `INT64_MIN` in the constructors).
//!
//! Left out: `extended_int`'s converting constructor and assignment from another `extended_int<M>`
//! (lines 267-271 and 302-307), which nothing instantiates (only `extended_int<64>` exists), and
//! the `typedef`s of the integer widths (lines 25-29), which are Rust's `i32`, `i64`, `u32`, `u64`
//! and `f64`.

use std::cmp::Ordering;
use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

// ---------------------------------------------------------------- ulp_comparison (31-69)

/// `ulp_comparison<fpt64>::Result`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(i32)]
pub enum UlpResult {
    /// `LESS = -1`.
    Less = -1,
    /// `EQUAL = 0`.
    Equal = 0,
    /// `MORE = 1`.
    More = 1,
}

/// `ulp_comparison<fpt64>`: compares two doubles within `maxUlps` units in the last place.
#[derive(Clone, Copy, Default, Debug)]
pub struct UlpComparison;

impl UlpComparison {
    /// `operator()(a, b, maxUlps)`.
    #[must_use]
    pub fn call(&self, a: f64, b: f64, max_ulps: u32) -> UlpResult {
        // Reinterpret double bits as 64-bit integers.
        let mut ll_a = a.to_bits();
        let mut ll_b = b.to_bits();

        // Positive 0.0 is integer zero, negative 0.0 is 0x8000000000000000: map negative zero to
        // the representation of positive zero, the smallest negative number to negative one.
        if ll_a < 0x8000_0000_0000_0000 {
            ll_a = 0x8000_0000_0000_0000 - ll_a;
        }
        if ll_b < 0x8000_0000_0000_0000 {
            ll_b = 0x8000_0000_0000_0000 - ll_b;
        }

        // Compare the integer representations.
        if ll_a > ll_b {
            return if ll_a - ll_b <= u64::from(max_ulps) {
                UlpResult::Equal
            } else {
                UlpResult::Less
            };
        }
        if ll_b - ll_a <= u64::from(max_ulps) {
            UlpResult::Equal
        } else {
            UlpResult::More
        }
    }
}

// ---------------------------------------------------------------- the sign helpers

/// The free functions `is_pos`, `is_neg` and `is_zero` of `voronoi_ctypes.hpp` and
/// `voronoi_robust_fpt.hpp`, one implementation per overload.
pub trait Sign {
    /// `is_pos(that)`.
    fn is_pos(&self) -> bool;
    /// `is_neg(that)`.
    fn is_neg(&self) -> bool;
    /// `is_zero(that)`.
    fn is_zero(&self) -> bool;
}

/// The free function `get_sqrt`, one implementation per overload.
pub trait GetSqrt {
    /// `get_sqrt(that)`.
    #[must_use]
    fn get_sqrt(&self) -> Self;
}

/// `is_pos(that)`.
pub fn is_pos<T: Sign + ?Sized>(that: &T) -> bool {
    that.is_pos()
}

/// `is_neg(that)`.
pub fn is_neg<T: Sign + ?Sized>(that: &T) -> bool {
    that.is_neg()
}

/// `is_zero(that)`.
pub fn is_zero<T: Sign + ?Sized>(that: &T) -> bool {
    that.is_zero()
}

/// `get_sqrt(that)`.
#[must_use]
pub fn get_sqrt<T: GetSqrt>(that: &T) -> T {
    that.get_sqrt()
}

// ---------------------------------------------------------------- extended_exponent_fpt (71-223)

/// `extened_exponent_fpt_traits<fpt64>::MAX_SIGNIFICANT_EXP_DIF`: the exponent gap past which a sum
/// keeps only the larger operand.
pub const MAX_SIGNIFICANT_EXP_DIF: i32 = 54;

/// `extended_exponent_fpt<double>`: `val · 2^exp` with `val` in [0.5, 1) or 0, as `frexp` leaves
/// it. It does not handle division by zero, subnormal results or NaNs, as the C++ says.
#[derive(Clone, Copy, Debug)]
pub struct ExtendedExponentFpt {
    val: f64,
    exp: i32,
}

/// `efpt64`, `extended_exponent_fpt<double>`.
pub type Efpt64 = ExtendedExponentFpt;

impl ExtendedExponentFpt {
    /// `explicit extended_exponent_fpt(val)`: `val_ = frexp(val, &exp_)`.
    #[must_use]
    pub fn new(val: f64) -> Self {
        let (val, exp) = orca_crt::frexp(val);
        Self { val, exp }
    }

    /// `extended_exponent_fpt(val, exp)`: `val_ = frexp(val, &exp_)`, then `exp_ += exp`.
    #[must_use]
    pub fn with_exp(val: f64, exp: i32) -> Self {
        let (val, e) = orca_crt::frexp(val);
        Self {
            val,
            exp: e.wrapping_add(exp),
        }
    }

    /// The member `val_` as the object holds it (the probe reads it from the object's bytes).
    #[must_use]
    pub fn val(&self) -> f64 {
        self.val
    }

    /// The member `exp_` as the object holds it.
    #[must_use]
    pub fn exp(&self) -> i32 {
        self.exp
    }

    /// `is_pos()`: `val_ > 0`.
    #[must_use]
    pub fn is_pos(&self) -> bool {
        self.val > 0.0
    }

    /// `is_neg()`: `val_ < 0`.
    #[must_use]
    pub fn is_neg(&self) -> bool {
        self.val < 0.0
    }

    /// `is_zero()`: `val_ == 0`.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.val == 0.0
    }

    /// `sqrt()`: an odd exponent is made even by doubling the mantissa, then halved with `>>`
    /// (MSVC's arithmetic shift).
    #[must_use]
    pub fn sqrt(&self) -> Self {
        let mut val = self.val;
        let mut exp = self.exp;
        if exp & 1 != 0 {
            val *= 2.0;
            exp = exp.wrapping_sub(1);
        }
        Self::with_exp(orca_crt::sqrt(val), exp >> 1)
    }

    /// `d()`: `ldexp(val_, exp_)`.
    #[must_use]
    pub fn d(&self) -> f64 {
        orca_crt::ldexp(self.val, self.exp)
    }
}

impl Neg for ExtendedExponentFpt {
    type Output = Self;
    /// `operator-()`: `extended_exponent_fpt(-val_, exp_)`.
    fn neg(self) -> Self {
        Self::with_exp(-self.val, self.exp)
    }
}

impl Add for ExtendedExponentFpt {
    type Output = Self;
    /// `operator+`.
    fn add(self, that: Self) -> Self {
        if self.val == 0.0 || that.exp > self.exp.wrapping_add(MAX_SIGNIFICANT_EXP_DIF) {
            return that;
        }
        if that.val == 0.0 || self.exp > that.exp.wrapping_add(MAX_SIGNIFICANT_EXP_DIF) {
            return self;
        }
        if self.exp >= that.exp {
            let exp_dif = self.exp.wrapping_sub(that.exp);
            let val = orca_crt::ldexp(self.val, exp_dif) + that.val;
            Self::with_exp(val, that.exp)
        } else {
            let exp_dif = that.exp.wrapping_sub(self.exp);
            let val = orca_crt::ldexp(that.val, exp_dif) + self.val;
            Self::with_exp(val, self.exp)
        }
    }
}

impl Sub for ExtendedExponentFpt {
    type Output = Self;
    /// `operator-(that)`.
    fn sub(self, that: Self) -> Self {
        if self.val == 0.0 || that.exp > self.exp.wrapping_add(MAX_SIGNIFICANT_EXP_DIF) {
            return Self::with_exp(-that.val, that.exp);
        }
        if that.val == 0.0 || self.exp > that.exp.wrapping_add(MAX_SIGNIFICANT_EXP_DIF) {
            return self;
        }
        if self.exp >= that.exp {
            let exp_dif = self.exp.wrapping_sub(that.exp);
            let val = orca_crt::ldexp(self.val, exp_dif) - that.val;
            Self::with_exp(val, that.exp)
        } else {
            let exp_dif = that.exp.wrapping_sub(self.exp);
            let val = orca_crt::ldexp(-that.val, exp_dif) + self.val;
            Self::with_exp(val, self.exp)
        }
    }
}

impl Mul for ExtendedExponentFpt {
    type Output = Self;
    /// `operator*`.
    fn mul(self, that: Self) -> Self {
        let val = self.val * that.val;
        let exp = self.exp.wrapping_add(that.exp);
        Self::with_exp(val, exp)
    }
}

impl Div for ExtendedExponentFpt {
    type Output = Self;
    /// `operator/`.
    fn div(self, that: Self) -> Self {
        let val = self.val / that.val;
        let exp = self.exp.wrapping_sub(that.exp);
        Self::with_exp(val, exp)
    }
}

impl AddAssign for ExtendedExponentFpt {
    /// `operator+=`: `*this = *this + that`.
    fn add_assign(&mut self, that: Self) {
        *self = *self + that;
    }
}

impl SubAssign for ExtendedExponentFpt {
    /// `operator-=`: `*this = *this - that`.
    fn sub_assign(&mut self, that: Self) {
        *self = *self - that;
    }
}

impl MulAssign for ExtendedExponentFpt {
    /// `operator*=`: `*this = *this * that`.
    fn mul_assign(&mut self, that: Self) {
        *self = *self * that;
    }
}

impl DivAssign for ExtendedExponentFpt {
    /// `operator/=`: `*this = *this / that`.
    fn div_assign(&mut self, that: Self) {
        *self = *self / that;
    }
}

impl GetSqrt for ExtendedExponentFpt {
    /// `get_sqrt(const extended_exponent_fpt&)`: `that.sqrt()`.
    fn get_sqrt(&self) -> Self {
        self.sqrt()
    }
}

impl Sign for ExtendedExponentFpt {
    fn is_pos(&self) -> bool {
        ExtendedExponentFpt::is_pos(self)
    }
    fn is_neg(&self) -> bool {
        ExtendedExponentFpt::is_neg(self)
    }
    fn is_zero(&self) -> bool {
        ExtendedExponentFpt::is_zero(self)
    }
}

// ---------------------------------------------------------------- extended_int (225-595)

/// `extended_int<N>`: a stack-allocated big integer of up to N 32-bit chunks, with `+`, `-` and
/// `*`.
#[derive(Clone, Copy, Debug)]
pub struct ExtendedInt<const N: usize> {
    chunks: [u32; N],
    count: i32,
}

/// `voronoi_ctype_traits<int32>::big_int_type`, `extended_int<64>`.
pub type BigInt = ExtendedInt<64>;

impl<const N: usize> Default for ExtendedInt<N> {
    /// `extended_int()`: the C++ leaves the count and the chunks unset, and every use assigns the
    /// value before reading it; here it is zero.
    fn default() -> Self {
        Self {
            chunks: [0; N],
            count: 0,
        }
    }
}

impl<const N: usize> ExtendedInt<N> {
    /// `extended_int(int32 that)`.
    #[must_use]
    pub fn from_i32(that: i32) -> Self {
        let mut v = Self::default();
        v.assign_i32(that);
        v
    }

    /// `extended_int(int64 that)`.
    #[must_use]
    pub fn from_i64(that: i64) -> Self {
        let mut v = Self::default();
        v.assign_i64(that);
        v
    }

    /// `extended_int(const std::vector<uint32>& chunks, bool plus = true)`: `chunks` highest
    /// first; the lowest N are kept, leading zero chunks included.
    #[must_use]
    pub fn from_chunks(chunks: &[u32], plus: bool) -> Self {
        let mut v = Self::default();
        // (std::min)(N, chunks.size()), at most 64 here.
        let n = if N < chunks.len() { N } else { chunks.len() };
        v.count = n as i32;
        for i in 0..n {
            v.chunks[i] = chunks[chunks.len() - i - 1];
        }
        if !plus {
            v.count = -v.count;
        }
        v
    }

    /// `operator=(int32 that)`: sets the count and the lowest chunk, leaving the others.
    pub fn assign_i32(&mut self, that: i32) {
        if that > 0 {
            self.chunks[0] = that as u32;
            self.count = 1;
        } else if that < 0 {
            self.chunks[0] = that.wrapping_neg() as u32;
            self.count = -1;
        } else {
            self.count = 0;
        }
    }

    /// `operator=(int64 that)`: sets the count and the two lowest chunks, leaving the others.
    pub fn assign_i64(&mut self, that: i64) {
        if that > 0 {
            self.chunks[0] = that as u32;
            self.chunks[1] = (that >> 32) as u32;
            self.count = if self.chunks[1] != 0 { 2 } else { 1 };
        } else if that < 0 {
            let that = that.wrapping_neg();
            self.chunks[0] = that as u32;
            self.chunks[1] = (that >> 32) as u32;
            self.count = if self.chunks[1] != 0 { -2 } else { -1 };
        } else {
            self.count = 0;
        }
    }

    /// `is_pos()`: `count_ > 0`.
    #[must_use]
    pub fn is_pos(&self) -> bool {
        self.count > 0
    }

    /// `is_neg()`: `count_ < 0`.
    #[must_use]
    pub fn is_neg(&self) -> bool {
        self.count < 0
    }

    /// `is_zero()`: `count_ == 0`.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.count == 0
    }

    /// `neg()`: negates the count in place.
    pub fn neg_in_place(&mut self) {
        self.count = -self.count;
    }

    /// `add(e1, e2)`: `*this = e1 + e2`.
    pub fn set_add(&mut self, e1: &Self, e2: &Self) {
        if e1.count() == 0 {
            *self = *e2;
            return;
        }
        if e2.count() == 0 {
            *self = *e1;
            return;
        }
        if (e1.count() > 0) ^ (e2.count() > 0) {
            self.dif_chunks(e1.chunks(), e1.size(), e2.chunks(), e2.size(), false);
        } else {
            self.add_chunks(e1.chunks(), e1.size(), e2.chunks(), e2.size());
        }
        if e1.count() < 0 {
            self.count = -self.count;
        }
    }

    /// `dif(e1, e2)`: `*this = e1 - e2`.
    pub fn set_dif(&mut self, e1: &Self, e2: &Self) {
        if e1.count() == 0 {
            *self = *e2;
            self.count = -self.count;
            return;
        }
        if e2.count() == 0 {
            *self = *e1;
            return;
        }
        if (e1.count() > 0) ^ (e2.count() > 0) {
            self.add_chunks(e1.chunks(), e1.size(), e2.chunks(), e2.size());
        } else {
            self.dif_chunks(e1.chunks(), e1.size(), e2.chunks(), e2.size(), false);
        }
        if e1.count() < 0 {
            self.count = -self.count;
        }
    }

    /// `mul(e1, e2)`: `*this = e1 * e2`.
    pub fn set_mul(&mut self, e1: &Self, e2: &Self) {
        if e1.count() == 0 || e2.count() == 0 {
            self.count = 0;
            return;
        }
        self.mul_chunks(e1.chunks(), e1.size(), e2.chunks(), e2.size());
        if (e1.count() > 0) ^ (e2.count() > 0) {
            self.count = -self.count;
        }
    }

    /// `chunks()`: all N chunks, lowest first; those past [`size`](Self::size) are not the
    /// number's.
    #[must_use]
    pub fn chunks(&self) -> &[u32] {
        &self.chunks
    }

    /// `count()`: the sign is the number's, the size the number of chunks in use.
    #[must_use]
    pub fn count(&self) -> i32 {
        self.count
    }

    /// `size()`: `std::abs(count_)`.
    #[must_use]
    pub fn size(&self) -> usize {
        self.count.unsigned_abs() as usize
    }

    /// `p()`: a double and a binary exponent; for three chunks or more, the top three chunks and
    /// `(size - 3) · 32`.
    #[must_use]
    pub fn p(&self) -> (f64, i32) {
        let mut ret_val: (f64, i32) = (0.0, 0);
        let sz = self.size();
        if sz == 0 {
            return ret_val;
        } else if sz == 1 {
            ret_val.0 = f64::from(self.chunks[0]);
        } else if sz == 2 {
            ret_val.0 =
                f64::from(self.chunks[1]) * (0x1_0000_0000_i64 as f64) + f64::from(self.chunks[0]);
        } else {
            for i in 1..=3 {
                ret_val.0 *= 0x1_0000_0000_i64 as f64;
                ret_val.0 += f64::from(self.chunks[sz - i]);
            }
            ret_val.1 = ((sz - 3) << 5) as i32;
        }
        if self.count < 0 {
            ret_val.0 = -ret_val.0;
        }
        ret_val
    }

    /// `d()`: `ldexp(p.first, p.second)`.
    #[must_use]
    pub fn d(&self) -> f64 {
        let p = self.p();
        orca_crt::ldexp(p.0, p.1)
    }

    /// The private `add(c1, sz1, c2, sz2)`: the magnitudes' sum, the carry dropped at N chunks.
    #[allow(clippy::needless_range_loop)] // the C++'s index loops over two arrays and the result
    fn add_chunks(&mut self, c1: &[u32], sz1: usize, c2: &[u32], sz2: usize) {
        if sz1 < sz2 {
            self.add_chunks(c2, sz2, c1, sz1);
            return;
        }
        self.count = sz1 as i32;
        let mut temp: u64 = 0;
        for i in 0..sz2 {
            temp += u64::from(c1[i]) + u64::from(c2[i]);
            self.chunks[i] = temp as u32;
            temp >>= 32;
        }
        for i in sz2..sz1 {
            temp += u64::from(c1[i]);
            self.chunks[i] = temp as u32;
            temp >>= 32;
        }
        if temp != 0 && self.count as usize != N {
            self.chunks[self.count as usize] = temp as u32;
            self.count += 1;
        }
    }

    /// The private `dif(c1, sz1, c2, sz2, rec)`: the magnitudes' difference with its sign; only
    /// the top chunk of the result is checked for zero.
    #[allow(clippy::needless_range_loop)] // the C++'s index loops over two arrays and the result
    fn dif_chunks(&mut self, c1: &[u32], mut sz1: usize, c2: &[u32], mut sz2: usize, rec: bool) {
        if sz1 < sz2 {
            self.dif_chunks(c2, sz2, c1, sz1, true);
            self.count = -self.count;
            return;
        } else if sz1 == sz2 && !rec {
            loop {
                sz1 -= 1;
                if c1[sz1] < c2[sz1] {
                    sz1 += 1;
                    self.dif_chunks(c2, sz1, c1, sz1, true);
                    self.count = -self.count;
                    return;
                } else if c1[sz1] > c2[sz1] {
                    sz1 += 1;
                    break;
                }
                if sz1 == 0 {
                    break;
                }
            }
            if sz1 == 0 {
                self.count = 0;
                return;
            }
            sz2 = sz1;
        }
        self.count = (sz1 - 1) as i32;
        let mut flag = false;
        for i in 0..sz2 {
            self.chunks[i] = c1[i]
                .wrapping_sub(c2[i])
                .wrapping_sub(if flag { 1 } else { 0 });
            flag = (c1[i] < c2[i]) || ((c1[i] == c2[i]) && flag);
        }
        for i in sz2..sz1 {
            self.chunks[i] = c1[i].wrapping_sub(if flag { 1 } else { 0 });
            flag = c1[i] == 0 && flag;
        }
        if self.chunks[self.count as usize] != 0 {
            self.count += 1;
        }
    }

    /// The private `mul(c1, sz1, c2, sz2)`: the magnitudes' product, cut at N chunks.
    #[allow(clippy::needless_range_loop)] // the C++'s index loops, `first` and `shift - first`
    fn mul_chunks(&mut self, c1: &[u32], sz1: usize, c2: &[u32], sz2: usize) {
        let mut cur: u64 = 0;
        let mut nxt: u64;
        let mut tmp: u64;
        // (std::min)(N, sz1 + sz2 - 1)
        let n = if sz1 + sz2 - 1 < N { sz1 + sz2 - 1 } else { N };
        self.count = n as i32;
        for shift in 0..self.count as usize {
            nxt = 0;
            for first in 0..=shift {
                if first >= sz1 {
                    break;
                }
                let second = shift - first;
                if second >= sz2 {
                    continue;
                }
                tmp = u64::from(c1[first]) * u64::from(c2[second]);
                cur += u64::from(tmp as u32);
                nxt += tmp >> 32;
            }
            self.chunks[shift] = cur as u32;
            cur = nxt + (cur >> 32);
        }
        if cur != 0 && self.count as usize != N {
            self.chunks[self.count as usize] = cur as u32;
            self.count += 1;
        }
    }

    /// `operator<`: the counts first, then the chunks from the top, the answer turned for
    /// negative numbers.
    fn less(&self, that: &Self) -> bool {
        if self.count != that.count() {
            return self.count < that.count();
        }
        let mut i = self.size();
        if i == 0 {
            return false;
        }
        loop {
            i -= 1;
            if self.chunks[i] != that.chunks()[i] {
                return (self.chunks[i] < that.chunks()[i]) ^ (self.count < 0);
            }
            if i == 0 {
                break;
            }
        }
        false
    }
}

impl<const N: usize> From<i32> for ExtendedInt<N> {
    fn from(that: i32) -> Self {
        Self::from_i32(that)
    }
}

impl<const N: usize> From<i64> for ExtendedInt<N> {
    fn from(that: i64) -> Self {
        Self::from_i64(that)
    }
}

impl<const N: usize> PartialEq for ExtendedInt<N> {
    /// `operator==`: the same count and the same chunks within it.
    fn eq(&self, that: &Self) -> bool {
        if self.count != that.count() {
            return false;
        }
        for i in 0..self.size() {
            if self.chunks[i] != that.chunks()[i] {
                return false;
            }
        }
        true
    }
}

impl<const N: usize> PartialOrd for ExtendedInt<N> {
    fn partial_cmp(&self, that: &Self) -> Option<Ordering> {
        Some(if self.less(that) {
            Ordering::Less
        } else if that.less(self) {
            Ordering::Greater
        } else {
            Ordering::Equal
        })
    }
    /// `operator<`.
    fn lt(&self, that: &Self) -> bool {
        self.less(that)
    }
    /// `operator>`: `that < *this`.
    fn gt(&self, that: &Self) -> bool {
        that.less(self)
    }
    /// `operator<=`: `!(that < *this)`.
    fn le(&self, that: &Self) -> bool {
        !that.less(self)
    }
    /// `operator>=`: `!(*this < that)`.
    fn ge(&self, that: &Self) -> bool {
        !self.less(that)
    }
}

impl<const N: usize> Neg for ExtendedInt<N> {
    type Output = Self;
    /// `operator-()`.
    fn neg(self) -> Self {
        let mut ret_val = self;
        ret_val.neg_in_place();
        ret_val
    }
}

impl<const N: usize> Add for ExtendedInt<N> {
    type Output = Self;
    /// `operator+`.
    fn add(self, that: Self) -> Self {
        let mut ret_val = Self::default();
        ret_val.set_add(&self, &that);
        ret_val
    }
}

impl<const N: usize> Sub for ExtendedInt<N> {
    type Output = Self;
    /// `operator-(that)`.
    fn sub(self, that: Self) -> Self {
        let mut ret_val = Self::default();
        ret_val.set_dif(&self, &that);
        ret_val
    }
}

impl<const N: usize> Mul for ExtendedInt<N> {
    type Output = Self;
    /// `operator*(const extended_int&)`.
    fn mul(self, that: Self) -> Self {
        let mut ret_val = Self::default();
        ret_val.set_mul(&self, &that);
        ret_val
    }
}

impl<const N: usize> Mul<i32> for ExtendedInt<N> {
    type Output = Self;
    /// `operator*(int32)`: the product with `extended_int(that)`.
    fn mul(self, that: i32) -> Self {
        let temp = Self::from_i32(that);
        self * temp
    }
}

impl<const N: usize> Mul<i64> for ExtendedInt<N> {
    type Output = Self;
    /// `operator*(int64)`: the product with `extended_int(that)`.
    fn mul(self, that: i64) -> Self {
        let temp = Self::from_i64(that);
        self * temp
    }
}

impl<const N: usize> Sign for ExtendedInt<N> {
    /// `is_pos(const extended_int&)`: `count() > 0`.
    fn is_pos(&self) -> bool {
        self.count() > 0
    }
    /// `is_neg(const extended_int&)`: `count() < 0`.
    fn is_neg(&self) -> bool {
        self.count() < 0
    }
    /// `is_zero(const extended_int&)`: `!count()`.
    fn is_zero(&self) -> bool {
        self.count() == 0
    }
}

// ---------------------------------------------------------------- the converters (597-619)

/// The overloads of `type_converter_fpt::operator()`: `static_cast<fpt64>` of a plain number,
/// `d()` of an `extended_int` or an `extended_exponent_fpt`.
pub trait ToFpt {
    /// The value as a double.
    fn to_fpt(&self) -> f64;
}

impl ToFpt for f64 {
    fn to_fpt(&self) -> f64 {
        *self
    }
}

impl ToFpt for i32 {
    fn to_fpt(&self) -> f64 {
        f64::from(*self)
    }
}

impl ToFpt for i64 {
    /// `cvtsi2sd`, correctly rounded, as Rust's `as` (`orca-crt`, module `convert`).
    fn to_fpt(&self) -> f64 {
        *self as f64
    }
}

impl ToFpt for u64 {
    /// MSVC halves a value of 2^63 or more with its low bit kept, converts and doubles it:
    /// correctly rounded, as Rust's `as` (`orca-crt`, module `convert`).
    fn to_fpt(&self) -> f64 {
        *self as f64
    }
}

impl<const N: usize> ToFpt for ExtendedInt<N> {
    fn to_fpt(&self) -> f64 {
        self.d()
    }
}

impl ToFpt for ExtendedExponentFpt {
    fn to_fpt(&self) -> f64 {
        self.d()
    }
}

/// `type_converter_fpt`: a value to `double`.
#[derive(Clone, Copy, Default, Debug)]
pub struct TypeConverterFpt;

impl TypeConverterFpt {
    /// `operator()(that)`.
    #[must_use]
    pub fn call<T: ToFpt + ?Sized>(&self, that: &T) -> f64 {
        that.to_fpt()
    }
}

/// `type_converter_efpt`: an `extended_int` to an extended-exponent double.
#[derive(Clone, Copy, Default, Debug)]
pub struct TypeConverterEfpt;

impl TypeConverterEfpt {
    /// `operator()(const extended_int<N>&)`: `extended_exponent_fpt(p.first, p.second)`.
    #[must_use]
    pub fn call<const N: usize>(&self, that: &ExtendedInt<N>) -> ExtendedExponentFpt {
        let p = that.p();
        ExtendedExponentFpt::with_exp(p.0, p.1)
    }
}

/// `voronoi_ctype_traits<int32>` (lines 621-638): the types the predicates and the builder compute
/// with for 32-bit input coordinates.
pub mod ctype_traits {
    /// `int_type`.
    pub type IntType = i32;
    /// `int_x2_type`.
    pub type IntX2Type = i64;
    /// `uint_x2_type`.
    pub type UintX2Type = u64;
    /// `big_int_type`.
    pub type BigIntType = super::ExtendedInt<64>;
    /// `fpt_type`.
    pub type FptType = f64;
    /// `efpt_type`.
    pub type EfptType = super::ExtendedExponentFpt;
    /// `ulp_cmp_type`.
    pub type UlpCmpType = super::UlpComparison;
    /// `to_fpt_converter_type`.
    pub type ToFptConverterType = super::TypeConverterFpt;
    /// `to_efpt_converter_type`.
    pub type ToEfptConverterType = super::TypeConverterEfpt;
}
