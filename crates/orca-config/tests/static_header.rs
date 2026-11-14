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
// Carries no OrcaSlicer code: it reads OrcaSlicer's src/libslic3r/PrintConfig.hpp (v2.4.2, commit
// 8500fcd) from the work folder at run time. Written on 2026-11-13 (orca-o3-front-1).

//! The header check of the static print configs: `src/print_config.rs` is generated from the work
//! folder's PrintConfig.hpp (`gen/print-config.ts`), and this test reads the header again, with a
//! reader of its own, and fails while the two differ.
//!
//! - Every enum: its name, its size (`unsigned char` or `int`), its enumerators and values, the
//!   generated `ENUMS` and each generated enum's `ConfigEnum` (`from_int`, `to_int`, `BYTES`).
//! - Every class of the `PRINT_CONFIG_CLASS_*` macros: its parents and fields in order
//!   (`CLASSES`), its flattened fields (`FIELDS`, the field order a hash follows), its table by key
//!   (`BY_KEY`), and each option's type and nullability as the header's option type gives them.
//!
//! The work folder is `SKETCH_ORCA_HOME`, else `%LOCALAPPDATA%\SketchDev\orca-engine` (Windows)
//! or `~/.local/share/SketchDev/orca-engine`; the test fails naming the header's path when the
//! file is missing.

use std::collections::BTreeMap;
use std::path::PathBuf;

use orca_config::print_config::{CLASSES, ENUMS};
use orca_config::{
    FullPrintConfig, GCodeConfig, MachineEnvelopeConfig, OptionType, PrintConfig,
    PrintObjectConfig, PrintRegionConfig, SLAFullPrintConfig, SLAMaterialConfig, SLAPrintConfig,
    SLAPrintObjectConfig, SLAPrinterConfig, StaticPrintConfig,
};

fn work_home() -> PathBuf {
    if let Some(h) = std::env::var_os("SKETCH_ORCA_HOME").filter(|h| !h.is_empty()) {
        return PathBuf::from(h);
    }
    if cfg!(windows) {
        let local = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("USERPROFILE")
                    .map(|u| PathBuf::from(u).join("AppData").join("Local"))
            })
            .expect("LOCALAPPDATA or USERPROFILE is set");
        return local.join("SketchDev").join("orca-engine");
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))
        .expect("XDG_DATA_HOME or HOME is set");
    data.join("SketchDev").join("orca-engine")
}

fn header() -> String {
    let path = work_home()
        .join("src")
        .join("src")
        .join("libslic3r")
        .join("PrintConfig.hpp");
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e} (the work folder's OrcaSlicer source, which the static configs are checked \
             against)",
            path.display()
        )
    })
}

/// The text with comments and preprocessor lines (with their continuations) blanked; string and
/// character literals kept.
fn strip_cpp(text: &str) -> String {
    let src: Vec<char> = text.replace("\r\n", "\n").chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    let mut line_start = true;
    while i < src.len() {
        let c = src[i];
        if line_start {
            let mut j = i;
            while j < src.len() && (src[j] == ' ' || src[j] == '\t') {
                j += 1;
            }
            if j < src.len() && src[j] == '#' {
                loop {
                    let end = (j..src.len())
                        .find(|&k| src[k] == '\n')
                        .unwrap_or(src.len());
                    let line: String = src[j..end].iter().collect();
                    out.push('\n');
                    j = (end + 1).min(src.len());
                    if !line.trim_end().ends_with('\\') || j >= src.len() {
                        break;
                    }
                }
                i = j;
                continue;
            }
        }
        if c == '/' && src.get(i + 1) == Some(&'/') {
            while i < src.len() && src[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && src.get(i + 1) == Some(&'*') {
            i += 2;
            while i < src.len() && !(src[i] == '*' && src.get(i + 1) == Some(&'/')) {
                if src[i] == '\n' {
                    out.push('\n');
                }
                i += 1;
            }
            i += 2;
            continue;
        }
        if c == '"' || c == '\'' {
            let mut j = i + 1;
            while j < src.len() && src[j] != c {
                j += if src[j] == '\\' { 2 } else { 1 };
            }
            let end = (j + 1).min(src.len());
            out.extend(&src[i..end]);
            i = end;
            line_start = false;
            continue;
        }
        out.push(c);
        line_start = c == '\n';
        i += 1;
    }
    out
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Whether `text[at..]` starts the whole word `word`.
fn word_at(text: &[char], at: usize, word: &str) -> bool {
    let w: Vec<char> = word.chars().collect();
    text.len() >= at + w.len()
        && text[at..at + w.len()] == w[..]
        && (at == 0 || !is_word(text[at - 1]))
        && text.get(at + w.len()).is_none_or(|c| !is_word(*c))
}

fn skip_space(text: &[char], mut at: usize) -> usize {
    while at < text.len() && text[at].is_whitespace() {
        at += 1;
    }
    at
}

fn read_word(text: &[char], at: usize) -> (String, usize) {
    let mut end = at;
    while end < text.len() && is_word(text[end]) {
        end += 1;
    }
    (text[at..end].iter().collect(), end)
}

/// One enum of the header: name, size, enumerators (aliases after, in declaration order).
#[derive(Debug, PartialEq)]
struct Enum {
    name: String,
    bytes: usize,
    values: Vec<(String, i32)>,
}

fn parse_enums(text: &str) -> Vec<Enum> {
    let t: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut at = 0;
    while at < t.len() {
        if !word_at(&t, at, "enum") {
            at += 1;
            continue;
        }
        let mut p = skip_space(&t, at + 4);
        if word_at(&t, p, "class") {
            p = skip_space(&t, p + 5);
        }
        let (name, after) = read_word(&t, p);
        p = skip_space(&t, after);
        let mut under = "int".to_string();
        if t.get(p) == Some(&':') {
            let open = (p..t.len()).find(|&k| t[k] == '{' || t[k] == ';').unwrap();
            under = t[p + 1..open]
                .iter()
                .collect::<String>()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            p = open;
        }
        if name.is_empty() || t.get(p) != Some(&'{') {
            // A forward declaration or an anonymous enum: not one of the header's enums.
            at = orca_stl::max(p, at + 1);
            continue;
        }
        let close = (p..t.len()).find(|&k| t[k] == '}').unwrap();
        let after_close = skip_space(&t, close + 1);
        assert_eq!(t.get(after_close), Some(&';'), "enum {name} ends without ;");
        let bytes = match under.as_str() {
            "int" => 4,
            "unsigned char" => 1,
            other => panic!("enum {name}: underlying type {other}"),
        };
        let body: String = t[p + 1..close].iter().collect();
        let mut values: Vec<(String, i32)> = Vec::new();
        let mut aliases: Vec<(String, i32)> = Vec::new();
        let mut known: BTreeMap<String, i32> = BTreeMap::new();
        let mut next = 0i32;
        for raw in body.split(',') {
            let item = raw.trim();
            if item.is_empty() {
                continue;
            }
            let (vname, value) = match item.split_once('=') {
                Some((n, e)) => {
                    let e = e.trim();
                    let v = e
                        .parse::<i32>()
                        .ok()
                        .or_else(|| known.get(e).copied())
                        .unwrap_or_else(|| panic!("enum {name}: {item}"));
                    (n.trim().to_string(), v)
                }
                None => (item.to_string(), next),
            };
            assert!(vname.chars().all(is_word), "enum {name}: {vname}");
            if values.iter().any(|(_, v)| *v == value) {
                aliases.push((vname.clone(), value));
            } else {
                values.push((vname.clone(), value));
            }
            known.insert(vname, value);
            next = value + 1;
        }
        values.extend(aliases);
        out.push(Enum {
            name,
            bytes,
            values,
        });
        at = after_close + 1;
    }
    out
}

/// One class of the header's macros.
#[derive(Debug, PartialEq)]
struct Class {
    name: String,
    parents: Vec<String>,
    fields: Vec<(String, String)>,
}

/// The macro call's arguments at `open` (its `(`), split at top-level commas.
fn macro_args(t: &[char], open: usize) -> Vec<String> {
    let mut args = Vec::new();
    let mut depth = 0;
    let mut start = open + 1;
    for (i, c) in t.iter().enumerate().skip(open) {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    args.push(t[start..i].iter().collect::<String>().trim().to_string());
                    return args;
                }
            }
            ',' if depth == 1 => {
                args.push(t[start..i].iter().collect::<String>().trim().to_string());
                start = i + 1;
            }
            _ => {}
        }
    }
    panic!("a macro call that does not close");
}

/// The `((Type, key))` items of a parameter sequence.
fn parse_seq(seq: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = seq.trim();
    while !rest.is_empty() {
        let inner = rest.strip_prefix("((").unwrap_or_else(|| {
            panic!("a parameter sequence item: {}", &rest[..rest.len().min(80)])
        });
        let close = inner.find("))").expect("an item closes");
        let (ty, key) = inner[..close]
            .rsplit_once(',')
            .expect("an item has a type and a key");
        let ty: String = ty.chars().filter(|c| !c.is_whitespace()).collect();
        out.push((ty, key.trim().to_string()));
        rest = inner[close + 2..].trim_start();
    }
    out
}

fn parse_classes(text: &str) -> Vec<Class> {
    let t: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let macros = [
        "PRINT_CONFIG_CLASS_DERIVED_DEFINE0",
        "PRINT_CONFIG_CLASS_DERIVED_DEFINE",
        "PRINT_CONFIG_CLASS_DEFINE",
    ];
    let mut at = 0;
    while at < t.len() {
        let Some(m) = macros.iter().find(|m| word_at(&t, at, m)) else {
            at += 1;
            continue;
        };
        let open = skip_space(&t, at + m.len());
        if t.get(open) != Some(&'(') {
            at += m.len();
            continue;
        }
        let args = macro_args(&t, open);
        if *m == "PRINT_CONFIG_CLASS_DEFINE" {
            assert_eq!(args.len(), 2, "{m}({args:?})");
            out.push(Class {
                name: args[0].clone(),
                parents: Vec::new(),
                fields: parse_seq(&args[1]),
            });
        } else {
            let want = if *m == "PRINT_CONFIG_CLASS_DERIVED_DEFINE0" {
                2
            } else {
                3
            };
            assert_eq!(args.len(), want, "{m}({args:?})");
            let parents = args[1]
                .trim_start_matches('(')
                .trim_end_matches(')')
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();
            out.push(Class {
                name: args[0].clone(),
                parents,
                fields: if want == 3 {
                    parse_seq(&args[2])
                } else {
                    Vec::new()
                },
            });
        }
        at = open + 1;
    }
    out
}

/// The type and nullability of a header option type.
fn option_type(ty: &str) -> (OptionType, bool) {
    if ty.starts_with("ConfigOptionEnum<") {
        return (OptionType::Enum, false);
    }
    match ty {
        "ConfigOptionFloat" => (OptionType::Float, false),
        "ConfigOptionFloats" => (OptionType::Floats, false),
        "ConfigOptionFloatsNullable" => (OptionType::Floats, true),
        "ConfigOptionInt" => (OptionType::Int, false),
        "ConfigOptionInts" => (OptionType::Ints, false),
        "ConfigOptionIntsNullable" => (OptionType::Ints, true),
        "ConfigOptionString" => (OptionType::String, false),
        "ConfigOptionStrings" => (OptionType::Strings, false),
        "ConfigOptionPercent" => (OptionType::Percent, false),
        "ConfigOptionPercents" => (OptionType::Percents, false),
        "ConfigOptionPercentsNullable" => (OptionType::Percents, true),
        "ConfigOptionFloatOrPercent" => (OptionType::FloatOrPercent, false),
        "ConfigOptionPoint" => (OptionType::Point, false),
        "ConfigOptionPoints" => (OptionType::Points, false),
        "ConfigOptionBool" => (OptionType::Bool, false),
        "ConfigOptionBools" => (OptionType::Bools, false),
        "ConfigOptionBoolsNullable" => (OptionType::Bools, true),
        "ConfigOptionEnumsGeneric" => (OptionType::Enums, false),
        "ConfigOptionEnumsGenericNullable" => (OptionType::Enums, true),
        "ConfigOptionPointsGroups" => (OptionType::PointsGroups, false),
        other => panic!("an option type the static configs do not hold: {other}"),
    }
}

fn flatten(name: &str, classes: &[Class]) -> Vec<(String, String)> {
    let c = classes
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no class {name}"));
    let mut out = Vec::new();
    for p in &c.parents {
        out.extend(flatten(p, classes));
    }
    out.extend(c.fields.iter().cloned());
    out
}

/// One generated class against the header.
fn check_class<T: StaticPrintConfig>(classes: &[Class]) {
    let flat = flatten(T::NAME, classes);
    let keys: Vec<&str> = flat.iter().map(|(_, k)| k.as_str()).collect();
    assert_eq!(T::FIELDS, &keys[..], "{}: FIELDS", T::NAME);
    let c = T::constructed();
    for (i, (ty, key)) in flat.iter().enumerate() {
        let (want_ty, want_null) = option_type(ty);
        let o = c.opt(i);
        assert_eq!(o.opt_type(), want_ty, "{}::{key}", T::NAME);
        assert_eq!(o.nullable(), want_null, "{}::{key} nullable", T::NAME);
        assert_eq!(T::index_of(key), Some(i), "{}::{key} by key", T::NAME);
    }
    assert_eq!(T::BY_KEY.len(), flat.len(), "{}: BY_KEY", T::NAME);
    for w in T::BY_KEY.windows(2) {
        assert!(
            w[0].0.as_bytes() < w[1].0.as_bytes(),
            "{}: BY_KEY order at {}",
            T::NAME,
            w[1].0
        );
    }
}

#[test]
fn the_static_configs_are_the_headers() {
    let text = strip_cpp(&header());
    let enums = parse_enums(&text);
    let classes = parse_classes(&text);
    assert!(
        !classes.is_empty(),
        "no PRINT_CONFIG_CLASS_* class in the header"
    );

    // The enums.
    let got: Vec<Enum> = ENUMS
        .iter()
        .map(|e| Enum {
            name: e.name.to_string(),
            bytes: e.bytes,
            values: e
                .values
                .iter()
                .map(|(n, v)| ((*n).to_string(), *v))
                .collect(),
        })
        .collect();
    assert_eq!(
        got.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        enums.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        "the enums"
    );
    for (g, h) in got.iter().zip(&enums) {
        assert_eq!(g, h, "enum {}", h.name);
    }

    // The classes as declared.
    let got: Vec<Class> = CLASSES
        .iter()
        .map(|c| Class {
            name: c.name.to_string(),
            parents: c.parents.iter().map(|p| (*p).to_string()).collect(),
            fields: c
                .fields
                .iter()
                .map(|(t, k)| ((*t).to_string(), (*k).to_string()))
                .collect(),
        })
        .collect();
    assert_eq!(
        got.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        classes.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        "the classes"
    );
    for (g, h) in got.iter().zip(&classes) {
        assert_eq!(g, h, "class {}", h.name);
    }

    // Each class's flattened fields and option types.
    check_class::<PrintObjectConfig>(&classes);
    check_class::<PrintRegionConfig>(&classes);
    check_class::<MachineEnvelopeConfig>(&classes);
    check_class::<GCodeConfig>(&classes);
    check_class::<PrintConfig>(&classes);
    check_class::<FullPrintConfig>(&classes);
    check_class::<SLAPrintConfig>(&classes);
    check_class::<SLAPrintObjectConfig>(&classes);
    check_class::<SLAMaterialConfig>(&classes);
    check_class::<SLAPrinterConfig>(&classes);
    check_class::<SLAFullPrintConfig>(&classes);
    assert_eq!(classes.len(), 11, "the header's classes, one check each");
}

/// Each generated enum's `ConfigEnum` against its declaration.
#[test]
fn the_enums_convert_as_declared() {
    use orca_config::ConfigEnum;
    use orca_config::print_config as pc;

    fn check<T: ConfigEnum>() {
        let decl = ENUMS
            .iter()
            .find(|e| e.name == T::NAME)
            .unwrap_or_else(|| panic!("no enum {}", T::NAME));
        assert_eq!(T::BYTES, decl.bytes, "{}", T::NAME);
        for (name, v) in decl.values {
            let e = T::from_int(*v).unwrap_or_else(|| panic!("{}::{name}", T::NAME));
            assert_eq!(e.to_int(), *v, "{}::{name}", T::NAME);
        }
        let max = decl
            .values
            .iter()
            .map(|(_, v)| *v)
            .fold(i32::MIN, |a, b| if b > a { b } else { a });
        assert!(
            T::from_int(max + 1).is_none(),
            "{} past its values",
            T::NAME
        );
        if let Some(key) = T::KEYS_OPTION {
            assert!(T::keys_map().is_some(), "{}: {key} has no names", T::NAME);
        }
    }
    check::<pc::GCodeFlavor>();
    check::<pc::InfillPattern>();
    check::<pc::BrimType>();
    check::<pc::PerimeterGeneratorType>();
    check::<pc::ExtruderType>();
    check::<pc::NozzleVolumeType>();
    check::<pc::LongRectrationLevel>();
    check::<pc::DraftShield>();
}
