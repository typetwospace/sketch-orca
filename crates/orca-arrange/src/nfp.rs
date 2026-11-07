// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Libnest2D code: libnest2d as OrcaSlicer v2.4.2 carries it in deps_src/libnest2d,
// translated on 2026-11-07 from include/libnest2d/geometry_traits_nfp.hpp (__nfp::_vsort,
// __nfp::buildPolygon, leftmostDownVertex, rightmostUpVertex, nfpInnerRectBed and nfpConvexOnly),
// with the NfpImpl of src/libslic3r/Arrange.cpp that instantiates it over
// boost::rational<LargeInt>. Copyright (C) Tamas Meszaros, modified by Prusa Research and the
// OrcaSlicer contributors, under the GNU Lesser General Public License, version 3
// (licenses/LGPL-3.0.txt).

//! No-fit polygons as `Arrange.cpp` computes them: `nfpConvexOnly` over
//! `boost::rational<LargeInt>`, where MSVC makes `LargeInt` Boost.Multiprecision's `int128_t`.
//!
//! - The edges of the stationary contour, then those of the orbiting contour reversed, each with
//!   its closing edge (the paths are open), are kept when their 64-bit squared length is
//!   positive, then sorted by angle with `std::sort` ([`orca_stl::sort_by`]): by quadrant
//!   (`{0, 3, 1, 2}` of the cosine's and sine's signs), within a quadrant by the signed squared
//!   cosine `lcos / lsq * sign * lcos` as a rational, compared with Boost's continued-fraction
//!   `operator<`.
//! - The polygon is built by chaining the sorted edges from the first one; its top vertex (by y,
//!   then x) is the reference point. Non-convex input gives what this construction gives.
//! - The rational operations may throw (`bad_rational`, a division by zero) only for 64-bit
//!   overflowed squared lengths; such an exception is an [`ExactError`] here.

use crate::geometry::{Box, Segment, bounding_box, dot_i64, dotperp_i64, magnsq_i64};
use orca_exact::{ExactError, Int128, Rational};
use orca_geom::expolygon::ExPolygon;
use orca_geom::point::{Point, Points};
use orca_geom::polygon::Polygon;
use std::cell::Cell;

/// `nfp::NfpResult`: the polygon and its reference point.
pub type NfpResult = (ExPolygon, Point);

/// `__nfp::_vsort`: by y, then x.
#[must_use]
pub fn vsort(v1: &Point, v2: &Point) -> bool {
    if v1.y == v2.y {
        v1.x < v2.x
    } else {
        v1.y < v2.y
    }
}

/// `nfp::leftmostDownVertex`: the first lowest contour point (`std::min_element`), or the origin
/// of an empty contour.
#[must_use]
pub fn leftmost_down_vertex(sh: &ExPolygon) -> Point {
    let pts = &sh.contour.points;
    if pts.is_empty() {
        return Point::new(0, 0);
    }
    pts[orca_stl::min_element_by(pts, vsort)]
}

/// `nfp::rightmostUpVertex`: the first highest contour point (`std::max_element`), or the origin
/// of an empty contour.
#[must_use]
pub fn rightmost_up_vertex(sh: &ExPolygon) -> Point {
    let pts = &sh.contour.points;
    if pts.is_empty() {
        return Point::new(0, 0);
    }
    pts[orca_stl::max_element_by(pts, vsort)]
}

/// `__nfp::buildPolygon`: the first edge's two points, then each next edge's end moved onto the
/// previous point; the top vertex is the highest point met (by y, then x).
fn build_polygon(edgelist: &[Segment]) -> (Points, Point) {
    let mut rsh: Points = Vec::with_capacity(2 * edgelist.len());
    rsh.push(edgelist[0].first);
    rsh.push(edgelist[0].second);
    let mut top_nfp = rsh[orca_stl::max_element_by(&rsh, vsort)];
    let mut tmp = 1usize;
    for e in &edgelist[1..] {
        let d = rsh[tmp] - e.first;
        let p = e.second + d;
        rsh.push(p);
        if vsort(&top_nfp, &p) {
            top_nfp = p;
        }
        tmp += 1;
    }
    (rsh, top_nfp)
}

/// The quadrant of an edge vector: `{0, 3, 1, 2}` of the signs of cosine and sine.
fn quadrant(lcos: i64, lsin: i64) -> i32 {
    const QUADRANTS: [i32; 4] = [0, 3, 1, 2];
    if lcos == 0 {
        if lsin > 0 { 1 } else { 3 }
    } else if lsin == 0 {
        if lcos > 0 { 0 } else { 2 }
    } else {
        QUADRANTS[(usize::from(lcos < 0) << 1) + usize::from(lsin < 0)]
    }
}

/// `Ratio(lcos) / lsq * sign * lcos` over `boost::rational<int128_t>`.
fn pcos(lcos: i64, lsq: i64, sign: i32) -> Result<Rational<Int128>, ExactError> {
    Rational::from_int(Int128::from_i64(lcos))
        .div_int(Int128::from_i64(lsq))?
        .mul_int(Int128::from(sign))?
        .mul_int(Int128::from_i64(lcos))
}

/// The edge comparator of `nfpConvexOnly` for a counter-clockwise path type.
fn edge_less(e1: &Segment, e2: &Segment) -> Result<bool, ExactError> {
    let ax = Point::new(1, 0);
    let p1 = e1.second - e1.first;
    let p2 = e2.second - e2.first;
    let lcos = [dot_i64(p1, ax), dot_i64(p2, ax)];
    let lsin = [
        dotperp_i64(p1, ax).wrapping_neg(),
        dotperp_i64(p2, ax).wrapping_neg(),
    ];
    let q = [quadrant(lcos[0], lsin[0]), quadrant(lcos[1], lsin[1])];
    if q[0] == q[1] {
        let lsq1 = magnsq_i64(p1);
        let lsq2 = magnsq_i64(p2);
        let sign = if q[0] == 1 || q[0] == 2 { -1 } else { 1 };
        let pcos1 = pcos(lcos[0], lsq1, sign)?;
        let pcos2 = pcos(lcos[1], lsq2, sign)?;
        return if q[0] < 2 {
            pcos1.gt(&pcos2)
        } else {
            pcos1.lt(&pcos2)
        };
    }
    Ok(q[0] < q[1])
}

/// `nfpConvexOnly<ExPolygon, boost::rational<LargeInt>>(sh, other)`.
///
/// # Errors
///
/// The exception a rational operation of the edge sort throws.
///
/// # Panics
///
/// For an empty contour or one with no edge of positive length, where the C++ dereferences an
/// empty range.
pub fn nfp_convex_only(sh: &ExPolygon, other: &ExPolygon) -> Result<NfpResult, ExactError> {
    let a = &sh.contour.points;
    let b = &other.contour.points;
    let mut edgelist: Vec<Segment> = Vec::with_capacity(a.len() + b.len());
    let mut add_edge = |v1: Point, v2: Point| {
        let e = Segment::new(v1, v2);
        if e.sqlength_i64() > 0 {
            edgelist.push(e);
        }
    };
    for w in a.windows(2) {
        if w[0] != w[1] {
            add_edge(w[0], w[1]);
        }
    }
    add_edge(a[a.len() - 1], a[0]);
    for w in b.windows(2) {
        if w[0] != w[1] {
            add_edge(w[1], w[0]);
        }
    }
    add_edge(b[0], b[b.len() - 1]);
    let error: Cell<Option<ExactError>> = Cell::new(None);
    orca_stl::sort_by(&mut edgelist, |e1, e2| {
        if error.get().is_some() {
            return false;
        }
        match edge_less(e1, e2) {
            Ok(r) => r,
            Err(e) => {
                error.set(Some(e));
                false
            }
        }
    });
    if let Some(e) = error.get() {
        return Err(e);
    }
    let (rsh, top_nfp) = build_polygon(&edgelist);
    Ok((ExPolygon::new(Polygon::new(rsh)), top_nfp))
}

/// `nfpInnerRectBed(bed, other)`: where the orbiter's reference point may go inside the box, as a
/// closed five-point contour; an empty shape when the orbiter is taller or wider than the box.
#[must_use]
pub fn nfp_inner_rect_bed(bed: &Box, other: &ExPolygon) -> NfpResult {
    let sbox = bounding_box(other);
    let sheight = sbox.height();
    let swidth = sbox.width();
    let sliding_top = rightmost_up_vertex(other);
    let left_offset = sliding_top.x.wrapping_sub(sbox.min.x);
    let right_offset = sliding_top.x.wrapping_sub(sbox.max.x);
    let top_offset = 0i64;
    let bottom_offset = sheight;
    let box_width = bed.width();
    let box_height = bed.height();
    let (bed_minx, bed_miny, bed_maxx, bed_maxy) = (bed.min.x, bed.min.y, bed.max.x, bed.max.y);
    let inner = vec![
        Point::new(
            bed_minx.wrapping_add(left_offset),
            bed_maxy.wrapping_add(top_offset),
        ),
        Point::new(
            bed_maxx.wrapping_add(right_offset),
            bed_maxy.wrapping_add(top_offset),
        ),
        Point::new(
            bed_maxx.wrapping_add(right_offset),
            bed_miny.wrapping_add(bottom_offset),
        ),
        Point::new(
            bed_minx.wrapping_add(left_offset),
            bed_miny.wrapping_add(bottom_offset),
        ),
        Point::new(
            bed_minx.wrapping_add(left_offset),
            bed_maxy.wrapping_add(top_offset),
        ),
    ];
    if sheight > box_height || swidth > box_width {
        (ExPolygon::default(), Point::new(0, 0))
    } else {
        (ExPolygon::new(Polygon::new(inner)), Point::new(0, 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x0: i64, y0: i64, x1: i64, y1: i64) -> ExPolygon {
        ExPolygon::new(Polygon::new(vec![
            Point::new(x0, y0),
            Point::new(x1, y0),
            Point::new(x1, y1),
            Point::new(x0, y1),
        ]))
    }

    #[test]
    fn nfp_of_two_squares_is_the_minkowski_square() {
        let (nfp, top) = nfp_convex_only(&square(0, 0, 10, 10), &square(0, 0, 4, 4)).unwrap();
        let pts = &nfp.contour.points;
        assert_eq!(pts.len(), 9);
        assert_eq!(pts.first(), pts.last());
        let bb = crate::geometry::envelope_points(pts);
        assert_eq!(bb.width(), 14);
        assert_eq!(bb.height(), 14);
        assert_eq!(top, bb.max);
    }
}
