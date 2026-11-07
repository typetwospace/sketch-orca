// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Point.hpp, Point.cpp and the scaling constants of
// libslic3r.h of OrcaSlicer v2.4.2, translated on 2026-11-07. Copyright the OrcaSlicer authors,
// the BambuStudio authors (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU
// Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! `coord_t`, `Point` and the scaling between millimetres and the integer grid.
//!
//! `Point` is `Eigen::Matrix<coord_t, 2, 1>` in C++, and Clipper's `IntPoint`; here a struct of two
//! `i64`s with the operations libslic3r uses, each written as the C++ evaluates it. The C++
//! integer arithmetic wraps on overflow under MSVC, so the operators here wrap too; casts from
//! `double` go through `orca-crt` (MSVC's truncation), and `std::round`, `sin`, `cos`, `atan2` and
//! `sqrt` through the static UCRT's.
//!
//! `SCALING_FACTOR` is a variable in C++ that the command line always leaves at 1e-6, so
//! `scale_(v)` is a division by 1e-6, never a multiplication by 1e6.

use core::ops::{Add, AddAssign, Div, Neg, Sub, SubAssign};

use orca_eigen::{Scalar, Vec2crd, Vec2d, Vec3d, Vector};

use crate::int128 as i128p;
use crate::line::Line;

/// `coord_t`.
pub type Coord = i64;
/// `coordf_t`.
pub type Coordf = f64;

/// `EPSILON`.
pub const EPSILON: f64 = 1e-4;
/// `SCALING_FACTOR`: the command line's 1e-6 (`SCALING_FACTOR_INTERNAL`).
pub const SCALING_FACTOR: f64 = 0.000001;
/// `PI`, written `3.141592653589793238`, which rounds to this `double`.
pub const PI: f64 = core::f64::consts::PI;
/// `RESOLUTION` in millimetres.
pub const RESOLUTION: f64 = 0.0125;
/// `SCALED_RESOLUTION`: `RESOLUTION / SCALING_FACTOR`.
pub const SCALED_RESOLUTION: f64 = RESOLUTION / SCALING_FACTOR;
/// `SCALED_EPSILON`: `scale_(EPSILON)`.
pub const SCALED_EPSILON: f64 = EPSILON / SCALING_FACTOR;
/// `INSET_OVERLAP_TOLERANCE`.
pub const INSET_OVERLAP_TOLERANCE: f64 = 0.4;

/// `scale_(val)`: `val / SCALING_FACTOR`.
#[must_use]
pub fn scale_(val: f64) -> f64 {
    val / SCALING_FACTOR
}

/// `unscale_(val)`: `val * SCALING_FACTOR`.
#[must_use]
pub fn unscale_(val: f64) -> f64 {
    val * SCALING_FACTOR
}

/// `unscale<double>(v)` and `unscaled<double>(v)` of a `coord_t`: `double(v) * SCALING_FACTOR`.
#[must_use]
pub fn unscale(v: Coord) -> f64 {
    v as f64 * SCALING_FACTOR
}

/// `unscaled<double>(v)` of a `double`.
#[must_use]
pub fn unscaled_f64(v: f64) -> f64 {
    v * SCALING_FACTOR
}

/// `scaled<coord_t>(v)`: `coord_t(v / SCALING_FACTOR)`, truncated.
#[must_use]
pub fn scaled(v: f64) -> Coord {
    orca_crt::f64_to_i64(v / SCALING_FACTOR)
}

/// `scaled<double>(v)`: `v / SCALING_FACTOR`.
#[must_use]
pub fn scaled_f64(v: f64) -> f64 {
    v / SCALING_FACTOR
}

/// `scaled<float>(v)` of a `double`: `float(v / SCALING_FACTOR)`.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn scaled_f32(v: f64) -> f32 {
    (v / SCALING_FACTOR) as f32
}

/// `sqr(x)`: `x * x`.
#[must_use]
pub fn sqr(x: f64) -> f64 {
    x * x
}

/// `fast_round_up<I>(a)`: `floor(a + 0.5)`, except that the `double` just below 0.5 gives 0.
#[must_use]
pub fn fast_round_up(a: f64) -> Coord {
    if a == 0.499_999_999_999_999_94 {
        0
    } else {
        orca_crt::f64_to_i64(orca_crt::floor(a + 0.5))
    }
}

/// `coord_t(std::round(v))`.
fn round_coord(v: f64) -> Coord {
    orca_crt::f64_to_i64(orca_crt::round(v))
}

/// A `Vec2d` of two `double`s.
#[must_use]
pub fn vec2d(x: f64, y: f64) -> Vec2d {
    Vector([x, y])
}

/// `Point`: `Vec2crd`. `Ord` is `operator<`: x, then y.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Point {
    /// `x()`.
    pub x: Coord,
    /// `y()`.
    pub y: Coord,
}

/// `Points`.
pub type Points = Vec<Point>;
/// `Vector`: a `Point` used as a vector.
pub type Vector2 = Point;
/// `Pointfs` and `Vec2ds`.
pub type Pointfs = Vec<Vec2d>;
/// `Pointf3s`.
pub type Pointf3s = Vec<Vec3d>;

impl Point {
    /// `Point(x, y)` of integers.
    #[must_use]
    pub const fn new(x: Coord, y: Coord) -> Self {
        Self { x, y }
    }

    /// `Point(double x, double y)`: each `std::round`ed.
    #[must_use]
    pub fn from_f64(x: f64, y: f64) -> Self {
        Self::new(round_coord(x), round_coord(y))
    }

    /// `explicit Point(const Vec2d&)`: each coordinate `std::round`ed.
    #[must_use]
    pub fn from_vec2d(v: Vec2d) -> Self {
        Self::from_f64(v.x(), v.y())
    }

    /// A `double` vector `.cast<coord_t>()`: each coordinate truncated.
    #[must_use]
    pub fn cast_from_vec2d(v: Vec2d) -> Self {
        Self::new(orca_crt::f64_to_i64(v.x()), orca_crt::f64_to_i64(v.y()))
    }

    /// `Point::new_scale(x, y)`: `coord_t(scale_(x))`, truncated.
    #[must_use]
    pub fn new_scale(x: Coordf, y: Coordf) -> Self {
        Self::new(scaled(x), scaled(y))
    }

    /// `.cast<double>()`.
    #[must_use]
    pub fn cast_f64(self) -> Vec2d {
        vec2d(self.x as f64, self.y as f64)
    }

    /// The point as an Eigen `Vec2crd`.
    #[must_use]
    pub fn to_vec2crd(self) -> Vec2crd {
        Vector([self.x, self.y])
    }

    /// The member `Point operator*(const double&)`, which non-const points pick: `Point(x * rhs,
    /// y * rhs)`, so rounded.
    #[must_use]
    pub fn mul_round(self, rhs: f64) -> Self {
        Self::from_f64(self.x as f64 * rhs, self.y as f64 * rhs)
    }

    /// The free `operator*(const Point&, const double&)`, which const points pick: `coord_t(x *
    /// r)`, truncated.
    #[must_use]
    pub fn mul_trunc(self, rhs: f64) -> Self {
        Self::new(
            orca_crt::f64_to_i64(self.x as f64 * rhs),
            orca_crt::f64_to_i64(self.y as f64 * rhs),
        )
    }

    /// `operator*=(const double&)`: truncated.
    pub fn scale_assign(&mut self, rhs: f64) {
        *self = self.mul_trunc(rhs);
    }

    /// `rotate(angle)`: `rotate(cos(angle), sin(angle))`.
    pub fn rotate(&mut self, angle: f64) {
        self.rotate_cs(orca_crt::cos(angle), orca_crt::sin(angle));
    }

    /// `rotate(cos_a, sin_a)`: each coordinate `round`ed.
    pub fn rotate_cs(&mut self, cos_a: f64, sin_a: f64) {
        let cur_x = self.x as f64;
        let cur_y = self.y as f64;
        self.x = round_coord(cos_a * cur_x - sin_a * cur_y);
        self.y = round_coord(cos_a * cur_y + sin_a * cur_x);
    }

    /// `rotate(angle, center)`: about `center`, each coordinate by `fast_round_up`.
    pub fn rotate_around(&mut self, angle: f64, center: Point) {
        let cur = self.cast_f64();
        let s = orca_crt::sin(angle);
        let c = orca_crt::cos(angle);
        let d = cur - center.cast_f64();
        self.x = fast_round_up(center.x as f64 + c * d.x() - s * d.y());
        self.y = fast_round_up(center.y as f64 + s * d.x() + c * d.y());
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
    pub fn rotated_around(self, angle: f64, center: Point) -> Self {
        let mut res = self;
        res.rotate_around(angle, center);
        res
    }

    /// `rotate_90_degree_ccw()`.
    #[must_use]
    pub fn rotate_90_degree_ccw(self) -> Self {
        Self::new(self.y.wrapping_neg(), self.x)
    }

    /// `nearest_point_index(points)`: the index of the nearest point, -1 for none; stops at the
    /// first whose squared distance is below `EPSILON`.
    #[must_use]
    pub fn nearest_point_index(&self, points: &[Point]) -> i32 {
        let mut idx = -1_i32;
        let mut distance = -1.0_f64;
        for (i, p) in points.iter().enumerate() {
            let mut d = sqr(self.x.wrapping_sub(p.x) as f64);
            if distance != -1.0 && d > distance {
                continue;
            }
            d += sqr(self.y.wrapping_sub(p.y) as f64);
            if distance != -1.0 && d > distance {
                continue;
            }
            idx = i as i32;
            distance = d;
            if distance < EPSILON {
                break;
            }
        }
        idx
    }

    /// `nearest_point(points, &point)`: the nearest point, `None` for none.
    #[must_use]
    pub fn nearest_point(&self, points: &[Point]) -> Option<Point> {
        let idx = self.nearest_point_index(points);
        if idx == -1 {
            None
        } else {
            Some(points[idx as usize])
        }
    }

    /// `ccw(p1, p2)`: `cross2(p2 - p1, this - p1)` in `double`s, positive for a counter-clockwise
    /// turn.
    #[must_use]
    pub fn ccw(&self, p1: Point, p2: Point) -> f64 {
        cross2((p2 - p1).cast_f64(), (*self - p1).cast_f64())
    }

    /// `ccw(line)`.
    #[must_use]
    pub fn ccw_line(&self, line: &Line) -> f64 {
        self.ccw(line.a, line.b)
    }

    /// `ccw_angle(p1, p2)`: the counter-clockwise angle from `this - p1` to `this - p2`, in
    /// (0, 2π].
    #[must_use]
    pub fn ccw_angle(&self, p1: Point, p2: Point) -> f64 {
        let angle = orca_crt::atan2(
            p1.x.wrapping_sub(self.x) as f64,
            p1.y.wrapping_sub(self.y) as f64,
        ) - orca_crt::atan2(
            p2.x.wrapping_sub(self.x) as f64,
            p2.y.wrapping_sub(self.y) as f64,
        );
        if angle <= 0.0 {
            angle + 2.0 * PI
        } else {
            angle
        }
    }

    /// `projection_onto(const MultiPoint&)`, given the first point and the lines of the
    /// MultiPoint: the nearest point on the lines, starting from the first point.
    #[must_use]
    pub fn projection_onto_lines(&self, first_point: Point, lines: &[Line]) -> Point {
        let mut running_projection = first_point;
        let mut running_min = (running_projection - *self).cast_f64().norm();
        for line in lines {
            let point_temp = self.projection_onto_line(line);
            if (point_temp - *self).cast_f64().norm() < running_min {
                running_projection = point_temp;
                running_min = (running_projection - *self).cast_f64().norm();
            }
        }
        running_projection
    }

    /// `projection_onto(line)`: the nearest point of the segment.
    #[must_use]
    pub fn projection_onto_line(&self, line: &Line) -> Point {
        if line.a == line.b {
            return line.a;
        }
        let lx = line.b.x.wrapping_sub(line.a.x) as f64;
        let ly = line.b.y.wrapping_sub(line.a.y) as f64;
        let theta = (line.b.x.wrapping_sub(self.x) as f64 * lx
            + line.b.y.wrapping_sub(self.y) as f64 * ly)
            / (sqr(lx) + sqr(ly));
        if (0.0..=1.0).contains(&theta) {
            return Point::cast_from_vec2d(
                line.a.cast_f64() * theta + line.b.cast_f64() * (1.0 - theta),
            );
        }
        if (line.a - *self).cast_f64().squared_norm() < (line.b - *self).cast_f64().squared_norm() {
            line.a
        } else {
            line.b
        }
    }

    /// `distance_to(point)`: `(point - this).cast<double>().norm()`.
    #[must_use]
    pub fn distance_to(&self, point: Point) -> f64 {
        (point - *self).cast_f64().norm()
    }

    /// `cwiseAbs()`.
    #[must_use]
    pub fn cwise_abs(self) -> Self {
        Self::new(self.x.wrapping_abs(), self.y.wrapping_abs())
    }

    /// `cwiseMin(o)`: both coordinates fill one SSE2 packet, so `pmin<Packet2l>` picks each, by
    /// a compare that takes the low 32-bit halves signed ([`orca_eigen::pcmp_lt_i64`]): past 2^31
    /// the larger coordinate can win.
    #[must_use]
    pub fn cwise_min(self, o: Point) -> Self {
        Self::new(self.x.packet_min(o.x), self.y.packet_min(o.y))
    }

    /// `cwiseMax(o)`: `pmax<Packet2l>` of each coordinate, as [`Point::cwise_min`].
    #[must_use]
    pub fn cwise_max(self, o: Point) -> Self {
        Self::new(self.x.packet_max(o.x), self.y.packet_max(o.y))
    }

    /// `dot(other)` in `coord_t`.
    #[must_use]
    pub fn dot(self, o: Point) -> Coord {
        self.x
            .wrapping_mul(o.x)
            .wrapping_add(self.y.wrapping_mul(o.y))
    }

    /// `squaredNorm()` in `coord_t`.
    #[must_use]
    pub fn squared_norm(self) -> Coord {
        self.dot(self)
    }
}

/// `operator/(coord_t)`: integer division of each coordinate, toward zero.
impl Div<Coord> for Point {
    type Output = Self;

    fn div(self, d: Coord) -> Self {
        Self::new(self.x.wrapping_div(d), self.y.wrapping_div(d))
    }
}

impl Add for Point {
    type Output = Point;
    fn add(self, o: Point) -> Point {
        Point::new(self.x.wrapping_add(o.x), self.y.wrapping_add(o.y))
    }
}

impl Sub for Point {
    type Output = Point;
    fn sub(self, o: Point) -> Point {
        Point::new(self.x.wrapping_sub(o.x), self.y.wrapping_sub(o.y))
    }
}

impl Neg for Point {
    type Output = Point;
    fn neg(self) -> Point {
        Point::new(self.x.wrapping_neg(), self.y.wrapping_neg())
    }
}

impl AddAssign for Point {
    fn add_assign(&mut self, o: Point) {
        *self = *self + o;
    }
}

impl SubAssign for Point {
    fn sub_assign(&mut self, o: Point) {
        *self = *self - o;
    }
}

/// `cross2(v1, v2)` of `double` vectors: `v1.x * v2.y - v1.y * v2.x`.
#[must_use]
pub fn cross2(v1: Vec2d, v2: Vec2d) -> f64 {
    v1.x() * v2.y() - v1.y() * v2.x()
}

/// `cross2(v1, v2)` of `coord_t` vectors, in `coord_t` (wrapping).
#[must_use]
pub fn cross2_coord(v1: Point, v2: Point) -> Coord {
    v1.x.wrapping_mul(v2.y)
        .wrapping_sub(v1.y.wrapping_mul(v2.x))
}

/// `perp(v)`: `(-v.y, v.x)`.
#[must_use]
pub fn perp(v: Vec2d) -> Vec2d {
    vec2d(-v.y(), v.x())
}

/// `angle(v1, v2)`: `atan2(cross2(v1, v2), v1.dot(v2))`, from v1 to v2 in [-π, π].
#[must_use]
pub fn angle(v1: Vec2d, v2: Vec2d) -> f64 {
    orca_crt::atan2(cross2(v1, v2), v1.dot(&v2))
}

/// `unscale(pt)` and `unscaled(pt)` of a `Point`: each coordinate times `SCALING_FACTOR`.
#[must_use]
pub fn unscale_point(pt: Point) -> Vec2d {
    vec2d(unscale(pt.x), unscale(pt.y))
}

/// `scaled(v)` of a `Vec2d`: `(v / SCALING_FACTOR).cast<coord_t>()`, truncated.
#[must_use]
pub fn scaled_point(v: Vec2d) -> Point {
    Point::new(scaled(v.x()), scaled(v.y()))
}

/// `is_approx(p1, p2, epsilon)` of points: both coordinates of `|p2 - p1|` below `epsilon`
/// (`coord_t(SCALED_EPSILON)` by default).
#[must_use]
pub fn is_approx_point(p1: Point, p2: Point, epsilon: Coord) -> bool {
    let d = (p2 - p1).cwise_abs();
    d.x < epsilon && d.y < epsilon
}

/// `lerp(a, b, t)` of points: `((1 - t) a + t b).cast<coord_t>()`, truncated.
#[must_use]
pub fn lerp_point(a: Point, b: Point, t: f64) -> Point {
    Point::cast_from_vec2d(a.cast_f64() * (1.0 - t) + b.cast_f64() * t)
}

/// `align_to_grid(coord, spacing)`: rounded down to a multiple of `spacing`.
#[must_use]
pub fn align_to_grid(coord: Coord, spacing: Coord) -> Coord {
    if coord < 0 {
        ((coord - spacing + 1) / spacing) * spacing
    } else {
        (coord / spacing) * spacing
    }
}

/// `has_duplicate_points(pts)`: sorts a copy (`std::sort`) and looks for equal neighbours.
#[must_use]
pub fn has_duplicate_points(pts: &[Point]) -> bool {
    let mut pts = pts.to_vec();
    orca_stl::sort(&mut pts);
    pts.windows(2).any(|w| w[0] == w[1])
}

/// `has_duplicate_successive_points(pts)`.
#[must_use]
pub fn has_duplicate_successive_points(pts: &[Point]) -> bool {
    pts.windows(2).any(|w| w[0] == w[1])
}

/// `has_duplicate_successive_points_closed(pts)`: also the last against the first.
#[must_use]
pub fn has_duplicate_successive_points_closed(pts: &[Point]) -> bool {
    has_duplicate_successive_points(pts) || (pts.len() >= 2 && pts[0] == pts[pts.len() - 1])
}

/// `collect_duplicates(pts)`: each point that occurs more than once, once, in sorted order. The
/// C++ reads the first point of an empty vector; here an empty vector gives none.
#[must_use]
pub fn collect_duplicates(pts: &[Point]) -> Points {
    let mut pts = pts.to_vec();
    orca_stl::sort(&mut pts);
    let mut duplicits = Points::new();
    let Some(&first) = pts.first() else {
        return duplicits;
    };
    let mut prev = first;
    for &act in &pts[1..] {
        if prev == act {
            if duplicits.last() == Some(&act) {
                continue;
            }
            duplicits.push(act);
        }
        prev = act;
    }
    duplicits
}

/// The `Slic3r::int128` predicates of `Point.cpp`.
pub mod int128 {
    use super::{Point, i128p};

    /// `int128::orient(p1, p2, p3)`: the sign of `cross2(p2 - p1, p3 - p1)`, exact.
    #[must_use]
    pub fn orient(p1: Point, p2: Point, p3: Point) -> i32 {
        let v1 = p2 - p1;
        let v2 = p3 - p1;
        i128p::sign_determinant_2x2_filtered(v1.x, v1.y, v2.x, v2.y)
    }

    /// `int128::cross(v1, v2)`: the sign of `cross2(v1, v2)`, exact.
    #[must_use]
    pub fn cross(v1: Point, v2: Point) -> i32 {
        i128p::sign_determinant_2x2_filtered(v1.x, v1.y, v2.x, v2.y)
    }
}
