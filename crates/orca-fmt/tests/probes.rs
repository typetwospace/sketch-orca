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
//! And every record of `cstdio.sop1` (o2-mesh, 2026-11-07), against the `stdio` and `scanf`
//! modules:
//!
//! - CSCN: `sscanf` of one input with one of the formats admesh passes: the count and what each
//!   argument holds afterwards.
//! - CFIL: a file opened with `"rb"`, `"r"` or `"rt"` and a script of `getc`, `fgets`, `fscanf`,
//!   `fread`, `ftell`, `fseek`, `rewind` and `feof` calls, each with its result.
//!
//! The committed subset (`parity/probes/`) runs by default; `SKETCH_ORCA_PROBES_DEEP=<folder>`
//! replays the full set from that folder (`<work>/parity/probes`).

use orca_fmt::Errc;
use orca_fmt::ostream::{self, Adjust, FloatField, Numpunct, Stream};
use orca_fmt::parse::{self, Errno, StoError};
use orca_fmt::printf::{self, Arg};
use orca_fmt::scanf::{ScanArg, fscanf, sscanf};
use orca_fmt::stdio::{File, Mode};
use orca_fmt::to_chars::{self, CharsFormat};
use orca_probe::{Mismatches, ProbeFile, Reader, load_probe};

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

// cstdio.sop1 (o2-mesh, 2026-11-07).

fn cstdio_file() -> ProbeFile {
    load_probe("cstdio").unwrap_or_else(|e| panic!("{e}"))
}

/// The formats of the CSCN and CFIL records, by index, with their arguments: `f` a `float`, `d` a
/// `double`, `s` a `char[32]`, `S` a `char[256]` (`parity/oracle/probe/cstdio.cpp`, FORMATS).
const SCAN_FORMATS: [(&[u8], &str); 11] = [
    (b"%f", "f"),
    (b"%lf", "d"),
    (b" vertex %f %f %f", "fff"),
    (b" vertex %f %f %f ", "fff"),
    (b" facet normal %31s %31s %31s", "sss"),
    (b" outer loop", ""),
    (b" endsolid%*[^\n]\n", ""),
    (b" solid%*[^\n]\n", ""),
    (b" solid %[^\n]", "S"),
    (b" ", ""),
    (b"%f %f %f", "fff"),
];

const FLOAT_SENTINEL: u32 = 0x1234_5678;
const DOUBLE_SENTINEL: u64 = 0x1234_5678_9ABC_DEF0;
const CHAR_SENTINEL: u8 = 0x55;

/// The arguments of one call, set to the probe's sentinels.
struct ScanArgs {
    f: [f32; 3],
    d: [f64; 3],
    s: [[u8; 32]; 3],
    big: [u8; 256],
}

impl ScanArgs {
    fn new() -> Self {
        ScanArgs {
            f: [f32::from_bits(FLOAT_SENTINEL); 3],
            d: [f64::from_bits(DOUBLE_SENTINEL); 3],
            s: [[CHAR_SENTINEL; 32]; 3],
            big: [CHAR_SENTINEL; 256],
        }
    }

    /// The `ScanArg`s of a format's argument list.
    fn args<'a>(&'a mut self, spec: &str) -> Vec<ScanArg<'a>> {
        let ScanArgs { f, d, s, big } = self;
        let mut f = f.iter_mut();
        let mut d = d.iter_mut();
        let mut s = s.iter_mut();
        let mut big = Some(&mut big[..]);
        spec.bytes()
            .map(|c| match c {
                b'f' => ScanArg::F32(f.next().expect("three floats at most")),
                b'd' => ScanArg::F64(d.next().expect("three doubles at most")),
                b's' => ScanArg::Chars(&mut s.next().expect("three buffers at most")[..]),
                _ => ScanArg::Chars(big.take().expect("one big buffer at most")),
            })
            .collect()
    }

    /// Whether each argument holds what the record says, read from `r`: a description of the
    /// first that does not.
    fn compare(&self, spec: &str, r: &mut Reader<'_>) -> Option<String> {
        let (mut nf, mut nd, mut ns) = (0, 0, 0);
        let mut first = None;
        for c in spec.bytes() {
            let diff = match c {
                b'f' => {
                    let want = r.u32().unwrap();
                    let got = self.f[nf].to_bits();
                    nf += 1;
                    (got != want).then(|| format!("float {got:#010x}, the UCRT {want:#010x}"))
                }
                b'd' => {
                    let want = r.u64().unwrap();
                    let got = self.d[nd].to_bits();
                    nd += 1;
                    (got != want).then(|| format!("double {got:#018x}, the UCRT {want:#018x}"))
                }
                b's' => {
                    let want = r.bytes().unwrap();
                    let got = &self.s[ns][..];
                    ns += 1;
                    (got != want).then(|| {
                        format!(
                            "buffer {:?}, the UCRT {:?}",
                            String::from_utf8_lossy(got),
                            String::from_utf8_lossy(want)
                        )
                    })
                }
                _ => {
                    let want = r.bytes().unwrap();
                    (self.big[..] != *want).then(|| {
                        format!(
                            "buffer {:?}, the UCRT {:?}",
                            String::from_utf8_lossy(&self.big),
                            String::from_utf8_lossy(want)
                        )
                    })
                }
            };
            if first.is_none() {
                first = diff;
            }
        }
        first
    }
}

#[test]
fn sscanf_reads_what_the_ucrt_reads() {
    let file = cstdio_file();
    let mut m = Mismatches::default();
    let mut cases = 0;
    for rec in file.of_kind("CSCN") {
        cases += 1;
        let mut r = rec.reader();
        let fi = usize::from(r.u8().unwrap());
        let input = r.bytes().unwrap();
        let want = r.i32().unwrap();
        let (format, spec) = SCAN_FORMATS[fi];
        let mut a = ScanArgs::new();
        let got = sscanf(input, format, &mut a.args(spec));
        let diff = a.compare(spec, &mut r);
        r.finish("CSCN").unwrap();
        m.check(got == Ok(want) && diff.is_none(), || {
            format!(
                "sscanf({:?}, {:?}) gave {got:?}, the UCRT {want}{}",
                String::from_utf8_lossy(input),
                String::from_utf8_lossy(format),
                diff.map(|d| format!("; {d}")).unwrap_or_default()
            )
        });
    }
    assert!(cases > 0, "{}: no CSCN record", file.path.display());
    m.assert_none("CSCN");
}

/// One CFIL script replayed: a description of its first call that differs from the record.
fn replay_script(r: &mut Reader<'_>) -> Option<String> {
    let raw = r.bytes().unwrap();
    let mode = r.u8().unwrap();
    let ops = r.u32().unwrap();
    let mut fp = File::from_bytes(raw, if mode == 0 { Mode::Binary } else { Mode::Text });
    let mut first: Option<String> = None;
    let mut buf = vec![0u8; 4096];
    for i in 0..ops {
        let op = r.u8().unwrap();
        let diff = match op {
            0 => {
                let want = r.i32().unwrap();
                let got = fp.getc();
                (got != want).then(|| format!("getc gave {got}, the UCRT {want}"))
            }
            1 => {
                let n = usize::try_from(r.i32().unwrap()).unwrap();
                let want_nonnull = r.bool().unwrap();
                let want = r.bytes().unwrap();
                buf.fill(CHAR_SENTINEL);
                let got_nonnull = fp.fgets(&mut buf, n);
                let k = buf[..n].iter().position(|&c| c == 0).map_or(n, |k| k + 1);
                (got_nonnull != want_nonnull || buf[..k] != *want).then(|| {
                    format!(
                        "fgets({n}) gave {got_nonnull} {:?}, the UCRT {want_nonnull} {:?}",
                        String::from_utf8_lossy(&buf[..k]),
                        String::from_utf8_lossy(want)
                    )
                })
            }
            2 => {
                let fi = usize::from(r.u8().unwrap());
                let want = r.i32().unwrap();
                let (format, spec) = SCAN_FORMATS[fi];
                let mut a = ScanArgs::new();
                let got = fscanf(&mut fp, format, &mut a.args(spec));
                let diff = a.compare(spec, r);
                (got != Ok(want) || diff.is_some()).then(|| {
                    format!(
                        "fscanf({:?}) gave {got:?}, the UCRT {want}{}",
                        String::from_utf8_lossy(format),
                        diff.map(|d| format!("; {d}")).unwrap_or_default()
                    )
                })
            }
            3 => {
                let size = r.u32().unwrap() as usize;
                let count = r.u32().unwrap() as usize;
                let want_n = r.u64().unwrap();
                let want = r.bytes().unwrap();
                buf.fill(CHAR_SENTINEL);
                let got_n = fp.fread(&mut buf, size, count) as u64;
                // The whole buffer for a binary stream; for a text stream the whole items read
                // only, as the probe records (cstdio.cpp, FREAD).
                let keep = if mode == 0 {
                    size * count
                } else {
                    got_n as usize * size
                };
                let got = &buf[..keep];
                (got_n != want_n || got != want).then(|| {
                    let at = got.iter().zip(want).position(|(a, b)| a != b);
                    format!(
                        "fread({size}, {count}) gave {got_n} items, the UCRT {want_n}{}",
                        at.map(|k| format!(
                            "; byte {k} is {:#04x}, the UCRT's {:#04x}",
                            got[k], want[k]
                        ))
                        .unwrap_or_default()
                    )
                })
            }
            4 => {
                let want = r.i32().unwrap();
                let got = fp.ftell();
                (got != want).then(|| format!("ftell gave {got}, the UCRT {want}"))
            }
            5 => {
                let offset = r.i32().unwrap();
                let whence = r.i32().unwrap();
                let want = r.i32().unwrap();
                let got = fp.fseek(offset, whence);
                (got != want)
                    .then(|| format!("fseek({offset}, {whence}) gave {got}, the UCRT {want}"))
            }
            6 => {
                fp.rewind();
                None
            }
            7 => {
                let want = r.bool().unwrap();
                let got = fp.feof();
                (got != want).then(|| format!("feof gave {got}, the UCRT {want}"))
            }
            // `fscanf(" solid %[^\n]")` where the line holds 256 bytes or more: the probe does not
            // make the C call, which would overrun its buffer, so the record holds nothing and the
            // stream reads on as it was. The Rust call must stop with `Overflow`.
            8 => {
                let mut probe = fp.clone();
                let mut line = [CHAR_SENTINEL; 256];
                let got = fscanf(
                    &mut probe,
                    b" solid %[^\n]",
                    &mut [ScanArg::Chars(&mut line)],
                );
                got.is_ok().then(|| {
                    format!(
                        "fscanf(\" solid %[^\\n]\") on a line of 256 bytes or more gave {got:?}"
                    )
                })
            }
            _ => panic!("a CFIL operation {op}"),
        };
        if first.is_none() {
            first = diff.map(|d| {
                format!(
                    "{:?} opened {}: call {i}: {d}",
                    String::from_utf8_lossy(raw),
                    ["rb", "r", "rt"][usize::from(mode)]
                )
            });
        }
    }
    first
}

#[test]
fn streams_read_what_the_ucrt_reads() {
    let file = cstdio_file();
    let mut m = Mismatches::default();
    let mut scripts = 0;
    for rec in file.of_kind("CFIL") {
        scripts += 1;
        let mut r = rec.reader();
        let diff = replay_script(&mut r);
        r.finish("CFIL").unwrap();
        m.check(diff.is_none(), || diff.unwrap_or_default());
    }
    assert!(scripts > 0, "{}: no CFIL record", file.path.display());
    m.assert_none("CFIL");
}
