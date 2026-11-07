// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! orca-fmt's `stdio` and `scanf` against the static UCRT that this test program links, the C
//! library the C++ engine's `/MT` build links (o2-mesh, 2026-11-07). The `ucrt_` tests are the
//! black-box tests the two modules are written from: each finds a behaviour in the UCRT itself
//! (its `fopen`, `getc`, `fgets`, `fread`, `fseek`, `ftell` and its scanf family through
//! `__stdio_common_vfscanf` and `__stdio_common_vsscanf`, with the options a C++ program's startup
//! sets) and holds orca-fmt to it on every case. Among them, `ucrt_scanf_float_is_strtof`
//! establishes that `%f` stores what `strtof` gives for the same characters, and `%lf` what
//! `strtod` gives. Every case runs with a seeded generator, so a failure repeats; each test counts
//! its cases and asserts the count, so none passes by doing less.

use orca_fmt::parse;
use orca_fmt::scanf::{self, Overflow, ScanArg};
use orca_fmt::stdio::{self, EOF, File, Mode, SEEK_CUR, SEEK_END, SEEK_SET};
use std::ffi::{c_char, c_int, c_void};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// The UCRT functions the tests compare with.
mod ucrt {
    use super::{c_char, c_int, c_void};

    unsafe extern "C" {
        pub fn __local_stdio_scanf_options() -> *mut u64;
        pub fn __stdio_common_vsscanf(
            options: u64,
            buffer: *const c_char,
            count: usize,
            format: *const c_char,
            locale: *mut c_void,
            args: *const u64,
        ) -> c_int;
        pub fn __stdio_common_vfscanf(
            options: u64,
            stream: *mut c_void,
            format: *const c_char,
            locale: *mut c_void,
            args: *const u64,
        ) -> c_int;
        pub fn fopen(path: *const c_char, mode: *const c_char) -> *mut c_void;
        pub fn fclose(stream: *mut c_void) -> c_int;
        pub fn fgets(buf: *mut c_char, n: c_int, stream: *mut c_void) -> *mut c_char;
        pub fn getc(stream: *mut c_void) -> c_int;
        pub fn fread(buf: *mut c_void, size: usize, count: usize, stream: *mut c_void) -> usize;
        pub fn fseek(stream: *mut c_void, offset: i32, whence: c_int) -> c_int;
        pub fn ftell(stream: *mut c_void) -> i32;
        pub fn feof(stream: *mut c_void) -> c_int;
        pub fn rewind(stream: *mut c_void);
        pub fn strtof(s: *const c_char, end: *mut *mut c_char) -> f32;
        pub fn strtod(s: *const c_char, end: *mut *mut c_char) -> f64;
    }
}

/// The scanf options word of this program (`_CRT_INTERNAL_LOCAL_SCANF_OPTIONS`).
fn scanf_options() -> u64 {
    // SAFETY: returns the address of the CRT's option word.
    unsafe { *ucrt::__local_stdio_scanf_options() }
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

    /// One of the byte strings `items`.
    fn piece<'a>(&mut self, items: &[&'a [u8]]) -> &'a [u8] {
        items[self.below(items.len() as u64) as usize]
    }

    /// `n` decimal digits.
    fn digits(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| b'0' + self.below(10) as u8).collect()
    }
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

fn show(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    let s: String = s.chars().take(80).collect();
    format!("{s:?}")
}

/// A temporary file of this test program: one name per call site and thread, rewritten for each
/// case and removed by [`TempFile`]'s drop.
struct TempFile {
    path: PathBuf,
    c_path: Vec<u8>,
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

impl TempFile {
    fn new(tag: &str) -> TempFile {
        let dir = std::env::temp_dir().join(format!("orca-fmt-scanf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!("{tag}-{n}.txt"));
        let mut c_path = path.to_str().unwrap().as_bytes().to_vec();
        c_path.push(0);
        TempFile { path, c_path }
    }

    fn write(&self, bytes: &[u8]) {
        std::fs::write(&self.path, bytes).unwrap();
    }

    /// The UCRT's `fopen` of the file in `mode` (`"r"` or `"rb"`).
    fn open(&self, mode: Mode) -> *mut c_void {
        let m: &[u8] = match mode {
            Mode::Text => b"r\0",
            Mode::Binary => b"rb\0",
        };
        // SAFETY: both strings are NUL-terminated.
        let fp = unsafe { ucrt::fopen(self.c_path.as_ptr().cast(), m.as_ptr().cast()) };
        assert!(!fp.is_null(), "fopen {}", self.path.display());
        fp
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// The rest of a UCRT stream, read with `getc`.
fn ucrt_rest(fp: *mut c_void) -> Vec<u8> {
    let mut rest = Vec::new();
    loop {
        // SAFETY: fp is an open stream.
        let c = unsafe { ucrt::getc(fp) };
        if c < 0 {
            return rest;
        }
        rest.push(c as u8);
    }
}

fn our_rest(f: &mut File) -> Vec<u8> {
    let mut rest = Vec::new();
    loop {
        let c = f.getc();
        if c == EOF {
            return rest;
        }
        rest.push(c as u8);
    }
}

/// The kind of a scanf argument in a comparison: a `float`, a `double`, or a `char` buffer of the
/// given size.
#[derive(Clone, Copy, Debug)]
enum Kind {
    F32,
    F64,
    Chars(usize),
}

/// The bytes a test argument starts as, so that what a call leaves untouched is compared too.
const SENTINEL_F32: u32 = 0x7FC0_1234;
const SENTINEL_F64: u64 = 0x7FF8_0000_DEAD_BEEF;
const SENTINEL_BYTE: u8 = 0xA5;

/// What one call gave: its return value, every argument's bytes afterwards, and (for `fscanf`)
/// the rest of the stream.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Outcome {
    ret: i32,
    args: Vec<Vec<u8>>,
    rest: Vec<u8>,
}

/// Fresh argument storage for `kinds`.
fn fresh(kinds: &[Kind]) -> Vec<Vec<u8>> {
    kinds
        .iter()
        .map(|k| match k {
            Kind::F32 => SENTINEL_F32.to_le_bytes().to_vec(),
            Kind::F64 => SENTINEL_F64.to_le_bytes().to_vec(),
            Kind::Chars(n) => vec![SENTINEL_BYTE; *n],
        })
        .collect()
}

/// The UCRT's `fscanf` (on a file holding `input`, opened in `mode`) or `sscanf` (on `input` up to
/// its first NUL) of `format` with arguments of `kinds`.
fn ucrt_scan(
    input: &[u8],
    format: &[u8],
    kinds: &[Kind],
    file: Option<(&TempFile, Mode)>,
) -> Outcome {
    let mut storage = fresh(kinds);
    let mut slots = [0u64; 16];
    for (slot, s) in slots.iter_mut().zip(storage.iter_mut()) {
        *slot = s.as_mut_ptr() as u64;
    }
    let mut fmt = format.to_vec();
    fmt.push(0);
    let (ret, rest) = match file {
        Some((tmp, mode)) => {
            tmp.write(input);
            let fp = tmp.open(mode);
            // SAFETY: the slots point at storage of the right size for each conversion.
            let ret = unsafe {
                ucrt::__stdio_common_vfscanf(
                    scanf_options(),
                    fp,
                    fmt.as_ptr().cast(),
                    std::ptr::null_mut(),
                    slots.as_ptr(),
                )
            };
            let rest = ucrt_rest(fp);
            // SAFETY: fp was opened above.
            unsafe { ucrt::fclose(fp) };
            (ret, rest)
        }
        None => {
            let mut c_in = input.to_vec();
            c_in.push(0);
            // SAFETY: as above; the input is NUL-terminated.
            let ret = unsafe {
                ucrt::__stdio_common_vsscanf(
                    scanf_options(),
                    c_in.as_ptr().cast(),
                    usize::MAX,
                    fmt.as_ptr().cast(),
                    std::ptr::null_mut(),
                    slots.as_ptr(),
                )
            };
            (ret, Vec::new())
        }
    };
    Outcome {
        ret,
        args: storage,
        rest,
    }
}

/// orca-fmt's `fscanf` or `sscanf`, as [`ucrt_scan`].
fn our_scan(input: &[u8], format: &[u8], kinds: &[Kind], mode: Option<Mode>) -> Outcome {
    let mut storage = fresh(kinds);
    let mut floats: Vec<f32> = Vec::new();
    let mut doubles: Vec<f64> = Vec::new();
    for (k, s) in kinds.iter().zip(&storage) {
        match k {
            Kind::F32 => floats.push(f32::from_le_bytes(s[..4].try_into().unwrap())),
            Kind::F64 => doubles.push(f64::from_le_bytes(s[..8].try_into().unwrap())),
            Kind::Chars(_) => {}
        }
    }
    let (ret, rest) = {
        let mut fi = floats.iter_mut();
        let mut di = doubles.iter_mut();
        let mut args: Vec<ScanArg<'_>> = Vec::new();
        for (k, s) in kinds.iter().zip(storage.iter_mut()) {
            args.push(match k {
                Kind::F32 => ScanArg::F32(fi.next().unwrap()),
                Kind::F64 => ScanArg::F64(di.next().unwrap()),
                Kind::Chars(_) => ScanArg::Chars(s),
            });
        }
        match mode {
            Some(mode) => {
                let mut f = File::from_bytes(input, mode);
                let ret = scanf::fscanf(&mut f, format, &mut args).expect("no overflow");
                (ret, our_rest(&mut f))
            }
            None => (
                scanf::sscanf(input, format, &mut args).expect("no overflow"),
                Vec::new(),
            ),
        }
    };
    let mut fi = floats.iter();
    let mut di = doubles.iter();
    for (k, s) in kinds.iter().zip(storage.iter_mut()) {
        match k {
            Kind::F32 => *s = fi.next().unwrap().to_bits().to_le_bytes().to_vec(),
            Kind::F64 => *s = di.next().unwrap().to_bits().to_le_bytes().to_vec(),
            Kind::Chars(_) => {}
        }
    }
    Outcome {
        ret,
        args: storage,
        rest,
    }
}

/// Compares one input in the three ways: `fscanf` on a text stream and on a binary stream, and
/// `sscanf`. Returns the number of comparisons (3).
fn compare_scan(
    tmp: &TempFile,
    input: &[u8],
    format: &[u8],
    kinds: &[Kind],
    mismatches: &mut Mismatches,
) -> u64 {
    for mode in [Mode::Text, Mode::Binary] {
        let theirs = ucrt_scan(input, format, kinds, Some((tmp, mode)));
        let ours = our_scan(input, format, kinds, Some(mode));
        if ours != theirs {
            mismatches.add(format!(
                "fscanf {mode:?} {} on {}: orca-fmt {ours:?} UCRT {theirs:?}",
                show(format),
                show(input)
            ));
        }
    }
    let theirs = ucrt_scan(input, format, kinds, None);
    let ours = our_scan(input, format, kinds, None);
    if ours != theirs {
        mismatches.add(format!(
            "sscanf {} on {}: orca-fmt {ours:?} UCRT {theirs:?}",
            show(format),
            show(input)
        ));
    }
    3
}

/// The `%f` grammar: every prefix of each spelling, followed by the end, a letter, a space or a
/// newline. What the UCRT reads, where it stops, what it stores and what it returns: an input
/// that ends inside a number is an input failure (`EOF`), one that goes on with a character that
/// cannot continue it a matching failure (0, the character left unread), and the characters read
/// before are lost either way.
#[test]
fn ucrt_float_grammar() {
    let tmp = TempFile::new("grammar");
    let spellings: &[&str] = &[
        "infinity",
        "INFINITY",
        "InFiNiTy",
        "-inf",
        "+infinity",
        "infix",
        "infinx",
        "nan",
        "NaN",
        "nan()",
        "nan(a)",
        "nan(a b)",
        "nan(_9Z)",
        "nan(ind)",
        "-nan(ind)",
        "nan(snan)",
        "nan(((",
        "nan(a(",
        "nan)",
        "nanq",
        "1",
        "1.",
        ".1",
        "1e5",
        "1e+5",
        "1e-5",
        "1E5",
        "1..5",
        "1.5.5",
        "1e5e5",
        "1.e5",
        "-.5e-1",
        "+.e1",
        "--1",
        "+-1",
        "-e",
        ".e1",
        "e1",
        "0e",
        "00x1",
        "0x",
        "0X1",
        "0x1.",
        "0x.8",
        "0x1p+1",
        "0x1P-2",
        "0x1e",
        "0xp1",
        "0x.p1",
        "0x1.p1",
        "0xg",
        "0x1.8p1x",
        "1.#INF",
        "1.#QNAN",
        "123456789012345678901234567890",
        "1.0000000596046447753906250000000001",
        "3.4028235e38",
        "3.4028236e38",
        "1e39",
        "1e-46",
        "1.4e-45",
        "-0",
        "-0.0e0",
    ];
    let mut mismatches = Mismatches::default();
    let mut cases = 0u64;
    for spelling in spellings {
        let s = spelling.as_bytes();
        for len in 1..=s.len() {
            for suffix in [&b""[..], b"x", b" ", b"\n", b"\t5"] {
                let mut input = s[..len].to_vec();
                input.extend_from_slice(suffix);
                for leading in [&b""[..], b"  \n\t"] {
                    let mut full = leading.to_vec();
                    full.extend_from_slice(&input);
                    cases += compare_scan(&tmp, &full, b"%f", &[Kind::F32], &mut mismatches);
                }
            }
        }
    }
    mismatches.assert_none(cases, "float grammar");
    assert_eq!(cases, 11_250);

    // What the module documentation says, found in the UCRT.
    let f = |input: &[u8]| ucrt_scan(input, b"%f", &[Kind::F32], Some((&tmp, Mode::Binary)));
    let sentinel = SENTINEL_F32.to_le_bytes().to_vec();
    for (input, ret, rest) in [
        (&b"1e+"[..], EOF, &b""[..]),
        (b"1e+x", 0, b"x"),
        (b"-", EOF, b""),
        (b"- 1", 0, b" 1"),
        (b"0x", EOF, b""),
        (b"0xg", 0, b"g"),
        (b"infin", EOF, b""),
        (b"infix", 0, b"x"),
        (b"nan(ab", EOF, b""),
        (b"nan(a b)", 0, b" b)"),
        (b"", EOF, b""),
        (b" \n ", EOF, b""),
    ] {
        let got = f(input);
        assert_eq!(
            (got.ret, got.rest.as_slice()),
            (ret, rest),
            "{}",
            show(input)
        );
        assert_eq!(got.args[0], sentinel, "a failed %f stores nothing");
    }
    for (input, bits, rest) in [
        (&b"inf"[..], 0x7F80_0000u32, &b""[..]),
        (b"infx", 0x7F80_0000, b"x"),
        (b"nan", 0x7FFF_FFFF, b""),
        (b"nan)", 0x7FFF_FFFF, b")"),
        (b"-nan(ind)", 0xFFC0_0000, b""),
        (b"nan(snan)", 0x7F80_0001, b""),
        (b"1..5", 0x3F80_0000, b".5"),
        (b"00x1", 0, b"x1"),
        (b"0x1e", 0x41F0_0000, b""),
        (b"1.#INF", 0x3F80_0000, b"#INF"),
    ] {
        let got = f(input);
        assert_eq!(
            (got.ret, got.args[0].clone(), got.rest.as_slice()),
            (1, bits.to_le_bytes().to_vec(), rest),
            "{}",
            show(input)
        );
    }
}

/// `n * 2^e` written exactly: its digits, without trailing zeros, and the power of ten they are
/// multiplied by (as in `tests/fmt.rs`).
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
    let mut v = limbs;
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
    let mut digits = chunks.last().map_or_else(|| "0".to_owned(), u32::to_string);
    for chunk in chunks.iter().rev().skip(1) {
        digits.push_str(&format!("{chunk:09}"));
    }
    let mut digits = digits.into_bytes();
    while digits.len() > 1 && digits.last() == Some(&b'0') {
        digits.pop();
        exp10 += 1;
    }
    (digits, exp10)
}

/// The tie above the positive finite float or double of `bits`: its digits and power of ten.
fn tie_above(bits: u64, mantissa_bits: u32, exponent_bits: u32) -> (Vec<u8>, i32) {
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
    exact_decimal(2 * m + 1, e - 1)
}

/// `digits * 10^exp10` written with a point: `d.ddd` and an exponent, or positionally.
fn decimal_forms(digits: &[u8], exp10: i32) -> [Vec<u8>; 2] {
    let mut sci = digits[..1].to_vec();
    if digits.len() > 1 {
        sci.push(b'.');
        sci.extend_from_slice(&digits[1..]);
    }
    sci.extend_from_slice(format!("e{}", exp10 + digits.len() as i32 - 1).as_bytes());
    let positional = if exp10 >= 0 {
        let mut p = digits.to_vec();
        p.resize(digits.len() + exp10 as usize, b'0');
        p
    } else {
        let places = (-exp10) as usize;
        if places >= digits.len() {
            let mut p = b"0.".to_vec();
            p.resize(2 + places - digits.len(), b'0');
            p.extend_from_slice(digits);
            p
        } else {
            let mut p = digits[..digits.len() - places].to_vec();
            p.push(b'.');
            p.extend_from_slice(&digits[digits.len() - places..]);
            p
        }
    };
    [sci, positional]
}

/// The UCRT's `sscanf(text, "%f%n")` (or `"%lf%n"`): its return value, the stored value's bits
/// and the characters read.
fn ucrt_scan_number(text: &[u8], double: bool) -> (c_int, u64, i32) {
    let mut c_text = text.to_vec();
    c_text.push(0);
    let mut f = f32::from_bits(SENTINEL_F32);
    let mut d = f64::from_bits(SENTINEL_F64);
    let mut n: i32 = -1;
    let fmt: &[u8] = if double { b"%lf%n\0" } else { b"%f%n\0" };
    let target = if double {
        (&raw mut d) as u64
    } else {
        (&raw mut f) as u64
    };
    let slots = [target, (&raw mut n) as u64, 0, 0];
    // SAFETY: the slots point at a float or double and an int; both strings are NUL-terminated.
    let ret = unsafe {
        ucrt::__stdio_common_vsscanf(
            scanf_options(),
            c_text.as_ptr().cast(),
            usize::MAX,
            fmt.as_ptr().cast(),
            std::ptr::null_mut(),
            slots.as_ptr(),
        )
    };
    let bits = if double {
        d.to_bits()
    } else {
        u64::from(f.to_bits())
    };
    (ret, bits, n)
}

/// The UCRT's `strtof` or `strtod` of `text`: the value's bits and the characters read.
fn ucrt_strto(text: &[u8], double: bool) -> (u64, usize) {
    let mut c_text = text.to_vec();
    c_text.push(0);
    let start = c_text.as_ptr().cast::<c_char>();
    let mut end: *mut c_char = std::ptr::null_mut();
    // SAFETY: c_text is NUL-terminated; end receives a pointer into it.
    let bits = unsafe {
        if double {
            ucrt::strtod(start, &raw mut end).to_bits()
        } else {
            u64::from(ucrt::strtof(start, &raw mut end).to_bits())
        }
    };
    (bits, end as usize - start as usize)
}

/// The numbers `ucrt_scanf_float_is_strtof` reads: `count` of each family.
fn number_texts(rng: &mut Rng, double: bool, count: usize) -> Vec<Vec<u8>> {
    let mut texts: Vec<Vec<u8>> = Vec::new();
    // Plain decimals: signs, points, exponents, long digit strings.
    for _ in 0..count {
        let mut t = rng.piece(&[b"", b"", b"-", b"+"]).to_vec();
        let int_digits = rng.below(30) as usize;
        let frac_digits = rng.below(30) as usize;
        let int_digits = if int_digits + frac_digits == 0 {
            1
        } else {
            int_digits
        };
        t.extend(rng.digits(int_digits));
        if frac_digits > 0 || rng.below(4) == 0 {
            t.push(b'.');
            t.extend(rng.digits(frac_digits));
        }
        if rng.below(2) == 0 {
            t.push(*rng.piece(&[b"e", b"E"]).first().unwrap());
            t.extend_from_slice(rng.piece(&[b"", b"+", b"-"]));
            let e = rng.below(if double { 330 } else { 50 });
            t.extend_from_slice(e.to_string().as_bytes());
        }
        texts.push(t);
    }
    // Exact ties between two neighbours (the UCRT rounds some up, `tests/fmt.rs`).
    let (mantissa_bits, exponent_bits, max_biased) =
        if double { (53, 11, 2046) } else { (24, 8, 254) };
    for i in 0..count {
        let biased = if i % 5 == 0 {
            rng.below(max_biased)
        } else if double {
            1013 + rng.below(70)
        } else {
            117 + rng.below(40)
        };
        let bits =
            (biased << (mantissa_bits - 1)) | (rng.next() & ((1 << (mantissa_bits - 1)) - 1));
        let (digits, exp10) = tie_above(bits, mantissa_bits, exponent_bits);
        let [sci, positional] = decimal_forms(&digits, exp10);
        texts.push(sci);
        if positional.len() < 400 {
            texts.push(positional);
        }
    }
    // Digits past the 768th, which the UCRT ignores: a tie with a 1 as its 768th, 769th or 900th
    // significant digit.
    for _ in 0..count / 50 + 1 {
        let biased = if double {
            1076 + rng.below(200)
        } else {
            150 + rng.below(40)
        };
        let bits = ((biased << (mantissa_bits - 1))
            | (rng.next() & ((1 << (mantissa_bits - 1)) - 1)))
            & !1;
        let (digits, exp10) = tie_above(bits, mantissa_bits, exponent_bits);
        if digits.len() >= 700 {
            continue;
        }
        for place in [768usize, 769, 900] {
            let mut t = digits.clone();
            t.resize(place - 1, b'0');
            t.push(b'1');
            let power = exp10 - (place - digits.len()) as i32;
            t.extend_from_slice(format!("e{power}").as_bytes());
            texts.push(t);
        }
    }
    // Hexadecimal.
    for _ in 0..count {
        let mut t = rng.piece(&[b"", b"-", b"+"]).to_vec();
        t.extend_from_slice(rng.piece(&[b"0x", b"0X"]));
        let hex = b"0123456789abcdefABCDEF";
        let a = rng.below(12) as usize;
        let b = rng.below(12) as usize;
        let a = if a + b == 0 { 1 } else { a };
        for _ in 0..a {
            t.push(hex[rng.below(hex.len() as u64) as usize]);
        }
        if b > 0 || rng.below(3) == 0 {
            t.push(b'.');
            for _ in 0..b {
                t.push(hex[rng.below(hex.len() as u64) as usize]);
            }
        }
        if rng.below(3) != 0 {
            t.push(*rng.piece(&[b"p", b"P"]).first().unwrap());
            t.extend_from_slice(rng.piece(&[b"", b"+", b"-"]));
            t.extend_from_slice(rng.below(1100).to_string().as_bytes());
        }
        texts.push(t);
    }
    // Infinities and NaNs in every spelling scanf reads whole.
    for _ in 0..count / 4 + 1 {
        let mut t = rng.piece(&[b"", b"-", b"+"]).to_vec();
        match rng.below(3) {
            0 => t.extend_from_slice(rng.piece(&[
                b"inf",
                b"INF",
                b"Inf",
                b"infinity",
                b"INFINITY",
                b"iNfInItY",
            ])),
            1 => t.extend_from_slice(rng.piece(&[b"nan", b"NAN", b"nAn"])),
            _ => {
                t.extend_from_slice(b"nan(");
                let inner: &[u8] = b"snaidSNID_x09";
                let pieces: &[&[u8]] = &[b"ind", b"IND", b"snan", b"SNAN", b"sind", b""];
                if rng.below(2) == 0 {
                    t.extend_from_slice(rng.piece(pieces));
                }
                for _ in 0..rng.below(5) {
                    t.push(inner[rng.below(inner.len() as u64) as usize]);
                }
                t.push(b')');
            }
        }
        texts.push(t);
    }
    // Overflow, underflow and the smallest numbers.
    for t in [
        &b"3.4028234664e38"[..],
        b"3.4028235e38",
        b"3.40282356779733661637539395458142568448e38",
        b"3.40282357e38",
        b"3.4028236e38",
        b"1e39",
        b"-1e39",
        b"1e-45",
        b"1.4e-45",
        b"7.006e-46",
        b"7.0064923216240854e-46",
        b"7.0064923216240862e-46",
        b"1e-46",
        b"1.17549435e-38",
        b"1.1754942e-38",
        b"1.7976931348623157e308",
        b"1.7976931348623158e308",
        b"1e309",
        b"4.9406564584124654e-324",
        b"2.4703282292062327e-324",
        b"2.4703282292062328e-324",
        b"1e-400",
        b"0x1.fffffep127",
        b"0x1.ffffffp127",
        b"0x1p-149",
        b"0x1p-150",
        b"0x1.000001p-150",
        b"0x1p1024",
        b"0x1p-1075",
    ] {
        texts.push(t.to_vec());
    }
    // The UCRT's exponent limit (5200, before the digits' place is added).
    let z = "0".repeat(5195);
    for t in [
        format!("0.{z}1e5201"),
        format!("0.{z}1e5200"),
        format!("1{z}e-5201"),
        format!("1{z}e-5200"),
    ] {
        texts.push(t.into_bytes());
    }
    texts
}

/// `%f` stores `strtof` of the characters it reads, and `%lf` `strtod` of them, bit for bit: the
/// UCRT's ties rounded up, its 768-digit limit, its exponent limit, its NaN payloads, overflow to
/// infinity and underflow to zero alike. `%f` is not `strtod` rounded to `float`: 1 + 2^-24 +
/// 10^-34 is 1 + 2^-23 for both `%f` and `strtof`, where a double rounding gives 1.
#[test]
fn ucrt_scanf_float_is_strtof() {
    let mut rng = Rng(0x5EED_0B01);
    let mut mismatches = Mismatches::default();
    let mut cases = 0u64;
    for double in [false, true] {
        let texts = number_texts(&mut rng, double, if double { 2_000 } else { 6_000 });
        for text in &texts {
            let (ret, bits, n) = ucrt_scan_number(text, double);
            let (strto_bits, strto_end) = ucrt_strto(text, double);
            cases += 1;
            // The finding: scanf reads the whole number and stores strto*'s value.
            assert_eq!(
                (ret, n as usize, bits),
                (1, text.len(), strto_bits),
                "{} {}: scanf against strto",
                if double { "%lf" } else { "%f" },
                show(text)
            );
            assert_eq!(strto_end, text.len(), "strto reads {} whole", show(text));
            // orca-fmt agrees with both.
            let ours = if double {
                let mut d = f64::from_bits(SENTINEL_F64);
                let r = scanf::sscanf(text, b"%lf", &mut [ScanArg::F64(&mut d)]).unwrap();
                (r, d.to_bits(), parse::strtod(text).value.to_bits())
            } else {
                let mut f = f32::from_bits(SENTINEL_F32);
                let r = scanf::sscanf(text, b"%f", &mut [ScanArg::F32(&mut f)]).unwrap();
                (
                    r,
                    u64::from(f.to_bits()),
                    u64::from(parse::strtof(text).value.to_bits()),
                )
            };
            if ours != (1, bits, bits) {
                mismatches.add(format!("{}: orca-fmt {ours:x?} UCRT {bits:x}", show(text)));
            }
        }
    }
    mismatches.assert_none(cases, "%f and strtof");
    assert_eq!(cases, 34_410, "numbers");

    // No double rounding: 1 + 2^-24 + 10^-34.
    let text = b"1.0000000596046447753906250000000001";
    assert_eq!(ucrt_scan_number(text, false).1, 0x3F80_0001);
    assert_eq!(f64::from_bits(ucrt_scan_number(text, true).1) as f32, 1.0);
}

/// One row of `ucrt_directives`: a format, its arguments and the inputs it is run on.
type DirectiveCase<'a> = (&'a [u8], &'a [Kind], &'a [&'a [u8]]);

/// White space, literals, `%s` with a width, `%[^\n]` and `%*[^\n]`, and the return counts, on
/// the formats of admesh's ASCII STL reader and around them: on a text stream, on a binary stream
/// and in `sscanf`, the return value, every argument's bytes and the rest of the stream.
/// Found: white space at the end of the input is no failure (`" "` returns 0 there), while a
/// literal or a conversion that meets the end returns `EOF` if nothing was stored; `*`
/// conversions count for neither.
#[test]
fn ucrt_directives() {
    use Kind::{Chars, F32, F64};
    let tmp = TempFile::new("directives");
    let cases: &[DirectiveCase<'_>] = &[
        (
            b" outer loop",
            &[],
            &[
                b"",
                b"   ",
                b"outer loop",
                b"outer lo",
                b"outer  loop",
                b"outerloop",
                b"outer\nloop",
                b"outer loopx",
                b"outex",
                b"\n\n  outer loop\n",
                b"o",
                b"outer",
                b"\r\nouter loop\r\n",
                b"outer\r\nloop",
                b"OUTER LOOP",
                b"outer \x1a loop",
            ],
        ),
        (
            b" endsolid%*[^\n]\n",
            &[],
            &[
                b"endsolid name\nfacet",
                b"endsolid\nfacet",
                b"endsolid",
                b"endsolid name",
                b"endso",
                b"facet",
                b"",
                b"  \n endsolid x\n\n  facet",
                b"endsolidx\n  \n  y",
                b"endsolid x",
                b"endsolid x\r\n  facet",
                b"endsolid\r\nfacet",
                b"endsolid \x00x\n",
            ],
        ),
        (
            b" solid%*[^\n]\n",
            &[],
            &[
                b"solid\n  facet",
                b"solid name\n\n  facet",
                b"solidfoo",
                b"sol",
                b"  facet",
                b"solid \r\r\n x",
            ],
        ),
        (
            b" solid %[^\n]",
            &[Chars(1024)],
            &[
                b"solid name\n",
                b"solid\nname\n",
                b"solid",
                b"solid ",
                b"solidx\n",
                b"solid \n\n\n",
                b"solid a b c \r\n",
                b"  solid  name with spaces  \nfacet",
                b"solid\n\n",
                b"solid a\rb\rc\r",
                b"solid MW 1.0 abc US\n",
                b"solid n\x00ame\n",
            ],
        ),
        (
            b" facet normal %31s %31s %31s",
            &[Chars(32), Chars(32), Chars(32)],
            &[
                b"facet normal 1 2 3\n",
                b"facet normal 0123456789012345678901234567890123456789 2 3",
                b"facet normal 1",
                b"facet normal",
                b"facet normals 1 2 3",
                b"facet normal 1 2",
                b"facet  normal\t1\n2\n3",
                b"facet normal -nan(ind) 1.#QNAN inf\r\n",
                b"facet normal 1\x0b2\x0c3",
            ],
        ),
        (
            b" vertex %f %f %f",
            &[F32, F32, F32],
            &[
                b"vertex 1 2 3\n",
                b"vertex 1 2",
                b"vertex 1 2 x",
                b"vertex 1,2,3",
                b"vertex",
            ],
        ),
        (
            b" vertex %f %f %f ",
            &[F32, F32, F32],
            &[
                b"vertex 1 2 3\n  endloop",
                b"vertex 1 2",
                b"vertex 1 2 x",
                b"vertex 1e5 -2.5E-3 +3.\n",
                b"vertex 1,2,3",
                b"vertex 1 2 3",
                b"vertex 1 2 3e",
                b"vertex 1 2e 3",
                b"vertex",
                b"vertex x",
                b"vertex 1 2 3\r\n\r\n endloop\r\n",
                b"vertex 1 2 3\x1a endloop",
            ],
        ),
        (
            b" ",
            &[],
            &[b"", b"  ", b" x", b"\n\nendfacet\n", b"\r\n\r\nendfacet"],
        ),
        (b"%*s x", &[], &[b"abc", b"abc y", b"abc x", b""]),
        (b"%*s%f", &[F32], &[b"abc", b"abc 1", b"abc x"]),
        (b"%*[^\n]x", &[], &[b"abc", b"abc\n", b"abcx", b"\n"]),
        (b"x%*s", &[], &[b"x", b"xa", b"y", b""]),
        (
            b"%f %f",
            &[F32, F32],
            &[b"2 1e", b"2 1e+x", b"2 ", b"2 x", b"2", b"2 3"],
        ),
        (
            b"%lf %lf",
            &[F64, F64],
            &[b"2 1e", b"0.1 1e400", b"2", b"-0 nan"],
        ),
        (
            b"%s %s %s",
            &[Chars(64), Chars(64), Chars(64)],
            &[b"1.0 abc US", b"1.0 abc", b"", b"  ", b"a\tb\nc"],
        ),
        (
            b"%3s%2s",
            &[Chars(8), Chars(8)],
            &[b"abcdef", b"ab cd", b"a", b""],
        ),
        (
            b"%[^\n]%[\n]",
            &[Chars(64), Chars(64)],
            &[b"ab\n\ncd", b"\nab", b"ab"],
        ),
        (b"%[]ab]", &[Chars(64)], &[b"]]ab]c", b"cab"]),
    ];
    let mut mismatches = Mismatches::default();
    let mut count = 0u64;
    for (format, kinds, inputs) in cases {
        for input in inputs.iter() {
            count += compare_scan(&tmp, input, format, kinds, &mut mismatches);
        }
    }
    mismatches.assert_none(count, "directives");
    assert_eq!(count, 3 * 117);

    // What the module documentation says, found in the UCRT.
    let text = |input: &[u8], format: &[u8], kinds: &[Kind]| {
        ucrt_scan(input, format, kinds, Some((&tmp, Mode::Text)))
    };
    assert_eq!(text(b"   ", b" ", &[]).ret, 0);
    assert_eq!(text(b"   ", b" outer loop", &[]).ret, EOF);
    assert_eq!(text(b"outer lo", b" outer loop", &[]).ret, EOF);
    let outex = text(b"outex", b" outer loop", &[]);
    assert_eq!((outex.ret, outex.rest.as_slice()), (0, &b"x"[..]));
    assert_eq!(text(b"abc", b"%*s x", &[]).ret, EOF);
    let skip = text(b"endsolidx\n  \n  y", b" endsolid%*[^\n]\n", &[]);
    assert_eq!((skip.ret, skip.rest.as_slice()), (0, &b"y"[..]));
    let solid = text(b"solid\nname\n", b" solid %[^\n]", &[Chars(16)]);
    assert_eq!((solid.ret, &solid.args[0][..5]), (1, &b"name\0"[..]));
    let three = text(b"vertex 1 2 3e", b" vertex %f %f %f ", &[F32, F32, F32]);
    assert_eq!(three.ret, 2);
}

/// The admesh formats on generated text: tokens of an ASCII STL, numbers, and every kind of line
/// end and white space, Ctrl-Z and NUL among them.
#[test]
fn ucrt_directives_generated() {
    use Kind::{Chars, F32};
    let tmp = TempFile::new("generated");
    let formats: &[(&[u8], &[Kind])] = &[
        (b" endsolid%*[^\n]\n", &[]),
        (b" solid%*[^\n]\n", &[]),
        (b" solid %[^\n]", &[Chars(4096)]),
        (
            b" facet normal %31s %31s %31s",
            &[Chars(32), Chars(32), Chars(32)],
        ),
        (b" outer loop", &[]),
        (b" vertex %f %f %f", &[F32, F32, F32]),
        (b" vertex %f %f %f ", &[F32, F32, F32]),
        (b" ", &[]),
        (b"%f", &[F32]),
        (b"%s %s %s", &[Chars(4096), Chars(4096), Chars(4096)]),
    ];
    let pieces: &[&[u8]] = &[
        b"solid",
        b"endsolid",
        b"facet",
        b"normal",
        b"outer",
        b"loop",
        b"vertex",
        b"endloop",
        b"endfacet",
        b" ",
        b"  ",
        b"\t",
        b"\n",
        b"\r",
        b"\r\n",
        b"\r\r\n",
        b"\x0b",
        b"\x0c",
        b"\x1a",
        b"\x00",
        b"1",
        b"-2.5",
        b"1e5",
        b"3.25E-2",
        b"+7.",
        b".5",
        b"1e",
        b"1e+",
        b"0x1p3",
        b"nan",
        b"-nan(ind)",
        b"inf",
        b"infi",
        b"name",
        b"MW",
        b"x",
        b"#",
        b"0123456789012345678901234567890123",
    ];
    let mut rng = Rng(0x5EED_0B02);
    let mut mismatches = Mismatches::default();
    let mut count = 0u64;
    for (format, kinds) in formats {
        for _ in 0..150 {
            let mut input = Vec::new();
            for _ in 0..rng.below(12) {
                input.extend_from_slice(rng.piece(pieces));
                if rng.below(2) == 0 {
                    input.push(b' ');
                }
            }
            count += compare_scan(&tmp, &input, format, kinds, &mut mismatches);
        }
    }
    mismatches.assert_none(count, "generated");
    assert_eq!(count, 3 * 150 * 10);
}

/// A text stream's bytes: CR LF reads as LF, any other CR as itself, Ctrl-Z ends the stream; a CR
/// LF across the UCRT's 4096-byte pieces reads as LF too. A binary stream reads the bytes as they
/// are. Compared through `getc` (with the end-of-file indicator), `fgets` and `fread`.
#[test]
fn ucrt_text_mode() {
    let tmp = TempFile::new("text");
    let mut inputs: Vec<Vec<u8>> = [
        &b"a\r\nb"[..],
        b"a\rb",
        b"a\r\r\nb",
        b"a\n\rb",
        b"a\x1ab",
        b"a\r\x1ab",
        b"a\r",
        b"\r\n",
        b"a\x1a",
        b"\x1a",
        b"",
        b"\r",
        b"\n\r\n\r",
        b"no final newline",
    ]
    .iter()
    .map(|s| s.to_vec())
    .collect();
    for at in [4093usize, 4094, 4095, 4096, 4097, 8191, 8192, 8193] {
        for tail in [&b"\r\nb\r"[..], b"\rb", b"\r\r\n", b"\x1ab", b"\r\x1a"] {
            let mut v = vec![b'a'; at];
            v.extend_from_slice(tail);
            inputs.push(v);
        }
    }
    let mut rng = Rng(0x5EED_0B03);
    for _ in 0..40 {
        let len = rng.below(10_000) as usize;
        let mut v = Vec::with_capacity(len);
        for _ in 0..len {
            let c = rng.piece(&[b"a", b"b", b"\r", b"\n", b" ", b"\x1a", b"\x00", b"\xff"])[0];
            // Ctrl-Z ends a text stream, so only now and then.
            v.push(if c == 0x1A && rng.below(20) != 0 {
                b'c'
            } else {
                c
            });
        }
        inputs.push(v);
    }
    let mut mismatches = Mismatches::default();
    let mut count = 0u64;
    for input in &inputs {
        tmp.write(input);
        for mode in [Mode::Text, Mode::Binary] {
            // getc to the end and three times past it, with feof after each.
            let fp = tmp.open(mode);
            let mut theirs = Vec::new();
            for _ in 0..input.len() + 3 {
                // SAFETY: fp is open.
                unsafe { theirs.push((ucrt::getc(fp), ucrt::feof(fp) != 0)) };
            }
            // SAFETY: fp is open.
            unsafe { ucrt::fclose(fp) };
            let mut f = File::from_bytes(input, mode);
            let ours: Vec<(i32, bool)> =
                (0..input.len() + 3).map(|_| (f.getc(), f.feof())).collect();
            count += 1;
            if ours != theirs {
                mismatches.add(format!("getc {mode:?} {}", show(input)));
            }
            // fgets of 100 and of 7 bytes, as the STL reader counts lines.
            for n in [100usize, 7] {
                let fp = tmp.open(mode);
                let mut f = File::from_bytes(input, mode);
                let mut theirs_buf = vec![SENTINEL_BYTE; n];
                let mut ours_buf = vec![SENTINEL_BYTE; n];
                loop {
                    // SAFETY: the buffer holds n bytes.
                    let got = unsafe {
                        !ucrt::fgets(theirs_buf.as_mut_ptr().cast(), n as c_int, fp).is_null()
                    };
                    let ours_got = f.fgets(&mut ours_buf, n);
                    count += 1;
                    if got != ours_got || theirs_buf != ours_buf {
                        mismatches.add(format!("fgets {n} {mode:?} {}", show(input)));
                        break;
                    }
                    if !got {
                        break;
                    }
                }
                // SAFETY: fp is open.
                unsafe { ucrt::fclose(fp) };
            }
            // fread in pieces of 1000 bytes.
            let fp = tmp.open(mode);
            let mut f = File::from_bytes(input, mode);
            loop {
                let mut theirs_buf = vec![0u8; 1000];
                let mut ours_buf = vec![0u8; 1000];
                // SAFETY: the buffer holds 1000 bytes.
                let got = unsafe { ucrt::fread(theirs_buf.as_mut_ptr().cast(), 1, 1000, fp) };
                let ours_got = f.fread(&mut ours_buf, 1, 1000);
                count += 1;
                if got != ours_got || theirs_buf != ours_buf {
                    mismatches.add(format!("fread {mode:?} {}", show(input)));
                    break;
                }
                if got == 0 {
                    break;
                }
            }
            // SAFETY: fp is open.
            unsafe { ucrt::fclose(fp) };
        }
    }
    mismatches.assert_none(count, "text mode");
    assert!(count > 1_000, "{count} comparisons");

    // The rule, found in the UCRT.
    for (raw, read) in [
        (&b"a\r\nb"[..], &b"a\nb"[..]),
        (b"a\rb", b"a\rb"),
        (b"a\r\r\nb", b"a\r\nb"),
        (b"a\r", b"a\r"),
        (b"a\r\x1ab", b"a\r"),
        (b"a\x1ab", b"a"),
    ] {
        tmp.write(raw);
        let fp = tmp.open(Mode::Text);
        assert_eq!(ucrt_rest(fp), read, "{}", show(raw));
        // SAFETY: fp is open.
        unsafe { ucrt::fclose(fp) };
        assert_eq!(stdio::text_translation(raw), read);
    }
}

/// A binary stream's `fseek`, `ftell`, `rewind` and `fread` counts, as the STL reader sizes a file
/// and reads its header and facets: positions past the end, short reads of whole items, the
/// end-of-file indicator and what clears it.
#[test]
fn ucrt_binary_stream() {
    let tmp = TempFile::new("binary");
    let mut rng = Rng(0x5EED_0B04);
    let mut mismatches = Mismatches::default();
    let mut count = 0u64;
    for len in [0usize, 1, 50, 83, 84, 211, 212, 283, 284, 334, 1000] {
        let input: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        tmp.write(&input);
        let fp = tmp.open(Mode::Binary);
        let mut f = File::from_bytes(&input, Mode::Binary);
        let ops: &[(i32, i32, usize, usize)] = &[
            (0, SEEK_END, 0, 0),
            (84, SEEK_SET, 128, 1),
            (0, SEEK_SET, 80, 1),
            (0, SEEK_CUR, 4, 1),
            (84, SEEK_SET, 1, 50),
            (10, SEEK_END, 1, 4),
            (5, SEEK_CUR, 3, 4),
            (2, SEEK_SET, 50, 1),
        ];
        for &(offset, whence, size, items) in ops {
            // SAFETY: fp is open; the buffer holds size * items bytes.
            let theirs = unsafe {
                let s = ucrt::fseek(fp, offset, whence);
                let t = ucrt::ftell(fp);
                let mut buf = vec![0u8; size * items];
                let r = if size * items > 0 {
                    ucrt::fread(buf.as_mut_ptr().cast(), size, items, fp)
                } else {
                    0
                };
                let e = ucrt::feof(fp) != 0;
                let t2 = ucrt::ftell(fp);
                (s, t, r, buf, e, t2)
            };
            let ours = {
                let s = f.fseek(offset, whence);
                let t = f.ftell();
                let mut buf = vec![0u8; size * items];
                let r = f.fread(&mut buf, size, items);
                (s, t, r, buf, f.feof(), f.ftell())
            };
            count += 1;
            if ours != theirs {
                mismatches.add(format!(
                    "len {len} fseek({offset}, {whence}) fread({size}, {items}): orca-fmt {:?} UCRT {:?}",
                    (ours.0, ours.1, ours.2, ours.4, ours.5),
                    (theirs.0, theirs.1, theirs.2, theirs.4, theirs.5)
                ));
            }
        }
        // rewind clears the end-of-file indicator; getc then reads the first byte.
        // SAFETY: fp is open.
        let theirs = unsafe {
            ucrt::rewind(fp);
            (ucrt::feof(fp) != 0, ucrt::getc(fp), ucrt::ftell(fp))
        };
        f.rewind();
        let ours = (f.feof(), f.getc(), f.ftell());
        count += 1;
        if ours != theirs {
            mismatches.add(format!(
                "len {len} rewind: orca-fmt {ours:?} UCRT {theirs:?}"
            ));
        }
        // SAFETY: fp is open.
        unsafe { ucrt::fclose(fp) };
        // A seek before the start fails in both. Where the UCRT's stream is left afterwards (at
        // the file's position under its buffer) is not modelled: no reader's seek can fail.
        for (offset, whence) in [(-1, SEEK_SET), (-(len as i32) - 1, SEEK_END)] {
            let fp = tmp.open(Mode::Binary);
            // SAFETY: fp is open, then closed.
            let theirs = unsafe {
                let s = ucrt::fseek(fp, offset, whence);
                ucrt::fclose(fp);
                s
            };
            let ours = File::from_bytes(&input, Mode::Binary).fseek(offset, whence);
            count += 1;
            if (ours, theirs) != (-1, -1) {
                mismatches.add(format!(
                    "len {len} fseek({offset}, {whence}): orca-fmt {ours} UCRT {theirs}"
                ));
            }
        }
    }
    mismatches.assert_none(count, "binary stream");
    assert_eq!(count, 11 * 11);
}

/// The CR box (`parity/HAZARDS.md`): admesh scans `" solid %[^\n]"` into `char[256]`, so a solid
/// line of 256 characters or more overflows it in C. orca-fmt stops before the 256th character
/// and returns [`Overflow`]; 255 characters still fit with their NUL.
#[test]
fn overlong_scanset_stops() {
    for (name_len, fits) in [(254usize, true), (255, true), (256, false), (3200, false)] {
        let mut input = b"solid ".to_vec();
        input.extend(std::iter::repeat_n(b'n', name_len));
        input.extend_from_slice(b"\nfacet");
        let mut buf = [0u8; 256];
        let mut f = File::from_bytes(&input, Mode::Text);
        let r = scanf::fscanf(&mut f, b" solid %[^\n]", &mut [ScanArg::Chars(&mut buf)]);
        if fits {
            assert_eq!(r, Ok(1));
            assert_eq!(buf.iter().position(|&c| c == 0), Some(name_len));
        } else {
            assert_eq!(r, Err(Overflow { arg: 0, size: 256 }));
            assert!(buf[..255].iter().all(|&c| c == b'n'));
        }
    }
    // A file whose lines end with a CR alone has no \n: the whole file is the solid's name.
    let mut cr_file = b"solid box\r".to_vec();
    for _ in 0..12 {
        cr_file.extend_from_slice(
            b"facet normal 0 0 1\r outer loop\r  vertex 0 0 0\r endloop\rendfacet\r",
        );
    }
    let mut buf = [0u8; 256];
    let mut f = File::from_bytes(&cr_file, Mode::Text);
    let r = scanf::fscanf(&mut f, b" solid %[^\n]", &mut [ScanArg::Chars(&mut buf)]);
    assert_eq!(r, Err(Overflow { arg: 0, size: 256 }));
    // %31s never overflows its char[32].
    let mut a = [0u8; 32];
    let long = vec![b'7'; 100];
    let r = scanf::sscanf(&long, b"%31s", &mut [ScanArg::Chars(&mut a)]);
    assert_eq!(r, Ok(1));
    assert_eq!(a[31], 0);
}

/// C's `stdout` goes to the sink when one is set, and back to the process's output when it is
/// taken away.
#[test]
fn stdout_sink() {
    use std::sync::{Arc, Mutex};
    let got = Arc::new(Mutex::new(Vec::new()));
    let g = Arc::clone(&got);
    let before = stdio::set_stdout_sink(Some(Box::new(move |b: &[u8]| {
        g.lock().unwrap().extend_from_slice(b)
    })));
    stdio::stdout_write(b"facet (1)/ first facet\n");
    let mine = stdio::set_stdout_sink(before);
    assert!(mine.is_some());
    assert_eq!(got.lock().unwrap().as_slice(), b"facet (1)/ first facet\n");
}

/// `fopen` of a missing file and of a folder gives no stream, as the C function's null pointer.
#[test]
fn fopen_failures() {
    let dir = std::env::temp_dir();
    assert!(stdio::fopen(&dir.join("orca-fmt-no-such-file.stl"), "rb").is_none());
    assert!(stdio::fopen(&dir, "rb").is_none());
    let tmp = TempFile::new("open");
    tmp.write(b"a\r\nb");
    let f = stdio::fopen(&tmp.path, "rb").unwrap();
    assert_eq!(f.len(), 4);
    let mut f = stdio::freopen(&tmp.path, "r", f).unwrap();
    assert_eq!((f.mode(), f.len()), (Mode::Text, 3));
    assert_eq!(f.ftell(), -1);
    assert_eq!(f.fseek(0, SEEK_SET), 0);
    assert_eq!(f.fseek(1, SEEK_SET), -1);
}

/// Every byte value after each prefix of a number, and against a literal, a scanset and `%s`, on
/// a binary stream (so that no byte is translated). Found: the character that ends a `%f` number
/// (or stops it from starting) is pushed back, except NUL and 0xFF, which `%f` drops; a literal,
/// a scanset and `%s` push back every byte, NUL and 0xFF included.
#[test]
fn ucrt_stop_bytes() {
    let tmp = TempFile::new("bytes");
    let mut mismatches = Mismatches::default();
    let mut count = 0u64;
    let contexts: &[(&[u8], &[u8], &[Kind])] = &[
        (b"", b"%f", &[Kind::F32]),
        (b"  ", b"%f", &[Kind::F32]),
        (b"1", b"%f", &[Kind::F32]),
        (b"1.5", b"%f", &[Kind::F32]),
        (b"1e", b"%f", &[Kind::F32]),
        (b"1e-", b"%f", &[Kind::F32]),
        (b"1e5", b"%f", &[Kind::F32]),
        (b"-", b"%f", &[Kind::F32]),
        (b"0x", b"%f", &[Kind::F32]),
        (b"0x1p", b"%f", &[Kind::F32]),
        (b"in", b"%f", &[Kind::F32]),
        (b"inf", b"%f", &[Kind::F32]),
        (b"infi", b"%f", &[Kind::F32]),
        (b"nan", b"%f", &[Kind::F32]),
        (b"nan(", b"%f", &[Kind::F32]),
        (b"nan(ab", b"%f", &[Kind::F32]),
        (b"", b"x", &[]),
        (b"  ", b" x", &[]),
        (b"ab", b"%[ab]", &[Kind::Chars(16)]),
        (b"", b"%[ab]", &[Kind::Chars(16)]),
        (b"ab", b"%s", &[Kind::Chars(16)]),
        (b"ab", b"%[^\n]", &[Kind::Chars(16)]),
    ];
    for (prefix, format, kinds) in contexts {
        for b in 0..=255u8 {
            let mut input = prefix.to_vec();
            input.push(b);
            input.extend_from_slice(b"z\n");
            let theirs = ucrt_scan(&input, format, kinds, Some((&tmp, Mode::Binary)));
            let ours = our_scan(&input, format, kinds, Some(Mode::Binary));
            count += 1;
            if ours != theirs {
                mismatches.add(format!(
                    "{} on {}: orca-fmt {ours:?} UCRT {theirs:?}",
                    show(format),
                    show(&input)
                ));
            }
        }
    }
    mismatches.assert_none(count, "stop bytes");
    assert_eq!(count, 22 * 256);

    // The finding.
    let f = |input: &[u8]| ucrt_scan(input, b"%f", &[Kind::F32], Some((&tmp, Mode::Binary)));
    assert_eq!(f(b"1\x00abc").rest, b"abc");
    assert_eq!(f(b"1\xffabc").rest, b"abc");
    assert_eq!(f(b"1\x01abc").rest, b"\x01abc");
    let x = ucrt_scan(b"\x00abc", b"x", &[], Some((&tmp, Mode::Binary)));
    assert_eq!(x.rest, b"\x00abc");
}
