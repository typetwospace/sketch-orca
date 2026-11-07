// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The decline: a valid job that gets past the presets and the model's file check and reaches a
//! step sketch-orca-rs does not do yet (every info, layers and slice job, for now) gets
//! `not-ported`, exit code -8 and `missing`, the steps, before any output is written: the result
//! is protocol/examples/result-not-ported.json for a slice job with Orca's default settings, the
//! progress file says the job, the presets and the model were reached and then the failure, and
//! no G-code, .gcode.3mf or temporary file is left (the work folder and its orca-data folder are
//! made, as the C++ program makes them). A slice's settings, its objects' and their height
//! ranges' add the families they turn on, a preset value that is not text adds every one, and
//! clumping detection with an exclusion area puts the prime tower on for one filament. Presets
//! that need a compatibility condition worked out are declined in the presets stage, with the
//! family preset-conditions.

mod common;

use common::{
    Scratch, declined_progress, fixture_files, program, progress_text, repo, valid_job,
    write_fixtures, write_job, write_preset,
};
use orca_json::{Object, Order, Value, parse};

/// Runs a valid `command` job, its fixtures written (common::write_fixtures) before `change`, with
/// --out and --progress; the scratch folder, the exit code, the result and the progress file.
fn declined(
    command: &str,
    change: impl FnOnce(&mut Object, &Scratch),
) -> (Scratch, i32, String, String) {
    let scratch = Scratch::new("decline", command);
    write_fixtures(&scratch);
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

/// Settings as preset values: text.
fn text(settings: &[(&'static str, &'static str)]) -> Vec<(&'static str, Value)> {
    settings
        .iter()
        .map(|(k, v)| (*k, Value::from(*v)))
        .collect()
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

/// The message of a result.
fn message_of(result: &str) -> String {
    parse(result.as_bytes(), Order::Insertion).unwrap()["message"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn a_slice_job_is_declined_with_the_steps_it_needs() {
    let (scratch, code, result, progress) = declined("slice", |job, dir| {
        let mut output = Object::new(Order::Insertion);
        output.insert("gcode", dir.path("out/plate_1.gcode"));
        output.insert("bambu3mf", dir.path("out/plate_1.gcode.3mf"));
        job.insert("output", output);
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
    assert_eq!(progress, declined_progress(&message_of(&result)));
    // Nothing else is written: no output folder, no temporary file; the work folder and its
    // orca-data folder are made.
    assert_eq!(
        scratch.files(),
        fixture_files(&["progress.ndjson", "result.json"])
    );
    assert!(std::path::Path::new(&scratch.path("work/orca-data")).is_dir());
    assert!(!std::path::Path::new(&scratch.path("out")).exists());
}

#[test]
fn the_families_a_slice_reaches_follow_its_settings_and_each_objects() {
    let (_scratch, code, result, _) = declined("slice", |job, dir| {
        write_preset(
            dir,
            "process",
            "process",
            &text(&[("enable_support", "1"), ("sparse_infill_density", "0%")]),
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
        object.insert("heightRanges", vec![Value::Object(range)]);
        job.insert("objects", vec![Value::Object(object)]);
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
fn a_slice_whose_preset_values_are_not_text_reaches_every_family_a_setting_turns_on() {
    // Orca reads the number; the corpus's lookup reads only text, so the settings are unknown.
    let (_scratch, code, result, _) = declined("slice", |_, dir| {
        write_preset(
            dir,
            "process",
            "process",
            &[("raft_layers", Value::from(2))],
        );
    });
    assert_eq!(code, -8, "{result}");
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
fn clumping_detection_with_an_exclusion_area_reaches_the_prime_tower_on_one_filament() {
    let area = Value::Array(
        ["145x310", "256x310", "256x326", "145x326"]
            .iter()
            .map(|p| Value::from(*p))
            .collect(),
    );
    for (detection, tower) in [("1", true), ("0", false)] {
        let (_scratch, code, result, _) = declined("slice", |job, dir| {
            write_preset(
                dir,
                "machine",
                "machine",
                &[("wrapping_exclude_area", area.clone())],
            );
            write_preset(
                dir,
                "process",
                "process",
                &text(&[("enable_prime_tower", "1")]),
            );
            let mut overrides = Object::new(Order::Insertion);
            overrides.insert("enable_wrapping_detection", detection);
            job.insert("overrides", overrides);
        });
        assert_eq!(code, -8, "{result}");
        let missing = missing_of(&result);
        assert_eq!(
            missing.iter().any(|m| m == "wipe-tower"),
            tower,
            "{detection}: {missing:?}"
        );
    }
}

#[test]
fn info_and_layers_jobs_are_declined_too() {
    for (command, missing) in [
        ("info", vec!["model-load", "placement", "info"]),
        ("layers", vec!["model-load", "placement", "slice-mesh"]),
    ] {
        let (scratch, code, result, progress) = declined(command, |_, _| {});
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
        assert_eq!(progress, declined_progress(&message_of(&result)));
        assert_eq!(
            scratch.files(),
            fixture_files(&["progress.ndjson", "result.json"])
        );
    }
}

#[test]
fn a_compatibility_condition_is_declined_in_the_presets_stage() {
    // A process with a condition and no list of printers: Orca works the condition out.
    let (scratch, code, result, progress) = declined("info", |_, dir| {
        write_preset(
            dir,
            "process",
            "process",
            &text(&[("compatible_printers_condition", "nozzle_diameter[0]==0.4")]),
        );
    });
    assert_eq!(code, -8, "{result}");
    let mut missing = missing_of(&result);
    orca_stl::sort(&mut missing);
    assert_eq!(
        missing,
        ["info", "model-load", "placement", "preset-conditions"]
    );
    assert_eq!(
        progress,
        progress_text(&["job", "presets"], Some(&message_of(&result)))
    );
    assert_eq!(
        scratch.files(),
        fixture_files(&["progress.ndjson", "result.json"])
    );
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

#[test]
fn the_presets_and_the_models_file_are_checked_before_the_decline() {
    // Each is refused as the C++ refuses it, not declined: a preset file that is not there, an
    // override Orca does not have, and a model that is not there.
    let (scratch, code, result, progress) = declined("info", |_, dir| {
        std::fs::remove_file(dir.path("presets/process.json")).unwrap();
    });
    let message = format!(
        "presets.process: there is no file {}",
        scratch.path("presets/process.json")
    );
    assert_eq!((code, message_of(&result)), (-3, message.clone()));
    assert_eq!(progress, progress_text(&["job", "presets"], Some(&message)));

    let (_scratch, code, result, _) = declined("info", |job, _| {
        let mut overrides = Object::new(Order::Insertion);
        overrides.insert("no_such_setting_here", "1");
        job.insert("overrides", overrides);
    });
    assert_eq!(
        (code, message_of(&result)),
        (
            -2,
            "overrides: no_such_setting_here is not a setting of Orca 2.4.2".to_string()
        )
    );

    let (scratch, code, result, progress) = declined("info", |_, dir| {
        std::fs::remove_file(dir.path("model.stl")).unwrap();
    });
    let message = format!("the model {} does not exist", scratch.path("model.stl"));
    assert_eq!((code, message_of(&result)), (-3, message.clone()));
    assert_eq!(progress, declined_progress(&message));
}
