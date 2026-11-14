// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: class Point3, Points3, append_points, the free operator* of a Point3,
// lerp of Point3s and the Points/Points3 conversions of src/libslic3r/Point.hpp (65, 90-91,
// 272-382, 426-430) and Point.cpp (266-347); class Line3 of Line.hpp (224-249) with the
// line_alg distance of Line.hpp (22-83) for three dimensions; and class BoundingBox3 of
// BoundingBox.hpp (234-251, with the constructors of BoundingBoxBase and BoundingBox3Base it
// calls), of OrcaSlicer v2.4.2, translated on 2026-11-13. Copyright the OrcaSlicer authors, the
// BambuStudio authors (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU
// Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! `Point3`, `Line3` and `BoundingBox3`: the 3D points of non-planar printing (ZAA), which an
//! `ExtrusionPath`'s `Polyline3` holds.
//!
//! `Point3` is a `Vec3crd` (`Eigen::Matrix<coord_t, 3, 1>`) in C++; here a struct of three `i64`s.
//! Its arithmetic wraps, as MSVC's does, and its casts from `double` go through `orca-crt`
//! (truncation), `std::round`, `sin` and `cos` through the static UCRT's. Most of its methods
//! work on x and y alone and keep z: `rotate`, `ccw`, `nearest_point`, `both_comp` and
//! `any_comp`. Two products by a `double` differ, as in C++: the member `operator*` rounds each
//! coordinate ([`Point3::mul_round`]), the free one and `*=` truncate ([`Point3::mul_trunc`]).
//! `projection_onto(const Line3&)` projects in x and y and takes z along the line by the 2D
//! distance from `a` over the 3D length; `projection_onto(const MultiPoint3&)` always throws
//! "Point3::projection_onto(MultiPoint3) not implemented yet", which is an `Err` of that text here.
//!
//! `BoundingBox3`'s `cwiseMin` and `cwiseMax` are Eigen's: x and y fill one SSE2 packet, whose
//! 64-bit compare takes the low halves signed past 2^31 ([`orca_eigen::pcmp_lt_i64`]); z is
//! compared alone, rightly. Left out: `BoundingBox3(const std::vector<Vec3crd>&)` (the base's
//! iterator constructor), which nothing in domain A builds.

use core::ops::{Add, AddAssign, Neg, Sub, SubAssign};

use orca_eigen::{Vec3crd, Vec3d, Vec3f, Vector};

use crate::line::Line;
use crate::point::{Coord, Coordf, EPSILON, Point, Points, scale_};

/// `coord_t(std::round(v))`.
fn round_coord(v: f64) -> Coord {
    orca_crt::f64_to_i64(orca_crt::round(v))
}

/// `Point3`: a `Vec3crd`. `==` compares x, y and z, as Eigen's does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Point3 {
    /// `x()`.
    pub x: Coord,
    /// `y()`.
    pub y: Coord,
    /// `z()`.
    pub z: Coord,
}

/// `Points3`.
pub type Points3 = Vec<Point3>;

impl Point3 {
    /// `Point3(x, y, z)` of integers (`int32_t` or `int64_t`).
    #[must_use]
    pub const fn new(x: Coord, y: Coord, z: Coord) -> Self {
        Self { x, y, z }
    }

    /// `Point3(double x, double y, double z)`: each `std::round`ed.
    #[must_use]
    pub fn from_f64(x: f64, y: f64, z: f64) -> Self {
        Self::new(round_coord(x), round_coord(y), round_coord(z))
    }

    /// `explicit Point3(const Vec2crd&, coord_t z)`.
    #[must_use]
    pub const fn from_point(p: Point, z: Coord) -> Self {
        Self::new(p.x, p.y, z)
    }

    /// `explicit Point3(const Vec3crd&)`.
    #[must_use]
    pub fn from_vec3crd(v: Vec3crd) -> Self {
        Self::new(v.x(), v.y(), v.z())
    }

    /// A `double` vector `.cast<coord_t>()` made a `Point3`: each coordinate truncated.
    #[must_use]
    pub fn cast_from_vec3d(v: Vec3d) -> Self {
        Self::new(
            orca_crt::f64_to_i64(v.x()),
            orca_crt::f64_to_i64(v.y()),
            orca_crt::f64_to_i64(v.z()),
        )
    }

    /// `Point3::new_scale(x, y, z)`: `coord_t(scale_(x))` of each, truncated.
    #[must_use]
    pub fn new_scale(x: Coordf, y: Coordf, z: Coordf) -> Self {
        Self::new(
            orca_crt::f64_to_i64(scale_(x)),
            orca_crt::f64_to_i64(scale_(y)),
            orca_crt::f64_to_i64(scale_(z)),
        )
    }

    /// `Point3::new_scale(const Vec3d&)`.
    #[must_use]
    pub fn new_scale_vec3d(v: Vec3d) -> Self {
        Self::new_scale(v.x(), v.y(), v.z())
    }

    /// `Point3::new_scale(const Vec3f&)`: `scale_` divides each `float` by the `double`
    /// `SCALING_FACTOR`, so in `double`.
    #[must_use]
    pub fn new_scale_vec3f(v: Vec3f) -> Self {
        Self::new_scale(f64::from(v.x()), f64::from(v.y()), f64::from(v.z()))
    }

    /// `.cast<double>()`.
    #[must_use]
    pub fn cast_f64(self) -> Vec3d {
        Vector([self.x as f64, self.y as f64, self.z as f64])
    }

    /// The point as an Eigen `Vec3crd`.
    #[must_use]
    pub fn to_vec3crd(self) -> Vec3crd {
        Vector([self.x, self.y, self.z])
    }

    /// `operator*=(const double&)`: each coordinate truncated.
    pub fn scale_assign(&mut self, rhs: f64) {
        self.x = orca_crt::f64_to_i64(self.x as f64 * rhs);
        self.y = orca_crt::f64_to_i64(self.y as f64 * rhs);
        self.z = orca_crt::f64_to_i64(self.z as f64 * rhs);
    }

    /// The member `Point3 operator*(const double&) const`: `Point3(x * rhs, y * rhs, z * rhs)`,
    /// the `double` constructor, so each coordinate rounded.
    #[must_use]
    pub fn mul_round(self, rhs: f64) -> Self {
        Self::from_f64(
            self.x as f64 * rhs,
            self.y as f64 * rhs,
            self.z as f64 * rhs,
        )
    }

    /// The free `operator*(const Point3&, const double&)`: `coord_t(x * r)` of each, truncated.
    #[must_use]
    pub fn mul_trunc(self, r: f64) -> Self {
        Self::new(
            orca_crt::f64_to_i64(self.x as f64 * r),
            orca_crt::f64_to_i64(self.y as f64 * r),
            orca_crt::f64_to_i64(self.z as f64 * r),
        )
    }

    /// `both_comp(rhs, op)`: x and y both `>` (or both `<`) `rhs`'s; false for another `op`.
    #[must_use]
    pub fn both_comp(&self, rhs: &Point3, op: &str) -> bool {
        if op == ">" {
            self.x > rhs.x && self.y > rhs.y
        } else if op == "<" {
            self.x < rhs.x && self.y < rhs.y
        } else {
            false
        }
    }

    /// `any_comp(rhs, op)`: x or y `>` (or `<`) `rhs`'s; false for another `op`.
    #[must_use]
    pub fn any_comp(&self, rhs: &Point3, op: &str) -> bool {
        if op == ">" {
            self.x > rhs.x || self.y > rhs.y
        } else if op == "<" {
            self.x < rhs.x || self.y < rhs.y
        } else {
            false
        }
    }

    /// `any_comp(coord_t val, op)`: x or y `>` (or `<`) `val`; false for another `op`.
    #[must_use]
    pub fn any_comp_coord(&self, val: Coord, op: &str) -> bool {
        if op == ">" {
            self.x > val || self.y > val
        } else if op == "<" {
            self.x < val || self.y < val
        } else {
            false
        }
    }

    /// `rotate(angle)`: `rotate(cos(angle), sin(angle))`.
    pub fn rotate(&mut self, angle: f64) {
        self.rotate_cs(orca_crt::cos(angle), orca_crt::sin(angle));
    }

    /// `rotate(cos_a, sin_a)`: x and y about the origin, each `round`ed; z kept.
    pub fn rotate_cs(&mut self, cos_a: f64, sin_a: f64) {
        let cur_x = self.x as f64;
        let cur_y = self.y as f64;
        self.x = round_coord(cos_a * cur_x - sin_a * cur_y);
        self.y = round_coord(cos_a * cur_y + sin_a * cur_x);
    }

    /// `rotate(angle, center)`: the difference from `center` rotated about the origin, then
    /// `center` added back (z with it, unchanged).
    pub fn rotate_around(&mut self, angle: f64, center: &Point3) {
        let diff = *self - *center;
        let mut temp = Point3::new(diff.x, diff.y, diff.z);
        temp.rotate(angle);
        let sum = temp + *center;
        *self = Point3::new(sum.x, sum.y, sum.z);
    }

    /// `rotated(angle)`.
    #[must_use]
    pub fn rotated(self, angle: f64) -> Self {
        let mut res = self;
        res.rotate(angle);
        res
    }

    /// `rotated(cos_a, sin_a)`.
    #[must_use]
    pub fn rotated_cs(self, cos_a: f64, sin_a: f64) -> Self {
        let mut res = self;
        res.rotate_cs(cos_a, sin_a);
        res
    }

    /// `rotated(angle, center)`.
    #[must_use]
    pub fn rotated_around(self, angle: f64, center: &Point3) -> Self {
        let mut res = self;
        res.rotate_around(angle, center);
        res
    }

    /// `rotate_90_degree_ccw()`: `Point3(-y, x, z)`.
    #[must_use]
    pub fn rotate_90_degree_ccw(self) -> Self {
        Self::new(self.y.wrapping_neg(), self.x, self.z)
    }

    /// `nearest_point_index(points)`: [`Point::nearest_point_index`] of x and y.
    #[must_use]
    pub fn nearest_point_index(&self, points: &[Point]) -> i32 {
        self.to_point().nearest_point_index(points)
    }

    /// `nearest_point(points, &point)`: the nearest of the 2D points, with this point's z; `None`
    /// (the out-parameter left alone) when there are none.
    #[must_use]
    pub fn nearest_point(&self, points: &[Point]) -> Option<Point3> {
        self.to_point()
            .nearest_point(points)
            .map(|pt2d| Point3::from_point(pt2d, self.z))
    }

    /// `ccw(p1, p2)`: [`Point::ccw`] of x and y.
    #[must_use]
    pub fn ccw(&self, p1: &Point3, p2: &Point3) -> f64 {
        self.to_point().ccw(p1.to_point(), p2.to_point())
    }

    /// `ccw(const Line3&)`: [`Point::ccw_line`] of x and y.
    #[must_use]
    pub fn ccw_line3(&self, line: &Line3) -> f64 {
        let a2d = Point::new(line.a.x, line.a.y);
        let b2d = Point::new(line.b.x, line.b.y);
        self.to_point().ccw_line(&Line::new(a2d, b2d))
    }

    /// `ccw_angle(p1, p2)`: [`Point::ccw_angle`] of x and y.
    #[must_use]
    pub fn ccw_angle(&self, p1: &Point3, p2: &Point3) -> f64 {
        self.to_point().ccw_angle(p1.to_point(), p2.to_point())
    }

    /// `projection_onto(const MultiPoint3&)`: a stub in C++ that always throws a `RuntimeError`;
    /// here the `Err` of its text.
    ///
    /// # Errors
    /// Always: "Point3::projection_onto(MultiPoint3) not implemented yet".
    pub fn projection_onto_multi_point3(&self, _poly: &[Point3]) -> Result<Point3, String> {
        Err("Point3::projection_onto(MultiPoint3) not implemented yet".to_owned())
    }

    /// `projection_onto(const Line3&)`: the 2D projection onto the segment's x and y
    /// ([`Point::projection_onto_line`]); z interpolated along the line by the 2D distance from
    /// `a` over the line's 3D length (clamped to [0, 1], truncated), or `a`'s z for a line
    /// shorter than `EPSILON`.
    #[must_use]
    pub fn projection_onto_line3(&self, line: &Line3) -> Point3 {
        let pt2d = self.to_point();
        let line_a = Point::new(line.a.x, line.a.y);
        let line_b = Point::new(line.b.x, line.b.y);
        let line2d = Line::new(line_a, line_b);
        let proj2d = pt2d.projection_onto_line(&line2d);

        // The z along the line.
        let line_len = line.length();
        if line_len < EPSILON {
            return Point3::from_point(proj2d, line.a.z);
        }
        let dist_from_a = (proj2d - line_a).cast_f64().norm();
        let mut t = dist_from_a / line_len;
        t = orca_stl::clamp(t, 0.0, 1.0);
        let z = orca_crt::f64_to_i64(line.a.z as f64 + t * line.b.z.wrapping_sub(line.a.z) as f64);
        Point3::from_point(proj2d, z)
    }

    /// `to_point()`: x and y.
    #[must_use]
    pub const fn to_point(&self) -> Point {
        Point::new(self.x, self.y)
    }

    /// `distance_to(point)`: `(point - this).cast<double>().norm()`.
    #[must_use]
    pub fn distance_to(&self, point: &Point3) -> f64 {
        (*point - *self).cast_f64().norm()
    }
}

impl Add for Point3 {
    type Output = Point3;
    fn add(self, o: Point3) -> Point3 {
        Point3::new(
            self.x.wrapping_add(o.x),
            self.y.wrapping_add(o.y),
            self.z.wrapping_add(o.z),
        )
    }
}

impl Sub for Point3 {
    type Output = Point3;
    fn sub(self, o: Point3) -> Point3 {
        Point3::new(
            self.x.wrapping_sub(o.x),
            self.y.wrapping_sub(o.y),
            self.z.wrapping_sub(o.z),
        )
    }
}

impl Neg for Point3 {
    type Output = Point3;
    fn neg(self) -> Point3 {
        Point3::new(
            self.x.wrapping_neg(),
            self.y.wrapping_neg(),
            self.z.wrapping_neg(),
        )
    }
}

/// `operator+=(const Point3&)`.
impl AddAssign for Point3 {
    fn add_assign(&mut self, o: Point3) {
        *self = *self + o;
    }
}

/// `operator-=(const Point3&)`.
impl SubAssign for Point3 {
    fn sub_assign(&mut self, o: Point3) {
        *self = *self - o;
    }
}

/// `append_points(dst, src)`: the x and y of each of `src` appended to `dst`.
pub fn append_points(dst: &mut Points, src: &[Point3]) {
    dst.extend(src.iter().map(Point3::to_point));
}

/// `to_points(points3)`: x and y of each.
#[must_use]
pub fn to_points(points3: &[Point3]) -> Points {
    let mut points2 = Points::with_capacity(points3.len());
    for pt in points3 {
        points2.push(pt.to_point());
    }
    points2
}

/// `to_points3(points)`: each with z 0 (`Point3(const Vec2crd&, coord_t z = 0)`).
#[must_use]
pub fn to_points3(points: &[Point]) -> Points3 {
    let mut points3 = Points3::with_capacity(points.len());
    for &pt in points {
        points3.push(Point3::from_point(pt, 0));
    }
    points3
}

/// `lerp(a, b, t)` of Point3s: `((1 - t) a + t b).cast<coord_t>()`, truncated.
#[must_use]
pub fn lerp(a: &Point3, b: &Point3, t: f64) -> Point3 {
    Point3::cast_from_vec3d((1.0 - t) * a.cast_f64() + t * b.cast_f64())
}

/// `Line3`: a 3D segment from `a` to `b`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Line3 {
    /// `a`.
    pub a: Point3,
    /// `b`.
    pub b: Point3,
}

/// `Lines3`.
pub type Lines3 = Vec<Line3>;

impl Line3 {
    /// `Line3(a, b)` (also `Line3(const Vec3crd&, const Vec3crd&)`).
    #[must_use]
    pub const fn new(a: Point3, b: Point3) -> Self {
        Self { a, b }
    }

    /// `length()`: `(a - b).cast<double>().norm()`.
    #[must_use]
    pub fn length(&self) -> f64 {
        (self.a - self.b).cast_f64().norm()
    }

    /// `vector()`: `b - a`.
    #[must_use]
    pub fn vector(&self) -> Point3 {
        let v = self.b - self.a;
        Point3::new(v.x, v.y, v.z)
    }

    /// `midpoint()`: each coordinate `(a + b) / 2` in `coord_t`, toward zero.
    #[must_use]
    pub fn midpoint(&self) -> Point3 {
        Point3::new(
            self.a.x.wrapping_add(self.b.x).wrapping_div(2),
            self.a.y.wrapping_add(self.b.y).wrapping_div(2),
            self.a.z.wrapping_add(self.b.z).wrapping_div(2),
        )
    }

    /// `to_line()`: x and y of both ends.
    #[must_use]
    pub const fn to_line(&self) -> Line {
        Line::new(self.a.to_point(), self.b.to_point())
    }

    /// `Line3::distance_to_squared(point, a, b)`: the squared distance from `point` to the
    /// segment ([`distance_to_squared_nearest`]).
    #[must_use]
    pub fn distance_to_squared(point: &Point3, a: &Point3, b: &Point3) -> f64 {
        distance_to_squared_nearest(&Line3::new(*a, *b), point).0
    }
}

/// `line_alg::distance_to_squared(line, point, &nearest_point)` of a `Line3`: the squared
/// distance from `point` to the segment and its nearest point, in `double`s (Eigen's packet sums
/// for the dot product and the squared norms), the nearest point inside the segment truncated.
#[must_use]
pub fn distance_to_squared_nearest(line: &Line3, point: &Point3) -> (f64, Point3) {
    let v = (line.b - line.a).cast_f64();
    let va = (*point - line.a).cast_f64();
    let l2 = v.squared_norm(); // avoid a sqrt
    if l2 == 0.0 {
        // a == b
        return (va.squared_norm(), line.a);
    }
    // The projection onto the line a + t (b - a): t = [(this - a) . (b - a)] / |b - a|^2.
    let t = va.dot(&v) / l2;
    if t <= 0.0 {
        // Beyond the 'a' end of the segment.
        return (va.squared_norm(), line.a);
    } else if t >= 1.0 {
        // Beyond the 'b' end of the segment.
        return ((*point - line.b).cast_f64().squared_norm(), line.b);
    }
    let nearest_point = Point3::cast_from_vec3d(line.a.cast_f64() + t * v);
    ((t * v - va).squared_norm(), nearest_point)
}

/// `line_alg::distance_to(line, point)` of a `Line3`: `sqrt` of the squared distance.
#[must_use]
pub fn distance_to(line: &Line3, point: &Point3) -> f64 {
    orca_crt::sqrt(distance_to_squared_nearest(line, point).0)
}

/// `BoundingBox3`: `BoundingBox3Base<Vec3crd>`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoundingBox3 {
    /// `min`.
    pub min: Vec3crd,
    /// `max`.
    pub max: Vec3crd,
    /// `defined`.
    pub defined: bool,
}

impl Default for BoundingBox3 {
    fn default() -> Self {
        Self::new()
    }
}

impl BoundingBox3 {
    /// `BoundingBox3()`: zero corners, not defined.
    #[must_use]
    pub fn new() -> Self {
        Self {
            min: Vec3crd::zero(),
            max: Vec3crd::zero(),
            defined: false,
        }
    }

    /// `BoundingBox3(pmin, pmax)`: defined when `pmin` is below `pmax` in x, y and z.
    #[must_use]
    pub fn from_corners(pmin: Vec3crd, pmax: Vec3crd) -> Self {
        let mut defined = pmin.x() < pmax.x() && pmin.y() < pmax.y();
        if pmin.z() >= pmax.z() {
            defined = false;
        }
        Self {
            min: pmin,
            max: pmax,
            defined,
        }
    }

    /// `BoundingBox3(const Points3&)`: from the first point, `cwiseMin` and `cwiseMax` with each
    /// (the first again); defined for any point, a single one included. None leaves it the
    /// default.
    #[must_use]
    pub fn from_points(points: &[Point3]) -> Self {
        let mut bb = Self::new();
        if let Some(front) = points.first() {
            bb.min = front.to_vec3crd();
            bb.max = front.to_vec3crd();
            for p in points {
                bb.min = bb.min.cwise_min(&p.to_vec3crd());
                bb.max = bb.max.cwise_max(&p.to_vec3crd());
            }
            bb.defined = true;
        }
        bb
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_products_differ() {
        let p = Point3::new(3, -3, 5);
        assert_eq!(p.mul_round(0.5), Point3::new(2, -2, 3));
        assert_eq!(p.mul_trunc(0.5), Point3::new(1, -1, 2));
        let mut q = p;
        q.scale_assign(0.5);
        assert_eq!(q, Point3::new(1, -1, 2));
    }

    #[test]
    fn projection_onto_a_line_takes_z_along_it() {
        let l = Line3::new(Point3::new(0, 0, 0), Point3::new(100, 0, 100));
        let p = Point3::new(50, 10, 7).projection_onto_line3(&l);
        // The 2D distance from a over the 3D length: 50 / 141.42..., times 100.
        assert_eq!(p, Point3::new(50, 0, 35));
        let short = Line3::new(Point3::new(1, 1, 9), Point3::new(1, 1, 9));
        assert_eq!(
            Point3::new(5, 5, 0).projection_onto_line3(&short),
            Point3::new(1, 1, 9)
        );
    }

    #[test]
    fn a_box_of_one_point_is_defined() {
        let bb = BoundingBox3::from_points(&[Point3::new(1, 2, 3)]);
        assert!(bb.defined);
        assert_eq!(bb.min, Vector([1, 2, 3]));
        assert!(!BoundingBox3::from_points(&[]).defined);
        assert!(!BoundingBox3::from_corners(Vector([0, 0, 1]), Vector([1, 1, 1])).defined);
    }
}
