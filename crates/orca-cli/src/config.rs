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
// Carries OrcaSlicer code: DynamicConfig::optptr, ConfigBase::apply_only, set_deserialize_nothrow
// and set_deserialize_raw, ConfigOptionDef::create_default_option and is_value_valid,
// ConfigOptionVector's set, set_at, resize and set_with_restore_2,
// ConfigOptionBools::deserialize_with_substitutions, and ConfigHelpers::looks_like_enum_value and
// enum_looks_like_true_value are translated from src/libslic3r/Config.hpp and Config.cpp;
// DynamicPrintConfig::normalize_fdm, validate (Slic3r::validate),
// PrintConfigDef::handle_legacy_composite, support_different_extruders, get_index_for_extruder,
// update_values_to_printer_extruders, update_values_to_printer_extruders_for_multiple_filaments,
// get_extruder_variant_string, the sets printer_options_with_variant_1 and
// filament_options_with_variant, and the members of FullPrintConfig from
// src/libslic3r/PrintConfig.cpp and PrintConfig.hpp; make_and_check_thumbnail_list and
// get_error_string from src/libslic3r/GCode/Thumbnails.cpp; all of OrcaSlicer v2.4.2, commit
// 8500fcd, on 2026-11-06. Their copyright is the OrcaSlicer contributors', under the AGPL-3.0.

//! Orca's configuration as the presets stage handles it (`presets.rs`): a `DynamicPrintConfig`
//! ([`Config`], values by key in key order, as its `std::map` keeps them) and what
//! cli/presets.cpp calls on one.
//!
//! - [`Config::create`] is `option(key, true)`: a key Orca defines gets its default
//!   ([`create_default`]: the definition's default, an enum vector's from [`ENUMS_DEFAULTS`]);
//!   [`Config::apply`] is `apply(other, true)`; [`set_value`], [`set_at`], [`resize`] and
//!   [`set_with_restore_2`] are `ConfigOptionVector`'s, with its messages.
//! - [`Config::set_deserialize`] is `set_deserialize_nothrow`: the legacy rename or drop, an
//!   alias, the substitution of an enum, a bool or a list of bools Orca does not know
//!   ([`Rule::Enable`], logged), and Orca's exceptions as [`SetError`].
//! - [`normalize_fdm`], [`validate`] (with every member of `FullPrintConfig`,
//!   [`with_full_print_config`]) and [`handle_legacy_composite`] (the thumbnails list).
//! - The extruder variant picks the flushing volumes read: [`update_values_to_printer_extruders`]
//!   and [`update_values_to_printer_extruders_for_multiple_filaments`].
//!
//! The values follow orca-config's [`OptionValue`]: a definition's default is read back from its
//! serialisation, so a double default holds the six significant digits `serialize` writes
//! (parity/HAZARDS.md).

use std::collections::{BTreeMap, BTreeSet};

use orca_config::value::EnumKeys;
use orca_config::{ConfigError, OptionDef, OptionType, OptionValue};
use orca_fmt::istream::{get_f64, is_space};
use orca_fmt::ostream::{Stream, f64_to_string, to_string_f64};

/// The defaults of Orca's nine `coEnums` options (`ConfigOptionEnumsGeneric{...}` in
/// PrintConfig.cpp). A definition records its default as its serialisation, which an enum vector
/// without its names writes as "" (orca-config), so the values are kept here.
pub const ENUMS_DEFAULTS: &[(&str, i32)] = &[
    ("default_nozzle_volume_type", 0),
    ("extruder_type", 0),
    ("filament_retract_lift_enforce", 0),
    ("filament_z_hop_types", 2),
    ("nozzle_type", 0),
    ("nozzle_volume_type", 0),
    ("overhang_fan_threshold", 5),
    ("retract_lift_enforce", 0),
    ("z_hop_types", 2),
];

/// `filament_options_with_variant` (PrintConfig.cpp), in its order (a `std::set`).
pub const FILAMENT_OPTIONS_WITH_VARIANT: &[&str] = &[
    "activate_air_filtration",
    "activate_air_filtration_during_print",
    "activate_air_filtration_on_completion",
    "complete_print_exhaust_fan_speed",
    "during_print_exhaust_fan_speed",
    "filament_adaptive_volumetric_speed",
    "filament_cooling_before_tower",
    "filament_deretraction_speed",
    "filament_extruder_variant",
    "filament_flow_ratio",
    "filament_flush_temp",
    "filament_flush_volumetric_speed",
    "filament_ironing_flow",
    "filament_ironing_inset",
    "filament_ironing_spacing",
    "filament_ironing_speed",
    "filament_long_retractions_when_cut",
    "filament_max_volumetric_speed",
    "filament_retract_before_wipe",
    "filament_retract_lift_above",
    "filament_retract_lift_below",
    "filament_retract_lift_enforce",
    "filament_retract_restart_extra",
    "filament_retract_when_changing_layer",
    "filament_retraction_distances_when_cut",
    "filament_retraction_length",
    "filament_retraction_minimum_travel",
    "filament_retraction_speed",
    "filament_wipe",
    "filament_wipe_distance",
    "filament_z_hop",
    "filament_z_hop_types",
    "long_retractions_when_ec",
    "nozzle_temperature",
    "nozzle_temperature_initial_layer",
    "retraction_distances_when_ec",
    "volumetric_speed_coefficients",
];

/// `printer_options_with_variant_1` (PrintConfig.cpp), in its order (a `std::set`).
pub const PRINTER_OPTIONS_WITH_VARIANT_1: &[&str] = &[
    "deretraction_speed",
    "long_retractions_when_cut",
    "nozzle_flush_dataset",
    "nozzle_type",
    "nozzle_volume",
    "printer_extruder_id",
    "printer_extruder_variant",
    "retract_before_wipe",
    "retract_length_toolchange",
    "retract_lift_above",
    "retract_lift_below",
    "retract_lift_enforce",
    "retract_restart_extra",
    "retract_restart_extra_toolchange",
    "retract_when_changing_layer",
    "retraction_distances_when_cut",
    "retraction_length",
    "retraction_minimum_travel",
    "retraction_speed",
    "travel_slope",
    "wipe",
    "wipe_distance",
    "z_hop",
    "z_hop_types",
];

/// The members of `FullPrintConfig` (PrintObjectConfig, PrintRegionConfig, MachineEnvelopeConfig,
/// GCodeConfig and PrintConfig in PrintConfig.hpp), by key, in byte order: what
/// `FullPrintConfig::keys()` holds and `validate` checks.
#[rustfmt::skip]
pub const FULL_PRINT_CONFIG_KEYS: &[&str] = &[
    "accel_to_decel_enable", "accel_to_decel_factor", "activate_air_filtration",
    "activate_air_filtration_during_print", "activate_air_filtration_on_completion",
    "activate_chamber_temp_control", "adaptive_bed_mesh_margin", "adaptive_pressure_advance",
    "adaptive_pressure_advance_bridges", "adaptive_pressure_advance_model",
    "adaptive_pressure_advance_overhangs", "additional_cooling_fan_speed",
    "additional_fan_full_speed_layer", "align_infill_direction_to_model", "alternate_extra_wall",
    "auxiliary_fan", "bbl_calib_mark_logo", "bed_custom_model", "bed_custom_texture",
    "bed_exclude_area", "bed_mesh_max", "bed_mesh_min", "bed_mesh_probe_distance",
    "bed_temperature_formula", "before_layer_change_gcode", "best_object_pos",
    "bottom_shell_layers", "bottom_shell_thickness", "bottom_solid_infill_flow_ratio",
    "bottom_surface_density", "bottom_surface_filament_id", "bottom_surface_pattern",
    "bridge_acceleration", "bridge_angle", "bridge_density", "bridge_flow", "bridge_line_width",
    "bridge_no_support", "bridge_speed", "brim_ears_detection_length", "brim_ears_max_angle",
    "brim_flow_ratio", "brim_object_gap", "brim_type", "brim_use_efc_outline", "brim_width",
    "calib_flowrate_topinfill_special_order", "chamber_minimal_temperature", "chamber_temperature",
    "change_extrusion_role_gcode", "change_filament_gcode", "close_additional_fan_first_x_layers",
    "close_fan_the_first_x_layers", "combine_brims", "complete_print_exhaust_fan_speed",
    "cool_plate_temp", "cool_plate_temp_initial_layer", "cooling_tube_length",
    "cooling_tube_retraction", "counterbore_hole_bridging", "curr_bed_type", "default_acceleration",
    "default_filament_colour", "default_jerk", "default_junction_deviation", "deretraction_speed",
    "detect_narrow_internal_solid_infill", "detect_overhang_wall", "detect_thin_wall",
    "disable_m73", "dont_filter_internal_bridges", "dont_slow_down_outer_wall", "draft_shield",
    "during_print_exhaust_fan_speed", "elefant_foot_compensation",
    "elefant_foot_compensation_layers", "elefant_foot_layers_density",
    "emit_machine_limits_to_gcode", "enable_arc_fitting", "enable_extra_bridge_layer",
    "enable_filament_ramming", "enable_long_retraction_when_cut", "enable_overhang_bridge_fan",
    "enable_overhang_speed", "enable_power_loss_recovery", "enable_pressure_advance",
    "enable_prime_tower", "enable_support", "enable_tower_interface_cooldown_during_tower",
    "enable_tower_interface_features", "enable_wrapping_detection", "enforce_support_layers",
    "eng_plate_temp", "eng_plate_temp_initial_layer", "ensure_vertical_shell_thickness",
    "exclude_object", "extra_loading_move", "extra_perimeters_on_overhangs", "extra_solid_infills",
    "extruder_ams_count", "extruder_clearance_height_to_lid", "extruder_clearance_height_to_rod",
    "extruder_clearance_radius", "extruder_colour", "extruder_offset", "extruder_printable_area",
    "extruder_printable_height", "extruder_type",
    "extrusion_rate_smoothing_external_perimeter_only", "fan_cooling_layer_time", "fan_kickstart",
    "fan_max_speed", "fan_min_speed", "fan_speedup_overhangs", "fan_speedup_time",
    "filament_adaptive_volumetric_speed", "filament_adhesiveness_category",
    "filament_change_extrusion_role_gcode", "filament_change_length", "filament_colour",
    "filament_cooling_before_tower", "filament_cooling_final_speed",
    "filament_cooling_initial_speed", "filament_cooling_moves", "filament_cost", "filament_density",
    "filament_diameter", "filament_end_gcode", "filament_extruder_variant", "filament_flow_ratio",
    "filament_flush_temp", "filament_flush_volumetric_speed", "filament_ids",
    "filament_ironing_flow", "filament_ironing_inset", "filament_ironing_spacing",
    "filament_ironing_speed", "filament_is_support", "filament_loading_speed",
    "filament_loading_speed_start", "filament_map", "filament_map_mode",
    "filament_max_volumetric_speed", "filament_minimal_purge_on_wipe_tower",
    "filament_multitool_ramming", "filament_multitool_ramming_flow",
    "filament_multitool_ramming_volume", "filament_notes", "filament_printable",
    "filament_ramming_parameters", "filament_shrink", "filament_shrinkage_compensation_z",
    "filament_soluble", "filament_stamping_distance", "filament_stamping_loading_speed",
    "filament_start_gcode", "filament_toolchange_delay",
    "filament_tower_interface_pre_extrusion_dist", "filament_tower_interface_pre_extrusion_length",
    "filament_tower_interface_print_temp", "filament_tower_interface_purge_volume",
    "filament_tower_ironing_area", "filament_type", "filament_unloading_speed",
    "filament_unloading_speed_start", "filament_vendor", "file_start_gcode", "filename_format",
    "fill_multiline", "filter_out_gap_fill", "first_layer_flow_ratio", "first_layer_print_sequence",
    "first_x_layer_fan_speed", "flush_into_infill", "flush_into_objects", "flush_into_support",
    "flush_multiplier", "flush_volumes_matrix", "flush_volumes_vector", "full_fan_speed_layer",
    "fuzzy_skin", "fuzzy_skin_first_layer", "fuzzy_skin_layers_between_ripple_offset",
    "fuzzy_skin_mode", "fuzzy_skin_noise_type", "fuzzy_skin_octaves", "fuzzy_skin_persistence",
    "fuzzy_skin_point_distance", "fuzzy_skin_ripple_offset", "fuzzy_skin_ripples_per_layer",
    "fuzzy_skin_scale", "fuzzy_skin_thickness", "gap_fill_flow_ratio", "gap_fill_target",
    "gap_infill_speed", "gcode_add_line_number", "gcode_comments", "gcode_flavor",
    "gcode_label_objects", "grab_length", "gyroid_optimized", "has_scarf_joint_seam",
    "head_wrap_detect_zone", "high_current_on_filament_swap", "hole_to_polyhole",
    "hole_to_polyhole_threshold", "hole_to_polyhole_twisted", "hot_plate_temp",
    "hot_plate_temp_initial_layer", "idle_temperature", "independent_support_layer_height",
    "infill_anchor", "infill_anchor_max", "infill_combination",
    "infill_combination_max_layer_height", "infill_direction", "infill_jerk", "infill_lock_depth",
    "infill_overhang_angle", "infill_shift_step", "infill_wall_overlap",
    "initial_layer_acceleration", "initial_layer_infill_speed", "initial_layer_jerk",
    "initial_layer_line_width", "initial_layer_min_bead_width", "initial_layer_print_height",
    "initial_layer_speed", "initial_layer_travel_acceleration", "initial_layer_travel_jerk",
    "initial_layer_travel_speed", "inner_wall_acceleration", "inner_wall_filament_id",
    "inner_wall_flow_ratio", "inner_wall_jerk", "inner_wall_line_width", "inner_wall_speed",
    "input_shaping_damp_x", "input_shaping_damp_y", "input_shaping_emit", "input_shaping_freq_x",
    "input_shaping_freq_y", "input_shaping_type", "interface_shells", "interlocking_beam",
    "interlocking_beam_layer_count", "interlocking_beam_width", "interlocking_boundary_avoidance",
    "interlocking_depth", "interlocking_orientation", "internal_bridge_angle",
    "internal_bridge_density", "internal_bridge_fan_speed", "internal_bridge_flow",
    "internal_bridge_speed", "internal_solid_filament_id", "internal_solid_infill_acceleration",
    "internal_solid_infill_flow_ratio", "internal_solid_infill_line_width",
    "internal_solid_infill_pattern", "internal_solid_infill_speed", "ironing_angle",
    "ironing_angle_fixed", "ironing_fan_speed", "ironing_flow", "ironing_inset", "ironing_pattern",
    "ironing_spacing", "ironing_speed", "ironing_type", "is_infill_first",
    "lateral_lattice_angle_1", "lateral_lattice_angle_2", "layer_change_gcode", "layer_height",
    "lightning_overhang_angle", "lightning_prune_angle", "lightning_straightening_angle",
    "line_width", "long_retractions_when_cut", "long_retractions_when_ec", "machine_end_gcode",
    "machine_load_filament_time", "machine_max_acceleration_e",
    "machine_max_acceleration_extruding", "machine_max_acceleration_retracting",
    "machine_max_acceleration_travel", "machine_max_acceleration_x", "machine_max_acceleration_y",
    "machine_max_acceleration_z", "machine_max_jerk_e", "machine_max_jerk_x", "machine_max_jerk_y",
    "machine_max_jerk_z", "machine_max_junction_deviation", "machine_max_speed_e",
    "machine_max_speed_x", "machine_max_speed_y", "machine_max_speed_z",
    "machine_min_extruding_rate", "machine_min_travel_rate", "machine_pause_gcode",
    "machine_start_gcode", "machine_tool_change_time", "machine_unload_filament_time",
    "make_overhang_printable", "make_overhang_printable_angle", "make_overhang_printable_hole_size",
    "manual_filament_change", "master_extruder_id", "max_bridge_length", "max_layer_height",
    "max_resonance_avoidance_speed", "max_travel_detour_distance",
    "max_volumetric_extrusion_rate_slope", "max_volumetric_extrusion_rate_slope_segment_length",
    "min_bead_width", "min_feature_size", "min_layer_height", "min_length_factor",
    "min_resonance_avoidance_speed", "min_skirt_length", "min_width_top_surface",
    "minimum_sparse_infill_area", "mmu_segmented_region_interlocking_depth",
    "mmu_segmented_region_max_width", "notes", "nozzle_diameter", "nozzle_flush_dataset",
    "nozzle_height", "nozzle_hrc", "nozzle_temperature", "nozzle_temperature_initial_layer",
    "nozzle_temperature_range_high", "nozzle_temperature_range_low", "nozzle_type", "nozzle_volume",
    "nozzle_volume_type", "only_one_wall_first_layer", "only_one_wall_top", "ooze_prevention",
    "other_layers_print_sequence", "other_layers_print_sequence_nums", "outer_wall_acceleration",
    "outer_wall_filament_id", "outer_wall_flow_ratio", "outer_wall_jerk", "outer_wall_line_width",
    "outer_wall_speed", "overhang_1_4_speed", "overhang_2_4_speed", "overhang_3_4_speed",
    "overhang_4_4_speed", "overhang_fan_speed", "overhang_fan_threshold", "overhang_flow_ratio",
    "overhang_reverse", "overhang_reverse_internal_only", "overhang_reverse_threshold",
    "parallel_printheads_bed_exclude_areas", "parallel_printheads_count", "parking_pos_retraction",
    "part_cooling_fan_min_pwm", "physical_extruder_map", "post_process", "precise_outer_wall",
    "precise_z_height", "preferred_orientation", "preheat_steps", "preheat_time",
    "pressure_advance", "prime_tower_brim_width", "prime_tower_enable_framework",
    "prime_tower_flat_ironing", "prime_tower_infill_gap", "prime_tower_skip_points",
    "prime_tower_width", "prime_volume", "print_extruder_id", "print_extruder_variant",
    "print_flow_ratio", "print_order", "print_sequence", "printable_area", "printable_height",
    "printer_extruder_id", "printer_extruder_variant", "printer_model", "printer_notes",
    "printer_structure", "printing_by_object_gcode", "process_change_extrusion_role_gcode",
    "purge_in_prime_tower", "raft_contact_distance", "raft_expansion", "raft_first_layer_density",
    "raft_first_layer_expansion", "raft_layers", "reduce_crossing_wall",
    "reduce_fan_stop_start_freq", "reduce_infill_retraction", "relative_bridge_angle",
    "required_nozzle_HRC", "resolution", "resonance_avoidance", "retract_before_wipe",
    "retract_length_toolchange", "retract_lift_above", "retract_lift_below", "retract_lift_enforce",
    "retract_restart_extra", "retract_restart_extra_toolchange", "retract_when_changing_layer",
    "retraction_distances_when_cut", "retraction_distances_when_ec", "retraction_length",
    "retraction_minimum_travel", "retraction_speed", "role_based_wipe_speed", "scan_first_layer",
    "scarf_angle_threshold", "scarf_joint_flow_ratio", "scarf_joint_speed",
    "scarf_overhang_threshold", "seam_gap", "seam_position", "seam_slope_conditional",
    "seam_slope_entire_loop", "seam_slope_inner_walls", "seam_slope_min_length",
    "seam_slope_start_height", "seam_slope_steps", "seam_slope_type", "set_other_flow_ratios",
    "silent_mode", "single_extruder_multi_material", "single_extruder_multi_material_priming",
    "single_loop_draft_shield", "skeleton_infill_density", "skeleton_infill_line_width",
    "skin_infill_density", "skin_infill_depth", "skin_infill_line_width", "skirt_distance",
    "skirt_height", "skirt_loops", "skirt_speed", "skirt_start_angle", "skirt_type",
    "slice_closing_radius", "slicing_mode", "slow_down_for_layer_cooling", "slow_down_layer_time",
    "slow_down_layers", "slow_down_min_speed", "slowdown_for_curled_perimeters",
    "small_area_infill_flow_compensation", "small_area_infill_flow_compensation_model",
    "small_perimeter_speed", "small_perimeter_threshold", "solid_infill_direction",
    "solid_infill_rotate_template", "sparse_infill_acceleration", "sparse_infill_density",
    "sparse_infill_filament_id", "sparse_infill_flow_ratio", "sparse_infill_line_width",
    "sparse_infill_pattern", "sparse_infill_rotate_template", "sparse_infill_speed",
    "spiral_finishing_flow_ratio", "spiral_mode", "spiral_mode_max_xy_smoothing",
    "spiral_mode_smooth", "spiral_starting_flow_ratio", "staggered_inner_seams",
    "standby_temperature_delta", "start_end_points", "supertack_plate_temp",
    "supertack_plate_temp_initial_layer", "support_air_filtration", "support_angle",
    "support_base_pattern", "support_base_pattern_spacing", "support_bottom_interface_spacing",
    "support_bottom_z_distance", "support_chamber_temp_control", "support_critical_regions_only",
    "support_expansion", "support_filament", "support_flow_ratio",
    "support_interface_bottom_layers", "support_interface_filament", "support_interface_flow_ratio",
    "support_interface_loop_pattern", "support_interface_not_for_body", "support_interface_pattern",
    "support_interface_spacing", "support_interface_speed", "support_interface_top_layers",
    "support_ironing", "support_ironing_flow", "support_ironing_pattern", "support_ironing_spacing",
    "support_line_width", "support_material_interface_fan_speed", "support_multi_bed_types",
    "support_object_first_layer_gap", "support_object_skip_flush", "support_object_xy_distance",
    "support_on_build_plate_only", "support_parallel_printheads", "support_remove_small_overhang",
    "support_speed", "support_style", "support_threshold_angle", "support_threshold_overlap",
    "support_top_z_distance", "support_type", "symmetric_infill_y_axis",
    "temperature_vitrification", "template_custom_gcode", "textured_cool_plate_temp",
    "textured_cool_plate_temp_initial_layer", "textured_plate_temp",
    "textured_plate_temp_initial_layer", "thick_bridges", "thick_internal_bridges", "thumbnails",
    "time_cost", "time_lapse_gcode", "timelapse_type", "tool_change_on_wipe_tower",
    "top_bottom_infill_wall_overlap", "top_shell_layers", "top_shell_thickness",
    "top_solid_infill_flow_ratio", "top_surface_acceleration", "top_surface_density",
    "top_surface_filament_id", "top_surface_jerk", "top_surface_line_width", "top_surface_pattern",
    "top_surface_speed", "travel_acceleration", "travel_jerk", "travel_slope", "travel_speed",
    "travel_speed_z", "tree_support_angle_slow", "tree_support_auto_brim",
    "tree_support_branch_angle", "tree_support_branch_angle_organic",
    "tree_support_branch_diameter", "tree_support_branch_diameter_angle",
    "tree_support_branch_diameter_organic", "tree_support_branch_distance",
    "tree_support_branch_distance_organic", "tree_support_brim_width", "tree_support_tip_diameter",
    "tree_support_top_rate", "tree_support_wall_count", "use_3mf", "use_firmware_retraction",
    "use_relative_e_distances", "volumetric_speed_coefficients", "wall_direction",
    "wall_distribution_count", "wall_generator", "wall_loops", "wall_maximum_deviation",
    "wall_maximum_resolution", "wall_sequence", "wall_transition_angle",
    "wall_transition_filter_deviation", "wall_transition_length", "wipe",
    "wipe_before_external_loop", "wipe_distance", "wipe_on_loops", "wipe_speed",
    "wipe_tower_bridging", "wipe_tower_cone_angle", "wipe_tower_extra_flow",
    "wipe_tower_extra_rib_length", "wipe_tower_extra_spacing", "wipe_tower_filament",
    "wipe_tower_fillet_wall", "wipe_tower_max_purge_speed", "wipe_tower_no_sparse_layers",
    "wipe_tower_rib_width", "wipe_tower_rotation_angle", "wipe_tower_type", "wipe_tower_wall_type",
    "wipe_tower_x", "wipe_tower_y", "wiping_volumes_extruders", "wrapping_detection_gcode",
    "wrapping_detection_layers", "wrapping_exclude_area", "xy_contour_compensation",
    "xy_hole_compensation", "z_hop", "z_hop_types", "z_offset", "zaa_dont_alternate_fill_direction",
    "zaa_enabled", "zaa_min_z", "zaa_minimize_perimeter_height",
];

/// `def->default_value`: the option's default, or `None` when it has none.
#[must_use]
pub fn default_value(def: &OptionDef) -> Option<OptionValue> {
    if def.ty == OptionType::Enums
        && let Some((_, v)) = ENUMS_DEFAULTS.iter().find(|(k, _)| *k == def.key)
    {
        return Some(OptionValue::Enums {
            values: vec![*v],
            keys: def.enum_keys_map,
            nullable: def.nullable,
        });
    }
    def.default_value()
}

/// `create_default_option()`: the default, else an empty value of the option's type.
#[must_use]
pub fn create_default(def: &OptionDef) -> OptionValue {
    default_value(def).unwrap_or_else(|| def.empty_value())
}

/// `print_config_def.get(key)`, else the option that has `key` as an alias (`find_def` in
/// cli/presets.cpp, `set_deserialize_raw`).
#[must_use]
pub fn find_def(key: &str) -> Option<&'static OptionDef> {
    orca_config::get(key).or_else(|| {
        orca_config::options()
            .iter()
            .find(|d| d.aliases.contains(&key))
    })
}

/// `boost::trim` with the classic locale's white space.
#[must_use]
pub fn trim(s: &[u8]) -> &[u8] {
    let start = s.iter().position(|&c| !is_space(c)).unwrap_or(s.len());
    let end = s
        .iter()
        .rposition(|&c| !is_space(c))
        .map_or(start, |i| i + 1);
    &s[start..end]
}

/// The pieces `while (std::getline(is, item, delim))` reads: none for an empty text, and no
/// empty piece after a final delimiter.
#[must_use]
pub fn getline_split(s: &[u8], delim: u8) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < s.len() {
        match s[pos..].iter().position(|&c| c == delim) {
            Some(i) => {
                out.push(&s[pos..pos + i]);
                pos += i + 1;
            }
            None => {
                out.push(&s[pos..]);
                pos = s.len();
            }
        }
    }
    out
}

/// `boost::split(out, s, is_any_of(sep), token_compress_on)`: adjacent separators count as one; a
/// separator at either end leaves an empty piece there, and an empty text is one empty piece.
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

/// `ss << v` for a double: precision 6, `%g`.
fn put(v: f64) -> String {
    f64_to_string(&mut Stream::default(), v).expect("precision 6 always formats")
}

// ---------------------------------------------------------------- vectors

/// One value of a vector option.
#[derive(Clone, Debug, PartialEq)]
enum Elem {
    F(f64),
    I(i32),
    S(String),
    Fp(f64, bool),
    P([f64; 2]),
    B(u8),
    Pg(Vec<[f64; 2]>),
    Ig(Vec<i32>),
}

/// The values of a vector option, or `None` for a single value.
fn elems(v: &OptionValue) -> Option<Vec<Elem>> {
    Some(match v {
        OptionValue::Floats { values, .. } | OptionValue::Percents { values, .. } => {
            values.iter().map(|x| Elem::F(*x)).collect()
        }
        OptionValue::Ints { values, .. } | OptionValue::Enums { values, .. } => {
            values.iter().map(|x| Elem::I(*x)).collect()
        }
        OptionValue::Strings(values) => values.iter().map(|x| Elem::S(x.clone())).collect(),
        OptionValue::FloatsOrPercents { values, .. } => {
            values.iter().map(|(x, p)| Elem::Fp(*x, *p)).collect()
        }
        OptionValue::Points(values) => values.iter().map(|x| Elem::P(*x)).collect(),
        OptionValue::Bools { values, .. } => values.iter().map(|x| Elem::B(*x)).collect(),
        OptionValue::PointsGroups(values) => values.iter().map(|x| Elem::Pg(x.clone())).collect(),
        OptionValue::IntsGroups(values) => values.iter().map(|x| Elem::Ig(x.clone())).collect(),
        _ => return None,
    })
}

/// Replaces the values of a vector option (its kind, nullable or not, and its enum names kept).
/// The values come from an option of the same type.
fn put_elems(v: &mut OptionValue, items: Vec<Elem>) {
    fn f(e: Elem) -> f64 {
        if let Elem::F(x) = e { x } else { 0.0 }
    }
    fn i(e: Elem) -> i32 {
        if let Elem::I(x) = e { x } else { 0 }
    }
    match v {
        OptionValue::Floats { values, .. } | OptionValue::Percents { values, .. } => {
            *values = items.into_iter().map(f).collect();
        }
        OptionValue::Ints { values, .. } | OptionValue::Enums { values, .. } => {
            *values = items.into_iter().map(i).collect();
        }
        OptionValue::Strings(values) => {
            *values = items
                .into_iter()
                .map(|e| if let Elem::S(x) = e { x } else { String::new() })
                .collect();
        }
        OptionValue::FloatsOrPercents { values, .. } => {
            *values = items
                .into_iter()
                .map(|e| {
                    if let Elem::Fp(x, p) = e {
                        (x, p)
                    } else {
                        (0.0, false)
                    }
                })
                .collect();
        }
        OptionValue::Points(values) => {
            *values = items
                .into_iter()
                .map(|e| if let Elem::P(x) = e { x } else { [0.0; 2] })
                .collect();
        }
        OptionValue::Bools { values, .. } => {
            *values = items
                .into_iter()
                .map(|e| if let Elem::B(x) = e { x } else { 0 })
                .collect();
        }
        OptionValue::PointsGroups(values) => {
            *values = items
                .into_iter()
                .map(|e| if let Elem::Pg(x) = e { x } else { Vec::new() })
                .collect();
        }
        OptionValue::IntsGroups(values) => {
            *values = items
                .into_iter()
                .map(|e| if let Elem::Ig(x) = e { x } else { Vec::new() })
                .collect();
        }
        _ => {}
    }
}

/// The value a vector's `std::vector::resize` and `new_values.resize` give a new element: `T()`.
fn zero_elem(ty: OptionType) -> Elem {
    match ty {
        OptionType::Floats | OptionType::Percents => Elem::F(0.0),
        OptionType::Strings => Elem::S(String::new()),
        OptionType::FloatsOrPercents => Elem::Fp(0.0, false),
        OptionType::Points => Elem::P([0.0; 2]),
        OptionType::Bools => Elem::B(0),
        OptionType::PointsGroups => Elem::Pg(Vec::new()),
        OptionType::IntsGroups => Elem::Ig(Vec::new()),
        _ => Elem::I(0),
    }
}

/// The value of a single option as an element of the vector type whose `scalar_type()` it is.
fn scalar_elem(v: &OptionValue) -> Option<Elem> {
    Some(match v {
        OptionValue::Float(x) | OptionValue::Percent(x) => Elem::F(*x),
        OptionValue::Int(x) | OptionValue::Enum { value: x, .. } => Elem::I(*x),
        OptionValue::String(x) => Elem::S(x.clone()),
        OptionValue::FloatOrPercent { value, percent } => Elem::Fp(*value, *percent),
        OptionValue::Point(x) => Elem::P(*x),
        OptionValue::Bool(x) => Elem::B(u8::from(*x)),
        _ => return None,
    })
}

/// `scalar_type() == rhs->type()`: `rhs` is the single type of the vector type `ty`.
fn is_scalar_of(ty: OptionType, rhs: OptionType) -> bool {
    ty.is_vector() && ty.code() - 0x4000 == rhs.code()
}

/// The number of values of a vector option; `None` for a single value.
#[must_use]
pub fn vec_len(v: &OptionValue) -> Option<usize> {
    elems(v).map(|e| e.len())
}

/// `set(rhs)`: the value of an option of the same type.
///
/// # Errors
///
/// "ConfigOptionVector: Assigning an incompatible type" (or `ConfigOptionSingle`'s) for another
/// type.
pub fn set_value(dst: &mut OptionValue, src: &OptionValue) -> Result<(), ConfigError> {
    if dst.ty() != src.ty() {
        return Err(ConfigError(if dst.ty().is_vector() {
            "ConfigOptionVector: Assigning an incompatible type".to_string()
        } else {
            "ConfigOptionSingle: Assigning an incompatible type".to_string()
        }));
    }
    if let Some(items) = elems(src) {
        put_elems(dst, items);
    } else if let (OptionValue::Enum { value, .. }, OptionValue::Enum { value: v, .. }) =
        (&mut *dst, src)
    {
        *value = *v;
    } else {
        *dst = src.clone();
    }
    Ok(())
}

/// `set_at(rhs, i, j)`: value `i` becomes `rhs`'s value `j` (or `rhs` itself, a single value of
/// the vector's kind), the vector first growing to `i + 1` with copies of its first value.
///
/// # Errors
///
/// `ConfigOptionVector::set_at()`'s: from an empty vector, or from another type.
pub fn set_at(
    dst: &mut OptionValue,
    src: &OptionValue,
    i: usize,
    j: usize,
) -> Result<(), ConfigError> {
    let mut items = elems(dst).unwrap_or_default();
    if items.len() <= i {
        // Orca asserts the vector is not empty, and reads its first value.
        let first = items
            .first()
            .cloned()
            .ok_or_else(|| ConfigError("ConfigOptionVector::set_at(): an empty vector".into()))?;
        items.resize(i + 1, first);
    }
    let value = if src.ty() == dst.ty() {
        let other = elems(src).unwrap_or_default();
        let Some(first) = other.first() else {
            return Err(ConfigError(
                "ConfigOptionVector::set_at(): Assigning from an empty vector".into(),
            ));
        };
        other.get(j).unwrap_or(first).clone()
    } else if let (true, Some(e)) = (is_scalar_of(dst.ty(), src.ty()), scalar_elem(src)) {
        e
    } else {
        return Err(ConfigError(
            "ConfigOptionVector::set_at(): Assigning an incompatible type".into(),
        ));
    };
    items[i] = value;
    put_elems(dst, items);
    Ok(())
}

/// `resize(n, opt_default)`: shorter, or longer with copies of the first value; an empty vector
/// grows with `fill`'s first value (a vector of its type) or `fill` (a single value of its kind).
///
/// # Errors
///
/// `ConfigOptionVector::resize()`'s: growing an empty vector with no `fill` or one of another type.
pub fn resize(
    v: &mut OptionValue,
    n: usize,
    fill: Option<&OptionValue>,
) -> Result<(), ConfigError> {
    let mut items = elems(v).unwrap_or_default();
    if n == 0 {
        items.clear();
    } else if n < items.len() {
        items.truncate(n);
    } else if n > items.len() {
        let first = if let Some(first) = items.first() {
            first.clone()
        } else {
            let Some(fill) = fill else {
                return Err(ConfigError(
                    "ConfigOptionVector::resize(): No default value provided.".into(),
                ));
            };
            let incompatible = || {
                ConfigError(
                    "ConfigOptionVector::resize(): Extending with an incompatible type.".into(),
                )
            };
            if fill.ty().is_vector() {
                if fill.ty() != v.ty() {
                    return Err(incompatible());
                }
                // Orca reads the first value of `fill` unchecked; every caller gives a non-empty one.
                elems(fill)
                    .unwrap_or_default()
                    .first()
                    .cloned()
                    .ok_or_else(incompatible)?
            } else {
                if !is_scalar_of(v.ty(), fill.ty()) {
                    return Err(incompatible());
                }
                scalar_elem(fill).ok_or_else(incompatible)?
            }
        };
        items.resize(n, first);
    }
    put_elems(v, items);
    Ok(())
}

/// `set_with_restore_2(rhs, restore_index, start, len, skip_error)`: values `start` to
/// `start + len` (as far as there are any) are replaced by `rhs`'s, `rhs` first sized to
/// `restore.len()` with copies of its first value when `skip_error`; a value of `restore` that is
/// not -1 keeps that old value instead.
///
/// # Errors
///
/// `ConfigOptionVector::set_with_restore_2()`'s: another type, a vector shorter than `start`, or
/// `rhs` of another size without `skip_error`.
pub fn set_with_restore_2(
    dst: &mut OptionValue,
    src: &mut OptionValue,
    restore: &[i32],
    start: usize,
    len: usize,
    skip_error: bool,
) -> Result<(), ConfigError> {
    if src.ty() != dst.ty() {
        return Err(ConfigError(
            "ConfigOptionVector::set_with_restore_2(): Assigning an incompatible type".into(),
        ));
    }
    let mut items = elems(dst).unwrap_or_default();
    let backup = items.clone();
    if items.len() < start {
        return Err(ConfigError(
            "ConfigOptionVector::set_with_restore_2(): invalid size found".into(),
        ));
    }
    let len = orca_stl::min(len, items.len() - start);
    items.drain(start..start + len);
    let mut other = elems(src).unwrap_or_default();
    if other.len() != restore.len() {
        if !skip_error {
            return Err(ConfigError(
                "ConfigOptionVector::set_with_restore_2(): Assigning from an vector with invalid restore_index size".into(),
            ));
        }
        // Orca reads the first value unchecked; the merge refuses an empty list before.
        let first = other
            .first()
            .cloned()
            .unwrap_or_else(|| zero_elem(src.ty()));
        other.resize(restore.len(), first);
        put_elems(src, other.clone());
    }
    for (i, r) in restore.iter().enumerate() {
        // -1 (or any index past the old values) takes rhs's value.
        let value = match usize::try_from(*r) {
            Ok(r) if r < backup.len() => backup[r].clone(),
            _ => other[i].clone(),
        };
        items.insert(start + i, value);
    }
    put_elems(dst, items);
    Ok(())
}

/// `get_at(i)`: value `i`, or the first for an index past the end.
fn get_at(items: &[Elem], i: usize, ty: OptionType) -> Elem {
    items
        .get(i)
        .or_else(|| items.first())
        .cloned()
        .unwrap_or_else(|| zero_elem(ty))
}

// ---------------------------------------------------------------- the configuration

/// `ForwardCompatibilitySubstitutionRule`, the two the presets stage uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    /// `Disable`: a value Orca does not read is refused.
    Disable,
    /// `Enable`: an enum, a bool or a list of bools Orca does not know is substituted, and logged.
    Enable,
}

/// `ConfigSubstitution`: the option, the text it was given and the value it got.
#[derive(Clone, Debug, PartialEq)]
pub struct Substitution {
    pub opt_key: String,
    pub old_value: String,
    pub new_value: OptionValue,
}

/// What `set_deserialize_nothrow` throws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetError {
    /// `UnknownOptionException`: no option has the key or an alias of it.
    Unknown(String),
    /// `ConfigurationError` ("Deserializing nil into a non-nullable object").
    Config(ConfigError),
}

/// `DynamicPrintConfig`: the options it holds, by key.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Config {
    options: BTreeMap<String, OptionValue>,
}

impl Config {
    /// `has(key)`.
    #[must_use]
    pub fn has(&self, key: &str) -> bool {
        self.options.contains_key(key)
    }

    /// `option(key)`.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&OptionValue> {
        self.options.get(key)
    }

    /// `option(key)`, to change it.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut OptionValue> {
        self.options.get_mut(key)
    }

    /// `option(key, true)`: the option, made with its default when the configuration has none;
    /// `None` for a key Orca does not define.
    pub fn create(&mut self, key: &str) -> Option<&mut OptionValue> {
        if !self.options.contains_key(key) {
            let def = orca_config::get(key)?;
            self.options.insert(key.to_string(), create_default(def));
        }
        self.options.get_mut(key)
    }

    /// `set_key_value(key, value)`.
    pub fn insert(&mut self, key: &str, value: OptionValue) {
        self.options.insert(key.to_string(), value);
    }

    /// `erase(key)`.
    pub fn erase(&mut self, key: &str) -> bool {
        self.options.remove(key).is_some()
    }

    /// `keys()`, in key order.
    #[must_use]
    pub fn keys(&self) -> Vec<String> {
        self.options.keys().cloned().collect()
    }

    /// Whether it holds no option.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.options.is_empty()
    }

    /// `apply(other, true)`: each option of `other` set into this one, made first when missing.
    ///
    /// # Errors
    ///
    /// [`set_value`]'s, which options of one definition never give.
    pub fn apply(&mut self, other: &Config) -> Result<(), ConfigError> {
        for (key, value) in &other.options {
            if let Some(my) = self.create(key) {
                set_value(my, value)?;
            }
        }
        Ok(())
    }

    /// `opt_string(key)` of a text option, "" when it has none.
    #[must_use]
    pub fn string(&self, key: &str) -> String {
        match self.get(key) {
            Some(OptionValue::String(s)) => s.clone(),
            _ => String::new(),
        }
    }

    /// `option<ConfigOptionStrings>(key)->values`.
    #[must_use]
    pub fn strings(&self, key: &str) -> Option<&Vec<String>> {
        match self.get(key) {
            Some(OptionValue::Strings(v)) => Some(v),
            _ => None,
        }
    }

    /// `option<ConfigOptionInt>(key)->value`.
    #[must_use]
    pub fn int(&self, key: &str) -> Option<i32> {
        match self.get(key) {
            Some(OptionValue::Int(v)) => Some(*v),
            _ => None,
        }
    }

    /// `option<ConfigOptionFloats>(key)->values` (nullable or not: one type to Orca).
    #[must_use]
    pub fn floats(&self, key: &str) -> Option<&Vec<f64>> {
        match self.get(key) {
            Some(OptionValue::Floats { values, .. }) => Some(values),
            _ => None,
        }
    }

    /// `option<ConfigOptionBools>(key)->values` (nullable or not).
    #[must_use]
    pub fn bools(&self, key: &str) -> Option<&Vec<u8>> {
        match self.get(key) {
            Some(OptionValue::Bools { values, .. }) => Some(values),
            _ => None,
        }
    }

    /// `option<ConfigOptionInts>(key)->values` (nullable or not).
    #[must_use]
    pub fn ints(&self, key: &str) -> Option<&Vec<i32>> {
        match self.get(key) {
            Some(OptionValue::Ints { values, .. }) => Some(values),
            _ => None,
        }
    }

    /// `dynamic_cast<const ConfigOptionInts*>(option(key))`: not a nullable one.
    fn ints_class(&self, key: &str) -> Option<&Vec<i32>> {
        match self.get(key) {
            Some(OptionValue::Ints {
                values,
                nullable: false,
            }) => Some(values),
            _ => None,
        }
    }

    /// `dynamic_cast<const ConfigOptionEnumsGeneric*>(option(key))`: not a nullable one.
    fn enums_class(&self, key: &str) -> Option<&Vec<i32>> {
        match self.get(key) {
            Some(OptionValue::Enums {
                values,
                nullable: false,
                ..
            }) => Some(values),
            _ => None,
        }
    }

    /// `set_deserialize_nothrow(key, value, ctx)`: whether the value was read; with
    /// [`Rule::Enable`], a substitution is logged in `log`.
    ///
    /// # Errors
    ///
    /// [`SetError::Unknown`] for a key no option has, [`SetError::Config`] for "nil" in an option
    /// that takes none.
    pub fn set_deserialize(
        &mut self,
        key: &str,
        value: &str,
        rule: Rule,
        log: &mut Vec<Substitution>,
    ) -> Result<bool, SetError> {
        let (mut key, mut value) = (key.to_string(), value.to_string());
        orca_config::handle_legacy(&mut key, &mut value);
        if key.is_empty() {
            return Ok(true);
        }
        self.set_deserialize_raw(&key, &value, rule, log)
    }

    fn set_deserialize_raw(
        &mut self,
        key: &str,
        value: &str,
        rule: Rule,
        log: &mut Vec<Substitution>,
    ) -> Result<bool, SetError> {
        let Some(def) = find_def(key) else {
            return Err(SetError::Unknown(key.to_string()));
        };
        if !def.shortcut.is_empty() {
            for shortcut in def.shortcut {
                if !self.set_deserialize_raw(shortcut, value, rule, log)? {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
        let opt = self
            .create(def.key)
            .expect("an option Orca defines is made");
        let (success, substituted) = if def.ty == OptionType::Bools && rule != Rule::Disable {
            let defaults_to_true = matches!(
                default_value(def),
                Some(OptionValue::Bools { values, .. }) if values.len() == 1 && values[0] == 1
            );
            let OptionValue::Bools { values, nullable } = opt else {
                unreachable!("a coBools option holds bools");
            };
            match bools_with_substitutions(values, *nullable, value, defaults_to_true)
                .map_err(SetError::Config)?
            {
                BoolsRead::Loaded => (true, false),
                BoolsRead::Substituted => (true, true),
                BoolsRead::Failed => (false, false),
            }
        } else {
            let read = opt.deserialize(value).map_err(SetError::Config)?;
            if !read
                && rule != Rule::Disable
                && matches!(
                    def.ty,
                    OptionType::Enum | OptionType::Enums | OptionType::Bool
                )
            {
                if def.ty == OptionType::Bool {
                    *opt = OptionValue::Bool(enum_looks_like_true_value(value.as_bytes()));
                } else {
                    let default = create_default(def);
                    set_value(opt, &default).map_err(SetError::Config)?;
                }
                (true, true)
            } else {
                (read, false)
            }
        };
        if substituted && rule == Rule::Enable {
            log.push(Substitution {
                opt_key: def.opt_key.to_string(),
                old_value: value.to_string(),
                new_value: opt.clone(),
            });
        }
        Ok(success)
    }
}

/// `ConfigHelpers::DeserializationResult`.
enum BoolsRead {
    Loaded,
    Substituted,
    Failed,
}

/// `ConfigOptionBoolsTempl::deserialize_with_substitutions`: 1, 0, nil, or a word that looks
/// like an enum value, read as true when it says enabled or on (or the default is true).
fn bools_with_substitutions(
    values: &mut Vec<u8>,
    nullable: bool,
    text: &str,
    defaults_to_true: bool,
) -> Result<BoolsRead, ConfigError> {
    values.clear();
    let mut substituted = false;
    for item in getline_split(text.as_bytes(), b',') {
        let item = trim(item);
        let v = if item == b"nil" {
            if !nullable {
                return Err(ConfigError(
                    "Deserializing nil into a non-nullable object".into(),
                ));
            }
            u8::MAX
        } else if item == b"1" {
            1
        } else if item == b"0" {
            0
        } else if looks_like_enum_value(item) {
            substituted = true;
            u8::from(enum_looks_like_true_value(item) || defaults_to_true)
        } else {
            return Ok(BoolsRead::Failed);
        };
        values.push(v);
    }
    Ok(if substituted {
        BoolsRead::Substituted
    } else {
        BoolsRead::Loaded
    })
}

/// `ConfigHelpers::looks_like_enum_value`: a word of at most 64 ASCII letters, digits, `_` and
/// `-`, starting with a letter.
fn looks_like_enum_value(value: &[u8]) -> bool {
    let v = trim(value);
    !v.is_empty()
        && v.len() <= 64
        && v[0].is_ascii_alphabetic()
        && v.iter()
            .all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-')
}

/// `ConfigHelpers::enum_looks_like_true_value`: "enabled" or "on", in any letter case.
fn enum_looks_like_true_value(value: &[u8]) -> bool {
    let v = trim(value);
    v.eq_ignore_ascii_case(b"enabled") || v.eq_ignore_ascii_case(b"on")
}

// ---------------------------------------------------------------- normalize, validate

fn int_or_zero(c: &Config, key: &str) -> i32 {
    c.int(key).unwrap_or(0)
}

fn set_int(c: &mut Config, key: &str, v: i32) {
    if let Some(OptionValue::Int(x)) = c.create(key) {
        *x = v;
    }
}

/// `DynamicPrintConfig::normalize_fdm()` (no filament count): the old `extruder` setting spread
/// over the filament ids it stands for, the solid infill's filaments filled in from each other,
/// spiral vase mode's walls and infill, and a resolution of at least 0.001 mm.
pub fn normalize_fdm(c: &mut Config) {
    if c.has("extruder") {
        let extruder = int_or_zero(c, "extruder");
        c.erase("extruder");
        if extruder != 0 {
            for key in [
                "sparse_infill_filament_id",
                "outer_wall_filament_id",
                "inner_wall_filament_id",
                "internal_solid_filament_id",
                "top_surface_filament_id",
                "bottom_surface_filament_id",
            ] {
                if !c.has(key) || int_or_zero(c, key) == 0 {
                    set_int(c, key, extruder);
                }
            }
        }
    }
    if c.has("sparse_infill_filament_id") {
        let sparse = int_or_zero(c, "sparse_infill_filament_id");
        if sparse > 0
            && (!c.has("internal_solid_filament_id")
                || int_or_zero(c, "internal_solid_filament_id") == 0)
        {
            set_int(c, "internal_solid_filament_id", sparse);
        }
    }
    let internal_solid = int_or_zero(c, "internal_solid_filament_id");
    let top_surface = int_or_zero(c, "top_surface_filament_id");
    let bottom_surface = int_or_zero(c, "bottom_surface_filament_id");
    if internal_solid == 0 && top_surface > 0 {
        set_int(c, "internal_solid_filament_id", top_surface);
    }
    if internal_solid == 0 && bottom_surface > 0 {
        set_int(c, "internal_solid_filament_id", bottom_surface);
    }
    if top_surface == 0 && internal_solid > 0 {
        set_int(c, "top_surface_filament_id", internal_solid);
    }
    if bottom_surface == 0 && internal_solid > 0 {
        set_int(c, "bottom_surface_filament_id", internal_solid);
    }
    if matches!(c.get("spiral_mode"), Some(OptionValue::Bool(true))) {
        for key in [
            "retract_when_changing_layer",
            "filament_retract_when_changing_layer",
        ] {
            if let Some(OptionValue::Bools { values, .. }) = c.create(key) {
                values.iter_mut().for_each(|v| *v = 0);
            }
        }
        set_int(c, "wall_loops", 1);
        if let Some(OptionValue::Bool(b)) = c.create("alternate_extra_wall") {
            *b = false;
        }
        set_int(c, "top_shell_layers", 0);
        if let Some(OptionValue::Percent(p)) = c.create("sparse_infill_density") {
            *p = 0.0;
        }
    }
    if let Some(OptionValue::Float(r)) = c.get_mut("resolution") {
        *r = orca_stl::max(*r, 0.001);
    }
}

/// `c` with every member of `FullPrintConfig` it does not hold at its default: what
/// `FullPrintConfig fpc; fpc.apply(c)` holds, and `c` after cli/presets.cpp's round trip through
/// one (`fff.apply(cfg); cfg.apply(fff)`).
#[must_use]
pub fn with_full_print_config(c: &Config) -> Config {
    let mut out = c.clone();
    for key in FULL_PRINT_CONFIG_KEYS {
        if !out.has(key)
            && let Some(def) = orca_config::get(key)
        {
            out.insert(key, create_default(def));
        }
    }
    out
}

/// `ConfigOptionDef::is_value_valid(value)` (4 digits of precision).
fn is_value_valid(def: &OptionDef, value: f64) -> bool {
    if value.is_nan() {
        return def.nullable;
    }
    if def.min == 0.0 && value < 0.0 {
        return false;
    }
    let ep = orca_crt::pow_int(0.1, 4);
    let (min, max) = (f64::from(def.min), f64::from(def.max));
    if orca_crt::fabs(value - min) < ep || orca_crt::fabs(value - max) < ep {
        return true;
    }
    min <= value && value <= max
}

/// `SCALING_FACTOR` as the command line leaves it.
const SCALING_FACTOR: f64 = 0.000_001;

/// `MAX_LINE_WIDTH_MULTIPLIER` (a float).
const MAX_LINE_WIDTH_MULTIPLIER: f32 = 5.0;

fn float_of(c: &Config, key: &str) -> f64 {
    match c.get(key) {
        Some(OptionValue::Float(v) | OptionValue::Percent(v)) => *v,
        _ => 0.0,
    }
}

fn bool_of(c: &Config, key: &str) -> bool {
    matches!(c.get(key), Some(OptionValue::Bool(true)))
}

fn serialize_of(c: &Config, key: &str) -> Result<String, ConfigError> {
    c.get(key)
        .map_or_else(|| Ok(String::new()), OptionValue::serialize)
}

/// `DynamicPrintConfig::validate(true)` of an FFF configuration (`Slic3r::validate` of a
/// `FullPrintConfig` that has `c` applied): what Orca refuses, by key, each with its first
/// message.
///
/// # Errors
///
/// A `serialize` that throws (a NaN in a list that takes no nil): the run's internal error.
#[allow(clippy::too_many_lines)]
pub fn validate(c: &Config) -> Result<BTreeMap<String, String>, ConfigError> {
    let full = with_full_print_config(c);
    let f = &full;
    let mut errors: BTreeMap<String, String> = BTreeMap::new();
    let mut emplace = |key: &str, message: String| {
        errors.entry(key.to_string()).or_insert(message);
    };
    let invalid = "invalid value ";
    let layer_height = float_of(f, "layer_height");
    if layer_height <= 0.0 || orca_crt::fabs(orca_crt::fmod(layer_height, SCALING_FACTOR)) > 1e-4 {
        emplace(
            "layer_height",
            format!("{invalid}{}", to_string_f64(layer_height)),
        );
    }
    let first_layer = float_of(f, "initial_layer_print_height");
    if first_layer <= 0.0 {
        emplace(
            "initial_layer_print_height",
            format!("{invalid}{}", to_string_f64(first_layer)),
        );
    }
    if f.floats("filament_diameter")
        .is_some_and(|v| v.iter().any(|d| *d < 1.0))
    {
        emplace(
            "filament_diameter",
            format!("{invalid}{}", serialize_of(f, "filament_diameter")?),
        );
    }
    if f.floats("nozzle_diameter")
        .is_some_and(|v| v.iter().any(|d| *d < 0.005))
    {
        emplace(
            "nozzle_diameter",
            format!("{invalid}{}", serialize_of(f, "nozzle_diameter")?),
        );
    }
    for key in ["wall_loops", "top_shell_layers", "bottom_shell_layers"] {
        let v = int_or_zero(f, key);
        if v < 0 {
            emplace(key, format!("{invalid}{v}"));
        }
    }
    let firmware_retraction = bool_of(f, "use_firmware_retraction");
    let flavor = match f.get("gcode_flavor") {
        Some(OptionValue::Enum { value, .. }) => *value,
        _ => 0,
    };
    let flavor_def = orca_config::get("gcode_flavor");
    let named = |names: &[&str]| -> bool {
        flavor_def
            .and_then(|d| d.enum_keys_map)
            .is_some_and(|keys| keys.iter().any(|(n, v)| *v == flavor && names.contains(n)))
    };
    if firmware_retraction
        && !named(&[
            "klipper",
            "smoothie",
            "reprap",
            "reprapfirmware",
            "marlin",
            "marlin2",
            "machinekit",
            "repetier",
        ])
    {
        emplace(
            "use_firmware_retraction",
            "--use-firmware-retraction is only supported by Klipper, Marlin, Smoothie, RepRapFirmware, Repetier and Machinekit firmware".to_string(),
        );
    }
    if firmware_retraction && f.bools("wipe").is_some_and(|w| w.iter().any(|v| *v != 0)) {
        emplace(
            "use_firmware_retraction",
            "--use-firmware-retraction is not compatible with --wipe".to_string(),
        );
    }
    for key in [
        "gcode_flavor",
        "sparse_infill_pattern",
        "top_surface_pattern",
        "bottom_surface_pattern",
        "internal_solid_infill_pattern",
    ] {
        let text = serialize_of(f, key)?;
        let known = orca_config::get(key).is_some_and(|d| d.enum_values.contains(&text.as_str()));
        if !known {
            emplace(key, format!("{invalid}{text}"));
        }
    }
    let skirt_height = int_or_zero(f, "skirt_height");
    if skirt_height < 0 {
        emplace("skirt_height", format!("{invalid}{skirt_height}"));
    }
    for key in [
        "bridge_flow",
        "internal_bridge_flow",
        "extruder_clearance_radius",
        "extruder_clearance_height_to_rod",
        "extruder_clearance_height_to_lid",
        "nozzle_height",
    ] {
        let v = float_of(f, key);
        if v <= 0.0 {
            emplace(key, format!("{invalid}{}", to_string_f64(v)));
        }
    }
    if f.floats("filament_flow_ratio")
        .is_some_and(|v| v.iter().any(|r| *r <= 0.0))
    {
        emplace(
            "filament_flow_ratio",
            format!("{invalid}{}", serialize_of(f, "filament_flow_ratio")?),
        );
    }
    if bool_of(f, "spiral_mode") {
        let spiral = "Invalid value when spiral vase mode is enabled: ";
        let wall_loops = int_or_zero(f, "wall_loops");
        if wall_loops != 1 {
            emplace("wall_loops", format!("{spiral}{wall_loops}"));
        }
        let density = float_of(f, "sparse_infill_density");
        if density > 0.0 {
            emplace(
                "sparse_infill_density",
                format!("{spiral}{}", to_string_f64(density)),
            );
        }
        let top = int_or_zero(f, "top_shell_layers");
        if top > 0 {
            emplace("top_shell_layers", format!("{spiral}{top}"));
        }
        if bool_of(f, "enable_support") {
            emplace("enable_support", format!("{spiral}1"));
        }
        let enforce = int_or_zero(f, "enforce_support_layers");
        if enforce > 0 {
            emplace("enforce_support_layers", format!("{spiral}{enforce}"));
        }
    }
    {
        let mut max_nozzle = 0.0_f64;
        let mut min_nozzle = f64::MAX;
        for d in f.floats("nozzle_diameter").into_iter().flatten() {
            max_nozzle = orca_stl::max(max_nozzle, *d);
            min_nozzle = orca_stl::min(min_nozzle, *d);
        }
        for key in [
            "outer_wall_line_width",
            "inner_wall_line_width",
            "sparse_infill_line_width",
            "internal_solid_infill_line_width",
            "bridge_line_width",
            "top_surface_line_width",
            "support_line_width",
            "initial_layer_line_width",
            "skin_infill_line_width",
            "skeleton_infill_line_width",
        ] {
            let width = match f.get(key) {
                Some(OptionValue::FloatOrPercent { value, percent }) => {
                    if *percent {
                        max_nozzle * *value / 100.0
                    } else {
                        *value
                    }
                }
                _ => 0.0,
            };
            let bridge = key == "bridge_line_width";
            let allowed = if bridge {
                min_nozzle
            } else {
                f64::from(MAX_LINE_WIDTH_MULTIPLIER) * max_nozzle
            };
            if width > allowed {
                emplace(
                    key,
                    if bridge {
                        format!(
                            "Bridge line width must not exceed nozzle diameter: {}",
                            to_string_f64(width)
                        )
                    } else {
                        format!("too large line width {}", to_string_f64(width))
                    },
                );
            }
        }
    }
    for key in FULL_PRINT_CONFIG_KEYS {
        let (Some(opt), Some(def)) = (f.get(key), orca_config::get(key)) else {
            continue;
        };
        let out_of_range = match opt {
            OptionValue::Float(v)
            | OptionValue::Percent(v)
            | OptionValue::FloatOrPercent { value: v, .. } => !is_value_valid(def, *v),
            OptionValue::Floats { values, .. } | OptionValue::Percents { values, .. } => {
                values.iter().any(|v| !is_value_valid(def, *v))
            }
            OptionValue::Int(v) => !is_value_valid(def, f64::from(*v)),
            OptionValue::Ints { values, .. } => {
                values.iter().any(|v| !is_value_valid(def, f64::from(*v)))
            }
            _ => false,
        };
        if out_of_range && !errors.contains_key(*key) {
            let text = opt.serialize()?;
            errors.insert(
                (*key).to_string(),
                format!(
                    "{text} not in range [{},{}]",
                    to_string_f64(f64::from(def.min)),
                    to_string_f64(f64::from(def.max))
                ),
            );
        }
    }
    Ok(errors)
}

// ---------------------------------------------------------------- thumbnails

const THUMBNAIL_INVALID_VALUE: u8 = 1;
const THUMBNAIL_OUT_OF_RANGE: u8 = 2;
const THUMBNAIL_INVALID_EXT: u8 = 4;

/// `std::getline(iss, out, delim)` from `pos`: the piece and where the stream is after it, or
/// `None` when nothing is left to read (the stream fails).
fn getline_at(s: &[u8], pos: usize, delim: u8) -> Option<(&[u8], usize)> {
    if pos >= s.len() {
        return None;
    }
    Some(match s[pos..].iter().position(|&c| c == delim) {
        Some(i) => (&s[pos..pos + i], pos + i + 1),
        None => (&s[pos..], s.len()),
    })
}

/// `GCodeThumbnails::make_and_check_thumbnail_list(text, def_ext)`: each `XxY/EXT` as (format,
/// x, y), and the errors found (a bit each).
fn thumbnail_list(text: &str, def_ext: &str, keys: EnumKeys) -> (Vec<(i32, f64, f64)>, u8) {
    let mut list = Vec::new();
    let mut errors = 0u8;
    for piece in getline_split(text.as_bytes(), b',') {
        if let Some((x_text, after_x)) = getline_at(piece, 0, b'x')
            && !x_text.is_empty()
        {
            let x = get_f64(x_text).or(0.0);
            if let Some((y_text, after_y)) = getline_at(piece, after_x, b'/')
                && !y_text.is_empty()
            {
                let y = get_f64(y_text).or(0.0);
                if 0.0 < x && x < 1000.0 && 0.0 < y && y < 1000.0 {
                    let ext_text = getline_at(piece, after_y, b'/').map_or(&b""[..], |(e, _)| e);
                    let ext = if ext_text.is_empty() {
                        if def_ext.is_empty() { "PNG" } else { def_ext }
                            .as_bytes()
                            .to_ascii_uppercase()
                    } else {
                        ext_text.to_ascii_uppercase()
                    };
                    let format = keys
                        .iter()
                        .find(|(n, _)| n.as_bytes() == ext.as_slice())
                        .map(|(_, v)| *v);
                    if format.is_none() {
                        errors |= THUMBNAIL_INVALID_EXT;
                    }
                    list.push((format.unwrap_or(0), x, y));
                } else {
                    errors |= THUMBNAIL_OUT_OF_RANGE;
                }
                continue;
            }
        }
        errors |= THUMBNAIL_INVALID_VALUE;
    }
    (list, errors)
}

/// `PrintConfigDef::handle_legacy_composite(config)`: a preset's thumbnails list checked and
/// written as Orca writes it (`48x48/PNG, 300x300/PNG`).
///
/// # Errors
///
/// The `BadOptionValueException`'s message for a list with a bad size, a size out of range or an
/// extension Orca does not know; Orca does not catch it, so the run fails with an internal error.
pub fn handle_legacy_composite(c: &mut Config) -> Result<(), String> {
    if !c.has("thumbnails") {
        return Ok(());
    }
    let format_def = orca_config::get("thumbnails_format");
    let keys = format_def.and_then(|d| d.enum_keys_map).unwrap_or(&[]);
    let mut ext = String::new();
    if let (Some(def), Some(OptionValue::Enum { value, .. })) =
        (format_def, c.get("thumbnails_format"))
    {
        // `enum_values.at(value)`: MSVC's out_of_range says "invalid vector subscript".
        ext = usize::try_from(*value)
            .ok()
            .and_then(|i| def.enum_values.get(i))
            .ok_or_else(|| "invalid vector subscript".to_string())?
            .to_string();
    }
    let text = c.string("thumbnails");
    let (list, errors) = thumbnail_list(&text, &ext, keys);
    if errors != 0 {
        let mut message = format!("\nInvalid value provided for parameter thumbnails: {text}");
        if errors & THUMBNAIL_INVALID_VALUE != 0 {
            message.push_str("\n - Invalid input format. Expected vector of dimensions in the following format: \"XxY/EXT, XxY/EXT, ...\"");
        }
        if errors & THUMBNAIL_OUT_OF_RANGE != 0 {
            message.push_str("\n - Input value is out of range");
        }
        if errors & THUMBNAIL_INVALID_EXT != 0 {
            message.push_str("\n - Some extension in the input is invalid");
        }
        return Err(message);
    }
    if !list.is_empty() {
        let mut out = String::new();
        for (format, x, y) in list {
            let name = keys
                .iter()
                .rev()
                .find(|(_, v)| *v == format)
                .map_or("", |(n, _)| *n);
            out.push_str(&format!("{}x{}/{name}, ", put(x), put(y)));
        }
        out.truncate(out.len() - 2);
        c.insert("thumbnails", OptionValue::String(out));
    }
    Ok(())
}

// ---------------------------------------------------------------- extruder variants

/// `get_extruder_variant_string(extruder_type, nozzle_volume_type)`: "Direct Drive Standard"
/// and the like; "" for a type past the last.
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

/// `support_different_extruders(extruder_count)`: the count of nozzles, and whether the
/// extruder variants they list are more than one.
fn support_different_extruders(c: &Config) -> (usize, bool) {
    let mut variants: BTreeSet<String> = BTreeSet::new();
    let mut count = 0;
    if let Some(OptionValue::Floats {
        values: nozzles,
        nullable: false,
    }) = c.get("nozzle_diameter")
    {
        count = nozzles.len();
        if let Some(list) = c.strings("extruder_variant_list") {
            for index in 0..count {
                let variant = list
                    .get(index)
                    .or_else(|| list.first())
                    .cloned()
                    .unwrap_or_default();
                variants.extend(split_compress(&variant, ','));
            }
        }
    }
    (count, variants.len() > 1)
}

/// `get_index_for_extruder(id, id_name, extruder_type, nozzle_volume_type, variant_name)`
/// (stride 1): the index of the variant `variant_name` lists for that extruder type and nozzle
/// volume type and for extruder (or filament) `id`; -1 when there is none.
fn get_index_for_extruder(
    c: &Config,
    id: i32,
    id_name: &str,
    extruder_type: i32,
    nozzle_volume_type: i32,
    variant_name: &str,
) -> i32 {
    let Some(variants) = c.strings(variant_name) else {
        return -1;
    };
    let ids = if id_name.is_empty() {
        None
    } else {
        c.ints_class(id_name)
    };
    let list = c.strings("extruder_variant_list");
    let generated_extruder_id = |target: usize| -> i32 {
        let Some(list) = list else {
            return 0;
        };
        let mut variant_index = 0;
        for (extruder_index, entry) in list.iter().enumerate() {
            for variant in split_compress(entry, ',') {
                if trim(variant.as_bytes()).is_empty() {
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

/// The vector types the variant picks rewrite (the switch of both update functions).
fn picks_type(ty: OptionType) -> bool {
    matches!(
        ty,
        OptionType::Strings
            | OptionType::Ints
            | OptionType::Floats
            | OptionType::Percents
            | OptionType::FloatsOrPercents
            | OptionType::Bools
            | OptionType::Enums
    )
}

/// The value of an enum vector `get_at(i)` reads, `i` taken as `size_t` (a negative index is past
/// the end).
fn enum_at(values: &[i32], i: i64) -> i32 {
    usize::try_from(i)
        .ok()
        .and_then(|i| values.get(i))
        .or_else(|| values.first())
        .copied()
        .unwrap_or(0)
}

/// `update_values_to_printer_extruders(c, printer_options_with_variant_1, "printer_extruder_id",
/// "printer_extruder_variant")` on a configuration with every printer setting: each nozzle's
/// value of each setting the printer keeps per extruder variant, picked by the nozzle's extruder
/// type and nozzle volume type.
pub fn update_values_to_printer_extruders(c: &mut Config) {
    let (count, different) = support_different_extruders(c);
    if count <= 1 && !different {
        return;
    }
    let (Some(types), Some(volumes)) = (
        c.enums_class("extruder_type").cloned(),
        c.enums_class("nozzle_volume_type").cloned(),
    ) else {
        return;
    };
    let mut variant_index = Vec::with_capacity(count);
    for e in 0..count {
        let e64 = i64::try_from(e).unwrap_or(i64::MAX);
        let index = get_index_for_extruder(
            c,
            i32::try_from(e + 1).unwrap_or(i32::MAX),
            "printer_extruder_id",
            enum_at(&types, e64),
            enum_at(&volumes, e64),
            "printer_extruder_variant",
        );
        variant_index.push(orca_stl::max(index, 0));
    }
    for key in PRINTER_OPTIONS_WITH_VARIANT_1 {
        let Some(def) = orca_config::get(key) else {
            continue;
        };
        if !picks_type(def.ty) {
            continue;
        }
        // Orca reads the option unchecked; every key of the set is a member of FullPrintConfig.
        let Some(opt) = c.get_mut(key) else {
            continue;
        };
        if opt.ty() != def.ty {
            continue;
        }
        let items = elems(opt).unwrap_or_default();
        let picked: Vec<Elem> = variant_index
            .iter()
            .map(|v| get_at(&items, usize::try_from(*v).unwrap_or(0), def.ty))
            .collect();
        put_elems(opt, picked);
    }
}

/// `update_values_to_printer_extruders_for_multiple_filaments(c, filament_options_with_variant,
/// "filament_self_index", "filament_extruder_variant")`: each filament's value of each setting a
/// filament keeps per extruder variant, picked by the nozzle `filament_map` gives it; a variant
/// it does not have leaves `T()`.
pub fn update_values_to_printer_extruders_for_multiple_filaments(c: &mut Config) {
    let (count, different) = support_different_extruders(c);
    if count <= 1 && !different {
        return;
    }
    let Some(map) = c.ints("filament_map").cloned() else {
        return;
    };
    let (Some(types), Some(volumes)) = (
        c.enums_class("extruder_type").cloned(),
        c.enums_class("nozzle_volume_type").cloned(),
    ) else {
        return;
    };
    let ids = c.ints_class("filament_self_index").cloned();
    let mut variant_index = vec![-1_i32; map.len()];
    for (f, slot) in variant_index.iter_mut().enumerate() {
        let nozzle = i64::from(map[f]) - 1;
        let id = i32::try_from(f + 1).unwrap_or(i32::MAX);
        let mut index = get_index_for_extruder(
            c,
            id,
            "filament_self_index",
            enum_at(&types, nozzle),
            enum_at(&volumes, nozzle),
            "filament_extruder_variant",
        );
        if index < 0 {
            index = 0;
            if let Some(ids) = &ids
                && let Some(i) = ids.iter().position(|v| *v == id)
            {
                index = i32::try_from(i).unwrap_or(0);
            }
        }
        *slot = index;
    }
    for key in FILAMENT_OPTIONS_WITH_VARIANT {
        let Some(def) = orca_config::get(key) else {
            continue;
        };
        if !picks_type(def.ty) {
            continue;
        }
        let Some(opt) = c.get_mut(key) else {
            continue;
        };
        if opt.ty() != def.ty {
            continue;
        }
        let items = elems(opt).unwrap_or_default();
        let picked: Vec<Elem> = variant_index
            .iter()
            .map(|v| match usize::try_from(*v) {
                Ok(v) if v < items.len() => items[v].clone(),
                _ => zero_elem(def.ty),
            })
            .collect();
        put_elems(opt, picked);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floats(v: &[f64]) -> OptionValue {
        OptionValue::Floats {
            values: v.to_vec(),
            nullable: false,
        }
    }

    #[test]
    fn the_full_print_config_keys_are_sorted_and_defined() {
        assert_eq!(FULL_PRINT_CONFIG_KEYS.len(), 609);
        for w in FULL_PRINT_CONFIG_KEYS.windows(2) {
            assert!(w[0].as_bytes() < w[1].as_bytes(), "{} {}", w[0], w[1]);
        }
        for key in FULL_PRINT_CONFIG_KEYS
            .iter()
            .chain(FILAMENT_OPTIONS_WITH_VARIANT)
            .chain(PRINTER_OPTIONS_WITH_VARIANT_1)
        {
            assert!(orca_config::has(key), "{key}");
        }
        for key in PRINTER_OPTIONS_WITH_VARIANT_1 {
            assert!(FULL_PRINT_CONFIG_KEYS.contains(key), "{key}");
        }
        let enums: Vec<&str> = orca_config::options()
            .iter()
            .filter(|d| d.ty == OptionType::Enums)
            .map(|d| d.key)
            .collect();
        let named: Vec<&str> = ENUMS_DEFAULTS.iter().map(|(k, _)| *k).collect();
        assert_eq!(enums, named);
    }

    #[test]
    fn vectors_grow_shrink_and_take_values_as_orca_does() {
        let mut v = floats(&[1.0, 2.0]);
        set_at(&mut v, &floats(&[7.0, 8.0]), 4, 1).unwrap();
        assert_eq!(v, floats(&[1.0, 2.0, 1.0, 1.0, 8.0]));
        set_at(&mut v, &OptionValue::Float(3.0), 0, 0).unwrap();
        assert_eq!(v, floats(&[3.0, 2.0, 1.0, 1.0, 8.0]));
        assert_eq!(
            set_at(&mut v, &floats(&[]), 0, 0).unwrap_err().0,
            "ConfigOptionVector::set_at(): Assigning from an empty vector"
        );
        assert_eq!(
            set_at(&mut v, &OptionValue::Int(1), 0, 0).unwrap_err().0,
            "ConfigOptionVector::set_at(): Assigning an incompatible type"
        );
        resize(&mut v, 2, None).unwrap();
        assert_eq!(v, floats(&[3.0, 2.0]));
        resize(&mut v, 4, None).unwrap();
        assert_eq!(v, floats(&[3.0, 2.0, 3.0, 3.0]));
        let mut empty = floats(&[]);
        assert_eq!(
            resize(&mut empty, 2, None).unwrap_err().0,
            "ConfigOptionVector::resize(): No default value provided."
        );
        resize(&mut empty, 2, Some(&OptionValue::Float(5.0))).unwrap();
        assert_eq!(empty, floats(&[5.0, 5.0]));
        let mut strings = OptionValue::Strings(Vec::new());
        assert_eq!(
            resize(&mut strings, 1, Some(&OptionValue::Float(5.0)))
                .unwrap_err()
                .0,
            "ConfigOptionVector::resize(): Extending with an incompatible type."
        );
    }

    #[test]
    fn set_with_restore_2_replaces_a_block_of_variants() {
        let mut dst = floats(&[1.0, 2.0, 3.0, 4.0]);
        let mut src = floats(&[9.0]);
        set_with_restore_2(&mut dst, &mut src, &[-1, -1], 1, 2, true).unwrap();
        assert_eq!(dst, floats(&[1.0, 9.0, 9.0, 4.0]));
        assert_eq!(src, floats(&[9.0, 9.0]));
        let mut dst = floats(&[1.0]);
        set_with_restore_2(&mut dst, &mut floats(&[5.0]), &[-1], 1, 1, true).unwrap();
        assert_eq!(dst, floats(&[1.0, 5.0]));
        assert_eq!(
            set_with_restore_2(&mut floats(&[]), &mut floats(&[5.0]), &[-1], 1, 1, true)
                .unwrap_err()
                .0,
            "ConfigOptionVector::set_with_restore_2(): invalid size found"
        );
    }

    #[test]
    fn values_are_read_with_orcas_substitutions() {
        let mut c = Config::default();
        let mut log = Vec::new();
        // An enum value Orca does not know takes the default, logged.
        assert_eq!(
            c.set_deserialize("support_type", "sideways", Rule::Enable, &mut log),
            Ok(true)
        );
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].opt_key, "support_type");
        assert_eq!(log[0].old_value, "sideways");
        // A bool list with a word reads it as true only for enabled or on.
        assert_eq!(
            c.set_deserialize("wipe", "on,off", Rule::Enable, &mut log),
            Ok(true)
        );
        assert_eq!(c.bools("wipe"), Some(&vec![1, 0]));
        assert_eq!(log.len(), 2);
        // Without substitutions the same text is refused.
        assert_eq!(
            c.set_deserialize("wipe", "on", Rule::Disable, &mut log),
            Ok(false)
        );
        // A key Orca does not have is cleared by handle_legacy and skipped, as Orca skips it;
        // past handle_legacy, a key with no definition is unknown.
        assert_eq!(
            c.set_deserialize("no_such_key_at_all", "1", Rule::Enable, &mut log),
            Ok(true)
        );
        assert!(!c.has("no_such_key_at_all"));
        assert_eq!(
            c.set_deserialize_raw("no_such_key_at_all", "1", Rule::Enable, &mut log),
            Err(SetError::Unknown("no_such_key_at_all".to_string()))
        );
        assert_eq!(
            c.set_deserialize("wipe", "nil", Rule::Enable, &mut log),
            Err(SetError::Config(ConfigError(
                "Deserializing nil into a non-nullable object".to_string()
            )))
        );
        // A coEnums option is made with its real default.
        let mut d = Config::default();
        assert_eq!(
            d.create("z_hop_types").cloned().and_then(|v| vec_len(&v)),
            Some(1)
        );
    }

    #[test]
    fn spiral_vase_mode_is_normalized_and_validated() {
        let mut c = Config::default();
        c.insert("spiral_mode", OptionValue::Bool(true));
        c.insert("extruder", OptionValue::Int(2));
        normalize_fdm(&mut c);
        assert!(!c.has("extruder"));
        assert_eq!(c.int("sparse_infill_filament_id"), Some(2));
        assert_eq!(c.int("wall_loops"), Some(1));
        assert_eq!(c.int("top_shell_layers"), Some(0));
        assert!(validate(&c).unwrap().is_empty());
        c.insert("enable_support", OptionValue::Bool(true));
        c.insert("layer_height", OptionValue::Float(-0.1));
        let errors = validate(&c).unwrap();
        assert_eq!(
            errors.get("enable_support").map(String::as_str),
            Some("Invalid value when spiral vase mode is enabled: 1")
        );
        assert_eq!(
            errors.get("layer_height").map(String::as_str),
            Some("invalid value -0.100000")
        );
    }

    #[test]
    fn thumbnails_are_checked_and_rewritten() {
        let mut c = Config::default();
        c.insert(
            "thumbnails",
            OptionValue::String("48x48,300x300/jpg".into()),
        );
        handle_legacy_composite(&mut c).unwrap();
        assert_eq!(c.string("thumbnails"), "48x48/PNG, 300x300/JPG");
        c.insert(
            "thumbnails",
            OptionValue::String("0x5/PNG,abc,7x7/gif".into()),
        );
        assert_eq!(
            handle_legacy_composite(&mut c).unwrap_err(),
            concat!(
                "\nInvalid value provided for parameter thumbnails: 0x5/PNG,abc,7x7/gif",
                "\n - Invalid input format. Expected vector of dimensions in the following format: \"XxY/EXT, XxY/EXT, ...\"",
                "\n - Input value is out of range",
                "\n - Some extension in the input is invalid"
            )
        );
    }

    #[test]
    fn boost_split_with_compressed_separators() {
        assert_eq!(split_compress("", ','), [""]);
        assert_eq!(split_compress("a,,b", ','), ["a", "b"]);
        assert_eq!(split_compress(",a,", ','), ["", "a", ""]);
    }
}
