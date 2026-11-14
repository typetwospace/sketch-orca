// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries fast_float code: fastfloat_strncasecmp, parse_number_string, parse_infnan and
// from_chars_advanced of deps_src/fast_float/fast_float.h, as OrcaSlicer v2.4.2 bundles it,
// translated on 2026-11-10. fast_float by Daniel Lemire and João Paulo Magalhaes, with
// contributions from Eugene Golushkov, Maksim Kita, Marcin Wojdyr, Neal Richardson, Tim Paine and
// Fabio Pellacini, under the MIT licence (licenses/MIT-fast_float.txt).

//! `fast_float::from_chars(first, last, value)` for a `float` in `chars_format::general`, as the
//! 3MF reader's attribute getter calls it (`bbs_3mf.cpp:500-506`).
//!
//! The grammar is fast_float's (`parse_number_string`, `fast_float.h:522-654`): an optional `-`
//! (no `+`, no leading space), digits with an optional `.` and at least one digit in all, then an
//! exponent (`e` or `E`, an optional sign, digits) that is left out when it has no digit. What it
//! does not take goes to `parse_infnan` (`:2338-2376`): `nan`, `inf` or `infinity` in any case
//! with an optional `-`, and `nan(<letters, digits, _>)`. Nothing taken leaves the value as it
//! was.
//!
//! The value is the correctly rounded `float` of the characters the grammar takes, which
//! fast_float computes with its fast path, Eisel-Lemire and its long-decimal path; here Rust's own
//! `f32` parse of the same characters gives it, so those paths are not translated. One difference
//! is left: fast_float stops adding exponent digits once the exponent reaches 0x10000 (`:597`),
//! so a number whose exponent passes about 655359 and is balanced by as many mantissa digits reads
//! otherwise there (`parity/HAZARDS.md`, the 3MF table).

/// `from_chars_result`: where the parse stopped, and whether it took anything (`ec` clear).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FromCharsResult {
    /// `ptr`, as an offset into the input.
    pub ptr: usize,
    /// `ec == std::errc()`: a number, an infinity or a NaN was taken and the value written.
    pub ok: bool,
}

/// `fastfloat_strncasecmp(input1, input2, length)`: the bytes' differences OR-ed together are 0
/// or 32 (`fast_float.h:180-187`).
fn strncasecmp(input1: &[u8], input2: &[u8], length: usize) -> bool {
    let mut running_diff = 0u8;
    for i in 0..length {
        running_diff |= input1[i] ^ input2[i];
    }
    running_diff == 0 || running_diff == 32
}

/// `parse_number_string(p, pend, options)` for `chars_format::general` and `.`: where the number
/// it takes ends (`lastmatch`), or `None` when it is not valid. The mantissa, the exponent and
/// `too_many_digits` it also computes feed only the value, which [`from_chars`] takes from Rust's
/// parse.
fn parse_number_string(s: &[u8]) -> Option<usize> {
    let pend = s.len();
    let mut p = 0usize;
    if s[p] == b'-' {
        p += 1;
        if p == pend {
            return None;
        }
        if !s[p].is_ascii_digit() && s[p] != b'.' {
            return None;
        }
    }
    let start_digits = p;
    while p != pend && s[p].is_ascii_digit() {
        p += 1;
    }
    let end_of_integer_part = p;
    let mut digit_count = (end_of_integer_part - start_digits) as i64;
    if p != pend && s[p] == b'.' {
        p += 1;
        while p != pend && s[p].is_ascii_digit() {
            p += 1;
        }
        // exponent = end_of_integer_part + 1 - p; digit_count -= exponent
        digit_count += (p - (end_of_integer_part + 1)) as i64;
    }
    if digit_count == 0 {
        return None;
    }
    if p != pend && (s[p] == b'e' || s[p] == b'E') {
        let location_of_e = p;
        p += 1;
        // '-', or '+' (allowed on the exponent by C++17 20.19.3.(7.1))
        if p != pend && (s[p] == b'-' || s[p] == b'+') {
            p += 1;
        }
        if p == pend || !s[p].is_ascii_digit() {
            // chars_format::general holds fixed: the 'e' is left out.
            p = location_of_e;
        } else {
            while p != pend && s[p].is_ascii_digit() {
                p += 1;
            }
        }
    }
    Some(p)
}

/// `parse_infnan(first, last, value)`.
fn parse_infnan(s: &[u8], value: &mut f32) -> FromCharsResult {
    let last = s.len();
    let mut first = 0usize;
    let mut minus_sign = false;
    if s[first] == b'-' {
        minus_sign = true;
        first += 1;
    }
    if last - first >= 3 {
        if strncasecmp(&s[first..], b"nan", 3) {
            first += 3;
            let mut ptr_out = first;
            *value = if minus_sign { -f32::NAN } else { f32::NAN };
            if first != last && s[first] == b'(' {
                let mut ptr = first + 1;
                while ptr != last {
                    let c = s[ptr];
                    if c == b')' {
                        ptr_out = ptr + 1;
                        break;
                    } else if !(c.is_ascii_lowercase()
                        || c.is_ascii_uppercase()
                        || c.is_ascii_digit()
                        || c == b'_')
                    {
                        break;
                    }
                    ptr += 1;
                }
            }
            return FromCharsResult {
                ptr: ptr_out,
                ok: true,
            };
        }
        if strncasecmp(&s[first..], b"inf", 3) {
            let ptr = if last - first >= 8 && strncasecmp(&s[first + 3..], b"inity", 5) {
                first + 8
            } else {
                first + 3
            };
            *value = if minus_sign {
                -f32::INFINITY
            } else {
                f32::INFINITY
            };
            return FromCharsResult { ptr, ok: true };
        }
    }
    FromCharsResult { ptr: 0, ok: false }
}

/// `fast_float::from_chars(first, last, value)` for a `float` (`from_chars_advanced`,
/// `fast_float.h:2407-2440`).
pub fn from_chars(s: &[u8], value: &mut f32) -> FromCharsResult {
    if s.is_empty() {
        return FromCharsResult { ptr: 0, ok: false };
    }
    let Some(lastmatch) = parse_number_string(s) else {
        return parse_infnan(s, value);
    };
    // The characters taken are ASCII and in Rust's grammar (a sign, digits with at most one
    // point and at least one digit, an exponent with digits), so the parse cannot fail.
    let text = std::str::from_utf8(&s[..lastmatch]).unwrap_or("0");
    *value = text.parse::<f32>().unwrap_or(0.0);
    FromCharsResult {
        ptr: lastmatch,
        ok: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(s: &str) -> (f32, FromCharsResult) {
        let mut v = 0.0f32;
        let r = from_chars(s.as_bytes(), &mut v);
        (v, r)
    }

    // Expected values read from fast_float.h: parse_number_string (:522-654) takes an optional
    // '-', digits, a point and digits, and an exponent with digits; parse_infnan (:2338-2376)
    // the rest. Not replayed: no record holds these.
    #[test]
    fn the_grammar() {
        assert_eq!(read("1.5"), (1.5, FromCharsResult { ptr: 3, ok: true }));
        assert_eq!(read("-2"), (-2.0, FromCharsResult { ptr: 2, ok: true }));
        // :529-538 no '+'; a sign must be followed by a digit or the point.
        assert_eq!(read("+1"), (0.0, FromCharsResult { ptr: 0, ok: false }));
        assert_eq!(read(" 1"), (0.0, FromCharsResult { ptr: 0, ok: false }));
        assert_eq!(read("-x"), (0.0, FromCharsResult { ptr: 0, ok: false }));
        // :573 at least one digit.
        assert_eq!(read("."), (0.0, FromCharsResult { ptr: 0, ok: false }));
        assert_eq!(read("-.5"), (-0.5, FromCharsResult { ptr: 3, ok: true }));
        assert_eq!(read("5."), (5.0, FromCharsResult { ptr: 2, ok: true }));
        // :588-594 an exponent with no digit is left out (general holds fixed).
        assert_eq!(read("3e"), (3.0, FromCharsResult { ptr: 1, ok: true }));
        assert_eq!(read("3e+"), (3.0, FromCharsResult { ptr: 1, ok: true }));
        assert_eq!(read("3E-1x"), (0.3, FromCharsResult { ptr: 4, ok: true }));
        // Hexadecimal is not read: the "0" before the x.
        assert_eq!(read("0x1p3"), (0.0, FromCharsResult { ptr: 1, ok: true }));
        assert_eq!(read("3abc"), (3.0, FromCharsResult { ptr: 1, ok: true }));
        // Past float's range: infinity; below it: 0 (no range error in this fast_float).
        assert_eq!(read("1e39").0, f32::INFINITY);
        assert_eq!(read("1e-50").0, 0.0);
        assert_eq!(read("1.9").0, 1.9f32);
    }

    #[test]
    fn infinities_and_nans() {
        assert_eq!(
            read("INFINITY"),
            (f32::INFINITY, FromCharsResult { ptr: 8, ok: true })
        );
        assert_eq!(
            read("-Inf9"),
            (-f32::INFINITY, FromCharsResult { ptr: 4, ok: true })
        );
        let (v, r) = read("nan(ind)");
        assert!(v.is_nan() && v.is_sign_positive());
        assert_eq!(r, FromCharsResult { ptr: 8, ok: true });
        let (v, r) = read("-NaN(a-b)");
        assert!(v.is_nan() && v.is_sign_negative());
        assert_eq!(r, FromCharsResult { ptr: 4, ok: true });
        // :2351-2360 '(' with no ')' keeps the end after "nan".
        assert_eq!(read("nan(ab").1, FromCharsResult { ptr: 3, ok: true });
        assert_eq!(read("na").1, FromCharsResult { ptr: 0, ok: false });
        // :184-186 the differences OR-ed are 0 or 32, so only letters' case folds.
        assert_eq!(read("iNf").1, FromCharsResult { ptr: 3, ok: true });
    }
}
