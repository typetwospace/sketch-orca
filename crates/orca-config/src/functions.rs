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
// Carries OrcaSlicer code: get_extruder_ams_count (lines 617-637), compute_filament_override_value
// (lines 10300-10332) and has_skirt (lines 11572-11579) are translated from
// src/libslic3r/PrintConfig.cpp of OrcaSlicer v2.4.2, commit 8500fcd, on 2026-11-13
// (orca-o3-front-1); their copyright is the OrcaSlicer contributors', under the AGPL-3.0.

//! Three free functions of PrintConfig.cpp over a dynamic [`Config`].
//!
//! - [`get_extruder_ams_count`] reads "id#count|id#count" texts into maps (`std::map`, so a
//!   repeated id keeps its first count), splitting as `boost::split` does (empty parts kept).
//! - [`compute_filament_override_value`] applies a filament's override of a machine option
//!   (`apply_override` with the filament map) and records the key when the result differs from
//!   the old machine value. For `long_retractions_when_cut` and `retraction_distances_when_cut`
//!   the filament's values are ignored unless `enable_long_retraction_when_cut` is 2: the first
//!   becomes all nil; the second, through the C++'s slip (it fills the other default), becomes an
//!   empty vector, which overrides nothing.
//! - [`has_skirt`]: a skirt with layers and loops, or a draft shield.

use std::collections::BTreeMap;

use crate::config::Config;
use crate::option::{ConfigOption, apply_override, value_eq};
use crate::print_config::{DraftShield, LongRectrationLevel};
use crate::{ConfigError, OptionValue};

/// `std::stoi(s)` with MSVC's messages.
fn stoi(s: &str) -> Result<i32, ConfigError> {
    orca_fmt::parse::stoi(s, 10).map(|(v, _)| v).map_err(|e| {
        ConfigError(
            match e {
                orca_fmt::parse::StoError::InvalidArgument => "invalid stoi argument",
                orca_fmt::parse::StoError::OutOfRange => "stoi argument out of range",
            }
            .into(),
        )
    })
}

/// `get_extruder_ams_count(strs)`: one map per text, from the id to the count.
///
/// # Errors
///
/// `std::stoi`'s "invalid stoi argument" and "stoi argument out of range", and a part with no
/// `#` (whose second number the C++ reads past the end of its vector).
pub fn get_extruder_ams_count(strs: &[String]) -> Result<Vec<BTreeMap<i32, i32>>, ConfigError> {
    let mut out = Vec::with_capacity(strs.len());
    for s in strs {
        let mut info = BTreeMap::new();
        if s.is_empty() {
            out.push(info);
            continue;
        }
        for ams_info in s.split('|') {
            let numbers: Vec<&str> = ams_info.split('#').collect();
            let Some(second) = numbers.get(1) else {
                return Err(ConfigError(format!(
                    "get_extruder_ams_count(): {ams_info:?} has no '#' (the C++ reads past its \
                     numbers)"
                )));
            };
            // std::make_pair(stoi(numbers[0]), stoi(numbers[1])): MSVC evaluates the arguments
            // right to left.
            let count = stoi(second)?;
            let id = stoi(numbers[0])?;
            info.entry(id).or_insert(count);
        }
        out.push(info);
    }
    Ok(out)
}

/// `compute_filament_override_value(opt_key, old_machine, new_machine, new_filament,
/// new_full_config, diff_keys, filament_overrides, f_maps)`.
///
/// # Errors
///
/// `apply_override`'s and the virtual `operator==`'s, and the cases where the C++ dereferences a
/// null pointer: no int `enable_long_retraction_when_cut` in `new_full_config`, or a filament of
/// another type for the two retraction keys.
#[allow(clippy::too_many_arguments)]
pub fn compute_filament_override_value(
    opt_key: &str,
    old_machine: &OptionValue,
    new_machine: &OptionValue,
    new_filament: &OptionValue,
    new_full_config: &Config,
    diff_keys: &mut Vec<String>,
    filament_overrides: &mut Config,
    f_maps: &[i32],
) -> Result<(), ConfigError> {
    let enable_level = || match new_full_config.get("enable_long_retraction_when_cut") {
        Some(OptionValue::Int(v)) => Ok(*v),
        _ => Err(ConfigError(
            "compute_filament_override_value(): no int enable_long_retraction_when_cut (the C++ \
             dereferences a null option)"
                .into(),
        )),
    };
    let filament_only = LongRectrationLevel::EnableFilament as i32;
    let mut filament = new_filament.clone();
    if opt_key == "long_retractions_when_cut" && enable_level()? != filament_only {
        let OptionValue::Bools {
            values,
            nullable: true,
        } = new_filament
        else {
            return Err(ConfigError(
                "compute_filament_override_value(): the filament is no ConfigOptionBoolsNullable \
                 (the C++ dereferences a null pointer)"
                    .into(),
            ));
        };
        filament = OptionValue::Bools {
            values: vec![u8::MAX; values.len()],
            nullable: true,
        };
    }
    if opt_key == "retraction_distances_when_cut" && enable_level()? != filament_only {
        if !matches!(new_filament, OptionValue::Floats { nullable: true, .. }) {
            return Err(ConfigError(
                "compute_filament_override_value(): the filament is no ConfigOptionFloatsNullable \
                 (the C++ dereferences a null pointer)"
                    .into(),
            ));
        }
        // The C++ fills opt_long_retraction_default here and points at the untouched
        // opt_retraction_distance_default: an empty nullable vector.
        filament = OptionValue::Floats {
            values: Vec::new(),
            nullable: true,
        };
    }
    let mut copy = new_machine.clone();
    apply_override(&mut copy, &filament, f_maps)?;
    let changed = !value_eq(old_machine, &copy)?;
    if changed {
        diff_keys.push(opt_key.to_string());
        filament_overrides.insert(opt_key, copy);
    }
    Ok(())
}

/// `has_skirt(cfg)`: `skirt_height` and `skirt_loops` both above 0, or a `draft_shield` that is
/// not disabled.
///
/// # Errors
///
/// `getInt()`'s, for one of the three of a type with no int.
pub fn has_skirt(cfg: &Config) -> Result<bool, ConfigError> {
    let height = cfg.get("skirt_height");
    let loops = cfg.get("skirt_loops");
    let shield = cfg.get("draft_shield");
    // In the C++'s order: the height's int is read before the loops are looked at.
    let skirt = match height {
        Some(h) if h.get_int()? > 0 => match loops {
            Some(l) => l.get_int()? > 0,
            None => false,
        },
        _ => false,
    };
    if skirt {
        return Ok(true);
    }
    match shield {
        Some(s) => Ok(s.get_int()? != DraftShield::dsDisabled as i32),
        None => Ok(false),
    }
}
