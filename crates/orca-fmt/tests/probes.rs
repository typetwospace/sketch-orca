// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The probe gate for orca-fmt: every record of `fmt.sop1` (`parity/SOP1.md`, "Probe files"),
//! which the probe program wrote with the C++ engine's own compile and link lines, replayed
//! against the crate, byte for byte and bit for bit.
//!
//! - PRTF: `snprintf` into a 4096-byte buffer: the text and the count.
//! - TOCH: `std::to_chars` of a `double` and a `float`: the text and the `errc`.
//! - OSTR: `std::ostringstream << double` under the recorded flags, width, fill and locale.
//! - TOST: `std::to_string(double)`.
//! - PARS: `strtod`, `strtof` (with `errno`), `stod`, `stof` (with what they throw) and
//!   `from_chars` in each format: the value's bits, the characters read and the status.
//!
//! The committed subset (`parity/probes/`) runs by default; `SKETCH_ORCA_PROBES_DEEP=<folder>`
//! replays the full set from that folder (`<work>/parity/probes`).

use orca_fmt::Errc;
use orca_fmt::ostream::{self, Adjust, FloatField, Numpunct, Stream};
use orca_fmt::parse::{self, Errno, StoError};
use orca_fmt::printf::{self, Arg};
use orca_fmt::to_chars::{self, CharsFormat};
use orca_probe::{Mismatches, ProbeFile, load_probe};

fn file() -> ProbeFile {
    load_probe("fmt").unwrap_or_else(|e| panic!("{e}"))
}

/// MSVC's `std::errc` values: `invalid_argument` 22, `result_out_of_range` 34, `value_too_large`
/// 132.
fn errc_code(e: Option<Errc>) -> i32 {
    match e {
        None => 0,
        Some(Errc::InvalidArgument) => 22,
        Some(Errc::ResultOutOfRange) => 34,
        Some(Errc::ValueTooLarge) => 132,
    }
}

/// The UCRT's `errno` values: `EINVAL` 22, `ERANGE` 34.
fn errno_code(e: Option<Errno>) -> i32 {
    match e {
        None => 0,
        Some(Errno::Invalid) => 22,
        Some(Errno::Range) => 34,
    }
}

#[allow(clippy::cast_possible_truncation)]
fn f32_of(x: u64) -> f32 {
    f32::from_bits(x as u32)
}

fn f32_bits(x: f32) -> u64 {
    u64::from(x.to_bits())
}

const FORMATS: [CharsFormat; 4] = [
    CharsFormat::Scientific,
    CharsFormat::Fixed,
    CharsFormat::Hex,
    CharsFormat::General,
];

fn chars_format(i: i32) -> CharsFormat {
    FORMATS[usize::try_from(i).expect("a chars_format of 0 to 3")]
}

#[allow(clippy::cast_possible_wrap)]
fn printf_arg(ty: &str, arg: u64) -> Arg<'static> {
    match ty {
        "i32" | "i64" => Arg::Int(arg as i64),
        "u32" | "u64" => Arg::UInt(arg),
        "f64" => Arg::Double(f64::from_bits(arg)),
        _ => panic!("a PRTF argument of type {ty}"),
    }
}

#[test]
fn snprintf_gives_the_ucrts_text_and_count() {
    let file = file();
    let mut m = Mismatches::default();
    let mut buf = vec![0u8; 4096];
    for rec in file.of_kind("PRTF") {
        let mut r = rec.reader();
        let format = r.bytes().unwrap().to_vec();
        let ty = r.str().unwrap();
        let cases = r.count().unwrap();
        for _ in 0..cases {
            let arg = r.u64().unwrap();
            let want = r.bytes().unwrap();
            let want_n = r.i32().unwrap();
            let (got, got_n) = match printf::snprintf(&mut buf, &format, &[printf_arg(&ty, arg)]) {
                Ok(n) => (&buf[..n.min(buf.len() - 1)], i32::try_from(n).unwrap_or(-1)),
                Err(_) => (&buf[..0], -1),
            };
            m.check(got == want && got_n == want_n, || {
                format!(
                    "snprintf({:?}, {ty} {arg:#x}) gave {:?} ({got_n}), the UCRT {:?} ({want_n})",
                    String::from_utf8_lossy(&format),
                    String::from_utf8_lossy(got),
                    String::from_utf8_lossy(want)
                )
            });
        }
        r.finish("PRTF").unwrap();
    }
    m.assert_none("PRTF");
}

#[test]
fn to_chars_gives_msvcs_text_and_errc() {
    let file = file();
    let mut m = Mismatches::default();
    let mut kinds = 0;
    for rec in file.of_kind("TOCH") {
        kinds += 1;
        let mut r = rec.reader();
        let kind = r.str().unwrap();
        let width = r.u8().unwrap();
        let cases = r.count().unwrap();
        for _ in 0..cases {
            let bits = r.u64().unwrap();
            let format = r.i32().unwrap();
            let has_precision = r.bool().unwrap();
            let precision = r.i32().unwrap();
            let size = r.u32().unwrap() as usize;
            let want_errc = r.i32().unwrap();
            let want = r.bytes().unwrap();
            let mut buf = vec![0u8; size];
            let result = match (width, format, has_precision) {
                (64, -1, _) => to_chars::to_chars(&mut buf, f64::from_bits(bits)),
                (32, -1, _) => to_chars::to_chars(&mut buf, f32_of(bits)),
                (64, f, false) => {
                    to_chars::to_chars_format(&mut buf, f64::from_bits(bits), chars_format(f))
                }
                (32, f, false) => {
                    to_chars::to_chars_format(&mut buf, f32_of(bits), chars_format(f))
                }
                (64, f, true) => to_chars::to_chars_precision(
                    &mut buf,
                    f64::from_bits(bits),
                    chars_format(f),
                    precision,
                ),
                (32, f, true) => {
                    to_chars::to_chars_precision(&mut buf, f32_of(bits), chars_format(f), precision)
                }
                _ => panic!("a TOCH record of width {width}"),
            };
            let (got, got_errc) = match result {
                Ok(n) => (&buf[..n], 0),
                Err(e) => (&buf[..0], errc_code(Some(e))),
            };
            m.check(got == want && got_errc == want_errc, || {
                format!(
                    "to_chars {kind} f{width} {bits:#x} format {format} precision {has_precision} {precision} into {size}: gave {:?} (errc {got_errc}), MSVC {:?} (errc {want_errc})",
                    String::from_utf8_lossy(got),
                    String::from_utf8_lossy(want)
                )
            });
        }
        r.finish("TOCH").unwrap();
    }
    assert_eq!(
        kinds, 7,
        "the TOCH records: shortest, format and precision of each width, and short buffers"
    );
    m.assert_none("TOCH");
}

fn numpunct(locale: u8) -> Numpunct {
    match locale {
        0 => Numpunct::default(),
        1 | 2 => Numpunct {
            decimal_point: b',',
            thousands_sep: b'.',
            grouping: if locale == 1 { vec![3] } else { vec![1, 2] },
        },
        _ => panic!("an OSTR locale {locale}"),
    }
}

#[test]
fn ostream_writes_msvcs_text() {
    let file = file();
    let mut m = Mismatches::default();
    for rec in file.of_kind("OSTR") {
        let mut r = rec.reader();
        let cases = r.count().unwrap();
        for _ in 0..cases {
            let bits = r.u64().unwrap();
            let field = r.u8().unwrap();
            let flags = r.u8().unwrap();
            let adjust = r.u8().unwrap();
            let precision = r.i64().unwrap();
            let width = r.i64().unwrap();
            let fill = r.u8().unwrap();
            let locale = r.u8().unwrap();
            let want = r.bytes().unwrap();
            let mut stream = Stream {
                float_field: match field {
                    0 => FloatField::Default,
                    1 => FloatField::Fixed,
                    2 => FloatField::Scientific,
                    _ => FloatField::Hex,
                },
                adjust: match adjust {
                    1 => Adjust::Left,
                    2 => Adjust::Internal,
                    _ => Adjust::Right,
                },
                showpos: flags & 1 != 0,
                showpoint: flags & 2 != 0,
                uppercase: flags & 4 != 0,
                precision,
                width,
                fill,
                numpunct: numpunct(locale),
            };
            let mut got = Vec::new();
            let bad = ostream::put_f64(&mut got, &mut stream, f64::from_bits(bits)).is_err();
            let want_bad = flags & 8 != 0;
            m.check(got == want && bad == want_bad, || {
                format!(
                    "ostream << {bits:#x} field {field} flags {flags} adjust {adjust} precision {precision} width {width} fill {fill} locale {locale}: gave {:?} (bad {bad}), MSVC {:?}",
                    String::from_utf8_lossy(&got),
                    String::from_utf8_lossy(want)
                )
            });
        }
        r.finish("OSTR").unwrap();
    }
    m.assert_none("OSTR");
}

#[test]
fn to_string_writes_msvcs_text() {
    let file = file();
    let mut m = Mismatches::default();
    for rec in file.of_kind("TOST") {
        let mut r = rec.reader();
        let cases = r.count().unwrap();
        for _ in 0..cases {
            let bits = r.u64().unwrap();
            let want = r.str().unwrap();
            let got = ostream::to_string_f64(f64::from_bits(bits));
            m.check(got == want, || {
                format!("to_string({bits:#x}) gave {got:?}, MSVC {want:?}")
            });
        }
        r.finish("TOST").unwrap();
    }
    m.assert_none("TOST");
}

/// What a parser gave: the value's bits, the characters read and the status.
type Parsed = (u64, i64, i32);

fn consumed(n: usize) -> i64 {
    i64::try_from(n).expect("an input shorter than 2^63")
}

fn sto_status(e: StoError) -> i32 {
    match e {
        StoError::InvalidArgument => 1,
        StoError::OutOfRange => 2,
    }
}

fn parse_with(function: &str, input: &[u8]) -> Parsed {
    match function {
        "strtod" => {
            let s = parse::strtod(input);
            (s.value.to_bits(), consumed(s.end), errno_code(s.errno))
        }
        "strtof" => {
            let s = parse::strtof(input);
            (f32_bits(s.value), consumed(s.end), errno_code(s.errno))
        }
        "stod" => match parse::stod(input) {
            Ok((v, n)) => (v.to_bits(), consumed(n), 0),
            Err(e) => (0, 0, sto_status(e)),
        },
        "stof" => match parse::stof(input) {
            Ok((v, n)) => (f32_bits(v), consumed(n), 0),
            Err(e) => (0, 0, sto_status(e)),
        },
        _ => {
            let (width, name) = function
                .strip_prefix("from_chars-")
                .and_then(|rest| rest.split_once('-'))
                .unwrap_or_else(|| panic!("a PARS record for {function}"));
            let format = match name {
                "scientific" => CharsFormat::Scientific,
                "fixed" => CharsFormat::Fixed,
                "hex" => CharsFormat::Hex,
                "general" => CharsFormat::General,
                _ => panic!("a PARS record for {function}"),
            };
            if width == "f64" {
                let mut v = f64::from_bits(0xDEAD_BEEF);
                let res = parse::from_chars_float(input, &mut v, format);
                (v.to_bits(), consumed(res.ptr), errc_code(res.ec))
            } else {
                let mut v = f32::from_bits(0xDEAD_BEEF);
                let res = parse::from_chars_float(input, &mut v, format);
                (f32_bits(v), consumed(res.ptr), errc_code(res.ec))
            }
        }
    }
}

#[test]
fn parsers_read_what_msvc_reads() {
    let file = file();
    let mut m = Mismatches::default();
    let mut functions = 0;
    for rec in file.of_kind("PARS") {
        functions += 1;
        let mut r = rec.reader();
        let function = r.str().unwrap();
        let cases = r.count().unwrap();
        for _ in 0..cases {
            let input = r.bytes().unwrap();
            let want: Parsed = (r.u64().unwrap(), r.i64().unwrap(), r.i32().unwrap());
            let got = parse_with(&function, input);
            m.check(got == want, || {
                format!(
                    "{function}({:?}) gave (bits {:#x}, read {}, status {}), MSVC (bits {:#x}, read {}, status {})",
                    String::from_utf8_lossy(input),
                    got.0,
                    got.1,
                    got.2,
                    want.0,
                    want.1,
                    want.2
                )
            });
        }
        r.finish("PARS").unwrap();
    }
    assert_eq!(
        functions, 12,
        "the PARS records: strtod, strtof, stod, stof and from_chars in four formats each width"
    );
    m.assert_none("PARS");
}
