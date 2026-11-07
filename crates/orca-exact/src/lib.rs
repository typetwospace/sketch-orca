// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Exact arithmetic as Boost 1.84.0 has it: multiprecision's signed-magnitude int128_t and rational.
//!
//! The C++ engine computes exactly in two places that phase O2 reaches, both in libnest2d as
//! `Arrange.cpp` instantiates it: the edge sort of `nfpConvexOnly` compares `boost::rational`
//! values over `LargeInt`, which MSVC (no `__int128`) makes Boost.Multiprecision's `int128_t`;
//! the rotating calipers of `minAreaBoundingBox` and `fitIntoBoxRotation` do too, for
//! arrangements that rotate (`<work>/deps/OrcaSlicer_dep/usr/local/include/boost-1_84`).
//!
//! - [`Int128`] is `int128_t`: a sign and a 128-bit magnitude that wraps modulo 2^128, so it
//!   differs from Rust's `i128` on overflow (module `int128`).
//! - [`Rational`] is `boost::rational<T>` over any [`ExactInt`], with Boost's normalisation, its
//!   gcd-cancelling operators and its continued-fraction comparison (module `rational`).
//! - [`ExactInt`] is what `Rational` needs of its integer, so that later phases can put the
//!   GMP and MPFR replacements under the same operations.
//!
//! The C++ throws: `std::overflow_error` for an integer division by zero, `bad_rational` (a
//! `std::domain_error`) for a zero denominator, and `std::domain_error` from the bit queries of
//! `int128_t`'s gcd when a Lehmer step wraps. Each is an [`ExactError`] here, whose message is the
//! C++ `what()`; `orca-arrange` passes each one up to its caller (`ArrangeError::Exact`), as
//! libnest2d and `Arrange.cpp` catch none of them. A remainder by zero is no exception: the C++
//! faults on the division, and `Int128::rem` panics.
//!
//! The probe file `arrange.sop1` records the operations from the oracle's own Boost (records
//! `XI28` and `XRAT`, `parity/SOP1.md`), and `tests/probes.rs` replays them.

mod int128;
mod rational;

pub use int128::Int128;
pub use rational::Rational;

use std::fmt;

/// The crate's name, as Cargo has it.
pub const NAME: &str = "orca-exact";

/// An exception the C++ throws from exact arithmetic.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExactError {
    /// `std::overflow_error("Integer Division by zero.")` of `cpp_int`'s division.
    DivisionByZero,
    /// `boost::bad_rational(what)`, a `std::domain_error`.
    BadRational(&'static str),
    /// `std::domain_error(what)` of `eval_lsb` or `eval_msb`, which `int128_t`'s gcd reaches when
    /// a Lehmer step wraps U or V to a negative value or zero.
    Domain(&'static str),
}

impl ExactError {
    /// `bad_rational()`'s message.
    pub const ZERO_DENOMINATOR: &'static str = "bad rational: zero denominator";
    /// The message of `normalize()`'s other `bad_rational`.
    pub const SINGULAR_DENOMINATOR: &'static str = "bad rational: non-zero singular denominator";
    /// `eval_lsb` and `eval_msb` of zero.
    pub const NO_BITS_SET: &'static str = "No bits were set in the operand.";
    /// `eval_lsb` and `eval_msb` of a negative value.
    pub const NEGATIVE_BITS: &'static str =
        "Testing individual bits in negative values is not supported - results are undefined.";

    /// The C++ exception's `what()`.
    pub fn what(&self) -> &'static str {
        match self {
            ExactError::DivisionByZero => "Integer Division by zero.",
            ExactError::BadRational(what) | ExactError::Domain(what) => what,
        }
    }
}

impl fmt::Display for ExactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.what())
    }
}

impl std::error::Error for ExactError {}

/// The integer operations `boost::rational` uses, each with its C++ type's own overflow.
pub trait ExactInt: Copy + Ord + fmt::Debug {
    /// `IntType(0)`.
    fn zero() -> Self;
    /// `IntType(1)`.
    fn one() -> Self;
    /// `std::numeric_limits<IntType>::max()`.
    fn max_value() -> Self;
    /// Construction from a 64-bit signed integer.
    fn from_i64(v: i64) -> Self;
    /// `a + b`.
    fn add(self, other: Self) -> Self;
    /// `a - b`.
    fn sub(self, other: Self) -> Self;
    /// `a * b`.
    fn mul(self, other: Self) -> Self;
    /// `a / b`, throwing for a zero divisor.
    fn div(self, other: Self) -> Result<Self, ExactError>;
    /// `a % b`; a zero divisor does what the type's C++ does (`int128_t` faults: a panic).
    fn rem(self, other: Self) -> Result<Self, ExactError>;
    /// `-a`.
    fn neg(self) -> Self;
    /// `integer::gcd(a, b)` as the type gets it, with its exceptions.
    fn gcd(self, other: Self) -> Result<Self, ExactError>;
    /// `static_cast<double>(a)`.
    fn to_f64(self) -> f64;
}

impl ExactInt for Int128 {
    fn zero() -> Int128 {
        Int128::ZERO
    }
    fn one() -> Int128 {
        Int128::ONE
    }
    fn max_value() -> Int128 {
        Int128::MAX
    }
    fn from_i64(v: i64) -> Int128 {
        Int128::from_i64(v)
    }
    fn add(self, other: Int128) -> Int128 {
        Int128::add(self, other)
    }
    fn sub(self, other: Int128) -> Int128 {
        Int128::sub(self, other)
    }
    fn mul(self, other: Int128) -> Int128 {
        Int128::mul(self, other)
    }
    fn div(self, other: Int128) -> Result<Int128, ExactError> {
        Int128::div(self, other)
    }
    fn rem(self, other: Int128) -> Result<Int128, ExactError> {
        Ok(Int128::rem(self, other))
    }
    fn neg(self) -> Int128 {
        Int128::neg(self)
    }
    fn gcd(self, other: Int128) -> Result<Int128, ExactError> {
        Int128::gcd(self, other)
    }
    fn to_f64(self) -> f64 {
        Int128::to_f64(self)
    }
}

#[cfg(test)]
mod tests {
    use super::NAME;

    #[test]
    fn name_is_the_package_name() {
        assert_eq!(NAME, env!("CARGO_PKG_NAME"));
    }
}
