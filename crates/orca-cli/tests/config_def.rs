// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! `sketch-orca-rs config-def --out <file>` against the C++ program's file
//! (parity/goldens/front/config-def.json): byte for byte the same (the engine's version apart,
//! which is the workspace's), and the C++ program's refusals and exit codes.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use orca_json::{Order, parse};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

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

fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("orca-cli-config-def-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn config_def_writes_the_cpp_programs_file() {
    let dir = scratch("same");
    let out = dir.join("deeper").join("config-def.json");
    let r = program(&["config-def", "--out", out.to_str().unwrap()]);
    assert_eq!(
        r.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&r.stderr)
    );
    assert!(r.stdout.is_empty() && r.stderr.is_empty());
    let got = std::fs::read(&out).unwrap();
    let golden = std::fs::read(repo().join("parity/goldens/front/config-def.json")).unwrap();
    let golden_version = parse(&golden, Order::Insertion).unwrap()["engineVersion"]
        .as_str()
        .unwrap()
        .to_string();
    let golden = String::from_utf8(golden).unwrap().replacen(
        &format!("\"engineVersion\": \"{golden_version}\""),
        &format!("\"engineVersion\": \"{}\"", env!("CARGO_PKG_VERSION")),
        1,
    );
    if got != golden.as_bytes() {
        let at = got
            .iter()
            .zip(golden.as_bytes())
            .position(|(a, b)| a != b)
            .unwrap_or(got.len().min(golden.len()));
        let from = at.saturating_sub(120);
        panic!(
            "config-def differs at byte {at} (lengths {} and {}):\n  C++:  {:?}\n  Rust: {:?}",
            golden.len(),
            got.len(),
            &golden[from..(at + 120).min(golden.len())],
            String::from_utf8_lossy(&got[from..(at + 120).min(got.len())])
        );
    }
    assert!(!out.with_extension("json.tmp").exists());
    // --out=<file> is the same option.
    let again = dir.join("again.json");
    let r = program(&["config-def", &format!("--out={}", again.display())]);
    assert_eq!(r.status.code(), Some(0));
    assert_eq!(std::fs::read(&again).unwrap(), got);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn config_def_refuses_what_cli_main_refuses() {
    for (args, first) in [
        (
            &["config-def"][..],
            "sketch-orca: config-def needs --out <file>",
        ),
        (
            &["config-def", "--out"][..],
            "sketch-orca: --out needs a value",
        ),
        (
            &["config-def", "--out", "a", "--out", "b"][..],
            "sketch-orca: --out is given twice",
        ),
        (
            &["config-def", "--progress", "a"][..],
            "sketch-orca: unknown option --progress",
        ),
        (
            &["config-def", "a.json"][..],
            "sketch-orca: unexpected argument a.json",
        ),
    ] {
        let r = program(args);
        assert_eq!(r.status.code(), Some(-2), "{args:?}");
        assert!(r.stdout.is_empty());
        let err = String::from_utf8(r.stderr).unwrap();
        assert_eq!(err.lines().next(), Some(first), "{args:?}");
    }
}

#[test]
fn a_file_that_cannot_be_written_exits_with_output_failed() {
    let dir = scratch("blocked");
    std::fs::create_dir_all(&dir).unwrap();
    // A folder where the file's parent folder should be.
    let blocker = dir.join("file");
    std::fs::write(&blocker, b"x").unwrap();
    let out = blocker.join("config-def.json");
    let r = program(&["config-def", "--out", out.to_str().unwrap()]);
    assert_eq!(r.status.code(), Some(-13));
    let err = String::from_utf8(r.stderr).unwrap();
    assert!(
        err.starts_with(&format!(
            "sketch-orca: cannot write {}: cannot create the folder {}: ",
            out.display(),
            blocker.display()
        )),
        "{err}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
