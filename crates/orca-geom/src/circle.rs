// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: class Circle, enum ArcDirection and class ArcSegment of
// src/libslic3r/Circle.hpp (1-30, 32-114, 122-123, 125-144) and Circle.cpp (1-97, 103-300,
// 326-489, 571-629) of OrcaSlicer v2.4.2, translated on 2026-11-13; and their Points3 forms,
// Circle.hpp 31, 115-121 and 124 and Circle.cpp 98-102, 301-325 and 490-570, translated on
// 2026-11-13 (geom-2). Copyright the OrcaSlicer authors and the BambuStudio authors (Bambu Lab),
// whose arc fitting follows ArcWelderLib, under the GNU Affero General Public License, version 3
// (AGPL-3.0; LICENSE).

//! `Circle` and `ArcSegment`: the circles and arcs of the arc fitting (`ArcFitter`), which turns
//! runs of an extrusion path's points into G2 and G3 moves.
//!
//! A circle comes from three points (`try_create_circle`, the circumcircle, refused when the
//! points are nearly collinear or the radius passes the maximum) or from a run of points (the
//! first, the middle and the last; else the first, the last and whichever point between gives the
//! least squared deviation). An arc is a circle with a start, an end and a direction, checked
//! against the length of the points it replaces and against the angles of its points
//! (`are_points_within_slice`, which tests only the last two points, as the C++ loop starts
//! there).
//!
//! C++ out-parameters that are only written on success become `Option`s; `try_create_circle`
//! keeps its `&mut Circle`, as a failed fit of a point list can leave it changed. The `Points3`
//! forms (`try_create_circle_points3`, `try_create_arc_points3`,
//! `are_points_within_slice_points3`) take the points' x and y and give what the 2D forms give
//! for them.
//!
//! Inputs the C++ reads outside its arrays for are not given a meaning here (`parity/sop1/paths.md`,
//! "Edges"): `try_create_circle` of fewer than two points, `is_over_deviation` and
//! `get_deviation_sum_squared` of none, `ArcSegment::try_create_arc` of fewer than two and
//! `are_points_within_slice` of fewer than three (of `Point`s or of `Point3`s) panic, naming the
//! call.

use orca_eigen::{Vec2d, Vec3f, Vector};

use crate::line::Line;
use crate::point::{EPSILON, PI, Point, SCALED_EPSILON, scale_, vec2d};
use crate::point3::{Point3, to_points};

/// `ZERO_TOLERANCE`: the default tolerance of [`Circle::is_equal`] and the comparisons after it.
pub const ZERO_TOLERANCE: f64 = 0.000005;

/// `Parallel_area_threshold`: the area (in mm², scaled twice) below which three points count as
/// collinear.
const PARALLEL_AREA_THRESHOLD: f64 = 0.0001;

/// `DEFAULT_SCALED_MAX_RADIUS`: `scale_(2000)`, 2000 mm.
#[must_use]
pub fn default_scaled_max_radius() -> f64 {
    scale_(2000.0)
}

/// `DEFAULT_SCALED_RESOLUTION`: `scale_(0.05)`, 0.05 mm.
#[must_use]
pub fn default_scaled_resolution() -> f64 {
    scale_(0.05)
}

/// `DEFAULT_ARC_LENGTH_PERCENT_TOLERANCE`: 5 percent.
pub const DEFAULT_ARC_LENGTH_PERCENT_TOLERANCE: f64 = 0.05;

/// Stops on an input the C++ reads outside its arrays for (`parity/sop1/paths.md`, "Edges").
fn undefined(what: &str) -> ! {
    panic!("{what}: undefined in the C++ engine (it reads outside its arrays), not ported")
}

/// `Circle`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Circle {
    /// `center`.
    pub center: Point,
    /// `radius`.
    pub radius: f64,
}

impl Circle {
    /// `Circle(p, r)`.
    #[must_use]
    pub fn new(center: Point, radius: f64) -> Self {
        Self { center, radius }
    }

    /// `get_closest_point(input)`: the point of the circle in the direction of `input`, the step
    /// from the center truncated to `coord_t`.
    #[must_use]
    pub fn get_closest_point(&self, input: Point) -> Point {
        let v = (input - self.center).cast_f64().normalized();
        self.center + Point::cast_from_vec2d(v * self.radius)
    }

    /// `try_create_circle(p1, p2, p3, max_radius, new_circle)`: the circumcircle of three points,
    /// written to `new_circle` only on success; false when they are nearly collinear (twice the
    /// triangle's area at most `scale_(scale_(0.0001))`), when the determinant is below
    /// `SCALED_EPSILON` or when the radius passes `max_radius`.
    pub fn try_create_circle3(
        p1: Point,
        p2: Point,
        p3: Point,
        max_radius: f64,
        new_circle: &mut Circle,
    ) -> bool {
        let x1 = p1.x as f64;
        let y1 = p1.y as f64;
        let x2 = p2.x as f64;
        let y2 = p2.y as f64;
        let x3 = p3.x as f64;
        let y3 = p3.y as f64;

        // The area of the triangle, scaled twice as the points are scaled once.
        if orca_crt::fabs((y1 - y2) * (x1 - x3) - (y1 - y3) * (x1 - x2))
            <= scale_(scale_(PARALLEL_AREA_THRESHOLD))
        {
            return false;
        }

        let a = x1 * (y2 - y3) - y1 * (x2 - x3) + x2 * y3 - x3 * y2;
        if orca_crt::fabs(a) < SCALED_EPSILON {
            return false;
        }

        let b = (x1 * x1 + y1 * y1) * (y3 - y2)
            + (x2 * x2 + y2 * y2) * (y1 - y3)
            + (x3 * x3 + y3 * y3) * (y2 - y1);

        let c = (x1 * x1 + y1 * y1) * (x2 - x3)
            + (x2 * x2 + y2 * y2) * (x3 - x1)
            + (x3 * x3 + y3 * y3) * (x1 - x2);

        let center_x = -b / (2.0 * a);
        let center_y = -c / (2.0 * a);

        let delta_x = center_x - x1;
        let delta_y = center_y - y1;
        let radius = orca_crt::sqrt(delta_x * delta_x + delta_y * delta_y);
        if radius > max_radius {
            return false;
        }

        new_circle.center = Point::from_f64(center_x, center_y);
        new_circle.radius = radius;
        true
    }

    /// `try_create_circle(points, max_radius, tolerance, new_circle)`: the circle through the
    /// first, the middle and the last point (for three points, those three; for an even count, the
    /// middle is the mean of the two middle points, for an odd one of the middle point's two
    /// neighbours) when no point deviates past `tolerance`; else, of the circles through the
    /// first, the last and each point between (the middle index skipped), the one of least summed
    /// squared deviation. `new_circle` may be changed even when this returns false.
    ///
    /// # Panics
    /// Below two points, which the C++ reads outside the list for.
    pub fn try_create_circle(
        points: &[Point],
        max_radius: f64,
        tolerance: f64,
        new_circle: &mut Circle,
    ) -> bool {
        let count = points.len();
        if count < 2 {
            undefined("Circle::try_create_circle of fewer than two points");
        }
        let middle_index = count / 2;
        // The middle point will almost always give the best arc.
        if count == 3 {
            return Circle::try_create_circle3(
                points[0],
                points[middle_index],
                points[count - 1],
                max_radius,
                new_circle,
            ) && !new_circle.is_over_deviation(points, tolerance);
        }
        let middle_point = if count.is_multiple_of(2) {
            (points[middle_index] + points[middle_index - 1]) / 2
        } else {
            (points[middle_index - 1] + points[middle_index + 1]) / 2
        };
        if Circle::try_create_circle3(
            points[0],
            middle_point,
            points[count - 1],
            max_radius,
            new_circle,
        ) && !new_circle.is_over_deviation(points, tolerance)
        {
            return true;
        }

        // The circle with the least deviation, if one exists.
        let mut test_circle = Circle::default();
        let mut least_deviation = 0.0;
        let mut found_circle = false;
        for index in 1..count - 1 {
            if index == middle_index {
                // Already checked, and it failed.
                continue;
            }
            if Circle::try_create_circle3(
                points[0],
                points[index],
                points[count - 1],
                max_radius,
                &mut test_circle,
            ) {
                let (fits, current_deviation) =
                    test_circle.get_deviation_sum_squared(points, tolerance);
                if fits && (!found_circle || current_deviation < least_deviation) {
                    found_circle = true;
                    least_deviation = current_deviation;
                    *new_circle = test_circle;
                }
            }
        }
        found_circle
    }

    /// `get_polar_radians(p1)`: the angle of `p1` about the center in `[0, 2π)`, from `atan2` of
    /// the differences taken in `coord_t`.
    #[must_use]
    pub fn get_polar_radians(&self, p1: Point) -> f64 {
        let mut polar_radians = orca_crt::atan2(
            p1.y.wrapping_sub(self.center.y) as f64,
            p1.x.wrapping_sub(self.center.x) as f64,
        );
        if polar_radians < 0.0 {
            polar_radians += 2.0 * PI;
        }
        polar_radians
    }

    /// `is_over_deviation(points, tolerance)`: true when a point after the first, or the foot of
    /// the center on a segment, lies farther than `tolerance` from the circle.
    ///
    /// # Panics
    /// On no points, which the C++ counts from `size_t(-1)` for.
    #[must_use]
    pub fn is_over_deviation(&self, points: &[Point], tolerance: f64) -> bool {
        if points.is_empty() {
            undefined("Circle::is_over_deviation of no points");
        }
        // The first and last points fit by construction.
        for index in 0..points.len() - 1 {
            if index != 0 {
                let temp = points[index] - self.center;
                let distance_from_center =
                    orca_crt::sqrt(temp.x as f64 * temp.x as f64 + temp.y as f64 * temp.y as f64);
                if orca_crt::fabs(distance_from_center - self.radius) > tolerance {
                    return true;
                }
            }
            // The point of the segment perpendicular to the center.
            if let Some(closest_point) = Circle::get_closest_perpendicular_point(
                points[index],
                points[index + 1],
                self.center,
            ) {
                let temp = closest_point - self.center;
                let distance_from_center =
                    orca_crt::sqrt(temp.x as f64 * temp.x as f64 + temp.y as f64 * temp.y as f64);
                if orca_crt::fabs(distance_from_center - self.radius) > tolerance {
                    return true;
                }
            }
        }
        false
    }

    /// `get_closest_perpendicular_point(p1, p2, c, out)`: the foot of `c` on the segment, its
    /// coordinates truncated to `coord_t`; `None` when the foot is at an end or beyond (within
    /// [`ZERO_TOLERANCE`]).
    #[must_use]
    pub fn get_closest_perpendicular_point(p1: Point, p2: Point, c: Point) -> Option<Point> {
        let x1 = p1.x as f64;
        let y1 = p1.y as f64;
        let x2 = p2.x as f64;
        let y2 = p2.y as f64;
        let x_dif = x2 - x1;
        let y_dif = y2 - y1;
        // [(Cx - Ax)(Bx - Ax) + (Cy - Ay)(By - Ay)] / [(Bx - Ax)^2 + (By - Ay)^2]
        let num = (c.x as f64 - x1) * x_dif + (c.y as f64 - y1) * y_dif;
        let denom = (x_dif * x_dif) + (y_dif * y_dif);
        let t = num / denom;

        // At an end within the tolerance counts as a failure; the end is fine as it is.
        if Circle::less_than_or_equal(t, 0.0, ZERO_TOLERANCE)
            || Circle::greater_than_or_equal(t, 1.0, ZERO_TOLERANCE)
        {
            return None;
        }
        Some(Point::new(
            orca_crt::f64_to_i64(x1 + t * (x2 - x1)),
            orca_crt::f64_to_i64(y1 + t * (y2 - y1)),
        ))
    }

    /// `get_deviation_sum_squared(points, tolerance, total_deviation)`: whether every point between
    /// the first and the last, and every foot of the center on a segment, lies within `tolerance`
    /// of the circle, and the sum of the squared deviations up to the first that does not.
    ///
    /// # Panics
    /// On no points, which the C++ counts from `size_t(-1)` for.
    #[must_use]
    pub fn get_deviation_sum_squared(&self, points: &[Point], tolerance: f64) -> (bool, f64) {
        if points.is_empty() {
            undefined("Circle::get_deviation_sum_squared of no points");
        }
        let mut total_deviation = 0.0;
        // The first and last points are on the circle.
        for &p in points.iter().take(points.len() - 1).skip(1) {
            let temp = p - self.center;
            let distance_from_center =
                orca_crt::sqrt(temp.x as f64 * temp.x as f64 + temp.y as f64 * temp.y as f64);
            let deviation = orca_crt::fabs(distance_from_center - self.radius);
            total_deviation += deviation * deviation;
            if deviation > tolerance {
                return (false, total_deviation);
            }
        }
        // The points of the segments perpendicular to the center.
        for index in 0..points.len() - 1 {
            if let Some(closest_point) = Circle::get_closest_perpendicular_point(
                points[index],
                points[index + 1],
                self.center,
            ) {
                let temp = closest_point - self.center;
                let distance_from_center =
                    orca_crt::sqrt(temp.x as f64 * temp.x as f64 + temp.y as f64 * temp.y as f64);
                let deviation = orca_crt::fabs(distance_from_center - self.radius);
                total_deviation += deviation * deviation;
                if deviation > tolerance {
                    return (false, total_deviation);
                }
            }
        }
        (true, total_deviation)
    }

    /// `calc_tangential_vector(pos, center_pos, is_ccw)`: the unit tangent at `pos` of the circle
    /// about `center_pos` in the X-Y plane (z dropped), for the given direction.
    #[must_use]
    pub fn calc_tangential_vector(pos: Vec3f, center_pos: Vec3f, is_ccw: bool) -> Vec3f {
        let mut dir = center_pos - pos;
        dir[2] = 0.0;
        dir.normalize();
        if is_ccw {
            Vector([dir[1], -dir[0], 0.0])
        } else {
            Vector([-dir[1], dir[0], 0.0])
        }
    }

    /// `is_equal(x, y, tolerance)`: `|x - y| < tolerance`.
    #[must_use]
    pub fn is_equal(x: f64, y: f64, tolerance: f64) -> bool {
        let abs_difference = orca_crt::fabs(x - y);
        abs_difference < tolerance
    }

    /// `greater_than(x, y, tolerance)`.
    #[must_use]
    pub fn greater_than(x: f64, y: f64, tolerance: f64) -> bool {
        x > y && !Circle::is_equal(x, y, tolerance)
    }

    /// `greater_than_or_equal(x, y, tolerance)`.
    #[must_use]
    pub fn greater_than_or_equal(x: f64, y: f64, tolerance: f64) -> bool {
        x > y || Circle::is_equal(x, y, tolerance)
    }

    /// `less_than(x, y, tolerance)`.
    #[must_use]
    pub fn less_than(x: f64, y: f64, tolerance: f64) -> bool {
        x < y && !Circle::is_equal(x, y, tolerance)
    }

    /// `less_than_or_equal(x, y, tolerance)`.
    #[must_use]
    pub fn less_than_or_equal(x: f64, y: f64, tolerance: f64) -> bool {
        x < y || Circle::is_equal(x, y, tolerance)
    }
}

/// `ArcDirection`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum ArcDirection {
    /// `Arc_Dir_unknow`.
    #[default]
    Unknown = 0,
    /// `Arc_Dir_CCW`.
    Ccw = 1,
    /// `Arc_Dir_CW`.
    Cw = 2,
}

/// `ArcSegment`: a `Circle` with an arc on it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ArcSegment {
    /// `center` (of the `Circle` base).
    pub center: Point,
    /// `radius` (of the `Circle` base).
    pub radius: f64,
    /// `is_arc`.
    pub is_arc: bool,
    /// `length`.
    pub length: f64,
    /// `angle_radians`: negative for a clockwise arc.
    pub angle_radians: f64,
    /// `polar_start_theta`.
    pub polar_start_theta: f64,
    /// `polar_end_theta`.
    pub polar_end_theta: f64,
    /// `start_point`.
    pub start_point: Point,
    /// `end_point`.
    pub end_point: Point,
    /// `direction`.
    pub direction: ArcDirection,
}

impl ArcSegment {
    /// `ArcSegment(center, radius, start, end, dir)`: not an arc when the radius is 0, an end is
    /// the center or the ends are equal; else its angles and length.
    #[must_use]
    pub fn new(center: Point, radius: f64, start: Point, end: Point, dir: ArcDirection) -> Self {
        let mut a = ArcSegment {
            center,
            radius,
            start_point: start,
            end_point: end,
            direction: dir,
            ..ArcSegment::default()
        };
        if radius == 0.0 || start == center || end == center || start == end {
            a.is_arc = false;
            return a;
        }
        a.update_angle_and_length();
        a.is_arc = true;
        a
    }

    /// `(Circle)arc`: the circle the arc lies on.
    #[must_use]
    pub fn circle(&self) -> Circle {
        Circle::new(self.center, self.radius)
    }

    /// `get_polar_radians(p1)` of the circle.
    #[must_use]
    pub fn get_polar_radians(&self, p1: Point) -> f64 {
        self.circle().get_polar_radians(p1)
    }

    /// `get_closest_point(input)` of the circle.
    #[must_use]
    pub fn get_closest_point(&self, input: Point) -> Point {
        self.circle().get_closest_point(input)
    }

    /// `is_valid()`.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.is_arc
    }

    /// `reverse()`: the ends, the angles and the direction swapped (an unknown direction becomes
    /// counter-clockwise); false for no arc.
    pub fn reverse(&mut self) -> bool {
        if !self.is_valid() {
            return false;
        }
        core::mem::swap(&mut self.start_point, &mut self.end_point);
        self.direction = if self.direction == ArcDirection::Ccw {
            ArcDirection::Cw
        } else {
            ArcDirection::Ccw
        };
        self.angle_radians *= -1.0;
        core::mem::swap(&mut self.polar_start_theta, &mut self.polar_end_theta);
        true
    }

    /// `clip_start(point)`: the start moved to the circle's point toward `point`, when `point` is
    /// inside the arc's angle and not the center.
    pub fn clip_start(&mut self, point: Point) -> bool {
        if !self.is_valid() || point == self.center || !self.is_point_inside(point) {
            return false;
        }
        self.start_point = self.get_closest_point(point);
        self.update_angle_and_length();
        true
    }

    /// `clip_end(point)`: as [`ArcSegment::clip_start`], for the end.
    pub fn clip_end(&mut self, point: Point) -> bool {
        if !self.is_valid() || point == self.center || !self.is_point_inside(point) {
            return false;
        }
        self.end_point = self.get_closest_point(point);
        self.update_angle_and_length();
        true
    }

    /// `split_at(point, p1, p2)`: the two arcs on either side of the circle's point toward
    /// `point`, when `point` is inside the arc's angle and not the center.
    #[must_use]
    pub fn split_at(&self, point: Point) -> Option<(ArcSegment, ArcSegment)> {
        if !self.is_valid() || point == self.center || !self.is_point_inside(point) {
            return None;
        }
        let segment_point = self.get_closest_point(point);
        let p1 = ArcSegment::new(
            self.center,
            self.radius,
            self.start_point,
            segment_point,
            self.direction,
        );
        let p2 = ArcSegment::new(
            self.center,
            self.radius,
            segment_point,
            self.end_point,
            self.direction,
        );
        Some((p1, p2))
    }

    /// `is_point_inside(point)`: the point's angle strictly inside the arc's.
    #[must_use]
    pub fn is_point_inside(&self, point: Point) -> bool {
        let polar_theta = self.get_polar_radians(point);
        let mut radian_delta = polar_theta - self.polar_start_theta;
        if radian_delta > 0.0 && self.direction == ArcDirection::Cw {
            radian_delta -= 2.0 * PI;
        } else if radian_delta < 0.0 && self.direction == ArcDirection::Ccw {
            radian_delta += 2.0 * PI;
        }
        if self.direction == ArcDirection::Ccw {
            radian_delta > 0.0 && radian_delta < self.angle_radians
        } else {
            radian_delta < 0.0 && radian_delta > self.angle_radians
        }
    }

    /// `update_angle_and_length()`: the polar angles of the ends, the swept angle in the arc's
    /// direction and the length; the arc is valid after it.
    fn update_angle_and_length(&mut self) {
        self.polar_start_theta = self.get_polar_radians(self.start_point);
        self.polar_end_theta = self.get_polar_radians(self.end_point);
        self.angle_radians = self.polar_end_theta - self.polar_start_theta;
        if self.angle_radians < 0.0 && self.direction == ArcDirection::Ccw {
            self.angle_radians += 2.0 * PI;
        } else if self.angle_radians > 0.0 && self.direction == ArcDirection::Cw {
            self.angle_radians -= 2.0 * PI;
        }
        self.length = orca_crt::fabs(self.angle_radians) * self.radius;
        self.is_arc = true;
    }

    /// `try_create_arc(points, target_arc, approximate_length, max_radius, tolerance,
    /// path_tolerance_percent)`: the arc through the points, when a circle fits them, its length
    /// is within `path_tolerance_percent` of `approximate_length` and the points lie within its
    /// angles. (The C++ starts the circle from `target_arc`'s, which no outcome depends on.)
    ///
    /// # Panics
    /// Below two points, which the C++ reads outside the list for.
    #[must_use]
    pub fn try_create_arc(
        points: &[Point],
        approximate_length: f64,
        max_radius: f64,
        tolerance: f64,
        path_tolerance_percent: f64,
    ) -> Option<ArcSegment> {
        if points.len() < 2 {
            undefined("ArcSegment::try_create_arc of fewer than two points");
        }
        let mut test_circle = Circle::default();
        if !Circle::try_create_circle(points, max_radius, tolerance, &mut test_circle) {
            return None;
        }
        let mid_point_index = ((points.len() - 2) / 2) + 1;
        let test_arc = ArcSegment::try_create_arc_of_circle(
            &test_circle,
            points[0],
            points[mid_point_index],
            points[points.len() - 1],
            approximate_length,
            path_tolerance_percent,
        )?;
        if ArcSegment::are_points_within_slice(&test_arc, points) {
            Some(test_arc)
        } else {
            None
        }
    }

    /// The private `try_create_arc(c, start_point, mid_point, end_point, target_arc,
    /// approximate_length, path_tolerance_percent)`: the direction from the polar angles of the
    /// three points, then the length against `approximate_length`, trying the other way round the
    /// circle when it is off (the direction flips, the angle does not).
    fn try_create_arc_of_circle(
        c: &Circle,
        start_point: Point,
        mid_point: Point,
        end_point: Point,
        approximate_length: f64,
        path_tolerance_percent: f64,
    ) -> Option<ArcSegment> {
        let polar_start_theta = c.get_polar_radians(start_point);
        let polar_mid_theta = c.get_polar_radians(mid_point);
        let polar_end_theta = c.get_polar_radians(end_point);

        let mut angle_radians = 0.0;
        let mut direction = ArcDirection::Unknown;
        // The direction of the arc.
        if polar_end_theta > polar_start_theta {
            if polar_start_theta < polar_mid_theta && polar_mid_theta < polar_end_theta {
                direction = ArcDirection::Ccw;
                angle_radians = polar_end_theta - polar_start_theta;
            } else if (0.0 <= polar_mid_theta && polar_mid_theta < polar_start_theta)
                || (polar_end_theta < polar_mid_theta && polar_mid_theta < (2.0 * PI))
            {
                direction = ArcDirection::Cw;
                angle_radians = polar_start_theta + ((2.0 * PI) - polar_end_theta);
            }
        } else if polar_start_theta > polar_end_theta {
            if (polar_start_theta < polar_mid_theta && polar_mid_theta < (2.0 * PI))
                || (0.0 < polar_mid_theta && polar_mid_theta < polar_end_theta)
            {
                direction = ArcDirection::Ccw;
                angle_radians = polar_end_theta + ((2.0 * PI) - polar_start_theta);
            } else if polar_end_theta < polar_mid_theta && polar_mid_theta < polar_start_theta {
                direction = ArcDirection::Cw;
                angle_radians = polar_start_theta - polar_end_theta;
            }
        }

        // This does not always work: in rare cases the angle may be backward.
        if direction == ArcDirection::Unknown || orca_crt::fabs(angle_radians) < EPSILON {
            return None;
        }

        // The length against the original one: off through the differing path lengths, or the
        // direction above may be wrong.
        let mut arc_length = c.radius * angle_radians;
        let mut difference = (arc_length - approximate_length) / approximate_length;
        if orca_crt::fabs(difference) >= path_tolerance_percent {
            // The rest of the angle across the circle, and its arc's length.
            let test_radians = orca_crt::fabs(angle_radians - 2.0 * PI);
            let test_arc_length = c.radius * test_radians;
            difference = (test_arc_length - approximate_length) / approximate_length;
            if orca_crt::fabs(difference) >= path_tolerance_percent {
                return None;
            }
            // The new length, and the direction flipped (but not the angle).
            arc_length = test_arc_length;
            direction = if direction == ArcDirection::Ccw {
                ArcDirection::Cw
            } else {
                ArcDirection::Ccw
            };
        }

        if direction == ArcDirection::Cw {
            angle_radians *= -1.0;
        }

        Some(ArcSegment {
            is_arc: true,
            direction,
            center: c.center,
            radius: c.radius,
            start_point,
            end_point,
            length: arc_length,
            angle_radians,
            polar_start_theta,
            polar_end_theta,
        })
    }

    /// `are_points_within_slice(test_arc, points)`: the last two points' angles inside the arc's
    /// and moving its way, crossing angle 0 once exactly when the arc does, and their segments
    /// crossing neither ray from the center to an end. (The C++ loop starts at `point_count - 2`.)
    ///
    /// # Panics
    /// Below three points, which the C++ reads before the list for.
    #[must_use]
    pub fn are_points_within_slice(test_arc: &ArcSegment, points: &[Point]) -> bool {
        if points.len() < 3 {
            undefined("ArcSegment::are_points_within_slice of fewer than three points");
        }
        // All the points' angles inside the arc's.
        let mut previous_polar = test_arc.polar_start_theta;
        let mut crossed_zero = false;
        let point_count = points.len();

        let start_norm = vec2d(
            (test_arc.start_point.x as f64 - test_arc.center.x as f64) / test_arc.radius,
            (test_arc.start_point.y as f64 - test_arc.center.y as f64) / test_arc.radius,
        );
        let end_norm = vec2d(
            (test_arc.end_point.x as f64 - test_arc.center.x as f64) / test_arc.radius,
            (test_arc.end_point.y as f64 - test_arc.center.y as f64) / test_arc.radius,
        );

        let will_cross_zero = if test_arc.direction == ArcDirection::Ccw {
            test_arc.polar_start_theta > test_arc.polar_end_theta
        } else {
            test_arc.polar_start_theta < test_arc.polar_end_theta
        };

        for index in point_count - 2..point_count {
            let polar_test = if index < point_count - 1 {
                test_arc.get_polar_radians(points[index])
            } else {
                test_arc.polar_end_theta
            };

            // The point within the arc first.
            if test_arc.direction == ArcDirection::Ccw {
                // Only when it is not the end point.
                if index < point_count - 1 {
                    if will_cross_zero {
                        if !(polar_test > test_arc.polar_start_theta
                            || polar_test < test_arc.polar_end_theta)
                        {
                            return false;
                        }
                    } else if !(test_arc.polar_start_theta < polar_test
                        && polar_test < test_arc.polar_end_theta)
                    {
                        return false;
                    }
                }
                // The angles increasing.
                if previous_polar > polar_test {
                    if !will_cross_zero {
                        return false;
                    }
                    // Angle 0 crossed once at most.
                    if crossed_zero {
                        return false;
                    }
                    crossed_zero = true;
                }
            } else {
                if index < point_count - 1 {
                    if will_cross_zero {
                        if !(polar_test < test_arc.polar_start_theta
                            || polar_test > test_arc.polar_end_theta)
                        {
                            return false;
                        }
                    } else if !(test_arc.polar_start_theta > polar_test
                        && polar_test > test_arc.polar_end_theta)
                    {
                        return false;
                    }
                }
                // The angles decreasing.
                if previous_polar < polar_test {
                    if !will_cross_zero {
                        return false;
                    }
                    if crossed_zero {
                        return false;
                    }
                    crossed_zero = true;
                }
            }

            // The segment crossing either ray from the center to an end of the arc.
            let segment = Line::new(points[index - 1], points[index]);
            if (index != 1
                && ArcSegment::ray_intersects_segment(test_arc.center, start_norm, &segment))
                || (index != point_count - 1
                    && ArcSegment::ray_intersects_segment(test_arc.center, end_norm, &segment))
            {
                return false;
            }
            previous_polar = polar_test;
        }
        // Every arc that crosses zero has points that do.
        will_cross_zero == crossed_zero
    }

    /// `ray_intersects_segment(ray_origin, ray_direction, segment)`: whether the ray crosses the
    /// segment (false when they are within `SCALED_EPSILON` of parallel).
    #[must_use]
    pub fn ray_intersects_segment(ray_origin: Point, ray_direction: Vec2d, segment: &Line) -> bool {
        let v1 = vec2d(
            ray_origin.x.wrapping_sub(segment.a.x) as f64,
            ray_origin.y.wrapping_sub(segment.a.y) as f64,
        );
        let v2 = vec2d(
            segment.b.x.wrapping_sub(segment.a.x) as f64,
            segment.b.y.wrapping_sub(segment.a.y) as f64,
        );
        let v3 = vec2d(-ray_direction[1], ray_direction[0]);

        let dot = v2[0] * v3[0] + v2[1] * v3[1];
        if orca_crt::fabs(dot) < SCALED_EPSILON {
            return false;
        }

        let t1 = (v2[0] * v1[1] - v2[1] * v1[0]) / dot;
        let t2 = (v1[0] * v3[0] + v1[1] * v3[1]) / dot;

        // `t2 >= 0 && t2 <= 1`.
        t1 >= 0.0 && (0.0..=1.0).contains(&t2)
    }

    /// `calc_arc_radian(start_pos, end_pos, center_pos, is_ccw)`: the arc's angle in the X-Y
    /// plane in `float`, a full circle when the ends are within 1e-6 of each other.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn calc_arc_radian(
        start_pos: Vec3f,
        end_pos: Vec3f,
        center_pos: Vec3f,
        is_ccw: bool,
    ) -> f32 {
        let mut delta1 = center_pos - start_pos;
        let mut delta2 = center_pos - end_pos;
        // Only the arc in the X-Y plane: the z distance cleared.
        delta1[2] = 0.0;
        delta2[2] = 0.0;

        let mut radian: f32;
        if f64::from((delta1 - delta2).norm()) < 1e-6 {
            // The same start and end: a full circle.
            radian = (2.0 * PI) as f32;
        } else {
            let dot = f64::from(delta1.dot(&delta2));
            let cross = f64::from(delta1[0]) * f64::from(delta2[1])
                - f64::from(delta1[1]) * f64::from(delta2[0]);
            radian = orca_crt::atan2(cross, dot) as f32;
            if is_ccw {
                radian = if radian < 0.0 {
                    (2.0 * PI + f64::from(radian)) as f32
                } else {
                    radian
                };
            } else {
                // `abs(radian)` is MSVC's `float` overload.
                radian = if radian < 0.0 {
                    orca_crt::fabsf(radian)
                } else {
                    (2.0 * PI - f64::from(radian)) as f32
                };
            }
        }
        radian
    }

    /// `calc_arc_radius(start_pos, center_pos)`: the distance in the X-Y plane, in `float`.
    #[must_use]
    pub fn calc_arc_radius(start_pos: Vec3f, center_pos: Vec3f) -> f32 {
        let mut delta1 = center_pos - start_pos;
        delta1[2] = 0.0;
        delta1.norm()
    }

    /// `calc_arc_length(start_pos, end_pos, center_pos, is_ccw)`: the radius times the angle, in
    /// `float`.
    #[must_use]
    pub fn calc_arc_length(
        start_pos: Vec3f,
        end_pos: Vec3f,
        center_pos: Vec3f,
        is_ccw: bool,
    ) -> f32 {
        ArcSegment::calc_arc_radius(start_pos, center_pos)
            * ArcSegment::calc_arc_radian(start_pos, end_pos, center_pos, is_ccw)
    }
}

// The Points3 forms (geom-2, 2026-11-13): Circle.hpp 31, 115-121 and 124, Circle.cpp 98-102,
// 301-325 and 490-570. Each works on the points' x and y.

impl Circle {
    /// `try_create_circle(const Points3&, max_radius, tolerance, new_circle)`:
    /// [`Circle::try_create_circle`] of the points' x and y ([`to_points`]).
    ///
    /// # Panics
    /// Below two points, which the C++ reads outside the list for.
    pub fn try_create_circle_points3(
        points: &[Point3],
        max_radius: f64,
        tolerance: f64,
        new_circle: &mut Circle,
    ) -> bool {
        Circle::try_create_circle(&to_points(points), max_radius, tolerance, new_circle)
    }
}

impl ArcSegment {
    /// `try_create_arc(const Points3&, target_arc, approximate_length, max_radius, tolerance,
    /// path_tolerance_percent)`: as [`ArcSegment::try_create_arc`], the circle of the points' x and
    /// y ([`Circle::try_create_circle_points3`]) and the slice test of the Point3s
    /// ([`ArcSegment::are_points_within_slice_points3`]).
    ///
    /// # Panics
    /// Below two points, which the C++ reads outside the list for.
    #[must_use]
    pub fn try_create_arc_points3(
        points: &[Point3],
        approximate_length: f64,
        max_radius: f64,
        tolerance: f64,
        path_tolerance_percent: f64,
    ) -> Option<ArcSegment> {
        if points.len() < 2 {
            undefined("ArcSegment::try_create_arc of fewer than two Point3s");
        }
        let mut test_circle = Circle::default();
        if !Circle::try_create_circle_points3(points, max_radius, tolerance, &mut test_circle) {
            return None;
        }
        let mid_point_index = ((points.len() - 2) / 2) + 1;
        let test_arc = ArcSegment::try_create_arc_of_circle(
            &test_circle,
            points[0].to_point(),
            points[mid_point_index].to_point(),
            points[points.len() - 1].to_point(),
            approximate_length,
            path_tolerance_percent,
        )?;
        if ArcSegment::are_points_within_slice_points3(&test_arc, points) {
            Some(test_arc)
        } else {
            None
        }
    }

    /// `are_points_within_slice(test_arc, const Points3&)`: [`ArcSegment::are_points_within_slice`]
    /// line for line, on the points' x and y (the last two points only, as the C++ loop starts at
    /// `point_count - 2`).
    ///
    /// # Panics
    /// Below three points, which the C++ reads before the list for.
    #[must_use]
    pub fn are_points_within_slice_points3(test_arc: &ArcSegment, points: &[Point3]) -> bool {
        if points.len() < 3 {
            undefined("ArcSegment::are_points_within_slice of fewer than three Point3s");
        }
        // All the points' angles inside the arc's.
        let mut previous_polar = test_arc.polar_start_theta;
        let mut crossed_zero = false;
        let point_count = points.len();

        let start_norm = vec2d(
            (test_arc.start_point.x as f64 - test_arc.center.x as f64) / test_arc.radius,
            (test_arc.start_point.y as f64 - test_arc.center.y as f64) / test_arc.radius,
        );
        let end_norm = vec2d(
            (test_arc.end_point.x as f64 - test_arc.center.x as f64) / test_arc.radius,
            (test_arc.end_point.y as f64 - test_arc.center.y as f64) / test_arc.radius,
        );

        let will_cross_zero = if test_arc.direction == ArcDirection::Ccw {
            test_arc.polar_start_theta > test_arc.polar_end_theta
        } else {
            test_arc.polar_start_theta < test_arc.polar_end_theta
        };

        // Whether point 1 to point 2 crosses zero.
        for index in point_count - 2..point_count {
            let polar_test = if index < point_count - 1 {
                test_arc.get_polar_radians(points[index].to_point())
            } else {
                test_arc.polar_end_theta
            };

            // The point within the arc first.
            if test_arc.direction == ArcDirection::Ccw {
                // Only when it is not the end point.
                if index < point_count - 1 {
                    if will_cross_zero {
                        if !(polar_test > test_arc.polar_start_theta
                            || polar_test < test_arc.polar_end_theta)
                        {
                            return false;
                        }
                    } else if !(test_arc.polar_start_theta < polar_test
                        && polar_test < test_arc.polar_end_theta)
                    {
                        return false;
                    }
                }
                // The angles increasing.
                if previous_polar > polar_test {
                    if !will_cross_zero {
                        return false;
                    }
                    // Angle 0 crossed once at most.
                    if crossed_zero {
                        return false;
                    }
                    crossed_zero = true;
                }
            } else {
                if index < point_count - 1 {
                    if will_cross_zero {
                        if !(polar_test < test_arc.polar_start_theta
                            || polar_test > test_arc.polar_end_theta)
                        {
                            return false;
                        }
                    } else if !(test_arc.polar_start_theta > polar_test
                        && polar_test > test_arc.polar_end_theta)
                    {
                        return false;
                    }
                }
                // The angles decreasing.
                if previous_polar < polar_test {
                    if !will_cross_zero {
                        return false;
                    }
                    if crossed_zero {
                        return false;
                    }
                    crossed_zero = true;
                }
            }

            // The segment crossing either ray from the center to an end of the arc.
            let segment = Line::new(points[index - 1].to_point(), points[index].to_point());
            if (index != 1
                && ArcSegment::ray_intersects_segment(test_arc.center, start_norm, &segment))
                || (index != point_count - 1
                    && ArcSegment::ray_intersects_segment(test_arc.center, end_norm, &segment))
            {
                return false;
            }
            previous_polar = polar_test;
        }
        // Every arc that crosses zero has points that do.
        will_cross_zero == crossed_zero
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_points_on_a_circle() {
        let mut c = Circle::default();
        assert!(Circle::try_create_circle3(
            Point::new(0, 0),
            Point::new(1_000_000, 1_000_000),
            Point::new(2_000_000, 0),
            default_scaled_max_radius(),
            &mut c,
        ));
        assert_eq!(c.center, Point::new(1_000_000, 0));
        assert_eq!(c.radius, 1_000_000.0);
        // Collinear points make no circle and leave it.
        assert!(!Circle::try_create_circle3(
            Point::new(0, 0),
            Point::new(1, 0),
            Point::new(2, 0),
            default_scaled_max_radius(),
            &mut c,
        ));
        assert_eq!(c.center, Point::new(1_000_000, 0));
    }

    #[test]
    fn a_quarter_arc_and_its_reverse() {
        let a = ArcSegment::new(
            Point::new(0, 0),
            1000.0,
            Point::new(1000, 0),
            Point::new(0, 1000),
            ArcDirection::Ccw,
        );
        assert!(a.is_valid());
        assert!((a.angle_radians - PI / 2.0).abs() < 1e-12);
        assert!(a.is_point_inside(Point::new(700, 700)));
        let mut r = a;
        assert!(r.reverse());
        assert_eq!(r.direction, ArcDirection::Cw);
        assert_eq!(r.start_point, a.end_point);
        assert!(!ArcSegment::default().is_valid());
    }
}
