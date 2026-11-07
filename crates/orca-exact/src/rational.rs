// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Boost code: class boost::rational of Boost 1.84.0's boost/rational.hpp, translated on
// 2026-11-07: the constructors, normalize, the arithmetic assignment operators (with a rational
// and with an integer), the comparisons, unary minus and rational_cast. (C) Copyright Paul Moore
// 1999, with the changes of the Boost contributors its revision history names, under the file's
// own terms (like the MIT and BSD licences, which ask for the notice in all copies; NOTICE quotes
// it): "Permission to copy, use, modify, sell and distribute this software is granted provided
// this copyright notice appears in all copies. This software is provided "as is" without express
// or implied warranty, and with no claim as to its suitability for any purpose."

//! `boost::rational<IntType>` over any [`ExactInt`], as Boost 1.84.0 computes it.
//!
//! - A value constructed from one integer is `n/1`, unnormalised work; from two it is normalised:
//!   a zero denominator throws `bad_rational`, zero becomes `0/1`, numerator and denominator are
//!   divided by their gcd and the denominator made positive.
//! - The rational operators keep the normal form with the gcd tricks of rational.hpp (`+=`
//!   divides by `gcd(den, r_den)` first, `*=` cross-cancels), while `+= i` and `-= i` with an
//!   integer only add `i * den` to the numerator. Every step is one `IntType` operation in the
//!   C++ order, so an `IntType` that wraps (int128_t) wraps here at the same step.
//! - `<` between rationals expands both into continued fractions and compares the terms, which
//!   never multiplies; `<` against an integer compares the floor of the quotient. The asserts
//!   of a positive denominator are off in the release build and are left out.
//! - The comparisons and `rational_cast` throw only through `IntType`'s division by zero.

use crate::{ExactError, ExactInt};
use std::fmt;

/// `boost::rational<T>`: a numerator and a denominator, normalised as Boost leaves them.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rational<T> {
    num: T,
    den: T,
}

impl<T: ExactInt> fmt::Debug for Rational<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}/{:?}", self.num, self.den)
    }
}

impl<T: ExactInt> Default for Rational<T> {
    /// `rational()`: 0/1.
    fn default() -> Rational<T> {
        Rational {
            num: T::zero(),
            den: T::one(),
        }
    }
}

impl<T: ExactInt> Rational<T> {
    /// `rational(const T& n)`: `n/1`, not normalised (nothing to do).
    pub fn from_int(n: T) -> Rational<T> {
        Rational {
            num: n,
            den: T::one(),
        }
    }

    /// `rational(n, d)`: normalised, or `bad_rational` for a zero denominator.
    pub fn new(n: T, d: T) -> Result<Rational<T>, ExactError> {
        let mut r = Rational { num: n, den: d };
        r.normalize()?;
        Ok(r)
    }

    /// The numerator and denominator as stored, without normalising (for records and tests).
    pub fn from_parts(num: T, den: T) -> Rational<T> {
        Rational { num, den }
    }

    /// `numerator()`.
    pub fn numerator(&self) -> T {
        self.num
    }

    /// `denominator()`.
    pub fn denominator(&self) -> T {
        self.den
    }

    /// `normalize()`.
    fn normalize(&mut self) -> Result<(), ExactError> {
        let zero = T::zero();
        if self.den == zero {
            return Err(ExactError::BadRational(ExactError::ZERO_DENOMINATOR));
        }
        if self.num == zero {
            self.den = T::one();
            return Ok(());
        }
        let g = self.num.gcd(self.den)?;
        self.num = self.num.div(g)?;
        self.den = self.den.div(g)?;
        if self.den < T::max_value().neg() {
            return Err(ExactError::BadRational(ExactError::SINGULAR_DENOMINATOR));
        }
        if self.den < zero {
            self.num = self.num.neg();
            self.den = self.den.neg();
        }
        Ok(())
    }

    /// `operator+=(const rational&)`.
    #[allow(clippy::should_implement_trait)] // fallible, as Boost's operators throw
    pub fn add(self, r: Rational<T>) -> Result<Rational<T>, ExactError> {
        let (mut num, mut den) = (self.num, self.den);
        let (r_num, r_den) = (r.num, r.den);
        let mut g = den.gcd(r_den)?;
        den = den.div(g)?;
        num = num.mul(r_den.div(g)?).add(r_num.mul(den));
        g = num.gcd(g)?;
        num = num.div(g)?;
        den = den.mul(r_den.div(g)?);
        Ok(Rational { num, den })
    }

    /// `operator-=(const rational&)`.
    #[allow(clippy::should_implement_trait)] // fallible, as Boost's operators throw
    pub fn sub(self, r: Rational<T>) -> Result<Rational<T>, ExactError> {
        let (mut num, mut den) = (self.num, self.den);
        let (r_num, r_den) = (r.num, r.den);
        let mut g = den.gcd(r_den)?;
        den = den.div(g)?;
        num = num.mul(r_den.div(g)?).sub(r_num.mul(den));
        g = num.gcd(g)?;
        num = num.div(g)?;
        den = den.mul(r_den.div(g)?);
        Ok(Rational { num, den })
    }

    /// `operator*=(const rational&)`.
    #[allow(clippy::should_implement_trait)] // fallible, as Boost's operators throw
    pub fn mul(self, r: Rational<T>) -> Result<Rational<T>, ExactError> {
        let (r_num, r_den) = (r.num, r.den);
        let gcd1 = self.num.gcd(r_den)?;
        let gcd2 = r_num.gcd(self.den)?;
        let num = self.num.div(gcd1)?.mul(r_num.div(gcd2)?);
        let den = self.den.div(gcd2)?.mul(r_den.div(gcd1)?);
        Ok(Rational { num, den })
    }

    /// `operator/=(const rational&)`: `bad_rational` for a zero divisor.
    #[allow(clippy::should_implement_trait)] // fallible, as Boost's operators throw
    pub fn div(self, r: Rational<T>) -> Result<Rational<T>, ExactError> {
        let (r_num, r_den) = (r.num, r.den);
        let zero = T::zero();
        if r_num == zero {
            return Err(ExactError::BadRational(ExactError::ZERO_DENOMINATOR));
        }
        if self.num == zero {
            return Ok(self);
        }
        let gcd1 = self.num.gcd(r_num)?;
        let gcd2 = r_den.gcd(self.den)?;
        let mut num = self.num.div(gcd1)?.mul(r_den.div(gcd2)?);
        let mut den = self.den.div(gcd2)?.mul(r_num.div(gcd1)?);
        if den < zero {
            num = num.neg();
            den = den.neg();
        }
        Ok(Rational { num, den })
    }

    /// `operator+=(const T& i)`: `num += i * den`, nothing else.
    pub fn add_int(self, i: T) -> Rational<T> {
        Rational {
            num: self.num.add(i.mul(self.den)),
            den: self.den,
        }
    }

    /// `operator-=(const T& i)`: `num -= i * den`, nothing else.
    pub fn sub_int(self, i: T) -> Rational<T> {
        Rational {
            num: self.num.sub(i.mul(self.den)),
            den: self.den,
        }
    }

    /// `operator*=(const T& i)`: cancels `gcd(i, den)` first.
    pub fn mul_int(self, i: T) -> Result<Rational<T>, ExactError> {
        let gcd = i.gcd(self.den)?;
        let num = self.num.mul(i.div(gcd)?);
        let den = self.den.div(gcd)?;
        Ok(Rational { num, den })
    }

    /// `operator/=(const T& i)`: `bad_rational` for zero, unchanged for a zero numerator.
    pub fn div_int(self, i: T) -> Result<Rational<T>, ExactError> {
        let zero = T::zero();
        if i == zero {
            return Err(ExactError::BadRational(ExactError::ZERO_DENOMINATOR));
        }
        if self.num == zero {
            return Ok(self);
        }
        let gcd = self.num.gcd(i)?;
        let mut num = self.num.div(gcd)?;
        let mut den = self.den.mul(i.div(gcd)?);
        if den < zero {
            num = num.neg();
            den = den.neg();
        }
        Ok(Rational { num, den })
    }

    /// Unary minus: `rational(-num, den)`, normalised again.
    #[allow(clippy::should_implement_trait)] // fallible, as Boost's operators throw
    pub fn neg(self) -> Result<Rational<T>, ExactError> {
        Rational::new(self.num.neg(), self.den)
    }

    /// `abs`: the value itself for a numerator `>= 0`, else its negation.
    pub fn abs(self) -> Result<Rational<T>, ExactError> {
        if self.num >= T::zero() {
            Ok(self)
        } else {
            self.neg()
        }
    }

    /// `operator<(const rational&)`: the continued-fraction comparison.
    pub fn lt(&self, r: &Rational<T>) -> Result<bool, ExactError> {
        let zero = T::zero();
        let mut ts = Cf {
            n: self.num,
            d: self.den,
            q: self.num.div(self.den)?,
            r: self.num.rem(self.den)?,
        };
        let mut rs = Cf {
            n: r.num,
            d: r.den,
            q: r.num.div(r.den)?,
            r: r.num.rem(r.den)?,
        };
        let mut reverse = false;
        while ts.r < zero {
            ts.r = ts.r.add(ts.d);
            ts.q = ts.q.sub(T::one());
        }
        while rs.r < zero {
            rs.r = rs.r.add(rs.d);
            rs.q = rs.q.sub(T::one());
        }
        loop {
            if ts.q != rs.q {
                return Ok(if reverse { ts.q > rs.q } else { ts.q < rs.q });
            }
            reverse = !reverse;
            if ts.r == zero || rs.r == zero {
                break;
            }
            ts.n = ts.d;
            ts.d = ts.r;
            ts.q = ts.n.div(ts.d)?;
            ts.r = ts.n.rem(ts.d)?;
            rs.n = rs.d;
            rs.d = rs.r;
            rs.q = rs.n.div(rs.d)?;
            rs.r = rs.n.rem(rs.d)?;
        }
        if ts.r == rs.r {
            Ok(false)
        } else {
            Ok((ts.r != zero) != reverse)
        }
    }

    /// `operator>(const rational&)`: `r < *this`.
    pub fn gt(&self, r: &Rational<T>) -> Result<bool, ExactError> {
        r.lt(self)
    }

    /// `operator==(const rational&)`: the stored parts compared.
    pub fn eq_rational(&self, r: &Rational<T>) -> bool {
        self.num == r.num && self.den == r.den
    }

    /// `operator<(const T& i)`: the floor of the quotient against `i`.
    pub fn lt_int(&self, i: T) -> Result<bool, ExactError> {
        let zero = T::zero();
        let mut q = self.num.div(self.den)?;
        let mut r = self.num.rem(self.den)?;
        while r < zero {
            r = r.add(self.den);
            q = q.sub(T::one());
        }
        Ok(q < i)
    }

    /// `operator>(const T& i)`: false when equal, else not less.
    pub fn gt_int(&self, i: T) -> Result<bool, ExactError> {
        if self.eq_int(i) {
            Ok(false)
        } else {
            Ok(!self.lt_int(i)?)
        }
    }

    /// `operator==(const T& i)`: `den == 1 && num == i`.
    pub fn eq_int(&self, i: T) -> bool {
        self.den == T::one() && self.num == i
    }

    /// `rational_cast<double>`: the numerator and denominator converted, then divided.
    pub fn to_f64(&self) -> f64 {
        self.num.to_f64() / self.den.to_f64()
    }
}

/// One side of the continued-fraction comparison: `{ n, d, q, r }`.
struct Cf<T> {
    n: T,
    d: T,
    q: T,
    r: T,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Int128;

    fn r(n: i64, d: i64) -> Rational<Int128> {
        Rational::new(Int128::from_i64(n), Int128::from_i64(d)).unwrap()
    }

    #[test]
    fn construction_normalises() {
        let x = r(6, -4);
        assert_eq!(x.numerator(), Int128::from_i64(-3));
        assert_eq!(x.denominator(), Int128::from_i64(2));
        assert_eq!(r(0, -7), r(0, 1));
        assert_eq!(
            Rational::new(Int128::ONE, Int128::ZERO),
            Err(ExactError::BadRational(ExactError::ZERO_DENOMINATOR))
        );
    }

    #[test]
    fn arithmetic_keeps_the_normal_form() {
        assert_eq!(r(1, 6).add(r(1, 3)).unwrap(), r(1, 2));
        assert_eq!(r(1, 6).sub(r(1, 3)).unwrap(), r(-1, 6));
        assert_eq!(r(2, 3).mul(r(9, 4)).unwrap(), r(3, 2));
        assert_eq!(r(2, 3).div(r(-4, 9)).unwrap(), r(-3, 2));
        assert_eq!(r(3, 1).div_int(Int128::from_i64(-6)).unwrap(), r(-1, 2));
        assert_eq!(r(1, 6).mul_int(Int128::from_i64(4)).unwrap(), r(2, 3));
    }

    #[test]
    fn comparisons_follow_the_values() {
        assert!(r(1, 3).lt(&r(1, 2)).unwrap());
        assert!(!r(1, 2).lt(&r(1, 2)).unwrap());
        assert!(r(-1, 2).lt(&r(-1, 3)).unwrap());
        assert!(r(22, 7).gt(&r(355, 113)).unwrap());
        assert!(r(-5, 2).lt_int(Int128::from_i64(-2)).unwrap());
        assert!(!r(4, 2).gt_int(Int128::from_i64(2)).unwrap());
        assert_eq!(r(1, 4).to_f64(), 0.25);
    }
}
