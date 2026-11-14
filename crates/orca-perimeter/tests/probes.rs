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
// (parity/oracle/probe/walls/basics.cpp and extra.cpp). Written on 2026-11-14 (orca-o3-walls-1).

//! The probe gate for orca-perimeter: the walls-1 records of `walls.sop1`
//! (`parity/sop1/walls.md`), from the committed subset (`parity/probes/`), or from the full set
//! when `SKETCH_ORCA_PROBES_DEEP` names its folder.
//!
//! Each record holds its inputs, then the engine's results. The replay decodes the inputs (an
//! entity, a flow or an Arachne line is encoded again and must give its bytes back, which proves
//! the decoder; a flow is made again as the probe made it, by `bridging_flow` or by `Flow(width,
//! height, nozzle)`, so its spacing must come out the same), runs the translation, encodes each
//! result as the probe's writers do (`parity/oracle/probe/walls/values.cpp`) and compares the bytes
//! part by part; the first part that differs ends the record's replay and is reported.
//!
//! - WLMT, WLPC: the principal components' moments of a triangle and components of polygons.
//! - WLDB: `detect_bridging_direction`, both overloads.
//! - WLBD: `BridgeDetector`: `resolution`, `detect_angle`, `angle`, three coverages and two lists
//!   of unsupported edges.
//! - WLEA: `extrusion_paths_append` of ClipperLib_Z paths and of an Arachne line.
//! - WLRC, WLPT, WLSX, WLXP: `reconnect_polylines`, `paths_touch`, `sort_extra_perimeters` and
//!   `generate_extra_perimeters_over_overhangs` (the configurations at their defaults).
//!
//! WLTP, WLCP, WLCL and WLAR are walls-2's and walls-3's: the test counts them as known and leaves
//! them. Any other kind fails the test, as does a record count other than the probe's.

use std::collections::BTreeMap;

use orca_arachne::extrusion_junction::ExtrusionJunction;
use orca_arachne::extrusion_line::ExtrusionLine;
use orca_config::print_config::{PrintConfig, PrintObjectConfig};
use orca_eigen::Vec2f;
use orca_geom::arc_fitter::{EMovePathType, PathFittingData};
use orca_geom::circle::{ArcDirection, ArcSegment};
use orca_geom::clipper_z::{self, IntPoint};
use orca_geom::expolygon::ExPolygon;
use orca_geom::line::Line;
use orca_geom::point::{Point, Points};
use orca_geom::point3::Point3;
use orca_geom::polygon::Polygon;
use orca_geom::polyline::Polyline;
use orca_geom::polyline3::Polyline3;
use orca_layer::flow::{Flow, FlowError};
use orca_layer::path::{ExtrusionPath, ExtrusionPaths};
use orca_layer::role::ExtrusionRole;
use orca_perimeter::bridge_detector::{
    BridgeDetector, detect_bridging_direction, detect_bridging_direction_lines,
};
use orca_perimeter::extra_perimeters::{
    generate_extra_perimeters_over_overhangs, paths_touch, reconnect_polylines,
    sort_extra_perimeters,
};
use orca_perimeter::extrusion_paths::{extrusion_paths_append_line, extrusion_paths_append_zpaths};
use orca_perimeter::principal_components::{
    compute_moments_of_area_of_triangle, compute_principal_components,
};
use orca_perimeter::stop::PerimeterError;
use orca_probe::{Error, Mismatches, ProbeSet, Reader, load_probe};

/// The kinds this test replays, with their record counts in the committed and the full set.
const OWN: &[(&str, usize, usize)] = &[
    ("WLMT", 40, 310),
    ("WLPC", 33, 137),
    ("WLDB", 44, 167),
    ("WLBD", 26, 58),
    ("WLEA", 27, 287),
    ("WLRC", 31, 131),
    ("WLPT", 29, 105),
    ("WLSX", 20, 86),
    ("WLXP", 11, 40),
];

/// The kinds of `walls.sop1` that walls-2 (WLCL) and walls-3 (WLTP, WLCP, WLAR) replay.
const LATER_PIECES: &[&str] = &["WLTP", "WLCP", "WLCL", "WLAR"];

// ---- Writing, as values.cpp's writers --------------------------------------------------------

/// An error's `what()`.
trait What {
    fn what_text(&self) -> String;
}

impl What for FlowError {
    fn what_text(&self) -> String {
        self.what()
    }
}

impl What for PerimeterError {
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
    fn expolygons(&mut self, v: &[ExPolygon]) -> &mut Self {
        self.len(v.len());
        for e in v {
            self.points(&e.contour.points).polygons(&e.holes);
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
    fn polylines(&mut self, pls: &[Polyline]) -> &mut Self {
        self.len(pls.len());
        for pl in pls {
            self.points(&pl.points).fitting(&pl.fitting_result);
        }
        self
    }
    fn lines(&mut self, lines: &[Line]) -> &mut Self {
        self.len(lines.len());
        for l in lines {
            self.point(l.a).point(l.b);
        }
        self
    }
    fn zpaths(&mut self, paths: &[clipper_z::Path]) -> &mut Self {
        self.len(paths.len());
        for path in paths {
            self.len(path.len());
            for pt in path {
                self.i64(pt.x).i64(pt.y).i64(pt.z);
            }
        }
        self
    }
    fn arachne_line(&mut self, line: &ExtrusionLine) -> &mut Self {
        self.len(line.inset_idx)
            .boolean(line.is_odd)
            .boolean(line.is_closed);
        self.len(line.junctions.len());
        for j in &line.junctions {
            self.i64(j.p.x).i64(j.p.y).i64(j.w).len(j.perimeter_index);
        }
        self
    }
    fn flow(&mut self, f: &Flow) -> &mut Self {
        self.f32(f.width()).f32(f.height()).f32(f.spacing());
        self.f32(f.nozzle_diameter()).boolean(f.bridge())
    }
    /// A call that may throw: `u8` 1 and its result, or `u8` 2 and the `what()`.
    fn attempt<T, E: What>(&mut self, res: Result<T, E>, f: impl FnOnce(&mut W, T)) -> &mut Self {
        match res {
            Ok(v) => {
                self.u8(1);
                f(self, v);
            }
            Err(e) => {
                self.u8(2).str(&e.what_text());
            }
        }
        self
    }
    /// An `ExtrusionPath` held by value (`put_entity`'s kind 1).
    fn path(&mut self, p: &ExtrusionPath) -> &mut Self {
        self.u8(1).u8(p.role() as u8).i32(p.inset_idx);
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
        self.boolean(p.can_reverse())
    }
    /// `put_paths`.
    fn paths(&mut self, v: &[ExtrusionPath]) -> &mut Self {
        self.len(v.len());
        for p in v {
            self.path(p);
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

fn rd_polygons(r: &mut Reader<'_>) -> Result<Vec<Polygon>, Error> {
    r.vec(|q| Ok(Polygon::new(rd_points(q)?)))
}

fn rd_expolygons(r: &mut Reader<'_>) -> Result<Vec<ExPolygon>, Error> {
    r.vec(|q| {
        let contour = Polygon::new(rd_points(q)?);
        let holes = rd_polygons(q)?;
        Ok(ExPolygon { contour, holes })
    })
}

fn rd_lines(r: &mut Reader<'_>) -> Result<Vec<Line>, Error> {
    r.vec(|q| {
        let a = rd_point(q)?;
        let b = rd_point(q)?;
        Ok(Line { a, b })
    })
}

fn rd_index(r: &mut Reader<'_>) -> Result<usize, Error> {
    usize::try_from(r.u64()?).map_err(|_| Error("an index".into()))
}

fn rd_fitting(r: &mut Reader<'_>) -> Result<Vec<PathFittingData>, Error> {
    r.vec(|q| {
        let start = rd_index(q)?;
        let end = rd_index(q)?;
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

fn rd_polylines(r: &mut Reader<'_>) -> Result<Vec<Polyline>, Error> {
    r.vec(|q| {
        let points = rd_points(q)?;
        let fitting_result = rd_fitting(q)?;
        Ok(Polyline {
            points,
            fitting_result,
        })
    })
}

fn rd_role(r: &mut Reader<'_>) -> Result<ExtrusionRole, Error> {
    let v = r.u8()?;
    ExtrusionRole::from_u8(v).ok_or_else(|| Error(format!("a role of {v}")))
}

/// An `ExtrusionPath` held by value (`put_entity`'s kind 1); no other kind is a walls-1 input.
fn rd_path(r: &mut Reader<'_>) -> Result<ExtrusionPath, Error> {
    match r.u8()? {
        1 => {}
        v => return fail(format!("a path of kind {v}")),
    }
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
    if !can_reverse {
        p.set_reverse();
    }
    Ok(p)
}

fn rd_paths(r: &mut Reader<'_>) -> Result<ExtrusionPaths, Error> {
    r.vec(rd_path)
}

/// A flow made again as the probe made it: a bridge by `bridging_flow`, else by `Flow(width,
/// height, nozzle)` (the spacing then recomputed; the echo checks it).
fn rd_flow(r: &mut Reader<'_>) -> Result<Flow, Error> {
    let width = r.f32()?;
    let height = r.f32()?;
    let _spacing = r.f32()?;
    let nozzle = r.f32()?;
    let bridge = r.bool()?;
    if bridge {
        Ok(Flow::bridging_flow(width, nozzle))
    } else {
        Flow::new(width, height, nozzle).map_err(|e| Error(format!("the record's flow: {e}")))
    }
}

fn rd_zpaths(r: &mut Reader<'_>) -> Result<Vec<clipper_z::Path>, Error> {
    r.vec(|q| q.vec(|p| Ok(IntPoint::new(p.i64()?, p.i64()?, p.i64()?))))
}

fn rd_arachne_line(r: &mut Reader<'_>) -> Result<ExtrusionLine, Error> {
    let inset = rd_index(r)?;
    let is_odd = r.bool()?;
    let is_closed = r.bool()?;
    let mut line = ExtrusionLine::new(inset, is_odd);
    line.is_closed = is_closed;
    line.junctions = r.vec(|q| {
        let p = rd_point(q)?;
        let w = q.i64()?;
        let perimeter_index = rd_index(q)?;
        Ok(ExtrusionJunction {
            p,
            w,
            perimeter_index,
        })
    })?;
    Ok(line)
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

    /// Reads inputs at the current byte.
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

    /// An input that must encode back to the bytes it was read from.
    fn echoed<T>(
        &mut self,
        what: &str,
        read: impl FnOnce(&mut Reader<'a>) -> Result<T, Error>,
        write: impl FnOnce(&mut W, &T),
    ) -> Option<T> {
        let start = self.at;
        let v = self.input(read)?;
        let mut w = W::default();
        write(&mut w, &v);
        self.parts += 1;
        if self.buf[start..self.at] == w.0[..] {
            Some(v)
        } else {
            let (got, want) = (hex(&w.0), hex(&self.buf[start..self.at]));
            self.fail(format!(
                "the input {what} at byte {start} encodes again as {got}, the record {want}"
            ));
            None
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

// ---- The records -----------------------------------------------------------------------------

fn wlmt(c: &mut Cmp<'_>) -> Option<()> {
    let v = c.input(|r| {
        let mut v = [0f32; 6];
        for x in &mut v {
            *x = r.f32()?;
        }
        Ok(v)
    })?;
    let (area, first, second, cov) = compute_moments_of_area_of_triangle(
        Vec2f::new(v[0], v[1]),
        Vec2f::new(v[2], v[3]),
        Vec2f::new(v[4], v[5]),
    );
    c.part("the moments", |w| {
        w.f32(area).f32(first.x()).f32(first.y());
        w.f32(second.x()).f32(second.y()).f32(cov);
    })
}

fn wlpc(c: &mut Cmp<'_>) -> Option<()> {
    let polys = c.input(rd_polygons)?;
    let (pc1, pc2) = compute_principal_components(&polys);
    c.part("the components", |w| {
        w.f32(pc1.x()).f32(pc1.y()).f32(pc2.x()).f32(pc2.y());
    })
}

fn wldb(c: &mut Cmp<'_>) -> Option<()> {
    let overload = c.input(Reader::u8)?;
    let (dir, cost) = match overload {
        0 => {
            let edges = c.echoed("floating edges", rd_lines, |w, l| {
                w.lines(l);
            })?;
            let area = c.input(rd_polygons)?;
            detect_bridging_direction_lines(&edges, &area)
        }
        1 => {
            let (to_cover, anchors) = c.input(|r| Ok((rd_polygons(r)?, rd_polygons(r)?)))?;
            detect_bridging_direction(&to_cover, &anchors)
        }
        v => return c.fail(format!("an overload of {v}")),
    };
    c.part("the direction and cost", |w| {
        w.f64(dir.x()).f64(dir.y()).f64(cost);
    })
}

/// The calls of `bridge_case`, in its order, after `u8` 1.
fn bridge_results(w: &mut W, d: &mut BridgeDetector<'_>, override_angle: f64, angle2: f64) {
    w.u8(1).f64(d.resolution);
    let found = d.detect_angle(override_angle);
    w.boolean(found).f64(d.angle);
    w.polygons(&d.coverage(-1., true));
    w.polygons(&d.coverage(-1., false));
    w.polygons(&d.coverage(angle2, true));
    w.polylines(&d.unsupported_edges(-1.));
    w.polylines(&d.unsupported_edges(angle2));
}

fn wlbd(c: &mut Cmp<'_>) -> Option<()> {
    let ctor = c.input(Reader::u8)?;
    let expolygons = c.echoed("bridge expolygons", rd_expolygons, |w, e| {
        w.expolygons(e);
    })?;
    let lower = c.echoed("lower slices", rd_expolygons, |w, e| {
        w.expolygons(e);
    })?;
    let (spacing, override_angle, angle2) = c.input(|r| Ok((r.i64()?, r.f64()?, r.f64()?)))?;
    let mut w = W::default();
    match ctor {
        0 => {
            let Some(front) = expolygons.first() else {
                return c.fail("the ExPolygon constructor of no expolygon".into());
            };
            let mut d = BridgeDetector::from_expolygon(front.clone(), &lower, spacing);
            bridge_results(&mut w, &mut d, override_angle, angle2);
        }
        1 => {
            let mut d = BridgeDetector::new(&expolygons, &lower, spacing);
            bridge_results(&mut w, &mut d, override_angle, angle2);
        }
        v => return c.fail(format!("a constructor of {v}")),
    }
    c.part("the detector's results", |out| {
        out.0.extend_from_slice(&w.0);
    })
}

fn wlea(c: &mut Cmp<'_>) -> Option<()> {
    let (overload, role) = c.input(|r| Ok((r.u8()?, rd_role(r)?)))?;
    let flow = c.echoed("flow", rd_flow, |w, f| {
        w.flow(f);
    })?;
    let mut dst = ExtrusionPaths::new();
    let res = match overload {
        0 => {
            let paths = c.echoed("3D paths", rd_zpaths, |w, p| {
                w.zpaths(p);
            })?;
            extrusion_paths_append_zpaths(&mut dst, &paths, role, &flow)
        }
        1 => {
            let line = c.echoed("Arachne line", rd_arachne_line, |w, l| {
                w.arachne_line(l);
            })?;
            extrusion_paths_append_line(&mut dst, &line, role, &flow)
        }
        v => return c.fail(format!("an overload of {v}")),
    };
    c.part("the appended paths", |w| {
        w.attempt(res.map(|()| dst), |w, d| {
            w.paths(&d);
        });
    })
}

fn wlrc(c: &mut Cmp<'_>) -> Option<()> {
    let pls = c.echoed("polylines", rd_polylines, |w, p| {
        w.polylines(p);
    })?;
    let limit = c.input(Reader::f64)?;
    let out = reconnect_polylines(&pls, limit);
    c.part("the reconnected polylines", |w| {
        w.polylines(&out);
    })
}

fn wlpt(c: &mut Cmp<'_>) -> Option<()> {
    let a = c.echoed("first path", rd_path, |w, p| {
        w.path(p);
    })?;
    let b = c.echoed("second path", rd_path, |w, p| {
        w.path(p);
    })?;
    let limit = c.input(Reader::f64)?;
    let touch = paths_touch(&a, &b, limit);
    c.part("paths_touch", |w| {
        w.boolean(touch);
    })
}

fn wlsx(c: &mut Cmp<'_>) -> Option<()> {
    let paths = c.echoed("paths", rd_paths, |w, p| {
        w.paths(p);
    })?;
    let (first_unanchored, spacing) = c.input(|r| Ok((r.i32()?, r.f64()?)))?;
    let sorted = sort_extra_perimeters(&paths, first_unanchored, spacing);
    c.part("the sorted paths", |w| {
        w.paths(&sorted);
    })
}

fn wlxp(c: &mut Cmp<'_>) -> Option<()> {
    let infill = c.echoed("infill area", rd_expolygons, |w, e| {
        w.expolygons(e);
    })?;
    let (lower, count) = c.input(|r| Ok((rd_polygons(r)?, r.i32()?)))?;
    let flow = c.echoed("flow", rd_flow, |w, f| {
        w.flow(f);
    })?;
    let resolution = c.input(Reader::f64)?;
    let object = PrintObjectConfig::default();
    let print = PrintConfig::default();
    let res = generate_extra_perimeters_over_overhangs(
        infill, &lower, count, &flow, resolution, &object, &print,
    );
    c.part("the extra perimeters", |w| {
        w.attempt(res, |w, (perims, filled)| {
            w.len(perims.len());
            for region in &perims {
                w.paths(region);
            }
            w.polygons(&filled);
        });
    })
}

#[test]
fn walls_probe_replays() {
    let file = load_probe("walls").unwrap_or_else(|e| panic!("{e}"));
    let full = file.set == ProbeSet::Full;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for rec in &file.records {
        *counts.entry(rec.kind().to_string()).or_default() += 1;
    }
    for kind in counts.keys() {
        assert!(
            OWN.iter().any(|(k, _, _)| k == kind) || LATER_PIECES.contains(&kind.as_str()),
            "walls.sop1 holds {kind} records, which no piece of orca-perimeter replays"
        );
    }
    for (kind, committed, all) in OWN {
        let want = if full { *all } else { *committed };
        let got = counts.get(*kind).copied().unwrap_or(0);
        assert_eq!(got, want, "{kind} records in the {} set", file.set.name());
    }
    let mut m = Mismatches::default();
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
            "WLMT" => wlmt(&mut c),
            "WLPC" => wlpc(&mut c),
            "WLDB" => wldb(&mut c),
            "WLBD" => wlbd(&mut c),
            "WLEA" => wlea(&mut c),
            "WLRC" => wlrc(&mut c),
            "WLPT" => wlpt(&mut c),
            "WLSX" => wlsx(&mut c),
            "WLXP" => wlxp(&mut c),
            _ => unreachable!("an own kind without a replay"),
        };
        c.finish(&mut m);
    }
    println!(
        "walls.sop1 ({} set): {} comparisons over {} walls-1 records",
        file.set.name(),
        m.checked,
        OWN.iter()
            .map(|(k, _, _)| counts.get(*k).copied().unwrap_or(0))
            .sum::<usize>(),
    );
    m.assert_none("walls.sop1 (orca-perimeter, walls-1)");
}
