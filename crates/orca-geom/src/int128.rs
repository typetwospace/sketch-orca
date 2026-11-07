// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Clipper code: the static functions of class Int128 in OrcaSlicer v2.4.2's
// src/libslic3r/Int128.hpp, an excerpt of Clipper 6.2.9 extended by Vojtech Bubnik, translated on
// 2026-11-07. Copyright Angus Johnson 2010-2015 and the PrusaSlicer authors, under the Boost
// Software License, version 1.0 (BSL-1.0; licenses/BSL-1.0.txt).

//! `Int128`'s predicates as MSVC x64 compiles them: the class takes its `_mul128` branch (no
//! `__int128` in MSVC), whose two-word arithmetic is two's complement modulo 2^128, so `i128` gives
//! the same integers. In that branch nothing before `multiply`, `sign_determinant_2x2` and
//! `compare_rationals` says `public:`, so callers reach only the two filtered predicates; the
//! exact ones stay private here too.
//!
//! The filtered forms first try an estimate from the upper 32 bits of each operand. Their
//! "rounding" adds `1 << 31`, an `int` expression that MSVC folds to `INT_MIN`, so each operand
//! is shifted by -2^31, not +2^31, before the shift right by 32. The estimate and its error bound
//! are translated as written, with that constant, and with the wrap-around MSVC gives on overflow.

/// `int(1 << 31)` as MSVC folds it: `INT_MIN`, sign-extended when added to an `int64_t`.
const HALF_WORD_BIAS: i64 = -2_147_483_648;

/// `Int128::multiply(lhs, rhs)`: the exact 128-bit product (`_mul128`).
fn multiply(lhs: i64, rhs: i64) -> i128 {
    i128::from(lhs) * i128::from(rhs)
}

/// `Int128::sign()` of a 128-bit value: -1, 0 or 1.
fn sign(v: i128) -> i32 {
    i32::from(v > 0) - i32::from(v < 0)
}

/// `Int128::sign_determinant_2x2(a11, a12, a21, a22)`: the sign of `a11 a22 - a12 a21`, exact.
fn sign_determinant_2x2(a11: i64, a12: i64, a21: i64, a22: i64) -> i32 {
    sign(multiply(a11, a22).wrapping_sub(multiply(a12, a21)))
}

/// `Int128::compare_rationals(p1, q1, p2, q2)`: the sign of `p1/q1 - p2/q2`, exact.
#[cfg_attr(not(test), allow(dead_code))]
fn compare_rationals(p1: i64, q1: i64, p2: i64, q2: i64) -> i32 {
    let invert = if (q1 < 0) == (q2 < 0) { 1 } else { -1 };
    sign(multiply(p1, q2).wrapping_sub(multiply(p2, q1))) * invert
}

/// The upper-word estimate of an operand: `(a + (1 << 31)) >> 32` with MSVC's `1 << 31`.
fn upper(a: i64) -> i64 {
    a.wrapping_add(HALF_WORD_BIAS) >> 32
}

/// `Int128::sign_determinant_2x2_filtered(a11, a12, a21, a22)`: the estimate's sign when it
/// exceeds its error bound, else the exact `sign_determinant_2x2`.
#[must_use]
pub fn sign_determinant_2x2_filtered(a11: i64, a12: i64, a21: i64, a22: i64) -> i32 {
    let (a11s, a12s, a21s, a22s) = (upper(a11), upper(a12), upper(a21), upper(a22));
    let det = a11s
        .wrapping_mul(a22s)
        .wrapping_sub(a12s.wrapping_mul(a21s));
    let err = (a11s
        .wrapping_abs()
        .wrapping_add(a12s.wrapping_abs())
        .wrapping_add(a21s.wrapping_abs())
        .wrapping_add(a22s.wrapping_abs())
        << 1)
        .wrapping_add(1);
    if det.wrapping_abs() > err {
        if det > 0 { 1 } else { -1 }
    } else {
        sign_determinant_2x2(a11, a12, a21, a22)
    }
}

/// `Int128::compare_rationals_filtered(p1, q1, p2, q2)`: the sign of `p1/q1 - p2/q2`, through the
/// upper-word estimate when both denominators' estimates are not zero, else (as the C++ does)
/// the exact sign of the determinant `p1 q2 - q1 p2`, inverted when the denominators' signs
/// differ.
#[must_use]
pub fn compare_rationals_filtered(p1: i64, q1: i64, p2: i64, q2: i64) -> i32 {
    let invert = if (q1 < 0) == (q2 < 0) { 1 } else { -1 };
    let (q1s, q2s) = (upper(q1), upper(q2));
    if q1s != 0 && q2s != 0 {
        let (p1s, p2s) = (upper(p1), upper(p2));
        let det = p1s.wrapping_mul(q2s).wrapping_sub(p2s.wrapping_mul(q1s));
        let err = (p1s
            .wrapping_abs()
            .wrapping_add(q1s.wrapping_abs())
            .wrapping_add(p2s.wrapping_abs())
            .wrapping_add(q2s.wrapping_abs())
            << 1)
            .wrapping_add(1);
        if det.wrapping_abs() > err {
            return (if det > 0 { 1 } else { -1 }) * invert;
        }
    }
    sign_determinant_2x2(p1, q1, p2, q2) * invert
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_signs() {
        assert_eq!(
            sign_determinant_2x2(i64::MAX, i64::MAX, i64::MAX, i64::MAX),
            0
        );
        assert_eq!(
            sign_determinant_2x2(i64::MIN, i64::MIN, i64::MAX, i64::MIN),
            1
        );
        assert_eq!(compare_rationals(1, 3, 1, 2), -1);
        assert_eq!(compare_rationals(1, -3, 1, 2), -1);
        assert_eq!(compare_rationals(-1, -3, 1, 2), -1);
    }

    #[test]
    fn the_estimate_subtracts_half_a_word() {
        // (2^32 + 2^31 - 2^31) >> 32 = 1, where adding 2^31 would give 1 as well; 2^31 itself
        // estimates to 0 with the folded constant (it would be 1 with +2^31).
        assert_eq!(upper(1 << 32), 0);
        assert_eq!(upper((1 << 32) + (1 << 31)), 1);
        assert_eq!(upper(0), -1);
        assert_eq!(sign_determinant_2x2_filtered(1 << 40, 0, 0, 1 << 40), 1);
        assert_eq!(sign_determinant_2x2_filtered(3, 2, 6, 4), 0);
    }
}
