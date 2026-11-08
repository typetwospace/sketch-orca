// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: circle_center, CircleSq, Circle, circle_ransac and
// smallest_enclosing_circle_welzl of src/libslic3r/Geometry/Circle.hpp and Circle.cpp, of
// OrcaSlicer v2.4.2, translated on 2026-11-07. Copyright the OrcaSlicer authors, the BambuStudio
// authors (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU Affero General
// Public License, version 3 (AGPL-3.0; LICENSE).
//
// Carries Microsoft STL code: `sample` with `_Sample_selection_unchecked` and `_Rng_from_urng` of
// stl/inc/algorithm (MSVC 14.42.34433), translated on 2026-11-07. Copyright (c)
// Microsoft Corporation, under the Apache License, version 2.0, with the LLVM Exceptions
// (Apache-2.0 WITH LLVM-exception; licenses/Apache-2.0.txt and licenses/LLVM-exception.txt).

//! Circles for the print bed: the circle through three points, the circle RANSAC fits to a
//! discretised round bed, and the smallest circle around a convex hull.
//!
//! `circle_ransac` draws its three points with `std::sample` and a default-seeded `std::mt19937`,
//! as MSVC's STL draws them: selection sampling, each element kept when a draw in `[0, left)` is
//! below the number still wanted, a draw taking whole 32-bit engine words into a 64-bit value and
//! rejecting by division.

use orca_eigen::Vec2d;
use orca_geom::point::{EPSILON, Point, SCALED_EPSILON};
use orca_stl::random::{Mt19937, Urbg};

/// `Circled`: a centre and a radius (negative: invalid).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Circled {
    /// `center`.
    pub center: Vec2d,
    /// `radius`.
    pub radius: f64,
}

impl Default for Circled {
    fn default() -> Self {
        Circled {
            center: Vec2d::new(0.0, 0.0),
            radius: 0.0,
        }
    }
}

impl Circled {
    /// `make_invalid()`: centre (0, 0), radius -1.
    #[must_use]
    pub fn make_invalid() -> Self {
        Circled {
            center: Vec2d::new(0.0, 0.0),
            radius: -1.0,
        }
    }
}

/// `circle_center(a, bsrc, csrc, epsilon)`: the centre of the circle through three points, or the
/// midpoint of the two furthest apart when they are collinear within `epsilon`.
#[must_use]
pub fn circle_center(a: Vec2d, bsrc: Vec2d, csrc: Vec2d, epsilon: f64) -> Vec2d {
    let b = bsrc - a;
    let c = csrc - a;
    let lb = b.squared_norm();
    let lc = c.squared_norm();
    let d = b.x() * c.y() - b.y() * c.x();
    if d.abs() < epsilon {
        let lbc = (csrc - bsrc).squared_norm();
        let s = if lb > lc && lb > lbc {
            a + bsrc
        } else if lc > lb && lc > lbc {
            a + csrc
        } else {
            bsrc + csrc
        };
        s * 0.5
    } else {
        let v = b * lc - c * lb;
        a + Vec2d::new(-v.y(), v.x()) / (2.0 * d)
    }
}

/// `_Rng_from_urng<ptrdiff_t, mt19937>(g)(index)`: a draw in `[0, index)`.
fn rng_from_urng(g: &mut Mt19937, index: u64) -> u64 {
    let bits = 32u32;
    let bmask: u64 = 0xFFFF_FFFF;
    loop {
        let mut ret: u64 = 0;
        let mut mask: u64 = 0;
        while mask < index - 1 {
            ret = (ret << (bits - 1)) << 1;
            ret |= g.next_u64();
            mask = (mask << (bits - 1)) << 1;
            mask |= bmask;
        }
        if ret / index < mask / index || mask % index == index - 1 {
            return ret % index;
        }
    }
}

/// `std::sample(input, out, count, g)` for a vector source: selection sampling, in order.
fn sample(input: &[Vec2d], count: usize, g: &mut Mt19937) -> Vec<Vec2d> {
    let mut out = Vec::new();
    if count == 0 {
        return out;
    }
    let mut count = count.min(input.len());
    let mut pop_size = input.len();
    let mut first = 0usize;
    while pop_size > 0 {
        if rng_from_urng(g, pop_size as u64) < count as u64 {
            count -= 1;
            out.push(input[first]);
        }
        first += 1;
        pop_size -= 1;
    }
    out
}

/// `circle_ransac(input, iterations)`: the best of `iterations` circles through three sampled
/// points, by the largest distance of a point from the circle; the radius is the mean distance.
#[must_use]
pub fn circle_ransac(input: &[Vec2d], iterations: usize) -> (Circled, f64) {
    if input.len() < 3 {
        return (Circled::make_invalid(), 0.0);
    }
    let mut rng = Mt19937::default();
    let mut circle_best = Circled::make_invalid();
    let mut err_min = f64::MAX;
    for _ in 0..iterations {
        let samples = sample(input, 3, &mut rng);
        let center = circle_center(samples[0], samples[1], samples[2], EPSILON);
        let mut radius = 0.0;
        for pt in input {
            // `norm + acc` in the C++; an IEEE sum does not depend on the order of its two terms.
            radius += (*pt - center).norm();
        }
        radius /= input.len() as f64;
        let c = Circled { center, radius };
        let mut err = 0.0;
        for pt in input {
            err = orca_stl::max(err, ((*pt - c.center).norm() - c.radius).abs());
        }
        if err < err_min {
            err_min = err;
            circle_best = c;
        }
    }
    (circle_best, err_min)
}

/// `CircleSq<Vec2d>`: a centre and a squared radius.
#[derive(Clone, Copy, Debug)]
struct CircleSq {
    center: Vec2d,
    radius2: f64,
}

impl CircleSq {
    fn two(a: Vec2d, b: Vec2d) -> Self {
        let center = (a + b) * 0.5;
        CircleSq {
            center,
            radius2: (a - center).squared_norm(),
        }
    }

    fn three(a: Vec2d, b: Vec2d, c: Vec2d, epsilon: f64) -> Self {
        let center = circle_center(a, b, c, epsilon);
        CircleSq {
            center,
            radius2: (a - center).squared_norm(),
        }
    }

    fn contains(&self, p: Vec2d) -> bool {
        (p - self.center).squared_norm() < self.radius2
    }

    fn inflated(&self, epsilon: f64) -> Self {
        let r = self.radius2.sqrt() + epsilon;
        CircleSq {
            center: self.center,
            radius2: r * r,
        }
    }
}

/// `smallest_enclosing_circle_welzl(points)`: Welzl's circle of the scaled points, inflated by
/// `SCALED_EPSILON`, with squared radii until the end; a default circle (radius 0) for no
/// points, as the C++ leaves the squared radius unset there (`parity/HAZARDS.md`, orca-model).
#[must_use]
pub fn smallest_enclosing_circle_welzl(points: &[Point]) -> Circled {
    let epsilon = SCALED_EPSILON;
    let cast = |p: &Point| Vec2d::new(p.x as f64, p.y as f64);
    let circle = if points.is_empty() {
        CircleSq {
            center: Vec2d::new(0.0, 0.0),
            radius2: 0.0,
        }
    } else {
        let p0 = cast(&points[0]);
        if points.len() == 1 {
            CircleSq {
                center: p0,
                radius2: epsilon * epsilon,
            }
        } else {
            let mut circle = CircleSq::two(p0, cast(&points[1])).inflated(epsilon);
            for i in 2..points.len() {
                let p = cast(&points[i]);
                if !circle.contains(p) {
                    circle = CircleSq::two(p0, p).inflated(epsilon);
                    for j in 1..i {
                        let q = cast(&points[j]);
                        if !circle.contains(q) {
                            circle = CircleSq::two(p, q).inflated(epsilon);
                            for pk in &points[..j] {
                                let r = cast(pk);
                                if !circle.contains(r) {
                                    circle = CircleSq::three(p, q, r, epsilon).inflated(epsilon);
                                }
                            }
                        }
                    }
                }
            }
            circle
        }
    };
    Circled {
        center: circle.center,
        radius: if circle.radius2 <= 0.0 {
            circle.radius2
        } else {
            circle.radius2.sqrt()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_round_bed_fits_its_circle() {
        let pts: Vec<Vec2d> = (0..72)
            .map(|i| {
                let a = f64::from(i) * std::f64::consts::PI * 2.0 / 72.0;
                Vec2d::new(
                    100.0 + 50.0 * orca_crt::cos(a),
                    100.0 + 50.0 * orca_crt::sin(a),
                )
            })
            .collect();
        let (c, err) = circle_ransac(&pts, 20);
        assert!((c.center.x() - 100.0).abs() < 1e-9 && (c.radius - 50.0).abs() < 1e-9);
        assert!(err < 1e-9);
    }

    #[test]
    fn three_collinear_points_take_the_furthest_two() {
        let c = circle_center(
            Vec2d::new(0.0, 0.0),
            Vec2d::new(1.0, 0.0),
            Vec2d::new(3.0, 0.0),
            EPSILON,
        );
        assert_eq!(c, Vec2d::new(1.5, 0.0));
    }
}
