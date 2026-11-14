// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The probe gate of the piece geom-1: the records of `paths.sop1` whose sections in
//! `parity/sop1/paths.md` name geom-1, replayed against orca-geom bit for bit (added 2026-11-13):
//!
//! - PSZC: ClipperLib_Z ([`orca_geom::clipper_z`]) runs, with the Z fill callback's calls, the
//!   result paths and the tree's nodes, and the exceptions;
//! - PSRZ, PSIC, PSCV, PSRC: `rotation_xyz_diff` and `rotation_diff_z`,
//!   `is_point_inside_polygon_corner`, `Polygon::convex_points` and `concave_points`,
//!   `ray_circle_intersections_r2_lv2_c`;
//! - PSCI, PSAS, PSAC: `Circle` and `ArcSegment`;
//! - PSAF, PSPF: `ArcFitter` and `Polyline`'s fitting results;
//! - PSTP: `ThickPolyline`.
//!
//! The kinds of geom-2 (also orca-geom's) are replayed by [`geom2`], those of paths-1 by
//! orca-paths.

mod geom2;

use std::fmt::Debug;

use orca_eigen::{Vec3f, Vector};
use orca_geom::arc_fitter::{self, PathFittingData};
use orca_geom::circle::{ArcDirection, ArcSegment, Circle};
use orca_geom::clipper::IO_STRICTLY_SIMPLE;
use orca_geom::clipper_z as z;
use orca_geom::geometry;
use orca_geom::line::Line;
use orca_geom::point::{Point, vec2d};
use orca_geom::polygon::Polygon;
use orca_geom::polyline::{Polyline, ThickPolyline};
use orca_probe::{Mismatches, ProbeFile, load_probe};

use super::{Case, F, catch, clip_type, fill_type};

fn file() -> ProbeFile {
    load_probe("paths").unwrap_or_else(|e| panic!("{e}"))
}

/// The kinds this file replays: those of the piece geom-1.
const GEOM1_KINDS: [&str; 11] = [
    "PSZC", "PSRZ", "PSIC", "PSCV", "PSRC", "PSCI", "PSAS", "PSAC", "PSAF", "PSPF", "PSTP",
];
/// The kinds of the piece geom-2, which [`geom2`] replays.
const GEOM2_KINDS: [&str; 5] = ["PSP3", "PSD3", "PSL3", "PSC3", "PSO3"];
/// The kinds of the piece paths-1, which orca-paths replays.
const PATHS1_KINDS: [&str; 7] = ["PSLN", "PSCP", "PSCX", "PSXP", "PSXE", "PSPL", "PSUO"];

/// An `f32` compared by its bits.
#[derive(Clone, Copy)]
struct F32(f32);

impl PartialEq for F32 {
    fn eq(&self, o: &Self) -> bool {
        self.0.to_bits() == o.0.to_bits()
    }
}

impl Debug for F32 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

/// An `ArcSegment` as the probe writes it.
#[derive(Debug, PartialEq)]
struct Arc {
    is_arc: bool,
    center: Point,
    radius: F,
    start: Point,
    end: Point,
    direction: u8,
    length: F,
    angle: F,
    polar_start: F,
    polar_end: F,
}

fn arc_of(a: &ArcSegment) -> Arc {
    Arc {
        is_arc: a.is_arc,
        center: a.center,
        radius: F(a.radius),
        start: a.start_point,
        end: a.end_point,
        direction: a.direction as u8,
        length: F(a.length),
        angle: F(a.angle_radians),
        polar_start: F(a.polar_start_theta),
        polar_end: F(a.polar_end_theta),
    }
}

/// One `PathFittingData` as the probe writes it.
type Fit = (u64, u64, u8, Arc);

fn fits_of(r: &[PathFittingData]) -> Vec<Fit> {
    r.iter()
        .map(|d| {
            (
                d.start_point_index as u64,
                d.end_point_index as u64,
                d.path_type as u8,
                arc_of(&d.arc_data),
            )
        })
        .collect()
}

/// A polyline with its fit.
fn polyline_of(pl: &Polyline) -> (Vec<Point>, Vec<Fit>) {
    (pl.points.clone(), fits_of(&pl.fitting_result))
}

fn direction(v: u8) -> ArcDirection {
    match v {
        0 => ArcDirection::Unknown,
        1 => ArcDirection::Ccw,
        2 => ArcDirection::Cw,
        _ => panic!("ArcDirection {v} is not recorded"),
    }
}

impl Case<'_, '_> {
    fn u64(&mut self) -> u64 {
        self.r.u64().unwrap()
    }
    fn vec3f(&mut self) -> Vec3f {
        let (x, y) = (self.f32(), self.f32());
        Vector([x, y, self.f32()])
    }
    fn zpoint(&mut self) -> z::IntPoint {
        let (x, y) = (self.i64(), self.i64());
        z::IntPoint::new(x, y, self.i64())
    }
    fn zpath(&mut self) -> z::Path {
        let n = self.u32();
        (0..n).map(|_| self.zpoint()).collect()
    }
    fn zpaths(&mut self) -> z::Paths {
        let n = self.u32();
        (0..n).map(|_| self.zpath()).collect()
    }
    fn arc(&mut self) -> Arc {
        let is_arc = self.bool();
        let center = self.point();
        let radius = F(self.f64());
        let start = self.point();
        let end = self.point();
        let direction = self.u8();
        let (length, angle) = (F(self.f64()), F(self.f64()));
        let (polar_start, polar_end) = (F(self.f64()), F(self.f64()));
        Arc {
            is_arc,
            center,
            radius,
            start,
            end,
            direction,
            length,
            angle,
            polar_start,
            polar_end,
        }
    }
    fn fits(&mut self) -> Vec<Fit> {
        let n = self.u32();
        (0..n)
            .map(|_| {
                let (start, end) = (self.u64(), self.u64());
                let path_type = self.u8();
                (start, end, path_type, self.arc())
            })
            .collect()
    }

    fn is_f32(&mut self, what: &str, got: f32) {
        let want = F32(self.f32());
        self.same(what, &F32(got), &want);
    }
    fn is_vec3f(&mut self, what: &str, got: Vec3f) {
        let want: Vec<F32> = (0..3).map(|_| F32(self.f32())).collect();
        let got: Vec<F32> = (0..3).map(|i| F32(got[i])).collect();
        self.same(what, &got, &want);
    }
    fn is_circle(&mut self, what: &str, got: &Circle) {
        let want = (self.point(), F(self.f64()));
        self.same(what, &(got.center, F(got.radius)), &want);
    }
    fn is_arc(&mut self, what: &str, got: &ArcSegment) {
        let want = self.arc();
        self.same(what, &arc_of(got), &want);
    }
    fn is_fits(&mut self, what: &str, got: &[PathFittingData]) {
        let want = self.fits();
        self.same(what, &fits_of(got), &want);
    }
    fn is_polyline(&mut self, what: &str, got: &Polyline) {
        let points = self.path();
        let want = (points, self.fits());
        self.same(what, &polyline_of(got), &want);
    }
}

/// Replays every record of `kind` with `f`; a record that matched must have been read to its end.
fn replay(kind: &str, f: impl Fn(&mut Case)) {
    let file = file();
    let mut m = Mismatches::default();
    for (i, rec) in file.of_kind(kind).enumerate() {
        let before = m.count;
        let mut c = Case {
            r: rec.reader(),
            m: &mut m,
            label: format!("{kind} {i}"),
        };
        f(&mut c);
        if c.m.count == before {
            c.r.finish(&c.label).unwrap();
        }
    }
    m.assert_none(kind);
}

/// A node of a Z `PolyTree` as the probe writes it, in `GetNext` order.
#[derive(Debug, PartialEq)]
struct ZNode {
    depth: u32,
    hole: bool,
    open: bool,
    contour: z::Path,
}

fn pszc(c: &mut Case) {
    let ct = c.u8();
    let subject_open = c.bool();
    let (subj_fill, clip_fill) = (c.u8(), c.u8());
    let strict = c.bool();
    let zfill = c.bool();
    let tree = c.bool();
    let subject = c.zpaths();
    let clip = c.zpaths();

    let mut calls: Vec<[z::IntPoint; 5]> = Vec::new();
    let (mut added_subject, mut added_clip, mut ok) = (false, false, false);
    let mut result = z::Paths::new();
    let mut nodes: Vec<ZNode> = Vec::new();
    let thrown = {
        let mut clipper = z::Clipper::new(if strict { IO_STRICTLY_SIMPLE } else { 0 });
        if zfill {
            let calls = &mut calls;
            clipper.z_fill_function(move |e1bot, e1top, e2bot, e2top, pt| {
                calls.push([e1bot, e1top, e2bot, e2top, *pt]);
                pt.z = -(calls.len() as i64);
            });
        }
        catch(|| {
            added_subject = clipper.add_paths(
                subject.iter().map(Vec::as_slice),
                z::PolyType::Subject,
                !subject_open,
            );
            added_clip = clipper.add_paths(clip.iter().map(Vec::as_slice), z::PolyType::Clip, true);
            if tree {
                let mut t = z::PolyTree::new();
                ok = clipper.execute_tree(
                    clip_type(ct),
                    &mut t,
                    fill_type(subj_fill),
                    fill_type(clip_fill),
                );
                let mut n = t.get_first();
                while let Some(id) = n {
                    let mut depth = 0;
                    let mut q = t.node(id).parent;
                    while let Some(p) = q {
                        if p == z::ROOT {
                            break;
                        }
                        depth += 1;
                        q = t.node(p).parent;
                    }
                    nodes.push(ZNode {
                        depth,
                        hole: t.is_hole(id),
                        open: t.is_open(id),
                        contour: t.node(id).contour.clone(),
                    });
                    n = t.get_next(id);
                }
                result = z::poly_tree_to_paths(&t);
            } else {
                ok = clipper.execute(
                    clip_type(ct),
                    &mut result,
                    fill_type(subj_fill),
                    fill_type(clip_fill),
                );
            }
        })
    };
    c.is_bool("AddPaths(subject)", added_subject);
    c.is_bool("AddPaths(clip)", added_clip);
    c.is_bool("threw", thrown.is_err());
    let what = c.str();
    c.same("what()", &thrown.err().unwrap_or_default(), &what);
    c.is_bool("Execute", ok);
    let n = c.u32();
    let want: Vec<[z::IntPoint; 5]> = (0..n)
        .map(|_| {
            let (a, b, d) = (c.zpoint(), c.zpoint(), c.zpoint());
            let e = c.zpoint();
            [a, b, d, e, c.zpoint()]
        })
        .collect();
    c.same("ZFillFunction calls", &calls, &want);
    let want = c.zpaths();
    c.same("result", &result, &want);
    let n = c.u32();
    let want: Vec<ZNode> = (0..n)
        .map(|_| {
            let depth = c.u32();
            let hole = c.bool();
            let open = c.bool();
            ZNode {
                depth,
                hole,
                open,
                contour: c.zpath(),
            }
        })
        .collect();
    c.same("nodes", &nodes, &want);
}

#[test]
fn clipper_z_matches() {
    replay("PSZC", pszc);
}

fn psrz(c: &mut Case) {
    let from = c.vec3();
    let to = c.vec3();
    let q = geometry::rotation_xyz_diff(&from, &to);
    c.is_f64("rotation_xyz_diff w", q.w());
    c.is_f64("rotation_xyz_diff x", q.x());
    c.is_f64("rotation_xyz_diff y", q.y());
    c.is_f64("rotation_xyz_diff z", q.z());
    c.is_f64("rotation_diff_z", geometry::rotation_diff_z(&from, &to));
}

#[test]
fn rotation_diffs_match() {
    replay("PSRZ", psrz);
}

fn psic(c: &mut Case) {
    let (a, b) = (c.point(), c.point());
    let (cc, q) = (c.point(), c.point());
    c.is_bool(
        "is_point_inside_polygon_corner",
        geometry::is_point_inside_polygon_corner(a, b, cc, q),
    );
}

#[test]
fn polygon_corners_match() {
    replay("PSIC", psic);
}

fn pscv(c: &mut Case) {
    let pts = c.path();
    let th = c.f64();
    let poly = Polygon::new(pts);
    c.is_path("convex_points", &poly.convex_points(th));
    c.is_path("concave_points", &poly.concave_points(th));
}

#[test]
fn convex_and_concave_points_match() {
    replay("PSCV", pscv);
}

fn psrc(c: &mut Case) {
    let (r2, a, b) = (c.f64(), c.f64(), c.f64());
    let (lv2, cc) = (c.f64(), c.f64());
    let mut out = (vec2d(0., 0.), vec2d(0., 0.));
    let n = geometry::ray_circle_intersections_r2_lv2_c(r2, a, b, lv2, cc, &mut out);
    c.is_i32("ray_circle_intersections_r2_lv2_c", n);
    c.is_vec2("out.first", out.0);
    c.is_vec2("out.second", out.1);
}

#[test]
fn ray_circle_intersections_match() {
    replay("PSRC", psrc);
}

fn psci(c: &mut Case) {
    let pts = c.path();
    let (max_r, tol) = (c.f64(), c.f64());
    let pos = c.vec3f();
    let cpos = c.vec3f();
    let ccw = c.bool();
    let n = pts.len();
    let mut c3 = Circle::default();
    let ok3 = Circle::try_create_circle3(pts[0], pts[n / 2], pts[n - 1], max_r, &mut c3);
    c.is_bool("try_create_circle(p0, pm, pn)", ok3);
    c.is_circle("circle of three", &c3);
    let mut cn = Circle::default();
    let okn = Circle::try_create_circle(&pts, max_r, tol, &mut cn);
    c.is_bool("try_create_circle(points)", okn);
    c.is_circle("circle of the points", &cn);
    let circle = if okn {
        cn
    } else if ok3 {
        c3
    } else {
        Circle::new(
            pts[0],
            orca_stl::max(1., (pts[n - 1] - pts[0]).cast_f64().norm()),
        )
    };
    c.is_circle("circle", &circle);
    for &q in &pts {
        c.is_f64(
            &format!("get_polar_radians({q:?})"),
            circle.get_polar_radians(q),
        );
    }
    for &q in &pts {
        c.is_point(
            &format!("get_closest_point({q:?})"),
            circle.get_closest_point(q),
        );
    }
    c.is_bool("is_over_deviation", circle.is_over_deviation(&pts, tol));
    let (okd, sum) = circle.get_deviation_sum_squared(&pts, tol);
    c.is_bool("get_deviation_sum_squared", okd);
    c.is_f64("get_deviation_sum_squared sum", sum);
    for (i, w) in pts.windows(2).enumerate() {
        c.is_opt(
            &format!("get_closest_perpendicular_point {i}"),
            Circle::get_closest_perpendicular_point(w[0], w[1], circle.center),
        );
    }
    c.is_vec3f(
        "calc_tangential_vector",
        Circle::calc_tangential_vector(pos, cpos, ccw),
    );
}

#[test]
fn circles_match() {
    replay("PSCI", psci);
}

/// The queries of an arc (PSAS and PSAC).
fn arc_follow(c: &mut Case, arc: &ArcSegment, queries: &[Point]) {
    let mut r = *arc;
    c.is_bool("reverse", r.reverse());
    c.is_arc("reversed", &r);
    for &q in queries {
        c.is_bool(&format!("is_point_inside({q:?})"), arc.is_point_inside(q));
        let mut s = *arc;
        c.is_bool(&format!("clip_start({q:?})"), s.clip_start(q));
        c.is_arc(&format!("clip_start({q:?}) arc"), &s);
        let mut t = *arc;
        c.is_bool(&format!("clip_end({q:?})"), t.clip_end(q));
        c.is_arc(&format!("clip_end({q:?}) arc"), &t);
        let split = arc.split_at(q);
        c.is_bool(&format!("split_at({q:?})"), split.is_some());
        let (a1, a2) = split.unwrap_or_default();
        c.is_arc(&format!("split_at({q:?}) first"), &a1);
        c.is_arc(&format!("split_at({q:?}) second"), &a2);
    }
}

fn psas(c: &mut Case) {
    let pts = c.path();
    let (approx, max_r) = (c.f64(), c.f64());
    let (tol, pct) = (c.f64(), c.f64());
    let queries = c.path();
    let arc = ArcSegment::try_create_arc(&pts, approx, max_r, tol, pct);
    let ok = arc.is_some();
    let arc = arc.unwrap_or_default();
    c.is_bool("try_create_arc", ok);
    c.is_arc("arc", &arc);
    c.is_bool(
        "are_points_within_slice",
        ok && ArcSegment::are_points_within_slice(&arc, &pts),
    );
    arc_follow(c, &arc, &queries);
}

#[test]
fn arcs_of_points_match() {
    replay("PSAS", psas);
}

fn psac(c: &mut Case) {
    let center = c.point();
    let radius = c.f64();
    let (start, end) = (c.point(), c.point());
    let dir = c.u8();
    let slice = c.path();
    let queries = c.path();
    let (s3, e3) = (c.vec3f(), c.vec3f());
    let c3 = c.vec3f();
    let ccw = c.bool();
    let origin = c.point();
    let ray = c.vec2();
    let (sa, sb) = (c.point(), c.point());
    let arc = ArcSegment::new(center, radius, start, end, direction(dir));
    c.is_arc("ArcSegment(center, radius, start, end, dir)", &arc);
    c.is_bool(
        "are_points_within_slice",
        arc.is_valid() && ArcSegment::are_points_within_slice(&arc, &slice),
    );
    arc_follow(c, &arc, &queries);
    c.is_f32(
        "calc_arc_radian",
        ArcSegment::calc_arc_radian(s3, e3, c3, ccw),
    );
    c.is_f32("calc_arc_radius", ArcSegment::calc_arc_radius(s3, c3));
    c.is_f32(
        "calc_arc_length",
        ArcSegment::calc_arc_length(s3, e3, c3, ccw),
    );
    c.is_bool(
        "ray_intersects_segment",
        ArcSegment::ray_intersects_segment(origin, ray, &Line::new(sa, sb)),
    );
}

#[test]
fn arc_segments_match() {
    replay("PSAC", psac);
}

fn psaf(c: &mut Case) {
    let pts = c.path();
    let tol = c.f64();
    let mut r = Vec::new();
    arc_fitter::do_arc_fitting(&pts, &mut r, tol);
    c.is_fits("do_arc_fitting", &r);
    let mut q = pts.clone();
    let mut r2 = Vec::new();
    arc_fitter::do_arc_fitting_and_simplify(&mut q, &mut r2, tol);
    c.is_path("do_arc_fitting_and_simplify points", &q);
    c.is_fits("do_arc_fitting_and_simplify", &r2);
}

#[test]
fn arc_fitting_matches() {
    replay("PSAF", psaf);
}

fn pspf(c: &mut Case) {
    let pts = c.path();
    let tol = c.f64();
    let pts2 = c.path();
    let tol2 = c.f64();
    let q = c.point();
    let k = c.u64();
    let (dist, frac) = (c.f64(), c.f64());
    let mut pl = Polyline::new(pts.clone());
    pl.simplify_by_fitting_arc(tol);
    c.is_polyline("simplify_by_fitting_arc", &pl);
    {
        let mut r = pl.clone();
        r.reverse();
        c.is_polyline("reverse", &r);
    }
    {
        let mut r = pl.clone();
        r.clip_end(dist);
        c.is_polyline("clip_end", &r);
    }
    {
        let mut r = pl.clone();
        r.clip_start(dist);
        c.is_polyline("clip_start", &r);
    }
    {
        let split = pl.split_at_index(usize::try_from(k).unwrap());
        c.is_bool("split_at_index", split.is_some());
        let (a, b) = split.unwrap_or_default();
        c.is_polyline("split_at_index first", &a);
        c.is_polyline("split_at_index second", &b);
    }
    {
        let (a, b, qq) = pl
            .split_at(q)
            .unwrap_or_else(|| (Polyline::default(), Polyline::default(), q));
        c.is_point("split_at point", qq);
        c.is_polyline("split_at first", &a);
        c.is_polyline("split_at second", &b);
    }
    {
        let split = pl.split_at_length(pl.length() * frac);
        c.is_bool("split_at_length", split.is_some());
        let (a, b) = split.unwrap_or_default();
        c.is_polyline("split_at_length first", &a);
        c.is_polyline("split_at_length second", &b);
    }
    let mut pl2 = Polyline::new(pts2.clone());
    pl2.simplify_by_fitting_arc(tol2);
    c.is_polyline("second simplify_by_fitting_arc", &pl2);
    {
        let mut a = pl.clone();
        a.append_polyline(&pl2);
        c.is_polyline("append(const Polyline &)", &a);
    }
    {
        let mut a = pl.clone();
        let mut b = pl2.clone();
        a.append_polyline_moved(&mut b);
        c.is_polyline("append(Polyline &&)", &a);
        c.is_polyline("append(Polyline &&) source", &b);
    }
    {
        let mut a = pl.clone();
        a.append(q);
        c.is_polyline("append(point)", &a);
    }
    {
        let mut a = pl.clone();
        a.append_before(q);
        c.is_polyline("append_before(point)", &a);
    }
    {
        let mut a = Polyline::new(pts2.clone());
        a.append_polyline(&pl);
        c.is_polyline("Polyline(second points).append(pl)", &a);
    }
    {
        let mut a = pl.clone();
        a.append_points(&pts2);
        c.is_polyline("append(const Points &)", &a);
    }
}

#[test]
fn polyline_fits_match() {
    replay("PSPF", pspf);
}

fn pstp(c: &mut Case) {
    let pts = c.path();
    let n = c.u32();
    let width: Vec<f64> = (0..n).map(|_| c.f64()).collect();
    let (ep1, ep2) = (c.bool(), c.bool());
    let index = c.i32();
    let mut tp = ThickPolyline {
        polyline: Polyline::new(pts),
        width,
        endpoints: (ep1, ep2),
    };
    let got: Vec<(Point, Point, F, F)> = tp
        .thicklines()
        .iter()
        .map(|l| (l.a, l.b, F(l.a_width), F(l.b_width)))
        .collect();
    let n = c.u32();
    let want: Vec<(Point, Point, F, F)> = (0..n)
        .map(|_| {
            let (a, b) = (c.point(), c.point());
            let wa = F(c.f64());
            (a, b, wa, F(c.f64()))
        })
        .collect();
    c.same("thicklines", &got, &want);
    tp.start_at_index(index);
    c.is_path("start_at_index points", &tp.points);
    let n = c.u32();
    let want: Vec<F> = (0..n).map(|_| F(c.f64())).collect();
    let got: Vec<F> = tp.width.iter().copied().map(F).collect();
    c.same("start_at_index widths", &got, &want);
}

#[test]
fn thick_polylines_match() {
    replay("PSTP", pstp);
}

/// Every kind of `paths.sop1` belongs to a piece, and every geom-1 kind is recorded.
#[test]
fn every_paths_kind_has_a_piece() {
    let file = file();
    let unknown: Vec<&str> = file
        .records
        .iter()
        .map(|r| r.kind())
        .filter(|k| {
            !GEOM1_KINDS.contains(k) && !GEOM2_KINDS.contains(k) && !PATHS1_KINDS.contains(k)
        })
        .collect();
    assert!(
        unknown.is_empty(),
        "{}: {} records of no piece ({:?}...)",
        file.path.display(),
        unknown.len(),
        &unknown[..unknown.len().min(4)]
    );
    for kind in GEOM1_KINDS {
        assert!(
            file.of_kind(kind).next().is_some(),
            "{}: no {kind} record",
            file.path.display()
        );
    }
}
