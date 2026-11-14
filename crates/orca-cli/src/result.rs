// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! `run`: the job read and checked, its result written (always, also when it fails) and the
//! progress file, as `cli/run.cpp`'s `run_job` and `cli/common.cpp` do them.
//!
//! - The run's files are kept apart first (sketch-orca, 2026-11-01): the job is read, every
//!   absolute path its JSON gives is taken as it is ([`run_files`]), and a result that would
//!   replace the job or one of them is not written at all (stderr says why, -2); a progress file
//!   that would replace a file of the run is refused (`invalid-params`) and never opened; a job
//!   whose outputs would replace an input or each other is `job-invalid`, and a result on one of
//!   its outputs `invalid-params` ([`check_outputs`]). Two paths name one file as
//!   [`crate::paths::same_file`] says.
//! - The result (`sketch-orca-result/1`): format, ok, code, message, objectIndex (when there is
//!   one), missing (a decline's), exitCode, warnings, command, engine, written with two spaces of
//!   indent through `<out>.tmp`. A crash while the run is armed writes it too (`crash.rs`).
//! - The progress file: one JSON line per step, `{stage, percent, message}`, the percent never
//!   going down, then `done` at 100 or `failed` where it stopped.
//! - Past the checks of the job, its work as far as it is ported (`run.rs`): the folders, the
//!   presets stage and the model stage, each failing as the C++ fails, with its warnings, and an
//!   info job's answer (`info`). Past that this program does only what is certified
//!   (`decline.rs`): a job that orients its objects, and a layers or slice job, is declined,
//!   `not-ported` with exit -8, before any output is written. It writes no G-code or .gcode.3mf
//!   yet: besides the result and the progress file, only the work folder, its orca-data folder and
//!   a 3MF's backup folder in it, which the C++ makes too.

use std::io::Write;
use std::sync::Mutex;

use orca_json::{Object, Order, Value};

use crate::common::{Failure, exit_code_for, fail};
use crate::files::{make_parent_directories, read_file, write_file_atomic};
use crate::paths::{is_absolute_path, is_file, same_file};
use crate::{crash, job, run, version};

/// `RESULT_FORMAT`.
pub const RESULT_FORMAT: &str = "sketch-orca-result/1";

/// Where the crash message goes in the armed result's text.
const MARK: &str = "@@sketch-orca-crash-message@@";

/// `struct Warning`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub code: String,
    pub message: String,
    pub object_index: i32,
}

/// `class Warnings`: identical entries are kept once.
#[derive(Debug, Default)]
pub struct Warnings {
    items: Mutex<Vec<Warning>>,
}

impl Warnings {
    pub fn add(&self, code: &str, message: &str, object_index: i32) {
        let mut items = self
            .items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if items
            .iter()
            .any(|w| w.code == code && w.message == message && w.object_index == object_index)
        {
            return;
        }
        items.push(Warning {
            code: code.to_string(),
            message: message.to_string(),
            object_index,
        });
    }

    #[must_use]
    pub fn list(&self) -> Vec<Warning> {
        self.items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

/// `warnings_json(warnings)`.
fn warnings_json(warnings: &Warnings) -> Value {
    Value::Array(
        warnings
            .list()
            .into_iter()
            .map(|w| {
                let mut o = Object::new(Order::Insertion);
                o.insert("code", w.code);
                o.insert("message", w.message);
                if w.object_index >= 0 {
                    o.insert("objectIndex", w.object_index);
                }
                Value::Object(o)
            })
            .collect(),
    )
}

/// `class Progress`: the --progress file, one JSON object per line, flushed after each.
#[derive(Debug, Default)]
pub struct Progress {
    file: Option<std::fs::File>,
    last: f64,
}

impl Progress {
    /// `open(path)`: false when the file, or its folder, cannot be made.
    pub fn open(&mut self, path: &str) -> bool {
        if make_parent_directories(path).is_err() {
            return false;
        }
        match std::fs::File::create(path) {
            Ok(f) => {
                self.file = Some(f);
                true
            }
            Err(_) => false,
        }
    }

    /// `line(stage, percent, message)`: the stage noted for a crash, the percent never going down
    /// and held between 0 and 100.
    pub fn line(&mut self, stage: &str, percent: f64, message: &str) {
        crash::note_stage(stage);
        let p = if percent.is_finite() {
            percent
        } else {
            self.last
        };
        let p = orca_stl::max(self.last, orca_stl::min(100., orca_stl::max(0., p)));
        self.last = p;
        self.write(stage, p, message);
    }

    /// `finish(ok, message)`: "done" at 100, or "failed" where the run stopped.
    pub fn finish(&mut self, ok: bool, message: &str) {
        if ok {
            self.last = 100.;
        }
        let last = self.last;
        self.write(if ok { "done" } else { "failed" }, last, message);
    }

    fn write(&mut self, stage: &str, percent: f64, message: &str) {
        let Some(file) = self.file.as_mut() else {
            return;
        };
        let mut j = Object::new(Order::Insertion);
        j.insert("stage", stage);
        j.insert("percent", orca_crt::round(percent * 10.) / 10.);
        j.insert("message", message);
        let line = Value::Object(j).dump(None) + "\n";
        let _ = file.write_all(line.as_bytes());
        let _ = file.flush();
    }
}

/// `ordered_result(...)`: the result's members in their order; `body` gives command, info,
/// layers, printZ, outputs and stats when it has them.
fn ordered_result(
    ok: bool,
    failure: Option<&Failure>,
    body: &Object,
    warnings: &Warnings,
) -> Value {
    let mut out = Object::new(Order::Insertion);
    out.insert("format", RESULT_FORMAT);
    out.insert("ok", ok);
    let (code, message, object_index, exit_code) = match failure {
        Some(f) => (
            f.code.as_str(),
            f.message.as_str(),
            f.object_index,
            f.exit_code,
        ),
        None => ("ok", "", -1, 0),
    };
    out.insert("code", code);
    out.insert("message", message);
    if object_index >= 0 {
        out.insert("objectIndex", object_index);
    }
    if let Some(f) = failure
        && !f.missing.is_empty()
    {
        out.insert("missing", f.missing.clone());
    }
    out.insert("exitCode", exit_code);
    out.insert("warnings", warnings_json(warnings));
    for key in ["command", "info", "layers", "printZ", "outputs", "stats"] {
        if let Some(v) = body.get(key) {
            out.insert(key, v.clone());
        }
    }
    out.insert("engine", version::version_json());
    Value::Object(out)
}

/// The result as written: two spaces of indent, then a line feed.
fn result_text(v: &Value) -> String {
    v.dump(Some(2)) + "\n"
}

/// `write_failure_result(out_path, code, message, exit_code)`: false when it cannot be written.
pub fn write_failure_result(out_path: &str, code: &str, message: &str, exit_code: i32) -> bool {
    let mut f = fail(code, message);
    f.exit_code = exit_code;
    let text = result_text(&ordered_result(
        false,
        Some(&f),
        &Object::new(Order::Insertion),
        &Warnings::default(),
    ));
    write_file_atomic(out_path, text.as_bytes()).is_ok()
}

/// `arm_crash(out_path, body)`: an internal-error result kept ready for a crash.
fn arm_crash(out_path: &str, body: &Object) {
    let f = fail("internal-error", MARK);
    let text = result_text(&ordered_result(false, Some(&f), body, &Warnings::default()));
    if let Some(at) = text.find(MARK) {
        crash::arm(out_path, &text[..at], &text[at + MARK.len()..]);
    }
}

/// `struct Written`: a file the run writes, with the temporary files it is written through.
struct Written {
    what: String,
    files: Vec<String>,
}

/// `result_files(out_path)`: the result, through `<out>.tmp` and a crash's `<out>.crash.tmp`.
fn result_files(out_path: &str) -> Written {
    Written {
        what: format!("the result (--out) {out_path}"),
        files: vec![
            out_path.to_string(),
            format!("{out_path}.tmp"),
            format!("{out_path}.crash.tmp"),
        ],
    }
}

/// `shared_file(a, b)`: the first file of `a` that is a file of `b`.
fn shared_file(a: &[String], b: &[String]) -> Option<String> {
    a.iter()
        .find(|x| b.iter().any(|y| same_file(x, y)))
        .cloned()
}

/// A file of a run, and what names it: a field of the job ("model.path") or the command line.
#[derive(Clone, Debug)]
pub struct NamedFile {
    pub field: String,
    pub path: String,
}

/// `named_files(v, field, inputs, outputs)`: every absolute path the job's JSON gives, by its
/// field, output.gcode and output.bambu3mf in `outputs` and every other one in `inputs`.
fn named_files(v: &Value, field: &str, inputs: &mut Vec<NamedFile>, outputs: &mut Vec<NamedFile>) {
    match v {
        Value::String(path) => {
            if is_absolute_path(path) {
                let named = NamedFile {
                    field: field.to_string(),
                    path: path.clone(),
                };
                if field == "output.gcode" || field == "output.bambu3mf" {
                    outputs.push(named);
                } else {
                    inputs.push(named);
                }
            }
        }
        Value::Object(o) => {
            for (k, x) in o.iter() {
                let f = if field.is_empty() {
                    k.clone()
                } else {
                    format!("{field}.{k}")
                };
                named_files(x, &f, inputs, outputs);
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                named_files(x, &format!("{field}[{i}]"), inputs, outputs);
            }
        }
        _ => {}
    }
}

/// `struct RunFiles`: the files a run reads, and the outputs the job writes.
pub struct RunFiles {
    pub inputs: Vec<NamedFile>,
    outputs: Vec<Written>,
}

/// `run_files(job_path, text)`: `text` is the job as read, `None` when it cannot be read; a job
/// that is not JSON names no file.
#[must_use]
pub fn run_files(job_path: &str, text: Option<&[u8]>) -> RunFiles {
    let mut files = RunFiles {
        inputs: vec![NamedFile {
            field: "the job".to_string(),
            path: job_path.to_string(),
        }],
        outputs: Vec::new(),
    };
    let Some(text) = text else {
        return files;
    };
    let Ok(root) = orca_json::parse(text, Order::Insertion) else {
        return files;
    };
    let mut outputs = Vec::new();
    named_files(&root, "", &mut files.inputs, &mut outputs);
    for out in outputs {
        let p = &out.path;
        files.outputs.push(if out.field == "output.gcode" {
            Written {
                what: format!("output.gcode {p}"),
                files: vec![
                    p.clone(),
                    format!("{p}.tmp"),
                    format!("{p}.tmp.postprocess"),
                ],
            }
        } else {
            Written {
                what: format!("output.bambu3mf {p}"),
                files: vec![p.clone(), format!("{p}.tmp"), format!("{p}.rels.tmp")],
            }
        });
    }
    files
}

/// `result_clash(out_path, inputs)`: what the result, or a file it is written through, would
/// replace of `inputs`.
fn result_clash(out_path: &str, inputs: &[NamedFile]) -> Option<String> {
    let result = result_files(out_path);
    inputs.iter().find_map(|input| {
        shared_file(&result.files, std::slice::from_ref(&input.path)).map(|clash| {
            format!(
                "the result file {clash} would replace {} {}",
                input.field, input.path
            )
        })
    })
}

/// `progress_clash(progress_path, out_path, files)`: what the progress file would replace of
/// the run's files, the result's included.
fn progress_clash(progress_path: &str, out_path: &str, files: &RunFiles) -> Option<String> {
    let what = format!("the progress file (--progress) {progress_path}");
    for input in &files.inputs {
        if same_file(progress_path, &input.path) {
            return Some(format!(
                "{what} would replace {} {}",
                input.field, input.path
            ));
        }
    }
    let progress = [progress_path.to_string()];
    let result = result_files(out_path);
    if let Some(clash) = shared_file(&progress, &result.files) {
        return Some(format!("{what} would replace {} ({clash})", result.what));
    }
    for out in &files.outputs {
        if let Some(clash) = shared_file(&progress, &out.files) {
            return Some(format!("{what} and {} are one file ({clash})", out.what));
        }
    }
    None
}

/// `check_outputs(files, out_path)`: the job's outputs must be files of their own.
fn check_outputs(files: &RunFiles, out_path: &str) -> Result<(), Failure> {
    for out in &files.outputs {
        for input in &files.inputs {
            if let Some(clash) = shared_file(&out.files, std::slice::from_ref(&input.path)) {
                return Err(fail(
                    "job-invalid",
                    format!(
                        "{} would replace {} {} ({clash})",
                        out.what, input.field, input.path
                    ),
                ));
            }
        }
    }
    for (i, a) in files.outputs.iter().enumerate() {
        for b in &files.outputs[i + 1..] {
            if let Some(clash) = shared_file(&a.files, &b.files) {
                return Err(fail(
                    "job-invalid",
                    format!("{} and {} are one file ({clash})", a.what, b.what),
                ));
            }
        }
    }
    let result = result_files(out_path);
    for out in &files.outputs {
        if let Some(clash) = shared_file(&result.files, &out.files) {
            return Err(fail(
                "invalid-params",
                format!("{} and {} are one file ({clash})", result.what, out.what),
            ));
        }
    }
    Ok(())
}

/// Text for stderr as the C++ streams write it on Windows (`main.rs` `write_text`).
fn say(err: &mut dyn Write, text: &str) {
    crate::write_text(err, text);
}

/// `write_command_line_failure(out_path, named, jobs, message, exit_code)`: the invalid-params
/// result of a `run` whose command line is wrong, unless the result would replace a file the
/// command line names or one a job it names names; then nothing is written and stderr says why.
pub fn write_command_line_failure(
    out_path: &str,
    named: &[String],
    jobs: &[String],
    message: &str,
    exit_code: i32,
    err: &mut dyn Write,
) -> bool {
    let mut inputs: Vec<NamedFile> = named
        .iter()
        .map(|v| NamedFile {
            field: "the file the command line names".to_string(),
            path: v.clone(),
        })
        .collect();
    for job_path in jobs {
        let Ok(text) = read_file(job_path) else {
            continue;
        };
        inputs.extend(run_files(job_path, Some(&text)).inputs);
    }
    if let Some(clash) = result_clash(out_path, &inputs) {
        say(err, &format!("sketch-orca: {clash}; nothing is written\n"));
        return false;
    }
    write_failure_result(out_path, "invalid-params", message, exit_code)
}

/// `run_job(job_path, out_path, progress_path)`: the exit code.
pub fn run_job(job_path: &str, out_path: &str, progress_path: &str, err: &mut dyn Write) -> i32 {
    let text = read_file(job_path).ok();
    let files = run_files(job_path, text.as_deref());
    if let Some(clash) = result_clash(out_path, &files.inputs) {
        say(err, &format!("sketch-orca: {clash}; nothing is written\n"));
        return exit_code_for("invalid-params");
    }
    let progress_refused = if progress_path.is_empty() {
        None
    } else {
        progress_clash(progress_path, out_path, &files)
    };

    let warnings = Warnings::default();
    let mut progress = Progress::default();
    let mut body = Object::new(Order::Insertion);
    if !progress_path.is_empty() && progress_refused.is_none() {
        if !progress.open(progress_path) {
            warnings.add(
                "progress-unwritable",
                &format!("cannot write the progress file {progress_path}"),
                -1,
            );
        }
        progress.line("job", 1., "reading the job");
    }
    arm_crash(out_path, &body);
    let outcome = (|| -> Result<(), Failure> {
        if let Some(refused) = &progress_refused {
            return Err(fail("invalid-params", refused.clone()));
        }
        let Some(text) = &text else {
            let code = if is_file(job_path) {
                "job-invalid"
            } else {
                "file-not-found"
            };
            return Err(fail(code, format!("cannot read the job {job_path}")));
        };
        let job = job::parse_job(text)?;
        body.insert("command", job.command_name.as_str());
        arm_crash(out_path, &body);
        check_outputs(&files, out_path)?;
        // The job's work, as far as it is ported (run.rs): an info job's answer, else the decline.
        run::run(&job, &mut body, &warnings, &mut progress)
    })();
    let failure = outcome.err();
    let ok = failure.is_none();
    // A failed result holds no partial results: the failure, the warnings and the command only.
    if !ok {
        for key in ["info", "layers", "printZ", "outputs", "stats"] {
            body.remove(key);
        }
    }
    crash::disarm();
    let result = ordered_result(ok, failure.as_ref(), &body, &warnings);
    let exit_code = failure.as_ref().map_or(0, |f| f.exit_code);
    if let Err(e) = write_file_atomic(out_path, result_text(&result).as_bytes()) {
        say(
            err,
            &format!("sketch-orca: cannot write the result {out_path}: {e}\n"),
        );
        return if ok {
            exit_code_for("output-failed")
        } else {
            exit_code
        };
    }
    progress.finish(ok, failure.as_ref().map_or("done", |f| f.message.as_str()));
    exit_code
}

#[cfg(test)]
mod tests {
    use super::{Progress, Warnings, ordered_result, warnings_json};
    use crate::common::fail;
    use orca_json::{Object, Order};

    #[test]
    fn warnings_are_kept_once_in_their_order() {
        let w = Warnings::default();
        w.add("a", "one", -1);
        w.add("b", "two", 2);
        w.add("a", "one", -1);
        assert_eq!(
            warnings_json(&w).dump(None),
            r#"[{"code":"a","message":"one"},{"code":"b","message":"two","objectIndex":2}]"#
        );
    }

    #[test]
    fn a_result_has_its_members_in_the_cpp_order() {
        let mut f = fail("not-ported", "m");
        f.exit_code = -8;
        f.missing = vec!["model-load".to_string()];
        let mut body = Object::new(Order::Insertion);
        body.insert("stats", 1);
        body.insert("command", "info");
        let text = ordered_result(false, Some(&f), &body, &Warnings::default()).dump(None);
        assert!(
            text.starts_with(r#"{"format":"sketch-orca-result/1","ok":false,"code":"not-ported","message":"m","missing":["model-load"],"exitCode":-8,"warnings":[],"command":"info","stats":1,"engine":{"engine":"sketch-orca""#),
            "{text}"
        );
    }

    #[test]
    fn progress_never_goes_down_and_rounds_to_a_tenth() {
        let dir = std::env::temp_dir().join(format!("orca-cli-progress-{}", std::process::id()));
        let path = dir.join("p.ndjson");
        let mut p = Progress::default();
        assert!(p.open(&path.to_string_lossy()));
        p.line("other-a", 12.34, "a");
        p.line("other-b", 5., "b");
        p.line("other-c", f64::NAN, "c");
        p.line("other-d", 250., "d");
        p.finish(false, "stopped");
        drop(p);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            concat!(
                "{\"stage\":\"other-a\",\"percent\":12.3,\"message\":\"a\"}\n",
                "{\"stage\":\"other-b\",\"percent\":12.3,\"message\":\"b\"}\n",
                "{\"stage\":\"other-c\",\"percent\":12.3,\"message\":\"c\"}\n",
                "{\"stage\":\"other-d\",\"percent\":100.0,\"message\":\"d\"}\n",
                "{\"stage\":\"failed\",\"percent\":100.0,\"message\":\"stopped\"}\n",
            )
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
