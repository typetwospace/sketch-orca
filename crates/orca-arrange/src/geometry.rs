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
// translated on 2026-11-07 from include/libnest2d/geometry_traits.hpp (_Box, _Circle, _Segment,
// pointlike and shapelike), common.hpp (Pi, Pi_2) and backends/libslic3r/geometries.hpp (the
// libslic3r backend: offset, translate, rotate, merge, subtract). Copyright (C) Tamas Meszaros,
// modified by Prusa Research and the OrcaSlicer contributors, under the GNU Lesser General Public
// License, version 3 (licenses/LGPL-3.0.txt).

//! libnest2d's geometry traits over libslic3r's types, as `Arrange.cpp` instantiates them:
//! points are `Slic3r::Point` (`coord_t`, 64-bit), paths `Slic3r::Polygon` (open, declared
//! counter-clockwise) and shapes `Slic3r::ExPolygon`.
//!
//! - `TCompute<Point>` is `coord_t` itself (libnest2d has no wider type for it), so the integer
//!   products here (`dot`, `magnsq`, box areas) are 64-bit and wrap as MSVC's `imul` does.
//! - A box made by `boost::geometry::envelope` (a path's or shape's bounding box) is not
//!   `defined`; one made from two corners is. [`bounding_box2`] skips an undefined box, which is
//!   how libnest2d merges boxes.
//! - The path area is libnest2d's own shoelace in `double`, negated because the path type is
//!   counter-clockwise; a circle's area is `Pi_2 * r * r`, twice the true area, as libnest2d
//!   writes it.
//! - The convex hull is libnest2d's own monotone chain (the backend disables Boost's): fewer than
//!   four points give an empty hull.

use orca_geom::clipper::JoinType;
use orca_geom::clipper_utils::{
    ApplySafetyOffset, DEFAULT_MITER_LIMIT, diff_ex_expolygons, offset_ex_expolygon,
    union_ex_expolygons,
};
use orca_geom::expolygon::{ExPolygon, ExPolygons};
use orca_geom::point::{Coord, Point, Points};
use orca_geom::polygon::Polygon;

/// libnest2d's `Pi`.
pub const PI: f64 = std::f64::consts::PI;
/// libnest2d's `Pi_2`, which is `2 * Pi`.
pub const PI_2: f64 = 2.0 * PI;

/// `_Box<Point>`: two corners and whether the box is `defined`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Box {
    /// `minCorner()`.
    pub min: Point,
    /// `maxCorner()`.
    pub max: Point,
    /// `defined`.
    pub defined: bool,
}

impl Default for Box {
    /// `_Box()`: the empty box at the origin, not defined.
    fn default() -> Box {
        Box::at(Point::new(0, 0))
    }
}

impl Box {
    /// `_Box(p, pp)`: defined.
    #[must_use]
    pub const fn new(min: Point, max: Point) -> Box {
        Box {
            min,
            max,
            defined: true,
        }
    }

    /// `_Box(center)`: `_Box(0, 0, center)`.
    #[must_use]
    pub fn at(center: Point) -> Box {
        Box::with_size(0, 0, center)
    }

    /// `_Box(width, height, center)`: the corners `center -+ (width / 2, height / 2)`, the upper
    /// one plus `(width % 2, height % 2)`; not defined.
    #[must_use]
    pub fn with_size(width: Coord, height: Coord, center: Point) -> Box {
        let half = Point::new(width.wrapping_div(2), height.wrapping_div(2));
        let odd = Point::new(width.wrapping_rem(2), height.wrapping_rem(2));
        Box {
            min: center - half,
            max: center + half + odd,
            defined: false,
        }
    }

    /// `_Box::infinite(center)`: `M = coord_t((lowest + 2 * center) / 4.01)` per axis, the
    /// corners `center + M` (lower) and `center - M` (upper); not defined.
    #[must_use]
    pub fn infinite(center: Point) -> Box {
        let m = |c: Coord| {
            orca_crt::f64_to_i64(i64::MIN.wrapping_add(2i64.wrapping_mul(c)) as f64 / 4.01)
        };
        let d = Point::new(m(center.x), m(center.y));
        Box {
            min: center + d,
            max: center - d,
            defined: false,
        }
    }

    /// `width()`.
    #[must_use]
    pub fn width(&self) -> Coord {
        self.max.x.wrapping_sub(self.min.x)
    }

    /// `height()`.
    #[must_use]
    pub fn height(&self) -> Coord {
        self.max.y.wrapping_sub(self.min.y)
    }

    /// `center()`: each corner halved first, in integers.
    #[must_use]
    pub fn center(&self) -> Point {
        Point::new(
            (self.min.x / 2).wrapping_add(self.max.x / 2),
            (self.min.y / 2).wrapping_add(self.max.y / 2),
        )
    }

    /// `area<double>()`: `double(width()) * height()`.
    #[must_use]
    pub fn area_f64(&self) -> f64 {
        self.width() as f64 * self.height() as f64
    }

    /// `area()` in `TCompute`, which is `coord_t`: `width() * height()`, wrapping.
    #[must_use]
    pub fn area_i64(&self) -> i64 {
        self.width().wrapping_mul(self.height())
    }

    /// `intersection(other)`: the overlap, or the lower corner twice and not defined when the
    /// boxes do not overlap.
    #[must_use]
    pub fn intersection(&self, other: &Box) -> Box {
        let mut inter = Box {
            min: Point::new(
                orca_stl::max(self.min.x, other.min.x),
                orca_stl::max(self.min.y, other.min.y),
            ),
            max: Point::new(
                orca_stl::min(self.max.x, other.max.x),
                orca_stl::min(self.max.y, other.max.y),
            ),
            defined: true,
        };
        if inter.max.y < inter.min.y || inter.max.x < inter.min.x {
            inter.max = inter.min;
            inter.defined = false;
        }
        inter
    }
}

/// `_Circle<Point>`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Circle {
    /// `center()`.
    pub center: Point,
    /// `radius()`.
    pub radius: f64,
}

impl Circle {
    /// `_Circle(center, r)`.
    #[must_use]
    pub const fn new(center: Point, radius: f64) -> Circle {
        Circle { center, radius }
    }

    /// `area()`: `Pi_2 * radius * radius`.
    #[must_use]
    pub fn area(&self) -> f64 {
        PI_2 * self.radius * self.radius
    }
}

/// `_Segment<Point>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    /// `first()`.
    pub first: Point,
    /// `second()`.
    pub second: Point,
}

impl Segment {
    /// `_Segment(p, pp)`.
    #[must_use]
    pub const fn new(first: Point, second: Point) -> Segment {
        Segment { first, second }
    }

    /// `angleToXaxis()`: `atan2(dy, dx)` of the integer differences, plus `Pi_2` when negative.
    #[must_use]
    pub fn angle_to_xaxis(&self) -> f64 {
        let dx = self.second.x.wrapping_sub(self.first.x);
        let dy = self.second.y.wrapping_sub(self.first.y);
        let mut a = orca_crt::atan2(dy as f64, dx as f64);
        if a.is_sign_negative() {
            a += PI_2;
        }
        a
    }

    /// `sqlength()` in `TCompute`, which is `coord_t`.
    #[must_use]
    pub fn sqlength_i64(&self) -> i64 {
        squared_distance_i64(self.first, self.second)
    }

    /// `sqlength<double>()`.
    #[must_use]
    pub fn sqlength_f64(&self) -> f64 {
        squared_distance_f64(self.first, self.second)
    }
}

/// `pointlike::squaredDistance<P, coord_t>`.
#[must_use]
pub fn squared_distance_i64(p1: Point, p2: Point) -> i64 {
    let a = p2.x.wrapping_sub(p1.x);
    let b = p2.y.wrapping_sub(p1.y);
    a.wrapping_mul(a).wrapping_add(b.wrapping_mul(b))
}

/// `pointlike::squaredDistance<P, double>`: the coordinates converted first.
#[must_use]
pub fn squared_distance_f64(p1: Point, p2: Point) -> f64 {
    let a = p2.x as f64 - p1.x as f64;
    let b = p2.y as f64 - p1.y as f64;
    a * a + b * b
}

/// `pointlike::distance`: `sqrt` of the `double` squared distance.
#[must_use]
pub fn distance(p1: Point, p2: Point) -> f64 {
    orca_crt::sqrt(squared_distance_f64(p1, p2))
}

/// `pointlike::dot` in `coord_t`.
#[must_use]
pub fn dot_i64(a: Point, b: Point) -> i64 {
    a.x.wrapping_mul(b.x).wrapping_add(a.y.wrapping_mul(b.y))
}

/// `pointlike::dotperp` in `coord_t`.
#[must_use]
pub fn dotperp_i64(a: Point, b: Point) -> i64 {
    a.x.wrapping_mul(b.y).wrapping_sub(a.y.wrapping_mul(b.x))
}

/// `pointlike::magnsq` in `coord_t`.
#[must_use]
pub fn magnsq_i64(p: Point) -> i64 {
    p.x.wrapping_mul(p.x).wrapping_add(p.y.wrapping_mul(p.y))
}

/// `shapelike::area` of a path: the shoelace in `double` over the open path, negated (the path
/// type is counter-clockwise); 0 below three points.
#[must_use]
pub fn area_path(points: &[Point]) -> f64 {
    let n = points.len();
    if n < 3 {
        return 0.0;
    }
    let mut a = 0.0;
    let mut j = n - 1;
    for i in 0..n {
        let (xj, yj) = (points[j].x as f64, points[j].y as f64);
        let (xi, yi) = (points[i].x as f64, points[i].y as f64);
        a += (xj + xi) * (yj - yi);
        j = i;
    }
    a /= 2.0;
    -a
}

/// `shapelike::area` of a shape: the contour's area plus each hole's, in order.
#[must_use]
pub fn area_expolygon(sh: &ExPolygon) -> f64 {
    let mut a = area_path(&sh.contour.points);
    for h in &sh.holes {
        a += area_path(&h.points);
    }
    a
}

/// `shapelike::area` of shapes: 0 plus each shape's area, in order.
#[must_use]
pub fn area_shapes(shapes: &[ExPolygon]) -> f64 {
    let mut a = 0.0;
    for s in shapes {
        a += area_expolygon(s);
    }
    a
}

/// `boost::geometry::envelope` of points into a libnest2d box: the lowest and highest
/// coordinates, or the inverted box (`highest`, `lowest`) of an empty range; not defined.
#[must_use]
pub fn envelope_points(points: &[Point]) -> Box {
    let mut b = Box::default();
    match points.split_first() {
        None => {
            b.min = Point::new(i64::MAX, i64::MAX);
            b.max = Point::new(i64::MIN, i64::MIN);
        }
        Some((first, rest)) => {
            b.min = *first;
            b.max = *first;
            for p in rest {
                b.min.x = orca_stl::min(b.min.x, p.x);
                b.min.y = orca_stl::min(b.min.y, p.y);
                b.max.x = orca_stl::max(b.max.x, p.x);
                b.max.y = orca_stl::max(b.max.y, p.y);
            }
        }
    }
    b
}

/// `shapelike::boundingBox` of a shape: the envelope of its contour.
#[must_use]
pub fn bounding_box(sh: &ExPolygon) -> Box {
    envelope_points(&sh.contour.points)
}

/// `shapelike::boundingBox` of shapes (`boost::geometry::envelope` of the multi-polygon): each
/// non-empty polygon's exterior ring, or its holes when the exterior ring is empty, merged; the
/// inverted box when nothing was there. Not defined.
#[must_use]
pub fn bounding_box_shapes(shapes: &[ExPolygon]) -> Box {
    let mut acc: Option<(Point, Point)> = None;
    let mut expand = |p: &Point| match &mut acc {
        None => acc = Some((*p, *p)),
        Some((lo, hi)) => {
            lo.x = orca_stl::min(lo.x, p.x);
            lo.y = orca_stl::min(lo.y, p.y);
            hi.x = orca_stl::max(hi.x, p.x);
            hi.y = orca_stl::max(hi.y, p.y);
        }
    };
    for s in shapes {
        if s.contour.points.is_empty() {
            for h in &s.holes {
                h.points.iter().for_each(&mut expand);
            }
        } else {
            s.contour.points.iter().for_each(&mut expand);
        }
    }
    let mut b = Box::default();
    match acc {
        None => {
            b.min = Point::new(i64::MAX, i64::MAX);
            b.max = Point::new(i64::MIN, i64::MIN);
        }
        Some((lo, hi)) => {
            b.min = lo;
            b.max = hi;
        }
    }
    b
}

/// `shapelike::boundingBox` of a circle: `coord_t(center -+ radius)`, truncated; defined.
#[must_use]
pub fn bounding_box_circle(c: &Circle) -> Box {
    let lo = |v: Coord| orca_crt::f64_to_i64(v as f64 - c.radius);
    let hi = |v: Coord| orca_crt::f64_to_i64(v as f64 + c.radius);
    Box::new(
        Point::new(lo(c.center.x), lo(c.center.y)),
        Point::new(hi(c.center.x), hi(c.center.y)),
    )
}

/// `shapelike::boundingBox(bb1, bb2)`: an undefined box gives the other; else the box over both,
/// defined.
#[must_use]
pub fn bounding_box2(bb1: &Box, bb2: &Box) -> Box {
    if !bb1.defined {
        return *bb2;
    }
    if !bb2.defined {
        return *bb1;
    }
    Box::new(
        Point::new(
            orca_stl::min(bb1.min.x, bb2.min.x),
            orca_stl::min(bb1.min.y, bb2.min.y),
        ),
        Point::new(
            orca_stl::max(bb1.max.x, bb2.max.x),
            orca_stl::max(bb1.max.y, bb2.max.y),
        ),
    )
}

/// `shapelike::convexHull` of a path: libnest2d's monotone chain. Fewer than four points give
/// no hull; a closing point equal to the first is dropped and put back. The points are sorted by
/// x, then y (`std::sort`); the lower chain comes first, then the upper one backwards, as for a
/// counter-clockwise path type.
#[must_use]
pub fn convex_hull_points(sh: &[Point]) -> Points {
    let edges = sh.len();
    if edges <= 3 {
        return Points::new();
    }
    let mut closed = false;
    let mut upper: Points = Vec::with_capacity(1 + edges / 2);
    let mut lower: Points = Vec::with_capacity(1 + edges / 2);
    let mut pts: Points = sh.to_vec();
    let (fpt, lpt) = (pts[0], pts[edges - 1]);
    if fpt.x == lpt.x && fpt.y == lpt.y {
        closed = true;
        pts.pop();
    }
    orca_stl::sort_by(&mut pts, |v1, v2| {
        if v1.x == v2.x {
            v1.y < v2.y
        } else {
            v1.x < v2.x
        }
    });
    let dir = |p: Point, q: Point, r: Point| -> i64 {
        (q.y.wrapping_sub(p.y))
            .wrapping_mul(r.x.wrapping_sub(p.x))
            .wrapping_sub((q.x.wrapping_sub(p.x)).wrapping_mul(r.y.wrapping_sub(p.y)))
    };
    for &ik in &pts {
        while upper.len() > 1 && dir(upper[upper.len() - 2], upper[upper.len() - 1], ik) <= 0 {
            upper.pop();
        }
        while lower.len() > 1 && dir(lower[lower.len() - 2], lower[lower.len() - 1], ik) >= 0 {
            lower.pop();
        }
        upper.push(ik);
        lower.push(ik);
    }
    let mut ret: Points = Vec::with_capacity(upper.len() + lower.len());
    ret.extend_from_slice(&lower[..lower.len() - 1]);
    ret.extend(upper[1..].iter().rev());
    if closed {
        ret.push(upper[0]);
    }
    ret
}

/// `shapelike::convexHull` of shapes: the hull of every contour point, in order, as a shape with
/// no holes.
#[must_use]
pub fn convex_hull_shapes(shapes: &[ExPolygon]) -> ExPolygon {
    let mut cntr = Points::new();
    for s in shapes {
        cntr.extend_from_slice(&s.contour.points);
    }
    ExPolygon::new(Polygon::new(convex_hull_points(&cntr)))
}

/// `isInside(point, circle)`: the squared distance in `coord_t`, compared as a `double` with
/// `r * r`.
#[must_use]
pub fn is_inside_point_circle(p: Point, c: &Circle) -> bool {
    let r = c.radius;
    (squared_distance_i64(p, c.center) as f64) < r * r
}

/// `isInside(point, box)`: strictly inside.
#[must_use]
pub fn is_inside_point_box(p: Point, b: &Box) -> bool {
    p.x > b.min.x && p.x < b.max.x && p.y > b.min.y && p.y < b.max.y
}

/// `isInside(shape, circle)`: every contour point inside.
#[must_use]
pub fn is_inside_shape_circle(sh: &ExPolygon, c: &Circle) -> bool {
    sh.contour
        .points
        .iter()
        .all(|&p| is_inside_point_circle(p, c))
}

/// `isInside(box, circle)`: both corners inside.
#[must_use]
pub fn is_inside_box_circle(b: &Box, c: &Circle) -> bool {
    is_inside_point_circle(b.min, c) && is_inside_point_circle(b.max, c)
}

/// `isInside(box, box)`: inclusive on every side.
#[must_use]
pub fn is_inside_box_box(ibb: &Box, b: &Box) -> bool {
    ibb.min.x >= b.min.x && ibb.max.x <= b.max.x && ibb.min.y >= b.min.y && ibb.max.y <= b.max.y
}

/// `isInside(shape, box)`: the shape's bounding box inside the box.
#[must_use]
pub fn is_inside_shape_box(sh: &ExPolygon, b: &Box) -> bool {
    is_inside_box_box(&bounding_box(sh), b)
}

/// The backend's `offset(ExPolygon, distance)`: `offset_ex(sh, float(distance), jtSquare)`, the
/// first result replacing the shape when there is one.
pub fn offset_shape(sh: &mut ExPolygon, distance: Coord) {
    let res = offset_ex_expolygon(sh, distance as f32, JoinType::Square, DEFAULT_MITER_LIMIT);
    if let Some(first) = res.into_iter().next() {
        *sh = first;
    }
}

/// The backend's `rotate(ExPolygon, rads)`: `ExPolygon::rotate(double)`.
pub fn rotate_shape(sh: &mut ExPolygon, rads: f64) {
    sh.rotate(rads);
}

/// The backend's `translate(ExPolygon, offs)`: `ExPolygon::translate(offs)`.
pub fn translate_shape(sh: &mut ExPolygon, offs: Point) {
    sh.translate(offs);
}

/// `offset(box, d)`: the corners moved out by `d`.
pub fn offset_box(b: &mut Box, d: Coord) {
    let md = Point::new(d, d);
    b.min -= md;
    b.max += md;
}

/// `nfp::merge(shapes)`: `union_ex`.
#[must_use]
pub fn merge(shapes: &[ExPolygon]) -> ExPolygons {
    union_ex_expolygons(shapes)
}

/// `nfp::merge(shapes, shape)`: the shapes merged, the shape added, merged again.
#[must_use]
pub fn merge_with(shapes: &[ExPolygon], sh: &ExPolygon) -> ExPolygons {
    let mut m = merge(shapes);
    m.push(sh.clone());
    merge(&m)
}

/// `nfp::subtract(outer, shapes)`: `diff_ex`.
#[must_use]
pub fn subtract(outer: &[ExPolygon], shapes: &[ExPolygon]) -> ExPolygons {
    diff_ex_expolygons(outer, shapes, ApplySafetyOffset::No)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convex_hull_drops_inner_points_and_needs_four() {
        let tri = [Point::new(0, 0), Point::new(10, 0), Point::new(0, 10)];
        assert!(convex_hull_points(&tri).is_empty());
        let sq = [
            Point::new(0, 0),
            Point::new(10, 0),
            Point::new(5, 5),
            Point::new(10, 10),
            Point::new(0, 10),
        ];
        let h = convex_hull_points(&sq);
        assert_eq!(
            h,
            vec![
                Point::new(0, 0),
                Point::new(10, 0),
                Point::new(10, 10),
                Point::new(0, 10)
            ]
        );
        assert_eq!(area_path(&h), 100.0);
    }

    #[test]
    fn undefined_boxes_are_skipped_when_merged() {
        let a = envelope_points(&[Point::new(0, 0), Point::new(5, 5)]);
        let b = Box::new(Point::new(1, 1), Point::new(9, 2));
        assert_eq!(bounding_box2(&a, &b), b);
        assert_eq!(bounding_box2(&b, &a), b);
        assert_eq!(Box::default().center(), Point::new(0, 0));
        assert_eq!(
            Box::new(Point::new(-3, -3), Point::new(4, 4)).center(),
            Point::new(1, 1)
        );
    }
}
