// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! orca-fmt against the static UCRT that this test program links, the C library the C++ engine's
//! `/MT` build links: its printf, with the options a C++ program's startup sets, through the two
//! entry points of the UCRT's printf families (`__stdio_common_vsprintf` for `sprintf` and
//! `snprintf`, `__stdio_common_vsnprintf_s` for `_snprintf_s`), and its `strtod`, `strtof`, `atof`,
//! `strtol`, `strtoul`, `strtoll`, `strtoull`, `atoi`, `atol` and `atoll`. Besides: the `sto*`
//! functions follow the C functions; `to_chars` output reads back through `from_chars` and
//! `strtod`, with Rust's shortest digits but at exact ties; integers round-trip in every base;
//! `ostream << double` prints the UCRT's `sprintf_s` of the format MSVC's `num_put` builds, padded
//! as its adjustment says, and what a program built with MSVC 14.42 printed for 25 streams. Every
//! case runs with a seeded generator, so a failure repeats; each test counts its cases and asserts
//! the count, so none passes by doing less.
//!
//! The recording program of the 25 streams (`MSVC_OSTREAM`) is
//! `parity/oracle/probe/recorded/msvc_crt_fmt.cpp`.

use orca_fmt::Errc;
use orca_fmt::ostream::{self, Adjust, FloatField, Numpunct, Stream};
use orca_fmt::parse::{self, Errno, FromCharsResult, StoError, Strto};
use orca_fmt::printf::{self, Arg, FormatError, Mode};
use orca_fmt::to_chars::{self, CharsFormat};
use std::ffi::{c_char, c_int, c_void};

/// The UCRT functions the tests compare with.
mod ucrt {
    use super::{c_char, c_int, c_void};

    pub type InvalidParameterHandler =
        unsafe extern "C" fn(*const u16, *const u16, *const u16, u32, usize);

    unsafe extern "C" {
        pub fn __local_stdio_printf_options() -> *mut u64;
        pub fn __stdio_common_vsprintf(
            options: u64,
            buffer: *mut c_char,
            count: usize,
            format: *const c_char,
            locale: *mut c_void,
            args: *const u64,
        ) -> c_int;
        pub fn __stdio_common_vsnprintf_s(
            options: u64,
            buffer: *mut c_char,
            count: usize,
            max_count: usize,
            format: *const c_char,
            locale: *mut c_void,
            args: *const u64,
        ) -> c_int;
        pub fn _set_thread_local_invalid_parameter_handler(
            handler: Option<InvalidParameterHandler>,
        ) -> Option<InvalidParameterHandler>;
        pub fn _errno() -> *mut c_int;
        pub fn strtod(s: *const c_char, end: *mut *mut c_char) -> f64;
        pub fn strtof(s: *const c_char, end: *mut *mut c_char) -> f32;
        pub fn strtol(s: *const c_char, end: *mut *mut c_char, base: c_int) -> i32;
        pub fn strtoul(s: *const c_char, end: *mut *mut c_char, base: c_int) -> u32;
        pub fn strtoll(s: *const c_char, end: *mut *mut c_char, base: c_int) -> i64;
        pub fn strtoull(s: *const c_char, end: *mut *mut c_char, base: c_int) -> u64;
        pub fn atof(s: *const c_char) -> f64;
        pub fn atoi(s: *const c_char) -> c_int;
        pub fn atol(s: *const c_char) -> i32;
        pub fn atoll(s: *const c_char) -> i64;
    }
}

/// `_snprintf_s`'s `_TRUNCATE`.
const TRUNCATE: usize = usize::MAX;
/// `_CRT_INTERNAL_PRINTF_STANDARD_SNPRINTF_BEHAVIOR`: C99 `snprintf`, which `sprintf` uses too.
const STANDARD_SNPRINTF_BEHAVIOR: u64 = 2;
const EINVAL: c_int = 22;
const EILSEQ: c_int = 42;
const ERANGE: c_int = 34;

/// The invalid parameter handler the tests install on their threads: it returns, so the UCRT
/// function returns its error instead of ending the process, as `_set_invalid_parameter_handler`
/// with a handler that returns does in a C++ program.
unsafe extern "C" fn ignore_invalid_parameter(
    _: *const u16,
    _: *const u16,
    _: *const u16,
    _: u32,
    _: usize,
) {
}

fn install_handler() {
    // SAFETY: sets this thread's handler to a function that does nothing.
    unsafe { ucrt::_set_thread_local_invalid_parameter_handler(Some(ignore_invalid_parameter)) };
}

fn set_errno(value: c_int) {
    // SAFETY: _errno returns this thread's errno.
    unsafe { *ucrt::_errno() = value };
}

fn errno() -> c_int {
    // SAFETY: as in set_errno.
    unsafe { *ucrt::_errno() }
}

/// SplitMix64: a small seeded generator, so every run tests the same cases.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value below `n`.
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }

    /// One of the byte strings `items`.
    fn piece<'a>(&mut self, items: &[&'a [u8]]) -> &'a [u8] {
        items[self.below(items.len() as u64) as usize]
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    /// A signed value in `low..=high`.
    fn range(&mut self, low: i64, high: i64) -> i64 {
        low + self.below((high - low + 1) as u64) as i64
    }
}

/// Doubles a printf or a parser finds hard: zeros, infinities, NaNs with every kind of payload,
/// the subnormal and normal limits, ties at every precision, and values just beside powers of ten.
fn special_doubles() -> Vec<f64> {
    let mut v = vec![
        0.0,
        -0.0,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::from_bits(0x7FF8_0000_0000_0000),
        f64::from_bits(0xFFF8_0000_0000_0000),
        f64::from_bits(0x7FF8_0000_0000_0001),
        f64::from_bits(0xFFF8_0000_0000_0001),
        f64::from_bits(0x7FF0_0000_0000_0001),
        f64::from_bits(0xFFF0_0000_0000_0001),
        f64::from_bits(0x7FF4_0000_0000_0000),
        f64::from_bits(0x7FFF_FFFF_FFFF_FFFF),
        f64::from_bits(0xFFFF_FFFF_FFFF_FFFF),
        f64::MIN_POSITIVE,
        f64::from_bits(1),
        f64::from_bits(0x000F_FFFF_FFFF_FFFF),
        f64::MAX,
        f64::MIN,
        f64::EPSILON,
        1.0,
        0.5,
        1.5,
        2.5,
        0.125,
        0.375,
        9.5,
        99.5,
        999.5,
        0.05,
        0.15,
        0.25,
        0.35,
        1e15,
        1e16,
        1e17,
        1e21,
        1e22,
        1e23,
        123456789.0,
        9007199254740992.0,
        9007199254740993.0,
        0.1,
        0.2,
        0.3,
        1.0 / 3.0,
        2.0 / 3.0,
        5e-324,
        f64::from_bits(0x0010_0000_0000_0001),
        1.7976931348623157e308,
        4.35,
        0.000_099_999_9,
        0.000_1,
        99999.95,
        999999.5,
        9999995.0,
    ];
    let negative: Vec<f64> = v.iter().map(|x| -x).collect();
    v.extend(negative);
    v
}

/// A double from one of several families: special values, any bit pattern, short decimals, exact
/// binary fractions (ties), and values near a decimal rounding boundary.
fn random_double(rng: &mut Rng, specials: &[f64]) -> f64 {
    match rng.below(8) {
        0 => *rng.pick(specials),
        1 | 2 => f64::from_bits(rng.next()),
        3 => {
            // A short decimal, mantissa and exponent: 1234e-7.
            let digits = rng.below(18) + 1;
            let mut mantissa = String::new();
            for _ in 0..digits {
                mantissa.push(char::from(b'0' + rng.below(10) as u8));
            }
            let exponent = rng.range(-330, 310);
            format!("{mantissa}e{exponent}").parse().unwrap()
        }
        4 => {
            // An exact binary fraction with few bits: ties for %.Nf and %.Ne.
            let bits = rng.below(40);
            let numerator = rng.below(1 << bits) as f64;
            let denominator = (1u64 << rng.below(40)) as f64;
            numerator / denominator
        }
        5 => {
            // A tie at some decimal digit: d.ddd5 times a power of ten, nearest double.
            let digits = rng.below(16) + 1;
            let mut s = String::new();
            for _ in 0..digits {
                s.push(char::from(b'0' + rng.below(10) as u8));
            }
            s.push('5');
            let exponent = rng.range(-30, 30);
            format!("{s}e{exponent}").parse().unwrap()
        }
        6 => {
            // A neighbour of a power of ten.
            let p: f64 = format!("1e{}", rng.range(-310, 308)).parse().unwrap();
            let bits = p.to_bits();
            f64::from_bits(bits.wrapping_add(rng.range(-3, 3) as u64))
        }
        _ => {
            // A subnormal or a small normal.
            f64::from_bits(rng.below(1 << 54))
        }
    }
    .copysign(if rng.chance(30) { -1.0 } else { 1.0 })
}

/// An integer of a random number of bits, or one of the limits.
fn random_integer(rng: &mut Rng) -> u64 {
    match rng.below(6) {
        0 => *rng.pick(&[
            0,
            1,
            u64::MAX,
            i64::MAX as u64,
            i64::MIN as u64,
            u64::from(u32::MAX),
            i32::MAX as u64,
            i32::MIN as i64 as u64,
            0x8000_0000,
            0xFFFF,
            0x8000,
            0xFF,
            0x80,
            0x7F,
            10,
            100,
            8,
        ]),
        1 => rng.next(),
        _ => {
            let bits = rng.below(64) + 1;
            let v = rng.next() >> (64 - bits);
            if rng.chance(30) { v.wrapping_neg() } else { v }
        }
    }
}

/// The strings `%s` and `%ls` print: NUL-terminated, with spare NULs after them so that a string
/// read as the other width still ends inside its buffer.
struct Strings {
    narrow: Vec<Vec<u8>>,
    wide: Vec<Vec<u16>>,
}

impl Strings {
    fn new() -> Strings {
        let mut narrow: Vec<Vec<u8>> = [
            &b""[..],
            b"a",
            b"hello",
            b"(null)",
            b"x y z",
            b"0123456789abcdefghijklmnopqrstuvwxyz",
            b"caf\xE9 \x80\xFF",
            b"%d %s",
        ]
        .iter()
        .map(|s| s.to_vec())
        .collect();
        narrow.push(b"long ".repeat(70));
        for s in &mut narrow {
            s.extend_from_slice(&[0, 0, 0, 0]);
        }
        let mut wide: Vec<Vec<u16>> = vec![
            vec![],
            vec![0x48, 0x69],
            vec![0xE9, 0xFF, 0x41],
            vec![0x41, 0x100, 0x42],
            vec![0x20AC],
            vec![0x7F, 0x80, 0x9F],
            "wide string of some length".encode_utf16().collect(),
        ];
        for s in &mut wide {
            s.extend_from_slice(&[0, 0]);
        }
        Strings { narrow, wide }
    }
}

/// One printf call: the format (NUL-terminated), the arguments orca-fmt gets and the slots the UCRT
/// gets, one for one.
struct Call<'a> {
    format: Vec<u8>,
    args: Vec<Arg<'a>>,
    slots: Vec<u64>,
}

impl<'a> Call<'a> {
    fn new() -> Call<'a> {
        Call {
            format: Vec::new(),
            args: Vec::new(),
            slots: Vec::new(),
        }
    }

    fn push(&mut self, arg: Arg<'a>) {
        let slot = match arg {
            Arg::Int(v) => v as u64,
            Arg::UInt(v) => v,
            Arg::Double(v) => v.to_bits(),
            Arg::Str(s) => s.as_ptr() as u64,
            Arg::WStr(s) => s.as_ptr() as u64,
            Arg::Null => 0,
            Arg::Ptr(p) => p as u64,
        };
        self.args.push(arg);
        self.slots.push(slot);
    }
}

/// What the UCRT printed, or the `errno` of its failure.
fn ucrt_printf(mode: Mode, format: &[u8], slots: &[u64]) -> Result<Vec<u8>, c_int> {
    assert_eq!(format.last(), Some(&0), "the format is NUL-terminated");
    // Spare zero slots: a format the generator did not plan reads zeros, as orca-fmt reads
    // Arg::UInt(0) (both sides get the same list).
    let mut padded = slots.to_vec();
    padded.resize(slots.len() + 64, 0);
    let mut buffer = vec![0u8; 1 << 16];
    // SAFETY: the format is NUL-terminated; the slots are a va_list of x64 (an array of 8-byte
    // slots), with a string slot pointing at a NUL-terminated string that lives for the call; the
    // buffer is writable for its length.
    let n = unsafe {
        let options = *ucrt::__local_stdio_printf_options();
        set_errno(0);
        match mode {
            Mode::Standard => ucrt::__stdio_common_vsprintf(
                options | STANDARD_SNPRINTF_BEHAVIOR,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                format.as_ptr().cast(),
                std::ptr::null_mut(),
                padded.as_ptr(),
            ),
            Mode::Secure => ucrt::__stdio_common_vsnprintf_s(
                options,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                TRUNCATE,
                format.as_ptr().cast(),
                std::ptr::null_mut(),
                padded.as_ptr(),
            ),
        }
    };
    if n < 0 {
        return Err(errno());
    }
    let n = n as usize;
    assert!(n < buffer.len(), "the UCRT's output fits the test's buffer");
    buffer.truncate(n);
    Ok(buffer)
}

/// Collects the cases where orca-fmt and the UCRT differ, and prints the first of them.
#[derive(Default)]
struct Mismatches {
    count: u64,
    shown: Vec<String>,
}

impl Mismatches {
    fn add(&mut self, what: String) {
        self.count += 1;
        if self.shown.len() < 40 {
            self.shown.push(what);
        }
    }

    fn assert_none(&self, cases: u64, name: &str) {
        assert!(
            self.count == 0,
            "{name}: {} of {cases} cases differ from the UCRT; the first:\n{}",
            self.count,
            self.shown.join("\n")
        );
    }
}

/// Compares one call in both families; returns the number of comparisons made (2).
fn compare_printf(call: &Call<'_>, mismatches: &mut Mismatches) -> u64 {
    let mut format = call.format.clone();
    format.push(0);
    // orca-fmt gets the same spare zero slots as the UCRT.
    let mut args = call.args.clone();
    args.resize(call.args.len() + 64, Arg::UInt(0));
    for mode in [Mode::Standard, Mode::Secure] {
        let theirs = ucrt_printf(mode, &format, &call.slots);
        let ours = printf::format(mode, &call.format, &args);
        let same = match (&ours, &theirs) {
            (Ok(a), Ok(b)) => a == b,
            (Err(FormatError::Invalid), Err(e)) => *e == EINVAL,
            (Err(FormatError::Unconvertible), Err(e)) => *e == EILSEQ,
            (Err(FormatError::Overflow), Err(_)) => true,
            _ => false,
        };
        if !same {
            let show = |r: &Result<Vec<u8>, String>| match r {
                Ok(v) => format!("{:?}", String::from_utf8_lossy(v)),
                Err(e) => e.clone(),
            };
            mismatches.add(format!(
                "{mode:?} {:?} args {:?}: orca-fmt {} UCRT {}",
                String::from_utf8_lossy(&call.format),
                call.args,
                show(&ours.map_err(|e| format!("<{e:?}>"))),
                show(&theirs.map_err(|e| format!("<-1 errno {e}>"))),
            ));
        }
    }
    2
}

const FLAGS: &[u8] = b"-+ #0";
const LENGTHS: &[&str] = &[
    "", "", "", "", "", "", "h", "hh", "l", "ll", "L", "w", "j", "z", "t", "I", "I32", "I64", "T",
    "Ih", "Il", "Ill", "II64", "lh", "hl", "hhh", "lll", "LL", "I64I64", "jl", "Iw",
];
const CONVERSIONS: &[u8] = b"diuoxXcCsSpeEfFgGaAZ%n";

/// One conversion, valid or not, with its arguments, appended to `call`.
fn push_conversion<'a>(
    rng: &mut Rng,
    call: &mut Call<'a>,
    strings: &'a Strings,
    specials: &[f64],
    conversion: u8,
) {
    let float = b"eEfFgGaA".contains(&conversion);
    call.format.push(b'%');
    for _ in 0..rng.below(4) {
        if rng.chance(70) {
            call.format.push(*rng.pick(FLAGS));
        }
    }
    match rng.below(20) {
        0..=7 => {}
        8..=13 => call
            .format
            .extend_from_slice(rng.below(25).to_string().as_bytes()),
        14..=17 => {
            call.format.push(b'*');
            let width = if rng.chance(2) {
                i64::from(i32::MIN)
            } else {
                rng.range(-30, 30)
            };
            call.push(Arg::Int(width));
        }
        _ => call
            .format
            .extend_from_slice(rng.range(100, 400).to_string().as_bytes()),
    }
    match rng.below(20) {
        0..=7 => {}
        8 => call.format.push(b'.'),
        9..=14 => call
            .format
            .extend_from_slice(format!(".{}", rng.below(25)).as_bytes()),
        15..=17 => {
            call.format.extend_from_slice(b".*");
            call.push(Arg::Int(rng.range(-10, 40)));
        }
        // A long precision, now and then: up to every digit of a double's exact value and beyond.
        _ if rng.chance(20) => {
            let high = if float { 1100 } else { 300 };
            call.format
                .extend_from_slice(format!(".{}", rng.range(30, high)).as_bytes());
        }
        _ => call
            .format
            .extend_from_slice(format!(".{}", rng.range(25, 60)).as_bytes()),
    }
    let length = *rng.pick(LENGTHS);
    call.format.extend_from_slice(length.as_bytes());
    call.format.push(conversion);

    // The argument the conversion reads, of the kind the UCRT reads for it.
    match conversion {
        b'd' | b'i' | b'u' | b'o' | b'x' | b'X' => {
            let v = random_integer(rng);
            call.push(if rng.chance(50) {
                Arg::Int(v as i64)
            } else {
                Arg::UInt(v)
            });
        }
        b'c' | b'C' => {
            let v = if rng.chance(70) {
                rng.below(0x200)
            } else {
                rng.next()
            };
            call.push(Arg::UInt(v));
        }
        b's' | b'S' => {
            // The string conversions are wide with l or w (%s) or without h and T (%S), a bare I
            // giving way to what follows it.
            let effective = length.trim_start_matches('I');
            let wide = if conversion == b's' {
                matches!(effective, "l" | "w")
            } else {
                !matches!(effective, "h" | "T")
            };
            if rng.chance(10) {
                call.push(Arg::Null);
            } else if wide {
                let s: &Vec<u16> = rng.pick(&strings.wide);
                call.push(Arg::WStr(s));
            } else {
                let s: &Vec<u8> = rng.pick(&strings.narrow);
                call.push(Arg::Str(s));
            }
        }
        b'p' => call.push(Arg::Ptr(random_integer(rng) as usize)),
        b'Z' | b'n' => call.push(Arg::Null),
        b'%' => {}
        _ => {
            let v = random_double(rng, specials);
            call.push(if rng.chance(5) {
                Arg::UInt(v.to_bits())
            } else {
                Arg::Double(v)
            });
        }
    }
}

/// Text between conversions, without `%`.
fn push_text(rng: &mut Rng, call: &mut Call<'_>) {
    for _ in 0..rng.below(4) {
        call.format
            .push(*rng.pick(b"ab Z.-+0123456789\t,;:=()[]x\xC3\xA9"));
    }
}

/// Formats of one to four conversions of every kind, with random flags, widths, precisions and
/// lengths (valid or not) and arguments of the kind each reads, in both families.
#[test]
fn printf_matches_the_ucrt_on_random_conversions() {
    install_handler();
    let strings = Strings::new();
    let specials = special_doubles();
    let mut rng = Rng(0x5EED_0001);
    let mut mismatches = Mismatches::default();
    let mut cases = 0;
    for _ in 0..150_000 {
        let mut call = Call::new();
        for _ in 0..=rng.below(4) {
            push_text(&mut rng, &mut call);
            let conversion = *rng.pick(CONVERSIONS);
            push_conversion(&mut rng, &mut call, &strings, &specials, conversion);
        }
        push_text(&mut rng, &mut call);
        cases += compare_printf(&call, &mut mismatches);
    }
    mismatches.assert_none(cases, "random conversions");
    assert_eq!(cases, 300_000);
}

/// Each double printed with a random float conversion, flags, width and precision: a million
/// seeded values.
#[test]
fn printf_matches_the_ucrt_on_a_million_doubles() {
    install_handler();
    let specials = special_doubles();
    let mut rng = Rng(0x5EED_0002);
    let mut mismatches = Mismatches::default();
    let mut cases = 0;
    for i in 0..1_000_000u64 {
        let value = if (i as usize) < specials.len() {
            specials[i as usize]
        } else {
            random_double(&mut rng, &specials)
        };
        let conversion = *rng.pick(b"eEfFgGaA");
        let mut call = Call::new();
        call.format.push(b'%');
        for _ in 0..rng.below(3) {
            call.format.push(*rng.pick(FLAGS));
        }
        if rng.chance(30) {
            call.format
                .extend_from_slice(rng.below(30).to_string().as_bytes());
        }
        match rng.below(10) {
            0..=2 => {}
            3..=8 => call
                .format
                .extend_from_slice(format!(".{}", rng.below(25)).as_bytes()),
            _ => call
                .format
                .extend_from_slice(format!(".{}", rng.range(25, 800)).as_bytes()),
        }
        call.format.push(conversion);
        call.push(Arg::Double(value));
        cases += compare_printf(&call, &mut mismatches);
    }
    mismatches.assert_none(cases, "doubles");
    assert_eq!(cases, 2_000_000);
}

/// Each integer printed with a random integer conversion, length, flags, width and precision: a
/// million seeded values.
#[test]
fn printf_matches_the_ucrt_on_a_million_integers() {
    install_handler();
    let mut rng = Rng(0x5EED_0003);
    let mut mismatches = Mismatches::default();
    let mut cases = 0;
    for _ in 0..1_000_000u64 {
        let value = random_integer(&mut rng);
        let mut call = Call::new();
        call.format.push(b'%');
        for _ in 0..rng.below(4) {
            call.format.push(*rng.pick(FLAGS));
        }
        if rng.chance(40) {
            call.format
                .extend_from_slice(rng.below(30).to_string().as_bytes());
        }
        if rng.chance(40) {
            call.format
                .extend_from_slice(format!(".{}", rng.below(30)).as_bytes());
        }
        call.format.extend_from_slice(
            rng.pick(&[
                "", "", "h", "hh", "l", "ll", "j", "z", "t", "I", "I32", "I64",
            ])
            .as_bytes(),
        );
        call.format.push(*rng.pick(b"diuoxXcp"));
        call.push(Arg::UInt(value));
        cases += compare_printf(&call, &mut mismatches);
    }
    mismatches.assert_none(cases, "integers");
    assert_eq!(cases, 2_000_000);
}

/// Random byte strings over printf's own characters and others: formats with every kind of
/// mistake, which the two families treat differently. No conversion reads a string, so any slot
/// the format happens to read is safe to read; widths and precisions stay small.
#[test]
fn printf_matches_the_ucrt_on_random_formats() {
    install_handler();
    let mut rng = Rng(0x5EED_0004);
    let mut mismatches = Mismatches::default();
    let mut cases = 0;
    let alphabet: &[u8] =
        b"%%%%%%-+ #0123456789..**hlLwjztIIT36diuoxXcCpeEfFgGaAn%y$q\xC3\xA9\x80 a";
    let mut tested = 0;
    while tested < 300_000 {
        let mut call = Call::new();
        for _ in 0..rng.range(1, 12) {
            call.format.push(*rng.pick(alphabet));
        }
        // A width or precision of five to ten digits would print up to 2 GB.
        let mut run = 0;
        let long_number = call.format.iter().any(|c| {
            run = if c.is_ascii_digit() { run + 1 } else { 0 };
            run > 3
        });
        if long_number {
            continue;
        }
        for _ in 0..8 {
            // A slot read as an int is small; read as a double it is a double whose low 32 bits
            // are small.
            let small = rng.range(-40, 40) as u64 & 0xFFFF_FFFF;
            let slot = if rng.chance(50) {
                small | (random_double(&mut rng, &[1.5]).to_bits() & !0xFFFF_FFFF)
            } else {
                small
            };
            call.push(Arg::UInt(slot));
        }
        cases += compare_printf(&call, &mut mismatches);
        tested += 1;
    }
    mismatches.assert_none(cases, "random formats");
    assert_eq!(cases, 600_000);
}

/// Hand-picked formats: each mistake the UCRT treats in its own way, and the limits.
#[test]
fn printf_matches_the_ucrt_on_chosen_formats() {
    install_handler();
    let strings = Strings::new();
    let mut mismatches = Mismatches::default();
    let mut cases = 0;
    let int_args = |values: &[i64]| values.iter().map(|&v| Arg::Int(v)).collect::<Vec<_>>();
    let mut chosen: Vec<(&[u8], Vec<Arg<'_>>)> = vec![
        (b"%2147483648d|", int_args(&[0])),
        (b"%.2147483648d|", int_args(&[0])),
        (b"%99999999999999999999d|", int_args(&[0])),
        (b"%*d|", int_args(&[i64::from(i32::MIN), 1])),
        (b"%.*d|", int_args(&[i64::from(i32::MIN), 1])),
        (b"%5lc|%-5lc|%05lc|", int_args(&[0x100, 0x20AC, 0x41])),
        (b"%5C|%5wc|", int_args(&[0x1FF, 0xFF])),
        (b"%10.2ls|", vec![Arg::WStr(&strings.wide[3])]),
        (b"%10.1ls|", vec![Arg::WStr(&strings.wide[3])]),
        (b"%-10ls|", vec![Arg::WStr(&strings.wide[4])]),
        (b"%s|%10s|%.2s|%ls|%S|%.3S", vec![Arg::Null; 6]),
        (b"%Z|%10Z|%.2Z|%-8lZ|%hZ", vec![Arg::Null; 5]),
        (b"%c%c%c", int_args(&[0, 0x141, -1])),
        (b"%%%%%|%", vec![]),
        (b"%\0%d", int_args(&[1])),
        (b"abc%", vec![]),
        (
            b"%p|%#p|%+p|%.3p|%020p|%-20p|%lp|%hp",
            int_args(&[-1, 1, 1, 1, 1, 1, 1, 1]),
        ),
    ];
    let big_precision: Vec<u8> = b"%.1100f|%.1100e|%.1100g|%#.1100g|%.1100a".to_vec();
    chosen.push((&big_precision, vec![Arg::Double(5e-324); 5]));
    let big_width = b"%1100.3f|%-1100d|%01100x".to_vec();
    chosen.push((
        &big_width,
        vec![Arg::Double(-1.5), Arg::Int(7), Arg::Int(255)],
    ));
    for (format, args) in chosen {
        let mut call = Call::new();
        call.format = format.to_vec();
        for arg in args {
            call.push(arg);
        }
        cases += compare_printf(&call, &mut mismatches);
    }
    mismatches.assert_none(cases, "chosen formats");
    assert_eq!(cases, 38);
}

/// `snprintf` cuts the output at the buffer and NUL-terminates it, as the UCRT's C99 `snprintf`.
#[test]
fn snprintf_cuts_as_the_ucrt_does() {
    install_handler();
    let mut cases = 0;
    for size in 0..12 {
        let calls = [
            (&b"%d-%d\0"[..], [Arg::Int(123_456), Arg::Int(-7)]),
            (b"%.3f %x\0", [Arg::Double(-2.0 / 3.0), Arg::Int(0xBEEF)]),
        ];
        for (format, args) in calls {
            let mut ours = vec![0xAAu8; size];
            let mut theirs = vec![0xAAu8; size];
            let n = printf::snprintf(&mut ours, format, &args).unwrap();
            let mut call = Call::new();
            for arg in args {
                call.push(arg);
            }
            let slots = call.slots;
            // SAFETY: as in ucrt_printf; `theirs` is writable for `size` bytes.
            let m = unsafe {
                ucrt::__stdio_common_vsprintf(
                    *ucrt::__local_stdio_printf_options() | STANDARD_SNPRINTF_BEHAVIOR,
                    theirs.as_mut_ptr().cast(),
                    size,
                    format.as_ptr().cast(),
                    std::ptr::null_mut(),
                    slots.as_ptr(),
                )
            };
            assert_eq!(
                n as c_int,
                m,
                "{:?} into {size}",
                String::from_utf8_lossy(format)
            );
            assert_eq!(
                ours,
                theirs,
                "{:?} into {size}",
                String::from_utf8_lossy(format)
            );
            cases += 1;
        }
    }
    assert_eq!(cases, 24);
}

/// A finite double's exact value in fixed notation, trailing zeros dropped.
fn fixed(v: f64) -> Vec<u8> {
    let mut t = printf::sprintf(b"%.1100f", &[Arg::Double(v)]).unwrap();
    while t.last() == Some(&b'0') {
        t.pop();
    }
    if t.last() == Some(&b'.') {
        t.pop();
    }
    t
}

/// The exact value halfway between a positive double from 2^-1021 up and the next one: the sum of
/// the two exact decimals of `v` and of half its ulp (a power of two, so a double too).
fn double_midpoint(v: f64) -> Vec<u8> {
    let half_ulp = (f64::from_bits(v.to_bits() + 1) - v) / 2.0;
    let a = printf::sprintf(b"%.1100f", &[Arg::Double(v)]).unwrap();
    let b = printf::sprintf(b"%.1100f", &[Arg::Double(half_ulp)]).unwrap();
    // Both have 1100 decimals: align the integer parts and add digit by digit.
    let width = if a.len() < b.len() { b.len() } else { a.len() };
    let pad = |s: &[u8]| {
        let mut p = vec![b'0'; width - s.len()];
        p.extend_from_slice(s);
        p
    };
    let (a, b) = (pad(&a), pad(&b));
    let mut sum = vec![b'0'; width];
    let mut carry = 0;
    for i in (0..width).rev() {
        if a[i] == b'.' {
            sum[i] = b'.';
            continue;
        }
        let d = (a[i] - b'0') + (b[i] - b'0') + carry;
        sum[i] = b'0' + d % 10;
        carry = d / 10;
    }
    if carry > 0 {
        sum.insert(0, b'1');
    }
    while sum.last() == Some(&b'0') {
        sum.pop();
    }
    if sum.last() == Some(&b'.') {
        sum.pop();
    }
    sum
}

/// A text for the parsers: number-like pieces glued at random, decimal and hexadecimal numbers
/// (long ones and huge exponents among them), `to_chars` output, near-ties written out in full,
/// exact ties between two floats or doubles and texts beside them, long runs of zeros with
/// exponents beyond ±5200, and `inf` and `nan` spellings, with white space, signs and trailing
/// junk.
fn random_number_text(rng: &mut Rng, specials: &[f64]) -> Vec<u8> {
    const PIECES: &[&[u8]] = &[
        b" ",
        b"\t",
        b"\n",
        b"\x0B",
        b"\x0C",
        b"\r",
        b"\xA0",
        b"+",
        b"-",
        b"0",
        b"1",
        b"9",
        b"5",
        b".",
        b"e",
        b"E",
        b"p",
        b"P",
        b"x",
        b"X",
        b"0x",
        b"0X",
        b"inf",
        b"INF",
        b"infinity",
        b"iNfIn",
        b"nan",
        b"NaN",
        b"(",
        b")",
        b"ind",
        b"IND",
        b"snan",
        b"_",
        b"a",
        b"f",
        b"z",
        b"d",
        b"D",
        b"12345678901234567890",
        b"1e308",
        b"1e-324",
        b"4e-324",
        b"2.2250738585072011e-308",
    ];
    let mut s = Vec::new();
    if rng.chance(10) {
        s.extend_from_slice(rng.piece(&[b" ", b"\t\n", b"  \r"]));
    }
    if rng.chance(30) {
        s.push(*rng.pick(b"+-"));
    }
    match rng.below(9) {
        0 => {
            for _ in 0..rng.range(1, 8) {
                s.extend_from_slice(rng.piece(PIECES));
            }
        }
        1 | 2 => {
            let long = rng.chance(5);
            let digits = rng.range(1, if long { 900 } else { 25 });
            for _ in 0..digits {
                s.push(b'0' + rng.below(10) as u8);
            }
            if rng.chance(50) {
                let at = rng.below(s.len() as u64 + 1) as usize;
                s.insert(at, b'.');
            }
            if rng.chance(60) {
                s.push(*rng.pick(b"eE"));
                if rng.chance(50) {
                    s.push(*rng.pick(b"+-"));
                }
                let exponent = match rng.below(10) {
                    0 => rng.below(7000).to_string(),
                    1 => "9".repeat(rng.range(18, 25) as usize),
                    _ => rng.below(400).to_string(),
                };
                s.extend_from_slice(exponent.as_bytes());
            }
        }
        3 => {
            s.extend_from_slice(rng.piece(&[b"0x", b"0X"]));
            let first_hexit = s.len();
            let most = if rng.chance(5) { 300 } else { 20 };
            let digits = rng.range(0, most);
            for _ in 0..digits {
                s.push(*rng.pick(b"0123456789abcdefABCDEF"));
            }
            if rng.chance(50) {
                let at = first_hexit + rng.below((s.len() - first_hexit) as u64 + 1) as usize;
                s.insert(at, b'.');
            }
            if rng.chance(60) {
                s.push(*rng.pick(b"pP"));
                if rng.chance(50) {
                    s.push(*rng.pick(b"+-"));
                }
                s.extend_from_slice(rng.below(2300).to_string().as_bytes());
            }
        }
        4 => {
            let v = random_double(rng, specials);
            let text = match rng.below(3) {
                0 => to_chars::to_chars_string(v),
                1 => to_chars::to_chars_format_string(v, CharsFormat::Scientific),
                _ => format!(
                    "0x{}",
                    to_chars::to_chars_format_string(v, CharsFormat::Hex)
                ),
            };
            s.extend_from_slice(text.trim_start_matches('-').as_bytes());
        }
        5 => {
            // A double's exact value written out, cut short somewhere: on, just below or just
            // above a rounding boundary.
            let v = f64::from_bits(rng.below(0x7FF0_0000_0000_0000));
            let mut t = printf::sprintf(b"%.1100e", &[Arg::Double(v)]).unwrap();
            let e = t.iter().position(|&c| c == b'e').unwrap();
            let cut = rng.range(17, e as i64) as usize;
            t.drain(cut..e);
            if rng.chance(30) {
                let last = cut - 1;
                if t[last].is_ascii_digit() {
                    t[last] = b'0' + (t[last] - b'0' + 1) % 10;
                }
            }
            s.extend_from_slice(&t);
        }
        6 => {
            // An exact tie between two floats or two doubles, or a digit or a few hundred beside
            // one, past the 768th digit among them.
            let mut t = if rng.chance(50) {
                let f = f32::from_bits(rng.below(0x7F7F_FFFF) as u32);
                let up = f32::from_bits(f.to_bits() + 1);
                fixed((f64::from(f) + f64::from(up)) / 2.0)
            } else {
                let v =
                    f64::from_bits(rng.range(0x0020_0000_0000_0000, 0x7FEF_FFFF_FFFF_FFFF) as u64);
                double_midpoint(v)
            };
            match rng.below(4) {
                0 => {
                    let last = t.len() - 1;
                    t[last] = if t[last] == b'9' { b'8' } else { t[last] + 1 };
                }
                1 => {
                    if !t.contains(&b'.') {
                        t.push(b'.');
                    }
                    let zeros = rng.range(0, 900) as usize;
                    t.extend(std::iter::repeat_n(b'0', zeros));
                    t.push(b'1');
                }
                _ => {}
            }
            s.extend_from_slice(&t);
        }
        7 => {
            // Long runs of zeros before or after the digits, with exponents beyond ±5200.
            let zeros = rng.range(0, 7000) as usize;
            if rng.chance(50) {
                s.extend_from_slice(b"0.");
                s.extend(std::iter::repeat_n(b'0', zeros));
                s.extend_from_slice(b"123");
            } else {
                s.extend_from_slice(b"123");
                s.extend(std::iter::repeat_n(b'0', zeros));
            }
            s.push(b'e');
            s.push(*rng.pick(b"+-"));
            s.extend_from_slice(rng.range(0, 7000).to_string().as_bytes());
        }
        _ => {
            let names: &[&[u8]] = &[b"nan", b"NAN", b"nAn", b"inf", b"InFiNiTy", b"infinit"];
            s.extend_from_slice(rng.piece(names));
            if rng.chance(60) {
                s.push(b'(');
                for _ in 0..rng.below(6) {
                    s.push(*rng.pick(b"indsnaINDSNA_09z-. "));
                }
                if rng.chance(70) {
                    s.push(b')');
                }
            }
        }
    }
    if rng.chance(20) {
        s.extend_from_slice(rng.piece(PIECES));
    }
    s
}

fn errno_of(errno: Option<Errno>) -> c_int {
    match errno {
        None => 0,
        Some(Errno::Range) => ERANGE,
        Some(Errno::Invalid) => EINVAL,
    }
}

/// `strtod`, `strtof` and `atof` against the UCRT's, value bits, end and `errno`, on random texts.
#[test]
fn strtod_and_strtof_match_the_ucrt() {
    install_handler();
    let specials = special_doubles();
    let mut rng = Rng(0x5EED_0005);
    let mut mismatches = Mismatches::default();
    let mut cases = 0;
    let mut ucrt_rounds_otherwise = 0;
    for _ in 0..300_000 {
        let text = random_number_text(&mut rng, &specials);
        let mut c_text = text.clone();
        c_text.push(0);
        let start = c_text.as_ptr().cast::<c_char>();
        let shown = String::from_utf8_lossy(&text).into_owned();

        let mut end: *mut c_char = std::ptr::null_mut();
        set_errno(0);
        // SAFETY: c_text is NUL-terminated; end receives a pointer into it.
        let d = unsafe { ucrt::strtod(start, &raw mut end) };
        let theirs = (d.to_bits(), end as usize - start as usize, errno());
        let r = parse::strtod(&text);
        let ours = (r.value.to_bits(), r.end, errno_of(r.errno));
        if ours != theirs {
            mismatches.add(format!(
                "strtod {shown:?}: orca-fmt {ours:x?} UCRT {theirs:x?}"
            ));
        }

        set_errno(0);
        // SAFETY: as above.
        let f = unsafe { ucrt::strtof(start, &raw mut end) };
        let theirs = (
            u64::from(f.to_bits()),
            end as usize - start as usize,
            errno(),
        );
        let r = parse::strtof(&text);
        let ours = (u64::from(r.value.to_bits()), r.end, errno_of(r.errno));
        if ours != theirs {
            mismatches.add(format!(
                "strtof {shown:?}: orca-fmt {ours:x?} UCRT {theirs:x?}"
            ));
        }

        // SAFETY: as above.
        let a = unsafe { ucrt::atof(start) };
        if a.to_bits() != parse::atof(&text).to_bits() {
            mismatches.add(format!("atof {shown:?}"));
        }
        cases += 3;

        // Counts the plain decimal texts, read whole by both, that strtof rounds otherwise than
        // from_chars: the UCRT's differences are exercised, not only allowed.
        if text.first().is_some_and(u8::is_ascii_digit) && !text.starts_with(b"0x") {
            let mut stl = 0f32;
            let fc = parse::from_chars_float(&text, &mut stl, CharsFormat::General);
            if fc.ptr == r.end && fc.ec.is_none() && stl.to_bits() != r.value.to_bits() {
                ucrt_rounds_otherwise += 1;
            }
        }
    }
    mismatches.assert_none(cases, "strtod");
    assert_eq!(cases, 900_000);
    assert!(
        ucrt_rounds_otherwise > 100,
        "{ucrt_rounds_otherwise} texts where the UCRT differs"
    );
}

/// A text for the integer parsers: signs, prefixes, digits of every base and the limits.
fn random_integer_text(rng: &mut Rng) -> Vec<u8> {
    const PIECES: &[&[u8]] = &[
        b"-",
        b"+",
        b" ",
        b"\t",
        b"0",
        b"0x",
        b"0X",
        b"1",
        b"7",
        b"8",
        b"9",
        b"f",
        b"F",
        b"z",
        b"Z",
        b"g",
        b"_",
        b".",
        b"18446744073709551615",
        b"18446744073709551616",
        b"9223372036854775807",
        b"9223372036854775808",
        b"2147483647",
        b"2147483648",
        b"4294967295",
        b"4294967296",
        b"255",
        b"256",
        b"36893488147419103232",
    ];
    let mut s = Vec::new();
    for _ in 0..rng.range(1, 5) {
        s.extend_from_slice(rng.piece(PIECES));
    }
    s
}

/// `strtol`, `strtoul`, `strtoll`, `strtoull`, `atoi`, `atol` and `atoll` against the UCRT's, in
/// every base and some invalid ones.
#[test]
fn strtol_family_matches_the_ucrt() {
    install_handler();
    let mut rng = Rng(0x5EED_0006);
    let mut mismatches = Mismatches::default();
    let mut cases = 0;
    for _ in 0..200_000 {
        let text = random_integer_text(&mut rng);
        let base = match rng.below(20) {
            0..=4 => 0,
            5..=9 => 10,
            10 => *rng.pick(&[-1, 1, 37, 100]),
            _ => rng.range(2, 36) as i32,
        };
        let mut c_text = text.clone();
        c_text.push(0);
        let start = c_text.as_ptr().cast::<c_char>();
        let shown = String::from_utf8_lossy(&text).into_owned();

        macro_rules! compare {
            ($name:literal, $ucrt:path, $ours:path) => {{
                let mut end: *mut c_char = start.cast_mut();
                set_errno(0);
                // SAFETY: c_text is NUL-terminated; end receives a pointer into it.
                let v = unsafe { $ucrt(start, &raw mut end, base) };
                let theirs = (v as u64, end as usize - start as usize, errno());
                let r = $ours(&text, base);
                let ours = (r.value as u64, r.end, errno_of(r.errno));
                if ours != theirs {
                    mismatches.add(format!(
                        "{} {shown:?} base {base}: orca-fmt {ours:?} UCRT {theirs:?}",
                        $name
                    ));
                }
                cases += 1;
            }};
        }
        compare!("strtol", ucrt::strtol, parse::strtol);
        compare!("strtoul", ucrt::strtoul, parse::strtoul);
        compare!("strtoll", ucrt::strtoll, parse::strtoll);
        compare!("strtoull", ucrt::strtoull, parse::strtoull);

        // SAFETY: c_text is NUL-terminated.
        let theirs = unsafe { (ucrt::atoi(start), ucrt::atol(start), ucrt::atoll(start)) };
        let ours = (parse::atoi(&text), parse::atol(&text), parse::atoll(&text));
        if ours != theirs {
            mismatches.add(format!("atoi/atol/atoll {shown:?}: {ours:?} {theirs:?}"));
        }
        cases += 1;
    }
    mismatches.assert_none(cases, "strtol");
    assert_eq!(cases, 1_000_000);
}

/// What a `std::sto*` function gives for what its C function gave.
fn sto_of<T>(r: Strto<T>) -> Result<(T, usize), StoError> {
    if r.end == 0 {
        Err(StoError::InvalidArgument)
    } else if r.errno == Some(Errno::Range) {
        Err(StoError::OutOfRange)
    } else {
        Ok((r.value, r.end))
    }
}

/// The `sto*` functions are the C functions with the STL's two exceptions.
#[test]
fn sto_functions_follow_the_c_functions() {
    let specials = special_doubles();
    let mut rng = Rng(0x5EED_0007);
    let mut cases = 0;
    for _ in 0..50_000 {
        let text = random_number_text(&mut rng, &specials);
        let bits = |r: Result<(f64, usize), StoError>| r.map(|(v, n)| (v.to_bits(), n));
        assert_eq!(bits(parse::stod(&text)), bits(sto_of(parse::strtod(&text))));
        let bits32 = |r: Result<(f32, usize), StoError>| r.map(|(v, n)| (v.to_bits(), n));
        assert_eq!(
            bits32(parse::stof(&text)),
            bits32(sto_of(parse::strtof(&text)))
        );
        let text = random_integer_text(&mut rng);
        let base = if rng.chance(50) {
            10
        } else {
            rng.range(2, 36) as i32
        };
        assert_eq!(parse::stoi(&text, base), sto_of(parse::strtol(&text, base)));
        assert_eq!(
            parse::stoul(&text, base),
            sto_of(parse::strtoul(&text, base))
        );
        assert_eq!(
            parse::stoll(&text, base),
            sto_of(parse::strtoll(&text, base))
        );
        assert_eq!(
            parse::stoull(&text, base),
            sto_of(parse::strtoull(&text, base))
        );
        cases += 6;
    }
    assert_eq!(cases, 300_000);
    assert_eq!(parse::stoi("  -12x", 10), Ok((-12, 5)));
    assert_eq!(parse::stoi("x", 10), Err(StoError::InvalidArgument));
    assert_eq!(parse::stol("2147483648", 10), Err(StoError::OutOfRange));
    assert_eq!(parse::stod("1e309"), Err(StoError::OutOfRange));
    assert_eq!(parse::stod(" 0x1p-1074"), Ok((5e-324, 10)));
}

/// The digits and exponent of a shortest scientific form: `1.25e-05` and Rust's `1.25e-5` give the
/// same.
fn digits_and_exponent(s: &str) -> (String, i32) {
    let (mantissa, exponent) = s.split_once('e').unwrap();
    (mantissa.replace(['.', '-'], ""), exponent.parse().unwrap())
}

/// Asserts that a shortest scientific form `ours` of `value` has the digits of Rust's own shortest
/// form, except where the exact value lies halfway between two shortest candidates (2^-25 is
/// 2.98023223876953125e-8): there Ryu, and so MSVC, takes the even one, and Rust the upper one.
fn assert_shortest_digits(ours: &str, rust: &str, value: f64) {
    let (a, exponent) = digits_and_exponent(ours);
    let (b, rust_exponent) = digits_and_exponent(rust);
    if (&a, exponent) == (&b, rust_exponent) {
        return;
    }
    let (x, y): (u64, u64) = (a.parse().unwrap(), b.parse().unwrap());
    let exact = printf::sprintf(b"%.1100e", &[Arg::Double(value)]).unwrap();
    let (exact_digits, exact_exponent) = digits_and_exponent(std::str::from_utf8(&exact).unwrap());
    let halfway = format!("{}5", x.min(y));
    assert!(
        exponent == rust_exponent
            && a.len() == b.len()
            && x.abs_diff(y) == 1
            && x % 2 == 0
            && exact_exponent == exponent
            && exact_digits.trim_end_matches('0') == halfway,
        "{ours} and Rust's {rust} differ, and not at a tie"
    );
}

/// Every finite double and float read back from its shortest `to_chars` form, by `from_chars` and
/// `strtod`, is the value written; the shortest form has the digits of Rust's own shortest
/// round-trip formatting but at exact ties; and the hex form, `%.17g`, `%.16e` and `%a` read back
/// too. A million.
#[test]
fn to_chars_round_trips_through_the_parsers() {
    let specials = special_doubles();
    let mut rng = Rng(0x5EED_0008);
    let mut cases = 0;
    for _ in 0..1_000_000 {
        let v = random_double(&mut rng, &specials);
        let f = f32::from_bits(rng.next() as u32);
        if !v.is_finite() || !f.is_finite() {
            continue;
        }
        let shortest = to_chars::to_chars_string(v);
        let mut back = f64::NAN;
        let r = parse::from_chars_float(shortest.as_bytes(), &mut back, CharsFormat::General);
        let whole = FromCharsResult {
            ptr: shortest.len(),
            ec: None,
        };
        assert_eq!((back.to_bits(), r), (v.to_bits(), whole), "{shortest}");
        assert_eq!(
            parse::strtod(shortest.as_bytes()).value.to_bits(),
            v.to_bits(),
            "{shortest}"
        );

        let scientific = to_chars::to_chars_format_string(v, CharsFormat::Scientific);
        assert_shortest_digits(&scientific, &format!("{v:e}"), v);

        let hex = to_chars::to_chars_format_string(v, CharsFormat::Hex);
        let r = parse::from_chars_float(hex.as_bytes(), &mut back, CharsFormat::Hex);
        assert_eq!((back.to_bits(), r.ec), (v.to_bits(), None), "{hex}");

        for format in [&b"%.17g"[..], b"%a", b"%.16e"] {
            let text = printf::sprintf(format, &[Arg::Double(v)]).unwrap();
            assert_eq!(parse::strtod(&text).value.to_bits(), v.to_bits());
        }

        let shortest = to_chars::to_chars_string(f);
        let mut back = f32::NAN;
        let r = parse::from_chars_float(shortest.as_bytes(), &mut back, CharsFormat::General);
        assert_eq!((back.to_bits(), r.ec), (f.to_bits(), None), "{shortest}");
        assert_eq!(
            parse::strtof(shortest.as_bytes()).value.to_bits(),
            f.to_bits()
        );
        let scientific = to_chars::to_chars_format_string(f, CharsFormat::Scientific);
        assert_shortest_digits(&scientific, &format!("{f:e}"), f64::from(f));
        cases += 1;
    }
    assert!(cases > 700_000, "{cases} finite pairs");
}

/// Integers through `to_chars` and back through `from_chars`, in every base and width.
#[test]
fn integers_round_trip() {
    let mut rng = Rng(0x5EED_0009);
    let mut buf = [0u8; 72];
    for _ in 0..200_000 {
        let v = random_integer(&mut rng) as i64;
        let base = rng.range(2, 36) as u32;
        let n = to_chars::to_chars_i64(&mut buf, v, base).unwrap();
        let mut back = 0i64;
        let whole = FromCharsResult { ptr: n, ec: None };
        assert_eq!(parse::from_chars_int(&buf[..n], &mut back, base), whole);
        assert_eq!(back, v);
        let mut narrow = 5i16;
        let r = parse::from_chars_int(&buf[..n], &mut narrow, base);
        match i16::try_from(v) {
            Ok(w) => assert_eq!((r.ec, narrow), (None, w)),
            Err(_) => assert_eq!((r.ec, narrow), (Some(Errc::ResultOutOfRange), 5)),
        }
        let n = to_chars::to_chars_u64(&mut buf, v as u64, base).unwrap();
        let mut back = 0u64;
        assert_eq!(parse::from_chars_int(&buf[..n], &mut back, base).ec, None);
        assert_eq!(back, v as u64);
    }
}

/// `ostream << double` builds its printf format from the stream's flags and pads the result: its
/// text without padding is the UCRT's `sprintf_s` of that format, and the padding goes where
/// `adjustfield` says.
#[test]
fn ostream_prints_what_the_ucrt_prints() {
    install_handler();
    let specials = special_doubles();
    let mut rng = Rng(0x5EED_000A);
    let mut cases = 0;
    let fields = [
        FloatField::Default,
        FloatField::Fixed,
        FloatField::Scientific,
        FloatField::Hex,
    ];
    for _ in 0..200_000 {
        let v = random_double(&mut rng, &specials);
        let float_field = *rng.pick(&fields);
        let mut stream = Stream {
            float_field,
            showpos: rng.chance(25),
            showpoint: rng.chance(25),
            uppercase: rng.chance(25),
            precision: if rng.chance(3) {
                rng.range(0, 400)
            } else {
                rng.range(-2, 22)
            },
            ..Stream::default()
        };
        // The format MSVC's _Ffmt builds, printed by the UCRT.
        let mut format = b"%".to_vec();
        if stream.showpos {
            format.push(b'+');
        }
        if stream.showpoint && v.is_finite() {
            format.push(b'#');
        }
        format.extend_from_slice(b".*");
        format.push(match (float_field, stream.uppercase) {
            (FloatField::Fixed, _) => b'f',
            (FloatField::Scientific, false) => b'e',
            (FloatField::Scientific, true) => b'E',
            (FloatField::Hex, false) => b'a',
            (FloatField::Hex, true) => b'A',
            (FloatField::Default, false) => b'g',
            (FloatField::Default, true) => b'G',
        });
        format.push(0);
        let precision = if float_field == FloatField::Hex {
            -1
        } else {
            stream.precision
        };
        let theirs = ucrt_printf(Mode::Secure, &format, &[precision as u64, v.to_bits()]).unwrap();
        let ours = ostream::f64_to_string(&mut stream, v).unwrap();
        assert_eq!(
            ours.as_bytes(),
            &theirs[..],
            "{:?} {v:?}",
            String::from_utf8_lossy(&format)
        );

        // The fill, before, after or inside the sign (and a hex float's 0x).
        let width = theirs.len() as i64 + rng.range(-2, 6);
        stream.width = width;
        stream.fill = b'*';
        stream.adjust = *rng.pick(&[Adjust::Right, Adjust::Left, Adjust::Internal]);
        let padded = ostream::f64_to_string(&mut stream, v).unwrap();
        assert_eq!(stream.width, 0);
        let fill = "*".repeat(usize::try_from(width - theirs.len() as i64).unwrap_or(0));
        let text = String::from_utf8(theirs).unwrap();
        let sign = usize::from(text.starts_with(['+', '-']));
        let hex_prefix = float_field == FloatField::Hex
            && text[sign..].len() >= 2
            && text[sign..sign + 2].eq_ignore_ascii_case("0x");
        let prefix = sign + if hex_prefix { 2 } else { 0 };
        let expected = match stream.adjust {
            Adjust::Right => format!("{fill}{text}"),
            Adjust::Left => format!("{text}{fill}"),
            Adjust::Internal => format!("{}{fill}{}", &text[..prefix], &text[prefix..]),
        };
        assert_eq!(padded, expected);
        cases += 2;
    }
    assert_eq!(cases, 400_000);
}

/// One stream of [`MSVC_OSTREAM`].
type RecordedStream = (
    u64,
    u8,
    bool,
    bool,
    bool,
    u8,
    i64,
    i64,
    u8,
    &'static [u8],
    &'static str,
);

/// What MSVC 14.42's `std::ostringstream` printed for these streams, recorded from a C++ program
/// built with `cl /O2 /EHsc /MT` (x64): `os.flags(...)`, `os.precision(p)`, `os.width(w)`,
/// `os.fill(c)`, an `imbue` of a `numpunct` with `,` and `.` for the comma locales, then
/// `os << value`. The fields: the value's bits; floatfield (0 none, 1 fixed, 2 scientific, 3
/// both); showpos, showpoint, uppercase; adjustfield (0 none, 1 left, 2 internal, 3 right);
/// precision; width; fill; grouping (none, `"\3"` or `"\1\2"`); the text.
#[rustfmt::skip]
const MSVC_OSTREAM: [RecordedStream; 25] = [
    (0x43F9_36ED_8848_ADAB, 1, false, false, false, 3, 1, 0, b'*', b"\x01\x02", "2.90.70.41.07.08.50.00.00.76.8,0"),
    (0x7C56_F1D7_B7B9_F040, 3, false, false, false, 3, 2, 25, b'*', b"\x01\x02", "***0x1,6f1d7b7b9f040p+966"),
    (0x6802_3506_B36B_B032, 2, true, false, false, 0, 1, 23, b' ', b"\x01\x02", "              +1,0e+193"),
    (0x7FF0_0000_0000_0001, 0, false, false, true, 2, 15, 36, b'*', b"\x03", "***************************NAN(SNAN)"),
    (0x3DFA_24F7_74C0_0000, 1, true, true, false, 1, 19, 17, b' ', b"\x03", "+0,0000000003804511053"),
    (0x4292_020B_F770_0000, 3, false, true, false, 1, 19, 31, b'*', b"\x03", "0x1,2020bf7700000p+42**********"),
    (0x432F_52BD_EF97_5800, 1, false, false, false, 1, 19, 0, b' ', b"\x03", "4.408.350.000.000.000,0000000000000000000"),
    (0x4363_55C8_7DC7_9EC8, 1, false, false, false, 3, 9, 0, b'*', b"\x03", "43.538.753.196.455.488,000000000"),
    (0x4340_0000_0000_0000, 1, false, false, false, 0, -1, 0, b'*', b"\x03", "9.007.199.254.740.992,000000"),
    (0x4266_7C15_1CC7_C060, 0, false, false, false, 0, 15, 38, b' ', b"\x03", "                   772.568.311.358,012"),
    (0x4146_B5F9_55E1_949C, 0, true, false, false, 1, 21, 39, b'*', b"\x03", "+2.976.754,67094667069614**************"),
    (0xC0EB_B79C_E000_0000, 0, false, false, false, 3, 16, 0, b'*', b"\x03", "-56.764,90234375"),
    (0xCC8B_92C2_440F_4BA3, 2, false, false, false, 2, 131, 17, b' ', b"\x03", "-5,53856842916834983823039041146959070448834209571069752875417600000000000000000000000000000000000000000000000000000000000000000000000e+60"),
    (0xB187_2B05_8322_DA01, 0, false, true, false, 2, -2, 20, b'*', b"", "-********4.19605e-70"),
    (0x421D_54D1_F6F2_0000, 3, true, false, false, 3, 5, 12, b' ', b"", "+0x1.d54d1f6f20000p+34"),
    (0x4049_0000_0000_0000, 3, false, true, true, 0, 20, 13, b' ', b"", "0X1.9000000000000P+5"),
    (0x73D7_ECB7_FF3A_FE63, 3, false, true, true, 0, 393, 0, b' ', b"", "0X1.7ECB7FF3AFE63P+830"),
    (0xFFF8_0000_0000_0000, 0, false, false, false, 2, 22, 0, b' ', b"", "-nan(ind)"),
    (0xFFF0_0000_0000_0000, 2, false, false, true, 2, 2, 0, b' ', b"", "-INF"),
    (0x7FF0_0000_0000_0001, 2, true, true, false, 1, 19, 29, b' ', b"", "+nan(snan)                   "),
    (0x515D_73C7_AC0C_5C53, 0, false, true, true, 2, 8, 0, b' ', b"", "8.9400000E+83"),
    (0x3E30_0000_0000_0000, 0, false, false, true, 3, 0, 13, b' ', b"", "        4E-09"),
    (0xE03B_2BAC_6DF0_50CD, 0, false, true, false, 3, 0, 0, b' ', b"", "-4.e+155"),
    (0x5AE9_0D48_54E2_2706, 2, true, false, true, 1, -2, 39, b'*', b"", "+8.682575E+129*************************"),
    (0x3F0D_7E00_0000_0000, 3, false, false, false, 2, 278, 0, b'*', b"", "0x1.d7e0000000000p-15"),
];

#[test]
fn ostream_matches_recorded_msvc_output() {
    for &(
        bits,
        field,
        showpos,
        showpoint,
        uppercase,
        adjust,
        precision,
        width,
        fill,
        grouping,
        text,
    ) in &MSVC_OSTREAM
    {
        let mut stream = Stream {
            float_field: [
                FloatField::Default,
                FloatField::Fixed,
                FloatField::Scientific,
                FloatField::Hex,
            ][usize::from(field)],
            adjust: [Adjust::Right, Adjust::Left, Adjust::Internal, Adjust::Right]
                [usize::from(adjust)],
            showpos,
            showpoint,
            uppercase,
            precision,
            width,
            fill,
            numpunct: if grouping.is_empty() {
                Numpunct::default()
            } else {
                Numpunct {
                    decimal_point: b',',
                    thousands_sep: b'.',
                    grouping: grouping.to_vec(),
                }
            },
        };
        let value = f64::from_bits(bits);
        assert_eq!(
            ostream::f64_to_string(&mut stream, value).unwrap(),
            text,
            "{bits:#018x}"
        );
        assert_eq!(stream.width, 0);
    }
}

// The black-box tests of the UCRT's strtod and strtof that the parser's UCRT rules come from
// (parse.rs, `Rules`, `ucrt_nan_sequence`): each finds the behaviour in the UCRT itself, says
// where it differs from the STL's from_chars, and holds orca-fmt to the UCRT on every case.

/// The UCRT's `strtod` of `text`: the value's bits, the bytes read and `errno`.
fn ucrt_strtod(text: &[u8]) -> (u64, usize, c_int) {
    let mut c_text = text.to_vec();
    c_text.push(0);
    let start = c_text.as_ptr().cast::<c_char>();
    let mut end: *mut c_char = std::ptr::null_mut();
    set_errno(0);
    // SAFETY: c_text is NUL-terminated; end receives a pointer into it.
    let d = unsafe { ucrt::strtod(start, &raw mut end) };
    (d.to_bits(), end as usize - start as usize, errno())
}

/// The UCRT's `strtof` of `text`, as [`ucrt_strtod`].
fn ucrt_strtof(text: &[u8]) -> (u64, usize, c_int) {
    let mut c_text = text.to_vec();
    c_text.push(0);
    let start = c_text.as_ptr().cast::<c_char>();
    let mut end: *mut c_char = std::ptr::null_mut();
    set_errno(0);
    // SAFETY: as in ucrt_strtod.
    let f = unsafe { ucrt::strtof(start, &raw mut end) };
    (
        u64::from(f.to_bits()),
        end as usize - start as usize,
        errno(),
    )
}

fn ours_strtod(text: &[u8]) -> (u64, usize, c_int) {
    let r = parse::strtod(text);
    (r.value.to_bits(), r.end, errno_of(r.errno))
}

fn ours_strtof(text: &[u8]) -> (u64, usize, c_int) {
    let r = parse::strtof(text);
    (u64::from(r.value.to_bits()), r.end, errno_of(r.errno))
}

/// `std::from_chars` of a double: the value's bits.
fn stl_from_chars(text: &[u8]) -> u64 {
    let mut v = 0f64;
    let r = parse::from_chars_float(text, &mut v, CharsFormat::General);
    assert_eq!(r.ptr, text.len(), "{}", String::from_utf8_lossy(text));
    v.to_bits()
}

/// The decimal digits of a little-endian big integer.
fn big_decimal(limbs: &[u32]) -> Vec<u8> {
    let mut v = limbs.to_vec();
    let mut chunks = Vec::new();
    while v.last() == Some(&0) {
        v.pop();
    }
    while !v.is_empty() {
        let mut rem = 0u64;
        for limb in v.iter_mut().rev() {
            let cur = (rem << 32) | u64::from(*limb);
            *limb = (cur / 1_000_000_000) as u32;
            rem = cur % 1_000_000_000;
        }
        chunks.push(rem as u32);
        while v.last() == Some(&0) {
            v.pop();
        }
    }
    let mut out = chunks.last().map_or_else(|| "0".to_owned(), u32::to_string);
    for chunk in chunks.iter().rev().skip(1) {
        out.push_str(&format!("{chunk:09}"));
    }
    out.into_bytes()
}

/// `n * 2^e` written exactly: its digits, without trailing zeros, and the power of ten they are
/// multiplied by.
fn exact_decimal(n: u64, e: i32) -> (Vec<u8>, i32) {
    let mut limbs = vec![n as u32, (n >> 32) as u32];
    let mut exp10 = 0;
    if e >= 0 {
        for _ in 0..e {
            let mut carry = 0u32;
            for limb in &mut limbs {
                let next = *limb >> 31;
                *limb = (*limb << 1) | carry;
                carry = next;
            }
            if carry > 0 {
                limbs.push(carry);
            }
        }
    } else {
        // n * 2^e = n * 5^-e * 10^e.
        for _ in 0..-e {
            let mut carry = 0u64;
            for limb in &mut limbs {
                let v = u64::from(*limb) * 5 + carry;
                *limb = v as u32;
                carry = v >> 32;
            }
            if carry > 0 {
                limbs.push(carry as u32);
            }
        }
        exp10 = e;
    }
    let mut digits = big_decimal(&limbs);
    while digits.len() > 1 && digits.last() == Some(&b'0') {
        digits.pop();
        exp10 += 1;
    }
    (digits, exp10)
}

/// `digits * 10^exp10` as text: `<digits>e<exp10>`.
fn decimal_text(digits: &[u8], exp10: i32) -> Vec<u8> {
    let mut t = digits.to_vec();
    t.extend_from_slice(format!("e{exp10}").as_bytes());
    t
}

/// The tie above the positive finite double or float of `bits`: `(digits, exp10)`, with the
/// number of decimal places it has.
fn tie_above(bits: u64, mantissa_bits: u32, exponent_bits: u32) -> (Vec<u8>, i32, i32) {
    let fraction_mask = (1u64 << (mantissa_bits - 1)) - 1;
    let biased = (bits >> (mantissa_bits - 1)) as i32;
    let bias = (1 << (exponent_bits - 1)) - 1;
    let (m, e) = if biased == 0 {
        (bits & fraction_mask, 1 - bias - (mantissa_bits as i32 - 1))
    } else {
        (
            (bits & fraction_mask) | (fraction_mask + 1),
            biased - bias - (mantissa_bits as i32 - 1),
        )
    };
    let (digits, exp10) = exact_decimal(2 * m + 1, e - 1);
    (digits, exp10, -exp10)
}

/// Exact ties between two doubles or two floats, read by `strtod` and `strtof`. The UCRT rounds a
/// tie to even, except when its fractional part has eight or nine decimal places, and is not too
/// small: then it rounds up. That is a fraction divided by a one-element denominator (10^8 or
/// 10^9) with a numerator of two or more elements, whose zero remainder the UCRT takes for one
/// that is not; `from_chars` rounds every tie to even.
#[test]
fn ucrt_rounds_some_ties_up() {
    install_handler();
    let mut rng = Rng(0x5EED_0009);
    let mut mismatches = Mismatches::default();
    let mut cases = 0u64;
    let mut up_not_even = [0u64; 2];
    for i in 0..240_000u64 {
        let double = i % 3 != 0;
        let bits = if double {
            // Binary exponents from 2^-10 to 2^60 above 1, where ties have fractional digits,
            // and every other one now and then.
            let biased = if i % 7 == 0 {
                1 + rng.below(2045)
            } else {
                1013 + rng.below(70)
            };
            (biased << 52) | (rng.next() & ((1 << 52) - 1))
        } else {
            let biased = if i % 7 == 0 {
                1 + rng.below(253)
            } else {
                117 + rng.below(40)
            };
            (biased << 23) | (rng.next() & ((1 << 23) - 1))
        };
        let (mantissa_bits, exponent_bits, last) = if double {
            (53, 11, 0x7FEF_FFFF_FFFF_FFFF)
        } else {
            (24, 8, 0x7F7F_FFFF)
        };
        if bits >= last {
            continue;
        }
        let (digits, exp10, places) = tie_above(bits, mantissa_bits, exponent_bits);
        let text = decimal_text(&digits, exp10);
        let (theirs, ours) = if double {
            (ucrt_strtod(&text), ours_strtod(&text))
        } else {
            (ucrt_strtof(&text), ours_strtof(&text))
        };
        cases += 1;
        if ours != theirs {
            mismatches.add(format!(
                "{} {:?}: orca-fmt {ours:x?} UCRT {theirs:x?}",
                if double { "strtod" } else { "strtof" },
                String::from_utf8_lossy(&text)
            ));
        }
        let even = if bits % 2 == 0 { bits } else { bits + 1 };
        if theirs.0 != even {
            assert_eq!(theirs.0, bits + 1, "a tie read as neither neighbour");
            assert!(
                places == 8 || places == 9,
                "the UCRT rounds up a tie with {places} decimal places: {}",
                String::from_utf8_lossy(&text)
            );
            up_not_even[usize::from(double)] += 1;
        }
        if double {
            assert_eq!(
                stl_from_chars(&text),
                even,
                "from_chars rounds ties to even"
            );
        }
    }
    mismatches.assert_none(cases, "ties");
    assert!(cases > 239_000, "{cases} ties");
    assert!(
        up_not_even[0] > 1000 && up_not_even[1] > 1000,
        "ties the UCRT rounds up: {up_not_even:?}"
    );

    // Two of the ties found this way, with eight and nine decimal places: up for strtod, even for
    // from_chars.
    for (text, even) in [
        (&b"39902590343405.22265625"[..], 0x42C2_2545_DC8C_769Cu64),
        (b"21362022564617.869140625", 0x42B3_6DBB_C443_09DE),
    ] {
        assert_eq!(ucrt_strtod(text).0, even + 1);
        assert_eq!(ours_strtod(text).0, even + 1);
        assert_eq!(stl_from_chars(text), even);
    }
}

/// A significant digit after the 768th, decimal or hexadecimal, is ignored by the UCRT: a tie with
/// a 1 as its 768th digit rounds up, with the 1 as its 769th it rounds as the tie (to even), and
/// leading zeros, before or after the point, are not counted. `from_chars` counts the 769th digit
/// as a tail that is not zero and rounds up.
#[test]
fn ucrt_ignores_digits_after_the_768th() {
    install_handler();
    let mut rng = Rng(0x5EED_000A);
    let mut mismatches = Mismatches::default();
    let mut cases = 0u64;
    let mut ties = 0u64;
    while ties < 300 {
        // An even double, so that its tie rounds down; whole numbers and fractions.
        let biased = if ties.is_multiple_of(2) {
            1076 + rng.below(200)
        } else {
            960 + rng.below(60)
        };
        let bits = ((biased << 52) | (rng.next() & ((1 << 52) - 1))) & !1;
        let (digits, exp10, _) = tie_above(bits, 53, 11);
        if digits.len() >= 700 {
            continue;
        }
        ties += 1;
        for (place, rounds_up) in [(768usize, true), (769, false), (900, false)] {
            for lead in [0usize, 1, 120] {
                // <lead zeros><tie digits><zeros>1 e<power>, the 1 the place-th significant digit.
                let mut text = vec![b'0'; lead];
                text.extend_from_slice(&digits);
                text.resize(lead + place - 1, b'0');
                text.push(b'1');
                let power = exp10 - (place - digits.len()) as i32;
                text.extend_from_slice(format!("e{power}").as_bytes());
                // The same with the point after the first digit, and as 0.<lead zeros><digits>.
                let significant = &text[lead..lead + place];
                let mut pointed = significant[..1].to_vec();
                pointed.push(b'.');
                pointed.extend_from_slice(&significant[1..]);
                pointed.extend_from_slice(format!("e{}", power + place as i32 - 1).as_bytes());
                let mut fraction = b"0.".to_vec();
                fraction.resize(2 + lead, b'0');
                fraction.extend_from_slice(significant);
                fraction
                    .extend_from_slice(format!("e{}", power + (place + lead) as i32).as_bytes());
                for t in [&text, &pointed, &fraction] {
                    let theirs = ucrt_strtod(t);
                    cases += 1;
                    if ours_strtod(t) != theirs {
                        mismatches.add(format!("strtod {:?}", String::from_utf8_lossy(t)));
                    }
                    assert_eq!(theirs.0, if rounds_up { bits + 1 } else { bits });
                    assert_eq!(stl_from_chars(t), bits + 1);
                }
            }
        }
    }

    // Hexadecimal: 1 + 2^-53 is a tie; a 1 as its 768th hexadecimal digit rounds it up.
    for (place, up) in [(700usize, true), (768, true), (769, false), (2000, false)] {
        let mut text = b"0x1.00000000000008".to_vec();
        text.resize(text.len() + place - 16, b'0');
        text.push(b'1');
        let theirs = ucrt_strtod(&text);
        cases += 1;
        if ours_strtod(&text) != theirs {
            mismatches.add(format!("strtod hex, 1 at {place}"));
        }
        assert_eq!(
            theirs.0,
            0x3FF0_0000_0000_0000 + u64::from(up),
            "hex 1 at {place}"
        );
    }
    mismatches.assert_none(cases, "digits");
    assert_eq!(cases, 300 * 3 * 3 * 3 + 4);
}

/// The UCRT takes an exponent above 5200 for an overflow and one below -5200 for an underflow
/// (`ERANGE`), before the digits' place is added: `0.<5195 zeros>1e5201` is infinite though it is
/// 1e5. At 5200 and -5200 the place is added. Hexadecimal binary exponents alike. `from_chars`
/// adds the place first.
#[test]
fn ucrt_exponent_limit() {
    install_handler();
    let mut mismatches = Mismatches::default();
    let mut cases = 0u64;
    for zeros in [0usize, 400, 5000, 5195, 5199, 5200, 5201, 9000] {
        for exponent in [5195i64, 5199, 5200, 5201, 5202, 9999] {
            let z = "0".repeat(zeros);
            let texts = [
                format!("0.{z}1e{exponent}"),
                format!("1{z}e-{exponent}"),
                format!("-1{z}e-{exponent}"),
                format!("0x0.{z}1p{exponent}"),
                format!("0x1{z}p-{exponent}"),
            ];
            for t in texts {
                let t = t.as_bytes();
                for (theirs, ours, name) in [
                    (ucrt_strtod(t), ours_strtod(t), "strtod"),
                    (ucrt_strtof(t), ours_strtof(t), "strtof"),
                ] {
                    cases += 1;
                    if ours != theirs {
                        mismatches.add(format!(
                            "{name} {:?}…: orca-fmt {ours:x?} UCRT {theirs:x?}",
                            String::from_utf8_lossy(&t[..t.len().min(20)])
                        ));
                    }
                }
            }
        }
    }
    mismatches.assert_none(cases, "exponents");
    assert_eq!(cases, 8 * 6 * 5 * 2);

    let z = "0".repeat(5195);
    let infinity = f64::INFINITY.to_bits();
    for (text, value, range_error, stl) in [
        (format!("0.{z}1e5201"), infinity, true, 1e5f64.to_bits()),
        (
            format!("0.{z}1e5200"),
            1e4f64.to_bits(),
            false,
            1e4f64.to_bits(),
        ),
        (format!("1{z}e-5201"), 0, true, 1e-6f64.to_bits()),
        (
            format!("1{z}e-5200"),
            1e-5f64.to_bits(),
            false,
            1e-5f64.to_bits(),
        ),
    ] {
        let theirs = ucrt_strtod(text.as_bytes());
        assert_eq!(
            (theirs.0, theirs.2 == ERANGE),
            (value, range_error),
            "{}…",
            &text[..12]
        );
        assert_eq!(stl_from_chars(text.as_bytes()), stl);
    }
    let z = "0".repeat(1300);
    assert_eq!(ucrt_strtod(format!("0x0.{z}1p5201").as_bytes()).0, infinity);
    assert_eq!(
        ucrt_strtod(format!("0x0.{z}1p5200").as_bytes()).0,
        0.0625f64.to_bits()
    );
}

/// The NaN spellings of the UCRT's `strtod` and `strtof`: `nan(` with a sequence of letters,
/// digits and `_` and a `)` is read whole, and anything else after `nan` is not. `nan` is the quiet
/// NaN with every payload bit set; `snan` in any case is the signalling NaN with payload 1, its
/// sign kept; `ind` alone or after `s`, `sn`, `sna` or `snan`, in any case, is the negative quiet
/// NaN without payload, even after a `+`. Every sequence of up to five of the characters below,
/// with and without a sign, is compared.
#[test]
fn ucrt_nan_spellings() {
    install_handler();
    const CHARACTERS: &[u8] = b"snaidSNID_x()";
    let mut mismatches = Mismatches::default();
    let mut cases = 0u64;
    let mut sequences: Vec<Vec<u8>> = vec![Vec::new()];
    let mut all = Vec::new();
    for _ in 0..5 {
        let mut longer = Vec::new();
        for s in &sequences {
            for &c in CHARACTERS {
                let mut t = s.clone();
                t.push(c);
                longer.push(t);
            }
        }
        all.extend(longer.iter().cloned());
        sequences = longer;
    }
    for sequence in &all {
        for sign in [&b""[..], b"-", b"+"] {
            let mut text = sign.to_vec();
            text.extend_from_slice(b"nan(");
            text.extend_from_slice(sequence);
            for (theirs, ours) in [
                (ucrt_strtod(&text), ours_strtod(&text)),
                (ucrt_strtof(&text), ours_strtof(&text)),
            ] {
                cases += 1;
                if ours != theirs {
                    mismatches.add(format!(
                        "{:?}: orca-fmt {ours:x?} UCRT {theirs:x?}",
                        String::from_utf8_lossy(&text)
                    ));
                }
            }
        }
    }
    mismatches.assert_none(cases, "nan");
    let n = CHARACTERS.len() as u64;
    assert_eq!(cases, (n + n * n + n.pow(3) + n.pow(4) + n.pow(5)) * 3 * 2);

    for (text, bits, end) in [
        (&b"nan"[..], 0x7FFF_FFFF_FFFF_FFFFu64, 3),
        (b"-nan", 0xFFFF_FFFF_FFFF_FFFF, 4),
        (b"nan(", 0x7FFF_FFFF_FFFF_FFFF, 3),
        (b"nan(ab c)", 0x7FFF_FFFF_FFFF_FFFF, 3),
        (b"nan(_x9)", 0x7FFF_FFFF_FFFF_FFFF, 8),
        (b"+nan(ind)", 0xFFF8_0000_0000_0000, 9),
        (b"nan(InD)", 0xFFF8_0000_0000_0000, 8),
        (b"nan(snaind)", 0xFFF8_0000_0000_0000, 11),
        (b"nan(snanind)", 0xFFF8_0000_0000_0000, 12),
        (b"nan(xind)", 0x7FFF_FFFF_FFFF_FFFF, 9),
        (b"nan(snan)", 0x7FF0_0000_0000_0001, 9),
        (b"-nan(SNAN)", 0xFFF0_0000_0000_0001, 10),
        (b"nan(snan)ind)", 0x7FF0_0000_0000_0001, 9),
    ] {
        assert_eq!(
            ucrt_strtod(text),
            (bits, end, 0),
            "{}",
            String::from_utf8_lossy(text)
        );
    }
}
