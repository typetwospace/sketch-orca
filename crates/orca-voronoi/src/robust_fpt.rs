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
// from boost/polygon/detail/voronoi_robust_fpt.hpp, lines 1-501 (get_sqrt, is_pos, is_neg and
// is_zero, robust_fpt, robust_dif with its free operators, and robust_sqrt_expr). Copyright Andrii
// Sydorchuk 2010-2012, under the Boost Software License, version 1.0 (BSL-1.0;
// licenses/BSL-1.0.txt).

//! Boost.Polygon's relative-error arithmetic (`voronoi_robust_fpt.hpp`), which the circle
//! formation of the Voronoi builder computes with before it falls back to exact arithmetic.
//!
//! - The free templates `get_sqrt` (`std::sqrt`), `is_pos` (`that > 0`), `is_neg` (`that < 0`)
//!   and `is_zero` (`that == 0`) for plain numbers are the [`GetSqrt`] and [`Sign`]
//!   implementations of `f64`, `i32` and `i64` here; module `ctypes` holds the overloads of the
//!   extended types.
//! - [`RobustFpt`] is `robust_fpt<double>`: a value and its relative error in machine epsilons.
//!   Each operation adds `ROUNDING_ERROR` (1). A sum of values of opposite signs (and a difference
//!   of like signs) takes `|a·ra − b·rb| / (a + b)` (resp. `+`) as its error, which divides by zero
//!   when the two cancel exactly and then holds an infinity, as the C++ does. The compound
//!   operators `*=` and `/=` add the errors in another order than `*` and `/` (`re += that.re + 1`
//!   against `re + that.re + 1`), so each keeps its own form.
//! - [`RobustDif`] is `robust_dif<robust_fpt<double>>`, the only instantiation (the lazy circle
//!   formation functor's `robust_dif_type`): two non-negative sums whose difference is the value,
//!   so that no subtraction loses precision until `dif()`.
//! - [`RobustSqrtExpr`] is `robust_sqrt_expr<extended_int<64>, extended_exponent_fpt<double>,
//!   type_converter_efpt>`, the only instantiation (`voronoi_predicates`' `robust_sqrt_expr_type`):
//!   the sum of up to four `A[i]·sqrt(B[i])`, where a sum of opposite signs is computed as a
//!   difference of squares over the difference, through its own scratch arrays `tA` and `tB`.
//!
//! Left out: `robust_dif`'s one-value constructor (lines 247-249), which compares the value with
//! `0` and so cannot be instantiated with `robust_fpt` (it has no comparison); `robust_fpt`'s
//! `ulp()` (line 98) is [`RobustFpt::re`] under its other name.

use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

use crate::ctypes::{BigInt, Efpt64, GetSqrt, Sign, TypeConverterEfpt};

// ---------------------------------------------------------------- the free templates (59-77)

impl GetSqrt for f64 {
    /// `get_sqrt(const T&)`: `std::sqrt(that)`, `sqrtsd` (`orca-crt`).
    fn get_sqrt(&self) -> Self {
        orca_crt::sqrt(*self)
    }
}

impl Sign for f64 {
    /// `is_pos(const T&)`: `that > 0`.
    fn is_pos(&self) -> bool {
        *self > 0.0
    }
    /// `is_neg(const T&)`: `that < 0`.
    fn is_neg(&self) -> bool {
        *self < 0.0
    }
    /// `is_zero(const T&)`: `that == 0` (true for `-0.0`).
    fn is_zero(&self) -> bool {
        *self == 0.0
    }
}

impl Sign for i32 {
    fn is_pos(&self) -> bool {
        *self > 0
    }
    fn is_neg(&self) -> bool {
        *self < 0
    }
    fn is_zero(&self) -> bool {
        *self == 0
    }
}

impl Sign for i64 {
    fn is_pos(&self) -> bool {
        *self > 0
    }
    fn is_neg(&self) -> bool {
        *self < 0
    }
    fn is_zero(&self) -> bool {
        *self == 0
    }
}

// ---------------------------------------------------------------- robust_fpt (79-233)

/// `robust_fpt<double>::ROUNDING_ERROR`: the rounding error of one operation, at most 1 EPS.
pub const ROUNDING_ERROR: f64 = 1.0;

/// `robust_fpt<double>`: a value and its relative error.
#[derive(Clone, Copy, Debug)]
pub struct RobustFpt {
    fpv: f64,
    re: f64,
}

impl Default for RobustFpt {
    /// `robust_fpt()`: zero with no error.
    fn default() -> Self {
        Self { fpv: 0.0, re: 0.0 }
    }
}

impl RobustFpt {
    /// `explicit robust_fpt(fpv)`: no error.
    #[must_use]
    pub fn new(fpv: f64) -> Self {
        Self { fpv, re: 0.0 }
    }

    /// `robust_fpt(fpv, error)`.
    #[must_use]
    pub fn with_error(fpv: f64, error: f64) -> Self {
        Self { fpv, re: error }
    }

    /// `fpv()`: the value.
    #[must_use]
    pub fn fpv(&self) -> f64 {
        self.fpv
    }

    /// `re()` (and `ulp()`): the relative error.
    #[must_use]
    pub fn re(&self) -> f64 {
        self.re
    }

    /// `has_pos_value()`: `is_pos(fpv_)`.
    #[must_use]
    pub fn has_pos_value(&self) -> bool {
        self.fpv.is_pos()
    }

    /// `has_neg_value()`: `is_neg(fpv_)`.
    #[must_use]
    pub fn has_neg_value(&self) -> bool {
        self.fpv.is_neg()
    }

    /// `has_zero_value()`: `is_zero(fpv_)`.
    #[must_use]
    pub fn has_zero_value(&self) -> bool {
        self.fpv.is_zero()
    }

    /// `sqrt()`: half the error, plus the rounding error.
    #[must_use]
    pub fn sqrt(&self) -> Self {
        Self::with_error(self.fpv.get_sqrt(), self.re * 0.5 + ROUNDING_ERROR)
    }
}

impl Neg for RobustFpt {
    type Output = Self;
    /// `operator-()`.
    fn neg(self) -> Self {
        Self::with_error(-self.fpv, self.re)
    }
}

impl AddAssign for RobustFpt {
    /// `operator+=`.
    fn add_assign(&mut self, that: Self) {
        let fpv = self.fpv + that.fpv;
        if (!self.fpv.is_neg() && !that.fpv.is_neg()) || (!self.fpv.is_pos() && !that.fpv.is_pos())
        {
            self.re = orca_stl::max(self.re, that.re) + ROUNDING_ERROR;
        } else {
            let mut temp = (self.fpv * self.re - that.fpv * that.re) / fpv;
            if temp.is_neg() {
                temp = -temp;
            }
            self.re = temp + ROUNDING_ERROR;
        }
        self.fpv = fpv;
    }
}

impl SubAssign for RobustFpt {
    /// `operator-=`.
    fn sub_assign(&mut self, that: Self) {
        let fpv = self.fpv - that.fpv;
        if (!self.fpv.is_neg() && !that.fpv.is_pos()) || (!self.fpv.is_pos() && !that.fpv.is_neg())
        {
            self.re = orca_stl::max(self.re, that.re) + ROUNDING_ERROR;
        } else {
            let mut temp = (self.fpv * self.re + that.fpv * that.re) / fpv;
            if temp.is_neg() {
                temp = -temp;
            }
            self.re = temp + ROUNDING_ERROR;
        }
        self.fpv = fpv;
    }
}

impl MulAssign for RobustFpt {
    /// `operator*=`: `re_ += that.re_ + ROUNDING_ERROR`, then the product.
    fn mul_assign(&mut self, that: Self) {
        self.re += that.re + ROUNDING_ERROR;
        self.fpv *= that.fpv;
    }
}

impl DivAssign for RobustFpt {
    /// `operator/=`: `re_ += that.re_ + ROUNDING_ERROR`, then the quotient.
    fn div_assign(&mut self, that: Self) {
        self.re += that.re + ROUNDING_ERROR;
        self.fpv /= that.fpv;
    }
}

impl Add for RobustFpt {
    type Output = Self;
    /// `operator+`.
    fn add(self, that: Self) -> Self {
        let fpv = self.fpv + that.fpv;
        let re = if (!self.fpv.is_neg() && !that.fpv.is_neg())
            || (!self.fpv.is_pos() && !that.fpv.is_pos())
        {
            orca_stl::max(self.re, that.re) + ROUNDING_ERROR
        } else {
            let mut temp = (self.fpv * self.re - that.fpv * that.re) / fpv;
            if temp.is_neg() {
                temp = -temp;
            }
            temp + ROUNDING_ERROR
        };
        Self::with_error(fpv, re)
    }
}

impl Sub for RobustFpt {
    type Output = Self;
    /// `operator-(that)`.
    fn sub(self, that: Self) -> Self {
        let fpv = self.fpv - that.fpv;
        let re = if (!self.fpv.is_neg() && !that.fpv.is_pos())
            || (!self.fpv.is_pos() && !that.fpv.is_neg())
        {
            orca_stl::max(self.re, that.re) + ROUNDING_ERROR
        } else {
            let mut temp = (self.fpv * self.re + that.fpv * that.re) / fpv;
            if temp.is_neg() {
                temp = -temp;
            }
            temp + ROUNDING_ERROR
        };
        Self::with_error(fpv, re)
    }
}

impl Mul for RobustFpt {
    type Output = Self;
    /// `operator*`: `re_ + that.re_ + ROUNDING_ERROR`.
    fn mul(self, that: Self) -> Self {
        let fpv = self.fpv * that.fpv;
        let re = self.re + that.re + ROUNDING_ERROR;
        Self::with_error(fpv, re)
    }
}

impl Div for RobustFpt {
    type Output = Self;
    /// `operator/`: `re_ + that.re_ + ROUNDING_ERROR`.
    fn div(self, that: Self) -> Self {
        let fpv = self.fpv / that.fpv;
        let re = self.re + that.re + ROUNDING_ERROR;
        Self::with_error(fpv, re)
    }
}

impl GetSqrt for RobustFpt {
    /// `get_sqrt(const robust_fpt&)`: `that.sqrt()`.
    fn get_sqrt(&self) -> Self {
        self.sqrt()
    }
}

impl Sign for RobustFpt {
    /// `is_pos(const robust_fpt&)`: `has_pos_value()`.
    fn is_pos(&self) -> bool {
        self.has_pos_value()
    }
    /// `is_neg(const robust_fpt&)`: `has_neg_value()`.
    fn is_neg(&self) -> bool {
        self.has_neg_value()
    }
    /// `is_zero(const robust_fpt&)`: `has_zero_value()`.
    fn is_zero(&self) -> bool {
        self.has_zero_value()
    }
}

// ---------------------------------------------------------------- robust_dif (235-423)

/// `robust_dif<robust_fpt<double>>`: `positive_sum − negative_sum`, both kept non-negative by the
/// operations (a negative operand goes to the other sum).
#[derive(Clone, Copy, Debug, Default)]
pub struct RobustDif {
    positive_sum: RobustFpt,
    negative_sum: RobustFpt,
}

impl RobustDif {
    /// `robust_dif()`: both sums `robust_fpt(0)`, the value [`RobustFpt::default`] also holds.
    #[must_use]
    pub fn new() -> Self {
        Self {
            positive_sum: RobustFpt::new(0.0),
            negative_sum: RobustFpt::new(0.0),
        }
    }

    /// `robust_dif(pos, neg)`.
    #[must_use]
    pub fn from_sums(pos: RobustFpt, neg: RobustFpt) -> Self {
        Self {
            positive_sum: pos,
            negative_sum: neg,
        }
    }

    /// `dif()`: `positive_sum_ - negative_sum_`.
    #[must_use]
    pub fn dif(&self) -> RobustFpt {
        self.positive_sum - self.negative_sum
    }

    /// `pos()`: the positive sum.
    #[must_use]
    pub fn pos(&self) -> RobustFpt {
        self.positive_sum
    }

    /// `neg()`: the negative sum.
    #[must_use]
    pub fn neg_sum(&self) -> RobustFpt {
        self.negative_sum
    }

    /// The private `swap()`.
    fn swap(&mut self) {
        std::mem::swap(&mut self.positive_sum, &mut self.negative_sum);
    }
}

impl Neg for RobustDif {
    type Output = Self;
    /// `operator-()`: the sums exchanged.
    fn neg(self) -> Self {
        Self::from_sums(self.negative_sum, self.positive_sum)
    }
}

impl AddAssign<RobustFpt> for RobustDif {
    /// `operator+=(const T&)`.
    fn add_assign(&mut self, val: RobustFpt) {
        if !val.is_neg() {
            self.positive_sum += val;
        } else {
            self.negative_sum -= val;
        }
    }
}

impl AddAssign for RobustDif {
    /// `operator+=(const robust_dif&)`.
    fn add_assign(&mut self, that: Self) {
        self.positive_sum += that.positive_sum;
        self.negative_sum += that.negative_sum;
    }
}

impl SubAssign<RobustFpt> for RobustDif {
    /// `operator-=(const T&)`.
    fn sub_assign(&mut self, val: RobustFpt) {
        if !val.is_neg() {
            self.negative_sum += val;
        } else {
            self.positive_sum -= val;
        }
    }
}

impl SubAssign for RobustDif {
    /// `operator-=(const robust_dif&)`.
    fn sub_assign(&mut self, that: Self) {
        self.positive_sum += that.negative_sum;
        self.negative_sum += that.positive_sum;
    }
}

impl MulAssign<RobustFpt> for RobustDif {
    /// `operator*=(const T&)`: a negative factor multiplies by its negation and exchanges the
    /// sums.
    fn mul_assign(&mut self, val: RobustFpt) {
        if !val.is_neg() {
            self.positive_sum *= val;
            self.negative_sum *= val;
        } else {
            self.positive_sum *= -val;
            self.negative_sum *= -val;
            self.swap();
        }
    }
}

impl MulAssign for RobustDif {
    /// `operator*=(const robust_dif&)`.
    fn mul_assign(&mut self, that: Self) {
        let positive_sum =
            self.positive_sum * that.positive_sum + self.negative_sum * that.negative_sum;
        let negative_sum =
            self.positive_sum * that.negative_sum + self.negative_sum * that.positive_sum;
        self.positive_sum = positive_sum;
        self.negative_sum = negative_sum;
    }
}

impl DivAssign<RobustFpt> for RobustDif {
    /// `operator/=(const T&)`: a negative divisor divides by its negation and exchanges the sums.
    fn div_assign(&mut self, val: RobustFpt) {
        if !val.is_neg() {
            self.positive_sum /= val;
            self.negative_sum /= val;
        } else {
            self.positive_sum /= -val;
            self.negative_sum /= -val;
            self.swap();
        }
    }
}

impl Add for RobustDif {
    type Output = Self;
    /// `operator+(const robust_dif&, const robust_dif&)`.
    fn add(self, rhs: Self) -> Self {
        Self::from_sums(self.pos() + rhs.pos(), self.neg_sum() + rhs.neg_sum())
    }
}

impl Add<RobustFpt> for RobustDif {
    type Output = Self;
    /// `operator+(const robust_dif&, const T&)`.
    fn add(self, rhs: RobustFpt) -> Self {
        if !rhs.is_neg() {
            Self::from_sums(self.pos() + rhs, self.neg_sum())
        } else {
            Self::from_sums(self.pos(), self.neg_sum() - rhs)
        }
    }
}

impl Add<RobustDif> for RobustFpt {
    type Output = RobustDif;
    /// `operator+(const T&, const robust_dif&)`.
    fn add(self, rhs: RobustDif) -> RobustDif {
        if !self.is_neg() {
            RobustDif::from_sums(self + rhs.pos(), rhs.neg_sum())
        } else {
            RobustDif::from_sums(rhs.pos(), rhs.neg_sum() - self)
        }
    }
}

impl Sub for RobustDif {
    type Output = Self;
    /// `operator-(const robust_dif&, const robust_dif&)`.
    fn sub(self, rhs: Self) -> Self {
        Self::from_sums(self.pos() + rhs.neg_sum(), self.neg_sum() + rhs.pos())
    }
}

impl Sub<RobustFpt> for RobustDif {
    type Output = Self;
    /// `operator-(const robust_dif&, const T&)`.
    fn sub(self, rhs: RobustFpt) -> Self {
        if !rhs.is_neg() {
            Self::from_sums(self.pos(), self.neg_sum() + rhs)
        } else {
            Self::from_sums(self.pos() - rhs, self.neg_sum())
        }
    }
}

impl Sub<RobustDif> for RobustFpt {
    type Output = RobustDif;
    /// `operator-(const T&, const robust_dif&)`.
    fn sub(self, rhs: RobustDif) -> RobustDif {
        if !self.is_neg() {
            RobustDif::from_sums(self + rhs.neg_sum(), rhs.pos())
        } else {
            RobustDif::from_sums(rhs.neg_sum(), rhs.pos() - self)
        }
    }
}

impl Mul for RobustDif {
    type Output = Self;
    /// `operator*(const robust_dif&, const robust_dif&)`.
    fn mul(self, rhs: Self) -> Self {
        let res_pos = self.pos() * rhs.pos() + self.neg_sum() * rhs.neg_sum();
        let res_neg = self.pos() * rhs.neg_sum() + self.neg_sum() * rhs.pos();
        Self::from_sums(res_pos, res_neg)
    }
}

impl Mul<RobustFpt> for RobustDif {
    type Output = Self;
    /// `operator*(const robust_dif&, const T&)`: for a negative factor, `-lhs.neg() * val` and
    /// `-lhs.pos() * val`.
    fn mul(self, val: RobustFpt) -> Self {
        if !val.is_neg() {
            Self::from_sums(self.pos() * val, self.neg_sum() * val)
        } else {
            Self::from_sums(-self.neg_sum() * val, -self.pos() * val)
        }
    }
}

impl Mul<RobustDif> for RobustFpt {
    type Output = RobustDif;
    /// `operator*(const T&, const robust_dif&)`: for a negative factor, `-val * rhs.neg()` and
    /// `-val * rhs.pos()`.
    fn mul(self, rhs: RobustDif) -> RobustDif {
        if !self.is_neg() {
            RobustDif::from_sums(self * rhs.pos(), self * rhs.neg_sum())
        } else {
            RobustDif::from_sums(-self * rhs.neg_sum(), -self * rhs.pos())
        }
    }
}

impl Div<RobustFpt> for RobustDif {
    type Output = Self;
    /// `operator/(const robust_dif&, const T&)`: for a negative divisor, `-lhs.neg() / val` and
    /// `-lhs.pos() / val`.
    fn div(self, val: RobustFpt) -> Self {
        if !val.is_neg() {
            Self::from_sums(self.pos() / val, self.neg_sum() / val)
        } else {
            Self::from_sums(-self.neg_sum() / val, -self.pos() / val)
        }
    }
}

// ---------------------------------------------------------------- robust_sqrt_expr (425-496)

/// `robust_sqrt_expr<extended_int<64>, extended_exponent_fpt<double>, type_converter_efpt>`:
/// evaluates `sum(i = 1 .. n)(A[i] * sqrt(B[i]))` for `1 <= n <= 4`.
#[derive(Clone, Copy, Debug, Default)]
pub struct RobustSqrtExpr {
    t_a: [BigInt; 5],
    t_b: [BigInt; 5],
    convert: TypeConverterEfpt,
}

impl RobustSqrtExpr {
    /// `MAX_RELATIVE_ERROR_EVAL1`.
    pub const MAX_RELATIVE_ERROR_EVAL1: i32 = 4;
    /// `MAX_RELATIVE_ERROR_EVAL2`.
    pub const MAX_RELATIVE_ERROR_EVAL2: i32 = 7;
    /// `MAX_RELATIVE_ERROR_EVAL3`.
    pub const MAX_RELATIVE_ERROR_EVAL3: i32 = 16;
    /// `MAX_RELATIVE_ERROR_EVAL4`.
    pub const MAX_RELATIVE_ERROR_EVAL4: i32 = 25;

    /// A fresh evaluator (the C++ object's scratch arrays start unset; every evaluation writes
    /// the entries it reads).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `eval1(A, B)` (re = 4 EPS): `A[0] * sqrt(B[0])`.
    pub fn eval1(&mut self, a: &[BigInt], b: &[BigInt]) -> Efpt64 {
        let a = self.convert.call(&a[0]);
        let b = self.convert.call(&b[0]);
        a * b.get_sqrt()
    }

    /// `eval2(A, B)` (re = 7 EPS): `A[0] * sqrt(B[0]) + A[1] * sqrt(B[1])`.
    pub fn eval2(&mut self, a_in: &[BigInt], b_in: &[BigInt]) -> Efpt64 {
        let a = self.eval1(a_in, b_in);
        let b = self.eval1(&a_in[1..], &b_in[1..]);
        if (!a.is_neg() && !b.is_neg()) || (!a.is_pos() && !b.is_pos()) {
            return a + b;
        }
        self.convert
            .call(&(a_in[0] * a_in[0] * b_in[0] - a_in[1] * a_in[1] * b_in[1]))
            / (a - b)
    }

    /// `eval3(A, B)` (re = 16 EPS): `A[0] * sqrt(B[0]) + A[1] * sqrt(B[1]) + A[2] * sqrt(B[2])`.
    pub fn eval3(&mut self, a_in: &[BigInt], b_in: &[BigInt]) -> Efpt64 {
        let a = self.eval2(a_in, b_in);
        let b = self.eval1(&a_in[2..], &b_in[2..]);
        if (!a.is_neg() && !b.is_neg()) || (!a.is_pos() && !b.is_pos()) {
            return a + b;
        }
        self.t_a[3] =
            a_in[0] * a_in[0] * b_in[0] + a_in[1] * a_in[1] * b_in[1] - a_in[2] * a_in[2] * b_in[2];
        self.t_b[3].assign_i32(1);
        self.t_a[4] = a_in[0] * a_in[1] * 2_i32;
        self.t_b[4] = b_in[0] * b_in[1];
        let (t_a, t_b) = (self.t_a, self.t_b);
        self.eval2(&t_a[3..], &t_b[3..]) / (a - b)
    }

    /// `eval4(A, B)` (re = 25 EPS): `A[0] * sqrt(B[0]) + A[1] * sqrt(B[1]) + A[2] * sqrt(B[2]) +
    /// A[3] * sqrt(B[3])`.
    pub fn eval4(&mut self, a_in: &[BigInt], b_in: &[BigInt]) -> Efpt64 {
        let a = self.eval2(a_in, b_in);
        let b = self.eval2(&a_in[2..], &b_in[2..]);
        if (!a.is_neg() && !b.is_neg()) || (!a.is_pos() && !b.is_pos()) {
            return a + b;
        }
        self.t_a[0] = a_in[0] * a_in[0] * b_in[0] + a_in[1] * a_in[1] * b_in[1]
            - a_in[2] * a_in[2] * b_in[2]
            - a_in[3] * a_in[3] * b_in[3];
        self.t_b[0].assign_i32(1);
        self.t_a[1] = a_in[0] * a_in[1] * 2_i32;
        self.t_b[1] = b_in[0] * b_in[1];
        self.t_a[2] = a_in[2] * a_in[3] * -2_i32;
        self.t_b[2] = b_in[2] * b_in[3];
        let (t_a, t_b) = (self.t_a, self.t_b);
        self.eval3(&t_a, &t_b) / (a - b)
    }
}
