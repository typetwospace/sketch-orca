// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Geometry/ConvexHull.cpp and ConvexHull.hpp of
// OrcaSlicer v2.4.2, translated on 2026-11-07. Copyright the OrcaSlicer authors, the BambuStudio
// authors (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU Affero General
// Public License, version 3 (AGPL-3.0; LICENSE).

//! `Geometry/ConvexHull`: Andrew's monotone chain hulls, the rotating-calipers test of two convex
//! polygons, and the top / bottom chain decomposition with its point test.
//!
//! The C++ compares angles in `cmp_angles` with Boost.Multiprecision's `int256_t`; its products
//! are below 2^189, so a sign and a 192-bit magnitude hold them exactly here. Its dot products
//! themselves are `int64_t` (the template's default scalar) and wrap as MSVC's do.

use orca_eigen::{Vec2d, Vec3d};

use crate::expolygon::ExPolygon;
use crate::geometry::{Orientation, orient};
use crate::point::{Point, Pointf3s, Points, cross2};
use crate::polygon::Polygon;
use crate::polyline::Polyline;

/// `convex_hull(Points)`: sorted by x then y, duplicates removed, lower then upper chain; empty
/// below three distinct points.
#[must_use]
pub fn convex_hull(mut pts: Points) -> Polygon {
    orca_stl::sort_by(&mut pts, |a, b| a.x < b.x || (a.x == b.x && a.y < b.y));
    let last = orca_stl::unique_by(&mut pts, |a, b| a.x == b.x && a.y == b.y);
    pts.truncate(last);
    let mut hull = Polygon::default();
    let n = pts.len();
    if n >= 3 {
        let mut k = 0_usize;
        hull.points.resize(2 * n, Point::default());
        // The lower chain.
        for &p in &pts {
            while k >= 2 && orient(p, hull.points[k - 2], hull.points[k - 1]) != Orientation::Ccw {
                k -= 1;
            }
            hull.points[k] = p;
            k += 1;
        }
        // The upper chain.
        let t = k + 1;
        for i in (0..n - 1).rev() {
            while k >= t
                && orient(pts[i], hull.points[k - 2], hull.points[k - 1]) != Orientation::Ccw
            {
                k -= 1;
            }
            hull.points[k] = pts[i];
            k += 1;
        }
        hull.points.truncate(k);
        hull.points.pop();
    }
    hull
}

/// `convex_hull(Pointf3s)`: the hull in the xy plane, the orientation tests on `Point::new_scale`d
/// copies; empty below three points.
#[must_use]
pub fn convex_hull_3d(mut points: Pointf3s) -> Pointf3s {
    orca_stl::sort_by(&mut points, |a: &Vec3d, b: &Vec3d| {
        a.x() < b.x() || (a.x() == b.x() && a.y() < b.y())
    });
    let n = points.len();
    let mut hull: Pointf3s = Vec::new();
    let scaled = |v: &Vec3d| Point::new_scale(v.x(), v.y());
    if n >= 3 {
        let mut k = 0_usize;
        hull.resize(2 * n, orca_eigen::Vector([0.0; 3]));
        for &pt in &points {
            let p = scaled(&pt);
            while k >= 2 {
                let k1 = scaled(&hull[k - 1]);
                let k2 = scaled(&hull[k - 2]);
                if orient(p, k2, k1) == Orientation::Ccw {
                    break;
                }
                k -= 1;
            }
            hull[k] = pt;
            k += 1;
        }
        let t = k + 1;
        for i in (0..n - 1).rev() {
            let p = scaled(&points[i]);
            while k >= t {
                let k1 = scaled(&hull[k - 1]);
                let k2 = scaled(&hull[k - 2]);
                if orient(p, k2, k1) == Orientation::Ccw {
                    break;
                }
                k -= 1;
            }
            hull[k] = points[i];
            k += 1;
        }
        hull.truncate(k);
        hull.pop();
    }
    hull
}

/// `convex_hull(Polygons)`: of all their points.
#[must_use]
pub fn convex_hull_polygons(polygons: &[Polygon]) -> Polygon {
    let mut pp = Points::new();
    for p in polygons {
        pp.extend_from_slice(&p.points);
    }
    convex_hull(pp)
}

/// `convex_hull(ExPolygons)`: of all their contour points.
#[must_use]
pub fn convex_hull_expolygons(expolygons: &[ExPolygon]) -> Polygon {
    let mut pp = Points::new();
    for e in expolygons {
        pp.extend_from_slice(&e.contour.points);
    }
    convex_hull(pp)
}

/// `convex_hulll(Polylines)` (sic): of all their points.
#[must_use]
pub fn convex_hulll(polylines: &[Polyline]) -> Polygon {
    let mut pp = Points::new();
    for p in polylines {
        pp.extend_from_slice(&p.points);
    }
    convex_hull(pp)
}

/// `rotcalip::dot` with its default `int64_t` scalar.
fn dot(a: Point, b: Point) -> i64 {
    a.x.wrapping_mul(b.x).wrapping_add(a.y.wrapping_mul(b.y))
}

/// `rotcalip::dotperp` with its default `int64_t` scalar.
fn dotperp(a: Point, b: Point) -> i64 {
    a.x.wrapping_mul(b.y).wrapping_sub(a.y.wrapping_mul(b.x))
}

/// `m * |a| * a` exactly: its sign and its magnitude's three 64-bit limbs, least first.
fn triple_product(m: i64, a: i64) -> (i32, [u64; 3]) {
    let sign = (m.signum() * a.signum()) as i32;
    let ma = u128::from(m.unsigned_abs()) * u128::from(a.unsigned_abs());
    let b = u128::from(a.unsigned_abs());
    let p0 = (ma & u128::from(u64::MAX)) * b;
    let p1 = (ma >> 64) * b;
    let l0 = p0 as u64;
    let carry = (p0 >> 64) + (p1 & u128::from(u64::MAX));
    let l1 = carry as u64;
    let l2 = ((carry >> 64) + (p1 >> 64)) as u64;
    (sign, [l0, l1, l2])
}

/// The ordering of two magnitudes.
fn cmp_magnitude(a: &[u64; 3], b: &[u64; 3]) -> core::cmp::Ordering {
    a[2].cmp(&b[2]).then(a[1].cmp(&b[1])).then(a[0].cmp(&b[0]))
}

/// `rotcalip::cmp_angles(dir, dirA, dirB)`: -1 when the angle from `dir` to `dirA` is the smaller,
/// 1 when it is the larger, 0 when equal.
fn cmp_angles(dir: Point, dir_a: Point, dir_b: Point) -> i32 {
    let dot_a = dot(dir, dir_a);
    let dot_b = dot(-dir, dir_b);
    let (sa, ma) = triple_product(dot(dir_b, dir_b), dot_a);
    let (sb, mb) = triple_product(dot(dir_a, dir_a), dot_b);
    // The sign of dcosa - dcosb.
    let diff = if sa != sb {
        (sa - sb).signum()
    } else if sa == 0 {
        0
    } else {
        let ord = cmp_magnitude(&ma, &mb) as i32;
        if sa > 0 { ord } else { -ord }
    };
    -diff
}

/// `rotcalip::Idx`: a vertex index into a polygon.
#[derive(Clone, Copy)]
struct Idx<'a> {
    idx: usize,
    poly: &'a Polygon,
}

impl<'a> Idx<'a> {
    fn new(idx: usize, poly: &'a Polygon) -> Self {
        Self { idx, poly }
    }

    fn size(&self) -> usize {
        self.poly.points.len()
    }

    fn at(&self, i: usize) -> Point {
        self.poly.points[i]
    }

    fn next(&self) -> usize {
        (self.idx + 1) % self.size()
    }

    fn inc(&mut self) {
        self.idx = (self.idx + 1) % self.size();
    }

    fn pt(&self) -> Point {
        self.at(self.idx)
    }

    fn prev_dir(&self) -> Point {
        self.pt() - self.at((self.idx + self.size() - 1) % self.size())
    }

    fn dir(&self) -> Point {
        self.at(self.next()) - self.pt()
    }

    fn next_dir(&self) -> Point {
        self.at((self.idx + 2) % self.size()) - self.at(self.next())
    }
}

/// `rotcalip::visit_antipodals<AntipodalVisitMode::EdgesOnly>(ia, ib, fn)`.
fn visit_antipodals_edges_only<'a>(
    ia: &mut Idx<'a>,
    ib: &mut Idx<'a>,
    mut f: impl FnMut(usize, usize, Point) -> bool,
) {
    let mut cmp = cmp_angles(ia.prev_dir(), ia.dir(), ib.dir());
    let mut current_is_a = cmp <= 0;
    let initial_is_a = current_is_a;
    let start = if initial_is_a { ia.idx } else { ib.idx };
    let mut visitor_continue = true;
    let mut finished = false;
    while visitor_continue && !finished {
        let current_dir_a = if current_is_a { ia.dir() } else { -ib.dir() };
        visitor_continue = f(ia.idx, ib.idx, current_dir_a);
        let (current, other) = if current_is_a {
            (&mut *ia, &*ib)
        } else {
            (&mut *ib, &*ia)
        };
        cmp = cmp_angles(current.dir(), current.next_dir(), other.dir());
        current.inc();
        if cmp > 0 {
            current_is_a = !current_is_a;
        }
        let initial_idx = if initial_is_a { ia.idx } else { ib.idx };
        if initial_idx == start {
            finished = true;
        }
    }
}

/// The extreme vertices of a polygon: x by `Point`'s `<` (x, then y), y by y then x.
struct Extremes {
    xmin: usize,
    xmax: usize,
    ymin: usize,
    ymax: usize,
}

impl Extremes {
    fn of(p: &Polygon) -> Self {
        let cmpy = |l: Point, u: Point| l.y < u.y || (l.y == u.y && l.x < u.x);
        let pts = &p.points;
        let mut e = Self {
            xmin: 0,
            xmax: 0,
            ymin: 0,
            ymax: 0,
        };
        for i in 0..pts.len() {
            if pts[i] < pts[e.xmin] {
                e.xmin = i;
            }
            if pts[e.xmax] < pts[i] {
                e.xmax = i;
            }
            if cmpy(pts[i], pts[e.ymin]) {
                e.ymin = i;
            }
            if cmpy(pts[e.ymax], pts[i]) {
                e.ymax = i;
            }
        }
        e
    }
}

/// `convex_polygons_intersect(A, B)`: true when the convex polygons overlap or touch, found by
/// rotating calipers looking for a separating edge.
#[must_use]
pub fn convex_polygons_intersect(a: &Polygon, b: &Polygon) -> bool {
    let ba = Extremes::of(a);
    let bb = Extremes::of(b);
    let mut imin = Idx::new(ba.xmin, a);
    let mut imax = Idx::new(bb.xmax, b);
    if b.points[bb.xmin] < imin.pt() {
        imin = Idx::new(bb.xmin, b);
    }
    if imax.pt() < a.points[ba.xmax] {
        imax = Idx::new(ba.xmax, a);
    }
    if core::ptr::eq(imin.poly, imax.poly) {
        imin = Idx::new(ba.ymin, a);
        imax = Idx::new(bb.ymax, b);
        if b.points[bb.ymin] < imin.pt() {
            imin = Idx::new(bb.ymin, b);
        }
        if imax.pt() < a.points[ba.ymax] {
            imax = Idx::new(ba.ymax, a);
        }
    }
    if core::ptr::eq(imin.poly, imax.poly) {
        return true;
    }
    let pa = imin.poly;
    let pb = imax.poly;
    let mut found_divisor = false;
    visit_antipodals_edges_only(&mut imin, &mut imax, |ia, ib, dir| {
        let ref_a = pa.points[(ia + 2) % pa.points.len()];
        let ref_b = pb.points[(ib + 2) % pb.points.len()];
        let is_left_a = dotperp(dir, ref_a - pa.points[ia]) > 0;
        let is_left_b = dotperp(-dir, ref_b - pb.points[ib]) > 0;
        let d = dotperp(dir, pb.points[ib] - pa.points[ia]);
        found_divisor = if d == 0 {
            is_left_a == is_left_b
        } else if d > 0 {
            // B is to the left of the edge.
            !is_left_a && !is_left_b
        } else {
            is_left_a && is_left_b
        };
        !found_divisor
    });
    !found_divisor
}

/// The lexicographic `<` of `Vec2d` (x, then y).
fn less_xy(l: &Vec2d, r: &Vec2d) -> bool {
    l.x() < r.x() || (l.x() == r.x() && l.y() < r.y())
}

/// Drops the trailing points that share the last point's x but the first of them.
fn trim_equal_x_tail(chain: &mut Vec<Vec2d>) {
    if chain.len() > 1 {
        let back_x = chain[chain.len() - 1].x();
        let mut it = chain.len() - 1;
        while it != 0 && chain[it - 1].x() == back_x {
            it -= 1;
        }
        chain.truncate(it + 1);
    }
}

/// `decompose_convex_polygon_top_bottom(src)`: the bottom chain from the leftmost-lowest to the
/// rightmost-highest vertex and the top chain, both left to right; both empty when either has
/// fewer than two points. The polygon must be convex and counter-clockwise.
#[must_use]
pub fn decompose_convex_polygon_top_bottom(src: &[Vec2d]) -> (Vec<Vec2d>, Vec<Vec2d>) {
    let mut bottom = Vec::new();
    let mut top = Vec::new();
    if !src.is_empty() {
        let left_bottom = orca_stl::min_element_by(src, less_xy);
        let right_top = orca_stl::max_element_by(src, less_xy);
        if left_bottom != right_top {
            if left_bottom < right_top {
                bottom.extend_from_slice(&src[left_bottom..=right_top]);
                top.extend_from_slice(&src[right_top..]);
                top.extend_from_slice(&src[..=left_bottom]);
            } else {
                bottom.extend_from_slice(&src[left_bottom..]);
                bottom.extend_from_slice(&src[..=right_top]);
                top.extend_from_slice(&src[right_top..=left_bottom]);
            }
            trim_equal_x_tail(&mut bottom);
            trim_equal_x_tail(&mut top);
            top.reverse();
        }
    }
    if top.len() < 2 || bottom.len() < 2 {
        top.clear();
        bottom.clear();
    }
    (bottom, top)
}

/// `inside_convex_polygon(top_bottom_decomposition, pt)`: in O(log n), the boundary included.
#[must_use]
pub fn inside_convex_polygon(top_bottom: &(Vec<Vec2d>, Vec<Vec2d>), pt: Vec2d) -> bool {
    let (bottom, top) = top_bottom;
    let by_x = |l: &Vec2d, r: &Vec2d| l.x() < r.x();
    let it_bottom = orca_stl::lower_bound_by(bottom, &pt, by_x);
    let it_top = orca_stl::lower_bound_by(top, &pt, by_x);
    if it_bottom == bottom.len() {
        return false;
    }
    if it_bottom == 0 {
        if pt.x() < bottom[it_bottom].x() {
            return false;
        }
        return pt.y() >= bottom[it_bottom].y() && pt.y() <= top[it_top].y();
    }
    let det = cross2(
        bottom[it_bottom] - bottom[it_bottom - 1],
        pt - bottom[it_bottom - 1],
    );
    if det < 0.0 {
        return false;
    }
    let det = cross2(top[it_top] - top[it_top - 1], pt - top[it_top - 1]);
    det <= 0.0
}
