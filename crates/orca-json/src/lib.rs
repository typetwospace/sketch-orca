// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.
//
// Carries JSON for Modern C++ code: the lexer, the parser, the serializer and the messages of their
// exceptions are translated, branch for branch, from detail/input/lexer.hpp (scan, scan_string,
// scan_number, scan_literal, get, unget, get_codepoint, next_byte_in_range, skip_bom,
// skip_whitespace, get_token_string, token_type_name), detail/input/parser.hpp
// (sax_parse_internal, parse, exception_message), detail/exceptions.hpp (parse_error,
// out_of_range and type_error's what()) and detail/output/serializer.hpp (dump, dump_escaped,
// decode) of nlohmann/json 3.10.4, as OrcaSlicer v2.4.2 carries it in deps_src/nlohmann, on
// 2026-11-06. Copyright (c) 2013-2019 Niels Lohmann, under the MIT licence
// (licenses/MIT-nlohmann-json.txt).

//! JSON with `nlohmann::json`'s semantics, as the C++ front end uses it (`nlohmann` 3.10.4).
//!
//! - [`Value`]: null, a boolean, a signed or an unsigned integer, a double, a string, an array or
//!   an object. As in `nlohmann::json`, a parsed number without a `-` is unsigned, one with a `-`
//!   is signed, and one with a fraction or an exponent, or one too big for 64 bits, is a double.
//! - [`Object`]: an object's members in the order of `nlohmann::json` ([`Order::Sorted`]: by key,
//!   byte for byte, as `std::map<std::string, json>` keeps them) or of `nlohmann::ordered_json`
//!   ([`Order::Insertion`]: in the order keys were first set). Setting a key that is there keeps
//!   its place; a key given twice in a parsed text keeps its first place and its last value.
//! - [`parse`]: `json::parse(text)`, with the same refusals and the same `what()` ([`Error`]),
//!   lines and columns included. Numbers are read with `orca_fmt::parse`'s `strtoull`, `strtoll`
//!   and `strtod`, the Universal CRT's.
//! - [`Value::dump`]: `dump(indent, indent_char, ensure_ascii)`, the same bytes: doubles through
//!   nlohmann's Grisu2 (`dtoa.rs`), `null` for an infinity or a NaN, strings escaped as
//!   `dump_escaped` escapes them.
//! - [`escape_bytes`] and [`string_from_bytes`]: `dump_escaped` of a `std::string` that need not be
//!   UTF-8, with `error_handler_t` strict, replace or ignore. A [`Value`]'s strings are Rust
//!   strings, so text from outside (an OS message, a file name) goes through
//!   [`string_from_bytes`] with [`ErrorHandler::Replace`] first; dumping the result writes the
//!   bytes `dump(..., error_handler_t::replace)` writes for the raw bytes.

mod dtoa;

use std::fmt;

/// How an object keeps its members.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    /// `nlohmann::json`: sorted by key, as `std::less<std::string>` compares (bytes, unsigned).
    Sorted,
    /// `nlohmann::ordered_json`: in the order the keys were first set.
    Insertion,
}

/// A JSON object: its members and how it orders them.
#[derive(Clone, Debug, PartialEq)]
pub struct Object {
    order: Order,
    entries: Vec<(String, Value)>,
}

impl Object {
    /// An empty object of that order.
    #[must_use]
    pub fn new(order: Order) -> Object {
        Object {
            order,
            entries: Vec::new(),
        }
    }

    /// How the object orders its members.
    #[must_use]
    pub fn order(&self) -> Order {
        self.order
    }

    fn position(&self, key: &str) -> Result<usize, usize> {
        match self.order {
            Order::Sorted => {
                // std::map's lower_bound: the first key that does not go before `key`.
                let i = orca_stl::lower_bound_by(&self.entries, key, |(k, _), key: &str| {
                    k.as_bytes() < key.as_bytes()
                });
                match self.entries.get(i) {
                    Some((k, _)) if k == key => Ok(i),
                    _ => Err(i),
                }
            }
            Order::Insertion => self
                .entries
                .iter()
                .position(|(k, _)| k == key)
                .ok_or(self.entries.len()),
        }
    }

    /// `object[key] = value`: replaces the value of a key that is there, in its place, or adds
    /// the key where the order puts it.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<Value>) {
        let key = key.into();
        let value = value.into();
        match self.position(&key) {
            Ok(i) => self.entries[i].1 = value,
            Err(i) => self.entries.insert(i, (key, value)),
        }
    }

    /// `object[key]`: the value of `key`, added as null if it is not there.
    pub fn entry(&mut self, key: &str) -> &mut Value {
        let i = match self.position(key) {
            Ok(i) => i,
            Err(i) => {
                self.entries.insert(i, (key.to_string(), Value::Null));
                i
            }
        };
        &mut self.entries[i].1
    }

    /// The value of `key`, if it is there.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.position(key).ok().map(|i| &self.entries[i].1)
    }

    /// The value of `key` to change, if it is there.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        match self.position(key) {
            Ok(i) => Some(&mut self.entries[i].1),
            Err(_) => None,
        }
    }

    /// `object.count(key) != 0`.
    #[must_use]
    pub fn contains_key(&self, key: &str) -> bool {
        self.position(key).is_ok()
    }

    /// `object.erase(key)`: the value that was there.
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        match self.position(key) {
            Ok(i) => Some(self.entries.remove(i).1),
            Err(_) => None,
        }
    }

    /// The members in the object's order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }

    /// The keys in the object's order.
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.entries.iter().map(|(k, _)| k)
    }

    /// The number of members.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the object has no members.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// A JSON value, as `nlohmann::basic_json` holds one.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// `number_integer` (`std::int64_t`).
    Int(i64),
    /// `number_unsigned` (`std::uint64_t`).
    UInt(u64),
    /// `number_float` (`double`).
    Float(f64),
    /// `string`.
    String(String),
    /// `array`.
    Array(Vec<Value>),
    /// `object`.
    Object(Object),
}

impl From<bool> for Value {
    fn from(v: bool) -> Value {
        Value::Bool(v)
    }
}

impl From<i32> for Value {
    fn from(v: i32) -> Value {
        Value::Int(i64::from(v))
    }
}

impl From<i64> for Value {
    fn from(v: i64) -> Value {
        Value::Int(v)
    }
}

impl From<u32> for Value {
    fn from(v: u32) -> Value {
        Value::UInt(u64::from(v))
    }
}

impl From<u64> for Value {
    fn from(v: u64) -> Value {
        Value::UInt(v)
    }
}

impl From<usize> for Value {
    fn from(v: usize) -> Value {
        Value::UInt(v as u64)
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Value {
        Value::Float(v)
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Value {
        Value::String(v.to_string())
    }
}

impl From<String> for Value {
    fn from(v: String) -> Value {
        Value::String(v)
    }
}

impl From<&String> for Value {
    fn from(v: &String) -> Value {
        Value::String(v.clone())
    }
}

impl From<Object> for Value {
    fn from(v: Object) -> Value {
        Value::Object(v)
    }
}

impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(v: Vec<T>) -> Value {
        Value::Array(v.into_iter().map(Into::into).collect())
    }
}

impl Value {
    /// An empty object of that order (`json::object()`).
    #[must_use]
    pub fn object(order: Order) -> Value {
        Value::Object(Object::new(order))
    }

    /// `type_name()`: "null", "boolean", "number", "string", "array" or "object".
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Int(_) | Value::UInt(_) | Value::Float(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        }
    }

    /// `is_null()`.
    #[must_use]
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// `is_number()`.
    #[must_use]
    pub fn is_number(&self) -> bool {
        matches!(self, Value::Int(_) | Value::UInt(_) | Value::Float(_))
    }

    /// `is_number_integer()`: a signed or an unsigned integer.
    #[must_use]
    pub fn is_number_integer(&self) -> bool {
        matches!(self, Value::Int(_) | Value::UInt(_))
    }

    /// The string, if it is one.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// The boolean, if it is one.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// The object, if it is one.
    #[must_use]
    pub fn as_object(&self) -> Option<&Object> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }

    /// The object to change, if it is one.
    pub fn as_object_mut(&mut self) -> Option<&mut Object> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }

    /// The array, if it is one.
    #[must_use]
    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    /// Any number as a double (`get<double>()`).
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::UInt(u) => Some(*u as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// A member of an object (`None` for a key that is not there or a value that is not an
    /// object).
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_object().and_then(|o| o.get(key))
    }

    /// `dump(indent, indent_char, ensure_ascii)`, as bytes: `indent` `None` is -1 (one line),
    /// `Some(n)` puts each member and element on a line of its own, `n` more `indent_char`s in
    /// per level (past 512 columns, spaces, as `nlohmann` widens its indent string).
    #[must_use]
    pub fn dump_bytes(
        &self,
        indent: Option<usize>,
        indent_char: u8,
        ensure_ascii: bool,
    ) -> Vec<u8> {
        let mut s = Serializer {
            out: Vec::new(),
            indent_char,
            ensure_ascii,
        };
        match indent {
            Some(step) => s.dump(self, true, step, 0),
            None => s.dump(self, false, 0, 0),
        }
        s.out
    }

    /// `dump(indent, ' ', false, ...)`, the front end's way: the error handler does not matter,
    /// since a [`Value`]'s strings are UTF-8.
    #[must_use]
    pub fn dump(&self, indent: Option<usize>) -> String {
        let bytes = self.dump_bytes(indent, b' ', false);
        String::from_utf8(bytes).expect("dump writes the UTF-8 it is given")
    }
}

impl std::ops::Index<&str> for Value {
    type Output = Value;

    fn index(&self, key: &str) -> &Value {
        self.get(key).unwrap_or_else(|| panic!("no member {key:?}"))
    }
}

/// `error_handler_t`: what `dump_escaped` does with bytes that are not UTF-8.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorHandler {
    /// Throw `type_error` 316.
    Strict,
    /// Replace each invalid sequence with U+FFFD.
    Replace,
    /// Leave each invalid sequence out.
    Ignore,
}

/// An exception of `nlohmann::json`: its kind, its id, its `what()` and, for a parse error, the
/// byte it was found at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    /// `parse_error`, `out_of_range` or `type_error`.
    pub kind: &'static str,
    /// The exception's id (101, 406, 316 ...).
    pub id: i32,
    /// `what()`.
    pub message: String,
    /// `parse_error::byte`: the characters read when the error was found (0 for the others).
    pub byte: usize,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

/// `exception::name(ename, id)`.
fn exception_name(kind: &str, id: i32) -> String {
    format!("[json.exception.{kind}.{id}] ")
}

// ------------------------------------------------------------------------------------ serializer

const UTF8_ACCEPT: u8 = 0;
const UTF8_REJECT: u8 = 1;

/// The table of `serializer::decode`: Bjoern Hoehrmann's UTF-8 automaton (byte classes, then the
/// transitions).
#[rustfmt::skip]
const UTF8D: [u8; 400] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 00..1F
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 20..3F
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 40..5F
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 60..7F
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, // 80..9F
    7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, // A0..BF
    8, 8, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, // C0..DF
    0xA, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 4, 3, 3, 0xB, 6, 6, 6, 5, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, // E0..FF
    0x0, 0x1, 0x2, 0x3, 0x5, 0x8, 0x7, 0x1, 0x1, 0x1, 0x4, 0x6, 0x1, 0x1, 0x1, 0x1, // s0..s0
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 1, // s1..s2
    1, 2, 1, 1, 1, 1, 1, 2, 1, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 1, 1, 1, 1, 1, 1, 1, 1, // s3..s4
    1, 2, 1, 1, 1, 1, 1, 1, 1, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 1, 3, 1, 1, 1, 1, 1, 1, // s5..s6
    1, 3, 1, 1, 1, 1, 1, 3, 1, 3, 1, 1, 1, 1, 1, 1, 1, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // s7..s8
];

/// `serializer::decode`: one step of the automaton.
fn decode(state: &mut u8, codepoint: &mut u32, byte: u8) -> u8 {
    let ty = UTF8D[usize::from(byte)];
    *codepoint = if *state == UTF8_ACCEPT {
        (0xFFu32 >> ty) & u32::from(byte)
    } else {
        (u32::from(byte) & 0x3F) | (*codepoint << 6)
    };
    *state = UTF8D[256 + usize::from(*state) * 16 + usize::from(ty)];
    *state
}

/// `dump_escaped(s, ensure_ascii)` with `error_handler`, appended to `out`.
fn dump_escaped(
    out: &mut Vec<u8>,
    s: &[u8],
    ensure_ascii: bool,
    handler: ErrorHandler,
) -> Result<(), Error> {
    let mut codepoint = 0u32;
    let mut state = UTF8_ACCEPT;
    let mut after_last_accept = out.len();
    let mut undumped_chars = 0usize;
    let mut i = 0usize;
    while i < s.len() {
        let byte = s[i];
        match decode(&mut state, &mut codepoint, byte) {
            UTF8_ACCEPT => {
                match codepoint {
                    0x08 => out.extend_from_slice(b"\\b"),
                    0x09 => out.extend_from_slice(b"\\t"),
                    0x0A => out.extend_from_slice(b"\\n"),
                    0x0C => out.extend_from_slice(b"\\f"),
                    0x0D => out.extend_from_slice(b"\\r"),
                    0x22 => out.extend_from_slice(b"\\\""),
                    0x5C => out.extend_from_slice(b"\\\\"),
                    _ => {
                        if codepoint <= 0x1F || (ensure_ascii && codepoint >= 0x7F) {
                            if codepoint <= 0xFFFF {
                                out.extend_from_slice(format!("\\u{codepoint:04x}").as_bytes());
                            } else {
                                let high = 0xD7C0 + (codepoint >> 10);
                                let low = 0xDC00 + (codepoint & 0x3FF);
                                out.extend_from_slice(
                                    format!("\\u{high:04x}\\u{low:04x}").as_bytes(),
                                );
                            }
                        } else {
                            // The bytes before this one were copied as they came.
                            out.push(byte);
                        }
                    }
                }
                after_last_accept = out.len();
                undumped_chars = 0;
            }
            UTF8_REJECT => match handler {
                ErrorHandler::Strict => {
                    return Err(Error {
                        kind: "type_error",
                        id: 316,
                        message: format!(
                            "{}invalid UTF-8 byte at index {i}: 0x{byte:02X}",
                            exception_name("type_error", 316)
                        ),
                        byte: 0,
                    });
                }
                ErrorHandler::Ignore | ErrorHandler::Replace => {
                    // A byte that broke a sequence may start one of its own: read it again.
                    if undumped_chars > 0 {
                        i -= 1;
                    }
                    out.truncate(after_last_accept);
                    if handler == ErrorHandler::Replace {
                        out.extend_from_slice(if ensure_ascii {
                            b"\\ufffd".as_slice()
                        } else {
                            b"\xEF\xBF\xBD".as_slice()
                        });
                        after_last_accept = out.len();
                    }
                    undumped_chars = 0;
                    state = UTF8_ACCEPT;
                }
            },
            _ => {
                if !ensure_ascii {
                    out.push(byte);
                }
                undumped_chars += 1;
            }
        }
        i += 1;
    }
    if state != UTF8_ACCEPT {
        match handler {
            ErrorHandler::Strict => {
                let last = s[s.len() - 1];
                return Err(Error {
                    kind: "type_error",
                    id: 316,
                    message: format!(
                        "{}incomplete UTF-8 string; last byte: 0x{last:02X}",
                        exception_name("type_error", 316)
                    ),
                    byte: 0,
                });
            }
            ErrorHandler::Ignore => out.truncate(after_last_accept),
            ErrorHandler::Replace => {
                out.truncate(after_last_accept);
                out.extend_from_slice(if ensure_ascii {
                    b"\\ufffd".as_slice()
                } else {
                    b"\xEF\xBF\xBD".as_slice()
                });
            }
        }
    }
    Ok(())
}

/// `dump_escaped` of the bytes of a `std::string` that need not be UTF-8, without the quotes.
///
/// # Errors
///
/// With [`ErrorHandler::Strict`], `type_error` 316 ("invalid UTF-8 byte at index I: 0xBB" or
/// "incomplete UTF-8 string; last byte: 0xBB") for bytes that are not UTF-8.
pub fn escape_bytes(s: &[u8], ensure_ascii: bool, handler: ErrorHandler) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    dump_escaped(&mut out, s, ensure_ascii, handler)?;
    Ok(out)
}

/// The string `dump_escaped` treats bytes as: each sequence that is not UTF-8 replaced by U+FFFD
/// ([`ErrorHandler::Replace`]) or left out ([`ErrorHandler::Ignore`]), as `dump_escaped` finds
/// them (so dumping the result gives what dumping the bytes gives).
///
/// # Errors
///
/// With [`ErrorHandler::Strict`], `type_error` 316, as [`escape_bytes`].
pub fn string_from_bytes(s: &[u8], handler: ErrorHandler) -> Result<String, Error> {
    let mut out = Vec::with_capacity(s.len());
    let mut codepoint = 0u32;
    let mut state = UTF8_ACCEPT;
    let mut after_last_accept = 0usize;
    let mut undumped_chars = 0usize;
    let mut i = 0usize;
    let replacement = "\u{FFFD}".as_bytes();
    while i < s.len() {
        let byte = s[i];
        match decode(&mut state, &mut codepoint, byte) {
            UTF8_ACCEPT => {
                out.push(byte);
                after_last_accept = out.len();
                undumped_chars = 0;
            }
            UTF8_REJECT => {
                if handler == ErrorHandler::Strict {
                    return escape_bytes(s, false, handler).map(|_| String::new());
                }
                if undumped_chars > 0 {
                    i -= 1;
                }
                out.truncate(after_last_accept);
                if handler == ErrorHandler::Replace {
                    out.extend_from_slice(replacement);
                    after_last_accept = out.len();
                }
                undumped_chars = 0;
                state = UTF8_ACCEPT;
            }
            _ => {
                out.push(byte);
                undumped_chars += 1;
            }
        }
        i += 1;
    }
    if state != UTF8_ACCEPT {
        match handler {
            ErrorHandler::Strict => return escape_bytes(s, false, handler).map(|_| String::new()),
            ErrorHandler::Ignore => out.truncate(after_last_accept),
            ErrorHandler::Replace => {
                out.truncate(after_last_accept);
                out.extend_from_slice(replacement);
            }
        }
    }
    Ok(String::from_utf8(out).expect("the automaton accepts only UTF-8"))
}

struct Serializer {
    out: Vec<u8>,
    indent_char: u8,
    ensure_ascii: bool,
}

impl Serializer {
    /// `indent_string`: 512 `indent_char`s, widened with spaces.
    fn indent(&mut self, n: usize) {
        let own = n.min(512);
        self.out.resize(self.out.len() + own, self.indent_char);
        self.out.resize(self.out.len() + (n - own), b' ');
    }

    fn string(&mut self, s: &str) {
        self.out.push(b'"');
        dump_escaped(
            &mut self.out,
            s.as_bytes(),
            self.ensure_ascii,
            ErrorHandler::Strict,
        )
        .expect("a Rust string is UTF-8");
        self.out.push(b'"');
    }

    fn dump(&mut self, val: &Value, pretty: bool, step: usize, current: usize) {
        match val {
            Value::Object(o) => {
                if o.is_empty() {
                    self.out.extend_from_slice(b"{}");
                    return;
                }
                let last = o.len() - 1;
                if pretty {
                    self.out.extend_from_slice(b"{\n");
                    let new_indent = current + step;
                    for (i, (k, v)) in o.entries.iter().enumerate() {
                        self.indent(new_indent);
                        self.string(k);
                        self.out.extend_from_slice(b": ");
                        self.dump(v, true, step, new_indent);
                        if i < last {
                            self.out.extend_from_slice(b",\n");
                        }
                    }
                    self.out.push(b'\n');
                    self.indent(current);
                    self.out.push(b'}');
                } else {
                    self.out.push(b'{');
                    for (i, (k, v)) in o.entries.iter().enumerate() {
                        self.string(k);
                        self.out.push(b':');
                        self.dump(v, false, step, current);
                        if i < last {
                            self.out.push(b',');
                        }
                    }
                    self.out.push(b'}');
                }
            }
            Value::Array(a) => {
                if a.is_empty() {
                    self.out.extend_from_slice(b"[]");
                    return;
                }
                let last = a.len() - 1;
                if pretty {
                    self.out.extend_from_slice(b"[\n");
                    let new_indent = current + step;
                    for (i, v) in a.iter().enumerate() {
                        self.indent(new_indent);
                        self.dump(v, true, step, new_indent);
                        if i < last {
                            self.out.extend_from_slice(b",\n");
                        }
                    }
                    self.out.push(b'\n');
                    self.indent(current);
                    self.out.push(b']');
                } else {
                    self.out.push(b'[');
                    for (i, v) in a.iter().enumerate() {
                        self.dump(v, false, step, current);
                        if i < last {
                            self.out.push(b',');
                        }
                    }
                    self.out.push(b']');
                }
            }
            Value::String(s) => self.string(s),
            Value::Bool(b) => self
                .out
                .extend_from_slice(if *b { b"true" } else { b"false" }),
            Value::Int(i) => self.out.extend_from_slice(i.to_string().as_bytes()),
            Value::UInt(u) => self.out.extend_from_slice(u.to_string().as_bytes()),
            Value::Float(f) => {
                if f.is_finite() {
                    dtoa::to_chars(&mut self.out, *f);
                } else {
                    self.out.extend_from_slice(b"null");
                }
            }
            Value::Null => self.out.extend_from_slice(b"null"),
        }
    }
}

// ----------------------------------------------------------------------------------------- lexer

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Token {
    Uninitialized,
    LiteralTrue,
    LiteralFalse,
    LiteralNull,
    ValueString,
    ValueUnsigned,
    ValueInteger,
    ValueFloat,
    BeginArray,
    BeginObject,
    EndArray,
    EndObject,
    NameSeparator,
    ValueSeparator,
    ParseError,
    EndOfInput,
    LiteralOrValue,
}

/// `lexer::token_type_name`.
fn token_type_name(t: Token) -> &'static str {
    match t {
        Token::Uninitialized => "<uninitialized>",
        Token::LiteralTrue => "true literal",
        Token::LiteralFalse => "false literal",
        Token::LiteralNull => "null literal",
        Token::ValueString => "string literal",
        Token::ValueUnsigned | Token::ValueInteger | Token::ValueFloat => "number literal",
        Token::BeginArray => "'['",
        Token::BeginObject => "'{'",
        Token::EndArray => "']'",
        Token::EndObject => "'}'",
        Token::NameSeparator => "':'",
        Token::ValueSeparator => "','",
        Token::ParseError => "<parse error>",
        Token::EndOfInput => "end of input",
        Token::LiteralOrValue => "'[', '{', or a literal",
    }
}

/// `std::char_traits<char>::eof()`.
const EOF: i32 = -1;

/// `position_t`.
#[derive(Clone, Copy, Default)]
struct Position {
    chars_read_total: usize,
    chars_read_current_line: usize,
    lines_read: usize,
}

const CONTROL_MESSAGES: [&str; 32] = [
    "invalid string: control character U+0000 (NUL) must be escaped to \\u0000",
    "invalid string: control character U+0001 (SOH) must be escaped to \\u0001",
    "invalid string: control character U+0002 (STX) must be escaped to \\u0002",
    "invalid string: control character U+0003 (ETX) must be escaped to \\u0003",
    "invalid string: control character U+0004 (EOT) must be escaped to \\u0004",
    "invalid string: control character U+0005 (ENQ) must be escaped to \\u0005",
    "invalid string: control character U+0006 (ACK) must be escaped to \\u0006",
    "invalid string: control character U+0007 (BEL) must be escaped to \\u0007",
    "invalid string: control character U+0008 (BS) must be escaped to \\u0008 or \\b",
    "invalid string: control character U+0009 (HT) must be escaped to \\u0009 or \\t",
    "invalid string: control character U+000A (LF) must be escaped to \\u000A or \\n",
    "invalid string: control character U+000B (VT) must be escaped to \\u000B",
    "invalid string: control character U+000C (FF) must be escaped to \\u000C or \\f",
    "invalid string: control character U+000D (CR) must be escaped to \\u000D or \\r",
    "invalid string: control character U+000E (SO) must be escaped to \\u000E",
    "invalid string: control character U+000F (SI) must be escaped to \\u000F",
    "invalid string: control character U+0010 (DLE) must be escaped to \\u0010",
    "invalid string: control character U+0011 (DC1) must be escaped to \\u0011",
    "invalid string: control character U+0012 (DC2) must be escaped to \\u0012",
    "invalid string: control character U+0013 (DC3) must be escaped to \\u0013",
    "invalid string: control character U+0014 (DC4) must be escaped to \\u0014",
    "invalid string: control character U+0015 (NAK) must be escaped to \\u0015",
    "invalid string: control character U+0016 (SYN) must be escaped to \\u0016",
    "invalid string: control character U+0017 (ETB) must be escaped to \\u0017",
    "invalid string: control character U+0018 (CAN) must be escaped to \\u0018",
    "invalid string: control character U+0019 (EM) must be escaped to \\u0019",
    "invalid string: control character U+001A (SUB) must be escaped to \\u001A",
    "invalid string: control character U+001B (ESC) must be escaped to \\u001B",
    "invalid string: control character U+001C (FS) must be escaped to \\u001C",
    "invalid string: control character U+001D (GS) must be escaped to \\u001D",
    "invalid string: control character U+001E (RS) must be escaped to \\u001E",
    "invalid string: control character U+001F (US) must be escaped to \\u001F",
];

const ILL_FORMED: &str = "invalid string: ill-formed UTF-8 byte";
const NEED_HEX: &str = "invalid string: '\\u' must be followed by 4 hex digits";
const NEED_LOW: &str =
    "invalid string: surrogate U+D800..U+DBFF must be followed by U+DC00..U+DFFF";

struct Lexer<'a> {
    input: &'a [u8],
    at: usize,
    current: i32,
    next_unget: bool,
    position: Position,
    token_string: Vec<u8>,
    token_buffer: Vec<u8>,
    error_message: &'static str,
    value_integer: i64,
    value_unsigned: u64,
    value_float: f64,
}

/// The states of `scan_number`'s gotos.
#[derive(Clone, Copy)]
enum NumberState {
    Minus,
    Zero,
    Any1,
    Decimal1,
    Decimal2,
    Exponent,
    Sign,
    Any2,
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
impl<'a> Lexer<'a> {
    fn new(input: &'a [u8]) -> Lexer<'a> {
        Lexer {
            input,
            at: 0,
            current: EOF,
            next_unget: false,
            position: Position::default(),
            token_string: Vec::new(),
            token_buffer: Vec::new(),
            error_message: "",
            value_integer: 0,
            value_unsigned: 0,
            value_float: 0.0,
        }
    }

    fn get(&mut self) -> i32 {
        self.position.chars_read_total += 1;
        self.position.chars_read_current_line += 1;
        if self.next_unget {
            self.next_unget = false;
        } else {
            self.current = match self.input.get(self.at) {
                Some(&b) => {
                    self.at += 1;
                    i32::from(b)
                }
                None => EOF,
            };
        }
        if self.current != EOF {
            self.token_string.push(self.current as u8);
        }
        if self.current == i32::from(b'\n') {
            self.position.lines_read += 1;
            self.position.chars_read_current_line = 0;
        }
        self.current
    }

    fn unget(&mut self) {
        self.next_unget = true;
        self.position.chars_read_total -= 1;
        if self.position.chars_read_current_line == 0 {
            if self.position.lines_read > 0 {
                self.position.lines_read -= 1;
            }
        } else {
            self.position.chars_read_current_line -= 1;
        }
        if self.current != EOF {
            self.token_string.pop();
        }
    }

    fn add(&mut self, c: i32) {
        self.token_buffer.push(c as u8);
    }

    fn reset(&mut self) {
        self.token_buffer.clear();
        self.token_string.clear();
        self.token_string.push(self.current as u8);
    }

    /// `get_token_string`: control characters as `<U+XXXX>`.
    fn token_string(&self) -> String {
        let mut out = Vec::new();
        for &c in &self.token_string {
            if c <= 0x1F {
                out.extend_from_slice(format!("<U+{c:04X}>").as_bytes());
            } else {
                out.push(c);
            }
        }
        // what() is a C string of these bytes; the message is kept as text.
        string_from_bytes(&out, ErrorHandler::Replace).expect("replace refuses nothing")
    }

    fn get_codepoint(&mut self) -> i32 {
        let mut codepoint = 0i32;
        for factor in [12, 8, 4, 0] {
            let c = self.get();
            let digit = match c {
                0x30..=0x39 => c - 0x30,
                0x41..=0x46 => c - 0x37,
                0x61..=0x66 => c - 0x57,
                _ => return -1,
            };
            codepoint += digit << factor;
        }
        codepoint
    }

    fn next_byte_in_range(&mut self, ranges: &[i32]) -> bool {
        self.add(self.current);
        for pair in ranges.chunks(2) {
            let c = self.get();
            if pair[0] <= c && c <= pair[1] {
                self.add(c);
            } else {
                self.error_message = ILL_FORMED;
                return false;
            }
        }
        true
    }

    fn scan_unicode_escape(&mut self) -> Result<(), &'static str> {
        let codepoint1 = self.get_codepoint();
        let mut codepoint = codepoint1;
        if codepoint1 == -1 {
            return Err(NEED_HEX);
        }
        if (0xD800..=0xDBFF).contains(&codepoint1) {
            if self.get() == 0x5C && self.get() == 0x75 {
                let codepoint2 = self.get_codepoint();
                if codepoint2 == -1 {
                    return Err(NEED_HEX);
                }
                if (0xDC00..=0xDFFF).contains(&codepoint2) {
                    codepoint = ((codepoint1 << 10) + codepoint2) - 0x035F_DC00;
                } else {
                    return Err(NEED_LOW);
                }
            } else {
                return Err(NEED_LOW);
            }
        } else if (0xDC00..=0xDFFF).contains(&codepoint1) {
            return Err("invalid string: surrogate U+DC00..U+DFFF must follow U+D800..U+DBFF");
        }
        if codepoint < 0x80 {
            self.add(codepoint);
        } else if codepoint <= 0x7FF {
            self.add(0xC0 | (codepoint >> 6));
            self.add(0x80 | (codepoint & 0x3F));
        } else if codepoint <= 0xFFFF {
            self.add(0xE0 | (codepoint >> 12));
            self.add(0x80 | ((codepoint >> 6) & 0x3F));
            self.add(0x80 | (codepoint & 0x3F));
        } else {
            self.add(0xF0 | (codepoint >> 18));
            self.add(0x80 | ((codepoint >> 12) & 0x3F));
            self.add(0x80 | ((codepoint >> 6) & 0x3F));
            self.add(0x80 | (codepoint & 0x3F));
        }
        Ok(())
    }

    fn scan_string(&mut self) -> Token {
        self.reset();
        loop {
            let c = self.get();
            let ranges: &[i32] = match c {
                EOF => {
                    self.error_message = "invalid string: missing closing quote";
                    return Token::ParseError;
                }
                0x22 => return Token::ValueString,
                0x5C => {
                    match self.get() {
                        0x22 => self.add(0x22),
                        0x5C => self.add(0x5C),
                        0x2F => self.add(0x2F),
                        0x62 => self.add(0x08),
                        0x66 => self.add(0x0C),
                        0x6E => self.add(0x0A),
                        0x72 => self.add(0x0D),
                        0x74 => self.add(0x09),
                        0x75 => {
                            if let Err(message) = self.scan_unicode_escape() {
                                self.error_message = message;
                                return Token::ParseError;
                            }
                        }
                        _ => {
                            self.error_message =
                                "invalid string: forbidden character after backslash";
                            return Token::ParseError;
                        }
                    }
                    continue;
                }
                0x00..=0x1F => {
                    self.error_message = CONTROL_MESSAGES[c as usize];
                    return Token::ParseError;
                }
                0x20..=0x7F => {
                    self.add(c);
                    continue;
                }
                0xC2..=0xDF => &[0x80, 0xBF],
                0xE0 => &[0xA0, 0xBF, 0x80, 0xBF],
                0xE1..=0xEC | 0xEE | 0xEF => &[0x80, 0xBF, 0x80, 0xBF],
                0xED => &[0x80, 0x9F, 0x80, 0xBF],
                0xF0 => &[0x90, 0xBF, 0x80, 0xBF, 0x80, 0xBF],
                0xF1..=0xF3 => &[0x80, 0xBF, 0x80, 0xBF, 0x80, 0xBF],
                0xF4 => &[0x80, 0x8F, 0x80, 0xBF, 0x80, 0xBF],
                _ => {
                    self.error_message = ILL_FORMED;
                    return Token::ParseError;
                }
            };
            if !self.next_byte_in_range(ranges) {
                return Token::ParseError;
            }
        }
    }

    fn is_digit(c: i32) -> bool {
        (0x30..=0x39).contains(&c)
    }

    fn scan_number(&mut self) -> Token {
        self.reset();
        let mut number_type = Token::ValueUnsigned;
        self.add(self.current);
        let mut state = match self.current {
            0x2D => NumberState::Minus,
            0x30 => NumberState::Zero,
            _ => NumberState::Any1,
        };
        loop {
            state = match state {
                NumberState::Minus => {
                    number_type = Token::ValueInteger;
                    let c = self.get();
                    if c == 0x30 {
                        self.add(c);
                        NumberState::Zero
                    } else if Self::is_digit(c) {
                        self.add(c);
                        NumberState::Any1
                    } else {
                        self.error_message = "invalid number; expected digit after '-'";
                        return Token::ParseError;
                    }
                }
                NumberState::Zero => match self.get() {
                    0x2E => {
                        self.add(0x2E);
                        NumberState::Decimal1
                    }
                    c @ (0x65 | 0x45) => {
                        self.add(c);
                        NumberState::Exponent
                    }
                    _ => break,
                },
                NumberState::Any1 => match self.get() {
                    c if Self::is_digit(c) => {
                        self.add(c);
                        NumberState::Any1
                    }
                    0x2E => {
                        self.add(0x2E);
                        NumberState::Decimal1
                    }
                    c @ (0x65 | 0x45) => {
                        self.add(c);
                        NumberState::Exponent
                    }
                    _ => break,
                },
                NumberState::Decimal1 => {
                    number_type = Token::ValueFloat;
                    let c = self.get();
                    if Self::is_digit(c) {
                        self.add(c);
                        NumberState::Decimal2
                    } else {
                        self.error_message = "invalid number; expected digit after '.'";
                        return Token::ParseError;
                    }
                }
                NumberState::Decimal2 => match self.get() {
                    c if Self::is_digit(c) => {
                        self.add(c);
                        NumberState::Decimal2
                    }
                    c @ (0x65 | 0x45) => {
                        self.add(c);
                        NumberState::Exponent
                    }
                    _ => break,
                },
                NumberState::Exponent => {
                    number_type = Token::ValueFloat;
                    match self.get() {
                        c @ (0x2B | 0x2D) => {
                            self.add(c);
                            NumberState::Sign
                        }
                        c if Self::is_digit(c) => {
                            self.add(c);
                            NumberState::Any2
                        }
                        _ => {
                            self.error_message =
                                "invalid number; expected '+', '-', or digit after exponent";
                            return Token::ParseError;
                        }
                    }
                }
                NumberState::Sign => {
                    let c = self.get();
                    if Self::is_digit(c) {
                        self.add(c);
                        NumberState::Any2
                    } else {
                        self.error_message = "invalid number; expected digit after exponent sign";
                        return Token::ParseError;
                    }
                }
                NumberState::Any2 => {
                    let c = self.get();
                    if Self::is_digit(c) {
                        self.add(c);
                        NumberState::Any2
                    } else {
                        break;
                    }
                }
            };
        }
        // The character after the number was read only to end it.
        self.unget();
        if number_type == Token::ValueUnsigned {
            let r = orca_fmt::parse::strtoull(&self.token_buffer, 10);
            if r.errno.is_none() {
                self.value_unsigned = r.value;
                return Token::ValueUnsigned;
            }
        } else if number_type == Token::ValueInteger {
            let r = orca_fmt::parse::strtoll(&self.token_buffer, 10);
            if r.errno.is_none() {
                self.value_integer = r.value;
                return Token::ValueInteger;
            }
        }
        // A fraction or an exponent, or an integer out of range.
        self.value_float = orca_fmt::parse::strtod(&self.token_buffer).value;
        Token::ValueFloat
    }

    fn scan_literal(&mut self, literal: &[u8], token: Token) -> Token {
        for &expected in &literal[1..] {
            // to_char_type(get()): EOF is 0xFF, which no literal holds.
            if self.get() != i32::from(expected) {
                self.error_message = "invalid literal";
                return Token::ParseError;
            }
        }
        token
    }

    fn skip_bom(&mut self) -> bool {
        if self.get() == 0xEF {
            return self.get() == 0xBB && self.get() == 0xBF;
        }
        self.unget();
        true
    }

    fn scan(&mut self) -> Token {
        if self.position.chars_read_total == 0 && !self.skip_bom() {
            self.error_message = "invalid BOM; must be 0xEF 0xBB 0xBF if given";
            return Token::ParseError;
        }
        loop {
            self.get();
            if !matches!(self.current, 0x20 | 0x09 | 0x0A | 0x0D) {
                break;
            }
        }
        match self.current {
            0x5B => Token::BeginArray,
            0x5D => Token::EndArray,
            0x7B => Token::BeginObject,
            0x7D => Token::EndObject,
            0x3A => Token::NameSeparator,
            0x2C => Token::ValueSeparator,
            0x74 => self.scan_literal(b"true", Token::LiteralTrue),
            0x66 => self.scan_literal(b"false", Token::LiteralFalse),
            0x6E => self.scan_literal(b"null", Token::LiteralNull),
            0x22 => self.scan_string(),
            0x2D | 0x30..=0x39 => self.scan_number(),
            // The null byte ends a text as the end of input does.
            0x00 | EOF => Token::EndOfInput,
            _ => {
                self.error_message = "invalid literal";
                Token::ParseError
            }
        }
    }

    /// `get_string()`: the string just scanned (UTF-8, as `scan_string` checked).
    fn string(&self) -> String {
        String::from_utf8(self.token_buffer.clone()).expect("scan_string accepts only UTF-8")
    }
}

// ---------------------------------------------------------------------------------------- parser

/// A container being filled, with the key its next member goes under.
enum Frame {
    Array(Vec<Value>),
    Object(Object, String),
}

/// A value that is complete: into the container being filled, or the result.
fn attach(stack: &mut [Frame], root: &mut Option<Value>, v: Value) {
    match stack.last_mut() {
        None => *root = Some(v),
        Some(Frame::Array(a)) => a.push(v),
        Some(Frame::Object(o, key)) => o.insert(std::mem::take(key), v),
    }
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    last_token: Token,
    order: Order,
}

impl Parser<'_> {
    fn get_token(&mut self) -> Token {
        self.last_token = self.lexer.scan();
        self.last_token
    }

    fn exception_message(&self, expected: Token, context: &str) -> String {
        let mut msg = String::from("syntax error ");
        if !context.is_empty() {
            msg.push_str("while parsing ");
            msg.push_str(context);
            msg.push(' ');
        }
        msg.push_str("- ");
        if self.last_token == Token::ParseError {
            msg.push_str(self.lexer.error_message);
            msg.push_str("; last read: '");
            msg.push_str(&self.lexer.token_string());
            msg.push('\'');
        } else {
            msg.push_str("unexpected ");
            msg.push_str(token_type_name(self.last_token));
        }
        if expected != Token::Uninitialized {
            msg.push_str("; expected ");
            msg.push_str(token_type_name(expected));
        }
        msg
    }

    fn parse_error(&self, expected: Token, context: &str) -> Error {
        let pos = self.lexer.position;
        Error {
            kind: "parse_error",
            id: 101,
            message: format!(
                "{}parse error at line {}, column {}: {}",
                exception_name("parse_error", 101),
                pos.lines_read + 1,
                pos.chars_read_current_line,
                self.exception_message(expected, context)
            ),
            byte: pos.chars_read_total,
        }
    }

    /// `sax_parse_internal` with the DOM parser.
    fn parse_value(&mut self) -> Result<Value, Error> {
        let mut stack: Vec<Frame> = Vec::new();
        let mut root: Option<Value> = None;
        let mut skip_to_state_evaluation = false;
        loop {
            if skip_to_state_evaluation {
                skip_to_state_evaluation = false;
            } else {
                let value = match self.last_token {
                    Token::BeginObject => {
                        if self.get_token() == Token::EndObject {
                            Value::Object(Object::new(self.order))
                        } else {
                            if self.last_token != Token::ValueString {
                                return Err(self.parse_error(Token::ValueString, "object key"));
                            }
                            let key = self.lexer.string();
                            if self.get_token() != Token::NameSeparator {
                                return Err(
                                    self.parse_error(Token::NameSeparator, "object separator")
                                );
                            }
                            stack.push(Frame::Object(Object::new(self.order), key));
                            self.get_token();
                            continue;
                        }
                    }
                    Token::BeginArray => {
                        if self.get_token() == Token::EndArray {
                            Value::Array(Vec::new())
                        } else {
                            stack.push(Frame::Array(Vec::new()));
                            continue;
                        }
                    }
                    Token::ValueFloat => {
                        let res = self.lexer.value_float;
                        if !res.is_finite() {
                            return Err(Error {
                                kind: "out_of_range",
                                id: 406,
                                message: format!(
                                    "{}number overflow parsing '{}'",
                                    exception_name("out_of_range", 406),
                                    self.lexer.token_string()
                                ),
                                byte: 0,
                            });
                        }
                        Value::Float(res)
                    }
                    Token::LiteralFalse => Value::Bool(false),
                    Token::LiteralNull => Value::Null,
                    Token::LiteralTrue => Value::Bool(true),
                    Token::ValueInteger => Value::Int(self.lexer.value_integer),
                    Token::ValueString => Value::String(self.lexer.string()),
                    Token::ValueUnsigned => Value::UInt(self.lexer.value_unsigned),
                    // "uninitialized" leaves the "expected" part out.
                    Token::ParseError => {
                        return Err(self.parse_error(Token::Uninitialized, "value"));
                    }
                    _ => return Err(self.parse_error(Token::LiteralOrValue, "value")),
                };
                attach(&mut stack, &mut root, value);
            }
            // A value was parsed.
            match stack.last() {
                None => return Ok(root.expect("a value was parsed")),
                Some(Frame::Array(_)) => {
                    if self.get_token() == Token::ValueSeparator {
                        self.get_token();
                        continue;
                    }
                    if self.last_token == Token::EndArray {
                        if let Some(Frame::Array(a)) = stack.pop() {
                            attach(&mut stack, &mut root, Value::Array(a));
                        }
                        skip_to_state_evaluation = true;
                        continue;
                    }
                    return Err(self.parse_error(Token::EndArray, "array"));
                }
                Some(Frame::Object(..)) => {
                    if self.get_token() == Token::ValueSeparator {
                        if self.get_token() != Token::ValueString {
                            return Err(self.parse_error(Token::ValueString, "object key"));
                        }
                        let key = self.lexer.string();
                        if let Some(Frame::Object(_, k)) = stack.last_mut() {
                            *k = key;
                        }
                        if self.get_token() != Token::NameSeparator {
                            return Err(self.parse_error(Token::NameSeparator, "object separator"));
                        }
                        self.get_token();
                        continue;
                    }
                    if self.last_token == Token::EndObject {
                        if let Some(Frame::Object(o, _)) = stack.pop() {
                            attach(&mut stack, &mut root, Value::Object(o));
                        }
                        skip_to_state_evaluation = true;
                        continue;
                    }
                    return Err(self.parse_error(Token::EndObject, "object"));
                }
            }
        }
    }
}

/// `basic_json::parse(text)` (strict, no callback, no comments): the value, its objects in
/// `order`.
///
/// # Errors
///
/// `parse_error` 101, with the line, the column and what was wrong, or `out_of_range` 406 for a
/// number too big for a double, with `nlohmann`'s `what()`.
pub fn parse(text: &[u8], order: Order) -> Result<Value, Error> {
    let mut p = Parser {
        lexer: Lexer::new(text),
        last_token: Token::Uninitialized,
        order,
    };
    p.get_token();
    let value = p.parse_value()?;
    if p.get_token() != Token::EndOfInput {
        return Err(p.parse_error(Token::EndOfInput, "value"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::{Order, Value, parse};

    #[test]
    fn a_number_keeps_its_kind() {
        assert_eq!(parse(b"1", Order::Insertion).unwrap(), Value::UInt(1));
        assert_eq!(parse(b"-1", Order::Insertion).unwrap(), Value::Int(-1));
        assert_eq!(parse(b"1.0", Order::Insertion).unwrap(), Value::Float(1.0));
        assert_eq!(
            parse(b"18446744073709551616", Order::Insertion).unwrap(),
            Value::Float(18_446_744_073_709_551_616.0)
        );
    }
}
