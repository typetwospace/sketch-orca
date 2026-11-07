// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Boost code: boost/spirit/home/qi/numeric/detail/real_impl.hpp, real_policies.hpp,
// detail/numeric_utils.hpp (extract_int, extract_uint, the accumulators) and
// boost/spirit/home/support/detail/pow10.hpp of Boost 1.84.0, translated on 2026-11-07. Copyright
// (c) 2001-2011 Joel de Guzman, Hartmut Kaiser and the Spirit contributors, under the Boost
// Software License, version 1.0 (BSL-1.0; licenses/BSL-1.0.txt).

//! Spirit Qi's number parsers as the placeholder grammar uses them: `strict_double` (a
//! `real_parser<double>` with `strict_real_policies` and neither NaN nor infinity, so a dot or an
//! exponent is needed) and `int_` (`extract_int<int, 10, 1, -1>`).
//!
//! Each takes the text and a position already past the white space the parser skips, and gives
//! the value and the position after it, or `None` where Spirit's parser fails. The double is
//! built as `real_impl` builds it: at most 17 digits of the whole part accumulated in a `u64`
//! (the rest counted as an exponent), the fraction's digits accumulated with overflow checks,
//! then one multiplication or division by the `pow10` table's literal (`1e0` to `1e308`), so the
//! value is Spirit's, not a correctly rounded one.

use std::sync::OnceLock;

/// `pow10<double>(n)`: the table of literals `1e0` to `1e308` Spirit reads it from.
fn pow10(n: u32) -> f64 {
    static TABLE: OnceLock<Vec<f64>> = OnceLock::new();
    let t = TABLE.get_or_init(|| {
        // Each literal is the double nearest to 10^n, as the compiler reads it and as Rust's
        // correctly rounded parse gives it.
        (0..=308)
            .map(|k| format!("1e{k}").parse::<f64>().expect("a literal"))
            .collect()
    });
    t[n as usize]
}

/// `std::numeric_limits<double>::max_exponent10`.
const MAX_EXP10: i32 = 308;
/// `std::numeric_limits<double>::min_exponent10`.
const MIN_EXP10: i32 = -307;

/// `traits::scale(exp, n, acc_n)`: false where the exponent leaves the double's range.
fn scale(exp: i32, acc: u64) -> Option<f64> {
    if exp >= 0 {
        if exp > MAX_EXP10 {
            return None;
        }
        Some(acc as f64 * pow10(exp as u32))
    } else if exp < MIN_EXP10 {
        // compensate_roundoff for an integral accumulator.
        let mut n = ((acc / 10) * 10) as f64;
        n += (acc % 10) as f64;
        n /= pow10((-MIN_EXP10) as u32);
        let exp = exp + (-MIN_EXP10);
        if exp < MIN_EXP10 {
            return None;
        }
        Some(n / pow10((-exp) as u32))
    } else {
        Some(acc as f64 / pow10((-exp) as u32))
    }
}

/// `extract_uint<u64, 10, 1, 17, false, true>` (`parse_n`): leading zeros skipped (at most 17),
/// then digits accumulated without a check while fewer than 17 digits, zeros included, have been
/// read.
fn parse_n(s: &[u8], p: &mut usize) -> Option<u64> {
    const MAX_DIGITS: usize = 17;
    let mut it = *p;
    let mut leading_zeros = 0usize;
    while it < s.len() && s[it] == b'0' && leading_zeros < MAX_DIGITS {
        it += 1;
        leading_zeros += 1;
    }
    let mut val: u64 = 0;
    let mut count = 0usize;
    while count + leading_zeros < MAX_DIGITS && it < s.len() && s[it].is_ascii_digit() {
        val = val.wrapping_mul(10).wrapping_add(u64::from(s[it] - b'0'));
        it += 1;
        count += 1;
    }
    if count + leading_zeros >= 1 {
        *p = it;
        Some(val)
    } else {
        None
    }
}

/// `extract_uint<unused_type, 10, 1, -1>`: the digits at the position, skipped; how many.
fn skip_digits(s: &[u8], p: &mut usize) -> usize {
    let start = *p;
    while *p < s.len() && s[*p].is_ascii_digit() {
        *p += 1;
    }
    *p - start
}

/// `parse_frac_n`: `extract_uint<u64, 10, 1, -1, true, true>` onto `acc` (each digit checked; at
/// an overflow it stops there and succeeds), the digits it took counted, the rest skipped.
fn parse_frac_n(s: &[u8], p: &mut usize, acc: &mut u64, frac_digits: &mut i32) -> bool {
    let savef = *p;
    let mut it = *p;
    let mut val = *acc;
    let mut count = 0usize;
    while it < s.len() && s[it].is_ascii_digit() {
        let digit = u64::from(s[it] - b'0');
        // positive_accumulator::add, checked.
        if val > u64::MAX / 10 {
            break;
        }
        let tmp = val * 10;
        if tmp > u64::MAX - digit {
            break;
        }
        val = tmp + digit;
        it += 1;
        count += 1;
    }
    let overflowed = it < s.len() && s[it].is_ascii_digit();
    if count == 0 && !overflowed {
        return false;
    }
    *acc = val;
    *p = it;
    *frac_digits = (*p - savef) as i32;
    skip_digits(s, p);
    true
}

/// `extract_int<int, 10, 1, -1>`: an optional sign and digits, failing on an overflow (and then
/// leaving the position where it was).
pub fn parse_int(s: &[u8], p: &mut usize) -> Option<i32> {
    if *p == s.len() {
        return None;
    }
    let save = *p;
    let mut it = *p;
    let neg = match s[it] {
        b'-' => {
            it += 1;
            true
        }
        b'+' => {
            it += 1;
            false
        }
        _ => false,
    };
    let mut leading_zeros = 0usize;
    while it < s.len() && s[it] == b'0' {
        it += 1;
        leading_zeros += 1;
    }
    let mut val: i64 = 0;
    let mut count = 0usize;
    while it < s.len() && s[it].is_ascii_digit() {
        let digit = i64::from(s[it] - b'0');
        val = if neg {
            val * 10 - digit
        } else {
            val * 10 + digit
        };
        if val > i64::from(i32::MAX) || val < i64::from(i32::MIN) {
            *p = save;
            return None;
        }
        it += 1;
        count += 1;
    }
    if count + leading_zeros >= 1 {
        *p = it;
        Some(val as i32)
    } else {
        *p = save;
        None
    }
}

/// `strict_double`: `real_impl<double, strict_real_policies_without_nan_inf>::parse`.
pub fn parse_strict_double(s: &[u8], p: &mut usize) -> Option<f64> {
    if *p == s.len() {
        return None;
    }
    let save = *p;
    let mut first = *p;
    // parse_sign
    let neg = match s[first] {
        b'-' => {
            first += 1;
            true
        }
        b'+' => {
            first += 1;
            false
        }
        _ => false,
    };
    let mut acc: u64 = 0;
    let got_a_number = match parse_n(s, &mut first) {
        Some(v) => {
            acc = v;
            true
        }
        None => false,
    };
    let mut excess_n: i32 = 0;
    if got_a_number {
        excess_n = skip_digits(s, &mut first) as i32;
    }
    // (allow_leading_dot: a missing whole part goes on to the dot.)
    let mut frac_digits: i32 = 0;
    let e_pos;
    let e_hit;
    if first < s.len() && s[first] == b'.' {
        first += 1;
        if excess_n != 0 {
            skip_digits(s, &mut first);
        } else if parse_frac_n(s, &mut first, &mut acc, &mut frac_digits) {
        } else if !got_a_number {
            *p = save;
            return None;
        }
        // (allow_trailing_dot: a number without a fraction keeps its dot.)
        e_pos = first;
        e_hit = parse_exp(s, &mut first);
    } else {
        if !got_a_number {
            *p = save;
            return None;
        }
        e_pos = first;
        e_hit = parse_exp(s, &mut first);
        // expect_dot
        if !e_hit {
            *p = save;
            return None;
        }
    }
    let n;
    if e_hit {
        let mut q = first;
        match parse_int(s, &mut q) {
            Some(exp) => {
                first = q;
                // MSVC's int addition wraps.
                n = scale(exp.wrapping_add(excess_n).wrapping_sub(frac_digits), acc)?;
            }
            None => {
                first = e_pos;
                n = scale(-frac_digits, acc).expect("scale(-frac) always fits");
            }
        }
    } else if frac_digits != 0 {
        n = scale(-frac_digits, acc).expect("scale(-frac) always fits");
    } else if excess_n != 0 {
        n = scale(excess_n, acc)?;
    } else {
        n = acc as f64;
    }
    *p = first;
    Some(if neg { -n.abs() } else { n })
}

/// `parse_exp`: an `e` or `E`.
fn parse_exp(s: &[u8], p: &mut usize) -> bool {
    if *p < s.len() && (s[*p] == b'e' || s[*p] == b'E') {
        *p += 1;
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(t: &str) -> Option<(f64, usize)> {
        let mut p = 0;
        parse_strict_double(t.as_bytes(), &mut p).map(|v| (v, p))
    }

    #[test]
    fn strict_doubles_need_a_dot_or_an_exponent() {
        assert_eq!(d("0.4"), Some((0.4, 3)));
        assert_eq!(d(".5x"), Some((0.5, 2)));
        assert_eq!(d("5."), Some((5.0, 2)));
        assert_eq!(d("5"), None);
        assert_eq!(d("5e3"), Some((5000.0, 3)));
        assert_eq!(d("1e"), Some((1.0, 1)));
        assert_eq!(d("1.5e+2"), Some((150.0, 6)));
        assert_eq!(d("."), None);
        assert_eq!(d("1e400"), None);
        // Past -307 the accumulator is divided twice: 1e-400 underflows to zero, and only past
        // -614 does scale refuse.
        assert_eq!(d("1e-400"), Some((0.0, 6)));
        assert_eq!(d("1e-700"), None);
        assert_eq!(d("1e-310").map(|v| v.0 > 0.0), Some(true));
        // 17 digits accumulated, the other 4 an exponent, the fraction skipped.
        assert_eq!(
            d("123456789012345678901.5"),
            Some((12345678901234567u64 as f64 * 1e4, 23))
        );
    }

    #[test]
    fn ints_fail_on_overflow() {
        let i = |t: &str| {
            let mut p = 0;
            parse_int(t.as_bytes(), &mut p).map(|v| (v, p))
        };
        assert_eq!(i("2147483647"), Some((i32::MAX, 10)));
        assert_eq!(i("2147483648"), None);
        assert_eq!(i("-2147483648"), Some((i32::MIN, 11)));
        assert_eq!(i("007x"), Some((7, 3)));
        assert_eq!(i("x"), None);
    }
}
