// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Boost code: boost::multiprecision::int128_t of Boost 1.84.0, translated on 2026-11-07
// from boost/multiprecision/cpp_int.hpp (the fixed-width signed-magnitude cpp_int_base: sign,
// negate, normalize), cpp_int/add_unsigned.hpp, cpp_int/add.hpp, cpp_int/multiply.hpp,
// cpp_int/divide.hpp, cpp_int/comparison.hpp and cpp_int/misc.hpp (eval_gcd and eval_convert_to
// of a floating-point type). Copyright 2012-2022 John Maddock, Copyright 2019-2022 Christopher
// Kormanyos, Copyright 2019-2020 Madhur Chauhan and Copyright 2021 Matt Borland, under the Boost
// Software License, version 1.0 (BSL-1.0; licenses/BSL-1.0.txt).

//! `boost::multiprecision::int128_t` as MSVC builds it for `Arrange.cpp` (which picks it as
//! `LargeInt` where there is no `__int128`): `cpp_int_backend<128, 128, signed_magnitude,
//! unchecked>`, four 32-bit limbs and a sign flag.
//!
//! - The value is a sign and a 128-bit magnitude, so the range is +-(2^128 - 1), one wider than
//!   two's complement at the bottom and one narrower nowhere: `-x` never overflows.
//! - Zero is never negative: `sign(bool)` and `negate()` clear the flag of a zero magnitude.
//! - Unchecked overflow wraps the magnitude modulo 2^128 and keeps the sign the operation gives
//!   (the first operand's, for an addition of like signs), so `MAX + 1` is zero and
//!   `-MAX - 1` is zero too, where `i128` would wrap to the other end.
//! - Division and remainder truncate toward zero (the quotient's sign is the XOR of the signs, the
//!   remainder's that of the dividend). A zero divisor makes the division throw
//!   `std::overflow_error("Integer Division by zero.")`, an [`ExactError::DivisionByZero`] here,
//!   but the remainder divides by the zero limb unchecked (`eval_modulus` by a limb,
//!   cpp_int/divide.hpp), a hardware fault that ends the process: a panic here.
//! - `gcd` is Boost.Multiprecision's `eval_gcd` (the `using boost::multiprecision::gcd` in
//!   namespace `boost::integer` makes `integer::gcd` pick it over Boost.Integer's template),
//!   translated step for step: a one-limb operand takes the limb path; otherwise the common
//!   powers of two come off, two limbs or fewer finish with a native gcd, and larger values take
//!   Lehmer's double-digit steps (32-bit limbs and a native 64-bit double limb, MSVC's branch) or
//!   a remainder. A Lehmer step multiplies U and V by its cosequences modulo 2^128, so for some
//!   operands of four limbs the products wrap and U or V comes out negative or zero; `eval_lsb`
//!   or `eval_msb` then throws `std::domain_error` (an [`ExactError::Domain`]), and other
//!   operands get a value that is not their greatest common divisor, as the C++ does.
//! - Conversion to `double` keeps the top 53 bits of the magnitude and rounds to nearest, ties to
//!   even (the bit test after the truncation), which is what Rust's `u128 as f64` does.

use crate::ExactError;
use std::cmp::Ordering;

/// A `boost::multiprecision::int128_t`: a sign and a 128-bit magnitude, zero never negative.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Int128 {
    negative: bool,
    magnitude: u128,
}

impl Int128 {
    /// Zero.
    pub const ZERO: Int128 = Int128 {
        negative: false,
        magnitude: 0,
    };
    /// One.
    pub const ONE: Int128 = Int128 {
        negative: false,
        magnitude: 1,
    };
    /// `std::numeric_limits<int128_t>::max()`: 2^128 - 1.
    pub const MAX: Int128 = Int128 {
        negative: false,
        magnitude: u128::MAX,
    };
    /// `std::numeric_limits<int128_t>::min()`: -(2^128 - 1).
    pub const MIN: Int128 = Int128 {
        negative: true,
        magnitude: u128::MAX,
    };

    /// The value with this sign and magnitude; a zero magnitude is positive (`sign(bool)`).
    pub const fn from_sign_magnitude(negative: bool, magnitude: u128) -> Int128 {
        Int128 {
            negative: negative && magnitude != 0,
            magnitude,
        }
    }

    /// Construction from a 64-bit signed integer (its magnitude by `unsigned_abs`).
    pub const fn from_i64(v: i64) -> Int128 {
        Int128::from_sign_magnitude(v < 0, v.unsigned_abs() as u128)
    }

    /// Construction from a 64-bit unsigned integer.
    pub const fn from_u64(v: u64) -> Int128 {
        Int128::from_sign_magnitude(false, v as u128)
    }

    /// Construction from an `i128` (any value fits: |i128::MIN| = 2^127).
    pub const fn from_i128(v: i128) -> Int128 {
        Int128::from_sign_magnitude(v < 0, v.unsigned_abs())
    }

    /// Whether the sign flag is set (never for zero).
    pub const fn is_negative(self) -> bool {
        self.negative
    }

    /// The magnitude.
    pub const fn magnitude(self) -> u128 {
        self.magnitude
    }

    /// Whether the value is zero.
    pub const fn is_zero(self) -> bool {
        self.magnitude == 0
    }

    /// The value as an `i128`, when it fits.
    pub fn to_i128(self) -> Option<i128> {
        if self.negative {
            if self.magnitude <= 1u128 << 127 {
                Some((self.magnitude as i128).wrapping_neg())
            } else {
                None
            }
        } else {
            i128::try_from(self.magnitude).ok()
        }
    }

    /// `eval_add`: like signs add the magnitudes modulo 2^128 under the first operand's sign;
    /// unlike signs subtract the smaller magnitude from the larger, under the larger's sign.
    pub const fn add(self, other: Int128) -> Int128 {
        if self.negative == other.negative {
            Int128::from_sign_magnitude(self.negative, self.magnitude.wrapping_add(other.magnitude))
        } else if self.magnitude >= other.magnitude {
            Int128::from_sign_magnitude(self.negative, self.magnitude - other.magnitude)
        } else {
            Int128::from_sign_magnitude(other.negative, other.magnitude - self.magnitude)
        }
    }

    /// `eval_subtract`: unlike signs add the magnitudes modulo 2^128 under the first operand's
    /// sign; like signs subtract, the result negated when the second magnitude is the larger.
    pub const fn sub(self, other: Int128) -> Int128 {
        if self.negative != other.negative {
            Int128::from_sign_magnitude(self.negative, self.magnitude.wrapping_add(other.magnitude))
        } else if self.magnitude >= other.magnitude {
            Int128::from_sign_magnitude(self.negative, self.magnitude - other.magnitude)
        } else {
            Int128::from_sign_magnitude(!self.negative, other.magnitude - self.magnitude)
        }
    }

    /// `eval_multiply`: the product of the magnitudes modulo 2^128, the sign the XOR of the signs.
    pub const fn mul(self, other: Int128) -> Int128 {
        Int128::from_sign_magnitude(
            self.negative != other.negative,
            self.magnitude.wrapping_mul(other.magnitude),
        )
    }

    /// `eval_divide`: the truncated quotient of the magnitudes, the sign the XOR of the signs.
    pub const fn div(self, other: Int128) -> Result<Int128, ExactError> {
        if other.magnitude == 0 {
            return Err(ExactError::DivisionByZero);
        }
        Ok(Int128::from_sign_magnitude(
            self.negative != other.negative,
            self.magnitude / other.magnitude,
        ))
    }

    /// `eval_modulus`: the remainder of the magnitudes under the dividend's sign.
    ///
    /// # Panics
    ///
    /// For a zero divisor, where the C++ divides by the zero limb unchecked and the process ends
    /// on the hardware fault.
    pub const fn rem(self, other: Int128) -> Int128 {
        assert!(
            other.magnitude != 0,
            "int128_t % 0: an integer division by zero"
        );
        Int128::from_sign_magnitude(self.negative, self.magnitude % other.magnitude)
    }

    /// Unary minus (`negate()`): the sign flipped, zero staying positive.
    pub const fn neg(self) -> Int128 {
        Int128::from_sign_magnitude(!self.negative, self.magnitude)
    }

    /// `abs`: the magnitude, positive.
    pub const fn abs(self) -> Int128 {
        Int128::from_sign_magnitude(false, self.magnitude)
    }

    /// `boost::multiprecision::gcd(a, b)`, `eval_gcd` of cpp_int/misc.hpp step for step: the
    /// greatest common divisor of the magnitudes, positive, for every operand but those whose
    /// Lehmer steps wrap (see the module notes).
    ///
    /// # Errors
    ///
    /// [`ExactError::Domain`] where `eval_lsb` or `eval_msb` meets a negative or zero U or V.
    pub fn gcd(self, other: Int128) -> Result<Int128, ExactError> {
        let (a, b) = (self, other);
        if limbs(a.magnitude) == 1 {
            return Ok(gcd_limb(b, limb(a.magnitude, 0)));
        }
        if limbs(b.magnitude) == 1 {
            return Ok(gcd_limb(a, limb(b.magnitude, 0)));
        }
        // Both have two limbs or more, so neither is zero and U and V start positive.
        let (mut u, mut v) = (a.abs(), b.abs());
        let us = u.lsb()?;
        let vs = v.lsb()?;
        let shift = us.min(vs);
        if us != 0 {
            u = u.shr(us);
        }
        if vs != 0 {
            v = v.shr(vs);
        }
        if u < v {
            std::mem::swap(&mut u, &mut v);
        }
        while v.magnitude != 0 {
            let ul = limbs(u.magnitude);
            if ul <= 2 {
                // The native gcd of the low limbs, whatever V's size.
                u = if ul == 1 {
                    Int128::from_u64(u64::from(gcd_u32(
                        limb(v.magnitude, 0),
                        limb(u.magnitude, 0),
                    )))
                } else {
                    let i = u.magnitude as u64;
                    let j = if limbs(v.magnitude) == 1 {
                        u64::from(limb(v.magnitude, 0))
                    } else {
                        v.magnitude as u64
                    };
                    Int128::from_u64(gcd_u64(i, j))
                };
                break;
            }
            let lu = u.msb()? + 1;
            let lv = v.msb()? + 1;
            // std::size_t arithmetic: a V longer than U wraps to a huge difference.
            if (lu as usize).wrapping_sub(lv as usize) <= 16 {
                lehmer(&mut u, &mut v, lu)?;
            } else {
                let t = u.rem(v);
                u = v;
                v = t;
            }
        }
        Ok(if shift != 0 { u.shl(shift) } else { u })
    }

    /// `eval_lsb`: the lowest set bit of a positive value.
    fn lsb(self) -> Result<u32, ExactError> {
        if self.magnitude == 0 {
            return Err(ExactError::Domain(ExactError::NO_BITS_SET));
        }
        if self.negative {
            return Err(ExactError::Domain(ExactError::NEGATIVE_BITS));
        }
        Ok(self.magnitude.trailing_zeros())
    }

    /// `eval_msb`: the highest set bit of a positive value.
    fn msb(self) -> Result<u32, ExactError> {
        if self.magnitude == 0 {
            return Err(ExactError::Domain(ExactError::NO_BITS_SET));
        }
        if self.negative {
            return Err(ExactError::Domain(ExactError::NEGATIVE_BITS));
        }
        Ok(127 - self.magnitude.leading_zeros())
    }

    /// `eval_right_shift` of a positive value.
    const fn shr(self, s: u32) -> Int128 {
        Int128::from_sign_magnitude(self.negative, self.magnitude >> s)
    }

    /// `eval_left_shift`: the bits past the 128th dropped, the sign kept.
    const fn shl(self, s: u32) -> Int128 {
        Int128::from_sign_magnitude(self.negative, self.magnitude << s)
    }

    /// `eval_multiply(result, a, limb_type)`: zero for a zero limb, else the product modulo
    /// 2^128 under `a`'s sign.
    const fn mul_limb(self, l: u32) -> Int128 {
        if l == 0 {
            return Int128::ZERO;
        }
        Int128::from_sign_magnitude(self.negative, self.magnitude.wrapping_mul(l as u128))
    }

    /// `convert_to<double>()`: the magnitude rounded to nearest, ties to even, then the sign.
    pub fn to_f64(self) -> f64 {
        let m = self.magnitude as f64;
        if self.negative { -m } else { m }
    }
}

/// `size()`: the 32-bit limbs of a magnitude, one for zero.
const fn limbs(m: u128) -> u32 {
    if m == 0 {
        1
    } else {
        (128 - m.leading_zeros()).div_ceil(32)
    }
}

/// `limbs()[k]`.
const fn limb(m: u128, k: u32) -> u32 {
    (m >> (32 * k)) as u32
}

/// `eval_gcd(limb_type, limb_type)`: `std::gcd`, with gcd(x, 0) = x.
const fn gcd_u32(mut u: u32, mut v: u32) -> u32 {
    while v != 0 {
        let t = u % v;
        u = v;
        v = t;
    }
    u
}

/// `eval_gcd(double_limb_type, double_limb_type)`: `std::gcd`, with gcd(x, 0) = x.
const fn gcd_u64(mut u: u64, mut v: u64) -> u64 {
    while v != 0 {
        let t = u % v;
        u = v;
        v = t;
    }
    u
}

/// `eval_gcd(result, a, limb_type b)`: `|a| | b` when either is zero, else the gcd of `b` and
/// `|a| mod b`; positive.
fn gcd_limb(a: Int128, b: u32) -> Int128 {
    if b == 0 || a.magnitude == 0 {
        return Int128::from_sign_magnitude(false, a.magnitude | u128::from(b));
    }
    let r = (a.magnitude % u128::from(b)) as u32;
    Int128::from_u64(u64::from(gcd_u32(r, b)))
}

/// `eval_gcd_lehmer(U, V, lu, storage)` without `__int128`: the leading 64 bits of U and V run a
/// simulated Euclid until Jebelean's conditions stop it, then the cosequences rebuild U and V
/// (in 128 bits, so the products may wrap), and a stray power of two comes off.
fn lehmer(big_u: &mut Int128, big_v: &mut Int128, lu: u32) -> Result<(), ExactError> {
    let (um, vm) = (big_u.magnitude, big_v.magnitude);
    let us = limbs(um);
    let h = lu % 32;
    let mut u = (u64::from(limb(um, us - 1)) << 32) | u64::from(limb(um, us - 2));
    let top = if limbs(vm) < us {
        0
    } else {
        limb(vm, limbs(vm) - 1)
    };
    let mut v = (u64::from(top) << 32) | u64::from(limb(vm, us - 2));
    if h != 0 {
        u <<= 32 - h;
        u |= u64::from(limb(um, us - 3) >> h);
        v <<= 32 - h;
        v |= u64::from(limb(vm, us - 3) >> h);
    }
    let mut x = [1u64, 0, 0];
    let mut y = [0u64, 1, 0];
    let mut i: u64 = 0;
    loop {
        let q = u / v;
        x[2] = x[0].wrapping_add(q.wrapping_mul(x[1]));
        y[2] = y[0].wrapping_add(q.wrapping_mul(y[1]));
        let tu = u;
        u = v;
        v = tu.wrapping_sub(q.wrapping_mul(v));
        i += 1;
        if y[2] >> 32 != 0 {
            break;
        }
        if i & 1 == 0 {
            if v < x[2] || u.wrapping_sub(v) < y[2].wrapping_add(y[1]) {
                break;
            }
        } else if v < y[2] || u.wrapping_sub(v) < x[2].wrapping_add(x[1]) {
            break;
        }
        x[0] = x[1];
        x[1] = x[2];
        y[0] = y[1];
        y[1] = y[2];
    }
    if i == 1 {
        // Stalled: one step of Euclid's own.
        let t = big_u.rem(*big_v);
        *big_u = *big_v;
        *big_v = t;
        return Ok(());
    }
    // The multipliers are cast to limb_type; the zero tests read the double limbs.
    let t1 = big_u.mul_limb(x[0] as u32);
    let mut t2 = big_v.mul_limb(y[0] as u32);
    let t3 = big_u.mul_limb(x[1] as u32);
    if i & 1 == 0 {
        *big_u = if x[0] == 0 { t2 } else { t2.sub(t1) };
    } else {
        *big_u = t1.sub(t2);
    }
    t2 = big_v.mul_limb(y[1] as u32);
    if i & 1 != 0 {
        *big_v = if x[1] == 0 { t2 } else { t2.sub(t3) };
    } else {
        *big_v = t3.sub(t2);
    }
    // lu < 2048 always holds for 128 bits.
    if limb(big_u.magnitude, 0) & 1 == 0 {
        let s = big_u.lsb()?;
        *big_u = big_u.shr(s);
        if *big_u < *big_v {
            std::mem::swap(big_u, big_v);
        }
    } else if limb(big_v.magnitude, 0) & 1 == 0 {
        let s = big_v.lsb()?;
        *big_v = big_v.shr(s);
    }
    Ok(())
}

impl Ord for Int128 {
    /// `compare`: the sign first (negative below zero below positive), then the magnitude, its
    /// order reversed for two negative values.
    fn cmp(&self, other: &Int128) -> Ordering {
        match (self.negative, other.negative) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => self.magnitude.cmp(&other.magnitude),
            (true, true) => other.magnitude.cmp(&self.magnitude),
        }
    }
}

impl PartialOrd for Int128 {
    fn partial_cmp(&self, other: &Int128) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl From<i64> for Int128 {
    fn from(v: i64) -> Int128 {
        Int128::from_i64(v)
    }
}

impl From<i32> for Int128 {
    fn from(v: i32) -> Int128 {
        Int128::from_i64(i64::from(v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overflow_wraps_the_magnitude_and_keeps_the_sign() {
        assert_eq!(Int128::MAX.add(Int128::ONE), Int128::ZERO);
        assert_eq!(Int128::MIN.sub(Int128::ONE), Int128::ZERO);
        assert_eq!(Int128::MIN.neg(), Int128::MAX);
        let half = Int128::from_sign_magnitude(true, 1u128 << 127);
        assert_eq!(half.add(half), Int128::ZERO);
        assert!(!half.add(half).is_negative());
        let big = Int128::from_sign_magnitude(true, (1u128 << 127) + 5);
        assert_eq!(big.mul(Int128::from_i64(2)), Int128::from_i64(-10));
    }

    #[test]
    fn division_truncates_and_the_remainder_follows_the_dividend() {
        let a = Int128::from_i64(-7);
        let b = Int128::from_i64(2);
        assert_eq!(a.div(b), Ok(Int128::from_i64(-3)));
        assert_eq!(a.rem(b), Int128::from_i64(-1));
        assert_eq!(b.rem(a), Int128::from_i64(2));
        assert_eq!(a.div(Int128::ZERO), Err(ExactError::DivisionByZero));
        assert_eq!(Int128::from_i64(-4).rem(b), Int128::ZERO);
        assert!(!Int128::from_i64(-4).rem(b).is_negative());
    }

    #[test]
    #[should_panic(expected = "int128_t % 0")]
    fn a_remainder_by_zero_ends_the_run() {
        let _ = Int128::from_i64(7).rem(Int128::ZERO);
    }

    #[test]
    fn gcd_is_of_the_magnitudes() {
        let g = |a: Int128, b: Int128| a.gcd(b).unwrap();
        assert_eq!(
            g(Int128::from_i64(-12), Int128::from_i64(18)),
            Int128::from_i64(6)
        );
        assert_eq!(g(Int128::ZERO, Int128::from_i64(-5)), Int128::from_i64(5));
        assert_eq!(g(Int128::ZERO, Int128::ZERO), Int128::ZERO);
        assert_eq!(g(Int128::MIN, Int128::ZERO), Int128::MAX);
        // Two limbs and more: the common power of two, the native finish and Lehmer's steps.
        let a = Int128::from_sign_magnitude(true, 6 << 100);
        let b = Int128::from_sign_magnitude(false, 9 << 90);
        assert_eq!(g(a, b), Int128::from_sign_magnitude(false, 3 << 90));
        let p = 0xFFFF_FFFF_FFFF_FFC5u128; // the largest prime below 2^64
        let q = 1_000_000_007u128;
        assert_eq!(
            g(
                Int128::from_sign_magnitude(false, p * q),
                Int128::from_sign_magnitude(false, p * 3)
            ),
            Int128::from_sign_magnitude(false, p)
        );
    }

    #[test]
    fn a_wrapped_lehmer_step_throws_as_boost_does() {
        // XI28's random record 44: the products of the cosequences wrap past 2^128, V turns
        // negative and eval_msb throws.
        let a = Int128::MIN;
        let b = Int128::from_sign_magnitude(true, 0x0dd8_de2d_a15d_007e_fa76_2ed8_aeaa_a40c);
        assert_eq!(a.gcd(b), Err(ExactError::Domain(ExactError::NEGATIVE_BITS)));
    }

    #[test]
    fn conversion_to_double_rounds_half_to_even() {
        let m = Int128::from_sign_magnitude(false, (1u128 << 53) + 1);
        assert_eq!(m.to_f64(), 9007199254740992.0);
        let m = Int128::from_sign_magnitude(true, (1u128 << 53) + 3);
        assert_eq!(m.to_f64(), -9007199254740996.0);
        assert_eq!(
            Int128::MAX.to_f64(),
            340282366920938463463374607431768211456.0
        );
    }
}
