// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The static UCRT's `fscanf` and `sscanf`, written from the C standard (C17 7.21.6.2) and from
//! black-box tests of the UCRT (`tests/scanf.rs`, the `ucrt_` tests; the `cstdio.sop1` probe
//! records), never from the UCRT's source, for the formats the engine's model readers pass:
//! admesh's ASCII STL reader (`deps_src/admesh/stlinit.cpp`: `" solid %[^\n]"`,
//! `" endsolid%*[^\n]\n"`, `" solid%*[^\n]\n"`, `" facet normal %31s %31s %31s"`,
//! `" outer loop"`, `" vertex %f %f %f"`, `" "`, `"%s %s %s"` and `"%f"`). Filled by the o2-mesh
//! task (2026-11-07).
//!
//! The directives, as the UCRT runs them:
//!
//! - **White space** in the format (any run of it) reads and drops all white space in the input
//!   (`isspace` of the "C" locale), none included. Reaching the end there is no failure.
//! - **Any other character** must come next in the input. A different character is pushed back
//!   and ends the call (a matching failure); the end of the input ends it too (an input failure).
//! - **`%s`** drops white space, then reads characters up to the next white space, at most the
//!   width; **`%[...]`** reads characters of the set (`^` first: not of the set), at most the
//!   width, without dropping white space first. Each stores what it read and a NUL. Reading nothing
//!   is a matching failure, or an input failure at the end of the input.
//! - **`%f`** (and `%e`, `%g`, `%a` and their capitals; `%lf` and the rest for a `double`) drops
//!   white space and reads the longest prefix of a number `strtod` takes: a sign; then digits with
//!   one point and an exponent (`e`, sign, digits) after at least one digit, or `0x` and
//!   hexadecimal digits with one point and a binary exponent (`p`), or `inf`, `infinity`, `nan`
//!   or `nan(` letters, digits and `_` `)`, in any case. The first character that cannot continue
//!   the number is pushed back, unless it is NUL or 0xFF: `%f` drops those two, where a literal, a
//!   scanset and `%s` push back every byte (`ucrt_stop_bytes`). If what was read is not a whole
//!   number (a sign alone, `1e`, `1e+`, `0x`, `infin`, `nan(ab`), the conversion fails: a matching
//!   failure, but an input failure when
//!   it was the end of the input that stopped it (`"1e+"` gives `EOF`, `"1e+x"` gives 0 and leaves
//!   the `x`). No more than one character is ever pushed back, so `"infix"` leaves `"x"`, its
//!   `"infi"` read and lost. A whole number is converted by `strtof` (`strtod` for `%lf`) of the
//!   characters read: the same value bit for bit (`ucrt_scanf_float_is_strtof`), with the UCRT's
//!   own rounding of ties and its 768-digit limit (`crate::parse`).
//! - **`*`** reads as the conversion would and stores nothing; it does not count.
//!
//! The call returns the number of conversions stored, or `EOF` when an input failure came before
//! any was stored (a format with no conversion that reaches the end of the input inside a literal
//! returns `EOF`; one whose last white space reaches it returns 0).
//!
//! A buffer the C code passes without a width that could hold the input (`" solid %[^\n]"` into
//! `char[256]`) would overflow in C. Here the call stops instead, before storing the byte that
//! would not fit with its NUL, and returns [`Overflow`] (`parity/HAZARDS.md`, the CR box).

use crate::parse::{strtod, strtof};
use crate::stdio::{EOF, File};

/// Where a conversion stores what it read.
#[derive(Debug)]
pub enum ScanArg<'a> {
    /// `float*`, for `%f` and its siblings.
    F32(&'a mut f32),
    /// `double*`, for `%lf` and its siblings.
    F64(&'a mut f64),
    /// `char[N]`, for `%s` and `%[`: N is the slice's length.
    Chars(&'a mut [u8]),
}

/// A conversion that would have written past the end of its `char` buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Overflow {
    /// Which argument (0 for the first).
    pub arg: usize,
    /// Its size in bytes.
    pub size: usize,
}

/// A byte source for the scanner: a `FILE` or a C string.
trait Source {
    /// The next byte, or [`EOF`].
    fn get(&mut self) -> i32;
    /// Pushes back `c`, the byte just read ([`EOF`] is not pushed back).
    fn unget(&mut self, c: i32);
}

impl Source for File {
    fn get(&mut self) -> i32 {
        self.getc()
    }
    fn unget(&mut self, c: i32) {
        if c != EOF {
            let back = self.ungetc(c);
            debug_assert_eq!(back, c, "scanf pushes back only the byte it read");
        }
    }
}

/// `sscanf`'s input: a C string, which ends at its first NUL.
struct CStr<'a> {
    s: &'a [u8],
    pos: usize,
}

impl Source for CStr<'_> {
    fn get(&mut self) -> i32 {
        match self.s.get(self.pos) {
            Some(&c) if c != 0 => {
                self.pos += 1;
                i32::from(c)
            }
            _ => EOF,
        }
    }
    fn unget(&mut self, c: i32) {
        if c != EOF {
            self.pos -= 1;
        }
    }
}

/// `isspace` of the "C" locale: space, `\t`, `\n`, `\v`, `\f` and `\r`.
const fn is_space(c: i32) -> bool {
    matches!(c, 0x20 | 0x09 | 0x0A | 0x0B | 0x0C | 0x0D)
}

/// How a directive failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    /// The input did not match: the call returns the conversions stored so far.
    Matching,
    /// The input ended: the call returns `EOF` if nothing was stored yet.
    Input,
}

/// `fscanf(stream, format, ...)`: see the module documentation.
///
/// # Errors
///
/// [`Overflow`] when a `%s` or `%[` conversion would write past its buffer.
///
/// # Panics
///
/// On a directive the readers do not use (`%d`, `%c`, `%n`, `%%`, a range in a scanset, a width on
/// `%f`), or arguments that do not match the format.
pub fn fscanf(stream: &mut File, format: &[u8], args: &mut [ScanArg<'_>]) -> Result<i32, Overflow> {
    scan(stream, format, args)
}

/// `sscanf(s, format, ...)`, `s` read up to its first NUL or its end: as [`fscanf`].
///
/// # Errors
///
/// As [`fscanf`].
///
/// # Panics
///
/// As [`fscanf`].
pub fn sscanf(s: &[u8], format: &[u8], args: &mut [ScanArg<'_>]) -> Result<i32, Overflow> {
    scan(&mut CStr { s, pos: 0 }, format, args)
}

/// Drops white space from `src`; the first byte that is not is pushed back.
fn skip_space<S: Source>(src: &mut S) {
    loop {
        let c = src.get();
        if !is_space(c) {
            src.unget(c);
            return;
        }
    }
}

/// A scanset: which bytes `%[...]` takes.
struct ScanSet {
    member: [bool; 256],
}

impl ScanSet {
    /// Parses the set that starts at `format[i]`, just after the `[`; returns it and the index
    /// after its `]`.
    fn parse(format: &[u8], mut i: usize) -> (ScanSet, usize) {
        let negate = format.get(i) == Some(&b'^');
        if negate {
            i += 1;
        }
        let mut listed = [false; 256];
        let mut first = true;
        loop {
            let Some(&c) = format.get(i) else {
                panic!("orca-fmt scanf: a scanset without its ]");
            };
            if c == b']' && !first {
                i += 1;
                break;
            }
            let next = format.get(i + 1).copied();
            assert!(
                !(c == b'-' && !first && next != Some(b']')),
                "orca-fmt scanf: ranges in a scanset are not supported"
            );
            listed[usize::from(c)] = true;
            first = false;
            i += 1;
        }
        let mut member = [false; 256];
        for (b, m) in member.iter_mut().enumerate() {
            *m = listed[b] != negate;
        }
        (ScanSet { member }, i)
    }

    fn contains(&self, c: i32) -> bool {
        (0..256).contains(&c) && self.member[c as usize]
    }
}

/// Reads characters `take` accepts, at most `width` (0: no limit), into `out` (or nowhere when
/// suppressed). `first` is the first byte, already read.
fn read_chars<S: Source>(
    src: &mut S,
    first: i32,
    width: usize,
    take: impl Fn(i32) -> bool,
    mut out: Option<&mut [u8]>,
    arg: usize,
) -> Result<Result<(), Failure>, Overflow> {
    let mut n = 0usize;
    let mut c = first;
    loop {
        if c == EOF {
            if n == 0 {
                return Ok(Err(Failure::Input));
            }
            break;
        }
        if !take(c) {
            src.unget(c);
            if n == 0 {
                return Ok(Err(Failure::Matching));
            }
            break;
        }
        if let Some(buf) = out.as_deref_mut() {
            // The byte and a NUL after it must fit.
            if n + 1 >= buf.len() {
                src.unget(c);
                return Err(Overflow {
                    arg,
                    size: buf.len(),
                });
            }
            buf[n] = c as u8;
        }
        n += 1;
        if width != 0 && n == width {
            break;
        }
        c = src.get();
    }
    if let Some(buf) = out {
        buf[n] = 0;
    }
    Ok(Ok(()))
}

/// Pushes back `c`, the character that ended a `%f` number or kept it from starting, unless it is
/// NUL or 0xFF: the UCRT's `%f` drops those two (`ucrt_stop_bytes`).
fn unget_number_end<S: Source>(src: &mut S, c: i32) {
    if c != 0 && c != 0xFF {
        src.unget(c);
    }
}

/// The failure for a number stopped by `c`: an input failure at the end, otherwise a matching
/// failure with `c` pushed back (see [`unget_number_end`]).
fn stopped<S: Source>(src: &mut S, c: i32) -> Failure {
    if c == EOF {
        Failure::Input
    } else {
        unget_number_end(src, c);
        Failure::Matching
    }
}

/// Whether `c` is `want` (a lower-case letter) in either case.
fn is_letter(c: i32, want: u8) -> bool {
    c == i32::from(want) || c == i32::from(want.to_ascii_uppercase())
}

fn is_digit(c: i32) -> bool {
    (i32::from(b'0')..=i32::from(b'9')).contains(&c)
}

fn is_hex_digit(c: i32) -> bool {
    is_digit(c)
        || (i32::from(b'a')..=i32::from(b'f')).contains(&c)
        || (i32::from(b'A')..=i32::from(b'F')).contains(&c)
}

fn is_alnum(c: i32) -> bool {
    is_digit(c)
        || (i32::from(b'a')..=i32::from(b'z')).contains(&c)
        || (i32::from(b'A')..=i32::from(b'Z')).contains(&c)
}

/// Reads the characters of a `%f` number, `first` (not white space, not `EOF`) already read: see
/// the module documentation.
fn read_number<S: Source>(src: &mut S, first: i32) -> Result<Vec<u8>, Failure> {
    let mut text = Vec::new();
    let mut c = first;
    if c == i32::from(b'+') || c == i32::from(b'-') {
        text.push(c as u8);
        c = src.get();
    }
    if is_letter(c, b'i') {
        text.push(c as u8);
        for &want in b"nf" {
            c = src.get();
            if !is_letter(c, want) {
                return Err(stopped(src, c));
            }
            text.push(c as u8);
        }
        c = src.get();
        if !is_letter(c, b'i') {
            unget_number_end(src, c);
            return Ok(text);
        }
        text.push(c as u8);
        for &want in b"nity" {
            c = src.get();
            if !is_letter(c, want) {
                return Err(stopped(src, c));
            }
            text.push(c as u8);
        }
        return Ok(text);
    }
    if is_letter(c, b'n') {
        text.push(c as u8);
        for &want in b"an" {
            c = src.get();
            if !is_letter(c, want) {
                return Err(stopped(src, c));
            }
            text.push(c as u8);
        }
        c = src.get();
        if c != i32::from(b'(') {
            unget_number_end(src, c);
            return Ok(text);
        }
        text.push(b'(');
        loop {
            c = src.get();
            if c == i32::from(b')') {
                text.push(b')');
                return Ok(text);
            }
            if !(is_alnum(c) || c == i32::from(b'_')) {
                return Err(stopped(src, c));
            }
            text.push(c as u8);
        }
    }
    let mut digits = 0usize;
    let mut hex = false;
    if c == i32::from(b'0') {
        text.push(b'0');
        digits = 1;
        c = src.get();
        if is_letter(c, b'x') {
            text.push(c as u8);
            digits = 0;
            hex = true;
            c = src.get();
        }
    }
    let digit = |c: i32| if hex { is_hex_digit(c) } else { is_digit(c) };
    while digit(c) {
        text.push(c as u8);
        digits += 1;
        c = src.get();
    }
    if c == i32::from(b'.') {
        text.push(b'.');
        c = src.get();
        while digit(c) {
            text.push(c as u8);
            digits += 1;
            c = src.get();
        }
    }
    if digits == 0 {
        return Err(stopped(src, c));
    }
    if is_letter(c, if hex { b'p' } else { b'e' }) {
        text.push(c as u8);
        c = src.get();
        if c == i32::from(b'+') || c == i32::from(b'-') {
            text.push(c as u8);
            c = src.get();
        }
        if !is_digit(c) {
            return Err(stopped(src, c));
        }
        while is_digit(c) {
            text.push(c as u8);
            c = src.get();
        }
    }
    unget_number_end(src, c);
    Ok(text)
}

/// The scanner both entry points run.
fn scan<S: Source>(src: &mut S, format: &[u8], args: &mut [ScanArg<'_>]) -> Result<i32, Overflow> {
    let mut stored = 0i32;
    let mut next_arg = 0usize;
    let mut i = 0usize;
    let result_of = |failure: Failure, stored: i32| -> i32 {
        if failure == Failure::Input && stored == 0 {
            EOF
        } else {
            stored
        }
    };
    while i < format.len() {
        let f = format[i];
        if f == 0 {
            break;
        }
        if is_space(i32::from(f)) {
            while i < format.len() && is_space(i32::from(format[i])) {
                i += 1;
            }
            skip_space(src);
            continue;
        }
        if f != b'%' {
            let c = src.get();
            if c == EOF {
                return Ok(result_of(Failure::Input, stored));
            }
            if c != i32::from(f) {
                src.unget(c);
                return Ok(stored);
            }
            i += 1;
            continue;
        }
        i += 1;
        let suppress = format.get(i) == Some(&b'*');
        if suppress {
            i += 1;
        }
        let mut width = 0usize;
        while let Some(&d) = format.get(i) {
            if !d.is_ascii_digit() {
                break;
            }
            width = width * 10 + usize::from(d - b'0');
            i += 1;
        }
        let long = format.get(i) == Some(&b'l');
        if long {
            i += 1;
        }
        let Some(&conv) = format.get(i) else {
            panic!("orca-fmt scanf: a format that ends inside a conversion");
        };
        i += 1;
        let arg_index = next_arg;
        let mut target = if suppress {
            None
        } else {
            next_arg += 1;
            Some(
                args.get_mut(arg_index)
                    .expect("orca-fmt scanf: fewer arguments than conversions"),
            )
        };
        match conv {
            b's' | b'[' => {
                let set = if conv == b'[' {
                    let (set, after) = ScanSet::parse(format, i);
                    i = after;
                    Some(set)
                } else {
                    None
                };
                let first = src.get();
                let first = if set.is_none() {
                    // %s drops white space first; %[ does not.
                    let mut c = first;
                    while is_space(c) {
                        c = src.get();
                    }
                    c
                } else {
                    first
                };
                let out = match target.as_deref_mut() {
                    None => None,
                    Some(ScanArg::Chars(buf)) => Some(&mut **buf),
                    Some(other) => panic!("orca-fmt scanf: %{} given {other:?}", conv as char),
                };
                let read = match &set {
                    Some(set) => {
                        read_chars(src, first, width, |c| set.contains(c), out, arg_index)?
                    }
                    None => read_chars(src, first, width, |c| !is_space(c), out, arg_index)?,
                };
                if let Err(failure) = read {
                    return Ok(result_of(failure, stored));
                }
                if !suppress {
                    stored += 1;
                }
            }
            b'f' | b'F' | b'e' | b'E' | b'g' | b'G' | b'a' | b'A' => {
                assert!(width == 0, "orca-fmt scanf: a width on %f is not supported");
                let mut c = src.get();
                while is_space(c) {
                    c = src.get();
                }
                if c == EOF {
                    return Ok(result_of(Failure::Input, stored));
                }
                let text = match read_number(src, c) {
                    Ok(text) => text,
                    Err(failure) => return Ok(result_of(failure, stored)),
                };
                match target {
                    None => {}
                    Some(ScanArg::F32(v)) if !long => **v = strtof(&text).value,
                    Some(ScanArg::F64(v)) if long => **v = strtod(&text).value,
                    Some(other) => panic!(
                        "orca-fmt scanf: %{}{} given {other:?}",
                        if long { "l" } else { "" },
                        conv as char
                    ),
                }
                if !suppress {
                    stored += 1;
                }
            }
            _ => panic!(
                "orca-fmt scanf: the conversion %{} is not supported",
                conv as char
            ),
        }
    }
    Ok(stored)
}
