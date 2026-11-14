// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! sketch-orca-rs: the Rust program of the sketch-orca translation, a separate program from the
//! C++ `sketch-orca` of `cli/` and under the same licence.
//!
//! Its command line is `cli/main.cpp`'s: `help` (and `--help`, `-h`), `version` (and
//! `--version`), `config-def --out <file>` and `run --job <file> --out <file> [--progress <file>]`,
//! with the same usage text, the same messages for a command line it does not take, and the same
//! exit codes. `version` and `config-def` answer as the C++ program does (`version` adds
//! `implementation: "rust"`). `run` checks its command line and reads and checks the job as the
//! C++ does, writing the same result and progress file for a job it refuses (`result.rs`,
//! `job.rs`); then it does the job's work as far as it is ported (`run.rs`): the resources and
//! work folders, the presets stage (`presets.rs`, on Orca's configuration in `config.rs`), the
//! model stage (`model.rs`, on orca-model's readers) and the objects' settings and placement
//! (`place.rs`, on orca-model's and orca-arrange's arrangement), with the C++ failures and
//! warnings. An info job it answers whole (`info.rs`). A job that orients its objects, and a
//! layers or slice job once its objects are placed, it declines, `not-ported` with exit -8, while
//! what the job needs is not ported and certified (`decline.rs`). Orca's log goes to stderr as
//! the C++ program's does (`log.rs`), and a crash or a panic still writes the result
//! (`crash.rs`).
//!
//! For parity work it reads the oracle's knobs (`SKETCH_ORCA_ORACLE_*`) as the oracle does
//! (`oracle.rs`): `main` turns the CRT's FMA3 paths off when asked, refuses a knob it cannot read
//! before anything else (exit 78), and runs the rest, the engine's main, in the thread knob's
//! arena when that knob is set.

mod common;
mod config;
mod config_def;
mod crash;
mod decline;
mod files;
mod info;
mod job;
mod log;
mod model;
mod oracle;
mod paths;
mod place;
mod presets;
mod result;
mod run;
mod version;

use std::collections::BTreeMap;
use std::io::Write;

use common::{FAILURE_TABLE, exit_code_for};

/// The exit code of a command line the program does not take (`EXIT_INVALID_PARAMS` in
/// `cli/main.cpp`; the stock Orca command line's `CLI_INVALID_PARAMS`).
const EXIT_INVALID_PARAMS: i32 = -2;

/// The usage text of `cli/main.cpp`'s `usage()`: the commands, then the exit codes from the
/// highest (closest to zero) to the lowest with the failure codes that share each, then the
/// licence.
fn usage() -> String {
    let mut text = String::from(concat!(
        "sketch-orca: a command-line slicer on OrcaSlicer's slicing library.\n",
        "\n",
        "  sketch-orca version\n",
        "      Prints the engine and Orca versions as one line of JSON.\n",
        "  sketch-orca config-def --out <file>\n",
        "      Writes every print, filament and printer setting, with its type, values, default and\n",
        "      limits, and the obsolete keys Orca drops or renames when it reads them, as JSON.\n",
        "  sketch-orca run --job <file> --out <file> [--progress <file>]\n",
        "      Does a job (sketch-orca-job/1: slice, layers or info) and writes its result\n",
        "      (sketch-orca-result/1), also when it fails. With --progress, writes one JSON line\n",
        "      { stage, percent, message } per step.\n",
        "\n",
        "SKETCH_ORCA_LOG_LEVEL sets Orca's log level on stderr (0 fatal ... 5 trace; 1 by default, as in\n",
        "the stock command line). Nothing but the commands' own output goes to stdout. A crash in\n",
        "Orca's code still writes the result, as internal-error.\n",
        "\n",
        "Exit codes: 0 when the result is ok, else the stock Orca command line's code for the failure:\n",
    ));
    // An ordered map, read from its end as the C++ reads its std::map with rbegin.
    let mut by_code: BTreeMap<i32, Vec<&str>> = BTreeMap::new();
    for (name, code) in FAILURE_TABLE.iter().filter(|e| e.1 != 0) {
        by_code.entry(*code).or_default().push(name);
    }
    for (code, names) in by_code.iter().rev() {
        text.push_str(&format!("  {:>6}  {}\n", code, names.join(", ")));
    }
    text.push_str(
        "A collision printing layer by layer exits with -64 (the stock code) instead of -63.\n",
    );
    text.push_str(concat!(
        "\n",
        "sketch-orca is a modified version of OrcaSlicer: its slicing library with a command-line front\n",
        "end (modified 2026-10-30). It is free software under the GNU Affero General Public License,\n",
        "version 3, and comes with NO WARRANTY, to the extent permitted by law. It is a separate\n",
        "program: programs that run it, such as Sketch, talk to it only through this command line and\n",
        "its files. The licences are in the LICENSES folder next to the engine, and its complete source\n",
        "is offered with every release of it (the source bundle).\n",
    ));
    text
}

/// What `usage_error` of `cli/main.cpp` writes to stderr: "sketch-orca: <message>", a blank line
/// and the usage.
fn usage_error_text(message: &str) -> String {
    format!("sketch-orca: {message}\n\n{}", usage())
}

/// `parse_options`: `--name value` or `--name=value`, every option with a value, each known,
/// given once and not empty; the message of the first that is not.
fn parse_options(
    args: &[String],
    first: usize,
    known: &[&str],
) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    parse_options_into(args, first, known, &mut out)?;
    Ok(out)
}

/// `parse_options` as the C++ calls it: `out` holds the options read before the first that is
/// wrong.
fn parse_options_into(
    args: &[String],
    first: usize,
    known: &[&str],
    out: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    let mut i = first;
    while i < args.len() {
        let a = &args[i];
        if a.len() < 3 || !a.starts_with("--") {
            return Err(format!("unexpected argument {a}"));
        }
        let mut name = a[2..].to_string();
        let value;
        if let Some(eq) = name.find('=') {
            value = name[eq + 1..].to_string();
            name.truncate(eq);
        } else {
            if i + 1 >= args.len() {
                return Err(format!("--{name} needs a value"));
            }
            i += 1;
            value = args[i].clone();
        }
        if !known.contains(&name.as_str()) {
            return Err(format!("unknown option --{name}"));
        }
        if out.contains_key(&name) {
            return Err(format!("--{name} is given twice"));
        }
        if value.is_empty() {
            return Err(format!("--{name} is empty"));
        }
        out.insert(name, value);
        i += 1;
    }
    Ok(())
}

/// `find_option(args, first, name)`: the value of `--name` (`--name value` or `--name=value`)
/// wherever it is, "" when it is not given; it finds --out before the options are checked.
fn find_option(args: &[String], first: usize, name: &str) -> String {
    let flag = format!("--{name}");
    let with_eq = format!("{flag}=");
    for i in first..args.len() {
        if args[i] == flag {
            return args.get(i + 1).cloned().unwrap_or_default();
        }
        if let Some(value) = args[i].strip_prefix(&with_eq) {
            return value.to_string();
        }
    }
    String::new()
}

/// `command_line_values(args, out, values, jobs)`: the values a command line gives after its
/// command (`--name value`, `--name=value` and stray ones), but the first --out's, which is
/// `out`; and the values of --job.
fn command_line_values(args: &[String], out: &str) -> (Vec<String>, Vec<String>) {
    let (mut values, mut jobs) = (Vec::new(), Vec::new());
    let mut out_seen = false;
    for i in 2..args.len() {
        let a = &args[i];
        let (name, value);
        if let Some(rest) = a.strip_prefix("--") {
            let Some(eq) = rest.find('=') else {
                continue;
            };
            name = rest[..eq].to_string();
            value = rest[eq + 1..].to_string();
        } else {
            value = a.clone();
            let prev = &args[i - 1];
            name = if i > 2 && prev.starts_with("--") && !prev.contains('=') {
                prev[2..].to_string()
            } else {
                String::new()
            };
        }
        if !out_seen && name == "out" && value == out {
            out_seen = true;
            continue;
        }
        if name == "job" {
            jobs.push(value.clone());
        }
        values.push(value);
    }
    (values, jobs)
}

/// Where the program writes: its standard output and standard error.
struct Streams<'a> {
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
}

/// `usage_error`: the message and the usage on stderr, -2.
fn usage_error(s: &mut Streams<'_>, message: &str) -> i32 {
    write_text(s.err, &usage_error_text(message));
    EXIT_INVALID_PARAMS
}

fn command_config_def(args: &[String], s: &mut Streams<'_>) -> i32 {
    let opts = match parse_options(args, 2, &["out"]) {
        Ok(o) => o,
        Err(e) => return usage_error(s, &e),
    };
    let Some(out) = opts.get("out") else {
        return usage_error(s, "config-def needs --out <file>");
    };
    if let Err(e) = files::write_file_atomic(out, config_def::config_def_text().as_bytes()) {
        write_text(s.err, &format!("sketch-orca: cannot write {out}: {e}\n"));
        return exit_code_for("output-failed");
    }
    0
}

fn command_run(args: &[String], s: &mut Streams<'_>) -> i32 {
    let mut opts = BTreeMap::new();
    let parsed = parse_options_into(args, 2, &["job", "out", "progress"], &mut opts);
    let out = opts
        .get("out")
        .cloned()
        .unwrap_or_else(|| find_option(args, 2, "out"));
    // A wrong command line gets its result at --out, unless that would replace a file the command
    // line or its job names (sketch-orca, 2026-11-01).
    let refuse = |s: &mut Streams<'_>, message: &str| {
        let code = usage_error(s, message);
        if out.is_empty() {
            return code;
        }
        let (values, jobs) = command_line_values(args, &out);
        result::write_command_line_failure(&out, &values, &jobs, message, code, s.err);
        code
    };
    if let Err(e) = parsed {
        return refuse(s, &e);
    }
    if out.is_empty() {
        return usage_error(s, "run needs --out <file> for the result");
    }
    let Some(job) = opts.get("job") else {
        return refuse(s, "run needs --job <file>");
    };
    let progress = opts.get("progress").map_or("", String::as_str);
    result::run_job(job, &out, progress, s.err)
}

/// Runs the command line `args` (the program's name first) and returns the exit code.
fn run(args: &[String], s: &mut Streams<'_>) -> i32 {
    let Some(command) = args.get(1) else {
        return usage_error(s, "no command");
    };
    match command.as_str() {
        "help" | "--help" | "-h" => {
            write_text(s.out, &usage());
            0
        }
        "version" | "--version" => {
            if args.len() > 2 {
                return usage_error(s, "version takes no options");
            }
            write_text(s.out, &version::version_line());
            0
        }
        "config-def" => command_config_def(args, s),
        "run" => command_run(args, s),
        _ => usage_error(s, &format!("unknown command {command}")),
    }
}

/// Writes `text` as the C++ program's streams do on Windows: in text mode, so each line feed
/// goes out as a carriage return and a line feed. A broken stream cannot be told about; the
/// exit code still says what happened.
pub(crate) fn write_text(out: &mut dyn Write, text: &str) {
    let bytes = if cfg!(windows) {
        text.replace('\n', "\r\n")
    } else {
        text.to_string()
    };
    let _ = out.write_all(bytes.as_bytes());
    let _ = out.flush();
}

/// The engine's main (`engine_main` of the oracle's `cli/main.cpp`): the clock, seed and dump
/// knobs checked, then the crash handlers, the log and the command.
fn engine_main() -> i32 {
    if let Some(problem) = oracle::engine_problem() {
        return oracle::knob_error(&mut std::io::stderr(), &problem);
    }
    crash::install();
    let args: Vec<String> = std::env::args_os()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    // cli/main.cpp: log_to_stderr() and set_logging_level(log_level()), before the command.
    log::install();
    let mut out = std::io::stdout();
    let mut err = std::io::stderr();
    run(
        &args,
        &mut Streams {
            out: &mut out,
            err: &mut err,
        },
    )
}

fn main() {
    // The oracle's static initializer runs first of all: SKETCH_ORCA_ORACLE_FMA3=0.
    oracle::fma3_switch();
    if let Some(problem) = oracle::main_problem() {
        std::process::exit(oracle::knob_error(&mut std::io::stderr(), problem));
    }
    let code = match oracle::threads() {
        None => engine_main(),
        Some(p) => oracle::in_thread_arena(p, engine_main),
    };
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_with(args: &[&str]) -> (i32, String) {
        let args: Vec<String> = args.iter().map(|a| (*a).to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(
            &args,
            &mut Streams {
                out: &mut out,
                err: &mut err,
            },
        );
        assert!(out.is_empty() || err.is_empty(), "one stream only");
        let text = if out.is_empty() { err } else { out };
        (code, String::from_utf8(text).unwrap().replace("\r\n", "\n"))
    }

    #[test]
    fn on_windows_every_line_ends_with_a_carriage_return_and_a_line_feed_as_the_cpp_streams_write()
    {
        let mut out = Vec::new();
        write_text(&mut out, "a\nb\n");
        assert_eq!(
            out,
            if cfg!(windows) {
                b"a\r\nb\r\n".to_vec()
            } else {
                b"a\nb\n".to_vec()
            }
        );
    }

    #[test]
    fn no_command_prints_the_usage_and_exits_with_minus_2() {
        let (code, err) = run_with(&["sketch-orca-rs"]);
        assert_eq!(code, -2);
        assert!(err.starts_with("sketch-orca: no command\n\nsketch-orca: a command-line slicer"));
        assert!(err.ends_with("is offered with every release of it (the source bundle).\n"));
    }

    #[test]
    fn the_exit_codes_run_from_minus_1_down_with_the_names_that_share_each() {
        let text = usage();
        let at = text.find("Exit codes:").unwrap();
        let table: Vec<&str> = text[at..]
            .lines()
            .skip(1)
            .take_while(|l| l.starts_with("  "))
            .collect();
        assert_eq!(
            table.first().copied(),
            Some("      -1  environment, internal-error")
        );
        assert_eq!(
            table.get(1).copied(),
            Some(
                "      -2  invalid-params, job-invalid, setting-invalid, layer-profile-invalid, custom-gcode-invalid"
            )
        );
        assert_eq!(
            table.last().copied(),
            Some("    -102  gcode-in-unprintable-area")
        );
        // 28 failure codes in the table, "ok" apart, on 22 distinct exit codes.
        assert_eq!(table.len(), 22);
        let mut seen = Vec::new();
        for line in &table {
            let (code, names) = line.trim().split_once("  ").unwrap();
            let code: i32 = code.parse().unwrap();
            assert!(seen.iter().all(|&c| c > code), "codes descend: {line}");
            seen.push(code);
            for name in names.split(", ") {
                assert!(FAILURE_TABLE.contains(&(name, code)), "{name} is {code}");
            }
        }
    }

    #[test]
    fn every_failure_code_is_in_the_usage_but_ok() {
        let text = usage();
        for (name, code) in FAILURE_TABLE {
            let listed = text
                .lines()
                .any(|l| l.starts_with("  ") && l.contains(name));
            assert_eq!(listed, *code != 0, "{name}");
        }
    }

    #[test]
    fn the_usage_gives_the_licence_as_version_3_and_the_program_as_a_separate_program() {
        let text = usage();
        assert!(text.contains("GNU Affero General Public License,\nversion 3"));
        assert!(!text.contains("later"));
        assert!(text.contains("It is a separate\nprogram"));
    }

    #[test]
    fn help_prints_the_usage_on_stdout_and_an_unknown_command_is_a_usage_error() {
        for h in ["help", "--help", "-h"] {
            let (code, out) = run_with(&["sketch-orca-rs", h]);
            assert_eq!((code, out), (0, usage()));
        }
        let (code, err) = run_with(&["sketch-orca-rs", "slice"]);
        assert_eq!(code, -2);
        assert!(err.starts_with("sketch-orca: unknown command slice\n\n"));
        let (code, err) = run_with(&["sketch-orca-rs", "version", "--x"]);
        assert_eq!(code, -2);
        assert!(err.starts_with("sketch-orca: version takes no options\n\n"));
    }

    #[test]
    fn options_are_refused_as_cli_main_refuses_them() {
        let refused = |args: &[&str]| {
            let mut all = vec!["sketch-orca-rs", "config-def"];
            all.extend_from_slice(args);
            let (code, err) = run_with(&all);
            assert_eq!(code, -2, "{args:?}");
            err.lines().next().unwrap().to_string()
        };
        assert_eq!(refused(&[]), "sketch-orca: config-def needs --out <file>");
        assert_eq!(
            refused(&["out.json"]),
            "sketch-orca: unexpected argument out.json"
        );
        assert_eq!(refused(&["--out"]), "sketch-orca: --out needs a value");
        assert_eq!(refused(&["--job=a"]), "sketch-orca: unknown option --job");
        assert_eq!(
            refused(&["--out=a", "--out", "b"]),
            "sketch-orca: --out is given twice"
        );
        assert_eq!(refused(&["--out="]), "sketch-orca: --out is empty");
        assert_eq!(refused(&["--"]), "sketch-orca: unexpected argument --");
        let (code, err) = run_with(&["sketch-orca-rs", "run", "--job", "j.json"]);
        assert_eq!(code, -2);
        assert!(err.starts_with("sketch-orca: run needs --out <file> for the result\n"));
        // A run without --job still gets its result at --out.
        let dir = std::env::temp_dir().join(format!("orca-cli-main-{}", std::process::id()));
        let out = dir.join("r.json").to_string_lossy().into_owned();
        let (code, err) = run_with(&["sketch-orca-rs", "run", "--out", &out]);
        assert_eq!(code, -2);
        assert!(err.starts_with("sketch-orca: run needs --job <file>\n"));
        let result = std::fs::read_to_string(&out).unwrap();
        assert!(result.contains("\"code\": \"invalid-params\",\n  \"message\": \"run needs --job <file>\",\n  \"exitCode\": -2,"), "{result}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_command_line_values_are_those_cli_main_reads() {
        let args: Vec<String> = [
            "x",
            "run",
            "--job",
            "j",
            "--out=o",
            "--progress",
            "p",
            "stray",
            "--out",
            "o",
            "--x=y",
        ]
        .iter()
        .map(|a| (*a).to_string())
        .collect();
        assert_eq!(find_option(&args, 2, "out"), "o");
        assert_eq!(find_option(&args, 2, "progress"), "p");
        assert_eq!(find_option(&args, 2, "none"), "");
        let (values, jobs) = command_line_values(&args, "o");
        assert_eq!(values, ["j", "p", "stray", "o", "y"]);
        assert_eq!(jobs, ["j"]);
        let mut partial = BTreeMap::new();
        let e = parse_options_into(&args[..5], 2, &["job", "out"], &mut partial);
        assert_eq!(e, Ok(()));
        assert_eq!(partial.get("out").map(String::as_str), Some("o"));
        let mut partial = BTreeMap::new();
        let e = parse_options_into(&args, 2, &["job", "out"], &mut partial);
        assert_eq!(e, Err("unknown option --progress".to_string()));
        assert_eq!(partial.len(), 2);
    }
}
