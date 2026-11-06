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
//! folder as the parity corpus moves them, and the smoke refusals the C++ refuses while reading
//! the job get the same result, byte for byte (the engine block apart: this program's version and
//! `implementation`), the same exit code and the same progress file. The run's files are kept
//! apart as the C++ keeps them: a result that would replace a file of the run is not written, a
//! progress file that would is refused, and a job whose outputs would replace an input is
//! refused; a command line it does not take still gets its result.

mod common;

use common::{
    Scratch, as_cpp_result, failed_at_intake_progress, program, repo, stream_text, valid_job,
    write_job,
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
