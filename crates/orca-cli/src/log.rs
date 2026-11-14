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
// Carries OrcaSlicer code: set_logging_level and level_to_boost of src/libslic3r/utils.cpp,
// OrcaSlicer v2.4.2, commit 8500fcd, translated on 2026-11-11 (orca-info-load). Copyright the
// OrcaSlicer contributors, the BambuStudio authors (Bambu Lab) and the PrusaSlicer authors
// (Prusa Research), under the AGPL-3.0.

//! Orca's log on stderr, as `cli/main.cpp` sets it up before it runs a command: one Boost.Log sink
//! (`log_to_stderr`) that writes each record to `std::clog` as `[<severity>] <message>`, with a line
//! feed added where the message has none ([`record_text`]), and `set_logging_level(log_level())`,
//! which keeps the records at or above a severity ([`threshold`]): `SKETCH_ORCA_LOG_LEVEL`
//! ([`level`]) 0 for fatal, 1 for error (the stock command line's default), 2 warning, 3 info,
//! 4 debug and 5 trace. Orca 2.4.2 is a release, so `set_logging_level` does not lower the
//! threshold to info as it does for a dev, alpha or beta version. `std::clog` writes through the C
//! runtime's text-mode `stderr`, so each line feed goes out as a carriage return and a line feed
//! on Windows.
//!
//! The crates that translate Orca's code log through `orca_mesh::log`, which sends a record
//! nowhere until a sink is set; [`install`] sets this one at the start of `main`, after the crash
//! handler, as `cli/main.cpp` does. The presets stage's own record (a printer's condition that
//! does not parse, `presets.rs`) goes the same way.

use orca_mesh::log::{Severity, set_sink};

/// `log_level()` of cli/main.cpp: SKETCH_ORCA_LOG_LEVEL as `strtoul` reads it, 0 to 5, else the
/// stock command line's 1.
#[must_use]
pub fn level() -> u32 {
    const STOCK: u32 = 1;
    let Some(env) = std::env::var_os("SKETCH_ORCA_LOG_LEVEL") else {
        return STOCK;
    };
    level_of(&env.to_string_lossy()).unwrap_or(STOCK)
}

/// The level `text` gives as `strtoul(text, &end, 10)` reads it, when it reads the whole text and
/// the value is 0 to 5.
fn level_of(text: &str) -> Option<u32> {
    let b = text.as_bytes();
    if b.is_empty() {
        return None;
    }
    let mut i = 0;
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r') {
        i += 1;
    }
    let neg = i < b.len() && b[i] == b'-';
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let start = i;
    let mut v: u64 = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        v = orca_stl::min(v * 10 + u64::from(b[i] - b'0'), u64::from(u32::MAX) + 1);
        i += 1;
    }
    if i == start || i != b.len() || v > u64::from(u32::MAX) {
        return None;
    }
    let v = u32::try_from(v).unwrap_or(u32::MAX);
    let v = if neg { v.wrapping_neg() } else { v };
    (v <= 5).then_some(v)
}

/// `level_to_boost(level)`: the lowest severity the log keeps at `level`.
#[must_use]
pub fn threshold(level: u32) -> Severity {
    match level {
        0 => Severity::Fatal,
        1 => Severity::Error,
        2 => Severity::Warning,
        3 => Severity::Info,
        4 => Severity::Debug,
        _ => Severity::Trace,
    }
}

/// What the sink writes for a record: `[<severity>] <message>`, and a line feed where the message
/// does not end with one (the text backend's `insert_if_missing`).
#[must_use]
pub fn record_text(severity: Severity, message: &str) -> String {
    let mut text = format!("[{}] {message}", severity.name());
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

/// `log_to_stderr()` and `set_logging_level(log_level())`: from now on, each record at or above
/// the level goes to stderr, flushed at once (`auto_flush`).
pub fn install() {
    let keep = threshold(level());
    set_sink(Some(Box::new(move |severity, message| {
        if severity >= keep {
            crate::write_text(&mut std::io::stderr(), &record_text(severity, message));
        }
    })));
}

#[cfg(test)]
mod tests {
    use super::{Severity, level_of, record_text, threshold};

    #[test]
    fn the_level_is_read_as_strtoul_reads_it() {
        assert_eq!(level_of("2"), Some(2));
        assert_eq!(level_of(" 3"), Some(3));
        assert_eq!(level_of("+4"), Some(4));
        assert_eq!(level_of("0"), Some(0));
        assert_eq!(level_of("5"), Some(5));
        // Past 5, not a number, or more after it: the stock level.
        for text in ["6", "", "x", "2x", "2 ", "-1", "4294967296", " "] {
            assert_eq!(level_of(text), None, "{text:?}");
        }
        // strtoul negates a negative number in unsigned arithmetic: -4294967295 is 1.
        assert_eq!(level_of("-4294967295"), Some(1));
    }

    #[test]
    fn each_level_keeps_its_severity_and_those_above() {
        assert_eq!(threshold(0), Severity::Fatal);
        assert_eq!(threshold(1), Severity::Error);
        assert_eq!(threshold(2), Severity::Warning);
        assert_eq!(threshold(3), Severity::Info);
        assert_eq!(threshold(4), Severity::Debug);
        assert_eq!(threshold(5), Severity::Trace);
        assert!(Severity::Fatal >= threshold(1) && Severity::Warning < threshold(1));
    }

    #[test]
    fn a_record_is_its_severity_and_message_on_a_line() {
        assert_eq!(record_text(Severity::Error, "a b"), "[error] a b\n");
        assert_eq!(
            record_text(Severity::Warning, "two\nlines\n"),
            "[warning] two\nlines\n"
        );
        assert_eq!(record_text(Severity::Info, ""), "[info] \n");
    }
}
