// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Clipper code: deps_src/clipper/clipper.cpp and clipper.hpp of OrcaSlicer v2.4.2
// (Clipper 6.4.2 with the PrusaSlicer authors' changes), translated on 2026-11-07. Copyright Angus
// Johnson 2010-2017 and the PrusaSlicer authors, under the Boost Software License, version 1.0
// (BSL-1.0; licenses/BSL-1.0.txt).

//! Clipper 6.4.2 as OrcaSlicer vendors it and libslic3r compiles it (`src/libslic3r/clipper.cpp`:
//! namespace `Slic3r::ClipperLib`, `IntPoint` = `Slic3r::Point`, `use_lines` on, 64-bit
//! coordinates with `double` cross products).
//!
//! - [`Clipper`] (`engine`): the Vatti clipper, with every clip and fill type, open paths, the
//!   `Paths` and `PolyTree` results, `StrictlySimple`, `PreserveCollinear` and `ReverseSolution`.
//! - [`ClipperOffset`] (`offset`): the offsetter, with every join and end type.
//! - [`PolyTree`] (`polytree`): the result tree.
//! - The helpers of `clipper.cpp`: [`area`], [`orientation`], [`point_in_polygon`],
//!   [`simplify_polygon`], [`simplify_polygons`], [`clean_polygon`], [`clean_polygons`], the
//!   Minkowski sums and difference, the `PolyTree` to `Paths` conversions and the reversals.
//!
//! Where the C++ throws `clipperException` to its caller (a coordinate beyond `hiRange` or an open
//! clip path in `AddPath`, an open path in `Execute` to `Paths`), the translation panics with
//! [`ClipperException`] as the payload ([`ClipperException::throw`]), so that a caller that
//! catches it in C++ can catch it here with `catch_unwind`. The exceptions `ExecuteInternal`
//! catches itself only make `execute` return false, as in C++.
//!
//! ClipperLib_Z, libslic3r's second build of the same source with `CLIPPERLIB_USE_XYZ`, differs in
//! a `z` on each point and a callback that fills it at intersections; the sites where it would do
//! so are marked "ClipperLib_Z" in `engine.rs`, so that it can be added as a point type of its
//! own. Clipper2 is not part of this module.

mod engine;
mod offset;
mod polytree;

pub use engine::Clipper;
pub use offset::ClipperOffset;
pub use polytree::{NodeId, PolyNode, PolyTree, ROOT};

use crate::point::Point;

/// `IntPoint`: libslic3r's `Point` (`CLIPPERLIB_INTPOINT_TYPE Slic3r::Point`).
pub type IntPoint = Point;
/// `cInt`: a coordinate, `int64_t` (`CLIPPERLIB_INT32` is not defined).
pub type CInt = i64;
/// `Path`: `std::vector<IntPoint>`.
pub type Path = Vec<IntPoint>;
/// `Paths`: `std::vector<Path>`.
pub type Paths = Vec<Path>;

/// `loRange`: the largest coordinate whose cross products fit 64-bit arithmetic.
pub const LO_RANGE: CInt = 0x3FFF_FFFF;
/// `hiRange`: the largest coordinate `AddPath` accepts.
pub const HI_RANGE: CInt = 0x3FFF_FFFF_FFFF_FFFF;

/// `ioReverseSolution`, an `initOptions` flag of [`Clipper::new`].
pub const IO_REVERSE_SOLUTION: i32 = 1;
/// `ioStrictlySimple`, an `initOptions` flag of [`Clipper::new`].
pub const IO_STRICTLY_SIMPLE: i32 = 2;
/// `ioPreserveCollinear`, an `initOptions` flag of [`Clipper::new`].
pub const IO_PRESERVE_COLLINEAR: i32 = 4;

/// `ClipType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipType {
    /// `ctIntersection`.
    Intersection,
    /// `ctUnion`.
    Union,
    /// `ctDifference`.
    Difference,
    /// `ctXor`.
    Xor,
}

/// `PolyType`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PolyType {
    /// `ptSubject` (0, what `InitEdge`'s `memset` leaves).
    #[default]
    Subject,
    /// `ptClip`.
    Clip,
}

/// `PolyFillType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolyFillType {
    /// `pftEvenOdd`.
    EvenOdd,
    /// `pftNonZero`.
    NonZero,
    /// `pftPositive`.
    Positive,
    /// `pftNegative`.
    Negative,
}

/// `JoinType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinType {
    /// `jtSquare`.
    Square,
    /// `jtRound`.
    Round,
    /// `jtMiter`.
    Miter,
}

/// `EndType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndType {
    /// `etClosedPolygon`.
    ClosedPolygon,
    /// `etClosedLine`.
    ClosedLine,
    /// `etOpenButt`.
    OpenButt,
    /// `etOpenSquare`.
    OpenSquare,
    /// `etOpenRound`.
    OpenRound,
}

/// `IntRect`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IntRect {
    /// `left`.
    pub left: CInt,
    /// `top`.
    pub top: CInt,
    /// `right`.
    pub right: CInt,
    /// `bottom`.
    pub bottom: CInt,
}

/// `clipperException`: what Clipper throws, with its `what()` text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClipperException(&'static str);

impl ClipperException {
    /// `RangeTest`'s.
    pub const COORDINATE_OUTSIDE_ALLOWED_RANGE: Self = Self("Coordinate outside allowed range");
    /// `AddPathInternal`'s, for an open clip path.
    pub const OPEN_PATHS_MUST_BE_SUBJECT: Self = Self("AddPath: Open paths must be subject.");
    /// `Execute(ClipType, Paths&, ...)`'s, after an open path was added.
    pub const POLYTREE_NEEDED: Self =
        Self("Error: PolyTree struct is needed for open path clipping.");
    /// `UpdateEdgeIntoAEL`'s, caught by `ExecuteInternal`.
    pub const UPDATE_EDGE_INTO_AEL: Self = Self("UpdateEdgeIntoAEL: invalid call");
    /// `DoMaxima`'s, caught by `ExecuteInternal`.
    pub const DO_MAXIMA: Self = Self("DoMaxima error");

    /// `what()`.
    #[must_use]
    pub const fn what(&self) -> &'static str {
        self.0
    }

    /// Throws the exception to the caller: a panic whose payload is the exception.
    pub fn throw(self) -> ! {
        std::panic::panic_any(self)
    }
}

impl core::fmt::Display for ClipperException {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for ClipperException {}

/// `pi`, written `3.141592653589793238` in the C++, which rounds to the same `double`.
pub(crate) const PI: f64 = core::f64::consts::PI;
/// `two_pi`: `pi * 2`.
pub(crate) const TWO_PI: f64 = PI * 2.0;
/// `def_arc_tolerance`.
pub(crate) const DEF_ARC_TOLERANCE: f64 = 0.25;

/// `FRound`: `floor(a + 0.5)`, except that the `double` just below 0.5 rounds to 0.
pub(crate) fn fround(a: f64) -> f64 {
    if a == 0.499_999_999_999_999_94 {
        0.0
    } else {
        orca_crt::floor(a + 0.5)
    }
}

/// `Round<cInt>` and `Round<int64_t>`: [`fround`], then `static_cast<int64_t>` (MSVC's
/// truncating conversion, through `orca-crt`).
pub(crate) fn round_cint(v: f64) -> CInt {
    orca_crt::f64_to_i64(fround(v))
}

/// `Area(const Path&)`: the signed area, positive for a counter-clockwise path (y up); 0 below
/// three points.
#[must_use]
pub fn area(poly: &[IntPoint]) -> f64 {
    let size = poly.len();
    if size < 3 {
        return 0.0;
    }
    let mut a = 0.0;
    let mut j = size - 1;
    for i in 0..size {
        a += (poly[j].x as f64 + poly[i].x as f64) * (poly[j].y as f64 - poly[i].y as f64);
        j = i;
    }
    -a * 0.5
}

/// `Orientation`: `Area(poly) >= 0`.
#[must_use]
pub fn orientation(poly: &[IntPoint]) -> bool {
    area(poly) >= 0.0
}

/// `PointInPolygon(const IntPoint&, const Path&)`: 1 inside, 0 outside, -1 on the boundary
/// (Hormann and Agathos).
#[must_use]
pub fn point_in_polygon(pt: IntPoint, path: &[IntPoint]) -> i32 {
    let mut result = 0;
    let cnt = path.len();
    if cnt < 3 {
        return 0;
    }
    let mut ip = path[0];
    for i in 1..=cnt {
        let ip_next = if i == cnt { path[0] } else { path[i] };
        if ip_next.y == pt.y
            && (ip_next.x == pt.x || (ip.y == pt.y && ((ip_next.x > pt.x) == (ip.x < pt.x))))
        {
            return -1;
        }
        if (ip.y < pt.y) != (ip_next.y < pt.y) {
            if ip.x >= pt.x {
                if ip_next.x > pt.x {
                    result = 1 - result;
                } else {
                    let d = cross_from(pt, ip, ip_next);
                    if d == 0.0 {
                        return -1;
                    }
                    if (d > 0.0) == (ip_next.y > ip.y) {
                        result = 1 - result;
                    }
                }
            } else if ip_next.x > pt.x {
                let d = cross_from(pt, ip, ip_next);
                if d == 0.0 {
                    return -1;
                }
                if (d > 0.0) == (ip_next.y > ip.y) {
                    result = 1 - result;
                }
            }
        }
        ip = ip_next;
    }
    result
}

/// `PointInPolygon`'s cross product of `a - pt` and `b - pt`, in `CrossProductType` (`double`),
/// the differences taken in 64-bit integers first.
pub(crate) fn cross_from(pt: IntPoint, a: IntPoint, b: IntPoint) -> f64 {
    a.x.wrapping_sub(pt.x) as f64 * b.y.wrapping_sub(pt.y) as f64
        - b.x.wrapping_sub(pt.x) as f64 * a.y.wrapping_sub(pt.y) as f64
}

/// `SimplifyPolygon`: the union of one path with itself, `StrictlySimple` by default.
#[must_use]
pub fn simplify_polygon(
    in_poly: &[IntPoint],
    fill_type: PolyFillType,
    strictly_simple: bool,
) -> Paths {
    let mut c = Clipper::new(0);
    c.set_strictly_simple(strictly_simple);
    c.add_path(in_poly, PolyType::Subject, true);
    let mut out = Paths::new();
    c.execute(ClipType::Union, &mut out, fill_type, fill_type);
    out
}

/// `SimplifyPolygons`: the union of the paths, `StrictlySimple` by default.
pub fn simplify_polygons<'a>(
    in_polys: impl IntoIterator<Item = &'a [IntPoint]>,
    fill_type: PolyFillType,
    strictly_simple: bool,
) -> Paths {
    let mut c = Clipper::new(0);
    c.set_strictly_simple(strictly_simple);
    c.add_paths(in_polys, PolyType::Subject, true);
    let mut out = Paths::new();
    c.execute(ClipType::Union, &mut out, fill_type, fill_type);
    out
}

/// `CleanPolygon`'s default `distance`.
pub const CLEAN_POLYGON_DISTANCE: f64 = 1.415;

/// `DistanceFromLineSqrd`: the squared distance of `pt` from the line through `ln1` and `ln2`.
fn distance_from_line_sqrd(pt: IntPoint, ln1: IntPoint, ln2: IntPoint) -> f64 {
    let a = ln1.y.wrapping_sub(ln2.y) as f64;
    let b = ln2.x.wrapping_sub(ln1.x) as f64;
    let mut c = a * ln1.x as f64 + b * ln1.y as f64;
    c = a * pt.x as f64 + b * pt.y as f64 - c;
    (c * c) / (a * a + b * b)
}

/// `SlopesNearCollinear`: tests the point geometrically between the other two.
fn slopes_near_collinear(pt1: IntPoint, pt2: IntPoint, pt3: IntPoint, dist_sqrd: f64) -> bool {
    if pt1.x.wrapping_sub(pt2.x).wrapping_abs() > pt1.y.wrapping_sub(pt2.y).wrapping_abs() {
        if (pt1.x > pt2.x) == (pt1.x < pt3.x) {
            distance_from_line_sqrd(pt1, pt2, pt3) < dist_sqrd
        } else if (pt2.x > pt1.x) == (pt2.x < pt3.x) {
            distance_from_line_sqrd(pt2, pt1, pt3) < dist_sqrd
        } else {
            distance_from_line_sqrd(pt3, pt1, pt2) < dist_sqrd
        }
    } else if (pt1.y > pt2.y) == (pt1.y < pt3.y) {
        distance_from_line_sqrd(pt1, pt2, pt3) < dist_sqrd
    } else if (pt2.y > pt1.y) == (pt2.y < pt3.y) {
        distance_from_line_sqrd(pt2, pt1, pt3) < dist_sqrd
    } else {
        distance_from_line_sqrd(pt3, pt1, pt2) < dist_sqrd
    }
}

/// `PointsAreClose`.
fn points_are_close(pt1: IntPoint, pt2: IntPoint, dist_sqrd: f64) -> bool {
    let dx = pt1.x.wrapping_sub(pt2.x) as f64;
    let dy = pt1.y.wrapping_sub(pt2.y) as f64;
    (dx * dx) + (dy * dy) <= dist_sqrd
}

/// `CleanPolygon(in_poly, out_poly, distance)`: removes the vertices closer than `distance` to
/// their neighbours or to the line through them; fewer than three left gives an empty path.
#[must_use]
pub fn clean_polygon(in_poly: &[IntPoint], distance: f64) -> Path {
    let mut size = in_poly.len();
    if size == 0 {
        return Path::new();
    }
    // The C++ links a vector of OutPt into a loop; `next`, `prev` and `idx` are its fields.
    let mut next: Vec<usize> = (0..size).map(|i| (i + 1) % size).collect();
    let mut prev: Vec<usize> = (0..size).map(|i| (i + size - 1) % size).collect();
    let mut idx = vec![0_u8; size];
    let dist_sqrd = distance * distance;
    let mut op = 0;
    while idx[op] == 0 && next[op] != prev[op] {
        if points_are_close(in_poly[op], in_poly[prev[op]], dist_sqrd) {
            op = exclude_op(op, &mut next, &mut prev, &mut idx);
            size -= 1;
        } else if points_are_close(in_poly[prev[op]], in_poly[next[op]], dist_sqrd) {
            exclude_op(next[op], &mut next, &mut prev, &mut idx);
            op = exclude_op(op, &mut next, &mut prev, &mut idx);
            size -= 2;
        } else if slopes_near_collinear(
            in_poly[prev[op]],
            in_poly[op],
            in_poly[next[op]],
            dist_sqrd,
        ) {
            op = exclude_op(op, &mut next, &mut prev, &mut idx);
            size -= 1;
        } else {
            idx[op] = 1;
            op = next[op];
        }
    }
    if size < 3 {
        size = 0;
    }
    let mut out = Path::with_capacity(size);
    for _ in 0..size {
        out.push(in_poly[op]);
        op = next[op];
    }
    out
}

/// `ExcludeOp`: unlinks `op` from the loop and returns its predecessor, marked for another look.
fn exclude_op(op: usize, next: &mut [usize], prev: &mut [usize], idx: &mut [u8]) -> usize {
    let result = prev[op];
    next[result] = next[op];
    prev[next[op]] = result;
    idx[result] = 0;
    result
}

/// `CleanPolygons(in_polys, out_polys, distance)`.
#[must_use]
pub fn clean_polygons(in_polys: &[Path], distance: f64) -> Paths {
    in_polys
        .iter()
        .map(|p| clean_polygon(p, distance))
        .collect()
}

/// `Minkowski(poly, path, solution, isSum, isClosed)`: the quads of the sum or difference.
fn minkowski(poly: &[IntPoint], path: &[IntPoint], is_sum: bool, is_closed: bool) -> Paths {
    let delta = usize::from(is_closed);
    let poly_cnt = poly.len();
    let path_cnt = path.len();
    let mut pp: Paths = Vec::with_capacity(path_cnt);
    for pi in path {
        let p: Path = if is_sum {
            poly.iter()
                .map(|q| Point::new(pi.x.wrapping_add(q.x), pi.y.wrapping_add(q.y)))
                .collect()
        } else {
            poly.iter()
                .map(|q| Point::new(pi.x.wrapping_sub(q.x), pi.y.wrapping_sub(q.y)))
                .collect()
        };
        pp.push(p);
    }
    let mut solution = Paths::new();
    // `pathCnt - 1 + delta` in `size_t`: an empty path wraps, and the C++ then divides by zero.
    let rows = path_cnt.wrapping_sub(1).wrapping_add(delta);
    for i in 0..rows {
        for j in 0..poly_cnt {
            let mut quad = vec![
                pp[i % path_cnt][j % poly_cnt],
                pp[(i + 1) % path_cnt][j % poly_cnt],
                pp[(i + 1) % path_cnt][(j + 1) % poly_cnt],
                pp[i % path_cnt][(j + 1) % poly_cnt],
            ];
            if !orientation(&quad) {
                quad.reverse();
            }
            solution.push(quad);
        }
    }
    solution
}

/// `MinkowskiSum(pattern, path, solution, pathIsClosed)`.
#[must_use]
pub fn minkowski_sum(pattern: &[IntPoint], path: &[IntPoint], path_is_closed: bool) -> Paths {
    let quads = minkowski(pattern, path, true, path_is_closed);
    let mut c = Clipper::new(0);
    c.add_paths(quads.iter().map(Vec::as_slice), PolyType::Subject, true);
    let mut solution = Paths::new();
    c.execute(
        ClipType::Union,
        &mut solution,
        PolyFillType::NonZero,
        PolyFillType::NonZero,
    );
    solution
}

/// `MinkowskiSum(pattern, paths, solution, pathIsClosed)`.
#[must_use]
pub fn minkowski_sum_paths(pattern: &[IntPoint], paths: &[Path], path_is_closed: bool) -> Paths {
    let mut c = Clipper::new(0);
    for path in paths {
        let tmp = minkowski(pattern, path, true, path_is_closed);
        c.add_paths(tmp.iter().map(Vec::as_slice), PolyType::Subject, true);
        if path_is_closed {
            // `TranslatePath(paths[i], tmp2, pattern[0])`.
            let d = pattern[0];
            let tmp2: Path = path
                .iter()
                .map(|p| Point::new(p.x.wrapping_add(d.x), p.y.wrapping_add(d.y)))
                .collect();
            c.add_path(&tmp2, PolyType::Clip, true);
        }
    }
    let mut solution = Paths::new();
    c.execute(
        ClipType::Union,
        &mut solution,
        PolyFillType::NonZero,
        PolyFillType::NonZero,
    );
    solution
}

/// `MinkowskiDiff(poly1, poly2, solution)`.
#[must_use]
pub fn minkowski_diff(poly1: &[IntPoint], poly2: &[IntPoint]) -> Paths {
    let quads = minkowski(poly1, poly2, false, true);
    let mut c = Clipper::new(0);
    c.add_paths(quads.iter().map(Vec::as_slice), PolyType::Subject, true);
    let mut solution = Paths::new();
    c.execute(
        ClipType::Union,
        &mut solution,
        PolyFillType::NonZero,
        PolyFillType::NonZero,
    );
    solution
}

/// `PolyTreeToPaths`: every non-empty contour, depth first, the tree's own first.
#[must_use]
pub fn poly_tree_to_paths(polytree: &PolyTree) -> Paths {
    let mut paths = Paths::with_capacity(polytree.total().max_or_zero());
    polytree.add_to_paths(ROOT, false, &mut paths);
    paths
}

/// `ClosedPathsFromPolyTree`: as [`poly_tree_to_paths`], without the open paths.
#[must_use]
pub fn closed_paths_from_poly_tree(polytree: &PolyTree) -> Paths {
    let mut paths = Paths::with_capacity(polytree.total().max_or_zero());
    polytree.add_to_paths(ROOT, true, &mut paths);
    paths
}

/// `OpenPathsFromPolyTree`: the open paths, which are all children of the tree.
#[must_use]
pub fn open_paths_from_poly_tree(polytree: &PolyTree) -> Paths {
    let mut paths = Paths::new();
    for &c in &polytree.root().childs {
        if polytree.is_open(c) {
            paths.push(polytree.node(c).contour.clone());
        }
    }
    paths
}

/// `ReversePath`.
pub fn reverse_path(p: &mut Path) {
    p.reverse();
}

/// `ReversePaths`.
pub fn reverse_paths(p: &mut Paths) {
    for path in p {
        reverse_path(path);
    }
}

/// A count from C++'s `int`, as a capacity.
trait MaxOrZero {
    fn max_or_zero(self) -> usize;
}

impl MaxOrZero for i32 {
    fn max_or_zero(self) -> usize {
        usize::try_from(self).unwrap_or(0)
    }
}
