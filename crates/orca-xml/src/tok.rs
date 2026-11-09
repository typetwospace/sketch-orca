// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Expat code: the tokenizer of xmltok_impl.inc and the encodings, conversions, encoding
// detection and XML declaration parser of xmltok.c and xmltok_ns.inc (Expat 2.5.0, as OrcaSlicer
// v2.4.2's dependency build compiles it from deps/EXPAT/expat: no XML_DTD, no XML_NS, no
// XML_MIN_SIZE, XML_Char is char), translated on 2026-11-08. Copyright (c) 1998-2000 Thai Open
// Source Software Center Ltd and Clark Cooper and Copyright (c) 2001-2022 Expat maintainers, under
// the MIT licence (licenses/MIT-expat.txt).

//! The tokenizer: `xmltok_impl.inc` once for each kind of encoding, as the C includes it three
//! times (`normal_`, `little2_`, `big2_`).
//!
//! The C's `ENCODING` vtables are [`Enc`]; the macros each inclusion defines (`MINBPC`,
//! `BYTE_TYPE`, `BYTE_TO_ASCII`, `CHAR_MATCHES`, `IS_NAME_CHAR`, `IS_NMSTRT_CHAR`,
//! `IS_INVALID_CHAR` and the `_MINBPC` forms) are the [`Scan`] trait, and each tokenizer function
//! is generic over it, monomorphized as the C's macros are. A pointer is an index into the bytes
//! scanned; `next` is the C's `nextTokPtr`, written only where the C writes it.
//!
//! The encodings are the six of the encoding table (`latin1`, `ascii`, `utf8`, `big2` and
//! `little2`), the internal `internal_utf8` that entity text is scanned in, and the initial
//! encoding that detects the document's from its first bytes (`initScan`, in the prolog only: a
//! parser made by `XML_ParserCreate(NULL)` never scans content with it). Without an
//! unknown-encoding handler no other encoding exists.

use crate::tables::*;

// Tokens (`XML_TOK_*` of xmltok.h).
pub(crate) const TOK_TRAILING_RSQB: i32 = -5;
pub(crate) const TOK_NONE: i32 = -4;
pub(crate) const TOK_TRAILING_CR: i32 = -3;
pub(crate) const TOK_PARTIAL_CHAR: i32 = -2;
pub(crate) const TOK_PARTIAL: i32 = -1;
pub(crate) const TOK_INVALID: i32 = 0;
pub(crate) const TOK_START_TAG_WITH_ATTS: i32 = 1;
pub(crate) const TOK_START_TAG_NO_ATTS: i32 = 2;
pub(crate) const TOK_EMPTY_ELEMENT_WITH_ATTS: i32 = 3;
pub(crate) const TOK_EMPTY_ELEMENT_NO_ATTS: i32 = 4;
pub(crate) const TOK_END_TAG: i32 = 5;
pub(crate) const TOK_DATA_CHARS: i32 = 6;
pub(crate) const TOK_DATA_NEWLINE: i32 = 7;
pub(crate) const TOK_CDATA_SECT_OPEN: i32 = 8;
pub(crate) const TOK_ENTITY_REF: i32 = 9;
pub(crate) const TOK_CHAR_REF: i32 = 10;
pub(crate) const TOK_PI: i32 = 11;
pub(crate) const TOK_XML_DECL: i32 = 12;
pub(crate) const TOK_COMMENT: i32 = 13;
pub(crate) const TOK_BOM: i32 = 14;
pub(crate) const TOK_PROLOG_S: i32 = 15;
pub(crate) const TOK_DECL_OPEN: i32 = 16;
pub(crate) const TOK_DECL_CLOSE: i32 = 17;
pub(crate) const TOK_NAME: i32 = 18;
pub(crate) const TOK_NMTOKEN: i32 = 19;
pub(crate) const TOK_POUND_NAME: i32 = 20;
pub(crate) const TOK_OR: i32 = 21;
pub(crate) const TOK_PERCENT: i32 = 22;
pub(crate) const TOK_OPEN_PAREN: i32 = 23;
pub(crate) const TOK_CLOSE_PAREN: i32 = 24;
pub(crate) const TOK_OPEN_BRACKET: i32 = 25;
pub(crate) const TOK_CLOSE_BRACKET: i32 = 26;
pub(crate) const TOK_LITERAL: i32 = 27;
pub(crate) const TOK_PARAM_ENTITY_REF: i32 = 28;
pub(crate) const TOK_INSTANCE_START: i32 = 29;
pub(crate) const TOK_NAME_QUESTION: i32 = 30;
pub(crate) const TOK_NAME_ASTERISK: i32 = 31;
pub(crate) const TOK_NAME_PLUS: i32 = 32;
pub(crate) const TOK_COND_SECT_OPEN: i32 = 33;
pub(crate) const TOK_COND_SECT_CLOSE: i32 = 34;
pub(crate) const TOK_CLOSE_PAREN_QUESTION: i32 = 35;
pub(crate) const TOK_CLOSE_PAREN_ASTERISK: i32 = 36;
pub(crate) const TOK_CLOSE_PAREN_PLUS: i32 = 37;
pub(crate) const TOK_COMMA: i32 = 38;
pub(crate) const TOK_ATTRIBUTE_VALUE_S: i32 = 39;
pub(crate) const TOK_CDATA_SECT_CLOSE: i32 = 40;

/// `enum XML_Convert_Result`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Convert {
    Completed,
    InputIncomplete,
    OutputExhausted,
}

/// `POSITION`: a line and a column counted from zero, as `XML_Size` (`unsigned long`, 32 bits on
/// Windows) counts them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Position {
    /// `lineNumber`.
    pub line: u32,
    /// `columnNumber`.
    pub column: u32,
}

/// An `ATTRIBUTE` that `getAtts` fills: indices of the name and of the value's first byte and
/// end, and whether the value needs no normalizing.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Att {
    pub name: usize,
    pub value_ptr: usize,
    pub value_end: usize,
    pub normalized: bool,
}

/// What one inclusion of `xmltok_impl.inc` is compiled with.
pub(crate) trait Scan {
    const MINBPC: usize;
    fn byte_type(&self, b: &[u8], p: usize) -> u8;
    fn byte_to_ascii(b: &[u8], p: usize) -> i32;
    fn char_matches(b: &[u8], p: usize, c: u8) -> bool;
    fn is_name_char(&self, b: &[u8], p: usize, n: usize) -> bool;
    fn is_nmstrt_char(&self, b: &[u8], p: usize, n: usize) -> bool;
    fn is_invalid_char(&self, b: &[u8], p: usize, n: usize) -> bool;
    fn is_name_char_minbpc(b: &[u8], p: usize) -> bool;
    fn is_nmstrt_char_minbpc(b: &[u8], p: usize) -> bool;
}

fn ucs2_get_naming(pages: &[u8; 256], hi: u8, lo: u8) -> bool {
    NAMING_BITMAP[((pages[hi as usize] as usize) << 3) + ((lo as usize) >> 5)]
        & (1u32 << (lo & 0x1F))
        != 0
}

fn utf8_get_naming2(pages: &[u8; 256], p: &[u8]) -> bool {
    let i = ((pages[((p[0] >> 2) & 7) as usize] as usize) << 3)
        + (((p[0] & 3) as usize) << 1)
        + (((p[1] >> 5) & 1) as usize);
    NAMING_BITMAP[i] & (1u32 << (p[1] & 0x1F)) != 0
}

fn utf8_get_naming3(pages: &[u8; 256], p: &[u8]) -> bool {
    let i = ((pages[(((p[0] & 0xF) as usize) << 4) + (((p[1] >> 2) & 0xF) as usize)] as usize)
        << 3)
        + (((p[1] & 3) as usize) << 1)
        + (((p[2] >> 5) & 1) as usize);
    NAMING_BITMAP[i] & (1u32 << (p[2] & 0x1F)) != 0
}

fn utf8_invalid2(p: &[u8]) -> bool {
    p[0] < 0xC2 || (p[1] & 0x80) == 0 || (p[1] & 0xC0) == 0xC0
}

fn utf8_invalid3(p: &[u8]) -> bool {
    (p[2] & 0x80) == 0
        || (if p[0] == 0xEF && p[1] == 0xBF {
            p[2] > 0xBD
        } else {
            (p[2] & 0xC0) == 0xC0
        })
        || (if p[0] == 0xE0 {
            p[1] < 0xA0 || (p[1] & 0xC0) == 0xC0
        } else {
            (p[1] & 0x80) == 0
                || (if p[0] == 0xED {
                    p[1] > 0x9F
                } else {
                    (p[1] & 0xC0) == 0xC0
                })
        })
}

fn utf8_invalid4(p: &[u8]) -> bool {
    (p[3] & 0x80) == 0
        || (p[3] & 0xC0) == 0xC0
        || (p[2] & 0x80) == 0
        || (p[2] & 0xC0) == 0xC0
        || (if p[0] == 0xF0 {
            p[1] < 0x90 || (p[1] & 0xC0) == 0xC0
        } else {
            (p[1] & 0x80) == 0
                || (if p[0] == 0xF4 {
                    p[1] > 0x8F
                } else {
                    (p[1] & 0xC0) == 0xC0
                })
        })
}

/// `struct normal_encoding` with one byte a character: its byte types, and whether its
/// `isName2`.. functions are UTF-8's (`utf8` and `internal_utf8`) or NULL (`latin1` and `ascii`,
/// whose tables hold no lead bytes, so the C never calls them).
pub(crate) struct Normal {
    tab: [u8; 256],
    utf8_naming: bool,
}

impl Scan for Normal {
    const MINBPC: usize = 1;
    fn byte_type(&self, b: &[u8], p: usize) -> u8 {
        self.tab[b[p] as usize]
    }
    fn byte_to_ascii(b: &[u8], p: usize) -> i32 {
        // `*(p)`: a `char`, signed on MSVC.
        b[p] as i8 as i32
    }
    fn char_matches(b: &[u8], p: usize, c: u8) -> bool {
        b[p] == c
    }
    fn is_name_char(&self, b: &[u8], p: usize, n: usize) -> bool {
        self.utf8_naming
            && match n {
                2 => utf8_get_naming2(&NAME_PAGES, &b[p..]),
                3 => utf8_get_naming3(&NAME_PAGES, &b[p..]),
                _ => false,
            }
    }
    fn is_nmstrt_char(&self, b: &[u8], p: usize, n: usize) -> bool {
        self.utf8_naming
            && match n {
                2 => utf8_get_naming2(&NMSTRT_PAGES, &b[p..]),
                3 => utf8_get_naming3(&NMSTRT_PAGES, &b[p..]),
                _ => false,
            }
    }
    fn is_invalid_char(&self, b: &[u8], p: usize, n: usize) -> bool {
        self.utf8_naming
            && match n {
                2 => utf8_invalid2(&b[p..]),
                3 => utf8_invalid3(&b[p..]),
                _ => utf8_invalid4(&b[p..]),
            }
    }
    fn is_name_char_minbpc(_: &[u8], _: usize) -> bool {
        false
    }
    fn is_nmstrt_char_minbpc(_: &[u8], _: usize) -> bool {
        false
    }
}

/// `unicode_byte_type`.
fn unicode_byte_type(hi: u8, lo: u8) -> u8 {
    match hi {
        0xD8..=0xDB => BT_LEAD4,
        0xDC..=0xDF => BT_TRAIL,
        0xFF if lo == 0xFF || lo == 0xFE => BT_NONXML,
        _ => BT_NONASCII,
    }
}

/// UTF-16, little-endian (`little2_`) when `BIG` is false, big-endian (`big2_`) when true. Both
/// classify the low 256 characters with `asciitab.h` and `latin1tab.h`.
pub(crate) struct Utf16<const BIG: bool>;

impl<const BIG: bool> Utf16<BIG> {
    fn hi(b: &[u8], p: usize) -> u8 {
        if BIG { b[p] } else { b[p + 1] }
    }
    fn lo(b: &[u8], p: usize) -> u8 {
        if BIG { b[p + 1] } else { b[p] }
    }
}

impl<const BIG: bool> Scan for Utf16<BIG> {
    const MINBPC: usize = 2;
    fn byte_type(&self, b: &[u8], p: usize) -> u8 {
        let (hi, lo) = (Self::hi(b, p), Self::lo(b, p));
        if hi == 0 {
            latin1_type(lo)
        } else {
            unicode_byte_type(hi, lo)
        }
    }
    fn byte_to_ascii(b: &[u8], p: usize) -> i32 {
        if Self::hi(b, p) == 0 {
            Self::lo(b, p) as i8 as i32
        } else {
            -1
        }
    }
    fn char_matches(b: &[u8], p: usize, c: u8) -> bool {
        Self::hi(b, p) == 0 && Self::lo(b, p) == c
    }
    fn is_name_char(&self, _: &[u8], _: usize, _: usize) -> bool {
        false
    }
    fn is_nmstrt_char(&self, _: &[u8], _: usize, _: usize) -> bool {
        false
    }
    fn is_invalid_char(&self, _: &[u8], _: usize, _: usize) -> bool {
        false
    }
    fn is_name_char_minbpc(b: &[u8], p: usize) -> bool {
        ucs2_get_naming(&NAME_PAGES, Self::hi(b, p), Self::lo(b, p))
    }
    fn is_nmstrt_char_minbpc(b: &[u8], p: usize) -> bool {
        ucs2_get_naming(&NMSTRT_PAGES, Self::hi(b, p), Self::lo(b, p))
    }
}

/// `latin1_encoding.type[c]`: `asciitab.h` then `latin1tab.h`.
pub(crate) fn latin1_type(c: u8) -> u8 {
    if c < 0x80 {
        ASCII_TAB[c as usize]
    } else {
        LATIN1_TAB[(c - 0x80) as usize]
    }
}

const fn table(low: &[u8; 128], high: Option<&[u8; 128]>) -> [u8; 256] {
    let mut t = [BT_NONXML; 256];
    let mut i = 0;
    while i < 128 {
        t[i] = low[i];
        if let Some(h) = high {
            t[128 + i] = h[i];
        }
        i += 1;
    }
    t
}

static UTF8: Normal = Normal {
    tab: table(&ASCII_TAB, Some(&UTF8_TAB)),
    utf8_naming: true,
};
static INTERNAL_UTF8: Normal = Normal {
    tab: table(&IASCII_TAB, Some(&UTF8_TAB)),
    utf8_naming: true,
};
static LATIN1: Normal = Normal {
    tab: table(&ASCII_TAB, Some(&LATIN1_TAB)),
    utf8_naming: false,
};
// `ascii_encoding`'s high half is zero, `BT_NONXML`.
static ASCII: Normal = Normal {
    tab: table(&ASCII_TAB, None),
    utf8_naming: false,
};

// ---------------------------------------------------------------------------------------------
// xmltok_impl.inc

#[inline]
fn has_chars<S: Scan>(ptr: usize, end: usize, count: usize) -> bool {
    end >= ptr + count * S::MINBPC
}

#[inline]
fn has_char<S: Scan>(ptr: usize, end: usize) -> bool {
    has_chars::<S>(ptr, end, 1)
}

/// What a group of `case` labels made by one of the C's macros did with a byte type.
enum Case {
    /// The type is not one of the macro's labels.
    No,
    /// The labels advanced `ptr` and broke out of the `switch`.
    Go,
    /// The labels returned this token.
    Ret(i32),
}

fn lead_n(bt: u8) -> usize {
    match bt {
        BT_LEAD2 => 2,
        BT_LEAD3 => 3,
        _ => 4,
    }
}

/// `INVALID_CASES(ptr, nextTokPtr)`.
#[inline]
fn invalid_cases<S: Scan>(
    s: &S,
    b: &[u8],
    bt: u8,
    ptr: &mut usize,
    end: usize,
    next: &mut usize,
) -> Case {
    match bt {
        BT_LEAD2 | BT_LEAD3 | BT_LEAD4 => {
            let n = lead_n(bt);
            if end < *ptr + n {
                return Case::Ret(TOK_PARTIAL_CHAR);
            }
            if s.is_invalid_char(b, *ptr, n) {
                *next = *ptr;
                return Case::Ret(TOK_INVALID);
            }
            *ptr += n;
            Case::Go
        }
        BT_NONXML | BT_MALFORM | BT_TRAIL => {
            *next = *ptr;
            Case::Ret(TOK_INVALID)
        }
        _ => Case::No,
    }
}

/// `CHECK_NAME_CASES(enc, ptr, end, nextTokPtr)`.
#[inline]
fn check_name_cases<S: Scan>(
    s: &S,
    b: &[u8],
    bt: u8,
    ptr: &mut usize,
    end: usize,
    next: &mut usize,
) -> Case {
    match bt {
        BT_NONASCII => {
            if !S::is_name_char_minbpc(b, *ptr) {
                *next = *ptr;
                return Case::Ret(TOK_INVALID);
            }
            *ptr += S::MINBPC;
            Case::Go
        }
        BT_NMSTRT | BT_HEX | BT_DIGIT | BT_NAME | BT_MINUS => {
            *ptr += S::MINBPC;
            Case::Go
        }
        BT_LEAD2 | BT_LEAD3 | BT_LEAD4 => {
            let n = lead_n(bt);
            if end < *ptr + n {
                return Case::Ret(TOK_PARTIAL_CHAR);
            }
            if s.is_invalid_char(b, *ptr, n) || !s.is_name_char(b, *ptr, n) {
                *next = *ptr;
                return Case::Ret(TOK_INVALID);
            }
            *ptr += n;
            Case::Go
        }
        _ => Case::No,
    }
}

/// `CHECK_NMSTRT_CASES(enc, ptr, end, nextTokPtr)`.
#[inline]
fn check_nmstrt_cases<S: Scan>(
    s: &S,
    b: &[u8],
    bt: u8,
    ptr: &mut usize,
    end: usize,
    next: &mut usize,
) -> Case {
    match bt {
        BT_NONASCII => {
            if !S::is_nmstrt_char_minbpc(b, *ptr) {
                *next = *ptr;
                return Case::Ret(TOK_INVALID);
            }
            *ptr += S::MINBPC;
            Case::Go
        }
        BT_NMSTRT | BT_HEX => {
            *ptr += S::MINBPC;
            Case::Go
        }
        BT_LEAD2 | BT_LEAD3 | BT_LEAD4 => {
            let n = lead_n(bt);
            if end < *ptr + n {
                return Case::Ret(TOK_PARTIAL_CHAR);
            }
            if s.is_invalid_char(b, *ptr, n) || !s.is_nmstrt_char(b, *ptr, n) {
                *next = *ptr;
                return Case::Ret(TOK_INVALID);
            }
            *ptr += n;
            Case::Go
        }
        _ => Case::No,
    }
}

macro_rules! case {
    ($e:expr) => {
        match $e {
            Case::No => false,
            Case::Go => true,
            Case::Ret(t) => return t,
        }
    };
}

fn scan_comment<S: Scan>(s: &S, b: &[u8], mut ptr: usize, end: usize, next: &mut usize) -> i32 {
    if has_char::<S>(ptr, end) {
        if !S::char_matches(b, ptr, b'-') {
            *next = ptr;
            return TOK_INVALID;
        }
        ptr += S::MINBPC;
        while has_char::<S>(ptr, end) {
            let bt = s.byte_type(b, ptr);
            if case!(invalid_cases(s, b, bt, &mut ptr, end, next)) {
                continue;
            }
            if bt == BT_MINUS {
                ptr += S::MINBPC;
                if !has_char::<S>(ptr, end) {
                    return TOK_PARTIAL;
                }
                if S::char_matches(b, ptr, b'-') {
                    ptr += S::MINBPC;
                    if !has_char::<S>(ptr, end) {
                        return TOK_PARTIAL;
                    }
                    if !S::char_matches(b, ptr, b'>') {
                        *next = ptr;
                        return TOK_INVALID;
                    }
                    *next = ptr + S::MINBPC;
                    return TOK_COMMENT;
                }
            } else {
                ptr += S::MINBPC;
            }
        }
    }
    TOK_PARTIAL
}

fn scan_decl<S: Scan>(s: &S, b: &[u8], mut ptr: usize, end: usize, next: &mut usize) -> i32 {
    if !has_char::<S>(ptr, end) {
        return TOK_PARTIAL;
    }
    match s.byte_type(b, ptr) {
        BT_MINUS => return scan_comment(s, b, ptr + S::MINBPC, end, next),
        BT_LSQB => {
            *next = ptr + S::MINBPC;
            return TOK_COND_SECT_OPEN;
        }
        BT_NMSTRT | BT_HEX => ptr += S::MINBPC,
        _ => {
            *next = ptr;
            return TOK_INVALID;
        }
    }
    while has_char::<S>(ptr, end) {
        match s.byte_type(b, ptr) {
            BT_PERCNT => {
                if !has_chars::<S>(ptr, end, 2) {
                    return TOK_PARTIAL;
                }
                // The C falls through to the white-space labels when no white space follows.
                match s.byte_type(b, ptr + S::MINBPC) {
                    BT_S | BT_CR | BT_LF | BT_PERCNT => {
                        *next = ptr;
                        return TOK_INVALID;
                    }
                    _ => {}
                }
                *next = ptr;
                return TOK_DECL_OPEN;
            }
            BT_S | BT_CR | BT_LF => {
                *next = ptr;
                return TOK_DECL_OPEN;
            }
            BT_NMSTRT | BT_HEX => ptr += S::MINBPC,
            _ => {
                *next = ptr;
                return TOK_INVALID;
            }
        }
    }
    TOK_PARTIAL
}

/// `checkPiTarget`: false when the target is `xml` in some mixed case (`XML`, `Xml`..); `tok`
/// becomes `XML_TOK_XML_DECL` for `xml` itself and `XML_TOK_PI` otherwise.
fn check_pi_target<S: Scan>(b: &[u8], mut ptr: usize, end: usize, tok: &mut i32) -> bool {
    let mut upper = false;
    *tok = TOK_PI;
    if end - ptr != S::MINBPC * 3 {
        return true;
    }
    match S::byte_to_ascii(b, ptr) {
        0x78 => {}
        0x58 => upper = true,
        _ => return true,
    }
    ptr += S::MINBPC;
    match S::byte_to_ascii(b, ptr) {
        0x6D => {}
        0x4D => upper = true,
        _ => return true,
    }
    ptr += S::MINBPC;
    match S::byte_to_ascii(b, ptr) {
        0x6C => {}
        0x4C => upper = true,
        _ => return true,
    }
    if upper {
        return false;
    }
    *tok = TOK_XML_DECL;
    true
}

fn scan_pi<S: Scan>(s: &S, b: &[u8], mut ptr: usize, end: usize, next: &mut usize) -> i32 {
    let mut tok = 0;
    let target = ptr;
    if !has_char::<S>(ptr, end) {
        return TOK_PARTIAL;
    }
    let bt = s.byte_type(b, ptr);
    if !case!(check_nmstrt_cases(s, b, bt, &mut ptr, end, next)) {
        *next = ptr;
        return TOK_INVALID;
    }
    while has_char::<S>(ptr, end) {
        let bt = s.byte_type(b, ptr);
        if case!(check_name_cases(s, b, bt, &mut ptr, end, next)) {
            continue;
        }
        match bt {
            BT_S | BT_CR | BT_LF => {
                if !check_pi_target::<S>(b, target, ptr, &mut tok) {
                    *next = ptr;
                    return TOK_INVALID;
                }
                ptr += S::MINBPC;
                while has_char::<S>(ptr, end) {
                    let bt = s.byte_type(b, ptr);
                    if case!(invalid_cases(s, b, bt, &mut ptr, end, next)) {
                        continue;
                    }
                    if bt == BT_QUEST {
                        ptr += S::MINBPC;
                        if !has_char::<S>(ptr, end) {
                            return TOK_PARTIAL;
                        }
                        if S::char_matches(b, ptr, b'>') {
                            *next = ptr + S::MINBPC;
                            return tok;
                        }
                    } else {
                        ptr += S::MINBPC;
                    }
                }
                return TOK_PARTIAL;
            }
            BT_QUEST => {
                if !check_pi_target::<S>(b, target, ptr, &mut tok) {
                    *next = ptr;
                    return TOK_INVALID;
                }
                ptr += S::MINBPC;
                if !has_char::<S>(ptr, end) {
                    return TOK_PARTIAL;
                }
                if S::char_matches(b, ptr, b'>') {
                    *next = ptr + S::MINBPC;
                    return tok;
                }
                // Falls through to `default`.
                *next = ptr;
                return TOK_INVALID;
            }
            _ => {
                *next = ptr;
                return TOK_INVALID;
            }
        }
    }
    TOK_PARTIAL
}

fn scan_cdata_section<S: Scan>(b: &[u8], mut ptr: usize, end: usize, next: &mut usize) -> i32 {
    const CDATA_LSQB: [u8; 6] = *b"CDATA[";
    if !has_chars::<S>(ptr, end, 6) {
        return TOK_PARTIAL;
    }
    for c in CDATA_LSQB {
        if !S::char_matches(b, ptr, c) {
            *next = ptr;
            return TOK_INVALID;
        }
        ptr += S::MINBPC;
    }
    *next = ptr;
    TOK_CDATA_SECT_OPEN
}

/// Trims `end` to whole characters of `MINBPC` bytes, as the three entry points do; `None` is
/// `XML_TOK_PARTIAL` (less than one character).
fn trim_end<S: Scan>(ptr: usize, end: usize) -> Option<usize> {
    if S::MINBPC > 1 {
        let mut n = end - ptr;
        if n & (S::MINBPC - 1) != 0 {
            n &= !(S::MINBPC - 1);
            if n == 0 {
                return None;
            }
            return Some(ptr + n);
        }
    }
    Some(end)
}

fn cdata_section_tok<S: Scan>(
    s: &S,
    b: &[u8],
    mut ptr: usize,
    end: usize,
    next: &mut usize,
) -> i32 {
    if ptr >= end {
        return TOK_NONE;
    }
    let Some(end) = trim_end::<S>(ptr, end) else {
        return TOK_PARTIAL;
    };
    let bt = s.byte_type(b, ptr);
    match bt {
        BT_RSQB => 'rsqb: {
            ptr += S::MINBPC;
            if !has_char::<S>(ptr, end) {
                return TOK_PARTIAL;
            }
            if !S::char_matches(b, ptr, b']') {
                break 'rsqb;
            }
            ptr += S::MINBPC;
            if !has_char::<S>(ptr, end) {
                return TOK_PARTIAL;
            }
            if !S::char_matches(b, ptr, b'>') {
                ptr -= S::MINBPC;
                break 'rsqb;
            }
            *next = ptr + S::MINBPC;
            return TOK_CDATA_SECT_CLOSE;
        }
        BT_CR => {
            ptr += S::MINBPC;
            if !has_char::<S>(ptr, end) {
                return TOK_PARTIAL;
            }
            if s.byte_type(b, ptr) == BT_LF {
                ptr += S::MINBPC;
            }
            *next = ptr;
            return TOK_DATA_NEWLINE;
        }
        BT_LF => {
            *next = ptr + S::MINBPC;
            return TOK_DATA_NEWLINE;
        }
        _ => {
            if !case!(invalid_cases(s, b, bt, &mut ptr, end, next)) {
                ptr += S::MINBPC;
            }
        }
    }
    while has_char::<S>(ptr, end) {
        let bt = s.byte_type(b, ptr);
        match bt {
            BT_LEAD2 | BT_LEAD3 | BT_LEAD4 => {
                let n = lead_n(bt);
                if end < ptr + n || s.is_invalid_char(b, ptr, n) {
                    *next = ptr;
                    return TOK_DATA_CHARS;
                }
                ptr += n;
            }
            BT_NONXML | BT_MALFORM | BT_TRAIL | BT_CR | BT_LF | BT_RSQB => {
                *next = ptr;
                return TOK_DATA_CHARS;
            }
            _ => ptr += S::MINBPC,
        }
    }
    *next = ptr;
    TOK_DATA_CHARS
}

fn scan_end_tag<S: Scan>(s: &S, b: &[u8], mut ptr: usize, end: usize, next: &mut usize) -> i32 {
    if !has_char::<S>(ptr, end) {
        return TOK_PARTIAL;
    }
    let bt = s.byte_type(b, ptr);
    if !case!(check_nmstrt_cases(s, b, bt, &mut ptr, end, next)) {
        *next = ptr;
        return TOK_INVALID;
    }
    while has_char::<S>(ptr, end) {
        let bt = s.byte_type(b, ptr);
        if case!(check_name_cases(s, b, bt, &mut ptr, end, next)) {
            continue;
        }
        match bt {
            BT_S | BT_CR | BT_LF => {
                ptr += S::MINBPC;
                while has_char::<S>(ptr, end) {
                    match s.byte_type(b, ptr) {
                        BT_S | BT_CR | BT_LF => {}
                        BT_GT => {
                            *next = ptr + S::MINBPC;
                            return TOK_END_TAG;
                        }
                        _ => {
                            *next = ptr;
                            return TOK_INVALID;
                        }
                    }
                    ptr += S::MINBPC;
                }
                return TOK_PARTIAL;
            }
            BT_GT => {
                *next = ptr + S::MINBPC;
                return TOK_END_TAG;
            }
            _ => {
                *next = ptr;
                return TOK_INVALID;
            }
        }
    }
    TOK_PARTIAL
}

fn scan_hex_char_ref<S: Scan>(
    s: &S,
    b: &[u8],
    mut ptr: usize,
    end: usize,
    next: &mut usize,
) -> i32 {
    if has_char::<S>(ptr, end) {
        match s.byte_type(b, ptr) {
            BT_DIGIT | BT_HEX => {}
            _ => {
                *next = ptr;
                return TOK_INVALID;
            }
        }
        ptr += S::MINBPC;
        while has_char::<S>(ptr, end) {
            match s.byte_type(b, ptr) {
                BT_DIGIT | BT_HEX => {}
                BT_SEMI => {
                    *next = ptr + S::MINBPC;
                    return TOK_CHAR_REF;
                }
                _ => {
                    *next = ptr;
                    return TOK_INVALID;
                }
            }
            ptr += S::MINBPC;
        }
    }
    TOK_PARTIAL
}

fn scan_char_ref<S: Scan>(s: &S, b: &[u8], mut ptr: usize, end: usize, next: &mut usize) -> i32 {
    if has_char::<S>(ptr, end) {
        if S::char_matches(b, ptr, b'x') {
            return scan_hex_char_ref(s, b, ptr + S::MINBPC, end, next);
        }
        if s.byte_type(b, ptr) != BT_DIGIT {
            *next = ptr;
            return TOK_INVALID;
        }
        ptr += S::MINBPC;
        while has_char::<S>(ptr, end) {
            match s.byte_type(b, ptr) {
                BT_DIGIT => {}
                BT_SEMI => {
                    *next = ptr + S::MINBPC;
                    return TOK_CHAR_REF;
                }
                _ => {
                    *next = ptr;
                    return TOK_INVALID;
                }
            }
            ptr += S::MINBPC;
        }
    }
    TOK_PARTIAL
}

fn scan_ref<S: Scan>(s: &S, b: &[u8], mut ptr: usize, end: usize, next: &mut usize) -> i32 {
    if !has_char::<S>(ptr, end) {
        return TOK_PARTIAL;
    }
    let bt = s.byte_type(b, ptr);
    if !case!(check_nmstrt_cases(s, b, bt, &mut ptr, end, next)) {
        if bt == BT_NUM {
            return scan_char_ref(s, b, ptr + S::MINBPC, end, next);
        }
        *next = ptr;
        return TOK_INVALID;
    }
    while has_char::<S>(ptr, end) {
        let bt = s.byte_type(b, ptr);
        if case!(check_name_cases(s, b, bt, &mut ptr, end, next)) {
            continue;
        }
        if bt == BT_SEMI {
            *next = ptr + S::MINBPC;
            return TOK_ENTITY_REF;
        }
        *next = ptr;
        return TOK_INVALID;
    }
    TOK_PARTIAL
}

/// The tail the C reaches through its `sol` label: `/` then `>`.
fn empty_close<S: Scan>(b: &[u8], mut ptr: usize, end: usize, next: &mut usize, tok: i32) -> i32 {
    ptr += S::MINBPC;
    if !has_char::<S>(ptr, end) {
        return TOK_PARTIAL;
    }
    if !S::char_matches(b, ptr, b'>') {
        *next = ptr;
        return TOK_INVALID;
    }
    *next = ptr + S::MINBPC;
    tok
}

fn scan_atts<S: Scan>(s: &S, b: &[u8], mut ptr: usize, end: usize, next: &mut usize) -> i32 {
    while has_char::<S>(ptr, end) {
        let bt = s.byte_type(b, ptr);
        if case!(check_name_cases(s, b, bt, &mut ptr, end, next)) {
            continue;
        }
        match bt {
            BT_S | BT_CR | BT_LF | BT_EQUALS => {}
            _ => {
                *next = ptr;
                return TOK_INVALID;
            }
        }
        if bt != BT_EQUALS {
            loop {
                ptr += S::MINBPC;
                if !has_char::<S>(ptr, end) {
                    return TOK_PARTIAL;
                }
                let t = s.byte_type(b, ptr);
                if t == BT_EQUALS {
                    break;
                }
                match t {
                    BT_S | BT_LF | BT_CR => {}
                    _ => {
                        *next = ptr;
                        return TOK_INVALID;
                    }
                }
            }
        }
        // case BT_EQUALS:
        let open;
        loop {
            ptr += S::MINBPC;
            if !has_char::<S>(ptr, end) {
                return TOK_PARTIAL;
            }
            let o = s.byte_type(b, ptr);
            if o == BT_QUOT || o == BT_APOS {
                open = o;
                break;
            }
            match o {
                BT_S | BT_LF | BT_CR => {}
                _ => {
                    *next = ptr;
                    return TOK_INVALID;
                }
            }
        }
        ptr += S::MINBPC;
        loop {
            if !has_char::<S>(ptr, end) {
                return TOK_PARTIAL;
            }
            let t = s.byte_type(b, ptr);
            if t == open {
                break;
            }
            if case!(invalid_cases(s, b, t, &mut ptr, end, next)) {
                continue;
            }
            match t {
                BT_AMP => {
                    let tok = scan_ref(s, b, ptr + S::MINBPC, end, &mut ptr);
                    if tok <= 0 {
                        if tok == TOK_INVALID {
                            *next = ptr;
                        }
                        return tok;
                    }
                }
                BT_LT => {
                    *next = ptr;
                    return TOK_INVALID;
                }
                _ => ptr += S::MINBPC,
            }
        }
        ptr += S::MINBPC;
        if !has_char::<S>(ptr, end) {
            return TOK_PARTIAL;
        }
        match s.byte_type(b, ptr) {
            BT_S | BT_CR | BT_LF => {}
            BT_SOL => return empty_close::<S>(b, ptr, end, next, TOK_EMPTY_ELEMENT_WITH_ATTS),
            BT_GT => {
                *next = ptr + S::MINBPC;
                return TOK_START_TAG_WITH_ATTS;
            }
            _ => {
                *next = ptr;
                return TOK_INVALID;
            }
        }
        loop {
            ptr += S::MINBPC;
            if !has_char::<S>(ptr, end) {
                return TOK_PARTIAL;
            }
            let bt = s.byte_type(b, ptr);
            if case!(check_nmstrt_cases(s, b, bt, &mut ptr, end, next)) {
                break;
            }
            match bt {
                BT_S | BT_CR | BT_LF => continue,
                BT_GT => {
                    *next = ptr + S::MINBPC;
                    return TOK_START_TAG_WITH_ATTS;
                }
                BT_SOL => {
                    return empty_close::<S>(b, ptr, end, next, TOK_EMPTY_ELEMENT_WITH_ATTS);
                }
                _ => {
                    *next = ptr;
                    return TOK_INVALID;
                }
            }
        }
    }
    TOK_PARTIAL
}

fn scan_lt<S: Scan>(s: &S, b: &[u8], mut ptr: usize, end: usize, next: &mut usize) -> i32 {
    if !has_char::<S>(ptr, end) {
        return TOK_PARTIAL;
    }
    let bt = s.byte_type(b, ptr);
    if !case!(check_nmstrt_cases(s, b, bt, &mut ptr, end, next)) {
        match bt {
            BT_EXCL => {
                ptr += S::MINBPC;
                if !has_char::<S>(ptr, end) {
                    return TOK_PARTIAL;
                }
                match s.byte_type(b, ptr) {
                    BT_MINUS => return scan_comment(s, b, ptr + S::MINBPC, end, next),
                    BT_LSQB => return scan_cdata_section::<S>(b, ptr + S::MINBPC, end, next),
                    _ => {}
                }
                *next = ptr;
                return TOK_INVALID;
            }
            BT_QUEST => return scan_pi(s, b, ptr + S::MINBPC, end, next),
            BT_SOL => return scan_end_tag(s, b, ptr + S::MINBPC, end, next),
            _ => {
                *next = ptr;
                return TOK_INVALID;
            }
        }
    }
    while has_char::<S>(ptr, end) {
        let bt = s.byte_type(b, ptr);
        if case!(check_name_cases(s, b, bt, &mut ptr, end, next)) {
            continue;
        }
        match bt {
            BT_S | BT_CR | BT_LF => {
                ptr += S::MINBPC;
                while has_char::<S>(ptr, end) {
                    let bt = s.byte_type(b, ptr);
                    if case!(check_nmstrt_cases(s, b, bt, &mut ptr, end, next)) {
                        return scan_atts(s, b, ptr, end, next);
                    }
                    match bt {
                        BT_GT => {
                            *next = ptr + S::MINBPC;
                            return TOK_START_TAG_NO_ATTS;
                        }
                        BT_SOL => {
                            return empty_close::<S>(b, ptr, end, next, TOK_EMPTY_ELEMENT_NO_ATTS);
                        }
                        BT_S | BT_CR | BT_LF => {
                            ptr += S::MINBPC;
                            continue;
                        }
                        _ => {
                            *next = ptr;
                            return TOK_INVALID;
                        }
                    }
                }
                return TOK_PARTIAL;
            }
            BT_GT => {
                *next = ptr + S::MINBPC;
                return TOK_START_TAG_NO_ATTS;
            }
            BT_SOL => return empty_close::<S>(b, ptr, end, next, TOK_EMPTY_ELEMENT_NO_ATTS),
            _ => {
                *next = ptr;
                return TOK_INVALID;
            }
        }
    }
    TOK_PARTIAL
}

fn content_tok<S: Scan>(s: &S, b: &[u8], mut ptr: usize, end: usize, next: &mut usize) -> i32 {
    if ptr >= end {
        return TOK_NONE;
    }
    let Some(end) = trim_end::<S>(ptr, end) else {
        return TOK_PARTIAL;
    };
    let bt = s.byte_type(b, ptr);
    match bt {
        BT_LT => return scan_lt(s, b, ptr + S::MINBPC, end, next),
        BT_AMP => return scan_ref(s, b, ptr + S::MINBPC, end, next),
        BT_CR => {
            ptr += S::MINBPC;
            if !has_char::<S>(ptr, end) {
                return TOK_TRAILING_CR;
            }
            if s.byte_type(b, ptr) == BT_LF {
                ptr += S::MINBPC;
            }
            *next = ptr;
            return TOK_DATA_NEWLINE;
        }
        BT_LF => {
            *next = ptr + S::MINBPC;
            return TOK_DATA_NEWLINE;
        }
        BT_RSQB => 'rsqb: {
            ptr += S::MINBPC;
            if !has_char::<S>(ptr, end) {
                return TOK_TRAILING_RSQB;
            }
            if !S::char_matches(b, ptr, b']') {
                break 'rsqb;
            }
            ptr += S::MINBPC;
            if !has_char::<S>(ptr, end) {
                return TOK_TRAILING_RSQB;
            }
            if !S::char_matches(b, ptr, b'>') {
                ptr -= S::MINBPC;
                break 'rsqb;
            }
            *next = ptr;
            return TOK_INVALID;
        }
        _ => {
            if !case!(invalid_cases(s, b, bt, &mut ptr, end, next)) {
                ptr += S::MINBPC;
            }
        }
    }
    while has_char::<S>(ptr, end) {
        let bt = s.byte_type(b, ptr);
        match bt {
            BT_LEAD2 | BT_LEAD3 | BT_LEAD4 => {
                let n = lead_n(bt);
                if end < ptr + n || s.is_invalid_char(b, ptr, n) {
                    *next = ptr;
                    return TOK_DATA_CHARS;
                }
                ptr += n;
            }
            BT_RSQB => {
                if has_chars::<S>(ptr, end, 2) {
                    if !S::char_matches(b, ptr + S::MINBPC, b']') {
                        ptr += S::MINBPC;
                        continue;
                    }
                    if has_chars::<S>(ptr, end, 3) {
                        if !S::char_matches(b, ptr + 2 * S::MINBPC, b'>') {
                            ptr += S::MINBPC;
                            continue;
                        }
                        *next = ptr + 2 * S::MINBPC;
                        return TOK_INVALID;
                    }
                }
                *next = ptr;
                return TOK_DATA_CHARS;
            }
            BT_AMP | BT_LT | BT_NONXML | BT_MALFORM | BT_TRAIL | BT_CR | BT_LF => {
                *next = ptr;
                return TOK_DATA_CHARS;
            }
            _ => ptr += S::MINBPC,
        }
    }
    *next = ptr;
    TOK_DATA_CHARS
}

fn scan_percent<S: Scan>(s: &S, b: &[u8], mut ptr: usize, end: usize, next: &mut usize) -> i32 {
    if !has_char::<S>(ptr, end) {
        return TOK_PARTIAL;
    }
    let bt = s.byte_type(b, ptr);
    if !case!(check_nmstrt_cases(s, b, bt, &mut ptr, end, next)) {
        *next = ptr;
        return match bt {
            BT_S | BT_LF | BT_CR | BT_PERCNT => TOK_PERCENT,
            _ => TOK_INVALID,
        };
    }
    while has_char::<S>(ptr, end) {
        let bt = s.byte_type(b, ptr);
        if case!(check_name_cases(s, b, bt, &mut ptr, end, next)) {
            continue;
        }
        if bt == BT_SEMI {
            *next = ptr + S::MINBPC;
            return TOK_PARAM_ENTITY_REF;
        }
        *next = ptr;
        return TOK_INVALID;
    }
    TOK_PARTIAL
}

fn scan_pound_name<S: Scan>(s: &S, b: &[u8], mut ptr: usize, end: usize, next: &mut usize) -> i32 {
    if !has_char::<S>(ptr, end) {
        return TOK_PARTIAL;
    }
    let bt = s.byte_type(b, ptr);
    if !case!(check_nmstrt_cases(s, b, bt, &mut ptr, end, next)) {
        *next = ptr;
        return TOK_INVALID;
    }
    while has_char::<S>(ptr, end) {
        let bt = s.byte_type(b, ptr);
        if case!(check_name_cases(s, b, bt, &mut ptr, end, next)) {
            continue;
        }
        *next = ptr;
        return match bt {
            BT_CR | BT_LF | BT_S | BT_RPAR | BT_GT | BT_PERCNT | BT_VERBAR => TOK_POUND_NAME,
            _ => TOK_INVALID,
        };
    }
    -TOK_POUND_NAME
}

fn scan_lit<S: Scan>(
    open: u8,
    s: &S,
    b: &[u8],
    mut ptr: usize,
    end: usize,
    next: &mut usize,
) -> i32 {
    while has_char::<S>(ptr, end) {
        let t = s.byte_type(b, ptr);
        if case!(invalid_cases(s, b, t, &mut ptr, end, next)) {
            continue;
        }
        match t {
            BT_QUOT | BT_APOS => {
                ptr += S::MINBPC;
                if t != open {
                    continue;
                }
                if !has_char::<S>(ptr, end) {
                    return -TOK_LITERAL;
                }
                *next = ptr;
                return match s.byte_type(b, ptr) {
                    BT_S | BT_CR | BT_LF | BT_GT | BT_PERCNT | BT_LSQB => TOK_LITERAL,
                    _ => TOK_INVALID,
                };
            }
            _ => ptr += S::MINBPC,
        }
    }
    TOK_PARTIAL
}

fn prolog_tok<S: Scan>(s: &S, b: &[u8], mut ptr: usize, end: usize, next: &mut usize) -> i32 {
    if ptr >= end {
        return TOK_NONE;
    }
    let Some(end) = trim_end::<S>(ptr, end) else {
        return TOK_PARTIAL;
    };
    let tok;
    let bt = s.byte_type(b, ptr);
    match bt {
        BT_QUOT => return scan_lit(BT_QUOT, s, b, ptr + S::MINBPC, end, next),
        BT_APOS => return scan_lit(BT_APOS, s, b, ptr + S::MINBPC, end, next),
        BT_LT => {
            ptr += S::MINBPC;
            if !has_char::<S>(ptr, end) {
                return TOK_PARTIAL;
            }
            match s.byte_type(b, ptr) {
                BT_EXCL => return scan_decl(s, b, ptr + S::MINBPC, end, next),
                BT_QUEST => return scan_pi(s, b, ptr + S::MINBPC, end, next),
                BT_NMSTRT | BT_HEX | BT_NONASCII | BT_LEAD2 | BT_LEAD3 | BT_LEAD4 => {
                    *next = ptr - S::MINBPC;
                    return TOK_INSTANCE_START;
                }
                _ => {}
            }
            *next = ptr;
            return TOK_INVALID;
        }
        BT_CR | BT_S | BT_LF => {
            if bt == BT_CR && ptr + S::MINBPC == end {
                *next = end;
                return -TOK_PROLOG_S;
            }
            loop {
                ptr += S::MINBPC;
                if !has_char::<S>(ptr, end) {
                    break;
                }
                match s.byte_type(b, ptr) {
                    BT_S | BT_LF => {}
                    BT_CR if ptr + S::MINBPC != end => {}
                    _ => {
                        *next = ptr;
                        return TOK_PROLOG_S;
                    }
                }
            }
            *next = ptr;
            return TOK_PROLOG_S;
        }
        BT_PERCNT => return scan_percent(s, b, ptr + S::MINBPC, end, next),
        BT_COMMA => {
            *next = ptr + S::MINBPC;
            return TOK_COMMA;
        }
        BT_LSQB => {
            *next = ptr + S::MINBPC;
            return TOK_OPEN_BRACKET;
        }
        BT_RSQB => {
            ptr += S::MINBPC;
            if !has_char::<S>(ptr, end) {
                return -TOK_CLOSE_BRACKET;
            }
            if S::char_matches(b, ptr, b']') {
                if !has_chars::<S>(ptr, end, 2) {
                    return TOK_PARTIAL;
                }
                if S::char_matches(b, ptr + S::MINBPC, b'>') {
                    *next = ptr + 2 * S::MINBPC;
                    return TOK_COND_SECT_CLOSE;
                }
            }
            *next = ptr;
            return TOK_CLOSE_BRACKET;
        }
        BT_LPAR => {
            *next = ptr + S::MINBPC;
            return TOK_OPEN_PAREN;
        }
        BT_RPAR => {
            ptr += S::MINBPC;
            if !has_char::<S>(ptr, end) {
                return -TOK_CLOSE_PAREN;
            }
            match s.byte_type(b, ptr) {
                BT_AST => {
                    *next = ptr + S::MINBPC;
                    return TOK_CLOSE_PAREN_ASTERISK;
                }
                BT_QUEST => {
                    *next = ptr + S::MINBPC;
                    return TOK_CLOSE_PAREN_QUESTION;
                }
                BT_PLUS => {
                    *next = ptr + S::MINBPC;
                    return TOK_CLOSE_PAREN_PLUS;
                }
                BT_CR | BT_LF | BT_S | BT_GT | BT_COMMA | BT_VERBAR | BT_RPAR => {
                    *next = ptr;
                    return TOK_CLOSE_PAREN;
                }
                _ => {}
            }
            *next = ptr;
            return TOK_INVALID;
        }
        BT_VERBAR => {
            *next = ptr + S::MINBPC;
            return TOK_OR;
        }
        BT_GT => {
            *next = ptr + S::MINBPC;
            return TOK_DECL_CLOSE;
        }
        BT_NUM => return scan_pound_name(s, b, ptr + S::MINBPC, end, next),
        BT_LEAD2 | BT_LEAD3 | BT_LEAD4 => {
            let n = lead_n(bt);
            if end < ptr + n {
                return TOK_PARTIAL_CHAR;
            }
            if s.is_invalid_char(b, ptr, n) {
                *next = ptr;
                return TOK_INVALID;
            }
            if s.is_nmstrt_char(b, ptr, n) {
                ptr += n;
                tok = TOK_NAME;
            } else if s.is_name_char(b, ptr, n) {
                ptr += n;
                tok = TOK_NMTOKEN;
            } else {
                *next = ptr;
                return TOK_INVALID;
            }
        }
        BT_NMSTRT | BT_HEX => {
            tok = TOK_NAME;
            ptr += S::MINBPC;
        }
        BT_DIGIT | BT_NAME | BT_MINUS => {
            tok = TOK_NMTOKEN;
            ptr += S::MINBPC;
        }
        BT_NONASCII if S::is_nmstrt_char_minbpc(b, ptr) => {
            ptr += S::MINBPC;
            tok = TOK_NAME;
        }
        BT_NONASCII if S::is_name_char_minbpc(b, ptr) => {
            ptr += S::MINBPC;
            tok = TOK_NMTOKEN;
        }
        _ => {
            *next = ptr;
            return TOK_INVALID;
        }
    }
    while has_char::<S>(ptr, end) {
        let bt = s.byte_type(b, ptr);
        if case!(check_name_cases(s, b, bt, &mut ptr, end, next)) {
            continue;
        }
        match bt {
            BT_GT | BT_RPAR | BT_COMMA | BT_VERBAR | BT_LSQB | BT_PERCNT | BT_S | BT_CR | BT_LF => {
                *next = ptr;
                return tok;
            }
            BT_PLUS | BT_AST | BT_QUEST => {
                if tok == TOK_NMTOKEN {
                    *next = ptr;
                    return TOK_INVALID;
                }
                *next = ptr + S::MINBPC;
                return match bt {
                    BT_PLUS => TOK_NAME_PLUS,
                    BT_AST => TOK_NAME_ASTERISK,
                    _ => TOK_NAME_QUESTION,
                };
            }
            _ => {
                *next = ptr;
                return TOK_INVALID;
            }
        }
    }
    -tok
}

fn attribute_value_tok<S: Scan>(
    s: &S,
    b: &[u8],
    mut ptr: usize,
    end: usize,
    next: &mut usize,
) -> i32 {
    if ptr >= end {
        return TOK_NONE;
    } else if !has_char::<S>(ptr, end) {
        return TOK_PARTIAL;
    }
    let start = ptr;
    while has_char::<S>(ptr, end) {
        let bt = s.byte_type(b, ptr);
        match bt {
            BT_LEAD2 | BT_LEAD3 | BT_LEAD4 => ptr += lead_n(bt),
            BT_AMP => {
                if ptr == start {
                    return scan_ref(s, b, ptr + S::MINBPC, end, next);
                }
                *next = ptr;
                return TOK_DATA_CHARS;
            }
            BT_LT => {
                *next = ptr;
                return TOK_INVALID;
            }
            BT_LF => {
                if ptr == start {
                    *next = ptr + S::MINBPC;
                    return TOK_DATA_NEWLINE;
                }
                *next = ptr;
                return TOK_DATA_CHARS;
            }
            BT_CR => {
                if ptr == start {
                    ptr += S::MINBPC;
                    if !has_char::<S>(ptr, end) {
                        return TOK_TRAILING_CR;
                    }
                    if s.byte_type(b, ptr) == BT_LF {
                        ptr += S::MINBPC;
                    }
                    *next = ptr;
                    return TOK_DATA_NEWLINE;
                }
                *next = ptr;
                return TOK_DATA_CHARS;
            }
            BT_S => {
                if ptr == start {
                    *next = ptr + S::MINBPC;
                    return TOK_ATTRIBUTE_VALUE_S;
                }
                *next = ptr;
                return TOK_DATA_CHARS;
            }
            _ => ptr += S::MINBPC,
        }
    }
    *next = ptr;
    TOK_DATA_CHARS
}

fn entity_value_tok<S: Scan>(s: &S, b: &[u8], mut ptr: usize, end: usize, next: &mut usize) -> i32 {
    if ptr >= end {
        return TOK_NONE;
    } else if !has_char::<S>(ptr, end) {
        return TOK_PARTIAL;
    }
    let start = ptr;
    while has_char::<S>(ptr, end) {
        let bt = s.byte_type(b, ptr);
        match bt {
            BT_LEAD2 | BT_LEAD3 | BT_LEAD4 => ptr += lead_n(bt),
            BT_AMP => {
                if ptr == start {
                    return scan_ref(s, b, ptr + S::MINBPC, end, next);
                }
                *next = ptr;
                return TOK_DATA_CHARS;
            }
            BT_PERCNT => {
                if ptr == start {
                    let tok = scan_percent(s, b, ptr + S::MINBPC, end, next);
                    return if tok == TOK_PERCENT { TOK_INVALID } else { tok };
                }
                *next = ptr;
                return TOK_DATA_CHARS;
            }
            BT_LF => {
                if ptr == start {
                    *next = ptr + S::MINBPC;
                    return TOK_DATA_NEWLINE;
                }
                *next = ptr;
                return TOK_DATA_CHARS;
            }
            BT_CR => {
                if ptr == start {
                    ptr += S::MINBPC;
                    if !has_char::<S>(ptr, end) {
                        return TOK_TRAILING_CR;
                    }
                    if s.byte_type(b, ptr) == BT_LF {
                        ptr += S::MINBPC;
                    }
                    *next = ptr;
                    return TOK_DATA_NEWLINE;
                }
                *next = ptr;
                return TOK_DATA_CHARS;
            }
            _ => ptr += S::MINBPC,
        }
    }
    *next = ptr;
    TOK_DATA_CHARS
}

/// `isPublicId`: `Err(badPtr)` at the first character a public identifier may not hold.
fn is_public_id<S: Scan>(s: &S, b: &[u8], mut ptr: usize, mut end: usize) -> Result<(), usize> {
    ptr += S::MINBPC;
    end -= S::MINBPC;
    while has_char::<S>(ptr, end) {
        let bt = s.byte_type(b, ptr);
        let ok = match bt {
            BT_DIGIT | BT_HEX | BT_MINUS | BT_APOS | BT_LPAR | BT_RPAR | BT_PLUS | BT_COMMA
            | BT_SOL | BT_EQUALS | BT_QUEST | BT_CR | BT_LF | BT_SEMI | BT_EXCL | BT_AST
            | BT_PERCNT | BT_NUM => true,
            BT_S => {
                if S::char_matches(b, ptr, b'\t') {
                    return Err(ptr);
                }
                true
            }
            // BT_NAME and BT_NMSTRT fall through to `default` when not ASCII.
            BT_NAME | BT_NMSTRT if (S::byte_to_ascii(b, ptr) & !0x7f) == 0 => true,
            _ => matches!(S::byte_to_ascii(b, ptr), 0x24 | 0x40),
        };
        if !ok {
            return Err(ptr);
        }
        ptr += S::MINBPC;
    }
    Ok(())
}

/// `getAtts`: every attribute of the start tag at `ptr`, in order (the C fills `attsMax` and
/// counts the rest, and is called again with room for all).
fn get_atts<S: Scan>(s: &S, b: &[u8], mut ptr: usize) -> Vec<Att> {
    #[derive(PartialEq)]
    enum St {
        Other,
        InName,
        InValue,
    }
    let mut state = St::InName;
    let mut atts: Vec<Att> = Vec::new();
    let mut cur = Att::default();
    let mut open = 0u8;
    ptr += S::MINBPC;
    loop {
        let bt = s.byte_type(b, ptr);
        match bt {
            BT_LEAD2 | BT_LEAD3 | BT_LEAD4 | BT_NONASCII | BT_NMSTRT | BT_HEX => {
                if state == St::Other {
                    cur = Att {
                        name: ptr,
                        value_ptr: 0,
                        value_end: 0,
                        normalized: true,
                    };
                    state = St::InName;
                }
                if bt == BT_LEAD2 || bt == BT_LEAD3 || bt == BT_LEAD4 {
                    ptr += lead_n(bt) - S::MINBPC;
                }
            }
            BT_QUOT | BT_APOS => {
                if state != St::InValue {
                    cur.value_ptr = ptr + S::MINBPC;
                    state = St::InValue;
                    open = bt;
                } else if open == bt {
                    state = St::Other;
                    cur.value_end = ptr;
                    atts.push(cur);
                }
            }
            BT_AMP => cur.normalized = false,
            BT_S => {
                if state == St::InName {
                    state = St::Other;
                } else if state == St::InValue
                    && cur.normalized
                    && (ptr == cur.value_ptr
                        || S::byte_to_ascii(b, ptr) != 0x20
                        || S::byte_to_ascii(b, ptr + S::MINBPC) == 0x20
                        || s.byte_type(b, ptr + S::MINBPC) == open)
                {
                    cur.normalized = false;
                }
            }
            BT_CR | BT_LF => {
                if state == St::InName {
                    state = St::Other;
                } else if state == St::InValue {
                    cur.normalized = false;
                }
            }
            BT_GT | BT_SOL => {
                if state != St::InValue {
                    return atts;
                }
            }
            _ => {}
        }
        ptr += S::MINBPC;
    }
}

/// `checkCharRefNumber`.
fn check_char_ref_number(result: i32) -> i32 {
    match result >> 8 {
        0xD8..=0xDF => return -1,
        0 => {
            if latin1_type(result as u8) == BT_NONXML {
                return -1;
            }
        }
        0xFF => {
            if result == 0xFFFE || result == 0xFFFF {
                return -1;
            }
        }
        _ => {}
    }
    result
}

fn char_ref_number<S: Scan>(b: &[u8], mut ptr: usize) -> i32 {
    let mut result: i32 = 0;
    ptr += 2 * S::MINBPC;
    if S::char_matches(b, ptr, b'x') {
        ptr += S::MINBPC;
        while !S::char_matches(b, ptr, b';') {
            let c = S::byte_to_ascii(b, ptr);
            match c {
                0x30..=0x39 => {
                    result <<= 4;
                    result |= c - 0x30;
                }
                0x41..=0x46 => {
                    result <<= 4;
                    result += 10 + (c - 0x41);
                }
                0x61..=0x66 => {
                    result <<= 4;
                    result += 10 + (c - 0x61);
                }
                _ => {}
            }
            if result >= 0x110000 {
                return -1;
            }
            ptr += S::MINBPC;
        }
    } else {
        while !S::char_matches(b, ptr, b';') {
            let c = S::byte_to_ascii(b, ptr);
            result *= 10;
            result += c - 0x30;
            if result >= 0x110000 {
                return -1;
            }
            ptr += S::MINBPC;
        }
    }
    check_char_ref_number(result)
}

fn predefined_entity_name<S: Scan>(b: &[u8], mut ptr: usize, end: usize) -> u8 {
    match (end - ptr) / S::MINBPC {
        2 => {
            if S::char_matches(b, ptr + S::MINBPC, b't') {
                match S::byte_to_ascii(b, ptr) {
                    0x6C => return b'<',
                    0x67 => return b'>',
                    _ => {}
                }
            }
        }
        3 => {
            if S::char_matches(b, ptr, b'a') {
                ptr += S::MINBPC;
                if S::char_matches(b, ptr, b'm') {
                    ptr += S::MINBPC;
                    if S::char_matches(b, ptr, b'p') {
                        return b'&';
                    }
                }
            }
        }
        4 => match S::byte_to_ascii(b, ptr) {
            0x71 => {
                ptr += S::MINBPC;
                if S::char_matches(b, ptr, b'u') {
                    ptr += S::MINBPC;
                    if S::char_matches(b, ptr, b'o') {
                        ptr += S::MINBPC;
                        if S::char_matches(b, ptr, b't') {
                            return b'"';
                        }
                    }
                }
            }
            0x61 => {
                ptr += S::MINBPC;
                if S::char_matches(b, ptr, b'p') {
                    ptr += S::MINBPC;
                    if S::char_matches(b, ptr, b'o') {
                        ptr += S::MINBPC;
                        if S::char_matches(b, ptr, b's') {
                            return b'\'';
                        }
                    }
                }
            }
            _ => {}
        },
        _ => {}
    }
    0
}

fn name_matches_ascii<S: Scan>(b: &[u8], mut ptr1: usize, end1: usize, ptr2: &[u8]) -> bool {
    for &c in ptr2 {
        if end1 < ptr1 + S::MINBPC {
            return false;
        }
        if !S::char_matches(b, ptr1, c) {
            return false;
        }
        ptr1 += S::MINBPC;
    }
    ptr1 == end1
}

fn name_length<S: Scan>(s: &S, b: &[u8], mut ptr: usize) -> usize {
    let start = ptr;
    loop {
        let bt = s.byte_type(b, ptr);
        match bt {
            BT_LEAD2 | BT_LEAD3 | BT_LEAD4 => ptr += lead_n(bt),
            BT_NONASCII | BT_NMSTRT | BT_HEX | BT_DIGIT | BT_NAME | BT_MINUS => ptr += S::MINBPC,
            _ => return ptr - start,
        }
    }
}

fn update_position<S: Scan>(s: &S, b: &[u8], mut ptr: usize, end: usize, pos: &mut Position) {
    while has_char::<S>(ptr, end) {
        let bt = s.byte_type(b, ptr);
        match bt {
            BT_LEAD2 | BT_LEAD3 | BT_LEAD4 => {
                ptr += lead_n(bt);
                pos.column = pos.column.wrapping_add(1);
            }
            BT_LF => {
                pos.column = 0;
                pos.line = pos.line.wrapping_add(1);
                ptr += S::MINBPC;
            }
            BT_CR => {
                pos.line = pos.line.wrapping_add(1);
                ptr += S::MINBPC;
                if has_char::<S>(ptr, end) && s.byte_type(b, ptr) == BT_LF {
                    ptr += S::MINBPC;
                }
                pos.column = 0;
            }
            _ => {
                ptr += S::MINBPC;
                pos.column = pos.column.wrapping_add(1);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The conversions to UTF-8 (`XmlUtf8Convert`), from `from` toward `from_lim`, onto `out` until it
// holds `out_lim` bytes.

/// `_INTERNAL_trim_to_complete_utf8_characters`.
fn trim_to_complete_utf8_characters(b: &[u8], from: usize, from_lim: usize) -> usize {
    let mut lim = from_lim;
    let mut walked: usize = 0;
    while lim > from {
        let prev = b[lim - 1];
        if prev & 0xf8 == 0xf0 {
            if walked + 1 >= 4 {
                lim += 4 - 1;
                break;
            } else {
                walked = 0;
            }
        } else if prev & 0xf0 == 0xe0 {
            if walked + 1 >= 3 {
                lim += 3 - 1;
                break;
            } else {
                walked = 0;
            }
        } else if prev & 0xe0 == 0xc0 {
            if walked + 1 >= 2 {
                lim += 2 - 1;
                break;
            } else {
                walked = 0;
            }
        } else if prev & 0x80 == 0x00 {
            break;
        }
        lim -= 1;
        walked += 1;
    }
    lim
}

fn utf8_to_utf8(
    b: &[u8],
    from: &mut usize,
    mut from_lim: usize,
    out: &mut Vec<u8>,
    out_lim: usize,
) -> Convert {
    let mut input_incomplete = false;
    let mut output_exhausted = false;
    let available = from_lim as isize - *from as isize;
    let storable = out_lim as isize - out.len() as isize;
    if available > storable {
        from_lim = (*from as isize + storable) as usize;
        output_exhausted = true;
    }
    let before = from_lim;
    from_lim = trim_to_complete_utf8_characters(b, *from, from_lim);
    if from_lim < before {
        input_incomplete = true;
    }
    if from_lim > *from {
        out.extend_from_slice(&b[*from..from_lim]);
        *from = from_lim;
    }
    if output_exhausted {
        Convert::OutputExhausted
    } else if input_incomplete {
        Convert::InputIncomplete
    } else {
        Convert::Completed
    }
}

fn latin1_to_utf8(
    b: &[u8],
    from: &mut usize,
    from_lim: usize,
    out: &mut Vec<u8>,
    out_lim: usize,
) -> Convert {
    loop {
        if *from == from_lim {
            return Convert::Completed;
        }
        let c = b[*from];
        if c & 0x80 != 0 {
            if out_lim < out.len() + 2 {
                return Convert::OutputExhausted;
            }
            out.push((c >> 6) | 0xc0);
            out.push((c & 0x3f) | 0x80);
            *from += 1;
        } else {
            if out.len() == out_lim {
                return Convert::OutputExhausted;
            }
            out.push(c);
            *from += 1;
        }
    }
}

fn ascii_to_utf8(
    b: &[u8],
    from: &mut usize,
    from_lim: usize,
    out: &mut Vec<u8>,
    out_lim: usize,
) -> Convert {
    while *from < from_lim && out.len() < out_lim {
        out.push(b[*from]);
        *from += 1;
    }
    if out.len() == out_lim && *from < from_lim {
        Convert::OutputExhausted
    } else {
        Convert::Completed
    }
}

fn utf16_to_utf8<const BIG: bool>(
    b: &[u8],
    from_p: &mut usize,
    from_lim: usize,
    out: &mut Vec<u8>,
    out_lim: usize,
) -> Convert {
    let mut from = *from_p;
    // Round down to an even number of bytes.
    let from_lim = from + (((from_lim - from) >> 1) << 1);
    while from < from_lim {
        let lo = Utf16::<BIG>::lo(b, from);
        let hi = Utf16::<BIG>::hi(b, from);
        let room = out_lim as isize - out.len() as isize;
        match hi {
            0 if lo < 0x80 => {
                if room == 0 {
                    *from_p = from;
                    return Convert::OutputExhausted;
                }
                out.push(lo);
            }
            0..=7 => {
                if room < 2 {
                    *from_p = from;
                    return Convert::OutputExhausted;
                }
                out.push((lo >> 6) | (hi << 2) | 0xc0);
                out.push((lo & 0x3f) | 0x80);
            }
            0xD8..=0xDB => {
                if room < 4 {
                    *from_p = from;
                    return Convert::OutputExhausted;
                }
                if from_lim - from < 4 {
                    *from_p = from;
                    return Convert::InputIncomplete;
                }
                let plane = ((((hi & 0x3) as i32) << 2) | (((lo >> 6) & 0x3) as i32)) + 1;
                out.push(((plane >> 2) as u8) | 0xf0);
                out.push(((lo >> 2) & 0xF) | (((plane & 0x3) as u8) << 4) | 0x80);
                from += 2;
                let lo2 = Utf16::<BIG>::lo(b, from);
                out.push(
                    ((lo & 0x3) << 4)
                        | ((Utf16::<BIG>::hi(b, from) & 0x3) << 2)
                        | (lo2 >> 6)
                        | 0x80,
                );
                out.push((lo2 & 0x3f) | 0x80);
            }
            _ => {
                if room < 3 {
                    *from_p = from;
                    return Convert::OutputExhausted;
                }
                out.push((hi >> 4) | 0xe0);
                out.push(((hi & 0xf) << 2) | (lo >> 6) | 0x80);
                out.push((lo & 0x3f) | 0x80);
            }
        }
        from += 2;
    }
    *from_p = from;
    if from < from_lim {
        Convert::InputIncomplete
    } else {
        Convert::Completed
    }
}

/// `XmlUtf8Encode`: a character's UTF-8 bytes, none for a negative number or one past U+10FFFF.
pub(crate) fn utf8_encode(c: i32, buf: &mut [u8; 4]) -> usize {
    if c < 0 {
        return 0;
    }
    if c < 0x80 {
        buf[0] = c as u8;
        return 1;
    }
    if c < 0x800 {
        buf[0] = ((c >> 6) | 0xc0) as u8;
        buf[1] = ((c & 0x3f) | 0x80) as u8;
        return 2;
    }
    if c < 0x10000 {
        buf[0] = ((c >> 12) | 0xe0) as u8;
        buf[1] = (((c >> 6) & 0x3f) | 0x80) as u8;
        buf[2] = ((c & 0x3f) | 0x80) as u8;
        return 3;
    }
    if c < 0x110000 {
        buf[0] = ((c >> 18) | 0xf0) as u8;
        buf[1] = (((c >> 12) & 0x3f) | 0x80) as u8;
        buf[2] = (((c >> 6) & 0x3f) | 0x80) as u8;
        buf[3] = ((c & 0x3f) | 0x80) as u8;
        return 4;
    }
    0
}

// ---------------------------------------------------------------------------------------------
// The encodings.

/// An `ENCODING`: one of the encoding table's, the internal one, or the initial one that has not
/// yet seen the document's first bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Enc {
    /// `utf8_encoding`.
    Utf8,
    /// `internal_utf8_encoding` (`XmlGetInternalEncoding()`): entity text.
    InternalUtf8,
    /// `latin1_encoding`.
    Latin1,
    /// `ascii_encoding`.
    Ascii,
    /// `little2_encoding`.
    Little2,
    /// `big2_encoding`.
    Big2,
    /// `m_initEncoding` as `XmlInitEncoding(.., NULL)` sets it up (`INIT_ENC_INDEX` is `NO_ENC`).
    Init,
}

/// `encodings[]` of xmltok_ns.inc, by `getEncodingIndex`: ISO-8859-1, US-ASCII, UTF-8, UTF-16,
/// UTF-16BE, UTF-16LE, and `NO_ENC`.
const ENCODINGS: [Enc; 7] = [
    Enc::Latin1,
    Enc::Ascii,
    Enc::Utf8,
    Enc::Big2,
    Enc::Big2,
    Enc::Little2,
    Enc::Utf8,
];
const UTF_8_ENC: usize = 2;
const UTF_16BE_ENC: usize = 4;
const UTF_16LE_ENC: usize = 5;
const NO_ENC: usize = 6;

const L2: Utf16<false> = Utf16;
const B2: Utf16<true> = Utf16;

/// Calls `$f` with the encoding's scanner, as the C calls through the vtable.
macro_rules! dispatch {
    ($enc:expr, $s:ident => $body:expr) => {
        match $enc {
            Enc::Utf8 => {
                let $s = &UTF8;
                $body
            }
            Enc::InternalUtf8 => {
                let $s = &INTERNAL_UTF8;
                $body
            }
            Enc::Latin1 => {
                let $s = &LATIN1;
                $body
            }
            Enc::Ascii => {
                let $s = &ASCII;
                $body
            }
            Enc::Little2 => {
                let $s = &L2;
                $body
            }
            Enc::Big2 => {
                let $s = &B2;
                $body
            }
            Enc::Init => unreachable!("the initial encoding scans only through initScan"),
        }
    };
}

/// `streqci`: equal but for the case of ASCII letters (the C upper-cases `a` to `z` of both).
fn streqci(s1: &[u8], s2: &[u8]) -> bool {
    s1.eq_ignore_ascii_case(s2)
}

/// `getEncodingIndex` (never `NULL` here): `None` is `UNKNOWN_ENC`.
fn get_encoding_index(name: &[u8]) -> Option<usize> {
    const NAMES: [&[u8]; 6] = [
        b"ISO-8859-1",
        b"US-ASCII",
        b"UTF-8",
        b"UTF-16",
        b"UTF-16BE",
        b"UTF-16LE",
    ];
    NAMES.iter().position(|n| streqci(name, n))
}

impl Enc {
    /// `minBytesPerChar`.
    pub(crate) fn minbpc(self) -> usize {
        match self {
            Enc::Little2 | Enc::Big2 => 2,
            _ => 1,
        }
    }

    /// `isUtf8`: character data passes to the handler without conversion.
    pub(crate) fn is_utf8(self) -> bool {
        matches!(self, Enc::Utf8 | Enc::InternalUtf8 | Enc::Ascii)
    }

    /// `XmlPrologTok`. On the initial encoding this is `initScanProlog`, which sets `*enc` to the
    /// encoding it detects before scanning with it (`*encPtr`).
    pub(crate) fn prolog_tok(
        enc: &mut Enc,
        b: &[u8],
        ptr: usize,
        end: usize,
        next: &mut usize,
    ) -> i32 {
        if *enc == Enc::Init {
            return init_scan_prolog(enc, b, ptr, end, next);
        }
        dispatch!(*enc, s => prolog_tok(s, b, ptr, end, next))
    }

    /// `XmlContentTok`.
    pub(crate) fn content_tok(self, b: &[u8], ptr: usize, end: usize, next: &mut usize) -> i32 {
        dispatch!(self, s => content_tok(s, b, ptr, end, next))
    }

    /// `XmlCdataSectionTok`.
    pub(crate) fn cdata_section_tok(
        self,
        b: &[u8],
        ptr: usize,
        end: usize,
        next: &mut usize,
    ) -> i32 {
        dispatch!(self, s => cdata_section_tok(s, b, ptr, end, next))
    }

    /// `XmlAttributeValueTok`.
    pub(crate) fn attribute_value_tok(
        self,
        b: &[u8],
        ptr: usize,
        end: usize,
        next: &mut usize,
    ) -> i32 {
        dispatch!(self, s => attribute_value_tok(s, b, ptr, end, next))
    }

    /// `XmlEntityValueTok`.
    pub(crate) fn entity_value_tok(
        self,
        b: &[u8],
        ptr: usize,
        end: usize,
        next: &mut usize,
    ) -> i32 {
        dispatch!(self, s => entity_value_tok(s, b, ptr, end, next))
    }

    /// `XmlNameMatchesAscii`.
    pub(crate) fn name_matches_ascii(self, b: &[u8], ptr: usize, end: usize, ascii: &[u8]) -> bool {
        match self {
            Enc::Little2 => name_matches_ascii::<Utf16<false>>(b, ptr, end, ascii),
            Enc::Big2 => name_matches_ascii::<Utf16<true>>(b, ptr, end, ascii),
            _ => name_matches_ascii::<Normal>(b, ptr, end, ascii),
        }
    }

    /// `XmlNameLength`.
    pub(crate) fn name_length(self, b: &[u8], ptr: usize) -> usize {
        dispatch!(self, s => name_length(s, b, ptr))
    }

    /// `XmlGetAttributes`.
    pub(crate) fn get_atts(self, b: &[u8], ptr: usize) -> Vec<Att> {
        dispatch!(self, s => get_atts(s, b, ptr))
    }

    /// `XmlCharRefNumber`.
    pub(crate) fn char_ref_number(self, b: &[u8], ptr: usize) -> i32 {
        match self {
            Enc::Little2 => char_ref_number::<Utf16<false>>(b, ptr),
            Enc::Big2 => char_ref_number::<Utf16<true>>(b, ptr),
            _ => char_ref_number::<Normal>(b, ptr),
        }
    }

    /// `XmlPredefinedEntityName`: the character `&lt;` and its kin stand for, or 0.
    pub(crate) fn predefined_entity_name(self, b: &[u8], ptr: usize, end: usize) -> u8 {
        match self {
            Enc::Little2 => predefined_entity_name::<Utf16<false>>(b, ptr, end),
            Enc::Big2 => predefined_entity_name::<Utf16<true>>(b, ptr, end),
            _ => predefined_entity_name::<Normal>(b, ptr, end),
        }
    }

    /// `XmlUpdatePosition`; the initial encoding's is `initUpdatePosition`, UTF-8's.
    pub(crate) fn update_position(self, b: &[u8], ptr: usize, end: usize, pos: &mut Position) {
        match self {
            Enc::Init => update_position(&UTF8, b, ptr, end, pos),
            _ => dispatch!(self, s => update_position(s, b, ptr, end, pos)),
        }
    }

    /// `XmlIsPublicId`.
    pub(crate) fn is_public_id(self, b: &[u8], ptr: usize, end: usize) -> Result<(), usize> {
        dispatch!(self, s => is_public_id(s, b, ptr, end))
    }

    /// `XmlUtf8Convert`.
    pub(crate) fn to_utf8(
        self,
        b: &[u8],
        from: &mut usize,
        from_lim: usize,
        out: &mut Vec<u8>,
        out_lim: usize,
    ) -> Convert {
        match self {
            Enc::Utf8 | Enc::InternalUtf8 => utf8_to_utf8(b, from, from_lim, out, out_lim),
            Enc::Latin1 => latin1_to_utf8(b, from, from_lim, out, out_lim),
            Enc::Ascii => ascii_to_utf8(b, from, from_lim, out, out_lim),
            Enc::Little2 => utf16_to_utf8::<false>(b, from, from_lim, out, out_lim),
            Enc::Big2 => utf16_to_utf8::<true>(b, from, from_lim, out, out_lim),
            Enc::Init => unreachable!("the initial encoding converts nothing"),
        }
    }

    /// `poolAppend`'s loop: `ptr..end` converted to UTF-8 and appended whole (the C grows its
    /// pool and converts again until the conversion completes or stops at an incomplete
    /// character).
    pub(crate) fn append_utf8(self, b: &[u8], mut ptr: usize, end: usize, out: &mut Vec<u8>) {
        loop {
            let lim = out.len() + 1024;
            match self.to_utf8(b, &mut ptr, end, out, lim) {
                Convert::Completed | Convert::InputIncomplete => return,
                Convert::OutputExhausted => {}
            }
        }
    }

    /// `toAscii`: the character at `ptr` if it converts to one byte, or -1.
    fn to_ascii(self, b: &[u8], ptr: usize, end: usize) -> i32 {
        let mut buf = Vec::with_capacity(1);
        let mut p = ptr;
        self.to_utf8(b, &mut p, end, &mut buf, 1);
        if buf.is_empty() {
            -1
        } else {
            buf[0] as i8 as i32
        }
    }
}

/// `initScan` in the prolog state, with `INIT_ENC_INDEX` `NO_ENC`.
fn init_scan_prolog(enc: &mut Enc, b: &[u8], ptr: usize, end: usize, next: &mut usize) -> i32 {
    if ptr >= end {
        return TOK_NONE;
    }
    if ptr + 1 == end {
        // `state != XML_CONTENT_STATE`.
        return TOK_PARTIAL;
    }
    match ((b[ptr] as u32) << 8) | b[ptr + 1] as u32 {
        0xFEFF => {
            *next = ptr + 2;
            *enc = ENCODINGS[UTF_16BE_ENC];
            return TOK_BOM;
        }
        0x3C00 => {
            *enc = ENCODINGS[UTF_16LE_ENC];
            return Enc::prolog_tok(enc, b, ptr, end, next);
        }
        0xFFFE => {
            *next = ptr + 2;
            *enc = ENCODINGS[UTF_16LE_ENC];
            return TOK_BOM;
        }
        0xEFBB => {
            if ptr + 2 == end {
                return TOK_PARTIAL;
            }
            if b[ptr + 2] == 0xBF {
                *next = ptr + 3;
                *enc = ENCODINGS[UTF_8_ENC];
                return TOK_BOM;
            }
        }
        _ => {
            if b[ptr] == 0 {
                *enc = ENCODINGS[UTF_16BE_ENC];
                return Enc::prolog_tok(enc, b, ptr, end, next);
            } else if b[ptr + 1] == 0 {
                *enc = ENCODINGS[UTF_16LE_ENC];
                return Enc::prolog_tok(enc, b, ptr, end, next);
            }
        }
    }
    *enc = ENCODINGS[NO_ENC];
    Enc::prolog_tok(enc, b, ptr, end, next)
}

fn is_space(c: i32) -> bool {
    matches!(c, 0x20 | 0xD | 0xA | 0x9)
}

/// A pseudo-attribute of the XML declaration: `None` when the declaration ends first.
struct Pseudo {
    name: usize,
    name_end: usize,
    val: usize,
}

/// `parsePseudoAttribute`: `Err(nextTokPtr)` when malformed; `ptr` becomes `nextTokPtr`.
fn parse_pseudo_attribute(
    enc: Enc,
    b: &[u8],
    ptr: &mut usize,
    end: usize,
) -> Result<Option<Pseudo>, usize> {
    let m = enc.minbpc();
    let mut p = *ptr;
    if p == end {
        return Ok(None);
    }
    if !is_space(enc.to_ascii(b, p, end)) {
        return Err(p);
    }
    loop {
        p += m;
        if !is_space(enc.to_ascii(b, p, end)) {
            break;
        }
    }
    if p == end {
        return Ok(None);
    }
    let name = p;
    let name_end;
    loop {
        let c = enc.to_ascii(b, p, end);
        if c == -1 {
            return Err(p);
        }
        if c == b'=' as i32 {
            name_end = p;
            break;
        }
        if is_space(c) {
            name_end = p;
            let mut c;
            loop {
                p += m;
                c = enc.to_ascii(b, p, end);
                if !is_space(c) {
                    break;
                }
            }
            if c != b'=' as i32 {
                return Err(p);
            }
            break;
        }
        p += m;
    }
    if p == name {
        return Err(p);
    }
    p += m;
    let mut c = enc.to_ascii(b, p, end);
    while is_space(c) {
        p += m;
        c = enc.to_ascii(b, p, end);
    }
    if c != b'"' as i32 && c != b'\'' as i32 {
        return Err(p);
    }
    let open = c;
    p += m;
    let val = p;
    loop {
        let c = enc.to_ascii(b, p, end);
        if c == open {
            break;
        }
        let ok = (0x61..=0x7A).contains(&c)
            || (0x41..=0x5A).contains(&c)
            || (0x30..=0x39).contains(&c)
            || c == b'.' as i32
            || c == b'-' as i32
            || c == b'_' as i32;
        if !ok {
            return Err(p);
        }
        p += m;
    }
    *ptr = p + m;
    Ok(Some(Pseudo {
        name,
        name_end,
        val,
    }))
}

/// What `XmlParseXmlDecl` found in a document's XML declaration.
pub(crate) struct XmlDecl {
    /// `encodingName`: where the encoding's name starts.
    pub encoding_name: Option<usize>,
    /// `*encoding`: the encoding the name names, `None` when it names none expat knows.
    pub new_encoding: Option<Enc>,
    /// `standalone`: -1, 0 or 1.
    pub standalone: i32,
}

/// `findEncoding`.
fn find_encoding(enc: Enc, b: &[u8], ptr: usize, end: usize) -> Option<Enc> {
    let mut buf = Vec::new();
    let mut p = ptr;
    enc.to_utf8(b, &mut p, end, &mut buf, 127);
    if p != end {
        return None;
    }
    if streqci(&buf, b"UTF-16") && enc.minbpc() == 2 {
        return Some(enc);
    }
    get_encoding_index(&buf).map(|i| ENCODINGS[i])
}

/// `XmlParseXmlDecl` for a document (`isGeneralTextEntity` 0): `Err(badPtr)` when malformed.
pub(crate) fn parse_xml_decl(enc: Enc, b: &[u8], ptr: usize, end: usize) -> Result<XmlDecl, usize> {
    let m = enc.minbpc();
    let mut ptr = ptr + 5 * m;
    let end = end - 2 * m;
    let mut decl = XmlDecl {
        encoding_name: None,
        new_encoding: None,
        standalone: -1,
    };
    // `! parsePseudoAttribute(..) || ! name`: `ptr` is where it failed, or unmoved.
    let mut att = match parse_pseudo_attribute(enc, b, &mut ptr, end) {
        Ok(Some(a)) => a,
        Ok(None) => return Err(ptr),
        Err(p) => return Err(p),
    };
    if !enc.name_matches_ascii(b, att.name, att.name_end, b"version") {
        return Err(att.name);
    }
    match parse_pseudo_attribute(enc, b, &mut ptr, end) {
        Err(p) => return Err(p),
        Ok(None) => return Ok(decl),
        Ok(Some(a)) => att = a,
    }
    if enc.name_matches_ascii(b, att.name, att.name_end, b"encoding") {
        let c = enc.to_ascii(b, att.val, end);
        if !((0x61..=0x7A).contains(&c) || (0x41..=0x5A).contains(&c)) {
            return Err(att.val);
        }
        decl.encoding_name = Some(att.val);
        decl.new_encoding = find_encoding(enc, b, att.val, ptr - m);
        match parse_pseudo_attribute(enc, b, &mut ptr, end) {
            Err(p) => return Err(p),
            Ok(None) => return Ok(decl),
            Ok(Some(a)) => att = a,
        }
    }
    if !enc.name_matches_ascii(b, att.name, att.name_end, b"standalone") {
        return Err(att.name);
    }
    if enc.name_matches_ascii(b, att.val, ptr - m, b"yes") {
        decl.standalone = 1;
    } else if enc.name_matches_ascii(b, att.val, ptr - m, b"no") {
        decl.standalone = 0;
    } else {
        return Err(att.val);
    }
    while is_space(enc.to_ascii(b, ptr, end)) {
        ptr += m;
    }
    if ptr != end {
        return Err(ptr);
    }
    Ok(decl)
}
