// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Arachne/utils/linearAlg2D.hpp lines 1-65 whole, of
// OrcaSlicer v2.4.2, translated on 2026-11-13 by arachne-1. Copyright Ultimaker B.V. (2020,
// CuraEngine), with the PrusaSlicer authors' (Prusa Research) and the OrcaSlicer authors' changes,
// under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! `Arachne::LinearAlg2D`: which side of a line a point lies on, and the angle of a corner.
//!
//! [`point_is_left_of_line`] is the determinant of `ab` and `ap` in wrapping `int64_t`.
//! [`get_angle_left`] is the angle at `b` on the left of `a -> b -> c`, in [0, 2π): a zero
//! determinant gives 0 (a pointy bit, both vectors the same way) or `float(M_PI)` (straight);
//! otherwise `-atan2(det, dot)` of the `int64_t` determinant and dot product as `double`s
//! (orca-crt's UCRT `atan2`), narrowed to `float`, and `M_PI * 2 + angle` in `double`, narrowed
//! again, when it is negative.

use orca_geom::point::{PI, Point, cross2_coord};

/// `pointIsLeftOfLine(p, a, b)`: positive when `p` is left of the line from `a` to `b`, negative
/// right of it, zero on it.
#[must_use]
pub fn point_is_left_of_line(p: Point, a: Point, b: Point) -> i64 {
    b.x.wrapping_sub(a.x)
        .wrapping_mul(p.y.wrapping_sub(a.y))
        .wrapping_sub(b.y.wrapping_sub(a.y).wrapping_mul(p.x.wrapping_sub(a.x)))
}

/// `getAngleLeft(a, b, c)`: the angle in radians, in [0, 2π), of the corner at `b`, on the left
/// of `b` looking from `a`.
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
pub fn get_angle_left(a: Point, b: Point, c: Point) -> f32 {
    let ba = a - b;
    let bc = c - b;
    let dott = ba.dot(bc);
    let det = cross2_coord(ba, bc);
    if det == 0 {
        if (ba.x != 0 && (ba.x > 0) == (bc.x > 0)) || (ba.x == 0 && (ba.y > 0) == (bc.y > 0)) {
            // A pointy bit.
            return 0.0;
        }
        // A straight bit.
        return PI as f32;
    }
    // From -π to π.
    let angle = (-orca_crt::atan2(det as f64, dott as f64)) as f32;
    if angle >= 0.0 {
        angle
    } else {
        (PI * 2.0 + f64::from(angle)) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn left_is_positive() {
        let o = Point::new(0, 0);
        assert!(point_is_left_of_line(Point::new(0, 1), o, Point::new(1, 0)) > 0);
        assert!(point_is_left_of_line(Point::new(0, -1), o, Point::new(1, 0)) < 0);
    }

    #[test]
    fn collinear_corners() {
        let o = Point::new(0, 0);
        assert_eq!(get_angle_left(Point::new(10, 0), o, Point::new(20, 0)), 0.0);
        assert_eq!(
            get_angle_left(Point::new(10, 0), o, Point::new(-20, 0)).to_bits(),
            (PI as f32).to_bits()
        );
    }
}
