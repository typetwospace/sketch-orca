// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The model stage and the info answer (`run.rs`, `model.rs`, `info.rs`), held to the C++
//! program's bytes.
//!
//! - Three corpus info jobs, the 20 mm cube as a binary STL (mesh-20mm-cube-info), Orca's
//!   20mm_cube.obj read as OBJ (mesh-obj-20mm-cube-info) and Orca's Büchse.3mf
//!   (mesh-buchse-3mf-info), and the lone triangle smoke.ts's rs-info runs as an info job: the
//!   result, the progress file, stdout and stderr are the C++ program's. Its bytes are in
//!   tests/info/cases/<case>/: the corpus's goldens (the recording of 2026-11-06) for the STL and
//!   the 3MF, one run of the staged C++ program (2026-11-11, orca-info-load) for the OBJ and the
//!   lone triangle. The jobs are the corpus's plainJob on its generic preset set
//!   (tests/info/presets, its three files verbatim) with a resources folder that holds what the
//!   presets stage reads there for that set (tests/info/resources: the Voron 2.4 300 printer
//!   model's file verbatim, and its vendor bundle reduced to that model). The 3MF is kept under an
//!   ASCII name and copied to the case's own name, Büchse.3mf. After its run the job's work folder
//!   holds the 3MF reader's backup folder, orcaslicer_model: run.rs makes the work folder Orca's
//!   temporary folder (`set_temporary_dir`), without which the backup goes to the drive's root.
//! - The cube naming a material library that is not there (model-obj-mtl-missing-info), whose
//!   result and progress are the 0.3.0 record of the staged C++ program (2026-11-12) in
//!   tests/info/cases/model-obj-mtl-missing-info: the reader logs the missing library as the C++
//!   one does and reads past it, so stderr holds that one line, naming the test's own folder. As a
//!   layers job the same model is read and logged the same way, then declined with -8.
//! - A model path the job writes with `/`: the reader gets it made preferred (`\` on Windows), so
//!   the OBJ cube is still named 20mm_cube.obj (its case's bytes), and an empty STL's result keeps
//!   the job's path while admesh's log line names the preferred one. One run of the staged C++
//!   program on both jobs (2026-11-11, orca-info-load's review fix) gave the same.
//! - The model stage's refusals: a flat model and a lone triangle in a slice job (the C++ goldens
//!   refusal-flat-model and refusal-lone-triangle of parity/goldens/front/intake, and the flat
//!   part's log line from the staged run), an object the job lists past the model's, an empty file
//!   and a binary STL of no triangles (the C++ program's result, progress and log line, from the
//!   same staged run; the log line goes to stderr at the stock level and not at level 0), and the
//!   CR box, where the C++ engine dies (CORPUS.md, "An engine fault"), refused as
//!   model-unreadable.

mod common;

use std::path::{Path, PathBuf};

use common::{
    Scratch, as_cpp_result, as_recorded_result, declined_progress, program, repo, stream_text,
    valid_job, write_fixtures, write_job,
};
use orca_json::{Object, Order, Value, parse};

/// tests/info: the cases' bytes, the generic presets and the resources.
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("info")
}

/// The corpus's plainJob for `command` on the model `model` in `dir`: the generic presets, the
/// fixtures' resources, the work folder in `dir`, nothing overridden.
fn corpus_job(dir: &Scratch, command: &str, model: &str) -> Object {
    corpus_job_at(dir, command, &dir.path(model))
}

/// [`corpus_job`] on the model at `model_path`, written in the job as it is given.
fn corpus_job_at(dir: &Scratch, command: &str, model_path: &str) -> Object {
    let path = |p: PathBuf| p.to_string_lossy().into_owned();
    let presets_dir = fixtures().join("presets");
    let mut presets = Object::new(Order::Insertion);
    presets.insert("machine", path(presets_dir.join("machine.json")));
    presets.insert("process", path(presets_dir.join("process.json")));
    presets.insert("filaments", vec![path(presets_dir.join("filament_1.json"))]);
    let mut m = Object::new(Order::Insertion);
    m.insert("path", model_path);
    let mut output = Object::new(Order::Insertion);
    output.insert("gcode", "");
    output.insert("bambu3mf", Value::Null);
    output.insert("stats", true);
    let mut job = Object::new(Order::Insertion);
    job.insert("format", "sketch-orca-job/1");
    job.insert("command", command);
    job.insert("resources", path(fixtures().join("resources")));
    job.insert("workDir", dir.path("work"));
    job.insert("model", m);
    job.insert("presets", presets);
    job.insert("overrides", Object::new(Order::Insertion));
    job.insert("objects", Value::Array(Vec::new()));
    job.insert("customGcodes", Value::Array(Vec::new()));
    job.insert("arrange", true);
    job.insert("orient", false);
    job.insert("output", output);
    job
}

/// What a run gave: its exit code, result, progress file, stdout and stderr.
struct Ran {
    code: i32,
    result: String,
    progress: String,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

/// Runs `job` from `dir` with --out and --progress and the variables of `env`.
fn run(dir: &Scratch, job: &Object, env: &[(&str, &str)]) -> Ran {
    let path = write_job(dir, job);
    let (out, progress) = (dir.path("result.json"), dir.path("progress.ndjson"));
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
    Ran {
        code: r.status.code().unwrap(),
        result: std::fs::read_to_string(&out).unwrap(),
        progress: std::fs::read_to_string(&progress).unwrap(),
        stdout: r.stdout,
        stderr: r.stderr,
    }
}

/// Runs the info case `id` (tests/info/cases/<id>) on its model `file`, copied into the case's
/// folder as `name`, and holds it to the C++ program's result and progress file, with nothing on
/// stdout or stderr.
fn check_info_case(id: &str, file: &str, name: &str) {
    check_info_case_with(id, file, name, str::to_owned);
}

/// [`check_info_case`], the job naming the model by `as_job_writes_it` of its path. Returns the
/// case's scratch folder, for what the run left in its work folder.
fn check_info_case_with(
    id: &str,
    file: &str,
    name: &str,
    as_job_writes_it: fn(&str) -> String,
) -> Scratch {
    let scratch = Scratch::new("info", id);
    let case = fixtures().join("cases").join(id);
    std::fs::copy(case.join(file), scratch.path(name)).unwrap();
    let model = as_job_writes_it(&scratch.path(name));
    let ran = run(&scratch, &corpus_job_at(&scratch, "info", &model), &[]);
    assert_eq!(ran.code, 0, "{id}: {}", ran.result);
    assert!(
        ran.stdout.is_empty() && ran.stderr.is_empty(),
        "{id}: {}{}",
        String::from_utf8_lossy(&ran.stdout),
        String::from_utf8_lossy(&ran.stderr)
    );
    let want = std::fs::read_to_string(case.join("result.json")).unwrap();
    assert_eq!(as_cpp_result(&ran.result), want, "{id}");
    let want = std::fs::read_to_string(case.join("progress.ndjson")).unwrap();
    assert_eq!(ran.progress, want, "{id}");
    scratch
}

#[test]
fn an_info_job_on_a_binary_stl_is_answered_as_the_cpp_program_answers_it() {
    check_info_case("mesh-20mm-cube-info", "model.stl", "model.stl");
}

#[test]
fn an_info_job_on_an_obj_file_is_answered_as_the_cpp_program_answers_it() {
    check_info_case("mesh-obj-20mm-cube-info", "20mm_cube.obj", "20mm_cube.obj");
}

#[test]
fn an_info_job_on_a_3mf_file_is_answered_as_the_cpp_program_answers_it() {
    check_info_case("mesh-buchse-3mf-info", "buchse.3mf", "B\u{fc}chse.3mf");
}

#[test]
fn the_3mf_readers_backup_folder_is_made_in_the_jobs_work_folder() {
    let scratch = check_info_case_with(
        "mesh-buchse-3mf-info",
        "buchse.3mf",
        "B\u{fc}chse.3mf",
        str::to_owned,
    );
    // The Büchse case again: the work folder is Orca's temporary folder (run.rs,
    // set_temporary_dir), so the 3MF reader's backup folder is made there, not at the drive's root.
    assert!(
        Path::new(&scratch.path("work/orcaslicer_model")).is_dir(),
        "the 3MF reader's backup folder is in the job's work folder"
    );
}

#[test]
fn an_info_job_answers_for_a_lone_triangle_which_it_does_not_check_for_a_volume() {
    check_info_case("lone-triangle-info", "model.obj", "model.obj");
}

#[test]
fn an_obj_files_missing_material_library_is_logged_and_read_past_as_the_cpp_program_does_it() {
    // The C++ reader logs a library that is not there at error and goes on (OBJ.cpp:59): the info
    // job answers with the staged C++ program's result and progress (the 0.3.0 record,
    // 2026-11-12) and its one stderr line, which names the folder of the path the reader got
    // (`\` on Windows), then `/missing.mtl`.
    let id = "model-obj-mtl-missing-info";
    let case = fixtures().join("cases").join(id);
    let scratch = Scratch::new("info", id);
    std::fs::copy(case.join("unpainted.obj"), scratch.path("unpainted.obj")).unwrap();
    let model = scratch.path("unpainted.obj");
    let folder = Path::new(&model)
        .parent()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let log = stream_text(&format!(
        "[error] load_obj: failed to load mtl_path:{folder}/missing.mtl\n"
    ));
    let ran = run(
        &scratch,
        &corpus_job(&scratch, "info", "unpainted.obj"),
        &[],
    );
    assert_eq!(ran.code, 0, "{id}: {}", ran.result);
    assert!(ran.stdout.is_empty(), "{id}");
    assert_eq!(String::from_utf8_lossy(&ran.stderr), log, "{id}");
    let want = std::fs::read_to_string(case.join("result.json")).unwrap();
    assert_eq!(as_recorded_result(&ran.result, &want), want, "{id}");
    let r = parse(ran.result.as_bytes(), Order::Insertion).unwrap();
    assert_eq!(
        r["info"]["objects"].as_array().unwrap()[0]["name"].as_str(),
        Some("unpainted.obj")
    );
    let want = std::fs::read_to_string(case.join("progress.ndjson")).unwrap();
    assert_eq!(ran.progress, want, "{id}");
    // The same model as a layers job: read and logged the same way, then declined past the model
    // stage, as slicing is not ported.
    let ran = run(
        &scratch,
        &corpus_job(&scratch, "layers", "unpainted.obj"),
        &[],
    );
    let r = parse(ran.result.as_bytes(), Order::Insertion).unwrap();
    assert_eq!(
        (ran.code, r["code"].as_str()),
        (-8, Some("not-ported")),
        "{id} as layers: {}",
        ran.result
    );
    assert!(
        ran.progress
            .contains(r#"{"stage":"model","percent":8.0,"message":"reading the model"}"#),
        "{id} as layers: {}",
        ran.progress
    );
    assert!(ran.stdout.is_empty(), "{id} as layers");
    assert_eq!(String::from_utf8_lossy(&ran.stderr), log, "{id} as layers");
}

/// `path` with each `\` written `/`, as a job may write a Windows path.
fn forward(path: &str) -> String {
    path.replace('\\', "/")
}

#[test]
fn a_model_path_with_forward_slashes_is_made_preferred_for_the_reader_alone() {
    // The reader gets the path made preferred (`\` on Windows), so the OBJ cube is named after the
    // text behind the last `\`, "20mm_cube.obj", not after the whole path: the staged C++
    // program's result and progress for this job are its case's bytes (one run, 2026-11-11).
    check_info_case_with(
        "mesh-obj-20mm-cube-info",
        "20mm_cube.obj",
        "20mm_cube.obj",
        forward,
    );
    // An empty STL: the result keeps the path as the job wrote it, with `/`, and admesh's log line
    // names the path the reader got, with `\` on Windows, as in the same run of the C++ program.
    let scratch = Scratch::new("info", "empty-model-file-forward");
    std::fs::write(scratch.path("model.stl"), b"").unwrap();
    let model = forward(&scratch.path("model.stl"));
    let preferred = if cfg!(windows) {
        model.replace('/', "\\")
    } else {
        model.clone()
    };
    let ran = run(&scratch, &corpus_job_at(&scratch, "info", &model), &[]);
    let r = parse(ran.result.as_bytes(), Order::Insertion).unwrap();
    let message = format!("cannot read the model {model}: Loading of a model file failed.");
    assert_eq!(
        (ran.code, r["code"].as_str(), r["message"].as_str()),
        (-6, Some("model-unreadable"), Some(message.as_str()))
    );
    assert_eq!(ran.progress, declined_progress(&message));
    assert!(ran.stdout.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&ran.stderr),
        stream_text(&format!(
            "[error] stl_open_count_facets: The input is an empty file: {preferred}\n"
        ))
    );
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

/// A smoke refusal's slice job (common::valid_job with the cube's overrides) on the model `name`
/// with `bytes`.
fn refused_slice(id: &str, name: &str, bytes: &[u8]) -> (Scratch, Ran) {
    let scratch = Scratch::new("info", id);
    write_fixtures(&scratch);
    std::fs::write(scratch.path(name), bytes).unwrap();
    let mut job = valid_job(&scratch, "slice");
    let mut overrides = Object::new(Order::Insertion);
    for (k, v) in CUBE {
        overrides.insert(k, v);
    }
    job.insert("overrides", overrides);
    let mut model = Object::new(Order::Insertion);
    model.insert("path", scratch.path(name));
    job.insert("model", model);
    let ran = run(&scratch, &job, &[]);
    (scratch, ran)
}

#[test]
fn a_flat_model_and_a_lone_triangle_are_refused_as_the_cpp_program_refuses_them() {
    let flat = common::box_stl([0.0; 3], [20.0, 20.0, 0.0], "flat");
    let triangle = b"v 0 0 0\nv 20 0 0\nv 0 0 20\nf 1 2 3\n";
    // The flat part's convex hull fails as the model is read, which Orca logs as an error (the
    // staged C++ program's stderr for this job, 2026-11-11); a part of one triangle gets no hull.
    let hull = stream_text("[error] its_convex_hull: Unable to create convex hull\n");
    for (id, name, bytes, stderr) in [
        (
            "refusal-flat-model",
            "model.stl",
            flat.as_slice(),
            hull.as_str(),
        ),
        (
            "refusal-lone-triangle",
            "model.obj",
            triangle.as_slice(),
            "",
        ),
    ] {
        let (_scratch, ran) = refused_slice(id, name, bytes);
        let want = std::fs::read_to_string(
            repo().join(format!("parity/goldens/front/intake/{id}.result.json")),
        )
        .unwrap();
        assert_eq!(as_cpp_result(&ran.result), want, "{id}");
        assert_eq!(ran.code, -6, "{id}");
        let message = parse(want.as_bytes(), Order::Insertion).unwrap()["message"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(ran.progress, declined_progress(&message), "{id}");
        assert!(ran.stdout.is_empty(), "{id}");
        assert_eq!(String::from_utf8_lossy(&ran.stderr), stderr, "{id}");
    }
}

#[test]
fn an_object_the_job_lists_past_the_models_is_refused_after_the_model_is_read() {
    let scratch = Scratch::new("info", "index-past-model");
    write_fixtures(&scratch);
    let mut job = valid_job(&scratch, "layers");
    let mut object = Object::new(Order::Insertion);
    object.insert("index", 1);
    job.insert("objects", vec![Value::Object(object)]);
    let ran = run(&scratch, &job, &[]);
    let r = parse(ran.result.as_bytes(), Order::Insertion).unwrap();
    let message = "objects[0].index is 1, but the model has 1 object(s)";
    assert_eq!(
        (ran.code, r["code"].as_str(), r["message"].as_str()),
        (-2, Some("job-invalid"), Some(message))
    );
    assert!(r.get("objectIndex").is_none());
    assert_eq!(ran.progress, declined_progress(message));
}

#[test]
fn a_file_of_no_triangles_is_refused_and_logged_as_the_cpp_program_does_it() {
    let no_triangles = {
        let mut b = b"sketch-orca smoke model: no triangles".to_vec();
        b.resize(80, b' ');
        b.extend_from_slice(&0u32.to_le_bytes());
        b
    };
    for (id, bytes) in [
        ("empty-model-file", Vec::new()),
        ("stl-no-triangles", no_triangles),
    ] {
        let scratch = Scratch::new("info", id);
        let model = scratch.path("model.stl");
        std::fs::write(&model, &bytes).unwrap();
        let job = corpus_job(&scratch, "info", "model.stl");
        let message = format!("cannot read the model {model}: Loading of a model file failed.");
        let log = stream_text(&format!(
            "[error] stl_open_count_facets: The input is an empty file: {model}\n"
        ));
        for (env, stderr) in [
            (&[][..], log.as_str()),
            (&[("SKETCH_ORCA_LOG_LEVEL", "0")][..], ""),
        ] {
            let ran = run(&scratch, &job, env);
            let r = parse(ran.result.as_bytes(), Order::Insertion).unwrap();
            assert_eq!(
                (ran.code, r["code"].as_str(), r["message"].as_str()),
                (-6, Some("model-unreadable"), Some(message.as_str())),
                "{id}"
            );
            assert_eq!(ran.progress, declined_progress(&message), "{id}");
            assert!(ran.stdout.is_empty(), "{id}");
            assert_eq!(String::from_utf8_lossy(&ran.stderr), stderr, "{id}");
        }
    }
}

#[test]
fn the_cr_box_is_refused_where_the_cpp_engine_dies() {
    // A file of CR line ends is one line: its solid line passes the 255 bytes admesh reads it into.
    let scratch = Scratch::new("info", "cr-box");
    let mut text = String::from("solid box\r");
    for _ in 0..12 {
        text.push_str("facet normal 0 0 0\r outer loop\r  vertex 0 0 0\r  vertex 20 0 0\r  vertex 0 20 0\r endloop\rendfacet\r");
    }
    text.push_str("endsolid box\r");
    std::fs::write(scratch.path("box.stl"), &text).unwrap();
    let ran = run(&scratch, &corpus_job(&scratch, "info", "box.stl"), &[]);
    let r = parse(ran.result.as_bytes(), Order::Insertion).unwrap();
    assert_eq!(
        (ran.code, r["code"].as_str()),
        (-6, Some("model-unreadable")),
        "{}",
        ran.result
    );
    let prefix = format!(
        "cannot read the model {}: the ASCII STL's solid line is longer than 255 bytes",
        scratch.path("box.stl")
    );
    assert!(
        r["message"].as_str().unwrap().starts_with(&prefix),
        "{}",
        ran.result
    );
    // Nothing is logged: the reader stops before admesh says anything.
    assert!(
        ran.stdout.is_empty() && ran.stderr.is_empty(),
        "{:?} {:?}",
        String::from_utf8_lossy(&ran.stdout),
        String::from_utf8_lossy(&ran.stderr)
    );
}
