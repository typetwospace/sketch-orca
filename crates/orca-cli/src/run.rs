// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! A job's work after it is read and checked, as `cli/run.cpp`'s `run` does it, as far as it is
//! ported: the progress line "presets" (3), the resources and work folders
//! ([`set_up_folders`]: `environment` when the resources folder has no profiles folder,
//! `output-failed` when the work folder or its orca-data folder cannot be made), the presets
//! stage (`presets.rs`), the progress line "model" (8), and the model's file (`file-not-found`
//! when it is no file). Then every job is declined (`decline.rs`), before the model is read:
//! reading it is the first step that is not ported. A decline goes out through the crash
//! trigger `decline` (`crash.rs`).
//!
//! Orca's global folders, which `set_up_folders` also sets (resources, temporary and data), are
//! read only by the steps after the model check, so they are left out.

use std::path::Path;

use crate::common::{Failure, fail};
use crate::files::make_directories;
use crate::job::Job;
use crate::paths::{is_directory, is_file};
use crate::result::{Progress, Warnings};
use crate::{crash, decline, presets};

/// `fs::path(left) / name` as text, as Boost.Filesystem (v3) joins them: the preferred separator
/// between them unless `left` is empty or ends with a separator (or, on Windows, a drive's colon).
#[must_use]
pub fn join_path(left: &str, name: &str) -> String {
    let ends_open = match left.as_bytes().last() {
        None | Some(b'/') => true,
        Some(b'\\' | b':') => cfg!(windows),
        Some(_) => false,
    };
    let separator = if ends_open {
        ""
    } else if cfg!(windows) {
        "\\"
    } else {
        "/"
    };
    format!("{left}{separator}{name}")
}

/// `set_up_folders(job)`: the resources folder must have a profiles folder, and the work folder
/// and its orca-data folder are made.
///
/// # Errors
///
/// `environment` or `output-failed`, with the C++ messages.
pub fn set_up_folders(job: &Job) -> Result<(), Failure> {
    if !is_directory(&job.resources) || !is_directory(&join_path(&job.resources, "profiles")) {
        return Err(fail(
            "environment",
            format!(
                "the resources folder {} has no profiles folder; give the engine's resources folder",
                job.resources
            ),
        ));
    }
    for folder in [job.work_dir.clone(), join_path(&job.work_dir, "orca-data")] {
        make_directories(Path::new(&folder)).map_err(|e| {
            fail(
                "output-failed",
                format!("cannot use the work folder: {}", e.0),
            )
        })?;
    }
    Ok(())
}

/// A decline, through the crash trigger `decline` first.
fn declined(f: Failure) -> Failure {
    crash::test_point("decline");
    f
}

/// `run(job, result, warnings, progress)`, as far as it is ported.
///
/// # Errors
///
/// The failures of the folders, the presets stage and the model check, and the decline.
pub fn run(job: &Job, warnings: &Warnings, progress: &mut Progress) -> Result<(), Failure> {
    progress.line("presets", 3., "reading the presets");
    set_up_folders(job)?;
    let _settings = presets::load_settings(job, warnings).map_err(|f| {
        if f.code == "not-ported" {
            declined(f)
        } else {
            f
        }
    })?;
    progress.line("model", 8., "reading the model");
    if !is_file(&job.model_path) {
        return Err(fail(
            "file-not-found",
            format!("the model {} does not exist", job.model_path),
        ));
    }
    // Reading the model is the first step that is not ported.
    decline::check(job).map_err(declined)
}

#[cfg(test)]
mod tests {
    use super::join_path;

    #[test]
    fn paths_are_joined_as_boost_joins_them() {
        if cfg!(windows) {
            assert_eq!(join_path("C:\\res", "profiles"), "C:\\res\\profiles");
            assert_eq!(join_path("C:\\res\\", "profiles"), "C:\\res\\profiles");
            assert_eq!(join_path("C:/res/", "profiles"), "C:/res/profiles");
            assert_eq!(join_path("C:", "profiles"), "C:profiles");
            assert_eq!(join_path("C:/work", "orca-data"), "C:/work\\orca-data");
        } else {
            assert_eq!(join_path("/res", "profiles"), "/res/profiles");
            assert_eq!(join_path("/res/", "profiles"), "/res/profiles");
        }
        assert_eq!(join_path("", "profiles"), "profiles");
    }
}
