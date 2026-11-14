// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The probe gate for orca-fill: the records of `fill.sop1` (`parity/sop1/fill.md`) that the
//! pieces fill-1 and fill-2 own, each rebuilt whole from the crate and compared byte for byte with
//! the C++ engine's (added 2026-11-14 by orca-o3-fill-1; fill-2's kinds the same day by
//! orca-o3-fill-2):
//!
//! - FLDF: `FillParams{}`, its `full_infill()` and `dont_connect()`, `Fill::infill_anchor` and
//!   `Fill::infill_anchor_max`, a new filler and `Surface()`;
//! - FLPA: `full_infill()` and `dont_connect()` of a density and a longest anchor;
//! - FLAS: `Fill::_adjust_solid_spacing`;
//! - FLDR: `Fill::_infill_direction`, with what it prints on stdout, and `Fill::_layer_angle` of
//!   the filler's layer and the next (the layer id plus one, wrapping);
//! - FLSU: `Fill::fill_surface` (entry 0) and `Fill::fill_surface_arachne` (entry 1), through a
//!   filler of the test's own ([`Spy`], the probe's `SpyFill`) whose `_fill_surface_single` keeps
//!   the thickness in layers, the direction and the region it is given, with what the call prints;
//! - FLCN (fill-2): `Fill::connect_infill` of an island (entry 0) and of its polygons with a box
//!   (entry 1), and `Fill::chain_or_connect_infill` (entry 2);
//! - FLEB (fill-2): `Fill::extended_object_bounding_box`;
//! - FLGP (fill-2): `Fill::_create_gap_fill` on a collection that holds the paths of the record's
//!   lines, with what the engine logs at error or fatal;
//! - FLEX (fill-2): `Fill::fill_surface_extrusion` through a filler of the test's own ([`Stub`],
//!   the probe's `StubFill`) whose `fill_surface` and `fill_surface_arachne` set a spacing and
//!   return the record's lines or throw, with the log.
//!
//! A record holds its inputs and then the engine's results; the test reads the inputs, runs the
//! crate and writes the whole record again (the probe's `put` of each value), so the defaults the
//! record holds are checked too. What the crate prints on stdout (orca-fmt's C `stdout`) goes to
//! a sink ([`orca_fmt::stdio::set_stdout_sink`]) held by one test at a time; what it logs
//! (orca-mesh's `BOOST_LOG_TRIVIAL`) goes to a sink that hands each record at error or fatal to
//! the capture of the thread that wrote it, as the probe's `ErrorLog` writes them. fill-3 and
//! fill-4 replay the other kinds; until they do, this file only checks that no other kind is in
//! the file.
//! The committed subset (`parity/probes/`) runs by default; `SKETCH_ORCA_PROBES_DEEP=<folder>`
//! replays the full set from that folder (`<work>/parity/probes`).

use std::cell::RefCell;
use std::sync::{Arc, Mutex, Once, OnceLock, PoisonError};

use orca_config::ConfigEnum;
use orca_config::print_config::{
    GapFillTarget, InfillPattern, PrintConfig, PrintObjectConfig, PrintRegionConfig,
};
use orca_fill::base::{
    Fill, FillBase, FillError, FillParams, INFILL_ANCHOR, INFILL_ANCHOR_MAX, adjust_solid_spacing,
    connect_infill_expolygon, connect_infill_polygons,
};
use orca_fill::connect::chain_or_connect_infill;
use orca_fill::extrusion::create_gap_fill;
use orca_fmt::stdio::set_stdout_sink;
use orca_geom::arc_fitter::{EMovePathType, PathFittingData};
use orca_geom::bounding_box::BoundingBox;
use orca_geom::circle::{ArcDirection, ArcSegment};
use orca_geom::expolygon::{ExPolygon, to_polygons};
use orca_geom::point::{Point, Points};
use orca_geom::polygon::Polygon;
use orca_geom::polyline::{Polyline, Polylines, ThickPolyline, ThickPolylines};
use orca_layer::collection::ExtrusionEntityCollection;
use orca_layer::entity::{ExtrusionEntitiesPtr, ExtrusionEntity, extrusion_entities_append_paths};
use orca_layer::extrusion_loop::{ExtrusionLoop, ExtrusionLoopSloped};
use orca_layer::flow::Flow;
use orca_layer::multi_path::ExtrusionMultiPath;
use orca_layer::path::{ExtrusionPath, ExtrusionPathSloped};
use orca_layer::role::ExtrusionRole;
use orca_layer::surface::{Surface, SurfaceType};
use orca_mesh::log::Severity;
use orca_probe::{Mismatches, ProbeFile, Reader, Record, load_probe};

/// The probe file, read once.
fn file() -> &'static ProbeFile {
    static FILE: OnceLock<ProbeFile> = OnceLock::new();
    FILE.get_or_init(|| load_probe("fill").unwrap_or_else(|e| panic!("{e}")))
}

/// The kinds this file replays: fill-1's, then fill-2's.
const OWN_KINDS: [&str; 9] = [
    "FLDF", "FLPA", "FLAS", "FLDR", "FLSU", "FLCN", "FLEB", "FLGP", "FLEX",
];
/// The kinds fill-3 and fill-4 replay.
const OTHER_PIECES_KINDS: [&str; 5] = ["FLRP", "FLCP", "FLNT", "FLNM", "FLRA"];

// ---------------------------------------------------------------- reading the inputs

fn point(r: &mut Reader<'_>) -> Point {
    let x = r.i64().unwrap();
    let y = r.i64().unwrap();
    Point::new(x, y)
}

fn points(r: &mut Reader<'_>) -> Points {
    let n = r.count().unwrap();
    (0..n).map(|_| point(r)).collect()
}

fn expolygon(r: &mut Reader<'_>) -> ExPolygon {
    let mut e = ExPolygon::new(Polygon::new(points(r)));
    let holes = r.count().unwrap();
    for _ in 0..holes {
        e.holes.push(Polygon::new(points(r)));
    }
    e
}

fn bounding_box(r: &mut Reader<'_>) -> BoundingBox {
    let mut b = BoundingBox::new();
    b.min = point(r);
    b.max = point(r);
    b.defined = r.bool().unwrap();
    b
}

fn surface(r: &mut Reader<'_>) -> Surface {
    let surface_type = r.u32().unwrap();
    let surface_type = SurfaceType::from_i32(i32::try_from(surface_type).unwrap())
        .unwrap_or_else(|| panic!("a surface type of {surface_type}"));
    let mut s = Surface::with_expolygon(surface_type, expolygon(r));
    s.thickness = r.f64().unwrap();
    s.thickness_layers = r.u16().unwrap();
    s.bridge_angle = r.f64().unwrap();
    s.extra_perimeters = r.u16().unwrap();
    s
}

/// The configs a record's filler (or its params) points at when its flag is set and the record
/// holds no configs of its own: the defaults, as the probe's `Configs`.
struct Configs {
    print: PrintConfig,
    object: PrintObjectConfig,
    region: PrintRegionConfig,
}

fn configs() -> &'static Configs {
    static CONFIGS: OnceLock<Configs> = OnceLock::new();
    CONFIGS.get_or_init(|| Configs {
        print: PrintConfig::default(),
        object: PrintObjectConfig::default(),
        region: PrintRegionConfig::default(),
    })
}

/// A record's `filler`: the fields of a [`FillBase`], its configs pointed at the defaults when its
/// flag is set.
fn filler(r: &mut Reader<'_>) -> FillBase<'static> {
    let (mut b, has_configs) = filler_fields(r);
    if has_configs {
        b.print_config = Some(&configs().print);
        b.print_object_config = Some(&configs().object);
    }
    b
}

/// A record's `filler` without its configs, and whether it points at them.
fn filler_fields(r: &mut Reader<'_>) -> (FillBase<'static>, bool) {
    let mut b = FillBase::new();
    b.layer_id = usize::try_from(r.u64().unwrap()).unwrap();
    b.z = r.f64().unwrap();
    b.spacing = r.f64().unwrap();
    b.overlap = r.f64().unwrap();
    b.angle = r.f32().unwrap();
    b.fixed_angle = r.bool().unwrap();
    b.link_max_length = r.i64().unwrap();
    b.loop_clipping = r.i64().unwrap();
    b.bounding_box = bounding_box(r);
    b.dont_alternate_fill_direction = r.bool().unwrap();
    let n = r.count().unwrap();
    b.no_overlap_expolygons = (0..n).map(|_| expolygon(r)).collect();
    let has_configs = r.bool().unwrap();
    (b, has_configs)
}

fn fitting(r: &mut Reader<'_>) -> Vec<PathFittingData> {
    let n = r.count().unwrap();
    (0..n)
        .map(|_| {
            let start = usize::try_from(r.u64().unwrap()).unwrap();
            let end = usize::try_from(r.u64().unwrap()).unwrap();
            let path_type = match r.u8().unwrap() {
                0 => EMovePathType::NoopMove,
                1 => EMovePathType::LinearMove,
                2 => EMovePathType::ArcMoveCw,
                3 => EMovePathType::ArcMoveCcw,
                v => panic!("a move type of {v}"),
            };
            let is_arc = r.bool().unwrap();
            let center = point(r);
            let radius = r.f64().unwrap();
            let length = r.f64().unwrap();
            let angle_radians = r.f64().unwrap();
            let polar_start_theta = r.f64().unwrap();
            let polar_end_theta = r.f64().unwrap();
            let start_point = point(r);
            let end_point = point(r);
            let direction = match r.u8().unwrap() {
                0 => ArcDirection::Unknown,
                1 => ArcDirection::Ccw,
                2 => ArcDirection::Cw,
                v => panic!("an arc direction of {v}"),
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
            PathFittingData::new(start, end, path_type, arc)
        })
        .collect()
}

fn polyline(r: &mut Reader<'_>) -> Polyline {
    let points = points(r);
    let fitting_result = fitting(r);
    Polyline {
        points,
        fitting_result,
    }
}

fn polylines(r: &mut Reader<'_>) -> Polylines {
    let n = r.count().unwrap();
    (0..n).map(|_| polyline(r)).collect()
}

fn thick_polylines(r: &mut Reader<'_>) -> ThickPolylines {
    let n = r.count().unwrap();
    (0..n)
        .map(|_| {
            let polyline = polyline(r);
            let widths = r.count().unwrap();
            let width = (0..widths).map(|_| r.f64().unwrap()).collect();
            let first = r.bool().unwrap();
            let second = r.bool().unwrap();
            ThickPolyline {
                polyline,
                width,
                endpoints: (first, second),
            }
        })
        .collect()
}

/// A flow as the record writes it (width, height, spacing, nozzle, bridge), made again: a bridge
/// by `bridging_flow`, an all-zero flow by `Flow()`, else by `Flow(width, height, nozzle)`; then
/// the record's spacing set.
fn flow(r: &mut Reader<'_>) -> Flow {
    let width = r.f32().unwrap();
    let height = r.f32().unwrap();
    let spacing = r.f32().unwrap();
    let nozzle = r.f32().unwrap();
    let bridge = r.bool().unwrap();
    let mut f = if bridge {
        Flow::bridging_flow(width, nozzle)
    } else if width == 0. && height == 0. && nozzle == 0. {
        Flow::default()
    } else {
        Flow::new(width, height, nozzle).unwrap_or_else(|e| panic!("the record's flow: {e}"))
    };
    f.set_spacing(spacing);
    f
}

/// A record's params, without their region config, and whether the record's had one.
fn params(r: &mut Reader<'_>) -> (FillParams<'static>, bool) {
    // The fields in the record's order (a struct expression evaluates its fields as written).
    let has_config;
    let q = FillParams {
        density: r.f32().unwrap(),
        multiline: r.i32().unwrap(),
        anchor_length: r.f32().unwrap(),
        anchor_length_max: r.f32().unwrap(),
        resolution: r.f64().unwrap(),
        dont_adjust: r.bool().unwrap(),
        monotonic: r.bool().unwrap(),
        complete: r.bool().unwrap(),
        use_arachne: r.bool().unwrap(),
        layer_height: r.f64().unwrap(),
        gyroid_optimized: r.bool().unwrap(),
        lateral_lattice_angle_1: r.f64().unwrap(),
        lateral_lattice_angle_2: r.f64().unwrap(),
        pattern: {
            let pattern = r.i32().unwrap();
            InfillPattern::from_int(pattern)
                .unwrap_or_else(|| panic!("an infill pattern of {pattern}"))
        },
        infill_overhang_angle: r.f32().unwrap(),
        flow: flow(r),
        extrusion_role: {
            let role = r.u8().unwrap();
            ExtrusionRole::from_u8(role).unwrap_or_else(|| panic!("an extrusion role of {role}"))
        },
        using_internal_flow: r.bool().unwrap(),
        no_extrusion_overlap: r.f32().unwrap(),
        config: {
            has_config = r.bool().unwrap();
            None
        },
        dont_sort: r.bool().unwrap(),
        can_reverse: r.bool().unwrap(),
        horiz_move: r.f32().unwrap(),
        symmetric_infill_y_axis: r.bool().unwrap(),
        symmetric_y_axis: r.i64().unwrap(),
        locked_zag: r.bool().unwrap(),
        infill_lock_depth: r.f32().unwrap(),
        skin_infill_depth: r.f32().unwrap(),
    };
    (q, has_config)
}

/// A record's own configs (`configs`): the defaults with the nozzle diameters, the gap fill
/// target and the gap fill filter set.
struct RecordConfigs {
    print: PrintConfig,
    object: PrintObjectConfig,
    region: PrintRegionConfig,
}

fn record_configs(r: &mut Reader<'_>) -> RecordConfigs {
    let mut c = RecordConfigs {
        print: PrintConfig::default(),
        object: PrintObjectConfig::default(),
        region: PrintRegionConfig::default(),
    };
    let n = r.count().unwrap();
    c.print.nozzle_diameter.values = (0..n).map(|_| r.f64().unwrap()).collect();
    let target = r.i32().unwrap();
    c.object.gap_fill_target.value =
        GapFillTarget::from_int(target).unwrap_or_else(|| panic!("a gap fill target of {target}"));
    c.region.filter_out_gap_fill.value = r.f64().unwrap();
    c
}

// ---------------------------------------------------------------- writing the record again

/// A record's payload, written as the probe's `Payload` writes it (little-endian, `bool` as a
/// byte, `f32` and `f64` by their bits, `str` as a `u32` length and its bytes, counts as `u64`).
#[derive(Default)]
struct W(Vec<u8>);

impl W {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i64(&mut self, v: i64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn bool(&mut self, v: bool) {
        self.u8(u8::from(v));
    }
    fn f32(&mut self, v: f32) {
        self.u32(v.to_bits());
    }
    fn f64(&mut self, v: f64) {
        self.u64(v.to_bits());
    }
    fn str(&mut self, s: &[u8]) {
        self.u32(u32::try_from(s.len()).unwrap());
        self.0.extend_from_slice(s);
    }
    fn count(&mut self, n: usize) {
        self.u64(n as u64);
    }
    fn point(&mut self, p: Point) {
        self.i64(p.x);
        self.i64(p.y);
    }
    fn points(&mut self, pts: &[Point]) {
        self.count(pts.len());
        for p in pts {
            self.point(*p);
        }
    }
    fn expolygon(&mut self, e: &ExPolygon) {
        self.points(&e.contour.points);
        self.count(e.holes.len());
        for h in &e.holes {
            self.points(&h.points);
        }
    }
    fn bounding_box(&mut self, b: &BoundingBox) {
        self.point(b.min);
        self.point(b.max);
        self.bool(b.defined);
    }
    fn surface(&mut self, s: &Surface) {
        self.u32(s.surface_type as i32 as u32);
        self.expolygon(&s.expolygon);
        self.f64(s.thickness);
        self.u16(s.thickness_layers);
        self.f64(s.bridge_angle);
        self.u16(s.extra_perimeters);
    }
    fn flow(&mut self, f: &Flow) {
        self.f32(f.width());
        self.f32(f.height());
        self.f32(f.spacing());
        self.f32(f.nozzle_diameter());
        self.bool(f.bridge());
    }
    fn params(&mut self, q: &FillParams<'_>) {
        self.f32(q.density);
        self.i32(q.multiline);
        self.f32(q.anchor_length);
        self.f32(q.anchor_length_max);
        self.f64(q.resolution);
        self.bool(q.dont_adjust);
        self.bool(q.monotonic);
        self.bool(q.complete);
        self.bool(q.use_arachne);
        self.f64(q.layer_height);
        self.bool(q.gyroid_optimized);
        self.f64(q.lateral_lattice_angle_1);
        self.f64(q.lateral_lattice_angle_2);
        self.i32(q.pattern as i32);
        self.f32(q.infill_overhang_angle);
        self.flow(&q.flow);
        self.u8(q.extrusion_role as u8);
        self.bool(q.using_internal_flow);
        self.f32(q.no_extrusion_overlap);
        self.bool(q.config.is_some());
        self.bool(q.dont_sort);
        self.bool(q.can_reverse);
        self.f32(q.horiz_move);
        self.bool(q.symmetric_infill_y_axis);
        self.i64(q.symmetric_y_axis);
        self.bool(q.locked_zag);
        self.f32(q.infill_lock_depth);
        self.f32(q.skin_infill_depth);
    }
    fn filler(&mut self, b: &FillBase<'_>) {
        self.u64(b.layer_id as u64);
        self.f64(b.z);
        self.f64(b.spacing);
        self.f64(b.overlap);
        self.f32(b.angle);
        self.bool(b.fixed_angle);
        self.i64(b.link_max_length);
        self.i64(b.loop_clipping);
        self.bounding_box(&b.bounding_box);
        self.bool(b.dont_alternate_fill_direction);
        self.count(b.no_overlap_expolygons.len());
        for e in &b.no_overlap_expolygons {
            self.expolygon(e);
        }
        self.bool(b.print_object_config.is_some());
    }
    fn fitting(&mut self, fitting: &[PathFittingData]) {
        self.count(fitting.len());
        for f in fitting {
            self.count(f.start_point_index);
            self.count(f.end_point_index);
            self.u8(f.path_type as u8);
            let arc = &f.arc_data;
            self.bool(arc.is_arc);
            self.point(arc.center);
            self.f64(arc.radius);
            self.f64(arc.length);
            self.f64(arc.angle_radians);
            self.f64(arc.polar_start_theta);
            self.f64(arc.polar_end_theta);
            self.point(arc.start_point);
            self.point(arc.end_point);
            self.u8(arc.direction as u8);
        }
    }
    fn polyline(&mut self, pl: &Polyline) {
        self.points(&pl.points);
        self.fitting(&pl.fitting_result);
    }
    fn polylines(&mut self, pls: &[Polyline]) {
        self.count(pls.len());
        for pl in pls {
            self.polyline(pl);
        }
    }
    fn thick_polylines(&mut self, ts: &[ThickPolyline]) {
        self.count(ts.len());
        for t in ts {
            self.polyline(&t.polyline);
            self.count(t.width.len());
            for w in &t.width {
                self.f64(*w);
            }
            self.bool(t.endpoints.0);
            self.bool(t.endpoints.1);
        }
    }
    fn configs(&mut self, c: &RecordConfigs) {
        self.count(c.print.nozzle_diameter.values.len());
        for d in &c.print.nozzle_diameter.values {
            self.f64(*d);
        }
        self.i32(c.object.gap_fill_target.value.to_int());
        self.f64(c.region.filter_out_gap_fill.value);
    }
    /// The probe's `put_threw`: a 1 and the exception's `what()`.
    fn threw(&mut self, e: &FillError) {
        self.u8(1);
        self.str(e.what().as_bytes());
    }

    /// `put_path_fields`, with the class's `can_reverse()`.
    fn path_fields(&mut self, p: &ExtrusionPath, can_reverse: bool) {
        self.u8(p.role() as u8);
        self.i32(p.inset_idx);
        self.count(p.polyline.points.len());
        for q in &p.polyline.points {
            self.i64(q.x);
            self.i64(q.y);
            self.i64(q.z);
        }
        self.fitting(&p.polyline.fitting_result);
        self.f64(p.overhang_degree);
        self.i32(p.curve_degree);
        self.f64(p.mm3_per_mm);
        self.f32(p.width);
        self.f32(p.height);
        self.f64(p.smooth_speed);
        self.bool(p.z_contoured);
        self.bool(p.is_force_no_extrusion());
        self.bool(can_reverse);
    }
    fn path(&mut self, p: &ExtrusionPath) {
        self.u8(1);
        self.path_fields(p, p.can_reverse());
    }
    fn sloped(&mut self, s: &ExtrusionPathSloped) {
        self.u8(3);
        self.path_fields(&s.path, s.path.can_reverse());
        self.f64(s.slope_begin.z_ratio);
        self.f64(s.slope_begin.e_ratio);
        self.f64(s.slope_end.z_ratio);
        self.f64(s.slope_end.e_ratio);
    }
    fn multi_path(&mut self, m: &ExtrusionMultiPath) {
        self.u8(5);
        self.i32(m.inset_idx);
        self.bool(m.can_reverse());
        self.count(m.paths.len());
        for p in &m.paths {
            self.path(p);
        }
    }
    fn loop_fields(&mut self, l: &ExtrusionLoop) {
        self.i32(l.inset_idx);
        self.u8(l.loop_role().0);
        self.count(l.paths.len());
        for p in &l.paths {
            self.path(p);
        }
    }
    fn loop_sloped(&mut self, l: &ExtrusionLoopSloped) {
        self.u8(7);
        self.loop_fields(&l.base);
        self.count(l.starts.len());
        for s in &l.starts {
            self.sloped(s);
        }
        self.count(l.ends.len());
        for s in &l.ends {
            self.sloped(s);
        }
    }
    fn collection(&mut self, c: &ExtrusionEntityCollection) {
        self.u8(8);
        self.i32(c.inset_idx);
        self.bool(c.no_sort);
        self.bool(c.can_reverse());
        self.entities(&c.entities);
    }
    /// `put_entity`: the kind by the class, then its fields.
    fn entity(&mut self, e: &ExtrusionEntity) {
        match e {
            ExtrusionEntity::Path(p) => self.path(p),
            ExtrusionEntity::PathContoured(p) => {
                self.u8(2);
                self.path_fields(&p.path, p.path.can_reverse());
                self.count(p.z_diffs.len());
                for z in &p.z_diffs {
                    self.f64(*z);
                }
            }
            ExtrusionEntity::PathSloped(s) => self.sloped(s),
            ExtrusionEntity::PathOriented(o) => {
                self.u8(4);
                self.path_fields(&o.path, o.can_reverse());
            }
            ExtrusionEntity::MultiPath(m) => self.multi_path(m),
            ExtrusionEntity::Loop(l) => {
                self.u8(6);
                self.loop_fields(l);
            }
            ExtrusionEntity::LoopSloped(l) => self.loop_sloped(l),
            ExtrusionEntity::Collection(c) => self.collection(c),
        }
    }
    fn entities(&mut self, v: &[ExtrusionEntity]) {
        self.count(v.len());
        for e in v {
            self.entity(e);
        }
    }
}

// ---------------------------------------------------------------- the filler and stdout

/// One call of `_fill_surface_single`: the thickness in layers, the direction and the region.
type Call = (u32, (f32, Point), ExPolygon);

/// The probe's `SpyFill`: Fill's base `fill_surface` and `fill_surface_arachne` call its
/// `_fill_surface_single`, which keeps its arguments and fills nothing.
#[derive(Clone)]
struct Spy<'a> {
    base: FillBase<'a>,
    calls: Vec<Call>,
}

impl<'a> Fill<'a> for Spy<'a> {
    fn base(&self) -> &FillBase<'a> {
        &self.base
    }
    fn base_mut(&mut self) -> &mut FillBase<'a> {
        &mut self.base
    }
    fn clone_box(&self) -> Box<dyn Fill<'a> + 'a> {
        Box::new(self.clone())
    }
    fn is_self_crossing(&self) -> bool {
        false
    }
    fn fill_surface_single(
        &mut self,
        _params: &FillParams<'_>,
        thickness_layers: u32,
        direction: (f32, Point),
        expolygon: ExPolygon,
        _polylines_out: &mut Polylines,
    ) -> Result<(), FillError> {
        self.calls.push((thickness_layers, direction, expolygon));
        Ok(())
    }
    fn fill_surface_single_thick(
        &mut self,
        _params: &FillParams<'_>,
        thickness_layers: u32,
        direction: (f32, Point),
        expolygon: ExPolygon,
        _thick_polylines_out: &mut ThickPolylines,
    ) -> Result<(), FillError> {
        self.calls.push((thickness_layers, direction, expolygon));
        Ok(())
    }
}

/// Runs `f` with what the crate prints on stdout taken into bytes, one test at a time (the sink
/// is the process's: orca-fmt's C `stdout`).
fn capture_stdout<T>(f: impl FnOnce() -> T) -> (T, Vec<u8>) {
    static CAPTURE: Mutex<()> = Mutex::new(());
    let _one = CAPTURE.lock().unwrap_or_else(PoisonError::into_inner);
    let got = Arc::new(Mutex::new(Vec::new()));
    let into = Arc::clone(&got);
    let before = set_stdout_sink(Some(Box::new(move |b: &[u8]| {
        into.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(b);
    })));
    let out = f();
    set_stdout_sink(before);
    let text = got.lock().unwrap_or_else(PoisonError::into_inner).clone();
    (out, text)
}

/// The probe's `StubFill`: its `fill_surface` and `fill_surface_arachne` set the spacing the
/// record gives and return the record's lines, or throw `InfillFailedException`.
#[derive(Clone)]
struct Stub<'a> {
    base: FillBase<'a>,
    stub_no_sort: bool,
    set_spacing: f64,
    throws: bool,
    give: Polylines,
    give_thick: ThickPolylines,
}

impl<'a> Fill<'a> for Stub<'a> {
    fn base(&self) -> &FillBase<'a> {
        &self.base
    }
    fn base_mut(&mut self) -> &mut FillBase<'a> {
        &mut self.base
    }
    fn clone_box(&self) -> Box<dyn Fill<'a> + 'a> {
        Box::new(self.clone())
    }
    fn is_self_crossing(&self) -> bool {
        false
    }
    fn no_sort(&self) -> bool {
        self.stub_no_sort
    }
    fn fill_surface(
        &mut self,
        _surface: &Surface,
        _params: &FillParams<'_>,
    ) -> Result<Polylines, FillError> {
        self.base.spacing = self.set_spacing;
        if self.throws {
            return Err(FillError::InfillFailed);
        }
        Ok(self.give.clone())
    }
    fn fill_surface_arachne(
        &mut self,
        _surface: &Surface,
        _params: &FillParams<'_>,
    ) -> Result<ThickPolylines, FillError> {
        self.base.spacing = self.set_spacing;
        if self.throws {
            return Err(FillError::InfillFailed);
        }
        Ok(self.give_thick.clone())
    }
}

thread_local! {
    /// What the crate logs at error or fatal on this thread while [`capture_log`] runs.
    static LOG: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
}

/// Runs `f` with what the crate logs at error or fatal on this thread taken into bytes, as the
/// probe's `ErrorLog` writes them: `[<severity>] <message>`, a line feed added when the message
/// has none (Boost.Log's text backend).
fn capture_log<T>(f: impl FnOnce() -> T) -> (T, Vec<u8>) {
    static SINK: Once = Once::new();
    SINK.call_once(|| {
        orca_mesh::log::set_sink(Some(Box::new(|severity: Severity, message: &str| {
            if severity < Severity::Error {
                return;
            }
            LOG.with(|log| {
                if let Some(text) = log.borrow_mut().as_mut() {
                    text.extend_from_slice(b"[");
                    text.extend_from_slice(severity.name().as_bytes());
                    text.extend_from_slice(b"] ");
                    text.extend_from_slice(message.as_bytes());
                    if !message.ends_with('\n') {
                        text.push(b'\n');
                    }
                }
            });
        })));
    });
    LOG.with(|log| *log.borrow_mut() = Some(Vec::new()));
    let out = f();
    let text = LOG.with(|log| log.borrow_mut().take()).unwrap_or_default();
    (out, text)
}

// ---------------------------------------------------------------- the comparison

/// Compares each record of `kind` with the payload `rebuild` writes for it, by its bytes.
fn replay(kind: &str, rebuild: impl Fn(&Record) -> Vec<u8>) {
    let mut m = Mismatches::default();
    for (i, record) in file().of_kind(kind).enumerate() {
        let got = rebuild(record);
        let want = &record.payload;
        m.check(got == *want, || {
            let at = got
                .iter()
                .zip(want.iter())
                .position(|(a, b)| a != b)
                .unwrap_or(got.len().min(want.len()));
            format!(
                "{kind} #{i}: {} bytes against the engine's {}, first different at byte {at}",
                got.len(),
                want.len()
            )
        });
    }
    m.assert_none(kind);
}

#[test]
fn the_file_holds_only_known_kinds() {
    let mut m = Mismatches::default();
    for record in &file().records {
        let kind = record.kind();
        m.check(
            OWN_KINDS.contains(&kind) || OTHER_PIECES_KINDS.contains(&kind),
            || format!("a record of kind {kind:?}, which no piece replays"),
        );
    }
    for kind in OWN_KINDS {
        m.check(file().of_kind(kind).next().is_some(), || {
            format!("no record of kind {kind}")
        });
    }
    m.assert_none("the record kinds");
}

#[test]
fn fldf_the_defaults() {
    replay("FLDF", |_| {
        let mut w = W::default();
        let params = FillParams::default();
        w.params(&params);
        w.bool(params.full_infill());
        w.bool(params.dont_connect());
        w.f32(INFILL_ANCHOR);
        w.f32(INFILL_ANCHOR_MAX);
        w.filler(&FillBase::new());
        w.surface(&Surface::default());
        w.0
    });
}

#[test]
fn flpa_full_infill_and_dont_connect() {
    replay("FLPA", |record| {
        let mut r = record.reader();
        let params = FillParams {
            density: r.f32().unwrap(),
            anchor_length_max: r.f32().unwrap(),
            ..FillParams::default()
        };
        let mut w = W::default();
        w.f32(params.density);
        w.f32(params.anchor_length_max);
        w.bool(params.full_infill());
        w.bool(params.dont_connect());
        w.0
    });
}

#[test]
fn flas_adjust_solid_spacing() {
    replay("FLAS", |record| {
        let mut r = record.reader();
        let width = r.i64().unwrap();
        let distance = r.i64().unwrap();
        let mut w = W::default();
        w.i64(width);
        w.i64(distance);
        w.i64(adjust_solid_spacing(width, distance));
        w.0
    });
}

#[test]
fn fldr_infill_direction_and_layer_angle() {
    replay("FLDR", |record| {
        let mut r = record.reader();
        let base = filler(&mut r);
        let surface = surface(&mut r);
        let f = Spy {
            base,
            calls: Vec::new(),
        };
        let ((angle, shift), out) = capture_stdout(|| f.infill_direction(&surface));
        let mut w = W::default();
        w.filler(&f.base);
        w.surface(&surface);
        w.f32(angle);
        w.point(shift);
        w.str(&out);
        w.f32(f.layer_angle(f.base.layer_id));
        w.f32(f.layer_angle(f.base.layer_id.wrapping_add(1)));
        w.0
    });
}

#[test]
fn flsu_fill_surface_and_fill_surface_arachne() {
    replay("FLSU", |record| {
        let mut r = record.reader();
        let entry = r.u8().unwrap();
        let base = filler(&mut r);
        let surface = surface(&mut r);
        let params = FillParams {
            use_arachne: entry == 1,
            ..FillParams::default()
        };
        let mut f = Spy {
            base,
            calls: Vec::new(),
        };
        let mut w = W::default();
        w.u8(entry);
        w.filler(&f.base);
        w.surface(&surface);
        w.params(&params);
        let (returned, out) = capture_stdout(|| match entry {
            0 => f.fill_surface(&surface, &params).map(|v| v.len()),
            1 => f.fill_surface_arachne(&surface, &params).map(|v| v.len()),
            _ => panic!("an FLSU entry of {entry}"),
        });
        let returned = returned.unwrap_or_else(|e| panic!("the spy threw {e}"));
        w.str(&out);
        w.count(f.calls.len());
        for (thickness_layers, (angle, shift), region) in &f.calls {
            w.u32(*thickness_layers);
            w.f32(*angle);
            w.point(*shift);
            w.expolygon(region);
        }
        w.count(returned);
        w.0
    });
}

#[test]
fn flcn_connect_infill() {
    replay("FLCN", |record| {
        let mut r = record.reader();
        let entry = r.u8().unwrap();
        let boundary = expolygon(&mut r);
        let bbox = bounding_box(&mut r);
        let spacing = r.f64().unwrap();
        let (mut q, has_config) = params(&mut r);
        if has_config {
            q.config = Some(&configs().region);
        }
        let infill = polylines(&mut r);
        let mut w = W::default();
        w.u8(entry);
        w.expolygon(&boundary);
        w.bounding_box(&bbox);
        w.f64(spacing);
        w.params(&q);
        w.polylines(&infill);
        let mut out = Polylines::new();
        match entry {
            0 => connect_infill_expolygon(infill, &boundary, &mut out, spacing, &q),
            1 => {
                let polygons = to_polygons(&boundary);
                connect_infill_polygons(infill, &polygons, &bbox, &mut out, spacing, &q);
            }
            2 => chain_or_connect_infill(infill, &boundary, &mut out, spacing, &q),
            _ => panic!("an FLCN entry of {entry}"),
        }
        w.u8(0);
        w.polylines(&out);
        w.0
    });
}

#[test]
fn fleb_extended_object_bounding_box() {
    replay("FLEB", |record| {
        let mut r = record.reader();
        let b = bounding_box(&mut r);
        let mut f = FillBase::new();
        f.bounding_box = b;
        let mut w = W::default();
        w.bounding_box(&b);
        w.bounding_box(&f.extended_object_bounding_box());
        w.0
    });
}

#[test]
fn flgp_create_gap_fill() {
    replay("FLGP", |record| {
        let mut r = record.reader();
        let (fields, has_configs) = filler_fields(&mut r);
        let surface = surface(&mut r);
        let (params_read, has_config) = params(&mut r);
        let c = record_configs(&mut r);
        let lines = polylines(&mut r);
        let mut f = fields;
        if has_configs {
            f.print_config = Some(&c.print);
            f.print_object_config = Some(&c.object);
        }
        let mut q = params_read;
        if has_config {
            q.config = Some(&c.region);
        }
        let mut w = W::default();
        w.filler(&f);
        w.surface(&surface);
        w.params(&q);
        w.configs(&c);
        let flow = q.flow;
        let mm3 = flow
            .mm3_per_mm()
            .unwrap_or_else(|e| panic!("the record's flow: {e}"));
        w.polylines(&lines);
        w.u8(q.extrusion_role as u8);
        w.f64(mm3);
        w.f32(flow.width());
        w.f32(flow.height());
        let mut out = ExtrusionEntityCollection::default();
        extrusion_entities_append_paths(
            &mut out.entities,
            &lines,
            q.extrusion_role,
            mm3,
            flow.width(),
            flow.height(),
            true,
        );
        let before = out.entities.len();
        let (res, log) = capture_log(|| create_gap_fill(&f, &surface, &q, &mut out));
        match res {
            Ok(()) => {
                // The call only appends: how many paths stayed, and the entities after them.
                let len = out.entities.len();
                let kept = if len < before { len } else { before };
                w.u8(0);
                w.count(kept);
                w.entities(&out.entities[kept..]);
            }
            Err(e) => w.threw(&e),
        }
        w.str(&log);
        w.0
    });
}

#[test]
fn flex_fill_surface_extrusion() {
    replay("FLEX", |record| {
        let mut r = record.reader();
        let (fields, has_configs) = filler_fields(&mut r);
        let stub_no_sort = r.bool().unwrap();
        let set_spacing = r.f64().unwrap();
        let throws = r.bool().unwrap();
        let give = polylines(&mut r);
        let give_thick = thick_polylines(&mut r);
        let surface = surface(&mut r);
        let (params_read, has_config) = params(&mut r);
        let c = record_configs(&mut r);
        let mut f = Stub {
            base: fields,
            stub_no_sort,
            set_spacing,
            throws,
            give,
            give_thick,
        };
        if has_configs {
            f.base.print_config = Some(&c.print);
            f.base.print_object_config = Some(&c.object);
        }
        let mut q = params_read;
        if has_config {
            q.config = Some(&c.region);
        }
        let mut w = W::default();
        w.filler(&f.base);
        w.bool(stub_no_sort);
        w.f64(set_spacing);
        w.bool(throws);
        w.polylines(&f.give);
        w.thick_polylines(&f.give_thick);
        w.surface(&surface);
        w.params(&q);
        w.configs(&c);
        let mut out = ExtrusionEntitiesPtr::new();
        let (res, log) = capture_log(|| f.fill_surface_extrusion(&surface, &q, &mut out));
        match res {
            Ok(()) => {
                w.u8(0);
                w.entities(&out);
            }
            Err(e) => w.threw(&e),
        }
        w.str(&log);
        w.0
    });
}
