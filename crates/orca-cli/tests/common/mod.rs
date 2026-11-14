// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! What the `run` tests share (intake.rs, decline.rs, crash.rs, info.rs, place.rs): the program
//! run hidden, with no crash trigger, parity knob (`SKETCH_ORCA_ORACLE_*`) or log level set but
//! those a test sets, a scratch folder of the test's own, a valid job in it and the files its work
//! reads up to the decline (a resources folder with a profiles folder, three preset files and a
//! model file, the 20 mm cube as smoke.ts writes it), a job one run of the C++ program was given
//! made to run in a scratch folder ([`recorded_job`]), the progress files the C++ program writes,
//! a result's engine block made comparable with the C++ program's goldens (parity/goldens/front/),
//! and whether parity/parity.json certifies a family (what a decline names depends on it).

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use orca_json::{Object, Order, Value, parse};

pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// The variables that change what the program does, which a test sets itself when it wants one:
/// the crash trigger, the parity knobs and the log level.
const PROGRAM_VARIABLES: [&str; 7] = [
    "SKETCH_ORCA_TEST_CRASH",
    "SKETCH_ORCA_ORACLE_TIME",
    "SKETCH_ORCA_ORACLE_SEED",
    "SKETCH_ORCA_ORACLE_THREADS",
    "SKETCH_ORCA_ORACLE_FMA3",
    "SKETCH_ORCA_ORACLE_DUMP",
    "SKETCH_ORCA_LOG_LEVEL",
];

/// Runs the program, hidden, with `args` and the variables `env` adds.
pub fn program(args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_sketch-orca-rs"));
    c.args(args);
    for k in PROGRAM_VARIABLES {
        c.env_remove(k);
    }
    for (k, v) in env {
        c.env(k, v);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c.output().expect("the program runs")
}

/// A folder of the test's own, empty, removed when it goes out of scope.
pub struct Scratch(pub PathBuf);

impl Scratch {
    /// A folder no other test of this process has: the tests of a file run at once.
    pub fn new(suite: &str, name: &str) -> Scratch {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "orca-cli-{suite}-{}-{n}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    /// The path of `rel` (forward slashes) in the folder, as text.
    pub fn path(&self, rel: &str) -> String {
        let mut p = self.0.clone();
        for part in rel.split('/') {
            p.push(part);
        }
        p.to_string_lossy().into_owned()
    }

    /// The files in the folder, by their forward-slash path, sorted.
    pub fn files(&self) -> Vec<String> {
        fn walk(dir: &Path, rel: &str, out: &mut Vec<String>) {
            for e in std::fs::read_dir(dir).unwrap() {
                let e = e.unwrap();
                let name = e.file_name().to_string_lossy().into_owned();
                let r = if rel.is_empty() {
                    name
                } else {
                    format!("{rel}/{name}")
                };
                if e.file_type().unwrap().is_dir() {
                    walk(&e.path(), &r, out);
                } else {
                    out.push(r);
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.0, "", &mut out);
        orca_stl::sort(&mut out);
        out
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A valid job in `dir`: `command`, every path absolute (the files need not exist for intake).
pub fn valid_job(dir: &Scratch, command: &str) -> Object {
    let mut presets = Object::new(Order::Insertion);
    presets.insert("machine", dir.path("presets/machine.json"));
    presets.insert("process", dir.path("presets/process.json"));
    presets.insert("filaments", vec![dir.path("presets/filament_1.json")]);
    let mut model = Object::new(Order::Insertion);
    model.insert("path", dir.path("model.stl"));
    let mut job = Object::new(Order::Insertion);
    job.insert("format", "sketch-orca-job/1");
    job.insert("command", command);
    job.insert("resources", dir.path("resources"));
    job.insert("workDir", dir.path("work"));
    job.insert("model", model);
    job.insert("presets", presets);
    if command == "slice" {
        let mut output = Object::new(Order::Insertion);
        output.insert("gcode", dir.path("out.gcode"));
        job.insert("output", output);
    }
    job
}

/// Writes `job` to `<dir>/job.json` as the harness writes jobs (two spaces of indent) and
/// returns its path.
pub fn write_job(dir: &Scratch, job: &Object) -> String {
    let path = dir.path("job.json");
    std::fs::write(&path, Value::Object(job.clone()).dump(Some(2)) + "\n").unwrap();
    path
}

/// tests/info: the corpus's generic preset set (tests/info/presets, its three files verbatim) and
/// the resources folder its presets stage reads (tests/info/resources).
pub fn info_fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("info")
}

/// The files of a recorded case that are what the C++ program gave, not what it read.
const RECORDED_OUTPUTS: [&str; 5] = [
    "job.json",
    "result.json",
    "progress.ndjson",
    "stdout",
    "stderr",
];

/// The job of a case one run of the C++ program was given (`case`/job.json, its paths written
/// `<case>/...`, `<presets>/...` and `<resources>`), made to run in `dir`: the case's other input
/// files (its model, a preset of its own) are copied into `dir`, which `<case>` names;
/// `<presets>` names a copy in `dir` of the corpus's preset sets that run read, `generic` (the
/// three files of tests/info/presets) and `generic2` (the same with `filament_2.json`, a copy of
/// the first filament); and `<resources>` names tests/info/resources.
pub fn recorded_job(dir: &Scratch, case: &Path) -> Object {
    for e in std::fs::read_dir(case).unwrap() {
        let e = e.unwrap();
        let name = e.file_name().to_string_lossy().into_owned();
        if e.file_type().unwrap().is_file()
            && !RECORDED_OUTPUTS.contains(&name.as_str())
            && !name.ends_with(".sop1")
        {
            std::fs::copy(e.path(), dir.path(&name)).unwrap();
        }
    }
    let presets = info_fixtures().join("presets");
    for (set, files) in [
        ("generic", &["machine", "process", "filament_1"][..]),
        (
            "generic2",
            &["machine", "process", "filament_1", "filament_2"][..],
        ),
    ] {
        std::fs::create_dir_all(dir.path(&format!("presets/{set}"))).unwrap();
        for file in files {
            let from = if *file == "filament_2" {
                "filament_1"
            } else {
                file
            };
            std::fs::copy(
                presets.join(format!("{from}.json")),
                dir.path(&format!("presets/{set}/{file}.json")),
            )
            .unwrap();
        }
    }
    let resources = info_fixtures()
        .join("resources")
        .to_string_lossy()
        .into_owned();
    let at = |text: &str| -> Option<String> {
        if text == "<resources>" {
            Some(resources.clone())
        } else if let Some(rest) = text.strip_prefix("<case>/") {
            Some(dir.path(rest))
        } else if let Some(rest) = text.strip_prefix("<presets>/") {
            Some(dir.path(&format!("presets/{rest}")))
        } else {
            assert!(!text.starts_with('<'), "a placeholder of no folder: {text}");
            None
        }
    };
    fn place(v: &mut Value, at: &dyn Fn(&str) -> Option<String>) {
        match v {
            Value::String(s) => {
                if let Some(path) = at(s) {
                    *s = path;
                }
            }
            Value::Array(items) => {
                for item in items {
                    place(item, at);
                }
            }
            Value::Object(o) => {
                let keys: Vec<String> = o.keys().cloned().collect();
                for k in keys {
                    place(o.get_mut(&k).unwrap(), at);
                }
            }
            _ => {}
        }
    }
    let mut job = parse(
        &std::fs::read(case.join("job.json")).unwrap(),
        Order::Insertion,
    )
    .unwrap();
    place(&mut job, &at);
    job.as_object().unwrap().clone()
}

/// Writes the preset file `presets/<file>.json` of `kind` ("Test <kind>", from the system) with
/// `settings` besides, in their order.
pub fn write_preset(dir: &Scratch, file: &str, kind: &str, settings: &[(&str, Value)]) {
    write_named_preset(dir, file, kind, &format!("Test {kind}"), settings);
}

/// Writes the preset file `presets/<file>.json` of `kind` named `name`, from the system, with
/// `settings` besides, in their order (a setting named `from` takes the place of the system).
pub fn write_named_preset(
    dir: &Scratch,
    file: &str,
    kind: &str,
    name: &str,
    settings: &[(&str, Value)],
) {
    std::fs::create_dir_all(dir.path("presets")).unwrap();
    let mut p = Object::new(Order::Insertion);
    p.insert("type", kind);
    p.insert("name", name);
    p.insert("from", "system");
    for (k, v) in settings {
        p.insert(*k, v.clone());
    }
    std::fs::write(
        dir.path(&format!("presets/{file}.json")),
        Value::Object(p).dump(Some(2)) + "\n",
    )
    .unwrap();
}

/// The files a valid job of [`valid_job`] reads up to the decline (`run.rs`): the resources folder
/// with its profiles folder, the three preset files with no settings of their own (Orca's
/// defaults), and the model's file, the 20 mm cube as a binary STL ([`box_stl`]).
pub fn write_fixtures(dir: &Scratch) {
    std::fs::create_dir_all(dir.path("resources/profiles")).unwrap();
    write_preset(dir, "machine", "machine", &[]);
    write_preset(dir, "process", "process", &[]);
    write_preset(dir, "filament_1", "filament", &[]);
    std::fs::write(dir.path("model.stl"), box_stl([0.0; 3], [20.0; 3], "cube")).unwrap();
}

/// An axis-aligned box from `min` to `max` as a binary STL, as `scripts/lib/models.ts` writes one
/// (`boxTriangles`, `encodeBinaryStl`): an 80-byte header naming it, the count, then each of the 12
/// triangles' normal and corners as 32-bit floats and an attribute of 0. A box of no height is
/// smoke.ts's `flatSquare`.
pub fn box_stl(min: [f64; 3], max: [f64; 3], name: &str) -> Vec<u8> {
    let p = |i: usize, j: usize, k: usize| -> [f64; 3] {
        [
            if i == 1 { max[0] } else { min[0] },
            if j == 1 { max[1] } else { min[1] },
            if k == 1 { max[2] } else { min[2] },
        ]
    };
    let quads = [
        [p(0, 0, 0), p(0, 1, 0), p(1, 1, 0), p(1, 0, 0)],
        [p(0, 0, 1), p(1, 0, 1), p(1, 1, 1), p(0, 1, 1)],
        [p(0, 0, 0), p(1, 0, 0), p(1, 0, 1), p(0, 0, 1)],
        [p(0, 1, 0), p(0, 1, 1), p(1, 1, 1), p(1, 1, 0)],
        [p(0, 0, 0), p(0, 0, 1), p(0, 1, 1), p(0, 1, 0)],
        [p(1, 0, 0), p(1, 1, 0), p(1, 1, 1), p(1, 0, 1)],
    ];
    let mut triangles = Vec::new();
    for [a, b, c, d] in quads {
        triangles.push([a, b, c]);
        triangles.push([a, c, d]);
    }
    let mut header = format!("sketch-orca smoke model: {name}").into_bytes();
    header.resize(80, b' ');
    let mut out = header;
    out.extend_from_slice(&u32::try_from(triangles.len()).unwrap().to_le_bytes());
    for t in triangles {
        let u = [t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]];
        let v = [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]];
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        let n = if len == 0.0 {
            [0.0; 3]
        } else {
            [n[0] / len, n[1] / len, n[2] / len]
        };
        for x in n.iter().chain(t.iter().flatten()) {
            // Buffer.writeFloatLE: the double rounded to the nearest float.
            out.extend_from_slice(&(*x as f32).to_le_bytes());
        }
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    out
}

/// Whether parity/parity.json, as this program was built with it, certifies `family`. A decline
/// names a family this program has the code for only while it is not certified.
pub fn certified(family: &str) -> bool {
    let doc = std::fs::read(repo().join("parity/parity.json")).unwrap();
    let doc = parse(&doc, Order::Insertion).unwrap();
    doc["families"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["name"].as_str() == Some(family) && f["certified"].as_bool() == Some(true))
}

/// `families` without those parity.json certifies: what a decline names of the families this
/// program has the code for (decline.rs `PORTED`).
pub fn uncertified(families: &[&str]) -> Vec<String> {
    families
        .iter()
        .filter(|f| !certified(f))
        .map(|f| (*f).to_string())
        .collect()
}

/// The files [`write_fixtures`] and [`write_job`] write, with `more` (forward slashes), sorted.
pub fn fixture_files(more: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = [
        "job.json",
        "model.stl",
        "presets/filament_1.json",
        "presets/machine.json",
        "presets/process.json",
    ]
    .iter()
    .chain(more)
    .map(|s| (*s).to_string())
    .collect();
    orca_stl::sort(&mut out);
    out
}

/// A progress file as the C++ program writes it: a line for each of `stages`, then, with a
/// message, the failure at the last stage's percent.
pub fn progress_text(stages: &[&str], failed: Option<&str>) -> String {
    let mut out = String::new();
    let mut last = 0.0;
    for stage in stages {
        let (percent, message) = match *stage {
            "job" => (1.0, "reading the job"),
            "presets" => (3.0, "reading the presets"),
            "model" => (8.0, "reading the model"),
            "orient" => (10.0, "orienting the objects"),
            "arrange" => (12.0, "arranging the objects on the plate"),
            other => panic!("no progress line for the stage {other}"),
        };
        let mut line = Object::new(Order::Insertion);
        line.insert("stage", *stage);
        line.insert("percent", percent);
        line.insert("message", message);
        out.push_str(&Value::Object(line).dump(None));
        out.push('\n');
        last = percent;
    }
    if let Some(message) = failed {
        let mut line = Object::new(Order::Insertion);
        line.insert("stage", "failed");
        line.insert("percent", last);
        line.insert("message", message);
        out.push_str(&Value::Object(line).dump(None));
        out.push('\n');
    }
    out
}

/// The golden's engine version (the C++ program's when it was recorded).
pub fn golden_engine_version() -> String {
    let v = std::fs::read(repo().join("parity/goldens/front/version.json")).unwrap();
    parse(&v, Order::Insertion).unwrap()["engineVersion"]
        .as_str()
        .unwrap()
        .to_string()
}

/// A result of this program made comparable with the C++ program's: its engine block without
/// `implementation`, and the golden's engine version.
pub fn as_cpp_result(text: &str) -> String {
    text.replacen(",\n    \"implementation\": \"rust\"", "", 1)
        .replacen(
            &format!("\"engineVersion\": \"{}\"", env!("CARGO_PKG_VERSION")),
            &format!("\"engineVersion\": \"{}\"", golden_engine_version()),
            1,
        )
}

/// A result of this program made comparable with `want`, the C++ program's result of one run: its
/// engine block without `implementation`, and that run's engine version.
pub fn as_recorded_result(text: &str, want: &str) -> String {
    let version = |t: &str| {
        parse(t.as_bytes(), Order::Insertion).unwrap()["engine"]["engineVersion"]
            .as_str()
            .unwrap()
            .to_string()
    };
    text.replacen(",\n    \"implementation\": \"rust\"", "", 1)
        .replacen(
            &format!("\"engineVersion\": \"{}\"", version(text)),
            &format!("\"engineVersion\": \"{}\"", version(want)),
            1,
        )
}

/// What a stream of the C++ program recorded on Windows (`text`, its line feeds CR LF) is on this
/// platform: the same on Windows, plain line feeds elsewhere.
pub fn recorded_stream(text: &str) -> String {
    if cfg!(windows) {
        text.to_string()
    } else {
        text.replace("\r\n", "\n")
    }
}

/// The progress file of a run that failed at intake: the job line, then the failure.
pub fn failed_at_intake_progress(message: &str) -> String {
    progress_text(&["job"], Some(message))
}

/// The progress file of a run that failed or was declined in or after the model stage, before
/// orienting or arranging its objects: the job, presets and model lines, then the failure.
pub fn declined_progress(message: &str) -> String {
    progress_text(&["job", "presets", "model"], Some(message))
}

/// The progress file of a layers or slice job that arranged its objects and was declined once
/// they were placed: the job, presets, model and arrange lines, then the failure.
pub fn placed_progress(message: &str) -> String {
    progress_text(&["job", "presets", "model", "arrange"], Some(message))
}

/// The progress file of a run that answered: its lines for `stages`, then `done` at 100.
pub fn done_progress(stages: &[&str]) -> String {
    let mut out = progress_text(stages, None);
    let mut line = Object::new(Order::Insertion);
    line.insert("stage", "done");
    line.insert("percent", 100.0);
    line.insert("message", "done");
    out.push_str(&Value::Object(line).dump(None));
    out.push('\n');
    out
}

/// The text a C++ stream of the program writes for `text` (line feeds as CR LF on Windows).
pub fn stream_text(text: &str) -> String {
    if cfg!(windows) {
        text.replace('\n', "\r\n")
    } else {
        text.to_string()
    }
}
