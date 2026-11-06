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
// Carries OrcaSlicer code: handle_legacy is translated, branch for branch and in its order, from
// PrintConfigDef::handle_legacy in src/libslic3r/PrintConfig.cpp (with the file's
// SplitStringAndRemoveDuplicateElement and ReplaceString) of OrcaSlicer v2.4.2, commit 8500fcd, on
// 2026-11-06; its copyright is the OrcaSlicer contributors', under the AGPL-3.0.

//! `PrintConfigDef::handle_legacy(opt_key, value)`: what Orca does to every key and value it reads
//! (a preset, a project, an override) before it looks the key up. It renames old keys (with their
//! values where the meaning changed), rewrites old values, and drops (an empty key) the keys it no
//! longer reads: those in its ignore set, and every key `print_config_def` does not define.
//! `tests/def.rs` checks it against every case the configdef probe recorded.

use std::collections::BTreeSet;

/// `SplitStringAndRemoveDuplicateElement`: the pieces of `s` between `separator`s (empty pieces
/// included), as a sorted set; none for an empty `s`.
fn split_unique(s: &str, separator: &str) -> BTreeSet<String> {
    let mut result = BTreeSet::new();
    if s.is_empty() {
        return result;
    }
    let strs = format!("{s}{separator}");
    let size = strs.len();
    let mut i = 0usize;
    while i < size {
        if let Some(found) = strs[i..].find(separator) {
            let pos = i + found;
            result.insert(strs[i..pos].to_string());
            i = pos + separator.len() - 1;
        }
        i += 1;
    }
    result
}

/// `ReplaceString`: every `old` in `s` replaced by `new`, left to right, going on after each
/// replacement.
fn replace_string(s: &mut String, old: &str, new: &str) {
    let mut pos = 0usize;
    while let Some(found) = s.get(pos..).and_then(|rest| rest.find(old)) {
        let at = pos + found;
        s.replace_range(at..at + old.len(), new);
        pos = at + new.len();
        if old.is_empty() && new.is_empty() {
            // std::string::find("") at the same place for ever; the C++ never gets here.
            break;
        }
    }
}

/// `boost::iequals` for ASCII text.
fn iequals(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// The keys handle_legacy drops whatever their value (its `ignore` set).
const IGNORE: &[&str] = &[
    "acceleration",
    "scale",
    "rotate",
    "duplicate",
    "duplicate_grid",
    "bed_size",
    "print_center",
    "g0",
    "wipe_tower_per_color_wipe",
    "support_sharp_tails",
    "support_remove_small_overhangs",
    "support_with_sheath",
    "tree_support_collision_resolution",
    "tree_support_with_infill",
    "max_volumetric_speed",
    "max_print_speed",
    "support_closing_radius",
    "remove_freq_sweep",
    "remove_bed_leveling",
    "remove_extrusion_calibration",
    "support_transition_line_width",
    "support_transition_speed",
    "bed_temperature",
    "bed_temperature_initial_layer",
    "can_switch_nozzle_type",
    "can_add_auxiliary_fan",
    "extra_flush_volume",
    "spaghetti_detector",
    "adaptive_layer_height",
    "z_hop_type",
    "z_lift_type",
    "bed_temperature_difference",
    "long_retraction_when_cut",
    "retraction_distance_when_cut",
    "internal_bridge_support_thickness",
    "top_area_threshold",
    "reduce_wall_solid_infill",
    "filament_load_time",
    "filament_unload_time",
    "smooth_coefficient",
    "overhang_totally_speed",
    "silent_mode",
    "overhang_speed_classic",
    "filament_prime_volume",
];

/// `PrintConfigDef::handle_legacy(opt_key, value)`: an empty key afterwards means Orca drops the
/// setting.
#[allow(clippy::too_many_lines)]
pub fn handle_legacy(opt_key: &mut String, value: &mut String) {
    let key = opt_key.as_str();
    let rename = |k: &mut String, to: &str| *k = to.to_string();
    let filament_id = |k: &mut String, v: &mut String, to: &str| {
        *k = to.to_string();
        if v == "1" {
            *v = "0".to_string();
        }
    };
    if key == "curr_bed_type" && value == "SuperTack Plate" {
        *value = "Supertack Plate".into();
    } else if key == "enable_wipe_tower" {
        rename(opt_key, "enable_prime_tower");
    } else if key == "wipe_tower_width" {
        rename(opt_key, "prime_tower_width");
    } else if key == "wiping_volume" {
        rename(opt_key, "prime_volume");
    } else if key == "wipe_tower_brim_width" {
        rename(opt_key, "prime_tower_brim_width");
    } else if key == "tool_change_gcode" {
        rename(opt_key, "change_filament_gcode");
    } else if key == "bridge_fan_speed" {
        rename(opt_key, "overhang_fan_speed");
    } else if key == "infill_extruder" || key == "sparse_infill_filament" {
        filament_id(opt_key, value, "sparse_infill_filament_id");
    } else if key == "solid_infill_extruder" || key == "solid_infill_filament" {
        filament_id(opt_key, value, "internal_solid_filament_id");
    } else if key == "top_solid_infill_filament" {
        filament_id(opt_key, value, "top_surface_filament_id");
    } else if key == "bottom_solid_infill_filament" {
        filament_id(opt_key, value, "bottom_surface_filament_id");
    } else if key == "perimeter_extruder" || key == "wall_filament" || key == "wall_filament_id" {
        filament_id(opt_key, value, "outer_wall_filament_id");
    } else if key == "inner_wall_filament" {
        filament_id(opt_key, value, "inner_wall_filament_id");
    } else if key == "outer_wall_filament" {
        filament_id(opt_key, value, "outer_wall_filament_id");
    } else if key == "wipe_tower_extruder" {
        rename(opt_key, "wipe_tower_filament");
    } else if key == "support_material_extruder" {
        rename(opt_key, "support_filament");
    } else if key == "support_material_interface_extruder" {
        rename(opt_key, "support_interface_filament");
    } else if key == "support_material_angle" {
        rename(opt_key, "support_angle");
    } else if key == "support_material_enforce_layers" {
        rename(opt_key, "enforce_support_layers");
    } else if matches!(
        key,
        "initial_layer_print_height"
            | "initial_layer_speed"
            | "internal_solid_infill_speed"
            | "top_surface_speed"
            | "support_interface_speed"
            | "outer_wall_speed"
            | "support_object_xy_distance"
    ) && value.contains('%')
    {
        // An old profile's percentage, where the value must now be absolute: dropped.
        opt_key.clear();
    } else if key == "inherits_cummulative" {
        rename(opt_key, "inherits_group");
    } else if key == "compatible_printers_condition_cummulative" {
        rename(opt_key, "compatible_machine_expression_group");
    } else if key == "compatible_prints_condition_cummulative" {
        rename(opt_key, "compatible_process_expression_group");
    } else if key == "cooling" {
        rename(opt_key, "slow_down_for_layer_cooling");
    } else if key == "timelapse_no_toolhead" {
        rename(opt_key, "timelapse_type");
    } else if key == "timelapse_type" && value == "2" {
        *value = "0".into();
    } else if key == "support_type" && value == "normal" {
        *value = "normal(manual)".into();
    } else if key == "support_type" && value == "tree" {
        *value = "tree(manual)".into();
    } else if key == "support_type" && value == "hybrid(auto)" {
        *value = "tree(auto)".into();
    } else if key == "support_base_pattern" && value == "none" {
        *value = "hollow".into();
    } else if key == "different_settings_to_system" {
        let copy_value: String = value.chars().filter(|&c| c != '"').collect();
        for split_key in split_unique(&copy_value, ";") {
            let mut copy_key = split_key.clone();
            let mut empty = String::new();
            handle_legacy(&mut copy_key, &mut empty);
            if copy_key != split_key {
                replace_string(value, &split_key, &copy_key);
            }
        }
    } else if key == "overhang_fan_threshold" && value == "5%" {
        *value = "10%".into();
    } else if key == "wall_infill_order" {
        match value.as_str() {
            "inner wall/outer wall/infill" | "infill/inner wall/outer wall" => {
                *value = "inner wall/outer wall".into();
            }
            "outer wall/inner wall/infill" | "infill/outer wall/inner wall" => {
                *value = "outer wall/inner wall".into();
            }
            "inner-outer-inner wall/infill" => *value = "inner-outer-inner wall".into(),
            _ => {}
        }
        rename(opt_key, "wall_sequence");
    } else if matches!(
        key,
        "nozzle_volume_type"
            | "default_nozzle_volume_type"
            | "printer_extruder_variant"
            | "print_extruder_variant"
            | "filament_extruder_variant"
            | "extruder_variant_list"
    ) {
        replace_string(value, "Normal", "Standard");
        replace_string(value, "Big Traffic", "High Flow");
    } else if key == "extruder_type" {
        replace_string(value, "DirectDrive", "Direct Drive");
    } else if key == "enable_power_loss_recovery" {
        if value == "1" || iequals(value, "true") {
            *value = "enable".into();
        } else if value == "0" || iequals(value, "false") {
            *value = "disable".into();
        }
    } else if key == "ensure_vertical_shell_thickness" {
        if value == "1" {
            *value = "ensure_all".into();
        } else if value == "0" {
            *value = "ensure_moderate".into();
        }
    } else if key == "rotate_solid_infill_direction" {
        rename(opt_key, "solid_infill_rotate_template");
        if value == "1" {
            *value = "0,90".into();
        } else if value == "0" {
            *value = "0".into();
        }
    } else if key == "sparse_infill_anchor" {
        rename(opt_key, "infill_anchor");
    } else if key == "sparse_infill_anchor_max" {
        rename(opt_key, "infill_anchor_max");
    } else if key == "chamber_temperatures" {
        rename(opt_key, "chamber_temperature");
    } else if key == "thumbnail_size" {
        rename(opt_key, "thumbnails");
    } else if key == "top_one_wall_type" && value != "none" {
        rename(opt_key, "only_one_wall_top");
        *value = "1".into();
    } else if key == "initial_layer_flow_ratio" {
        rename(opt_key, "bottom_solid_infill_flow_ratio");
    } else if key == "ironing_direction" {
        rename(opt_key, "ironing_angle");
    } else if key == "ironing_angle" && value.starts_with('-') {
        *value = "0".into();
    } else if key == "counterbole_hole_bridging" {
        rename(opt_key, "counterbore_hole_bridging");
    } else if key == "draft_shield" && value == "limited" {
        *value = "disabled".into();
    } else if matches!(
        key,
        "sparse_infill_pattern"
            | "top_surface_pattern"
            | "bottom_surface_pattern"
            | "internal_solid_infill_pattern"
            | "ironing_pattern"
            | "support_ironing_pattern"
    ) && value == "zig-zag"
    {
        *value = "rectilinear".into();
    } else if key == "filament_map_mode" {
        if value == "Auto" {
            *value = "Auto For Flush".into();
        }
    } else if key == "filament_type" {
        // std::getline(ss, token, ';') over the value: no piece after a final ';'.
        let mut type_list: Vec<String> = Vec::new();
        let mut rebuild_value = false;
        let bytes = value.as_bytes();
        let mut pos = 0usize;
        while pos < bytes.len() {
            let end = value[pos..].find(';').map_or(value.len(), |i| pos + i);
            let mut token = value[pos..end].to_string();
            pos = if end < value.len() { end + 1 } else { end };
            if token.len() >= 2 && token.starts_with('"') && token.ends_with('"') {
                token = token[1..token.len() - 1].to_string();
            }
            if token == "ASA-Aero" {
                token = "ASA-AERO".into();
                rebuild_value = true;
            }
            type_list.push(token);
        }
        if rebuild_value {
            let mut rebuilt = String::new();
            for (idx, t) in type_list.iter().enumerate() {
                if idx != 0 {
                    rebuilt.push(';');
                }
                rebuilt.push('"');
                rebuilt.push_str(t);
                rebuilt.push('"');
            }
            *value = rebuilt;
        }
    } else if key == "prime_tower_rib_wall" {
        if value == "1" {
            rename(opt_key, "wipe_tower_wall_type");
            *value = "rib".into();
        } else {
            opt_key.clear();
        }
    } else if key == "prime_tower_extra_rib_length" {
        rename(opt_key, "wipe_tower_extra_rib_length");
    } else if key == "prime_tower_rib_width" {
        rename(opt_key, "wipe_tower_rib_width");
    } else if key == "prime_tower_fillet_wall" {
        rename(opt_key, "wipe_tower_fillet_wall");
    } else if key == "extruder_clearance_max_radius" {
        rename(opt_key, "extruder_clearance_radius");
    } else if key == "machine_switch_extruder_time" {
        rename(opt_key, "machine_tool_change_time");
    } else if key == "wall_direction" && value == "auto" {
        *value = "ccw".into();
    }

    if IGNORE.contains(&opt_key.as_str()) {
        opt_key.clear();
        return;
    }
    if !crate::has(opt_key) {
        opt_key.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::{replace_string, split_unique};

    #[test]
    fn split_keeps_empty_pieces_and_sorts() {
        let s: Vec<String> = split_unique("b;a;;b", ";").into_iter().collect();
        assert_eq!(s, vec!["", "a", "b"]);
        assert!(split_unique("", ";").is_empty());
    }

    #[test]
    fn replace_goes_on_after_each_replacement() {
        let mut s = "aaa".to_string();
        replace_string(&mut s, "a", "aa");
        assert_eq!(s, "aaaaaa");
        let mut t = "Normal;Big Traffic".to_string();
        replace_string(&mut t, "Normal", "Standard");
        assert_eq!(t, "Standard;Big Traffic");
    }
}
