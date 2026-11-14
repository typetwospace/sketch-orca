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
// Carries OrcaSlicer code: src/libslic3r/Print.cpp lines 93-421 (Print::
// invalidate_state_by_config_options) and 428-435 (Print::invalidate_step), and
// src/libslic3r/PrintObject.cpp lines 1091-1509 (PrintObject::invalidate_state_by_config_options,
// invalidate_step and invalidate_all_steps), of OrcaSlicer v2.4.2, translated on 2026-11-14
// (orca-o3-print-1). Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab), the
// PrusaSlicer authors (Prusa Research) and the Slic3r authors (Alessandro Ranellucci), under the
// GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! Which steps a changed option invalidates, and how an invalidated step reaches the steps that
//! depend on it: `Print::invalidate_state_by_config_options`, `Print::invalidate_step`,
//! `PrintObject::invalidate_state_by_config_options`, `PrintObject::invalidate_step` and
//! `PrintObject::invalidate_all_steps`.
//!
//! The C++'s `if ... else if` chains over the option keys are tables here, in the source's order:
//! [`PRINT_STEPS_GCODE`] (the `steps_gcode` set, its two repeated keys kept), [`PRINT_BRANCHES`],
//! [`OBJECT_BRANCHES`] and [`OBJECT_STEP_PROPAGATION`]. A key takes the first branch that names
//! it, as the chain does, so a key named twice (`initial_layer_line_width`, `support_type`,
//! `outer_wall_speed` and others) acts only through its first branch. The branches whose bodies
//! read the configs are [`ObjectAction`]s with the body translated in
//! [`print_object_invalidate_state_by_config_options`]. `tests/invalidation.rs` reads the C++
//! source again and holds the tables to its lists of keys and steps.
//!
//! `Print`'s and `PrintObject`'s own `invalidate_step` hide their bases' (the C++'s
//! `Inherited::invalidate_step`), which are the methods of orca-layer's
//! `PrintBaseWithState` and `PrintObjectBaseWithState`; the bases' `invalidate_steps` and
//! `invalidate_all_steps` are not hidden, so the C++'s `this->invalidate_steps(...)` and
//! `m_print->invalidate_all_steps()` call the bases' and do not propagate. The print a print object
//! reaches through `m_print` is a [`PrintInherited`] argument (the print's `base`).

use orca_config::option::is_approx;
use orca_config::print_config::BrimType;
use orca_config::{
    ConfigBase, ConfigEnum, ConfigOptionEnum, ConfigOptionFloat, ConfigOptionPercent, OptionValue,
};
use orca_index::stop::not_ported;
use orca_layer::print::Print;
use orca_layer::print_object::{PrintObject, PrintObjectStep, PrintStep};

use crate::print::{PrintInherited, sort_remove_duplicates};

/// Print.cpp lines 102-244: `steps_gcode`, the print options that influence the G-code generator
/// only, or are notes without influence on the generated G-code, in the source's order
/// (`use_firmware_retraction` and `filament_notes` appear twice there, and here).
pub const PRINT_STEPS_GCODE: &[&str] = &[
    //BBS
    "additional_cooling_fan_speed",
    "reduce_crossing_wall",
    "max_travel_detour_distance",
    "printable_area",
    //BBS: add bed_exclude_area
    "bed_exclude_area",
    "thumbnail_size",
    "before_layer_change_gcode",
    "enable_pressure_advance",
    "pressure_advance",
    "enable_overhang_bridge_fan",
    "overhang_fan_speed",
    "overhang_fan_threshold",
    "slow_down_for_layer_cooling",
    "default_acceleration",
    "deretraction_speed",
    "close_fan_the_first_x_layers",
    "machine_end_gcode",
    "printing_by_object_gcode",
    "filament_end_gcode",
    "post_process",
    "extruder_clearance_height_to_rod",
    "extruder_clearance_height_to_lid",
    "extruder_clearance_radius",
    "nozzle_height",
    "extruder_colour",
    "extruder_offset",
    "filament_flow_ratio",
    "reduce_fan_stop_start_freq",
    "dont_slow_down_outer_wall",
    "fan_cooling_layer_time",
    "full_fan_speed_layer",
    "fan_kickstart",
    "part_cooling_fan_min_pwm",
    "fan_speedup_overhangs",
    "fan_speedup_time",
    "filament_colour",
    "default_filament_colour",
    "filament_diameter",
    "volumetric_speed_coefficients",
    "filament_density",
    "filament_cost",
    "filament_notes",
    "outer_wall_acceleration",
    "inner_wall_acceleration",
    "initial_layer_acceleration",
    "top_surface_acceleration",
    "bridge_acceleration",
    "travel_acceleration",
    "sparse_infill_acceleration",
    "internal_solid_infill_acceleration",
    // BBS
    "supertack_plate_temp_initial_layer",
    "cool_plate_temp_initial_layer",
    "textured_cool_plate_temp_initial_layer",
    "eng_plate_temp_initial_layer",
    "hot_plate_temp_initial_layer",
    "textured_plate_temp_initial_layer",
    "gcode_add_line_number",
    "layer_change_gcode",
    "time_lapse_gcode",
    "wrapping_detection_gcode",
    "fan_min_speed",
    "fan_max_speed",
    "printable_height",
    "slow_down_min_speed",
    "max_volumetric_extrusion_rate_slope",
    "max_volumetric_extrusion_rate_slope_segment_length",
    "extrusion_rate_smoothing_external_perimeter_only",
    "reduce_infill_retraction",
    "filename_format",
    "retraction_minimum_travel",
    "retract_before_wipe",
    "retract_when_changing_layer",
    "retraction_length",
    "retract_length_toolchange",
    "z_hop",
    "travel_slope",
    "retract_lift_above",
    "retract_lift_below",
    "retract_lift_enforce",
    "retract_restart_extra",
    "retract_restart_extra_toolchange",
    "retraction_speed",
    "use_firmware_retraction",
    "slow_down_layer_time",
    "standby_temperature_delta",
    "preheat_time",
    "preheat_steps",
    "machine_start_gcode",
    "filament_start_gcode",
    "change_filament_gcode",
    "wipe",
    // BBS
    "wipe_distance",
    "curr_bed_type",
    "nozzle_volume",
    "nozzle_hrc",
    "required_nozzle_HRC",
    "upward_compatible_machine",
    "is_infill_first",
    // Orca
    "chamber_temperature",
    "chamber_minimal_temperature",
    "thumbnails",
    "thumbnails_format",
    "seam_gap",
    "role_based_wipe_speed",
    "wipe_speed",
    "use_relative_e_distances",
    "accel_to_decel_enable",
    "accel_to_decel_factor",
    "wipe_on_loops",
    "gcode_comments",
    "gcode_label_objects",
    "exclude_object",
    "support_material_interface_fan_speed",
    "internal_bridge_fan_speed", // ORCA: Add support for separate internal bridge fan speed control
    "ironing_fan_speed",
    "single_extruder_multi_material_priming",
    "activate_air_filtration",
    "activate_air_filtration_during_print",
    "activate_air_filtration_on_completion",
    "during_print_exhaust_fan_speed",
    "complete_print_exhaust_fan_speed",
    "activate_chamber_temp_control",
    "manual_filament_change",
    "disable_m73",
    "use_firmware_retraction",
    "enable_long_retraction_when_cut",
    "long_retractions_when_cut",
    "retraction_distances_when_cut",
    "filament_long_retractions_when_cut",
    "filament_retraction_distances_when_cut",
    "grab_length",
    "bed_temperature_formula",
    "filament_notes",
    "process_notes",
    "printer_notes",
    "use_3mf",
];

/// Print.cpp line 246: `steps_ignore`, the options with no influence on the G-code whatsoever.
/// The C++ leaves it empty.
pub const PRINT_STEPS_IGNORE: &[&str] = &[];

/// One branch of `Print::invalidate_state_by_config_options`'s chain after the two sets: the keys
/// its condition names, in order, and the print steps and object steps its body adds, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrintBranch {
    /// The keys the condition compares `opt_key` with.
    pub keys: &'static [&'static str],
    /// The print steps the body adds to `steps`.
    pub steps: &'static [PrintStep],
    /// The object steps the body adds to `osteps`.
    pub osteps: &'static [PrintObjectStep],
}

/// Print.cpp lines 259-403: the branches of `Print::invalidate_state_by_config_options` after
/// `steps_gcode` and `steps_ignore`, in order. A key none of them names invalidates all the
/// print's steps (lines 404-409).
pub const PRINT_BRANCHES: &[PrintBranch] = &[
    PrintBranch {
        keys: &[
            "skirt_type",
            "skirt_loops",
            "skirt_speed",
            "skirt_height",
            "min_skirt_length",
            "single_loop_draft_shield",
            "draft_shield",
            "skirt_distance",
            "skirt_start_angle",
            "ooze_prevention",
            "wipe_tower_x",
            "wipe_tower_y",
            "wipe_tower_rotation_angle",
        ],
        steps: &[PrintStep::SkirtBrim],
        osteps: &[],
    },
    PrintBranch {
        keys: &[
            "initial_layer_print_height",
            "nozzle_diameter",
            "filament_shrink",
            "filament_shrinkage_compensation_z",
            "resolution",
            "precise_z_height",
            // Spiral Vase forces different kind of slicing than the normal model:
            // In Spiral Vase mode, holes are closed and only the largest area contour is kept at each layer.
            // Therefore toggling the Spiral Vase on / off requires complete reslicing.
            "spiral_mode",
        ],
        steps: &[],
        osteps: &[PrintObjectStep::Slice],
    },
    PrintBranch {
        keys: &[
            "print_sequence",
            "filament_type",
            "chamber_temperature",
            "nozzle_temperature_initial_layer",
            "filament_minimal_purge_on_wipe_tower",
            "filament_max_volumetric_speed",
            "filament_adaptive_volumetric_speed",
            "filament_loading_speed",
            "filament_loading_speed_start",
            "filament_unloading_speed",
            "filament_unloading_speed_start",
            "filament_toolchange_delay",
            "filament_cooling_moves",
            "filament_stamping_loading_speed",
            "filament_stamping_distance",
            "filament_cooling_initial_speed",
            "filament_cooling_final_speed",
            "filament_ramming_parameters",
            "filament_multitool_ramming",
            "filament_multitool_ramming_volume",
            "filament_multitool_ramming_flow",
            "filament_max_volumetric_speed",
            "gcode_flavor",
            "single_extruder_multi_material",
            "nozzle_temperature",
            // BBS
            "supertack_plate_temp",
            "cool_plate_temp",
            "textured_cool_plate_temp",
            "eng_plate_temp",
            "hot_plate_temp",
            "textured_plate_temp",
            "enable_prime_tower",
            "enable_wrapping_detection",
            "prime_tower_enable_framework",
            "prime_tower_width",
            "prime_tower_brim_width",
            "wipe_tower_type",
            "prime_tower_skip_points",
            "prime_tower_flat_ironing",
            "enable_tower_interface_features",
            "first_layer_print_sequence",
            "other_layers_print_sequence",
            "other_layers_print_sequence_nums",
            "extruder_ams_count",
            "filament_map_mode",
            "filament_map",
            "filament_adhesiveness_category",
            "filament_tower_interface_pre_extrusion_dist",
            "filament_tower_interface_pre_extrusion_length",
            "filament_tower_ironing_area",
            "filament_tower_interface_purge_volume",
            "filament_tower_interface_print_temp",
            "wipe_tower_bridging",
            "wipe_tower_extra_flow",
            "wipe_tower_no_sparse_layers",
            "flush_volumes_matrix",
            "prime_volume",
            "flush_into_infill",
            "flush_into_support",
            "initial_layer_infill_speed",
            "travel_speed",
            "travel_speed_z",
            "initial_layer_speed",
            "initial_layer_travel_speed",
            "initial_layer_travel_acceleration",
            "initial_layer_travel_jerk",
            "slow_down_layers",
            "idle_temperature",
            "wipe_tower_cone_angle",
            "wipe_tower_extra_spacing",
            "wipe_tower_max_purge_speed",
            "wipe_tower_wall_type",
            "wipe_tower_extra_rib_length",
            "wipe_tower_rib_width",
            "wipe_tower_fillet_wall",
            "wipe_tower_filament",
            "wiping_volumes_extruders",
            "enable_filament_ramming",
            "tool_change_on_wipe_tower",
            "purge_in_prime_tower",
            "z_offset",
            "support_multi_bed_types",
        ],
        steps: &[PrintStep::WipeTower, PrintStep::SkirtBrim],
        osteps: &[],
    },
    PrintBranch {
        keys: &[
            "filament_soluble",
            "filament_is_support",
            "filament_printable",
            "filament_change_length",
            "independent_support_layer_height",
        ],
        steps: &[PrintStep::WipeTower],
        // Soluble support interface / non-soluble base interface produces non-soluble interface layers below soluble interface layers.
        // Thus switching between soluble / non-soluble interface layer material may require recalculation of supports.
        //FIXME Killing supports on any change of "filament_soluble" is rough. We should check for each object whether that is necessary.
        osteps: &[
            PrintObjectStep::SupportMaterial,
            PrintObjectStep::SimplifySupportPath,
        ],
    },
    PrintBranch {
        keys: &[
            "initial_layer_line_width",
            "min_layer_height",
            "max_layer_height",
            //|| opt_key == "resolution"
            //BBS: when enable arc fitting, we must re-generate perimeter
            "enable_arc_fitting",
            "print_order",
            "wall_sequence",
        ],
        steps: &[PrintStep::SkirtBrim],
        osteps: &[
            PrintObjectStep::Perimeters,
            PrintObjectStep::EstimateCurledExtrusions,
            PrintObjectStep::Infill,
            PrintObjectStep::SupportMaterial,
            PrintObjectStep::SimplifyPath,
            PrintObjectStep::SimplifyInfill,
            PrintObjectStep::SimplifySupportPath,
        ],
    },
    PrintBranch {
        keys: &["z_hop_types"],
        steps: &[],
        osteps: &[PrintObjectStep::DetectOverhangsForLift],
    },
];

/// What a branch of `PrintObject::invalidate_state_by_config_options` does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectAction {
    /// Adds these steps, in order.
    Steps(&'static [PrintObjectStep]),
    /// The brim's branch (lines 1117-1126): `posSupportMaterial` (brim is printed below
    /// supports, support invalidates brim and skirt), and for `brim_type`, `posPerimeters` when the
    /// old or the new brim type is `btOuterOnly`.
    Brim,
    /// The gap fill's branch (lines 1149-1167): `posSlice` under a condition that is never true
    /// (`opt_key` would have to be two keys at once), then `posPerimeters`.
    GapFill,
    /// `enable_support` (lines 1207-1214): `posSupportMaterial`, and `posSlice` when the object's
    /// `support_top_z_distance` is 0.
    EnableSupport,
    /// `sparse_infill_density` (lines 1345-1354): `posPerimeters` when the old or the new density
    /// is about 0 or 100 per cent, then `posPrepareInfill`.
    SparseInfillDensity,
    /// `bridge_flow` and `internal_bridge_flow` (lines 1383-1389): `posPerimeters`, `posInfill`
    /// and `posSupportMaterial` when the object's `support_top_z_distance` is above 0.
    BridgeFlow,
    /// Invalidates the print's `psGCodeExport` at once (line 1450).
    GCodeExport,
    /// Invalidates the print's `psWipeTower`, then its `psGCodeExport`, at once (lines 1455-1456).
    FlushInto,
}

/// One branch of `PrintObject::invalidate_state_by_config_options`'s chain: the keys its
/// condition names, in order, and what its body does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectBranch {
    /// The keys the condition compares `opt_key` with.
    pub keys: &'static [&'static str],
    /// What the body does.
    pub action: ObjectAction,
}

/// PrintObject.cpp lines 1102-1456: the branches of
/// `PrintObject::invalidate_state_by_config_options`, in order. A key none of them names
/// invalidates all the object's steps and the print's (lines 1457-1461).
pub const OBJECT_BRANCHES: &[ObjectBranch] = &[
    ObjectBranch {
        keys: &[
            "brim_width",
            "brim_object_gap",
            "brim_use_efc_outline",
            "brim_type",
            "brim_ears_max_angle",
            "brim_ears_detection_length",
            // BBS: brim generation depends on printing speed
            "outer_wall_speed",
            "small_perimeter_speed",
            "small_perimeter_threshold",
            "sparse_infill_speed",
            "inner_wall_speed",
            "support_speed",
            "internal_solid_infill_speed",
            "top_surface_speed",
        ],
        action: ObjectAction::Brim,
    },
    ObjectBranch {
        keys: &[
            "wall_loops",
            "alternate_extra_wall",
            "top_one_wall_type",
            "min_width_top_surface",
            "only_one_wall_first_layer",
            "extra_perimeters_on_overhangs",
            "detect_overhang_wall",
            "initial_layer_line_width",
            "inner_wall_line_width",
            "infill_wall_overlap",
            "top_bottom_infill_wall_overlap",
            "seam_gap",
            "role_based_wipe_speed",
            "wipe_on_loops",
            "wipe_speed",
        ],
        action: ObjectAction::Steps(&[PrintObjectStep::Perimeters]),
    },
    ObjectBranch {
        keys: &["small_area_infill_flow_compensation_model"],
        action: ObjectAction::Steps(&[PrintObjectStep::Slice]),
    },
    ObjectBranch {
        keys: &["gap_infill_speed", "filter_out_gap_fill"],
        action: ObjectAction::GapFill,
    },
    ObjectBranch {
        keys: &[
            "layer_height",
            "mmu_segmented_region_max_width",
            "mmu_segmented_region_interlocking_depth",
            "raft_layers",
            "raft_contact_distance",
            "slice_closing_radius",
            "slicing_mode",
            "slowdown_for_curled_perimeters",
            "make_overhang_printable",
            "make_overhang_printable_angle",
            "make_overhang_printable_hole_size",
            "interlocking_beam",
            "interlocking_orientation",
            "interlocking_beam_layer_count",
            "interlocking_depth",
            "interlocking_boundary_avoidance",
            "interlocking_beam_width",
        ],
        action: ObjectAction::Steps(&[PrintObjectStep::Slice]),
    },
    ObjectBranch {
        keys: &[
            "elefant_foot_compensation",
            "elefant_foot_compensation_layers",
            "elefant_foot_layers_density",
            "support_top_z_distance",
            "support_bottom_z_distance",
            "xy_hole_compensation",
            "xy_contour_compensation",
            //BBS: [Arthur] the following params affect bottomBridge surface type detection
            "support_type",
            "bridge_no_support",
            "max_bridge_length",
            "support_interface_top_layers",
            "support_critical_regions_only",
            "hole_to_polyhole",
            "hole_to_polyhole_threshold",
            "hole_to_polyhole_twisted",
        ],
        action: ObjectAction::Steps(&[PrintObjectStep::Slice]),
    },
    ObjectBranch {
        keys: &["enable_support"],
        action: ObjectAction::EnableSupport,
    },
    ObjectBranch {
        keys: &[
            "support_type",
            "support_angle",
            "support_on_build_plate_only",
            "support_critical_regions_only",
            "support_remove_small_overhang",
            "enforce_support_layers",
            "support_filament",
            "support_line_width",
            "support_interface_top_layers",
            "support_interface_bottom_layers",
            "support_interface_pattern",
            "support_interface_loop_pattern",
            "support_interface_filament",
            "support_interface_not_for_body",
            "support_interface_spacing",
            "support_bottom_interface_spacing", //BBS
            "support_base_pattern",
            "support_style",
            "support_object_xy_distance",
            "support_object_first_layer_gap",
            "support_base_pattern_spacing",
            "support_expansion",
            "independent_support_layer_height", // Orca
            "support_threshold_angle",
            "support_threshold_overlap",
            "support_ironing",
            "support_ironing_pattern",
            "support_ironing_flow",
            "support_ironing_spacing",
            "raft_expansion",
            "raft_first_layer_density",
            "raft_first_layer_expansion",
            "bridge_no_support",
            "max_bridge_length",
            "initial_layer_line_width",
            "tree_support_auto_brim",
            "tree_support_brim_width",
            "tree_support_top_rate",
            "tree_support_branch_distance",
            "tree_support_branch_distance_organic",
            "tree_support_tip_diameter",
            "tree_support_branch_diameter",
            "tree_support_branch_diameter_organic",
            "tree_support_branch_diameter_angle",
            "tree_support_branch_angle",
            "tree_support_branch_angle_organic",
            "tree_support_angle_slow",
            "tree_support_wall_count",
        ],
        action: ObjectAction::Steps(&[PrintObjectStep::SupportMaterial]),
    },
    ObjectBranch {
        // The body's `#if (0)` block (lines 1270-1286) is not compiled.
        keys: &["bottom_shell_layers", "top_shell_layers"],
        action: ObjectAction::Steps(&[PrintObjectStep::Slice]),
    },
    ObjectBranch {
        keys: &[
            "interface_shells",
            "infill_multiline",
            "infill_combination",
            "infill_combination_max_layer_height",
            "bottom_shell_thickness",
            "top_shell_thickness",
            "minimum_sparse_infill_area",
            "sparse_infill_filament_id",
            "internal_solid_filament_id",
            "top_surface_filament_id",
            "bottom_surface_filament_id",
            "sparse_infill_line_width",
            "skin_infill_line_width",
            "skeleton_infill_line_width",
            "infill_direction",
            "solid_infill_direction",
            "align_infill_direction_to_model",
            "extra_solid_infills",
            "ensure_vertical_shell_thickness",
            "bridge_angle",
            "internal_bridge_angle", // ORCA: Internal bridge angle override
            "relative_bridge_angle", // ORCA: Relative bridge angle
            //BBS
            "bridge_line_width",
            "bridge_density",
            "internal_bridge_density",
        ],
        action: ObjectAction::Steps(&[PrintObjectStep::PrepareInfill]),
    },
    ObjectBranch {
        keys: &[
            "top_surface_pattern",
            "bottom_surface_pattern",
            "internal_solid_infill_pattern",
            "external_fill_link_max_length",
            "infill_anchor",
            "infill_anchor_max",
            "top_surface_line_width",
            "top_surface_density",
            "bottom_surface_density",
            "initial_layer_line_width",
            "small_area_infill_flow_compensation",
            "lateral_lattice_angle_1",
            "lateral_lattice_angle_2",
            "infill_overhang_angle",
        ],
        action: ObjectAction::Steps(&[PrintObjectStep::Infill]),
    },
    ObjectBranch {
        keys: &[
            "sparse_infill_pattern",
            "symmetric_infill_y_axis",
            "infill_shift_step",
            "sparse_infill_rotate_template",
            "solid_infill_rotate_template",
            "lightning_overhang_angle",
            "lightning_prune_angle",
            "lightning_straightening_angle",
            "skeleton_infill_density",
            "skin_infill_density",
            "infill_lock_depth",
            "skin_infill_depth",
        ],
        action: ObjectAction::Steps(&[PrintObjectStep::PrepareInfill]),
    },
    ObjectBranch {
        keys: &["sparse_infill_density"],
        action: ObjectAction::SparseInfillDensity,
    },
    ObjectBranch {
        // This value is used for calculating perimeter - infill overlap, thus perimeters need to be recalculated.
        keys: &["internal_solid_infill_line_width"],
        action: ObjectAction::Steps(&[PrintObjectStep::Perimeters, PrintObjectStep::PrepareInfill]),
    },
    ObjectBranch {
        keys: &[
            "outer_wall_line_width",
            "outer_wall_filament_id",
            "inner_wall_filament_id",
            "fuzzy_skin",
            "fuzzy_skin_thickness",
            "fuzzy_skin_point_distance",
            "fuzzy_skin_first_layer",
            "fuzzy_skin_mode",
            "fuzzy_skin_noise_type",
            "fuzzy_skin_scale",
            "fuzzy_skin_octaves",
            "fuzzy_skin_persistence",
            "detect_overhang_wall",
            "overhang_reverse",
            "overhang_reverse_internal_only",
            "overhang_reverse_threshold",
            "wall_direction",
            "enable_overhang_speed",
            "detect_thin_wall",
            "precise_outer_wall",
        ],
        action: ObjectAction::Steps(&[
            PrintObjectStep::Perimeters,
            PrintObjectStep::SupportMaterial,
        ]),
    },
    ObjectBranch {
        keys: &["bridge_flow", "internal_bridge_flow"],
        action: ObjectAction::BridgeFlow,
    },
    ObjectBranch {
        keys: &[
            "wall_generator",
            "wall_transition_length",
            "wall_transition_filter_deviation",
            "wall_transition_angle",
            "wall_distribution_count",
            "wall_maximum_resolution",
            "wall_maximum_deviation",
            "min_feature_size",
            "min_length_factor",
            "min_bead_width",
        ],
        action: ObjectAction::Steps(&[PrintObjectStep::Slice]),
    },
    ObjectBranch {
        keys: &[
            "seam_position",
            "seam_slope_type",
            "seam_slope_conditional",
            "scarf_angle_threshold",
            "scarf_overhang_threshold",
            "scarf_joint_speed",
            "seam_slope_start_height",
            "seam_slope_entire_loop",
            "seam_slope_min_length",
            "seam_slope_steps",
            "seam_slope_inner_walls",
            "support_speed",
            "support_interface_speed",
            "overhang_1_4_speed",
            "overhang_2_4_speed",
            "overhang_3_4_speed",
            "overhang_4_4_speed",
            "bridge_speed",
            "internal_bridge_speed",
            "outer_wall_speed",
            "small_perimeter_speed",
            "small_perimeter_threshold",
            "sparse_infill_speed",
            "inner_wall_speed",
            "internal_solid_infill_speed",
            "top_surface_speed",
            "bed_mesh_min",
            "bed_mesh_max",
            "adaptive_bed_mesh_margin",
            "bed_mesh_probe_distance",
            "print_flow_ratio",
            "first_layer_flow_ratio",
            "top_solid_infill_flow_ratio",
            "bottom_solid_infill_flow_ratio",
            "outer_wall_flow_ratio",
            "inner_wall_flow_ratio",
            "overhang_flow_ratio",
            "sparse_infill_flow_ratio",
            "internal_solid_infill_flow_ratio",
            "gap_fill_flow_ratio",
            "support_flow_ratio",
            "support_interface_flow_ratio",
            "brim_flow_ratio",
            "filament_flow_ratio",
            "scarf_joint_flow_ratio",
            "spiral_starting_flow_ratio",
            "spiral_finishing_flow_ratio",
        ],
        action: ObjectAction::GCodeExport,
    },
    ObjectBranch {
        keys: &[
            "flush_into_infill",
            "flush_into_objects",
            "flush_into_support",
        ],
        action: ObjectAction::FlushInto,
    },
];

/// One branch of `PrintObject::invalidate_step`'s propagation: the step, the object's steps and
/// the print's steps it invalidates with the bases' `invalidate_steps`, and whether it resets
/// the slicing parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectStepPropagation {
    /// The step invalidated.
    pub step: PrintObjectStep,
    /// `this->invalidate_steps({...})`.
    pub object_steps: &'static [PrintObjectStep],
    /// `m_print->invalidate_steps({...})`; empty where the branch has no such call.
    pub print_steps: &'static [PrintStep],
    /// `m_slicing_params.valid = false`.
    pub resets_slicing_params: bool,
}

/// PrintObject.cpp lines 1475-1491: the propagation to dependent steps, in order.
pub const OBJECT_STEP_PROPAGATION: &[ObjectStepPropagation] = &[
    ObjectStepPropagation {
        step: PrintObjectStep::Perimeters,
        object_steps: &[
            PrintObjectStep::PrepareInfill,
            PrintObjectStep::Infill,
            PrintObjectStep::Ironing,
            PrintObjectStep::Contouring,
            PrintObjectStep::SimplifyPath,
            PrintObjectStep::SimplifyInfill,
        ],
        print_steps: &[PrintStep::SkirtBrim],
        resets_slicing_params: false,
    },
    ObjectStepPropagation {
        step: PrintObjectStep::PrepareInfill,
        object_steps: &[
            PrintObjectStep::Infill,
            PrintObjectStep::Ironing,
            PrintObjectStep::Contouring,
            PrintObjectStep::SimplifyPath,
            PrintObjectStep::SimplifyInfill,
        ],
        print_steps: &[],
        resets_slicing_params: false,
    },
    ObjectStepPropagation {
        step: PrintObjectStep::Infill,
        object_steps: &[
            PrintObjectStep::Ironing,
            PrintObjectStep::Contouring,
            PrintObjectStep::SimplifyInfill,
        ],
        print_steps: &[PrintStep::SkirtBrim],
        resets_slicing_params: false,
    },
    ObjectStepPropagation {
        step: PrintObjectStep::Slice,
        object_steps: &[
            PrintObjectStep::Perimeters,
            PrintObjectStep::PrepareInfill,
            PrintObjectStep::Infill,
            PrintObjectStep::Ironing,
            PrintObjectStep::Contouring,
            PrintObjectStep::SupportMaterial,
            PrintObjectStep::SimplifyPath,
            PrintObjectStep::SimplifyInfill,
        ],
        print_steps: &[PrintStep::SkirtBrim],
        resets_slicing_params: true,
    },
    ObjectStepPropagation {
        step: PrintObjectStep::SupportMaterial,
        object_steps: &[PrintObjectStep::SimplifySupportPath],
        print_steps: &[PrintStep::SkirtBrim],
        resets_slicing_params: true,
    },
];

/// The first branch of [`PRINT_BRANCHES`] that names `opt_key`.
#[must_use]
pub fn print_branch(opt_key: &str) -> Option<&'static PrintBranch> {
    PRINT_BRANCHES.iter().find(|b| b.keys.contains(&opt_key))
}

/// The first branch of [`OBJECT_BRANCHES`] that names `opt_key`.
#[must_use]
pub fn object_branch(opt_key: &str) -> Option<&'static ObjectBranch> {
    OBJECT_BRANCHES.iter().find(|b| b.keys.contains(&opt_key))
}

/// `Print::invalidate_step(step)`: the base's, then, for any step but `psGCodeExport`, the
/// base's `psGCodeExport` (each always called).
pub fn print_invalidate_step(print: &mut PrintInherited, step: PrintStep) -> bool {
    let mut invalidated = print.invalidate_step(step);
    // Propagate to dependent steps.
    if step != PrintStep::GCodeExport {
        invalidated |= print.invalidate_step(PrintStep::GCodeExport);
    }
    invalidated
}

/// `Print::invalidate_state_by_config_options(new_config, opt_keys)`: each changed print option
/// adds the steps its set or branch names (the G-code export for `steps_gcode`, nothing for
/// `steps_ignore`), or invalidates all the print's steps at once when none names it. Then the
/// print steps, sorted and unique, through [`print_invalidate_step`], and the object steps,
/// sorted and unique, through [`print_object_invalidate_step`] of each object. The new config is
/// not read, as in the C++.
pub fn print_invalidate_state_by_config_options(
    print: &mut Print,
    _new_config: &dyn ConfigBase,
    opt_keys: &[String],
) -> bool {
    if opt_keys.is_empty() {
        return false;
    }

    let mut steps: Vec<PrintStep> = Vec::new();
    let mut osteps: Vec<PrintObjectStep> = Vec::new();
    let mut invalidated = false;

    for opt_key in opt_keys {
        let key = opt_key.as_str();
        if PRINT_STEPS_GCODE.contains(&key) {
            // These options only affect G-code export or they are just notes without influence on the generated G-code,
            // so there is nothing to invalidate.
            steps.push(PrintStep::GCodeExport);
        } else if PRINT_STEPS_IGNORE.contains(&key) {
            // These steps have no influence on the G-code whatsoever. Just ignore them.
        } else if let Some(branch) = print_branch(key) {
            steps.extend_from_slice(branch.steps);
            osteps.extend_from_slice(branch.osteps);
        } else {
            // for legacy, if we can't handle this option let's invalidate all steps
            //FIXME invalidate all steps of all objects as well?
            invalidated |= print.base.invalidate_all_steps();
            // Continue with the other opt_keys to possibly invalidate any object specific steps.
        }
    }

    sort_remove_duplicates(&mut steps);
    for step in steps {
        invalidated |= print_invalidate_step(&mut print.base, step);
    }
    sort_remove_duplicates(&mut osteps);
    for ostep in osteps {
        for object in &mut print.objects {
            invalidated |= print_object_invalidate_step(object, &mut print.base, ostep);
        }
    }

    invalidated
}

/// `ConfigOptionResolver::option<ConfigOptionEnum<T>>(key)`'s value: the option of a static
/// config, or of a dynamic config whose value is an enum of `T`'s names; none where the C++'s
/// `dynamic_cast` gives null.
fn option_enum<E: ConfigEnum>(config: &dyn ConfigBase, key: &str) -> Option<E> {
    let opt = config.option(key)?;
    if let Some(o) = opt.as_any().downcast_ref::<ConfigOptionEnum<E>>() {
        return Some(o.value);
    }
    match opt.as_any().downcast_ref::<OptionValue>() {
        Some(OptionValue::Enum { value, keys }) if *keys == E::keys_map() => E::from_int(*value),
        _ => None,
    }
}

/// `ConfigOptionResolver::option<ConfigOptionFloat>(key)`'s value: a float option, or a percent
/// (`ConfigOptionPercent` derives from `ConfigOptionFloat`); none where the cast gives null.
fn option_float(config: &dyn ConfigBase, key: &str) -> Option<f64> {
    let opt = config.option(key)?;
    let any = opt.as_any();
    if let Some(o) = any.downcast_ref::<ConfigOptionFloat>() {
        return Some(o.value);
    }
    if let Some(o) = any.downcast_ref::<ConfigOptionPercent>() {
        return Some(o.value);
    }
    match any.downcast_ref::<OptionValue>() {
        Some(OptionValue::Float(v) | OptionValue::Percent(v)) => Some(*v),
        _ => None,
    }
}

/// `ConfigOptionResolver::option<ConfigOptionPercent>(key)`'s value; none where the cast gives
/// null.
fn option_percent(config: &dyn ConfigBase, key: &str) -> Option<f64> {
    let opt = config.option(key)?;
    let any = opt.as_any();
    if let Some(o) = any.downcast_ref::<ConfigOptionPercent>() {
        return Some(o.value);
    }
    match any.downcast_ref::<OptionValue>() {
        Some(OptionValue::Percent(v)) => Some(*v),
        _ => None,
    }
}

/// The C++'s `->value` of an option `option<T>` gave: a null one is dereferenced.
fn deref<T>(value: Option<T>) -> T {
    value.unwrap_or_else(|| {
        not_ported(
            "PrintObject::invalidate_state_by_config_options",
            "a config has no option of the key's type (the C++ dereferences the null option)",
        )
    })
}

/// `PrintObject::invalidate_state_by_config_options(old_config, new_config, opt_keys)`: each
/// changed object or region option takes the first branch of [`OBJECT_BRANCHES`] that names it,
/// and a key none names invalidates all the object's steps and the print's at once
/// ([`print_object_invalidate_all_steps`]). Then the steps, sorted and unique, through
/// [`print_object_invalidate_step`].
pub fn print_object_invalidate_state_by_config_options(
    object: &mut PrintObject,
    print: &mut PrintInherited,
    old_config: &dyn ConfigBase,
    new_config: &dyn ConfigBase,
    opt_keys: &[String],
) -> bool {
    if opt_keys.is_empty() {
        return false;
    }

    let mut steps: Vec<PrintObjectStep> = Vec::new();
    let mut invalidated = false;
    for opt_key in opt_keys {
        let key = opt_key.as_str();
        match object_branch(key).map(|b| b.action) {
            Some(ObjectAction::Steps(s)) => steps.extend_from_slice(s),
            Some(ObjectAction::Brim) => {
                // Brim is printed below supports, support invalidates brim and skirt.
                steps.push(PrintObjectStep::SupportMaterial);
                if key == "brim_type" {
                    let old_brim_type = option_enum::<BrimType>(old_config, key);
                    let new_brim_type = option_enum::<BrimType>(new_config, key);
                    //BBS: When switch to manual brim, the object must have brim, then re-generate perimeter
                    //to make the wall order of first layer to be outer-first
                    if deref(old_brim_type) == BrimType::btOuterOnly
                        || deref(new_brim_type) == BrimType::btOuterOnly
                    {
                        steps.push(PrintObjectStep::Perimeters);
                    }
                }
            }
            Some(ObjectAction::GapFill) => {
                // Return true if gap-fill speed has changed from zero value to non-zero or from non-zero value to zero.
                // todo multi_extruders: Parameter migration between single and double extruder printers
                let is_gap_fill_changed_state_due_to_speed = || -> bool {
                    if key == "gap_infill_speed" {
                        let old_gap_fill_speed = deref(option_float(old_config, key));
                        let new_gap_fill_speed = deref(option_float(new_config, key));
                        #[allow(clippy::float_cmp)] // The C++ compares with 0.f.
                        let changed = (old_gap_fill_speed > 0. && new_gap_fill_speed == 0.)
                            || (old_gap_fill_speed == 0. && new_gap_fill_speed > 0.);
                        return changed;
                    }
                    false
                };

                // Filtering of unprintable regions in multi-material segmentation depends on if gap-fill is enabled or not.
                // So step posSlice is invalidated when gap-fill was enabled/disabled by option "filter_out_gap_fill" or by
                // changing "gap_infill_speed" to force recomputation of the multi-material segmentation.
                if object.is_mm_painted(object.model_object(&print.base.model))
                    && (key == "filter_out_gap_fill"
                        && (key == "gap_infill_speed" && is_gap_fill_changed_state_due_to_speed()))
                {
                    steps.push(PrintObjectStep::Slice);
                }
                steps.push(PrintObjectStep::Perimeters);
            }
            Some(ObjectAction::EnableSupport) => {
                steps.push(PrintObjectStep::SupportMaterial);
                #[allow(clippy::float_cmp)] // The C++ compares the option with 0.
                let soluble = object.config.support_top_z_distance.value == 0.;
                if soluble {
                    // Enabling / disabling supports while soluble support interface is enabled.
                    // This changes the bridging logic (bridging enabled without supports, disabled with supports).
                    // Reset everything.
                    // See GH #1482 for details.
                    steps.push(PrintObjectStep::Slice);
                }
            }
            Some(ObjectAction::SparseInfillDensity) => {
                // One likely wants to reslice only when switching between zero infill to simulate boolean difference (subtracting volumes),
                // normal infill and 100% (solid) infill.
                let old_density = option_percent(old_config, key);
                let new_density = option_percent(new_config, key);
                //FIXME Vojtech is not quite sure about the 100% here, maybe it is not needed.
                // The C++ dereferences each option where the || reaches it.
                let old_value = || deref(old_density);
                let new_value = || deref(new_density);
                if is_approx(old_value(), 0.)
                    || is_approx(old_value(), 100.)
                    || is_approx(new_value(), 0.)
                    || is_approx(new_value(), 100.)
                {
                    steps.push(PrintObjectStep::Perimeters);
                }
                steps.push(PrintObjectStep::PrepareInfill);
            }
            Some(ObjectAction::BridgeFlow) => {
                if object.config.support_top_z_distance.value > 0. {
                    // Only invalidate due to bridging if bridging is enabled.
                    // If later "support_top_z_distance" is modified, the complete PrintObject is invalidated anyway.
                    steps.push(PrintObjectStep::Perimeters);
                    steps.push(PrintObjectStep::Infill);
                    steps.push(PrintObjectStep::SupportMaterial);
                }
            }
            Some(ObjectAction::GCodeExport) => {
                invalidated |= print_invalidate_step(print, PrintStep::GCodeExport);
            }
            Some(ObjectAction::FlushInto) => {
                invalidated |= print_invalidate_step(print, PrintStep::WipeTower);
                invalidated |= print_invalidate_step(print, PrintStep::GCodeExport);
            }
            None => {
                // for legacy, if we can't handle this option let's invalidate all steps
                print_object_invalidate_all_steps(object, print);
                invalidated = true;
            }
        }
    }

    sort_remove_duplicates(&mut steps);
    for step in steps {
        invalidated |= print_object_invalidate_step(object, print, step);
    }
    invalidated
}

/// `PrintObject::invalidate_step(step)`: the base's, then the dependent steps of
/// [`OBJECT_STEP_PROPAGATION`] (the object's and the print's through the bases'
/// `invalidate_steps`, and the slicing parameters' reset), then [`print_invalidate_step`] of the
/// wipe tower and of the G-code export, which every step reaches.
pub fn print_object_invalidate_step(
    object: &mut PrintObject,
    print: &mut PrintInherited,
    step: PrintObjectStep,
) -> bool {
    let mut invalidated = object.base.invalidate_step(step, &print.base);

    // propagate to dependent steps
    if let Some(p) = OBJECT_STEP_PROPAGATION.iter().find(|p| p.step == step) {
        invalidated |= object.base.invalidate_steps(p.object_steps, &print.base);
        // posPrepareInfill's branch has no m_print->invalidate_steps call.
        if !p.print_steps.is_empty() {
            invalidated |= print.invalidate_steps(p.print_steps);
        }
        if p.resets_slicing_params {
            object.slicing_params.valid = false;
        }
    }

    // Wipe tower depends on the ordering of extruders, which in turn depends on everything.
    // It also decides about what the flush_into_infill / wipe_into_object / flush_into_support features will do,
    // and that too depends on many of the settings.
    invalidated |= print_invalidate_step(print, PrintStep::WipeTower);
    // Invalidate G-code export in any case.
    invalidated |= print_invalidate_step(print, PrintStep::GCodeExport);
    invalidated
}

/// `PrintObject::invalidate_all_steps()`: the base's and the print base's `invalidate_all_steps`
/// (the C++'s `|` calls both; MSVC calls the left one first, as here), then the slicing
/// parameters' reset.
pub fn print_object_invalidate_all_steps(
    object: &mut PrintObject,
    print: &mut PrintInherited,
) -> bool {
    // First call the "invalidate" functions, which may cancel background processing.
    let result = object.base.invalidate_all_steps(&print.base) | print.invalidate_all_steps();
    // Then reset some of the depending values.
    object.slicing_params.valid = false;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use orca_config::{PrintObjectConfig, PrintRegionConfig};
    use orca_layer::print_base::StepType;

    /// A print base with every step done, and an object of it with every step done.
    fn done_print_and_object() -> (Print, PrintObject) {
        let mut print = Print::default();
        for i in 0..orca_layer::print_object::PS_COUNT {
            let step = PrintStep::from_index(i);
            print.base.set_started(step);
            print.base.set_done(step);
        }
        let mut object = PrintObject::with_members(0, orca_eigen::Transform3d::identity());
        for i in 0..orca_layer::print_object::POS_COUNT {
            let step = PrintObjectStep::from_index(i);
            object.base.set_started(step, &print.base.base);
            object.base.set_done(step, &print.base.base);
        }
        object.slicing_params.valid = true;
        (print, object)
    }

    fn object_done(object: &PrintObject) -> Vec<bool> {
        (0..orca_layer::print_object::POS_COUNT)
            .map(|i| {
                object
                    .base
                    .is_step_done_unguarded(PrintObjectStep::from_index(i))
            })
            .collect()
    }

    fn print_done(print: &Print) -> Vec<bool> {
        (0..orca_layer::print_object::PS_COUNT)
            .map(|i| print.base.is_step_done_unguarded(PrintStep::from_index(i)))
            .collect()
    }

    #[test]
    fn print_invalidate_step_reaches_the_gcode_export() {
        let (mut print, _) = done_print_and_object();
        assert!(print_invalidate_step(&mut print.base, PrintStep::SkirtBrim));
        assert_eq!(print_done(&print), vec![true, false, false, true]);
        // A second time nothing is left to invalidate.
        assert!(!print_invalidate_step(
            &mut print.base,
            PrintStep::SkirtBrim
        ));
    }

    #[test]
    fn object_step_propagation_follows_the_table() {
        let (mut print, mut object) = done_print_and_object();
        assert!(print_object_invalidate_step(
            &mut object,
            &mut print.base,
            PrintObjectStep::PrepareInfill
        ));
        let done = object_done(&object);
        // PrepareInfill, Infill, Ironing, Contouring, SimplifyPath and SimplifyInfill go.
        assert_eq!(
            done,
            vec![
                true, true, true, false, false, false, false, true, false, true, true, true, false
            ]
        );
        // The wipe tower and the G-code export go; the skirt and brim stay (no print steps).
        assert_eq!(print_done(&print), vec![false, true, false, true]);
        assert!(object.slicing_params.valid);

        let (mut print, mut object) = done_print_and_object();
        print_object_invalidate_step(&mut object, &mut print.base, PrintObjectStep::Slice);
        assert_eq!(
            object_done(&object),
            vec![
                false, false, true, false, false, false, false, false, false, true, true, true,
                false
            ]
        );
        assert_eq!(print_done(&print), vec![false, false, false, true]);
        assert!(!object.slicing_params.valid);
    }

    #[test]
    fn object_options_take_their_first_branch() {
        // initial_layer_line_width is named by three branches; the first adds posPerimeters only.
        let (mut print, mut object) = done_print_and_object();
        let config = PrintObjectConfig::default();
        let keys = vec!["initial_layer_line_width".to_string()];
        assert!(print_object_invalidate_state_by_config_options(
            &mut object,
            &mut print.base,
            &config,
            &config,
            &keys
        ));
        let done = object_done(&object);
        assert!(done[PrintObjectStep::Slice.index()]);
        assert!(!done[PrintObjectStep::Perimeters.index()]);
        assert!(done[PrintObjectStep::SupportMaterial.index()]);
        assert_eq!(
            object_branch("outer_wall_speed").map(|b| b.action),
            Some(ObjectAction::Brim)
        );
        assert_eq!(
            object_branch("support_type").map(|b| b.action),
            Some(ObjectAction::Steps(&[PrintObjectStep::Slice]))
        );
    }

    #[test]
    fn brim_type_outer_only_also_invalidates_perimeters() {
        let mut old = PrintObjectConfig::default();
        let mut new = PrintObjectConfig::default();
        old.brim_type.value = BrimType::btAutoBrim;
        new.brim_type.value = BrimType::btOuterOnly;
        let keys = vec!["brim_type".to_string()];
        let (mut print, mut object) = done_print_and_object();
        print_object_invalidate_state_by_config_options(
            &mut object,
            &mut print.base,
            &old,
            &new,
            &keys,
        );
        assert!(!object_done(&object)[PrintObjectStep::Perimeters.index()]);
        assert!(!object_done(&object)[PrintObjectStep::SupportMaterial.index()]);

        new.brim_type.value = BrimType::btEar;
        let (mut print, mut object) = done_print_and_object();
        print_object_invalidate_state_by_config_options(
            &mut object,
            &mut print.base,
            &old,
            &new,
            &keys,
        );
        assert!(object_done(&object)[PrintObjectStep::Perimeters.index()]);
        assert!(!object_done(&object)[PrintObjectStep::SupportMaterial.index()]);
    }

    #[test]
    fn sparse_infill_density_reaches_perimeters_only_at_the_ends() {
        // A region option: Print::apply passes the region's old and new configs.
        let mut old = PrintRegionConfig::default();
        let mut new = PrintRegionConfig::default();
        let keys = vec!["sparse_infill_density".to_string()];
        old.sparse_infill_density.value = 15.;
        new.sparse_infill_density.value = 20.;
        let (mut print, mut object) = done_print_and_object();
        print_object_invalidate_state_by_config_options(
            &mut object,
            &mut print.base,
            &old,
            &new,
            &keys,
        );
        assert!(object_done(&object)[PrintObjectStep::Perimeters.index()]);
        assert!(!object_done(&object)[PrintObjectStep::PrepareInfill.index()]);

        new.sparse_infill_density.value = 100.;
        let (mut print, mut object) = done_print_and_object();
        print_object_invalidate_state_by_config_options(
            &mut object,
            &mut print.base,
            &old,
            &new,
            &keys,
        );
        assert!(!object_done(&object)[PrintObjectStep::Perimeters.index()]);
    }

    #[test]
    fn enable_support_and_bridge_flow_read_the_top_z_distance() {
        let config = PrintObjectConfig::default();
        let (mut print, mut object) = done_print_and_object();
        object.config.support_top_z_distance.value = 0.;
        let keys = vec!["enable_support".to_string()];
        print_object_invalidate_state_by_config_options(
            &mut object,
            &mut print.base,
            &config,
            &config,
            &keys,
        );
        assert!(!object_done(&object)[PrintObjectStep::Slice.index()]);

        let (mut print, mut object) = done_print_and_object();
        object.config.support_top_z_distance.value = 0.2;
        print_object_invalidate_state_by_config_options(
            &mut object,
            &mut print.base,
            &config,
            &config,
            &keys,
        );
        assert!(object_done(&object)[PrintObjectStep::Slice.index()]);
        assert!(!object_done(&object)[PrintObjectStep::SupportMaterial.index()]);

        let keys = vec!["bridge_flow".to_string()];
        let (mut print, mut object) = done_print_and_object();
        object.config.support_top_z_distance.value = 0.;
        assert!(!print_object_invalidate_state_by_config_options(
            &mut object,
            &mut print.base,
            &config,
            &config,
            &keys
        ));
        assert!(object_done(&object).iter().all(|d| *d));
    }

    #[test]
    fn gcode_and_flush_branches_invalidate_the_print_at_once() {
        let config = PrintObjectConfig::default();
        let (mut print, mut object) = done_print_and_object();
        let keys = vec!["seam_position".to_string()];
        assert!(print_object_invalidate_state_by_config_options(
            &mut object,
            &mut print.base,
            &config,
            &config,
            &keys
        ));
        assert_eq!(print_done(&print), vec![true, true, false, true]);
        assert!(object_done(&object).iter().all(|d| *d));

        let (mut print, mut object) = done_print_and_object();
        let keys = vec!["flush_into_objects".to_string()];
        print_object_invalidate_state_by_config_options(
            &mut object,
            &mut print.base,
            &config,
            &config,
            &keys,
        );
        assert_eq!(print_done(&print), vec![false, true, false, true]);
    }

    #[test]
    fn an_unknown_object_option_invalidates_everything() {
        let config = PrintObjectConfig::default();
        let (mut print, mut object) = done_print_and_object();
        let keys = vec!["no_such_option".to_string()];
        assert!(print_object_invalidate_state_by_config_options(
            &mut object,
            &mut print.base,
            &config,
            &config,
            &keys
        ));
        assert!(object_done(&object).iter().all(|d| !*d));
        assert!(print_done(&print).iter().all(|d| !*d));
        assert!(!object.slicing_params.valid);
    }

    #[test]
    fn print_options_add_their_steps() {
        let config = PrintObjectConfig::default();
        let (mut print, object) = done_print_and_object();
        print.objects.push(object);
        // A G-code-only option.
        let keys = vec!["filament_notes".to_string()];
        assert!(print_invalidate_state_by_config_options(
            &mut print, &config, &keys
        ));
        assert_eq!(print_done(&print), vec![true, true, false, true]);
        assert!(object_done(&print.objects[0]).iter().all(|d| *d));

        // chamber_temperature is in steps_gcode, which goes first.
        let (mut print, object) = done_print_and_object();
        print.objects.push(object);
        let keys = vec!["chamber_temperature".to_string()];
        print_invalidate_state_by_config_options(&mut print, &config, &keys);
        assert_eq!(print_done(&print), vec![true, true, false, true]);

        // spiral_mode reslices every object.
        let (mut print, object) = done_print_and_object();
        print.objects.push(object);
        let keys = vec!["spiral_mode".to_string()];
        assert!(print_invalidate_state_by_config_options(
            &mut print, &config, &keys
        ));
        assert!(!object_done(&print.objects[0])[PrintObjectStep::Slice.index()]);

        // An unknown option invalidates the print's steps, not the objects'.
        let (mut print, object) = done_print_and_object();
        print.objects.push(object);
        let keys = vec!["no_such_option".to_string()];
        assert!(print_invalidate_state_by_config_options(
            &mut print, &config, &keys
        ));
        assert!(print_done(&print).iter().all(|d| !*d));
        assert!(object_done(&print.objects[0]).iter().all(|d| *d));

        assert!(!print_invalidate_state_by_config_options(
            &mut print,
            &config,
            &[]
        ));
    }
}
