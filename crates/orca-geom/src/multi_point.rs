// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: class MultiPoint of src/libslic3r/MultiPoint.hpp and MultiPoint.cpp of
// OrcaSlicer v2.4.2, translated on 2026-11-07. Copyright the OrcaSlicer authors, the BambuStudio
// authors (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU Affero General
// Public License, version 3 (AGPL-3.0; LICENSE).

//! `MultiPoint`, the base of `Polygon` and `Polyline`, as functions of their points.
//!
//! The C++ class has a virtual `lines()` (closed for a polygon, open for a polyline); the
//! functions that need the lines take them. `visivalingam` is not here: nothing in OrcaSlicer
//! v2.4.2 calls it, and it is undefined (its heap orders `vis_node*` by address and it writes
//! `areas` past its size), so there is no behaviour to match (`parity/HAZARDS.md`, orca-geom).

use crate::bounding_box::BoundingBox;
use crate::line::Line;
use crate::point::{Coord, Point, Points, SCALED_EPSILON};

/// `scale(factor)`: each point `*= factor`, truncated.
pub fn scale(points: &mut [Point], factor: f64) {
    for pt in points {
        pt.scale_assign(factor);
    }
}

/// `scale(factor_x, factor_y)`: each coordinate `coord_t(c * factor)`, truncated.
pub fn scale_xy(points: &mut [Point], factor_x: f64, factor_y: f64) {
    for pt in points {
        pt.x = orca_crt::f64_to_i64(pt.x as f64 * factor_x);
        pt.y = orca_crt::f64_to_i64(pt.y as f64 * factor_y);
    }
}

/// `translate(vector)`.
pub fn translate(points: &mut [Point], v: Point) {
    for pt in points {
        *pt += v;
    }
}

/// `translate(x, y)`: by `Point(coord_t(x), coord_t(y))`, truncated.
pub fn translate_f64(points: &mut [Point], x: f64, y: f64) {
    translate(
        points,
        Point::new(orca_crt::f64_to_i64(x), orca_crt::f64_to_i64(y)),
    );
}

/// `rotate(angle)`: `rotate(cos(angle), sin(angle))`.
pub fn rotate(points: &mut [Point], angle: f64) {
    rotate_cs(points, orca_crt::cos(angle), orca_crt::sin(angle));
}

/// `rotate(cos_angle, sin_angle)`: each coordinate `round`ed.
pub fn rotate_cs(points: &mut [Point], cos_angle: f64, sin_angle: f64) {
    for pt in points {
        let cur_x = pt.x as f64;
        let cur_y = pt.y as f64;
        pt.x = orca_crt::f64_to_i64(orca_crt::round(cos_angle * cur_x - sin_angle * cur_y));
        pt.y = orca_crt::f64_to_i64(orca_crt::round(cos_angle * cur_y + sin_angle * cur_x));
    }
}

/// `rotate(angle, center)`: each coordinate `round`ed.
pub fn rotate_around(points: &mut [Point], angle: f64, center: Point) {
    let s = orca_crt::sin(angle);
    let c = orca_crt::cos(angle);
    for pt in points {
        let v = *pt - center;
        pt.x = orca_crt::f64_to_i64(orca_crt::round(
            center.x as f64 + c * v.x as f64 - s * v.y as f64,
        ));
        pt.y = orca_crt::f64_to_i64(orca_crt::round(
            center.y as f64 + c * v.y as f64 + s * v.x as f64,
        ));
    }
}

/// `length()` of the lines.
#[must_use]
pub fn lines_length(lines: &[Line]) -> f64 {
    let mut len = 0.0;
    for l in lines {
        len += l.length();
    }
    len
}

/// `find_point(point)`: the index of the first equal point, -1 for none.
#[must_use]
pub fn find_point(points: &[Point], point: Point) -> i32 {
    points
        .iter()
        .position(|&p| p == point)
        .map_or(-1, |i| i as i32)
}

/// `find_point(point, scaled_epsilon)`: the nearest point if it is closer than
/// `scaled_epsilon`, -1 otherwise; with 0 the exact search.
#[must_use]
pub fn find_point_eps(points: &[Point], point: Point, scaled_epsilon: f64) -> i32 {
    if scaled_epsilon == 0.0 {
        return find_point(points, point);
    }
    let mut dist2_min = f64::MAX;
    let eps2 = scaled_epsilon * scaled_epsilon;
    let mut idx_min = -1_i32;
    for (i, &pt) in points.iter().enumerate() {
        let d2 = (pt - point).cast_f64().squared_norm();
        if d2 < dist2_min {
            idx_min = i as i32;
            dist2_min = d2;
        }
    }
    if dist2_min < eps2 { idx_min } else { -1 }
}

/// `has_boundary_point(point)`: the point's projection onto the lines is closer than
/// `SCALED_EPSILON`.
#[must_use]
pub fn has_boundary_point(points: &[Point], lines: &[Line], point: Point) -> bool {
    let dist = (point.projection_onto_lines(points[0], lines) - point)
        .cast_f64()
        .norm();
    dist < SCALED_EPSILON
}

/// `closest_point_index(point)`: the first nearest point's index, -1 for none.
#[must_use]
pub fn closest_point_index(points: &[Point], point: Point) -> i32 {
    let mut idx = -1_i32;
    if let Some(&first) = points.first() {
        idx = 0;
        let mut dist_min = (point - first).cast_f64().norm();
        for (i, &p) in points.iter().enumerate().skip(1) {
            let d = (p - point).cast_f64().norm();
            if d < dist_min {
                dist_min = d;
                idx = i as i32;
            }
        }
    }
    idx
}

/// `bounding_box()`: `BoundingBox(points)`.
#[must_use]
pub fn bounding_box(points: &[Point]) -> BoundingBox {
    BoundingBox::from_points(points)
}

/// `has_duplicate_points()`: two equal neighbours.
#[must_use]
pub fn has_duplicate_points(points: &[Point]) -> bool {
    points.windows(2).any(|w| w[0] == w[1])
}

/// `remove_duplicate_points()`: drops each point equal to the one kept before it; true when any
/// was dropped.
pub fn remove_duplicate_points(points: &mut Points) -> bool {
    let mut j = 0_usize;
    for i in 1..points.len() {
        if points[j] != points[i] {
            j += 1;
            if j < i {
                points[j] = points[i];
            }
        }
    }
    j += 1;
    if j < points.len() {
        points.truncate(j);
        return true;
    }
    false
}

/// `intersection(line, &point)`: the first of the lines that meets `line`.
#[must_use]
pub fn intersection(lines: &[Line], line: &Line) -> Option<Point> {
    lines.iter().find_map(|l| l.intersection(line))
}

/// `first_intersection(line, &point)`: the meeting point nearest `line.a`.
#[must_use]
pub fn first_intersection(lines: &[Line], line: &Line) -> Option<Point> {
    let mut found: Option<Point> = None;
    let mut dmin = 0.0;
    for l in lines {
        if let Some(ip) = l.intersection(line) {
            let d = (line.a - ip).cast_f64().norm();
            if found.is_none() || d < dmin {
                dmin = d;
                found = Some(ip);
            }
        }
    }
    found
}

/// `intersections(line, &points)`: appends every meeting point; true when any was found.
pub fn intersections(lines: &[Line], line: &Line, out: &mut Points) -> bool {
    let size = out.len();
    for l in lines {
        if let Some(ip) = l.intersection(line) {
            out.push(ip);
        }
    }
    out.len() > size
}

/// `symmetric_y(x_axis)`: each x mirrored, `2 * x_axis - x`.
pub fn symmetric_y(points: &mut [Point], x_axis: Coord) {
    for pt in points {
        pt.x = x_axis.wrapping_mul(2).wrapping_sub(pt.x);
    }
}

/// `MultiPoint::_douglas_peucker(points, tolerance)`: the Douglas-Peucker simplification, with
/// an explicit stack, keeping the first and the last point.
#[must_use]
pub fn douglas_peucker(pts: &[Point], tolerance: f64) -> Points {
    let mut result_pts = Points::new();
    let tolerance_sq = tolerance * tolerance;
    if pts.is_empty() {
        return result_pts;
    }
    let mut anchor_idx = 0_usize;
    let mut floater_idx = pts.len() - 1;
    result_pts.reserve(pts.len());
    result_pts.push(pts[anchor_idx]);
    if anchor_idx != floater_idx {
        let mut dp_stack: Vec<usize> = Vec::with_capacity(pts.len());
        dp_stack.push(floater_idx);
        loop {
            let mut max_dist_sq = 0.0;
            let mut furthest_idx = anchor_idx;
            let (anchor, floater) = (pts[anchor_idx], pts[floater_idx]);
            for (i, &p) in pts
                .iter()
                .enumerate()
                .take(floater_idx)
                .skip(anchor_idx + 1)
            {
                let dist_sq = Line::new(anchor, floater).distance_to_squared(p);
                if dist_sq > max_dist_sq {
                    max_dist_sq = dist_sq;
                    furthest_idx = i;
                }
            }
            if max_dist_sq <= tolerance_sq {
                result_pts.push(floater);
                anchor_idx = floater_idx;
                dp_stack.pop();
                match dp_stack.last() {
                    None => break,
                    Some(&f) => floater_idx = f,
                }
            } else {
                floater_idx = furthest_idx;
                dp_stack.push(floater_idx);
            }
        }
    }
    result_pts
}

/// `MultiPoint::concave_hull_2d(pts, tolerance)`.
#[must_use]
pub fn concave_hull_2d(pts: &[Point], tolerance: f64) -> Points {
    let mut hull = Points::new();
    let n = pts.len();
    if n >= 3 {
        let mut k = 0_usize;
        hull.resize(n, Point::default());
        for &p in pts {
            while k >= 2 && {
                let ccw = p.ccw(hull[k - 2], hull[k - 1]);
                ccw <= 0.0 && -ccw / (p - hull[k - 1]).cast_f64().norm() < tolerance
            } {
                k -= 1;
            }
            hull[k] = p;
            k += 1;
        }
        hull.truncate(k);
        if hull.first() == hull.last() {
            hull.pop();
        }
    }
    hull
}

/// `MultiPoint::squaredDistanceToLineSegment(p, v, w)`: in `coord_t` squared norms, with the
/// projection a rounded `Point`.
fn squared_distance_to_line_segment(p: Point, v: Point, w: Point) -> f64 {
    let l2 = (v - w).squared_norm() as f64;
    if l2 == 0.0 {
        return (p - v).squared_norm() as f64;
    }
    let t = orca_stl::max(0.0, orca_stl::min(1.0, (p - v).dot(w - v) as f64 / l2));
    let projection = Point::from_f64(
        v.x as f64 + t * w.x.wrapping_sub(v.x) as f64,
        v.y as f64 + t * w.y.wrapping_sub(v.y) as f64,
    );
    (p - projection).squared_norm() as f64
}

/// `MultiPoint::minimumDistanceBetweenLinesDefinedByPoints(a, b)`.
#[must_use]
pub fn minimum_distance_between_lines_defined_by_points(a: &[Point], b: &[Point]) -> f64 {
    let mut min_distance = f64::INFINITY;
    for i in 0..a.len().wrapping_sub(1) {
        for &q in b {
            let distance = squared_distance_to_line_segment(q, a[i], a[i + 1]);
            min_distance = orca_stl::min(min_distance, orca_crt::sqrt(distance));
        }
    }
    for i in 0..b.len().wrapping_sub(1) {
        for &q in a {
            let distance = squared_distance_to_line_segment(q, b[i], b[i + 1]);
            min_distance = orca_stl::min(min_distance, orca_crt::sqrt(distance));
        }
    }
    min_distance
}

/// `get_extents_rotated(points, angle)`: the box of the points rotated about the origin, each
/// coordinate `round`ed.
#[must_use]
pub fn get_extents_rotated(points: &[Point], angle: f64) -> BoundingBox {
    let mut bbox = BoundingBox::new();
    if let Some((first, rest)) = points.split_first() {
        let s = orca_crt::sin(angle);
        let c = orca_crt::cos(angle);
        let rot = |p: &Point| {
            let (cur_x, cur_y) = (p.x as f64, p.y as f64);
            (
                orca_crt::f64_to_i64(orca_crt::round(c * cur_x - s * cur_y)),
                orca_crt::f64_to_i64(orca_crt::round(c * cur_y + s * cur_x)),
            )
        };
        let (x0, y0) = rot(first);
        bbox.min = Point::new(x0, y0);
        bbox.max = bbox.min;
        for p in rest {
            let (x, y) = rot(p);
            bbox.min.x = orca_stl::min(x, bbox.min.x);
            bbox.min.y = orca_stl::min(y, bbox.min.y);
            bbox.max.x = orca_stl::max(x, bbox.max.x);
            bbox.max.y = orca_stl::max(y, bbox.max.y);
        }
        bbox.defined = true;
    }
    bbox
}

/// `length(points)`: the open path's length.
#[must_use]
pub fn length(pts: &[Point]) -> f64 {
    let mut total = 0.0;
    for w in pts.windows(2) {
        total += (w[1] - w[0]).cast_f64().norm();
    }
    total
}

/// `area(points)` of `MultiPoint.hpp`: twice the signed area, as
/// `Σ double(x_i + x_j) * double(y_i - y_j)` in `coord_t` sums.
#[must_use]
pub fn area2(polygon: &[Point]) -> f64 {
    let mut area = 0.0;
    let n = polygon.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        area += polygon[i].x.wrapping_add(polygon[j].x) as f64
            * polygon[i].y.wrapping_sub(polygon[j].y) as f64;
        j = i;
    }
    area
}
