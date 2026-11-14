// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The probe gate of the piece geom-2: the records of `paths.sop1` whose sections in
//! `parity/sop1/paths.md` name geom-2, replayed against orca-geom bit for bit (added 2026-11-13):
//!
//! - PSP3: `Point3`, `Line3`, `lerp`, the `Points`/`Points3` conversions, `BoundingBox3`, the
//!   `MultiPoint3` members of a `Polyline3`, `foot_pt` of a `Points3`, and the throwing
//!   `projection_onto(MultiPoint3)`;
//! - PSD3: the 3D Douglas-Peucker and `Polyline3::simplify`;
//! - PSL3: `Polyline3` with and without a fitting result (reverse, clip_end, the splits, the
//!   appends, to_polyline);
//! - PSC3: the `Points3` forms of `Circle::try_create_circle`, `ArcSegment::try_create_arc` and
//!   `ArcSegment::are_points_within_slice`;
//! - PSO3: `offset` of a `Polyline3`.

use orca_eigen::Vec3f;
use orca_geom::circle::{ArcDirection, ArcSegment, Circle};
use orca_geom::clipper_utils as cu;
use orca_geom::point::Point;
use orca_geom::point3::{self, BoundingBox3, Line3, Point3};
use orca_geom::polyline3::{self, Polyline3};

use super::super::{Case, end_type, join_type};
use super::{Fit, GEOM2_KINDS, file, fits_of, replay};

/// A Polyline3 with its fit, as the probe writes it.
fn polyline3_of(pl: &Polyline3) -> (Vec<Point3>, Vec<Fit>) {
    (pl.points.clone(), fits_of(&pl.fitting_result))
}

impl Case<'_, '_> {
    fn point3(&mut self) -> Point3 {
        let (x, y) = (self.i64(), self.i64());
        Point3::new(x, y, self.i64())
    }
    fn points3(&mut self) -> Vec<Point3> {
        let n = self.u32();
        (0..n).map(|_| self.point3()).collect()
    }
    fn is_point3(&mut self, what: &str, got: Point3) {
        let want = self.point3();
        self.same(what, &got, &want);
    }
    fn is_points3(&mut self, what: &str, got: &[Point3]) {
        let want = self.points3();
        self.same(what, &got.to_vec(), &want);
    }
    fn is_bbox3(&mut self, what: &str, got: &BoundingBox3) {
        let (min, max) = (self.point3(), self.point3());
        let want = (min, max, self.bool());
        let got = (
            Point3::from_vec3crd(got.min),
            Point3::from_vec3crd(got.max),
            got.defined,
        );
        self.same(what, &got, &want);
    }
    fn is_polyline3(&mut self, what: &str, got: &Polyline3) {
        let points = self.points3();
        let want = (points, self.fits());
        self.same(what, &polyline3_of(got), &want);
    }
}

fn psp3(c: &mut Case) {
    let p = c.point3();
    let a = c.point3();
    let b = c.point3();
    let cc = c.point3();
    let (angle, factor) = (c.f64(), c.f64());
    let (t, eps) = (c.f64(), c.f64());
    let pts2 = c.path();
    let pts3 = c.points3();
    let v = c.vec3();
    let k = c.i64();

    let q = p;
    c.is_point3("rotated(angle)", q.rotated(angle));
    c.is_point3("rotated(angle, c)", q.rotated_around(angle, &cc));
    c.is_point3("rotate_90_degree_ccw", q.rotate_90_degree_ccw());
    c.is_point3("operator*(factor) member", q.mul_round(factor));
    c.is_point3("operator*(p, factor) free", q.mul_trunc(factor));
    {
        let mut r = q;
        r.scale_assign(factor);
        c.is_point3("*= factor", r);
    }
    c.is_f64("ccw(a, b)", q.ccw(&a, &b));
    c.is_f64("ccw(Line3(a, b))", q.ccw_line3(&Line3::new(a, b)));
    c.is_f64("ccw_angle(a, b)", q.ccw_angle(&a, &b));
    c.is_point3(
        "projection_onto(Line3(a, b))",
        q.projection_onto_line3(&Line3::new(a, b)),
    );
    c.is_i32("nearest_point_index", q.nearest_point_index(&pts2));
    {
        let got = q.nearest_point(&pts2);
        let ok = c.bool();
        let want = (ok, c.point3());
        c.same(
            "nearest_point",
            &(got.is_some(), got.unwrap_or_default()),
            &want,
        );
    }
    c.is_f64("distance_to(a)", q.distance_to(&a));
    c.is_point("to_point", q.to_point());
    c.is_bool("both_comp(a, >)", q.both_comp(&a, ">"));
    c.is_bool("both_comp(a, <)", q.both_comp(&a, "<"));
    c.is_bool("any_comp(a, >)", q.any_comp(&a, ">"));
    c.is_bool("any_comp(a, <)", q.any_comp(&a, "<"));
    c.is_bool("any_comp(c.x, >)", q.any_comp_coord(cc.x, ">"));
    c.is_bool("any_comp(c.x, <)", q.any_comp_coord(cc.x, "<"));

    let l = Line3::new(a, b);
    c.is_f64("Line3::length", l.length());
    c.is_point3("Line3::vector", l.vector());
    c.is_point3("Line3::midpoint", l.midpoint());
    c.is_point("Line3::to_line a", l.to_line().a);
    c.is_point("Line3::to_line b", l.to_line().b);
    c.is_f64(
        "Line3::distance_to_squared(p, a, b)",
        Line3::distance_to_squared(&p, &a, &b),
    );
    c.is_point3("lerp(a, b, t)", point3::lerp(&a, &b, t));
    c.is_point3("Point3::new_scale(Vec3d)", Point3::new_scale_vec3d(v));
    let vf: Vec3f = v.cast::<f32>();
    c.is_point3("Point3::new_scale(Vec3f)", Point3::new_scale_vec3f(vf));
    c.is_point3(
        "Point3::new_scale(x, y, z)",
        Point3::new_scale(v.x(), v.y(), v.z()),
    );
    c.is_path("to_points(pts3)", &point3::to_points(&pts3));
    c.is_points3("to_points3(pts2)", &point3::to_points3(&pts2));

    c.is_bbox3("BoundingBox3(pts3)", &BoundingBox3::from_points(&pts3));

    let pl = Polyline3::new(pts3.clone());
    c.is_f64("Polyline3::length", pl.length());
    c.is_bbox3("Polyline3::bounding_box", &pl.bounding_box());
    let vertex = if pts3.is_empty() {
        a
    } else {
        pts3[(k as u64 % pts3.len() as u64) as usize]
    };
    for (name, f) in [("a", a), ("vertex", vertex)] {
        c.is_i32(
            &format!("find_point({name} 2D)"),
            pl.find_point(f.to_point()),
        );
        c.is_i32(
            &format!("find_point({name} 2D, eps)"),
            pl.find_point_eps(f.to_point(), eps),
        );
        c.is_i32(&format!("find_point({name})"), pl.find_point3(&f));
        c.is_i32(
            &format!("find_point({name}, eps)"),
            pl.find_point3_eps(&f, eps),
        );
    }
    {
        let mut m = pl.clone();
        m.translate(&cc);
        c.is_points3("translate(c)", &m.points);
    }
    {
        let mut m = pl.clone();
        m.rotate(angle);
        c.is_points3("rotate(angle)", &m.points);
    }
    {
        let mut m = pl.clone();
        m.rotate_around(angle, &cc);
        c.is_points3("rotate(angle, c)", &m.points);
    }
    {
        let mut m = pl.clone();
        let ok = m.remove_duplicate_points();
        c.is_bool("remove_duplicate_points", ok);
        c.is_points3("remove_duplicate_points points", &m.points);
    }
    {
        let got: Vec<(Point3, Point3)> = pl.lines().iter().map(|s| (s.a, s.b)).collect();
        let n = c.u32();
        let want: Vec<(Point3, Point3)> = (0..n)
            .map(|_| {
                let sa = c.point3();
                (sa, c.point3())
            })
            .collect();
        c.same("lines", &got, &want);
    }
    {
        let (idx, foot) = polyline3::foot_pt(&pts3, &p);
        c.is_i32("foot_pt index", idx);
        c.is_point3("foot_pt point", foot);
    }
    {
        let r = q.projection_onto_multi_point3(&pl.points);
        c.is_bool("projection_onto(MultiPoint3) threw", r.is_err());
        let what = c.str();
        c.same(
            "projection_onto(MultiPoint3) what()",
            &r.err().unwrap_or_default(),
            &what,
        );
    }
}

#[test]
fn points3_lines3_and_boxes3_match() {
    replay("PSP3", psp3);
}

fn psd3(c: &mut Case) {
    let pts = c.points3();
    let tol = c.f64();
    c.is_points3(
        "_douglas_peucker",
        &orca_geom::multi_point3::douglas_peucker(&pts, tol),
    );
    let mut pl = Polyline3::new(pts);
    pl.simplify(tol);
    c.is_points3("Polyline3::simplify", &pl.points);
}

#[test]
fn douglas_peucker_3d_matches() {
    replay("PSD3", psd3);
}

/// The inputs of a PSL3 record.
struct L3Case {
    q2: Point,
    q3: Point3,
    k: i64,
    dist: f64,
    frac: f64,
}

/// The operations of PSL3 on `base` (fitted or not), `other` being the second polyline.
fn l3_ops(c: &mut Case, base: &Polyline3, other: &Polyline3, x: &L3Case) {
    c.is_polyline3("base", base);
    {
        let mut r = base.clone();
        r.reverse();
        c.is_polyline3("reverse", &r);
    }
    {
        let mut r = base.clone();
        r.clip_end(x.dist);
        c.is_polyline3("clip_end", &r);
    }
    {
        let (mut a, mut b) = (Polyline3::default(), Polyline3::default());
        let ok = base.split_at_index(usize::try_from(x.k).unwrap(), &mut a, &mut b);
        c.is_bool("split_at_index", ok);
        c.is_polyline3("split_at_index first", &a);
        c.is_polyline3("split_at_index second", &b);
    }
    {
        let mut q = x.q2;
        let (mut a, mut b) = (Polyline3::default(), Polyline3::default());
        base.split_at_point(&mut q, &mut a, &mut b);
        c.is_point("split_at(Point) point", q);
        c.is_polyline3("split_at(Point) first", &a);
        c.is_polyline3("split_at(Point) second", &b);
    }
    {
        let mut q = x.q3;
        let (mut a, mut b) = (Polyline3::default(), Polyline3::default());
        base.split_at_point3(&mut q, &mut a, &mut b);
        c.is_point3("split_at(Point3) point", q);
        c.is_polyline3("split_at(Point3) first", &a);
        c.is_polyline3("split_at(Point3) second", &b);
    }
    {
        let (mut a, mut b) = (Polyline3::default(), Polyline3::default());
        let ok = base.split_at_length(base.length() * x.frac, &mut a, &mut b);
        c.is_bool("split_at_length", ok);
        c.is_polyline3("split_at_length first", &a);
        c.is_polyline3("split_at_length second", &b);
    }
    c.is_polyline3("other", other);
    {
        let mut a = base.clone();
        a.append_polyline3(other);
        c.is_polyline3("append(other)", &a);
    }
    {
        let mut a = base.clone();
        a.append(x.q3);
        c.is_polyline3("append(q3)", &a);
    }
    {
        let mut a = base.clone();
        a.append_before(x.q3);
        c.is_polyline3("append_before(q3)", &a);
    }
    c.is_path("to_polyline", &base.to_polyline().points);
}

fn psl3(c: &mut Case) {
    let pts = c.points3();
    let pts2 = c.points3();
    let (tol, tol2) = (c.f64(), c.f64());
    let q2 = c.point();
    let q3 = c.point3();
    let k = c.i64();
    let (dist, frac) = (c.f64(), c.f64());
    let x = L3Case {
        q2,
        q3,
        k,
        dist,
        frac,
    };
    let raw = Polyline3::new(pts);
    let raw2 = Polyline3::new(pts2);
    l3_ops(c, &raw, &raw2, &x);
    let mut fit = raw.clone();
    let mut fit2 = raw2.clone();
    fit.simplify_by_fitting_arc(tol);
    fit2.simplify_by_fitting_arc(tol2);
    l3_ops(c, &fit, &fit2, &x);
    {
        // A polyline without a fitting result takes one with it: the fake linear move.
        let mut a = raw.clone();
        a.append_polyline3(&fit2);
        c.is_polyline3("raw append(fit2)", &a);
    }
}

#[test]
fn polylines3_match() {
    replay("PSL3", psl3);
}

fn psc3(c: &mut Case) {
    let pts = c.points3();
    let (max_r, tol) = (c.f64(), c.f64());
    let (approx, pct) = (c.f64(), c.f64());
    let mut circle = Circle::default();
    let okc = Circle::try_create_circle_points3(&pts, max_r, tol, &mut circle);
    c.is_bool("try_create_circle(Points3)", okc);
    c.is_circle("circle", &circle);
    let arc = if pts.len() >= 2 {
        ArcSegment::try_create_arc_points3(&pts, approx, max_r, tol, pct)
    } else {
        None
    };
    let oka = arc.is_some();
    let arc = arc.unwrap_or_default();
    c.is_bool("try_create_arc(Points3)", oka);
    c.is_arc("arc", &arc);
    c.is_bool(
        "are_points_within_slice(arc, Points3)",
        oka && ArcSegment::are_points_within_slice_points3(&arc, &pts),
    );
    let pts2 = point3::to_points(&pts);
    for dir in [ArcDirection::Ccw, ArcDirection::Cw] {
        let a = ArcSegment::new(
            circle.center,
            if okc { circle.radius } else { 0.0 },
            pts[0].to_point(),
            pts[pts.len() - 1].to_point(),
            dir,
        );
        let valid = a.is_valid();
        c.is_bool(&format!("{dir:?} is_valid"), valid);
        c.is_bool(
            &format!("{dir:?} are_points_within_slice(Points3)"),
            valid && pts.len() >= 3 && ArcSegment::are_points_within_slice_points3(&a, &pts),
        );
        c.is_bool(
            &format!("{dir:?} are_points_within_slice(Points)"),
            valid && pts.len() >= 3 && ArcSegment::are_points_within_slice(&a, &pts2),
        );
    }
}

#[test]
fn arc_fits_of_points3_match() {
    replay("PSC3", psc3);
}

fn pso3(c: &mut Case) {
    let pts = c.points3();
    let delta = c.f32();
    let join = c.u8();
    let miter = c.f64();
    let end = c.u8();
    c.is_polygons(
        "offset(Polyline3)",
        &cu::offset_polyline3(
            &Polyline3::new(pts),
            delta,
            join_type(join),
            miter,
            end_type(end),
        ),
    );
}

#[test]
fn offsets_of_polylines3_match() {
    replay("PSO3", pso3);
}

/// Every geom-2 kind is recorded.
#[test]
fn every_geom2_kind_is_recorded() {
    let file = file();
    for kind in GEOM2_KINDS {
        assert!(
            file.of_kind(kind).next().is_some(),
            "{}: no {kind} record",
            file.path.display()
        );
    }
}
