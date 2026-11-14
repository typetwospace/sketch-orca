// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Boost code: boost::lexical_cast<int> from a std::string, as Boost 1.84.0 does it in the
// classic locale (shr_signed of boost/lexical_cast/detail/converter_lexical_streams.hpp and
// lcast_ret_unsigned's convert, main_convert_loop and main_convert_iteration of
// lcast_unsigned_converters.hpp), translated on 2026-11-11. Copyright Kevlin Henney, 2000-2005,
// Alexander Nasonov, 2006-2010, and Antony Polukhin, 2011-2023, under the Boost Software License,
// version 1.0 (licenses/BSL-1.0.txt).

//! `boost::lexical_cast<int>(token)`, as the 3MF reader's `parse_int_list` calls it
//! (`bbs_3mf.cpp:554`): the whole token must be an optional `-` or `+` and then decimal digits, no
//! white space; the digits are read as an `unsigned int` from the last to the first, refused on
//! the first that would overflow it (a zero digit never overflows, so leading zeros are taken);
//! then the value must fit `int` (2147483648 after a `-`). A refused token is a
//! `bad_lexical_cast`, which `parse_int_list` catches and drops (`:553-556`), so here it is
//! `None`.
//!
//! Boost tests the global locale: in the classic one (the engine never sets another) the digits
//! are read with no thousands separator, as here.

/// `lcast_ret_unsigned<Traits, unsigned int, char>(value, begin, end).convert()` in the classic
/// locale: the digits as an `unsigned int`, or `None` when one is not a digit or the value
/// overflows.
fn lcast_ret_unsigned(digits: &[u8]) -> Option<u32> {
    let maxv = u32::MAX;
    // --m_end; m_value = 0;
    let (&last, rest) = digits.split_last()?;
    if !last.is_ascii_digit() {
        return None;
    }
    let mut value = u32::from(last - b'0');
    let mut multiplier: u32 = 1;
    let mut multiplier_overflowed = false;
    // main_convert_loop: from the digit before the last back to the first
    for &c in rest.iter().rev() {
        multiplier_overflowed = multiplier_overflowed || (maxv / 10 < multiplier);
        multiplier = multiplier.wrapping_mul(10);
        let dig_value = u32::from(c.wrapping_sub(b'0'));
        let new_sub_value = multiplier.wrapping_mul(dig_value);
        // We must correctly handle situations like `000000000000000000000000000001`.
        // So we take care of overflow only if `dig_value` is not '0'.
        if !c.is_ascii_digit()
            || (dig_value != 0
                && (multiplier_overflowed
                    || maxv / dig_value < multiplier
                    || maxv - new_sub_value < value))
        {
            return None;
        }
        value = value.wrapping_add(new_sub_value);
    }
    Some(value)
}

/// `boost::lexical_cast<int>(token)` (`shr_signed`): the value, or `None` for a
/// `bad_lexical_cast`.
#[must_use]
pub fn lexical_cast_int(token: &[u8]) -> Option<i32> {
    let (&first, _) = token.split_first()?;
    let has_minus = first == b'-';
    let start = usize::from(has_minus || first == b'+');
    let out_tmp = lcast_ret_unsigned(&token[start..])?;
    if has_minus {
        // comp_val = 1 << digits
        let comp_val = 1u32 << 31;
        if out_tmp > comp_val {
            return None;
        }
        #[allow(clippy::cast_possible_wrap)]
        Some(0u32.wrapping_sub(out_tmp) as i32)
    } else {
        let comp_val = i32::MAX.unsigned_abs();
        if out_tmp > comp_val {
            return None;
        }
        #[allow(clippy::cast_possible_wrap)]
        Some(out_tmp as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::lexical_cast_int;

    // Expected values read from Boost 1.84's converter_lexical_streams.hpp (shr_signed, :518-541)
    // and lcast_unsigned_converters.hpp (convert, :183-190; main_convert_iteration, :254-277).
    #[test]
    fn whole_tokens_of_digits_within_int() {
        let v = |s: &str| lexical_cast_int(s.as_bytes());
        assert_eq!(v("42"), Some(42));
        assert_eq!(v("+7"), Some(7));
        assert_eq!(v("-7"), Some(-7));
        assert_eq!(v("0007"), Some(7));
        assert_eq!(v("00000000000000000000000000000001"), Some(1));
        assert_eq!(v("2147483647"), Some(i32::MAX));
        assert_eq!(v("2147483648"), None);
        assert_eq!(v("-2147483648"), Some(i32::MIN));
        assert_eq!(v("-2147483649"), None);
        assert_eq!(v("4294967295"), None);
        assert_eq!(v("4294967296"), None);
        assert_eq!(v("-0"), Some(0));
        assert_eq!(v(""), None);
        assert_eq!(v("-"), None);
        assert_eq!(v("+"), None);
        assert_eq!(v(" 1"), None);
        assert_eq!(v("1 "), None);
        assert_eq!(v("1x"), None);
        assert_eq!(v("+-1"), None);
        assert_eq!(v("0x10"), None);
    }
}
