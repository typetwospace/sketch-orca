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
// Carries no OrcaSlicer code: it replays records the C++ engine's probe wrote
// (parity/oracle/probe/layer/flow.cpp, extrusion.cpp, surface.cpp and print.cpp). Written on
// 2026-11-13 (orca-o3-layer-1); the layer-2 records added on 2026-11-14 (orca-o3-layer-2), the
// layer-3 records the same day (orca-o3-layer-3).

//! The probe gate for orca-layer: the layer-1, layer-2 and layer-3 records of `layer.sop1`
//! (`parity/sop1/layer.md`), from the committed subset (`parity/probes/`), or from the full set
//! when `SKETCH_ORCA_PROBES_DEEP` names its folder.
//!
//! Each record holds its inputs, then the engine's results. The replay decodes the inputs (an
//! entity input is encoded again and must give its bytes back, which proves the decoder), runs the
//! translation, encodes each result as the probe's writers do (`common.hpp`) and compares the bytes
//! part by part; the first part that differs ends the record's replay (the bytes after it no
//! longer line up) and is reported.
//!
//! - LYFL, LYFS, LYFW: Flow, its helpers and `extrusion_width`.
//! - LYRO: the roles. LYEN, LYEM, LYEL, LYES: a path, a multi path, a loop, a sloped loop.
//! - LYEC: a collection, `chained_path_from` included (layer-3's chaining). The gap fill path
//!   `replace` puts in comes from the probe's generator, not the inputs: the replay takes it from
//!   the recorded result.
//! - LYEL's `has_overhang_point` is of a polygon vertex the record does not hold: when the
//!   record's point v is a vertex (the probe's case 0) the replay compares the answer for it,
//!   else it requires some vertex to give the recorded answer.
//! - LYEA: the append helpers. LYVW: variable width.
//! - LYSU (layer-2): a list of surfaces, each surface's predicates, area, box, polygons and
//!   colour, the list's box, polygons, islands and polygon count, and `surfaces_could_merge` of
//!   every ordered pair; the last LYSU record holds the colour names of `SurfaceType(-1)`,
//!   `stCount` and `stInternalAfterExternalBridge` alone.
//! - LYSC (layer-2): `SurfaceCollection`'s operations, each on a collection of the list as given.
//!   A `SurfacesPtr` is written as the surfaces' indices in the collection.
//! - LYCH (layer-3): `chain_extrusion_entities` and `reorder_extrusion_entities`,
//!   `chain_and_reorder_extrusion_paths`, and `chained_path_from` of a collection holding entities
//!   without ends.
//! - LYPC (layer-3): a print case. The replay builds the print `Print::apply` leaves
//!   (orca-o3-print-2's, not translated yet) from the case: the configs from the full config
//!   normalised as apply normalises it and the model objects' settings over them (checked against
//!   the recorded hashes of the print's, each object's and each region's config), one print object
//!   per model object and instance scale in `std::set` order, the instances' shifts from the
//!   record. It compares `chain_print_object_instances` and `has_brim()`. Apply's status and the
//!   objects' sizes and extruders are print-2's results: read and counted, not compared.
//! - LYPR, LYSF, LYPH, LYPT, LYLA (layer-3), on the print of their case: a region's extruders,
//!   flows, nozzle averages and printing extruders; the support flows; PrintBase's object
//!   placeholders and output file names; Print's filament and extruder accessors; a layer with
//!   regions of its own and what Layer and LayerRegion compute of it. A layer's regions are
//!   appended to the object's printing regions for its replay. An output file name of a format
//!   that is not empty runs `PlaceholderParser::process`, orca-gcode's (phase O4): the translation
//!   stops there with `NotPorted`, and the replay skips the recorded result and counts it.
//!
//! The file's other kinds are orca-config's (front-1); any kind outside the two lists fails the
//! test, as does a record count other than the probe's.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::Arc;

use orca_config::config::{
    Config, normalize_fdm, update_values_to_printer_extruders,
    update_values_to_printer_extruders_for_multiple_filaments,
};
use orca_config::print_config::FuzzySkinType;
use orca_config::{
    ConfigBase, ConfigOptionFloatOrPercent, OptionValue, PrintObjectConfig, PrintRegionConfig,
    StaticPrintConfig, full_print_config,
};
use orca_eigen::{Transform3d, Vec3crd, Vec3d};
use orca_geom::arc_fitter::{EMovePathType, PathFittingData};
use orca_geom::bounding_box::BoundingBox;
use orca_geom::circle::{ArcDirection, ArcSegment};
use orca_geom::expolygon::ExPolygon;
use orca_geom::point::{Point, Points, scale_, scaled_f64};
use orca_geom::point3::Point3;
use orca_geom::polygon::Polygon;
use orca_geom::polyline::{Polyline, Polylines, ThickPolyline};
use orca_geom::polyline3::Polyline3;
use orca_index::stop::NotPorted;
use orca_layer::chain::{
    chain_and_reorder_extrusion_paths, chain_extrusion_entities,
    chain_print_object_instances_of_print, reorder_extrusion_entities,
};
use orca_layer::collection::{ExtrusionEntityCollection, filter_by_extrusion_role};
use orca_layer::entity::{
    ExtrusionEntitiesPtr, ExtrusionEntity, RuntimeError, extrusion_entities_append_loops,
    extrusion_entities_append_loops_and_paths, extrusion_entities_append_paths,
    extrusion_entities_append_paths_with_wipe, extrusion_paths_append,
};
use orca_layer::extrusion_loop::{ExtrusionLoop, ExtrusionLoopSloped};
use orca_layer::flow::{
    Flow, FlowError, FlowRole, support_material_1st_layer_flow, support_material_flow,
    support_material_interface_flow, support_transition_flow,
};
use orca_layer::layer::{Layer, get_extents_layer_region, get_extents_layer_regions};
use orca_layer::multi_path::ExtrusionMultiPath;
use orca_layer::path::{
    ExtrusionPath, ExtrusionPathContoured, ExtrusionPathOriented, ExtrusionPathSloped,
    ExtrusionPaths, Slope,
};
use orca_layer::print::{Print, has_brim};
use orca_layer::print_base::PlaceholderParserError;
use orca_layer::print_object::{PrintInstance, PrintObject, PrintObjectRegions};
use orca_layer::print_region::PrintRegion;
use orca_layer::role::{
    ExtrusionLoopRole, ExtrusionRole, is_bridge, is_external_perimeter, is_infill,
    is_internal_perimeter, is_perimeter, is_solid_infill, is_top_surface, role_to_string,
    string_to_role,
};
use orca_layer::surface::{
    ST_COUNT, Surface, SurfaceType, get_extents, get_extents_surfaces, number_polygons,
    surface_type_to_color_name, surface_type_value_to_color_name, surfaces_could_merge,
    to_expolygons, to_polygons, to_polygons_surfaces,
};
use orca_layer::surface_collection::SurfaceCollection;
use orca_layer::variable_width::{thick_polyline_to_multi_path, variable_width};
use orca_model::model::{Model, ModelInstance, ModelObject};
use orca_probe::{Error, Mismatches, ProbeSet, Reader, load_probe};

/// The kinds this test replays, with their record counts in the committed and the full set.
const OWN: &[(&str, usize, usize)] = &[
    ("LYFL", 57, 1517),
    ("LYFS", 40, 400),
    ("LYFW", 11, 300),
    ("LYRO", 1, 1),
    ("LYEN", 7, 600),
    ("LYEM", 4, 300),
    ("LYEL", 4, 400),
    ("LYES", 2, 200),
    ("LYEC", 2, 300),
    ("LYEA", 15, 200),
    ("LYVW", 6, 400),
    ("LYSU", 14, 301),
    ("LYSC", 9, 300),
    ("LYCH", 15, 200),
    ("LYPC", 10, 10),
    ("LYPR", 13, 13),
    ("LYSF", 13, 13),
    ("LYPH", 10, 10),
    ("LYPT", 10, 10),
    ("LYLA", 7, 190),
];

/// The kinds of `layer.sop1` that orca-config replays (front-1).
const ORCA_CONFIG: &[&str] = &[
    "LYCK", "LYCA", "LYCD", "LYCO", "LYDY", "LYAM", "LYFO", "LYSK", "LYMT",
];

/// The function of the stop an output file name of a format reaches (orca-gcode, phase O4).
const PLACEHOLDER_PROCESS: &str = "PlaceholderParser::process";

// ---- Writing, as common.hpp's writers --------------------------------------------------------

/// An error's `what()`.
trait What {
    fn what_text(&self) -> String;
}

impl What for FlowError {
    fn what_text(&self) -> String {
        self.what()
    }
}

impl What for RuntimeError {
    fn what_text(&self) -> String {
        self.what()
    }
}

impl What for PlaceholderParserError {
    fn what_text(&self) -> String {
        self.what()
    }
}

/// A payload, written as the probe's `Payload` writes it.
#[derive(Default)]
struct W(Vec<u8>);

impl W {
    fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }
    fn boolean(&mut self, v: bool) -> &mut Self {
        self.u8(u8::from(v))
    }
    fn u16(&mut self, v: u16) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn expolygon(&mut self, e: &ExPolygon) -> &mut Self {
        self.points(&e.contour.points).polygons(&e.holes)
    }
    fn expolygons(&mut self, v: &[ExPolygon]) -> &mut Self {
        self.len(v.len());
        for e in v {
            self.expolygon(e);
        }
        self
    }
    fn bbox(&mut self, b: &BoundingBox) -> &mut Self {
        self.point(b.min).point(b.max).boolean(b.defined)
    }
    fn surface(&mut self, s: &Surface) -> &mut Self {
        self.u32(s.surface_type as u32).expolygon(&s.expolygon);
        self.f64(s.thickness).u16(s.thickness_layers);
        self.f64(s.bridge_angle).u16(s.extra_perimeters)
    }
    fn surfaces(&mut self, v: &[Surface]) -> &mut Self {
        self.len(v.len());
        for s in v {
            self.surface(s);
        }
        self
    }
    /// A `SurfacesPtr` as the surfaces' indices in the list that owns them.
    fn surface_ptrs(&mut self, all: &[Surface], v: &[&Surface]) -> &mut Self {
        self.len(v.len());
        for p in v {
            let i = all.iter().position(|s| core::ptr::eq(s, *p)).unwrap();
            self.len(i);
        }
        self
    }
    fn i32(&mut self, v: i32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn u64(&mut self, v: u64) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn len(&mut self, n: usize) -> &mut Self {
        self.u64(n as u64)
    }
    fn i64(&mut self, v: i64) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn f32(&mut self, v: f32) -> &mut Self {
        self.u32(v.to_bits())
    }
    fn f64(&mut self, v: f64) -> &mut Self {
        self.u64(v.to_bits())
    }
    fn str(&mut self, s: &str) -> &mut Self {
        self.u32(u32::try_from(s.len()).unwrap());
        self.0.extend_from_slice(s.as_bytes());
        self
    }
    fn point(&mut self, p: Point) -> &mut Self {
        self.i64(p.x).i64(p.y)
    }
    fn points(&mut self, pts: &[Point]) -> &mut Self {
        self.len(pts.len());
        for &p in pts {
            self.point(p);
        }
        self
    }
    fn polygons(&mut self, polys: &[Polygon]) -> &mut Self {
        self.len(polys.len());
        for p in polys {
            self.points(&p.points);
        }
        self
    }
    fn fitting(&mut self, fitting: &[PathFittingData]) -> &mut Self {
        self.len(fitting.len());
        for f in fitting {
            self.len(f.start_point_index).len(f.end_point_index);
            self.u8(f.path_type as u8);
            let arc = &f.arc_data;
            self.boolean(arc.is_arc).point(arc.center);
            self.f64(arc.radius).f64(arc.length).f64(arc.angle_radians);
            self.f64(arc.polar_start_theta).f64(arc.polar_end_theta);
            self.point(arc.start_point).point(arc.end_point);
            self.u8(arc.direction as u8);
        }
        self
    }
    fn polyline(&mut self, pl: &Polyline) -> &mut Self {
        self.points(&pl.points).fitting(&pl.fitting_result)
    }
    fn polylines(&mut self, pls: &[Polyline]) -> &mut Self {
        self.len(pls.len());
        for pl in pls {
            self.polyline(pl);
        }
        self
    }
    fn flow(&mut self, f: &Flow) -> &mut Self {
        self.f32(f.width()).f32(f.height()).f32(f.spacing());
        self.f32(f.nozzle_diameter()).boolean(f.bridge())
    }
    fn attempt<T, E: What>(&mut self, res: Result<T, E>, f: impl FnOnce(&mut W, T)) -> &mut Self {
        match res {
            Ok(v) => {
                self.u8(1);
                f(self, v);
            }
            Err(e) => {
                self.u8(0).str(&e.what_text());
            }
        }
        self
    }

    /// `put_path_fields`, with the class's `can_reverse()`.
    fn path_fields(&mut self, p: &ExtrusionPath, can_reverse: bool) -> &mut Self {
        self.u8(p.role() as u8).i32(p.inset_idx);
        self.len(p.polyline.points.len());
        for q in &p.polyline.points {
            self.i64(q.x).i64(q.y).i64(q.z);
        }
        self.fitting(&p.polyline.fitting_result);
        self.f64(p.overhang_degree)
            .i32(p.curve_degree)
            .f64(p.mm3_per_mm);
        self.f32(p.width).f32(p.height).f64(p.smooth_speed);
        self.boolean(p.z_contoured)
            .boolean(p.is_force_no_extrusion());
        self.boolean(can_reverse)
    }
    /// An `ExtrusionPath` held by value (kind 1).
    fn path(&mut self, p: &ExtrusionPath) -> &mut Self {
        self.u8(1).path_fields(p, p.can_reverse())
    }
    fn paths(&mut self, v: &[ExtrusionPath]) -> &mut Self {
        self.len(v.len());
        for p in v {
            self.path(p);
        }
        self
    }
    fn sloped(&mut self, s: &ExtrusionPathSloped) -> &mut Self {
        self.u8(3).path_fields(&s.path, s.path.can_reverse());
        self.f64(s.slope_begin.z_ratio).f64(s.slope_begin.e_ratio);
        self.f64(s.slope_end.z_ratio).f64(s.slope_end.e_ratio)
    }
    fn multi_path(&mut self, m: &ExtrusionMultiPath) -> &mut Self {
        self.u8(5).i32(m.inset_idx).boolean(m.can_reverse());
        self.paths(&m.paths)
    }
    fn loop_fields(&mut self, l: &ExtrusionLoop) -> &mut Self {
        self.i32(l.inset_idx).u8(l.loop_role().0);
        self.paths(&l.paths)
    }
    fn extrusion_loop(&mut self, l: &ExtrusionLoop) -> &mut Self {
        self.u8(6).loop_fields(l)
    }
    fn loop_sloped(&mut self, l: &ExtrusionLoopSloped) -> &mut Self {
        self.u8(7).loop_fields(&l.base);
        self.len(l.starts.len());
        for s in &l.starts {
            self.sloped(s);
        }
        self.len(l.ends.len());
        for s in &l.ends {
            self.sloped(s);
        }
        self
    }
    fn collection(&mut self, c: &ExtrusionEntityCollection) -> &mut Self {
        self.u8(8)
            .i32(c.inset_idx)
            .boolean(c.no_sort)
            .boolean(c.can_reverse());
        self.entities(&c.entities)
    }
    /// `put_entity`: the kind by the class, then its fields.
    fn entity(&mut self, e: &ExtrusionEntity) -> &mut Self {
        match e {
            ExtrusionEntity::Path(p) => self.path(p),
            ExtrusionEntity::PathContoured(p) => {
                self.u8(2).path_fields(&p.path, p.path.can_reverse());
                self.len(p.z_diffs.len());
                for &z in &p.z_diffs {
                    self.f64(z);
                }
                self
            }
            ExtrusionEntity::PathSloped(s) => self.sloped(s),
            ExtrusionEntity::PathOriented(o) => self.u8(4).path_fields(&o.path, o.can_reverse()),
            ExtrusionEntity::MultiPath(m) => self.multi_path(m),
            ExtrusionEntity::Loop(l) => self.extrusion_loop(l),
            ExtrusionEntity::LoopSloped(l) => self.loop_sloped(l),
            ExtrusionEntity::Collection(c) => self.collection(c),
        }
    }
    fn entities(&mut self, v: &[ExtrusionEntity]) -> &mut Self {
        self.len(v.len());
        for e in v {
            self.entity(e);
        }
        self
    }
}

// ---- Reading ---------------------------------------------------------------------------------

fn fail<T>(what: impl Into<String>) -> Result<T, Error> {
    Err(Error(what.into()))
}

fn rd_point(r: &mut Reader<'_>) -> Result<Point, Error> {
    Ok(Point::new(r.i64()?, r.i64()?))
}

fn rd_points(r: &mut Reader<'_>) -> Result<Points, Error> {
    r.vec(rd_point)
}

fn rd_expolygon(r: &mut Reader<'_>) -> Result<ExPolygon, Error> {
    let contour = Polygon::new(rd_points(r)?);
    let holes = r.vec(|q| Ok(Polygon::new(rd_points(q)?)))?;
    Ok(ExPolygon { contour, holes })
}

fn rd_surface_type(r: &mut Reader<'_>) -> Result<SurfaceType, Error> {
    let v = r.u32()?;
    i32::try_from(v)
        .ok()
        .and_then(SurfaceType::from_i32)
        .map_or_else(|| fail(format!("a surface type of {v}")), Ok)
}

fn rd_surface(r: &mut Reader<'_>) -> Result<Surface, Error> {
    let surface_type = rd_surface_type(r)?;
    let mut s = Surface::with_expolygon(surface_type, rd_expolygon(r)?);
    s.thickness = r.f64()?;
    s.thickness_layers = r.u16()?;
    s.bridge_angle = r.f64()?;
    s.extra_perimeters = r.u16()?;
    Ok(s)
}

fn rd_surfaces(r: &mut Reader<'_>) -> Result<Vec<Surface>, Error> {
    r.vec(rd_surface)
}

fn rd_fitting(r: &mut Reader<'_>) -> Result<Vec<PathFittingData>, Error> {
    r.vec(|q| {
        let start = usize::try_from(q.u64()?).map_err(|_| Error("an index".into()))?;
        let end = usize::try_from(q.u64()?).map_err(|_| Error("an index".into()))?;
        let path_type = match q.u8()? {
            0 => EMovePathType::NoopMove,
            1 => EMovePathType::LinearMove,
            2 => EMovePathType::ArcMoveCw,
            3 => EMovePathType::ArcMoveCcw,
            v => return fail(format!("a move type of {v}")),
        };
        let is_arc = q.bool()?;
        let center = rd_point(q)?;
        let radius = q.f64()?;
        let length = q.f64()?;
        let angle_radians = q.f64()?;
        let polar_start_theta = q.f64()?;
        let polar_end_theta = q.f64()?;
        let start_point = rd_point(q)?;
        let end_point = rd_point(q)?;
        let direction = match q.u8()? {
            0 => ArcDirection::Unknown,
            1 => ArcDirection::Ccw,
            2 => ArcDirection::Cw,
            v => return fail(format!("an arc direction of {v}")),
        };
        let arc = ArcSegment {
            center,
            radius,
            is_arc,
            length,
            angle_radians,
            polar_start_theta,
            polar_end_theta,
            start_point,
            end_point,
            direction,
        };
        Ok(PathFittingData::new(start, end, path_type, arc))
    })
}

fn rd_polyline(r: &mut Reader<'_>) -> Result<Polyline, Error> {
    let points = rd_points(r)?;
    let fitting_result = rd_fitting(r)?;
    Ok(Polyline {
        points,
        fitting_result,
    })
}

fn rd_role(r: &mut Reader<'_>) -> Result<ExtrusionRole, Error> {
    let v = r.u8()?;
    ExtrusionRole::from_u8(v).ok_or_else(|| Error(format!("a role of {v}")))
}

/// `put_path_fields`: the path and the `can_reverse()` written.
fn rd_path_fields(r: &mut Reader<'_>) -> Result<(ExtrusionPath, bool), Error> {
    let role = rd_role(r)?;
    let inset = r.i32()?;
    let points = r.vec(|q| Ok(Point3::new(q.i64()?, q.i64()?, q.i64()?)))?;
    let fitting_result = rd_fitting(r)?;
    let overhang_degree = r.f64()?;
    let curve_degree = r.i32()?;
    let mm3_per_mm = r.f64()?;
    let width = r.f32()?;
    let height = r.f32()?;
    let smooth_speed = r.f64()?;
    let z_contoured = r.bool()?;
    let no_extrusion = r.bool()?;
    let can_reverse = r.bool()?;
    let mut p = ExtrusionPath::new_no_extrusion(role, mm3_per_mm, width, height, no_extrusion);
    p.polyline = Polyline3 {
        points,
        fitting_result,
    };
    p.overhang_degree = overhang_degree;
    p.curve_degree = curve_degree;
    p.smooth_speed = smooth_speed;
    p.z_contoured = z_contoured;
    p.inset_idx = inset;
    Ok((p, can_reverse))
}

/// A path whose `can_reverse()` is its member.
fn rd_member_path(r: &mut Reader<'_>) -> Result<ExtrusionPath, Error> {
    let (mut p, can_reverse) = rd_path_fields(r)?;
    if !can_reverse {
        p.set_reverse();
    }
    Ok(p)
}

fn rd_slope(r: &mut Reader<'_>) -> Result<Slope, Error> {
    Ok(Slope {
        z_ratio: r.f64()?,
        e_ratio: r.f64()?,
    })
}

fn rd_sloped(r: &mut Reader<'_>) -> Result<ExtrusionPathSloped, Error> {
    match r.u8()? {
        3 => {
            let path = rd_member_path(r)?;
            let slope_begin = rd_slope(r)?;
            let slope_end = rd_slope(r)?;
            Ok(ExtrusionPathSloped {
                path,
                slope_begin,
                slope_end,
            })
        }
        v => fail(format!("a sloped path of kind {v}")),
    }
}

/// A `vec<entity>` of paths held by value (kind 1).
fn rd_paths(r: &mut Reader<'_>) -> Result<ExtrusionPaths, Error> {
    r.vec(|q| match q.u8()? {
        1 => rd_member_path(q),
        v => fail(format!("a path of kind {v}")),
    })
}

fn rd_loop_fields(r: &mut Reader<'_>) -> Result<ExtrusionLoop, Error> {
    let inset = r.i32()?;
    let role = ExtrusionLoopRole(r.u8()?);
    let mut l = ExtrusionLoop::from_paths(rd_paths(r)?, role);
    l.inset_idx = inset;
    Ok(l)
}

/// An `entity`. An oriented path's record holds the class's `can_reverse()` (false): its member
/// is left true, as `ExtrusionPathOriented`'s constructor leaves it. A collection that may not be
/// sorted records false whatever its `is_reverse`, which nothing then reads: it is left true.
fn rd_entity(r: &mut Reader<'_>) -> Result<ExtrusionEntity, Error> {
    Ok(match r.u8()? {
        1 => ExtrusionEntity::Path(rd_member_path(r)?),
        2 => {
            let path = rd_member_path(r)?;
            let z_diffs = r.vec(Reader::f64)?;
            ExtrusionEntity::PathContoured(ExtrusionPathContoured { path, z_diffs })
        }
        3 => {
            let path = rd_member_path(r)?;
            let slope_begin = rd_slope(r)?;
            let slope_end = rd_slope(r)?;
            ExtrusionEntity::PathSloped(ExtrusionPathSloped {
                path,
                slope_begin,
                slope_end,
            })
        }
        4 => {
            let (path, _) = rd_path_fields(r)?;
            ExtrusionEntity::PathOriented(ExtrusionPathOriented { path })
        }
        5 => {
            let inset = r.i32()?;
            let can_reverse = r.bool()?;
            let mut m = ExtrusionMultiPath::default();
            m.paths = rd_paths(r)?;
            m.inset_idx = inset;
            if !can_reverse {
                m.set_reverse();
            }
            ExtrusionEntity::MultiPath(m)
        }
        6 => ExtrusionEntity::Loop(rd_loop_fields(r)?),
        7 => {
            let base = rd_loop_fields(r)?;
            let starts = r.vec(rd_sloped)?;
            let ends = r.vec(rd_sloped)?;
            ExtrusionEntity::LoopSloped(ExtrusionLoopSloped { base, starts, ends })
        }
        8 => {
            let inset = r.i32()?;
            let no_sort = r.bool()?;
            let can_reverse = r.bool()?;
            let mut c = ExtrusionEntityCollection::default();
            c.entities = r.vec(rd_entity)?;
            c.no_sort = no_sort;
            c.inset_idx = inset;
            if !no_sort && !can_reverse {
                c.set_reverse();
            }
            ExtrusionEntity::Collection(c)
        }
        0 => {
            let name = r.str()?;
            return fail(format!("an entity of the unknown class {name}"));
        }
        v => return fail(format!("an entity of kind {v}")),
    })
}

// ---- Comparing -------------------------------------------------------------------------------

fn hex(b: &[u8]) -> String {
    let shown: Vec<String> = b.iter().take(48).map(|x| format!("{x:02x}")).collect();
    let more = if b.len() > 48 { "…" } else { "" };
    format!("{} bytes [{}{more}]", b.len(), shown.join(" "))
}

/// One record's replay: its inputs read and its results compared, part by part, in order.
struct Cmp<'a> {
    buf: &'a [u8],
    at: usize,
    label: String,
    parts: u64,
    failed: Option<String>,
}

impl<'a> Cmp<'a> {
    fn new(buf: &'a [u8], label: String) -> Self {
        Self {
            buf,
            at: 0,
            label,
            parts: 0,
            failed: None,
        }
    }

    fn fail(&mut self, what: String) -> Option<()> {
        self.failed = Some(format!("{}: {what}", self.label));
        None
    }

    /// Reads inputs (or a result the replay takes from the record) at the current byte.
    fn input<T>(&mut self, f: impl FnOnce(&mut Reader<'a>) -> Result<T, Error>) -> Option<T> {
        if self.failed.is_some() {
            return None;
        }
        let buf: &'a [u8] = self.buf;
        let mut r = Reader::new(&buf[self.at..]);
        match f(&mut r) {
            Ok(v) => {
                self.at = buf.len() - r.remaining();
                Some(v)
            }
            Err(e) => {
                self.fail(format!("reading at byte {}: {e}", self.at));
                None
            }
        }
    }

    /// The value at the current byte, not taken.
    fn peek<T>(&mut self, f: impl FnOnce(&mut Reader<'a>) -> Result<T, Error>) -> Option<T> {
        let at = self.at;
        let v = self.input(f);
        self.at = at;
        v
    }

    /// The bytes from `start` to here, which an input took, must be what `f` writes.
    fn echo(&mut self, start: usize, what: &str, f: impl FnOnce(&mut W)) -> Option<()> {
        if self.failed.is_some() {
            return None;
        }
        let mut w = W::default();
        f(&mut w);
        self.parts += 1;
        if self.buf[start..self.at] == w.0[..] {
            Some(())
        } else {
            let (got, want) = (hex(&w.0), hex(&self.buf[start..self.at]));
            self.fail(format!(
                "the input {what} at byte {start} encodes again as {got}, the record {want}"
            ))
        }
    }

    /// A result: what `f` writes must be the next bytes.
    fn part(&mut self, what: &str, f: impl FnOnce(&mut W)) -> Option<()> {
        if self.failed.is_some() {
            return None;
        }
        let mut w = W::default();
        f(&mut w);
        self.parts += 1;
        let end = self.at + w.0.len();
        if self.buf.get(self.at..end) == Some(&w.0[..]) {
            self.at = end;
            Some(())
        } else {
            let first =
                w.0.iter()
                    .zip(&self.buf[self.at..])
                    .position(|(a, b)| a != b)
                    .unwrap_or(w.0.len().min(self.buf.len() - self.at));
            let want = &self.buf[self.at..self.buf.len().min(end + 16)];
            let (got, want) = (hex(&w.0), hex(want));
            self.fail(format!(
                "{what} at byte {} differs from its byte {first}: the translation {got}, the C++ {want}",
                self.at
            ))
        }
    }

    /// A comparison of values the record does not let the replay rebuild byte for byte.
    fn check(&mut self, what: &str, same: bool) -> Option<()> {
        if self.failed.is_some() {
            return None;
        }
        self.parts += 1;
        if same {
            Some(())
        } else {
            self.fail(format!("{what} differs"))
        }
    }

    fn finish(mut self, m: &mut Mismatches) {
        if self.failed.is_none() && self.at != self.buf.len() {
            let left = self.buf.len() - self.at;
            self.fail(format!("{left} bytes are left after the replay"));
        }
        // A record can fail before its first part (an input that does not read).
        let ok = self.parts.saturating_sub(u64::from(self.failed.is_some()));
        for _ in 0..ok {
            m.check(true, String::new);
        }
        if let Some(f) = self.failed {
            m.check(false, || f);
        }
    }
}

/// The four covers: `try<vec<polygon>>` of the width covers and the spacing covers, at 0 and eps.
fn covers(c: &mut Cmp<'_>, e: &ExtrusionEntity, eps: f32) -> Option<()> {
    for (name, at) in [("0", 0.0f32), ("eps", eps)] {
        c.part(&format!("polygons_covered_by_width({name})"), |w| {
            w.attempt(Ok::<_, FlowError>(e.covered_by_width(at)), |w, p| {
                w.polygons(&p);
            });
        })?;
    }
    for (name, at) in [("0", 0.0f32), ("eps", eps)] {
        c.part(&format!("polygons_covered_by_spacing({name})"), |w| {
            w.attempt(e.covered_by_spacing(at), |w, p| {
                w.polygons(&p);
            });
        })?;
    }
    Some(())
}

/// An entity input, checked by encoding it again.
fn entity_input(c: &mut Cmp<'_>, what: &str) -> Option<ExtrusionEntity> {
    let start = c.at;
    let e = c.input(rd_entity)?;
    c.echo(start, what, |w| {
        w.entity(&e);
    })?;
    Some(e)
}

// ---- Flow ------------------------------------------------------------------------------------

fn lyfl(c: &mut Cmp<'_>) -> Option<()> {
    let (how, a, b, nozzle, role, value, percent) = c.input(|r| {
        Ok((
            r.u8()?,
            r.f32()?,
            r.f32()?,
            r.f32()?,
            r.u8()?,
            r.f64()?,
            r.bool()?,
        ))
    })?;
    let made = match how {
        0 => Flow::new(a, b, nozzle),
        1 => Ok(Flow::bridging_flow(a, nozzle)),
        2 => {
            let Some(role) = FlowRole::from_u8(role) else {
                return c.fail(format!("a flow role of {role}"));
            };
            let width = ConfigOptionFloatOrPercent::new(value, percent);
            Flow::new_from_config_width(role, &width, nozzle, b)
        }
        _ => Ok(Flow::default()),
    };
    c.part("the flow made", |w| match &made {
        Ok(_) => {
            w.u8(1);
        }
        Err(e) => {
            w.u8(0).str(&e.what());
        }
    })?;
    let Ok(f) = made else {
        return Some(());
    };
    c.part("the flow", |w| {
        w.flow(&f);
    })?;
    c.part("mm3_per_mm()", |w| {
        w.attempt(f.mm3_per_mm(), |w, v| {
            w.f64(v);
        });
    })?;
    c.part("the scaled widths", |w| {
        w.i64(f.scaled_width()).i64(f.scaled_spacing());
        w.i64(f.scaled_elephant_foot_spacing());
    })?;
    let (w2, h2, s2, area, ratio) =
        c.input(|r| Ok((r.f32()?, r.f32()?, r.f32()?, r.f32()?, r.f64()?)))?;
    let flow_try = |w: &mut W, g: Result<Flow, FlowError>| {
        w.attempt(g, |w, g| {
            w.flow(&g);
        });
    };
    c.part("with_width(w2)", |w| flow_try(w, f.with_width(w2)))?;
    c.part("with_height(h2)", |w| flow_try(w, f.with_height(h2)))?;
    c.part("with_spacing(s2)", |w| flow_try(w, f.with_spacing(s2)))?;
    c.part("with_cross_section(area)", |w| {
        flow_try(w, f.with_cross_section(area));
    })?;
    c.part("with_flow_ratio(ratio)", |w| {
        flow_try(w, f.with_flow_ratio(ratio));
    })?;
    let reference = Flow::new(0.45, 0.2, 0.4).ok()?;
    c.part("the reference flow", |w| {
        w.flow(&reference);
    })?;
    c.part("== and !=", |w| {
        w.boolean(f == reference).boolean(f != reference);
    })?;
    c.part("<", |w| {
        w.attempt(f.less_than(&reference), |w, v| {
            w.boolean(v);
        });
    })
}

fn lyfs(c: &mut Cmp<'_>) -> Option<()> {
    let (w0, h0, n) = c.input(|r| Ok((r.f32()?, r.f32()?, r.f32()?)))?;
    c.part("rounded_rectangle_extrusion_spacing", |w| {
        w.attempt(Flow::rounded_rectangle_extrusion_spacing(w0, h0), |w, v| {
            w.f32(v);
        });
    })?;
    c.part("rounded_rectangle_extrusion_width_from_spacing", |w| {
        w.f32(Flow::rounded_rectangle_extrusion_width_from_spacing(w0, h0));
    })?;
    c.part("bridge_extrusion_spacing", |w| {
        w.f32(Flow::bridge_extrusion_spacing(w0));
    })?;
    c.part("auto_extrusion_width of each role", |w| {
        for role in FlowRole::ALL {
            w.f32(Flow::auto_extrusion_width(role, n));
        }
    })
}

fn lyfw(c: &mut Cmp<'_>) -> Option<()> {
    let pairs = c.input(|r| r.vec(|q| Ok((q.str()?, q.str()?))))?;
    let first = c.input(Reader::u32)?;
    let mut cfg = Config::default();
    for (k, v) in &pairs {
        if let Err(e) = cfg.set_deserialize_strict(k, v) {
            return c.fail(format!("set_deserialize_strict({k}, {v}): {}", e.0));
        }
    }
    let n = c.input(Reader::count)?;
    for _ in 0..n {
        let key = c.input(Reader::str)?;
        c.part(&format!("extrusion_width({key})"), |w| {
            w.attempt(Flow::extrusion_width(&key, &cfg, first), |w, v| {
                w.f64(v);
            });
        })?;
    }
    Some(())
}

// ---- Extrusion entities ----------------------------------------------------------------------

fn lyro(c: &mut Cmp<'_>) -> Option<()> {
    let n = c.input(Reader::count)?;
    c.check("the number of roles", n == ExtrusionRole::ALL.len())?;
    for _ in 0..n {
        let v = c.input(Reader::u8)?;
        let Some(role) = ExtrusionRole::from_u8(v) else {
            return c.fail(format!("a role of {v}"));
        };
        let name = role_to_string(role);
        c.part(&format!("role {v}"), |w| {
            w.str(&name).u8(string_to_role(&name) as u8);
            let mut bits = 0u8;
            for (i, f) in [
                is_perimeter,
                is_internal_perimeter,
                is_external_perimeter,
                is_infill,
                is_top_surface,
                is_solid_infill,
                is_bridge,
            ]
            .iter()
            .enumerate()
            {
                if f(role) {
                    bits |= 1 << i;
                }
            }
            w.u8(bits);
        })?;
    }
    let n = c.input(Reader::count)?;
    for _ in 0..n {
        let s = c.input(Reader::str)?;
        c.part(&format!("string_to_role({s:?})"), |w| {
            w.u8(string_to_role(&s) as u8);
        })?;
    }
    Some(())
}

fn lyen(c: &mut Cmp<'_>) -> Option<()> {
    let e = entity_input(c, "path")?;
    let (eps, d, tol) = c.input(|r| Ok((r.f32()?, r.f64()?, r.f64()?)))?;
    let clip = c.input(|r| r.vec(rd_expolygon))?;
    let Some(path) = e.as_path() else {
        return c.fail("the input is not a path".to_string());
    };
    c.part("length()", |w| {
        w.f64(path.length());
    })?;
    c.part("first_point() and last_point()", |w| {
        w.point(e.first_point()).point(e.last_point());
    })?;
    c.part("is_closed()", |w| {
        w.boolean(path.is_closed());
    })?;
    c.part("min_mm3_per_mm() and total_volume()", |w| {
        w.f64(e.min_mm3_per_mm()).f64(e.total_volume());
    })?;
    c.part("as_polyline()", |w| {
        w.polyline(&path.as_polyline());
    })?;
    let mut pls = Polylines::new();
    e.collect_polylines(&mut pls);
    c.part("collect_polylines", |w| {
        w.polylines(&pls);
    })?;
    let mut cps = Points::new();
    e.collect_points(&mut cps);
    c.part("collect_points", |w| {
        w.points(&cps);
    })?;
    covers(c, &e, eps)?;

    let mut reversed = e.clone();
    reversed.reverse();
    c.part("reverse() of a clone", |w| {
        w.entity(&reversed);
    })?;
    let mut clipped = e.clone();
    clipped.as_path_mut()?.clip_end(d);
    c.part("clip_end(d) of a clone", |w| {
        w.entity(&clipped);
    })?;
    // `simplify` is virtual: the contoured path's does nothing.
    let mut simplified = e.clone();
    match &mut simplified {
        ExtrusionEntity::PathContoured(p) => p.simplify(tol),
        other => other.as_path_mut()?.simplify(tol),
    }
    c.part("simplify(tol) of a clone", |w| {
        w.entity(&simplified);
    })?;
    // `c->ExtrusionPath::simplify_by_fitting_arc(tol)`: the path's own, whatever the class.
    let mut fitted = e.clone();
    fitted.as_path_mut()?.simplify_by_fitting_arc(tol);
    c.part(
        "ExtrusionPath::simplify_by_fitting_arc(tol) of a clone",
        |w| {
            w.entity(&fitted);
        },
    )?;
    let mut inside = ExtrusionEntityCollection::default();
    let mut outside = ExtrusionEntityCollection::default();
    path.intersect_expolygons(&clip, &mut inside);
    path.subtract_expolygons(&clip, &mut outside);
    c.part("intersect_expolygons(clip)", |w| {
        w.collection(&inside);
    })?;
    c.part("subtract_expolygons(clip)", |w| {
        w.collection(&outside);
    })
}

fn lyem(c: &mut Cmp<'_>) -> Option<()> {
    let e = entity_input(c, "multi path")?;
    let eps = c.input(Reader::f32)?;
    let ExtrusionEntity::MultiPath(mp) = &e else {
        return c.fail("the input is not a multi path".to_string());
    };
    c.part("length() and role()", |w| {
        w.f64(mp.length()).u8(mp.role() as u8);
    })?;
    c.part("first and last point", |w| {
        w.point(mp.first_point()).point(mp.last_point());
    })?;
    c.part("min_mm3_per_mm() and total_volume()", |w| {
        w.f64(mp.min_mm3_per_mm()).f64(mp.total_volume());
    })?;
    c.part("as_polyline()", |w| {
        w.polyline(&mp.as_polyline());
    })?;
    let mut pls = Polylines::new();
    mp.collect_polylines(&mut pls);
    c.part("collect_polylines", |w| {
        w.polylines(&pls);
    })?;
    let mut cps = Points::new();
    mp.collect_points(&mut cps);
    c.part("collect_points", |w| {
        w.points(&cps);
    })?;
    covers(c, &e, eps)?;
    let mut r = mp.clone();
    r.reverse();
    c.part("reverse() of a copy", |w| {
        w.multi_path(&r);
    })?;
    r.set_reverse();
    c.part("can_reverse() after set_reverse()", |w| {
        w.boolean(r.can_reverse());
    })
}

fn lyel(c: &mut Cmp<'_>) -> Option<()> {
    let e = entity_input(c, "loop")?;
    let (near_v, q) = c.input(|r| Ok((rd_point(r)?, rd_point(r)?)))?;
    let (prefer, eps, d, ceps) = c.input(|r| Ok((r.bool()?, r.f64()?, r.f64()?, r.f32()?)))?;
    let ExtrusionEntity::Loop(lp) = &e else {
        return c.fail("the input is not a loop".to_string());
    };
    let poly = lp.polygon();
    c.part("polygon()", |w| {
        w.points(&poly.points);
    })?;
    c.part("length() and role()", |w| {
        w.f64(lp.length()).u8(lp.role() as u8);
    })?;
    let mut a = lp.clone();
    c.part("is_clockwise() and is_counter_clockwise()", |w| {
        w.boolean(a.is_clockwise())
            .boolean(a.is_counter_clockwise());
    })?;
    let was = a.make_clockwise();
    c.part("make_clockwise()", |w| {
        w.boolean(was).extrusion_loop(&a);
    })?;
    let mut b = lp.clone();
    let was = b.make_counter_clockwise();
    c.part("make_counter_clockwise()", |w| {
        w.boolean(was).extrusion_loop(&b);
    })?;
    let mut sv = lp.clone();
    let found = sv.split_at_vertex(&near_v, scaled_f64(0.001));
    c.part("split_at_vertex(v)", |w| {
        w.boolean(found).extrusion_loop(&sv);
    })?;
    let cp = lp.get_closest_path_and_point(&q, prefer);
    c.part("get_closest_path_and_point(q, prefer)", |w| {
        w.len(cp.path_idx).len(cp.segment_idx).point(cp.foot_pt);
    })?;
    let mut s = lp.clone();
    s.split_at(&q, prefer, eps);
    c.part("split_at(q, prefer, eps)", |w| {
        w.extrusion_loop(&s);
    })?;
    let mut clipped = ExtrusionPaths::new();
    lp.clip_end(d, &mut clipped);
    c.part("clip_end(d)", |w| {
        w.paths(&clipped);
    })?;
    let want = c.input(Reader::bool)?;
    if poly.points.contains(&near_v) {
        c.check(
            "has_overhang_point of the vertex v",
            lp.has_overhang_point(&near_v) == want,
        )?;
    } else {
        c.check(
            "has_overhang_point of some vertex",
            poly.points.iter().any(|v| lp.has_overhang_point(v) == want),
        )?;
    }
    c.part("is_smooth() and is_smooth(0.5, 0.1)", |w| {
        w.boolean(lp.is_smooth(0.174, 0.025))
            .boolean(lp.is_smooth(0.5, 0.1));
    })?;
    c.part("as_polyline()", |w| {
        w.polyline(&lp.as_polyline());
    })?;
    c.part("min_mm3_per_mm() and total_volume()", |w| {
        w.f64(lp.min_mm3_per_mm()).f64(lp.total_volume());
    })?;
    covers(c, &e, ceps)
}

fn lyes(c: &mut Cmp<'_>) -> Option<()> {
    let start = c.at;
    let original = c.input(rd_paths)?;
    c.echo(start, "paths", |w| {
        w.paths(&original);
    })?;
    let (seam_gap, min_len, max_seg, ratio, role, d) = c.input(|r| {
        Ok((
            r.f64()?,
            r.f64()?,
            r.f64()?,
            r.f64()?,
            ExtrusionLoopRole(r.u8()?),
            r.f64()?,
        ))
    })?;
    c.part("the attempt", |w| {
        w.u8(1);
    })?;
    let s = ExtrusionLoopSloped::new(&original, seam_gap, min_len, max_seg, ratio, role);
    c.part("the sloped loop", |w| {
        w.loop_sloped(&s);
    })?;
    c.part("get_all_paths().size() and slope_path_length()", |w| {
        w.len(s.get_all_paths().len()).f64(s.slope_path_length());
    })?;
    c.part("the starts' interpolate(0.5) and is_flat()", |w| {
        w.len(s.starts.len());
        for st in &s.starts {
            let mid = st.interpolate(0.5);
            w.f64(mid.z_ratio).f64(mid.e_ratio).boolean(st.is_flat());
        }
    })?;
    let mut a = s.clone();
    a.clip_slope(d, false);
    c.part("clip_slope(d) of a copy", |w| {
        w.loop_sloped(&a);
    })?;
    let mut b = s.clone();
    b.clip_end(d);
    c.part("clip_end(d) of a copy", |w| {
        w.loop_sloped(&b);
    })?;
    let mut f = s.clone();
    f.clip_front(d);
    c.part("clip_front(d) of a copy", |w| {
        w.loop_sloped(&f);
    })
}

/// `chained_path_from(start, role)`.
fn chained(
    c: &mut Cmp<'_>,
    coll: &ExtrusionEntityCollection,
    start: &Point,
    role: ExtrusionRole,
) -> Option<()> {
    let out = coll.chained_path_from(start, role);
    c.part(&format!("chained_path_from(start, {role:?})"), |w| {
        w.collection(&out);
    })
}

fn lyec(c: &mut Cmp<'_>) -> Option<()> {
    let e = entity_input(c, "collection")?;
    let (eps, start, frole, ri) =
        c.input(|r| Ok((r.f32()?, rd_point(r)?, rd_role(r)?, r.u64()?)))?;
    let ExtrusionEntity::Collection(coll) = &e else {
        return c.fail("the input is not a collection".to_string());
    };
    let Some(ri) = usize::try_from(ri)
        .ok()
        .filter(|&i| i < coll.entities.len())
    else {
        return c.fail(format!("an index {ri} past the entities"));
    };
    c.part("role() and items_count()", |w| {
        w.u8(coll.role() as u8).len(coll.items_count());
    })?;
    c.part(
        "has_perimeters, has_infill, has_solid_infill, can_sort, can_reverse",
        |w| {
            w.boolean(coll.has_perimeters()).boolean(coll.has_infill());
            w.boolean(coll.has_solid_infill()).boolean(coll.can_sort());
            w.boolean(coll.can_reverse());
        },
    )?;
    c.part("min_mm3_per_mm() and total_volume()", |w| {
        w.f64(coll.min_mm3_per_mm()).f64(coll.total_volume());
    })?;
    c.part("first and last point", |w| {
        w.point(coll.first_point()).point(coll.last_point());
    })?;
    let mut pls = Polylines::new();
    coll.collect_polylines(&mut pls);
    c.part("collect_polylines", |w| {
        w.polylines(&pls);
    })?;
    let mut cps = Points::new();
    coll.collect_points(&mut cps);
    c.part("collect_points", |w| {
        w.points(&cps);
    })?;
    covers(c, &e, eps)?;
    c.part("flatten(false)", |w| {
        w.collection(&coll.flatten(false));
    })?;
    c.part("flatten(true)", |w| {
        w.collection(&coll.flatten(true));
    })?;
    let mut r = coll.clone();
    r.reverse();
    c.part("reverse() of a copy", |w| {
        w.collection(&r);
    })?;
    c.part("ExtrusionPaths(collection)", |w| {
        w.paths(&coll.to_paths());
    })?;
    chained(c, coll, &start, ExtrusionRole::Mixed)?;
    chained(c, coll, &start, frole)?;
    let kept = filter_by_extrusion_role(&coll.entities, frole);
    c.part("filter_by_extrusion_role(entities, r)", |w| {
        w.len(kept.len());
        for k in &kept {
            let at = coll.entities.iter().position(|x| std::ptr::eq(x, *k));
            w.len(at.unwrap_or(usize::MAX));
        }
    })?;
    // The gap fill path the probe made for `replace` is in the recorded result, at ri.
    let recorded = c.peek(rd_entity)?;
    let replacement = match &recorded {
        ExtrusionEntity::Collection(rc) => match rc.entities.get(ri) {
            Some(ExtrusionEntity::Path(p)) => p.clone(),
            _ => return c.fail("the replaced entity is not a path".to_string()),
        },
        _ => return c.fail("the result of replace is not a collection".to_string()),
    };
    let mut replaced = coll.clone();
    let mut with = replacement;
    // A clone goes in: the index of what is passed does not.
    with.inset_idx = 7;
    replaced.replace(ri, &ExtrusionEntity::Path(with));
    c.part("replace(i, path) of a copy", |w| {
        w.collection(&replaced);
    })?;
    let mut removed = coll.clone();
    removed.remove(ri);
    c.part("remove(i) of a copy", |w| {
        w.collection(&removed);
    })?;
    let mut assigned = ExtrusionEntityCollection::default();
    assigned.assign_from(coll);
    c.part(
        "can_reverse() and no_sort of a collection assigned from it",
        |w| {
            w.boolean(assigned.can_reverse()).boolean(assigned.no_sort);
        },
    )?;
    let mut s = coll.clone();
    s.set_reverse();
    c.part("can_reverse() of a copy after set_reverse()", |w| {
        w.boolean(s.can_reverse());
    })
}

fn lyea(c: &mut Cmp<'_>) -> Option<()> {
    let start = c.at;
    let pls = c.input(|r| r.vec(rd_polyline))?;
    c.echo(start, "polylines", |w| {
        w.polylines(&pls);
    })?;
    let (role, mm3, width, height, rev) =
        c.input(|r| Ok((rd_role(r)?, r.f64()?, r.f32()?, r.f32()?, r.bool()?)))?;
    let mut paths = ExtrusionPaths::new();
    extrusion_paths_append(&mut paths, &pls, role, mm3, width, height);
    c.part("extrusion_paths_append", |w| {
        w.paths(&paths);
    })?;
    let mut dst = ExtrusionEntitiesPtr::new();
    extrusion_entities_append_paths(&mut dst, &pls, role, mm3, width, height, rev);
    c.part("extrusion_entities_append_paths", |w| {
        w.entities(&dst);
    })?;
    let mut dst = ExtrusionEntitiesPtr::new();
    extrusion_entities_append_paths_with_wipe(&mut dst, pls.clone(), role, mm3, width, height);
    c.part("extrusion_entities_append_paths_with_wipe", |w| {
        w.entities(&dst);
    })?;
    let polys: Vec<Polygon> = pls
        .iter()
        .map(|pl| Polygon::new(pl.points.clone()))
        .collect();
    let mut dst = ExtrusionEntitiesPtr::new();
    extrusion_entities_append_loops(&mut dst, polys, role, mm3, width, height);
    c.part("extrusion_entities_append_loops", |w| {
        w.entities(&dst);
    })?;
    let mut dst = ExtrusionEntitiesPtr::new();
    extrusion_entities_append_loops_and_paths(&mut dst, pls, role, mm3, width, height);
    c.part("extrusion_entities_append_loops_and_paths", |w| {
        w.entities(&dst);
    })
}

fn lyvw(c: &mut Cmp<'_>) -> Option<()> {
    let tps = c.input(|r| {
        r.vec(|q| {
            let points = rd_points(q)?;
            let width = q.vec(Reader::f64)?;
            let endpoints = (q.bool()?, q.bool()?);
            Ok(ThickPolyline {
                polyline: Polyline::new(points),
                width,
                endpoints,
            })
        })
    })?;
    let role = c.input(rd_role)?;
    let start = c.at;
    let (fw, fh, _, fnz, bridge) =
        c.input(|r| Ok((r.f32()?, r.f32()?, r.f32()?, r.f32()?, r.bool()?)))?;
    // The probe's flows: Flow(w, h, nozzle), or a bridging flow of its width.
    let flow = if bridge {
        Flow::bridging_flow(fw, fnz)
    } else {
        match Flow::new(fw, fh, fnz) {
            Ok(f) => f,
            Err(e) => return c.fail(format!("the input flow: {}", e.what())),
        }
    };
    c.echo(start, "flow", |w| {
        w.flow(&flow);
    })?;
    let (tol, mtol) = c.input(|r| Ok((r.f32()?, r.f32()?)))?;
    let mut out = ExtrusionEntitiesPtr::new();
    let res = variable_width(&tps, role, &flow, &mut out).map(|()| out);
    c.part("variable_width", |w| {
        w.attempt(res, |w, out| {
            w.entities(&out);
        });
    })?;
    c.part("the number of polylines", |w| {
        w.len(tps.len());
    })?;
    for (i, tp) in tps.iter().enumerate() {
        c.part(
            &format!("thick_polyline_to_multi_path of polyline {i}"),
            |w| {
                w.attempt(
                    thick_polyline_to_multi_path(tp, role, &flow, tol, mtol),
                    |w, m| {
                        w.multi_path(&m);
                    },
                );
            },
        )?;
    }
    Some(())
}

// ---- Surfaces (layer-2) ----------------------------------------------------------------------

/// A surface list input, checked by encoding it again.
fn surfaces_input(c: &mut Cmp<'_>, what: &str) -> Option<Vec<Surface>> {
    let start = c.at;
    let ss = c.input(rd_surfaces)?;
    c.echo(start, what, |w| {
        w.surfaces(&ss);
    })?;
    Some(ss)
}

/// The probe's `predicates(s)`.
fn predicates(s: &Surface) -> u16 {
    let mut b = 0u16;
    b |= if s.empty() { 1 } else { 0 };
    b |= if s.is_top() { 2 } else { 0 };
    b |= if s.is_bottom() { 4 } else { 0 };
    b |= if s.is_bridge() { 8 } else { 0 };
    b |= if s.is_internal_bridge() { 16 } else { 0 };
    b |= if s.is_external() { 32 } else { 0 };
    b |= if s.is_internal() { 64 } else { 0 };
    b |= if s.is_solid() { 128 } else { 0 };
    b |= if s.is_solid_infill() { 256 } else { 0 };
    b
}

fn lysu(c: &mut Cmp<'_>, last: bool) -> Option<()> {
    if last {
        // The colour names of the types past the list.
        c.part("surface_type_to_color_name(SurfaceType(-1))", |w| {
            w.str(surface_type_value_to_color_name(-1));
        })?;
        c.part("surface_type_to_color_name(stCount)", |w| {
            w.str(surface_type_to_color_name(SurfaceType::Count));
        })?;
        return c.part(
            "surface_type_to_color_name(stInternalAfterExternalBridge)",
            |w| {
                w.str(surface_type_to_color_name(
                    SurfaceType::InternalAfterExternalBridge,
                ));
            },
        );
    }
    let ss = surfaces_input(c, "surfaces")?;
    for (i, s) in ss.iter().enumerate() {
        c.part(&format!("surface {i}'s predicates and area"), |w| {
            w.u16(predicates(s)).f64(s.area());
        })?;
        c.part(&format!("get_extents(surface {i})"), |w| {
            w.bbox(&get_extents(s));
        })?;
        c.part(&format!("to_polygons(surface {i})"), |w| {
            w.polygons(&to_polygons(s));
        })?;
        c.part(&format!("surface_type_to_color_name of surface {i}"), |w| {
            w.str(surface_type_to_color_name(s.surface_type));
        })?;
    }
    c.part("get_extents(surfaces)", |w| {
        w.bbox(&get_extents_surfaces(&ss));
    })?;
    c.part("to_polygons(surfaces)", |w| {
        w.polygons(&to_polygons_surfaces(&ss));
    })?;
    c.part("to_expolygons(surfaces)", |w| {
        w.expolygons(&to_expolygons(&ss));
    })?;
    c.part("number_polygons(surfaces)", |w| {
        w.len(number_polygons(&ss));
    })?;
    c.part("surfaces_could_merge of each ordered pair", |w| {
        for a in &ss {
            for b in &ss {
                w.boolean(surfaces_could_merge(a, b));
            }
        }
    })
}

fn lysc(c: &mut Cmp<'_>) -> Option<()> {
    let ss = surfaces_input(c, "surfaces")?;
    let (t, t2, tol, q, q2) = c.input(|r| {
        Ok((
            rd_surface_type(r)?,
            rd_surface_type(r)?,
            r.f64()?,
            rd_point(r)?,
            rd_point(r)?,
        ))
    })?;
    {
        let mut coll = SurfaceCollection::new(&ss);
        coll.simplify(tol);
        c.part("simplify(tol)", |w| {
            w.surfaces(&coll.surfaces);
        })?;
    }
    {
        let coll = SurfaceCollection::new(&ss);
        let mut groups = Vec::new();
        coll.group(&mut groups);
        c.part("group()", |w| {
            w.len(groups.len());
            for gr in &groups {
                w.surface_ptrs(&coll.surfaces, gr);
            }
        })?;
    }
    {
        let coll = SurfaceCollection::new(&ss);
        c.part("filter_by_type(t)", |w| {
            w.surface_ptrs(&coll.surfaces, &coll.filter_by_type(t));
        })?;
        c.part("filter_by_types({t, t2})", |w| {
            w.surface_ptrs(&coll.surfaces, &coll.filter_by_types(&[t, t2]));
        })?;
        let mut polys = Vec::new();
        coll.filter_by_type_polygons(t, &mut polys);
        c.part("filter_by_type(t, &polygons)", |w| {
            w.polygons(&polys);
        })?;
        let mut has = 0u16;
        for k in 0..ST_COUNT {
            let st = SurfaceType::from_i32(k as i32).unwrap();
            has |= if coll.has(st) { 1u16 << k } else { 0 };
        }
        c.part("has(type) of each type", |w| {
            w.u16(has);
        })?;
        c.part(
            "any_internal_contains and any_bottom_contains of q and q2",
            |w| {
                w.boolean(coll.any_internal_contains(&q));
                w.boolean(coll.any_internal_contains(&q2));
                w.boolean(coll.any_bottom_contains(&q));
                w.boolean(coll.any_bottom_contains(&q2));
            },
        )?;
    }
    {
        let mut coll = SurfaceCollection::new(&ss);
        coll.keep_type(t);
        c.part("keep_type(t)", |w| {
            w.surfaces(&coll.surfaces);
        })?;
    }
    {
        let mut coll = SurfaceCollection::new(&ss);
        coll.keep_types(&[t, t2]);
        c.part("keep_types({t, t2})", |w| {
            w.surfaces(&coll.surfaces);
        })?;
    }
    {
        let mut coll = SurfaceCollection::new(&ss);
        coll.remove_type(t);
        c.part("remove_type(t)", |w| {
            w.surfaces(&coll.surfaces);
        })?;
    }
    {
        let mut coll = SurfaceCollection::new(&ss);
        let mut removed = Vec::new();
        coll.remove_type_into(t, &mut removed);
        c.part("remove_type(t, &expolygons)", |w| {
            w.surfaces(&coll.surfaces).expolygons(&removed);
        })?;
    }
    {
        let mut coll = SurfaceCollection::new(&ss);
        coll.remove_types(&[t, t2]);
        c.part("remove_types({t, t2})", |w| {
            w.surfaces(&coll.surfaces);
        })?;
    }
    let mut coll = SurfaceCollection::new(&ss);
    coll.change_to_new_type(t, t2);
    c.part("change_to_new_type(t, t2)", |w| {
        w.surfaces(&coll.surfaces);
    })?;
    coll.set_type(t);
    c.part("set_type(t)", |w| {
        w.surfaces(&coll.surfaces);
    })
}

// ---- Chaining (layer-3) ----------------------------------------------------------------------

fn lych(c: &mut Cmp<'_>) -> Option<()> {
    let (kind, has_s, start) = c.input(|r| Ok((r.u8()?, r.bool()?, rd_point(r)?)))?;
    let near = if has_s { Some(&start) } else { None };
    match kind {
        0 => {
            let at = c.at;
            let mut v = c.input(|r| r.vec(rd_entity))?;
            c.echo(at, "entities", |w| {
                w.entities(&v);
            })?;
            let chain = chain_extrusion_entities(&v, near);
            c.part("chain_extrusion_entities(entities, start)", |w| {
                w.len(chain.len());
                for &(i, reversed) in &chain {
                    w.len(i).boolean(reversed);
                }
            })?;
            reorder_extrusion_entities(&mut v, &chain);
            c.part("reorder_extrusion_entities(entities, chain)", |w| {
                w.entities(&v);
            })
        }
        1 => {
            let at = c.at;
            let mut v = c.input(rd_paths)?;
            c.echo(at, "paths", |w| {
                w.paths(&v);
            })?;
            chain_and_reorder_extrusion_paths(&mut v, near);
            c.part("chain_and_reorder_extrusion_paths(paths, start)", |w| {
                w.paths(&v);
            })
        }
        2 => {
            let e = entity_input(c, "collection")?;
            let ExtrusionEntity::Collection(coll) = &e else {
                return c.fail("the input is not a collection".to_string());
            };
            chained(c, coll, &start, ExtrusionRole::Mixed)
        }
        v => c.fail(format!("a chaining case of kind {v}")),
    }
}

// ---- The print (layer-3) ---------------------------------------------------------------------

/// The probe's `InstSpec`: an instance's offset, scale and printable flag.
struct InstSpec {
    ox: f64,
    oy: f64,
    sx: f64,
    sy: f64,
    sz: f64,
    printable: bool,
}

/// The probe's `ObjSpec`, but its mesh, which no layer-3 record reads.
struct ObjSpec {
    name: String,
    input_file: String,
    instances: Vec<InstSpec>,
    config: Vec<(String, String)>,
}

fn rd_kv(r: &mut Reader<'_>) -> Result<Vec<(String, String)>, Error> {
    r.vec(|q| Ok((q.str()?, q.str()?)))
}

fn rd_obj_spec(r: &mut Reader<'_>) -> Result<ObjSpec, Error> {
    let name = r.str()?;
    let input_file = r.str()?;
    let _mesh = (r.u8()?, r.f64()?, r.f64()?, r.f64()?);
    let instances = r.vec(|q| {
        Ok(InstSpec {
            ox: q.f64()?,
            oy: q.f64()?,
            sx: q.f64()?,
            sy: q.f64()?,
            sz: q.f64()?,
            printable: q.bool()?,
        })
    })?;
    let config = rd_kv(r)?;
    Ok(ObjSpec {
        name,
        input_file,
        instances,
        config,
    })
}

/// A `try<T>` the replay reads but does not compare: the value `f` reads, or the error's text.
fn rd_skipped_attempt<'a, T>(
    r: &mut Reader<'a>,
    f: impl FnOnce(&mut Reader<'a>) -> Result<T, Error>,
) -> Result<(), Error> {
    match r.u8()? {
        1 => f(r).map(|_| ()),
        0 => r.str().map(|_| ()),
        v => fail(format!("an attempt flag of {v}")),
    }
}

/// A configuration of the given settings, each set as `set_deserialize_strict` sets it.
fn config_of(kv: &[(String, String)]) -> Result<Config, String> {
    let mut cfg = Config::default();
    for (k, v) in kv {
        cfg.set_deserialize_strict(k, v)
            .map_err(|e| format!("set_deserialize_strict({k}, {v}): {}", e.0))?;
    }
    Ok(cfg)
}

/// The full config `Print::apply` works from (PrintApply.cpp): `full_print_config()` with the
/// case's settings, the three settings ids made, `normalize_fdm_1` (`normalize_fdm_2` changes
/// nothing on a print with no objects yet), and the printer's and filaments' per-variant values
/// picked.
fn apply_full_config(kv: &[(String, String)]) -> Result<Config, String> {
    let mut cfg = full_print_config();
    for (k, v) in kv {
        cfg.set_deserialize_strict(k, v)
            .map_err(|e| format!("set_deserialize_strict({k}, {v}): {}", e.0))?;
    }
    for key in [
        "print_settings_id",
        "filament_settings_id",
        "printer_settings_id",
    ] {
        let _ = cfg.create(key);
    }
    normalize_fdm(&mut cfg);
    // printer_options_with_variant_1 (orca-config), then _2 and print_options_with_variant, which
    // orca-config does not carry (the second is checked before the first rewrites the printer's
    // variant list).
    let count = cfg.floats("nozzle_diameter").map_or(0, Vec::len);
    if let Some(list) = cfg.strings("extruder_variant_list")
        && list.iter().any(|v| v.contains(','))
    {
        return Err("an extruder with several variants, which the replay does not rebuild".into());
    }
    if count > 1 {
        pick_single_variant(
            &mut cfg,
            PRINTER_OPTIONS_WITH_VARIANT_2,
            "printer_extruder_variant",
            2,
            count,
        )?;
    }
    update_values_to_printer_extruders(&mut cfg);
    if count > 1 {
        pick_single_variant(
            &mut cfg,
            PRINT_OPTIONS_WITH_VARIANT,
            "print_extruder_variant",
            1,
            count,
        )?;
    }
    update_values_to_printer_extruders_for_multiple_filaments(&mut cfg);
    Ok(cfg)
}

/// `printer_options_with_variant_2` (PrintConfig.cpp), each two values (normal and silent) per
/// variant.
const PRINTER_OPTIONS_WITH_VARIANT_2: &[&str] = &[
    "machine_max_acceleration_e",
    "machine_max_acceleration_extruding",
    "machine_max_acceleration_retracting",
    "machine_max_acceleration_travel",
    "machine_max_acceleration_x",
    "machine_max_acceleration_y",
    "machine_max_acceleration_z",
    "machine_max_jerk_e",
    "machine_max_jerk_x",
    "machine_max_jerk_y",
    "machine_max_jerk_z",
    "machine_max_speed_e",
    "machine_max_speed_x",
    "machine_max_speed_y",
    "machine_max_speed_z",
];

/// `print_options_with_variant` (PrintConfig.cpp).
const PRINT_OPTIONS_WITH_VARIANT: &[&str] = &["print_extruder_id", "print_extruder_variant"];

/// `update_values_to_printer_extruders(c, keys, id_name, variant_name, stride)` on a printer of
/// `count` extruders whose list `variant_name` holds one variant: every extruder takes variant 0
/// (its own, or Orca's fallback for one the list lacks), so each option becomes its first
/// `stride` values once per extruder (`get_at`, the first value past the end). A list of several
/// variants is not rebuilt.
fn pick_single_variant(
    cfg: &mut Config,
    keys: &[&str],
    variant_name: &str,
    stride: usize,
    count: usize,
) -> Result<(), String> {
    if cfg.strings(variant_name).map_or(0, Vec::len) != 1 {
        return Err(format!(
            "{variant_name} of several variants, which the replay does not rebuild"
        ));
    }
    fn pick<T: Clone>(values: &[T], stride: usize, count: usize) -> Option<Vec<T>> {
        let mut out = Vec::with_capacity(stride * count);
        for _ in 0..count {
            for i in 0..stride {
                out.push(values.get(i).or_else(|| values.first())?.clone());
            }
        }
        Some(out)
    }
    for key in keys {
        let picked = match cfg.get_mut(key) {
            None => continue,
            Some(OptionValue::Floats { values, .. }) => {
                pick(values, stride, count).map(|p| *values = p)
            }
            Some(OptionValue::Ints { values, .. }) => {
                pick(values, stride, count).map(|p| *values = p)
            }
            Some(OptionValue::Strings(values)) => pick(values, stride, count).map(|p| *values = p),
            Some(_) => return Err(format!("{key}: a type the replay does not pick")),
        };
        if picked.is_none() {
            return Err(format!("{key}: empty, which get_at reads past"));
        }
    }
    Ok(())
}

/// `PrintObject::object_config_from_model_object` over the defaults with the full config: the
/// model object's settings normalised over them, the support filaments past the printer's set to
/// 1.
fn object_config(
    full: &Config,
    object: &Config,
    num_extruders: usize,
) -> Result<PrintObjectConfig, String> {
    let mut config = PrintObjectConfig::default();
    config.apply_config(full, true).map_err(|e| e.0)?;
    let mut src = object.clone();
    normalize_fdm(&mut src);
    config.apply_config(&src, true).map_err(|e| e.0)?;
    let n = i32::try_from(num_extruders).unwrap_or(i32::MAX);
    for opt in [
        &mut config.support_filament,
        &mut config.support_interface_filament,
    ] {
        if opt.value > n {
            opt.value = 1;
        }
    }
    Ok(config)
}

/// The six feature filament settings `apply_to_print_region_config` sets only when positive.
const FEATURE_FILAMENTS: [&str; 6] = [
    "sparse_infill_filament_id",
    "internal_solid_filament_id",
    "top_surface_filament_id",
    "bottom_surface_filament_id",
    "outer_wall_filament_id",
    "inner_wall_filament_id",
];

/// `region_config_from_model_volume` of a model part with no settings of its own, over the
/// defaults with the full config: the model object's settings over them (a feature filament only
/// when positive; the probe's objects set no `extruder`), the feature filaments made valid, the
/// sparse infill density bounded and fuzzy skin off when too fine.
fn region_config(
    full: &Config,
    object: &Config,
    num_extruders: usize,
) -> Result<PrintRegionConfig, String> {
    let mut config = PrintRegionConfig::default();
    config.apply_config(full, true).map_err(|e| e.0)?;
    if object.has("extruder") {
        return Err("a model object that sets extruder, which the replay does not rebuild".into());
    }
    let mut src = Config::default();
    for (k, v) in object.iter() {
        if FEATURE_FILAMENTS.contains(&k.as_str()) && object.int(k).unwrap_or(0) <= 0 {
            continue;
        }
        src.insert(k, v.clone());
    }
    config.apply_config(&src, true).map_err(|e| e.0)?;
    let n = i32::try_from(num_extruders).unwrap_or(i32::MAX);
    for opt in [
        &mut config.sparse_infill_filament_id,
        &mut config.outer_wall_filament_id,
        &mut config.inner_wall_filament_id,
        &mut config.internal_solid_filament_id,
        &mut config.top_surface_filament_id,
        &mut config.bottom_surface_filament_id,
    ] {
        if opt.value <= 0 || opt.value > n {
            opt.value = 1;
        }
    }
    let density = &mut config.sparse_infill_density.value;
    *density = if *density < f64::from(0.00011f32) {
        0.
    } else {
        orca_stl::min(*density, 100.)
    };
    if config.fuzzy_skin.value != FuzzySkinType::None
        && (config.fuzzy_skin_point_distance.value < 0.01
            || config.fuzzy_skin_thickness.value < 0.001)
    {
        config.fuzzy_skin.value = FuzzySkinType::None;
    }
    Ok(config)
}

/// A print object `print_objects_from_model_object` makes: its model object, its printable
/// instances (their indices in the model object's) and their scale.
struct Group {
    model_object: usize,
    instances: Vec<usize>,
    scale: [f64; 3],
}

/// `transform3d_lower` of two instance matrices that differ only by their scale (no instance
/// turns, and the offset is reset): the scales in turn.
fn scale_lower(a: &[f64; 3], b: &[f64; 3]) -> bool {
    for i in 0..3 {
        if a[i] < b[i] {
            return true;
        }
        if a[i] > b[i] {
            return false;
        }
    }
    false
}

/// The print objects of the model objects, in order: one per distinct transformation of each
/// model object's printable instances, in `std::set` order.
fn object_groups(objs: &[ObjSpec]) -> Vec<Group> {
    let mut out = Vec::new();
    for (mo, o) in objs.iter().enumerate() {
        let mut set: Vec<Group> = Vec::new();
        for (ii, inst) in o.instances.iter().enumerate() {
            if !inst.printable {
                continue;
            }
            let scale = [inst.sx, inst.sy, inst.sz];
            let group = Group {
                model_object: mo,
                instances: vec![ii],
                scale,
            };
            match set.iter().position(|g| !scale_lower(&g.scale, &scale)) {
                Some(at) if !scale_lower(&scale, &set[at].scale) => set[at].instances.push(ii),
                Some(at) => set.insert(at, group),
                None => set.push(group),
            }
        }
        out.extend(set);
    }
    out
}

/// The model the print copies: each object's name, input file and instances (scale, offset and
/// printable flag).
fn case_model(objs: &[ObjSpec]) -> Model {
    let mut model = Model::default();
    for o in objs {
        let mut mo = ModelObject::default();
        mo.name.clone_from(&o.name);
        mo.input_file.clone_from(&o.input_file);
        for i in &o.instances {
            let mut mi = ModelInstance::default();
            mi.set_offset(&Vec3d::new(i.ox, i.oy, 0.));
            mi.set_scaling_factor(&Vec3d::new(i.sx, i.sy, i.sz));
            mi.printable = i.printable;
            mo.instances.push(mi);
        }
        model.objects.push(mo);
    }
    model
}

/// One print case as the replay rebuilds it, for the records after its LYPC.
struct Case {
    print: Print,
    printable: bool,
}

/// What the layer-3 replays read but do not compare, or cannot run yet.
#[derive(Default)]
struct Skipped {
    /// LYPC's results of `Print::apply` (orca-o3-print-2): its status, each object's size and
    /// extruders.
    apply: usize,
    /// LYPH's output file names of a format (`PlaceholderParser::process`, orca-gcode, phase O4).
    process: usize,
}

fn lypc(c: &mut Cmp<'_>, cases: &mut BTreeMap<u32, Case>, skipped: &mut Skipped) -> Option<()> {
    let (case_id, _label, _domain_a, kv) =
        c.input(|r| Ok((r.u32()?, r.str()?, r.bool()?, rd_kv(r)?)))?;
    let objs = c.input(|r| r.vec(rd_obj_spec))?;
    c.input(|r| rd_skipped_attempt(r, Reader::u32))?;
    skipped.apply += 1;
    let full = match apply_full_config(&kv) {
        Ok(f) => f,
        Err(e) => return c.fail(e),
    };
    let mut print = Print::new();
    if let Err(e) = print.config.apply_config(&full, true) {
        return c.fail(format!("the print config: {}", e.0));
    }
    let num_extruders = print.config.gcode.filament_diameter.values.len();
    print.model = case_model(&objs);
    let groups = object_groups(&objs);
    let n = c.input(Reader::count)?;
    c.check("the number of print objects", n == groups.len())?;
    for (oi, g) in groups.iter().enumerate() {
        let shifts = c.input(|r| r.vec(rd_point))?;
        c.check(
            &format!("print object {oi}'s number of instances"),
            shifts.len() == g.instances.len(),
        )?;
        let size = c.input(|r| Ok((r.i64()?, r.i64()?, r.i64()?)))?;
        let object_kv = match config_of(&objs[g.model_object].config) {
            Ok(o) => o,
            Err(e) => return c.fail(e),
        };
        let oc = match object_config(&full, &object_kv, num_extruders) {
            Ok(o) => o,
            Err(e) => return c.fail(format!("print object {oi}'s config: {e}")),
        };
        c.part(&format!("print object {oi}'s config hash"), |w| {
            w.u64(oc.hash());
        })?;
        let rc = match region_config(&full, &object_kv, num_extruders) {
            Ok(r) => r,
            Err(e) => return c.fail(format!("print object {oi}'s region config: {e}")),
        };
        let rc_hash = rc.hash();
        c.part(
            &format!("print object {oi}'s regions' config hashes"),
            |w| {
                w.len(1).u64(rc_hash);
            },
        )?;
        c.input(|r| r.vec(Reader::u32))?;
        skipped.apply += 2;
        let mut obj = PrintObject::with_members(g.model_object, Transform3d::identity());
        obj.config = oc;
        obj.size = Vec3crd::new(size.0, size.1, size.2);
        for (k, &mi) in g.instances.iter().enumerate() {
            obj.instances.push(PrintInstance::new(mi, shifts[k]));
        }
        obj.shared_regions = Some(Arc::new(PrintObjectRegions {
            all_regions: vec![PrintRegion::new(rc, rc_hash, 0)],
            ..PrintObjectRegions::default()
        }));
        print.objects.push(obj);
    }
    let chain = chain_print_object_instances_of_print(&print);
    c.part("chain_print_object_instances(print)", |w| {
        w.len(chain.len());
        for &(oi, ii) in &chain {
            w.len(oi).len(ii);
        }
    })?;
    c.part("the print config's hash and has_brim()", |w| {
        w.u64(print.config.hash()).boolean(has_brim(&print));
    })?;
    let printable = objs.iter().any(|o| o.instances.iter().any(|i| i.printable));
    cases.insert(case_id, Case { print, printable });
    Some(())
}

/// The case and print object a record names.
fn case_object<'c>(
    c: &mut Cmp<'_>,
    cases: &'c BTreeMap<u32, Case>,
    case_id: u32,
    oi: u64,
) -> Option<(&'c Print, &'c PrintObject)> {
    let Some(case) = cases.get(&case_id) else {
        c.fail(format!("case {case_id}, whose LYPC did not replay"));
        return None;
    };
    let Some(obj) = usize::try_from(oi)
        .ok()
        .and_then(|i| case.print.objects.get(i))
    else {
        c.fail(format!(
            "print object {oi} of case {case_id}, which it lacks"
        ));
        return None;
    };
    Some((&case.print, obj))
}

fn lypr(c: &mut Cmp<'_>, cases: &BTreeMap<u32, Case>) -> Option<()> {
    let (case_id, oi, ri) = c.input(|r| Ok((r.u32()?, r.u64()?, r.u64()?)))?;
    let (print, obj) = case_object(c, cases, case_id, oi)?;
    let Some(ri) = usize::try_from(ri)
        .ok()
        .filter(|&i| i < obj.num_printing_regions())
    else {
        return c.fail(format!("region {ri}, which the object lacks"));
    };
    let region = obj.printing_region(ri);
    c.part("config_hash()", |w| {
        w.u64(region.config_hash());
    })?;
    c.part("extruder(role) of each role", |w| {
        for role in FlowRole::ALL {
            w.attempt(region.extruder(role), |w, v| {
                w.u32(v);
            });
        }
    })?;
    let heights = [obj.config().layer_height.value, 0.1, 0.];
    for role in FlowRole::ALL {
        c.part(
            &format!("flow(object, {role:?}, h, first) of each height and layer kind"),
            |w| {
                for h in heights {
                    for first in [false, true] {
                        w.attempt(region.flow(obj, print, role, h, first), |w, f| {
                            w.flow(&f);
                        });
                    }
                }
            },
        )?;
    }
    c.part("nozzle_dmr_avg and bridging_height_avg", |w| {
        w.f64(region.nozzle_dmr_avg(print.config()));
        w.f64(region.bridging_height_avg(print.config()));
    })?;
    let mut ex = Vec::new();
    region.collect_object_printing_extruders(print, &mut ex);
    c.part("collect_object_printing_extruders(print)", |w| {
        w.len(ex.len());
        for &e in &ex {
            w.u32(e);
        }
    })?;
    for brim in [false, true] {
        let mut e2 = Vec::new();
        PrintRegion::collect_object_printing_extruders_of(
            print.config(),
            region.config(),
            brim,
            &mut e2,
        );
        c.part(
            &format!("collect_object_printing_extruders(config, region config, {brim})"),
            |w| {
                w.len(e2.len());
                for &e in &e2 {
                    w.u32(e);
                }
            },
        )?;
    }
    Some(())
}

fn lysf(c: &mut Cmp<'_>, cases: &BTreeMap<u32, Case>) -> Option<()> {
    let (case_id, oi) = c.input(|r| Ok((r.u32()?, r.u64()?)))?;
    let (print, obj) = case_object(c, cases, case_id, oi)?;
    let flow_try = |w: &mut W, f: Result<Flow, FlowError>| {
        w.attempt(f, |w, f| {
            w.flow(&f);
        });
    };
    for h in [0.0f32, 0.15] {
        c.part(&format!("support_material_flow(object, {h})"), |w| {
            flow_try(w, support_material_flow(obj, print, h));
        })?;
    }
    c.part("support_transition_flow(object)", |w| {
        flow_try(w, Ok(support_transition_flow(obj, print)));
    })?;
    for h in [0.0f32, 0.25] {
        c.part(
            &format!("support_material_1st_layer_flow(object, {h})"),
            |w| {
                flow_try(w, support_material_1st_layer_flow(obj, print, h));
            },
        )?;
    }
    for h in [0.0f32, 0.15] {
        c.part(
            &format!("support_material_interface_flow(object, {h})"),
            |w| {
                flow_try(w, support_material_interface_flow(obj, print, h));
            },
        )?;
    }
    Some(())
}

fn lyph(c: &mut Cmp<'_>, cases: &BTreeMap<u32, Case>, skipped: &mut Skipped) -> Option<()> {
    let case_id = c.input(Reader::u32)?;
    let Some(case) = cases.get(&case_id) else {
        return c.fail(format!("case {case_id}, whose LYPC did not replay"));
    };
    let print = &case.print;
    let mut cfg = Config::default();
    print.update_object_placeholders(&mut cfg, ".gcode");
    c.part("update_object_placeholders(config, \".gcode\")", |w| {
        let keys = cfg.keys();
        w.len(keys.len());
        for k in &keys {
            let v = cfg
                .opt_serialize(k)
                .unwrap_or_else(|e| format!("<{}>", e.0));
            w.str(k).str(&v);
        }
    })?;
    let n = c.input(Reader::count)?;
    for _ in 0..n {
        let (f, b) = c.input(|r| Ok((r.str()?, r.str()?)))?;
        if f.is_empty() && b.is_empty() && !case.printable {
            // The probe leaves out the call that reads an unset input_filename_base.
            c.part("the call left out", |w| {
                w.u8(2);
            })?;
            continue;
        }
        match catch_unwind(AssertUnwindSafe(|| {
            print.output_filename(&f, ".gcode", &b, None)
        })) {
            Ok(res) => {
                c.part(&format!("output_filename({f:?}, \".gcode\", {b:?})"), |w| {
                    w.attempt(res, |w, s| {
                        w.str(&s);
                    });
                })?;
            }
            Err(payload) => {
                let at_process = payload
                    .downcast_ref::<NotPorted>()
                    .is_some_and(|stop| stop.function == PLACEHOLDER_PROCESS);
                if !at_process {
                    resume_unwind(payload);
                }
                skipped.process += 1;
                c.input(|r| rd_skipped_attempt(r, Reader::str))?;
            }
        }
    }
    Some(())
}

fn lypt(c: &mut Cmp<'_>, cases: &BTreeMap<u32, Case>) -> Option<()> {
    let case_id = c.input(Reader::u32)?;
    let Some(case) = cases.get(&case_id) else {
        return c.fail(format!("case {case_id}, whose LYPC did not replay"));
    };
    let print = &case.print;
    c.part("get_filament_maps()", |w| {
        let maps = print.get_filament_maps();
        w.len(maps.len());
        for &m in &maps {
            w.i32(m);
        }
    })?;
    c.part("get_filament_map_mode()", |w| {
        w.u32(print.get_filament_map_mode() as u32);
    })?;
    c.part("get_extruder_printable_height()", |w| {
        let heights = print.get_extruder_printable_height();
        w.len(heights.len());
        for &h in &heights {
            w.f64(h);
        }
    })?;
    c.part("get_extruder_printable_polygons()", |w| {
        let polys = print.get_extruder_printable_polygons();
        w.len(polys.len());
        for ps in &polys {
            w.polygons(ps);
        }
    })?;
    c.part("get_extruder_unprintable_polygons()", |w| {
        let polys = print.get_extruder_unprintable_polygons();
        w.len(polys.len());
        for ps in &polys {
            w.polygons(ps);
        }
    })?;
    c.part("get_extruder_id(0..4)", |w| {
        for f in 0..4 {
            w.len(print.get_extruder_id(f));
        }
    })
}

/// LYLA: the layer built with its regions, which are appended to the object's printing regions
/// for the replay and taken off after it.
fn lyla(c: &mut Cmp<'_>, cases: &mut BTreeMap<u32, Case>) -> Option<()> {
    let (case_id, id, height, print_z, slice_z, nregions) =
        c.input(|r| Ok((r.u32()?, r.u64()?, r.f64()?, r.f64()?, r.f64()?, r.count()?)))?;
    let Some(case) = cases.get_mut(&case_id) else {
        return c.fail(format!("case {case_id}, whose LYPC did not replay"));
    };
    let Some(obj) = case.print.objects.first_mut() else {
        return c.fail(format!("case {case_id} has no print object"));
    };
    let Ok(id) = usize::try_from(id) else {
        return c.fail(format!("a layer id of {id}"));
    };
    let mut layer = Layer::new(id, height, print_z, slice_z);
    let base_config = obj.printing_region(0).config().clone();
    let first = obj.num_printing_regions();
    let mut regions = Vec::new();
    for r in 0..nregions {
        let ov = c.input(rd_kv)?;
        let mut rc = base_config.clone();
        for (k, v) in &ov {
            if let Err(e) = rc.set_deserialize_strict(k, v) {
                return c.fail(format!("set_deserialize_strict({k}, {v}): {}", e.0));
            }
        }
        let rc_hash = rc.hash();
        c.part(&format!("region {r}'s config hash"), |w| {
            w.u64(rc_hash);
        })?;
        let slices = surfaces_input(c, "slices")?;
        let at = c.at;
        let fill = c.input(|q| q.vec(rd_expolygon))?;
        c.echo(at, "fill_expolygons", |w| {
            w.expolygons(&fill);
        })?;
        regions.push(PrintRegion::new(rc, rc_hash, -1));
        let lr = layer.add_region(first + r);
        lr.slices.surfaces = slices;
        lr.fill_expolygons = fill;
    }
    let saved = obj.shared_regions.clone();
    let mut all = obj.shared_regions().cloned().unwrap_or_default();
    all.all_regions.extend(regions);
    obj.shared_regions = Some(Arc::new(all));
    let result = lyla_layer(c, &case.print, &mut layer);
    if let Some(obj) = case.print.objects.first_mut() {
        obj.shared_regions = saved;
    }
    result
}

/// What LYLA records of a layer once its regions are in place.
fn lyla_layer(c: &mut Cmp<'_>, print: &Print, layer: &mut Layer) -> Option<()> {
    for (i, lr) in layer.regions.iter_mut().enumerate() {
        lr.slices_to_fill_surfaces_clipped();
        c.part(
            &format!("region {i}'s fill_surfaces after slices_to_fill_surfaces_clipped()"),
            |w| {
                w.surfaces(&lr.fill_surfaces.surfaces);
            },
        )?;
    }
    c.part("empty()", |w| {
        w.boolean(layer.empty());
    })?;
    layer.make_slices();
    let layer_now: &Layer = layer;
    lyla_reads(c, print, layer_now)?;
    layer.backup_untyped_slices();
    for (i, lr) in layer.regions().iter().enumerate() {
        c.part(
            &format!("region {i}'s raw_slices after backup_untyped_slices()"),
            |w| {
                w.expolygons(&lr.raw_slices);
            },
        )?;
    }
    layer.restore_untyped_slices();
    for (i, lr) in layer.regions().iter().enumerate() {
        c.part(
            &format!("region {i}'s slices after restore_untyped_slices()"),
            |w| {
                w.surfaces(&lr.slices.surfaces);
            },
        )?;
    }
    layer.restore_untyped_slices_no_extra_perimeters();
    for (i, lr) in layer.regions().iter().enumerate() {
        c.part(
            &format!("region {i}'s slices after restore_untyped_slices_no_extra_perimeters()"),
            |w| {
                w.surfaces(&lr.slices.surfaces);
            },
        )?;
    }
    c.part("get_extruder_id(0) and get_extruder_id(1)", |w| {
        w.len(layer.get_extruder_id(print, 0));
        w.len(layer.get_extruder_id(print, 1));
    })
}

/// What LYLA records of a layer between `make_slices` and `backup_untyped_slices`.
fn lyla_reads(c: &mut Cmp<'_>, print: &Print, layer: &Layer) -> Option<()> {
    let obj = &print.objects[0];
    c.part("lslices after make_slices()", |w| {
        w.expolygons(&layer.lslices);
    })?;
    for off in [0., 0.1, 1.0] {
        c.part(&format!("merged(scale_({off}))"), |w| {
            w.expolygons(&layer.merged(obj, scale_(off) as f32));
        })?;
    }
    for (i, lr) in layer.regions().iter().enumerate() {
        c.part(&format!("get_extents(region {i})"), |w| {
            w.bbox(&get_extents_layer_region(lr));
        })?;
    }
    c.part("get_extents(regions)", |w| {
        w.bbox(&get_extents_layer_regions(layer.regions()));
    })?;
    let flow_try = |w: &mut W, f: Result<Flow, FlowError>| {
        w.attempt(f, |w, f| {
            w.flow(&f);
        });
    };
    for (i, lr) in layer.regions().iter().enumerate() {
        for role in FlowRole::ALL {
            c.part(
                &format!("region {i}'s flow({role:?}), flow({role:?}, 0.3) and bridging flows"),
                |w| {
                    flow_try(w, lr.flow(layer, obj, print, role));
                    flow_try(w, lr.flow_at(layer, obj, print, role, 0.3));
                    flow_try(w, lr.bridging_flow(layer, obj, print, role, false));
                    flow_try(w, lr.bridging_flow(layer, obj, print, role, true));
                },
            )?;
        }
    }
    c.part("get_sparse_infill_max_void_area()", |w| {
        w.attempt(layer.get_sparse_infill_max_void_area(obj, print), |w, v| {
            w.f64(v);
        });
    })
}

/// Keeps the expected stops (`NotPorted` payloads, caught above) out of the test's output.
fn quiet_stops() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if info.payload().downcast_ref::<NotPorted>().is_none() {
            default(info);
        }
    }));
}

#[test]
fn layer_probe_replays() {
    quiet_stops();
    let file = load_probe("layer").unwrap_or_else(|e| panic!("{e}"));
    let full = file.set == ProbeSet::Full;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for rec in &file.records {
        *counts.entry(rec.kind().to_string()).or_default() += 1;
    }
    for kind in counts.keys() {
        assert!(
            OWN.iter().any(|(k, _, _)| k == kind) || ORCA_CONFIG.contains(&kind.as_str()),
            "layer.sop1 holds {kind} records, which neither orca-config nor orca-layer replays"
        );
    }
    for (kind, committed, all) in OWN {
        let want = if full { *all } else { *committed };
        let got = counts.get(*kind).copied().unwrap_or(0);
        assert_eq!(got, want, "{kind} records in the {} set", file.set.name());
    }
    let mut m = Mismatches::default();
    let mut cases: BTreeMap<u32, Case> = BTreeMap::new();
    let mut skipped = Skipped::default();
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for rec in &file.records {
        let kind = rec.kind().to_string();
        if !OWN.iter().any(|(k, _, _)| *k == kind) {
            continue;
        }
        let n = seen.entry(kind.clone()).or_default();
        let mut c = Cmp::new(&rec.payload, format!("{kind} record {n}"));
        *n += 1;
        let _ = match kind.as_str() {
            "LYFL" => lyfl(&mut c),
            "LYFS" => lyfs(&mut c),
            "LYFW" => lyfw(&mut c),
            "LYRO" => lyro(&mut c),
            "LYEN" => lyen(&mut c),
            "LYEM" => lyem(&mut c),
            "LYEL" => lyel(&mut c),
            "LYES" => lyes(&mut c),
            "LYEC" => lyec(&mut c),
            "LYEA" => lyea(&mut c),
            "LYVW" => lyvw(&mut c),
            "LYSU" => lysu(&mut c, *n == counts["LYSU"]),
            "LYSC" => lysc(&mut c),
            "LYCH" => lych(&mut c),
            "LYPC" => lypc(&mut c, &mut cases, &mut skipped),
            "LYPR" => lypr(&mut c, &cases),
            "LYSF" => lysf(&mut c, &cases),
            "LYPH" => lyph(&mut c, &cases, &mut skipped),
            "LYPT" => lypt(&mut c, &cases),
            "LYLA" => lyla(&mut c, &mut cases),
            _ => unreachable!("an own kind without a replay"),
        };
        c.finish(&mut m);
    }
    println!(
        "layer.sop1 ({} set): {} comparisons over {} orca-layer records; read, not compared: {} \
         results of Print::apply (orca-o3-print-2's: its status, the objects' sizes and \
         extruders); {} output file names of a format await PlaceholderParser::process \
         (orca-gcode, phase O4)",
        file.set.name(),
        m.checked,
        OWN.iter()
            .map(|(k, _, _)| counts.get(*k).copied().unwrap_or(0))
            .sum::<usize>(),
        skipped.apply,
        skipped.process
    );
    m.assert_none("layer.sop1 (orca-layer)");
}
