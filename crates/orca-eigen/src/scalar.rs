// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Eigen code: the packet sizes and the SSE2 pcmp_lt, pmin and pmax of Packet2l of
// Eigen/src/Core/arch/SSE/PacketMath.h, the casts of Eigen/src/Core/arch/SSE/TypeCasting.h and
// numext::hypot of Eigen/src/Core/MathFunctionsImpl.h (positive_real_hypot), of Eigen 5.0.1,
// translated on 2026-11-07. Their copyright is the Eigen
// authors' (Benoit Jacob, Gael Guennebaud and the Eigen contributors), under the Mozilla Public
// License, version 2.0 (MPL-2.0; licenses/MPL-2.0.txt).
//
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0. If a copy of
// the MPL was not distributed with this file, You can obtain one at https://mozilla.org/MPL/2.0/.
// (Exhibit A of the MPL-2.0: the Eigen code here stays under it; the rest of the file is
// sketch-orca's, under the AGPL-3.0, as the MPL's section 3.3 allows.)

//! The scalar types Eigen's fixed-size vectors and matrices hold in libslic3r, and what Eigen does
//! with each coefficient.
//!
//! [`Scalar`] is a coefficient: `float`, `double`, `int32_t` or `coord_t` (`int64_t`). Its
//! [`Scalar::PACKET`] is the number of coefficients in one SSE2 packet (`packet_traits<T>::size`),
//! which decides how Eigen vectorises a reduction or a product of that type (`redux.rs`).
//! Integer arithmetic wraps, as MSVC's `imul` and `add` do (signed overflow is undefined in C++,
//! and the engine's build wraps); so the order of an integer reduction never changes its result.
//!
//! A comparison inside a packet is the scalar one for every type but `int64_t`: SSE2 has no 64-bit
//! compare, and Eigen builds one from 32-bit compares that takes the low halves signed
//! ([`pcmp_lt_i64`]). [`Scalar::packet_lt`], [`Scalar::packet_min`] and [`Scalar::packet_max`]
//! are the lane operations `cwiseMin`, `cwiseMax` and the packet steps of `minCoeff`, `maxCoeff`
//! and their index forms use (`vector.rs`, `redux.rs`).
//!
//! [`Real`] adds what Eigen calls on `float` and `double`: `std::sqrt`, `std::sin`, `std::cos`,
//! `std::atan2` through orca-crt (the static UCRT's results), and Eigen's own `numext::hypot`.
//!
//! [`Cast`] is `static_cast` of one coefficient, which `.cast<U>()` applies to every coefficient.
//! SSE2's packet casts give the scalar casts' results (`pcast<Packet2d, Packet2l>` converts each
//! lane with `cvttsd2si`, `pcast<Packet2l, Packet2d>` each with `static_cast<double>`,
//! `cvttpd2dq`, `cvtpd2ps` and `cvtdq2ps` round as their scalar forms do), so a cast is the C++
//! cast of each coefficient: through orca-crt for a float-to-integer cast (`cvttsd2si`, with its
//! "integer indefinite" for NaN and out-of-range values), and Rust's `as`, which rounds to
//! nearest-even as `cvtsi2sd`, `cvtsi2ss` and `cvtsd2ss` do, for the rest.

use core::fmt::Debug;

/// A coefficient of an Eigen vector or matrix in libslic3r: `f32`, `f64`, `i32` or `i64`.
pub trait Scalar: Copy + PartialEq + PartialOrd + Debug + 'static {
    /// The coefficients in one SSE2 packet of this type (`packet_traits<T>::size`): 4 for `float`
    /// and `int32_t`, 2 for `double` and `int64_t`.
    const PACKET: usize;
    /// `T(0)`.
    const ZERO: Self;
    /// `T(1)`.
    const ONE: Self;
    /// `a + b` (wrapping for integers).
    fn add(self, rhs: Self) -> Self;
    /// `a - b` (wrapping for integers).
    fn sub(self, rhs: Self) -> Self;
    /// `a * b` (wrapping for integers).
    fn mul(self, rhs: Self) -> Self;
    /// `a / b` (truncating for integers; a zero divisor panics, as the C++ engine traps).
    fn div(self, rhs: Self) -> Self;
    /// `-a` (wrapping for integers).
    fn neg(self) -> Self;
    /// `std::abs(a)` (`fabs`; wrapping for integers, so `abs(INT64_MIN)` is `INT64_MIN`).
    fn abs(self) -> Self;
    /// `pcmp_lt` in one lane of an SSE2 packet: `a < b` (`_mm_cmplt_pd`, `_mm_cmplt_ps`,
    /// `_mm_cmplt_epi32`), but [`pcmp_lt_i64`] for `int64_t`.
    #[inline]
    fn packet_lt(self, rhs: Self) -> bool {
        self < rhs
    }
    /// `pmin(a, b)` in one lane of an SSE2 packet: `std::min(a, b)` (`_mm_min_pd(b, a)`,
    /// `_mm_min_ps(b, a)`, the `int32_t` select of `a < b`), but for `int64_t` `a` where
    /// [`pcmp_lt_i64`]`(a, b)` and `b` otherwise.
    #[inline]
    fn packet_min(self, rhs: Self) -> Self {
        orca_stl::min(self, rhs)
    }
    /// `pmax(a, b)` in one lane of an SSE2 packet: `std::max(a, b)`, but for `int64_t` `b` where
    /// [`pcmp_lt_i64`]`(a, b)` and `a` otherwise.
    #[inline]
    fn packet_max(self, rhs: Self) -> Self {
        orca_stl::max(self, rhs)
    }
}

/// `pcmp_lt<Packet2l>(a, b)` as MSVC builds Eigen's SSE2 PacketMath.h, without SSE4.2's
/// `_mm_cmpgt_epi64`: `hi(a) < hi(b) || (hi(a) == hi(b) && lo(a) < lo(b))`, each 32-bit half
/// compared as a signed `int32_t` (`_mm_cmplt_epi32`), the low halves too. A true 64-bit compare
/// takes the low halves unsigned, so this one is wrong where the high halves are equal and one low
/// half has its top bit set while the other does not: `0x1_0000_0000 < 0x1_8000_0000` is false
/// and `0x1_8000_0000 < 0x1_0000_0000` true. Values in [-2^31, 2^31) all compare right.
#[must_use]
pub fn pcmp_lt_i64(a: i64, b: i64) -> bool {
    let (ha, hb) = ((a >> 32) as i32, (b >> 32) as i32);
    let (la, lb) = (a as i32, b as i32);
    ha < hb || (ha == hb && la < lb)
}

macro_rules! float_scalar {
    ($t:ty, $packet:expr) => {
        impl Scalar for $t {
            const PACKET: usize = $packet;
            const ZERO: Self = 0.0;
            const ONE: Self = 1.0;
            #[inline]
            fn add(self, rhs: Self) -> Self {
                self + rhs
            }
            #[inline]
            fn sub(self, rhs: Self) -> Self {
                self - rhs
            }
            #[inline]
            fn mul(self, rhs: Self) -> Self {
                self * rhs
            }
            #[inline]
            fn div(self, rhs: Self) -> Self {
                self / rhs
            }
            #[inline]
            fn neg(self) -> Self {
                -self
            }
            #[inline]
            fn abs(self) -> Self {
                <$t>::abs(self)
            }
        }
    };
}

macro_rules! int_scalar {
    ($t:ty, $packet:expr $(; $($lanes:tt)*)?) => {
        impl Scalar for $t {
            const PACKET: usize = $packet;
            const ZERO: Self = 0;
            const ONE: Self = 1;
            #[inline]
            fn add(self, rhs: Self) -> Self {
                self.wrapping_add(rhs)
            }
            #[inline]
            fn sub(self, rhs: Self) -> Self {
                self.wrapping_sub(rhs)
            }
            #[inline]
            fn mul(self, rhs: Self) -> Self {
                self.wrapping_mul(rhs)
            }
            #[inline]
            fn div(self, rhs: Self) -> Self {
                self.wrapping_div(rhs)
            }
            #[inline]
            fn neg(self) -> Self {
                self.wrapping_neg()
            }
            #[inline]
            fn abs(self) -> Self {
                self.wrapping_abs()
            }
            $($($lanes)*)?
        }
    };
}

float_scalar!(f32, 4);
float_scalar!(f64, 2);
int_scalar!(i32, 4);
// `pmin<Packet2l>`: `por(pandnot(b, mask), pand(a, mask))` with `mask = pcmp_lt(a, b)`; `pmax`
// selects the other way.
int_scalar!(i64, 2;
    #[inline]
    fn packet_lt(self, rhs: Self) -> bool {
        pcmp_lt_i64(self, rhs)
    }
    #[inline]
    fn packet_min(self, rhs: Self) -> Self {
        if pcmp_lt_i64(self, rhs) { self } else { rhs }
    }
    #[inline]
    fn packet_max(self, rhs: Self) -> Self {
        if pcmp_lt_i64(self, rhs) { rhs } else { self }
    }
);

/// A floating-point coefficient: what Eigen computes on `float` and `double` beyond arithmetic.
pub trait Real: Scalar {
    /// `NumTraits<T>::epsilon()`.
    const EPSILON: Self;
    /// `std::numeric_limits<T>::min()`, the smallest normal value.
    const MIN_POSITIVE: Self;
    /// `NumTraits<T>::highest()`.
    const HIGHEST: Self;
    /// `NumTraits<T>::dummy_precision()`: 1e-5 for `float`, 1e-12 for `double`.
    const DUMMY_PRECISION: Self;
    /// `T(EIGEN_PI)`.
    const PI: Self;
    /// `T(x)`: a C++ literal of type `double` converted to this type.
    fn from_f64(x: f64) -> Self;
    /// `std::sqrt` (`sqrtsd`, `sqrtss`).
    fn sqrt(self) -> Self;
    /// `std::sin`, the static UCRT's.
    fn sin(self) -> Self;
    /// `std::cos`, the static UCRT's.
    fn cos(self) -> Self;
    /// `std::atan2(y, x)`, the static UCRT's.
    fn atan2(self, x: Self) -> Self;
    /// `std::pow(x, y)`, the static UCRT's.
    fn pow(self, y: Self) -> Self;
    /// `std::isnan`.
    fn is_nan(self) -> bool;
    /// `std::isinf`.
    fn is_infinite(self) -> bool;
    /// `std::isfinite`.
    fn is_finite(self) -> bool;
}

impl Real for f64 {
    const EPSILON: Self = f64::EPSILON;
    const MIN_POSITIVE: Self = f64::MIN_POSITIVE;
    const HIGHEST: Self = f64::MAX;
    const DUMMY_PRECISION: Self = 1e-12;
    const PI: Self = core::f64::consts::PI;
    #[inline]
    fn from_f64(x: f64) -> Self {
        x
    }
    #[inline]
    fn sqrt(self) -> Self {
        orca_crt::sqrt(self)
    }
    #[inline]
    fn sin(self) -> Self {
        orca_crt::sin(self)
    }
    #[inline]
    fn cos(self) -> Self {
        orca_crt::cos(self)
    }
    #[inline]
    fn atan2(self, x: Self) -> Self {
        orca_crt::atan2(self, x)
    }
    #[inline]
    fn pow(self, y: Self) -> Self {
        orca_crt::pow(self, y)
    }
    #[inline]
    fn is_nan(self) -> bool {
        f64::is_nan(self)
    }
    #[inline]
    fn is_infinite(self) -> bool {
        f64::is_infinite(self)
    }
    #[inline]
    fn is_finite(self) -> bool {
        f64::is_finite(self)
    }
}

impl Real for f32 {
    const EPSILON: Self = f32::EPSILON;
    const MIN_POSITIVE: Self = f32::MIN_POSITIVE;
    const HIGHEST: Self = f32::MAX;
    const DUMMY_PRECISION: Self = 1e-5;
    const PI: Self = core::f32::consts::PI;
    #[inline]
    fn from_f64(x: f64) -> Self {
        x as f32
    }
    #[inline]
    fn sqrt(self) -> Self {
        orca_crt::sqrtf(self)
    }
    #[inline]
    fn sin(self) -> Self {
        orca_crt::sinf(self)
    }
    #[inline]
    fn cos(self) -> Self {
        orca_crt::cosf(self)
    }
    #[inline]
    fn atan2(self, x: Self) -> Self {
        orca_crt::atan2f(self, x)
    }
    #[inline]
    fn pow(self, y: Self) -> Self {
        orca_crt::powf(self, y)
    }
    #[inline]
    fn is_nan(self) -> bool {
        f32::is_nan(self)
    }
    #[inline]
    fn is_infinite(self) -> bool {
        f32::is_infinite(self)
    }
    #[inline]
    fn is_finite(self) -> bool {
        f32::is_finite(self)
    }
}

/// `numext::hypot(x, y)`: Eigen's own, not the UCRT's (`positive_real_hypot` of `|x|` and
/// `|y|`): an infinity if either is infinite, NaN if either is NaN, 0 if both are 0, else
/// `p * sqrt(1 + q*q)` with `p = std::max(|x|, |y|)` and `q = std::min(|y|, |x|) / p`.
pub fn hypot<T: Real>(x: T, y: T) -> T {
    let (x, y) = (x.abs(), y.abs());
    if x.is_infinite() || y.is_infinite() {
        return T::from_f64(f64::INFINITY);
    }
    if x.is_nan() || y.is_nan() {
        return T::from_f64(f64::NAN);
    }
    let p = orca_stl::max(x, y);
    if p == T::ZERO {
        return T::ZERO;
    }
    let qp = orca_stl::min(y, x).div(p);
    p.mul(T::ONE.add(qp.mul(qp)).sqrt())
}

/// `static_cast<U>(x)` of one coefficient, as `.cast<U>()` applies it to each.
pub trait Cast<U> {
    /// The C++ cast.
    fn cast(self) -> U;
}

macro_rules! cast_as {
    ($($from:ty => $to:ty),+ $(,)?) => {
        $(impl Cast<$to> for $from {
            #[inline]
            fn cast(self) -> $to {
                self as $to
            }
        })+
    };
}

// The conversions Rust's `as` gives exactly: the identities, widening, integer truncation and the
// rounding conversions to floating point (round to nearest, ties to even, as SSE2 rounds).
cast_as!(
    f64 => f64, f32 => f32, i64 => i64, i32 => i32,
    f64 => f32, f32 => f64,
    i64 => f64, i64 => f32, i32 => f64, i32 => f32,
    i64 => i32, i32 => i64,
);

impl Cast<i64> for f64 {
    #[inline]
    fn cast(self) -> i64 {
        orca_crt::f64_to_i64(self)
    }
}
impl Cast<i32> for f64 {
    #[inline]
    fn cast(self) -> i32 {
        orca_crt::f64_to_i32(self)
    }
}
impl Cast<i64> for f32 {
    #[inline]
    fn cast(self) -> i64 {
        orca_crt::f32_to_i64(self)
    }
}
impl Cast<i32> for f32 {
    #[inline]
    fn cast(self) -> i32 {
        orca_crt::f32_to_i32(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_to_integer_casts_are_msvcs() {
        assert_eq!(Cast::<i64>::cast(f64::NAN), i64::MIN);
        assert_eq!(Cast::<i32>::cast(3.0e9_f64), i32::MIN);
        assert_eq!(Cast::<i64>::cast(-2.9_f64), -2);
        assert_eq!(Cast::<f32>::cast(i64::MAX), 9.223_372e18_f32);
    }

    #[test]
    fn int64_packet_compare_takes_the_low_halves_signed() {
        assert!(!pcmp_lt_i64(0x1_0000_0000, 0x1_8000_0000));
        assert!(pcmp_lt_i64(0x1_8000_0000, 0x1_0000_0000));
        assert!(pcmp_lt_i64(-0x8000_0000, 0x7FFF_FFFF));
        assert!(pcmp_lt_i64(-0x1_0000_0001, -1));
        assert_eq!(0x1_0000_0000_i64.packet_min(0x1_8000_0000), 0x1_8000_0000);
        assert_eq!(0x1_0000_0000_i64.packet_max(0x1_8000_0000), 0x1_0000_0000);
        assert_eq!(5_i32.packet_min(-3), -3);
        assert!(f64::NAN.packet_min(1.0).is_nan());
    }

    #[test]
    fn hypot_is_eigens() {
        assert_eq!(hypot(3.0_f64, -4.0), 5.0);
        assert_eq!(hypot(0.0_f64, -0.0), 0.0);
        assert!(hypot(f64::NAN, f64::INFINITY).is_infinite());
    }
}
