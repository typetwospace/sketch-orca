// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.
//
// Carries OrcaSlicer code: cli/run.cpp's load_model, which reads the model as CLI::run of
// src/OrcaSlicer.cpp does (Model::read_from_file with LoadModel | AddDefaultInstances, then
// ensure_on_bed for each object), OrcaSlicer v2.4.2, commit 8500fcd, translated on 2026-11-11
// (orca-info-load). Those portions are copyright the OrcaSlicer contributors, under the AGPL-3.0.

//! The model stage, as `cli/run.cpp` does it:
//!
//! - [`load_model`]: the model's file must be a file (`file-not-found`). Its path is made
//!   preferred ([`make_preferred`]: Orca's readers name an object after the text behind the last
//!   native separator, `\` on Windows, so a path with `/` would name it, and label it, by the
//!   whole path), and `Model::read_from_file` reads it as the stock command line asks
//!   (`LoadModel | AddDefaultInstances`, orca-model). A reader's error is `model-unreadable`,
//!   "cannot read the model <path>: <the reader's text>" ("unknown error" for an empty text), and
//!   so is a model of no object, which no file reaches: `read_from_file` refuses one first ("The
//!   supplied file couldn't be read because it's empty"). The job's presets decide every setting
//!   (sketch-orca, 2026-11-01), so each object's and part's settings are dropped but the filament
//!   it prints with (`extruder`), as are each object's height ranges and layer heights; the plate
//!   is the first, and each object is put on the bed (`ensure_on_bed`).
//! - [`check_solid_objects`]: every part Orca prints must enclose a volume. A part of fewer than
//!   four triangles, a flat part (no side longer than `EPSILON`) and an object of modifiers only
//!   are refused, `model-unreadable` naming the object ([`object_label`]).
//!
//! What the C++ does that has nothing to do here: it releases the 3MF's plates and presets and
//! clears the model's custom G-code per plate, which orca-model's reader does not keep. Two answers
//! are this program's own: a file that needs a reader orca-model has not translated
//! (`ReadError::NotPorted`; a job's model is a .3mf, .stl or .obj file, so no job reaches one) is
//! declined naming model-load (`decline.rs`), and the CR box, an ASCII STL whose `solid` line
//! passes 255 bytes, where the C++ engine dies (`parity/CORPUS.md`, "An engine fault"), is
//! `model-unreadable` with orca-model's reason (`ReadError::OverlongSolidLine`).

use orca_fmt::printf::{Arg, snprintf};
use orca_geom::point::EPSILON;
use orca_model::config::ModelConfig;
use orca_model::model::Model;
use orca_model::read::{ReadError, load_strategy, read_from_file};

use crate::common::{Failure, fail, fail_object};
use crate::decline;
use crate::job::Job;
use crate::paths::is_file;

/// `boost::filesystem::path(p).make_preferred().string()`: on Windows each `/` is made `\` (the
/// path's round trip through UTF-16 leaves UTF-8 text as it is); elsewhere the path as it is.
#[must_use]
pub fn make_preferred(p: &str) -> String {
    if cfg!(windows) {
        p.replace('/', "\\")
    } else {
        p.to_string()
    }
}

/// An object or part's settings without those the job's presets decide: only `extruder` stays.
fn keep_filament_only(config: &mut ModelConfig) {
    for key in config.keys() {
        if key != "extruder" {
            config.erase(&key);
        }
    }
}

/// `load_model(job)`: the model as the run reads it.
///
/// # Errors
///
/// `file-not-found` when the model is no file, `model-unreadable` when it cannot be read, and the
/// decline of a reader that is not translated (`not-ported`).
pub fn load_model(job: &Job) -> Result<Model, Failure> {
    if !is_file(&job.model_path) {
        return Err(fail(
            "file-not-found",
            format!("the model {} does not exist", job.model_path),
        ));
    }
    let path = make_preferred(&job.model_path);
    let options = load_strategy::LOAD_MODEL | load_strategy::ADD_DEFAULT_INSTANCES;
    let mut model = match read_from_file(&path, options) {
        Ok(model) => model,
        Err(ReadError::NotPorted(_)) => return Err(decline::model_reader(job)),
        Err(e) => {
            let mut error = e.what();
            if error.is_empty() {
                error = "unknown error".to_string();
            }
            return Err(fail(
                "model-unreadable",
                format!("cannot read the model {}: {error}", job.model_path),
            ));
        }
    };
    if model.objects.is_empty() {
        return Err(fail(
            "model-unreadable",
            format!("the model {} has no objects", job.model_path),
        ));
    }
    for mo in &mut model.objects {
        keep_filament_only(&mut mo.config);
        mo.layer_config_ranges.clear();
        mo.layer_height_profile.clear();
        for v in &mut mo.volumes {
            keep_filament_only(&mut v.config);
        }
    }
    model.curr_plate_index = 0;
    for mo in &mut model.objects {
        mo.ensure_on_bed(false);
    }
    Ok(model)
}

/// `object_label(model, index)`: "object <index>", with the object's name in brackets when it has
/// one; "an object" for an index the model does not have.
#[must_use]
pub fn object_label(model: &Model, index: i32) -> String {
    let object = usize::try_from(index)
        .ok()
        .and_then(|i| model.objects.get(i));
    match object {
        None => "an object".to_string(),
        Some(mo) if mo.name.is_empty() => format!("object {index}"),
        Some(mo) => format!("object {index} ({})", mo.name),
    }
}

/// `mm(v)`: `v` as `%.3f` writes it into a 64-byte buffer, its trailing zeros and then a trailing
/// point dropped.
#[must_use]
pub fn mm(v: f64) -> String {
    let mut buf = [0u8; 64];
    let _ = snprintf(&mut buf, b"%.3f", &[Arg::Double(v)]);
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    let mut s = String::from_utf8_lossy(&buf[..end]).into_owned();
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    s
}

/// `check_solid_objects(model)`: every part Orca prints encloses a volume. A part with fewer than
/// two triangles gets no convex hull (ModelVolume computes one from two triangles up), which the
/// print volume check and arrange read without a check, and a flat part has an empty hull, which
/// the print volume check skips, so its object reads as outside the bed.
///
/// # Errors
///
/// `model-unreadable`, naming the first object with such a part, or with modifiers only.
pub fn check_solid_objects(model: &Model) -> Result<(), Failure> {
    for (oi, mo) in model.objects.iter().enumerate() {
        let index = oi as i32;
        let mut any_part = false;
        for v in &mo.volumes {
            if !v.is_model_part() {
                continue;
            }
            any_part = true;
            let facets = v.mesh().facets_count();
            let size = v.mesh().bounding_box().size();
            let part = if mo.volumes.len() > 1 {
                format!("its part \"{}\"", v.name)
            } else {
                "its mesh".to_string()
            };
            let what = format!(
                "{} has no volume to print: {part}",
                object_label(model, index)
            );
            if facets < 4 {
                return Err(fail_object(
                    "model-unreadable",
                    format!("{what} has {facets} triangle(s); a closed surface has at least four"),
                    index,
                ));
            }
            // The C++'s test as it writes it, which refuses a NaN side too.
            #[allow(clippy::neg_cmp_op_on_partial_ord)]
            let flat = !(size.min_coeff() > EPSILON);
            if flat {
                return Err(fail_object(
                    "model-unreadable",
                    format!(
                        "{what} is flat ({} x {} x {} mm)",
                        mm(size.x()),
                        mm(size.y()),
                        mm(size.z())
                    ),
                    index,
                ));
            }
        }
        if !any_part {
            return Err(fail_object(
                "model-unreadable",
                format!(
                    "{} has no part to print, only modifiers",
                    object_label(model, index)
                ),
                index,
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{make_preferred, mm};

    #[test]
    fn a_length_is_written_with_three_decimals_and_no_trailing_zeros() {
        assert_eq!(mm(20.0), "20");
        assert_eq!(mm(0.0), "0");
        assert_eq!(mm(-0.0), "-0");
        assert_eq!(mm(0.5), "0.5");
        assert_eq!(mm(12.3456), "12.346");
        assert_eq!(mm(100.0), "100");
        assert_eq!(mm(0.0004), "0");
        assert_eq!(mm(f64::INFINITY), "inf");
        // %.3f of 1e100 is 105 characters: the buffer keeps the first 63, all digits.
        let long = mm(1e100);
        assert!(
            long.len() <= 63 && long.starts_with("1000000000000000015902"),
            "{long}"
        );
    }

    #[test]
    fn a_path_is_made_preferred_as_boost_makes_it() {
        if cfg!(windows) {
            assert_eq!(make_preferred("C:/a/b\\c.stl"), "C:\\a\\b\\c.stl");
        } else {
            assert_eq!(make_preferred("/a/b\\c.stl"), "/a/b\\c.stl");
        }
        assert_eq!(make_preferred("B\u{fc}chse.3mf"), "B\u{fc}chse.3mf");
    }
}
