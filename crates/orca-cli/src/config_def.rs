// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! `sketch-orca-rs config-def --out <file>`: the Rust form of cli/config_def.cpp, step for step.
//! Every print, filament and printer setting of Orca's `print_config_def` (orca-config's generated
//! definitions), but SLA's, `coNone`'s and the obsolete keys; then the obsolete keys, those Orca
//! renames, and those it still defines. The file is `dump(2)` of an `ordered_json` and a line
//! feed, the same bytes as the C++ program's (tests/config_def.rs).

use std::collections::{BTreeMap, BTreeSet};

use orca_config::{GuiType, Mode, OptionType, PrinterTechnology};
use orca_json::{Object, Order, Value};

use crate::version::{ENGINE, ENGINE_VERSION, ORCA_COMMIT, ORCA_VERSION};

/// The values cli/config_def.cpp reads each legacy key with (`PROBE_VALUES`).
const PROBE_VALUES: &[&str] = &["", "0", "1", "-1", "2", "none", "50%", "nil"];

/// `obsolete_keys()`: each legacy key Orca never keeps under its own name, whatever the value,
/// and the key it reads it as for the first value it renames it for ("" when it only drops it).
#[must_use]
pub fn obsolete_keys() -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for &legacy in orca_config::LEGACY_KEYS {
        let mut kept = false;
        let mut read_as = String::new();
        for &probe in PROBE_VALUES {
            let mut k = legacy.to_string();
            let mut value = probe.to_string();
            orca_config::handle_legacy(&mut k, &mut value);
            if k == legacy {
                kept = true;
                break;
            }
            if read_as.is_empty() {
                read_as = k;
            }
        }
        if !kept {
            // std::map::emplace keeps the first.
            out.entry(legacy.to_string()).or_insert(read_as);
        }
    }
    out
}

fn type_name(t: OptionType) -> &'static str {
    match t {
        OptionType::Float => "float",
        OptionType::Floats => "floats",
        OptionType::Int => "int",
        OptionType::Ints => "ints",
        OptionType::String => "string",
        OptionType::Strings => "strings",
        OptionType::Percent => "percent",
        OptionType::Percents => "percents",
        OptionType::FloatOrPercent => "floatOrPercent",
        OptionType::FloatsOrPercents => "floatsOrPercents",
        OptionType::Point => "point",
        OptionType::Points => "points",
        OptionType::Point3 => "point3",
        OptionType::Bool => "bool",
        OptionType::Bools => "bools",
        OptionType::Enum => "enum",
        OptionType::Enums => "enums",
        OptionType::PointsGroups => "pointsGroups",
        OptionType::IntsGroups => "intsGroups",
        OptionType::None => "none",
    }
}

fn mode_name(m: Mode) -> &'static str {
    match m {
        Mode::Simple => "simple",
        Mode::Advanced => "advanced",
        Mode::Develop => "develop",
        Mode::Expert => "expert",
    }
}

fn gui_type_name(t: GuiType) -> &'static str {
    match t {
        GuiType::IEnumOpen => "i_enum_open",
        GuiType::FEnumOpen => "f_enum_open",
        GuiType::Color => "color",
        GuiType::SelectOpen => "select_open",
        GuiType::Slider => "slider",
        GuiType::Legend => "legend",
        GuiType::OneString => "one_string",
        GuiType::Undefined => "",
    }
}

fn strings(list: &[&str]) -> Value {
    Value::Array(list.iter().map(|s| Value::from(*s)).collect())
}

/// `config_def_json()`.
#[must_use]
pub fn config_def_json() -> Value {
    let process: BTreeSet<&str> = orca_config::PRINT_OPTIONS.iter().copied().collect();
    let filament: BTreeSet<&str> = orca_config::FILAMENT_OPTIONS.iter().copied().collect();
    let machine: BTreeSet<&str> = orca_config::PRINTER_OPTIONS.iter().copied().collect();
    let obsolete = obsolete_keys();

    let mut options = Vec::new();
    for def in orca_config::options() {
        let key = def.key;
        if def.printer_technology == PrinterTechnology::Sla
            || def.ty == OptionType::None
            || obsolete.contains_key(key)
        {
            continue;
        }
        let mut o = Object::new(Order::Insertion);
        o.insert("key", key);
        o.insert("type", type_name(def.ty));
        o.insert("nullable", def.nullable);
        let mut sections = Vec::new();
        for (set, name) in [
            (&process, "process"),
            (&filament, "filament"),
            (&machine, "machine"),
        ] {
            if set.contains(key) {
                sections.push(Value::from(name));
            }
        }
        o.insert("sections", Value::Array(sections));
        o.insert("category", def.category);
        o.insert("label", def.label);
        o.insert("fullLabel", def.full_label);
        o.insert("tooltip", def.tooltip);
        o.insert("sidetext", def.sidetext);
        o.insert("mode", mode_name(def.mode));
        o.insert("readonly", def.readonly);
        o.insert("multiline", def.multiline);
        o.insert("isCode", def.is_code);
        let gui = gui_type_name(def.gui_type);
        if !gui.is_empty() {
            o.insert("guiType", gui);
        }
        let default = def.default_value().and_then(|v| v.serialize().ok());
        o.insert("default", default.map_or(Value::Null, Value::from));
        if def.min > -f32::MAX {
            o.insert("min", f64::from(def.min));
        }
        if def.max < f32::MAX {
            o.insert("max", f64::from(def.max));
        }
        if !def.enum_values.is_empty() {
            o.insert("enumValues", strings(def.enum_values));
        }
        if !def.enum_labels.is_empty() {
            o.insert("enumLabels", strings(def.enum_labels));
        }
        if !def.ratio_over.is_empty() {
            o.insert("ratioOver", def.ratio_over);
        }
        if !def.aliases.is_empty() {
            o.insert("aliases", strings(def.aliases));
        }
        options.push(Value::Object(o));
    }

    let mut obsolete_list = Vec::new();
    let mut renamed = Object::new(Order::Insertion);
    let mut still_defined = Vec::new();
    for (key, read_as) in &obsolete {
        obsolete_list.push(Value::from(key));
        if !read_as.is_empty() {
            renamed.insert(key.clone(), read_as);
        }
        if orca_config::has(key) {
            still_defined.push(Value::from(key));
        }
    }

    let mut out = Object::new(Order::Insertion);
    out.insert("format", "sketch-orca-config-def/1");
    out.insert("engine", ENGINE);
    out.insert("engineVersion", ENGINE_VERSION);
    out.insert("orcaVersion", ORCA_VERSION);
    out.insert("orcaCommit", ORCA_COMMIT);
    out.insert("count", options.len());
    out.insert("options", Value::Array(options));
    out.insert("obsolete", Value::Array(obsolete_list));
    out.insert("renamed", renamed);
    out.insert("stillDefined", Value::Array(still_defined));
    Value::Object(out)
}

/// The file's text: `dump(2)` and a line feed.
#[must_use]
pub fn config_def_text() -> String {
    config_def_json().dump(Some(2)) + "\n"
}
