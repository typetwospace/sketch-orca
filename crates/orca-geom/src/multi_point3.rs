// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: class MultiPoint3 of src/libslic3r/MultiPoint.hpp (117-166) and
// MultiPoint.cpp (425-532 and 578-649) of OrcaSlicer v2.4.2, translated on 2026-11-13. Copyright
// the OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and the PrusaSlicer authors (Prusa
// Research), under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! `MultiPoint3`: the members of the abstract base of `Polyline3`, as functions of a point slice
//! (the C++ class's virtual `lines()` is `Polyline3`'s, the only kind there is, so `length` takes
//! the lines), and `_douglas_peucker` of Point3s.
//!
//! The members that change points work on x and y and keep z (`rotate`); `translate` moves all
//! three. `find_point` has four forms: of a `Point` (x and y) or a `Point3` (all three), exact or
//! the nearest within an epsilon (0 meaning exact). The Douglas-Peucker measures the 3D distance
//! to each segment ([`Line3::distance_to_squared`]) and keeps the same stack of floaters as the 2D
//! one. The appends of the C++ class (`append(Point3)`, `append(Vec3crd)`, a range) are the
//! vector's own `push` and `extend`; `Polyline3` hides them behind its fitting-aware `append`.

use crate::point::{Coord, Point};
use crate::point3::{BoundingBox3, Line3, Point3, Points3};

/// `translate(vector)`: each point plus `v`.
pub fn translate(points: &mut [Point3], v: &Point3) {
    for pt in points {
        *pt += *v;
    }
}

/// `translate(x, y, z)`: `translate(Point3(coord_t(x), coord_t(y), coord_t(z)))`, truncated.
pub fn translate_f64(points: &mut [Point3], x: f64, y: f64, z: f64) {
    translate(
        points,
        &Point3::new(
            orca_crt::f64_to_i64(x),
            orca_crt::f64_to_i64(y),
            orca_crt::f64_to_i64(z),
        ),
    );
}

/// `rotate(angle)`: `rotate(cos(angle), sin(angle))`.
pub fn rotate(points: &mut [Point3], angle: f64) {
    rotate_cs(points, orca_crt::cos(angle), orca_crt::sin(angle));
}

/// `rotate(cos_angle, sin_angle)`: x and y of each about the origin, `round`ed; z kept.
pub fn rotate_cs(points: &mut [Point3], cos_angle: f64, sin_angle: f64) {
    for pt in points {
        let cur_x = pt.x as f64;
        let cur_y = pt.y as f64;
        pt.x = round_coord(cos_angle * cur_x - sin_angle * cur_y);
        pt.y = round_coord(cos_angle * cur_y + sin_angle * cur_x);
        // Z kept.
    }
}

/// `rotate(angle, center)`: x and y of each about `center`'s, `round`ed; z kept.
pub fn rotate_around(points: &mut [Point3], angle: f64, center: &Point3) {
    let s = orca_crt::sin(angle);
    let c = orca_crt::cos(angle);
    for pt in points {
        let v = *pt - *center;
        pt.x = round_coord(center.x as f64 + c * v.x as f64 - s * v.y as f64);
        pt.y = round_coord(center.y as f64 + c * v.y as f64 + s * v.x as f64);
        // Z kept from the original point.
    }
}

/// `coord_t(round(v))`.
fn round_coord(v: f64) -> Coord {
    orca_crt::f64_to_i64(orca_crt::round(v))
}

/// `length()`: the sum of the lines' lengths, in order.
#[must_use]
pub fn lines_length(lines: &[Line3]) -> f64 {
    let mut len = 0.0;
    for l in lines {
        len += l.length();
    }
    len
}

/// `bounding_box()`: `BoundingBox3(points)`.
#[must_use]
pub fn bounding_box(points: &[Point3]) -> BoundingBox3 {
    BoundingBox3::from_points(points)
}

/// `find_point(const Point&)`: the index of the first point whose x and y equal `point`, -1 for
/// none.
#[must_use]
pub fn find_point(points: &[Point3], point: Point) -> i32 {
    for (i, pt) in points.iter().enumerate() {
        if pt.to_point() == point {
            return i as i32;
        }
    }
    -1 // not found
}

/// `find_point(const Point&, scaled_epsilon)`: exact for an epsilon of 0; else the first point
/// of least 2D squared distance, if below `scaled_epsilon` squared, else -1.
#[must_use]
pub fn find_point_eps(points: &[Point3], point: Point, scaled_epsilon: f64) -> i32 {
    if scaled_epsilon == 0.0 {
        return find_point(points, point);
    }
    let mut dist2_min = f64::MAX;
    let eps2 = scaled_epsilon * scaled_epsilon;
    let mut idx_min = -1;
    for (i, pt) in points.iter().enumerate() {
        let d2 = (pt.to_point() - point).cast_f64().squared_norm();
        if d2 < dist2_min {
            idx_min = i as i32;
            dist2_min = d2;
        }
    }
    if dist2_min < eps2 { idx_min } else { -1 }
}

/// `find_point(const Point3&)`: the index of the first point equal to `point` in x, y and z, -1
/// for none.
#[must_use]
pub fn find_point3(points: &[Point3], point: &Point3) -> i32 {
    for (i, pt) in points.iter().enumerate() {
        if pt == point {
            return i as i32;
        }
    }
    -1 // not found
}

/// `find_point(const Point3&, scaled_epsilon)`: exact for an epsilon of 0; else the first point
/// of least 3D squared distance, if below `scaled_epsilon` squared, else -1.
#[must_use]
pub fn find_point3_eps(points: &[Point3], point: &Point3, scaled_epsilon: f64) -> i32 {
    if scaled_epsilon == 0.0 {
        return find_point3(points, point);
    }
    let mut dist2_min = f64::MAX;
    let eps2 = scaled_epsilon * scaled_epsilon;
    let mut idx_min = -1;
    for (i, pt) in points.iter().enumerate() {
        let d2 = (*pt - *point).cast_f64().squared_norm();
        if d2 < dist2_min {
            idx_min = i as i32;
            dist2_min = d2;
        }
    }
    if dist2_min < eps2 { idx_min } else { -1 }
}

/// `remove_duplicate_points()`: successive equal points (in x, y and z) kept once; true when any
/// was removed.
pub fn remove_duplicate_points(points: &mut Points3) -> bool {
    let mut j = 0;
    for i in 1..points.len() {
        // An equal point just moves i on.
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

/// `MultiPoint3::_douglas_peucker(points, tolerance)`: the points kept by Douglas-Peucker with
/// the 3D distance to each chord; the first and the last always.
#[must_use]
pub fn douglas_peucker(pts: &[Point3], tolerance: f64) -> Points3 {
    let mut result_pts = Points3::new();
    let tolerance_sq = tolerance * tolerance;
    if !pts.is_empty() {
        let mut anchor_idx = 0;
        let mut floater_idx = pts.len() - 1;
        result_pts.reserve(pts.len());
        result_pts.push(pts[anchor_idx]);
        if anchor_idx != floater_idx {
            let mut dp_stack: Vec<usize> = Vec::with_capacity(pts.len());
            dp_stack.push(floater_idx);
            loop {
                let mut max_dist_sq = 0.0;
                let mut furthest_idx = anchor_idx;
                // The point furthest from the chord (anchor, floater).
                for i in anchor_idx + 1..floater_idx {
                    let dist_sq =
                        Line3::distance_to_squared(&pts[i], &pts[anchor_idx], &pts[floater_idx]);
                    if dist_sq > max_dist_sq {
                        max_dist_sq = dist_sq;
                        furthest_idx = i;
                    }
                }
                // Removed when within the tolerance.
                if max_dist_sq <= tolerance_sq {
                    result_pts.push(pts[floater_idx]);
                    anchor_idx = floater_idx;
                    dp_stack.pop();
                    match dp_stack.last() {
                        None => break,
                        Some(&top) => floater_idx = top,
                    }
                } else {
                    floater_idx = furthest_idx;
                    dp_stack.push(floater_idx);
                }
            }
        }
    }
    result_pts
}
