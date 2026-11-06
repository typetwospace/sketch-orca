// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.
//
// Carries Microsoft STL code: get_f64 and get_i32 are translated from stl/inc/xlocnum of MSVC
// 14.42.34433 (num_get::do_get for long and double, _Parse_int_with_locale and
// _Parse_fp_with_locale, in the classic locale: no grouping, '.'), stl/inc/istream
// (operator>>(int&), _Common_extract_with_num_get and the sentry's skipping of white space) and
// stl/src/xstol.cpp and xstoul.cpp (_Stolx, _Stoulx), on 2026-11-06; their copyright is Microsoft
// Corporation's, under the Apache-2.0 WITH LLVM-exception licence.

//! `std::istringstream(text) >> value`, as MSVC 14.42's standard library reads a number in the
//! classic locale: the field `num_get` gathers, then the UCRT's `strtod` (orca-fmt's
//! [`strtod`](crate::parse::strtod)) or `_Stolx`.
//!
//! The parse is the STL's, read from its source; unlike the rest of this crate it is not yet
//! replayed against the engine's runtime (`parity/HAZARDS.md`, `num_get`).
//!
//! An extraction gives an [`Extract`]: the value it stored, if any, and whether it set `failbit`.
//! When the stream holds only white space the sentry fails and nothing is stored (the variable
//! keeps what it held); when the field is not a number `num_get` stores 0; when it overflows, the
//! limit (`LONG_MAX`, `LONG_MIN`, or what `strtod` gives) with `failbit`.

/// What `stream >> value` did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Extract<T> {
    /// The value stored, or `None` when the sentry failed and the variable keeps its value.
    pub value: Option<T>,
    /// Whether `failbit` is set afterwards (`stream.fail()`).
    pub fail: bool,
    /// The bytes read, white space included.
    pub read: usize,
}

impl<T: Copy> Extract<T> {
    /// The variable after the extraction, given what it held before.
    #[must_use]
    pub fn or(self, before: T) -> T {
        self.value.unwrap_or(before)
    }
}

/// `isspace` of the classic `ctype<char>`: space, `\t`, `\n`, `\v`, `\f` and `\r`.
#[must_use]
pub fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')
}

/// The sentry's skip of white space: the index of the first byte that is not, or `None` at the
/// end (the sentry then sets `eofbit | failbit`).
fn sentry(s: &[u8]) -> Option<usize> {
    s.iter().position(|&c| !is_space(c))
}

/// `_Find_elem(_Atoms, c)` on "0123456789ABCDEFabcdef-+XxPp" (28 when it is not there).
fn atom(c: u8) -> usize {
    b"0123456789ABCDEFabcdef-+XxPp"
        .iter()
        .position(|&a| a == c)
        .unwrap_or(28)
}

/// `_MAX_SIG_DIG_V2`.
const MAX_SIG_DIG: i64 = 768;

/// `_Parse_fp_with_locale` in the classic locale: the field (`_Ac`) and the bytes read, or `None`
/// for a field with no digit.
#[allow(clippy::too_many_lines)]
fn parse_fp_field(s: &[u8]) -> (Option<Vec<u8>>, usize) {
    const SRC: &[u8] = b"0123456789ABCDEFabcdef-+XxPp";
    let mut i = 0usize;
    let mut ac: Vec<u8> = Vec::new();
    let at = |i: usize| s.get(i).copied();
    match at(i) {
        Some(b'+') => {
            ac.push(b'+');
            i += 1;
        }
        Some(b'-') => {
            ac.push(b'-');
            i += 1;
        }
        _ => {}
    }
    ac.push(b'0'); // the backstop
    let mut parse_hex = false;
    let mut seen_digit = false;
    if at(i) == Some(b'0') {
        i += 1;
        if at(i).is_none() {
            // "0" alone.
            return (Some(ac), i);
        }
        if matches!(at(i), Some(b'x' | b'X')) {
            parse_hex = true;
            i += 1;
            ac.push(b'x');
        } else {
            seen_digit = true;
        }
    }
    let mut has_unaccumulated_digits = false;
    let mut significant: i64 = 0;
    let mut power_of_rep_base: i64 = 0;
    let digit_end = if parse_hex { 22 } else { 10 };
    while let Some(c) = at(i) {
        let idx = atom(c);
        if idx >= digit_end {
            break;
        }
        if significant >= MAX_SIG_DIG {
            power_of_rep_base += 1;
            if idx > 0 {
                has_unaccumulated_digits = true;
            }
        } else if idx != 0 || significant != 0 {
            ac.push(SRC[idx]);
            significant += 1;
        }
        seen_digit = true;
        i += 1;
    }
    if parse_hex && seen_digit && significant == 0 {
        ac.push(b'0');
    }
    if at(i) == Some(b'.') {
        ac.push(b'.');
        i += 1;
    }
    if significant == 0 {
        while at(i) == Some(b'0') {
            power_of_rep_base -= 1;
            seen_digit = true;
            i += 1;
        }
    }
    while let Some(c) = at(i) {
        let idx = atom(c);
        if idx >= digit_end {
            break;
        }
        if significant < MAX_SIG_DIG {
            ac.push(SRC[idx]);
            significant += 1;
        } else if idx > 0 {
            has_unaccumulated_digits = true;
        }
        seen_digit = true;
        i += 1;
    }
    if has_unaccumulated_digits {
        let n = ac.len();
        let last = if ac[n - 1] == b'.' { n - 2 } else { n - 1 };
        if ac[last] == b'0' || ac[last] == (if parse_hex { b'8' } else { b'5' }) {
            ac[last] += 1;
        }
    }
    let (lower, upper) = if parse_hex {
        (b'p', b'P')
    } else {
        (b'e', b'E')
    };
    let mut exponent_part: i64 = 0;
    if seen_digit && matches!(at(i), Some(c) if c == lower || c == upper) {
        i += 1;
        seen_digit = false;
        let mut negative = false;
        match at(i) {
            Some(b'+') => i += 1,
            Some(b'-') => {
                negative = true;
                i += 1;
            }
            _ => {}
        }
        while at(i) == Some(b'0') {
            seen_digit = true;
            i += 1;
        }
        while let Some(c) = at(i) {
            let idx = atom(c);
            if idx >= 10 {
                break;
            }
            #[allow(clippy::cast_possible_wrap)]
            let d = idx as i64;
            exponent_part = if exponent_part < i64::MAX / 10
                || (exponent_part == i64::MAX / 10 && d <= i64::MAX % 10)
            {
                exponent_part * 10 + d
            } else {
                i64::MAX
            };
            seen_digit = true;
            i += 1;
        }
        if negative {
            exponent_part = -exponent_part;
        }
    }
    if !seen_digit {
        return (None, i);
    }
    let exp_abs_bound: i64 = if parse_hex { 4200 } else { 1100 };
    let exp_rep_abs_bound: i64 = if parse_hex { 4200 / 4 } else { 1100 };
    let scale = |p: i64| if parse_hex { p * 4 } else { p };
    let mut power_adjusted = power_of_rep_base;
    loop {
        if exponent_part >= 0
            && power_adjusted >= 0
            && (exponent_part >= exp_abs_bound || power_adjusted >= exp_rep_abs_bound)
        {
            exponent_part = exp_abs_bound;
            break;
        } else if exponent_part <= 0
            && power_adjusted <= 0
            && (exponent_part <= -exp_abs_bound || power_adjusted <= -exp_rep_abs_bound)
        {
            exponent_part = -exp_abs_bound;
            break;
        } else if exponent_part.abs() <= exp_abs_bound && power_adjusted.abs() <= exp_rep_abs_bound
        {
            exponent_part += scale(power_adjusted);
            if exponent_part > exp_abs_bound {
                exponent_part = exp_abs_bound;
            } else if exponent_part < -exp_abs_bound {
                exponent_part = -exp_abs_bound;
            }
            break;
        }
        let round_up = if parse_hex {
            (exponent_part.abs() - 1) / 4 + 1
        } else {
            exponent_part.abs()
        };
        let adjustment = round_up.min(power_of_rep_base.abs());
        if exponent_part >= 0 {
            exponent_part -= scale(adjustment);
            power_adjusted += adjustment;
        } else {
            exponent_part += scale(adjustment);
            power_adjusted -= adjustment;
        }
    }
    if exponent_part != 0 {
        ac.push(if parse_hex { b'p' } else { b'e' });
        if exponent_part < 0 {
            ac.push(b'-');
        }
        ac.extend_from_slice(exponent_part.unsigned_abs().to_string().as_bytes());
    }
    (Some(ac), i)
}

/// `std::istringstream(s) >> double`.
#[must_use]
pub fn get_f64(s: &[u8]) -> Extract<f64> {
    let Some(start) = sentry(s) else {
        return Extract {
            value: None,
            fail: true,
            read: s.len(),
        };
    };
    let (field, n) = parse_fp_field(&s[start..]);
    let read = start + n;
    match field {
        None => Extract {
            value: Some(0.0),
            fail: true,
            read,
        },
        Some(ac) => {
            let r = crate::parse::strtod(&ac);
            Extract {
                value: Some(r.value),
                fail: r.end == 0 || r.errno.is_some(),
                read,
            }
        }
    }
}

/// `std::istringstream(s) >> int` (`long`, which is 32 bits on Windows, through `_Stolx`).
#[must_use]
pub fn get_i32(s: &[u8]) -> Extract<i32> {
    let Some(start) = sentry(s) else {
        return Extract {
            value: None,
            fail: true,
            read: s.len(),
        };
    };
    // _Parse_int_with_locale, base 10, no grouping: a sign, then digits (leading zeros dropped).
    let t = &s[start..];
    let mut i = 0usize;
    let negative = match t.first() {
        Some(b'-') => {
            i += 1;
            true
        }
        Some(b'+') => {
            i += 1;
            false
        }
        _ => false,
    };
    let digits_start = i;
    let mut magnitude: u64 = 0;
    let mut overflow = false;
    while let Some(&c) = t.get(i) {
        if !c.is_ascii_digit() {
            break;
        }
        magnitude = magnitude * 10 + u64::from(c - b'0');
        if magnitude > u64::from(u32::MAX) {
            // _Stoulx: ULONG_MAX, ERANGE.
            overflow = true;
            magnitude = u64::from(u32::MAX) + 1;
        }
        i += 1;
    }
    let read = start + i;
    if i == digits_start {
        return Extract {
            value: Some(0),
            fail: true,
            read,
        };
    }
    // _Stolx: the sign, then LONG_MIN or LONG_MAX for what does not fit.
    let limit = if negative {
        1u64 << 31
    } else {
        (1u64 << 31) - 1
    };
    if overflow || magnitude > limit {
        let value = if negative { i32::MIN } else { i32::MAX };
        return Extract {
            value: Some(value),
            fail: true,
            read,
        };
    }
    #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
    let value = if negative {
        (magnitude as i64).wrapping_neg() as i32
    } else {
        magnitude as i32
    };
    Extract {
        value: Some(value),
        fail: false,
        read,
    }
}

#[cfg(test)]
mod tests {
    use super::{get_f64, get_i32};

    #[test]
    fn doubles_read_as_num_get_reads_them() {
        let v = |s: &str| get_f64(s.as_bytes());
        assert_eq!(v("0.2").value, Some(0.2));
        assert_eq!(v("  50%").value, Some(50.0));
        assert!(!v("50%").fail);
        assert_eq!(v("0.001").value, Some(0.001));
        assert_eq!(v("-0").value.map(f64::to_bits), Some((-0.0f64).to_bits()));
        assert_eq!(v("1e5x").value, Some(1e5));
        assert_eq!(v("0x10").value, Some(16.0));
        assert_eq!(
            v("abc"),
            super::Extract {
                value: Some(0.0),
                fail: true,
                read: 0
            }
        );
        assert_eq!(v("   ").value, None);
        assert!(v("1e999").fail);
        assert_eq!(v(".5").value, Some(0.5));
        assert!(v("e5").fail);
    }

    #[test]
    fn ints_read_as_num_get_and_stolx_read_them() {
        let v = |s: &str| get_i32(s.as_bytes());
        assert_eq!(v("42").value, Some(42));
        assert_eq!(v(" -7x").value, Some(-7));
        assert_eq!(v("0.9").value, Some(0));
        assert_eq!(v("2147483648").value, Some(i32::MAX));
        assert!(v("2147483648").fail);
        assert_eq!(v("-2147483648").value, Some(i32::MIN));
        assert!(!v("-2147483648").fail);
        assert_eq!(v("-99999999999").value, Some(i32::MIN));
        assert_eq!(v("x").value, Some(0));
        assert!(v("x").fail);
        assert_eq!(v("").value, None);
    }
}
