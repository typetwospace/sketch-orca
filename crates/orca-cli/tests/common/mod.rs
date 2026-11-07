// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! What the `run` tests share (intake.rs, decline.rs, crash.rs): the program run hidden, a
//! scratch folder of the test's own, a valid job in it and the files its work reads up to the
//! decline (a resources folder with a profiles folder, three preset files and a model file), the
//! progress files the C++ program writes, and a result's engine block made comparable with the C++
//! program's goldens (parity/goldens/front/).

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use orca_json::{Object, Order, Value, parse};

pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// Runs the program, hidden, with `args` and the variables `env` adds.
pub fn program(args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_sketch-orca-rs"));
    c.args(args);
    c.env_remove("SKETCH_ORCA_TEST_CRASH");
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
/// defaults), and the model's file, whose bytes are not read before the decline.
pub fn write_fixtures(dir: &Scratch) {
    std::fs::create_dir_all(dir.path("resources/profiles")).unwrap();
    write_preset(dir, "machine", "machine", &[]);
    write_preset(dir, "process", "process", &[]);
    write_preset(dir, "filament_1", "filament", &[]);
    std::fs::write(dir.path("model.stl"), b"solid").unwrap();
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

/// The progress file of a run that failed at intake: the job line, then the failure.
pub fn failed_at_intake_progress(message: &str) -> String {
    progress_text(&["job"], Some(message))
}

/// The progress file of a run declined after the model's file check: the job, presets and model
/// lines, then the failure.
pub fn declined_progress(message: &str) -> String {
    progress_text(&["job", "presets", "model"], Some(message))
}

/// The text a C++ stream of the program writes for `text` (line feeds as CR LF on Windows).
pub fn stream_text(text: &str) -> String {
    if cfg!(windows) {
        text.replace('\n', "\r\n")
    } else {
        text.to_string()
    }
}
