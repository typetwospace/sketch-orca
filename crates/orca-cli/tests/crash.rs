// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The crash result: an access violation, an abort or a panic while a run is armed writes the
//! C++ program's internal-error result ("the engine crashed (<what>) during '<stage>', in Orca's
//! code"), and the process ends with -1, through `SKETCH_ORCA_TEST_CRASH` as the smoke case
//! crash-result sets it. sketch-orca-rs reaches the stages `job`, `presets` and `model`: a crash
//! at `presets` or `model` crashes as the C++ program does there, before that stage's progress
//! line, and the point `decline`, past the model's file check, names the stage `model`. A crash at
//! the stage `job`, before the result is armed, writes no result, as in the C++ program.

mod common;

use common::{
    Scratch, as_cpp_result, fixture_files, program, progress_text, valid_job, write_fixtures,
    write_job,
};
use orca_json::{Order, parse};

/// A valid slice job, its fixtures written (common::write_fixtures), run with
/// `SKETCH_ORCA_TEST_CRASH=<trigger>`: the folder, the exit code, the result (if any) and the
/// progress file.
fn crashed(trigger: &str) -> (Scratch, i32, Option<String>, String) {
    let scratch = Scratch::new("crash", &trigger.replace(':', "-"));
    write_fixtures(&scratch);
    let job = write_job(&scratch, &valid_job(&scratch, "slice"));
    let (out, progress) = (scratch.path("result.json"), scratch.path("progress.ndjson"));
    let r = program(
        &["run", "--job", &job, "--out", &out, "--progress", &progress],
        &[("SKETCH_ORCA_TEST_CRASH", trigger)],
    );
    assert!(r.stdout.is_empty() && r.stderr.is_empty(), "{r:?}");
    (
        scratch,
        r.status.code().unwrap(),
        std::fs::read_to_string(&out).ok(),
        std::fs::read_to_string(&progress).unwrap_or_default(),
    )
}

/// The C++ program's crash result for a slice job, with `message`.
fn crash_result(message: &str) -> String {
    let mut text = String::from(concat!(
        "{\n",
        "  \"format\": \"sketch-orca-result/1\",\n",
        "  \"ok\": false,\n",
        "  \"code\": \"internal-error\",\n",
    ));
    text.push_str(&format!("  \"message\": \"{message}\",\n"));
    text.push_str("  \"exitCode\": -1,\n  \"warnings\": [],\n  \"command\": \"slice\",\n");
    text
}

/// Checks a crash at `trigger` that names `stage`, with the progress lines of `lines` before it
/// and `what` opening the crash's description.
fn check_crash(trigger: &str, what: &str, stage: &str, lines: &[&str]) {
    let (scratch, code, result, progress) = crashed(trigger);
    assert_eq!(code, -1, "{trigger}");
    let result = result.unwrap_or_else(|| panic!("{trigger}: no result"));
    let r = parse(result.as_bytes(), Order::Insertion).unwrap();
    let message = r["message"].as_str().unwrap();
    assert!(
        message.starts_with(&format!("the engine crashed ({what}"))
            && message.ends_with(&format!(") during '{stage}', in Orca's code")),
        "{trigger}: {message}"
    );
    let cpp = as_cpp_result(&result);
    assert!(cpp.starts_with(&crash_result(message)), "{trigger}: {cpp}");
    assert_eq!(progress, progress_text(lines, None), "{trigger}");
    assert_eq!(
        scratch.files(),
        fixture_files(&["progress.ndjson", "result.json"]),
        "{trigger}: no temporary file is left"
    );
    // The work folder is made after the presets line, before the presets are read.
    assert_eq!(
        std::path::Path::new(&scratch.path("work/orca-data")).is_dir(),
        lines.contains(&"presets"),
        "{trigger}"
    );
}

#[test]
fn an_access_violation_an_abort_and_a_panic_write_the_internal_error_result() {
    for (trigger, what) in [
        ("decline", "exception 0xC0000005 at "),
        ("decline:abort", "abort"),
        ("decline:panic", "panic at crash.rs:"),
    ] {
        check_crash(trigger, what, "model", &["job", "presets", "model"]);
    }
}

#[test]
fn a_crash_at_the_presets_or_the_model_crashes_there_as_the_cpp_program_does() {
    for (stage, lines) in [
        ("presets", &["job"][..]),
        ("model", &["job", "presets"][..]),
    ] {
        for (how, what) in [
            ("", "exception 0xC0000005 at "),
            (":abort", "abort"),
            (":panic", "panic at crash.rs:"),
        ] {
            check_crash(&format!("{stage}{how}"), what, stage, lines);
        }
    }
}

#[test]
fn the_access_violations_place_is_a_module_and_an_offset_as_the_mask_reads_it() {
    let (_scratch, _, result, _) = crashed("decline");
    let r = parse(result.unwrap().as_bytes(), Order::Insertion).unwrap();
    let message = r["message"].as_str().unwrap();
    let place = message
        .strip_prefix("the engine crashed (exception 0xC0000005 at ")
        .and_then(|m| m.strip_suffix(") during 'model', in Orca's code"))
        .unwrap();
    let (module, offset) = place.split_once("+0x").unwrap();
    assert!(
        !module.is_empty()
            && module
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'.' || c == b'-' || c == b'_'),
        "{module}"
    );
    assert!(
        !offset.is_empty()
            && offset
                .bytes()
                .all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f')),
        "{offset}"
    );
}

#[test]
fn a_crash_before_the_result_is_armed_writes_none() {
    for trigger in ["job", "job:abort", "job:panic"] {
        let (scratch, code, result, progress) = crashed(trigger);
        assert_eq!(code, -1, "{trigger}");
        assert!(result.is_none(), "{trigger}");
        assert_eq!(progress, "", "{trigger}");
        assert_eq!(
            scratch.files(),
            fixture_files(&["progress.ndjson"]),
            "{trigger}"
        );
    }
}
