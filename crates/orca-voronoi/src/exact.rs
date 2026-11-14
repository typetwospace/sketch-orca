// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries no other project's code: written for this crate on 2026-11-14 by orca-o3-voronoi-4 from
// the definition of the signs it computes. It belongs in orca-exact, which has no exact
// arithmetic on doubles yet; it is here for VoronoiUtilsCgal's checks (module
// `voronoi_utils_cgal`).

//! The exact sign of a polynomial in `double`s: what CGAL's filtered predicates answer.
//!
//! VoronoiUtilsCgal (`Geometry/VoronoiUtilsCgal.cpp`) asks CGAL for the sign of polynomials in
//! the coordinates of Voronoi vertices and input points. Its filtered predicates evaluate the
//! polynomial in interval arithmetic and, when the interval holds zero, again in `MP_Float`, an
//! exact type; its exact kernel decides intersections exactly. Either way the answer is the exact
//! sign of the polynomial on the `double` inputs, taken as the real numbers they are. This module
//! gives that sign without CGAL:
//!
//! - [`Interval`]: a filter. Each operation rounds to nearest and widens the result by one ulp on
//!   each side (`next_down`, `next_up`), which encloses the exact result; an interval that does not
//!   hold zero gives the sign ([`Interval::certain_sign`]). Any bound that is not finite gives no
//!   answer, so an overflow never decides.
//! - [`Dyadic`]: the exact value, a signed integer of 32-bit limbs times a power of two. Every
//!   finite `double` is one, and sums, differences and products of them are exact.
//! - [`Ring`]: the operations both share, so that one generic function writes each polynomial
//!   once and is evaluated first as an [`Interval`], then, only when that holds zero, as a
//!   [`Dyadic`] ([`exact_sign`]).
//!
//! Both give the exact sign whenever they answer, so the filter changes how much work is done,
//! never the answer.

use core::cmp::Ordering;

/// The operations a polynomial predicate uses, shared by [`Interval`] and [`Dyadic`].
pub trait Ring: Sized + Clone {
    /// The number a finite `double` is.
    fn from_f64(x: f64) -> Self;
    /// `self + o`.
    #[must_use]
    fn add(&self, o: &Self) -> Self;
    /// `self - o`.
    #[must_use]
    fn sub(&self, o: &Self) -> Self;
    /// `self * o`.
    #[must_use]
    fn mul(&self, o: &Self) -> Self;
    /// `-self`.
    #[must_use]
    fn neg(&self) -> Self;
}

// ---------------------------------------------------------------- the filter

/// A closed interval `[lo, hi]` of `double`s that holds the exact value; both bounds NaN once an
/// operation met a bound that is not finite (no answer).
#[derive(Clone, Copy, Debug)]
pub struct Interval {
    lo: f64,
    hi: f64,
}

impl Interval {
    /// The interval of no answer.
    const POISON: Self = Self {
        lo: f64::NAN,
        hi: f64::NAN,
    };

    /// Whether both bounds are finite.
    fn finite(&self) -> bool {
        self.lo.is_finite() && self.hi.is_finite()
    }

    /// `[next_down(lo), next_up(hi)]` of results rounded to nearest: an enclosure of the exact
    /// values they round.
    fn widened(lo: f64, hi: f64) -> Self {
        let r = Self {
            lo: lo.next_down(),
            hi: hi.next_up(),
        };
        if r.finite() { r } else { Self::POISON }
    }

    /// The sign every value of the interval has: 1 when the interval is above zero, -1 when it is
    /// below, `None` when it holds zero or has a bound that is not finite.
    #[must_use]
    pub fn certain_sign(&self) -> Option<i32> {
        if !self.finite() {
            None
        } else if self.lo > 0.0 {
            Some(1)
        } else if self.hi < 0.0 {
            Some(-1)
        } else {
            None
        }
    }
}

/// The smaller of two finite values.
fn lesser(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// The larger of two finite values.
fn greater(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

impl Ring for Interval {
    fn from_f64(x: f64) -> Self {
        if x.is_finite() {
            Self { lo: x, hi: x }
        } else {
            Self::POISON
        }
    }

    fn add(&self, o: &Self) -> Self {
        if !self.finite() || !o.finite() {
            return Self::POISON;
        }
        Self::widened(self.lo + o.lo, self.hi + o.hi)
    }

    fn sub(&self, o: &Self) -> Self {
        if !self.finite() || !o.finite() {
            return Self::POISON;
        }
        Self::widened(self.lo - o.hi, self.hi - o.lo)
    }

    fn mul(&self, o: &Self) -> Self {
        if !self.finite() || !o.finite() {
            return Self::POISON;
        }
        let a = self.lo * o.lo;
        let b = self.lo * o.hi;
        let c = self.hi * o.lo;
        let d = self.hi * o.hi;
        Self::widened(
            lesser(lesser(a, b), lesser(c, d)),
            greater(greater(a, b), greater(c, d)),
        )
    }

    fn neg(&self) -> Self {
        Self {
            lo: -self.hi,
            hi: -self.lo,
        }
    }
}

// ---------------------------------------------------------------- the exact value

/// An exact dyadic number: `(-1)^neg * mag * 2^exp`, `mag` an integer of 32-bit limbs, lowest
/// first, with no zero limb at the top; zero has no limbs.
#[derive(Clone, Debug)]
pub struct Dyadic {
    neg: bool,
    mag: Vec<u32>,
    exp: i32,
}

impl Dyadic {
    /// Zero.
    #[must_use]
    pub fn zero() -> Self {
        Self {
            neg: false,
            mag: Vec::new(),
            exp: 0,
        }
    }

    /// A value from its parts, normalised (no zero limb at the top; zero positive).
    fn make(neg: bool, mut mag: Vec<u32>, exp: i32) -> Self {
        trim(&mut mag);
        if mag.is_empty() {
            Self::zero()
        } else {
            Self { neg, mag, exp }
        }
    }

    /// The sign: -1, 0 or 1.
    #[must_use]
    pub fn sign(&self) -> i32 {
        if self.mag.is_empty() {
            0
        } else if self.neg {
            -1
        } else {
            1
        }
    }
}

/// Drops the zero limbs at the top.
fn trim(mag: &mut Vec<u32>) {
    while mag.last() == Some(&0) {
        mag.pop();
    }
}

/// `mag << bits`.
fn shl(mag: &[u32], bits: u32) -> Vec<u32> {
    let limbs = (bits / 32) as usize;
    let shift = bits % 32;
    let mut out = vec![0u32; limbs];
    out.reserve(mag.len() + 1);
    if shift == 0 {
        out.extend_from_slice(mag);
    } else {
        let mut carry = 0u32;
        for &m in mag {
            out.push((m << shift) | carry);
            carry = m >> (32 - shift);
        }
        out.push(carry);
    }
    trim(&mut out);
    out
}

/// Compares two magnitudes with no zero limb at the top.
fn cmp_mag(a: &[u32], b: &[u32]) -> Ordering {
    if a.len() != b.len() {
        return a.len().cmp(&b.len());
    }
    for (x, y) in a.iter().rev().zip(b.iter().rev()) {
        match x.cmp(y) {
            Ordering::Equal => {}
            other => return other,
        }
    }
    Ordering::Equal
}

/// `a + b` of magnitudes.
fn add_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
    let (long, short) = if a.len() < b.len() { (b, a) } else { (a, b) };
    let mut out = Vec::with_capacity(long.len() + 1);
    let mut carry = 0u64;
    for (i, &l) in long.iter().enumerate() {
        let s = u64::from(l) + u64::from(short.get(i).copied().unwrap_or(0)) + carry;
        out.push(s as u32);
        carry = s >> 32;
    }
    if carry != 0 {
        out.push(carry as u32);
    }
    out
}

/// `a - b` of magnitudes, `a >= b`.
fn sub_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(a.len());
    let mut borrow = 0i64;
    for (i, &x) in a.iter().enumerate() {
        let mut d = i64::from(x) - i64::from(b.get(i).copied().unwrap_or(0)) - borrow;
        if d < 0 {
            d += 1i64 << 32;
            borrow = 1;
        } else {
            borrow = 0;
        }
        out.push(d as u32);
    }
    out
}

/// `a * b` of magnitudes (schoolbook).
fn mul_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut out = vec![0u32; a.len() + b.len()];
    for (i, &x) in a.iter().enumerate() {
        let mut carry = 0u64;
        for (j, &y) in b.iter().enumerate() {
            let t = u64::from(x) * u64::from(y) + u64::from(out[i + j]) + carry;
            out[i + j] = t as u32;
            carry = t >> 32;
        }
        let mut k = i + b.len();
        while carry != 0 {
            let t = u64::from(out[k]) + carry;
            out[k] = t as u32;
            carry = t >> 32;
            k += 1;
        }
    }
    out
}

impl Ring for Dyadic {
    /// The exact value of a finite `double`. Panics on an infinity or a NaN, which no caller
    /// passes (the filter answers no on them, and the callers check finiteness first).
    fn from_f64(x: f64) -> Self {
        assert!(x.is_finite(), "Dyadic::from_f64 of {x}");
        let bits = x.to_bits();
        let neg = (bits >> 63) != 0;
        let biased = ((bits >> 52) & 0x7ff) as i32;
        let frac = bits & ((1u64 << 52) - 1);
        let (mant, exp) = if biased == 0 {
            (frac, -1074)
        } else {
            (frac | (1u64 << 52), biased - 1075)
        };
        Self::make(neg, vec![mant as u32, (mant >> 32) as u32], exp)
    }

    fn add(&self, o: &Self) -> Self {
        if self.mag.is_empty() {
            return o.clone();
        }
        if o.mag.is_empty() {
            return self.clone();
        }
        let e = if self.exp < o.exp { self.exp } else { o.exp };
        let a = shl(&self.mag, (self.exp - e) as u32);
        let b = shl(&o.mag, (o.exp - e) as u32);
        if self.neg == o.neg {
            Self::make(self.neg, add_mag(&a, &b), e)
        } else {
            match cmp_mag(&a, &b) {
                Ordering::Equal => Self::zero(),
                Ordering::Greater => Self::make(self.neg, sub_mag(&a, &b), e),
                Ordering::Less => Self::make(o.neg, sub_mag(&b, &a), e),
            }
        }
    }

    fn sub(&self, o: &Self) -> Self {
        self.add(&o.neg())
    }

    fn mul(&self, o: &Self) -> Self {
        if self.mag.is_empty() || o.mag.is_empty() {
            return Self::zero();
        }
        Self::make(
            self.neg != o.neg,
            mul_mag(&self.mag, &o.mag),
            self.exp + o.exp,
        )
    }

    fn neg(&self) -> Self {
        let mut r = self.clone();
        if !r.mag.is_empty() {
            r.neg = !r.neg;
        }
        r
    }
}

/// The exact sign of a polynomial written once over [`Ring`]: `filtered` (the polynomial as an
/// [`Interval`]) when it holds no zero, else `exact` (the same polynomial as a [`Dyadic`]).
pub fn exact_sign(filtered: impl FnOnce() -> Interval, exact: impl FnOnce() -> Dyadic) -> i32 {
    match filtered().certain_sign() {
        Some(s) => s,
        None => exact().sign(),
    }
}

#[cfg(test)]
mod tests {
    use super::{Dyadic, Interval, Ring, exact_sign};

    /// `a * b - c * d` over a [`Ring`].
    fn det<N: Ring>(a: f64, b: f64, c: f64, d: f64) -> N {
        N::from_f64(a)
            .mul(&N::from_f64(b))
            .sub(&N::from_f64(c).mul(&N::from_f64(d)))
    }

    #[test]
    fn the_exact_sign_sees_what_doubles_lose() {
        // (1 + 2^-52)^2 - (1 + 2^-51) = 2^-104, which rounds to 0 in doubles.
        let e = 1.0 + f64::EPSILON;
        assert_eq!(e * e - (1.0 + 2.0 * f64::EPSILON), 0.0);
        let s = exact_sign(
            || det::<Interval>(e, e, 1.0 + 2.0 * f64::EPSILON, 1.0),
            || det::<Dyadic>(e, e, 1.0 + 2.0 * f64::EPSILON, 1.0),
        );
        assert_eq!(s, 1);
        assert_eq!(
            det::<Interval>(e, e, 1.0 + 2.0 * f64::EPSILON, 1.0).certain_sign(),
            None
        );
        assert_eq!(det::<Dyadic>(3.0, 4.0, 6.0, 2.0).sign(), 0);
        assert_eq!(det::<Dyadic>(-3.0, 4.0, 6.0, 2.0).sign(), -1);
    }

    #[test]
    fn far_apart_exponents_and_subnormals_are_exact() {
        let tiny = f64::from_bits(1); // 2^-1074
        let x = Dyadic::from_f64(1.0e300).add(&Dyadic::from_f64(tiny));
        assert_eq!(x.sub(&Dyadic::from_f64(1.0e300)).sign(), 1);
        assert_eq!(Dyadic::from_f64(-0.0).sign(), 0);
        assert_eq!(Interval::from_f64(f64::INFINITY).certain_sign(), None);
        let big = Interval::from_f64(1.0e300);
        assert_eq!(big.mul(&big).certain_sign(), None);
        assert_eq!(Interval::from_f64(2.0).neg().certain_sign(), Some(-1));
    }
}
