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
// OrcaSlicer v2.4.2, commit 8500fcd, translated on 2026-11-06. Their copyright is the OrcaSlicer
// contributors', under the AGPL-3.0.

//! The presets stage: the job's machine, process and filament presets read and merged into one
//! print configuration, the job's overrides applied and the result checked, as `cli/presets.cpp`'s
//! `load_settings` does it, step for step, with the same failures and warnings in the same order.
//!
//! - Each preset file ([`load_preset`]): read with sorted keys as Orca reads one; its description
//!   keys (name, type, from ...) kept apart; a list written as Orca's text; each value read with
//!   Orca's substitutions (an enum or bool value Orca does not know takes the default:
//!   `setting-substituted`); a key Orca does not have skipped (`unknown-setting`), as is a value
//!   that is neither text nor a list (`preset-value-skipped`). Refused: a missing file
//!   (`file-not-found`), a file that is not a JSON object, a value Orca cannot read, a `from` that
//!   is not system, User or user, a preset of the wrong type (`preset-invalid`), and a resin
//!   printer (`not-fdm`). A bad thumbnails list stops the run as Orca's exception does
//!   (`internal-error`).
//! - The process must be made for the printer (`process-not-compatible`), and each filament is
//!   checked against the printer and the process (`filament-not-compatible`, a warning), by their
//!   `compatible_printers` and `compatible_prints` lists. A preset that has no list but a
//!   `compatible_printers_condition` or `compatible_prints_condition` needs Orca's placeholder
//!   parser, which is not ported: the job is declined there, with the family `preset-conditions`
//!   besides its own (decline.rs).
//! - The merge (machine, then process, then each filament's values at its index, the lists with
//!   extruder variants in blocks by `filament_self_index`), the job's overrides
//!   ([`parse_setting`], no substitutions: `setting-invalid`), each filament's colour, the
//!   overrides that must give one value per filament, the flushing volumes' inputs (the
//!   extruder-variant picks Orca makes for each nozzle and filament, then each volume and
//!   retraction length the computation turns into a whole number, `config-invalid` past 20000),
//!   Orca's normalisation, its defaults for every setting of a print, and its own check of the
//!   values (`config-invalid`, "Orca refuses these settings: ...").
//!
//! Left out, as only the steps after the model check read them (and those are not ported): the
//! flushing volumes themselves (`flush_volumes_matrix` is left as n x n zeros for each nozzle,
//! where Orca computes each from the filaments' colours and `nozzle_flush_dataset`), the second
//! set of extruder-variant picks Orca makes there (`printer_options_with_variant_2`, the machine
//! limits, which nothing in this stage checks), the printer's model id and whether it is a
//! Bambu Lab printer, and the plate the GUI would pick (`curr_bed_type`). Where Orca reads the
//! first value of an empty list in the picks (a printer with two nozzles given an empty list of a
//! per-nozzle setting), its behaviour is undefined; this program reads 0 there, which no check of
//! this stage reads as a failure (the volumes and lengths it checks are lists of numbers, which
//! Orca never reads as empty).

// Settings and LoadedPreset hold what the steps after the model check read; they are not ported
// yet (decline.rs).
#![allow(dead_code)]

use std::collections::BTreeMap;

use orca_config::value::escape_string_cstyle;
use orca_config::{ConfigError, OptionType, OptionValue, PrinterTechnology};
use orca_json::{Order, Value};

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
fn flush_input(v: f64, key: &str) -> Result<(), Failure> {
    if !v.is_finite() || orca_crt::fabs(v) > f64::from(MAX_FLUSH_VOLUME) {
        return Err(fail(
            "config-invalid",
            format!(
                "{key} cannot be {} for the flushing volumes (at most {MAX_FLUSH_VOLUME} either way)",
                format_number(v)
            ),
        ));
    }
    Ok(())
}

/// `float_values(cfg, key, out)`: every value of a list of floats, nullable or not.
fn float_values(cfg: &Config, key: &str) -> Option<Vec<f64>> {
    cfg.floats(key).cloned()
}

/// `bool_values(cfg, key, out)`.
fn bool_values(cfg: &Config, key: &str) -> Option<Vec<u8>> {
    cfg.bools(key).cloned()
}

/// `min_flush_volumes(cfg, nozzle_id)` (Plater.cpp's `get_min_flush_volumes`), for its checks
/// only: each volume and retraction length it turns into a whole number ([`flush_input`]).
fn min_flush_volumes(cfg: &Config, nozzle: usize) -> Result<(), Failure> {
    if let Some(volumes) = float_values(cfg, "nozzle_volume")
        && let Some(first) = volumes.first()
    {
        let v = *volumes.get(nozzle).unwrap_or(first);
        if !v.is_nan() {
            flush_input(v, "nozzle_volume")?;
        }
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
    for idx in 0..size {
        if level != 0 && activated {
            flush_input(
                at(&printer_distance, nozzle),
                "retraction_distances_when_cut",
            )?;
        }
        let filament_activated = filament_long.get(idx).copied().unwrap_or(0);
        let filament_length = at(&filament_distance, idx);
        if filament_activated == 1 && level == LONG_RETRACTION_ENABLE_FILAMENT {
            if filament_length.is_nan() {
                flush_input(
                    at(&printer_distance, nozzle),
                    "retraction_distances_when_cut",
                )?;
            } else {
                flush_input(filament_length, "filament_retraction_distances_when_cut")?;
            }
        }
    }
    Ok(())
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

/// The flushing volumes between filaments, as far as they can refuse a job
/// (`compute_flush_volumes`): a job's own flush_volumes_matrix must have n x n values for each
/// nozzle; otherwise filament_is_support must give one value per filament, and each nozzle's
/// minimum volumes are computed from the extruder-variant picks Orca makes. The matrix is left
/// as zeros (see the top of this file).
fn compute_flush_volumes(
    cfg: &mut Config,
    matrix_given: bool,
    extruder_count: usize,
) -> Result<(), Failure> {
    let n = created_len(cfg, "filament_colour");
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
    let supports = created_len(cfg, "filament_is_support");
    if supports != n {
        return Err(fail(
            "preset-invalid",
            format!("the settings give {supports} filament_is_support values for {n} filaments"),
        ));
    }
    cfg.create("nozzle_flush_dataset");
    let mut picked = config::with_full_print_config(cfg);
    config::update_values_to_printer_extruders(&mut picked);
    config::update_values_to_printer_extruders_for_multiple_filaments(&mut picked);
    for nozzle in 0..extruder_count {
        min_flush_volumes(&picked, nozzle)?;
    }
    if let Some(OptionValue::Floats { values, .. }) = cfg.create("flush_volumes_matrix") {
        *values = vec![0.0; n * n * extruder_count];
    }
    Ok(())
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

/// Whether a preset is made for the printer (`is_compatible_with_printer`): its
/// compatible_printers lists the printer, or the parent of a printer that is not a system preset,
/// or there is no list; `None` where Orca would evaluate its compatible_printers_condition.
fn compatible_with_printer(preset: &Config, printer: &LoadedPreset) -> Option<bool> {
    let list = preset
        .strings("compatible_printers")
        .filter(|l| !l.is_empty());
    let Some(list) = list else {
        return if preset.string("compatible_printers_condition").is_empty() {
            Some(true)
        } else {
            None
        };
    };
    let parent = printer.config.string("inherits");
    Some(
        printer.name.is_empty()
            || list.contains(&printer.name)
            || (printer.from != "system" && list.contains(&parent)),
    )
}

/// Whether a filament is made for the process (`is_compatible_with_print`): its
/// compatible_prints lists the process, or there is no list; `None` where Orca would evaluate its
/// compatible_prints_condition.
fn compatible_with_print(filament: &Config, process: &LoadedPreset) -> Option<bool> {
    let list = filament
        .strings("compatible_prints")
        .filter(|l| !l.is_empty());
    let Some(list) = list else {
        return if filament.string("compatible_prints_condition").is_empty() {
            Some(true)
        } else {
            None
        };
    };
    Some(process.name.is_empty() || list.contains(&process.name))
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
    let conditions = || decline::preset_conditions(job);
    if !compatible_with_printer(&process.config, machine).ok_or_else(conditions)? {
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
        if !compatible_with_printer(&filament.config, machine).ok_or_else(conditions)? {
            not_made_for("printer", &machine.name);
        } else if !compatible_with_print(&filament.config, process).ok_or_else(conditions)? {
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
/// decline of presets with conditions (`decline::preset_conditions`).
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

    // The overrides go in before the flushing volumes are computed, which read some of them, and
    // the nozzles are counted on the settings the print gets.
    cfg.apply(&extra).map_err(|e| internal(&e))?;
    let extruder_count = orca_stl::max(1, cfg.floats("nozzle_diameter").map_or(0, Vec::len));
    cfg.create("printer_extruder_variant");
    compute_flush_volumes(&mut cfg, extra.has("flush_volumes_matrix"), extruder_count)?;
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
    fn compatibility_follows_the_lists_and_declines_a_condition() {
        let preset = |pairs: &[(&str, OptionValue)]| {
            let mut c = Config::default();
            for (k, v) in pairs {
                c.insert(k, v.clone());
            }
            c
        };
        let printer = LoadedPreset {
            name: "P".into(),
            from: "User".into(),
            config: preset(&[("inherits", OptionValue::String("Parent".into()))]),
            ..LoadedPreset::default()
        };
        let list = |l: &[&str]| OptionValue::Strings(l.iter().map(|s| (*s).to_string()).collect());
        assert_eq!(
            compatible_with_printer(&Config::default(), &printer),
            Some(true)
        );
        assert_eq!(
            compatible_with_printer(&preset(&[("compatible_printers", list(&["Q"]))]), &printer),
            Some(false)
        );
        assert_eq!(
            compatible_with_printer(
                &preset(&[("compatible_printers", list(&["Parent"]))]),
                &printer
            ),
            Some(true)
        );
        assert_eq!(
            compatible_with_printer(
                &preset(&[(
                    "compatible_printers_condition",
                    OptionValue::String("nozzle_diameter[0]==0.4".into())
                )]),
                &printer
            ),
            None
        );
        let process = LoadedPreset {
            name: "Fine".into(),
            ..LoadedPreset::default()
        };
        assert_eq!(
            compatible_with_print(&preset(&[("compatible_prints", list(&["Fine"]))]), &process),
            Some(true)
        );
        assert_eq!(
            compatible_with_print(
                &preset(&[(
                    "compatible_prints_condition",
                    OptionValue::String("x".into())
                )]),
                &process
            ),
            None
        );
    }
}
