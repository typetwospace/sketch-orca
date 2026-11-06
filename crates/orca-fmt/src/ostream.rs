// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Microsoft STL code: put_f64 is translated from stl/inc/xlocnum (num_put::do_put for
// double, _Float_put_desired_precision, _Ffmt and _Fput_v3) and to_string_f64 from stl/inc/string
// (to_string(double)) of microsoft/STL at the tag vs-2022-17.12 (commit
// 91e425587a0b9b5ac0d1005844c661fee2e2813b), the headers of MSVC 14.42, on 2026-11-06. Its
// copyright holder is Microsoft Corporation, and it is under Apache-2.0 WITH LLVM-exception
// (licenses/Apache-2.0.txt and licenses/LLVM-exception.txt).

//! `std::ostream << double` as MSVC 14.42's standard library writes it, and `std::to_string`.
//!
//! MSVC's `num_put::do_put` for a `double` does not format the number itself: it builds a printf
//! format from the stream's flags (`_Ffmt`: `%`, `+` for `showpos`, `#` for `showpoint`, `.*` and
//! `f`, `e`, `g` or `a`), prints the value with `sprintf_s` and the stream's precision, and then
//! (`_Fput_v3`) swaps in the locale's decimal point, inserts its thousands separators and pads the
//! text to the stream's width with its fill character. [`put_f64`] does the same, with
//! [`crate::printf`]'s `sprintf_s`, so its digits are the UCRT's:
//!
//! - `defaultfloat` (no `floatfield`) is `%.*g`, `fixed` is `%.*f` (lowercase even with
//!   `uppercase`, as MSVC has it), `scientific` is `%.*e` and `hexfloat` (`fixed | scientific`) is
//!   `%a` with every hexit of the fraction (the precision is ignored);
//! - precision 0 is `%.0g`, so 1 significant digit, for `defaultfloat`, and a negative precision
//!   is printf's default of 6;
//! - `showpoint` is dropped for an infinity and a NaN (MSVC's fix for DevCom-10519861), so they
//!   print as `inf` and `nan`, never `i.nf`;
//! - the width applies once and is then reset to 0, as C++ resets `ios_base::width`;
//! - `left` puts the fill after the number, `internal` between its sign (and `0x`) and its digits,
//!   anything else before it.
//!
//! A `float` is written as the `double` it widens to (MSVC's `operator<<(float)` does so):
//! [`put_f32`].
//!
//! The locale is the "C" locale the engine runs in (the C runtime's `localeconv()` decimal point is
//! `.`); the stream's own `numpunct` is a [`Numpunct`], the classic one by default (`.`, no
//! grouping).

use crate::printf::{self, Arg};

/// `ios_base::floatfield`: the notation of a floating-point number.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FloatField {
    /// No flag (`std::defaultfloat`): `%g`.
    #[default]
    Default,
    /// `std::fixed`: `%f`.
    Fixed,
    /// `std::scientific`: `%e`.
    Scientific,
    /// `std::hexfloat` (`fixed | scientific`): `%a`.
    Hex,
}

/// `ios_base::adjustfield`: where the fill goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Adjust {
    /// `std::right`, and no flag (or several): the fill before the number.
    #[default]
    Right,
    /// `std::left`: the fill after the number.
    Left,
    /// `std::internal`: the fill after the sign (and a hex float's `0x`).
    Internal,
}

/// The `numpunct<char>` facet of the stream's locale.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Numpunct {
    /// `decimal_point()`.
    pub decimal_point: u8,
    /// `thousands_sep()`.
    pub thousands_sep: u8,
    /// `grouping()`: the size of each group of digits, from the decimal point leftwards; the last
    /// one repeats, and a size of 0 or above 126 (`CHAR_MAX` as a `char`) ends the grouping.
    pub grouping: Vec<u8>,
}

impl Default for Numpunct {
    /// The classic locale's: `.`, `,`, and no grouping.
    fn default() -> Numpunct {
        Numpunct {
            decimal_point: b'.',
            thousands_sep: b',',
            grouping: Vec::new(),
        }
    }
}

/// What an `ostream` reads when it writes a number: its format flags, precision, width, fill and
/// locale. The default is a new stream's state: precision 6, width 0, fill a space.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stream {
    /// `floatfield`.
    pub float_field: FloatField,
    /// `adjustfield`.
    pub adjust: Adjust,
    /// `showpos`: a `+` before a number that is not negative.
    pub showpos: bool,
    /// `showpoint`: the decimal point always, and `%g`'s trailing zeros.
    pub showpoint: bool,
    /// `uppercase`: `E`, `INF`, `NAN`, `0X` and `P` (not in `fixed`, which is always lowercase).
    pub uppercase: bool,
    /// `precision()`.
    pub precision: i64,
    /// `width()`: the field's least width, reset to 0 by each number written.
    pub width: i64,
    /// `fill()`.
    pub fill: u8,
    /// The locale's `numpunct<char>`.
    pub numpunct: Numpunct,
}

impl Default for Stream {
    fn default() -> Stream {
        Stream {
            float_field: FloatField::Default,
            adjust: Adjust::Right,
            showpos: false,
            showpoint: false,
            uppercase: false,
            precision: 6,
            width: 0,
            fill: b' ',
            numpunct: Numpunct::default(),
        }
    }
}

/// Why nothing was written: the C++ `string::resize` of `do_put` throws `std::length_error`, which
/// `operator<<` turns into `badbit`. It happens for a precision that a cast to `int` makes
/// negative (2^31 to 2^32 - 1, and so on).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LengthError;

impl std::fmt::Display for LengthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("num_put's buffer size overflows (std::length_error)")
    }
}

impl std::error::Error for LengthError {}

/// `_Float_put_desired_precision<double>`: the digits after the point that `do_put` sizes its
/// buffer for.
fn desired_precision(precision: i64, float_field: FloatField) -> i32 {
    if float_field == FloatField::Hex {
        // ((DBL_MANT_DIG - 1) + 3) / 4
        return (52 + 3) / 4;
    }
    if precision > 0 {
        // static_cast<int>(_Precision)
        precision as i32
    } else if precision == 0 {
        if float_field == FloatField::Default {
            1
        } else {
            0
        }
    } else {
        6
    }
}

/// `_Ffmt(_Fmt, 0, _Flags)`: the printf format `do_put` prints with.
fn printf_format(stream: &Stream, showpoint: bool) -> Vec<u8> {
    let mut format = vec![b'%'];
    if stream.showpos {
        format.push(b'+');
    }
    if showpoint {
        format.push(b'#');
    }
    format.extend_from_slice(b".*");
    format.push(match (stream.float_field, stream.uppercase) {
        (FloatField::Fixed, _) => b'f',
        (FloatField::Hex, true) => b'A',
        (FloatField::Hex, false) => b'a',
        (FloatField::Scientific, true) => b'E',
        (FloatField::Scientific, false) => b'e',
        (FloatField::Default, true) => b'G',
        (FloatField::Default, false) => b'g',
    });
    format
}

/// `num_put<char>::do_put(dest, stream, stream.fill, value)` for a `double`: appends the text to
/// `out` and resets `stream.width` to 0.
///
/// # Errors
///
/// [`LengthError`] (nothing written, the width kept) where C++ throws `std::length_error`.
pub fn put_f64(out: &mut Vec<u8>, stream: &mut Stream, value: f64) -> Result<(), LengthError> {
    let is_hex = stream.float_field == FloatField::Hex;
    let precision = if is_hex { -1 } else { stream.precision };
    // The buffer is sized for the desired precision plus 50 bytes (and the integer digits of a
    // fixed value above 1e10), which holds every result; only a size the int cast made negative
    // fails, in string::resize.
    if desired_precision(precision, stream.float_field) < 0 {
        return Err(LengthError);
    }
    let finite = value.is_finite();
    // TRANSITION, DevCom-10519861: no showpoint for an infinity or a NaN.
    let format = printf_format(stream, stream.showpoint && finite);
    let text = printf::sprintf_s(
        &format,
        &[Arg::Int(i64::from(precision as i32)), Arg::Double(value)],
    )
    .expect("num_put's format is valid and its buffer holds the result");
    put_text(out, stream, &text, finite);
    Ok(())
}

/// `std::ostream << float`: the `double` the `float` widens to, through [`put_f64`].
///
/// # Errors
///
/// As [`put_f64`].
pub fn put_f32(out: &mut Vec<u8>, stream: &mut Stream, value: f32) -> Result<(), LengthError> {
    put_f64(out, stream, f64::from(value))
}

/// [`put_f64`] into a new `String` (a fill or punctuation byte that is not ASCII becomes U+FFFD).
///
/// # Errors
///
/// As [`put_f64`].
pub fn f64_to_string(stream: &mut Stream, value: f64) -> Result<String, LengthError> {
    let mut out = Vec::new();
    put_f64(&mut out, stream, value)?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// `_Fput_v3`: the locale's decimal point and grouping, then the fill.
fn put_text(out: &mut Vec<u8>, stream: &mut Stream, text: &[u8], finite: bool) {
    let count = text.len();
    let mut prefix = usize::from(matches!(text.first(), Some(b'+' | b'-')));
    let exponents: &[u8] = if stream.float_field == FloatField::Hex {
        if prefix + 2 <= count && text[prefix] == b'0' && matches!(text[prefix + 1], b'x' | b'X') {
            prefix += 2;
        }
        b"pP"
    } else {
        b"eE"
    };
    // strcspn: the offset of the exponent and of the C locale's decimal point, or the length.
    let eoff = text
        .iter()
        .position(|c| exponents.contains(c))
        .unwrap_or(count);
    let poff = text.iter().position(|&c| c == b'.').unwrap_or(count);

    let punct = &stream.numpunct;
    let mut group_string = text.to_vec();
    if poff != count {
        group_string[poff] = punct.decimal_point;
    }

    if finite {
        let mut off = if poff == count { eoff } else { poff };
        let mut pg = 0;
        // A grouping char is signed: 0 and the negative values (above 127) end it, as CHAR_MAX
        // (127) does.
        let group = |i: usize| punct.grouping.get(i).map_or(0, |&g| g as i8);
        while group(pg) != i8::MAX && group(pg) > 0 && (group(pg) as usize) < off - prefix {
            off -= group(pg) as usize;
            group_string.insert(off, punct.thousands_sep);
            if group(pg + 1) > 0 {
                pg += 1;
            }
        }
    }

    let count = group_string.len();
    let mut fill_count = match usize::try_from(stream.width) {
        Ok(width) if width > count => width - count,
        _ => 0,
    };
    let fill = stream.fill;
    let rep = |out: &mut Vec<u8>, n: usize| out.resize(out.len() + n, fill);
    match stream.adjust {
        Adjust::Right => {
            rep(out, fill_count);
            fill_count = 0;
            out.extend_from_slice(&group_string[..prefix]);
        }
        Adjust::Internal => {
            out.extend_from_slice(&group_string[..prefix]);
            rep(out, fill_count);
            fill_count = 0;
        }
        Adjust::Left => out.extend_from_slice(&group_string[..prefix]),
    }
    out.extend_from_slice(&group_string[prefix..]);
    stream.width = 0;
    rep(out, fill_count);
}

/// `std::to_string(double)` (and of a `float`, widened): `sprintf_s` with `%f`.
#[must_use]
pub fn to_string_f64(value: f64) -> String {
    let text = printf::sprintf_s(b"%f", &[Arg::Double(value)]).expect("%f is a valid format");
    String::from_utf8(text).expect("%f prints ASCII")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(stream: &mut Stream, value: f64) -> String {
        f64_to_string(stream, value).unwrap()
    }

    #[test]
    fn default_stream() {
        let mut s = Stream::default();
        assert_eq!(put(&mut s, 0.1), "0.1");
        assert_eq!(put(&mut s, 1.0 / 3.0), "0.333333");
        assert_eq!(put(&mut s, 1e6), "1e+06");
        assert_eq!(put(&mut s, 123456.0), "123456");
        assert_eq!(put(&mut s, 1234567.0), "1.23457e+06");
        assert_eq!(put(&mut s, -0.0), "-0");
        assert_eq!(put(&mut s, f64::INFINITY), "inf");
        assert_eq!(put(&mut s, f64::NAN), "nan");
        assert_eq!(put(&mut s, -f64::NAN), "-nan(ind)");
        assert_eq!(put(&mut s, 1e-5), "1e-05");
    }

    #[test]
    fn notations_and_precisions() {
        let mut s = Stream {
            precision: 17,
            ..Stream::default()
        };
        assert_eq!(put(&mut s, 0.1), "0.10000000000000001");
        s.precision = 0;
        assert_eq!(put(&mut s, 2.5), "2");
        s.float_field = FloatField::Fixed;
        assert_eq!(put(&mut s, 2.5), "2");
        assert_eq!(put(&mut s, 3.5), "4");
        s.precision = 3;
        assert_eq!(put(&mut s, 1e21), "1000000000000000000000.000");
        s.uppercase = true;
        assert_eq!(put(&mut s, f64::INFINITY), "inf");
        s.float_field = FloatField::Scientific;
        assert_eq!(put(&mut s, 12345.678), "1.235E+04");
        assert_eq!(put(&mut s, f64::NEG_INFINITY), "-INF");
        s.float_field = FloatField::Hex;
        assert_eq!(put(&mut s, 1.0), "0X1.0000000000000P+0");
        s.uppercase = false;
        assert_eq!(put(&mut s, -0.5), "-0x1.0000000000000p-1");
        s.precision = -1;
        s.float_field = FloatField::Default;
        assert_eq!(put(&mut s, 1.0 / 3.0), "0.333333");
    }

    #[test]
    fn showpoint_and_showpos() {
        let mut s = Stream {
            showpoint: true,
            ..Stream::default()
        };
        assert_eq!(put(&mut s, 1.0), "1.00000");
        assert_eq!(put(&mut s, f64::INFINITY), "inf");
        s.precision = 0;
        s.float_field = FloatField::Fixed;
        assert_eq!(put(&mut s, 3.0), "3.");
        assert_eq!(put(&mut s, f64::NEG_INFINITY), "-inf");
        s.showpos = true;
        assert_eq!(put(&mut s, 3.0), "+3.");
        assert_eq!(put(&mut s, f64::NAN), "+nan");
    }

    #[test]
    fn width_fill_and_adjust() {
        let mut s = Stream {
            width: 10,
            fill: b'*',
            ..Stream::default()
        };
        assert_eq!(put(&mut s, -1.5), "******-1.5");
        assert_eq!(s.width, 0);
        assert_eq!(put(&mut s, -1.5), "-1.5");
        s.width = 10;
        s.adjust = Adjust::Left;
        assert_eq!(put(&mut s, -1.5), "-1.5******");
        s.width = 10;
        s.adjust = Adjust::Internal;
        assert_eq!(put(&mut s, -1.5), "-******1.5");
        s.width = 12;
        s.float_field = FloatField::Hex;
        assert_eq!(put(&mut s, 1.0), "0x1.0000000000000p+0");
        s.width = 24;
        assert_eq!(put(&mut s, -1.0), "-0x***1.0000000000000p+0");
        s.width = 8;
        s.float_field = FloatField::Default;
        assert_eq!(put(&mut s, f64::NEG_INFINITY), "-****inf");
    }

    #[test]
    fn numpunct_grouping() {
        let mut s = Stream {
            float_field: FloatField::Fixed,
            precision: 2,
            numpunct: Numpunct {
                decimal_point: b',',
                thousands_sep: b'.',
                grouping: vec![3],
            },
            ..Stream::default()
        };
        assert_eq!(put(&mut s, 1234567.891), "1.234.567,89");
        assert_eq!(put(&mut s, -123.0), "-123,00");
        assert_eq!(put(&mut s, -1234.0), "-1.234,00");
        s.numpunct.grouping = vec![1, 2];
        assert_eq!(put(&mut s, 1234567.0), "12.34.56.7,00");
        // A 0 after a group repeats it, as the end of the string does; CHAR_MAX ends the grouping.
        s.numpunct.grouping = vec![2, 0];
        assert_eq!(put(&mut s, 1234567.0), "1.23.45.67,00");
        s.numpunct.grouping = vec![2, 127];
        assert_eq!(put(&mut s, 1234567.0), "12345.67,00");
        s.numpunct.grouping = vec![3];
        assert_eq!(put(&mut s, f64::INFINITY), "inf");
        s.float_field = FloatField::Default;
        s.precision = 6;
        assert_eq!(put(&mut s, 1e20), "1e+20");
        assert_eq!(put(&mut s, 123456.0), "123.456");
        s.float_field = FloatField::Scientific;
        assert_eq!(put(&mut s, 123456.0), "1,234560e+05");
    }

    #[test]
    fn length_error_and_to_string() {
        let mut s = Stream {
            precision: 1 << 31,
            width: 5,
            ..Stream::default()
        };
        assert_eq!(put_f64(&mut Vec::new(), &mut s, 1.0), Err(LengthError));
        assert_eq!(s.width, 5);
        s.precision = 1 << 32;
        assert_eq!(put(&mut s, 2.5), "    2");
        assert_eq!(to_string_f64(0.1), "0.100000");
        assert_eq!(to_string_f64(-1e300).len(), 1 + 301 + 7);
        assert_eq!(to_string_f64(f64::NAN), "nan");
    }
}
