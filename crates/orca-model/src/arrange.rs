// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: get_instance_arrange_poly and get_arrange_poly of
// src/libslic3r/ModelArrange.cpp and ModelArrange.hpp (PtrWrapper), ModelInstance::
// get_arrange_polygon of Model.cpp, update_arrange_params, update_selected_items_inflation,
// update_unselected_items_inflation and get_shrink_bedpts of Arrange.cpp, get_bed_shape,
// get_shared_poly, has_skirt and get_real_skirt_dist of PrintConfig.cpp with get_bed_temp_key and
// get_bed_temp_1st_layer_key of PrintConfig.hpp, Print::get_filament_temp_type and
// Print::get_compatible_filament_type of Print.cpp, and MaterialType's table and
// get_temperature_range of MaterialType.cpp, of OrcaSlicer v2.4.2, translated on 2026-11-07;
// and get_bed_shape(const PrintConfig&, bool) (PrintConfig.cpp:11527-11542), get_bed_excluded_area
// and get_bed_shape_with_excluded_area (11546-11571), translated on 2026-11-13 by
// orca-o3-front-2. Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and the PrusaSlicer authors
// (Prusa Research), under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! What the command line does around arrangement: an instance's arrange polygon with its
//! filaments, temperatures, brim and height, the bed shrunk for the skirt, the items' inflation,
//! and the arranged position written back to the instance.
//!
//! The settings are read through [`OptionSource`] (`DynamicPrintConfig`'s `option`): an option
//! the C++ dereferences without a check (`opt_float`, `opt_int`, `support_type`) panics here when
//! it is missing or of another type, where the C++ dereferences a null pointer.
//!
//! The bed with its excluded area ([`get_bed_excluded_area`], [`get_bed_shape_with_excluded_area`],
//! with [`get_bed_shape_of`], the `PrintConfig` overload of `get_bed_shape` they call) reads the
//! print's static configuration (orca-config's `PrintConfig`), as the C++ does; orca-config
//! cannot hold them, since the shared extruder area is this module's (added by orca-o3-front-2,
//! 2026-11-13).

use std::collections::BTreeMap;

use orca_arrange::arrange::{ArrangeParams, ArrangePolygon};
use orca_config::OptionValue;
use orca_config::print_config::PrintConfig;
use orca_eigen::Vec2d;
use orca_geom::clipper_utils::{ApplySafetyOffset, diff, intersection, make_counter_clockwise};
use orca_geom::point::{Coord, Point, Points, scale_, scaled};
use orca_geom::polygon::{Polygon, Polygons};

use crate::config::ModelConfig;
use crate::model::{ModelObject, option_get_int};
use crate::transformation::{Transformation, X, Y};

/// `FilamentTempType::HighTemp`.
pub const HIGH_TEMP: i32 = 0;
/// `FilamentTempType::LowTemp`.
pub const LOW_TEMP: i32 = 1;
/// `FilamentTempType::HighLowCompatible`.
pub const HIGH_LOW_COMPATIBLE: i32 = 2;
/// `FilamentTempType::Undefine`.
pub const UNDEFINE: i32 = 3;

/// `MAX_OUTER_NOZZLE_DIAMETER`.
pub const MAX_OUTER_NOZZLE_DIAMETER: f32 = 4.0;

/// A configuration's options by key: `ConfigOptionResolver::option(key)`.
pub trait OptionSource {
    /// The option, or `None` when the configuration has none of that key.
    fn option(&self, key: &str) -> Option<&OptionValue>;
}

impl OptionSource for ModelConfig {
    fn option(&self, key: &str) -> Option<&OptionValue> {
        ModelConfig::option(self, key)
    }
}

impl OptionSource for BTreeMap<String, OptionValue> {
    fn option(&self, key: &str) -> Option<&OptionValue> {
        self.get(key)
    }
}

/// The configuration as orca-model's arrangement reads one (`ConfigOptionResolver::option`).
impl OptionSource for orca_config::config::Config {
    fn option(&self, key: &str) -> Option<&OptionValue> {
        self.get(key)
    }
}

/// `opt_float(key)` of a const configuration: the `value` of a float, a percent or a float or
/// percent (`dynamic_cast<const ConfigOptionFloat*>` takes the derived ones).
fn opt_float(cfg: &dyn OptionSource, key: &str) -> f64 {
    match cfg.option(key) {
        Some(OptionValue::Float(v) | OptionValue::Percent(v)) => *v,
        Some(OptionValue::FloatOrPercent { value, .. }) => *value,
        other => panic!("opt_float(\"{key}\") of {other:?}: the C++ dereferences a null pointer"),
    }
}

/// `opt_int(key)` of a const configuration.
fn opt_int(cfg: &dyn OptionSource, key: &str) -> i32 {
    match cfg.option(key) {
        Some(OptionValue::Int(v)) => *v,
        other => panic!("opt_int(\"{key}\") of {other:?}: the C++ dereferences a null pointer"),
    }
}

/// `opt_int(key, idx)` of a const configuration: `ConfigOptionInts::get_at(idx)` (the first value
/// past the end).
fn opt_int_at(cfg: &dyn OptionSource, key: &str, idx: usize) -> i32 {
    match cfg.option(key) {
        Some(OptionValue::Ints {
            values,
            nullable: false,
        }) => ints_get_at(values, idx),
        other => {
            panic!("opt_int(\"{key}\", {idx}) of {other:?}: the C++ dereferences a null pointer")
        }
    }
}

/// `ConfigOptionVector::get_at(i)`: the value at `i`, or the first past the end.
fn ints_get_at(values: &[i32], idx: usize) -> i32 {
    if idx < values.len() {
        values[idx]
    } else {
        values[0]
    }
}

/// `option<ConfigOptionInts>(key)` of a const configuration: exactly that type.
fn option_ints<'a>(cfg: &'a dyn OptionSource, key: &str) -> Option<&'a Vec<i32>> {
    match cfg.option(key) {
        Some(OptionValue::Ints {
            values,
            nullable: false,
        }) => Some(values),
        _ => None,
    }
}

/// `option<ConfigOptionBool>(key)` of a const configuration.
fn option_bool(o: Option<&OptionValue>) -> Option<bool> {
    match o {
        Some(OptionValue::Bool(b)) => Some(*b),
        _ => None,
    }
}

/// `option<ConfigOptionInt>(key)` of a const configuration.
fn option_int(o: Option<&OptionValue>) -> Option<i32> {
    match o {
        Some(OptionValue::Int(v)) => Some(*v),
        _ => None,
    }
}

/// `MaterialType::all()`: the name, the lowest and the highest printing temperature.
const MATERIAL_TYPES: &[(&str, i32, i32)] = &[
    ("ABS", 190, 300),
    ("ABS-CF", 220, 300),
    ("ABS-GF", 240, 280),
    ("ASA", 220, 300),
    ("ASA-CF", 230, 300),
    ("ASA-GF", 240, 300),
    ("ASA-AERO", 240, 280),
    ("BVOH", 190, 240),
    ("CoPE", 190, 240),
    ("EVA", 175, 220),
    ("FLEX", 210, 230),
    ("HIPS", 220, 270),
    ("PA", 235, 280),
    ("PA-CF", 240, 315),
    ("PA-GF", 240, 290),
    ("PA6", 260, 300),
    ("PA6-CF", 230, 300),
    ("PA6-GF", 260, 300),
    ("PA11", 275, 295),
    ("PA11-CF", 275, 295),
    ("PA11-GF", 275, 295),
    ("PA12", 250, 270),
    ("PA12-CF", 250, 300),
    ("PA12-GF", 255, 270),
    ("PAHT", 260, 310),
    ("PAHT-CF", 270, 310),
    ("PAHT-GF", 270, 310),
    ("PC", 240, 300),
    ("PC-ABS", 230, 270),
    ("PC-CF", 270, 295),
    ("PC-PBT", 260, 300),
    ("PCL", 130, 170),
    ("PCTG", 220, 300),
    ("PE", 175, 260),
    ("PE-CF", 175, 260),
    ("PE-GF", 230, 270),
    ("PEI-1010", 370, 430),
    ("PEI-1010-CF", 380, 430),
    ("PEI-1010-GF", 380, 430),
    ("PEI-9085", 350, 390),
    ("PEI-9085-CF", 365, 390),
    ("PEI-9085-GF", 370, 390),
    ("PEEK", 350, 460),
    ("PEEK-CF", 380, 410),
    ("PEEK-GF", 375, 410),
    ("PEKK", 325, 400),
    ("PEKK-CF", 360, 400),
    ("PES", 340, 390),
    ("PET", 200, 290),
    ("PET-CF", 240, 320),
    ("PET-GF", 280, 320),
    ("PETG", 190, 260),
    ("PETG-CF", 230, 290),
    ("PETG-GF", 210, 270),
    ("PHA", 190, 250),
    ("PI", 390, 410),
    ("PLA", 180, 240),
    ("PLA-AERO", 220, 270),
    ("PLA-CF", 190, 250),
    ("POM", 210, 250),
    ("PP", 200, 240),
    ("PP-CF", 210, 250),
    ("PP-GF", 220, 260),
    ("PPA-CF", 260, 300),
    ("PPA-GF", 260, 290),
    ("PPS", 300, 345),
    ("PPS-CF", 295, 350),
    ("PPSU", 360, 420),
    ("PSU", 350, 380),
    ("PVA", 185, 250),
    ("PVB", 190, 250),
    ("PVDF", 245, 265),
    ("SBS", 195, 250),
    ("TPI", 420, 445),
    ("TPU", 175, 260),
];

/// `MaterialType::get_temperature_range(type)`: the table's range, or 190 to 300 and `false` for
/// a name it does not hold (compared exactly).
#[must_use]
pub fn get_temperature_range(filament_type: &str) -> (bool, i32, i32) {
    match MATERIAL_TYPES.iter().find(|m| m.0 == filament_type) {
        Some(&(_, lo, hi)) => (true, lo, hi),
        None => (false, 190, 300),
    }
}

/// `Print::get_filament_temp_type(filament_type)`: low up to 250 degrees, compatible below 280,
/// high from there, `Undefine` for an unknown material.
#[must_use]
pub fn get_filament_temp_type(filament_type: &str) -> i32 {
    let (known, _, max_temp) = get_temperature_range(filament_type);
    if known {
        if max_temp <= 250 {
            LOW_TEMP
        } else if max_temp < 280 {
            HIGH_LOW_COMPATIBLE
        } else {
            HIGH_TEMP
        }
    } else {
        UNDEFINE
    }
}

/// `Print::get_compatible_filament_type(filament_types)`.
#[must_use]
pub fn get_compatible_filament_type(filament_types: &[i32]) -> i32 {
    let high = filament_types.contains(&HIGH_TEMP);
    let low = filament_types.contains(&LOW_TEMP);
    if high && low {
        HIGH_LOW_COMPATIBLE
    } else if high {
        HIGH_TEMP
    } else if low {
        LOW_TEMP
    } else {
        HIGH_LOW_COMPATIBLE
    }
}

/// `get_bed_temp_key(type)` of a `BedType` value ("" for the default and unknown ones).
#[must_use]
pub fn get_bed_temp_key(bed_type: i32) -> &'static str {
    match bed_type {
        6 => "supertack_plate_temp",
        1 => "cool_plate_temp",
        5 => "textured_cool_plate_temp",
        2 => "eng_plate_temp",
        3 => "hot_plate_temp",
        4 => "textured_plate_temp",
        _ => "",
    }
}

/// `get_bed_temp_1st_layer_key(type)`.
#[must_use]
pub fn get_bed_temp_1st_layer_key(bed_type: i32) -> &'static str {
    match bed_type {
        6 => "supertack_plate_temp_initial_layer",
        1 => "cool_plate_temp_initial_layer",
        5 => "textured_cool_plate_temp_initial_layer",
        2 => "eng_plate_temp_initial_layer",
        3 => "hot_plate_temp_initial_layer",
        4 => "textured_plate_temp_initial_layer",
        _ => "",
    }
}

/// `ModelInstance::get_arrange_polygon(ap, config_global)` of the instance: the object's 2D hull
/// through the instance without its z rotation and with only its z offset, the instance's x and
/// y offset scaled as the translation, its z rotation, and the filaments of its parts and
/// supports (consecutive repeats removed; 1 when there are none).
pub fn get_arrange_polygon(
    object: &mut ModelObject,
    instance_idx: usize,
    config_global: &dyn OptionSource,
) -> ArrangePolygon {
    let inst = &object.instances[instance_idx];
    let mut rotation = inst.get_rotation();
    rotation[2] = 0.0;
    let mut t: Transformation = *inst.get_transformation();
    t.set_offset(&(orca_eigen::Vec3d::unit_z() * inst.get_offset().z()));
    t.set_rotation(&rotation);
    let translation = Point::new(
        scaled(inst.get_offset_axis(X)),
        scaled(inst.get_offset_axis(Y)),
    );
    let rot_z = inst.get_rotation_axis(2);
    let p = object.convex_hull_2d(t.get_matrix());
    let mut ret = ArrangePolygon::default();
    ret.poly.contour = p;
    ret.translation = translation;
    ret.rotation = rot_z;
    for v in &object.volumes {
        if v.is_model_part() {
            ret.extrude_ids.extend(v.get_extruders(&object.config));
        }
    }
    let is_support_enabled =
        option_bool(object.get_config_value(config_global, "enable_support")).unwrap_or(false);
    if is_support_enabled {
        let op1 = option_int(object.get_config_value(config_global, "support_filament"));
        let op2 = option_int(object.get_config_value(config_global, "support_interface_filament"));
        if let Some(id) = op1
            && id > 0
        {
            ret.extrude_ids.push(id);
        }
        if let Some(id) = op2
            && id > 0
        {
            ret.extrude_ids.push(id);
        }
    }
    orca_stl::select::unique_vec(&mut ret.extrude_ids);
    if ret.extrude_ids.is_empty() {
        ret.extrude_ids.push(1);
    }
    ret
}

/// `get_instance_arrange_poly(instance, config)`: [`get_arrange_polygon`] on bed 0, with the bed
/// and nozzle temperatures of the first filament, the filaments' temperature class, the brim
/// width (1, 6 with normal supports, 24 with tree supports), the height of the instance's hull
/// and the object's name.
pub fn get_instance_arrange_poly(
    object: &mut ModelObject,
    instance_idx: usize,
    config: &dyn OptionSource,
) -> ArrangePolygon {
    let mut ap = get_arrange_polygon(object, instance_idx, config);
    ap.bed_idx = 0;
    let first = (ap.extrude_ids[0] - 1) as u32 as usize;
    if let Some(bed_type) = config.option("curr_bed_type") {
        ap.bed_temp = 0;
        ap.first_bed_temp = 0;
        let curr_bed_type = option_get_int(bed_type);
        if let Some(bed) = option_ints(config, get_bed_temp_key(curr_bed_type)) {
            ap.bed_temp = ints_get_at(bed, first);
        }
        if let Some(bed) = option_ints(config, get_bed_temp_1st_layer_key(curr_bed_type)) {
            ap.first_bed_temp = ints_get_at(bed, first);
        }
    }
    if config.option("nozzle_temperature").is_some() {
        ap.print_temp = opt_int_at(config, "nozzle_temperature", first);
    }
    if config.option("nozzle_temperature_initial_layer").is_some() {
        ap.first_print_temp = opt_int_at(config, "nozzle_temperature_initial_layer", first);
    }
    if config.option("temperature_vitrification").is_some() {
        ap.vitrify_temp = opt_int_at(config, "temperature_vitrification", first);
    }
    if let Some(OptionValue::Strings(types)) = config.option("filament_type") {
        let mut temp_types: Vec<i32> = Vec::new();
        for &i in &ap.extrude_ids {
            // `get_at(i - 1)`: the int widened to size_t, so -1 is past the end.
            let idx = (i - 1) as isize as usize;
            let type_str = if idx < types.len() {
                &types[idx]
            } else {
                &types[0]
            };
            let temp_type = get_filament_temp_type(type_str);
            if !temp_types.contains(&temp_type) {
                temp_types.push(temp_type);
            }
        }
        ap.filament_temp_type = get_compatible_filament_type(&temp_types);
    }
    ap.brim_width = 1.0;
    let enable_support = match object.get_config_value(config, "enable_support") {
        Some(o) => option_get_int(o) != 0,
        None => panic!("enable_support is missing: the C++ dereferences a null pointer"),
    };
    let support_type = match object.get_config_value(config, "support_type") {
        Some(o) => option_get_int(o),
        None => panic!("support_type is missing: the C++ dereferences a null pointer"),
    };
    // stNormalAuto 0, stTreeAuto 1, stNormal 2, stTree 3.
    if enable_support && (support_type == 0 || support_type == 2) {
        ap.brim_width = 6.0;
    } else if enable_support {
        ap.brim_width = 24.0;
        ap.has_tree_support = true;
    }
    let size = object
        .instance_convex_hull_bounding_box(instance_idx, false)
        .size();
    ap.height = size.z();
    ap.name = object.name.clone();
    ap
}

/// `ArrangePolygon::apply()` with the setter `get_arrange_poly` gives an instance: once, and only
/// when the item was arranged, the result written to the instance (`apply_arrange_result`) and
/// its `arrange_order` set to the item's id.
pub fn apply_arrange_polygon(
    object: &mut ModelObject,
    instance_idx: usize,
    ap: &mut ArrangePolygon,
) {
    if ap.is_applied != 0 {
        return;
    }
    if ap.is_arranged() {
        let t = (ap.translation.x as f64, ap.translation.y as f64);
        object.apply_arrange_result(instance_idx, t, ap.rotation);
        object.instances[instance_idx].arrange_order = ap.itemid;
    }
    ap.is_applied = 1;
}

/// `to_points(pointfs)`: each point scaled and truncated.
fn to_points(pts: &[Vec2d]) -> Points {
    pts.iter()
        .map(|v| {
            Point::new(
                orca_crt::f64_to_i64(scale_(v.x())),
                orca_crt::f64_to_i64(scale_(v.y())),
            )
        })
        .collect()
}

/// `get_shared_poly(extruder_polys)`: the intersection of the extruders' areas (the first polygon
/// of each intersection).
fn get_shared_poly(extruder_polys: &[Vec<[f64; 2]>]) -> Polygon {
    let mut result = Polygon::default();
    for (index, area) in extruder_polys.iter().enumerate() {
        let pts: Vec<Vec2d> = area.iter().map(|p| Vec2d::new(p[0], p[1])).collect();
        if index == 0 {
            result.points = to_points(&pts);
        } else {
            let poly = Polygon::new(to_points(&pts));
            let r = intersection(&[poly], &[result.clone()], ApplySafetyOffset::No);
            // `result_polygon[0]` of an empty intersection is undefined in the C++.
            result = r.into_iter().next().unwrap_or_default();
        }
    }
    result
}

/// `get_bed_shape(config, use_share)`: the scaled bed outline (counter-clockwise), the shared area
/// of the extruders when they have areas, the bed's centre alone when it has no outline.
#[must_use]
pub fn get_bed_shape(config: &dyn OptionSource, use_share: bool) -> Points {
    let Some(OptionValue::Points(bed)) = config.option("printable_area") else {
        if let Some(OptionValue::Point(c)) = config.option("center") {
            return vec![Point::new(scaled(c[0]), scaled(c[1]))];
        }
        return Vec::new();
    };
    let bed: Vec<Vec2d> = bed.iter().map(|p| Vec2d::new(p[0], p[1])).collect();
    if use_share
        && let Some(OptionValue::PointsGroups(areas)) = config.option("extruder_printable_area")
        && !areas.is_empty()
    {
        return get_shared_poly(areas).points;
    }
    to_points(&make_counter_clockwise(&bed))
}

/// `get_bed_shape(const PrintConfig& cfg, use_share)` (`PrintConfig.cpp:11527-11542`), on the
/// static configuration: the extruders' shared area when `use_share` and they have areas, else
/// the bed outline made counter-clockwise, scaled and truncated.
#[must_use]
pub fn get_bed_shape_of(cfg: &PrintConfig, use_share: bool) -> Points {
    let mut bed_poly = Polygon::default();
    let extruder_areas = &cfg.extruder_printable_area.values;
    if use_share && !extruder_areas.is_empty() {
        bed_poly = get_shared_poly(extruder_areas);
    } else {
        let bed: Vec<Vec2d> = cfg
            .printable_area
            .values
            .iter()
            .map(|p| Vec2d::new(p[0], p[1]))
            .collect();
        bed_poly.points = to_points(&make_counter_clockwise(&bed));
    }
    bed_poly.points
}

/// `get_bed_excluded_area(cfg)` (`PrintConfig.cpp:11546-11559`): one polygon, the excluded area's
/// points scaled and rounded (`Point(double, double)`), made counter-clockwise; an empty polygon
/// when the area has no points.
#[must_use]
pub fn get_bed_excluded_area(cfg: &PrintConfig) -> Polygons {
    let exclude_area_points = &cfg.bed_exclude_area.values;

    let mut exclude_poly = Polygon::default();
    for pt in exclude_area_points {
        exclude_poly
            .points
            .push(Point::from_f64(scale_(pt[0]), scale_(pt[1])));
    }

    exclude_poly.make_counter_clockwise();

    vec![exclude_poly]
}

/// `get_bed_shape_with_excluded_area(cfg, use_share)` (`PrintConfig.cpp:11561-11571`): the bed
/// (`get_bed_shape`) less the excluded area, the first polygon Clipper gives, or the bed itself
/// when the difference is empty.
#[must_use]
pub fn get_bed_shape_with_excluded_area(cfg: &PrintConfig, use_share: bool) -> Polygon {
    let mut bed_poly = Polygon::new(get_bed_shape_of(cfg, use_share));

    let exclude_polys = get_bed_excluded_area(cfg);
    let tmp = diff(
        std::slice::from_ref(&bed_poly),
        &exclude_polys,
        ApplySafetyOffset::No,
    );
    if let Some(first) = tmp.into_iter().next() {
        bed_poly = first;
    }
    bed_poly
}

/// `has_skirt(cfg)`: skirt layers and loops, or a draft shield.
fn has_skirt(cfg: &dyn OptionSource) -> bool {
    let h = cfg.option("skirt_height");
    let l = cfg.option("skirt_loops");
    let d = cfg.option("draft_shield");
    (h.is_some_and(|o| option_get_int(o) > 0) && l.is_some_and(|o| option_get_int(o) > 0))
        || d.is_some_and(|o| option_get_int(o) != 0)
}

/// `get_real_skirt_dist(cfg)`: the skirt's distance plus its loops (one for a draft shield) times
/// the first layer's line width (or the line width, or the largest nozzle, or 0.4), in floats.
#[must_use]
pub fn get_real_skirt_dist(cfg: &dyn OptionSource) -> f32 {
    if !has_skirt(cfg) {
        return 0.0;
    }
    let dist = opt_float(cfg, "skirt_distance") as f32;
    let mut loops = opt_int(cfg, "skirt_loops");
    if cfg
        .option("draft_shield")
        .is_some_and(|o| option_get_int(o) != 0)
        && loops == 0
    {
        loops = 1;
    }
    let mut width = opt_float(cfg, "initial_layer_line_width") as f32;
    if width <= 0.0 {
        width = opt_float(cfg, "line_width") as f32;
    }
    if width <= 0.0 {
        match cfg.option("nozzle_diameter") {
            Some(OptionValue::Floats {
                values,
                nullable: false,
            }) if !values.is_empty() => {
                let mut m = values[0];
                for &v in &values[1..] {
                    if m < v {
                        m = v;
                    }
                }
                width = m as f32;
            }
            _ => width = 0.4,
        }
    }
    dist + loops as f32 * width
}

/// `update_arrange_params(params, print_cfg, selected)`: the bed shrunk by the skirt, and grown
/// back by half the clearance radius for a sequential print.
pub fn update_arrange_params(params: &mut ArrangeParams, print_cfg: &dyn OptionSource) {
    let skirt_distance = f64::from(get_real_skirt_dist(print_cfg));
    params.brim_skirt_distance = skirt_distance as f32;
    params.bed_shrink_x += params.brim_skirt_distance;
    params.bed_shrink_y += params.brim_skirt_distance;
    if params.is_seq_print {
        params.bed_shrink_x -= params.clearance_radius / 2.0;
        params.bed_shrink_y -= params.clearance_radius / 2.0;
    }
}

/// `get_shrink_bedpts(print_cfg, params)`: the bed outline with each point moved toward the
/// centre by the shrink, axis by axis.
#[must_use]
pub fn get_shrink_bedpts(print_cfg: &dyn OptionSource, params: &ArrangeParams) -> Points {
    let mut bedpts = get_bed_shape(print_cfg, true);
    let shrink = |bedpts: &mut Points, dist: f64, direction: usize| {
        let center = Polygon::new(bedpts.clone()).bounding_box().center();
        for pt in bedpts.iter_mut() {
            let (c, p) = if direction == 0 {
                (center.x, pt.x)
            } else {
                (center.y, pt.y)
            };
            let sgn = if c - p >= 0 { 1.0 } else { -1.0 };
            let moved = orca_crt::f64_to_i64(p as f64 + dist * sgn);
            if direction == 0 {
                pt.x = moved;
            } else {
                pt.y = moved;
            }
        }
    };
    shrink(&mut bedpts, scaled_f32(params.bed_shrink_x) as f64, 0);
    shrink(&mut bedpts, scaled_f32(params.bed_shrink_y) as f64, 1);
    bedpts
}

/// `scaled(float)`: `coord_t(v / float(SCALING_FACTOR))`, the division in floats.
fn scaled_f32(v: f32) -> Coord {
    orca_crt::f32_to_i64(v / (orca_geom::point::SCALING_FACTOR as f32))
}

/// `update_selected_items_inflation(selected, print_cfg, params)`: each item's inflation (half the
/// object distance when one is set, else half the largest brim when an item has tree supports,
/// else its brim), capped by half the room the bed leaves around it; for a sequential print the
/// object distance is raised first.
pub fn update_selected_items_inflation(
    selected: &mut [ArrangePolygon],
    print_cfg: &dyn OptionSource,
    params: &mut ArrangeParams,
) {
    let bedpts = get_shrink_bedpts(print_cfg, params);
    let bedbb = Polygon::new(bedpts).bounding_box();
    if params.is_seq_print {
        let all_short = selected
            .iter()
            .all(|ap| ap.height < f64::from(params.nozzle_height));
        if all_short {
            let m = orca_stl::max(
                MAX_OUTER_NOZZLE_DIAMETER / 2.0,
                params.object_skirt_offset * 2.0,
            );
            params.min_obj_distance =
                orca_stl::max(params.min_obj_distance, scaled(f64::from(m) + 0.001));
        } else {
            params.min_obj_distance = orca_stl::max(
                params.min_obj_distance,
                scaled(f64::from(params.clearance_radius) + 0.001),
            );
        }
    }
    let mut brim_max = 0.0;
    let mut plate_has_tree_support = false;
    for ap in selected.iter() {
        brim_max = orca_stl::max(brim_max, ap.brim_width);
        if ap.has_tree_support {
            plate_has_tree_support = true;
        }
    }
    for ap in selected.iter_mut() {
        ap.inflation = if params.min_obj_distance != 0 {
            params.min_obj_distance / 2
        } else if plate_has_tree_support {
            scaled(brim_max / 2.0)
        } else {
            scaled(ap.brim_width)
        };
        let apbb = ap.poly.contour.bounding_box();
        let diffx = bedbb.size().x - apbb.size().x - 5;
        let diffy = bedbb.size().y - apbb.size().y - 5;
        if diffx > 0 && diffy > 0 {
            let min_diff = orca_stl::min(diffx, diffy);
            ap.inflation = orca_stl::min(min_diff / 2, ap.inflation);
        }
    }
}

/// `update_unselected_items_inflation(unselected, print_cfg, params)`: a fixed item's inflation is
/// its brim (or half the object distance); an excluded region's the exclusion gap, none for an
/// extrusion calibration region, moved by the bed's shrink for a sequential print.
pub fn update_unselected_items_inflation(
    unselected: &mut [ArrangePolygon],
    params: &ArrangeParams,
) {
    let mut exclusion_gap: f32 = 1.0;
    if params.is_seq_print {
        exclusion_gap = orca_stl::max(
            exclusion_gap,
            params.clearance_radius / 2.0 + params.bed_shrink_x + 1.0,
        );
        for region in unselected.iter_mut() {
            if region.is_virt_object {
                region.poly.translate_f64(
                    scaled_f32(params.bed_shrink_x) as f64,
                    scaled_f32(params.bed_shrink_y) as f64,
                );
            }
        }
    }
    for ap in unselected.iter_mut() {
        ap.inflation = if !ap.is_virt_object {
            if params.min_obj_distance == 0 {
                scaled(ap.brim_width)
            } else {
                params.min_obj_distance / 2
            }
        } else if ap.is_extrusion_cali_object {
            0
        } else {
            orca_crt::f64_to_i64(scale_(f64::from(exclusion_gap)))
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filament_temperature_classes() {
        assert_eq!(get_filament_temp_type("PLA"), LOW_TEMP);
        assert_eq!(get_filament_temp_type("PETG"), HIGH_LOW_COMPATIBLE);
        assert_eq!(get_filament_temp_type("ABS"), HIGH_TEMP);
        assert_eq!(get_filament_temp_type("pla"), UNDEFINE);
        assert_eq!(
            get_compatible_filament_type(&[LOW_TEMP, HIGH_TEMP]),
            HIGH_LOW_COMPATIBLE
        );
        assert_eq!(
            get_compatible_filament_type(&[UNDEFINE]),
            HIGH_LOW_COMPATIBLE
        );
    }
}
