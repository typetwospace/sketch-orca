// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The decline: a valid job that reaches a step sketch-orca-rs does not do yet (every info, layers
//! and slice job, for now) gets `not-ported`, exit code -8 and `missing`, the steps, before any
//! output is written: the result is protocol/examples/result-not-ported.json for a slice job with
//! Orca's default settings, the progress file says the job was read and then the failure, and no
//! G-code, .gcode.3mf or temporary file is left. A slice's settings, its objects' and their height
//! ranges' add the families they turn on, and presets that cannot be read add every one.

mod common;

use common::{Scratch, program, repo, valid_job, write_job};
use orca_json::{Object, Order, parse};

/// Runs a valid `command` job with --out and --progress; the scratch folder, the exit code, the
/// result and the progress file.
fn declined(
    command: &str,
    change: impl FnOnce(&mut Object, &Scratch),
) -> (Scratch, i32, String, String) {
    let scratch = Scratch::new("decline", command);
    let mut job = valid_job(&scratch, command);
    change(&mut job, &scratch);
    let path = write_job(&scratch, &job);
    let (out, progress) = (scratch.path("result.json"), scratch.path("progress.ndjson"));
    let r = program(
        &[
            "run",
            "--job",
            &path,
            "--out",
            &out,
            "--progress",
            &progress,
        ],
        &[],
    );
    assert!(r.stdout.is_empty() && r.stderr.is_empty(), "{r:?}");
    let result = std::fs::read_to_string(&out).unwrap();
    let progress = std::fs::read_to_string(&progress).unwrap();
    (scratch, r.status.code().unwrap(), result, progress)
}

/// Writes the job's three preset files, flattened, each with `settings` besides its type and name.
fn write_presets(dir: &Scratch, process: &[(&str, &str)], machine: &[(&str, &str)]) {
    std::fs::create_dir_all(dir.path("presets")).unwrap();
    for (file, kind, settings) in [
        ("process", "process", process),
        ("machine", "machine", machine),
        ("filament_1", "filament", &[][..]),
    ] {
        let mut p = Object::new(Order::Insertion);
        p.insert("type", kind);
        p.insert("name", format!("Test {kind}"));
        p.insert("from", "system");
        for (k, v) in settings {
            p.insert(*k, *v);
        }
        std::fs::write(
            dir.path(&format!("presets/{file}.json")),
            orca_json::Value::Object(p).dump(Some(2)) + "\n",
        )
        .unwrap();
    }
}

/// The families a result names as missing.
fn missing_of(result: &str) -> Vec<String> {
    parse(result.as_bytes(), Order::Insertion).unwrap()["missing"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn a_slice_job_is_declined_with_the_steps_it_needs() {
    let (scratch, code, result, progress) = declined("slice", |job, dir| {
        let mut output = Object::new(Order::Insertion);
        output.insert("gcode", dir.path("out/plate_1.gcode"));
        output.insert("bambu3mf", dir.path("out/plate_1.gcode.3mf"));
        job.insert("output", output);
        // Presets that leave every setting at Orca's default.
        write_presets(dir, &[], &[]);
    });
    assert_eq!(code, -8);
    let example =
        std::fs::read_to_string(repo().join("protocol/examples/result-not-ported.json")).unwrap();
    let example_version =
        parse(example.as_bytes(), Order::Insertion).unwrap()["engine"]["engineVersion"]
            .as_str()
            .unwrap()
            .to_string();
    let example = example.replacen(
        &format!("\"engineVersion\": \"{example_version}\""),
        &format!("\"engineVersion\": \"{}\"", env!("CARGO_PKG_VERSION")),
        1,
    );
    assert_eq!(result, example);
    let message = parse(result.as_bytes(), Order::Insertion).unwrap()["message"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(progress, common::failed_at_intake_progress(&message));
    // Nothing else is written: no output folder, no temporary file.
    assert_eq!(
        scratch.files(),
        [
            "job.json",
            "presets/filament_1.json",
            "presets/machine.json",
            "presets/process.json",
            "progress.ndjson",
            "result.json"
        ]
    );
}

#[test]
fn the_families_a_slice_reaches_follow_its_settings_and_each_objects() {
    let (_scratch, code, result, _) = declined("slice", |job, dir| {
        write_presets(
            dir,
            &[("enable_support", "1"), ("sparse_infill_density", "0%")],
            &[],
        );
        let mut overrides = Object::new(Order::Insertion);
        overrides.insert("wall_generator", "classic");
        job.insert("overrides", overrides);
        // One object irons; a height range of it has fuzzy skin.
        let mut range_overrides = Object::new(Order::Insertion);
        range_overrides.insert("fuzzy_skin", "external");
        let mut range = Object::new(Order::Insertion);
        range.insert("from", 0.0);
        range.insert("to", 2.0);
        range.insert("overrides", range_overrides);
        let mut object_overrides = Object::new(Order::Insertion);
        object_overrides.insert("ironing_type", "top");
        let mut object = Object::new(Order::Insertion);
        object.insert("index", 0);
        object.insert("overrides", object_overrides);
        object.insert("heightRanges", vec![orca_json::Value::Object(range)]);
        job.insert("objects", vec![orca_json::Value::Object(object)]);
    });
    assert_eq!(code, -8);
    let missing = missing_of(&result);
    for f in [
        "perimeters-classic",
        "supports",
        "step-simplify-support-path",
        "infill-supportbase",
        "ironing",
        "fuzzy-skin",
    ] {
        assert!(missing.iter().any(|m| m == f), "{f}: {missing:?}");
    }
    // Arachne nowhere, and no sparse infill at 0%.
    for f in [
        "perimeters-arachne",
        "infill-crosshatch",
        "raft",
        "wipe-tower",
    ] {
        assert!(!missing.iter().any(|m| m == f), "{f}: {missing:?}");
    }
}

#[test]
fn a_slice_whose_presets_cannot_be_read_reaches_every_family_a_setting_turns_on() {
    // valid_job names preset files it does not write.
    let (_scratch, code, result, _) = declined("slice", |_, _| {});
    assert_eq!(code, -8);
    let missing = missing_of(&result);
    for f in [
        "perimeters-classic",
        "perimeters-arachne",
        "infill-gyroid",
        "infill-lightning",
        "supports",
        "tree-supports",
        "raft",
        "ironing",
        "fuzzy-skin",
        "wipe-tower",
        "step-contouring",
        "step-simplify-support-path",
    ] {
        assert!(missing.iter().any(|m| m == f), "{f}: {missing:?}");
    }
}

#[test]
fn info_and_layers_jobs_are_declined_too() {
    for (command, missing) in [
        ("info", vec!["model-load", "placement", "info"]),
        ("layers", vec!["model-load", "placement", "slice-mesh"]),
    ] {
        let (scratch, code, result, _) = declined(command, |_, _| {});
        assert_eq!(code, -8, "{command}");
        let r = parse(result.as_bytes(), Order::Insertion).unwrap();
        assert_eq!(r["code"].as_str(), Some("not-ported"));
        assert_eq!(r["ok"].as_bool(), Some(false));
        assert_eq!(r["exitCode"].as_f64(), Some(-8.0));
        assert_eq!(r["command"].as_str(), Some(command));
        assert_eq!(r["engine"]["implementation"].as_str(), Some("rust"));
        let got: Vec<&str> = r["missing"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m.as_str().unwrap())
            .collect();
        assert_eq!(got, missing, "{command}");
        let keys: Vec<&str> = r.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "format", "ok", "code", "message", "missing", "exitCode", "warnings", "command",
                "engine"
            ]
        );
        assert_eq!(
            scratch.files(),
            ["job.json", "progress.ndjson", "result.json"]
        );
    }
}

#[test]
fn a_job_is_checked_whole_before_it_is_declined() {
    // An invalid job is refused as the C++ refuses it, not declined.
    let (_scratch, code, result, _) = declined("info", |job, _| {
        job.insert("orient", "yes");
    });
    assert_eq!(code, -2);
    let r = parse(result.as_bytes(), Order::Insertion).unwrap();
    assert_eq!(r["code"].as_str(), Some("job-invalid"));
    assert_eq!(r["message"].as_str(), Some("orient must be true or false"));
    assert!(r.get("missing").is_none());
}
