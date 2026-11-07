// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: class Line and line_alg of src/libslic3r/Line.hpp and Line.cpp of
// OrcaSlicer v2.4.2, translated on 2026-11-07. Copyright the OrcaSlicer authors, the BambuStudio
// authors (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU Affero General
// Public License, version 3 (AGPL-3.0; LICENSE).

//! `Line`: a segment between two points, and the `line_alg` distances and intersection for it.

use orca_eigen::Vec2d;

use crate::geometry::{directions_parallel, directions_perpendicular};
use crate::point::{Coord, EPSILON, PI, Point, cross2, scaled, sqr};

/// `Line`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Line {
    /// `a`.
    pub a: Point,
    /// `b`.
    pub b: Point,
}

/// `Lines`.
pub type Lines = Vec<Line>;

/// `line_alg::distance_to_squared(line, point, &nearest_point)` of a `Line`.
#[must_use]
pub fn distance_to_squared_nearest(a: Point, b: Point, point: Point) -> (f64, Point) {
    let v: Vec2d = (b - a).cast_f64();
    let va: Vec2d = (point - a).cast_f64();
    let l2 = v.squared_norm();
    if l2 == 0.0 {
        return (va.squared_norm(), a);
    }
    let t = va.dot(&v) / l2;
    if t <= 0.0 {
        return (va.squared_norm(), a);
    } else if t >= 1.0 {
        return ((point - b).cast_f64().squared_norm(), b);
    }
    let nearest = Point::cast_from_vec2d(a.cast_f64() + v * t);
    ((v * t - va).squared_norm(), nearest)
}

/// `line_alg::distance_to_infinite_squared(line, point, &closest_point)` of a `Line`.
#[must_use]
pub fn distance_to_infinite_squared_closest(a: Point, b: Point, point: Point) -> (f64, Point) {
    let v: Vec2d = (b - a).cast_f64();
    let va: Vec2d = (point - a).cast_f64();
    let l2 = v.squared_norm();
    if l2 == 0.0 {
        return (va.squared_norm(), a);
    }
    let t = va.dot(&v) / l2;
    let closest = Point::cast_from_vec2d(a.cast_f64() + v * t);
    ((v * t - va).squared_norm(), closest)
}

impl Line {
    /// `Line(a, b)`.
    #[must_use]
    pub const fn new(a: Point, b: Point) -> Self {
        Self { a, b }
    }

    /// `scale(factor)`: each end `*= factor`, truncated.
    pub fn scale(&mut self, factor: f64) {
        self.a.scale_assign(factor);
        self.b.scale_assign(factor);
    }

    /// `translate(v)`.
    pub fn translate(&mut self, v: Point) {
        self.a += v;
        self.b += v;
    }

    /// `translate(x, y)`: by `Point(x, y)`, rounded.
    pub fn translate_f64(&mut self, x: f64, y: f64) {
        self.translate(Point::from_f64(x, y));
    }

    /// `rotate(angle, center)`.
    pub fn rotate(&mut self, angle: f64, center: Point) {
        self.a.rotate_around(angle, center);
        self.b.rotate_around(angle, center);
    }

    /// `reverse()`.
    pub fn reverse(&mut self) {
        core::mem::swap(&mut self.a, &mut self.b);
    }

    /// `length()`.
    #[must_use]
    pub fn length(&self) -> f64 {
        (self.b - self.a).cast_f64().norm()
    }

    /// `midpoint()`: `(a + b) / 2` in integers.
    #[must_use]
    pub fn midpoint(&self) -> Point {
        (self.a + self.b) / 2
    }

    /// `intersection_infinite(other, &point)`: where the two lines meet, `None` when they are
    /// parallel (|cross| below `EPSILON`) or the point is outside `coord_t`.
    #[must_use]
    pub fn intersection_infinite(&self, other: &Line) -> Option<Point> {
        let a1 = self.a.cast_f64();
        let v12 = (other.a - self.a).cast_f64();
        let v1 = (self.b - self.a).cast_f64();
        let v2 = (other.b - other.a).cast_f64();
        let denom = cross2(v1, v2);
        if orca_crt::fabs(denom) < EPSILON {
            return None;
        }
        let t1 = cross2(v12, v2) / denom;
        let result = a1 + v1 * t1;
        let (hi, lo) = (Coord::MAX as f64, Coord::MIN as f64);
        if result.x() > hi || result.x() < lo || result.y() > hi || result.y() < lo {
            return None;
        }
        Some(Point::cast_from_vec2d(result))
    }

    /// `distance_to_squared(point)`.
    #[must_use]
    pub fn distance_to_squared(&self, point: Point) -> f64 {
        distance_to_squared_nearest(self.a, self.b, point).0
    }

    /// `distance_to_squared(point, &closest_point)`.
    #[must_use]
    pub fn distance_to_squared_closest(&self, point: Point) -> (f64, Point) {
        distance_to_squared_nearest(self.a, self.b, point)
    }

    /// `distance_to(point)`: `sqrt` of the squared distance.
    #[must_use]
    pub fn distance_to(&self, point: Point) -> f64 {
        orca_crt::sqrt(self.distance_to_squared(point))
    }

    /// `distance_to_infinite_squared(point, &closest_point)`.
    #[must_use]
    pub fn distance_to_infinite_squared(&self, point: Point) -> (f64, Point) {
        distance_to_infinite_squared_closest(self.a, self.b, point)
    }

    /// `Line::distance_to_infinite(point, a, b)`.
    #[must_use]
    pub fn distance_to_infinite(&self, point: Point) -> f64 {
        orca_crt::sqrt(self.distance_to_infinite_squared(point).0)
    }

    /// `perp_distance_to(point)`: the distance to the infinite line.
    #[must_use]
    pub fn perp_distance_to(&self, point: Point) -> f64 {
        let v = (self.b - self.a).cast_f64();
        let va = (point - self.a).cast_f64();
        if self.a == self.b {
            return va.norm();
        }
        orca_crt::fabs(cross2(v, va)) / v.norm()
    }

    /// `parallel_to(angle)`: `directions_parallel(direction(), angle)`.
    #[must_use]
    pub fn parallel_to_angle(&self, angle: f64) -> bool {
        directions_parallel(self.direction(), angle, 0.0)
    }

    /// `parallel_to(line)`.
    #[must_use]
    pub fn parallel_to(&self, line: &Line) -> bool {
        let v1 = (self.b - self.a).cast_f64();
        let v2 = (line.b - line.a).cast_f64();
        sqr(cross2(v1, v2)) < sqr(EPSILON) * v1.squared_norm() * v2.squared_norm()
    }

    /// `perpendicular_to(angle)`.
    #[must_use]
    pub fn perpendicular_to_angle(&self, angle: f64) -> bool {
        directions_perpendicular(self.direction(), angle, 0.0)
    }

    /// `perpendicular_to(line)`.
    #[must_use]
    pub fn perpendicular_to(&self, line: &Line) -> bool {
        let v1 = (self.b - self.a).cast_f64();
        let v2 = (line.b - line.a).cast_f64();
        sqr(v1.dot(&v2)) < sqr(EPSILON) * v1.squared_norm() * v2.squared_norm()
    }

    /// `atan2_()`.
    #[must_use]
    pub fn atan2_(&self) -> f64 {
        orca_crt::atan2(
            self.b.y.wrapping_sub(self.a.y) as f64,
            self.b.x.wrapping_sub(self.a.x) as f64,
        )
    }

    /// `orientation()`: `atan2_()` in [0, 2π).
    #[must_use]
    pub fn orientation(&self) -> f64 {
        let angle = self.atan2_();
        if angle < 0.0 { 2.0 * PI + angle } else { angle }
    }

    /// `direction()`: `atan2_()` folded into [0, π).
    #[must_use]
    pub fn direction(&self) -> f64 {
        let atan2 = self.atan2_();
        if orca_crt::fabs(atan2 - PI) < EPSILON {
            0.0
        } else if atan2 < 0.0 {
            atan2 + PI
        } else {
            atan2
        }
    }

    /// `vector()`: `b - a`.
    #[must_use]
    pub fn vector(&self) -> Point {
        self.b - self.a
    }

    /// `normal()`: `(b.y - a.y, -(b.x - a.x))`.
    #[must_use]
    pub fn normal(&self) -> Point {
        Point::new(
            self.b.y.wrapping_sub(self.a.y),
            self.b.x.wrapping_sub(self.a.x).wrapping_neg(),
        )
    }

    /// `intersection(line, &point)`: `line_alg::intersection` of the two segments.
    #[must_use]
    pub fn intersection(&self, l2: &Line) -> Option<Point> {
        let v1 = (self.b - self.a).cast_f64();
        let v2 = (l2.b - l2.a).cast_f64();
        let denom = cross2(v1, v2);
        if orca_crt::fabs(denom) < EPSILON {
            return None;
        }
        let v12 = (self.a - l2.a).cast_f64();
        let nume_a = cross2(v2, v12);
        let nume_b = cross2(v1, v12);
        let t1 = nume_a / denom;
        let t2 = nume_b / denom;
        if (0.0..=1.0).contains(&t1) && (0.0..=1.0).contains(&t2) {
            return Some(Point::cast_from_vec2d(self.a.cast_f64() + v1 * t1));
        }
        None
    }

    /// `extend(offset)`: moves both ends out by `offset` along the line.
    pub fn extend(&mut self, offset: f64) {
        let offset_vector = Point::cast_from_vec2d(self.vector().cast_f64().normalized() * offset);
        self.a -= offset_vector;
        self.b += offset_vector;
    }

    /// `overlap(line, &overlap_length)`: the length both share along x when parallel, `None`
    /// otherwise.
    #[must_use]
    pub fn overlap(&self, line: &Line) -> Option<f64> {
        if !self.parallel_to(line) {
            return None;
        }
        let line_ = Line::new(self.a, line.a);
        // `scaled(EPSILON)` is the `coord_t` one.
        if line_.length() > scaled(EPSILON) as f64 && !self.parallel_to(&line_) {
            return None;
        }
        let a_min = orca_stl::min(self.a.x, self.b.x);
        let a_max = orca_stl::max(self.a.x, self.b.x);
        let b_min = orca_stl::min(line.a.x, line.b.x);
        let b_max = orca_stl::max(line.a.x, line.b.x);
        if a_min > b_max || a_max < b_min {
            return None;
        }
        let mut overlap_length =
            orca_stl::max(0, orca_stl::min(a_max, b_max) - orca_stl::max(a_min, b_min)) as f64;
        overlap_length /= (a_max as f64 - a_min as f64) / self.length();
        Some(overlap_length)
    }
}
