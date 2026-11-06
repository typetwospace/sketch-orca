// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! `sketch-orca-rs version` against the C++ program's (parity/goldens/front/version.json, its
//! standard output): the same fields in the same order and the same bytes, with
//! `"implementation":"rust"` added before the closing brace; the engine's version is the
//! workspace's and Orca's are orca.lock.json's.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use orca_json::{Order, parse};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// Runs the program, hidden, with `args`.
fn program(args: &[&str]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_sketch-orca-rs"));
    c.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c.output().expect("the program runs")
}

#[test]
fn version_is_the_cpp_programs_line_with_the_implementation() {
    let out = program(&["version"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stderr.is_empty());
    let golden = std::fs::read_to_string(repo().join("parity/goldens/front/version.json")).unwrap();
    let golden_version = parse(golden.as_bytes(), Order::Insertion).unwrap()["engineVersion"]
        .as_str()
        .unwrap()
        .to_string();
    // The golden was recorded from the C++ program at the engine version of its day.
    let golden = golden.replace(
        &format!("\"engineVersion\":\"{golden_version}\""),
        &format!("\"engineVersion\":\"{}\"", env!("CARGO_PKG_VERSION")),
    );
    let end = if cfg!(windows) { "}\r\n" } else { "}\n" };
    let want = golden.trim_end_matches(end).to_string() + ",\"implementation\":\"rust\"" + end;
    assert_eq!(String::from_utf8(out.stdout).unwrap(), want);
    assert_eq!(program(&["--version"]).stdout, want.as_bytes());
}

#[test]
fn orcas_version_is_the_locks() {
    let lock = std::fs::read(repo().join("orca.lock.json")).unwrap();
    let lock = parse(&lock, Order::Insertion).unwrap();
    let out = program(&["version"]);
    let v = parse(&out.stdout, Order::Insertion).unwrap();
    assert_eq!(v["orcaVersion"], lock["version"]);
    assert_eq!(v["orcaCommit"], lock["commit"]);
    assert_eq!(v["orcaTag"], lock["tag"]);
    assert_eq!(v["engine"].as_str(), Some("sketch-orca"));
    assert_eq!(v["implementation"].as_str(), Some("rust"));
}

#[test]
fn version_takes_no_options() {
    let out = program(&["version", "--out", "x"]);
    assert_eq!(out.status.code(), Some(-2));
    assert!(out.stdout.is_empty());
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(
        err.starts_with(
            "sketch-orca: version takes no options\r\n\r\nsketch-orca: a command-line slicer"
        ) || !cfg!(windows)
    );
}
