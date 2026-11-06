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
//! For now it does what `cli/main.cpp` does with no command: it prints the usage after the
//! message "no command" on stderr and exits with -2 (invalid parameters). The commands themselves
//! (`version`, `config-def`, `run`) come with the tasks that follow.

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

/// Runs the command line `args` (the program's name first) and returns the exit code; what the
/// program writes goes to `err`.
fn run(args: &[String], err: &mut dyn Write) -> i32 {
    let message = match args.get(1) {
        None => "no command".to_string(),
        Some(command) => format!("unknown command {command}"),
    };
    write_text(err, &usage_error_text(&message));
    EXIT_INVALID_PARAMS
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
    let code = run(&args, &mut std::io::stderr());
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_with(args: &[&str]) -> (i32, String) {
        let args: Vec<String> = args.iter().map(|a| (*a).to_string()).collect();
        let mut err = Vec::new();
        let code = run(&args, &mut err);
        (code, String::from_utf8(err).unwrap().replace("\r\n", "\n"))
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
    fn a_command_not_yet_here_is_a_usage_error_naming_it() {
        let (code, err) = run_with(&["sketch-orca-rs", "version"]);
        assert_eq!(code, -2);
        assert!(err.starts_with("sketch-orca: unknown command version\n\n"));
    }
}
