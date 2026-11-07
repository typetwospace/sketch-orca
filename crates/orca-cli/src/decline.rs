// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The decline: sketch-orca-rs does only what the parity harness has certified it to do the C++
//! engine's way. A job is checked once the steps this program does are behind it (`run.rs`: the
//! presets stage and the model's file check), against `parity/parity.json` as this program was
//! built with it: a job that reaches a family (a step of the engine, as the harness names them)
//! that this program has no code for, or that `parity certify` has not certified, gets the result
//! code `not-ported`, exit code -8 (the stock command line's `CLI_UNSUPPORTED_OPERATION`, as
//! `unsupported`), and `missing`, those families in parity.json's order. It is refused before any
//! output file is written: the result, the progress file, and the work folder and its orca-data
//! folder (which the C++ engine makes before the presets, as this program does) are the run's
//! only files. A job whose presets have a compatibility condition Orca works out reaches the
//! family preset-conditions ([`preset_families`], as corpus.ts's `presetFamilies` finds it); the
//! presets stage works the condition out (`presets.rs`, orca-placeholder), and one the placeholder
//! parser does not take is declined there, naming preset-conditions besides the families the job
//! reaches ([`unsupported_condition`]).
//!
//! The families a job reaches are those `scripts/lib/parity/corpus.ts` gives it (`familiesFor`).
//! Some every job of a command reaches ([`command_families`]): model-load and placement for every
//! command, info for info, slice-mesh for layers and slice, and for slice the internal concentric
//! infill, the print steps every slice runs, and the G-code export and its processor. The others
//! a slice's settings turn on ([`setting_families`]): the wall generator, the infill pattern of
//! each surface, supports, raft, ironing, fuzzy skin, the prime tower and Z contouring. The prime
//! tower is on, as Orca's `Print::has_wipe_tower` has it, with two filaments or more, with smooth
//! timelapse, or with clumping detection (`enable_wrapping_detection`) and an exclusion area of
//! more than two points (`wrapping_exclude_area`). The settings are read as corpus.ts's
//! `settingLookup` reads them: the job's override, else the process, machine and filament presets
//! in that order (the job's flattened preset files; an array's first value), else Orca's default
//! (orca-config). A list of points is counted as corpus.ts's `settingPoints` counts it
//! ([`comma_pieces`]): an override's text or a preset's string in pieces at its commas, as Orca's
//! reader splits it, a preset's array by its length, and Orca's default (no points). Each
//! object's overrides (over the job's), and each height range's (over its object's), are read as
//! well and their families added, since they can turn a family on for one object. A preset file
//! that cannot be read, or a value that is not text, leaves the settings unknown, and the job then
//! reaches every family a setting can turn on ([`unsettled_families`]), so a job is never let
//! through for a family it might reach. The front end (version, config-def and intake) is decided
//! before this check, so it is never missing; for now every info, layers and slice job that gets
//! past the presets and the model's file is declined. `parity certify` holds the decline to the
//! corpus: a declined job must name every family the corpus lists for it that parity.json does not
//! certify, and must have reached a step this program does not do in the C++ run
//! (`scripts/lib/parity/certify.ts`).

use orca_json::{Order, Value};

use crate::common::Failure;
use crate::files::read_file;
use crate::job::{Command, Job, Overrides};

/// parity/parity.json as this program was built with it.
const PARITY_JSON: &str = include_str!("../../../parity/parity.json");

/// The families this program has the code for. A family is done only when it is here and
/// parity.json certifies it.
pub const PORTED: &[&str] = &["version", "config-def", "intake", "preset-conditions"];

/// The families a setting can turn on besides the infill patterns (corpus.ts `familiesFor`), in
/// parity.json's order.
const SETTING_FAMILIES: &[&str] = &[
    "perimeters-classic",
    "perimeters-arachne",
    "supports",
    "tree-supports",
    "raft",
    "ironing",
    "fuzzy-skin",
    "wipe-tower",
    "step-contouring",
    "step-simplify-support-path",
];

/// The families every job of a command reaches, whatever its settings.
#[must_use]
pub fn command_families(command: Command) -> &'static [&'static str] {
    match command {
        Command::Info => &["model-load", "placement", "info"],
        Command::Layers => &["model-load", "placement", "slice-mesh"],
        Command::Slice => &[
            "model-load",
            "placement",
            "slice-mesh",
            "infill-concentricinternal",
            "gcode-export",
            "gcode-processor",
            "step-estimate-curled-extrusions",
            "step-prepare-infill",
            "step-simplify-path",
            "step-detect-overhangs-for-lift",
            "step-simplify-wall",
            "step-simplify-infill",
            "step-skirt-brim",
            "step-conflict-check",
        ],
    }
}

/// parity.json's families, in its order, with whether each is certified.
#[must_use]
pub fn families() -> Vec<(String, bool)> {
    let doc = orca_json::parse(PARITY_JSON.as_bytes(), Order::Insertion)
        .expect("parity/parity.json is JSON");
    doc.get("families")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|f| {
                    let name = f.get("name")?.as_str()?.to_string();
                    let certified = f.get("certified").and_then(Value::as_bool) == Some(true);
                    Some((name, certified))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A setting a job's settings cannot settle: a preset value that is not text.
#[derive(Debug, PartialEq, Eq)]
pub struct Unknown;

/// A setting as a job sees it, or `None` where corpus.ts's lookup gives `undefined`.
pub type Setting = Result<Option<String>, Unknown>;

/// The infill pattern families (`infill-<pattern>`, corpus.ts `INFILL_PATTERNS`), from parity.json.
fn infill_families(all: &[(String, bool)]) -> Vec<String> {
    all.iter()
        .filter(|(n, _)| n.starts_with("infill-"))
        .map(|(n, _)| n.clone())
        .collect()
}

/// Whether `text` reads as a number above zero, or does not read as a number at all (which
/// leaves the family on); `strtod` reads it, as the C runtime reads Orca's numbers.
fn above_zero_or_unread(text: &str) -> bool {
    let r = orca_fmt::parse::strtod(text.as_bytes());
    r.end == 0 || r.value.is_nan() || r.value > 0.0
}

/// How many pieces Orca's reader of a list of points (`ConfigOptionPoints::deserialize`, a
/// `std::getline` at each comma) splits `text` into: none for empty text, and none after a closing
/// comma (corpus.ts `commaPieces`).
#[must_use]
pub fn comma_pieces(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    text.split(',').count() - usize::from(text.ends_with(','))
}

/// The families a slice's settings turn on (corpus.ts `familiesFor`), `get` reading each setting,
/// `points` counting the points of a list of them and `filaments` the job's filament count; `Err`
/// when a setting it reads is unknown.
///
/// # Errors
///
/// [`Unknown`], from `get` or `points`.
pub fn setting_families(
    get: &dyn Fn(&str) -> Setting,
    points: &dyn Fn(&str) -> Result<usize, Unknown>,
    filaments: usize,
    infill: &[String],
) -> Result<Vec<String>, Unknown> {
    let mut out: Vec<String> = Vec::new();
    let is = |key: &str, value: &str| -> Result<bool, Unknown> {
        Ok(get(key)?.as_deref() == Some(value))
    };
    out.push(if is("wall_generator", "classic")? {
        "perimeters-classic".to_string()
    } else {
        "perimeters-arachne".to_string()
    });
    let density = get("sparse_infill_density")?.unwrap_or_else(|| "0".to_string());
    let mut patterns = vec![
        get("top_surface_pattern")?,
        get("bottom_surface_pattern")?,
        get("internal_solid_infill_pattern")?,
    ];
    if above_zero_or_unread(&density) {
        patterns.push(get("sparse_infill_pattern")?);
    }
    for p in patterns.into_iter().flatten() {
        let family = format!("infill-{p}");
        if infill.contains(&family) {
            out.push(family);
        }
    }
    if is("enable_support", "1")? {
        let tree = get("support_type")?.is_some_and(|t| t.starts_with("tree"));
        out.push(if tree { "tree-supports" } else { "supports" }.to_string());
        out.push("step-simplify-support-path".to_string());
        out.push("infill-supportbase".to_string());
    }
    if above_zero_or_unread(&get("raft_layers")?.unwrap_or_else(|| "0".to_string())) {
        out.push("raft".to_string());
    }
    if get("ironing_type")?.as_deref().unwrap_or("no ironing") != "no ironing" {
        out.push("ironing".to_string());
    }
    if !matches!(
        get("fuzzy_skin")?.as_deref(),
        None | Some("none" | "disabled_fuzzy")
    ) {
        out.push("fuzzy-skin".to_string());
    }
    if is("enable_prime_tower", "1")?
        && (filaments > 1
            || is("timelapse_type", "1")?
            || (is("enable_wrapping_detection", "1")? && points("wrapping_exclude_area")? > 2))
    {
        out.push("wipe-tower".to_string());
    }
    if is("zaa_enabled", "1")? {
        out.push("step-contouring".to_string());
    }
    Ok(out)
}

/// Every family a slice's settings can turn on: what a job whose settings are unknown reaches.
#[must_use]
pub fn unsettled_families(infill: &[String]) -> Vec<String> {
    SETTING_FAMILIES
        .iter()
        .map(|s| (*s).to_string())
        .chain(infill.iter().cloned())
        .collect()
}

/// The job's preset files, process, machine and filaments in that order (corpus.ts
/// `settingLookup`'s order); `None` when one cannot be read as a JSON object.
fn read_presets(job: &Job) -> Option<Vec<Value>> {
    let paths = [&job.process, &job.machine]
        .into_iter()
        .chain(job.filaments.iter());
    let mut out = Vec::new();
    for path in paths {
        let text = read_file(path).ok()?;
        let doc = orca_json::parse(&text, Order::Insertion).ok()?;
        doc.as_object()?;
        out.push(doc);
    }
    Some(out)
}

/// A setting through `layers` of overrides (the innermost first; the last of a key in one wins),
/// then the presets (an array's first value), then Orca's default.
fn lookup(layers: &[&Overrides], presets: &[Value], key: &str) -> Setting {
    for overrides in layers {
        if let Some((_, v)) = overrides.iter().rev().find(|(k, _)| k == key) {
            return Ok(Some(v.clone()));
        }
    }
    for preset in presets {
        if let Some(v) = preset.get(key) {
            let v = match v {
                Value::Array(a) => a.first().ok_or(Unknown)?,
                other => other,
            };
            return v.as_str().map(|s| Some(s.to_string())).ok_or(Unknown);
        }
    }
    Ok(orca_config::get(key)
        .and_then(|d| d.default)
        .map(str::to_string))
}

/// The points of a list of them through `layers` of overrides, then the presets, then Orca's
/// default, read as [`lookup`] reads a setting: an override's text or a preset's string in
/// [`comma_pieces`], a preset's array by its length; a preset value that is neither is unknown.
fn points(layers: &[&Overrides], presets: &[Value], key: &str) -> Result<usize, Unknown> {
    for overrides in layers {
        if let Some((_, v)) = overrides.iter().rev().find(|(k, _)| k == key) {
            return Ok(comma_pieces(v));
        }
    }
    for preset in presets {
        if let Some(v) = preset.get(key) {
            return match v {
                Value::Array(a) => Ok(a.len()),
                other => other.as_str().map(comma_pieces).ok_or(Unknown),
            };
        }
    }
    Ok(orca_config::get(key)
        .and_then(|d| d.default)
        .map_or(0, comma_pieces))
}

/// The families a job's presets reach besides those of its settings (corpus.ts `presetFamilies`):
/// preset-conditions where Orca works a compatibility condition out, in the order the presets
/// stage checks them (the process with the printer, where a list that leaves the printer out ends
/// the job; then each filament with the printer and, when that holds, with the process). A
/// compatibility is worked out where a preset has no list (an empty one, or none) and a condition
/// that is not empty; a list holds when the printer (or the parent of a printer that is not a
/// system preset), or the process, is in it, or the name is empty. Presets that cannot be read
/// reach it too, so a job is never let through for it.
#[must_use]
pub fn preset_families(presets: Option<&[Value]>) -> Vec<String> {
    let conditions = vec!["preset-conditions".to_string()];
    let Some([process, machine, filaments @ ..]) = presets else {
        return conditions;
    };
    let name = |p: &Value| {
        p.get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let mut printer = vec![name(machine)];
    if machine.get("from").and_then(Value::as_str) != Some("system")
        && let Some(parent) = machine.get("inherits").and_then(Value::as_str)
    {
        printer.push(parent.to_string());
    }
    // compatibleByList: None where Orca works the condition out.
    let by_list = |preset: &Value, list: &str, condition: &str, names: &[String]| -> Option<bool> {
        let items: &[Value] = preset
            .get(list)
            .and_then(Value::as_array)
            .map_or(&[], Vec::as_slice);
        if items.is_empty() {
            let has_condition = preset
                .get(condition)
                .and_then(Value::as_str)
                .is_some_and(|c| !c.is_empty());
            return if has_condition { None } else { Some(true) };
        }
        Some(
            names[0].is_empty()
                || names
                    .iter()
                    .any(|n| items.iter().any(|i| i.as_str() == Some(n.as_str()))),
        )
    };
    let process_name = [name(process)];
    match by_list(
        process,
        "compatible_printers",
        "compatible_printers_condition",
        &printer,
    ) {
        None => return conditions,
        Some(false) => return Vec::new(),
        Some(true) => {}
    }
    for f in filaments {
        match by_list(
            f,
            "compatible_printers",
            "compatible_printers_condition",
            &printer,
        ) {
            None => return conditions,
            Some(true)
                if by_list(
                    f,
                    "compatible_prints",
                    "compatible_prints_condition",
                    &process_name,
                )
                .is_none() =>
            {
                return conditions;
            }
            _ => {}
        }
    }
    Vec::new()
}

/// The families `job` reaches: those of its presets, its command's, then those its settings turn
/// on, each once.
#[must_use]
pub fn reached(job: &Job) -> Vec<String> {
    let presets = read_presets(job);
    let mut out = preset_families(presets.as_deref());
    out.extend(
        command_families(job.command)
            .iter()
            .map(|s| (*s).to_string()),
    );
    if job.command == Command::Slice {
        let infill = infill_families(&families());
        let mut views: Vec<Vec<&Overrides>> = vec![vec![&job.overrides]];
        for object in &job.objects {
            views.push(vec![&object.overrides, &job.overrides]);
            for range in &object.height_ranges {
                views.push(vec![&range.overrides, &object.overrides, &job.overrides]);
            }
        }
        let settled = presets.and_then(|presets| {
            let mut all = Vec::new();
            for view in &views {
                let get = |key: &str| lookup(view, &presets, key);
                let count = |key: &str| points(view, &presets, key);
                all.extend(setting_families(&get, &count, job.filaments.len(), &infill).ok()?);
            }
            Some(all)
        });
        out.extend(settled.unwrap_or_else(|| unsettled_families(&infill)));
    }
    let mut once: Vec<String> = Vec::with_capacity(out.len());
    for f in out {
        if !once.contains(&f) {
            once.push(f);
        }
    }
    once
}

/// The families of `reach` that are not done (ported here and certified), in parity.json's
/// order; one parity.json does not list comes last, in the order of `reach`.
#[must_use]
pub fn missing_of(reach: &[String]) -> Vec<String> {
    let all = families();
    let done = |name: &str| PORTED.contains(&name) && all.iter().any(|(n, c)| n == name && *c);
    let mut out: Vec<String> = all
        .iter()
        .filter(|(n, _)| reach.contains(n) && !done(n))
        .map(|(n, _)| n.clone())
        .collect();
    for name in reach {
        if !all.iter().any(|(n, _)| n == name) {
            out.push(name.clone());
        }
    }
    out
}

/// The families `job` reaches that are not done, in parity.json's order.
#[must_use]
pub fn missing(job: &Job) -> Vec<String> {
    missing_of(&reached(job))
}

/// The decline of a job whose command reaches `missing`.
#[must_use]
pub fn not_ported(command_name: &str, missing: Vec<String>) -> Failure {
    Failure {
        code: "not-ported".to_string(),
        message: format!(
            "sketch-orca-rs cannot do this {command_name} job yet: it reaches steps that are not ported ({})",
            missing.join(", ")
        ),
        object_index: -1,
        exit_code: -8,
        missing,
    }
}

/// `Ok` when this program does every family the job reaches, else its decline.
///
/// # Errors
///
/// The [`not_ported`] failure.
pub fn check(job: &Job) -> Result<(), Failure> {
    let m = missing(job);
    if m.is_empty() {
        Ok(())
    } else {
        Err(not_ported(&job.command_name, m))
    }
}

/// The decline of a job whose presets have a compatibility condition the placeholder parser does
/// not take (`presets.rs`, orca-placeholder's `Unsupported`): the families the job reaches, with
/// preset-conditions first among them even where parity.json certifies it, since this job's
/// condition is past what was certified.
#[must_use]
pub fn unsupported_condition(job: &Job) -> Failure {
    let mut missing = missing_of(&reached(job));
    if !missing.iter().any(|m| m == "preset-conditions") {
        missing.insert(0, "preset-conditions".to_string());
    }
    not_ported(&job.command_name, missing)
}

#[cfg(test)]
mod tests {
    use super::{
        Command, PORTED, Setting, Unknown, comma_pieces, command_families, families,
        infill_families, missing_of, not_ported, setting_families, unsettled_families,
    };

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    /// The families `settings` turn on, with Orca's defaults (orca-config) under them; a list of
    /// points is counted from the same text.
    fn with(settings: &[(&str, &str)], filaments: usize) -> Result<Vec<String>, Unknown> {
        let text = |key: &str| -> Setting {
            if let Some((_, v)) = settings.iter().find(|(k, _)| *k == key) {
                return if *v == "?" {
                    Err(Unknown)
                } else {
                    Ok(Some((*v).to_string()))
                };
            }
            Ok(orca_config::get(key)
                .and_then(|d| d.default)
                .map(str::to_string))
        };
        let points = |key: &str| Ok(comma_pieces(&text(key)?.unwrap_or_default()));
        setting_families(&text, &points, filaments, &infill_families(&families()))
    }

    #[test]
    fn every_family_named_is_one_of_parity_json() {
        let all = families();
        assert!(all.len() > 50);
        let infill = infill_families(&all);
        assert!(infill.len() > 25);
        for c in [Command::Info, Command::Layers, Command::Slice] {
            for f in command_families(c) {
                assert!(all.iter().any(|(n, _)| n == f), "{f}");
            }
        }
        for f in unsettled_families(&infill) {
            assert!(all.iter().any(|(n, _)| *n == f), "{f}");
        }
        for p in PORTED {
            assert!(all.iter().any(|(n, _)| n == p), "{p}");
        }
    }

    #[test]
    fn every_job_is_declined_for_now_naming_the_steps_in_parity_json_order() {
        let reach = |c: Command| strings(command_families(c));
        assert_eq!(
            missing_of(&reach(Command::Info)),
            ["model-load", "placement", "info"]
        );
        assert_eq!(
            missing_of(&reach(Command::Layers)),
            ["model-load", "placement", "slice-mesh"]
        );
        let mut slice = reach(Command::Slice);
        slice.extend(with(&[], 1).unwrap());
        let slice = missing_of(&slice);
        assert_eq!(slice.len(), 18);
        assert_eq!(
            slice[..8],
            [
                "model-load",
                "placement",
                "slice-mesh",
                "perimeters-arachne",
                "infill-monotonic",
                "infill-monotonicline",
                "infill-crosshatch",
                "infill-concentricinternal"
            ]
        );
        assert_eq!(
            slice.last().map(String::as_str),
            Some("step-conflict-check")
        );
        let f = not_ported("info", missing_of(&reach(Command::Info)));
        assert_eq!(
            (f.code.as_str(), f.exit_code, f.object_index),
            ("not-ported", -8, -1)
        );
        assert_eq!(
            f.message,
            "sketch-orca-rs cannot do this info job yet: it reaches steps that are not ported (model-load, placement, info)"
        );
    }

    #[test]
    fn settings_turn_families_on_as_the_corpus_reads_them() {
        // Orca's defaults: Arachne walls, the three surface patterns and the sparse one at 20%.
        assert_eq!(
            with(&[], 1).unwrap(),
            [
                "perimeters-arachne",
                "infill-monotonicline",
                "infill-monotonic",
                "infill-monotonic",
                "infill-crosshatch"
            ]
        );
        // No sparse infill at 0%; a pattern the corpus does not name is no family.
        let none = with(
            &[
                ("sparse_infill_density", "0%"),
                ("top_surface_pattern", "gyroid"),
                ("bottom_surface_pattern", "nothing"),
            ],
            1,
        )
        .unwrap();
        assert_eq!(
            none,
            ["perimeters-arachne", "infill-gyroid", "infill-monotonic"]
        );
        let on = with(
            &[
                ("wall_generator", "classic"),
                ("enable_support", "1"),
                ("support_type", "tree(auto)"),
                ("raft_layers", "2"),
                ("ironing_type", "top"),
                ("fuzzy_skin", "external"),
                ("enable_prime_tower", "1"),
                ("zaa_enabled", "1"),
            ],
            2,
        )
        .unwrap();
        for f in [
            "perimeters-classic",
            "tree-supports",
            "step-simplify-support-path",
            "infill-supportbase",
            "raft",
            "ironing",
            "fuzzy-skin",
            "wipe-tower",
            "step-contouring",
        ] {
            assert!(on.iter().any(|x| x == f), "{f}");
        }
        assert!(!on.iter().any(|x| x == "perimeters-arachne"));
        // Normal supports; the prime tower needs two filaments or a timelapse.
        let normal = with(&[("enable_support", "1"), ("enable_prime_tower", "1")], 1).unwrap();
        assert!(normal.iter().any(|x| x == "supports"));
        assert!(!normal.iter().any(|x| x == "wipe-tower"));
        let timelapse = with(&[("enable_prime_tower", "1"), ("timelapse_type", "1")], 1).unwrap();
        assert!(timelapse.iter().any(|x| x == "wipe-tower"));
        // A value that does not read as a number leaves the family on.
        let raft = with(&[("raft_layers", "")], 1).unwrap();
        assert!(raft.iter().any(|x| x == "raft"));
        // A setting it cannot read leaves the job's settings unknown.
        assert_eq!(with(&[("enable_support", "?")], 1), Err(Unknown));
    }

    #[test]
    fn clumping_detection_with_an_exclusion_area_puts_a_prime_tower_on_one_filament() {
        let tower = |settings: &[(&str, &str)]| {
            with(settings, 1).unwrap().iter().any(|x| x == "wipe-tower")
        };
        let area = "145x310,256x310,256x326,145x326";
        let on = [
            ("enable_prime_tower", "1"),
            ("enable_wrapping_detection", "1"),
            ("wrapping_exclude_area", area),
        ];
        assert!(tower(&on));
        // Each of the three is needed; three points are enough, two are not.
        assert!(!tower(&[on[0], on[1]]));
        assert!(!tower(&[on[0], on[2]]));
        assert!(!tower(&[on[1], on[2]]));
        assert!(tower(&[
            on[0],
            on[1],
            ("wrapping_exclude_area", "0x0,1x0,1x1")
        ]));
        assert!(!tower(&[
            on[0],
            on[1],
            ("wrapping_exclude_area", "0x0,1x0,")
        ]));
        // An exclusion area it cannot read leaves the job's settings unknown.
        assert_eq!(
            with(&[on[0], on[1], ("wrapping_exclude_area", "?")], 1),
            Err(Unknown)
        );
    }

    #[test]
    fn points_are_counted_as_orcas_reader_splits_them() {
        assert_eq!(comma_pieces(""), 0);
        assert_eq!(comma_pieces("0x0"), 1);
        assert_eq!(comma_pieces("0x0,1x1"), 2);
        assert_eq!(comma_pieces("0x0,1x1,"), 2);
        assert_eq!(comma_pieces(","), 1);
        assert_eq!(comma_pieces(",,"), 2);
        assert_eq!(comma_pieces("0x0,,1x1"), 3);
    }
}
