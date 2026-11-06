// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! orca-json against `nlohmann::json` 3.10.4.
//!
//! The texts `nlohmann` dumped itself come first: the configdef probe (`dump(1)` of an
//! `ordered_json`, `parity/probes/configdef.json`) and the front end's goldens
//! (`parity/goldens/front`, `dump(2)` and `dump(-1)`) must parse and dump back to the same bytes,
//! which checks the Grisu2 output of every double they hold, the escapes of every string and the
//! layout. Then the order of `json` and `ordered_json`, and `nlohmann`'s own parse errors, the
//! messages its test suite (`unit-class_parser.cpp`) expects, lines and columns included.

use std::path::{Path, PathBuf};

use orca_json::{ErrorHandler, Object, Order, Value, escape_bytes, parse, string_from_bytes};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// Parses the file and dumps it again with `indent`, then `end` (a file written by
/// `write_file_atomic` ends in "\n"; standard output, in text mode, in "\r\n").
fn round_trip(path: &Path, indent: Option<usize>, end: &str) {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let value =
        parse(&bytes, Order::Insertion).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut again = value.dump(indent);
    again.push_str(end);
    if again.as_bytes() != bytes.as_slice() {
        let at = again
            .bytes()
            .zip(bytes.iter())
            .position(|(a, b)| a != *b)
            .unwrap_or(0);
        let from = at.saturating_sub(80);
        panic!(
            "{} dumps back differently at byte {at}:\n  nlohmann: {:?}\n  orca-json: {:?}",
            path.display(),
            String::from_utf8_lossy(&bytes[from..(at + 80).min(bytes.len())]),
            &again[from..(at + 80).min(again.len())]
        );
    }
}

#[test]
fn the_configdef_probe_dumps_back_byte_for_byte() {
    round_trip(
        &repo().join("parity").join("probes").join("configdef.json"),
        Some(1),
        "\n",
    );
}

#[test]
fn the_front_goldens_dump_back_byte_for_byte() {
    let front = repo().join("parity").join("goldens").join("front");
    round_trip(&front.join("config-def.json"), Some(2), "\n");
    round_trip(&front.join("version.json"), None, "\r\n");
    let mut n = 0;
    for entry in std::fs::read_dir(front.join("intake")).expect("the intake goldens") {
        let path = entry.expect("a golden").path();
        if path.extension().is_some_and(|e| e == "json") {
            round_trip(&path, Some(2), "\n");
            n += 1;
        }
    }
    assert!(n > 0, "no intake goldens");
}

fn compact(text: &str, order: Order) -> String {
    parse(text.as_bytes(), order).expect(text).dump(None)
}

#[test]
fn json_sorts_keys_and_ordered_json_keeps_them() {
    let text = r#"{"b":1,"a":{"z":1,"y":2},"B":3,"\u00e9":4}"#;
    assert_eq!(
        compact(text, Order::Sorted),
        r#"{"B":3,"a":{"y":2,"z":1},"b":1,"é":4}"#
    );
    assert_eq!(
        compact(text, Order::Insertion),
        r#"{"b":1,"a":{"z":1,"y":2},"B":3,"é":4}"#
    );
    // A key given twice keeps its first place and its last value.
    assert_eq!(
        compact(r#"{"a":1,"b":2,"a":3}"#, Order::Insertion),
        r#"{"a":3,"b":2}"#
    );
    assert_eq!(
        compact(r#"{"b":1,"a":2,"b":[3]}"#, Order::Sorted),
        r#"{"a":2,"b":[3]}"#
    );

    let mut o = Object::new(Order::Insertion);
    o.insert("z", 1);
    o.insert("a", "x");
    o.insert("z", 2);
    *o.entry("m") = Value::Bool(true);
    assert_eq!(Value::Object(o).dump(None), r#"{"z":2,"a":"x","m":true}"#);
}

#[test]
fn numbers_keep_their_kind_and_dump_as_nlohmann_dumps_them() {
    let cases: &[(&str, Value)] = &[
        ("0", Value::UInt(0)),
        ("-0", Value::Int(0)),
        ("18446744073709551615", Value::UInt(u64::MAX)),
        ("-9223372036854775808", Value::Int(i64::MIN)),
        (
            "18446744073709551616",
            Value::Float(18_446_744_073_709_551_616.0),
        ),
        (
            "-9223372036854775809",
            Value::Float(-9_223_372_036_854_775_809.0),
        ),
        ("1E2", Value::Float(100.0)),
        ("-0.0", Value::Float(-0.0)),
    ];
    for (text, want) in cases {
        let got = parse(text.as_bytes(), Order::Insertion).expect(text);
        assert_eq!(&got, want, "{text}");
    }
    let dumps: &[(f64, &str)] = &[
        (0.0, "0.0"),
        (-0.0, "-0.0"),
        (1.0, "1.0"),
        (-1.5, "-1.5"),
        (42.0, "42.0"),
        (0.1, "0.1"),
        (0.3, "0.3"),
        (1.1, "1.1"),
        (123.456, "123.456"),
        (std::f64::consts::PI, "3.141592653589793"),
        (9.876_543_21, "9.87654321"),
        (1e10, "10000000000.0"),
        (1e14, "100000000000000.0"),
        (123_456_789_012_345.0, "123456789012345.0"),
        (1e15, "1e+15"),
        (1e16, "1e+16"),
        (1e21, "1e+21"),
        (0.0001, "0.0001"),
        (1e-5, "1e-05"),
        (1e-10, "1e-10"),
        (1.234_567_890_123_456_7e19, "1.2345678901234567e+19"),
        (5e-324, "5e-324"),
        (2.225_073_858_507_201_4e-308, "2.2250738585072014e-308"),
        (f64::MAX, "1.7976931348623157e+308"),
        (3.402_823_466_385_288_6e38, "3.4028234663852886e+38"),
        (f64::INFINITY, "null"),
        (f64::NAN, "null"),
    ];
    for (value, want) in dumps {
        assert_eq!(Value::Float(*value).dump(None), *want, "{value:e}");
    }
    assert_eq!(Value::Int(-7).dump(None), "-7");
    assert_eq!(Value::UInt(u64::MAX).dump(None), "18446744073709551615");
}

#[test]
fn pretty_dumps_lay_out_as_nlohmann_does() {
    let v = parse(
        br#"{"a":[1,2],"b":{},"c":[],"d":{"e":null}}"#,
        Order::Insertion,
    )
    .unwrap();
    assert_eq!(
        v.dump(Some(2)),
        "{\n  \"a\": [\n    1,\n    2\n  ],\n  \"b\": {},\n  \"c\": [],\n  \"d\": {\n    \"e\": null\n  }\n}"
    );
    assert_eq!(
        v.dump(Some(0)),
        "{\n\"a\": [\n1,\n2\n],\n\"b\": {},\n\"c\": [],\n\"d\": {\n\"e\": null\n}\n}"
    );
    assert_eq!(
        String::from_utf8(v.dump_bytes(Some(1), b'\t', false)).unwrap(),
        "{\n\t\"a\": [\n\t\t1,\n\t\t2\n\t],\n\t\"b\": {},\n\t\"c\": [],\n\t\"d\": {\n\t\t\"e\": null\n\t}\n}"
    );
}

#[test]
fn strings_escape_as_dump_escaped_does() {
    let s = Value::from("q\"b\\s/\u{8}\u{c}\n\r\t\u{1}\u{1f}\u{7f}é\u{1F600}");
    assert_eq!(
        s.dump(None),
        "\"q\\\"b\\\\s/\\b\\f\\n\\r\\t\\u0001\\u001f\u{7f}é\u{1F600}\""
    );
    assert_eq!(
        String::from_utf8(s.dump_bytes(None, b' ', true)).unwrap(),
        "\"q\\\"b\\\\s/\\b\\f\\n\\r\\t\\u0001\\u001f\\u007f\\u00e9\\ud83d\\ude00\""
    );

    // Bytes that are not UTF-8, under each error handler.
    let replace = |b: &[u8], ascii| escape_bytes(b, ascii, ErrorHandler::Replace).unwrap();
    assert_eq!(replace(b"a\xFFb", false), b"a\xEF\xBF\xBDb");
    assert_eq!(replace(b"a\xFFb", true), b"a\\ufffdb");
    // A broken sequence is one replacement; the byte that broke it is read again.
    assert_eq!(replace(b"\xE2\x82a", false), b"\xEF\xBF\xBDa");
    assert_eq!(replace(b"\xE0\x80", false), b"\xEF\xBF\xBD\xEF\xBF\xBD");
    assert_eq!(replace(b"ok\xE2\x82", false), b"ok\xEF\xBF\xBD");
    assert_eq!(
        escape_bytes(b"a\xFFb\xC3", false, ErrorHandler::Ignore).unwrap(),
        b"ab"
    );
    assert_eq!(
        escape_bytes(b"ab\xFF", false, ErrorHandler::Strict)
            .unwrap_err()
            .message,
        "[json.exception.type_error.316] invalid UTF-8 byte at index 2: 0xFF"
    );
    assert_eq!(
        escape_bytes(b"ab\xC3", false, ErrorHandler::Strict)
            .unwrap_err()
            .message,
        "[json.exception.type_error.316] incomplete UTF-8 string; last byte: 0xC3"
    );
    // string_from_bytes gives the string that dumps as the bytes do.
    for bytes in [
        &b"a\xFFb"[..],
        b"\xE2\x82a",
        b"\xE0\x80",
        b"ok\xE2\x82",
        b"\xF0\x9F\x98",
    ] {
        let s = string_from_bytes(bytes, ErrorHandler::Replace).unwrap();
        assert_eq!(
            Value::from(s).dump_bytes(None, b' ', false),
            [b"\"".as_slice(), &replace(bytes, false), b"\""].concat(),
            "{bytes:?}"
        );
    }
}

fn error(text: &[u8]) -> String {
    match parse(text, Order::Insertion) {
        Ok(v) => panic!("{:?} parsed as {v:?}", String::from_utf8_lossy(text)),
        Err(e) => e.message,
    }
}

const PE: &str = "[json.exception.parse_error.101] parse error at line ";
const LIT: &str = "expected '[', '{', or a literal";

#[test]
fn parse_errors_are_nlohmanns() {
    let cases: &[(&[u8], String)] = &[
        (
            b"",
            format!(
                "{PE}1, column 1: syntax error while parsing value - unexpected end of input; {LIT}"
            ),
        ),
        (
            b"[",
            format!(
                "{PE}1, column 2: syntax error while parsing value - unexpected end of input; {LIT}"
            ),
        ),
        (
            b"{",
            format!(
                "{PE}1, column 2: syntax error while parsing object key - unexpected end of input; expected string literal"
            ),
        ),
        (
            b"{\"foo\"",
            format!(
                "{PE}1, column 7: syntax error while parsing object separator - unexpected end of input; expected ':'"
            ),
        ),
        (
            b"{\"foo\":",
            format!(
                "{PE}1, column 8: syntax error while parsing value - unexpected end of input; {LIT}"
            ),
        ),
        (
            b"{\"foo\":}",
            format!("{PE}1, column 8: syntax error while parsing value - unexpected '}}'; {LIT}"),
        ),
        (
            b"{\"foo\":1,}",
            format!(
                "{PE}1, column 10: syntax error while parsing object key - unexpected '}}'; expected string literal"
            ),
        ),
        (
            b"{\"foo\":1 2",
            format!(
                "{PE}1, column 10: syntax error while parsing object - unexpected number literal; expected '}}'"
            ),
        ),
        (
            b"}",
            format!("{PE}1, column 1: syntax error while parsing value - unexpected '}}'; {LIT}"),
        ),
        (
            b"[1",
            format!(
                "{PE}1, column 3: syntax error while parsing array - unexpected end of input; expected ']'"
            ),
        ),
        (
            b"[1,",
            format!(
                "{PE}1, column 4: syntax error while parsing value - unexpected end of input; {LIT}"
            ),
        ),
        (
            b"[1,]",
            format!("{PE}1, column 4: syntax error while parsing value - unexpected ']'; {LIT}"),
        ),
        (
            b"]",
            format!("{PE}1, column 1: syntax error while parsing value - unexpected ']'; {LIT}"),
        ),
        (
            b"\"",
            format!(
                "{PE}1, column 2: syntax error while parsing value - invalid string: missing closing quote; last read: '\"'"
            ),
        ),
        (
            b"\"\\\"",
            format!(
                "{PE}1, column 4: syntax error while parsing value - invalid string: missing closing quote; last read: '\"\\\"'"
            ),
        ),
        (
            b"\"\\u\"",
            format!(
                "{PE}1, column 4: syntax error while parsing value - invalid string: '\\u' must be followed by 4 hex digits; last read: '\"\\u\"'"
            ),
        ),
        (
            b"\"\\u01\"",
            format!(
                "{PE}1, column 6: syntax error while parsing value - invalid string: '\\u' must be followed by 4 hex digits; last read: '\"\\u01\"'"
            ),
        ),
        (
            b"\"\\uD80C\"",
            format!(
                "{PE}1, column 8: syntax error while parsing value - invalid string: surrogate U+D800..U+DBFF must be followed by U+DC00..U+DFFF; last read: '\"\\uD80C\"'"
            ),
        ),
        (
            b"\"\\uDC00\"",
            format!(
                "{PE}1, column 7: syntax error while parsing value - invalid string: surrogate U+DC00..U+DFFF must follow U+D800..U+DBFF; last read: '\"\\uDC00'"
            ),
        ),
        (
            b"\"\\x\"",
            format!(
                "{PE}1, column 3: syntax error while parsing value - invalid string: forbidden character after backslash; last read: '\"\\x'"
            ),
        ),
        (
            b"\"\x01\"",
            format!(
                "{PE}1, column 2: syntax error while parsing value - invalid string: control character U+0001 (SOH) must be escaped to \\u0001; last read: '\"<U+0001>'"
            ),
        ),
        (
            b"\"\x0a\"",
            format!(
                "{PE}2, column 0: syntax error while parsing value - invalid string: control character U+000A (LF) must be escaped to \\u000A or \\n; last read: '\"<U+000A>'"
            ),
        ),
        (
            b"01",
            format!(
                "{PE}1, column 2: syntax error while parsing value - unexpected number literal; expected end of input"
            ),
        ),
        (
            b"--1",
            format!(
                "{PE}1, column 2: syntax error while parsing value - invalid number; expected digit after '-'; last read: '--'"
            ),
        ),
        (
            b"1.",
            format!(
                "{PE}1, column 3: syntax error while parsing value - invalid number; expected digit after '.'; last read: '1.'"
            ),
        ),
        (
            b"1E",
            format!(
                "{PE}1, column 3: syntax error while parsing value - invalid number; expected '+', '-', or digit after exponent; last read: '1E'"
            ),
        ),
        (
            b"1E-",
            format!(
                "{PE}1, column 4: syntax error while parsing value - invalid number; expected digit after exponent sign; last read: '1E-'"
            ),
        ),
        (
            b"1.E1",
            format!(
                "{PE}1, column 3: syntax error while parsing value - invalid number; expected digit after '.'; last read: '1.E'"
            ),
        ),
        (
            b"-1E",
            format!(
                "{PE}1, column 4: syntax error while parsing value - invalid number; expected '+', '-', or digit after exponent; last read: '-1E'"
            ),
        ),
        (
            b"-0E#",
            format!(
                "{PE}1, column 4: syntax error while parsing value - invalid number; expected '+', '-', or digit after exponent; last read: '-0E#'"
            ),
        ),
        (
            b"-0E-#",
            format!(
                "{PE}1, column 5: syntax error while parsing value - invalid number; expected digit after exponent sign; last read: '-0E-#'"
            ),
        ),
        (
            b"-0#",
            format!(
                "{PE}1, column 3: syntax error while parsing value - invalid literal; last read: '-0#'; expected end of input"
            ),
        ),
        (
            b"-0.0:",
            format!(
                "{PE}1, column 5: syntax error while parsing value - unexpected ':'; expected end of input"
            ),
        ),
        (
            b"-0.0Z",
            format!(
                "{PE}1, column 5: syntax error while parsing value - invalid literal; last read: '-0.0Z'; expected end of input"
            ),
        ),
        (
            b"-0E123:",
            format!(
                "{PE}1, column 7: syntax error while parsing value - unexpected ':'; expected end of input"
            ),
        ),
        (
            b"-0e0-:",
            format!(
                "{PE}1, column 6: syntax error while parsing value - invalid number; expected digit after '-'; last read: '-:'; expected end of input"
            ),
        ),
        (
            b"-0e-:",
            format!(
                "{PE}1, column 5: syntax error while parsing value - invalid number; expected digit after exponent sign; last read: '-0e-:'"
            ),
        ),
        (
            b"-0f",
            format!(
                "{PE}1, column 4: syntax error while parsing value - invalid literal; last read: '-0f'; expected end of input"
            ),
        ),
        (
            b"nul",
            format!(
                "{PE}1, column 4: syntax error while parsing value - invalid literal; last read: 'nul'"
            ),
        ),
        (
            b"tru",
            format!(
                "{PE}1, column 4: syntax error while parsing value - invalid literal; last read: 'tru'"
            ),
        ),
        (
            b"fals",
            format!(
                "{PE}1, column 5: syntax error while parsing value - invalid literal; last read: 'fals'"
            ),
        ),
        (
            b"[\n1,\n]",
            format!("{PE}3, column 1: syntax error while parsing value - unexpected ']'; {LIT}"),
        ),
        (
            b"\xEF\xBB",
            format!(
                "{PE}1, column 3: syntax error while parsing value - invalid BOM; must be 0xEF 0xBB 0xBF if given; last read: '\u{FFFD}'"
            ),
        ),
    ];
    for (text, want) in cases {
        assert_eq!(&error(text), want, "{:?}", String::from_utf8_lossy(text));
    }
    let overflow = parse(b"1.18973e+4932", Order::Insertion).unwrap_err();
    assert_eq!(
        overflow.message,
        "[json.exception.out_of_range.406] number overflow parsing '1.18973e+4932'"
    );
    assert_eq!(overflow.kind, "out_of_range");
}

#[test]
fn what_ends_a_text() {
    // A BOM is skipped, and a null byte ends the text as its end does.
    assert_eq!(
        parse(b"\xEF\xBB\xBF[1]", Order::Insertion).unwrap(),
        Value::from(vec![1u64])
    );
    assert_eq!(
        parse(b" {} \0 trailing", Order::Insertion).unwrap(),
        Value::object(Order::Insertion)
    );
    assert_eq!(
        parse(b"\"\\ud83d\\ude00\"", Order::Insertion).unwrap(),
        Value::from("\u{1F600}")
    );
    assert_eq!(
        parse(b"\"\\u0000\"", Order::Insertion).unwrap(),
        Value::from("\0")
    );
}
