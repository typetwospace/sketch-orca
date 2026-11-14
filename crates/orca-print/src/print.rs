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
// Carries OrcaSlicer code: src/libslic3r/Print.cpp lines 1-92 (PrintRegion's constructors from a
// config, FilamentType, Print::clear and has_tpu_filament), 423-426 (set_calib_params), 437-448
// (is_step_done of an object step), 548-587 and 592-612 (num_object_instances,
// max_allowed_layer_height, print_object_ids, has_infinite_skirt, has_skirt,
// layers_sorted_for_object; has_brim, lines 588-591, is orca-layer's, re-exported), 3290-3302
// (has_wipe_tower), 3385-3388 (enable_timelapse_print) and 3851-3885 (the shrinkage compensation
// checks), and lines 450-547 (object_extruders, support_material_extruders, extruders), of
// OrcaSlicer v2.4.2, translated on 2026-11-14 (orca-o3-print-1). Copyright the OrcaSlicer
// authors, the BambuStudio authors (Bambu Lab), the PrusaSlicer authors (Prusa Research) and the
// Slic3r authors (Alessandro Ranellucci), under the GNU Affero General Public License, version 3
// (AGPL-3.0; LICENSE).

//! Print.cpp's members of [`Print`] that its state and its objects' set-up need, as free functions
//! over the print (orca-layer declares `Print`; a method a later crate carries is a free function
//! there).
//!
//! - [`clear`]: every step invalid, the objects (each logs its destructor's record, then drops),
//!   the print regions and the model's objects gone. The extruder statistics
//!   (`m_statistics_by_extruder_count`) are orca-plate's, held beside the print by orca-process,
//!   which clears them with the print.
//! - [`has_tpu_filament`]: the tool ordering's filaments (`m_wipe_tower_data.tool_ordering`,
//!   orca-plate's, held beside the print) are its argument.
//! - [`set_calib_params`], [`is_object_step_done`] (`is_step_done(PrintObjectStep)`),
//!   [`num_object_instances`], [`max_allowed_layer_height`], [`print_object_ids`],
//!   [`has_infinite_skirt`], [`has_skirt`], [`has_brim`] (orca-layer carries its body, as
//!   `PrintRegion::collect_object_printing_extruders` calls it; re-exported here),
//!   [`layers_sorted_for_object`], [`has_wipe_tower`], [`enable_timelapse_print`],
//!   [`has_same_shrinkage_compensations`] and [`shrinkage_compensation`].
//! - [`print_region_with_config`] and [`print_region_from_config`]: `PrintRegion`'s constructors
//!   from a config alone, which hash it (Print.cpp 55-56).
//!
//! [`object_extruders`], [`support_material_extruders`] and [`extruders`] (Print.cpp 450-547) are
//! orca-o3-process-1's lines; they are translated here because this piece's
//! [`max_allowed_layer_height`] and shrinkage checks call `extruders()`, and so does
//! `Print::apply` (orca-o3-print-2, PrintApply.cpp 1127, 1534 and 1620), while orca-process comes
//! after orca-print in the build order. They belong with the print's other queries; process-1 uses
//! them and does not translate them again. `extruders(true)` reads the tool changes among the
//! model's custom G-code per plate (`Model::plates_custom_gcodes`), which orca-model does not hold
//! (orca-cli holds them beside the model): with one filament the command line makes no tool
//! change (`cli/run.cpp`'s `set_custom_gcodes` turns a colour change into a pause and refuses a
//! filament change), so nothing is added; with more than one filament it stops with `NotPorted`
//! naming multi-filament, which domain A never reaches.
//!
//! The functions that change an object's or the print's steps (`invalidate_step`,
//! `invalidate_state_by_config_options`) are in [`crate::invalidation`].

use orca_config::OptionValue;
use orca_config::print_config::{DraftShield, TimelapseType};
use orca_config::{PrintRegionConfig, StaticPrintConfig};
use orca_eigen::Vec3d;
use orca_geom::bounding_box::BoundingBox;
use orca_geom::point::Points;
use orca_index::stop::not_ported;
use orca_layer::layer::Layer;
use orca_layer::print::{CalibParams, Print};
use orca_layer::print_base::PrintBaseWithState;
use orca_layer::print_object::{PS_COUNT, PrintObjectStep, PrintStep};
use orca_layer::print_region::PrintRegion;
use orca_model::object_id::ObjectID;

use crate::print_object;

pub use orca_layer::print::has_brim;

/// `Print::Inherited`, `PrintBaseWithState<PrintStep, psCount>`: what a print object's calls
/// reach through its `m_print` (the print's steps, its model copy, its cancel callback). A
/// [`Print`] holds it as its `base` and dereferences to it, so `&mut print` passes for it.
pub type PrintInherited = PrintBaseWithState<PrintStep, PS_COUNT>;

/// `PrintRegion(const PrintRegionConfig &config)`: the config and its hash, no object region.
#[must_use]
pub fn print_region_with_config(config: &PrintRegionConfig) -> PrintRegion {
    PrintRegion::new(config.clone(), config.hash(), -1)
}

/// `PrintRegion(PrintRegionConfig &&config)`: the hash is taken before the config moves in.
#[must_use]
pub fn print_region_from_config(config: PrintRegionConfig) -> PrintRegion {
    let config_hash = config.hash();
    PrintRegion::new(config, config_hash, -1)
}

/// `struct FilamentType` (BBS): a filament type's name, its temperature range and kind. Declared
/// at Print.cpp 62-67 and used nowhere in OrcaSlicer 2.4.2.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FilamentType {
    /// `name`.
    pub name: String,
    /// `min_temp`.
    pub min_temp: i32,
    /// `max_temp`.
    pub max_temp: i32,
    /// `temp_type`.
    pub temp_type: String,
}

/// `Print::clear()`: every step invalid (stopping any background processing, which a CLI run has
/// none of), each object deleted in order, the print regions and the model's objects gone. The
/// C++ locks the state mutex first, which is not translated (orca-layer's `print_base`). The
/// extruder statistics, held beside the print, are cleared by their holder.
pub fn clear(print: &mut Print) {
    // The following call should stop background processing if it is running.
    print.base.invalidate_all_steps();
    for i in 0..print.objects.len() {
        print_object::destructor_record(&print.objects, i);
    }
    print.objects.clear();
    print.print_regions.clear();
    print.base.base.model.clear_objects();
}

/// `Print::has_tpu_filament()`: some filament of the tool ordering (`all_extruders()` of the
/// wipe tower data's tool ordering, which orca-plate holds beside the print) is of type "TPU".
#[must_use]
pub fn has_tpu_filament(print: &Print, tool_ordering_all_extruders: &[u32]) -> bool {
    for &filament_id in tool_ordering_all_extruders {
        let filament_name = print
            .config
            .gcode
            .filament_type
            .get_at(filament_id as usize)
            .clone();
        if filament_name == "TPU" {
            return true;
        }
    }
    false
}

/// `Print::set_calib_params(params)`: the parameters, then their mode again.
pub fn set_calib_params(print: &mut Print, params: &CalibParams) {
    print.calib_params = params.clone();
    print.calib_params.mode = params.mode;
}

/// `Print::is_step_done(PrintObjectStep step)`: the object step is done on every object, and
/// there is at least one object. The C++ locks the state mutex, which is not translated.
#[must_use]
pub fn is_object_step_done(print: &Print, step: PrintObjectStep) -> bool {
    if print.objects.is_empty() {
        return false;
    }
    for object in &print.objects {
        if !object.base.is_step_done_unguarded(step) {
            return false;
        }
    }
    true
}

/// `getInt()` of a height range's `extruder` option: an int's or an enum's value (only
/// `ConfigOptionInt` and `ConfigOptionEnum` override it; every other option throws).
fn option_get_int(o: &OptionValue) -> i32 {
    match o {
        OptionValue::Int(v) | OptionValue::Enum { value: v, .. } => *v,
        _ => not_ported(
            "Print::object_extruders",
            "a height range's extruder option is not an int (the C++ throws \
             BadOptionTypeException)",
        ),
    }
}

/// `sort_remove_duplicates(vec)` (libslic3r.h): `std::sort`, then `std::unique` and the erase.
pub(crate) fn sort_remove_duplicates<T: PartialOrd>(vec: &mut Vec<T>) {
    orca_stl::sort(vec);
    orca_stl::unique_vec(vec);
}

/// `Print::object_extruders()`: the 0-based filaments the objects print with: each region's
/// printing extruders, each volume's (its painting and its own, less one), and each height
/// range's `extruder` above 0, less one; sorted, each once. orca-o3-process-1's lines (Print.cpp
/// 450-485), translated here (see the module's header).
#[must_use]
pub fn object_extruders(print: &Print) -> Vec<u32> {
    let mut extruders: Vec<u32> = Vec::with_capacity(
        print
            .print_regions
            .len()
            .wrapping_mul(print.objects.len())
            .wrapping_mul(3),
    );

    // Orca: Collect extruders from all regions.
    for object in &print.objects {
        for region in print_object::all_regions(object) {
            region.collect_object_printing_extruders(print, &mut extruders);
        }
    }

    for object in &print.objects {
        let mo = object.model_object(print.model());
        for mv in &mo.volumes {
            let volume_extruders = mv.get_extruders(&mo.config);
            for extruder in volume_extruders {
                // The C++ asserts extruder > 0; the int difference becomes an unsigned int.
                #[allow(clippy::cast_sign_loss)]
                extruders.push(extruder.wrapping_sub(1) as u32);
            }
        }

        // layer range
        for layer_range in &mo.layer_config_ranges {
            if layer_range.1.has("extruder") {
                //BBS: actually when user doesn't change filament by height range(value is default 0), height range should not save key "extruder".
                //Don't know why height range always save key "extruder" because of no change(should only save difference)...
                //Add protection here to avoid overflow
                let value = layer_range.1.option("extruder").map_or_else(
                    || {
                        not_ported(
                            "Print::object_extruders",
                            "a height range has extruder but no option for it",
                        )
                    },
                    option_get_int,
                );
                if value > 0 {
                    #[allow(clippy::cast_sign_loss)]
                    extruders.push((value - 1) as u32);
                }
            }
        }
    }
    sort_remove_duplicates(&mut extruders);
    extruders
}

/// `Print::support_material_extruders()`: the 0-based support and support interface filaments of
/// the objects with support material (a filament past the printer's counted as 0), and every
/// object filament when one of them is 0 (the current filament); sorted, each once.
/// orca-o3-process-1's lines (Print.cpp 487-520), translated here.
#[must_use]
pub fn support_material_extruders(print: &Print) -> Vec<u32> {
    let mut extruders: Vec<u32> = Vec::new();
    let mut support_uses_current_extruder = false;
    // BBS
    #[allow(clippy::cast_possible_truncation)]
    let num_extruders = print.config.gcode.filament_diameter.values.len() as u32;

    for object in &print.objects {
        if object.has_support_material() {
            // The C++ asserts support_filament >= 0.
            if object.config().support_filament.value == 0 {
                support_uses_current_extruder = true;
            } else {
                #[allow(clippy::cast_sign_loss)]
                let i = (object.config().support_filament.value as u32).wrapping_sub(1);
                extruders.push(if i >= num_extruders { 0 } else { i });
            }
            // The C++ asserts support_interface_filament >= 0.
            if object.config().support_interface_filament.value == 0 {
                support_uses_current_extruder = true;
            } else {
                #[allow(clippy::cast_sign_loss)]
                let i = (object.config().support_interface_filament.value as u32).wrapping_sub(1);
                extruders.push(if i >= num_extruders { 0 } else { i });
            }
        }
    }

    if support_uses_current_extruder {
        // Add all object extruders to the support extruders as it is not know which one will be used to print supports.
        extruders.extend(object_extruders(print));
    }

    sort_remove_duplicates(&mut extruders);
    extruders
}

/// `Print::extruders(conside_custom_gcode)`: the object and support filaments, 0-based; with
/// `conside_custom_gcode` the tool changes of the plate's custom G-code too; and the wipe tower's
/// filament when it is set, the print has a wipe tower and more than one filament prints; sorted,
/// each once. orca-o3-process-1's lines (Print.cpp 522-547), translated here.
///
/// The custom G-code per plate is not held by orca-model: with one filament (`filament_colour`
/// holds one) the command line makes no tool change, so none is added; with more it stops with
/// `NotPorted` (multi-filament).
#[must_use]
pub fn extruders(print: &Print, conside_custom_gcode: bool) -> Vec<u32> {
    let mut extruders = object_extruders(print);
    extruders.extend(support_material_extruders(print));

    if conside_custom_gcode {
        //BBS
        let num_extruders = print.config.gcode.filament_colour.values.len();
        if num_extruders > 1 {
            not_ported(
                "Print::extruders",
                "multi-filament: the tool changes of the plate's custom G-code \
                 (Model::plates_custom_gcodes, which orca-model does not hold) with more than one \
                 filament",
            );
        }
        // One filament: cli/run.cpp's set_custom_gcodes makes no ToolChange item, so the C++'s
        // loop over the plate's items adds nothing.
    }

    // If a wipe tower filament is explicitly set, ensure it participates in tool ordering.
    if has_wipe_tower(print) && print.config().wipe_tower_filament.value != 0 && extruders.len() > 1
    {
        // The C++ asserts the filament is in range; the config value is 1-based.
        #[allow(clippy::cast_sign_loss)]
        extruders.push((print.config().wipe_tower_filament.value - 1) as u32);
    }

    sort_remove_duplicates(&mut extruders);
    extruders
}

/// `Print::num_object_instances()`: the instances of every object, as an `unsigned int`.
#[must_use]
pub fn num_object_instances(print: &Print) -> u32 {
    let mut instances: u32 = 0;
    for print_object in &print.objects {
        #[allow(clippy::cast_possible_truncation)]
        let n = print_object.instances().len() as u32;
        instances = instances.wrapping_add(n);
    }
    instances
}

/// `Print::max_allowed_layer_height()`: the largest nozzle of the filaments in use (`std::max`
/// from 0).
#[must_use]
pub fn max_allowed_layer_height(print: &Print) -> f64 {
    let mut nozzle_diameter_max = 0.;
    for extruder_id in extruders(print, false) {
        nozzle_diameter_max = orca_stl::max(
            nozzle_diameter_max,
            *print.config.nozzle_diameter.get_at(extruder_id as usize),
        );
    }
    nozzle_diameter_max
}

/// `Print::print_object_ids()`: the objects' ids, in order (the C++ reserves one more, for the
/// caller to append the print's own).
#[must_use]
pub fn print_object_ids(print: &Print) -> Vec<ObjectID> {
    let mut out: Vec<ObjectID> = Vec::with_capacity(print.objects.len() + 1);
    for print_object in &print.objects {
        out.push(print_object.object_id());
    }
    out
}

/// `Print::has_infinite_skirt()`: a draft shield with skirt loops (Orca dropped the ooze
/// prevention condition).
#[must_use]
pub fn has_infinite_skirt(print: &Print) -> bool {
    // Orca: unclear why (m_config.ooze_prevention && this->extruders().size() > 1) logic is here, removed.
    // return (m_config.draft_shield == dsEnabled && m_config.skirt_loops > 0) || (m_config.ooze_prevention && this->extruders().size() > 1);

    print.config.draft_shield.value == DraftShield::dsEnabled && print.config.skirt_loops.value > 0
}

/// `Print::has_skirt()`: a skirt height above 0.
#[must_use]
pub fn has_skirt(print: &Print) -> bool {
    print.config.skirt_height.value > 0
}

/// `Print::layers_sorted_for_object(start, end, layers_of_objects, boundingBox_for_objects,
/// objects_instances_shift)` (BBS): for each object in order, its layers from `start` to `end`
/// and their overhangs' box appended ([`print_object::get_certain_layers`]); the objects' indices
/// sorted by that box's area, largest first (`std::sort`, its comparator reading the boxes from
/// the first, as the C++ indexes them); and the instances' shifts without the plate offset in
/// place of `objects_instances_shift`'s.
#[must_use]
pub fn layers_sorted_for_object<'a>(
    print: &'a Print,
    start: f32,
    end: f32,
    layers_of_objects: &mut Vec<Vec<&'a Layer>>,
    bounding_box_for_objects: &mut Vec<BoundingBox>,
    objects_instances_shift: &mut Vec<Points>,
) -> Vec<usize> {
    let mut idx_of_object_sorted: Vec<usize> = Vec::new();
    for (idx, object) in print.objects.iter().enumerate() {
        idx_of_object_sorted.push(idx);
        print_object::get_certain_layers(
            object,
            start,
            end,
            layers_of_objects,
            bounding_box_for_objects,
        );
    }
    // The lambda captures the boxes by value.
    let boxes = bounding_box_for_objects.clone();
    orca_stl::sort_by(&mut idx_of_object_sorted, |left, right| {
        boxes[*left].area() > boxes[*right].area()
    });

    objects_instances_shift.clear();
    objects_instances_shift.reserve(print.objects.len());
    for object in &print.objects {
        objects_instances_shift.push(print_object::get_instances_shift_without_plate_offset(
            object, print,
        ));
    }

    idx_of_object_sorted
}

/// `Print::has_wipe_tower()`: the prime tower is on, and clumping detection with an exclude area
/// of more than two points, the smooth timelapse, or more than one filament outside the spiral
/// vase.
#[must_use]
pub fn has_wipe_tower(print: &Print) -> bool {
    if print.config.enable_prime_tower.value {
        if print.config.gcode.enable_wrapping_detection.value
            && print.config.gcode.wrapping_exclude_area.values.len() > 2
        {
            return true;
        }

        if enable_timelapse_print(print) {
            return true;
        }

        return !print.config.spiral_mode.value
            && print.config.gcode.filament_diameter.values.len() > 1;
    }
    false
}

/// `Print::enable_timelapse_print()`: the smooth timelapse.
#[must_use]
pub fn enable_timelapse_print(print: &Print) -> bool {
    print.config.timelapse_type.value == TimelapseType::tlSmooth
}

/// `Print::has_same_shrinkage_compensations()` (Orca, after PrusaSlicer): some filament is used,
/// and every one used has the first one's XY and Z shrinkage.
#[must_use]
pub fn has_same_shrinkage_compensations(print: &Print) -> bool {
    let extruders = extruders(print, false);
    let Some(&front) = extruders.first() else {
        return false;
    };

    let filament_shrinkage_compensation_xy = *print.config.filament_shrink.get_at(front as usize);
    let filament_shrinkage_compensation_z = *print
        .config
        .filament_shrinkage_compensation_z
        .get_at(front as usize);

    for extruder in extruders {
        if filament_shrinkage_compensation_xy
            != *print.config.filament_shrink.get_at(extruder as usize)
            || filament_shrinkage_compensation_z
                != *print
                    .config
                    .filament_shrinkage_compensation_z
                    .get_at(extruder as usize)
        {
            return false;
        }
    }

    true
}

/// `Print::shrinkage_compensation()` (Orca): the scale of each axis that compensates the
/// filaments' shrinkage, `100 / percent` (so 100% is no compensation); ones unless every filament
/// used shrinks alike.
#[must_use]
pub fn shrinkage_compensation(print: &Print) -> Vec3d {
    if !has_same_shrinkage_compensations(print) {
        return Vec3d::new(1., 1., 1.);
    }

    // Not empty: has_same_shrinkage_compensations found a filament.
    let first_extruder = extruders(print, false)[0];

    let xy_shrinkage_percent = *print.config.filament_shrink.get_at(first_extruder as usize);
    let z_shrinkage_percent = *print
        .config
        .filament_shrinkage_compensation_z
        .get_at(first_extruder as usize);

    let xy_compensation = 100.0 / xy_shrinkage_percent;
    let z_compensation = 100.0 / z_shrinkage_percent;

    Vec3d::new(xy_compensation, xy_compensation, z_compensation)
}
