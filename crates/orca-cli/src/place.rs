// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: cli/run.cpp's apply_object_settings, set_custom_gcodes,
// plate_filaments, estimate_wipe_tower_size, excluded_areas, printable_polygon, rect_mm,
// tower_misfit, tower_places, nearest_point_in, nearest_tower_move, tower_set_by_job,
// place_prime_tower, add_exclusion_regions, arrange_objects and check_build_volume, which follow
// CLI::run of src/OrcaSlicer.cpp (its global arrange, its prime tower and its build volume check),
// PartPlate::get_extruders_under_cli, PartPlate::estimate_wipe_tower_size and
// PartPlateList::preprocess_exclude_areas of src/slic3r/GUI/PartPlate.cpp, and
// WipeTower::get_limit_depth_by_height and WipeTower::get_auto_brim_by_height of
// src/libslic3r/GCode/WipeTower.cpp, OrcaSlicer v2.4.2, commit 8500fcd, translated on 2026-11-11
// (orca-info-place). Those portions are copyright the OrcaSlicer contributors, under the AGPL-3.0.

//! The objects' settings and their placement on the plate, as `cli/run.cpp`'s `run` does them
//! after the model stage of a layers or slice job, in its order (`run.rs`):
//!
//! 1. [`apply_object_settings`]: each object the job lists gets its overrides (object settings,
//!    region settings and `extruder`, as Orca's object list offers them; a printer, filament or
//!    whole-plate setting is `setting-invalid`) and its height ranges (a range starts from the
//!    object's layer height, or the print's, and filament 0, and takes `layer_height`, `extruder`
//!    and region settings; a layer height not above 0 is `setting-invalid`). A filament past the
//!    job's is `setting-invalid`. Every failure names the object.
//! 2. [`set_custom_gcodes`]: the job's custom G-code per layer as Orca's custom G-code of plate 1
//!    (a pause is the printer's pause G-code; a colour change with one filament is a pause, with
//!    several a tool change; a filament change is a tool change; custom is the given G-code),
//!    `unsupported` with print_sequence "by object" and `custom-gcode-invalid` for what Orca could
//!    not do. orca-model's `Model` keeps no custom G-code, so the plates' come back as
//!    [`PlatesCustomGcodes`] for the steps that read them.
//! 3. [`place_prime_tower`]: when Orca prints a prime tower on plate 1 (two filaments or more,
//!    smooth timelapse, or clumping detection with an area), its place: the job's own
//!    (`wipe_tower_x`, `wipe_tower_y`), else the GUI's default corner kept between the bed's
//!    edges; it must lie, brim included, on the printable area and off the excluded areas. The
//!    default place moves to the nearest one that does (`arrange-failed` when none does), the
//!    job's own is `config-invalid`. Its footprint is a fixed item for arrange.
//! 4. [`arrange_objects`] (a job that arranges): every instance onto plate 1 as the stock command
//!    line arranges them, but never rotated; `temperature-difference` for objects whose filaments
//!    are far apart in temperature, `arrange-failed` for one that does not fit.
//! 5. [`check_build_volume`]: an instance partly or wholly outside the printable volume is
//!    `not-printable`, and a plate where nothing is inside `empty-plate`.
//!
//! The settings are read as the C++ helpers read them: [`cfg_bool`], [`cfg_enum`] and
//! `info::cfg_float` give their fallback for a setting the configuration does not hold or a
//! list, and `internal-error` with Orca's exception for one of another type. Where the C++
//! dereferences an option without a check (`opt_float`, `opt_string`, `option<T>(key, true)`), a
//! missing option panics here, which the crash result reports (`crash.rs`).

use std::collections::{BTreeMap, BTreeSet};

use orca_arrange::ArrangeError;
use orca_arrange::arrange::{
    ArrangeCallbacks, ArrangeParams, ArrangePolygon, BED_SHRINK_SEQ_PRINT, arrange_on_points,
    is_filaments_compatible,
};
use orca_arrange::nlopt::NloptException;
use orca_config::OptionValue;
use orca_eigen::Vec2d;
use orca_geom::bounding_box::BoundingBoxf;
use orca_geom::clipper::{IntPoint, minkowski_sum};
use orca_geom::clipper_utils::{
    ApplySafetyOffset, DEFAULT_JOIN_TYPE, DEFAULT_MITER_LIMIT, diff, diff_ex, intersection,
    offset_ex_expolygons, union_,
};
use orca_geom::expolygon::{ExPolygon, ExPolygons};
use orca_geom::point::{
    Coord, EPSILON, PI, Point, Points, SCALING_FACTOR, scale_, scaled, sqr, unscale,
};
use orca_geom::polygon::{Polygon, Polygons, area};
use orca_model::arrange::{
    apply_arrange_polygon, get_instance_arrange_poly, get_shrink_bedpts, update_arrange_params,
    update_selected_items_inflation, update_unselected_items_inflation,
};
use orca_model::build_volume::BuildVolume;
use orca_model::config::ModelConfig;
use orca_model::model::{LayerConfigRanges, Model, ModelObject, PrintVolumeState};

use crate::common::{Failure, fail, fail_object};
use crate::config::{Config, create_default, set_value};
use crate::info::cfg_float;
use crate::job::{Job, JobObject, is_c_space};
use crate::model::{mm, object_label};
use crate::presets::{Settings, parse_setting};

/// The options of `PrintObjectConfig` (PrintConfig.hpp), by key, in byte order.
#[rustfmt::skip]
const OBJECT_KEYS: &[&str] = &[
    "bridge_acceleration", "bridge_no_support", "brim_ears_detection_length", "brim_ears_max_angle",
    "brim_flow_ratio", "brim_object_gap", "brim_type", "brim_use_efc_outline", "brim_width",
    "calib_flowrate_topinfill_special_order", "default_acceleration", "default_jerk",
    "default_junction_deviation", "detect_narrow_internal_solid_infill",
    "dont_filter_internal_bridges", "elefant_foot_compensation", "elefant_foot_compensation_layers",
    "elefant_foot_layers_density", "enable_extra_bridge_layer", "enable_support",
    "enforce_support_layers", "flush_into_infill", "flush_into_objects", "flush_into_support",
    "gap_fill_target", "infill_jerk", "initial_layer_acceleration", "initial_layer_jerk",
    "initial_layer_min_bead_width", "inner_wall_acceleration", "inner_wall_jerk",
    "interface_shells", "interlocking_beam", "interlocking_beam_layer_count",
    "interlocking_beam_width", "interlocking_boundary_avoidance", "interlocking_depth",
    "interlocking_orientation", "internal_bridge_density", "internal_solid_infill_acceleration",
    "layer_height", "line_width", "make_overhang_printable_angle",
    "make_overhang_printable_hole_size", "max_bridge_length", "min_bead_width", "min_feature_size",
    "min_length_factor", "mmu_segmented_region_interlocking_depth",
    "mmu_segmented_region_max_width", "outer_wall_acceleration", "outer_wall_jerk",
    "precise_z_height", "raft_contact_distance", "raft_expansion", "raft_first_layer_density",
    "raft_first_layer_expansion", "raft_layers", "seam_position", "set_other_flow_ratios",
    "skirt_start_angle", "slice_closing_radius", "slicing_mode", "sparse_infill_acceleration",
    "staggered_inner_seams", "support_angle", "support_base_pattern",
    "support_base_pattern_spacing", "support_bottom_interface_spacing", "support_bottom_z_distance",
    "support_critical_regions_only", "support_expansion", "support_filament", "support_flow_ratio",
    "support_interface_bottom_layers", "support_interface_filament", "support_interface_flow_ratio",
    "support_interface_loop_pattern", "support_interface_not_for_body", "support_interface_pattern",
    "support_interface_spacing", "support_interface_speed", "support_interface_top_layers",
    "support_ironing", "support_ironing_flow", "support_ironing_pattern", "support_ironing_spacing",
    "support_line_width", "support_object_first_layer_gap", "support_object_xy_distance",
    "support_on_build_plate_only", "support_remove_small_overhang", "support_speed",
    "support_style", "support_threshold_angle", "support_threshold_overlap",
    "support_top_z_distance", "support_type", "thick_bridges", "thick_internal_bridges",
    "top_surface_acceleration", "top_surface_jerk", "travel_acceleration", "travel_jerk",
    "tree_support_angle_slow", "tree_support_auto_brim", "tree_support_branch_angle",
    "tree_support_branch_angle_organic", "tree_support_branch_diameter",
    "tree_support_branch_diameter_angle", "tree_support_branch_diameter_organic",
    "tree_support_branch_distance", "tree_support_branch_distance_organic",
    "tree_support_brim_width", "tree_support_tip_diameter", "tree_support_top_rate",
    "tree_support_wall_count", "wall_distribution_count", "wall_generator",
    "wall_maximum_deviation", "wall_maximum_resolution", "wall_transition_angle",
    "wall_transition_filter_deviation", "wall_transition_length", "xy_contour_compensation",
    "xy_hole_compensation",
];

/// The options of `PrintRegionConfig` (PrintConfig.hpp), by key, in byte order: its `keys()`,
/// with `ironing_direction` and `wall_infill_order`, which it declares with no definition.
#[rustfmt::skip]
const REGION_KEYS: &[&str] = &[
    "align_infill_direction_to_model", "alternate_extra_wall", "bottom_shell_layers",
    "bottom_shell_thickness", "bottom_solid_infill_flow_ratio", "bottom_surface_density",
    "bottom_surface_filament_id", "bottom_surface_pattern", "bridge_angle", "bridge_density",
    "bridge_flow", "bridge_line_width", "bridge_speed", "counterbore_hole_bridging",
    "detect_overhang_wall", "detect_thin_wall", "enable_overhang_speed",
    "ensure_vertical_shell_thickness", "extra_perimeters_on_overhangs", "extra_solid_infills",
    "filament_ironing_flow", "filament_ironing_inset", "filament_ironing_spacing",
    "filament_ironing_speed", "fill_multiline", "filter_out_gap_fill", "first_layer_flow_ratio",
    "fuzzy_skin", "fuzzy_skin_first_layer", "fuzzy_skin_layers_between_ripple_offset",
    "fuzzy_skin_mode", "fuzzy_skin_noise_type", "fuzzy_skin_octaves", "fuzzy_skin_persistence",
    "fuzzy_skin_point_distance", "fuzzy_skin_ripple_offset", "fuzzy_skin_ripples_per_layer",
    "fuzzy_skin_scale", "fuzzy_skin_thickness", "gap_fill_flow_ratio", "gap_infill_speed",
    "gyroid_optimized", "hole_to_polyhole", "hole_to_polyhole_threshold",
    "hole_to_polyhole_twisted", "infill_anchor", "infill_anchor_max", "infill_combination",
    "infill_combination_max_layer_height", "infill_direction", "infill_lock_depth",
    "infill_overhang_angle", "infill_shift_step", "infill_wall_overlap", "inner_wall_filament_id",
    "inner_wall_flow_ratio", "inner_wall_line_width", "inner_wall_speed", "internal_bridge_angle",
    "internal_bridge_flow", "internal_bridge_speed", "internal_solid_filament_id",
    "internal_solid_infill_flow_ratio", "internal_solid_infill_line_width",
    "internal_solid_infill_pattern", "internal_solid_infill_speed", "ironing_angle",
    "ironing_angle_fixed", "ironing_direction", "ironing_flow", "ironing_inset", "ironing_pattern",
    "ironing_spacing", "ironing_speed", "ironing_type", "is_infill_first",
    "lateral_lattice_angle_1", "lateral_lattice_angle_2", "lightning_overhang_angle",
    "lightning_prune_angle", "lightning_straightening_angle", "make_overhang_printable",
    "min_width_top_surface", "minimum_sparse_infill_area", "only_one_wall_first_layer",
    "only_one_wall_top", "outer_wall_filament_id", "outer_wall_flow_ratio", "outer_wall_line_width",
    "outer_wall_speed", "overhang_1_4_speed", "overhang_2_4_speed", "overhang_3_4_speed",
    "overhang_4_4_speed", "overhang_flow_ratio", "overhang_reverse",
    "overhang_reverse_internal_only", "overhang_reverse_threshold", "precise_outer_wall",
    "print_extruder_id", "print_extruder_variant", "print_flow_ratio", "relative_bridge_angle",
    "role_based_wipe_speed", "scarf_angle_threshold", "scarf_joint_flow_ratio", "scarf_joint_speed",
    "scarf_overhang_threshold", "seam_gap", "seam_slope_conditional", "seam_slope_entire_loop",
    "seam_slope_inner_walls", "seam_slope_min_length", "seam_slope_start_height",
    "seam_slope_steps", "seam_slope_type", "skeleton_infill_density", "skeleton_infill_line_width",
    "skin_infill_density", "skin_infill_depth", "skin_infill_line_width",
    "slowdown_for_curled_perimeters", "small_area_infill_flow_compensation",
    "small_perimeter_speed", "small_perimeter_threshold", "solid_infill_direction",
    "solid_infill_rotate_template", "sparse_infill_density", "sparse_infill_filament_id",
    "sparse_infill_flow_ratio", "sparse_infill_line_width", "sparse_infill_pattern",
    "sparse_infill_rotate_template", "sparse_infill_speed", "symmetric_infill_y_axis",
    "top_bottom_infill_wall_overlap", "top_shell_layers", "top_shell_thickness",
    "top_solid_infill_flow_ratio", "top_surface_density", "top_surface_filament_id",
    "top_surface_line_width", "top_surface_pattern", "top_surface_speed", "wall_direction",
    "wall_infill_order", "wall_loops", "wall_sequence", "wipe_before_external_loop",
    "wipe_on_loops", "wipe_speed", "zaa_dont_alternate_fill_direction", "zaa_enabled", "zaa_min_z",
    "zaa_minimize_perimeter_height",
];

/// `CustomGCode::PausePrint`.
pub const PAUSE_PRINT: i32 = 1;
/// `CustomGCode::ToolChange`.
pub const TOOL_CHANGE: i32 = 2;
/// `CustomGCode::Custom`.
pub const CUSTOM: i32 = 4;

/// `CustomGCode::SingleExtruder`.
const SINGLE_EXTRUDER: i32 = 1;
/// `CustomGCode::MultiAsSingle`.
const MULTI_AS_SINGLE: i32 = 2;
/// `CustomGCode::MultiExtruder`.
const MULTI_EXTRUDER: i32 = 3;

/// `PrintSequence::ByLayer` and `PrintSequence::ByObject`.
const BY_LAYER: i32 = 0;
const BY_OBJECT: i32 = 1;
/// `TimelapseType::tlTraditional` and `TimelapseType::tlSmooth`.
const TL_TRADITIONAL: i32 = 0;
const TL_SMOOTH: i32 = 1;
/// `WipeTowerWallType::wtwRectangle` and `WipeTowerWallType::wtwRib`.
const WTW_RECTANGLE: i32 = 0;
const WTW_RIB: i32 = 2;
/// `PrinterStructure::psUndefine` and `PrinterStructure::psI3` (a bed slinger).
const PS_UNDEFINE: i32 = 0;
const PS_I3: i32 = 2;

/// `WIPE_TOWER_MARGIN`.
const WIPE_TOWER_MARGIN: f64 = 1.;

/// `CustomGCode::Item`: an item's print Z, type, filament (1-based), colour and its text.
#[derive(Clone, Debug, PartialEq)]
pub struct GcodeItem {
    pub print_z: f64,
    pub ty: i32,
    pub extruder: i32,
    pub color: String,
    pub extra: String,
}

/// `CustomGCode::Info`: the mode and the items of one plate.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GcodeInfo {
    pub mode: i32,
    pub gcodes: Vec<GcodeItem>,
}

/// `Model::plates_custom_gcodes`: each plate's custom G-code, by plate.
pub type PlatesCustomGcodes = BTreeMap<i32, GcodeInfo>;

/// `getInt()` of a setting: an int's or an enum's value.
fn get_int(o: &OptionValue) -> Result<i32, Failure> {
    match o {
        OptionValue::Int(v) | OptionValue::Enum { value: v, .. } => Ok(*v),
        _ => Err(fail(
            "internal-error",
            "Calling ConfigOption::getInt on a non-int ConfigOption",
        )),
    }
}

/// `getBool()` of a setting.
fn get_bool(o: &OptionValue) -> Result<bool, Failure> {
    match o {
        OptionValue::Bool(v) => Ok(*v),
        _ => Err(fail(
            "internal-error",
            "Calling ConfigOption::getBool on a non-boolean ConfigOption",
        )),
    }
}

/// `cfg_bool(cfg, key)`: the value of a true-or-false setting; false for one the configuration
/// does not hold or a list.
///
/// # Errors
///
/// `internal-error` with `getBool`'s exception for any other single value.
pub fn cfg_bool(cfg: &Config, key: &str) -> Result<bool, Failure> {
    match cfg.get(key) {
        Some(o) if !o.ty().is_vector() => get_bool(o),
        _ => Ok(false),
    }
}

/// `cfg_enum(cfg, key, fallback)`: the value of an int or enum setting; `fallback` for one the
/// configuration does not hold or a list.
///
/// # Errors
///
/// `internal-error` with `getInt`'s exception for any other single value.
pub fn cfg_enum(cfg: &Config, key: &str, fallback: i32) -> Result<i32, Failure> {
    match cfg.get(key) {
        Some(o) if !o.ty().is_vector() => get_int(o),
        _ => Ok(fallback),
    }
}

/// `by_object(cfg)`: whether the print goes object by object (`print_sequence`).
fn by_object(cfg: &Config) -> Result<bool, Failure> {
    Ok(cfg_enum(cfg, "print_sequence", BY_LAYER)? == BY_OBJECT)
}

/// `blank(s)`: every byte white space in the "C" locale (an empty text too).
fn blank(s: &str) -> bool {
    s.bytes().all(is_c_space)
}

/// `opt_float(key)` of an option the C++ dereferences without a check.
fn opt_float(o: Option<&OptionValue>, key: &str) -> f64 {
    match o {
        Some(OptionValue::Float(v) | OptionValue::Percent(v)) => *v,
        Some(OptionValue::FloatOrPercent { value, .. }) => *value,
        other => panic!("opt_float(\"{key}\") of {other:?}: the C++ dereferences a null pointer"),
    }
}

/// `opt_int(key)` of an option the C++ dereferences without a check
/// (`dynamic_cast<const ConfigOptionInt*>`).
fn opt_int(o: Option<&OptionValue>, key: &str) -> i32 {
    match o {
        Some(OptionValue::Int(v)) => *v,
        other => panic!("opt_int(\"{key}\") of {other:?}: the C++ dereferences a null pointer"),
    }
}

/// `opt_string(key)` of an option the C++ dereferences without a check.
fn opt_string<'a>(cfg: &'a Config, key: &str) -> &'a str {
    match cfg.get(key) {
        Some(OptionValue::String(s)) => s,
        other => panic!("opt_string(\"{key}\") of {other:?}: the C++ dereferences a null pointer"),
    }
}

/// `option<ConfigOptionPoints>(key)`: the points, `None` when the configuration has no such
/// option.
fn points<'a>(cfg: &'a Config, key: &str) -> Option<&'a Vec<[f64; 2]>> {
    match cfg.get(key) {
        Some(OptionValue::Points(v)) => Some(v),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// Per-object settings, height ranges and custom G-code

/// `for_object(index, f)`'s catch: a failure that names no object names object `index`.
fn for_object(index: i32, e: Failure) -> Failure {
    if e.object_index >= 0 {
        e
    } else {
        Failure {
            object_index: index,
            ..e
        }
    }
}

/// `check_extruder_setting(c, filaments, field)`: an `extruder` of `c` names one of the job's
/// filaments, or 0.
fn check_extruder_setting(c: &Config, filaments: usize, field: &str) -> Result<(), Failure> {
    if !c.has("extruder") {
        return Ok(());
    }
    let e = opt_int(c.get("extruder"), "extruder");
    // size_t(e) > filaments, with e not negative.
    if e < 0 || e as usize > filaments {
        return Err(fail(
            "setting-invalid",
            format!(
                "{field}: extruder {e} is not one of the job's {filaments} filament(s) (0 keeps the default)"
            ),
        ));
    }
    Ok(())
}

/// `layer_config_ranges[range].assign_config(config)`: the range's settings replaced, or the
/// range put in its place by `std::pair`'s `<`.
#[allow(clippy::neg_cmp_op_on_partial_ord)] // std::pair's operator< as the C++ writes it
fn set_height_range(ranges: &mut LayerConfigRanges, range: (f64, f64), config: ModelConfig) {
    let less = |a: &(f64, f64), b: &(f64, f64)| a.0 < b.0 || (!(b.0 < a.0) && a.1 < b.1);
    let at = orca_stl::lower_bound_by(ranges, &range, |e, r| less(&e.0, r));
    if at < ranges.len() && !less(&range, &ranges[at].0) {
        ranges[at].1 = config;
    } else {
        ranges.insert(at, (range, config));
    }
}

/// `apply_object_settings`' work on the object the job lists at `pos`.
#[allow(clippy::neg_cmp_op_on_partial_ord)] // `!(h > 0.)` as the C++ writes it, a NaN refused
fn apply_object(
    model: &mut Model,
    jo: &JobObject,
    pos: usize,
    filaments: usize,
    s: &Settings,
) -> Result<(), Failure> {
    let f = format!("objects[{pos}]");
    // The index was checked against the model's objects in the model stage.
    let mo = &mut model.objects[jo.index as usize];
    if !jo.overrides.is_empty() {
        let mut over = Config::default();
        for (k, v) in &jo.overrides {
            let field = format!("{f}.overrides.{k}");
            let key = parse_setting(&mut over, k, v, &field)?;
            if key != "extruder"
                && !OBJECT_KEYS.contains(&key.as_str())
                && !REGION_KEYS.contains(&key.as_str())
            {
                return Err(fail_object(
                    "setting-invalid",
                    format!(
                        "{field}: {key} is a printer, filament or whole-plate setting, not a per-object one; set it in overrides"
                    ),
                    jo.index,
                ));
            }
        }
        check_extruder_setting(&over, filaments, &format!("{f}.overrides.extruder"))?;
        // ModelConfig::apply: each option's value set into the object's own option, made with
        // its default first when the object has none.
        for (key, value) in over.iter() {
            let mut mine = match mo.config.option(key) {
                Some(v) => v.clone(),
                None => create_default(
                    orca_config::get(key).expect("parse_setting gives a key Orca defines"),
                ),
            };
            set_value(&mut mine, value).map_err(|e| fail("internal-error", e.0))?;
            mo.config.set_key_value(key, mine);
        }
    }
    if !jo.height_ranges.is_empty() {
        mo.layer_config_ranges.clear();
        let layer_height = if mo.config.has("layer_height") {
            opt_float(mo.config.option("layer_height"), "layer_height")
        } else {
            opt_float(s.config.get("layer_height"), "layer_height")
        };
        for (r, hr) in jo.height_ranges.iter().enumerate() {
            let rf = format!("{f}.heightRanges[{r}]");
            let mut range = Config::default();
            range.insert("layer_height", OptionValue::Float(layer_height));
            range.insert("extruder", OptionValue::Int(0));
            for (k, v) in &hr.overrides {
                let field = format!("{rf}.overrides.{k}");
                let key = parse_setting(&mut range, k, v, &field)?;
                if key != "layer_height"
                    && key != "extruder"
                    && !REGION_KEYS.contains(&key.as_str())
                {
                    return Err(fail_object(
                        "setting-invalid",
                        format!(
                            "{field}: {key} cannot change over a height range; height ranges take layer_height, extruder and region settings (walls, infill, ...)"
                        ),
                        jo.index,
                    ));
                }
            }
            if !(opt_float(range.get("layer_height"), "layer_height") > 0.) {
                return Err(fail_object(
                    "setting-invalid",
                    format!("{rf}.overrides.layer_height must be above 0"),
                    jo.index,
                ));
            }
            check_extruder_setting(&range, filaments, &format!("{rf}.overrides.extruder"))?;
            let mut config = ModelConfig::new();
            for (key, value) in range.iter() {
                config.set_key_value(key, value.clone());
            }
            set_height_range(&mut mo.layer_config_ranges, (hr.from, hr.to), config);
        }
    }
    Ok(())
}

/// `apply_object_settings(model, job, s)`: the job's per-object overrides (object settings and
/// region settings, as the object list of Orca's GUI offers them) and height ranges (a modifier
/// over a z range of the object, with its own layer height, as
/// `ObjectList::get_default_layer_config` starts it).
///
/// # Errors
///
/// `setting-invalid` naming the object, as above, and `parse_setting`'s failures with the object
/// named.
pub fn apply_object_settings(model: &mut Model, job: &Job, s: &Settings) -> Result<(), Failure> {
    let n = s.filaments.len();
    for (pos, jo) in job.objects.iter().enumerate() {
        apply_object(model, jo, pos, n, s).map_err(|e| for_object(jo.index, e))?;
    }
    Ok(())
}

/// `set_custom_gcodes(model, job, s)`: the job's custom G-codes as Orca's per-layer custom G-code
/// of plate 1, the plate made current. Orca 2.4.2 compiles out the M600 branch of a colour change
/// (GCode.cpp, `ProcessLayer::emit_custom_gcode_per_print_z`, `#if 0`), so a colour change there
/// would emit nothing useful: a pause is Orca's pause (the printer's `machine_pause_gcode`); a
/// colour change with one filament is a pause too, for a manual swap; a colour change with several
/// filaments, and a filament change, switch to the given filament from that layer on, as a tool
/// change; custom is the given G-code.
///
/// # Errors
///
/// `unsupported` with print_sequence "by object", `custom-gcode-invalid` for a colour change
/// without its filament, a filament change with one filament or past the job's, and a pause
/// without the printer's pause G-code.
pub fn set_custom_gcodes(
    model: &mut Model,
    job: &Job,
    s: &Settings,
) -> Result<PlatesCustomGcodes, Failure> {
    let mut plates = PlatesCustomGcodes::new();
    model.curr_plate_index = 0;
    if job.custom_gcodes.is_empty() {
        return Ok(plates);
    }
    if by_object(&s.config)? {
        return Err(fail(
            "unsupported",
            "custom G-code per layer is not available with print_sequence \"by object\"; Orca runs it only for prints by layer",
        ));
    }
    let n = s.filaments.len();
    let pause_gcode = opt_string(&s.config, "machine_pause_gcode");
    let mut info = GcodeInfo::default();
    let mut tool_changes = false;
    for c in &job.custom_gcodes {
        let at = || format!("the {} at printZ {}", c.kind, mm(c.print_z));
        let mut item = GcodeItem {
            print_z: c.print_z,
            ty: CUSTOM,
            extruder: if c.extruder > 0 { c.extruder } else { 1 },
            color: c.color.clone(),
            extra: String::new(),
        };
        let mut pause = false;
        if c.kind == "pause" {
            pause = true;
        } else if c.kind == "color" {
            if n == 1 {
                pause = true;
            } else {
                if c.extruder == 0 {
                    return Err(fail(
                        "custom-gcode-invalid",
                        format!(
                            "{} needs the extruder: the job has {n} filaments, so a color change switches to the filament of that color",
                            at()
                        ),
                    ));
                }
                item.ty = TOOL_CHANGE;
            }
        } else if c.kind == "filament" {
            if n < 2 {
                return Err(fail(
                    "custom-gcode-invalid",
                    format!("{} needs at least two filaments; the job has one", at()),
                ));
            }
            item.ty = TOOL_CHANGE;
        } else {
            item.ty = CUSTOM;
            item.extra = c.gcode.clone();
        }
        if pause {
            if blank(pause_gcode) {
                return Err(fail(
                    "custom-gcode-invalid",
                    format!(
                        "{} needs the printer's pause G-code, and machine_pause_gcode is empty in this machine preset",
                        at()
                    ),
                ));
            }
            item.ty = PAUSE_PRINT;
            item.extra = c.note.clone();
            if item.extra.is_empty() && c.kind == "color" {
                item.extra = format!("Change the filament to {}", c.color);
            }
        }
        if item.ty == TOOL_CHANGE {
            // size_t(c.extruder) > n: a negative filament is past any.
            if c.extruder < 0 || c.extruder as usize > n {
                return Err(fail(
                    "custom-gcode-invalid",
                    format!(
                        "{}: filament {} is not one of the job's {n} filaments",
                        at(),
                        c.extruder
                    ),
                ));
            }
            tool_changes = true;
        }
        info.gcodes.push(item);
    }
    info.mode = if tool_changes {
        MULTI_AS_SINGLE
    } else if n == 1 {
        SINGLE_EXTRUDER
    } else {
        MULTI_EXTRUDER
    };
    plates.insert(0, info);
    Ok(plates)
}

// ---------------------------------------------------------------------------------------------
// Arrange, prime tower and build volume

/// `getInt()` of an object's own setting, `fallback` when the object has none (`object_int`).
fn object_int(mo: &ModelObject, key: &str, fallback: i32) -> Result<i32, Failure> {
    match mo.config.option(key) {
        Some(o) => get_int(o),
        None => Ok(fallback),
    }
}

/// `add(e)`: a filament from 1 to `n` counted once.
fn add_filament(used: &mut BTreeSet<i32>, e: i32, n: usize) {
    if e >= 1 && e as usize <= n {
        used.insert(e);
    }
}

/// `plate_filaments(model, cfg, n)`: the filaments plate 1 prints with, as the stock command line
/// counts them for its prime tower (`PartPlate::get_extruders_under_cli` with custom G-code): the
/// parts' filaments (assigned and painted; a part without one prints with filament 1), height
/// ranges, support when it is on, the per-feature filaments of the objects and the print, and
/// filament changes at a layer (`gcodes`, plate 1's). Orca's version counts the per-feature
/// filaments only when support is on; here they always count. An object that sets only
/// `raft_layers` keeps the print's `enable_support` (sketch-orca, 2026-11-01).
fn plate_filaments(
    model: &Model,
    cfg: &Config,
    n: usize,
    gcodes: &PlatesCustomGcodes,
) -> Result<BTreeSet<i32>, Failure> {
    let mut used = BTreeSet::new();
    let global_enable = cfg_bool(cfg, "enable_support")?;
    let global_raft = cfg_enum(cfg, "raft_layers", 0)?;
    for mo in &model.objects {
        if !mo.printable || !mo.instances.iter().any(|i| i.printable) {
            continue;
        }
        let object_extruder = object_int(mo, "extruder", 0)?;
        for v in &mo.volumes {
            if !v.is_model_part() {
                continue;
            }
            let mut e = v.get_extruders(&mo.config);
            if e.is_empty() {
                e.push(if object_extruder > 0 {
                    object_extruder
                } else {
                    1
                });
            }
            for x in e {
                add_filament(&mut used, x, n);
            }
        }
        for (_, range) in &mo.layer_config_ranges {
            if let Some(o) = range.option("extruder") {
                add_filament(&mut used, get_int(o)?, n);
            }
        }
        let enabled = match mo.config.option("enable_support") {
            Some(o) => get_bool(o)?,
            None => global_enable,
        };
        let support = enabled || object_int(mo, "raft_layers", global_raft)? > 0;
        if support {
            for key in ["support_filament", "support_interface_filament"] {
                let e = if object_int(mo, key, 0)? != 0 {
                    object_int(mo, key, 0)?
                } else {
                    cfg_enum(cfg, key, 0)?
                };
                add_filament(&mut used, e, n);
            }
        }
        for key in [
            "outer_wall_filament_id",
            "inner_wall_filament_id",
            "sparse_infill_filament_id",
            "internal_solid_filament_id",
            "top_surface_filament_id",
            "bottom_surface_filament_id",
        ] {
            let e = if object_int(mo, key, 0)? != 0 {
                object_int(mo, key, 0)?
            } else {
                cfg_enum(cfg, key, 0)?
            };
            add_filament(&mut used, e, n);
        }
    }
    if let Some(info) = gcodes.get(&0) {
        for item in &info.gcodes {
            if item.ty == TOOL_CHANGE {
                add_filament(&mut used, item.extruder, n);
            }
        }
    }
    Ok(used)
}

/// `WipeTower::min_depth_per_height`: the prime tower's least depth by its height, in order.
const MIN_DEPTH_PER_HEIGHT: [(f32, f32); 4] = [(5., 5.), (100., 20.), (250., 40.), (350., 60.)];

/// `WipeTower::get_limit_depth_by_height(max_height)`: the least depth of a tower that high, the
/// table's first below its first height and its last past its last, and in between the line
/// between the two heights around it.
fn get_limit_depth_by_height(max_height: f32) -> f32 {
    let mut min_wipe_tower_depth = 0.;
    let mut at = 0;
    while at < MIN_DEPTH_PER_HEIGHT.len() {
        let curr = MIN_DEPTH_PER_HEIGHT[at];
        if curr.0 >= max_height {
            min_wipe_tower_depth = curr.1;
            break;
        }
        at += 1;
        if at == MIN_DEPTH_PER_HEIGHT.len() {
            min_wipe_tower_depth = curr.1;
            break;
        }
        let next = MIN_DEPTH_PER_HEIGHT[at];
        if next.0 > max_height {
            let height_diff = next.0 - curr.0;
            let min_depth_base = curr.1;
            let depth_diff = next.1 - curr.1;
            min_wipe_tower_depth =
                min_depth_base + (max_height - curr.0) / height_diff * depth_diff;
            break;
        }
    }
    min_wipe_tower_depth
}

/// `WipeTower::get_auto_brim_by_height(max_height)`: 8 mm from 100 mm up, less below.
fn get_auto_brim_by_height(max_height: f32) -> f32 {
    if max_height < 100. {
        max_height / 100. * 8.
    } else {
        8.
    }
}

/// `PartPlate::estimate_wipe_tower_size(cfg, w, wipe_volume, extruder_count,
/// plate_extruder_size, use_global_objects, enable_wrapping)`: the prime tower's width, depth and
/// height (the tallest object's) for `plate_extruder_size` filaments, zero for none.
#[allow(clippy::too_many_arguments)] // the C++ function's own parameters
fn estimate_wipe_tower_size(
    cfg: &Config,
    model: &mut Model,
    w: f64,
    wipe_volume: f64,
    extruder_count: i32,
    plate_extruder_size: i32,
    enable_wrapping: bool,
) -> Result<[f64; 3], Failure> {
    let mut size = [0.; 3];
    let layer_height = cfg_float(cfg, "layer_height", 0.08)?;
    if plate_extruder_size == 0 {
        return Ok(size);
    }
    let mut max_height = 0.;
    for mo in &mut model.objects {
        max_height = orca_stl::max(mo.bounding_box_exact().size().z(), max_height);
    }
    size[2] = max_height;
    let need_wipe_tower =
        cfg_enum(cfg, "timelapse_type", TL_TRADITIONAL)? == TL_SMOOTH || enable_wrapping;
    let extra_spacing = cfg_float(cfg, "prime_tower_infill_gap", 100.)? / 100.;
    let use_rib_wall = cfg_enum(cfg, "wipe_tower_wall_type", WTW_RECTANGLE)? == WTW_RIB;
    let mut rib_width = cfg_float(cfg, "wipe_tower_rib_width", 8.)?;
    let filament_change_volume = {
        let (mut length, mut diameter) = (0., 1.75);
        if let Some(values) = cfg.floats("filament_change_length")
            && !values.is_empty()
        {
            length = values[orca_stl::max_element_f64(values)];
        }
        if let Some(values) = cfg.floats("filament_diameter")
            && !values.is_empty()
        {
            diameter = values[orca_stl::max_element_f64(values)];
        }
        length * PI * diameter * diameter / 4.
    };
    let mut volume = wipe_volume
        * f64::from(if extruder_count == 2 {
            plate_extruder_size
        } else {
            plate_extruder_size - 1
        });
    if extruder_count == 2 {
        volume += filament_change_volume * f64::from(plate_extruder_size / 2);
    }
    if use_rib_wall {
        let mut depth = orca_crt::sqrt(volume / layer_height * extra_spacing);
        if need_wipe_tower || plate_extruder_size > 1 {
            let min_depth = get_limit_depth_by_height(max_height as f32);
            let volume_depth = depth;
            depth = orca_stl::max(f64::from(min_depth), depth);
            rib_width = orca_stl::min(rib_width, depth / 2.);
            let extra = cfg_float(cfg, "wipe_tower_extra_rib_length", 0.)?;
            depth = rib_width / orca_crt::sqrt(2.) + orca_stl::max(depth + extra, volume_depth);
            size[0] = depth;
            size[1] = depth;
        }
    } else {
        let mut depth = volume / (layer_height * w) * extra_spacing;
        if need_wipe_tower || depth > EPSILON {
            let min_depth = get_limit_depth_by_height(max_height as f32);
            depth = orca_stl::max(f64::from(min_depth), depth);
        }
        size[0] = w;
        size[1] = depth;
    }
    Ok(size)
}

/// `excluded_areas(cfg)`: the areas of the bed nothing is placed on, with the names arrange gives
/// them (`PartPlateList::preprocess_exclude_areas`): the clumping detection area when it is on,
/// then each excluded rectangle (`bed_exclude_area`, four points each).
fn excluded_areas(cfg: &Config) -> Result<Vec<(String, Polygon)>, Failure> {
    let mut out = Vec::new();
    if cfg_bool(cfg, "enable_wrapping_detection")?
        && let Some(area) = points(cfg, "wrapping_exclude_area")
        && !area.is_empty()
    {
        let poly = Polygon::new(area.iter().map(|p| Point::new_scale(p[0], p[1])).collect());
        out.push(("WrappingRegion".to_string(), poly));
    }
    if let Some(pts) = points(cfg, "bed_exclude_area") {
        for (index, corners) in pts.chunks_exact(4).enumerate() {
            let mut bb = BoundingBoxf::default();
            for p in corners {
                bb.merge(Vec2d::new(p[0], p[1]));
            }
            out.push((
                format!("ExcludedRegion{index}"),
                Polygon::new(vec![
                    Point::new(scaled(bb.min.x()), scaled(bb.min.y())),
                    Point::new(scaled(bb.max.x()), scaled(bb.min.y())),
                    Point::new(scaled(bb.max.x()), scaled(bb.max.y())),
                    Point::new(scaled(bb.min.x()), scaled(bb.max.y())),
                ]),
            ));
        }
    }
    Ok(out)
}

/// `printable_polygon(cfg)`: the printer's printable area (`printable_area`) as a polygon, counter
/// clockwise; empty when it has fewer than three points.
fn printable_polygon(cfg: &Config) -> Polygon {
    let mut out = Polygon::default();
    if let Some(area) = points(cfg, "printable_area")
        && area.len() >= 3
    {
        out.points = area.iter().map(|p| Point::new_scale(p[0], p[1])).collect();
        out.make_counter_clockwise();
    }
    out
}

/// `rect_mm(x0, y0, x1, y1)`: the rectangle from x0, y0 to x1, y1, in mm.
fn rect_mm(x0: f64, y0: f64, x1: f64, y1: f64) -> Polygon {
    Polygon::new(vec![
        Point::new(scaled(x0), scaled(y0)),
        Point::new(scaled(x1), scaled(y0)),
        Point::new(scaled(x1), scaled(y1)),
        Point::new(scaled(x0), scaled(y1)),
    ])
}

/// `scaled(v)` of a `float`: `coord_t(v / float(SCALING_FACTOR))`, the division in floats.
fn scaled_float(v: f32) -> Coord {
    orca_crt::f32_to_i64(v / SCALING_FACTOR as f32)
}

/// `tower_misfit(foot, printable, excluded, objects)`: whether the prime tower's footprint `foot`
/// lies on the printable area (when the printer gives one) and clear of the excluded areas and of
/// `objects`: "" when it does, else why not (sketch-orca, 2026-11-01).
fn tower_misfit(
    foot: &Polygon,
    printable: &Polygon,
    excluded: &[(String, Polygon)],
    objects: &[Polygon],
) -> String {
    let tolerance = sqr(scale_(0.01));
    let feet = [foot.clone()];
    if !printable.is_empty() {
        for &corner in &foot.points {
            if !printable.contains(corner) {
                return "is not all on the printable area".to_string();
            }
        }
        if area(&diff(
            &feet,
            std::slice::from_ref(printable),
            ApplySafetyOffset::No,
        )) > tolerance
        {
            return "is not all on the printable area".to_string();
        }
    }
    for (name, poly) in excluded {
        if area(&intersection(
            &feet,
            std::slice::from_ref(poly),
            ApplySafetyOffset::No,
        )) > tolerance
        {
            return format!("is on an excluded area of the bed ({name})");
        }
    }
    if !objects.is_empty() && area(&intersection(&feet, objects, ApplySafetyOffset::No)) > tolerance
    {
        return "is on an object".to_string();
    }
    String::new()
}

/// `tower_places(foot, printable, obstacles)`: where the first corner of the convex footprint
/// `foot` may go so that the footprint lies on `printable` and clear of `obstacles`, as a region
/// (sketch-orca, 2026-11-01). With F the footprint seen from that corner, a place p is taken by
/// F + p, so the places a polygon O blocks are O grown by -F: O together with -F swept along O's
/// edges (Clipper's Minkowski sum). The printable area blocks the places where the footprint would
/// cross its edges: those edges swept by -F. The region is what of the printable area no obstacle
/// and no edge blocks.
fn tower_places(foot: &Polygon, printable: &Polygon, obstacles: &[Polygon]) -> ExPolygons {
    let corner = foot.points[0];
    let back: Vec<IntPoint> = foot.points.iter().map(|&q| corner - q).collect();
    let mut blocked: Polygons = Vec::new();
    let sweep = |edges: &Polygon, blocked: &mut Polygons| {
        for path in minkowski_sum(&back, &edges.points, true) {
            blocked.push(Polygon::new(path));
        }
    };
    sweep(printable, &mut blocked);
    for o in obstacles {
        if o.points.len() < 3 {
            continue;
        }
        let mut solid = o.clone();
        solid.make_counter_clockwise();
        blocked.push(solid.clone());
        sweep(&solid, &mut blocked);
    }
    diff_ex(
        std::slice::from_ref(printable),
        &union_(&blocked),
        ApplySafetyOffset::No,
    )
}

/// `nearest_point_in(region, p)`: the point of `region` nearest to `p`: `p` itself when it is in
/// the region, else the nearest point of an edge, rounded; none for an empty region.
fn nearest_point_in(region: &[ExPolygon], p: Point) -> Option<Point> {
    if region.iter().any(|e| e.contains(p, true)) {
        return Some(p);
    }
    let mut best = None;
    let mut best_d2 = f64::MAX;
    let q = p.cast_f64();
    for e in region {
        for poly in std::iter::once(&e.contour).chain(&e.holes) {
            let n = poly.points.len();
            for i in 0..n {
                let a = poly.points[i].cast_f64();
                let b = poly.points[(i + 1) % n].cast_f64();
                let ab = b - a;
                let l2 = ab.squared_norm();
                let t = if l2 > 0. {
                    orca_stl::clamp((q - a).dot(&ab) / l2, 0., 1.)
                } else {
                    0.
                };
                let c = a + ab * t;
                let d2 = (c - q).squared_norm();
                if d2 < best_d2 {
                    best_d2 = d2;
                    best = Some(Point::new(
                        orca_crt::llround(c.x()),
                        orca_crt::llround(c.y()),
                    ));
                }
            }
        }
    }
    best
}

/// `nearest_tower_move(foot, printable, excluded, objects)`: the shortest move, in mm, after
/// which the convex footprint `foot` fits ([`tower_misfit`]). It is found in the region of the
/// places where the footprint fits ([`tower_places`]), kept 5 micrometres inside so that the
/// footprint clears every edge: the nearest place, then, should rounding put that one on an edge,
/// the corners of the region by their distance (sketch-orca, 2026-11-01). None when there is no
/// such place or the printer gives no printable area.
fn nearest_tower_move(
    foot: &Polygon,
    printable: &Polygon,
    excluded: &[(String, Polygon)],
    objects: &[Polygon],
) -> Option<(f64, f64)> {
    if printable.is_empty() || foot.points.len() < 3 {
        return None;
    }
    let mut obstacles: Polygons = objects.to_vec();
    for (_, poly) in excluded {
        obstacles.push(poly.clone());
    }
    let places = offset_ex_expolygons(
        &tower_places(foot, printable, &obstacles),
        -(scale_(0.005) as f32),
        DEFAULT_JOIN_TYPE,
        DEFAULT_MITER_LIMIT,
    );
    let from = foot.points[0];
    let mut candidates: Points = Vec::new();
    if let Some(nearest) = nearest_point_in(&places, from) {
        candidates.push(nearest);
    }
    let mut corners: Points = Vec::new();
    for e in &places {
        corners.extend_from_slice(&e.contour.points);
        for h in &e.holes {
            corners.extend_from_slice(&h.points);
        }
    }
    orca_stl::stable_sort_by(&mut corners, |a, b| {
        (*a - from).cast_f64().squared_norm() < (*b - from).cast_f64().squared_norm()
    });
    candidates.extend(corners);
    for to in candidates {
        let mut moved = foot.clone();
        moved.translate(to - from);
        if tower_misfit(&moved, printable, excluded, objects).is_empty() {
            return Some((unscale(to.x - from.x), unscale(to.y - from.y)));
        }
    }
    None
}

/// `tower_set_by_job(s)`: whether the job sets the prime tower's place itself (`wipe_tower_x` or
/// `wipe_tower_y`).
fn tower_set_by_job(s: &Settings) -> bool {
    s.overridden.iter().any(|k| k == "wipe_tower_x")
        || s.overridden.iter().any(|k| k == "wipe_tower_y")
}

/// `option<ConfigOptionFloats>(key, true)`, the option made with its default when the
/// configuration has none: its first value (`None` for none), and the whole `None` for an option
/// of another type, the C++'s null pointer, which [`tower_coordinate_of`] dereferences.
fn tower_coordinate(cfg: &mut Config, key: &str) -> Option<Option<f64>> {
    match cfg.create(key) {
        Some(OptionValue::Floats { values, .. }) => Some(values.first().copied()),
        _ => None,
    }
}

/// The dereference of [`tower_coordinate`]'s pointer.
fn tower_coordinate_of(xs: Option<Option<f64>>, key: &str) -> Option<f64> {
    match xs {
        Some(first) => first,
        None => panic!(
            "option<ConfigOptionFloats>(\"{key}\", true) is not a list of floats: the C++ dereferences a null pointer"
        ),
    }
}

/// The tower's place written to `key`: its first value, or its only one when it has none.
fn set_tower_coordinate(cfg: &mut Config, key: &str, v: f32) {
    match cfg.get_mut(key) {
        Some(OptionValue::Floats { values, .. }) => {
            if values.is_empty() {
                values.push(f64::from(v));
            } else {
                values[0] = f64::from(v);
            }
        }
        _ => panic!(
            "option<ConfigOptionFloats>(\"{key}\", true) is not a list of floats: the C++ dereferences a null pointer"
        ),
    }
}

/// `place_prime_tower(cfg, model, s)`: where Orca's prime tower goes on plate 1, set in
/// `wipe_tower_x` and `wipe_tower_y`, and its footprint for arrange; none when Orca prints no
/// tower. The stock command line starts from the GUI's default corner (165, 250; 0, 250 on a bed
/// slinger), then `PartPlate::estimate_wipe_tower_polygon` keeps it on the plate. A job that sets
/// `wipe_tower_x` or `wipe_tower_y` starts from its own place instead. The corner is taken from
/// the bed's lowest corner, and the tower is kept between the bed's own edges, so a bed whose
/// printable area does not start at 0,0 gets its tower on it. Between the bed's edges is not on
/// the bed when the printable area is not a rectangle, nor clear of its excluded areas: the tower,
/// its brim included, must be on the printable area and off every excluded area; from the default
/// corner it moves to the nearest place that is (sketch-orca, 2026-11-01).
///
/// # Errors
///
/// `config-invalid` for a place the job sets that does not fit, `arrange-failed` when the tower
/// fits nowhere, and the settings' `internal-error`.
pub fn place_prime_tower(
    cfg: &mut Config,
    model: &mut Model,
    s: &Settings,
    gcodes: &PlatesCustomGcodes,
) -> Result<Option<ArrangePolygon>, Failure> {
    if by_object(cfg)? || !cfg_bool(cfg, "enable_prime_tower")? {
        return Ok(None);
    }
    let wrapping = cfg_bool(cfg, "enable_wrapping_detection")?;
    let wrap_area = points(cfg, "wrapping_exclude_area").is_some_and(|a| !a.is_empty());
    let smooth = cfg_enum(cfg, "timelapse_type", TL_TRADITIONAL)? == TL_SMOOTH;
    let filaments = plate_filaments(model, cfg, s.filaments.len(), gcodes)?.len();
    if filaments <= 1 && !smooth && !(wrapping && wrap_area) {
        return Ok(None);
    }

    let xs = tower_coordinate(cfg, "wipe_tower_x");
    let ys = tower_coordinate(cfg, "wipe_tower_y");
    let overridden = tower_set_by_job(s);
    let w = cfg_float(cfg, "prime_tower_width", 35.)? as f32;
    let (mut x, mut y): (f32, f32);
    if overridden {
        x = tower_coordinate_of(xs, "wipe_tower_x").unwrap_or(15.) as f32;
        y = tower_coordinate_of(ys, "wipe_tower_y").unwrap_or(220.) as f32;
    } else {
        let i3 = cfg_enum(cfg, "printer_structure", PS_UNDEFINE)? == PS_I3;
        x = if i3 { 0. } else { 165. };
        y = 250.;
        // OrcaSlicer.cpp names this the tower's brim but reads prime_tower_width; kept as it is.
        let tower_margin = WIPE_TOWER_MARGIN as f32 + w;
        x = orca_stl::max(x, tower_margin);
        y = orca_stl::max(y, tower_margin);
    }

    let v = cfg_float(cfg, "prime_volume", 45.)? as f32;
    let tower_brim_width = cfg_float(cfg, "prime_tower_brim_width", 3.)? as f32;
    let size = estimate_wipe_tower_size(
        cfg,
        model,
        f64::from(w),
        f64::from(v),
        s.extruder_count as i32,
        filaments as i32,
        wrapping,
    )?;
    let mut bed = BoundingBoxf::default();
    if let Some(area) = points(cfg, "printable_area") {
        for p in area {
            bed.merge(Vec2d::new(p[0], p[1]));
        }
    }
    let bed_x = (if bed.defined { bed.min.x() } else { 0. }) as f32;
    let bed_y = (if bed.defined { bed.min.y() } else { 0. }) as f32;
    let plate_width = (if bed.defined { bed.size().x() } else { 256. }) as f32;
    let plate_depth = (if bed.defined { bed.size().y() } else { 256. }) as f32;
    if !overridden {
        x += bed_x;
        y += bed_y;
    }
    let depth = size[1] as f32;
    let margin = WIPE_TOWER_MARGIN as f32 + tower_brim_width;
    let mut brim = tower_brim_width;
    if brim < 0. {
        brim = get_auto_brim_by_height(size[2] as f32);
    }
    // The engine's own place is kept between the bed's edges, as
    // PartPlate::estimate_wipe_tower_polygon keeps it; a place the job sets is checked as it is.
    if !overridden {
        let clamp = |value: f32, lo: f32, hi: f32| {
            if hi < lo {
                lo
            } else {
                orca_stl::clamp(value, lo, hi)
            }
        };
        let (lo_x, hi_x) = (bed_x + margin, bed_x + plate_width - w - margin - brim);
        let (lo_y, hi_y) = (bed_y + margin, bed_y + plate_depth - depth - margin - brim);
        x = clamp(x, lo_x, hi_x);
        y = clamp(y, lo_y, hi_y);
    }
    {
        let printable = printable_polygon(cfg);
        let excluded = excluded_areas(cfg)?;
        let foot = rect_mm(
            f64::from(x - brim),
            f64::from(y - brim),
            f64::from(x + w + brim),
            f64::from(y + depth + brim),
        );
        let why = tower_misfit(&foot, &printable, &excluded, &[]);
        if !why.is_empty() {
            let extent = format!(
                "{} x {} mm with its brim",
                mm(f64::from(w + 2. * brim)),
                mm(f64::from(depth + 2. * brim))
            );
            if overridden {
                return Err(fail(
                    "config-invalid",
                    format!(
                        "the prime tower at wipe_tower_x {}, wipe_tower_y {} ({extent}) {why}",
                        mm(f64::from(x)),
                        mm(f64::from(y))
                    ),
                ));
            }
            let Some(moved) = nearest_tower_move(&foot, &printable, &excluded, &[]) else {
                return Err(fail(
                    "arrange-failed",
                    format!(
                        "the prime tower ({extent}) fits nowhere on the printable area clear of its excluded areas"
                    ),
                ));
            };
            x += moved.0 as f32;
            y += moved.1 as f32;
        }
    }
    set_tower_coordinate(cfg, "wipe_tower_x", x);
    set_tower_coordinate(cfg, "wipe_tower_y", y);

    let contour = Polygon::new(vec![
        Point::new(scaled_float(x - brim), scaled_float(y - brim)),
        Point::new(scaled_float(x + w + brim), scaled_float(y - brim)),
        Point::new(scaled_float(x + w + brim), scaled_float(y + depth + brim)),
        Point::new(scaled_float(x - brim), scaled_float(y + depth + brim)),
    ]);
    Ok(Some(ArrangePolygon {
        poly: ExPolygon::new(contour),
        bed_idx: 0,
        translation: Point::new(0, 0),
        name: "WipeTower".to_string(),
        is_virt_object: true,
        is_wipe_tower: true,
        ..ArrangePolygon::default()
    }))
}

/// `add_exclusion_regions(out, cfg, num_plates, inflation)`: the bed's excluded areas
/// ([`excluded_areas`]) as fixed items on plates 0 to `num_plates` - 1.
fn add_exclusion_regions(
    out: &mut Vec<ArrangePolygon>,
    cfg: &Config,
    num_plates: i32,
    inflation: Coord,
) -> Result<(), Failure> {
    for (name, poly) in excluded_areas(cfg)? {
        for j in 0..num_plates {
            out.push(ArrangePolygon {
                poly: ExPolygon::new(poly.clone()),
                translation: Point::new(0, 0),
                rotation: 0.,
                is_virt_object: true,
                bed_idx: j,
                height: 1.,
                name: name.clone(),
                inflation,
                ..ArrangePolygon::default()
            });
        }
    }
    Ok(())
}

/// What the C++ program reports for an exception arrange throws: `std::bad_alloc` is
/// out-of-memory, any other `internal-error` with its `what()`.
fn arrange_failure(e: ArrangeError) -> Failure {
    match e {
        ArrangeError::Nlopt(NloptException::BadAlloc) => fail("out-of-memory", "out of memory"),
        ArrangeError::Nlopt(
            NloptException::RuntimeError(m) | NloptException::InvalidArgument(m),
        ) => fail("internal-error", m),
        ArrangeError::Nlopt(NloptException::RoundoffLimited) => {
            fail("internal-error", "nlopt roundoff-limited")
        }
        ArrangeError::Nlopt(NloptException::ForcedStop) => {
            fail("internal-error", "nlopt forced stop")
        }
        other => fail("internal-error", other.to_string()),
    }
}

/// `arrange_objects(model, cfg, tower)`: the stock command line's arrange of every object onto
/// plate 1 (OrcaSlicer.cpp, "global arrange") with its parameters, except that objects are not
/// rotated, so the parts keep the orientation the client gave them, and on a bed slinger the long
/// side is not turned along Y (`align_to_y_axis`). An object that does not fit on plate 1 is a
/// failure: a job has one plate. Orca's arrange keeps objects whose filaments are of far apart
/// temperatures off one plate; with one plate that is temperature-difference. The prime tower,
/// when there is one, and the excluded areas are fixed items.
///
/// # Errors
///
/// `temperature-difference`, `arrange-failed` naming the object, and arrange's exceptions.
pub fn arrange_objects(
    model: &mut Model,
    cfg: &Config,
    tower: Option<&ArrangePolygon>,
) -> Result<(), Failure> {
    let mut params = ArrangeParams {
        is_seq_print: by_object(cfg)?,
        ..ArrangeParams::default()
    };
    let (mut selected, mut unselected, mut unprintable) = (Vec::new(), Vec::new(), Vec::new());
    // The instance each item is of, which `ArrangePolygon::apply` writes to.
    let (mut selected_of, mut unprintable_of) = (Vec::new(), Vec::new());
    let mut owner: Vec<i32> = Vec::new();
    for oi in 0..model.objects.len() {
        let count = model.objects[oi].instances.len();
        for ii in 0..count {
            let mut ap = get_instance_arrange_poly(&mut model.objects[oi], ii, cfg);
            if model.objects[oi].instances[ii].printable {
                ap.itemid = selected.len() as i32;
                selected.push(ap);
                selected_of.push((oi, ii));
                owner.push(oi as i32);
            } else {
                ap.itemid = unprintable.len() as i32;
                unprintable.push(ap);
                unprintable_of.push((oi, ii));
            }
        }
    }
    if selected.is_empty() && unprintable.is_empty() {
        return Ok(());
    }
    add_exclusion_regions(&mut unselected, cfg, 16, 0)?;
    if let Some(tower) = tower {
        unselected.push(tower.clone());
    }

    params.allow_rotations = false;
    params.allow_multi_materials_on_same_plate = true;
    params.avoid_extrusion_cali_region = false;
    params.clearance_height_to_rod = cfg_float(cfg, "extruder_clearance_height_to_rod", 0.)? as f32;
    params.clearance_height_to_lid = cfg_float(cfg, "extruder_clearance_height_to_lid", 0.)? as f32;
    params.clearance_radius = cfg_float(cfg, "extruder_clearance_radius", 0.)? as f32;
    params.printable_height = cfg_float(cfg, "printable_height", 256.)? as f32;
    params.min_obj_distance = 0;
    if params.is_seq_print {
        params.bed_shrink_x = BED_SHRINK_SEQ_PRINT as f32;
        params.bed_shrink_y = BED_SHRINK_SEQ_PRINT as f32;
    }
    params.align_to_y_axis = false;

    for (i, a) in selected.iter().enumerate() {
        for (j, b) in selected.iter().enumerate().skip(i + 1) {
            if owner[i] != owner[j]
                && !is_filaments_compatible(&[a.filament_temp_type, b.filament_temp_type])
            {
                return Err(fail(
                    "temperature-difference",
                    format!(
                        "{} and {} print with filaments of far apart temperatures (a high-temperature and a low-temperature filament type), which Orca does not arrange on one plate; slice them as separate jobs",
                        object_label(model, owner[i]),
                        object_label(model, owner[j])
                    ),
                ));
            }
        }
    }

    update_arrange_params(&mut params, cfg);
    update_selected_items_inflation(&mut selected, cfg, &mut params);
    update_unselected_items_inflation(&mut unselected, &params);
    let beds = get_shrink_bedpts(cfg, &params);
    add_exclusion_regions(
        &mut params.excluded_regions,
        cfg,
        1,
        orca_crt::f64_to_i64(scale_(1.)),
    )?;

    // params.progressind is a lambda that does nothing: no callback.
    arrange_on_points(
        &mut selected,
        &unselected,
        &beds,
        &params,
        &mut ArrangeCallbacks::default(),
    )
    .map_err(arrange_failure)?;
    if !unprintable.is_empty() {
        arrange_on_points(
            &mut unprintable,
            &[],
            &beds,
            &params,
            &mut ArrangeCallbacks::default(),
        )
        .map_err(arrange_failure)?;
    }

    for (ap, &owner) in selected.iter().zip(&owner) {
        if ap.bed_idx != 0 {
            return Err(fail_object(
                "arrange-failed",
                format!(
                    "{} does not fit on the plate next to the other objects",
                    object_label(model, owner)
                ),
                owner,
            ));
        }
    }
    for (ap, &(oi, ii)) in selected.iter_mut().zip(&selected_of) {
        apply_arrange_polygon(&mut model.objects[oi], ii, ap);
    }
    for (ap, &(oi, ii)) in unprintable.iter_mut().zip(&unprintable_of) {
        if ap.bed_idx == 0 {
            apply_arrange_polygon(&mut model.objects[oi], ii, ap);
        }
    }
    for mo in &mut model.objects {
        mo.invalidate_bounding_box();
    }
    Ok(())
}

/// `PrintConfig::apply(cfg, true)`'s value of `key`: the configuration's, else Orca's default.
fn print_config_value(cfg: &Config, key: &str) -> OptionValue {
    match cfg.get(key) {
        Some(v) => v.clone(),
        None => create_default(orca_config::get(key).expect("an option of PrintConfig")),
    }
}

/// A list of points as `Vec2d`s.
fn vec2ds(points: &[[f64; 2]]) -> Vec<Vec2d> {
    points.iter().map(|p| Vec2d::new(p[0], p[1])).collect()
}

/// `check_build_volume(model, cfg)`: the build volume check before slicing (OrcaSlicer.cpp:
/// `BuildVolume`, `update_print_volume_state`). A printable instance partly or wholly outside is
/// a failure here, where `Print::apply` would leave it out silently.
///
/// # Errors
///
/// `not-printable` naming the first such instance (exit code -50 when no instance is inside, else
/// -52), and `empty-plate` when no instance is inside.
pub fn check_build_volume(model: &mut Model, cfg: &Config) -> Result<(), Failure> {
    let OptionValue::Points(area) = print_config_value(cfg, "printable_area") else {
        panic!("printable_area is not a list of points");
    };
    let OptionValue::Float(printable_height) = print_config_value(cfg, "printable_height") else {
        panic!("printable_height is not a float");
    };
    let OptionValue::PointsGroups(groups) = print_config_value(cfg, "extruder_printable_area")
    else {
        panic!("extruder_printable_area is not a list of point groups");
    };
    let OptionValue::Floats {
        values: heights, ..
    } = print_config_value(cfg, "extruder_printable_height")
    else {
        panic!("extruder_printable_height is not a list of floats");
    };
    let groups: Vec<Vec<Vec2d>> = groups.iter().map(|g| vec2ds(g)).collect();
    let volume = BuildVolume::new(&vec2ds(&area), printable_height, &groups, &heights);
    let inside = model.update_print_volume_state(&volume);
    let bv = volume.bounding_volume();
    let bed = format!(
        "the printable volume is x {}..{}, y {}..{}, height {} mm",
        mm(bv.min.x()),
        mm(bv.max.x()),
        mm(bv.min.y()),
        mm(bv.max.y()),
        mm(printable_height)
    );
    for (oi, mo) in model.objects.iter().enumerate() {
        if !mo.printable {
            continue;
        }
        for (ii, mi) in mo.instances.iter().enumerate() {
            if !mi.printable || mi.print_volume_state == PrintVolumeState::Inside {
                continue;
            }
            let bb = mo.instance_bounding_box(ii, false);
            let place = if mi.print_volume_state == PrintVolumeState::FullyOutside {
                "outside"
            } else {
                "partly outside"
            };
            let instance = if mo.instances.len() > 1 {
                format!(", instance {ii}")
            } else {
                String::new()
            };
            return Err(Failure {
                code: "not-printable".to_string(),
                message: format!(
                    "{}{instance} is {place} the printable volume: it spans x {}..{}, y {}..{}, z {}..{}; {bed}",
                    object_label(model, oi as i32),
                    mm(bb.min.x()),
                    mm(bb.max.x()),
                    mm(bb.min.y()),
                    mm(bb.max.y()),
                    mm(bb.min.z()),
                    mm(bb.max.z())
                ),
                object_index: oi as i32,
                exit_code: if inside == 0 { -50 } else { -52 },
                missing: Vec::new(),
            });
        }
    }
    if inside == 0 {
        return Err(fail(
            "empty-plate",
            "no object is printable: every object is marked not printable",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_lists_are_in_byte_order_without_repeats() {
        for keys in [OBJECT_KEYS, REGION_KEYS] {
            assert!(keys.windows(2).all(|w| w[0].as_bytes() < w[1].as_bytes()));
        }
        assert_eq!((OBJECT_KEYS.len(), REGION_KEYS.len()), (126, 155));
        // Each is an option Orca defines, but ironing_direction and wall_infill_order:
        // PrintRegionConfig declares them with no definition, and Orca reads those keys as others
        // (handle_legacy: ironing_angle and wall_sequence), so no setting a job gives is ever one
        // of them.
        for key in OBJECT_KEYS.iter().chain(REGION_KEYS) {
            assert_eq!(
                orca_config::get(key).is_some(),
                !["ironing_direction", "wall_infill_order"].contains(key),
                "{key}"
            );
        }
    }

    #[test]
    fn the_towers_least_depth_follows_its_height_as_the_wipe_tower_table_gives_it() {
        assert_eq!(get_limit_depth_by_height(1.), 5.);
        assert_eq!(get_limit_depth_by_height(5.), 5.);
        assert_eq!(get_limit_depth_by_height(100.), 20.);
        assert_eq!(get_limit_depth_by_height(50.), 5. + 45. / 95. * 15.);
        assert_eq!(get_limit_depth_by_height(300.), 40. + 50. / 100. * 20.);
        assert_eq!(get_limit_depth_by_height(350.), 60.);
        assert_eq!(get_limit_depth_by_height(400.), 60.);
        assert_eq!(get_limit_depth_by_height(f32::NAN), 60.);
        assert_eq!(get_auto_brim_by_height(50.), 4.);
        assert_eq!(get_auto_brim_by_height(100.), 8.);
        assert_eq!(get_auto_brim_by_height(250.), 8.);
    }

    #[test]
    fn a_rectangle_in_mm_is_scaled_and_truncated() {
        let r = rect_mm(1.5, -2., 3.25, 4.);
        assert_eq!(
            r.points,
            [
                Point::new(1_500_000, -2_000_000),
                Point::new(3_250_000, -2_000_000),
                Point::new(3_250_000, 4_000_000),
                Point::new(1_500_000, 4_000_000)
            ]
        );
        // A float's scaling divides in floats.
        assert_eq!(scaled_float(1.5), 1_500_000);
    }

    #[test]
    fn the_nearest_point_of_a_region_is_the_point_in_it_or_the_nearest_on_an_edge() {
        let square = [ExPolygon::new(rect_mm(0., 0., 10., 10.))];
        let inside = Point::new(5_000_000, 5_000_000);
        assert_eq!(nearest_point_in(&square, inside), Some(inside));
        assert_eq!(
            nearest_point_in(&square, Point::new(15_000_000, 5_000_000)),
            Some(Point::new(10_000_000, 5_000_000))
        );
        assert_eq!(
            nearest_point_in(&square, Point::new(-3_000_000, -4_000_000)),
            Some(Point::new(0, 0))
        );
        assert_eq!(nearest_point_in(&[], inside), None);
    }

    #[test]
    fn a_tower_off_the_bed_or_on_an_excluded_area_says_why_and_moves_to_the_nearest_place() {
        let printable = rect_mm(0., 0., 200., 200.);
        let excluded = vec![("ExcludedRegion0".to_string(), rect_mm(0., 0., 50., 50.))];
        assert_eq!(
            tower_misfit(&rect_mm(10., 60., 30., 80.), &printable, &excluded, &[]),
            ""
        );
        assert_eq!(
            tower_misfit(&rect_mm(190., 10., 210., 30.), &printable, &excluded, &[]),
            "is not all on the printable area"
        );
        let foot = rect_mm(10., 10., 30., 30.);
        assert_eq!(
            tower_misfit(&foot, &printable, &excluded, &[]),
            "is on an excluded area of the bed (ExcludedRegion0)"
        );
        let (dx, dy) = nearest_tower_move(&foot, &printable, &excluded, &[]).unwrap();
        let mut moved = foot.clone();
        moved.translate(Point::new(scaled(dx), scaled(dy)));
        assert_eq!(tower_misfit(&moved, &printable, &excluded, &[]), "");
        // The shortest way out is 20 mm and a little up or right.
        assert!(
            orca_stl::min(dx, dy) < 1e-3 && orca_stl::max(dx, dy) > 20.,
            "{dx} {dy}"
        );
        // Without a printable area there is no place to move to.
        assert_eq!(
            nearest_tower_move(&foot, &Polygon::default(), &excluded, &[]),
            None
        );
    }

    #[test]
    fn a_height_range_is_put_in_order_and_replaced_in_place() {
        let mut ranges = LayerConfigRanges::new();
        let config = |e: i32| {
            let mut c = ModelConfig::new();
            c.set_key_value("extruder", OptionValue::Int(e));
            c
        };
        set_height_range(&mut ranges, (5., 10.), config(1));
        set_height_range(&mut ranges, (0., 5.), config(2));
        set_height_range(&mut ranges, (5., 8.), config(3));
        set_height_range(&mut ranges, (5., 10.), config(4));
        let order: Vec<((f64, f64), i32)> = ranges
            .iter()
            .map(|(r, c)| (*r, c.opt_int("extruder").unwrap()))
            .collect();
        assert_eq!(order, [((0., 5.), 2), ((5., 8.), 3), ((5., 10.), 4)]);
    }

    #[test]
    fn settings_are_read_as_the_cpp_helpers_read_them() {
        let mut cfg = Config::default();
        cfg.insert("enable_support", OptionValue::Bool(true));
        cfg.insert("raft_layers", OptionValue::Int(3));
        cfg.insert("layer_height", OptionValue::Float(0.2));
        assert_eq!(cfg_bool(&cfg, "enable_support"), Ok(true));
        assert_eq!(cfg_bool(&cfg, "no_such_key"), Ok(false));
        assert_eq!(cfg_enum(&cfg, "raft_layers", 0), Ok(3));
        assert_eq!(cfg_enum(&cfg, "no_such_key", 7), Ok(7));
        assert_eq!(
            cfg_bool(&cfg, "raft_layers").unwrap_err().message,
            "Calling ConfigOption::getBool on a non-boolean ConfigOption"
        );
        assert_eq!(
            cfg_enum(&cfg, "enable_support", 0).unwrap_err().message,
            "Calling ConfigOption::getInt on a non-int ConfigOption"
        );
        assert_eq!(
            cfg_enum(&cfg, "layer_height", 0).unwrap_err().code,
            "internal-error"
        );
        assert!(blank("") && blank(" \t\r\n\x0b\x0c") && !blank(" M0 "));
    }
}
