// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! `printf`, written from the C standard (C17 7.21.6.1), with the UCRT's choice wherever the
//! standard leaves one open, so that its bytes are those the C++ engine's `sprintf`, `snprintf`
//! and `sprintf_s` give. `tests/fmt.rs` compares it, byte for byte, with the UCRT's own
//! `__stdio_common_vsprintf` and `_snprintf_s` (linked into the test program) on millions of
//! seeded formats and values, valid and invalid.
//!
//! The format is read up to its first NUL byte, as a C string is. A conversion is
//! `%[flags][width][.precision][length]conversion`:
//!
//! - flags `-`, `+`, space, `#` and `0`, in any order, repeated or not;
//! - width and precision as decimal digits or `*`, which reads an `int` argument: a negative width
//!   is the `-` flag and the width's magnitude, a negative precision is no precision, and a `.`
//!   without digits is precision 0. Digits after a `*` replace the value it read (`%*5d` has width
//!   5); a `*` after digits is not part of the conversion. A width or precision above `INT_MAX`
//!   is [`FormatError::Overflow`], and so is a `*` width of `INT_MIN` (whose negation the UCRT
//!   leaves at `INT_MIN`) unless the field is empty, which it then does not pad;
//! - length `hh`, `h`, `l`, `ll`, `L`, `w`, `j`, `z`, `t`, `I`, `I32`, `I64` or `T`: `l` is 32
//!   bits, as `long` is on Windows; `j`, `z`, `t`, `I` and `I64` are 64; a bare `I` is replaced by
//!   the length after it (`%Ihd` is `%hd`); any other second length (`%lh`, `%lll`, `%I64I64`) is
//!   refused at once;
//! - conversion `d i u o x X c C s S p e E f F g G a A Z %`.
//!
//! Where the standard lets the implementation choose, this does what the UCRT (with the options the
//! C++ engine's startup sets: standard rounding and the legacy wide specifiers) does:
//!
//! - infinities and NaNs print as `inf`, `nan`, `nan(snan)` (a signalling NaN) and `-nan(ind)`
//!   (the negative NaN with only the quiet bit set), uppercase for `F E G A`; the `0` flag does not
//!   pad them and `%a` gives them no `0x`. With `#` and precision 0, `%e`, `%f` and `%a` put a
//!   point after the first character all the same (`%#.0f` of infinity is `i.nf`, of minus
//!   infinity `-.inf`), after which a negative one is padded with zeros and `%a` adds `0x`
//!   (`%#012.0a` gives `-0x00000.inf`), as the UCRT does;
//! - `%a` prints `0x` and then, without a precision, all 13 hexits of a double's fraction (no
//!   trimming); a precision rounds half to even, and a carry gives a leading `2` (`0x2p+0`); a
//!   subnormal starts `0x0.`;
//! - `%p` prints the 16 uppercase hexits of the pointer; a precision, `#`, `0` and the sign flags do
//!   not apply to it;
//! - `%s` of a null pointer prints `(null)`, and `%Z` of one too (whatever the precision);
//! - the `0` flag pads `%c` and `%s` with zeros on the left;
//! - `%c` and `%s` are narrow, and wide with `l` or `w`; `%C` and `%S` are wide, and narrow with `h`
//!   or `T`. A wide character above 0xFF prints nothing, not even its padding, and a wide string
//!   with one is [`FormatError::Unconvertible`] (the conversions of the "C" locale);
//! - the floating conversions take any length (`%lf`, `%Lf`, `%hf` all read a `double`); the
//!   integer conversions refuse `L`, `w` and `T`; `%n` is refused (the UCRT disables it).
//!
//! The two families differ only on a format with a mistake in it ([`Mode`]): `sprintf` and
//! `snprintf` drop a conversion that a character cannot continue and print that character as text
//! (`%5-d` prints `-d`, `%y` prints `y`, a `%` at the end prints nothing, `%5%` prints `%`), while
//! `sprintf_s` and `_snprintf_s` refuse the whole call ([`FormatError::Invalid`]), as they refuse
//! a `%%` with anything between its two `%`. Both refuse `%n`, a second length that does not
//! combine, and a length the conversion does not take.
//!
//! Each [`Arg`] is one 64-bit argument slot, as the x64 calling convention passes `...`. A
//! conversion reads its type from the slot: `%d` of `Arg::Int(-1)` reads the low 32 bits and prints
//! `-1`, `%u` prints `4294967295`, `%hhd` of `Arg::Int(300)` prints `44`, and `%d` of a
//! `Arg::Double` reads the low bits of the double, as C++ would. A `float` argument is promoted to
//! `double`, as `...` promotes it.
//!
//! The digits come from [`crate::to_chars`]: Ryu printf for `%e`, `%f` and `%g`, correctly rounded
//! with ties to even (the rounding of the UCRT's standard mode, which a C++ program gets), and the
//! hex precision form for `%a`.

use crate::to_chars::{CharsFormat, to_chars_precision, to_chars_string};
use std::fmt;

/// One argument of a printf call: one 64-bit slot of the C++ code's `...`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Arg<'a> {
    /// A signed integer: `int`, `long`, `long long`, or a `char`, `short` or `bool` promoted to
    /// `int`.
    Int(i64),
    /// An unsigned integer: `unsigned`, `unsigned long long`, `size_t`.
    UInt(u64),
    /// A `double`, or a `float` promoted to `double`.
    Double(f64),
    /// A `char*` string: its bytes up to the first NUL or the end of the slice.
    Str(&'a [u8]),
    /// A `wchar_t*` string: its units up to the first NUL or the end of the slice.
    WStr(&'a [u16]),
    /// A null pointer: `(null)` for the string conversions, zero for the others.
    Null,
    /// A pointer value, for `%p`.
    Ptr(usize),
}

macro_rules! arg_from {
    ($($t:ty => $variant:ident as $wide:ty),* $(,)?) => {
        $(
            impl From<$t> for Arg<'_> {
                fn from(v: $t) -> Self {
                    Arg::$variant(<$wide>::from(v))
                }
            }
        )*
    };
}

arg_from!(
    i8 => Int as i64,
    i16 => Int as i64,
    i32 => Int as i64,
    i64 => Int as i64,
    bool => Int as i64,
    u8 => UInt as u64,
    u16 => UInt as u64,
    u32 => UInt as u64,
    u64 => UInt as u64,
    f32 => Double as f64,
    f64 => Double as f64,
);

impl From<isize> for Arg<'_> {
    fn from(v: isize) -> Self {
        // isize is 64 bits on the engine's only target.
        Arg::Int(v as i64)
    }
}

impl From<usize> for Arg<'_> {
    fn from(v: usize) -> Self {
        Arg::UInt(v as u64)
    }
}

impl<'a> From<&'a str> for Arg<'a> {
    fn from(v: &'a str) -> Self {
        Arg::Str(v.as_bytes())
    }
}

impl<'a> From<&'a [u8]> for Arg<'a> {
    fn from(v: &'a [u8]) -> Self {
        Arg::Str(v)
    }
}

impl<'a> From<&'a String> for Arg<'a> {
    fn from(v: &'a String) -> Self {
        Arg::Str(v.as_bytes())
    }
}

impl<'a> From<&'a [u16]> for Arg<'a> {
    fn from(v: &'a [u16]) -> Self {
        Arg::WStr(v)
    }
}

/// Which family of the UCRT's printf functions to behave as. They print every valid format the
/// same way and differ on a format with a mistake in it (see the module documentation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// `printf`, `sprintf`, `snprintf`, `vsnprintf`: a conversion that a character cannot continue
    /// is dropped and the character printed as text.
    Standard,
    /// `sprintf_s`, `_snprintf_s`, `vsprintf_s` (and so `std::to_string` and the `iostream`
    /// number output, which call them): such a format is refused.
    Secure,
}

/// Why a printf call printed nothing (the UCRT returns -1 for each).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatError {
    /// A format the UCRT refuses with its invalid parameter handler (`EINVAL`): see the module
    /// documentation.
    Invalid,
    /// A width or precision above `INT_MAX`, or an output longer than `INT_MAX` bytes.
    Overflow,
    /// A wide string with a character the "C" locale cannot convert (`EILSEQ`).
    Unconvertible,
    /// The format reads more arguments than there are.
    MissingArgument,
    /// An argument of the wrong kind for its conversion: a string for `%d` or `%f`, a number for
    /// `%s`, a narrow string for `%ls` or a wide one for `%s`.
    ArgumentType,
    /// `%Z` of a pointer that is not null, which [`Arg`] cannot carry.
    Unsupported,
    /// [`sprintf_string`]: the output is not UTF-8 (a `%c` or `%s` byte above 0x7F).
    NotUtf8,
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            FormatError::Invalid => "invalid printf format",
            FormatError::Overflow => "printf width, precision or output above INT_MAX",
            FormatError::Unconvertible => "wide printf argument the C locale cannot convert",
            FormatError::MissingArgument => "printf format reads more arguments than were given",
            FormatError::ArgumentType => "printf argument of the wrong kind for its conversion",
            FormatError::Unsupported => "printf %Z of a counted string",
            FormatError::NotUtf8 => "printf output is not UTF-8",
        })
    }
}

impl std::error::Error for FormatError {}

/// The output of `format` with `args`, as `mode`'s functions print it, without a terminating NUL.
///
/// # Errors
///
/// [`FormatError`] for a format the UCRT refuses, an overflow, a missing argument, an argument of
/// the wrong kind or a wide string the "C" locale cannot convert.
pub fn format(mode: Mode, format: &[u8], args: &[Arg<'_>]) -> Result<Vec<u8>, FormatError> {
    let mut out = Vec::new();
    format_into(&mut out, mode, format, args)?;
    if out.len() > i32::MAX as usize {
        return Err(FormatError::Overflow);
    }
    Ok(out)
}

/// `sprintf` into a new buffer: the bytes printed, without a terminating NUL.
///
/// # Errors
///
/// As [`format`] with [`Mode::Standard`].
pub fn sprintf(format: &[u8], args: &[Arg<'_>]) -> Result<Vec<u8>, FormatError> {
    self::format(Mode::Standard, format, args)
}

/// `sprintf_s` into a new buffer: the bytes printed, without a terminating NUL.
///
/// # Errors
///
/// As [`format`] with [`Mode::Secure`].
pub fn sprintf_s(format: &[u8], args: &[Arg<'_>]) -> Result<Vec<u8>, FormatError> {
    self::format(Mode::Secure, format, args)
}

/// [`sprintf`] as a `String`.
///
/// # Errors
///
/// As [`sprintf`], and [`FormatError::NotUtf8`] when the bytes are not UTF-8.
pub fn sprintf_string(format: &str, args: &[Arg<'_>]) -> Result<String, FormatError> {
    String::from_utf8(sprintf(format.as_bytes(), args)?).map_err(|_| FormatError::NotUtf8)
}

/// C99 `snprintf` as the UCRT has it: writes the first `buf.len() - 1` bytes of the output and a
/// NUL after them (nothing when `buf` is empty) and returns the length of the whole output,
/// without its NUL, so a result of `buf.len()` or more means the output was cut.
///
/// # Errors
///
/// As [`sprintf`]; `buf` is then left as it was.
pub fn snprintf(buf: &mut [u8], format: &[u8], args: &[Arg<'_>]) -> Result<usize, FormatError> {
    let out = sprintf(format, args)?;
    if let Some(room) = buf.len().checked_sub(1) {
        let n = out.len().min(room);
        buf[..n].copy_from_slice(&out[..n]);
        buf[n] = 0;
    }
    Ok(out.len())
}

/// A conversion's length modifier.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Length {
    #[default]
    None,
    Hh,
    H,
    L,
    Ll,
    BigL,
    W,
    J,
    Z,
    T,
    /// A bare `I`: 64 bits, and replaced by a length that follows it.
    I,
    I32,
    I64,
    /// `T`, the UCRT's "TCHAR" length: narrow in a narrow printf.
    Tchar,
}

/// A conversion's flags, width, precision and length.
#[derive(Clone, Copy, Debug, Default)]
struct Spec {
    left: bool,
    plus: bool,
    space: bool,
    alt: bool,
    zero: bool,
    width: usize,
    precision: Option<usize>,
    length: Length,
}

/// Where the reading of a conversion is: what the next character may be.
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    /// Just after the `%`.
    Percent,
    /// After a flag.
    Flag,
    /// After the width.
    Width,
    /// After the `.`.
    Dot,
    /// After the precision.
    Precision,
    /// After a length.
    Size,
}

/// The format's bytes up to its first NUL.
fn until_nul(s: &[u8]) -> &[u8] {
    match s.iter().position(|&c| c == 0) {
        Some(n) => &s[..n],
        None => s,
    }
}

/// The wide string's units up to its first NUL.
fn until_wnul(s: &[u16]) -> &[u16] {
    match s.iter().position(|&c| c == 0) {
        Some(n) => &s[..n],
        None => s,
    }
}

type Args<'s, 'a> = std::slice::Iter<'s, Arg<'a>>;

fn format_into(
    out: &mut Vec<u8>,
    mode: Mode,
    format: &[u8],
    args: &[Arg<'_>],
) -> Result<(), FormatError> {
    let format = until_nul(format);
    let mut args = args.iter();
    let mut i = 0;
    while i < format.len() {
        let c = format[i];
        i += 1;
        if c != b'%' {
            out.push(c);
            continue;
        }
        if out.len() > i32::MAX as usize {
            return Err(FormatError::Overflow);
        }

        let mut spec = Spec::default();
        let mut state = State::Percent;
        loop {
            let Some(&c) = format.get(i) else {
                // The format ends inside the conversion.
                return match mode {
                    Mode::Standard => Ok(()),
                    Mode::Secure => Err(FormatError::Invalid),
                };
            };
            i += 1;
            match (state, c) {
                (State::Percent | State::Flag, b'-' | b'+' | b' ' | b'#' | b'0') => {
                    match c {
                        b'-' => spec.left = true,
                        b'+' => spec.plus = true,
                        b' ' => spec.space = true,
                        b'#' => spec.alt = true,
                        _ => spec.zero = true,
                    }
                    state = State::Flag;
                }
                (State::Percent | State::Flag, b'*') => {
                    let width = int_arg(&mut args)?;
                    if width < 0 {
                        spec.left = true;
                    }
                    // INT_MIN gives 2^31, which write_padded treats as the UCRT does.
                    spec.width = width.unsigned_abs() as usize;
                    state = State::Width;
                }
                (State::Percent | State::Flag | State::Width, b'1'..=b'9')
                | (State::Width, b'0') => {
                    i -= 1;
                    spec.width = decimal(format, &mut i)?;
                    state = State::Width;
                }
                (State::Percent | State::Flag | State::Width, b'.') => {
                    spec.precision = Some(0);
                    state = State::Dot;
                }
                (State::Dot, b'*') => {
                    spec.precision = usize::try_from(int_arg(&mut args)?).ok();
                    state = State::Precision;
                }
                (State::Dot | State::Precision, b'0'..=b'9') => {
                    i -= 1;
                    spec.precision = Some(decimal(format, &mut i)?);
                    state = State::Precision;
                }
                (_, b'h' | b'l' | b'L' | b'w' | b'j' | b'z' | b't' | b'I' | b'T') => {
                    let token = length_token(format, &mut i, c);
                    spec.length = combine(spec.length, token)?;
                    state = State::Size;
                }
                (_, b'%') => {
                    if mode == Mode::Secure && state != State::Percent {
                        return Err(FormatError::Invalid);
                    }
                    out.push(b'%');
                    break;
                }
                (_, b'd' | b'i' | b'u' | b'o' | b'x' | b'X' | b'c' | b'C' | b's' | b'S')
                | (_, b'p' | b'e' | b'E' | b'f' | b'F' | b'g' | b'G' | b'a' | b'A' | b'Z' | b'n') =>
                {
                    convert(out, &spec, c, &mut args)?;
                    break;
                }
                _ => match mode {
                    // The conversion is dropped and the character is text.
                    Mode::Standard => {
                        out.push(c);
                        break;
                    }
                    Mode::Secure => return Err(FormatError::Invalid),
                },
            }
        }
    }
    Ok(())
}

/// A width or precision written as digits: `INT_MAX` at most.
fn decimal(format: &[u8], i: &mut usize) -> Result<usize, FormatError> {
    let mut value: u64 = 0;
    while let Some(&c) = format.get(*i) {
        if !c.is_ascii_digit() {
            break;
        }
        value = value * 10 + u64::from(c - b'0');
        if value > i32::MAX as u64 {
            return Err(FormatError::Overflow);
        }
        *i += 1;
    }
    Ok(value as usize)
}

/// The length token that starts with `c`, the character before `format[*i]`: `I32` and `I64`
/// take their digits, every other token is the one character.
fn length_token(format: &[u8], i: &mut usize, c: u8) -> Length {
    match c {
        b'h' => Length::H,
        b'l' => Length::L,
        b'L' => Length::BigL,
        b'w' => Length::W,
        b'j' => Length::J,
        b'z' => Length::Z,
        b't' => Length::T,
        b'T' => Length::Tchar,
        _ => {
            if format[*i..].starts_with(b"64") {
                *i += 2;
                Length::I64
            } else if format[*i..].starts_with(b"32") {
                *i += 2;
                Length::I32
            } else {
                Length::I
            }
        }
    }
}

/// The length after `token` follows `current`: `h` and `h` make `hh`, `l` and `l` make `ll`, a bare
/// `I` gives way to what follows it, and anything else is refused at once.
fn combine(current: Length, token: Length) -> Result<Length, FormatError> {
    match (current, token) {
        (Length::None | Length::I, t) => Ok(t),
        (Length::H, Length::H) => Ok(Length::Hh),
        (Length::L, Length::L) => Ok(Length::Ll),
        _ => Err(FormatError::Invalid),
    }
}

/// The next argument as a 64-bit slot, for the integer conversions, `%c`, `%p` and `*`.
fn slot_arg(args: &mut Args<'_, '_>) -> Result<u64, FormatError> {
    match args.next() {
        None => Err(FormatError::MissingArgument),
        Some(Arg::Int(v)) => Ok(*v as u64),
        Some(Arg::UInt(v)) => Ok(*v),
        Some(Arg::Double(v)) => Ok(v.to_bits()),
        Some(Arg::Ptr(p)) => Ok(*p as u64),
        Some(Arg::Null) => Ok(0),
        Some(Arg::Str(_) | Arg::WStr(_)) => Err(FormatError::ArgumentType),
    }
}

/// The next argument as an `int`, for a `*` width or precision: the low 32 bits of its slot.
fn int_arg(args: &mut Args<'_, '_>) -> Result<i32, FormatError> {
    Ok(slot_arg(args)? as u32 as i32)
}

/// The next argument as a `double`: its slot's bits.
fn double_arg(args: &mut Args<'_, '_>) -> Result<f64, FormatError> {
    match args.next() {
        None => Err(FormatError::MissingArgument),
        Some(Arg::Double(v)) => Ok(*v),
        Some(Arg::Int(v)) => Ok(f64::from_bits(*v as u64)),
        Some(Arg::UInt(v)) => Ok(f64::from_bits(*v)),
        Some(Arg::Ptr(p)) => Ok(f64::from_bits(*p as u64)),
        Some(Arg::Null) => Ok(0.0),
        Some(Arg::Str(_) | Arg::WStr(_)) => Err(FormatError::ArgumentType),
    }
}

/// A string argument: narrow, wide, or a null pointer.
enum StrArg<'a> {
    Narrow(&'a [u8]),
    Wide(&'a [u16]),
    Null,
}

/// The next argument as a string of the width `wide` says.
fn string_arg<'a>(args: &mut Args<'_, 'a>, wide: bool) -> Result<StrArg<'a>, FormatError> {
    match args.next() {
        None => Err(FormatError::MissingArgument),
        Some(Arg::Str(s)) if !wide => Ok(StrArg::Narrow(until_nul(s))),
        Some(Arg::WStr(s)) if wide => Ok(StrArg::Wide(until_wnul(s))),
        Some(Arg::Null | Arg::Ptr(0)) => Ok(StrArg::Null),
        Some(_) => Err(FormatError::ArgumentType),
    }
}

/// Whether `%c`, `%s`, `%C` or `%S` with this length is the wide form: `%c` and `%s` with `l` or
/// `w`; `%C` and `%S` unless `h` or `T` (the UCRT's legacy wide specifiers).
fn is_wide(conversion: u8, length: Length) -> bool {
    match conversion {
        b'c' | b's' => matches!(length, Length::L | Length::W),
        _ => !matches!(length, Length::H | Length::Tchar),
    }
}

/// The bits an integer conversion reads from its slot.
fn integer_bits(length: Length) -> Result<u32, FormatError> {
    match length {
        Length::None | Length::L | Length::I32 => Ok(32),
        Length::Hh => Ok(8),
        Length::H => Ok(16),
        Length::Ll | Length::J | Length::Z | Length::T | Length::I | Length::I64 => Ok(64),
        Length::BigL | Length::W | Length::Tchar => Err(FormatError::Invalid),
    }
}

/// A wide character in the "C" locale: itself as a byte up to 0xFF, nothing above.
fn narrow_char(unit: u16) -> Option<u8> {
    u8::try_from(unit).ok()
}

fn convert(
    out: &mut Vec<u8>,
    spec: &Spec,
    conversion: u8,
    args: &mut Args<'_, '_>,
) -> Result<(), FormatError> {
    match conversion {
        b'd' | b'i' => {
            let bits = integer_bits(spec.length)?;
            let slot = slot_arg(args)?;
            write_integer(out, spec, slot, bits, true, 10, false)
        }
        b'u' | b'o' | b'x' | b'X' => {
            let bits = integer_bits(spec.length)?;
            let slot = slot_arg(args)?;
            let base = match conversion {
                b'u' => 10,
                b'o' => 8,
                _ => 16,
            };
            write_integer(out, spec, slot, bits, false, base, conversion == b'X')
        }
        b'c' | b'C' => {
            let slot = slot_arg(args)?;
            let byte = if is_wide(conversion, spec.length) {
                // wint_t is 16 bits. A character the "C" locale cannot convert prints nothing, not
                // even the padding.
                match narrow_char(slot as u16) {
                    Some(byte) => byte,
                    None => return Ok(()),
                }
            } else {
                slot as u8
            };
            write_padded(out, spec.width, spec.left, spec.zero, &[], &[byte])
        }
        b's' | b'S' => {
            let body: Vec<u8> = match string_arg(args, is_wide(conversion, spec.length))? {
                StrArg::Narrow(s) => s.to_vec(),
                StrArg::Null => b"(null)".to_vec(),
                StrArg::Wide(s) => {
                    let n = spec.precision.map_or(s.len(), |p| p.min(s.len()));
                    s[..n]
                        .iter()
                        .map(|&u| narrow_char(u).ok_or(FormatError::Unconvertible))
                        .collect::<Result<_, _>>()?
                }
            };
            let body = match spec.precision {
                Some(p) if p < body.len() => &body[..p],
                _ => &body[..],
            };
            write_padded(out, spec.width, spec.left, spec.zero, &[], body)
        }
        b'Z' => match args.next() {
            None => Err(FormatError::MissingArgument),
            // The precision does not shorten it.
            Some(Arg::Null | Arg::Ptr(0)) => {
                write_padded(out, spec.width, spec.left, spec.zero, &[], b"(null)")
            }
            Some(_) => Err(FormatError::Unsupported),
        },
        b'p' => {
            let slot = slot_arg(args)?;
            let pointer = Spec {
                alt: false,
                plus: false,
                space: false,
                zero: false,
                precision: Some(16),
                ..*spec
            };
            write_integer(out, &pointer, slot, 64, false, 16, true)
        }
        b'e' | b'E' | b'f' | b'F' | b'g' | b'G' | b'a' | b'A' => {
            let value = double_arg(args)?;
            write_float(out, spec, conversion, value)
        }
        // %n: the UCRT disables it.
        _ => Err(FormatError::Invalid),
    }
}

/// The field: `prefix` (a sign, `0x`) and `body`, padded to `width` with spaces on the left, with
/// zeros between prefix and body (`zero`), or with spaces on the right (`left`, which wins).
fn write_padded(
    out: &mut Vec<u8>,
    width: usize,
    left: bool,
    zero: bool,
    prefix: &[u8],
    body: &[u8],
) -> Result<(), FormatError> {
    let len = prefix.len() + body.len();
    // The UCRT subtracts in int: a width of 2^31 (a `*` argument of INT_MIN, whose negation stays
    // INT_MIN) pads an empty field by nothing and any other by 2^31 - len, past INT_MAX.
    let pad = if width == 1 << 31 && len == 0 {
        0
    } else {
        width.saturating_sub(len)
    };
    if out.len() + pad + len > i32::MAX as usize {
        return Err(FormatError::Overflow);
    }
    if !left && !zero {
        out.resize(out.len() + pad, b' ');
    }
    out.extend_from_slice(prefix);
    if !left && zero {
        out.resize(out.len() + pad, b'0');
    }
    out.extend_from_slice(body);
    if left {
        out.resize(out.len() + pad, b' ');
    }
    Ok(())
}

/// The digits of `v` in base 8, 10 or 16.
fn push_digits(body: &mut Vec<u8>, mut v: u64, base: u64, upper: bool) {
    let digits: &[u8; 16] = if upper {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    // 22 octal digits hold any u64.
    let mut tmp = [0u8; 22];
    let mut at = tmp.len();
    while v != 0 {
        at -= 1;
        tmp[at] = digits[(v % base) as usize];
        v /= base;
    }
    body.extend_from_slice(&tmp[at..]);
}

/// `%d %i %u %o %x %X` (and `%p`): the slot's low `bits` bits, sign-extended for `signed`.
fn write_integer(
    out: &mut Vec<u8>,
    spec: &Spec,
    slot: u64,
    bits: u32,
    signed: bool,
    base: u64,
    upper: bool,
) -> Result<(), FormatError> {
    let shift = 64 - bits;
    let (negative, magnitude) = if signed {
        let v = ((slot << shift) as i64) >> shift;
        (v < 0, v.unsigned_abs())
    } else {
        (false, (slot << shift) >> shift)
    };

    // A precision turns the 0 flag off; without one, the precision is 1.
    let (precision, zero) = match spec.precision {
        Some(p) => (p, false),
        None => (1, spec.zero),
    };
    if precision > i32::MAX as usize {
        return Err(FormatError::Overflow);
    }

    // Zero with precision 0 has no digits.
    let mut digits = Vec::new();
    push_digits(&mut digits, magnitude, base, upper);
    let mut body = Vec::with_capacity(crate::cpp_max(precision, digits.len()) + 1);
    body.resize(precision.saturating_sub(digits.len()), b'0');
    body.extend_from_slice(&digits);

    // # on o makes the first digit a zero; on x and X it prefixes 0x to a value that is not zero.
    if spec.alt && base == 8 && body.first() != Some(&b'0') {
        body.insert(0, b'0');
    }

    let mut prefix: Vec<u8> = Vec::with_capacity(2);
    if signed {
        if negative {
            prefix.push(b'-');
        } else if spec.plus {
            prefix.push(b'+');
        } else if spec.space {
            prefix.push(b' ');
        }
    }
    if spec.alt && base == 16 && magnitude != 0 {
        prefix.extend_from_slice(if upper { b"0X" } else { b"0x" });
    }

    write_padded(out, spec.width, spec.left, zero, &prefix, &body)
}

/// The most digits after the point that a double's exact value has (5e-324 has 1074): `%f` prints
/// zeros after them.
const MAX_FIXED_DIGITS: usize = 1074;

/// The most significant digits that a double's exact value has, less one: `%e` prints zeros after
/// them.
const MAX_SCIENTIFIC_DIGITS: usize = 766;

/// The hexits of a double's fraction: `%a` prints zeros after them.
const MAX_HEX_DIGITS: usize = 13;

/// Calls a `to_chars` form on a buffer of `capacity` bytes at the end of `body`, keeping what it
/// wrote.
fn push_chars(
    body: &mut Vec<u8>,
    capacity: usize,
    write: impl FnOnce(&mut [u8]) -> Result<usize, crate::Errc>,
) {
    let start = body.len();
    body.resize(start + capacity, 0);
    let n = write(&mut body[start..]).expect("the buffer holds the longest result of this form");
    body.truncate(start + n);
}

/// Inserts `count` zeros before the exponent, `marker` (`e`, or `p` after hexits, which may be an
/// `e`), that `body` has after `start`.
fn zeros_before_exponent(body: &mut Vec<u8>, start: usize, count: usize, marker: u8) {
    if count == 0 {
        return;
    }
    let at = body[start..]
        .iter()
        .position(|&c| c == marker)
        .map_or(body.len(), |n| start + n);
    body.splice(at..at, std::iter::repeat_n(b'0', count));
}

/// `%.*f` of a value that is finite and not negative.
fn fixed(body: &mut Vec<u8>, magnitude: f64, precision: usize) {
    let exact = precision.min(MAX_FIXED_DIGITS);
    // 309 integer digits, the point and the digits after it.
    push_chars(body, 320 + exact, |buf| {
        to_chars_precision(buf, magnitude, CharsFormat::Fixed, exact as i32)
    });
    body.resize(body.len() + (precision - exact), b'0');
}

/// `%.*e` of a value that is finite and not negative.
fn scientific(body: &mut Vec<u8>, magnitude: f64, precision: usize) {
    let start = body.len();
    let exact = precision.min(MAX_SCIENTIFIC_DIGITS);
    // A digit, the point, the digits after it and e+308.
    push_chars(body, 16 + exact, |buf| {
        to_chars_precision(buf, magnitude, CharsFormat::Scientific, exact as i32)
    });
    zeros_before_exponent(body, start, precision - exact, b'e');
}

/// `%.*a` without its `0x`, of a value that is finite and not negative.
fn hex(body: &mut Vec<u8>, magnitude: f64, precision: Option<usize>) {
    let start = body.len();
    let (exact, extra) = match precision {
        None => (-1, 0),
        Some(p) => {
            let exact = p.min(MAX_HEX_DIGITS);
            (exact as i32, p - exact)
        }
    };
    push_chars(body, 32, |buf| {
        to_chars_precision(buf, magnitude, CharsFormat::Hex, exact)
    });
    zeros_before_exponent(body, start, extra, b'p');
}

/// `%.*g` of a value that is finite and not negative. P is the precision, 6 without one, 1 for 0;
/// X is the exponent `%.*e` would print with precision P - 1. If P > X >= -4 the value prints as
/// `%f` with precision P - 1 - X, otherwise as `%e` with precision P - 1, and then trailing zeros
/// and a trailing point go unless `#` keeps them.
fn general(body: &mut Vec<u8>, magnitude: f64, precision: Option<usize>, alt: bool) {
    let p = match precision {
        None => 6,
        Some(0) => 1,
        Some(p) => p,
    };
    if !alt {
        // to_chars's general precision form is exactly that, with the trimming; its digits stop at
        // the exact value's, so the precision it is given needs no bound here.
        push_chars(body, 800, |buf| {
            to_chars_precision(
                buf,
                magnitude,
                CharsFormat::General,
                p.min(i32::MAX as usize) as i32,
            )
        });
        return;
    }
    let mut e_form = Vec::new();
    scientific(&mut e_form, magnitude, p - 1);
    let x = exponent(&e_form);
    if (p as i64) > x && x >= -4 {
        fixed(body, magnitude, (p as i64 - 1 - x) as usize);
    } else {
        body.extend_from_slice(&e_form);
    }
}

/// The exponent after the `e` of a `%e` form.
fn exponent(e_form: &[u8]) -> i64 {
    let at = e_form
        .iter()
        .position(|&c| c == b'e')
        .map_or(e_form.len(), |n| n + 1);
    let (negative, digits) = match e_form.get(at) {
        Some(b'-') => (true, &e_form[at + 1..]),
        Some(b'+') => (false, &e_form[at + 1..]),
        _ => (false, &e_form[at..]),
    };
    let magnitude = digits
        .iter()
        .fold(0i64, |v, &c| v * 10 + i64::from(c - b'0'));
    if negative { -magnitude } else { magnitude }
}

/// The UCRT's forced point (`#`): after the first character and the digits that follow it, unless
/// the text has a point already.
fn force_point(text: &mut Vec<u8>) {
    if text.contains(&b'.') {
        return;
    }
    let mut at = 1.min(text.len());
    while at < text.len() && text[at].is_ascii_digit() {
        at += 1;
    }
    text.insert(at, b'.');
}

/// `%e %E %f %F %g %G %a %A`.
fn write_float(
    out: &mut Vec<u8>,
    spec: &Spec,
    conversion: u8,
    value: f64,
) -> Result<(), FormatError> {
    let upper = conversion.is_ascii_uppercase();
    let kind = conversion.to_ascii_lowercase();
    let finite = value.is_finite();
    let negative = value.is_sign_negative();
    if spec.precision.is_some_and(|p| p > i32::MAX as usize) {
        return Err(FormatError::Overflow);
    }

    // The text the UCRT works on: the sign of a negative value, then the digits or the name.
    let mut text = Vec::new();
    if negative {
        text.push(b'-');
    }
    if finite {
        let magnitude = value.abs();
        match kind {
            b'f' => fixed(&mut text, magnitude, spec.precision.unwrap_or(6)),
            b'e' => scientific(&mut text, magnitude, spec.precision.unwrap_or(6)),
            b'g' => general(&mut text, magnitude, spec.precision, spec.alt),
            _ => hex(&mut text, magnitude, spec.precision),
        }
    } else {
        // inf, nan, nan(snan) or nan(ind), as to_chars spells them, without the sign.
        let name = to_chars_string(value);
        text.extend_from_slice(name.trim_start_matches('-').as_bytes());
    }
    if upper {
        text.make_ascii_uppercase();
    }

    // # always prints the point: a %g prints it where the value has one, and %e, %f and %a with
    // precision 0 print it after the first character, an infinity's or a NaN's included.
    if spec.alt {
        if kind != b'g' && spec.precision == Some(0) {
            force_point(&mut text);
        } else if kind == b'g' && finite {
            let (sign, rest) = text.split_at(usize::from(negative));
            let mut rest = rest.to_vec();
            force_point(&mut rest);
            text = [sign, &rest[..]].concat();
        }
    }

    // A leading '-' is the sign; what follows it is a number unless it starts with a letter (inf,
    // nan): a number gets %a's 0x and may be padded with zeros.
    let (mut prefix, body): (Vec<u8>, &[u8]) = match text.split_first() {
        Some((b'-', rest)) => (vec![b'-'], rest),
        _ if spec.plus => (vec![b'+'], &text[..]),
        _ if spec.space => (vec![b' '], &text[..]),
        _ => (Vec::new(), &text[..]),
    };
    let number = !body.first().is_some_and(u8::is_ascii_alphabetic);
    if number && kind == b'a' {
        prefix.extend_from_slice(if upper { b"0X" } else { b"0x" });
    }

    write_padded(
        out,
        spec.width,
        spec.left,
        spec.zero && number,
        &prefix,
        body,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(format: &str, args: &[Arg<'_>]) -> String {
        match sprintf_string(format, args) {
            Ok(s) => s,
            Err(e) => format!("<{e}>"),
        }
    }

    #[test]
    fn integers() {
        assert_eq!(
            p("%d|%i|%u", &[Arg::Int(-5), Arg::Int(42), Arg::Int(-1)]),
            "-5|42|4294967295"
        );
        assert_eq!(
            p(
                "%x|%X|%o|%u|%d",
                &[
                    Arg::Int(-1),
                    Arg::Int(0xABCDEF),
                    Arg::Int(8),
                    Arg::Int(-1),
                    Arg::Int(i64::from(i32::MIN))
                ]
            ),
            "ffffffff|ABCDEF|10|4294967295|-2147483648"
        );
        assert_eq!(
            p(
                "%hhd|%hhu|%hd|%hu",
                &[
                    Arg::Int(300),
                    Arg::Int(300),
                    Arg::Int(70000),
                    Arg::Int(70000)
                ]
            ),
            "44|44|4464|4464"
        );
        assert_eq!(
            p(
                "%lld|%llu|%I64x",
                &[Arg::Int(-1), Arg::Int(-1), Arg::Int(-5)]
            ),
            "-1|18446744073709551615|fffffffffffffffb"
        );
        assert_eq!(
            p("%hhx|%hhi", &[Arg::Int(0x1FF), Arg::Int(0x80)]),
            "ff|-128"
        );
        assert_eq!(
            p(
                "%#o|%#x|%#X|%#.0o|%.0d|%+.0d|% .0d|%#.0x|%5.0d|",
                &[Arg::Int(0); 9]
            ),
            "0|0|0|0||+| ||     |"
        );
        assert_eq!(
            p(
                "%5.3d|%-5.3d|%05.3d|%.3d|%.3d",
                &[7.into(), 7.into(), 7.into(), (-7).into(), 0.into()]
            ),
            "  007|007  |  007|-007|000"
        );
        assert_eq!(
            p(
                "%*d|%-*d|%.*d|%*5d|%.*5d",
                &[
                    (-5).into(),
                    1.into(),
                    5.into(),
                    2.into(),
                    (-1).into(),
                    4.into(),
                    3.into(),
                    6.into(),
                    3.into(),
                    1.into()
                ]
            ),
            "1    |2    |4|    6|00001"
        );
        assert_eq!(
            p(
                "%Ihd|%Ild|%IId",
                &[Arg::Int(70000), Arg::Int(-1), Arg::Int(-1)]
            ),
            "4464|-1|-1"
        );
    }

    #[test]
    fn characters_strings_pointers() {
        assert_eq!(
            p(
                "%c|%5c|%-5c|%05c",
                &[Arg::Int(120), Arg::Int(121), Arg::Int(122), Arg::Int(119)]
            ),
            "x|    y|z    |0000w"
        );
        assert_eq!(
            p(
                "%s|%.3s|%10s|%Z",
                &[Arg::Null, Arg::Null, Arg::Null, Arg::Null]
            ),
            "(null)|(nu|    (null)|(null)"
        );
        assert_eq!(
            p("%p|%20p|%-20p|%020p|%.3p|%#p", &[Arg::Ptr(0x1234); 6]),
            "0000000000001234|    0000000000001234|0000000000001234    |    0000000000001234|0000000000001234|0000000000001234"
        );
        assert_eq!(sprintf(b"%c", &[Arg::Int(0)]), Ok(vec![0]));
        let wide: &[u16] = &[0x48, 0xE9, 0];
        assert_eq!(
            sprintf(
                b"%ls|%S|%.1ws|%lc|%C|",
                &[
                    wide.into(),
                    wide.into(),
                    wide.into(),
                    Arg::Int(0xE9),
                    Arg::Int(0x100)
                ]
            ),
            Ok(b"H\xE9|H\xE9|H|\xE9||".to_vec())
        );
        let euro: &[u16] = &[0x20AC];
        assert_eq!(
            sprintf(b"%ls", &[euro.into()]),
            Err(FormatError::Unconvertible)
        );
        assert_eq!(
            sprintf(
                b"%hS|%hC|%Ls|%TS",
                &["x".into(), Arg::Int(65), "y".into(), "z".into()]
            ),
            Ok(b"x|A|y|z".to_vec())
        );
    }

    #[test]
    fn floats() {
        assert_eq!(
            p("%f|%e|%g|%a", &[Arg::Double(1.5); 4]),
            "1.500000|1.500000e+00|1.5|0x1.8000000000000p+0"
        );
        assert_eq!(
            p(
                "%#.0f|%#.0e|%#g|%#.3g|%#.0g|%#g|%#g|%#.0a",
                &[
                    1.0.into(),
                    1.0.into(),
                    1.0.into(),
                    1.0.into(),
                    1.0.into(),
                    100000.0.into(),
                    1e-5.into(),
                    (-1.0).into()
                ]
            ),
            "1.|1.e+00|1.00000|1.00|1.|100000.|1.00000e-05|-0x1.p+0"
        );
        assert_eq!(
            p(
                "%.0f|%.0f|%.0f|%.0f",
                &[0.5.into(), 1.5.into(), 2.5.into(), 3.5.into()]
            ),
            "0|2|2|4"
        );
        assert_eq!(
            p("%.17g|%.20e|%.30f", &[0.1.into(); 3]),
            "0.10000000000000001|1.00000000000000005551e-01|0.100000000000000005551115123126"
        );
        assert_eq!(
            p(
                "%010.4f|% 010.4f|%+010.4f",
                &[(-3.24159).into(), 3.24159.into(), 3.24159.into()]
            ),
            "-0003.2416| 0003.2416|+0003.2416"
        );
        assert_eq!(
            p(
                "%a|%.20a|%.12a",
                &[
                    5e-324.into(),
                    0.1.into(),
                    f64::from_bits(0x000F_FFFF_FFFF_FFFF).into()
                ]
            ),
            "0x0.0000000000001p-1022|0x1.999999999999a0000000p-4|0x1.000000000000p-1022"
        );
        assert_eq!(
            p("%.1a|%.0a", &[f64::MAX.into(), 1.5.into()]),
            "0x2.0p+1023|0x2p+0"
        );
    }

    #[test]
    fn non_finite() {
        let ind = f64::from_bits(0xFFF8_0000_0000_0000);
        let snan = f64::from_bits(0x7FF0_0000_0000_0001);
        let inf = f64::INFINITY;
        assert_eq!(
            p("%f|%F|%e|%G|%a|%A", &[Arg::Double(inf); 6]),
            "inf|INF|inf|INF|inf|INF"
        );
        assert_eq!(
            p("%f|%E|%g", &[Arg::Double(ind); 3]),
            "-nan(ind)|-NAN(IND)|-nan(ind)"
        );
        assert_eq!(
            p("%a|%A|%f", &[snan.into(), snan.into(), (-snan).into()]),
            "nan(snan)|NAN(SNAN)|-nan(snan)"
        );
        assert_eq!(
            p(
                "%010f|%-6f|%+f|% .3f|%#f",
                &[
                    (-inf).into(),
                    inf.into(),
                    inf.into(),
                    inf.into(),
                    inf.into()
                ]
            ),
            "      -inf|inf   |+inf| inf|inf"
        );
        // The UCRT's forced point on a name, and what it does to the padding and to %a's prefix.
        assert_eq!(
            p(
                "%#.0f|%#.0e|%#.0g|%#.0F",
                &[inf.into(), (-inf).into(), inf.into(), ind.into()]
            ),
            "i.nf|-.inf|inf|-.NAN(IND)"
        );
        assert_eq!(
            p(
                "%#012.0f|%#012.0f|%+#012.0a|%+#012.0a",
                &[(-inf).into(), inf.into(), (-inf).into(), inf.into()]
            ),
            "-0000000.inf|        i.nf|-0x00000.inf|       +i.nf"
        );
    }

    #[test]
    fn large_precisions() {
        let s = p("%.1100f", &[5e-324.into()]);
        assert_eq!(s.len(), 1102);
        assert!(s.starts_with("0.000"));
        assert!(s.ends_with(&"0".repeat(20)));
        let s = p("%.800e", &[0.1.into()]);
        assert!(s.ends_with("00000e-01"));
        assert_eq!(s.len(), 2 + 800 + 4);
        let s = p("%.20a|%.30a", &[1.5.into(), 1.5.into()]);
        assert_eq!(
            s,
            format!("0x1.8{}p+0|0x1.8{}p+0", "0".repeat(19), "0".repeat(29))
        );
    }

    #[test]
    fn standard_and_secure_modes() {
        let both = |f: &[u8], args: &[Arg<'_>]| (sprintf(f, args), sprintf_s(f, args));
        assert_eq!(
            both(b"%5-d", &[Arg::Int(1)]),
            (Ok(b"-d".to_vec()), Err(FormatError::Invalid))
        );
        assert_eq!(
            both(b"%y|%", &[]),
            (Ok(b"y|".to_vec()), Err(FormatError::Invalid))
        );
        assert_eq!(
            both(b"%5%|%-%|%l%", &[]),
            (Ok(b"%|%|%".to_vec()), Err(FormatError::Invalid))
        );
        assert_eq!(
            both(b"%I6d|%1$d", &[Arg::Int(1)]),
            (Ok(b"6d|$d".to_vec()), Err(FormatError::Invalid))
        );
        assert_eq!(
            both(b"%%|%d", &[Arg::Int(1)]),
            (Ok(b"%|1".to_vec()), Ok(b"%|1".to_vec()))
        );
        for refused in [
            &b"%n"[..],
            b"%Ld",
            b"%wd",
            b"%Td",
            b"%lhd",
            b"%lll",
            b"%hhhd",
            b"%I64I64d",
            b"%jld",
        ] {
            assert_eq!(
                both(refused, &[Arg::Int(1)]),
                (Err(FormatError::Invalid), Err(FormatError::Invalid)),
                "{refused:?}"
            );
        }
        assert_eq!(
            sprintf(b"%2147483648d", &[Arg::Int(1)]),
            Err(FormatError::Overflow)
        );
        assert_eq!(
            sprintf(b"%*d", &[Arg::Int(i64::from(i32::MIN)), Arg::Int(1)]),
            Err(FormatError::Overflow)
        );
        // An INT_MIN width does not pad an empty field, and %% takes no padding.
        assert_eq!(
            sprintf(
                b"[%*s|%*%]",
                &[
                    Arg::Int(i64::from(i32::MIN)),
                    "".into(),
                    Arg::Int(i64::from(i32::MIN))
                ]
            ),
            Ok(b"[|%]".to_vec())
        );
    }

    #[test]
    fn errors_and_snprintf() {
        assert_eq!(sprintf(b"%d", &[]), Err(FormatError::MissingArgument));
        assert_eq!(
            sprintf(b"%d", &["x".into()]),
            Err(FormatError::ArgumentType)
        );
        assert_eq!(
            sprintf(b"%s", &[Arg::Int(1)]),
            Err(FormatError::ArgumentType)
        );
        assert_eq!(
            sprintf(
                b"%d|%f",
                &[Arg::Double(1.0), Arg::UInt(0x3FF8_0000_0000_0000)]
            ),
            Ok(b"0|1.500000".to_vec())
        );
        assert_eq!(sprintf(b"ab\0%d", &[]), Ok(b"ab".to_vec()));

        let mut buf = [0xAAu8; 8];
        assert_eq!(snprintf(&mut buf, b"0123456789", &[]), Ok(10));
        assert_eq!(&buf, b"0123456\0");
        let mut empty = [0u8; 0];
        assert_eq!(snprintf(&mut empty, b"%d", &[Arg::Int(12345)]), Ok(5));
    }
}
