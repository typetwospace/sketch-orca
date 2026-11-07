// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The probe gate for orca-geom: `geom.sop1`'s records but the Eigen ones, which orca-eigen replays
//! (`parity/SOP1.md`, "geom.sop1"), replayed against the crate bit for bit (added 2026-11-07):
//!
//! - CCLP: `Clipper` runs, every clip and fill type, `PolyTree` results, open paths, the three
//!   `initOptions`, `GetBounds` and the exceptions `AddPath` and `Execute` throw;
//! - COFF: `ClipperOffset` runs, every join and end type, miter limits, arc tolerances and the
//!   shortest edge length;
//! - CARE, CCLN, CSIM and CMNK: `Area`, `Orientation` and `PointInPolygon`; `CleanPolygon`;
//!   `SimplifyPolygon(s)`; `MinkowskiSum` and `MinkowskiDiff`;
//! - CINT: `Int128`'s filtered predicates, and through them the exact ones;
//! - GPNT: `Point` and `Line` and the `Geometry` predicates on them (`orient`, the segment and
//!   ray intersections, `directions_parallel`, the angle folds);
//! - GPLY: `Polygon`, `Polyline` and `MultiPoint` (Douglas-Peucker, simplify, clip, split, the
//!   intersections), `BoundingBox`, the 2D convex hull and the stick and collinear clean-ups;
//! - GCLU: ClipperUtils (offsets, the boolean operations on polygons, ExPolygons and polylines,
//!   closing and opening, the variable offsets, `top_level_islands`, the bounding box clip) and
//!   `ExPolygon`;
//! - GXFM: `assemble_transform`, `rotation_transform`, `extract_euler_angles`,
//!   `BoundingBoxf3::transformed`, the convex hull of `Vec3d`s in x and y, `make_circle`,
//!   `rad2deg_dir`, `convex_polygons_intersect` and the top and bottom chains of a convex polygon.
//!
//! The committed subset (`parity/probes/`) runs by default; `SKETCH_ORCA_PROBES_DEEP=<folder>`
//! replays the full set from that folder (`<work>/parity/probes`).

use std::fmt::{self, Debug, Formatter};
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::Once;

use orca_eigen::{Transform3d, Vec2d, Vec3d, Vector};
use orca_geom::bounding_box::{BoundingBox, BoundingBoxf3};
use orca_geom::clipper::{
    self, ClipType, Clipper, ClipperException, ClipperOffset, EndType, IntPoint, IntRect, JoinType,
    NodeId, Path, Paths, PolyFillType, PolyTree, PolyType, ROOT,
};
use orca_geom::clipper_utils::{self as cu, ApplySafetyOffset};
use orca_geom::convex_hull::{
    convex_hull, convex_hull_3d, convex_polygons_intersect, decompose_convex_polygon_top_bottom,
    inside_convex_polygon,
};
use orca_geom::expolygon::{ExPolygon, ExPolygons};
use orca_geom::geometry;
use orca_geom::int128;
use orca_geom::line::Line;
use orca_geom::multi_point;
use orca_geom::point::{self as pt, Point, Points, SCALING_FACTOR, vec2d};
use orca_geom::polygon::{self, Polygon, Polygons};
use orca_geom::polyline::{self, Polyline, Polylines};
use orca_probe::{Mismatches, ProbeFile, Reader, load_probe};

fn file() -> ProbeFile {
    load_probe("geom").unwrap_or_else(|e| panic!("{e}"))
}

/// The kinds of `geom.sop1` that orca-eigen's `tests/probes.rs` replays.
const EIGEN_KINDS: [&str; 5] = ["EVEC", "EMAT", "ETRF", "EROT", "ESVD"];
/// The kinds this file replays.
const GEOM_KINDS: [&str; 11] = [
    "CCLP", "COFF", "CARE", "CCLN", "CSIM", "CMNK", "CINT", "GPNT", "GPLY", "GCLU", "GXFM",
];

/// An `f64` compared by its bits, so that a sign of zero or a NaN counts.
#[derive(Clone, Copy)]
struct F(f64);

impl PartialEq for F {
    fn eq(&self, o: &Self) -> bool {
        self.0.to_bits() == o.0.to_bits()
    }
}

impl Debug for F {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

fn f2(v: Vec2d) -> [F; 2] {
    [F(v.x()), F(v.y())]
}

fn f3(v: Vec3d) -> [F; 3] {
    [F(v.x()), F(v.y()), F(v.z())]
}

/// Runs `f`, turning a thrown [`ClipperException`] into its `what()`; other panics go on.
fn catch<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    static QUIET: Once = Once::new();
    QUIET.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if info.payload().downcast_ref::<ClipperException>().is_none() {
                default(info);
            }
        }));
    });
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(v) => Ok(v),
        Err(payload) => match payload.downcast::<ClipperException>() {
            Ok(e) => Err(e.what().to_owned()),
            Err(other) => resume_unwind(other),
        },
    }
}

/// A `PolyNode` below the root as the probe writes it.
#[derive(Debug, PartialEq, Eq)]
struct Node {
    open: bool,
    hole: bool,
    contour: Path,
    children: Vec<Node>,
}

/// A `PolyTree` as the probe writes it: the root's contour, `Total()` and the children.
#[derive(Debug, PartialEq, Eq)]
struct Tree {
    contour: Path,
    total: i32,
    children: Vec<Node>,
}

fn nodes_of(t: &PolyTree, id: NodeId) -> Vec<Node> {
    t.node(id)
        .childs
        .iter()
        .map(|&c| Node {
            open: t.is_open(c),
            hole: t.is_hole(c),
            contour: t.node(c).contour.clone(),
            children: nodes_of(t, c),
        })
        .collect()
}

fn tree_of(t: &PolyTree) -> Tree {
    Tree {
        contour: t.root().contour.clone(),
        total: t.total(),
        children: nodes_of(t, ROOT),
    }
}

/// One record's reader, the mismatch list and the record's label.
struct Case<'a, 'm> {
    r: Reader<'a>,
    m: &'m mut Mismatches,
    label: String,
}

impl Case<'_, '_> {
    fn u8(&mut self) -> u8 {
        self.r.u8().unwrap()
    }
    fn u32(&mut self) -> u32 {
        self.r.u32().unwrap()
    }
    fn i32(&mut self) -> i32 {
        self.r.i32().unwrap()
    }
    fn i64(&mut self) -> i64 {
        self.r.i64().unwrap()
    }
    fn f64(&mut self) -> f64 {
        self.r.f64().unwrap()
    }
    fn bool(&mut self) -> bool {
        self.r.bool().unwrap()
    }
    fn str(&mut self) -> String {
        self.r.str().unwrap()
    }
    fn point(&mut self) -> IntPoint {
        let x = self.i64();
        IntPoint::new(x, self.i64())
    }
    fn path(&mut self) -> Path {
        let n = self.u32();
        (0..n).map(|_| self.point()).collect()
    }
    fn paths(&mut self) -> Paths {
        let n = self.u32();
        (0..n).map(|_| self.path()).collect()
    }
    fn nodes(&mut self) -> Vec<Node> {
        let n = self.u32();
        (0..n)
            .map(|_| {
                let open = self.bool();
                let hole = self.bool();
                let contour = self.path();
                Node {
                    open,
                    hole,
                    contour,
                    children: self.nodes(),
                }
            })
            .collect()
    }
    fn tree(&mut self) -> Tree {
        let contour = self.path();
        let total = self.i32();
        Tree {
            contour,
            total,
            children: self.nodes(),
        }
    }
    fn f32(&mut self) -> f32 {
        self.r.f32().unwrap()
    }
    fn vec2(&mut self) -> Vec2d {
        let x = self.f64();
        vec2d(x, self.f64())
    }
    fn vec3(&mut self) -> Vec3d {
        let (x, y) = (self.f64(), self.f64());
        Vector([x, y, self.f64()])
    }
    fn vec2s(&mut self) -> Vec<Vec2d> {
        let n = self.u32();
        (0..n).map(|_| self.vec2()).collect()
    }
    fn vec3s(&mut self) -> Vec<Vec3d> {
        let n = self.u32();
        (0..n).map(|_| self.vec3()).collect()
    }
    fn polygons(&mut self) -> Polygons {
        let n = self.u32();
        (0..n).map(|_| Polygon::new(self.path())).collect()
    }
    fn polylines(&mut self) -> Polylines {
        let n = self.u32();
        (0..n).map(|_| Polyline::new(self.path())).collect()
    }
    fn expolygons(&mut self) -> ExPolygons {
        let n = self.u32();
        (0..n)
            .map(|_| {
                let contour = Polygon::new(self.path());
                ExPolygon {
                    contour,
                    holes: self.polygons(),
                }
            })
            .collect()
    }
    fn deltas(&mut self, n: usize) -> Vec<Vec<f32>> {
        (0..n)
            .map(|_| {
                let k = self.u32();
                (0..k).map(|_| self.f32()).collect()
            })
            .collect()
    }

    /// Reads the engine's value of each kind and compares `got` with it.
    fn is_bool(&mut self, what: &str, got: bool) {
        let want = self.bool();
        self.same(what, &got, &want);
    }
    fn is_i32(&mut self, what: &str, got: i32) {
        let want = self.i32();
        self.same(what, &got, &want);
    }
    fn is_f64(&mut self, what: &str, got: f64) {
        let want = F(self.f64());
        self.same(what, &F(got), &want);
    }
    fn is_point(&mut self, what: &str, got: Point) {
        let want = self.point();
        self.same(what, &got, &want);
    }
    /// A `bool` and a point, the origin when the call returned false.
    fn is_opt(&mut self, what: &str, got: Option<Point>) {
        let ok = self.bool();
        let want = (ok, self.point());
        self.same(what, &(got.is_some(), got.unwrap_or_default()), &want);
    }
    fn is_vec2(&mut self, what: &str, got: Vec2d) {
        let want = f2(self.vec2());
        self.same(what, &f2(got), &want);
    }
    fn is_vec3(&mut self, what: &str, got: Vec3d) {
        let want = f3(self.vec3());
        self.same(what, &f3(got), &want);
    }
    fn is_vec2s(&mut self, what: &str, got: &[Vec2d]) {
        let want: Vec<[F; 2]> = self.vec2s().into_iter().map(f2).collect();
        let got: Vec<[F; 2]> = got.iter().copied().map(f2).collect();
        self.same(what, &got, &want);
    }
    fn is_vec3s(&mut self, what: &str, got: &[Vec3d]) {
        let want: Vec<[F; 3]> = self.vec3s().into_iter().map(f3).collect();
        let got: Vec<[F; 3]> = got.iter().copied().map(f3).collect();
        self.same(what, &got, &want);
    }
    fn is_path(&mut self, what: &str, got: &[Point]) {
        let want = self.path();
        self.same(what, &got.to_vec(), &want);
    }
    fn is_polygons(&mut self, what: &str, got: &Polygons) {
        let want = self.polygons();
        self.same(what, got, &want);
    }
    fn is_polylines(&mut self, what: &str, got: &Polylines) {
        let want = self.polylines();
        self.same(what, got, &want);
    }
    fn is_expolygons(&mut self, what: &str, got: &ExPolygons) {
        let want = self.expolygons();
        self.same(what, got, &want);
    }
    fn is_bbox(&mut self, what: &str, got: &BoundingBox) {
        let (min, max) = (self.point(), self.point());
        let want = BoundingBox {
            min,
            max,
            defined: self.bool(),
        };
        self.same(what, got, &want);
    }
    /// A transform's 4x4 matrix, row by row.
    fn is_transform(&mut self, what: &str, got: &Transform3d) {
        let want: Vec<F> = (0..16).map(|_| F(self.f64())).collect();
        let got: Vec<F> = (0..4)
            .flat_map(|r| (0..4).map(move |c| F(got.m.at(r, c))))
            .collect();
        self.same(what, &got, &want);
    }

    /// Compares one result with the C++ engine's.
    fn same<T: PartialEq + Debug>(&mut self, what: &str, got: &T, want: &T) {
        let label = &self.label;
        self.m.check(got == want, || {
            let cut = |s: String| s.chars().take(900).collect::<String>();
            format!(
                "{label}: {what}: {} instead of {}",
                cut(format!("{got:?}")),
                cut(format!("{want:?}"))
            )
        });
    }

    /// Reads the outcome of a call that may throw (0 or 1, or 2 and `what()`) and compares
    /// `got` with it; true when the record goes on (the call returned on both sides).
    fn outcome(&mut self, what: &str, got: &Result<bool, String>) -> bool {
        let flag = self.u8();
        let want = if flag == 2 {
            Err(self.str())
        } else {
            Ok(flag == 1)
        };
        self.same(what, got, &want);
        want.is_ok() && got.is_ok()
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

fn clip_type(v: u8) -> ClipType {
    [
        ClipType::Intersection,
        ClipType::Union,
        ClipType::Difference,
        ClipType::Xor,
    ][usize::from(v)]
}

fn fill_type(v: u8) -> PolyFillType {
    [
        PolyFillType::EvenOdd,
        PolyFillType::NonZero,
        PolyFillType::Positive,
        PolyFillType::Negative,
    ][usize::from(v)]
}

fn join_type(v: u8) -> JoinType {
    [JoinType::Square, JoinType::Round, JoinType::Miter][usize::from(v)]
}

fn end_type(v: u8) -> EndType {
    [
        EndType::ClosedPolygon,
        EndType::ClosedLine,
        EndType::OpenButt,
        EndType::OpenSquare,
        EndType::OpenRound,
    ][usize::from(v)]
}

fn poly_type(v: u8) -> PolyType {
    if v == 0 {
        PolyType::Subject
    } else {
        PolyType::Clip
    }
}

fn slices(paths: &[Path]) -> impl Iterator<Item = &[IntPoint]> {
    paths.iter().map(Vec::as_slice)
}

/// A whole CCLP record, which stops early where the C++ engine threw.
fn cclp(c: &mut Case) {
    let opts = i32::from(c.u8());
    let (ct, sf, cf) = (clip_type(c.u8()), fill_type(c.u8()), fill_type(c.u8()));
    let (tree, each, twice, convert) = (c.bool(), c.bool(), c.bool(), c.bool());
    let groups: Vec<(PolyType, bool, Paths)> = (0..c.u32())
        .map(|_| {
            let pt = poly_type(c.u8());
            let closed = c.bool();
            (pt, closed, c.paths())
        })
        .collect();
    let any_open = groups.iter().any(|g| !g.1);
    let mut clipper = Clipper::new(opts);
    for (pt, closed, paths) in &groups {
        if each {
            for path in paths {
                let got = catch(|| clipper.add_path(path, *pt, *closed));
                if !c.outcome("AddPath", &got) {
                    return;
                }
            }
        } else {
            let got = catch(|| clipper.add_paths(slices(paths), *pt, *closed));
            if !c.outcome("AddPaths", &got) {
                return;
            }
        }
    }
    if !any_open {
        let want = IntRect {
            left: c.i64(),
            top: c.i64(),
            right: c.i64(),
            bottom: c.i64(),
        };
        c.same("GetBounds", &clipper.get_bounds(), &want);
    }
    for _ in 0..if twice { 2 } else { 1 } {
        if tree {
            let mut t = PolyTree::new();
            let ok = clipper.execute_tree(ct, &mut t, sf, cf);
            let want = c.bool();
            c.same("Execute", &ok, &want);
            let want = c.tree();
            c.same("PolyTree", &tree_of(&t), &want);
            if convert {
                let want = c.paths();
                c.same("PolyTreeToPaths", &clipper::poly_tree_to_paths(&t), &want);
                let want = c.paths();
                c.same(
                    "ClosedPathsFromPolyTree",
                    &clipper::closed_paths_from_poly_tree(&t),
                    &want,
                );
                let want = c.paths();
                c.same(
                    "OpenPathsFromPolyTree",
                    &clipper::open_paths_from_poly_tree(&t),
                    &want,
                );
            }
        } else {
            let mut out = Paths::new();
            let got = catch(|| clipper.execute(ct, &mut out, sf, cf));
            if !c.outcome("Execute", &got) {
                return;
            }
            let want = c.paths();
            c.same("solution", &out, &want);
        }
    }
}

#[test]
fn clipper_runs_match_clipper() {
    replay("CCLP", cclp);
}

/// A whole COFF record.
fn coff(c: &mut Case) {
    let (miter, arc, shortest) = (c.f64(), c.f64(), c.f64());
    let mut co = ClipperOffset::new(miter, arc, shortest);
    for _ in 0..c.u32() {
        let (join, end) = (join_type(c.u8()), end_type(c.u8()));
        let path = c.path();
        co.add_path(&path, join, end);
    }
    let runs: Vec<(f64, bool)> = (0..c.u32())
        .map(|_| {
            let delta = c.f64();
            (delta, c.bool())
        })
        .collect();
    for (delta, tree) in runs {
        if tree {
            let mut t = PolyTree::new();
            let got = catch(|| co.execute_tree(&mut t, delta)).map(|()| true);
            if !c.outcome("Execute(PolyTree)", &got) {
                return;
            }
            let want = c.tree();
            c.same("PolyTree", &tree_of(&t), &want);
        } else {
            let mut out = Paths::new();
            let got = catch(|| co.execute(&mut out, delta)).map(|()| true);
            if !c.outcome("Execute(Paths)", &got) {
                return;
            }
            let want = c.paths();
            c.same("solution", &out, &want);
        }
    }
}

#[test]
fn offsets_match_clipper() {
    replay("COFF", coff);
}

#[test]
fn area_orientation_and_point_in_polygon_match_clipper() {
    replay("CARE", |c| {
        let path = c.path();
        let want = c.f64();
        c.same("Area", &clipper::area(&path).to_bits(), &want.to_bits());
        let want = c.bool();
        c.same("Orientation", &clipper::orientation(&path), &want);
        for _ in 0..c.u32() {
            let pt = c.point();
            let want = c.i32();
            c.same(
                &format!("PointInPolygon({pt:?})"),
                &clipper::point_in_polygon(pt, &path),
                &want,
            );
        }
    });
}

#[test]
fn clean_polygon_matches_clipper() {
    replay("CCLN", |c| {
        let path = c.path();
        let distance = c.f64();
        let want = c.path();
        c.same(
            "CleanPolygon",
            &clipper::clean_polygon(&path, distance),
            &want,
        );
    });
}

#[test]
fn simplify_polygons_match_clipper() {
    replay("CSIM", |c| {
        let many = c.bool();
        let paths = c.paths();
        let (fill, strictly) = (fill_type(c.u8()), c.bool());
        let got = if many {
            clipper::simplify_polygons(slices(&paths), fill, strictly)
        } else {
            clipper::simplify_polygon(&paths[0], fill, strictly)
        };
        let want = c.paths();
        c.same("SimplifyPolygon(s)", &got, &want);
    });
}

#[test]
fn minkowski_matches_clipper() {
    replay("CMNK", |c| {
        let (kind, closed) = (c.u8(), c.bool());
        let pattern = c.path();
        let paths = c.paths();
        let got = match kind {
            0 => clipper::minkowski_sum(&pattern, &paths[0], closed),
            1 => clipper::minkowski_sum_paths(&pattern, &paths, closed),
            _ => clipper::minkowski_diff(&pattern, &paths[0]),
        };
        let want = c.paths();
        c.same("Minkowski", &got, &want);
    });
}

#[test]
fn int128_predicates_match() {
    replay("CINT", |c| {
        let (a, b, d, e) = (c.i64(), c.i64(), c.i64(), c.i64());
        let got = [
            int128::sign_determinant_2x2_filtered(a, b, d, e),
            int128::compare_rationals_filtered(a, b, d, e),
        ];
        let want = [c.i32(), c.i32()];
        c.same(&format!("Int128 ({a}, {b}, {d}, {e})"), &got, &want);
    });
}

/// `double(q) * SCALING_FACTOR` of each coordinate, as the probe unscales a point.
fn unscaled(q: Point) -> Vec2d {
    vec2d(q.x as f64 * SCALING_FACTOR, q.y as f64 * SCALING_FACTOR)
}

/// A whole GPNT record.
fn gpnt(c: &mut Case) {
    let (p, a, b, cc, d, center) = (
        c.point(),
        c.point(),
        c.point(),
        c.point(),
        c.point(),
        c.point(),
    );
    let (angle, dmax) = (c.f64(), c.f64());

    c.is_point("rotate", p.rotated(angle));
    c.is_point("rotate(center)", p.rotated_around(angle, center));
    c.is_f64("ccw", p.ccw(a, b));
    c.is_f64("ccw_angle", p.ccw_angle(a, b));
    let (l, m) = (Line::new(a, b), Line::new(cc, d));
    c.is_point("projection_onto(Line)", p.projection_onto_line(&l));
    c.is_f64("distance_to", l.distance_to(p));
    c.is_f64("distance_to_infinite", l.distance_to_infinite(p));
    c.is_f64("perp_distance_to", l.perp_distance_to(p));
    c.is_opt("intersection", l.intersection(&m));
    c.is_opt("intersection_infinite", l.intersection_infinite(&m));
    c.is_f64("direction", l.direction());
    c.is_f64("orientation", l.orientation());
    c.is_f64("atan2_", l.atan2_());
    c.is_bool("parallel_to(Line)", l.parallel_to(&m));
    c.is_bool("perpendicular_to(Line)", l.perpendicular_to(&m));
    c.is_bool("parallel_to(angle)", l.parallel_to_angle(angle));
    c.is_bool("perpendicular_to(angle)", l.perpendicular_to_angle(angle));
    c.is_point("midpoint", l.midpoint());
    c.is_point("normal", l.normal());
    let mut e = l;
    e.extend(dmax);
    c.is_point("extend a", e.a);
    c.is_point("extend b", e.b);
    let overlap = l.overlap(&m);
    c.is_bool("overlap", overlap.is_some());
    c.is_f64("overlap length", overlap.unwrap_or(0.0));
    c.is_i32("orient", geometry::orient(a, b, p) as i32);
    c.is_bool(
        "segments_intersect",
        geometry::segments_intersect(a, b, cc, d),
    );
    let angle2 = m.direction();
    c.is_bool(
        "directions_parallel",
        geometry::directions_parallel(angle, angle2, dmax * 1e-9),
    );
    c.is_bool(
        "directions_perpendicular",
        geometry::directions_perpendicular(angle, angle2, dmax * 1e-9),
    );
    let mut r = angle * 7.0;
    geometry::to_range_pi_pi(&mut r);
    c.is_f64("to_range_pi_pi", r);
    c.is_f64("angle_to_0_2PI", geometry::angle_to_0_2pi(angle));
    let (dist, closest) = l.distance_to_squared_closest(p);
    c.is_f64("distance_to_squared", dist);
    c.is_point("distance_to_squared closest", closest);
    let (dist, closest) = l.distance_to_infinite_squared(p);
    c.is_f64("distance_to_infinite_squared", dist);
    c.is_point("distance_to_infinite_squared closest", closest);
    c.is_i32("nearest_point_index", p.nearest_point_index(&[a, b, cc, d]));
    let rays = [
        geometry::ray_ray_intersection(
            a.cast_f64(),
            (b - a).cast_f64(),
            cc.cast_f64(),
            (d - cc).cast_f64(),
        ),
        geometry::segment_segment_intersection(
            a.cast_f64(),
            (b - a).cast_f64(),
            cc.cast_f64(),
            (d - cc).cast_f64(),
        ),
    ];
    for (what, got) in ["ray_ray_intersection", "segment_segment_intersection"]
        .into_iter()
        .zip(rays)
    {
        c.is_bool(what, got.is_some());
        c.is_vec2(what, got.unwrap_or(vec2d(0.0, 0.0)));
    }
    c.is_vec2("foot_pt", geometry::foot_pt_line(&l, p));
    c.is_f64("ray_point_distance", geometry::ray_point_distance(&l, p));
}

#[test]
fn points_lines_and_predicates_match() {
    replay("GPNT", gpnt);
}

/// A whole GPLY record.
fn gply(c: &mut Case) {
    let pts = c.path();
    let (q, a, b) = (c.point(), c.point(), c.point());
    let (tol, frac, angle, factor) = (c.f64(), c.f64(), c.f64(), c.f64());

    let poly = Polygon::new(pts.clone());
    c.is_f64("area", poly.area());
    c.is_f64("length", poly.length());
    c.is_point("centroid", poly.centroid());
    c.is_bool("is_counter_clockwise", poly.is_counter_clockwise());
    c.is_bool("is_ccw", geometry::is_ccw(&poly));
    c.is_bool("contains", poly.contains(q));
    c.is_bool("contains(border false)", polygon::contains(&poly, q, false));
    c.is_point("point_projection", poly.point_projection(q));
    let mut dp = poly.clone();
    dp.douglas_peucker(tol);
    c.is_path("douglas_peucker", &dp.points);
    c.is_polygons("simplify", &poly.simplify(tol));
    let mut tri = Polygons::new();
    poly.triangulate_convex(&mut tri);
    c.is_polygons("triangulate_convex", &tri);
    c.is_bool(
        "polygon_is_convex",
        polygon::polygon_is_convex(&poly.points),
    );
    let mut rc = poly.clone();
    polygon::remove_collinear(&mut rc);
    c.is_path("remove_collinear", &rc.points);
    let mut rs = poly.clone();
    c.is_bool("remove_sticks", polygon::remove_sticks(&mut rs));
    c.is_path("remove_sticks points", &rs.points);
    let mut rn = poly.clone();
    c.is_bool(
        "remove_same_neighbor",
        polygon::remove_same_neighbor(&mut rn),
    );
    c.is_path("remove_same_neighbor points", &rn.points);
    let bb = poly.bounding_box();
    c.is_bbox("bounding_box", &bb);
    let dist = frac * orca_stl::max(1.0, poly.length()) * 0.25;
    let mut bo = bb;
    bo.offset(dist);
    c.is_bbox("BoundingBox::offset", &bo);
    let mut bs = bb;
    bs.scale(factor);
    c.is_bbox("BoundingBox::scale", &bs);
    c.is_point("BoundingBox::center", bb.center());
    c.is_point("BoundingBox::size", bb.size());
    c.is_f64("BoundingBox::radius", bb.radius());
    c.is_bool("BoundingBox::contains", bb.contains(q));
    c.is_bbox("BoundingBox::rotated", &bb.rotated(angle));
    c.is_path("BoundingBox::polygon", &bb.polygon().points);
    let l = Line::new(a, b);
    c.is_opt("first_intersection", poly.first_intersection(&l));
    let mut ips = Points::new();
    c.is_bool("intersections", poly.intersections(&l, &mut ips));
    c.is_path("intersections points", &ips);
    c.is_opt("intersection", poly.intersection(&l));
    c.is_bool("on_boundary", poly.on_boundary(q, dist));
    c.is_point(
        "projection_onto(MultiPoint)",
        q.projection_onto_lines(poly.first_point(), &poly.lines()),
    );

    let pl = Polyline::new(pts.clone());
    let pl_len = pl.length();
    c.is_f64("Polyline::length", pl_len);
    let mut ce = pl.clone();
    ce.clip_end(dist);
    c.is_path("clip_end", &ce.points);
    let mut cs = pl.clone();
    cs.clip_start(dist);
    c.is_path("clip_start", &cs.points);
    let mut ee = pl.clone();
    ee.extend_end(dist);
    c.is_path("extend_end", &ee.points);
    let spacing = orca_stl::max(pl_len / (2.0 + 18.0 * frac), 1.0);
    c.is_path("equally_spaced_points", &pl.equally_spaced_points(spacing));
    c.is_polylines("equally_spaced_lines", &pl.equally_spaced_lines(spacing));
    let mut si = pl.clone();
    si.simplify(tol);
    c.is_path("Polyline::simplify", &si.points);
    let split = pl.split_at_length(pl_len * frac);
    c.is_bool("split_at_length", split.is_some());
    let (p1, p2) = split.map_or((Points::new(), Points::new()), |(p1, p2)| {
        (p1.points, p2.points)
    });
    c.is_path("split_at_length p1", &p1);
    c.is_path("split_at_length p2", &p2);
    c.is_point("leftmost_point", pl.leftmost_point());
    c.is_bool("is_straight", pl.is_straight());
    let (index, foot) = polyline::foot_pt(&pts, q);
    c.is_i32("foot_pt index", index);
    c.is_point("foot_pt", foot);
    c.is_bool("has_duplicate_points", pt::has_duplicate_points(&pts));

    c.is_path("convex_hull", &convex_hull(pts.clone()).points);
    c.is_polygons(
        "polygons_simplify",
        &polygon::polygons_simplify(std::slice::from_ref(&poly), tol, true),
    );
    c.is_bbox(
        "get_extents_rotated",
        &multi_point::get_extents_rotated(&pts, angle),
    );
    let mut ro = poly.clone();
    ro.rotate_around(angle, q);
    c.is_path("rotate(center)", &ro.points);
    let mut rr = poly.clone();
    rr.rotate(angle);
    c.is_path("rotate", &rr.points);
    let mut sc = poly.clone();
    sc.scale(factor);
    c.is_path("scale", &sc.points);
    c.is_path("_douglas_peucker", &multi_point::douglas_peucker(&pts, tol));
    c.is_path(
        "Polygon::equally_spaced_points",
        &poly.equally_spaced_points(spacing),
    );
}

#[test]
fn polygons_polylines_and_boxes_match() {
    replay("GPLY", gply);
}

/// A whole GCLU record.
fn gclu(c: &mut Case) {
    let (a, b) = (c.polygons(), c.polygons());
    let (delta, jt, ml, safety) = (c.f32(), join_type(c.u8()), c.f64(), c.bool());
    let pl = Polyline::new(c.path());
    let q = c.point();
    let so = if safety {
        ApplySafetyOffset::Yes
    } else {
        ApplySafetyOffset::No
    };

    c.is_polygons("offset", &cu::offset(&a, delta, jt, ml));
    c.is_expolygons("offset_ex", &cu::offset_ex(&a, delta, jt, ml));
    c.is_polygons("union_", &cu::union_(&a));
    let ea = cu::union_ex(&a, PolyFillType::NonZero);
    let eb = cu::union_ex(&b, PolyFillType::NonZero);
    let want_ea = c.expolygons();
    c.same("union_ex", &ea, &want_ea);
    c.is_polygons("diff", &cu::diff(&a, &b, so));
    c.is_expolygons("diff_ex", &cu::diff_ex(&a, &b, so));
    c.is_polygons("intersection", &cu::intersection(&a, &b, so));
    c.is_expolygons("intersection_ex", &cu::intersection_ex(&a, &b, so));
    c.is_expolygons("xor_ex", &cu::xor_ex(&ea, &eb, so));
    c.is_expolygons("diff_ex(ExPolygons)", &cu::diff_ex_expolygons(&ea, &eb, so));
    c.is_expolygons(
        "offset_ex(ExPolygons)",
        &cu::offset_ex_expolygons(&ea, delta, jt, ml),
    );
    c.is_polygons(
        "offset(ExPolygons)",
        &cu::offset_expolygons(&ea, delta, jt, ml),
    );
    c.is_expolygons(
        "offset2_ex",
        &cu::offset2_ex(&ea, delta, -0.5f32 * delta, jt, ml),
    );
    let pos = orca_crt::fabsf(delta) + 1.0f32;
    c.is_polygons("closing", &cu::closing(&a, pos, pos, jt, ml));
    c.is_polygons("opening", &cu::opening(&a, pos, pos, jt, ml));
    let pls = std::slice::from_ref(&pl);
    c.is_polylines("diff_pl", &cu::diff_pl(pls, &a));
    c.is_polylines("intersection_pl", &cu::intersection_pl(pls, &a));
    c.is_polylines("diff_pl(Polygons)", &cu::diff_pl_polygons(&a, &b));
    c.is_polygons(
        "offset(Polyline)",
        &cu::offset_polyline(
            &pl,
            pos,
            cu::DEFAULT_LINE_JOIN_TYPE,
            cu::DEFAULT_LINE_MITER_LIMIT,
            cu::DEFAULT_END_TYPE,
        ),
    );
    c.is_polygons("simplify_polygons", &cu::simplify_polygons(&a));
    c.is_polygons("union_safety_offset", &cu::union_safety_offset(&a));
    c.is_polygons(
        "contour_to_polygons",
        &cu::contour_to_polygons_of(&a, 2.0f32 * pos, jt, ml),
    );
    c.is_polygons("diff_clipped", &cu::diff_clipped(&a, &b, so));
    c.is_polygons(
        "intersection_clipped",
        &cu::intersection_clipped(&a, &b, so),
    );
    let ab: Polygons = a.iter().chain(&b).cloned().collect();
    c.is_polygons("top_level_islands", &cu::top_level_islands(&ab));
    c.is_polygons(
        "clip_clipper_polygons_with_subject_bbox",
        &cu::clip_clipper_polygons_with_subject_bbox(&b, &polygon::get_extents_polygons(&a)),
    );
    // The engine wrote the rest for its own non-empty `union_ex(A)`.
    let Some(want_e) = want_ea.first() else {
        return;
    };
    let Some(e) = ea.first() else {
        return;
    };
    c.is_f64("ExPolygon::area", e.area());
    c.is_bool("ExPolygon::contains", e.contains(q, true));
    c.is_bool("ExPolygon::is_valid", e.is_valid());
    c.is_point("ExPolygon::point_projection", e.point_projection(q));
    c.is_bool(
        "ExPolygon::overlaps",
        eb.first().is_some_and(|f| e.overlaps(f)),
    );
    let n = want_e.num_contours();
    let (outer, inner) = (c.deltas(n), c.deltas(n));
    c.is_polygons(
        "variable_offset_outer",
        &cu::variable_offset_outer(e, &outer, ml),
    );
    c.is_polygons(
        "variable_offset_inner",
        &cu::variable_offset_inner(e, &inner, ml),
    );
    c.is_expolygons(
        "variable_offset_outer_ex",
        &cu::variable_offset_outer_ex(e, &outer, ml),
    );
    c.is_expolygons(
        "variable_offset_inner_ex",
        &cu::variable_offset_inner_ex(e, &inner, ml),
    );
}

#[test]
fn clipper_utils_and_expolygons_match() {
    replay("GCLU", gclu);
}

/// A whole GXFM record.
fn gxfm(c: &mut Case) {
    let (t, r, s, m) = (c.vec3(), c.vec3(), c.vec3(), c.vec3());
    let pts = c.vec3s();
    let (sa, sb) = (c.path(), c.path());
    let (radius, error, angle) = (c.f64(), c.f64(), c.f64());

    let tr = geometry::assemble_transform(&t, &r, &s, &m);
    c.is_transform("assemble_transform", &tr);
    let rot = geometry::rotation_transform(&r);
    c.is_transform("rotation_transform", &rot);
    c.is_vec3(
        "extract_euler_angles(Transform3d)",
        geometry::extract_euler_angles_of(&tr),
    );
    c.is_vec3(
        "extract_euler_angles(Matrix3d)",
        geometry::extract_euler_angles(&rot.linear()),
    );
    let bt = BoundingBoxf3::from_points(&pts).transformed(&tr);
    c.is_vec3("BoundingBoxf3::transformed min", bt.min);
    c.is_vec3("BoundingBoxf3::transformed max", bt.max);
    c.is_vec3s("convex_hull(Pointf3s)", &convex_hull_3d(pts.clone()));
    c.is_path("make_circle", &polygon::make_circle(radius, error).points);
    c.is_f64("rad2deg_dir", geometry::rad2deg_dir(angle));
    let (ha, hb) = (convex_hull(sa.clone()), convex_hull(sb.clone()));
    c.is_path("convex_hull a", &ha.points);
    c.is_path("convex_hull b", &hb.points);
    let both = ha.size() >= 3 && hb.size() >= 3;
    c.is_bool(
        "convex_polygons_intersect",
        both && convex_polygons_intersect(&ha, &hb),
    );
    let un: Vec<Vec2d> = ha.points.iter().copied().map(unscaled).collect();
    let tb = decompose_convex_polygon_top_bottom(&un);
    c.is_vec2s("decompose_convex_polygon_top_bottom bottom", &tb.0);
    c.is_vec2s("decompose_convex_polygon_top_bottom top", &tb.1);
    for &q in &sb {
        c.is_bool(
            &format!("inside_convex_polygon({q:?})"),
            !tb.0.is_empty() && inside_convex_polygon(&tb, unscaled(q)),
        );
    }
}

#[test]
fn transforms_hulls_and_circles_match() {
    replay("GXFM", gxfm);
}

/// Every kind of the file has a replay here or in orca-eigen, and every kind here is recorded.
#[test]
fn every_kind_is_replayed() {
    assert_eq!(orca_geom::NAME, "orca-geom");
    let file = file();
    let unknown: Vec<&str> = file
        .records
        .iter()
        .map(|r| r.kind())
        .filter(|k| !EIGEN_KINDS.contains(k) && !GEOM_KINDS.contains(k))
        .collect();
    assert!(
        unknown.is_empty(),
        "{}: {} records nothing replays ({:?}...)",
        file.path.display(),
        unknown.len(),
        &unknown[..unknown.len().min(4)]
    );
    for kind in GEOM_KINDS {
        assert!(
            file.of_kind(kind).next().is_some(),
            "{}: no {kind} record",
            file.path.display()
        );
    }
}
