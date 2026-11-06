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
// Carries OrcaSlicer code: serialize and deserialize of each ConfigOption type are translated from
// src/libslic3r/Config.hpp (ConfigOptionFloat through ConfigOptionEnumsGenericTempl, and
// ConfigOptionDef::create_empty_option's choice of type) and escape_string_cstyle,
// escape_strings_cstyle, unescape_string_cstyle and unescape_strings_cstyle from
// src/libslic3r/Config.cpp of OrcaSlicer v2.4.2, commit 8500fcd, on 2026-11-06; their copyright
// is the OrcaSlicer contributors', under the AGPL-3.0.

//! The values of Orca's `ConfigOption` types and their text: [`OptionValue::serialize`] and
//! [`OptionValue::deserialize`], as `ConfigOption::serialize` and `deserialize` write and read it.
//!
//! Numbers go through orca-fmt: `ostream << double` ([`orca_fmt::ostream`]) out, and
//! `istringstream >> double` / `>> int` ([`orca_fmt::istream`]) in; a point's `sscanf(" %lf ,
//! %lf %c")` reads each number as `strtod` does. Where the C++ reads into an uninitialised local
//! (`double value; iss >> value;` with nothing to read), the value is indeterminate there; here it
//! is 0. Where it throws `ConfigurationError`, here it is [`ConfigError`].

use orca_fmt::istream::{get_f64, get_i32, is_space};
use orca_fmt::ostream::{Stream, f64_to_string};

use crate::OptionType;

/// `Slic3r::ConfigurationError`: what serialize and deserialize throw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigError(pub String);

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ConfigError {}

/// An enum's names and values (`t_config_enum_values`, a `std::map` by name).
pub type EnumKeys = &'static [(&'static str, i32)];

/// The value of an option, one variant per `ConfigOption` type. A vector type that takes "nil"
/// (`...Nullable`) says so; a nil element is NaN for doubles, `i32::MAX` for ints and enums, and
/// 255 for bools, as in the C++.
#[derive(Clone, Debug, PartialEq)]
pub enum OptionValue {
    /// `ConfigOptionFloat`.
    Float(f64),
    /// `ConfigOptionFloats` (`Nullable`).
    Floats { values: Vec<f64>, nullable: bool },
    /// `ConfigOptionInt`.
    Int(i32),
    /// `ConfigOptionInts` (`Nullable`).
    Ints { values: Vec<i32>, nullable: bool },
    /// `ConfigOptionString`.
    String(String),
    /// `ConfigOptionStrings`.
    Strings(Vec<String>),
    /// `ConfigOptionPercent`.
    Percent(f64),
    /// `ConfigOptionPercents` (`Nullable`).
    Percents { values: Vec<f64>, nullable: bool },
    /// `ConfigOptionFloatOrPercent`.
    FloatOrPercent { value: f64, percent: bool },
    /// `ConfigOptionFloatsOrPercents` (`Nullable`).
    FloatsOrPercents {
        values: Vec<(f64, bool)>,
        nullable: bool,
    },
    /// `ConfigOptionPoint`.
    Point([f64; 2]),
    /// `ConfigOptionPoints`.
    Points(Vec<[f64; 2]>),
    /// `ConfigOptionPoint3`.
    Point3([f64; 3]),
    /// `ConfigOptionBool`.
    Bool(bool),
    /// `ConfigOptionBools` (`Nullable`): 0, 1, or 255 for nil.
    Bools { values: Vec<u8>, nullable: bool },
    /// `ConfigOptionEnum<T>` and `ConfigOptionEnumGeneric`: the value and the enum's names.
    Enum { value: i32, keys: Option<EnumKeys> },
    /// `ConfigOptionEnumsGeneric` (`Nullable`).
    Enums {
        values: Vec<i32>,
        keys: Option<EnumKeys>,
        nullable: bool,
    },
    /// `ConfigOptionPointsGroups`.
    PointsGroups(Vec<Vec<[f64; 2]>>),
    /// `ConfigOptionIntsGroups`.
    IntsGroups(Vec<Vec<i32>>),
}

/// `ss << v` for a double: precision 6, `%g`.
fn put(v: f64) -> String {
    f64_to_string(&mut Stream::default(), v).expect("precision 6 always formats")
}

/// `boost::trim`: white space off both ends.
fn trim(s: &[u8]) -> &[u8] {
    let start = s.iter().position(|&c| !is_space(c)).unwrap_or(s.len());
    let end = s
        .iter()
        .rposition(|&c| !is_space(c))
        .map_or(start, |i| i + 1);
    // An all-space text gives start == len and end == start, so end is never before start.
    &s[start..end]
}

/// The pieces `while (std::getline(is, item, delim))` reads: none for an empty text, and no
/// empty piece after a final delimiter.
fn getline_split(s: &[u8], delim: u8) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < s.len() {
        match s[pos..].iter().position(|&c| c == delim) {
            Some(i) => {
                out.push(&s[pos..pos + i]);
                pos += i + 1;
            }
            None => {
                out.push(&s[pos..]);
                pos = s.len();
            }
        }
    }
    out
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).expect("escaping and unescaping keep UTF-8 whole")
}

/// `escape_string_cstyle`: `\r`, `\n`, `\\` and `"` escaped.
#[must_use]
pub fn escape_string_cstyle(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len() * 2);
    for &c in s.as_bytes() {
        match c {
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\\' | b'"' => {
                out.push(b'\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    text(out)
}

/// `escape_strings_cstyle`: the strings joined by `;`, each quoted and escaped when it holds a
/// space, a tab, a backslash, a quote, `\r` or `\n` (and an empty string when it is the only one).
#[must_use]
pub fn escape_strings_cstyle(strs: &[String]) -> String {
    let mut out = Vec::new();
    for (j, s) in strs.iter().enumerate() {
        if j > 0 {
            out.push(b';');
        }
        let quote = (strs.len() == 1 && s.is_empty())
            || s.bytes()
                .any(|c| matches!(c, b' ' | b'\t' | b'\\' | b'"' | b'\r' | b'\n'));
        if quote {
            out.push(b'"');
            for &c in s.as_bytes() {
                match c {
                    b'\\' | b'"' => {
                        out.push(b'\\');
                        out.push(c);
                    }
                    b'\r' => out.extend_from_slice(b"\\r"),
                    b'\n' => out.extend_from_slice(b"\\n"),
                    _ => out.push(c),
                }
            }
            out.push(b'"');
        } else {
            out.extend_from_slice(s.as_bytes());
        }
    }
    text(out)
}

/// `unescape_string_cstyle`: `None` for a text that ends in a lone backslash.
#[must_use]
pub fn unescape_string_cstyle(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i];
        if c == b'\\' {
            i += 1;
            if i == b.len() {
                return None;
            }
            out.push(match b[i] {
                b'r' => b'\r',
                b'n' => b'\n',
                other => other,
            });
        } else {
            out.push(c);
        }
        i += 1;
    }
    Some(text(out))
}

/// `unescape_strings_cstyle`: the strings read into `out` (which keeps what it read when the
/// text turns out wrong), and whether the text was right.
pub fn unescape_strings_cstyle(s: &str, out: &mut Vec<String>) -> bool {
    let b = s.as_bytes();
    if b.is_empty() {
        return true;
    }
    let mut i = 0usize;
    loop {
        while b[i] == b' ' || b[i] == b'\t' {
            i += 1;
            if i == b.len() {
                return true;
            }
        }
        let mut buf = Vec::new();
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                let mut c = b[i];
                if c == b'"' {
                    break;
                }
                if c == b'\\' {
                    i += 1;
                    if i == b.len() {
                        return false;
                    }
                    c = match b[i] {
                        b'r' => b'\r',
                        b'n' => b'\n',
                        other => other,
                    };
                }
                buf.push(c);
                i += 1;
            }
            if i == b.len() {
                return false;
            }
            i += 1;
        } else {
            while i < b.len() && b[i] != b';' {
                buf.push(b[i]);
                i += 1;
            }
        }
        out.push(text(buf));
        if i == b.len() {
            return true;
        }
        while b[i] == b' ' || b[i] == b'\t' {
            i += 1;
            if i == b.len() {
                return true;
            }
        }
        if b[i] != b';' {
            return false;
        }
        i += 1;
        if i == b.len() {
            out.push(String::new());
            return true;
        }
    }
}

/// `sscanf(s, " %lf <sep> %lf ...")` for `n` numbers between `sep`s and a final `%c`: the numbers
/// it stored, in order, and its return value (EOF is -1).
fn scan_point(s: &[u8], sep: u8, n: usize) -> (Vec<f64>, i32) {
    let mut i = 0usize;
    let mut got = Vec::new();
    let skip = |i: &mut usize| {
        while *i < s.len() && is_space(s[*i]) {
            *i += 1;
        }
    };
    for k in 0..n {
        if k > 0 {
            skip(&mut i);
            if i == s.len() {
                return (got, conversions(k));
            }
            if s[i] != sep {
                return (got, conversions(k));
            }
            i += 1;
        }
        skip(&mut i);
        if i == s.len() {
            return (got, if k == 0 { -1 } else { conversions(k) });
        }
        let r = orca_fmt::parse::strtod(&s[i..]);
        if r.end == 0 {
            return (got, conversions(k));
        }
        got.push(r.value);
        i += r.end;
    }
    skip(&mut i);
    let count = if i < s.len() { n + 1 } else { n };
    (got, conversions(count))
}

fn conversions(n: usize) -> i32 {
    i32::try_from(n).expect("a handful of conversions")
}

/// `std::istringstream(item) >> value` into a local that held `before`.
fn read_f64(item: &[u8], before: f64) -> f64 {
    get_f64(item).or(before)
}

fn read_points(s: &[u8]) -> Vec<[f64; 2]> {
    getline_split(s, b',')
        .into_iter()
        .map(|point| {
            let mut p = [0.0, 0.0];
            let coords = getline_split(point, b'x');
            if let Some(x) = coords.first() {
                p[0] = read_f64(x, 0.0);
                if let Some(y) = coords.get(1) {
                    p[1] = read_f64(y, 0.0);
                }
            }
            p
        })
        .collect()
}

fn points_text(out: &mut String, points: &[[f64; 2]]) {
    for (i, p) in points.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&put(p[0]));
        out.push('x');
        out.push_str(&put(p[1]));
    }
}

fn nil_error() -> ConfigError {
    ConfigError("Deserializing nil into a non-nullable object".into())
}

/// `serialize_single_value` of `ConfigOptionFloatsTempl`.
fn float_item(out: &mut String, v: f64, nullable: bool) -> Result<(), ConfigError> {
    if v.is_finite() {
        out.push_str(&put(v));
        Ok(())
    } else if v.is_nan() {
        if nullable {
            out.push_str("nil");
            Ok(())
        } else {
            Err(ConfigError("Serializing NaN".into()))
        }
    } else {
        Err(ConfigError("Serializing invalid number".into()))
    }
}

/// `ConfigOptionEnum<T>::serialize`: `names[value]` of the names `enum_names_from_keys_map` makes
/// (the last name of a value in the map's order; "" for a value no name has).
fn enum_name(keys: Option<EnumKeys>, value: i32) -> String {
    keys.and_then(|k| k.iter().rev().find(|(_, v)| *v == value))
        .map_or(String::new(), |(n, _)| (*n).to_string())
}

impl OptionValue {
    /// A value of type `ty` as its default constructor makes it (`create_empty_option`): 0, false,
    /// empty, an enum's first value; `nullable` picks a `...Nullable` vector type.
    #[must_use]
    pub fn empty(ty: OptionType, nullable: bool, keys: Option<EnumKeys>) -> OptionValue {
        match ty {
            OptionType::Float | OptionType::None => OptionValue::Float(0.0),
            OptionType::Floats => OptionValue::Floats {
                values: Vec::new(),
                nullable,
            },
            OptionType::Int => OptionValue::Int(0),
            OptionType::Ints => OptionValue::Ints {
                values: Vec::new(),
                nullable,
            },
            OptionType::String => OptionValue::String(String::new()),
            OptionType::Strings => OptionValue::Strings(Vec::new()),
            OptionType::Percent => OptionValue::Percent(0.0),
            OptionType::Percents => OptionValue::Percents {
                values: Vec::new(),
                nullable,
            },
            OptionType::FloatOrPercent => OptionValue::FloatOrPercent {
                value: 0.0,
                percent: false,
            },
            OptionType::FloatsOrPercents => OptionValue::FloatsOrPercents {
                values: Vec::new(),
                nullable,
            },
            OptionType::Point => OptionValue::Point([0.0, 0.0]),
            OptionType::Points => OptionValue::Points(Vec::new()),
            OptionType::Point3 => OptionValue::Point3([0.0, 0.0, 0.0]),
            OptionType::Bool => OptionValue::Bool(false),
            OptionType::Bools => OptionValue::Bools {
                values: Vec::new(),
                nullable,
            },
            OptionType::Enum => OptionValue::Enum { value: 0, keys },
            OptionType::Enums => OptionValue::Enums {
                values: Vec::new(),
                keys,
                nullable,
            },
            OptionType::PointsGroups => OptionValue::PointsGroups(Vec::new()),
            OptionType::IntsGroups => OptionValue::IntsGroups(Vec::new()),
        }
    }

    /// The `ConfigOptionType` of the value.
    #[must_use]
    pub fn ty(&self) -> OptionType {
        match self {
            OptionValue::Float(_) => OptionType::Float,
            OptionValue::Floats { .. } => OptionType::Floats,
            OptionValue::Int(_) => OptionType::Int,
            OptionValue::Ints { .. } => OptionType::Ints,
            OptionValue::String(_) => OptionType::String,
            OptionValue::Strings(_) => OptionType::Strings,
            OptionValue::Percent(_) => OptionType::Percent,
            OptionValue::Percents { .. } => OptionType::Percents,
            OptionValue::FloatOrPercent { .. } => OptionType::FloatOrPercent,
            OptionValue::FloatsOrPercents { .. } => OptionType::FloatsOrPercents,
            OptionValue::Point(_) => OptionType::Point,
            OptionValue::Points(_) => OptionType::Points,
            OptionValue::Point3(_) => OptionType::Point3,
            OptionValue::Bool(_) => OptionType::Bool,
            OptionValue::Bools { .. } => OptionType::Bools,
            OptionValue::Enum { .. } => OptionType::Enum,
            OptionValue::Enums { .. } => OptionType::Enums,
            OptionValue::PointsGroups(_) => OptionType::PointsGroups,
            OptionValue::IntsGroups(_) => OptionType::IntsGroups,
        }
    }

    /// `serialize()`.
    ///
    /// # Errors
    ///
    /// `ConfigurationError` where the C++ throws one: a nil (or an infinity) in a vector that
    /// takes no nil.
    pub fn serialize(&self) -> Result<String, ConfigError> {
        let mut out = String::new();
        match self {
            OptionValue::Float(v) => out = put(*v),
            OptionValue::Percent(v) => out = put(*v) + "%",
            OptionValue::FloatOrPercent { value, percent } => {
                out = put(*value);
                if *percent {
                    out.push('%');
                }
            }
            OptionValue::Floats { values, nullable } => {
                for (i, v) in values.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    float_item(&mut out, *v, *nullable)?;
                }
            }
            OptionValue::Percents { values, nullable } => {
                for (i, v) in values.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    float_item(&mut out, *v, *nullable)?;
                    if !v.is_nan() {
                        out.push('%');
                    }
                }
            }
            OptionValue::FloatsOrPercents { values, nullable } => {
                for (i, (v, percent)) in values.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    float_item(&mut out, *v, *nullable)?;
                    if v.is_finite() && *percent {
                        out.push('%');
                    }
                }
            }
            OptionValue::Int(v) => out = v.to_string(),
            OptionValue::Ints { values, nullable } => {
                for (i, v) in values.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    if *v == i32::MAX {
                        if !*nullable {
                            return Err(ConfigError("Serializing NaN".into()));
                        }
                        out.push_str("nil");
                    } else {
                        out.push_str(&v.to_string());
                    }
                }
            }
            OptionValue::String(s) => out = escape_string_cstyle(s),
            OptionValue::Strings(v) => out = escape_strings_cstyle(v),
            OptionValue::Point(p) => out = format!("{},{}", put(p[0]), put(p[1])),
            OptionValue::Point3(p) => out = format!("{},{},{}", put(p[0]), put(p[1]), put(p[2])),
            OptionValue::Points(points) => points_text(&mut out, points),
            OptionValue::PointsGroups(groups) => {
                for (i, g) in groups.iter().enumerate() {
                    if i > 0 {
                        out.push('#');
                    }
                    points_text(&mut out, g);
                }
            }
            OptionValue::IntsGroups(groups) => {
                for (i, g) in groups.iter().enumerate() {
                    if i > 0 {
                        out.push('#');
                    }
                    for (j, v) in g.iter().enumerate() {
                        if j > 0 {
                            out.push(',');
                        }
                        out.push_str(&v.to_string());
                    }
                }
            }
            OptionValue::Bool(b) => out = if *b { "1" } else { "0" }.to_string(),
            OptionValue::Bools { values, nullable } => {
                for (i, v) in values.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    if *v == u8::MAX {
                        if !*nullable {
                            return Err(ConfigError("Serializing NaN".into()));
                        }
                        out.push_str("nil");
                    } else {
                        out.push_str(if *v != 0 { "1" } else { "0" });
                    }
                }
            }
            OptionValue::Enum { value, keys } => out = enum_name(*keys, *value),
            OptionValue::Enums {
                values,
                keys,
                nullable,
            } => {
                for (i, v) in values.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    if *v == i32::MAX {
                        if !*nullable {
                            return Err(ConfigError("Serializing NaN".into()));
                        }
                        out.push_str("nil");
                    } else if let Some(keys) = keys {
                        for (name, value) in keys.iter() {
                            if value == v {
                                out.push_str(name);
                            }
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    /// `deserialize(str, false)`: whether it read the text (the C++'s return value). A vector is
    /// cleared first and keeps what it read before a bad element, as in the C++.
    ///
    /// # Errors
    ///
    /// `ConfigurationError` where the C++ throws one: "nil" for a vector that takes no nil.
    #[allow(clippy::too_many_lines)]
    pub fn deserialize(&mut self, s: &str) -> Result<bool, ConfigError> {
        let b = s.as_bytes();
        Ok(match self {
            OptionValue::Float(v) | OptionValue::Percent(v) => {
                let e = get_f64(b);
                *v = e.or(*v);
                !e.fail
            }
            OptionValue::FloatOrPercent { value, percent } => {
                *percent = s.contains('%');
                let e = get_f64(b);
                *value = e.or(*value);
                !e.fail
            }
            OptionValue::Floats { values, nullable }
            | OptionValue::Percents { values, nullable } => {
                values.clear();
                if b.is_empty() {
                    values.push(0.0);
                    return Ok(true);
                }
                for item in getline_split(b, b',') {
                    let item = trim(item);
                    if item == b"nil" {
                        if !*nullable {
                            return Err(nil_error());
                        }
                        values.push(f64::NAN);
                    } else {
                        values.push(read_f64(item, 0.0));
                    }
                }
                true
            }
            OptionValue::FloatsOrPercents { values, nullable } => {
                values.clear();
                for item in getline_split(b, b',') {
                    let item = trim(item);
                    if item == b"nil" {
                        if !*nullable {
                            return Err(nil_error());
                        }
                        values.push((f64::NAN, false));
                    } else {
                        let percent = item.contains(&b'%');
                        values.push((read_f64(item, 0.0), percent));
                    }
                }
                true
            }
            OptionValue::Int(v) => {
                let e = get_i32(b);
                *v = e.or(*v);
                !e.fail
            }
            OptionValue::Ints { values, nullable } => {
                values.clear();
                for item in getline_split(b, b',') {
                    let item = trim(item);
                    if item == b"nil" {
                        if !*nullable {
                            return Err(nil_error());
                        }
                        values.push(i32::MAX);
                    } else {
                        values.push(get_i32(item).or(0));
                    }
                }
                true
            }
            OptionValue::String(v) => match unescape_string_cstyle(s) {
                Some(t) => {
                    *v = t;
                    true
                }
                None => false,
            },
            OptionValue::Strings(v) => {
                v.clear();
                unescape_strings_cstyle(s, v)
            }
            OptionValue::Point(p) => {
                let (got, n) = scan_point(b, b',', 2);
                let (got, n) = if n == 2 {
                    (got, n)
                } else {
                    for (k, x) in got.iter().enumerate() {
                        p[k] = *x;
                    }
                    scan_point(b, b'x', 2)
                };
                for (k, x) in got.iter().enumerate() {
                    p[k] = *x;
                }
                n == 2
            }
            OptionValue::Point3(p) => {
                let (got, n) = scan_point(b, b',', 3);
                let (got, n) = if n == 3 {
                    (got, n)
                } else {
                    for (k, x) in got.iter().enumerate() {
                        p[k] = *x;
                    }
                    scan_point(b, b'x', 3)
                };
                for (k, x) in got.iter().enumerate() {
                    p[k] = *x;
                }
                n == 3
            }
            OptionValue::Points(v) => {
                *v = read_points(b);
                true
            }
            OptionValue::PointsGroups(v) => {
                *v = getline_split(b, b'#')
                    .into_iter()
                    .map(read_points)
                    .collect();
                true
            }
            OptionValue::IntsGroups(v) => {
                *v = getline_split(b, b'#')
                    .into_iter()
                    .map(|g| {
                        getline_split(g, b',')
                            .into_iter()
                            .map(|x| get_i32(x).or(0))
                            .collect()
                    })
                    .collect();
                true
            }
            OptionValue::Bool(v) => match getline_split(b, b',').first().map(|x| trim(x)) {
                Some(b"1") => {
                    *v = true;
                    true
                }
                Some(b"0") => {
                    *v = false;
                    true
                }
                _ => false,
            },
            OptionValue::Bools { values, nullable } => {
                values.clear();
                for item in getline_split(b, b',') {
                    let item = trim(item);
                    let new_value = match item {
                        b"nil" => {
                            if !*nullable {
                                return Err(nil_error());
                            }
                            u8::MAX
                        }
                        b"1" => 1,
                        b"0" => 0,
                        // DeserializationSubstitution::Disabled: Failed.
                        _ => return Ok(false),
                    };
                    values.push(new_value);
                }
                true
            }
            OptionValue::Enum { value, keys } => {
                match keys.and_then(|k| k.iter().find(|(n, _)| *n == s)) {
                    Some((_, v)) => {
                        *value = *v;
                        true
                    }
                    None => false,
                }
            }
            OptionValue::Enums {
                values,
                keys,
                nullable,
            } => {
                values.clear();
                for item in getline_split(b, b',') {
                    let item = trim(item);
                    if item == b"nil" {
                        if !*nullable {
                            return Err(nil_error());
                        }
                        values.push(i32::MAX);
                    } else {
                        match keys.and_then(|k| k.iter().find(|(n, _)| n.as_bytes() == item)) {
                            Some((_, v)) => values.push(*v),
                            None => return Ok(false),
                        }
                    }
                }
                true
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{OptionValue, escape_strings_cstyle, getline_split, unescape_strings_cstyle};
    use crate::OptionType;

    fn read(ty: OptionType, nullable: bool, s: &str) -> (OptionValue, bool) {
        let mut v = OptionValue::empty(ty, nullable, None);
        let ok = v.deserialize(s).expect("no throw");
        (v, ok)
    }

    #[test]
    fn getline_reads_as_the_cpp_loop_does() {
        assert!(getline_split(b"", b',').is_empty());
        assert_eq!(getline_split(b"a,", b','), vec![&b"a"[..]]);
        assert_eq!(
            getline_split(b",a,,b", b','),
            vec![&b""[..], b"a", b"", b"b"]
        );
    }

    #[test]
    fn vectors_read_and_write_as_orca_does() {
        assert_eq!(
            read(OptionType::Floats, false, "").0,
            OptionValue::Floats {
                values: vec![0.0],
                nullable: false
            }
        );
        let (v, ok) = read(OptionType::Floats, true, " 0.2 ,nil, 3");
        assert!(ok);
        assert_eq!(v.serialize().unwrap(), "0.2,nil,3");
        let mut strict = OptionValue::empty(OptionType::Floats, false, None);
        assert_eq!(
            strict.deserialize("nil").unwrap_err().0,
            "Deserializing nil into a non-nullable object"
        );
        assert_eq!(
            read(OptionType::Percents, false, "50%,10")
                .0
                .serialize()
                .unwrap(),
            "50%,10%"
        );
        assert_eq!(
            read(OptionType::FloatsOrPercents, false, "50%,10")
                .0
                .serialize()
                .unwrap(),
            "50%,10"
        );
        assert_eq!(
            read(OptionType::Ints, false, "").0,
            OptionValue::Ints {
                values: vec![],
                nullable: false
            }
        );
        assert_eq!(
            read(OptionType::Points, false, "0x0,200x0,200x200")
                .0
                .serialize()
                .unwrap(),
            "0x0,200x0,200x200"
        );
        assert_eq!(
            read(OptionType::PointsGroups, false, "0x0,1x1#2x2")
                .0
                .serialize()
                .unwrap(),
            "0x0,1x1#2x2"
        );
        assert_eq!(
            read(OptionType::IntsGroups, false, "1,2#3")
                .0
                .serialize()
                .unwrap(),
            "1,2#3"
        );
        let (bools, ok) = read(OptionType::Bools, false, "1,0,yes");
        assert!(!ok);
        assert_eq!(
            bools,
            OptionValue::Bools {
                values: vec![1, 0],
                nullable: false
            }
        );
        assert_eq!(
            read(OptionType::Float, false, "0.1234567")
                .0
                .serialize()
                .unwrap(),
            "0.123457"
        );
        assert_eq!(
            read(OptionType::Float, false, "1e21")
                .0
                .serialize()
                .unwrap(),
            "1e+21"
        );
    }

    #[test]
    fn scalars_read_as_orca_does() {
        assert_eq!(
            read(OptionType::FloatOrPercent, false, "50%")
                .0
                .serialize()
                .unwrap(),
            "50%"
        );
        assert_eq!(
            read(OptionType::Percent, false, "15")
                .0
                .serialize()
                .unwrap(),
            "15%"
        );
        assert!(!read(OptionType::Float, false, "").1);
        assert!(!read(OptionType::Int, false, "abc").1);
        assert_eq!(read(OptionType::Int, false, "7.9").0, OptionValue::Int(7));
        assert!(read(OptionType::Point, false, " 1 , 2 ").1);
        assert!(read(OptionType::Point, false, "1x2").1);
        assert!(!read(OptionType::Point, false, "1,2,3").1);
        assert_eq!(
            read(OptionType::Point, false, "3x4").0.serialize().unwrap(),
            "3,4"
        );
        assert!(read(OptionType::Bool, false, " 1 ,0").1);
        assert!(!read(OptionType::Bool, false, "true").1);
    }

    #[test]
    fn strings_escape_as_orca_does() {
        let strs = vec!["a b".to_string(), "c".to_string(), "q\"\\\n".to_string()];
        let text = escape_strings_cstyle(&strs);
        assert_eq!(text, "\"a b\";c;\"q\\\"\\\\\\n\"");
        let mut back = Vec::new();
        assert!(unescape_strings_cstyle(&text, &mut back));
        assert_eq!(back, strs);
        assert_eq!(escape_strings_cstyle(&[String::new()]), "\"\"");
        let mut v = Vec::new();
        assert!(unescape_strings_cstyle("a;", &mut v));
        assert_eq!(v, vec!["a".to_string(), String::new()]);
        assert!(!unescape_strings_cstyle("\"open", &mut Vec::new()));
        assert_eq!(
            read(OptionType::String, false, "a\\nb").0,
            OptionValue::String("a\nb".into())
        );
        assert!(!read(OptionType::String, false, "a\\").1);
    }
}
