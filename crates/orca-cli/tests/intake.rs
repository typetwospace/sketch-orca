// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! `sketch-orca-rs run` on invalid jobs, against the C++ program's results
//! (parity/goldens/front/intake/): the invalid examples of protocol/examples/invalid, moved into a
//! folder as the parity corpus moves them, the smoke refusals the C++ refuses while reading the
//! job, and those it refuses in the presets stage (an override Orca does not have or a value it
//! does not take; a preset file that is missing, not JSON, not an object, of the wrong type, from
//! elsewhere, with a value Orca cannot read or for a resin printer; a process made for another
//! printer; the flushing volumes' inputs and Orca's own check), in its folders (no profiles, a work
//! folder that cannot be made) or at the model's file (a model that is not there, with and without
//! the stage's warnings) get the same result, byte for byte (the engine block apart: this
//! program's version and `implementation`), the same exit code and the same progress file; every
//! intake refusal of the corpus is one of them. The run's files are kept apart as the C++ keeps
//! them: a result that would replace a file of the run is not written, a progress file that would
//! is refused, and a job whose outputs would replace an input is refused; a command line it does
//! not take still gets its result.

mod common;

use common::{
    Scratch, as_cpp_result, failed_at_intake_progress, fixture_files, program, progress_text, repo,
    stream_text, valid_job, write_fixtures, write_job, write_named_preset,
};
use orca_json::{Object, Order, Value, parse};

/// The intake golden of corpus case `id`.
fn golden(id: &str) -> String {
    std::fs::read_to_string(repo().join(format!("parity/goldens/front/intake/{id}.result.json")))
        .unwrap()
}

/// Runs the job at `job` with --out and --progress in `dir`; the exit code, the result and the
/// progress file.
fn run(dir: &Scratch, job: &str) -> (i32, String, String) {
    let (out, progress) = (dir.path("result.json"), dir.path("progress.ndjson"));
    let r = program(
        &["run", "--job", job, "--out", &out, "--progress", &progress],
        &[],
    );
    assert!(r.stdout.is_empty() && r.stderr.is_empty(), "{r:?}");
    (
        r.status.code().unwrap(),
        std::fs::read_to_string(&out).unwrap(),
        std::fs::read_to_string(&progress).unwrap_or_default(),
    )
}

/// Checks a run against the golden of `id`: the result's bytes, the exit code, the progress.
fn check_against_golden(id: &str, dir: &Scratch, job: &str) {
    let want = golden(id);
    let w = parse(want.as_bytes(), Order::Insertion).unwrap();
    let (code, result, progress) = run(dir, job);
    assert_eq!(as_cpp_result(&result), want, "{id}");
    assert_eq!(
        i64::from(code),
        w["exitCode"].as_f64().unwrap() as i64,
        "{id}"
    );
    assert_eq!(
        progress,
        failed_at_intake_progress(w["message"].as_str().unwrap()),
        "{id}"
    );
    assert_eq!(
        dir.files(),
        ["job.json", "progress.ndjson", "result.json"],
        "{id}"
    );
}

/// `rewriteExample` (scripts/lib/parity/corpus.ts): the example's paths under /jobs/3/ moved into
/// `dir`, its resources the engine's (here a folder in `dir`), its presets the generic set's files.
fn rewrite_example(example: &Value, dir: &Scratch) -> Object {
    const FOLDER: &str = "/jobs/3/";
    let moved = |v: &Value| -> Value {
        match v.as_str().and_then(|s| s.strip_prefix(FOLDER)) {
            Some(rest) => Value::from(dir.path(rest)),
            None => v.clone(),
        }
    };
    let mut out = example.as_object().unwrap().clone();
    if out.get("resources").and_then(Value::as_str) == Some("/opt/sketch-orca/resources") {
        out.insert("resources", dir.path("resources"));
    }
    if let Some(w) = out.get("workDir").cloned() {
        out.insert("workDir", moved(&w));
    }
    if let Some(Value::Object(m)) = out.get("model").cloned() {
        let mut m = m.clone();
        if let Some(p) = m.get("path").cloned() {
            m.insert("path", moved(&p));
        }
        out.insert("model", m);
    }
    if let Some(Value::Object(p)) = out.get("presets").cloned() {
        let mut p = p.clone();
        for (key, file) in [
            ("machine", "presets/machine.json"),
            ("process", "presets/process.json"),
        ] {
            if p.get(key)
                .and_then(Value::as_str)
                .is_some_and(|s| s.starts_with(FOLDER))
            {
                p.insert(key, dir.path(file));
            }
        }
        if let Some(Value::Array(f)) = p.get("filaments").cloned() {
            let f: Vec<Value> = f
                .iter()
                .map(|x| match x.as_str() {
                    Some(s) if s.starts_with(FOLDER) => {
                        Value::from(dir.path("presets/filament_1.json"))
                    }
                    _ => x.clone(),
                })
                .collect();
            p.insert("filaments", Value::Array(f));
        }
        out.insert("presets", p);
    }
    if let Some(Value::Object(o)) = out.get("output").cloned() {
        let mut o = o.clone();
        for key in ["gcode", "bambu3mf"] {
            if let Some(v) = o.get(key).cloned() {
                o.insert(key, moved(&v));
            }
        }
        out.insert("output", o);
    }
    out
}

#[test]
fn the_invalid_examples_get_the_cpp_programs_result() {
    let dir = repo().join("protocol/examples/invalid");
    let names: std::collections::BTreeSet<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("job-") && n.ends_with(".json"))
        .collect();
    assert_eq!(names.len(), 7, "{names:?}");
    for name in names {
        let id = format!("example-{}", &name[4..name.len() - 5]);
        let scratch = Scratch::new("intake", &id);
        let example = parse(&std::fs::read(dir.join(&name)).unwrap(), Order::Insertion).unwrap();
        let job = write_job(&scratch, &rewrite_example(&example, &scratch));
        check_against_golden(&id, &scratch, &job);
    }
}

#[test]
fn the_refusals_of_the_job_itself_get_the_cpp_programs_result() {
    type Change = fn(&mut Object, &Scratch);
    let cases: [(&str, Change); 4] = [
        ("refusal-preset-file-key", |job, _| {
            let mut o = Object::new(Order::Insertion);
            o.insert("Inherits", "fdm_process_common");
            job.insert("overrides", o);
        }),
        ("refusal-two-at-one-print-z", |job, _| {
            let pause = |z: f64| {
                let mut g = Object::new(Order::Insertion);
                g.insert("printZ", z);
                g.insert("type", "pause");
                Value::Object(g)
            };
            job.insert("customGcodes", Value::Array(vec![pause(5.05), pause(5.05)]));
        }),
        ("refusal-profile-not-at-zero", |job, _| {
            let mut lh = Object::new(Order::Insertion);
            lh.insert("mode", "profile");
            lh.insert("profile", vec![0.2, 0.2, 10.0, 0.2]);
            let mut o = Object::new(Order::Insertion);
            o.insert("index", 0);
            o.insert("layerHeights", lh);
            job.insert("objects", Value::Array(vec![Value::Object(o)]));
        }),
        ("refusal-drive-rooted-path", |job, _| {
            job.insert("workDir", "\\sketch-smoke-work");
        }),
    ];
    for (id, change) in cases {
        let scratch = Scratch::new("intake", id);
        let mut job = valid_job(&scratch, "slice");
        change(&mut job, &scratch);
        let path = write_job(&scratch, &job);
        check_against_golden(id, &scratch, &path);
    }
}

/// The cube's overrides (smoke.ts cubeOverrides), which every smoke refusal's job has.
const CUBE: [(&str, &str); 7] = [
    ("brim_type", "no_brim"),
    ("skirt_loops", "0"),
    ("enable_support", "0"),
    ("enable_arc_fitting", "0"),
    ("precise_z_height", "0"),
    ("layer_height", "0.2"),
    ("initial_layer_print_height", "0.25"),
];

#[test]
fn the_refusals_of_the_presets_stage_and_the_models_file_get_the_cpp_programs_result() {
    // smoke.ts REFUSALS: the cube's overrides (cubeOverrides), then the one refused, on presets
    // that load (common::write_fixtures; the goldens' came from the generic set, and neither
    // gives a warning).
    let presets_stage: &[&str] = &["job", "presets"];
    let model_stage: &[&str] = &["job", "presets", "model"];
    for (id, refused, stages) in [
        (
            "refusal-unknown-setting",
            Some(("sketch_smoke_no_such_setting", "1")),
            presets_stage,
        ),
        (
            "refusal-unknown-capitalised-setting",
            Some(("Layer_Height", "0.2")),
            presets_stage,
        ),
        (
            "refusal-renamed-key-dropped-value",
            Some(("top_one_wall_type", "none")),
            presets_stage,
        ),
        ("refusal-missing-model", None, model_stage),
    ] {
        let scratch = Scratch::new("intake", id);
        write_fixtures(&scratch);
        let mut job = valid_job(&scratch, "slice");
        let mut overrides = Object::new(Order::Insertion);
        for (k, v) in CUBE.iter().chain(refused.iter()) {
            overrides.insert(*k, *v);
        }
        job.insert("overrides", overrides);
        if refused.is_none() {
            let mut model = Object::new(Order::Insertion);
            model.insert("path", scratch.path("missing.stl"));
            job.insert("model", model);
        }
        let path = write_job(&scratch, &job);
        let want = golden_in(id, &scratch);
        let w = parse(want.as_bytes(), Order::Insertion).unwrap();
        let (code, result, progress) = run(&scratch, &path);
        assert_eq!(as_cpp_result(&result), want, "{id}");
        assert_eq!(
            i64::from(code),
            w["exitCode"].as_f64().unwrap() as i64,
            "{id}"
        );
        assert_eq!(
            progress,
            progress_text(stages, Some(w["message"].as_str().unwrap())),
            "{id}"
        );
        assert_eq!(
            scratch.files(),
            fixture_files(&["progress.ndjson", "result.json"]),
            "{id}"
        );
    }
}

/// The names of smoke.ts's generic preset set (`SMOKE_PRESETS.generic`), which the goldens of the
/// presets stage's refusals name.
const GENERIC_MACHINE: &str = "Voron 2.4 300 0.4 nozzle";
const GENERIC_PROCESS: &str = "0.20mm Standard @Voron";
const GENERIC_FILAMENT: &str = "Generic PLA @System";

/// smoke.ts `PRESET_REFUSE`: a preset file that is not JSON and one that is not an object, and the
/// printer no preset of the goldens is made for.
const NOT_JSON: &str = "{\"type\": \"process\",";
const NOT_OBJECT: &str = "[1, 2]";
const OTHER_PRINTER: &str = "Another printer";

/// The files a refusal of the presets stage starts from: [`common::write_fixtures`]'s, its presets
/// named as the generic set's.
fn write_generic_fixtures(dir: &Scratch) {
    std::fs::create_dir_all(dir.path("resources/profiles")).unwrap();
    write_named_preset(dir, "machine", "machine", GENERIC_MACHINE, &[]);
    write_named_preset(dir, "process", "process", GENERIC_PROCESS, &[]);
    write_named_preset(dir, "filament_1", "filament", GENERIC_FILAMENT, &[]);
    std::fs::write(dir.path("model.stl"), b"solid").unwrap();
}

/// What the refusal `id` changes in the files of [`write_generic_fixtures`], the overrides or the
/// job, as smoke.ts `REFUSALS` changes the generic set's (each preset file it changes is written
/// in `presets/`, as there).
fn change_for(id: &str, dir: &Scratch, overrides: &mut Object, job: &mut Object) {
    let process = |settings: &[(&str, Value)]| {
        write_named_preset(dir, "process", "process", GENERIC_PROCESS, settings);
    };
    match id {
        "refusal-missing-preset" => std::fs::remove_file(dir.path("presets/machine.json")).unwrap(),
        "refusal-preset-not-json" => {
            std::fs::write(dir.path("presets/process.json"), NOT_JSON).unwrap()
        }
        "refusal-preset-not-object" => {
            std::fs::write(dir.path("presets/process.json"), NOT_OBJECT).unwrap();
        }
        // The process preset in the machine's place.
        "refusal-preset-wrong-type" => {
            write_named_preset(dir, "machine", "process", GENERIC_PROCESS, &[]);
        }
        "refusal-preset-bad-from" => process(&[("from", Value::from("elsewhere"))]),
        "refusal-preset-unreadable-value" => process(&[("layer_height", Value::from("abc"))]),
        "refusal-not-fdm" => {
            let sla = [("printer_technology", Value::from("SLA"))];
            write_named_preset(dir, "machine", "machine", GENERIC_MACHINE, &sla);
        }
        "refusal-process-not-compatible" => {
            process(&[("compatible_printers", Value::from(vec![OTHER_PRINTER]))]);
        }
        "refusal-setting-bad-value" => overrides.insert("sparse_infill_pattern", "zzz"),
        "refusal-flush-volume-past-int" => overrides.insert("nozzle_volume", "1e100"),
        "refusal-settings-orca-refuses" => overrides.insert("layer_height", "0"),
        "refusal-resources-without-profiles" => {
            std::fs::remove_dir(dir.path("resources/profiles")).unwrap();
        }
        // The work folder's orca-data folder has the name of a file.
        "refusal-work-folder-unmakeable" => {
            std::fs::create_dir_all(dir.path("work")).unwrap();
            std::fs::write(dir.path("work/orca-data"), "a file, not a folder\n").unwrap();
        }
        // A missing model on presets that give the stage's warnings.
        "refusal-preset-warnings" => {
            let mut speed = Object::new(Order::Insertion);
            speed.insert("value", 30);
            process(&[
                ("ironing_speed", Value::Object(speed)),
                (
                    "wall_loops",
                    Value::Array(vec![Value::from(vec![2]), Value::from(3)]),
                ),
                ("sparse_infill_pattern", Value::from("nopepattern")),
            ]);
            let other = [("compatible_printers", Value::from(vec![OTHER_PRINTER]))];
            write_named_preset(dir, "filament_1", "filament", GENERIC_FILAMENT, &other);
            let mut model = Object::new(Order::Insertion);
            model.insert("path", dir.path("missing.stl"));
            job.insert("model", model);
        }
        other => panic!("no change for {other}"),
    }
}

/// The golden of corpus case `id`, the case's folder `<case>` written as `dir`'s, as JSON text.
fn golden_in(id: &str, dir: &Scratch) -> String {
    let folder = Value::from(dir.0.to_string_lossy().into_owned()).dump(None);
    golden(id).replace("<case>", &folder[1..folder.len() - 1])
}

#[test]
fn the_refusals_and_warnings_of_the_presets_stage_and_its_folders_get_the_cpp_programs_result() {
    // smoke.ts REFUSALS made past reading the job and before the model is read (RUST_OWN_REFUSALS
    // but the four above), each the cube's job with one thing changed, here on presets that have
    // the generic set's names and Orca's defaults (the generic set's own values reach none of
    // these results). The run writes only its result and progress file.
    let presets_stage: &[&str] = &["job", "presets"];
    let model_stage: &[&str] = &["job", "presets", "model"];
    for (id, stages) in [
        ("refusal-missing-preset", presets_stage),
        ("refusal-preset-not-json", presets_stage),
        ("refusal-preset-not-object", presets_stage),
        ("refusal-preset-wrong-type", presets_stage),
        ("refusal-preset-bad-from", presets_stage),
        ("refusal-preset-unreadable-value", presets_stage),
        ("refusal-not-fdm", presets_stage),
        ("refusal-process-not-compatible", presets_stage),
        ("refusal-setting-bad-value", presets_stage),
        ("refusal-flush-volume-past-int", presets_stage),
        ("refusal-settings-orca-refuses", presets_stage),
        ("refusal-resources-without-profiles", presets_stage),
        ("refusal-work-folder-unmakeable", presets_stage),
        ("refusal-preset-warnings", model_stage),
    ] {
        let scratch = Scratch::new("intake", id);
        write_generic_fixtures(&scratch);
        let mut job = valid_job(&scratch, "slice");
        let mut overrides = Object::new(Order::Insertion);
        for (k, v) in CUBE {
            overrides.insert(k, v);
        }
        change_for(id, &scratch, &mut overrides, &mut job);
        job.insert("overrides", overrides);
        let path = write_job(&scratch, &job);
        let mut files = scratch.files();
        let want = golden_in(id, &scratch);
        let w = parse(want.as_bytes(), Order::Insertion).unwrap();
        let (code, result, progress) = run(&scratch, &path);
        assert_eq!(as_cpp_result(&result), want, "{id}");
        assert_eq!(
            i64::from(code),
            w["exitCode"].as_f64().unwrap() as i64,
            "{id}"
        );
        assert_eq!(
            progress,
            progress_text(stages, Some(w["message"].as_str().unwrap())),
            "{id}"
        );
        files.extend(["progress.ndjson".to_string(), "result.json".to_string()]);
        orca_stl::sort(&mut files);
        assert_eq!(scratch.files(), files, "{id}");
    }

    // The stage's warnings in their order: each preset file's skipped values in key order, its
    // substitutions, then the filaments made for another printer. unknown-setting never comes:
    // Orca's handle_legacy drops a key it does not have before the value is set.
    let w = parse(
        golden("refusal-preset-warnings").as_bytes(),
        Order::Insertion,
    )
    .unwrap();
    let codes: Vec<&str> = w["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["code"].as_str().unwrap())
        .collect();
    assert_eq!(
        codes,
        [
            "preset-value-skipped",
            "preset-value-skipped",
            "setting-substituted",
            "filament-not-compatible"
        ]
    );
}

#[test]
fn every_intake_refusal_of_the_corpus_is_held_to_its_golden_here() {
    // The corpus's refusal cases whose only family is intake (smoke.ts REFUSALS, in their order):
    // the C++ refuses them up to the model's file, where this program must give the same result.
    // Each is a case of one of the tests above: the job itself (preset-file-key, two-at-one-print-z,
    // profile-not-at-zero, drive-rooted-path), the overrides and the model's file (missing-model,
    // the unknown settings, renamed-key-dropped-value), or the presets stage and its folders (the
    // rest). A refusal added to the corpus fails here until it has its test.
    let corpus = std::fs::read(repo().join("parity/corpus.json")).unwrap();
    let corpus = parse(&corpus, Order::Insertion).unwrap();
    let intake: Vec<&str> = corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| {
            c["kind"].as_str() == Some("refusal")
                && c["families"].as_array().map(Vec::as_slice) == Some(&[Value::from("intake")][..])
        })
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        intake,
        [
            "refusal-missing-model",
            "refusal-unknown-setting",
            "refusal-unknown-capitalised-setting",
            "refusal-preset-file-key",
            "refusal-renamed-key-dropped-value",
            "refusal-two-at-one-print-z",
            "refusal-profile-not-at-zero",
            "refusal-drive-rooted-path",
            "refusal-missing-preset",
            "refusal-preset-not-json",
            "refusal-preset-not-object",
            "refusal-preset-wrong-type",
            "refusal-preset-bad-from",
            "refusal-preset-unreadable-value",
            "refusal-not-fdm",
            "refusal-process-not-compatible",
            "refusal-setting-bad-value",
            "refusal-flush-volume-past-int",
            "refusal-settings-orca-refuses",
            "refusal-resources-without-profiles",
            "refusal-work-folder-unmakeable",
            "refusal-preset-warnings",
        ]
    );
    for id in intake {
        assert!(
            repo()
                .join(format!("parity/goldens/front/intake/{id}.result.json"))
                .is_file(),
            "{id}"
        );
    }
}

#[test]
fn a_job_that_cannot_be_read_is_file_not_found() {
    let scratch = Scratch::new("intake", "unreadable");
    for job in [scratch.path("none.json"), scratch.path("")] {
        let (code, result, progress) = run(&scratch, &job);
        let message = format!("cannot read the job {job}");
        let r = parse(result.as_bytes(), Order::Insertion).unwrap();
        assert_eq!(code, -3);
        assert_eq!(r["code"].as_str(), Some("file-not-found"));
        assert_eq!(r["message"].as_str(), Some(message.as_str()));
        assert!(r.get("command").is_none());
        assert_eq!(progress, failed_at_intake_progress(&message));
    }
}

#[test]
fn a_result_that_would_replace_a_file_of_the_run_is_not_written() {
    let scratch = Scratch::new("intake", "result-on-model");
    std::fs::write(scratch.path("model.stl"), b"solid").unwrap();
    let job = write_job(&scratch, &valid_job(&scratch, "slice"));
    let model = scratch.path("model.stl");
    let r = program(
        &[
            "run",
            "--job",
            &job,
            "--out",
            &model,
            "--progress",
            &scratch.path("progress.ndjson"),
        ],
        &[],
    );
    assert_eq!(r.status.code(), Some(-2));
    assert_eq!(
        String::from_utf8(r.stderr).unwrap(),
        stream_text(&format!(
            "sketch-orca: the result file {model} would replace model.path {model}; nothing is written\n"
        ))
    );
    assert_eq!(std::fs::read(&model).unwrap(), b"solid");
    assert_eq!(scratch.files(), ["job.json", "model.stl"]);

    // The result's own temporary file on the job.
    let tmp = scratch.path("result.json.tmp");
    std::fs::copy(&job, &tmp).unwrap();
    let r = program(
        &["run", "--job", &tmp, "--out", &scratch.path("result.json")],
        &[],
    );
    assert_eq!(r.status.code(), Some(-2));
    assert!(String::from_utf8(r.stderr).unwrap().contains(&format!(
        "the result file {tmp} would replace the job {tmp}; nothing is written"
    )));
}

#[test]
fn a_progress_file_on_a_file_of_the_run_is_refused() {
    let scratch = Scratch::new("intake", "progress-on-model");
    std::fs::write(scratch.path("model.stl"), b"solid").unwrap();
    let job = write_job(&scratch, &valid_job(&scratch, "slice"));
    let model = scratch.path("model.stl");
    for progress in [
        model.clone(),
        format!("{model}::$DATA"),
        format!("{model}:progress"),
    ] {
        let out = scratch.path("result.json");
        let r = program(
            &["run", "--job", &job, "--out", &out, "--progress", &progress],
            &[],
        );
        assert_eq!(r.status.code(), Some(-2));
        let result = parse(&std::fs::read(&out).unwrap(), Order::Insertion).unwrap();
        assert_eq!(result["code"].as_str(), Some("invalid-params"));
        assert_eq!(
            result["message"].as_str().unwrap(),
            format!("the progress file (--progress) {progress} would replace model.path {model}")
        );
        assert_eq!(std::fs::read(&model).unwrap(), b"solid");
    }
}

#[test]
fn outputs_must_be_files_of_their_own() {
    let scratch = Scratch::new("intake", "outputs");
    // The G-code export writes through <gcode>.tmp, which an override names here.
    let mut job = valid_job(&scratch, "slice");
    let mut o = Object::new(Order::Insertion);
    o.insert("post_process", scratch.path("out.gcode.tmp"));
    job.insert("overrides", o);
    let path = write_job(&scratch, &job);
    let out = scratch.path("result.json");
    let r = program(&["run", "--job", &path, "--out", &out], &[]);
    assert_eq!(r.status.code(), Some(-2));
    let result = parse(&std::fs::read(&out).unwrap(), Order::Insertion).unwrap();
    assert_eq!(result["code"].as_str(), Some("job-invalid"));
    assert_eq!(result["command"].as_str(), Some("slice"));
    let gcode = scratch.path("out.gcode");
    assert_eq!(
        result["message"].as_str().unwrap(),
        format!(
            "output.gcode {gcode} would replace overrides.post_process {gcode}.tmp ({gcode}.tmp)"
        )
    );

    // The result on the G-code: invalid-params, written where it was asked for.
    let path = write_job(&scratch, &valid_job(&scratch, "slice"));
    let r = program(&["run", "--job", &path, "--out", &format!("{gcode}.")], &[]);
    assert_eq!(r.status.code(), Some(-2));
    let result = parse(&std::fs::read(&gcode).unwrap(), Order::Insertion).unwrap();
    assert_eq!(result["code"].as_str(), Some("invalid-params"));
    assert_eq!(
        result["message"].as_str().unwrap(),
        format!("the result (--out) {gcode}. and output.gcode {gcode} are one file ({gcode}.)")
    );
}

#[test]
fn a_command_line_it_does_not_take_still_gets_its_result() {
    let scratch = Scratch::new("intake", "command-line");
    let job = write_job(&scratch, &valid_job(&scratch, "slice"));
    let out = scratch.path("result.json");
    let r = program(
        &["run", "--job", &job, "--out", &out, "--colour", "red"],
        &[],
    );
    assert_eq!(r.status.code(), Some(-2));
    assert!(
        String::from_utf8(r.stderr)
            .unwrap()
            .starts_with(&stream_text(
                "sketch-orca: unknown option --colour\n\nsketch-orca: a command-line slicer"
            ))
    );
    let text = std::fs::read_to_string(&out).unwrap();
    let result = parse(text.as_bytes(), Order::Insertion).unwrap();
    assert_eq!(result["code"].as_str(), Some("invalid-params"));
    assert_eq!(result["message"].as_str(), Some("unknown option --colour"));
    assert_eq!(result["exitCode"].as_f64(), Some(-2.0));
    assert!(result.get("command").is_none());

    // Unless that result would replace a file the command line or its job names.
    std::fs::write(scratch.path("model.stl"), b"solid").unwrap();
    let model = scratch.path("model.stl");
    let r = program(
        &["run", "--job", &job, "--out", &model, "--colour", "red"],
        &[],
    );
    assert_eq!(r.status.code(), Some(-2));
    let err = String::from_utf8(r.stderr).unwrap();
    assert!(err.ends_with(&stream_text(&format!(
        "sketch-orca: the result file {model} would replace model.path {model}; nothing is written\n"
    ))), "{err}");
    assert_eq!(std::fs::read(&model).unwrap(), b"solid");
}
