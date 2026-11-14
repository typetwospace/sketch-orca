// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Print.hpp (800-867 and 881-1232: PrintStatistics, the
// pointer lists, FilamentTempType, FilamentCompatibilityType and class Print with its inline
// members) and Print.cpp (3206-3214 and 3251-3287: get_filament_maps, get_filament_map_mode, the
// extruders' printable heights and areas, get_extruder_id), with Print.cpp's has_brim (588-591)
// and calib.hpp's CalibMode and Calib_Params, of OrcaSlicer v2.4.2, translated on 2026-11-14
// (orca-o3-layer-3). Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab), the
// PrusaSlicer authors (Prusa Research) and the Slic3r authors, under the GNU Affero General Public
// License, version 3 (AGPL-3.0; LICENSE).

//! [`Print`]: the complete print tray, its objects, the print regions they use, and what the
//! later steps leave on it (skirt and brims, statistics, filament use).
//!
//! The print derives from `PrintBaseWithState<PrintStep, psCount>`, here its [`Print::base`],
//! which it dereferences to (as `PrintBaseWithState` dereferences to `PrintBase`). It owns its
//! objects (`m_objects`, `PrintObjectPtrs`); `ConstPrintObjectPtrsAdaptor` is a slice of them. Its
//! print regions (`m_print_regions`, pointers into the objects' shared regions) are
//! [`PrintRegionPtr`]s: the shared regions' `Arc` and the region's index there, which keeps the
//! regions alive as the pointer does not. A `PrintRegionPtr` reads the `Arc` it was made from, so
//! whoever replaces an object's shared regions (`Print::apply`, orca-o3-print-2) remakes the
//! pointers into them. The maps by `ObjectID` are `BTreeMap`s, which iterate in the key order
//! `std::map` does.
//!
//! The C++ leaves `m_isBBLPrinter`, `m_origin` and the members of `Calib_Params` but
//! `extruder_id` and `mode` uninitialised (`Print() = default`, which the command line calls on a
//! print it then sets up); here they are false and zero. A CLI run sets `is_BBL_printer()` before
//! any reader, and reads no calibration value.
//!
//! Held beside the print, not in it, as their types are orca-plate's (phase O4): the tool
//! ordering and wipe tower data (`m_tool_ordering`, `m_wipe_tower_data`, `tool_ordering()`,
//! `get_tool_ordering()`, `wipe_tower_data()`, `get_wipe_tower_depth()`, `get_wipe_tower_bbx()`,
//! `get_rib_offset()`), the extruder statistics (`m_statistics_by_extruder_count`,
//! `statistics_by_extruder()`, `StatisticsByExtruderCount`, Print.hpp 868-879), the conflict
//! result (`m_conflict_result`, `get_conflict_result()` and `get_conflict_string()`, which reads
//! it) and the fake wipe tower (`m_fake_wipe_tower`, `get_fake_wipe_tower()`).
//!
//! The members with bodies in Print.cpp, PrintApply.cpp, GCode.cpp and the rest are written with
//! their bodies, as free functions or extension traits of the crates that carry them (orca-print,
//! orca-process, orca-plate, orca-gcode): `clear`, `print_object_ids`, `apply`, `process`,
//! `export_gcode`, `export_cached_data`, `load_cached_data`, `is_step_done(PrintObjectStep)`,
//! `has_infinite_skirt`, `has_skirt`, `validate`, `skirt_first_layer_height`, `brim_flow`,
//! `skirt_flow`, `object_extruders`, `support_material_extruders`, `extruders`,
//! `max_allowed_layer_height`, `has_support_material`, `auto_assign_extruders`,
//! `num_object_instances`, `layers_sorted_for_object`, `has_wipe_tower`,
//! `update_filament_maps_to_config`, `apply_config_for_render`,
//! `get_physical_unprintable_filaments`, `enable_timelapse_print`, `output_filename`,
//! `get_model_name`, `get_plate_number_formatted`, `set_gcode_file_ready`,
//! `set_gcode_file_invalidated`, `export_gcode_from_previous_file`,
//! `sequential_print_clearance_valid`, `first_layer_wipe_tower_corners`, `set_calib_params`,
//! `translate_to_print_space`, `get_filament_temp_type`, `get_hrc_by_nozzle_type`,
//! `get_incompatible_filaments_by_nozzle`, `check_multi_filaments_compatibility`,
//! `is_filaments_compatible`, `get_compatible_filament_type`, `has_same_shrinkage_compensations`,
//! `shrinkage_compensation`, `object_skirt_offset`, `invalidate_step`, `check_multi_filament_valid`,
//! `has_tpu_filament`, `invalidate_state_by_config_options`, `_make_skirt`, `_make_wipe_tower`,
//! `finalize_first_layer_convex_hull` and `first_layer_islands`; and `PrintStatistics`'s `config`,
//! `placeholders`, `finalize_output_path` and its key strings. [`has_brim`] (Print.cpp 588-591,
//! orca-o3-print-1's lines) is translated here because `PrintRegion`'s
//! `collect_object_printing_extruders` calls it; it belongs in orca-print.
//!
//! [`CalibMode`] and [`CalibParams`] are calib.hpp's, which no crate carries yet; they belong
//! with the calibration code (orca-gcode, phase O4).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use orca_config::config::Config;
use orca_config::print_config::{FilamentMapMode, WipeTowerType};
use orca_config::{PrintConfig, PrintObjectConfig, PrintRegionConfig, PrinterTechnology};
use orca_eigen::{Vec2d, Vec3d};
use orca_geom::clipper_utils::{ApplySafetyOffset, diff_polygon};
use orca_geom::expolygon::ExPolygons;
use orca_geom::point::{Points, scale_};
use orca_geom::polygon::{Polygon, Polygons};
use orca_model::object_id::ObjectID;

use crate::collection::ExtrusionEntityCollection;
use crate::print_base::PrintBaseWithState;
use crate::print_object::{PS_COUNT, PrintObject, PrintObjectRegions, PrintStep};
use crate::print_region::PrintRegion;

/// `struct PrintStatistics`: the estimated print time and the filament used. The constructor
/// calls `clear()`, which leaves the two time texts as they were (empty).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PrintStatistics {
    /// `estimated_normal_print_time`.
    pub estimated_normal_print_time: String,
    /// `estimated_silent_print_time`.
    pub estimated_silent_print_time: String,
    /// `total_used_filament`.
    pub total_used_filament: f64,
    /// `total_extruded_volume`.
    pub total_extruded_volume: f64,
    /// `total_cost`.
    pub total_cost: f64,
    /// `total_toolchanges`.
    pub total_toolchanges: i32,
    /// `total_weight`.
    pub total_weight: f64,
    /// `total_wipe_tower_cost`.
    pub total_wipe_tower_cost: f64,
    /// `total_wipe_tower_filament`.
    pub total_wipe_tower_filament: f64,
    /// `initial_tool`.
    pub initial_tool: u32,
    /// `filament_stats`: filament used by filament index.
    pub filament_stats: BTreeMap<usize, f64>,
}

impl PrintStatistics {
    /// `PrintStatistics()`: `clear()` on empty texts.
    #[must_use]
    pub fn new() -> Self {
        let mut out = Self::default();
        out.clear();
        out
    }

    /// `clear()`: every number zero and the filament map empty; the time texts stay.
    pub fn clear(&mut self) {
        self.total_used_filament = 0.;
        self.total_extruded_volume = 0.;
        self.total_cost = 0.;
        self.total_toolchanges = 0;
        self.total_weight = 0.;
        self.total_wipe_tower_cost = 0.;
        self.total_wipe_tower_filament = 0.;
        self.initial_tool = 0;
        self.filament_stats.clear();
    }
}

/// `PrintObjectPtrs`: the print's objects, owned.
pub type PrintObjectPtrs = Vec<PrintObject>;

/// One `PrintRegion*` of `PrintRegionPtrs`: a region of an object's shared regions, by the shared
/// regions' `Arc` and its index in their `all_regions`.
#[derive(Clone, Debug)]
pub struct PrintRegionPtr {
    /// The shared regions that hold the region.
    pub regions: Arc<PrintObjectRegions>,
    /// The region's index in `regions.all_regions`.
    pub idx: usize,
}

impl PrintRegionPtr {
    /// `*ptr`: the region.
    #[must_use]
    pub fn get(&self) -> &PrintRegion {
        &self.regions.all_regions[self.idx]
    }
}

/// `PrintRegionPtrs`.
pub type PrintRegionPtrs = Vec<PrintRegionPtr>;

/// `enum FilamentTempType`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i32)]
pub enum FilamentTempType {
    /// `HighTemp`.
    #[default]
    HighTemp = 0,
    /// `LowTemp`.
    LowTemp = 1,
    /// `HighLowCompatible`.
    HighLowCompatible = 2,
    /// `Undefine`.
    Undefine = 3,
}

/// `enum FilamentCompatibilityType`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i32)]
pub enum FilamentCompatibilityType {
    /// `Compatible`.
    #[default]
    Compatible = 0,
    /// `HighLowMixed`.
    HighLowMixed = 1,
    /// `InvalidTemperatureRange`.
    InvalidTemperatureRange = 2,
}

/// `enum class CalibMode` (calib.hpp).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i32)]
pub enum CalibMode {
    /// `Calib_None`.
    #[default]
    None = 0,
    /// `Calib_PA_Line`.
    PaLine = 1,
    /// `Calib_PA_Pattern`.
    PaPattern = 2,
    /// `Calib_PA_Tower`.
    PaTower = 3,
    /// `Calib_Auto_PA_Line`.
    AutoPaLine = 4,
    /// `Calib_Flow_Rate`.
    FlowRate = 5,
    /// `Calib_Temp_Tower`.
    TempTower = 6,
    /// `Calib_Vol_speed_Tower`.
    VolSpeedTower = 7,
    /// `Calib_VFA_Tower`.
    VfaTower = 8,
    /// `Calib_Retraction_tower`.
    RetractionTower = 9,
    /// `Calib_Input_shaping_freq`.
    InputShapingFreq = 10,
    /// `Calib_Input_shaping_damp`.
    InputShapingDamp = 11,
    /// `Calib_Cornering`.
    Cornering = 12,
}

/// `struct Calib_Params` (calib.hpp). The C++ constructor sets only `mode` (and `extruder_id` its
/// initialiser); the rest is zero here.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CalibParams {
    /// `extruder_id`.
    pub extruder_id: i32,
    /// `start`.
    pub start: f64,
    /// `end`.
    pub end: f64,
    /// `step`.
    pub step: f64,
    /// `print_numbers`.
    pub print_numbers: bool,
    /// `freqStartX`.
    pub freq_start_x: f64,
    /// `freqEndX`.
    pub freq_end_x: f64,
    /// `freqStartY`.
    pub freq_start_y: f64,
    /// `freqEndY`.
    pub freq_end_y: f64,
    /// `test_model`.
    pub test_model: i32,
    /// `shaper_type`.
    pub shaper_type: String,
    /// `accelerations`.
    pub accelerations: Vec<f64>,
    /// `speeds`.
    pub speeds: Vec<f64>,
    /// `mode`.
    pub mode: CalibMode,
}

/// `Print::SkirtBrimGroup::Brim`.
#[derive(Clone, Debug, Default)]
pub struct Brim {
    /// `brim`.
    pub brim: ExtrusionEntityCollection,
    /// `object_ids`.
    pub object_ids: Vec<ObjectID>,
}

/// `Print::SkirtBrimGroup`: a skirt and the brims inside it. Brims stay separate unless Combine
/// brims merges colliding brims inside the group.
#[derive(Clone, Debug, Default)]
pub struct SkirtBrimGroup {
    /// `skirt`.
    pub skirt: ExtrusionEntityCollection,
    /// `object_ids`.
    pub object_ids: Vec<ObjectID>,
    /// `brims`.
    pub brims: Vec<Brim>,
}

/// `class Print`.
#[derive(Debug)]
pub struct Print {
    /// The `PrintBaseWithState<PrintStep, psCount>` it derives from.
    pub base: PrintBaseWithState<PrintStep, PS_COUNT>,
    /// `m_config`.
    pub config: PrintConfig,
    /// `m_default_object_config`.
    pub default_object_config: PrintObjectConfig,
    /// `m_default_region_config`.
    pub default_region_config: PrintRegionConfig,
    /// `m_objects`.
    pub objects: PrintObjectPtrs,
    /// `m_print_regions`.
    pub print_regions: PrintRegionPtrs,
    /// `m_isBBLPrinter` (SoftFever; uninitialised in the C++).
    pub is_bbl_printer: bool,
    /// `m_skirt`: the skirt loops.
    pub skirt: ExtrusionEntityCollection,
    /// `m_skirt_brim_groups`.
    pub skirt_brim_groups: Vec<SkirtBrimGroup>,
    /// `m_has_shared_per_object_skirt`.
    pub has_shared_per_object_skirt: bool,
    /// `m_brimMap` (BBS): the brim of each object.
    pub brim_map: BTreeMap<ObjectID, ExtrusionEntityCollection>,
    /// `m_supportBrimMap` (BBS).
    pub support_brim_map: BTreeMap<ObjectID, ExtrusionEntityCollection>,
    /// `m_objectBrimAreas` (Orca): the brims' footprints, for grouping per-object skirts.
    pub object_brim_areas: BTreeMap<ObjectID, ExPolygons>,
    /// `m_supportBrimAreas` (Orca).
    pub support_brim_areas: BTreeMap<ObjectID, ExPolygons>,
    /// `m_first_layer_convex_hull`.
    pub first_layer_convex_hull: Polygon,
    /// `m_skirt_convex_hull`.
    pub skirt_convex_hull: Points,
    /// `m_extruder_filament_info`.
    pub extruder_filament_info: Vec<Vec<Config>>,
    /// `m_print_statistics`.
    pub print_statistics: PrintStatistics,
    /// `m_support_used`.
    pub support_used: bool,
    /// `m_slice_used_filaments`.
    pub slice_used_filaments: Vec<u32>,
    /// `m_slice_used_filaments_first_layer`.
    pub slice_used_filaments_first_layer: Vec<u32>,
    /// `m_origin` (BBS): the plate's origin (uninitialised in the C++).
    pub origin: Vec3d,
    /// `m_modified_count` (BBS).
    pub modified_count: i32,
    /// `m_has_auto_filament_map_result`.
    pub has_auto_filament_map_result: bool,
    /// `m_geometric_unprintable_filaments`.
    pub geometric_unprintable_filaments: Vec<BTreeSet<i32>>,
    /// `m_calib_params` (SoftFever).
    pub calib_params: CalibParams,
    /// `m_need_check_multi_filaments_compatibility`.
    pub need_check_multi_filaments_compatibility: bool,
}

impl Default for Print {
    fn default() -> Self {
        Self::new()
    }
}

impl core::ops::Deref for Print {
    type Target = PrintBaseWithState<PrintStep, PS_COUNT>;
    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl core::ops::DerefMut for Print {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.base
    }
}

impl Print {
    /// `Print()`: the base (a new id, the model copy, every step invalid), the configs at their
    /// defaults, the rest empty.
    #[must_use]
    pub fn new() -> Self {
        Self {
            base: PrintBaseWithState::default(),
            config: PrintConfig::default(),
            default_object_config: PrintObjectConfig::default(),
            default_region_config: PrintRegionConfig::default(),
            objects: PrintObjectPtrs::new(),
            print_regions: PrintRegionPtrs::new(),
            is_bbl_printer: false,
            skirt: ExtrusionEntityCollection::default(),
            skirt_brim_groups: Vec::new(),
            has_shared_per_object_skirt: false,
            brim_map: BTreeMap::new(),
            support_brim_map: BTreeMap::new(),
            object_brim_areas: BTreeMap::new(),
            support_brim_areas: BTreeMap::new(),
            first_layer_convex_hull: Polygon::default(),
            skirt_convex_hull: Points::new(),
            extruder_filament_info: Vec::new(),
            print_statistics: PrintStatistics::new(),
            support_used: false,
            slice_used_filaments: Vec::new(),
            slice_used_filaments_first_layer: Vec::new(),
            origin: Vec3d::new(0., 0., 0.),
            modified_count: 0,
            has_auto_filament_map_result: false,
            geometric_unprintable_filaments: Vec::new(),
            calib_params: CalibParams::default(),
            need_check_multi_filaments_compatibility: true,
        }
    }

    /// `technology()`: FFF.
    #[must_use]
    pub fn technology(&self) -> PrinterTechnology {
        PrinterTechnology::Fff
    }

    /// `empty()`: no objects.
    #[must_use]
    pub fn empty(&self) -> bool {
        self.objects.is_empty()
    }

    /// `is_step_done(PrintStep)`.
    #[must_use]
    pub fn is_step_done(&self, step: PrintStep) -> bool {
        self.base.is_step_done(step)
    }

    /// `finished()`: the G-code export is done.
    #[must_use]
    pub fn finished(&self) -> bool {
        self.is_step_done(PrintStep::GCodeExport)
    }

    /// `has_auto_brim()` (BBS): an object whose brim type is automatic.
    #[must_use]
    pub fn has_auto_brim(&self) -> bool {
        self.objects.iter().any(|object| {
            object.config().brim_type.value == orca_config::print_config::BrimType::btAutoBrim
        })
    }

    /// `config()`.
    #[must_use]
    pub fn config(&self) -> &PrintConfig {
        &self.config
    }

    /// `default_object_config()`.
    #[must_use]
    pub fn default_object_config(&self) -> &PrintObjectConfig {
        &self.default_object_config
    }

    /// `default_region_config()`.
    #[must_use]
    pub fn default_region_config(&self) -> &PrintRegionConfig {
        &self.default_region_config
    }

    /// `objects()`.
    #[must_use]
    pub fn objects(&self) -> &[PrintObject] {
        &self.objects
    }

    /// `get_object(idx)`.
    #[must_use]
    pub fn get_object(&self, idx: usize) -> &PrintObject {
        &self.objects[idx]
    }

    /// `get_object(idx)`, the non-const one.
    pub fn get_object_mut(&mut self, idx: usize) -> &mut PrintObject {
        &mut self.objects[idx]
    }

    /// `get_object(ObjectID)`: the object of that id, or none; to bind slicing warnings to the
    /// objects they come from.
    #[must_use]
    pub fn get_object_by_id(&self, object_id: ObjectID) -> Option<&PrintObject> {
        self.objects.iter().find(|obj| obj.object_id() == object_id)
    }

    /// `get_brimMap()` (BBS).
    pub fn get_brim_map(&mut self) -> &mut BTreeMap<ObjectID, ExtrusionEntityCollection> {
        &mut self.brim_map
    }

    /// `objects_mutable()`.
    pub fn objects_mutable(&mut self) -> &mut PrintObjectPtrs {
        &mut self.objects
    }

    /// `print_regions_mutable()`.
    pub fn print_regions_mutable(&mut self) -> &mut PrintRegionPtrs {
        &mut self.print_regions
    }

    /// `skirt()`.
    #[must_use]
    pub fn skirt(&self) -> &ExtrusionEntityCollection {
        &self.skirt
    }

    /// `skirt_brim_groups()`.
    #[must_use]
    pub fn skirt_brim_groups(&self) -> &[SkirtBrimGroup] {
        &self.skirt_brim_groups
    }

    /// `has_shared_per_object_skirt()`.
    #[must_use]
    pub fn has_shared_per_object_skirt(&self) -> bool {
        self.has_shared_per_object_skirt
    }

    /// `first_layer_convex_hull()`: the convex hull of the first layer's extrusions (objects,
    /// supports, skirt, brim, wipe tower; not the purge line or custom G-code).
    #[must_use]
    pub fn first_layer_convex_hull(&self) -> &Polygon {
        &self.first_layer_convex_hull
    }

    /// `print_statistics()`.
    #[must_use]
    pub fn print_statistics(&self) -> &PrintStatistics {
        &self.print_statistics
    }

    /// `print_statistics()`, the non-const one.
    pub fn print_statistics_mut(&mut self) -> &mut PrintStatistics {
        &mut self.print_statistics
    }

    /// `get_filament_maps()`: the filament map's values, 1-based extruder groups.
    #[must_use]
    pub fn get_filament_maps(&self) -> Vec<i32> {
        self.config.gcode.filament_map.values.clone()
    }

    /// `get_filament_map_mode()`.
    #[must_use]
    pub fn get_filament_map_mode(&self) -> FilamentMapMode {
        self.config.gcode.filament_map_mode.value
    }

    /// `get_extruder_id(filament_id)`: the filament's group less one, as a `size_t` (a group 0
    /// wraps to `SIZE_MAX`); 0 past the map.
    #[must_use]
    pub fn get_extruder_id(&self, filament_id: u32) -> usize {
        let filament_map = self.get_filament_maps();
        let f = filament_id as usize;
        if f < filament_map.len() {
            // The int difference converted to size_t.
            return filament_map[f].wrapping_sub(1) as isize as usize;
        }
        0
    }

    /// `get_extruder_filament_info()`.
    #[must_use]
    pub fn get_extruder_filament_info(&self) -> &[Vec<Config>] {
        &self.extruder_filament_info
    }

    /// `set_extruder_filament_info(filament_info)`.
    pub fn set_extruder_filament_info(&mut self, filament_info: &[Vec<Config>]) {
        self.extruder_filament_info = filament_info.to_vec();
    }

    /// `set_geometric_unprintable_filaments(unprintables_filament_ids)`.
    pub fn set_geometric_unprintable_filaments(
        &mut self,
        unprintables_filament_ids: &[BTreeSet<i32>],
    ) {
        self.geometric_unprintable_filaments = unprintables_filament_ids.to_vec();
    }

    /// `get_geometric_unprintable_filaments()`.
    #[must_use]
    pub fn get_geometric_unprintable_filaments(&self) -> Vec<BTreeSet<i32>> {
        self.geometric_unprintable_filaments.clone()
    }

    /// `set_slice_used_filaments(first_layer_used_filaments, used_filaments)`.
    pub fn set_slice_used_filaments(
        &mut self,
        first_layer_used_filaments: &[u32],
        used_filaments: &[u32],
    ) {
        self.slice_used_filaments_first_layer = first_layer_used_filaments.to_vec();
        self.slice_used_filaments = used_filaments.to_vec();
    }

    /// `get_slice_used_filaments(first_layer)`.
    #[must_use]
    pub fn get_slice_used_filaments(&self, first_layer: bool) -> Vec<u32> {
        if first_layer {
            self.slice_used_filaments_first_layer.clone()
        } else {
            self.slice_used_filaments.clone()
        }
    }

    /// `get_extruder_printable_height()`: the extruders' printable heights.
    #[must_use]
    pub fn get_extruder_printable_height(&self) -> Vec<f64> {
        self.config.extruder_printable_height.values.clone()
    }

    /// `get_extruder_printable_polygons()`: each extruder's printable area, scaled, as one polygon.
    #[must_use]
    pub fn get_extruder_printable_polygons(&self) -> Vec<Polygons> {
        let mut extruder_printable_polys: Vec<Polygons> = Vec::new();
        let extruder_printable_areas = self.config.extruder_printable_area.values.clone();
        for e_printable_area in &extruder_printable_areas {
            let ploys: Polygons = vec![Polygon::new_scale(&vec2ds(e_printable_area))];
            extruder_printable_polys.push(ploys);
        }
        extruder_printable_polys
    }

    /// `get_extruder_unprintable_polygons()`: for each extruder, the bed's printable area less the
    /// extruder's (`diff` of the two scaled polygons).
    #[must_use]
    pub fn get_extruder_unprintable_polygons(&self) -> Vec<Polygons> {
        let printable_area = self.config.printable_area.values.clone();
        let printable_poly = Polygon::new_scale(&vec2ds(&printable_area));
        let extruder_printable_areas = self.config.extruder_printable_area.values.clone();
        let mut extruder_unprintable_polys: Vec<Polygons> = Vec::new();
        for e_printable_area in &extruder_printable_areas {
            let ploys = diff_polygon(
                &printable_poly,
                &Polygon::new_scale(&vec2ds(e_printable_area)),
                ApplySafetyOffset::No,
            );
            extruder_unprintable_polys.push(ploys);
        }
        extruder_unprintable_polys
    }

    /// `num_print_regions()`.
    #[must_use]
    pub fn num_print_regions(&self) -> usize {
        self.print_regions.len()
    }

    /// `get_print_region(idx)`.
    #[must_use]
    pub fn get_print_region(&self, idx: usize) -> &PrintRegion {
        self.print_regions[idx].get()
    }

    /// `set_plate_origin(origin)` (BBS).
    pub fn set_plate_origin(&mut self, origin: Vec3d) {
        self.origin = origin;
    }

    /// `get_plate_origin()` (BBS).
    #[must_use]
    pub fn get_plate_origin(&self) -> Vec3d {
        self.origin
    }

    /// `get_modified_count()` (BBS).
    #[must_use]
    pub fn get_modified_count(&self) -> i32 {
        self.modified_count
    }

    /// `is_support_used()` (BBS).
    #[must_use]
    pub fn is_support_used(&self) -> bool {
        self.support_used
    }

    /// `is_BBL_printer()` (SoftFever).
    #[must_use]
    pub fn is_bbl_printer(&self) -> bool {
        self.is_bbl_printer
    }

    /// `is_BBL_printer()`, the reference to set it.
    pub fn is_bbl_printer_mut(&mut self) -> &mut bool {
        &mut self.is_bbl_printer
    }

    /// `wipe_tower_type()`: type 1 on a Bambu printer, else the config's.
    #[must_use]
    pub fn wipe_tower_type(&self) -> WipeTowerType {
        if self.is_bbl_printer() {
            WipeTowerType::Type1
        } else {
            self.config.gcode.wipe_tower_type.value
        }
    }

    /// `calib_mode()`.
    #[must_use]
    pub fn calib_mode(&self) -> CalibMode {
        self.calib_params.mode
    }

    /// `calib_mode()`, the reference to set it.
    pub fn calib_mode_mut(&mut self) -> &mut CalibMode {
        &mut self.calib_params.mode
    }

    /// `calib_params()`.
    #[must_use]
    pub fn calib_params(&self) -> &CalibParams {
        &self.calib_params
    }

    /// `set_check_multi_filaments_compatibility(check)`.
    pub fn set_check_multi_filaments_compatibility(&mut self, check: bool) {
        self.need_check_multi_filaments_compatibility = check;
    }

    /// `need_check_multi_filaments_compatibility()`.
    #[must_use]
    pub fn need_check_multi_filaments_compatibility(&self) -> bool {
        self.need_check_multi_filaments_compatibility
    }

    /// `is_all_objects_are_short()`: every object lower than the nozzle's height (the scaled
    /// height compared as a `double`).
    #[must_use]
    pub fn is_all_objects_are_short(&self) -> bool {
        // coord_t compared with a double.
        self.objects
            .iter()
            .all(|obj| (obj.height() as f64) < scale_(self.config().nozzle_height.value))
    }
}

/// A point option's values (`Pointfs`, which the config holds as pairs) as `Vec2d`s.
fn vec2ds(points: &[[f64; 2]]) -> Vec<Vec2d> {
    points.iter().map(|p| Vec2d::new(p[0], p[1])).collect()
}

/// `Print::has_brim()` (Print.cpp 588-591): an object with a brim, its model object read from the
/// print's model. Belongs in orca-print (orca-o3-print-1's lines).
#[must_use]
pub fn has_brim(print: &Print) -> bool {
    print
        .objects
        .iter()
        .any(|object| object.has_brim(object.model_object(print.model())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use orca_eigen::Transform3d;

    #[test]
    fn statistics_clear_keeps_the_times() {
        let mut s = PrintStatistics::new();
        s.estimated_normal_print_time = "1h".into();
        s.total_cost = 2.;
        s.filament_stats.insert(1, 3.);
        s.clear();
        assert_eq!(s.estimated_normal_print_time, "1h");
        assert_eq!(s.total_cost, 0.);
        assert!(s.filament_stats.is_empty());
    }

    #[test]
    fn extruder_ids_wrap_as_size_t() {
        let mut p = Print::new();
        p.config.gcode.filament_map.values = vec![1, 2, 0];
        assert_eq!(p.get_extruder_id(0), 0);
        assert_eq!(p.get_extruder_id(1), 1);
        assert_eq!(p.get_extruder_id(2), usize::MAX);
        assert_eq!(p.get_extruder_id(3), 0);
        assert!(p.empty());
        assert!(p.is_all_objects_are_short());
        p.objects
            .push(PrintObject::with_members(0, Transform3d::identity()));
        assert!(!p.empty());
        assert!(p.get_object_by_id(p.objects[0].object_id()).is_some());
    }

    #[test]
    fn a_bambu_printer_takes_wipe_tower_type_1() {
        let mut p = Print::new();
        p.config.gcode.wipe_tower_type.value = WipeTowerType::Type2;
        assert_eq!(p.wipe_tower_type(), WipeTowerType::Type2);
        *p.is_bbl_printer_mut() = true;
        assert_eq!(p.wipe_tower_type(), WipeTowerType::Type1);
    }
}
