// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: cli/run.cpp's run, whose order follows CLI::run of src/OrcaSlicer.cpp,
// OrcaSlicer v2.4.2, commit 8500fcd, translated as far as the model stage and the info answer on
// 2026-11-11 (orca-info-load), and up to the front-objects boundary the same day
// (orca-info-place). Those portions are copyright the OrcaSlicer contributors, under the
// AGPL-3.0.

//! A job's work after it is read and checked, as `cli/run.cpp`'s `run` does it, in its order, as
//! far as it is ported:
//!
//! 1. The progress line "presets" (3), the resources and work folders ([`set_up_folders`]:
//!    `environment` when the resources folder has no profiles folder, `output-failed` when the
//!    work folder or its orca-data folder cannot be made; the work folder is Orca's temporary
//!    folder from then on, where the 3MF reader makes its backup folder), and the presets stage
//!    (`presets.rs`).
//! 2. The progress line "model" (8) and the model stage (`model.rs`): the model read, each object
//!    the job lists checked against the model's (`job-invalid`), and, for a layers or slice job
//!    or a job that orients its objects, each object checked for a volume to print.
//! 3. A job that orients its objects: the progress line "orient" (10), then the decline, as
//!    orientation is not ported.
//! 4. An info job is answered whole: the result's `info` (`info.rs`), and the run ends there, as in
//!    the C++.
//! 5. A layers or slice job places its objects (`place.rs`), on a copy of the presets' config:
//!    the objects' own settings and height ranges, the custom G-code per layer, the prime tower's
//!    place (in the copy's `wipe_tower_x` and `wipe_tower_y`), for a job that arranges the
//!    progress line "arrange" (12) and the arrange, and the build volume check. With
//!    `SKETCH_ORCA_ORACLE_DUMP` set, the placed objects are dumped (`oracle.rs`), at the oracle's
//!    front-objects boundary.
//! 6. Then the job is declined, as slicing is not ported, naming the families it reaches past the
//!    boundary that are not done (`decline.rs`).
//!
//! A decline goes out through the crash trigger `decline` (`crash.rs`). Orca's other global
//! folders, which `set_up_folders` also sets (resources, images, i18n, shapes, custom G-codes and
//! data), are read only by the steps after the boundary, so they are left out.

use std::path::Path;

use orca_json::Object;

use crate::common::{Failure, fail};
use crate::files::make_directories;
use crate::job::{Command, Job};
use crate::paths::is_directory;
use crate::result::{Progress, Warnings};
use crate::{crash, decline, info, model, oracle, place, presets};

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

/// `set_up_folders(job)`: the resources folder must have a profiles folder, the work folder and
/// its orca-data folder are made, and the work folder is Orca's temporary folder
/// (`set_temporary_dir`), which the 3MF reader makes its backup folder in.
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
    orca_model::utils::set_temporary_dir(&job.work_dir);
    Ok(())
}

/// A decline, through the crash trigger `decline` first.
fn declined(f: Failure) -> Failure {
    crash::test_point("decline");
    f
}

/// A failure as it is, or, for a decline (`not-ported`), through [`declined`].
fn declined_if_not_ported(f: Failure) -> Failure {
    if f.code == "not-ported" {
        declined(f)
    } else {
        f
    }
}

/// `run(job, result, warnings, progress)`, as far as it is ported; an info job's answer goes into
/// `body` as `info`.
///
/// # Errors
///
/// The failures of the folders, the presets stage, the model stage and the placement, and the
/// declines.
pub fn run(
    job: &Job,
    body: &mut Object,
    warnings: &Warnings,
    progress: &mut Progress,
) -> Result<(), Failure> {
    progress.line("presets", 3., "reading the presets");
    set_up_folders(job)?;
    let settings = presets::load_settings(job, warnings).map_err(declined_if_not_ported)?;

    progress.line("model", 8., "reading the model");
    let mut model = model::load_model(job).map_err(declined_if_not_ported)?;
    let count = model.objects.len();
    for (pos, object) in job.objects.iter().enumerate() {
        // size_t(index): a negative index is past any model.
        if !usize::try_from(object.index).is_ok_and(|i| i < count) {
            return Err(fail(
                "job-invalid",
                format!(
                    "objects[{pos}].index is {}, but the model has {count} object(s)",
                    object.index
                ),
            ));
        }
    }
    if job.command != Command::Info || job.orient {
        model::check_solid_objects(&model)?;
    }
    if job.orient {
        progress.line("orient", 10., "orienting the objects");
        // Orienting the objects is the first step on this path that is not ported.
        return Err(declined(decline::orient(job)));
    }
    if job.command == Command::Info {
        body.insert("info", info::info_json(&mut model, &settings)?);
        return Ok(());
    }

    let mut cfg = settings.config.clone();
    place::apply_object_settings(&mut model, job, &settings)?;
    let gcodes = place::set_custom_gcodes(&mut model, job, &settings)?;
    let tower = place::place_prime_tower(&mut cfg, &mut model, &settings, &gcodes)?;
    if job.arrange {
        progress.line("arrange", 12., "arranging the objects on the plate");
        place::arrange_objects(&mut model, &cfg, tower.as_ref())?;
    }
    place::check_build_volume(&mut model, &cfg)?;
    oracle::dump_front_objects(&model, &cfg, &gcodes)?;
    // Slicing comes next, and it is not ported.
    Err(declined(decline::placed(job, &model)))
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
