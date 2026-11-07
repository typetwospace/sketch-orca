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
// Carries OrcaSlicer code: cli/presets.cpp's merge of the presets, which ports load_config_file,
// load_default_gcodes_to_config, update_full_config, the printer safe check with cli_config.json,
// the flush volume computation and the filament settings of the print configuration from
// src/OrcaSlicer.cpp and get_min_flush_volumes from src/slic3r/GUI/Plater.cpp, and
// is_compatible_with_printer and is_compatible_with_print from src/libslic3r/Preset.cpp, all of
// OrcaSlicer v2.4.2, commit 8500fcd, translated on 2026-11-06. On 2026-11-07 (o2-front): the
// compatibility conditions of Preset.cpp (lines 771-845), the printer model id, the Bambu Lab
// check, the plate (Preset::get_default_bed_type) and the flushing volumes themselves, with
// FlushVolCalculator (src/libslic3r/FlushVolCalc.cpp), FlushVolPredictor and its colour distance
// (src/libslic3r/FlushVolPredictor.cpp), RGB2HSV (src/slic3r/Utils/ColorSpaceConvert.cpp),
// set_flush_volumes_matrix (src/libslic3r/PrintConfig.hpp) and
// DynamicPrintConfig::get_index_for_extruder (src/libslic3r/PrintConfig.cpp). Their copyright is
// the OrcaSlicer contributors', the BambuStudio authors' (Bambu Lab) and the PrusaSlicer
// authors' (Prusa Research), under the AGPL-3.0.

//! The presets stage: the job's machine, process and filament presets read and merged into one
//! print configuration, the job's overrides applied and the result checked, as `cli/presets.cpp`'s
//! `load_settings` does it, step for step, with the same failures and warnings in the same order.
//!
//! - Each preset file ([`load_preset`]): read with sorted keys as Orca reads one; its description
//!   keys (name, type, from ...) kept apart; a list written as Orca's text; each value read with
//!   Orca's substitutions (an enum or bool value Orca does not know takes the default:
//!   `setting-substituted`); a key Orca does not have skipped without a word, as Orca's
//!   handle_legacy drops it before the value is set (so the C++ `unknown-setting` warning, kept
//!   here as it is there, never comes), and a value that is neither text nor a list, or a list that
//!   mixes lists and values, skipped with `preset-value-skipped`. Refused: a missing file
//!   (`file-not-found`), a file that is not JSON or not a JSON object, a value Orca cannot read, a
//!   `from` that is not system, User or user, a preset of the wrong type (`preset-invalid`), and a
//!   resin printer (`not-fdm`). A bad thumbnails list stops the run as Orca's exception does
//!   (`internal-error`).
//! - The process must be made for the printer (`process-not-compatible`), and each filament is
//!   checked against the printer and the process (`filament-not-compatible`, a warning), by their
//!   `compatible_printers` and `compatible_prints` lists, or, for a preset with no list, by its
//!   `compatible_printers_condition` or `compatible_prints_condition`, worked out by Orca's
//!   placeholder parser (orca-placeholder, [`compatible_with_printer`]): against the printer
//!   preset (Orca's defaults of every printer setting with the file's values over them, and
//!   `printer_preset` and `num_extruders` over those), or against the process preset. A condition
//!   that does not parse makes the preset compatible, as in Orca: the printer's check logs it on
//!   stderr at log level 2 or more, the process's prints it on stdout, Orca's own lines. A
//!   condition past what the parser takes declines the job (`unsupported_condition`, decline.rs).
//! - The merge (machine, then process, then each filament's values at its index, the lists with
//!   extruder variants in blocks by `filament_self_index`), the job's overrides
//!   ([`parse_setting`], no substitutions: `setting-invalid`), each filament's colour, the
//!   overrides that must give one value per filament, the flushing volumes' inputs (the
//!   extruder-variant picks Orca makes for each nozzle and filament, then each volume and
//!   retraction length the computation turns into a whole number, `config-invalid` past 20000),
//!   Orca's normalisation, its defaults for every setting of a print, and its own check of the
//!   values (`config-invalid`, "Orca refuses these settings: ...").
//!
//! The parity corpus holds a job for each of these refusals and warnings, and the C++ program's
//! result for it as an intake golden (the smoke refusals, `scripts/lib/smoke.ts` `REFUSALS`;
//! `parity/goldens/front/intake/`, `tests/intake.rs`), but for the thumbnails' internal error and
//! the forms of `setting-invalid` past a key Orca does not have and a value it cannot read (a resin
//! setting, filament colours, the flushing matrix's size and a per-filament override's count).
//!
//! The printer's model id (its vendor bundle's `model_id`, [`find_printer_model_id`]), whether it
//! is a Bambu Lab printer, the plate the GUI would pick (`curr_bed_type`, [`default_bed_type`]) and
//! the flushing volumes between the filaments ([`compute_flush_volumes`]: each nozzle's n x n
//! matrix from the filaments' colours, Orca's flush data in the resources' `flush` folder and its
//! colour formula, each nozzle's `nozzle_flush_dataset` and minimum volume) complete the print
//! configuration, which equals the C++ program's key for key.
//!
//! Left out, as nothing reads it: the second set of extruder-variant picks Orca makes on its copy
//! for the flushing volumes (`printer_options_with_variant_2`, the machine limits, which the
//! volumes do not read). Where Orca reads the first value of an empty list in the picks (a printer
//! with two nozzles given an empty list of a per-nozzle setting), its behaviour is undefined; this
//! program reads 0 there, which no check of this stage reads as a failure (the volumes and lengths
//! it checks are lists of numbers, which Orca never reads as empty).

// Settings and LoadedPreset hold what the steps after the model check read; they are not ported
// yet (decline.rs).
#![allow(dead_code)]

use std::collections::BTreeMap;

use orca_config::value::escape_string_cstyle;
use orca_config::{ConfigError, OptionType, OptionValue, PrinterTechnology};
use orca_json::{Order, Value};
use orca_placeholder::{Error as ConditionError, Symbols, evaluate_boolean_expression};

use crate::common::{Failure, fail, format_number};
use crate::config::{
    self, Config, FILAMENT_OPTIONS_WITH_VARIANT, Rule, SetError, find_def, resize, set_at,
    set_with_restore_2, vec_len,
};
use crate::decline;
use crate::files::read_file;
use crate::job::Job;
use crate::paths::is_file;
use crate::result::Warnings;
use crate::version::ORCA_VERSION;

/// The keys of a preset file that describe the file rather than a setting (`META_KEYS`, Orca's
/// `BBL_JSON_KEY_*`).
const META_KEYS: &[&str] = &[
    "version",
    "name",
    "url",
    "type",
    "setting_id",
    "filament_id",
    "from",
    "description",
    "instantiation",
    "renamed_from",
];

/// The colour Orca gives a project's first filament (filament_colour's default).
const ORCA_FILAMENT_COLOUR: &str = "#F2754E";

/// The colours the GUI gives the filaments it adds after the first
/// (`Plater::get_next_color_for_filament`).
const GUI_FILAMENT_COLOURS: [&str; 16] = [
    "#00C1AE", "#F4E2C1", "#ED1C24", "#00FF7F", "#F26722", "#FFEB31", "#7841CE", "#115877",
    "#ED1E79", "#2EBDEF", "#345B2F", "#800080", "#FA8173", "#800000", "#F7B763", "#A4C41E",
];

/// `g_max_flush_volume` (FlushVolCalc.cpp).
const MAX_FLUSH_VOLUME: i32 = 20000;

/// `LongRectrationLevel::EnableFilament`.
const LONG_RETRACTION_ENABLE_FILAMENT: i32 = 2;

/// `g_min_flush_volume_from_support` (FlushVolCalc.cpp).
const MIN_FLUSH_VOLUME_FROM_SUPPORT: i32 = 700;

/// `g_flush_volume_to_support` (FlushVolCalc.cpp).
const FLUSH_VOLUME_TO_SUPPORT: i32 = 230;

/// `M_PI`.
const PI: f64 = std::f64::consts::PI;

/// `btPC` and `btPEI` of `BedType`, and `btCount`.
const BED_PC: i32 = 1;
const BED_PEI: i32 = 3;
const BED_COUNT: i32 = 7;

/// One preset file as the stage read it (`struct LoadedPreset`).
#[derive(Clone, Debug, Default)]
pub struct LoadedPreset {
    pub path: String,
    /// machine, process or filament.
    pub kind: String,
    pub name: String,
    /// system, User or user.
    pub from: String,
    /// The system preset a user preset is based on.
    pub inherits: String,
    pub filament_id: String,
    pub config: Config,
}

/// The job's settings (`struct Settings`).
#[derive(Clone, Debug, Default)]
pub struct Settings {
    /// The whole print configuration, every setting of a print present.
    pub config: Config,
    pub machine: LoadedPreset,
    pub process: LoadedPreset,
    pub filaments: Vec<LoadedPreset>,
    pub extruder_count: usize,
    /// The keys the job's overrides set, after Orca's renames.
    pub overridden: Vec<String>,
    /// The printer model's id in its vendor bundle ("BL-P001"), or "".
    pub printer_model_id: String,
    /// Whether the printer is a Bambu Lab one (by its model, or its preset's name).
    pub is_bbl: bool,
}

/// `in_quotes(s)`.
fn in_quotes(s: &str) -> String {
    format!("\"{s}\"")
}

/// The run's internal error for an exception Orca's code throws and the stage does not catch
/// (`run_job` reports its `what()`).
fn internal(e: &ConfigError) -> Failure {
    fail("internal-error", e.0.clone())
}

/// `scalar_text(v)`: text as it is, true and false as 1 and 0, a number as nlohmann writes it.
fn scalar_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => if *b { "1" } else { "0" }.to_string(),
        other => other.dump(None),
    }
}

fn is_scalar_value(v: &Value) -> bool {
    matches!(
        v,
        Value::String(_) | Value::Int(_) | Value::UInt(_) | Value::Float(_) | Value::Bool(_)
    )
}

/// `array_text(arr, single_sep, array_sep, escape, out)`: a list as the text Orca reads (its
/// `parse_str_arr`), items joined with `single_sep`, nested lists with `array_sep`, the items of a
/// list of strings quoted and escaped. False for a list that mixes lists and values, or holds an
/// item that is not text, a number or true/false.
fn array_text(
    items: &[Value],
    single_sep: char,
    array_sep: char,
    escape: bool,
    out: &mut String,
) -> bool {
    let has_list = items.iter().any(|i| matches!(i, Value::Array(_)));
    let has_value = items.iter().any(|i| !matches!(i, Value::Array(_)));
    if has_list && has_value {
        return false;
    }
    let mut first = true;
    for item in items {
        if let Value::Array(inner) = item {
            if !first {
                out.push(array_sep);
            }
            first = false;
            if !array_text(inner, single_sep, array_sep, escape, out) {
                return false;
            }
            continue;
        }
        if !is_scalar_value(item) {
            return false;
        }
        if !first {
            out.push(single_sep);
        }
        first = false;
        let s = scalar_text(item);
        if escape {
            out.push('"');
            out.push_str(&escape_string_cstyle(&s));
            out.push('"');
        } else {
            out.push_str(&s);
        }
    }
    true
}

/// `load_preset(path, slot, field, warnings)`: one preset file, read as the stock command line
/// reads `--load-settings` and `--load-filaments`.
///
/// # Errors
///
/// `file-not-found`, `preset-invalid`, `not-fdm`, or the internal error of a bad thumbnails list.
pub fn load_preset(
    path: &str,
    slot: &str,
    field: &str,
    warnings: &Warnings,
) -> Result<LoadedPreset, Failure> {
    if !is_file(path) {
        return Err(fail(
            "file-not-found",
            format!("{field}: there is no file {path}"),
        ));
    }
    let parsed = match read_file(path) {
        Ok(bytes) => orca_json::parse(&bytes, Order::Sorted).map_err(|e| e.message),
        Err(e) => Err(e.0),
    };
    let j = parsed.map_err(|what| {
        fail(
            "preset-invalid",
            format!("{field}: {path} is not JSON ({what})"),
        )
    })?;
    let Value::Object(members) = &j else {
        return Err(fail(
            "preset-invalid",
            format!("{field}: {path} is not a JSON object"),
        ));
    };

    let mut config = Config::default();
    let mut substitutions = Vec::new();
    let mut meta: BTreeMap<&str, String> = BTreeMap::new();
    let (mut tree_hybrid, mut infill_first) = (false, false);
    for (key, value) in members.iter() {
        if key.eq_ignore_ascii_case("is_custom_defined") {
            continue;
        }
        if let Some(m) = META_KEYS.iter().find(|m| key.eq_ignore_ascii_case(m)) {
            meta.entry(*m).or_insert_with(|| match value {
                Value::String(s) => s.clone(),
                other => other.dump(None),
            });
            continue;
        }
        let mut set_key = key.clone();
        let text = if is_scalar_value(value) {
            scalar_text(value)
        } else if let Value::Array(items) = value {
            let mut legacy_value = String::new();
            orca_config::handle_legacy(&mut set_key, &mut legacy_value);
            if set_key.is_empty() {
                // A setting Orca dropped, or one it never had; its loader skips it too.
                continue;
            }
            let (single_sep, escape) = match find_def(&set_key).map(|d| d.ty) {
                Some(OptionType::Strings) => (';', true),
                Some(OptionType::PointsGroups) => ('#', false),
                _ => (',', false),
            };
            let mut text = String::new();
            if !array_text(items, single_sep, '#', escape, &mut text) {
                warnings.add(
                    "preset-value-skipped",
                    &format!("{field}: {key} is a list Orca cannot read; it keeps Orca's default"),
                    -1,
                );
                continue;
            }
            text
        } else {
            warnings.add(
                "preset-value-skipped",
                &format!("{field}: {key} is neither text nor a list; it keeps Orca's default"),
                -1,
            );
            continue;
        };
        match config.set_deserialize(&set_key, &text, Rule::Enable, &mut substitutions) {
            Ok(true) => {}
            Ok(false) => {
                return Err(fail(
                    "preset-invalid",
                    format!(
                        "{field}: {key} has a value Orca cannot read: {}",
                        in_quotes(&text)
                    ),
                ));
            }
            Err(SetError::Unknown(_)) => {
                warnings.add(
                    "unknown-setting",
                    &format!(
                        "{field}: {key} is not a setting of Orca {ORCA_VERSION}; it is ignored"
                    ),
                    -1,
                );
                continue;
            }
            Err(SetError::Config(e)) => {
                return Err(fail("preset-invalid", format!("{field}: {key}: {}", e.0)));
            }
        }
        let is_text = matches!(value, Value::String(_));
        if key == "support_type" && is_text && text == "hybrid(auto)" {
            tree_hybrid = true;
        }
        if key == "wall_infill_order"
            && is_text
            && (text == "infill/outer wall/inner wall" || text == "infill/inner wall/outer wall")
        {
            infill_first = true;
        }
    }
    if tree_hybrid && let Some(OptionValue::Enum { value, .. }) = config.create("support_style") {
        // smsTreeHybrid.
        *value = 6;
    }
    if infill_first && let Some(OptionValue::Bool(b)) = config.create("is_infill_first") {
        *b = true;
    }
    config::handle_legacy_composite(&mut config).map_err(|what| fail("internal-error", what))?;

    let meta_of = |k: &str| meta.get(k).cloned().unwrap_or_default();
    let mut p = LoadedPreset {
        path: path.to_string(),
        kind: meta_of("type"),
        name: meta_of("name"),
        from: meta_of("from"),
        inherits: String::new(),
        filament_id: meta_of("filament_id"),
        config: Config::default(),
    };
    if !matches!(p.from.as_str(), "system" | "User" | "user") {
        return Err(fail(
            "preset-invalid",
            format!(
                "{field}: \"from\" must be system, User or user, got {}",
                in_quotes(&p.from)
            ),
        ));
    }
    if p.kind != slot {
        let what = if p.kind.is_empty() {
            "a file without a type".to_string()
        } else {
            format!("a {} preset", p.kind)
        };
        return Err(fail(
            "preset-invalid",
            format!("{field}: {path} is {what}, not a {slot} preset"),
        ));
    }
    if let Some(OptionValue::String(inherits)) = config.get("inherits") {
        p.inherits.clone_from(inherits);
    }
    // printer_technology_of: ptUnknown when the preset does not say.
    if let Some(OptionValue::Enum { value, .. }) = config.get("printer_technology")
        && *value != 0
    {
        return Err(fail(
            "not-fdm",
            format!("{field}: {} is not a filament (FFF) preset", p.name),
        ));
    }
    config::normalize_fdm(&mut config);
    for s in &substitutions {
        let new_value = s.new_value.serialize().map_err(|e| internal(&e))?;
        warnings.add(
            "setting-substituted",
            &format!(
                "{field}: {} = {} is not a value Orca {ORCA_VERSION} knows; it uses {}",
                s.opt_key,
                in_quotes(&s.old_value),
                in_quotes(&new_value)
            ),
            -1,
        );
    }
    p.config = config;
    Ok(p)
}

/// The two preset types `add_default_gcodes` fills in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Printer,
    Filament,
}

/// `add_default_gcodes(config, type)` (OrcaSlicer.cpp's `load_default_gcodes_to_config`): the
/// G-code options a printer or filament preset may leave out, made empty.
fn add_default_gcodes(config: &mut Config, kind: Kind) {
    if config.is_empty() {
        return;
    }
    match kind {
        Kind::Printer => {
            for key in [
                "change_filament_gcode",
                "layer_change_gcode",
                "machine_end_gcode",
                "machine_pause_gcode",
                "machine_start_gcode",
                "template_custom_gcode",
                "printing_by_object_gcode",
                "before_layer_change_gcode",
                "time_lapse_gcode",
                "wrapping_detection_gcode",
            ] {
                config.create(key);
            }
        }
        Kind::Filament => {
            for key in ["filament_start_gcode", "filament_end_gcode"] {
                if let Some(OptionValue::Strings(values)) = config.create(key)
                    && values.is_empty()
                {
                    values.push(String::new());
                }
            }
        }
    }
}

/// `merge_metadata_key(key)`: the lists the merge keeps itself, one entry per preset.
fn merge_metadata_key(key: &str) -> bool {
    matches!(
        key,
        "inherits_group"
            | "different_settings_to_system"
            | "compatible_machine_expression_group"
            | "compatible_process_expression_group"
            | "print_compatible_printers"
    )
}

/// `set_preset_slot(cfg, key, slot, n, value)`: entry `slot` of a list with one entry per preset
/// (the process at 0, filament i at i + 1, the machine at n + 1).
fn set_preset_slot(cfg: &mut Config, key: &str, slot: usize, n: usize, value: &str) {
    if let Some(OptionValue::Strings(values)) = cfg.create(key) {
        let want = orca_stl::max(n + 2, slot + 1);
        if values.len() < want {
            values.resize(want, String::new());
        }
        values[slot] = value.to_string();
    }
}

/// `copy_preset_keys(full, config, field)` (OrcaSlicer.cpp's `update_full_config`): every key of a
/// machine or process preset into the whole configuration, but those that describe the preset.
fn copy_preset_keys(full: &mut Config, config: &Config, field: &str) -> Result<(), Failure> {
    const SKIPPED: [&str; 11] = [
        "compatible_prints",
        "compatible_printers",
        "model_id",
        "inherits",
        "dev_model_name",
        "name",
        "from",
        "type",
        "version",
        "setting_id",
        "instantiation",
    ];
    for key in config.keys() {
        if SKIPPED.contains(&key.as_str()) || merge_metadata_key(&key) {
            continue;
        }
        let (Some(src), Some(dst)) = (config.get(&key), full.create(&key)) else {
            return Err(fail(
                "preset-invalid",
                format!("{field}: {key} cannot be stored"),
            ));
        };
        config::set_value(dst, src).map_err(|e| internal(&e))?;
    }
    Ok(())
}

/// `apply_cli_safe_limits(full, resources, printer_model)`: a Bambu printer's machine limits
/// lowered to the "cli_safe" values of profiles/BBL/cli_config.json, as the stock command line
/// lowers them when it loads a printer; a file it cannot read changes nothing.
fn apply_cli_safe_limits(full: &mut Config, resources: &str, printer_model: &str) {
    if printer_model.is_empty() {
        return;
    }
    let file = format!("{resources}/profiles/BBL/cli_config.json");
    if !is_file(&file) {
        return;
    }
    let Ok(bytes) = read_file(&file) else {
        return;
    };
    let Ok(root) = orca_json::parse(&bytes, Order::Sorted) else {
        return;
    };
    let limits = root
        .get("printer")
        .filter(|p| p.as_object().is_some())
        .and_then(|p| p.get(printer_model))
        .filter(|m| m.as_object().is_some())
        .and_then(|m| m.get("machine_limits"))
        .and_then(Value::as_object);
    let Some(limits) = limits else {
        return;
    };
    for (key, value) in limits.iter() {
        let Value::String(text) = value else {
            continue;
        };
        if key.len() < 8 {
            continue;
        }
        let mut renamed = b"machine_max".to_vec();
        renamed.extend_from_slice(&key.as_bytes()[8..]);
        let Ok(renamed) = String::from_utf8(renamed) else {
            continue;
        };
        // option<ConfigOptionFloats>: a list of floats, nullable or not.
        let Some(OptionValue::Floats {
            values: current, ..
        }) = full.get_mut(&renamed)
        else {
            continue;
        };
        let mut safe = OptionValue::Floats {
            values: Vec::new(),
            nullable: false,
        };
        // A nil the list cannot hold throws, which ends the check (the stock command line logs it).
        if safe.deserialize(text).is_err() {
            return;
        }
        let OptionValue::Floats { values: safe, .. } = safe else {
            continue;
        };
        for (v, s) in current.iter_mut().zip(safe.iter()) {
            if *s != 0.0 && *s < *v {
                *v = *s;
            }
        }
    }
}

/// `flush_input(v, key)`: a volume or length the flush computation takes as a whole number.
fn flush_input(v: f64, key: &str) -> Result<i32, Failure> {
    if !v.is_finite() || orca_crt::fabs(v) > f64::from(MAX_FLUSH_VOLUME) {
        return Err(fail(
            "config-invalid",
            format!(
                "{key} cannot be {} for the flushing volumes (at most {MAX_FLUSH_VOLUME} either way)",
                format_number(v)
            ),
        ));
    }
    Ok(orca_crt::f64_to_i32(v))
}

/// `float_values(cfg, key, out)`: every value of a list of floats, nullable or not.
fn float_values(cfg: &Config, key: &str) -> Option<Vec<f64>> {
    cfg.floats(key).cloned()
}

/// `bool_values(cfg, key, out)`.
fn bool_values(cfg: &Config, key: &str) -> Option<Vec<u8>> {
    cfg.bools(key).cloned()
}

/// `min_flush_volumes(cfg, nozzle_id)` (Plater.cpp's `get_min_flush_volumes`): each filament's
/// least flushing volume on the nozzle, its volume less the filament its retraction when cut
/// leaves in it; each volume and retraction length is turned into a whole number
/// ([`flush_input`]).
fn min_flush_volumes(cfg: &Config, nozzle: usize) -> Result<Vec<i32>, Failure> {
    let mut nozzle_volume_val = 0;
    if let Some(volumes) = float_values(cfg, "nozzle_volume")
        && let Some(first) = volumes.first()
    {
        let v = *volumes.get(nozzle).unwrap_or(first);
        nozzle_volume_val = if v.is_nan() {
            0
        } else {
            flush_input(v, "nozzle_volume")?
        };
    }
    let level = cfg.int("enable_long_retraction_when_cut").unwrap_or(0);
    let activated = bool_values(cfg, "long_retractions_when_cut")
        .is_some_and(|v| v.get(nozzle).is_some_and(|x| *x == 1));
    let size = float_values(cfg, "filament_diameter").map_or(0, |d| d.len());
    let filament_distance = float_values(cfg, "filament_retraction_distances_when_cut")
        .unwrap_or_else(|| vec![18.0; size]);
    let printer_distance =
        float_values(cfg, "retraction_distances_when_cut").unwrap_or_else(|| vec![18.0; size]);
    let filament_long =
        bool_values(cfg, "filament_long_retractions_when_cut").unwrap_or_else(|| vec![0; size]);
    let at = |v: &[f64], i: usize| v.get(i).copied().unwrap_or(18.0);
    let mut out = Vec::with_capacity(size);
    for idx in 0..size {
        let mut retract_length = if level != 0 && activated {
            flush_input(
                at(&printer_distance, nozzle),
                "retraction_distances_when_cut",
            )?
        } else {
            0
        };
        let filament_activated = filament_long.get(idx).copied().unwrap_or(0);
        let filament_length = at(&filament_distance, idx);
        if filament_activated == 0 {
            retract_length = 0;
        } else if filament_activated == 1 && level == LONG_RETRACTION_ENABLE_FILAMENT {
            retract_length = if filament_length.is_nan() {
                flush_input(
                    at(&printer_distance, nozzle),
                    "retraction_distances_when_cut",
                )?
            } else {
                flush_input(filament_length, "filament_retraction_distances_when_cut")?
            };
        }
        let retracted = orca_crt::f64_to_i32(PI * 1.75 * 1.75 / 4.0 * f64::from(retract_length));
        out.push(nozzle_volume_val.wrapping_sub(retracted));
    }
    Ok(out)
}

/// `is_hex_colour(s)`: `#rrggbb` or `#rrggbbaa`.
fn is_hex_colour(s: &str) -> bool {
    let b = s.as_bytes();
    (b.len() == 7 || b.len() == 9) && b[0] == b'#' && b[1..].iter().all(u8::is_ascii_hexdigit)
}

/// `set_filament_colours(cfg, extra, filaments)`: each filament's colour (its preset's
/// filament_colour, else default_filament_colour, else the GUI's), the job's filament_colour
/// override over them by position, and the project settings that follow them.
fn set_filament_colours(
    cfg: &mut Config,
    extra: &mut Config,
    filaments: &[LoadedPreset],
) -> Result<(), Failure> {
    let n = filaments.len();
    let mut colours = vec![String::new(); n];
    for (i, filament) in filaments.iter().enumerate() {
        for key in ["filament_colour", "default_filament_colour"] {
            if let Some(values) = filament.config.strings(key)
                && colours[i].is_empty()
                && let Some(first) = values.first()
                && is_hex_colour(first)
            {
                colours[i].clone_from(first);
            }
        }
        if colours[i].is_empty() {
            colours[i] = if i == 0 {
                ORCA_FILAMENT_COLOUR.to_string()
            } else {
                GUI_FILAMENT_COLOURS[(i - 1) % GUI_FILAMENT_COLOURS.len()].to_string()
            };
        }
    }
    if let Some(given) = extra.strings("filament_colour").cloned() {
        if given.len() > n {
            return Err(fail(
                "setting-invalid",
                format!(
                    "overrides: filament_colour gives {} colours for the job's {n} filament(s)",
                    given.len()
                ),
            ));
        }
        for (i, c) in given.iter().enumerate() {
            if c.is_empty() {
                continue;
            }
            if !is_hex_colour(c) {
                return Err(fail(
                    "setting-invalid",
                    format!(
                        "overrides: filament_colour: {} is not a colour (#rrggbb or #rrggbbaa)",
                        in_quotes(c)
                    ),
                ));
            }
            colours[i].clone_from(c);
        }
        extra.erase("filament_colour");
    }
    for key in ["filament_colour", "filament_multi_colour"] {
        if let Some(OptionValue::Strings(values)) = cfg.create(key) {
            values.clone_from(&colours);
        }
    }
    if let Some(OptionValue::Strings(values)) = cfg.create("filament_colour_type") {
        *values = vec!["1".to_string(); n];
    }
    if let Some(OptionValue::Ints { values, .. }) = cfg.create("filament_map") {
        values.resize(n, 1);
    }
    Ok(())
}

/// The number of values of the option `key`, made with its default when `cfg` has none
/// (`option(key, true)->size()`).
fn created_len(cfg: &mut Config, key: &str) -> usize {
    match cfg.create(key) {
        Some(v) => vec_len(v).unwrap_or(0),
        None => 0,
    }
}

/// `BitmapCache::parse_color4(s)`: red, green, blue and alpha of `#rrggbb` or `#rrggbbaa`; black
/// and opaque for anything else, the components read before a bad digit kept.
fn parse_color4(s: &str) -> [u8; 4] {
    let mut rgba = [0, 0, 0, 255];
    let b = s.as_bytes();
    if (b.len() != 7 && b.len() != 9) || b[0] != b'#' {
        return rgba;
    }
    let digit = |c: u8| char::from(c).to_digit(16);
    for i in 0..b.len() / 2 {
        let (Some(d1), Some(d2)) = (digit(b[1 + 2 * i]), digit(b[2 + 2 * i])) else {
            return rgba;
        };
        rgba[i] = u8::try_from(d1 * 16 + d2).unwrap_or(0);
    }
    rgba
}

/// `to_radians(degree)` of FlushVolCalc.cpp: a float division, then a double product, back to a
/// float.
fn to_radians(degree: f32) -> f32 {
    (f64::from(degree / 180.0) * PI) as f32
}

/// `get_luminance(r, g, b)`: computed in double (the constants are doubles), returned as a float.
fn get_luminance(r: f32, g: f32, b: f32) -> f32 {
    (f64::from(r) * 0.3 + f64::from(g) * 0.59 + f64::from(b) * 0.11) as f32
}

/// `calc_triangle_3rd_edge(edge_a, edge_b, degree_ab)`, in float.
fn calc_triangle_3rd_edge(edge_a: f32, edge_b: f32, degree_ab: f32) -> f32 {
    orca_crt::sqrtf(
        edge_a * edge_a + edge_b * edge_b
            - 2.0 * edge_a * edge_b * orca_crt::cosf(to_radians(degree_ab)),
    )
}

/// `DeltaHS_BBS(h1, s1, v1, h2, s2, v2)`: the distance of two colours' hue and saturation, at
/// most 1.2.
fn delta_hs_bbs(h1: f32, s1: f32, v1: f32, h2: f32, s2: f32, v2: f32) -> f32 {
    let h1_rad = to_radians(h1);
    let h2_rad = to_radians(h2);
    let dx = orca_crt::cosf(h1_rad) * s1 * v1 - orca_crt::cosf(h2_rad) * s2 * v2;
    let dy = orca_crt::sinf(h1_rad) * s1 * v1 - orca_crt::sinf(h2_rad) * s2 * v2;
    let dxy = orca_crt::sqrtf(dx * dx + dy * dy);
    orca_stl::min(1.2, dxy)
}

/// `RGB2HSV(r, g, b, &h, &s, &v)` (ColorSpaceConvert.cpp), in float.
fn rgb2hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let cmax = orca_stl::max(orca_stl::max(r, g), b);
    let cmin = orca_stl::min(orca_stl::min(r, g), b);
    let delta = cmax - cmin;
    let h = if f64::from(orca_crt::fabsf(delta)) < 0.001 {
        0.0
    } else if cmax == r {
        60.0 * orca_crt::fmodf((g - b) / delta, 6.0)
    } else if cmax == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    let s = if f64::from(orca_crt::fabsf(cmax)) < 0.001 {
        0.0
    } else {
        delta / cmax
    };
    (h, s, cmax)
}

/// A colour of the flush data (`FlushPredict::RGBColor`).
type Rgb = (u8, u8, u8);

/// `FlushPredict::RGB2LAB(color)`: CIE L*a*b* through XYZ, in double.
fn rgb2lab(c: Rgb) -> (f64, f64, f64) {
    let gamma = |x: f64| {
        if x > 0.04045 {
            orca_crt::pow((x + 0.055) / 1.055, 2.4)
        } else {
            x / 12.92
        }
    };
    let r = gamma(f64::from(c.0) / 255.0) * 100.0;
    let g = gamma(f64::from(c.1) / 255.0) * 100.0;
    let b = gamma(f64::from(c.2) / 255.0) * 100.0;
    let x = 0.412_453 * r + 0.357_580 * g + 0.180_423 * b;
    let y = 0.212_671 * r + 0.715_160 * g + 0.072_169 * b;
    let z = 0.019_334 * r + 0.119_193 * g + 0.950_227 * b;
    const XN: f64 = 95.0489;
    const YN: f64 = 100.0;
    const ZN: f64 = 108.8840;
    // `static const double threshold = 0.008856f`: the float literal, widened.
    let threshold = f64::from(0.008_856_f32);
    let f = |t: f64| {
        if t > threshold {
            orca_crt::pow(t, 1.0 / 3.0)
        } else {
            7.787 * t + 0.137_931
        }
    };
    let (xn, yn, zn) = (f(x / XN), f(y / YN), f(z / ZN));
    (116.0 * yn - 16.0, 500.0 * (xn - yn), 200.0 * (yn - zn))
}

/// `deg_to_rad(deg)` of FlushVolPredictor.cpp.
fn deg_to_rad(deg: f64) -> f64 {
    deg * PI / 180.0
}

/// `FlushPredict::calc_color_distance(lab1, lab2)`: CIEDE2000, in double, returned as a float.
#[allow(clippy::many_single_char_names, clippy::similar_names)]
fn calc_color_distance(lab1: (f64, f64, f64), lab2: (f64, f64, f64)) -> f32 {
    let pow = orca_crt::pow_int;
    let pow_25_to_7 = pow(25.0, 7);
    let c1 = orca_crt::sqrt(lab1.1 * lab1.1 + lab1.2 * lab1.2);
    let c2 = orca_crt::sqrt(lab2.1 * lab2.1 + lab2.2 * lab2.2);
    let c_mean = (c1 + c2) / 2.0;
    let pow_c_mean_to_7 = pow(c_mean, 7);
    let g = 0.5 * (1.0 - orca_crt::sqrt(pow_c_mean_to_7 / (pow_c_mean_to_7 + pow_25_to_7)));
    let (p_l1, p_l2) = (lab1.0, lab2.0);
    let p_a1 = (1. + g) * lab1.1;
    let p_a2 = (1. + g) * lab2.1;
    let (p_b1, p_b2) = (lab1.2, lab2.2);
    let p_c1 = orca_crt::sqrt(p_a1 * p_a1 + p_b1 * p_b1);
    let p_c2 = orca_crt::sqrt(p_a2 * p_a2 + p_b2 * p_b2);
    let hue = |b: f64, a: f64| {
        if a == 0.0 && b == 0.0 {
            0.0
        } else {
            let h = orca_crt::atan2(b, a);
            if h < 0.0 { h + PI * 2.0 } else { h }
        }
    };
    let p_h1 = hue(p_b1, p_a1);
    let p_h2 = hue(p_b2, p_a2);
    let delta_l = p_l2 - p_l1;
    let delta_c = p_c2 - p_c1;
    let p_c_multi = p_c1 * p_c2;
    let delta_h = if p_c_multi == 0.0 {
        0.0
    } else {
        let mut d = p_h2 - p_h1;
        if d < -PI {
            d += 2.0 * PI;
        } else if d > PI {
            d -= 2.0 * PI;
        }
        2.0 * orca_crt::sqrt(p_c_multi) * orca_crt::sin(d / 2.)
    };
    let p_l_mean = (p_l1 + p_l2) / 2.0;
    let p_c_mean = (p_c1 + p_c2) / 2.0;
    let p_h_sum = p_h1 + p_h2;
    let p_h_mean = if p_c1 * p_c2 == 0.0 {
        p_h_sum
    } else if orca_crt::fabs(p_h1 - p_h2) <= PI {
        p_h_sum / 2.0
    } else if p_h_sum < 2.0 * PI {
        (p_h_sum + 2.0 * PI) / 2.0
    } else {
        (p_h_sum - 2.0 * PI) / 2.0
    };
    let t = 1.0 - 0.17 * orca_crt::cos(p_h_mean - deg_to_rad(30.0))
        + 0.24 * orca_crt::cos(2.0 * p_h_mean)
        + 0.32 * orca_crt::cos(3.0 * p_h_mean + deg_to_rad(6.0))
        - 0.2 * orca_crt::cos(4.0 * p_h_mean - deg_to_rad(63.0));
    let dtheta = deg_to_rad(30.0)
        * orca_crt::exp(-pow((p_h_mean - deg_to_rad(275.0)) / deg_to_rad(25.0), 2));
    let pow_p_cmean_to_7 = pow(p_c_mean, 7);
    let r_c = 2.0 * orca_crt::sqrt(pow_p_cmean_to_7 / (pow_p_cmean_to_7 + pow_25_to_7));
    let pow_p_lmean_to_2 = pow(p_l_mean - 50.0, 2);
    let s_l = 1.0 + (0.015 * pow_p_lmean_to_2) / orca_crt::sqrt(20.0 + pow_p_lmean_to_2);
    let s_c = 1.0 + 0.045 * p_c_mean;
    let s_h = 1.0 + 0.015 * p_c_mean * t;
    let r_t = -orca_crt::sin(2.0 * dtheta) * r_c;
    let (k_l, k_c, k_h) = (1.0, 1.0, 1.0);
    let de = orca_crt::sqrt(
        pow(delta_l / (k_l * s_l), 2)
            + pow(delta_c / (k_c * s_c), 2)
            + pow(delta_h / (k_h * s_h), 2)
            + (r_t * (delta_c / (k_c * s_c)) * (delta_h / (k_h * s_h))),
    );
    de as f32
}

/// `FlushPredict::is_similar_color(from, to)`: within a CIEDE2000 distance of 5.
fn is_similar_color(from: Rgb, to: Rgb) -> bool {
    calc_color_distance(rgb2lab(from), rgb2lab(to)) <= 5.0
}

/// One `FlushVolPredictor`: the colours of a flush data file and the measured volume between
/// each pair it lists; an invalid one predicts nothing.
#[derive(Default)]
struct Predictor {
    colors: Vec<Rgb>,
    flush: BTreeMap<u64, f32>,
    valid: bool,
}

/// `generate_hash_key(from, to)`.
fn flush_key(from: Rgb, to: Rgb) -> u64 {
    (u64::from(from.0) << 40)
        | (u64::from(from.1) << 32)
        | (u64::from(from.2) << 24)
        | (u64::from(to.0) << 16)
        | (u64::from(to.1) << 8)
        | u64::from(to.2)
}

/// `rgb_hex_to_dec(hexstr, color)`: `#rrggbb`, each pair read as `std::hex` reads an unsigned
/// (its leading hex digits; the data's colours have two).
fn rgb_hex_to_dec(hex: &str) -> Option<Rgb> {
    let b = hex.as_bytes();
    if b.len() != 7 || b[0] != b'#' {
        return None;
    }
    let byte = |pair: &[u8]| -> u8 {
        let mut v: u32 = 0;
        for &c in pair {
            match char::from(c).to_digit(16) {
                Some(d) => v = v * 16 + d,
                None => break,
            }
        }
        u8::try_from(v).unwrap_or(0)
    };
    Some((byte(&b[1..3]), byte(&b[3..5]), byte(&b[5..7])))
}

/// What `iss >> value` reads into a float from a token: its longest leading number.
fn leading_float(token: &str) -> Option<f32> {
    let b = token.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let digits_from = |mut j: usize| {
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        j
    };
    let int_end = digits_from(i);
    let mut end = int_end;
    if end < b.len() && b[end] == b'.' {
        end = digits_from(end + 1);
    }
    if end == i || (end == i + 1 && b[i] == b'.') {
        return None;
    }
    if end < b.len() && (b[end] == b'e' || b[end] == b'E') {
        let mut j = end + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let k = digits_from(j);
        if k > j {
            end = k;
        }
    }
    token[..end].parse::<f32>().ok()
}

impl Predictor {
    /// `FlushVolPredictor(data_file)`: the file read as a text-mode stream reads it (each CR LF
    /// one line feed, a Ctrl-Z its end): a description line, the colours, the column names, then
    /// one `from to volume` line per pair (the first of a pair kept). A file that cannot be opened,
    /// a colour that is not `#rrggbb` or a line that is not two colours and a number makes it
    /// invalid.
    fn load(path: &str) -> Predictor {
        let Ok(raw) = std::fs::read(path) else {
            return Predictor::default();
        };
        let raw = raw.split(|&c| c == 0x1A).next().unwrap_or(&[]);
        let text = String::from_utf8_lossy(raw).replace("\r\n", "\n");
        let mut lines = text.split('\n');
        if text.is_empty() {
            lines.next();
        }
        let mut p = Predictor::default();
        let _description = lines.next();
        if let Some(colors) = lines.next() {
            for token in colors.split_ascii_whitespace() {
                match rgb_hex_to_dec(token) {
                    Some(c) => p.colors.push(c),
                    None => return Predictor::default(),
                }
            }
        }
        let _columns = lines.next();
        let mut rest: Vec<&str> = lines.collect();
        // getline gives no line after a final line feed.
        if rest.last() == Some(&"") {
            rest.pop();
        }
        for line in rest {
            let mut tokens = line.split_ascii_whitespace();
            let (Some(from), Some(to), Some(value)) = (
                tokens.next(),
                tokens.next(),
                tokens.next().and_then(leading_float),
            ) else {
                return Predictor { valid: false, ..p };
            };
            let (Some(from), Some(to)) = (rgb_hex_to_dec(from), rgb_hex_to_dec(to)) else {
                return Predictor { valid: false, ..p };
            };
            p.flush.entry(flush_key(from, to)).or_insert(value);
        }
        p.valid = true;
        p
    }

    /// `predict(from, to, flush)`: the volume measured between the first colours of the data
    /// similar to each.
    fn predict(&self, from: Rgb, to: Rgb) -> Option<f32> {
        if !self.valid {
            return None;
        }
        let similar_from = self.colors.iter().find(|c| is_similar_color(**c, from))?;
        let similar_to = self.colors.iter().find(|c| is_similar_color(**c, to))?;
        self.flush
            .get(&flush_key(*similar_from, *similar_to))
            .copied()
    }
}

/// `GenericFlushPredictor`'s instances by dataset (`predictor_instances`): dataset 0, 1 and 2 read
/// `flush_data_standard.txt`, `flush_data_dual_standard.txt` and `flush_data_dual_highflow.txt`
/// from the resources' `flush` folder; another dataset names the folder itself, which does not
/// open.
struct Predictors<'a> {
    resources: &'a str,
    by_dataset: BTreeMap<i32, Predictor>,
}

impl Predictors<'_> {
    fn predict(&mut self, dataset: i32, from: Rgb, to: Rgb) -> Option<f32> {
        let resources = self.resources;
        self.by_dataset
            .entry(dataset)
            .or_insert_with(|| {
                let mut path = resources.to_string();
                match dataset {
                    0 => path.push_str("/flush/flush_data_standard.txt"),
                    1 => path.push_str("/flush/flush_data_dual_standard.txt"),
                    2 => path.push_str("/flush/flush_data_dual_highflow.txt"),
                    _ => {}
                }
                Predictor::load(&path)
            })
            .predict(from, to)
    }
}

/// `FlushVolCalculator::calc_flush_vol_rgb(src, dst)`: dataset 0's measured volume, else Orca's
/// formula on hue, saturation and luminance (at least 60).
fn calc_flush_vol_rgb(predictors: &mut Predictors, dataset: i32, src: Rgb, dst: Rgb) -> i32 {
    if dataset == 0
        && let Some(flush) = predictors.predict(dataset, src, dst)
    {
        return orca_crt::f32_to_i32(flush);
    }
    let (src_r, src_g, src_b) = (
        f32::from(src.0) / 255.0,
        f32::from(src.1) / 255.0,
        f32::from(src.2) / 255.0,
    );
    let (dst_r, dst_g, dst_b) = (
        f32::from(dst.0) / 255.0,
        f32::from(dst.1) / 255.0,
        f32::from(dst.2) / 255.0,
    );
    let (from_h, from_s, from_v) = rgb2hsv(src_r, src_g, src_b);
    let (to_h, to_s, to_v) = rgb2hsv(dst_r, dst_g, dst_b);
    let mut hs_dist = delta_hs_bbs(from_h, from_s, from_v, to_h, to_s, to_v);
    let from_lumi = get_luminance(src_r, src_g, src_b);
    let to_lumi = get_luminance(dst_r, dst_g, dst_b);
    let lumi_flush = if to_lumi >= from_lumi {
        orca_crt::powf(to_lumi - from_lumi, 0.7) * 560.0
    } else {
        let inter_hsv_v = (0.67 * f64::from(to_v) + 0.33 * f64::from(from_v)) as f32;
        hs_dist = orca_stl::min(inter_hsv_v, hs_dist);
        (from_lumi - to_lumi) * 80.0
    };
    let hs_flush = 230.0 * hs_dist;
    let flush_volume = orca_stl::max(calc_triangle_3rd_edge(hs_flush, lumi_flush, 120.0), 60.0);
    orca_crt::f32_to_i32(flush_volume)
}

/// `FlushVolCalculator(min, max, dataset).calc_flush_vol(src, dst)` for two `parse_color4`
/// colours: a transparent colour taken as white; another dataset's measured volume, else the
/// formula (with 30% more from a dark colour to a light one for those datasets) plus the minimum,
/// at most `max`.
fn calc_flush_vol(
    predictors: &mut Predictors,
    min: i32,
    max: i32,
    dataset: i32,
    src: [u8; 4],
    dst: [u8; 4],
) -> i32 {
    let white = |c: [u8; 4]| -> Rgb {
        if c[3] == 0 {
            (255, 255, 255)
        } else {
            (c[0], c[1], c[2])
        }
    };
    let (src, dst) = (white(src), white(dst));
    if dataset != 0
        && let Some(flush) = predictors.predict(dataset, src, dst)
    {
        return orca_stl::min(orca_crt::f32_to_i32(flush), max);
    }
    // An int into a float, exactly.
    #[allow(clippy::cast_precision_loss)]
    let mut flush_volume = calc_flush_vol_rgb(predictors, dataset, src, dst) as f32;
    let dark_color_thres: f32 = 180.0 / 255.0;
    let light_color_thres: f32 = 75.0 / 255.0;
    let lum = |c: Rgb| get_luminance(f32::from(c.0), f32::from(c.1), f32::from(c.2));
    let is_from_dark = lum(src) > dark_color_thres;
    let is_to_light = lum(dst) < light_color_thres;
    if dataset != 0 && is_from_dark && is_to_light {
        flush_volume = (f64::from(flush_volume) * 1.3) as f32;
    }
    #[allow(clippy::cast_precision_loss)]
    {
        flush_volume += min as f32;
    }
    orca_stl::min(orca_crt::f32_to_i32(flush_volume), max)
}

/// `set_flush_volumes_matrix(out, matrix, nozzle, nozzle_count)`: one nozzle's n x n block at its
/// place in the matrix of every nozzle; with one nozzle, the whole matrix.
fn set_flush_volumes_matrix(out: &mut Vec<f64>, m: &[f64], nozzle: usize, nozzle_count: usize) {
    if nozzle_count != 1 {
        let start = out.len() / nozzle_count * nozzle;
        out[start..start + m.len()].copy_from_slice(m);
    } else {
        *out = m.to_vec();
    }
}

/// `boost::split(out, s, is_any_of(sep), token_compress_on)` (config.rs has its own; it is not
/// shared with this file).
fn split_compress(s: &str, sep: char) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut in_separators = false;
    for c in s.chars() {
        if c == sep {
            if !in_separators {
                out.push(String::new());
                in_separators = true;
            }
        } else {
            in_separators = false;
            if let Some(last) = out.last_mut() {
                last.push(c);
            }
        }
    }
    out
}

/// `get_extruder_variant_string(extruder_type, nozzle_volume_type)`.
fn extruder_variant_string(extruder_type: i32, nozzle_volume_type: i32) -> String {
    const EXTRUDERS: [&str; 2] = ["Direct Drive", "Bowden"];
    const VOLUMES: [&str; 2] = ["Standard", "High Flow"];
    match (
        usize::try_from(extruder_type)
            .ok()
            .and_then(|i| EXTRUDERS.get(i)),
        usize::try_from(nozzle_volume_type)
            .ok()
            .and_then(|i| VOLUMES.get(i)),
    ) {
        (Some(e), Some(v)) => format!("{e} {v}"),
        _ => String::new(),
    }
}

/// `get_index_for_extruder(id, "printer_extruder_id", extruder_type, nozzle_volume_type,
/// "printer_extruder_variant")` (stride 1): the printer variant of that extruder type and nozzle
/// volume type for extruder `id`, -1 when there is none. The same as config.rs's, which is
/// private to it.
fn printer_variant_index(
    cfg: &Config,
    id: i32,
    extruder_type: i32,
    nozzle_volume_type: i32,
) -> i32 {
    let Some(variants) = cfg.strings("printer_extruder_variant") else {
        return -1;
    };
    let ids = match cfg.get("printer_extruder_id") {
        Some(OptionValue::Ints {
            values,
            nullable: false,
        }) => Some(values),
        _ => None,
    };
    let list = cfg.strings("extruder_variant_list");
    let generated_extruder_id = |target: usize| -> i32 {
        let Some(list) = list else {
            return 0;
        };
        let mut variant_index = 0;
        for (extruder_index, entry) in list.iter().enumerate() {
            for variant in split_compress(entry, ',') {
                if config::trim(variant.as_bytes()).is_empty() {
                    continue;
                }
                if variant_index == target {
                    return i32::try_from(extruder_index + 1).unwrap_or(i32::MAX);
                }
                variant_index += 1;
            }
        }
        0
    };
    let complete = ids.is_some_and(|ids| ids.len() >= variants.len());
    let wanted = extruder_variant_string(extruder_type, nozzle_volume_type);
    for (index, variant) in variants.iter().enumerate() {
        if *variant != wanted {
            continue;
        }
        let found = match ids {
            Some(ids) => {
                let got = if complete {
                    ids[index]
                } else {
                    generated_extruder_id(index)
                };
                got == id
            }
            None => true,
        };
        if found {
            return i32::try_from(index).unwrap_or(-1);
        }
    }
    -1
}

/// The flushing volumes between filaments (`compute_flush_volumes`): a job's own
/// flush_volumes_matrix must have n x n values for each nozzle and is kept; otherwise
/// filament_is_support must give one value per filament, and each nozzle's matrix is computed
/// from the filaments' colours ([`calc_flush_vol`]) with its `nozzle_flush_dataset` (the printer
/// variant its extruder type and nozzle volume type pick) and its minimum volumes, which come from
/// the extruder-variant picks Orca makes on a copy with every setting ([`min_flush_volumes`]); a
/// flush to a support filament is 230, one from a support filament at least 700.
#[allow(clippy::too_many_lines)]
fn compute_flush_volumes(
    cfg: &mut Config,
    matrix_given: bool,
    extruder_count: usize,
    printer_variant_count: usize,
    resources: &str,
) -> Result<(), Failure> {
    let colors = match cfg.create("filament_colour") {
        Some(OptionValue::Strings(values)) => values.clone(),
        _ => Vec::new(),
    };
    let n = colors.len();
    if n == 0 {
        return Ok(());
    }
    if let Some(OptionValue::Floats { values, .. }) = cfg.create("flush_multiplier") {
        values.resize(extruder_count, 1.0);
    }
    if matrix_given {
        let given = created_len(cfg, "flush_volumes_matrix");
        let want = n * n * extruder_count;
        if given != want {
            return Err(fail(
                "setting-invalid",
                format!(
                    "overrides: flush_volumes_matrix has {given} values; with {n} filament(s) and {extruder_count} nozzle(s) it needs {want} (one row of {n} per filament, for each nozzle)"
                ),
            ));
        }
        return Ok(());
    }
    let is_support = match cfg.create("filament_is_support") {
        Some(OptionValue::Bools { values, .. }) => values.clone(),
        _ => Vec::new(),
    };
    if is_support.len() != n {
        return Err(fail(
            "preset-invalid",
            format!(
                "the settings give {} filament_is_support values for {n} filaments",
                is_support.len()
            ),
        ));
    }

    let mut matrix = vec![0.0; n * n * extruder_count];

    let mut dataset = vec![0_i32; extruder_count];
    {
        let mut full = match cfg.create("nozzle_flush_dataset") {
            Some(OptionValue::Ints { values, .. }) => values.clone(),
            _ => Vec::new(),
        };
        let size = if cfg.has("printer_extruder_variant") {
            orca_stl::max(printer_variant_count, 1)
        } else {
            1
        };
        full.resize(size, 0);
        let enums = |key: &str, default: i32| match cfg.get(key) {
            Some(OptionValue::Enums {
                values,
                nullable: false,
                ..
            }) => values.clone(),
            _ => vec![default],
        };
        // etDirectDrive and nvtStandard.
        let extruders = enums("extruder_type", 0);
        let volume_types = enums("nozzle_volume_type", 0);
        for (e, slot) in dataset.iter_mut().enumerate() {
            let mut index = 0;
            if cfg.has("printer_extruder_id")
                && cfg.has("printer_extruder_variant")
                && !extruders.is_empty()
                && !volume_types.is_empty()
            {
                index = printer_variant_index(
                    cfg,
                    i32::try_from(e + 1).unwrap_or(i32::MAX),
                    extruders[orca_stl::min(e, extruders.len() - 1)],
                    volume_types[orca_stl::min(e, volume_types.len() - 1)],
                );
            }
            *slot = match usize::try_from(index) {
                Ok(i) if i < full.len() => full[i],
                _ => full[0],
            };
        }
    }

    // The settings each nozzle and filament prints with, as Print::apply picks them, on a copy
    // with every setting (the machine limits' picks, printer_options_with_variant_2, left out:
    // the volumes do not read them).
    let mut picked = config::with_full_print_config(cfg);
    config::update_values_to_printer_extruders(&mut picked);
    config::update_values_to_printer_extruders_for_multiple_filaments(&mut picked);

    let mut predictors = Predictors {
        resources,
        by_dataset: BTreeMap::new(),
    };
    for (nozzle, &nozzle_dataset) in dataset.iter().enumerate() {
        let min_volumes = min_flush_volumes(&picked, nozzle)?;
        let mut m = vec![0.0; n * n];
        for from in 0..n {
            let from_rgb = parse_color4(&colors[from]);
            let from_support = is_support[from] != 0;
            for to in 0..n {
                if from == to {
                    continue;
                }
                let volume = if is_support[to] != 0 {
                    FLUSH_VOLUME_TO_SUPPORT
                } else {
                    let to_rgb = parse_color4(&colors[to]);
                    let min = min_volumes.get(from).copied().unwrap_or(0);
                    let v = calc_flush_vol(
                        &mut predictors,
                        min,
                        MAX_FLUSH_VOLUME,
                        nozzle_dataset,
                        from_rgb,
                        to_rgb,
                    );
                    if from_support {
                        orca_stl::max(MIN_FLUSH_VOLUME_FROM_SUPPORT, v)
                    } else {
                        v
                    }
                };
                m[n * from + to] = f64::from(volume);
            }
        }
        set_flush_volumes_matrix(&mut matrix, &m, nozzle, extruder_count);
    }
    if let Some(OptionValue::Floats { values, .. }) = cfg.create("flush_volumes_matrix") {
        *values = matrix;
    }
    Ok(())
}

/// `find_printer_model_id(resources, printer_model)`: the `model_id` of the printer model in the
/// vendor bundles of resources/profiles (each `<vendor>.json`, in path order, whose
/// `machine_model_list` names it, read from the entry's `sub_path` in the vendor's folder), or ""
/// (a bundle that cannot be read, or whose entry or model file does not give text, is passed
/// over).
fn find_printer_model_id(resources: &str, printer_model: &str) -> String {
    if printer_model.is_empty() {
        return String::new();
    }
    let profiles = std::path::Path::new(resources).join("profiles");
    let Ok(entries) = std::fs::read_dir(&profiles) else {
        return String::new();
    };
    let mut vendors: Vec<std::path::PathBuf> = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            break;
        };
        let path = entry.path();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
        let is_json = name
            .as_deref()
            .and_then(|n| n.rfind('.').map(|i| &n[i..]))
            .is_some_and(|ext| ext.eq_ignore_ascii_case(".json"));
        if is_json && path.is_file() {
            vendors.push(path);
        }
    }
    // boost::filesystem::path's order: element by element, by UTF-16 code unit.
    let key = |p: &std::path::PathBuf| -> Vec<u16> {
        p.file_name()
            .map(|n| n.to_string_lossy().encode_utf16().collect())
            .unwrap_or_default()
    };
    orca_stl::sort_by(&mut vendors, |a, b| key(a) < key(b));
    for index in &vendors {
        // nlohmann's value(key, ""): the default where the key is missing; a type_error (the
        // catch passes the vendor over) where the value is not text or the JSON not an object.
        let text = |v: &Value, k: &str| -> Option<String> {
            match v.as_object()?.get(k) {
                None => Some(String::new()),
                Some(Value::String(s)) => Some(s.clone()),
                Some(_) => None,
            }
        };
        let Ok(bytes) = read_file(&index.to_string_lossy()) else {
            continue;
        };
        let Ok(j) = orca_json::parse(&bytes, Order::Sorted) else {
            continue;
        };
        let Some(Value::Array(list)) = j.get("machine_model_list") else {
            continue;
        };
        let stem = index
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .map(|n| match n.rfind('.') {
                Some(i) => n[..i].to_string(),
                None => n,
            })
            .unwrap_or_default();
        for entry in list {
            if entry.as_object().is_none() {
                continue;
            }
            let Some(name) = text(entry, "name") else {
                break;
            };
            if name != printer_model {
                continue;
            }
            let Some(sub_path) = text(entry, "sub_path") else {
                break;
            };
            // boost::filesystem v3's operator/: a separator between, unless one is there.
            let mut model_file = profiles.join(&stem).to_string_lossy().into_owned();
            if !sub_path.is_empty()
                && !sub_path.starts_with(['/', '\\'])
                && !model_file.ends_with(['/', '\\', ':'])
            {
                model_file.push('\\');
            }
            model_file.push_str(&sub_path);
            let model = read_file(&model_file)
                .ok()
                .and_then(|b| orca_json::parse(&b, Order::Sorted).ok());
            return match model.as_ref().map(|m| text(m, "model_id")) {
                Some(Some(id)) => id,
                // The exception goes to the next vendor.
                _ => break,
            };
        }
    }
    String::new()
}

/// `default_bed_type(machine, model_id)` (Preset::get_default_bed_type): the printer's
/// default_bed_type (`strtol`'s number; btPEI past the plates), else the PC plate for the Bambu Lab
/// X1 Carbon, X1 and X1E, else PEI.
fn default_bed_type(machine: &Config, model_id: &str) -> i32 {
    if let Some(OptionValue::String(text)) = machine.get("default_bed_type")
        && !text.is_empty()
    {
        let v = strtol(text);
        return if v > 0 && v < i64::from(BED_COUNT) {
            i32::try_from(v).unwrap_or(BED_PEI)
        } else {
            BED_PEI
        };
    }
    if model_id == "BL-P001" || model_id == "BL-P002" || model_id == "C13" {
        return BED_PC;
    }
    BED_PEI
}

/// `std::strtol(s, nullptr, 10)` with a 32-bit `long`: leading white space, a sign and digits,
/// clamped to `LONG_MIN`..`LONG_MAX`; 0 without digits.
fn strtol(s: &str) -> i64 {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r') {
        i += 1;
    }
    let neg = i < b.len() && b[i] == b'-';
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let mut v: i64 = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        v = orca_stl::min(v * 10 + i64::from(b[i] - b'0'), i64::from(i32::MAX) + 1);
        i += 1;
    }
    if neg {
        orca_stl::max(-v, i64::from(i32::MIN))
    } else {
        orca_stl::min(v, i64::from(i32::MAX))
    }
}

/// `per_filament_key(key)`: a setting of `Preset::filament_options` that holds one value per
/// filament (those with extruder variants hold one per filament and variant).
fn per_filament_key(key: &str) -> bool {
    key != "compatible_prints"
        && key != "compatible_printers"
        && !FILAMENT_OPTIONS_WITH_VARIANT.contains(&key)
}

/// `pad_filament_options(cfg, given, n)`: a filament that does not give a setting gets Orca's
/// default at its index.
fn pad_filament_options(
    cfg: &mut Config,
    given: &BTreeMap<String, Vec<bool>>,
    n: usize,
) -> Result<(), ConfigError> {
    for key in orca_config::FILAMENT_OPTIONS {
        if !per_filament_key(key) {
            continue;
        }
        let dflt = orca_config::get(key).and_then(config::default_value);
        let Some(opt) = cfg.get_mut(key) else {
            continue;
        };
        if !opt.ty().is_vector() {
            continue;
        }
        let has_default = dflt
            .as_ref()
            .is_some_and(|d| d.ty().is_vector() && vec_len(d).unwrap_or(0) > 0);
        let gave = given.get(*key);
        for i in 0..n {
            let size = vec_len(opt).unwrap_or(0);
            if gave.is_some_and(|g| g.get(i) == Some(&true)) && i < size {
                continue;
            }
            match &dflt {
                Some(d) if has_default => {
                    if size == 0 {
                        resize(opt, 1, Some(d))?;
                    }
                    set_at(opt, d, i, 0)?;
                }
                _ => {
                    if size > 0 && size <= i {
                        resize(opt, i + 1, None)?;
                    }
                }
            }
        }
    }
    Ok(())
}

/// `size_filament_options(cfg, n)`: each per-filament setting holds one value per filament.
fn size_filament_options(cfg: &mut Config, n: usize) -> Result<(), ConfigError> {
    for key in orca_config::FILAMENT_OPTIONS {
        if !per_filament_key(key) {
            continue;
        }
        let Some(opt) = cfg.get_mut(key) else {
            continue;
        };
        if let Some(size) = vec_len(opt)
            && size > 0
            && size < n
        {
            resize(opt, n, None)?;
        }
    }
    Ok(())
}

/// `check_filament_overrides(extra, cfg, n)`: an override of a filament setting gives one value
/// for every filament or one per filament (per filament and variant for the settings with
/// extruder variants); one value is repeated for every filament.
fn check_filament_overrides(extra: &mut Config, cfg: &Config, n: usize) -> Result<(), Failure> {
    const PROJECT_KEYS: [&str; 3] = [
        "filament_multi_colour",
        "filament_colour_type",
        "filament_map",
    ];
    for key in extra.keys() {
        let variant = FILAMENT_OPTIONS_WITH_VARIANT.contains(&key.as_str());
        let filament =
            orca_config::FILAMENT_OPTIONS.contains(&key.as_str()) && per_filament_key(&key);
        if !PROJECT_KEYS.contains(&key.as_str()) && !filament && !variant {
            continue;
        }
        let Some(opt) = extra.get_mut(&key) else {
            continue;
        };
        let Some(size) = vec_len(opt) else {
            continue;
        };
        let mut want = n;
        if variant
            && let Some(variants) = cfg.strings("filament_extruder_variant")
            && !variants.is_empty()
        {
            want = variants.len();
        }
        if size == want {
            continue;
        }
        if size == 1 {
            resize(opt, want, None).map_err(|e| internal(&e))?;
            continue;
        }
        let per = if want == n {
            " (one per filament)"
        } else {
            " (one per filament and extruder variant)"
        };
        return Err(fail(
            "setting-invalid",
            format!(
                "overrides: {key} gives {size} values; give one for every filament, or {want}{per}"
            ),
        ));
    }
    Ok(())
}

/// `whole_number(item, integer, percent)`: the whole item is one number (an integer, or with a
/// trailing % where the setting takes a percentage), as `strtoll` or `strtod` reads it.
fn whole_number(item: &[u8], integer: bool, percent: bool) -> bool {
    let is_space = orca_fmt::istream::is_space;
    let start = item
        .iter()
        .position(|c| !is_space(*c))
        .unwrap_or(item.len());
    let end = item
        .iter()
        .rposition(|c| !is_space(*c))
        .map_or(start, |i| i + 1);
    let mut item = &item[start..end];
    if percent && let Some(rest) = item.strip_suffix(b"%") {
        item = rest;
    }
    if item.is_empty() {
        return false;
    }
    // `c_str()`: the text up to its first NUL.
    let c_len = item.iter().position(|c| *c == 0).unwrap_or(item.len());
    let read = if integer {
        orca_fmt::parse::strtoll(item, 10).end
    } else {
        let r = orca_fmt::parse::strtod(item);
        if !r.value.is_finite() {
            return false;
        }
        r.end
    };
    read == c_len
}

/// `readable_numbers(def, value)`: a number setting is taken only when each of its values (comma
/// separated) is a number, or nil where the setting has it.
fn readable_numbers(def: &orca_config::OptionDef, value: &str) -> bool {
    let (list, integer, percent) = match def.ty {
        OptionType::Float => (false, false, false),
        OptionType::Int => (false, true, false),
        OptionType::Percent | OptionType::FloatOrPercent => (false, false, true),
        OptionType::Floats => (true, false, false),
        OptionType::Ints => (true, true, false),
        OptionType::Percents | OptionType::FloatsOrPercents => (true, false, true),
        _ => return true,
    };
    if !list {
        return whole_number(value.as_bytes(), integer, percent);
    }
    value.as_bytes().split(|c| *c == b',').all(|item| {
        let first = item.iter().position(|c| *c != b' ' && *c != b'\t');
        let last = item.iter().rposition(|c| *c != b' ' && *c != b'\t');
        let nil = matches!((first, last), (Some(f), Some(l)) if &item[f..=l] == b"nil");
        (nil && def.nullable) || whole_number(item, integer, percent)
    })
}

/// `parse_setting(into, key, value, field)`: one setting read into `into` as the stock command
/// line reads its own options (Orca's renames, no substitutions); the key it was stored under.
///
/// # Errors
///
/// `setting-invalid` naming `field`: a key Orca does not have, a resin (SLA) setting, or a value
/// it cannot read, with a hint of what it takes.
pub fn parse_setting(
    into: &mut Config,
    key: &str,
    value: &str,
    field: &str,
) -> Result<String, Failure> {
    let (mut stored, mut legacy_value) = (key.to_string(), value.to_string());
    orca_config::handle_legacy(&mut stored, &mut legacy_value);
    let unknown = || {
        fail(
            "setting-invalid",
            format!("{field}: {key} is not a setting of Orca {ORCA_VERSION}"),
        )
    };
    if stored.is_empty() {
        return Err(unknown());
    }
    let Some(def) = find_def(&stored) else {
        return Err(unknown());
    };
    if def.printer_technology == PrinterTechnology::Sla {
        return Err(fail(
            "setting-invalid",
            format!("{field}: {key} is a resin (SLA) setting"),
        ));
    }
    let mut ok = readable_numbers(def, value);
    if ok {
        let mut log = Vec::new();
        ok = matches!(
            into.set_deserialize(key, value, Rule::Disable, &mut log),
            Ok(true)
        );
    }
    if !ok {
        let lists = matches!(
            def.ty,
            OptionType::Floats
                | OptionType::Ints
                | OptionType::Percents
                | OptionType::FloatsOrPercents
        );
        let numbers = matches!(
            def.ty,
            OptionType::Float | OptionType::Int | OptionType::Percent | OptionType::FloatOrPercent
        );
        let hint = if !def.enum_values.is_empty() {
            format!(" (one of: {})", def.enum_values.join(", "))
        } else if lists && value.contains(';') {
            " (a list of numbers is written with commas, one value per filament or extruder)"
                .to_string()
        } else if lists {
            " (numbers, separated by commas)".to_string()
        } else if numbers {
            " (a number)".to_string()
        } else {
            String::new()
        };
        return Err(fail(
            "setting-invalid",
            format!("{field}: {key} cannot be {}{hint}", in_quotes(value)),
        ));
    }
    Ok(def.key.to_string())
}

impl Symbols for Config {
    fn option(&self, key: &str) -> Option<&OptionValue> {
        self.get(key)
    }
}

/// A preset's configuration as the compatibility checks read it (`make_preset(type, loaded,
/// option_keys)`): Orca's default of each setting of its type (`new_from_defaults_keys`), the
/// file's values over them.
fn preset_config(loaded: &Config, keys: &[&str]) -> Result<Config, Failure> {
    let mut c = Config::default();
    for key in keys {
        if let Some(def) = orca_config::get(key) {
            c.insert(key, config::create_default(def));
        }
    }
    c.apply(loaded).map_err(|e| internal(&e))?;
    Ok(c)
}

/// `log_level()` of cli/main.cpp: SKETCH_ORCA_LOG_LEVEL as `strtoul` reads it, 0 to 5, else the
/// stock command line's 1.
fn log_level() -> u32 {
    const STOCK: u32 = 1;
    let Some(env) = std::env::var_os("SKETCH_ORCA_LOG_LEVEL") else {
        return STOCK;
    };
    let text = env.to_string_lossy();
    let b = text.as_bytes();
    if b.is_empty() {
        return STOCK;
    }
    let mut i = 0;
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r') {
        i += 1;
    }
    let neg = i < b.len() && b[i] == b'-';
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let start = i;
    let mut v: u64 = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        v = orca_stl::min(v * 10 + u64::from(b[i] - b'0'), u64::from(u32::MAX) + 1);
        i += 1;
    }
    if i == start || i != b.len() || v > u64::from(u32::MAX) {
        return STOCK;
    }
    let v = u32::try_from(v).unwrap_or(u32::MAX);
    let v = if neg { v.wrapping_neg() } else { v };
    if v <= 5 { v } else { STOCK }
}

/// The C string of a `std::string` (`c_str()`, which `printf("%s")` reads to its first NUL).
fn c_str(s: &str) -> &str {
    s.split('\0').next().unwrap_or("")
}

/// A compatibility: whether the preset is made for the printer or the process, or why its
/// condition is past what the placeholder parser takes (the job is declined).
type Compat = Result<bool, String>;

/// Whether a preset is made for the printer (`is_compatible_with_printer`): with no
/// compatible_printers list, its compatible_printers_condition holds for the printer preset
/// (`printer_config`, with `printer_preset`, the printer's name, and `num_extruders`, its count of
/// nozzle diameters, over it), or there is none, or it does not parse (logged as Orca logs it, a
/// warning on stderr at log level 2 or more); with a list, the list names the printer, or the
/// parent of a printer that is not a system preset, or the printer has no name.
fn compatible_with_printer(
    preset: &Config,
    printer: &LoadedPreset,
    printer_config: &Config,
) -> Compat {
    let list = preset
        .strings("compatible_printers")
        .filter(|l| !l.is_empty());
    let Some(list) = list else {
        let condition = preset.string("compatible_printers_condition");
        if condition.is_empty() {
            return Ok(true);
        }
        let mut over: BTreeMap<String, OptionValue> = BTreeMap::new();
        over.insert(
            "printer_preset".to_string(),
            OptionValue::String(printer.name.clone()),
        );
        if let Some(count) = printer_config.get("nozzle_diameter").and_then(vec_len) {
            over.insert(
                "num_extruders".to_string(),
                OptionValue::Int(i32::try_from(count).unwrap_or(i32::MAX)),
            );
        }
        return match evaluate_boolean_expression(condition.as_bytes(), printer_config, Some(&over))
        {
            Ok(b) => Ok(b),
            Err(ConditionError::Parse(what)) => {
                if log_level() >= 2 {
                    // Boost.Log's record, its newline added where the message has none.
                    let mut line = format!(
                        "[warning] Slic3r::is_compatible_with_printer: parsing error of compatible_printers_condition {}: {what}",
                        printer.name
                    );
                    if !line.ends_with('\n') {
                        line.push('\n');
                    }
                    crate::write_text(&mut std::io::stderr(), &line);
                }
                Ok(true)
            }
            Err(ConditionError::Unsupported(why)) => Err(why),
        };
    };
    let parent = printer.config.string("inherits");
    Ok(printer.name.is_empty()
        || list.contains(&printer.name)
        || (printer.from != "system" && list.contains(&parent)))
}

/// Whether a filament is made for the process (`is_compatible_with_print`): with no
/// compatible_prints list, its compatible_prints_condition holds for the process preset
/// (`process_config`), or there is none, or it does not parse (printed on stdout as Orca prints
/// it); with a list, the list names the process, or the process has no name.
fn compatible_with_print(
    filament: &Config,
    process: &LoadedPreset,
    process_config: &Config,
) -> Compat {
    let list = filament
        .strings("compatible_prints")
        .filter(|l| !l.is_empty());
    let Some(list) = list else {
        let condition = filament.string("compatible_prints_condition");
        if condition.is_empty() {
            return Ok(true);
        }
        return match evaluate_boolean_expression(condition.as_bytes(), process_config, None) {
            Ok(b) => Ok(b),
            Err(ConditionError::Parse(what)) => {
                let text = format!(
                    "Preset::is_compatible_with_print - parsing error of compatible_prints_condition {}:\n{}\n",
                    c_str(&process.name),
                    c_str(&what)
                );
                crate::write_text(&mut std::io::stdout(), &text);
                Ok(true)
            }
            Err(ConditionError::Unsupported(why)) => Err(why),
        };
    };
    Ok(process.name.is_empty() || list.contains(&process.name))
}

/// Is the process made for this printer, and each filament for both (Orca's own rules, which the
/// GUI uses).
fn check_compatibility(
    job: &Job,
    machine: &LoadedPreset,
    process: &LoadedPreset,
    filaments: &[LoadedPreset],
    warnings: &Warnings,
) -> Result<(), Failure> {
    let printer_config = preset_config(&machine.config, orca_config::PRINTER_OPTIONS)?;
    let process_config = preset_config(&process.config, orca_config::PRINT_OPTIONS)?;
    let declined = |_why: String| decline::unsupported_condition(job);
    if !compatible_with_printer(&process.config, machine, &printer_config).map_err(declined)? {
        return Err(fail(
            "process-not-compatible",
            format!(
                "the process preset {} is not made for the printer {} (its compatible_printers and compatible_printers_condition leave it out)",
                in_quotes(&process.name),
                in_quotes(&machine.name)
            ),
        ));
    }
    for (i, filament) in filaments.iter().enumerate() {
        let not_made_for = |what: &str, name: &str| {
            warnings.add(
                "filament-not-compatible",
                &format!(
                    "presets.filaments[{i}]: {} is not made for the {what} {}; Orca slices with it anyway",
                    in_quotes(&filament.name),
                    in_quotes(name)
                ),
                -1,
            );
        };
        if !compatible_with_printer(&filament.config, machine, &printer_config).map_err(declined)? {
            not_made_for("printer", &machine.name);
        } else if !compatible_with_print(&filament.config, process, &process_config)
            .map_err(declined)?
        {
            not_made_for("process", &process.name);
        }
    }
    Ok(())
}

/// What stops the filament merge: a failure of its own, or an exception of Orca's lists.
enum MergeError {
    Failure(Failure),
    Config(ConfigError),
}

impl From<ConfigError> for MergeError {
    fn from(e: ConfigError) -> MergeError {
        MergeError::Config(e)
    }
}

/// The filament settings of the print configuration (OrcaSlicer.cpp, "set the filament settings
/// into print config"): each filament's lists at its index, the lists with extruder variants
/// (`filament_options_with_variant`) in blocks that `filament_self_index` tells apart; `given`
/// marks, per setting, the filaments that gave it.
#[allow(clippy::too_many_lines)]
fn merge_filaments(
    cfg: &mut Config,
    filaments: &[LoadedPreset],
    given: &mut BTreeMap<String, Vec<bool>>,
) -> Result<(), MergeError> {
    let n = filaments.len();
    let mut old_start = vec![0_i64; n];
    let mut old_counts = vec![1_i64; n];
    let old_self = if let Some(values) = cfg.ints("filament_self_index") {
        values.clone()
    } else {
        let values: Vec<i32> = (1..=n)
            .map(|i| i32::try_from(i).unwrap_or(i32::MAX))
            .collect();
        if let Some(OptionValue::Ints { values: v, .. }) = cfg.create("filament_self_index") {
            v.clone_from(&values);
        }
        values
    };
    {
        let (mut k, mut current) = (-1_i64, 0_i32);
        for (i, v) in old_self.iter().enumerate() {
            if *v > current {
                current = *v;
                let next = usize::try_from(k + 1).unwrap_or(usize::MAX);
                if next >= n {
                    break;
                }
                k += 1;
                old_start[next] = i64::try_from(i).unwrap_or(i64::MAX);
                old_counts[next] = 1;
            } else if let Ok(k) = usize::try_from(k) {
                old_counts[k] += 1;
            }
        }
    }
    let mut new_counts = old_counts.clone();

    let empty_text = OptionValue::String(String::new());
    for (index, filament) in filaments.iter().enumerate() {
        let failure = |what: String| {
            MergeError::Failure(fail(
                "preset-invalid",
                format!("presets.filaments[{index}]: {what}"),
            ))
        };
        let mut config = filament.config.clone();
        add_default_gcodes(&mut config, Kind::Filament);

        for (key, text) in [
            ("filament_settings_id", &filament.name),
            ("filament_ids", &filament.filament_id),
        ] {
            let value = OptionValue::String(text.clone());
            if let Some(list) = cfg.create(key) {
                if vec_len(list).unwrap_or(0) < n {
                    resize(list, n, Some(&value))?;
                }
                set_at(list, &value, index, 0)?;
            }
            if key == "filament_settings_id" {
                config.erase("filament_settings_id");
                set_preset_slot(cfg, "different_settings_to_system", index + 1, n, "");
                let inherits = if filament.from == "system" {
                    ""
                } else {
                    filament.inherits.as_str()
                };
                set_preset_slot(cfg, "inherits_group", index + 1, n, inherits);
            }
        }

        if cfg.strings("filament_extruder_variant").is_none()
            && let Some(OptionValue::Strings(values)) = cfg.create("filament_extruder_variant")
        {
            values.resize(n, "Direct Drive Standard".to_string());
        }
        let new_variant_count = created_len(&mut config, "filament_extruder_variant");
        let new_variant_count = i64::try_from(new_variant_count).unwrap_or(i64::MAX);
        let old_variant_count = old_counts[index];

        for key in config.keys() {
            let Some(src) = config.get(&key).cloned() else {
                return Err(failure(format!("{key} cannot be read")));
            };
            if !src.ty().is_vector() {
                let group = match key.as_str() {
                    "compatible_printers_condition" => {
                        Some(("compatible_machine_expression_group", n + 2, index + 1))
                    }
                    "compatible_prints_condition" => {
                        Some(("compatible_process_expression_group", n, index))
                    }
                    _ => None,
                };
                if let Some((group, size, slot)) = group
                    && let Some(dst) = cfg.create(group)
                {
                    if vec_len(dst) == Some(0) {
                        resize(dst, size, Some(&empty_text))?;
                    }
                    set_at(dst, &src, slot, 0)?;
                }
                // Other single values of a filament preset do not reach the print.
                continue;
            }
            if matches!(
                key.as_str(),
                "compatible_prints"
                    | "compatible_printers"
                    | "model_id"
                    | "dev_model_name"
                    | "filament_settings_id"
            ) || merge_metadata_key(&key)
            {
                continue;
            }
            let Some(dst) = cfg.create(&key) else {
                return Err(failure(format!("{key} cannot be stored")));
            };
            // Orca's set_at and set_with_restore_2 read the first value of both lists unchecked.
            if vec_len(&src) == Some(0) {
                return Err(failure(format!("{key} is an empty list")));
            }
            if vec_len(dst) == Some(0) {
                resize(dst, 1, Some(&src))?;
            }
            let mut src = src;
            if FILAMENT_OPTIONS_WITH_VARIANT.contains(&key.as_str()) {
                let restore = vec![-1; usize::try_from(new_variant_count).unwrap_or(0)];
                let start = usize::try_from(old_start[index]).map_err(|_| {
                    ConfigError(
                        "ConfigOptionVector::set_with_restore_2(): invalid size found".to_string(),
                    )
                })?;
                let len = usize::try_from(old_variant_count).unwrap_or(0);
                set_with_restore_2(dst, &mut src, &restore, start, len, true)?;
                if key == "filament_extruder_variant" {
                    new_counts[index] =
                        i64::try_from(vec_len(&src).unwrap_or(0)).unwrap_or(i64::MAX);
                }
            } else {
                set_at(dst, &src, index, 0)?;
                let by = given.entry(key.clone()).or_default();
                by.resize(n, false);
                by[index] = true;
            }
        }
        if old_variant_count != new_variant_count {
            for start in &mut old_start[index + 1..] {
                *start += new_variant_count - old_variant_count;
            }
        }
    }

    if let Some(size) = cfg.strings("filament_extruder_variant").map(Vec::len)
        && let Some(OptionValue::Ints { values, .. }) = cfg.create("filament_self_index")
    {
        values.resize(size, 1);
        let mut k = 0;
        for (i, count) in new_counts.iter().enumerate() {
            let mut j = 0;
            while j < *count && k < values.len() {
                values[k] = i32::try_from(i + 1).unwrap_or(i32::MAX);
                k += 1;
                j += 1;
            }
        }
    }
    Ok(())
}

/// `load_settings(job, warnings)`: the presets the job names, read, checked and merged, with the
/// job's overrides and Orca's defaults, and checked whole.
///
/// # Errors
///
/// `file-not-found`, `preset-invalid`, `not-fdm`, `process-not-compatible`, `setting-invalid`,
/// `config-invalid`, the internal error of an exception Orca's code does not catch, and the
/// decline of a compatibility condition the placeholder parser does not take
/// (`decline::unsupported_condition`).
pub fn load_settings(job: &Job, warnings: &Warnings) -> Result<Settings, Failure> {
    let mut machine = load_preset(&job.machine, "machine", "presets.machine", warnings)?;
    let mut process = load_preset(&job.process, "process", "presets.process", warnings)?;
    let mut filaments = Vec::with_capacity(job.filaments.len());
    for (i, path) in job.filaments.iter().enumerate() {
        filaments.push(load_preset(
            path,
            "filament",
            &format!("presets.filaments[{i}]"),
            warnings,
        )?);
    }
    let n = filaments.len();
    check_compatibility(job, &machine, &process, &filaments, warnings)?;

    // The merge of OrcaSlicer.cpp for a model that brings no project settings: an empty
    // configuration takes every key of the machine, then of the process, then each filament's
    // values at its index.
    let mut cfg = Config::default();
    set_preset_slot(&mut cfg, "different_settings_to_system", n + 1, n, "");
    let machine_parent = if machine.from == "system" {
        String::new()
    } else {
        machine.inherits.clone()
    };
    set_preset_slot(&mut cfg, "inherits_group", n + 1, n, &machine_parent);

    machine.config.insert(
        "printer_settings_id",
        OptionValue::String(machine.name.clone()),
    );
    add_default_gcodes(&mut machine.config, Kind::Printer);
    copy_preset_keys(&mut cfg, &machine.config, "presets.machine")?;
    let printer_model = cfg.string("printer_model");
    apply_cli_safe_limits(&mut cfg, &job.resources, &printer_model);

    process.config.insert(
        "print_settings_id",
        OptionValue::String(process.name.clone()),
    );
    let compatible = process
        .config
        .strings("compatible_printers")
        .cloned()
        .unwrap_or_default();
    if let Some(OptionValue::Strings(values)) = cfg.create("print_compatible_printers") {
        *values = compatible;
    }
    set_preset_slot(&mut cfg, "different_settings_to_system", 0, n, "");
    let process_parent = if process.from == "system" {
        String::new()
    } else {
        process.inherits.clone()
    };
    set_preset_slot(&mut cfg, "inherits_group", 0, n, &process_parent);
    copy_preset_keys(&mut cfg, &process.config, "presets.process")?;

    let mut given: BTreeMap<String, Vec<bool>> = BTreeMap::new();
    merge_filaments(&mut cfg, &filaments, &mut given).map_err(|e| match e {
        MergeError::Failure(f) => f,
        MergeError::Config(e) => fail(
            "preset-invalid",
            format!("the filament presets cannot be merged: {}", e.0),
        ),
    })?;

    // The job's overrides, read as the stock command line reads its own options.
    let mut extra = Config::default();
    let mut overridden = Vec::with_capacity(job.overrides.len());
    for (key, value) in &job.overrides {
        overridden.push(parse_setting(&mut extra, key, value, "overrides")?);
    }

    pad_filament_options(&mut cfg, &given, n).map_err(|e| internal(&e))?;
    set_filament_colours(&mut cfg, &mut extra, &filaments)?;
    check_filament_overrides(&mut extra, &cfg, n)?;

    let printer_model_id = find_printer_model_id(&job.resources, &printer_model);
    let preset_name = if machine.from == "system" {
        &machine.name
    } else {
        &machine.inherits
    };
    let is_bbl = printer_model.starts_with("Bambu Lab")
        || (printer_model.is_empty() && preset_name.starts_with("Bambu Lab"));

    // The plate: the job's curr_bed_type, else the plate the GUI picks for this printer.
    if !extra.has("curr_bed_type")
        && let Some(OptionValue::Enum { value, .. }) = cfg.create("curr_bed_type")
    {
        *value = default_bed_type(&machine.config, &printer_model_id);
    }

    // The overrides go in before the flushing volumes are computed, which read some of them, and
    // the nozzles are counted on the settings the print gets.
    cfg.apply(&extra).map_err(|e| internal(&e))?;
    let extruder_count = orca_stl::max(1, cfg.floats("nozzle_diameter").map_or(0, Vec::len));
    let printer_variant_count = created_len(&mut cfg, "printer_extruder_variant");
    compute_flush_volumes(
        &mut cfg,
        extra.has("flush_volumes_matrix"),
        extruder_count,
        printer_variant_count,
        &job.resources,
    )?;
    config::normalize_fdm(&mut cfg);
    if let Some(OptionValue::Enum { value, .. }) = cfg.create("printer_technology") {
        // ptFFF.
        *value = 0;
    }
    cfg = config::with_full_print_config(&cfg);
    size_filament_options(&mut cfg, n).map_err(|e| internal(&e))?;
    let invalid = config::validate(&cfg).map_err(|e| internal(&e))?;
    if !invalid.is_empty() {
        let parts: Vec<String> = invalid.iter().map(|(k, v)| format!("{k}: {v}")).collect();
        return Err(fail(
            "config-invalid",
            format!("Orca refuses these settings: {}", parts.join("; ")),
        ));
    }
    Ok(Settings {
        config: cfg,
        machine,
        process,
        filaments,
        extruder_count,
        overridden,
        printer_model_id,
        is_bbl,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_become_the_text_orca_reads() {
        let parse = |s: &str| orca_json::parse(s.as_bytes(), Order::Sorted).unwrap();
        let text = |s: &str, sep: char, escape: bool| {
            let Value::Array(items) = parse(s) else {
                panic!("{s}")
            };
            let mut out = String::new();
            array_text(&items, sep, '#', escape, &mut out).then_some(out)
        };
        assert_eq!(
            text(r#"["0.4", 0.4, true]"#, ',', false).as_deref(),
            Some("0.4,0.4,1")
        );
        assert_eq!(
            text(r#"["a\"b", "c;d"]"#, ';', true).as_deref(),
            Some(r#""a\"b";"c;d""#)
        );
        assert_eq!(
            text(r#"[["0x0", "1x1"], ["2x2"]]"#, '#', false).as_deref(),
            Some("0x0#1x1#2x2")
        );
        assert_eq!(text(r#"[["0x0"], "1x1"]"#, ',', false), None);
        assert_eq!(text(r#"[null]"#, ',', false), None);
        assert_eq!(text("[]", ',', false).as_deref(), Some(""));
    }

    #[test]
    fn numbers_are_whole_numbers_or_nil() {
        assert!(whole_number(b" 12 ", true, false));
        assert!(!whole_number(b"12.5", true, false));
        assert!(whole_number(b"12.5", false, false));
        assert!(whole_number(b"40%", false, true));
        assert!(!whole_number(b"40%", false, false));
        assert!(!whole_number(b"", false, false));
        assert!(!whole_number(b"inf", false, false));
        assert!(!whole_number(b"220;270", false, false));
        // strtod stops at the C string's end, as c_str() does.
        assert!(whole_number(b"5\0x", false, false));
        let def = |k: &str| orca_config::get(k).unwrap();
        assert!(readable_numbers(def("nozzle_temperature"), "220, 230"));
        assert!(!readable_numbers(def("nozzle_temperature"), "220,"));
        assert!(readable_numbers(
            def("filament_retraction_length"),
            "nil,0.8"
        ));
        assert!(!readable_numbers(def("retraction_length"), "nil"));
        assert!(readable_numbers(def("support_type"), "anything"));
    }

    #[test]
    fn a_setting_is_refused_with_a_hint_of_what_it_takes() {
        let mut c = Config::default();
        let message = |r: Result<String, Failure>| r.unwrap_err().message;
        assert_eq!(
            message(parse_setting(&mut c, "no_such_key", "1", "overrides")),
            "overrides: no_such_key is not a setting of Orca 2.4.2"
        );
        assert_eq!(
            message(parse_setting(&mut c, "layer_height", "thin", "overrides")),
            "overrides: layer_height cannot be \"thin\" (a number)"
        );
        assert_eq!(
            message(parse_setting(
                &mut c,
                "nozzle_temperature",
                "220;230",
                "overrides"
            )),
            "overrides: nozzle_temperature cannot be \"220;230\" (a list of numbers is written with commas, one value per filament or extruder)"
        );
        assert_eq!(
            message(parse_setting(
                &mut c,
                "nozzle_temperature",
                "hot",
                "overrides"
            )),
            "overrides: nozzle_temperature cannot be \"hot\" (numbers, separated by commas)"
        );
        assert!(
            message(parse_setting(&mut c, "wall_generator", "wavy", "overrides")).starts_with(
                "overrides: wall_generator cannot be \"wavy\" (one of: classic, arachne"
            )
        );
        assert_eq!(
            parse_setting(&mut c, "layer_height", "0.2", "overrides"),
            Ok("layer_height".to_string())
        );
        assert_eq!(c.get("layer_height"), Some(&OptionValue::Float(0.2)));
    }

    #[test]
    fn flush_inputs_past_the_largest_volume_are_refused() {
        assert!(flush_input(20000.0, "nozzle_volume").is_ok());
        assert_eq!(
            flush_input(20000.5, "nozzle_volume").unwrap_err().message,
            "nozzle_volume cannot be 20000.5 for the flushing volumes (at most 20000 either way)"
        );
        assert_eq!(
            flush_input(f64::INFINITY, "retraction_distances_when_cut")
                .unwrap_err()
                .code,
            "config-invalid"
        );
    }

    #[test]
    fn colours_are_hex() {
        assert!(is_hex_colour("#00C1AE"));
        assert!(is_hex_colour("#00c1aeFF"));
        assert!(!is_hex_colour("00C1AE"));
        assert!(!is_hex_colour("#00C1AG"));
        assert!(!is_hex_colour("#00C1A"));
    }

    #[test]
    fn compatibility_follows_the_lists_and_the_conditions() {
        let preset = |pairs: &[(&str, OptionValue)]| {
            let mut c = Config::default();
            for (k, v) in pairs {
                c.insert(k, v.clone());
            }
            c
        };
        let mut machine = preset(&[("inherits", OptionValue::String("Parent".into()))]);
        machine.insert(
            "nozzle_diameter",
            OptionValue::Floats {
                values: vec![0.4, 0.6],
                nullable: false,
            },
        );
        let printer = LoadedPreset {
            name: "P".into(),
            from: "User".into(),
            config: machine,
            ..LoadedPreset::default()
        };
        let printer_config = preset_config(&printer.config, orca_config::PRINTER_OPTIONS).unwrap();
        let list = |l: &[&str]| OptionValue::Strings(l.iter().map(|s| (*s).to_string()).collect());
        let condition = |key: &str, c: &str| preset(&[(key, OptionValue::String(c.into()))]);
        let on_printer = |p: &Config| compatible_with_printer(p, &printer, &printer_config);
        assert_eq!(on_printer(&Config::default()), Ok(true));
        assert_eq!(
            on_printer(&preset(&[("compatible_printers", list(&["Q"]))])),
            Ok(false)
        );
        assert_eq!(
            on_printer(&preset(&[("compatible_printers", list(&["Parent"]))])),
            Ok(true)
        );
        // A condition, against the printer preset and printer_preset and num_extruders over it.
        let printers = "compatible_printers_condition";
        assert_eq!(
            on_printer(&condition(printers, "nozzle_diameter[0]==0.4")),
            Ok(true)
        );
        assert_eq!(
            on_printer(&condition(printers, "nozzle_diameter[1]==0.4")),
            Ok(false)
        );
        assert_eq!(
            on_printer(&condition(
                printers,
                "num_extruders == 2 and printer_preset == \"P\""
            )),
            Ok(true)
        );
        // An unset printer setting has Orca's default.
        assert_eq!(
            on_printer(&condition(printers, "printer_model == \"\"")),
            Ok(true)
        );
        // A list wins over a condition.
        let mut both = condition(printers, "false");
        both.insert("compatible_printers", list(&["P"]));
        assert_eq!(on_printer(&both), Ok(true));
        // A condition past the placeholder parser declines.
        assert!(on_printer(&condition(printers, "printer_model =~ /(?=x)/")).is_err());

        let process = LoadedPreset {
            name: "Fine".into(),
            config: preset(&[("layer_height", OptionValue::Float(0.2))]),
            ..LoadedPreset::default()
        };
        let process_config = preset_config(&process.config, orca_config::PRINT_OPTIONS).unwrap();
        let on_process = |p: &Config| compatible_with_print(p, &process, &process_config);
        assert_eq!(
            on_process(&preset(&[("compatible_prints", list(&["Fine"]))])),
            Ok(true)
        );
        let prints = "compatible_prints_condition";
        assert_eq!(
            on_process(&condition(prints, "layer_height > 0.15")),
            Ok(true)
        );
        assert_eq!(
            on_process(&condition(prints, "layer_height < 0.15")),
            Ok(false)
        );
    }

    #[test]
    fn colours_and_the_flush_formula() {
        assert_eq!(parse_color4("#00C1AE"), [0, 0xC1, 0xAE, 255]);
        assert_eq!(parse_color4("#00C1AE80"), [0, 0xC1, 0xAE, 0x80]);
        assert_eq!(parse_color4("#00G1AE"), [0, 0, 0, 255]);
        assert_eq!(parse_color4("red"), [0, 0, 0, 255]);
        assert_eq!(rgb2hsv(1.0, 0.0, 0.0), (0.0, 1.0, 1.0));
        // A dataset with no flush data (4) takes the formula, at least 60, plus the minimum.
        let mut none = Predictors {
            resources: "no such folder",
            by_dataset: BTreeMap::new(),
        };
        let white = [255, 255, 255, 255];
        let black = [0, 0, 0, 255];
        assert_eq!(
            calc_flush_vol(&mut none, 0, MAX_FLUSH_VOLUME, 4, white, white),
            60
        );
        assert_eq!(
            calc_flush_vol(&mut none, 100, MAX_FLUSH_VOLUME, 4, white, white),
            160
        );
        assert!(calc_flush_vol(&mut none, 0, MAX_FLUSH_VOLUME, 4, black, white) > 400);
        assert_eq!(calc_flush_vol(&mut none, 0, 50, 4, black, white), 50);
        // A transparent colour is white.
        assert_eq!(
            calc_flush_vol(&mut none, 0, MAX_FLUSH_VOLUME, 4, [0, 0, 0, 0], white),
            60
        );
        let mut m = vec![0.0; 8];
        set_flush_volumes_matrix(&mut m, &[1.0, 2.0, 3.0, 4.0], 1, 2);
        assert_eq!(m, [0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 4.0]);
        set_flush_volumes_matrix(&mut m, &[5.0], 0, 1);
        assert_eq!(m, [5.0]);
    }

    #[test]
    fn the_plate_and_the_numbers_read_as_the_c_library_reads_them() {
        let machine = |v: &str| {
            let mut c = Config::default();
            c.insert("default_bed_type", OptionValue::String(v.into()));
            c
        };
        assert_eq!(default_bed_type(&Config::default(), ""), BED_PEI);
        assert_eq!(default_bed_type(&Config::default(), "BL-P001"), BED_PC);
        assert_eq!(default_bed_type(&machine(""), "C13"), BED_PC);
        assert_eq!(default_bed_type(&machine(" 2x"), ""), 2);
        assert_eq!(default_bed_type(&machine("7"), ""), BED_PEI);
        assert_eq!(default_bed_type(&machine("plate"), "BL-P001"), BED_PEI);
        assert_eq!(strtol("99999999999"), i64::from(i32::MAX));
        assert_eq!(strtol("-99999999999"), i64::from(i32::MIN));
        assert_eq!(leading_float("236"), Some(236.0));
        assert_eq!(leading_float("2.5e1x"), Some(25.0));
        assert_eq!(leading_float("x"), None);
        assert_eq!(rgb_hex_to_dec("#0A2989"), Some((0x0A, 0x29, 0x89)));
        assert_eq!(rgb_hex_to_dec("0A2989"), None);
    }

    /// The settings this stage completes (the flushing volumes, the plate) as the C++ program's
    /// G-code gives them in its configuration block, for every golden case that wrote G-code:
    /// `SKETCH_ORCA_GOLDENS_DEEP` names the parity folder (`<work>/parity`), whose goldens and
    /// whose certify run's job files (`runs/certify/cases`, with their presets) are read.
    #[test]
    fn completed_settings_equal_the_golden_gcode() {
        let Some(parity) = std::env::var_os("SKETCH_ORCA_GOLDENS_DEEP") else {
            eprintln!("SKETCH_ORCA_GOLDENS_DEEP is not set: the golden G-code is not compared");
            return;
        };
        let parity = std::path::PathBuf::from(parity);
        let keys = [
            "flush_volumes_matrix",
            "flush_multiplier",
            "curr_bed_type",
            "filament_colour",
        ];
        let mut compared = 0;
        let mut differ = Vec::new();
        let mut gcodes = Vec::new();
        let cases = parity.join("goldens").join("cases");
        for case in std::fs::read_dir(&cases).unwrap().flatten() {
            for run in std::fs::read_dir(case.path()).unwrap().flatten() {
                let files = run.path().join("files");
                let direct = files.join("out.gcode");
                if direct.is_file() {
                    gcodes.push((case.file_name(), None, direct));
                }
                for sub in std::fs::read_dir(&files).into_iter().flatten().flatten() {
                    let g = sub.path().join("out.gcode");
                    if g.is_file() {
                        gcodes.push((case.file_name(), Some(sub.file_name()), g));
                    }
                }
            }
        }
        for (case, sub, gcode) in gcodes {
            let mut job_path = parity
                .join("runs")
                .join("certify")
                .join("cases")
                .join(&case);
            if let Some(sub) = &sub {
                job_path = job_path.join(sub);
            }
            let Ok(text) = std::fs::read(job_path.join("job.json")) else {
                continue;
            };
            let job = crate::job::parse_job(&text).unwrap();
            let settings = match load_settings(&job, &Warnings::default()) {
                Ok(s) => s,
                Err(f) => {
                    differ.push(format!("{}: {} {}", gcode.display(), f.code, f.message));
                    continue;
                }
            };
            let g = String::from_utf8_lossy(&std::fs::read(&gcode).unwrap()).into_owned();
            // GCode::append_full_config writes each nozzle's block of the matrix times its
            // flush_multiplier, rounded.
            let mut written = settings.config.clone();
            let multipliers = written
                .floats("flush_multiplier")
                .cloned()
                .unwrap_or_default();
            let colours = written.strings("filament_colour").map_or(0, Vec::len);
            if let Some(OptionValue::Floats { values, .. }) =
                written.get_mut("flush_volumes_matrix")
                && !multipliers.is_empty()
                && colours * colours * multipliers.len() == values.len()
            {
                let block = values.len() / multipliers.len();
                for (i, v) in values.iter_mut().enumerate() {
                    *v = orca_crt::round(*v * multipliers[i / block]);
                }
            }
            for key in keys {
                let prefix = format!("; {key} = ");
                let Some(line) = g.lines().find(|l| l.starts_with(&prefix)) else {
                    continue;
                };
                let want = &line[prefix.len()..];
                let got = written.get(key).map(|v| v.serialize().unwrap());
                compared += 1;
                if got.as_deref() != Some(want) {
                    differ.push(format!(
                        "{}: {key} = {got:?}, the G-code has {want}",
                        gcode.display()
                    ));
                }
            }
        }
        eprintln!("{compared} settings compared with the golden G-code");
        assert!(compared > 0);
        assert!(differ.is_empty(), "{}", differ.join("\n"));
    }
}
