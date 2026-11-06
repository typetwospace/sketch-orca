// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! What the parts of the program share, as cli/common.cpp gives it: the named failures with the
//! stock Orca command line's exit code for each ([`FAILURE_TABLE`], [`exit_code_for`],
//! [`Failure`]), and the text helpers the job's checks use ([`format_number`], [`lower`],
//! [`ends_with_ci`]).

use orca_fmt::printf::{Arg, sprintf_string};

/// The failure codes and the exit code of each, in the order of `cli/common.cpp`'s table (the
/// stock Orca command line's `cli_errors` for the same meanings).
pub const FAILURE_TABLE: &[(&str, i32)] = &[
    ("ok", 0),
    ("environment", -1),
    ("internal-error", -1),
    ("invalid-params", -2),
    ("job-invalid", -2),
    ("setting-invalid", -2),
    ("layer-profile-invalid", -2),
    ("custom-gcode-invalid", -2),
    ("file-not-found", -3),
    ("preset-invalid", -5),
    ("model-unreadable", -6),
    ("not-fdm", -7),
    ("unsupported", -8),
    ("output-failed", -13),
    ("out-of-memory", -14),
    ("process-not-compatible", -17),
    ("config-invalid", -18),
    ("arrange-failed", -21),
    ("orient-failed", -22),
    ("empty-plate", -50),
    ("validate-error", -51),
    ("not-printable", -52),
    ("filament-bed-mismatch", -61),
    ("temperature-difference", -62),
    ("collision", -63),
    ("slice-failed", -100),
    ("path-conflicts", -101),
    ("gcode-in-unprintable-area", -102),
];

/// The exit code of a failure code (`exit_code_for`): 0 for "ok", -1 for a name not in the table.
#[must_use]
pub fn exit_code_for(code: &str) -> i32 {
    FAILURE_TABLE
        .iter()
        .find(|e| e.0 == code)
        .map_or(-1, |e| e.1)
}

/// A failure the result names (`struct Failure`): its code, its message, the object it is about
/// (-1 for none) and the exit code. `missing` is the decline's list of families (decline.rs), empty
/// for every other failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub code: String,
    pub message: String,
    pub object_index: i32,
    pub exit_code: i32,
    pub missing: Vec<String>,
}

/// `fail(code, message)`: the failure with the code's exit code and no object.
#[must_use]
pub fn fail(code: &str, message: impl Into<String>) -> Failure {
    fail_object(code, message, -1)
}

/// `fail(code, message, object_index)`.
#[must_use]
pub fn fail_object(code: &str, message: impl Into<String>, object_index: i32) -> Failure {
    Failure {
        code: code.to_string(),
        message: message.into(),
        object_index,
        exit_code: exit_code_for(code),
        missing: Vec::new(),
    }
}

/// `format_number(v)`: a number as Orca writes it in its settings, "%.0f" for a whole number below
/// 1e15 and "%.12g" otherwise.
#[must_use]
pub fn format_number(v: f64) -> String {
    let format = if v.is_finite() && v.floor() == v && v.abs() < 1e15 {
        "%.0f"
    } else {
        "%.12g"
    };
    sprintf_string(format, &[Arg::Double(v)]).expect("a double formats")
}

/// `lower(s)`: `std::tolower` of each byte in the "C" locale, ASCII letters only.
#[must_use]
pub fn lower(s: &str) -> String {
    s.to_ascii_lowercase()
}

/// `ends_with_ci(s, suffix)`: the end of `s`, compared without ASCII letter case.
#[must_use]
pub fn ends_with_ci(s: &str, suffix: &str) -> bool {
    let (s, suffix) = (s.as_bytes(), suffix.as_bytes());
    s.len() >= suffix.len() && s[s.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}

#[cfg(test)]
mod tests {
    use super::{ends_with_ci, exit_code_for, format_number, lower};

    #[test]
    fn numbers_are_written_as_orca_writes_settings() {
        assert_eq!(format_number(0.0), "0");
        assert_eq!(format_number(12.0), "12");
        assert_eq!(format_number(-3.0), "-3");
        assert_eq!(format_number(0.2), "0.2");
        assert_eq!(format_number(5.05), "5.05");
        assert_eq!(format_number(0.1 + 0.2), "0.3");
        assert_eq!(format_number(1e15), "1e+15");
        assert_eq!(format_number(123_456_789_012_345.0), "123456789012345");
        assert_eq!(format_number(1.5e-7), "1.5e-07");
        assert_eq!(format_number(f64::INFINITY), "inf");
    }

    #[test]
    fn the_table_and_the_text_helpers() {
        assert_eq!(exit_code_for("ok"), 0);
        assert_eq!(exit_code_for("unsupported"), -8);
        assert_eq!(exit_code_for("no-such-code"), -1);
        assert_eq!(lower("Inherits_\u{e9}"), "inherits_\u{e9}");
        assert!(ends_with_ci("C:\\m\\Cube.STL", ".stl"));
        assert!(!ends_with_ci("stl", ".stl"));
    }
}
