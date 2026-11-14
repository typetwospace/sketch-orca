// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The decline: a valid layers or slice job that gets past the presets, the model stage and the
//! placement of its objects (place.rs) reaches slicing, which sketch-orca-rs does not do yet, and
//! gets `not-ported`, exit code -8 and `missing`, the steps past the front-objects boundary,
//! before any output is written: the result is protocol/examples/result-not-ported.json for a
//! slice job with Orca's default settings, the progress file says the job, the presets, the model
//! and the arrange were reached and then the failure, and no G-code, .gcode.3mf or temporary file
//! is left (the work folder and its orca-data folder are made, as the C++ program makes them). An
//! info job is answered instead (info.rs). A layers job is declined at the same place, naming
//! every family of process() it reaches and the layers answer. A job that orients its objects is
//! declined where orientation would start, after the progress line "orient", naming orient. A
//! layers or slice job's settings, its objects' and their height ranges' add the families they
//! turn on (enforced support layers, printing object by object and multiline infill among them),
//! a preset value that is not text adds every one, and clumping detection with an exclusion area
//! puts the prime tower on for one filament. Presets with a compatibility condition have it worked out (a condition that
//! does not hold refuses the process or warns of the filament; one that does not parse is
//! compatible, logged or printed as Orca does it, what it prints held to runs of the C++ program
//! in tests/conditions) and reach the family preset-conditions; one past the placeholder parser
//! is declined in the presets stage. A decline before the boundary names a family this program
//! has the code for (preset-conditions, model-load, placement, info) only while
//! parity/parity.json does not certify it (common::uncertified); the decline past it names none
//! of them.

mod common;

use std::path::{Path, PathBuf};

use common::{
    Scratch, as_recorded_result, declined_progress, done_progress, fixture_files, placed_progress,
    program, progress_text, recorded_job, recorded_stream, repo, stream_text, uncertified,
    valid_job, write_fixtures, write_job, write_preset,
};
use orca_json::{Object, Order, Value, parse};

/// tests/conditions: runs of the C++ program on jobs whose compatibility conditions do not parse.
fn conditions() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("conditions")
}

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

/// What a layers job with Orca's default settings (common::write_fixtures) names once its objects
/// are placed: every family it reaches past the front-objects boundary, in parity.json's order
/// (decline.rs `command_families` and `setting_families`, as corpus.ts `familiesFor` gives them).
const LAYERS_PAST_PLACEMENT: &[&str] = &[
    "slice-mesh",
    "perimeters-arachne",
    "infill-monotonic",
    "infill-monotonicline",
    "infill-crosshatch",
    "infill-concentricinternal",
    "step-estimate-curled-extrusions",
    "step-prepare-infill",
    "step-simplify-path",
    "step-detect-overhangs-for-lift",
    "step-simplify-wall",
    "step-simplify-infill",
    "step-skirt-brim",
    "step-conflict-check",
    "tool-ordering",
    "support-check",
    "layers",
];

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
    assert_eq!(progress, placed_progress(&message_of(&result)));
    // Nothing else is written: no output folder, no temporary file, no dump; the work folder and
    // its orca-data folder are made.
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
fn a_layers_jobs_settings_turn_families_on_as_a_slices_do() {
    // Enforced support layers are supports (no support check), and printing object by object and
    // multiline infill are named, as corpus.ts's familiesFor reads a layers job's settings.
    let (_scratch, code, result, _) = declined("layers", |job, _| {
        let mut overrides = Object::new(Order::Insertion);
        overrides.insert("enforce_support_layers", "2");
        overrides.insert("print_sequence", "by object");
        overrides.insert("fill_multiline", "2");
        job.insert("overrides", overrides);
    });
    assert_eq!(code, -8, "{result}");
    let missing = missing_of(&result);
    for f in [
        "supports",
        "step-simplify-support-path",
        "infill-supportbase",
        "print-by-object",
        "fill-multiline",
        "layers",
    ] {
        assert!(missing.iter().any(|m| m == f), "{f}: {missing:?}");
    }
    for f in [
        "support-check",
        "multi-filament",
        "painted-facets",
        "gcode-export",
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
        "support-check",
        "print-by-object",
        "fill-multiline",
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

/// The declined result's members in their order, its command `command`, naming `missing`.
fn assert_not_ported(result: &str, command: &str, missing: &[String]) {
    let r = parse(result.as_bytes(), Order::Insertion).unwrap();
    assert_eq!(r["code"].as_str(), Some("not-ported"), "{result}");
    assert_eq!(r["ok"].as_bool(), Some(false));
    assert_eq!(r["exitCode"].as_f64(), Some(-8.0));
    assert_eq!(r["command"].as_str(), Some(command));
    assert_eq!(r["engine"]["implementation"].as_str(), Some("rust"));
    assert_eq!(missing_of(result), missing, "{command}");
    let keys: Vec<&str> = r.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "format", "ok", "code", "message", "missing", "exitCode", "warnings", "command",
            "engine"
        ]
    );
}

#[test]
fn a_layers_job_is_declined_once_its_objects_are_placed_and_an_info_job_is_answered() {
    // Past the front-objects boundary: every family of process() and the layers answer, whatever
    // parity.json says of the families before it.
    let (scratch, code, result, progress) = declined("layers", |_, _| {});
    assert_eq!(code, -8, "{result}");
    let past: Vec<String> = LAYERS_PAST_PLACEMENT
        .iter()
        .map(|f| (*f).to_string())
        .collect();
    assert_not_ported(&result, "layers", &past);
    assert_eq!(progress, placed_progress(&message_of(&result)));
    assert_eq!(
        scratch.files(),
        fixture_files(&["progress.ndjson", "result.json"])
    );

    let (scratch, code, result, progress) = declined("info", |_, _| {});
    assert_eq!(code, 0, "{result}");
    let r = parse(result.as_bytes(), Order::Insertion).unwrap();
    assert_eq!(
        (r["ok"].as_bool(), r["code"].as_str()),
        (Some(true), Some("ok"))
    );
    assert!(r.get("missing").is_none());
    let objects = r["info"]["objects"].as_array().unwrap();
    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0]["triangles"].as_f64(), Some(12.0));
    assert_eq!(progress, done_progress(&["job", "presets", "model"]));
    assert_eq!(
        scratch.files(),
        fixture_files(&["progress.ndjson", "result.json"])
    );
}

#[test]
fn a_job_that_orients_its_objects_is_declined_where_orientation_would_start() {
    // parity.json does not list orient until it is recorded with the orient cases: an unlisted
    // family comes last.
    let listed = parse(
        &std::fs::read(repo().join("parity/parity.json")).unwrap(),
        Order::Insertion,
    )
    .unwrap()["families"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["name"].as_str() == Some("orient"));
    // A layers job reaches placement, then every family past the front-objects boundary, which
    // this program has no code for, so the decline names them whatever parity.json certifies.
    for (command, after, past) in [
        ("info", vec!["info"], &[][..]),
        ("layers", vec!["placement"], LAYERS_PAST_PLACEMENT),
    ] {
        let (scratch, code, result, progress) = declined(command, |job, _| {
            job.insert("orient", true);
        });
        assert_eq!(code, -8, "{result}");
        let mut missing = uncertified(&["model-load"]);
        if listed {
            missing.push("orient".to_string());
        }
        missing.extend(uncertified(&after));
        missing.extend(past.iter().map(|f| (*f).to_string()));
        if !listed {
            missing.push("orient".to_string());
        }
        assert_not_ported(&result, command, &missing);
        assert_eq!(
            progress,
            progress_text(
                &["job", "presets", "model", "orient"],
                Some(&message_of(&result))
            ),
            "{command}"
        );
        assert_eq!(
            scratch.files(),
            fixture_files(&["progress.ndjson", "result.json"])
        );
    }
}

/// Runs a valid `command` job whose process (and filament) presets get `process` and `filament`
/// as settings, with `env`: the scratch folder, the exit code, the result, the progress file, and
/// what the program wrote on stdout and stderr.
fn with_conditions(
    command: &str,
    process: &[(&'static str, &'static str)],
    filament: &[(&'static str, &'static str)],
    env: &[(&str, &str)],
) -> (Scratch, i32, String, String, String, String) {
    let scratch = Scratch::new("decline", "conditions");
    write_fixtures(&scratch);
    write_preset(&scratch, "process", "process", &text(process));
    write_preset(&scratch, "filament_1", "filament", &text(filament));
    let job = valid_job(&scratch, command);
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
        env,
    );
    let result = std::fs::read_to_string(&out).unwrap();
    let progress = std::fs::read_to_string(&progress).unwrap();
    (
        scratch,
        r.status.code().unwrap(),
        result,
        progress,
        String::from_utf8_lossy(&r.stdout).into_owned(),
        String::from_utf8_lossy(&r.stderr).into_owned(),
    )
}

/// An info job whose presets reach preset-conditions: answered, as the conditions held.
fn assert_answered_with_conditions(code: i32, result: &str, progress: &str) {
    assert_eq!(code, 0, "{result}");
    let r = parse(result.as_bytes(), Order::Insertion).unwrap();
    assert_eq!(r["code"].as_str(), Some("ok"));
    assert!(r.get("info").is_some(), "{result}");
    assert_eq!(progress, done_progress(&["job", "presets", "model"]));
}

/// The decline of a layers job whose presets reach preset-conditions: once its objects are placed,
/// naming the families past the front-objects boundary alone, as preset-conditions is behind it.
fn assert_declined_with_conditions(code: i32, result: &str, progress: &str) {
    assert_eq!(code, -8, "{result}");
    assert_eq!(missing_of(result), LAYERS_PAST_PLACEMENT);
    assert_eq!(progress, placed_progress(&message_of(result)));
}

#[test]
fn a_compatibility_condition_is_worked_out_and_the_job_reaches_preset_conditions() {
    // A process with a condition and no list of printers: Orca works the condition out against
    // the printer (Orca's defaults: one nozzle of 0.4 mm). The info job is answered; the layers
    // job goes on to the decline, which names preset-conditions while it is not certified.
    let process = [(
        "compatible_printers_condition",
        "nozzle_diameter[0]==0.4 and num_extruders==1 and printer_preset==\"Test machine\"",
    )];
    let (scratch, code, result, progress, stdout, stderr) =
        with_conditions("info", &process, &[], &[]);
    assert_answered_with_conditions(code, &result, &progress);
    assert!(stdout.is_empty() && stderr.is_empty(), "{stdout}{stderr}");
    assert_eq!(
        scratch.files(),
        fixture_files(&["progress.ndjson", "result.json"])
    );
    let (scratch, code, result, progress, stdout, stderr) =
        with_conditions("layers", &process, &[], &[]);
    assert_declined_with_conditions(code, &result, &progress);
    assert!(stdout.is_empty() && stderr.is_empty(), "{stdout}{stderr}");
    assert_eq!(
        scratch.files(),
        fixture_files(&["progress.ndjson", "result.json"])
    );
}

#[test]
fn a_condition_that_does_not_hold_refuses_the_process_and_warns_of_a_filament() {
    let (_scratch, code, result, _, _, _) = with_conditions(
        "info",
        &[("compatible_printers_condition", "nozzle_diameter[0]==0.6")],
        &[],
        &[],
    );
    let r = parse(result.as_bytes(), Order::Insertion).unwrap();
    assert_eq!(
        (code, r["code"].as_str()),
        (-17, Some("process-not-compatible"))
    );

    // The info job is answered, with the warning.
    let (_scratch, code, result, _, _, _) = with_conditions(
        "info",
        &[],
        &[("compatible_prints_condition", "layer_height > 1")],
        &[],
    );
    assert_eq!(code, 0, "{result}");
    let r = parse(result.as_bytes(), Order::Insertion).unwrap();
    let first = &r["warnings"].as_array().unwrap()[0];
    assert_eq!(first["code"].as_str(), Some("filament-not-compatible"));
    assert_eq!(
        first["message"].as_str(),
        Some(
            "presets.filaments[0]: \"Test filament\" is not made for the process \"Test process\"; Orca slices with it anyway"
        )
    );
}

#[test]
fn a_condition_that_does_not_parse_is_compatible_and_reported_as_orca_reports_it() {
    // The printer's check logs a warning on stderr, at log level 2 or more only.
    let printers = [("compatible_printers_condition", "foo == 1")];
    let (_scratch, code, result, progress, stdout, stderr) =
        with_conditions("info", &printers, &[], &[]);
    assert_answered_with_conditions(code, &result, &progress);
    assert!(stdout.is_empty() && stderr.is_empty(), "{stdout}{stderr}");
    let (_scratch, code, result, progress, stdout, stderr) =
        with_conditions("info", &printers, &[], &[("SKETCH_ORCA_LOG_LEVEL", "2")]);
    assert_answered_with_conditions(code, &result, &progress);
    assert!(stdout.is_empty(), "{stdout}");
    assert_eq!(
        stderr,
        stream_text(
            "[warning] Slic3r::is_compatible_with_printer: parsing error of compatible_printers_condition Test machine: Parsing error at line 1: Not a variable name\nfoo == 1\n^\n"
        )
    );
    // The same in a layers job, which goes on to the decline.
    let (_scratch, code, result, progress, stdout, stderr) =
        with_conditions("layers", &printers, &[], &[("SKETCH_ORCA_LOG_LEVEL", "2")]);
    assert_declined_with_conditions(code, &result, &progress);
    assert!(stdout.is_empty(), "{stdout}");
    assert!(
        stderr.starts_with(&stream_text(
            "[warning] Slic3r::is_compatible_with_printer: "
        )),
        "{stderr}"
    );
    // The process's check prints on stdout, at any level: what one run of the staged C++ program
    // printed for this job (2026-11-11, orca-info-place).
    let (_scratch, code, result, progress, stdout, stderr) =
        with_conditions("info", &[], &[("compatible_prints_condition", "1 +")], &[]);
    assert_answered_with_conditions(code, &result, &progress);
    assert!(stderr.is_empty(), "{stderr}");
    let printed =
        std::fs::read_to_string(conditions().join("test-presets-filament-condition-1-plus.stdout"))
            .unwrap();
    assert_eq!(stdout, recorded_stream(&printed));
}

#[test]
fn a_filament_condition_that_does_not_parse_prints_what_the_cpp_program_printed() {
    // The corpus's preset-filament-condition-unparsable: its filament's
    // compatible_prints_condition, "layer_height >=", does not parse, so the filament is taken as
    // compatible and the parser's message is printed on stdout. One run of the staged C++ program
    // (2026-11-11, orca-info-place) gave tests/conditions/preset-filament-condition-unparsable.
    let case = conditions().join("preset-filament-condition-unparsable");
    let scratch = Scratch::new("decline", "condition-unparsable");
    let job = write_job(&scratch, &recorded_job(&scratch, &case));
    let (out, progress) = (scratch.path("result.json"), scratch.path("progress.ndjson"));
    let r = program(
        &["run", "--job", &job, "--out", &out, "--progress", &progress],
        &[],
    );
    let result = std::fs::read_to_string(&out).unwrap();
    assert_eq!(r.status.code(), Some(0), "{result}");
    let want = std::fs::read_to_string(case.join("result.json")).unwrap();
    assert_eq!(as_recorded_result(&result, &want), want);
    assert_eq!(
        std::fs::read_to_string(&progress).unwrap(),
        std::fs::read_to_string(case.join("progress.ndjson")).unwrap()
    );
    let printed = std::fs::read_to_string(case.join("stdout")).unwrap();
    assert_eq!(
        String::from_utf8_lossy(&r.stdout),
        recorded_stream(&printed)
    );
    assert!(
        r.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&r.stderr)
    );
}

#[test]
fn a_condition_past_the_placeholder_parser_is_declined_in_the_presets_stage() {
    // An info job too: the presets stage declines it before the model is read.
    let (scratch, code, result, progress, stdout, stderr) = with_conditions(
        "info",
        &[("compatible_printers_condition", "printer_model =~ /(?=x)/")],
        &[],
        &[],
    );
    assert_eq!(code, -8, "{result}");
    assert!(stdout.is_empty() && stderr.is_empty(), "{stdout}{stderr}");
    let mut missing = vec!["preset-conditions".to_string()];
    missing.extend(uncertified(&["model-load", "info"]));
    assert_eq!(missing_of(&result), missing);
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
