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
// stl/src/xstol.cpp and xstoul.cpp (_Stolx, _Stoulx), on 2026-11-06; get_u64 and get_f32 from the
// same xlocnum (num_get::do_get for unsigned long long and float, _Parse_int_with_locale in base
// 10, _Stofx_v3) and istream (operator>> for unsigned long long and float) and stl/src/xstoull.cpp
// (_Stoullx), on 2026-11-11 by orca-ptree-port; get_bool_alpha from the same xlocnum
// (num_get::do_get for bool under boolalpha), stl/inc/xlocale (_Getloctxt) and istream
// (operator>>(bool&) and _Common_extract_with_num_get), on 2026-11-11 by orca-3mf-port-2; get_i32
// again, through the same _Parse_int_with_locale as get_u64 and a statement-by-statement _Stolx
// and _Stoulx (stl/src/xstol.cpp and xstoul.cpp), on 2026-11-11 by orca-xml-stack; their
// copyright is Microsoft Corporation's, under the Apache-2.0 WITH LLVM-exception licence.

//! `std::istringstream(text) >> value`, as MSVC 14.42's standard library reads a number in the
//! classic locale: the field `num_get` gathers, then the UCRT's `strtod` or `strtof` (orca-fmt's
//! [`strtod`](crate::parse::strtod) and [`strtof`](crate::parse::strtof)), `_Stolx` or
//! `_Stoullx`.
//!
//! The parse is the STL's, read from its source. The records of `ptree.sop1` (PTGT) replay it
//! against the engine's runtime through `orca-xml`'s `tests/ptree.rs`, as Boost's
//! `stream_translator` reads `int`, `size_t`, `float` and `double` (`parity/HAZARDS.md`,
//! `num_get`).
//!
//! An extraction gives an [`Extract`]: the value it stored, if any, and whether it set `failbit`.
//! When the stream holds only white space the sentry fails and nothing is stored (the variable
//! keeps what it held); when the field is not a number `num_get` stores 0; when it overflows, the
//! limit (`LONG_MAX`, `LONG_MIN`, or what `strtod` gives) with `failbit`.
//!
//! [`get_bool_alpha`] is `>> std::boolalpha >> b`, as the 3MF reader reads a plate's flags: the
//! classic locale's `truename` and `falsename`, matched case by case; anything else stores false
//! with `failbit`.

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

/// `std::istringstream(s) >> float`: the field `_Parse_fp_with_locale` gathers, through `strtof`
/// (`_Stofx_v3`).
#[must_use]
pub fn get_f32(s: &[u8]) -> Extract<f32> {
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
            let r = crate::parse::strtof(&ac);
            Extract {
                value: Some(r.value),
                fail: r.end == 0 || r.errno.is_some(),
                read,
            }
        }
    }
}

/// `_Max_int_dig`: the size of `num_get`'s field for an integer, its NUL included.
const MAX_INT_DIG: usize = 32;

/// `_Parse_int_with_locale` in the classic locale (no grouping, so no separator is skipped) with
/// the stream's base field unset (base 10): the field (`_Ac`, without its NUL) and the bytes read,
/// or `None` for a field with no digit (`~_Base`). A sign is kept; leading zeros are not; `0x`
/// stops the field after its `0`, since only base 0 or 16 takes it; a digit past the field's 31
/// characters is read and dropped.
fn parse_int_field(s: &[u8]) -> (Option<Vec<u8>>, usize) {
    const SRC: &[u8] = b"0123456789ABCDEFabcdef-+Xx";
    let at = |i: usize| s.get(i).copied();
    let mut i = 0usize;
    let mut ac: Vec<u8> = Vec::with_capacity(MAX_INT_DIG);
    match at(i) {
        // gather plus sign
        Some(b'+') => {
            ac.push(b'+');
            i += 1;
        }
        // gather minus sign
        Some(b'-') => {
            ac.push(b'-');
            i += 1;
        }
        _ => {}
    }
    let mut seen_digit = false; // seen a digit in input
    let mut nonzero = false; // seen a nonzero digit in input
    if at(i) == Some(b'0') {
        // leading zero; an x after it is taken only in base 0 or 16
        seen_digit = true;
        i += 1;
    }
    let dlen = 10; // _Dlen for base 10
    let pe = MAX_INT_DIG - 1; // _Pe = &_Ac[_Max_int_dig - 1]
    while let Some(c) = at(i) {
        // look for digits (the classic locale has no separators)
        let idx = atom(c);
        if idx >= dlen {
            break; // not a group separator, done
        }
        // got a digit: *_Ptr = _Src[_Idx], kept when it is not a leading zero and there is room
        let d = SRC[idx];
        if (nonzero || d != b'0') && ac.len() < pe {
            ac.push(d);
            nonzero = true;
        }
        seen_digit = true;
        i += 1;
    }
    if !seen_digit {
        return (None, i);
    }
    if !nonzero {
        ac.push(b'0'); // zero field, replace stripped zero(s)
    }
    (Some(ac), i)
}

/// `_Stoullx(s, &end, 10, &err)` (xstoull.cpp) on a NUL-terminated field: the value, how far it
/// read (0 for no digits) and whether it overflowed (`ERANGE`, the value `ULLONG_MAX`). A minus
/// sign negates modulo 2^64.
fn stoullx(s: &[u8]) -> (u64, usize, bool) {
    const BASE: u64 = 10;
    const NDIGS: isize = 20; // ndigs[10]: the digits of the largest number, 64-bits
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let mut sc = 0usize;
    while is_space(at(sc)) {
        sc += 1;
    }
    let mut sign = if at(sc) == b'-' || at(sc) == b'+' {
        sc += 1;
        at(sc - 1)
    } else {
        b'+'
    };
    let s1 = sc;
    while at(sc) == b'0' {
        // skip leading zeros
        sc += 1;
    }
    let mut x: u64 = 0;
    let mut y: u64 = 0;
    let mut dig: u64 = 0;
    let s2 = sc;
    while at(sc).is_ascii_digit() {
        // accumulate digits
        y = x;
        dig = u64::from(at(sc) - b'0'); // for overflow checking
        x = x.wrapping_mul(BASE).wrapping_add(dig);
        sc += 1;
    }
    if s1 == sc {
        // check string validity
        return (0, 0, false);
    }
    let n = isize::try_from(sc - s2).unwrap_or(isize::MAX) - NDIGS;
    let mut overflow = false;
    if n >= 0 && (0 < n || x < x.wrapping_sub(dig) || x.wrapping_sub(dig) / BASE != y) {
        // overflow
        overflow = true;
        x = u64::MAX;
        sign = b'+';
    }
    if sign == b'-' {
        // get final value
        x = 0u64.wrapping_sub(x);
    }
    (x, sc, overflow)
}

/// `std::istringstream(s) >> unsigned long long` (`size_t` on 64-bit Windows): the field
/// `_Parse_int_with_locale` gathers, through `_Stoullx`, which takes a sign and wraps a negative
/// number.
#[must_use]
pub fn get_u64(s: &[u8]) -> Extract<u64> {
    let Some(start) = sentry(s) else {
        return Extract {
            value: None,
            fail: true,
            read: s.len(),
        };
    };
    let (field, n) = parse_int_field(&s[start..]);
    let read = start + n;
    match field {
        // ditto "fails to convert the entire field"
        None => Extract {
            value: Some(0),
            fail: true,
            read,
        },
        Some(ac) => {
            let (value, end, overflow) = stoullx(&ac);
            Extract {
                value: Some(value),
                fail: end == 0 || overflow,
                read,
            }
        }
    }
}

/// `_Stoulx(s, &end, 10, &err)` (xstoul.cpp) on a NUL-terminated field: the value (an
/// `unsigned long`, 32 bits), how far it read (0 when it took no digit: `*endptr = s`) and
/// whether it overflowed (`*perr = 1`, `ERANGE`, the value `ULONG_MAX`). A minus sign negates
/// modulo 2^32. `num_get` passes base 10 here (the stream's base field is unset), so the silly
/// base and the `0x` of base 16 or 0 are not translated.
fn stoulx(s: &[u8]) -> (u32, usize, bool) {
    const BASE: u32 = 10;
    const NDIGS: isize = 10; // ndigs[10]: the digits of the largest number, 32-bits!
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let mut sc = 0usize;
    while is_space(at(sc)) {
        sc += 1;
    }
    let mut sign = if at(sc) == b'-' || at(sc) == b'+' {
        sc += 1;
        at(sc - 1)
    } else {
        b'+'
    };
    let s1 = sc;
    while at(sc) == b'0' {
        // skip leading zeros
        sc += 1;
    }
    let mut x: u32 = 0;
    let mut y: u32 = 0;
    let mut dig: u32 = 0;
    let s2 = sc;
    while at(sc).is_ascii_digit() {
        // accumulate digits
        y = x;
        dig = u32::from(at(sc) - b'0'); // for overflow checking
        x = x.wrapping_mul(BASE).wrapping_add(dig);
        sc += 1;
    }
    if s1 == sc {
        // check string validity
        return (0, 0, false);
    }
    let n = isize::try_from(sc - s2).unwrap_or(isize::MAX) - NDIGS;
    let mut overflow = false;
    if n >= 0 && (0 < n || x < x.wrapping_sub(dig) || x.wrapping_sub(dig) / BASE != y) {
        // overflow
        overflow = true;
        x = u32::MAX;
        sign = b'+'; // silence negation
    }
    if sign == b'-' {
        // get final value
        x = 0u32.wrapping_sub(x);
    }
    (x, sc, overflow)
}

/// `_Stolx(s, &end, 10, &err)` (xstol.cpp) on a NUL-terminated field: its own sign, then
/// `_Stoulx` on the rest; the value (a `long`, 32 bits), how far it read (0 when `_Stoulx` took
/// nothing: `*endptr = s`) and whether it overflowed (`*perr`): past `LONG_MAX`, or past
/// 2^31 negated, it is `LONG_MAX` or `LONG_MIN` by the sign, with `ERANGE`.
fn stolx(s: &[u8]) -> (i32, usize, bool) {
    const LONG_MAX: u32 = 0x7FFF_FFFF;
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let mut sc = 0usize;
    while is_space(at(sc)) {
        sc += 1;
    }
    let sign = if at(sc) == b'-' || at(sc) == b'+' {
        sc += 1;
        at(sc - 1)
    } else {
        b'+'
    };
    // _Stoulx's endptr is in its own text, which starts at sc; when it took nothing it is sc.
    let (x, read, mut err) = stoulx(&s[sc..]);
    let mut end = sc + read;
    if sc == end {
        end = 0;
    }
    if (end == 0 && x != 0) || (sign == b'+' && LONG_MAX < x) || (sign == b'-' && (1u32 << 31) < x)
    {
        // overflow
        err = true;
        return (if sign == b'-' { i32::MIN } else { i32::MAX }, end, err);
    }
    let value = if sign == b'-' {
        0u32.wrapping_sub(x)
    } else {
        x
    };
    (value.cast_signed(), end, err)
}

/// `std::istringstream(s) >> int`: `operator>>(int&)` reads a `long` (32 bits on Windows) with
/// `num_get::do_get`, which is the field `_Parse_int_with_locale` gathers (the one `get_u64`
/// reads), through `_Stolx`.
#[must_use]
pub fn get_i32(s: &[u8]) -> Extract<i32> {
    let Some(start) = sentry(s) else {
        return Extract {
            value: None,
            fail: true,
            read: s.len(),
        };
    };
    let (field, n) = parse_int_field(&s[start..]);
    let read = start + n;
    match field {
        // ditto "fails to convert the entire field"
        None => Extract {
            value: Some(0),
            fail: true,
            read,
        },
        Some(ac) => {
            let (value, end, err) = stolx(&ac);
            Extract {
                value: Some(value),
                fail: end == 0 || err,
                read,
            }
        }
    }
}

/// `_Getloctxt(first, last, 2, "\0false\0true", _Case_sensitive::_Yes)`: the field (0 for
/// `false`, 1 for `true`) the input matches whole, or `None`, and the bytes read. Each column is
/// tested against every field still viable; the answer is kept only when the loop stops at the
/// column that completed it.
fn getloctxt_bool(s: &[u8]) -> (Option<usize>, usize) {
    const FIELDS: [&[u8]; 2] = [b"false", b"true"];
    // one column counter for each field: set once the field is tested out
    let mut done = [false; 2];
    let mut first = 0usize;
    let mut column = 1usize;
    let mut ans: Option<usize> = None; // -2 at first, -1 after each column
    loop {
        let mut prefix = false; // seen at least one valid prefix
        for (field, name) in FIELDS.iter().enumerate() {
            if done[field] {
                continue; // skip tested columns in field
            }
            if column > name.len() {
                // matched all of field, save as possible answer
                done[field] = true;
                ans = Some(field);
            } else if first == s.len() || name[column - 1] != s[first] {
                done[field] = true; // no match, just save skip count
            } else {
                prefix = true; // still a valid prefix
            }
        }
        if !prefix || first == s.len() {
            break; // no pending prefixes or no input, give up
        }
        column += 1;
        first += 1;
        ans = None;
    }
    (ans, first)
}

/// `std::istringstream(s) >> std::boolalpha >> value`: the sentry skips white space (and fails on
/// none left, storing nothing), then `num_get::do_get` matches `true` or `false`; anything else
/// stores false and sets `failbit`.
#[must_use]
pub fn get_bool_alpha(s: &[u8]) -> Extract<bool> {
    let Some(start) = sentry(s) else {
        return Extract {
            value: None,
            fail: true,
            read: s.len(),
        };
    };
    let (field, n) = getloctxt_bool(&s[start..]);
    Extract {
        value: Some(field == Some(1)),
        fail: field.is_none(),
        read: start + n,
    }
}

#[cfg(test)]
mod tests {
    use super::{get_bool_alpha, get_f32, get_f64, get_i32, get_u64};

    // Expected values read from MSVC 14.42's xlocnum (do_get for bool, :349-368) and xlocale
    // (_Getloctxt, :3296-3338): a field is taken when the input holds it whole, whatever follows;
    // the classic names are case sensitive; a failed match stores false.
    #[test]
    fn bools_read_as_num_get_reads_them_under_boolalpha() {
        let v = |s: &str| get_bool_alpha(s.as_bytes());
        let e = |value: Option<bool>, fail: bool, read: usize| super::Extract { value, fail, read };
        assert_eq!(v("true"), e(Some(true), false, 4));
        assert_eq!(v("false"), e(Some(false), false, 5));
        assert_eq!(v("  true"), e(Some(true), false, 6));
        assert_eq!(v("truex"), e(Some(true), false, 4));
        assert_eq!(v("tru"), e(Some(false), true, 3));
        assert_eq!(v("TRUE"), e(Some(false), true, 0));
        assert_eq!(v("1"), e(Some(false), true, 0));
        assert_eq!(v("fx"), e(Some(false), true, 1));
        assert_eq!(v(""), e(None, true, 0));
        assert_eq!(v(" \t"), e(None, true, 2));
    }

    #[test]
    fn unsigned_long_longs_read_as_num_get_and_stoullx_read_them() {
        let v = |s: &str| get_u64(s.as_bytes());
        assert_eq!(v("42").value, Some(42));
        assert_eq!(v("-1").value, Some(u64::MAX));
        assert!(!v("-1").fail);
        assert_eq!(v("-18446744073709551615").value, Some(1));
        assert_eq!(v("-0").value, Some(0));
        assert_eq!(v("18446744073709551615").value, Some(u64::MAX));
        assert!(!v("18446744073709551615").fail);
        assert_eq!(v("18446744073709551616").value, Some(u64::MAX));
        assert!(v("18446744073709551616").fail);
        assert!(v("-18446744073709551616").fail);
        assert_eq!(v("000000000000000000000000000000000000007").value, Some(7));
        // 0x is not hexadecimal in base 10: the field is the 0.
        let x = v("0x10");
        assert_eq!((x.value, x.fail, x.read), (Some(0), false, 1));
        assert!(v("+").fail);
        assert_eq!(v(" \t").value, None);
    }

    #[test]
    fn floats_read_as_num_get_and_strtof_read_them() {
        let v = |s: &str| get_f32(s.as_bytes());
        assert_eq!(v("0.25").value, Some(0.25));
        assert!(!v("0.25").fail);
        assert!(v("1e39").fail);
        assert!(v("1e-46").fail);
        assert!(!v("7.1e-46").fail);
        assert_eq!(v("0x1p-1").value, Some(0.5));
        assert!(v("1e").fail);
    }

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

    // Expected values read from MSVC 14.42's xlocnum (do_get for long, _Parse_int_with_locale),
    // xstol.cpp and xstoul.cpp: _Stoulx overflows past ULONG_MAX (ndigs[10] = 10), _Stolx past
    // LONG_MAX, or past 2^31 with a minus sign; the field keeps 31 characters and drops leading
    // zeros, and num_get reads every digit.
    #[test]
    fn ints_take_the_field_num_get_gathers() {
        let e = |value: Option<i32>, fail: bool, read: usize| super::Extract { value, fail, read };
        let v = |s: &str| get_i32(s.as_bytes());
        assert_eq!(v("+5"), e(Some(5), false, 2));
        assert_eq!(v("-0"), e(Some(0), false, 2));
        assert_eq!(v("0x10"), e(Some(0), false, 1));
        assert_eq!(
            v("000000000000000000000000000000000000042"),
            e(Some(42), false, 39)
        );
        assert_eq!(v("  +2147483647 "), e(Some(i32::MAX), false, 13));
        assert_eq!(v("-2147483649"), e(Some(i32::MIN), true, 11));
        assert_eq!(v("4294967295"), e(Some(i32::MAX), true, 10));
        assert_eq!(v("-4294967295"), e(Some(i32::MIN), true, 11));
        assert_eq!(v("4294967296"), e(Some(i32::MAX), true, 10));
        assert_eq!(v("-4294967296"), e(Some(i32::MIN), true, 11));
        let long = "1234567890".repeat(4);
        assert_eq!(v(&long), e(Some(i32::MAX), true, 40));
        assert_eq!(v(&format!("-{long}")), e(Some(i32::MIN), true, 41));
        assert_eq!(v("+-5"), e(Some(0), true, 1));
        assert_eq!(v("- 5"), e(Some(0), true, 1));
    }
}
