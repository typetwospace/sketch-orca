// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The job file (`sketch-orca-job/1`), read and checked as `cli/job.cpp`'s `parse_job` does it,
//! field for field in the same order, so an invalid job gets the same failure: `job-invalid`
//! naming the field, `custom-gcode-invalid` for a print Z at or below 0 or two at one print Z, and
//! `layer-profile-invalid` (with the object's index) for a profile that does not start at z 0. An
//! optional field given as null is the same as one left out. The text is read as `nlohmann`'s
//! `ordered_json` reads it (`orca-json`, [`Order::Insertion`]).
//!
//! The job's fields past its command are kept for the steps that read them: the resources and
//! work folders, the presets, the overrides, the model's path, the objects' indexes and orient are
//! read by the presets stage and the model stage (`run.rs`, `presets.rs`, `model.rs`); until the
//! steps after them are ported, sketch-orca-rs declines every job that reaches them
//! (`decline.rs`).

// The steps that read most of these fields are not ported yet (decline.rs). The comparisons are
// cli/job.cpp's as it writes them (`!(h > 0.)`, `e < 1 || e > 1e6`), which also say what a NaN
// does.
#![allow(
    dead_code,
    clippy::neg_cmp_op_on_partial_ord,
    clippy::manual_range_contains
)]

use orca_json::{Object, Order, Value};

use crate::common::{Failure, ends_with_ci, fail, fail_object, format_number, lower};
use crate::paths::is_absolute_path;

/// `JOB_FORMAT`.
pub const JOB_FORMAT: &str = "sketch-orca-job/1";

/// Two custom G-codes closer than this are at the same print Z (Orca runs one per layer).
const Z_EPSILON: f64 = 1e-6;

/// `enum class Command`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Slice,
    Layers,
    Info,
}

/// Setting key and value as Orca writes it, in the order the job gives them.
pub type Overrides = Vec<(String, String)>;

/// `LayerHeights::Mode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerMode {
    Fixed,
    Adaptive,
    Profile,
}

/// `struct LayerHeights`.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerHeights {
    pub mode: LayerMode,
    /// Adaptive: 0 is the finest, 1 the coarsest.
    pub quality: f64,
    /// Profile: z0, h0, z1, h1, ... with z measured from the bottom of the object.
    pub profile: Vec<f64>,
}

impl Default for LayerHeights {
    fn default() -> LayerHeights {
        LayerHeights {
            mode: LayerMode::Fixed,
            quality: 0.5,
            profile: Vec::new(),
        }
    }
}

/// `struct HeightRange`: from the bottom of the object, in mm.
#[derive(Clone, Debug, PartialEq)]
pub struct HeightRange {
    pub from: f64,
    pub to: f64,
    pub overrides: Overrides,
}

/// `struct JobObject`.
#[derive(Clone, Debug, PartialEq)]
pub struct JobObject {
    pub index: i32,
    pub overrides: Overrides,
    pub layer_heights: LayerHeights,
    pub height_ranges: Vec<HeightRange>,
}

/// `struct CustomGcode`.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomGcode {
    /// The print Z of the layer this runs before.
    pub print_z: f64,
    /// pause, color, filament or custom.
    pub kind: String,
    /// 1-based; 0 when not given.
    pub extruder: i32,
    /// #rrggbb, lower case; empty when not given.
    pub color: String,
    pub gcode: String,
    pub note: String,
}

/// `struct Job`.
#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub command: Command,
    pub command_name: String,
    pub resources: String,
    pub work_dir: String,
    pub model_path: String,
    pub machine: String,
    pub process: String,
    pub filaments: Vec<String>,
    pub overrides: Overrides,
    pub objects: Vec<JobObject>,
    /// Sorted by print Z.
    pub custom_gcodes: Vec<CustomGcode>,
    pub arrange: bool,
    pub orient: bool,
    pub out_gcode: String,
    /// Empty when no .gcode.3mf is asked for.
    pub out_bambu3mf: String,
    pub stats: bool,
}

type Checked<T> = Result<T, Failure>;

/// `bad(message)`: job-invalid.
fn bad<T>(message: impl Into<String>) -> Checked<T> {
    Err(fail("job-invalid", message))
}

/// `quoted(v)`: the value as JSON on one line.
fn quoted(v: &Value) -> String {
    v.dump(None)
}

const NULL: &Value = &Value::Null;

/// `member(obj, key)`: the member, or null when it is not there (or `obj` is no object).
fn member<'a>(obj: &'a Value, key: &str) -> &'a Value {
    obj.get(key).unwrap_or(NULL)
}

/// `has(obj, key)`: there and not null.
fn has(obj: &Value, key: &str) -> bool {
    obj.get(key).is_some_and(|v| !v.is_null())
}

/// `record(v, field)`: `v` when it is an object.
fn record<'a>(v: &'a Value, field: &str) -> Checked<&'a Object> {
    match v.as_object() {
        Some(o) => Ok(o),
        None => bad(format!("{field} must be an object")),
    }
}

/// `text(v, field)`.
fn text(v: &Value, field: &str) -> Checked<String> {
    match v.as_str() {
        Some(s) => Ok(s.to_string()),
        None => bad(format!("{field} must be text")),
    }
}

/// `boolean(obj, key, field, fallback)`: an optional field given as null is the fallback.
fn boolean(obj: &Value, key: &str, field: &str, fallback: bool) -> Checked<bool> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(fallback),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => bad(format!("{field} must be true or false")),
    }
}

/// `number(v, field)`: a finite number.
fn number(v: &Value, field: &str) -> Checked<f64> {
    match v.as_f64() {
        Some(d) if d.is_finite() => Ok(d),
        _ => bad(format!("{field} must be a number")),
    }
}

/// `is_whole(d)`: `std::floor(d) == d`.
fn is_whole(d: f64) -> bool {
    d.floor() == d
}

/// `abs_path(v, field, extensions)`: absolute text, ending with one of `extensions` (letter case
/// aside) when any are given.
fn abs_path(v: &Value, field: &str, extensions: &[&str]) -> Checked<String> {
    let p = text(v, field)?;
    if p.is_empty() || !is_absolute_path(&p) {
        return bad(format!(
            "{field} must be an absolute path, got {}",
            quoted(v)
        ));
    }
    if !extensions.is_empty() && !extensions.iter().any(|e| ends_with_ci(&p, e)) {
        return bad(format!(
            "{field} must end with {}, got {}",
            extensions.join(" or "),
            quoted(v)
        ));
    }
    Ok(p)
}

/// `std::regex_match(k, "^[A-Za-z][A-Za-z0-9_]*$")`.
fn is_setting_key(k: &str) -> bool {
    let b = k.as_bytes();
    !b.is_empty()
        && b[0].is_ascii_alphabetic()
        && b[1..]
            .iter()
            .all(|&c| c.is_ascii_alphanumeric() || c == b'_')
}

/// `std::regex_match(c, "^#[0-9a-fA-F]{6}$")`.
fn is_color(c: &str) -> bool {
    let b = c.as_bytes();
    b.len() == 7 && b[0] == b'#' && b[1..].iter().all(u8::is_ascii_hexdigit)
}

/// `std::isspace` in the "C" locale: space, tab, line feed, vertical tab, form feed, carriage return.
pub fn is_c_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// The keys of the preset files, refused as overrides in any letter case.
const RESERVED: &[&str] = &["compatible_printers", "from", "inherits", "name", "type"];

/// `overrides(v, field)`: values as Orca writes them; numbers and true/false become that text.
fn overrides(v: &Value, field: &str) -> Checked<Overrides> {
    let mut out = Overrides::new();
    if v.is_null() {
        return Ok(out);
    }
    let o = record(v, field)?;
    for (k, raw) in o.iter() {
        if !is_setting_key(k) {
            return bad(format!("{field}: \"{k}\" is not an Orca setting key"));
        }
        if RESERVED.contains(&lower(k).as_str()) {
            return bad(format!(
                "{field}: \"{k}\" belongs to the preset files, not to overrides"
            ));
        }
        let value = match raw {
            Value::String(s) => s.clone(),
            Value::Bool(b) => if *b { "1" } else { "0" }.to_string(),
            _ if raw.as_f64().is_some_and(f64::is_finite) => raw.dump(None),
            _ => {
                return bad(format!(
                    "{field}.{k} must be text (the value as Orca writes it)"
                ));
            }
        };
        out.push((k.clone(), value));
    }
    Ok(out)
}

/// `layer_heights(v, field, object_index)`.
fn layer_heights(v: &Value, field: &str, object_index: i32) -> Checked<LayerHeights> {
    let mut lh = LayerHeights::default();
    if v.is_null() {
        return Ok(lh);
    }
    record(v, field)?;
    let mode = member(v, "mode");
    match mode.as_str() {
        Some("fixed") => Ok(lh),
        Some("adaptive") => {
            lh.mode = LayerMode::Adaptive;
            lh.quality = if has(v, "quality") {
                number(member(v, "quality"), &format!("{field}.quality"))?
            } else {
                0.5
            };
            if lh.quality < 0. || lh.quality > 1. {
                return bad(format!(
                    "{field}.quality must be between 0 (finest) and 1 (coarsest), got {}",
                    format_number(lh.quality)
                ));
            }
            Ok(lh)
        }
        Some("profile") => {
            lh.mode = LayerMode::Profile;
            let Some(arr) = member(v, "profile").as_array() else {
                return bad(format!(
                    "{field}.profile must be a list [z0, h0, z1, h1, ...]"
                ));
            };
            for (i, x) in arr.iter().enumerate() {
                lh.profile
                    .push(number(x, &format!("{field}.profile[{i}]"))?);
            }
            let p = &lh.profile;
            if p.len() < 4 || p.len() % 2 != 0 {
                return bad(format!(
                    "{field}.profile must hold pairs of z and height, at least two"
                ));
            }
            if p[0] != 0. {
                return Err(fail_object(
                    "layer-profile-invalid",
                    format!("{field}.profile must start at z 0, the bottom of the object"),
                    object_index,
                ));
            }
            for i in (0..p.len()).step_by(2) {
                if i > 0 && p[i] < p[i - 2] {
                    return bad(format!(
                        "{field}.profile: z {} comes after z {}",
                        format_number(p[i]),
                        format_number(p[i - 2])
                    ));
                }
                if !(p[i + 1] > 0.) {
                    return bad(format!(
                        "{field}.profile: the height at z {} must be above 0",
                        format_number(p[i])
                    ));
                }
            }
            Ok(lh)
        }
        _ => bad(format!(
            "{field}.mode must be fixed, adaptive or profile, got {}",
            quoted(mode)
        )),
    }
}

/// `objects(v)`.
fn objects(v: &Value) -> Checked<Vec<JobObject>> {
    let mut out = Vec::new();
    if v.is_null() {
        return Ok(out);
    }
    let Some(list) = v.as_array() else {
        return bad("objects must be a list");
    };
    // std::set<int>: whether an index was seen.
    let mut seen = std::collections::BTreeSet::new();
    for (i, item) in list.iter().enumerate() {
        let f = format!("objects[{i}]");
        record(item, &f)?;
        let index = number(member(item, "index"), &format!("{f}.index"))?;
        if !is_whole(index) || index < 0. || index > 1e9 {
            return bad(format!("{f}.index must be a whole number from 0"));
        }
        // A whole number from 0 to 1e9: exact as an int.
        #[allow(clippy::cast_possible_truncation)]
        let index = index as i32;
        if !seen.insert(index) {
            return bad(format!("{f}.index {index} is listed twice"));
        }
        let mut height_ranges = Vec::new();
        let ranges = member(item, "heightRanges");
        if !ranges.is_null() {
            let Some(ranges) = ranges.as_array() else {
                return bad(format!("{f}.heightRanges must be a list"));
            };
            for (j, r) in ranges.iter().enumerate() {
                let rf = format!("{f}.heightRanges[{j}]");
                record(r, &rf)?;
                let from = number(member(r, "from"), &format!("{rf}.from"))?;
                let to = number(member(r, "to"), &format!("{rf}.to"))?;
                if from < 0. || !(to > from) {
                    return bad(format!(
                        "{rf} must have 0 <= from < to, got {}..{}",
                        format_number(from),
                        format_number(to)
                    ));
                }
                let overrides = overrides(member(r, "overrides"), &format!("{rf}.overrides"))?;
                height_ranges.push(HeightRange {
                    from,
                    to,
                    overrides,
                });
            }
            for j in 1..height_ranges.len() {
                let (prev, cur) = (&height_ranges[j - 1], &height_ranges[j]);
                if cur.from < prev.to {
                    return bad(format!(
                        "{f}.heightRanges[{j}] starts at {}, inside the range before it (to {})",
                        format_number(cur.from),
                        format_number(prev.to)
                    ));
                }
            }
        }
        let overrides = overrides(member(item, "overrides"), &format!("{f}.overrides"))?;
        let layer_heights = layer_heights(
            member(item, "layerHeights"),
            &format!("{f}.layerHeights"),
            index,
        )?;
        out.push(JobObject {
            index,
            overrides,
            layer_heights,
            height_ranges,
        });
    }
    Ok(out)
}

/// `custom_gcodes(v)`: checked, then stable-sorted by print Z.
fn custom_gcodes(v: &Value) -> Checked<Vec<CustomGcode>> {
    let mut out = Vec::new();
    if v.is_null() {
        return Ok(out);
    }
    let Some(list) = v.as_array() else {
        return bad("customGcodes must be a list");
    };
    for (i, o) in list.iter().enumerate() {
        let f = format!("customGcodes[{i}]");
        record(o, &f)?;
        let print_z = number(member(o, "printZ"), &format!("{f}.printZ"))?;
        if !(print_z > 0.) {
            return Err(fail(
                "custom-gcode-invalid",
                format!("{f}.printZ must be above 0: there is no layer below the first"),
            ));
        }
        let kind = member(o, "type");
        let kind = match kind.as_str() {
            Some(k @ ("pause" | "color" | "filament" | "custom")) => k.to_string(),
            _ => {
                return bad(format!(
                    "{f}.type must be pause, color, filament, custom, got {}",
                    quoted(kind)
                ));
            }
        };
        let mut extruder = 0;
        if has(o, "extruder") {
            let e = number(member(o, "extruder"), &format!("{f}.extruder"))?;
            if !is_whole(e) || e < 1. || e > 1e6 {
                return bad(format!("{f}.extruder must be a whole number from 1"));
            }
            // A whole number from 1 to 1e6: exact as an int.
            #[allow(clippy::cast_possible_truncation)]
            {
                extruder = e as i32;
            }
        } else if kind == "filament" {
            return bad(format!(
                "{f}: a filament change needs the extruder to switch to"
            ));
        }
        let mut color = String::new();
        if has(o, "color") {
            color = text(member(o, "color"), &format!("{f}.color"))?;
            if !is_color(&color) {
                return bad(format!(
                    "{f}.color must be #rrggbb, got {}",
                    quoted(member(o, "color"))
                ));
            }
            color = lower(&color);
        } else if kind == "color" {
            return bad(format!("{f}: a color change needs the color"));
        }
        let mut gcode = String::new();
        if has(o, "gcode") {
            gcode = text(member(o, "gcode"), &format!("{f}.gcode"))?;
        }
        if kind == "custom" && gcode.bytes().all(is_c_space) {
            return bad(format!("{f}: a custom G-code needs its gcode"));
        }
        let mut note = String::new();
        if has(o, "note") {
            note = text(member(o, "note"), &format!("{f}.note"))?;
        }
        out.push(CustomGcode {
            print_z,
            kind,
            extruder,
            color,
            gcode,
            note,
        });
    }
    orca_stl::stable_sort_by(&mut out, |a, b| a.print_z < b.print_z);
    for i in 1..out.len() {
        if out[i].print_z - out[i - 1].print_z < Z_EPSILON {
            return Err(fail(
                "custom-gcode-invalid",
                format!(
                    "two custom G-codes at print Z {}; Orca runs one per layer",
                    format_number(out[i].print_z)
                ),
            ));
        }
    }
    Ok(out)
}

/// `parse_job(text)`: the job, or the failure of the first field that is wrong.
///
/// # Errors
///
/// `job-invalid` naming the field, or the contract's own code (`custom-gcode-invalid`,
/// `layer-profile-invalid`), with the C++ program's message.
pub fn parse_job(text_in: &[u8]) -> Result<Job, Failure> {
    let root = match orca_json::parse(text_in, Order::Insertion) {
        Ok(v) => v,
        Err(e) => return bad(format!("the job is not JSON ({})", e.message)),
    };
    record(&root, "job")?;
    let format = member(&root, "format");
    if format.as_str() != Some(JOB_FORMAT) {
        return bad(format!(
            "format must be \"{JOB_FORMAT}\", got {}",
            quoted(format)
        ));
    }

    let command_value = member(&root, "command");
    let command = match command_value.as_str() {
        Some("slice") => Command::Slice,
        Some("layers") => Command::Layers,
        Some("info") => Command::Info,
        _ => {
            return bad(format!(
                "command must be slice, layers, info, got {}",
                quoted(command_value)
            ));
        }
    };
    let command_name = command_value.as_str().unwrap_or_default().to_string();

    let model = member(&root, "model");
    record(model, "model")?;
    let presets = member(&root, "presets");
    record(presets, "presets")?;
    let fils = member(presets, "filaments");
    let fils = match fils.as_array() {
        Some(a) if !a.is_empty() => a,
        _ => return bad("presets.filaments must list at least one filament preset"),
    };

    let mut out_gcode = String::new();
    let mut out_bambu3mf = String::new();
    let stats;
    let output = member(&root, "output");
    if output.is_null() {
        if command == Command::Slice {
            return bad("a slice job needs output.gcode");
        }
        stats = false;
    } else {
        record(output, "output")?;
        let gcode = member(output, "gcode");
        if command == Command::Slice || (!gcode.is_null() && gcode.as_str() != Some("")) {
            out_gcode = abs_path(gcode, "output.gcode", &[".gcode"])?;
        }
        let b3mf = member(output, "bambu3mf");
        if !b3mf.is_null() {
            out_bambu3mf = abs_path(b3mf, "output.bambu3mf", &[".3mf"])?;
        }
        stats = boolean(output, "stats", "output.stats", true)?;
    }

    let resources = abs_path(member(&root, "resources"), "resources", &[])?;
    let work_dir = abs_path(member(&root, "workDir"), "workDir", &[])?;
    let model_path = abs_path(
        member(model, "path"),
        "model.path",
        &[".3mf", ".stl", ".obj"],
    )?;
    let machine = abs_path(member(presets, "machine"), "presets.machine", &[".json"])?;
    let process = abs_path(member(presets, "process"), "presets.process", &[".json"])?;
    let mut filaments = Vec::with_capacity(fils.len());
    for (i, f) in fils.iter().enumerate() {
        filaments.push(abs_path(f, &format!("presets.filaments[{i}]"), &[".json"])?);
    }
    let overrides = overrides(member(&root, "overrides"), "overrides")?;
    let objects = objects(member(&root, "objects"))?;
    let custom_gcodes = custom_gcodes(member(&root, "customGcodes"))?;
    let arrange = boolean(&root, "arrange", "arrange", true)?;
    let orient = boolean(&root, "orient", "orient", false)?;
    Ok(Job {
        command,
        command_name,
        resources,
        work_dir,
        model_path,
        machine,
        process,
        filaments,
        overrides,
        objects,
        custom_gcodes,
        arrange,
        orient,
        out_gcode,
        out_bambu3mf,
        stats,
    })
}

#[cfg(test)]
mod tests {
    use super::{Command, LayerMode, parse_job};

    fn root() -> &'static str {
        if cfg!(windows) { "C:\\\\j" } else { "/j" }
    }

    fn job(extra: &str) -> String {
        let r = root();
        format!(
            r#"{{"format":"sketch-orca-job/1","command":"slice","resources":"{r}\\res","workDir":"{r}\\work","model":{{"path":"{r}\\m.stl"}},"presets":{{"machine":"{r}\\a.json","process":"{r}\\b.json","filaments":["{r}\\c.json"]}},"output":{{"gcode":"{r}\\o.gcode"}}{extra}}}"#
        )
    }

    fn refusal(text: &str) -> (String, String, i32, i32) {
        let f = parse_job(text.as_bytes()).unwrap_err();
        (f.code, f.message, f.exit_code, f.object_index)
    }

    #[test]
    fn a_valid_job_reads_every_field() {
        let j = parse_job(
            job(r##","overrides":{"layer_height":0.2,"enable_support":true,"wall_loops":"3"},"objects":[{"index":1,"layerHeights":{"mode":"adaptive"}}],"customGcodes":[{"printZ":2,"type":"pause"},{"printZ":1,"type":"color","color":"#AABBCC"}],"orient":true"##)
                .as_bytes(),
        )
        .unwrap();
        assert_eq!(j.command, Command::Slice);
        assert_eq!(j.command_name, "slice");
        assert_eq!(
            j.overrides,
            vec![
                ("layer_height".to_string(), "0.2".to_string()),
                ("enable_support".to_string(), "1".to_string()),
                ("wall_loops".to_string(), "3".to_string()),
            ]
        );
        assert_eq!(j.objects[0].index, 1);
        assert_eq!(j.objects[0].layer_heights.mode, LayerMode::Adaptive);
        assert_eq!(j.custom_gcodes[0].print_z, 1.0);
        assert_eq!(j.custom_gcodes[0].color, "#aabbcc");
        assert!(j.arrange && j.orient && j.stats);
    }

    #[test]
    fn refusals_name_the_field_with_the_cpp_message() {
        assert_eq!(refusal("[1]").1, "job must be an object");
        assert_eq!(
            refusal("{").1,
            "the job is not JSON ([json.exception.parse_error.101] parse error at line 1, column 2: syntax error while parsing object key - unexpected end of input; expected string literal)"
        );
        assert_eq!(
            refusal(r#"{"format":"x"}"#).1,
            "format must be \"sketch-orca-job/1\", got \"x\""
        );
        assert_eq!(
            refusal(r#"{"format":"sketch-orca-job/1"}"#).1,
            "command must be slice, layers, info, got null"
        );
        assert_eq!(
            refusal(&job(r#","overrides":{"Inherits":"x"}"#)).1,
            "overrides: \"Inherits\" belongs to the preset files, not to overrides"
        );
        assert_eq!(
            refusal(&job(r#","overrides":{"a-b":"x"}"#)).1,
            "overrides: \"a-b\" is not an Orca setting key"
        );
        assert_eq!(
            refusal(&job(r#","overrides":{"a":null}"#)).1,
            "overrides.a must be text (the value as Orca writes it)"
        );
        assert_eq!(
            refusal(&job(r#","customGcodes":[{"printZ":0,"type":"pause"}]"#)),
            (
                "custom-gcode-invalid".to_string(),
                "customGcodes[0].printZ must be above 0: there is no layer below the first"
                    .to_string(),
                -2,
                -1
            )
        );
        assert_eq!(
            refusal(&job(r#","customGcodes":[{"printZ":1,"type":"pause"},{"printZ":1.0000001,"type":"pause"}]"#)).1,
            "two custom G-codes at print Z 1.0000001; Orca runs one per layer"
        );
        assert_eq!(
            refusal(&job(
                r#","objects":[{"index":3,"layerHeights":{"mode":"profile","profile":[0.2,0.2,1,0.2]}}]"#
            )),
            (
                "layer-profile-invalid".to_string(),
                "objects[0].layerHeights.profile must start at z 0, the bottom of the object"
                    .to_string(),
                -2,
                3
            )
        );
        assert_eq!(
            refusal(&job(
                r#","objects":[{"index":0,"heightRanges":[{"from":0,"to":2},{"from":1,"to":3}]}]"#
            ))
            .1,
            "objects[0].heightRanges[1] starts at 1, inside the range before it (to 2)"
        );
        assert_eq!(
            refusal(&job(
                r#","customGcodes":[{"printZ":1,"type":"custom","gcode":" \u000b\t"}]"#
            ))
            .1,
            "customGcodes[0]: a custom G-code needs its gcode"
        );
    }

    #[test]
    fn paths_must_be_absolute_with_their_extension() {
        let rel = job("").replace(&format!("{}\\\\m.stl", root()), "m.stl");
        assert_eq!(
            refusal(&rel).1,
            "model.path must be an absolute path, got \"m.stl\""
        );
        let ext = job("").replace("m.stl", "m.step");
        assert!(
            refusal(&ext)
                .1
                .starts_with("model.path must end with .3mf or .stl or .obj, got ")
        );
    }
}
