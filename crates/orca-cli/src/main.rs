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
//! `implementation: "rust"`); `run` checks its command line as the C++ does.

mod config_def;
mod files;
mod version;

use std::collections::BTreeMap;
use std::io::Write;

/// The exit code of a command line the program does not take (`EXIT_INVALID_PARAMS` in
/// `cli/main.cpp`; the stock Orca command line's `CLI_INVALID_PARAMS`).
const EXIT_INVALID_PARAMS: i32 = -2;

/// The failure codes and the exit code of each, in the order of `cli/common.cpp`'s table (the
/// stock Orca command line's `cli_errors` for the same meanings).
const FAILURE_TABLE: &[(&str, i32)] = &[
    ("ok", 0),
    ("environment", -1),
    ("internal-error", -1),
    ("invalid-params", -2),
    ("job-invalid", -2),
    ("setting-invalid", -2),
    ("layer-profile-invalid", -2),
    ("custom-gcode-invalid", -2),
    ("file-not-found", -3),
    ("preset-invalid", -5),
    ("model-unreadable", -6),
    ("not-fdm", -7),
    ("unsupported", -8),
    ("output-failed", -13),
    ("out-of-memory", -14),
    ("process-not-compatible", -17),
    ("config-invalid", -18),
    ("arrange-failed", -21),
    ("orient-failed", -22),
    ("empty-plate", -50),
    ("validate-error", -51),
    ("not-printable", -52),
    ("filament-bed-mismatch", -61),
    ("temperature-difference", -62),
    ("collision", -63),
    ("slice-failed", -100),
    ("path-conflicts", -101),
    ("gcode-in-unprintable-area", -102),
];

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
    Ok(out)
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

/// The exit code of a failure code (`exit_code_for`).
fn exit_code_for(code: &str) -> i32 {
    FAILURE_TABLE
        .iter()
        .find(|e| e.0 == code)
        .map_or(-1, |e| e.1)
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
    let opts = match parse_options(args, 2, &["job", "out", "progress"]) {
        Ok(o) => o,
        Err(e) => return usage_error(s, &e),
    };
    if !opts.contains_key("out") {
        return usage_error(s, "run needs --out <file> for the result");
    }
    if !opts.contains_key("job") {
        return usage_error(s, "run needs --job <file>");
    }
    write_text(s.err, "sketch-orca: run is not in sketch-orca-rs yet\n");
    exit_code_for("unsupported")
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
fn write_text(out: &mut dyn Write, text: &str) {
    let bytes = if cfg!(windows) {
        text.replace('\n', "\r\n")
    } else {
        text.to_string()
    };
    let _ = out.write_all(bytes.as_bytes());
    let _ = out.flush();
}

fn main() {
    let args: Vec<String> = std::env::args_os()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let mut out = std::io::stdout();
    let mut err = std::io::stderr();
    let code = run(
        &args,
        &mut Streams {
            out: &mut out,
            err: &mut err,
        },
    );
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
        let (code, err) = run_with(&["sketch-orca-rs", "run", "--out", "r.json"]);
        assert_eq!(code, -2);
        assert!(err.starts_with("sketch-orca: run needs --job <file>\n"));
    }
}
