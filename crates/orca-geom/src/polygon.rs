// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: class Polygon and its helpers of src/libslic3r/Polygon.hpp and
// Polygon.cpp of OrcaSlicer v2.4.2, translated on 2026-11-07; and Polygon.cpp 235-255 and 275-318
// (convex_points, concave_points and their filters), translated on 2026-11-13. Copyright the
// OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and the PrusaSlicer authors (Prusa
// Research), under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! `Polygon`: a closed path, counter-clockwise for a contour and clockwise for a hole.
//!
//! `convex_points` and `concave_points` pick the vertices that turn left or right, past an angle;
//! a polygon of fewer than two points, which the C++ reads before its list for, panics there.

use orca_eigen::Vec2d;

use crate::bounding_box::BoundingBox;
use crate::clipper::{self, PolyFillType};
use crate::line::{Line, Lines};
use crate::multi_point;
use crate::point::{EPSILON, Point, Points, SCALED_EPSILON, cross2, vec2d};
use crate::polyline::Polyline;

/// `Polygon`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Polygon {
    /// `points`.
    pub points: Points,
}

/// `Polygons`.
pub type Polygons = Vec<Polygon>;

impl Polygon {
    /// `Polygon(points)`.
    #[must_use]
    pub fn new(points: Points) -> Self {
        Self { points }
    }

    /// `Polygon::new_scale(points)`: each point `Point::new_scale`d (truncated).
    #[must_use]
    pub fn new_scale(points: &[Vec2d]) -> Self {
        Self::new(
            points
                .iter()
                .map(|p| Point::new_scale(p.x(), p.y()))
                .collect(),
        )
    }

    /// `size()`.
    #[must_use]
    pub fn size(&self) -> usize {
        self.points.len()
    }

    /// `empty()`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// `first_point()`.
    #[must_use]
    pub fn first_point(&self) -> Point {
        self.points[0]
    }

    /// `last_point()`: a polygon's last point is its first.
    #[must_use]
    pub fn last_point(&self) -> Point {
        self.points[0]
    }

    /// `length()`: the perimeter, the closing edge first.
    #[must_use]
    pub fn length(&self) -> f64 {
        let mut l = 0.0;
        if self.points.len() > 1 {
            l = (self.points[self.points.len() - 1] - self.points[0])
                .cast_f64()
                .norm();
            for i in 1..self.points.len() {
                l += (self.points[i] - self.points[i - 1]).cast_f64().norm();
            }
        }
        l
    }

    /// `lines()`: `to_lines(*this)`.
    #[must_use]
    pub fn lines(&self) -> Lines {
        to_lines(self)
    }

    /// `split_at_vertex(point)`.
    ///
    /// # Panics
    /// When the point is not a vertex, as the C++ throws `InvalidArgument("Point not found")`.
    #[must_use]
    pub fn split_at_vertex(&self, point: Point) -> Polyline {
        match self.points.iter().position(|&p| p == point) {
            Some(i) => self.split_at_index(i),
            None => panic!("Point not found"),
        }
    }

    /// `split_at_index(index)`: the polyline from `index` round to `index` again.
    #[must_use]
    pub fn split_at_index(&self, index: usize) -> Polyline {
        let mut polyline = Polyline::default();
        if self.points.is_empty() {
            return polyline;
        }
        polyline.points.reserve(self.points.len() + 1);
        polyline.points.extend_from_slice(&self.points[index..]);
        polyline.points.extend_from_slice(&self.points[..=index]);
        polyline
    }

    /// `split_at_first_point()`.
    #[must_use]
    pub fn split_at_first_point(&self) -> Polyline {
        self.split_at_index(0)
    }

    /// `equally_spaced_points(distance)`: of the closed polyline [`Polygon::split_at_first_point`].
    #[must_use]
    pub fn equally_spaced_points(&self, distance: f64) -> Points {
        self.split_at_first_point().equally_spaced_points(distance)
    }

    /// `Polygon::area(points)`: the signed area, `0.5 * Σ cross2(p1, p2)` from the last point.
    #[must_use]
    pub fn area_of(points: &[Point]) -> f64 {
        let mut a = 0.0;
        if points.len() >= 3 {
            let mut p1 = points[points.len() - 1].cast_f64();
            for p in points {
                let p2 = p.cast_f64();
                a += cross2(p1, p2);
                p1 = p2;
            }
        }
        0.5 * a
    }

    /// `area()`.
    #[must_use]
    pub fn area(&self) -> f64 {
        Self::area_of(&self.points)
    }

    /// `is_counter_clockwise()`: `ClipperLib::Orientation`.
    #[must_use]
    pub fn is_counter_clockwise(&self) -> bool {
        clipper::orientation(&self.points)
    }

    /// `is_clockwise()`.
    #[must_use]
    pub fn is_clockwise(&self) -> bool {
        !self.is_counter_clockwise()
    }

    /// `make_counter_clockwise()`: true when it reversed.
    pub fn make_counter_clockwise(&mut self) -> bool {
        if self.is_counter_clockwise() {
            false
        } else {
            self.points.reverse();
            true
        }
    }

    /// `make_clockwise()`: true when it reversed.
    pub fn make_clockwise(&mut self) -> bool {
        if self.is_counter_clockwise() {
            self.points.reverse();
            true
        } else {
            false
        }
    }

    /// `is_valid()`: three points at least.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.points.len() >= 3
    }

    /// `douglas_peucker(tolerance)`: the closed path simplified, the first point kept.
    pub fn douglas_peucker(&mut self, tolerance: f64) {
        self.points.push(self.points[0]);
        let mut p = multi_point::douglas_peucker(&self.points, tolerance);
        p.pop();
        self.points = p;
    }

    /// `simplify(tolerance)`: Douglas-Peucker, then `simplify_polygons`.
    #[must_use]
    pub fn simplify(&self, tolerance: f64) -> Polygons {
        let mut points = self.points.clone();
        points.push(points[0]);
        let mut p = Polygon::new(multi_point::douglas_peucker(&points, tolerance));
        p.points.pop();
        crate::clipper_utils::simplify_polygons(&[p])
    }

    /// `triangulate_convex(&polygons)`: the fan from the first point, the triangles of positive
    /// area appended.
    pub fn triangulate_convex(&self, polygons: &mut Polygons) {
        for i in 2..self.points.len() {
            let p = Polygon::new(vec![self.points[0], self.points[i - 1], self.points[i]]);
            if p.area() > 0.0 {
                polygons.push(p);
            }
        }
    }

    /// `centroid()`: the area centroid, `Point(Vec2d(c / (3 * area_sum)))`, rounded.
    #[must_use]
    pub fn centroid(&self) -> Point {
        let mut area_sum = 0.0;
        let mut c = vec2d(0.0, 0.0);
        if self.points.len() >= 3 {
            let mut p1 = self.points[self.points.len() - 1].cast_f64();
            for p in &self.points {
                let p2 = p.cast_f64();
                let a = cross2(p1, p2);
                area_sum += a;
                c += (p1 + p2) * a;
                p1 = p2;
            }
        }
        Point::from_vec2d(c / (3.0 * area_sum))
    }

    /// `intersection(line, &point)`: the closing edge first, then the others in order.
    #[must_use]
    pub fn intersection(&self, line: &Line) -> Option<Point> {
        if self.points.len() < 2 {
            return None;
        }
        let n = self.points.len();
        if let Some(ip) = Line::new(self.points[0], self.points[n - 1]).intersection(line) {
            return Some(ip);
        }
        (1..n).find_map(|i| Line::new(self.points[i - 1], self.points[i]).intersection(line))
    }

    /// `first_intersection(line, &point)`: the meeting point nearest `line.a`.
    #[must_use]
    pub fn first_intersection(&self, line: &Line) -> Option<Point> {
        if self.points.len() < 2 {
            return None;
        }
        let mut found: Option<Point> = None;
        let mut dmin = 0.0;
        let mut l = Line::new(self.points[self.points.len() - 1], self.points[0]);
        for &p in &self.points {
            l.b = p;
            if let Some(ip) = l.intersection(line) {
                let d = (line.a - ip).cast_f64().squared_norm();
                if found.is_none() || d < dmin {
                    dmin = d;
                    found = Some(ip);
                }
            }
            l.a = l.b;
        }
        found
    }

    /// `intersections(line, &points)`: appends every meeting point, the closing edge first.
    pub fn intersections(&self, line: &Line, out: &mut Points) -> bool {
        if self.points.len() < 2 {
            return false;
        }
        let size = out.len();
        let mut l = Line::new(self.points[self.points.len() - 1], self.points[0]);
        for &p in &self.points {
            l.b = p;
            if let Some(ip) = l.intersection(line) {
                out.push(ip);
            }
            l.a = l.b;
        }
        out.len() > size
    }

    /// `point_projection(point)`: the nearest point of the outline, vertices first, a foot on an
    /// edge rounded with `floor(x + 0.5)`.
    #[must_use]
    pub fn point_projection(&self, point: Point) -> Point {
        let mut proj = point;
        let mut dmin = f64::MAX;
        let n = self.points.len();
        for i in 0..n {
            let pt0 = self.points[i];
            let pt1 = self.points[if i + 1 == n { 0 } else { i + 1 }];
            let mut d = (point - pt0).cast_f64().norm();
            if d < dmin {
                dmin = d;
                proj = pt0;
            }
            d = (point - pt1).cast_f64().norm();
            if d < dmin {
                dmin = d;
                proj = pt1;
            }
            let v1 = vec2d(
                pt1.x.wrapping_sub(pt0.x) as f64,
                pt1.y.wrapping_sub(pt0.y) as f64,
            );
            let div = v1.squared_norm();
            if div > 0.0 {
                let v2 = vec2d(
                    point.x.wrapping_sub(pt0.x) as f64,
                    point.y.wrapping_sub(pt0.y) as f64,
                );
                let t = v1.dot(&v2) / div;
                if t > 0.0 && t < 1.0 {
                    let foot = Point::new(
                        orca_crt::f64_to_i64(orca_crt::floor(pt0.x as f64 + t * v1.x() + 0.5)),
                        orca_crt::f64_to_i64(orca_crt::floor(pt0.y as f64 + t * v1.y() + 0.5)),
                    );
                    d = (point - foot).cast_f64().norm();
                    if d < dmin {
                        dmin = d;
                        proj = foot;
                    }
                }
            }
        }
        proj
    }

    /// `contains(point)`: `Slic3r::contains(*this, point, true)`.
    #[must_use]
    pub fn contains(&self, point: Point) -> bool {
        contains(self, point, true)
    }

    /// `on_boundary(point, eps)`.
    #[must_use]
    pub fn on_boundary(&self, point: Point, eps: f64) -> bool {
        (self.point_projection(point) - point)
            .cast_f64()
            .squared_norm()
            < eps * eps
    }

    /// `bounding_box()`.
    #[must_use]
    pub fn bounding_box(&self) -> BoundingBox {
        BoundingBox::from_points(&self.points)
    }

    /// `translate(v)`.
    pub fn translate(&mut self, v: Point) {
        multi_point::translate(&mut self.points, v);
    }

    /// `rotate(angle)`.
    pub fn rotate(&mut self, angle: f64) {
        multi_point::rotate(&mut self.points, angle);
    }

    /// `rotate(cos_angle, sin_angle)`.
    pub fn rotate_cs(&mut self, cos_angle: f64, sin_angle: f64) {
        multi_point::rotate_cs(&mut self.points, cos_angle, sin_angle);
    }

    /// `rotate(angle, center)`.
    pub fn rotate_around(&mut self, angle: f64, center: Point) {
        multi_point::rotate_around(&mut self.points, angle, center);
    }

    /// `scale(factor)`.
    pub fn scale(&mut self, factor: f64) {
        multi_point::scale(&mut self.points, factor);
    }

    /// `scale(factor_x, factor_y)`.
    pub fn scale_xy(&mut self, factor_x: f64, factor_y: f64) {
        multi_point::scale_xy(&mut self.points, factor_x, factor_y);
    }

    /// `reverse()`.
    pub fn reverse(&mut self) {
        self.points.reverse();
    }

    /// `convex_points(angle_threshold)`: the vertices that turn left (`cross2 > 0` of the edge
    /// into them and the edge out), with an angle between the two edges past `angle_threshold`
    /// when that is above `EPSILON`; the last vertex first, then the others in order.
    ///
    /// # Panics
    /// Below two points, which the C++ reads before the list for (`parity/sop1/paths.md`, "Edges").
    #[must_use]
    pub fn convex_points(&self, angle_threshold: f64) -> Points {
        filter_convex_concave_points_by_angle_threshold(&self.points, angle_threshold, |v1, v2| {
            cross2(v1, v2) > 0.
        })
    }

    /// `concave_points(angle_threshold)`: as [`Polygon::convex_points`], for the vertices that
    /// turn right (`cross2 < 0`).
    ///
    /// # Panics
    /// Below two points, which the C++ reads before the list for (`parity/sop1/paths.md`, "Edges").
    #[must_use]
    pub fn concave_points(&self, angle_threshold: f64) -> Points {
        filter_convex_concave_points_by_angle_threshold(&self.points, angle_threshold, |v1, v2| {
            cross2(v1, v2) < 0.
        })
    }
}

/// `filter_points_by_vectors(poly, filter)`: each vertex for which `filter(v1, v2)` holds, `v1`
/// the edge into it and `v2` the edge out, visited from the last vertex (Polygon.cpp 235-255).
fn filter_points_by_vectors(poly: &[Point], filter: impl Fn(Vec2d, Vec2d) -> bool) -> Points {
    if poly.len() < 2 {
        panic!(
            "filter_points_by_vectors of fewer than two points: undefined in the C++ engine (it \
             reads before the list), not ported"
        );
    }
    // The last point is the first visited.
    let mut p1 = poly[poly.len() - 1];
    // The vector into p1.
    let mut v1 = (p1 - poly[poly.len() - 2]).cast_f64();
    let mut out = Points::new();
    for &p2 in poly {
        // p2 is the point after the visited p1.
        let v2 = (p2 - p1).cast_f64();
        if filter(v1, v2) {
            out.push(p1);
        }
        v1 = v2;
        p1 = p2;
    }
    out
}

/// `filter_convex_concave_points_by_angle_threshold(poly, angle_threshold, filter)`: with a
/// threshold above `EPSILON`, also the angle between the edges past it, the dot product of the
/// normalized edges below `cos(nextafter(angle_threshold, +inf))` (Polygon.cpp 275-308; the
/// asserts on the threshold are the debug build's).
fn filter_convex_concave_points_by_angle_threshold(
    poly: &[Point],
    angle_threshold: f64,
    convex_concave_filter: impl Fn(Vec2d, Vec2d) -> bool,
) -> Points {
    if angle_threshold > EPSILON {
        // Just enough added that an input of π/2 does not match a right angle.
        let cos_threshold = orca_crt::cos(orca_crt::nextafter(angle_threshold, f64::INFINITY));
        filter_points_by_vectors(poly, |v1, v2| {
            if !convex_concave_filter(v1, v2) {
                return false;
            }
            // The dot product of the unit vectors is the cosine of the angle between them.
            let vec_dot = v1.normalized().dot(&v2.normalized());
            vec_dot < cos_threshold
        })
    } else {
        filter_points_by_vectors(poly, convex_concave_filter)
    }
}

/// `contains(polygon, p, border_result)`: `PointInPolygon`, `border_result` on the boundary.
#[must_use]
pub fn contains(polygon: &Polygon, p: Point, border_result: bool) -> bool {
    let inside = clipper::point_in_polygon(p, &polygon.points);
    if inside == -1 {
        border_result
    } else {
        inside % 2 == 1
    }
}

/// `contains(polygons, p, border_result)`: inside an odd number of them.
#[must_use]
pub fn contains_polygons(polygons: &[Polygon], p: Point, border_result: bool) -> bool {
    let mut count = 0;
    for poly in polygons {
        let inside = clipper::point_in_polygon(p, &poly.points);
        if inside == -1 {
            return border_result;
        }
        count += inside;
    }
    count % 2 == 1
}

/// `to_lines(poly)`: the edges, the closing one last; none below three points.
#[must_use]
pub fn to_lines(poly: &Polygon) -> Lines {
    let mut lines = Lines::with_capacity(poly.points.len());
    if poly.points.len() > 2 {
        for w in poly.points.windows(2) {
            lines.push(Line::new(w[0], w[1]));
        }
        lines.push(Line::new(
            poly.points[poly.points.len() - 1],
            poly.points[0],
        ));
    }
    lines
}

/// `to_polyline(polygon)`: the points and the first again.
#[must_use]
pub fn to_polyline(polygon: &Polygon) -> Polyline {
    let mut out = Polyline::new(polygon.points.clone());
    out.points.push(polygon.points[0]);
    out
}

/// `to_polylines(polygons)`.
#[must_use]
pub fn to_polylines(polygons: &[Polygon]) -> Vec<Polyline> {
    polygons.iter().map(to_polyline).collect()
}

/// `to_points(polygons)`: every point, in order.
#[must_use]
pub fn to_points(polys: &[Polygon]) -> Points {
    let mut points = Points::with_capacity(count_points(polys));
    for poly in polys {
        points.extend_from_slice(&poly.points);
    }
    points
}

/// `count_points(polys)`.
#[must_use]
pub fn count_points(polys: &[Polygon]) -> usize {
    polys.iter().map(|p| p.points.len()).sum()
}

/// `total_length(polygons)`.
#[must_use]
pub fn total_length(polygons: &[Polygon]) -> f64 {
    let mut total = 0.0;
    for p in polygons {
        total += p.length();
    }
    total
}

/// `area(polygons)`: the sum of the signed areas.
#[must_use]
pub fn area(polys: &[Polygon]) -> f64 {
    let mut s = 0.0;
    for p in polys {
        s += p.area();
    }
    s
}

/// `get_extents(polygon)`.
#[must_use]
pub fn get_extents(poly: &Polygon) -> BoundingBox {
    poly.bounding_box()
}

/// `get_extents(polygons)`: the first polygon's box merged with the others'.
#[must_use]
pub fn get_extents_polygons(polygons: &[Polygon]) -> BoundingBox {
    let mut bb = BoundingBox::new();
    if let Some((first, rest)) = polygons.split_first() {
        bb = get_extents(first);
        for p in rest {
            bb.merge_box(&get_extents(p));
        }
    }
    bb
}

/// `get_extents_rotated(polygons, angle)`.
#[must_use]
pub fn get_extents_rotated_polygons(polygons: &[Polygon], angle: f64) -> BoundingBox {
    let mut bb = BoundingBox::new();
    if let Some((first, rest)) = polygons.split_first() {
        bb = multi_point::get_extents_rotated(&first.points, angle);
        for p in rest {
            bb.merge_box(&multi_point::get_extents_rotated(&p.points, angle));
        }
    }
    bb
}

/// `polygon_is_convex(points)`: no right turn, in `int64_t` cross products.
#[must_use]
pub fn polygon_is_convex(poly: &[Point]) -> bool {
    if poly.len() < 3 {
        return false;
    }
    let mut p0 = poly[poly.len() - 2];
    let mut p1 = poly[poly.len() - 1];
    for &p2 in poly {
        let det = crate::point::cross2_coord(p1 - p0, p2 - p1);
        if det < 0 {
            return false;
        }
        p0 = p1;
        p1 = p2;
    }
    true
}

/// `has_duplicate_points(polygons)`: over all their points together.
#[must_use]
pub fn has_duplicate_points_polygons(polys: &[Polygon]) -> bool {
    crate::point::has_duplicate_points(&to_points(polys))
}

/// `remove_same_neighbor(polygon)`: `std::unique`, then the last point when it equals the first.
pub fn remove_same_neighbor(polygon: &mut Polygon) -> bool {
    let points = &mut polygon.points;
    if points.is_empty() {
        return false;
    }
    let mut last = orca_stl::unique(points);
    if points[last - 1] == points[0] {
        last -= 1;
    }
    if last == points.len() {
        return false;
    }
    points.truncate(last);
    true
}

/// `remove_same_neighbor(polygons)`: each polygon, then those of two points or fewer dropped.
pub fn remove_same_neighbor_polygons(polygons: &mut Polygons) -> bool {
    if polygons.is_empty() {
        return false;
    }
    let mut exist = false;
    for p in polygons.iter_mut() {
        exist |= remove_same_neighbor(p);
    }
    polygons.retain(|p| p.points.len() > 2);
    exist
}

/// `is_stick(p1, p2, p3)`: p2 turns back (or is a duplicate) within `EPSILON` of the line.
fn is_stick(p1: Point, p2: Point, p3: Point) -> bool {
    let v1 = p2 - p1;
    let v2 = p3 - p2;
    let dir =
        v1.x.wrapping_mul(v2.x)
            .wrapping_add(v1.y.wrapping_mul(v2.y));
    if dir > 0 {
        return false;
    }
    let l2_1 = v1.x as f64 * v1.x as f64 + v1.y as f64 * v1.y as f64;
    let l2_2 = v2.x as f64 * v2.x as f64 + v2.y as f64 * v2.y as f64;
    if dir == 0 {
        return l2_1 == 0.0 || l2_2 == 0.0;
    }
    let cross = v1.x as f64 * v2.y as f64 - v2.x as f64 * v1.y as f64;
    let dist2 = cross * cross / orca_stl::max(l2_1, l2_2);
    dist2 < EPSILON * EPSILON
}

/// `remove_sticks(polygon)`.
pub fn remove_sticks(poly: &mut Polygon) -> bool {
    let mut modified = false;
    let mut j = 1_usize;
    let mut i = 1_usize;
    while i + 1 < poly.points.len() {
        if !is_stick(poly.points[j - 1], poly.points[i], poly.points[i + 1]) {
            if j < i {
                poly.points[j] = poly.points[i];
            }
            j += 1;
        }
        i += 1;
    }
    j += 1;
    if j < poly.points.len() {
        let last = poly.points[poly.points.len() - 1];
        poly.points[j - 1] = last;
        poly.points.truncate(j);
        modified = true;
    }
    while poly.points.len() >= 3 && {
        let n = poly.points.len();
        is_stick(poly.points[n - 2], poly.points[n - 1], poly.points[0])
    } {
        poly.points.pop();
        modified = true;
    }
    while poly.points.len() >= 3 && {
        let n = poly.points.len();
        is_stick(poly.points[n - 1], poly.points[0], poly.points[1])
    } {
        poly.points.remove(0);
    }
    modified
}

/// `remove_sticks(polygons)`: each polygon, then those below three points dropped.
pub fn remove_sticks_polygons(polys: &mut Polygons) -> bool {
    let mut modified = false;
    let mut j = 0_usize;
    for i in 0..polys.len() {
        modified |= remove_sticks(&mut polys[i]);
        if polys[i].points.len() >= 3 {
            if j < i {
                let pts = core::mem::take(&mut polys[i].points);
                polys[i].points = core::mem::replace(&mut polys[j].points, pts);
            }
            j += 1;
        }
    }
    polys.truncate(j);
    modified
}

/// `remove_degenerate(polygons)`: those below three points dropped.
pub fn remove_degenerate(polys: &mut Polygons) -> bool {
    let n = polys.len();
    polys.retain(|p| p.points.len() >= 3);
    polys.len() != n
}

/// `remove_small(polygons, min_area)`: those of smaller absolute area dropped.
pub fn remove_small(polys: &mut Polygons, min_area: f64) -> bool {
    let n = polys.len();
    polys.retain(|p| orca_crt::fabs(p.area()) >= min_area);
    polys.len() != n
}

/// `remove_collinear(polygon)`: drops each point within `SCALED_EPSILON` of the line through its
/// kept neighbours.
pub fn remove_collinear(poly: &mut Polygon) {
    if poly.points.len() > 2 {
        let mut pp = Points::with_capacity(poly.points.len() + 2);
        pp.push(poly.points[poly.points.len() - 1]);
        pp.extend_from_slice(&poly.points);
        pp.push(poly.points[0]);
        poly.points.clear();
        let mut i = 0_usize;
        while i < pp.len() - 2 {
            let mut k = i + 1;
            let p1 = pp[i];
            while k < pp.len() - 1 {
                let p2 = pp[k];
                let p3 = pp[k + 1];
                let l = Line::new(p1, p3);
                if l.distance_to(p2) < SCALED_EPSILON {
                    k += 1;
                } else {
                    if i > 0 {
                        poly.points.push(p1);
                    }
                    i = k;
                    break;
                }
            }
            if k > pp.len() - 2 {
                break;
            }
        }
        poly.points.push(pp[i]);
    }
}

/// `polygons_simplify(polygons, tolerance, strictly_simple)`: each polygon Douglas-Peucker
/// simplified as a closed polyline, then `SimplifyPolygons` (non-zero), reversed back when the
/// simplified path was clockwise.
#[must_use]
pub fn polygons_simplify(source: &[Polygon], tolerance: f64, strictly_simple: bool) -> Polygons {
    let mut out = Polygons::with_capacity(source.len());
    for sp in source {
        let mut simplified = multi_point::douglas_peucker(&to_polyline(sp).points, tolerance);
        simplified.pop();
        let ccw = clipper::area(&simplified) > 0.0;
        for mut path in clipper::simplify_polygons(
            core::iter::once(simplified.as_slice()),
            PolyFillType::NonZero,
            strictly_simple,
        ) {
            if !ccw {
                path.reverse();
            }
            out.push(Polygon::new(path));
        }
    }
    out
}

/// `make_circle_num_segments(radius, num_segments)`: points at `coord_t(cos(a) * r)`, truncated.
#[must_use]
pub fn make_circle_num_segments(radius: f64, num_segments: usize) -> Polygon {
    let mut out = Polygon::new(Points::with_capacity(num_segments));
    let angle_inc = 2.0 * core::f64::consts::PI / num_segments as f64;
    for i in 0..num_segments {
        let angle = angle_inc * i as f64;
        out.points.push(Point::new(
            orca_crt::f64_to_i64(orca_crt::cos(angle) * radius),
            orca_crt::f64_to_i64(orca_crt::sin(angle) * radius),
        ));
    }
    out
}

/// `make_circle(radius, error)`.
#[must_use]
pub fn make_circle(radius: f64, error: f64) -> Polygon {
    let angle = 2.0 * orca_crt::acos(1.0 - error / radius);
    let num_segments = orca_crt::f64_to_u64(orca_crt::ceil(2.0 * core::f64::consts::PI / angle));
    make_circle_num_segments(radius, num_segments as usize)
}
