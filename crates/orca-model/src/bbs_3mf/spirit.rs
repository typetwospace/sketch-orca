// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Boost code: qi::int_ for int in base 10 (extract_sign and int_parser_impl's call of
// boost/spirit/home/qi/numeric/numeric_utils.hpp; extract_int<T, 10, 1, -1>, int_extractor and
// the positive and negative accumulators of qi/numeric/detail/numeric_utils.hpp), of Boost 1.84.0,
// the Boost OrcaSlicer v2.4.2's dependency build pins, translated on 2026-11-10. Copyright (c)
// 2001-2011 Joel de Guzman, (c) 2001-2011 Hartmut Kaiser, (c) 2011 Jan Frederick Eick, (c) 2011
// Christopher Jefferson and (c) 2006 Stephen Nutt, under the Boost Software License 1.0
// (licenses/BSL-1.0.txt).

//! Boost Spirit's `qi::parse(first, last, qi::int_, value)` for an `int`, as the 3MF reader's
//! attribute getter calls it (`bbs_3mf.cpp:508-513`).
//!
//! A `-` or `+`, then leading zeros, then digits; it stops at the first byte that is not a digit
//! and needs no more of the input. It fails (and the getter keeps 0) when no digit follows the
//! sign. On an overflow it fails too, but only after assigning the value read so far: the digits
//! before the one that overflowed (`2147483648` gives 214748364), which the getter keeps.
//!
//! The first digit is added unchecked, and so are the next ones while `count` (the leading zeros,
//! plus the digits after the first, not counting the one being added) is below 8, `digits10 - 1`
//! for `int`; from there each digit is checked against `INT_MAX` (or `INT_MIN` after a `-`).

/// The digits after the first that `int_extractor` adds without a check: `digits10 - 1` of `int`.
const OVERFLOW_FREE: usize = 8;

/// `positive_accumulator` or `negative_accumulator`, checked or not: `None` on an overflow.
fn add(n: i32, digit: i32, negative: bool, checked: bool) -> Option<i32> {
    if !checked {
        return Some(if negative {
            n.wrapping_mul(10).wrapping_sub(digit)
        } else {
            n.wrapping_mul(10).wrapping_add(digit)
        });
    }
    if negative {
        let val = i32::MIN / 10;
        if n < val {
            return None;
        }
        let tmp = n * 10;
        if tmp < i32::MIN + digit {
            return None;
        }
        Some(tmp - digit)
    } else {
        let val = i32::MAX / 10;
        if n > val {
            return None;
        }
        let tmp = n * 10;
        if tmp > i32::MAX - digit {
            return None;
        }
        Some(tmp + digit)
    }
}

/// `extract_int<int, 10, 1, -1, Accumulator>::parse_main(first, last, attr)`: whether it took a
/// number; `attr` is assigned when it did, and on an overflow.
fn parse_main(s: &[u8], negative: bool, attr: &mut i32) -> bool {
    let mut it = 0usize;
    let mut count = 0usize;
    // Leading zeros.
    while it < s.len() && s[it] == b'0' {
        it += 1;
        count += 1;
    }
    if it == s.len() {
        if count == 0 {
            return false;
        }
        *attr = 0;
        return true;
    }
    let mut val = 0i32;
    let ch = s[it];
    if !ch.is_ascii_digit() {
        if count == 0 {
            return false;
        }
        *attr = val;
        return true;
    }
    // The first digit: int_extractor::call(ch, 0, val), unchecked.
    val = add(val, i32::from(ch - b'0'), negative, false).unwrap_or(val);
    it += 1;
    while it < s.len() {
        let ch = s[it];
        if !ch.is_ascii_digit() {
            break;
        }
        match add(val, i32::from(ch - b'0'), negative, count >= OVERFLOW_FREE) {
            Some(v) => val = v,
            None => {
                *attr = val;
                return false;
            }
        }
        it += 1;
        count += 1;
    }
    *attr = val;
    true
}

/// `qi::parse(first, last, qi::int_, value)`: whether it took a number; `value` as Spirit leaves
/// it (unchanged when no digit was read, the digits before the overflow on one).
pub fn parse_int(s: &[u8], value: &mut i32) -> bool {
    if s.is_empty() {
        return false;
    }
    // extract_sign
    let mut first = 0usize;
    let negative = s[0] == b'-';
    if negative || s[0] == b'+' {
        first = 1;
    }
    parse_main(&s[first..], negative, value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(s: &str) -> (bool, i32) {
        let mut v = 0;
        let ok = parse_int(s.as_bytes(), &mut v);
        (ok, v)
    }

    // Expected values read from Boost 1.84's qi/numeric/detail/numeric_utils.hpp
    // (extract_int<T, Radix, 1, -1>::parse_main, :417-507, and the accumulators, :138-200) and
    // qi/numeric/numeric_utils.hpp (extract_sign, :28-40). Not replayed: no record holds these
    // but the one probed through build items (parity/HAZARDS.md, the 3MF table).
    #[test]
    fn signs_digits_and_stops() {
        assert_eq!(read("42"), (true, 42));
        assert_eq!(read("+7x"), (true, 7));
        assert_eq!(read("-12"), (true, -12));
        assert_eq!(read("007"), (true, 7));
        assert_eq!(read("00x"), (true, 0));
        assert_eq!(read("0"), (true, 0));
        assert_eq!(read(""), (false, 0));
        assert_eq!(read("-"), (false, 0));
        assert_eq!(read("+-5"), (false, 0));
        assert_eq!(read(" 1"), (false, 0));
        assert_eq!(read("x"), (false, 0));
    }

    #[test]
    fn an_overflow_keeps_the_digits_before_it() {
        assert_eq!(read("2147483647"), (true, i32::MAX));
        assert_eq!(read("-2147483648"), (true, i32::MIN));
        assert_eq!(read("2147483648"), (false, 214_748_364));
        assert_eq!(read("2147483649x"), (false, 214_748_364));
        assert_eq!(read("-2147483649"), (false, -214_748_364));
        assert_eq!(read("0214748364"), (true, 214_748_364));
        assert_eq!(read("99999999999"), (false, 999_999_999));
    }
}
