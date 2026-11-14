// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/PrincipalComponents2D.hpp (1-23) and
// PrincipalComponents2D.cpp (1-140) of OrcaSlicer v2.4.2, translated on 2026-11-14
// (orca-o3-walls-1). Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and the
// PrusaSlicer authors (Prusa Research), under the GNU Affero General Public License, version 3
// (AGPL-3.0; LICENSE).

//! The principal components of an area: the two eigenvectors of the covariance matrix of the
//! area the polygons cover, the one of the larger eigenvalue first.
//!
//! [`compute_moments_of_area_of_triangle`] integrates x, x² and xy over a triangle through the map
//! of the unit triangle onto it, whose Jacobian is constant. [`compute_principal_components`] fans
//! each polygon into triangles from its first point, adds each triangle's moments signed by its
//! winding, and solves the 2×2 eigenproblem in closed form. The C++ mixes `float` and `double`
//! here, and this keeps each step's width: the moments are `float`; the covariance is a `float`
//! value kept in a `double`, so the eigenvalues are computed in `double` and narrowed to `float`.
//! `abs(covariance)` is the `double` `fabs` (MSVC's global `abs(double)` overload), which the
//! WLPC records confirm.

use orca_eigen::Vec2f;
use orca_geom::point::{EPSILON, unscale_point};
use orca_geom::polygon::Polygon;

/// `Vec2f{x, y}`.
fn vec2f(x: f32, y: f32) -> Vec2f {
    orca_eigen::Vector([x, y])
}

/// `cross2(v1, v2)` of `float` vectors: `v1.x * v2.y - v1.y * v2.x`.
fn cross2f(v1: Vec2f, v2: Vec2f) -> f32 {
    v1.x() * v2.y() - v1.y() * v2.x()
}

/// `unscaled(p).cast<float>()`: each coordinate `double(c) * SCALING_FACTOR`, narrowed.
#[allow(clippy::cast_possible_truncation)]
fn unscaled_f32(p: orca_geom::point::Point) -> Vec2f {
    let v = unscale_point(p);
    vec2f(v.x() as f32, v.y() as f32)
}

/// `compute_moments_of_area_of_triangle(a, b, c)`: the triangle's area, its first moments of
/// area (x and y), its second moments of area (x² and y²) and its second moment covariance (xy),
/// none divided by the area. Each value is the integral over the unit triangle times
/// `|J|`, the absolute Jacobian determinant, in `float` and in the C++ order.
#[must_use]
pub fn compute_moments_of_area_of_triangle(
    a: Vec2f,
    b: Vec2f,
    c: Vec2f,
) -> (f32, Vec2f, Vec2f, f32) {
    let jacobian_determinant_abs =
        orca_crt::fabsf((b.x() - a.x()) * (c.y() - a.y()) - (c.x() - a.x()) * (b.y() - a.y()));

    // Eigen evaluates `jac * (a∘a + b∘b + b∘c + c∘c + a∘(b + c)) / 12` per coordinate: the sum
    // left to right, times `jac`, over 12.
    let second = |i: usize| -> f32 {
        let s = a[i] * a[i] + b[i] * b[i] + b[i] * c[i] + c[i] * c[i] + a[i] * (b[i] + c[i]);
        jacobian_determinant_abs * s / 12.0f32
    };
    let second_moment_of_area_xy = vec2f(second(0), second(1));

    let second_moment_of_area_covariance = jacobian_determinant_abs
        * (1.0f32 / 24.0f32)
        * (a.y() * (b.x() + c.x())
            + a.x() * (2.0f32 * a.y() + b.y() + c.y())
            + b.y() * c.x()
            + b.x() * c.y()
            + 2.0f32 * b.x() * b.y()
            + 2.0f32 * c.x() * c.y());

    let area = jacobian_determinant_abs * 0.5f32;

    let first = |i: usize| -> f32 { jacobian_determinant_abs * (a[i] + b[i] + c[i]) / 6.0f32 };
    let first_moment_of_area_xy = vec2f(first(0), first(1));

    (
        area,
        first_moment_of_area_xy,
        second_moment_of_area_xy,
        second_moment_of_area_covariance,
    )
}

/// `compute_principal_components(polys)`: the two eigenvectors of the covered area, the one of the
/// larger eigenvalue first; two zero vectors where the signed area is not positive.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn compute_principal_components(polys: &[Polygon]) -> (Vec2f, Vec2f) {
    let mut centroid_accumulator = Vec2f::zero();
    let mut second_moment_of_area_accumulator = Vec2f::zero();
    let mut second_moment_of_area_covariance_accumulator = 0.0f32;
    let mut area = 0.0f32;

    for poly in polys {
        // `poly.first_point()` is read before the loop; a polygon of fewer than three points adds
        // no triangle, so one without points reads nothing here.
        if poly.points.len() < 3 {
            continue;
        }
        let p0 = unscaled_f32(poly.first_point());
        for i in 2..poly.points.len() {
            let p1 = unscaled_f32(poly.points[i - 1]);
            let p2 = unscaled_f32(poly.points[i]);

            let sign = if cross2f(p1 - p0, p2 - p1) > 0.0 {
                1.0f32
            } else {
                -1.0f32
            };

            let (
                triangle_area,
                first_moment_of_area,
                second_moment_area,
                second_moment_of_area_covariance,
            ) = compute_moments_of_area_of_triangle(p0, p1, p2);
            area += sign * triangle_area;
            centroid_accumulator += first_moment_of_area * sign;
            second_moment_of_area_accumulator += second_moment_area * sign;
            second_moment_of_area_covariance_accumulator += sign * second_moment_of_area_covariance;
        }
    }

    if f64::from(area) <= 0.0 {
        return (Vec2f::zero(), Vec2f::zero());
    }

    let centroid = centroid_accumulator / area;
    let variance = second_moment_of_area_accumulator / area - centroid.cwise_product(&centroid);
    let covariance = f64::from(
        second_moment_of_area_covariance_accumulator / area - centroid.x() * centroid.y(),
    );

    if orca_crt::fabs(covariance) < EPSILON {
        let result = (vec2f(variance.x(), 0.0), vec2f(0.0, variance.y()));
        if variance.y() > variance.x() {
            return (result.1, result.0);
        }
        return result;
    }

    // The eigenvalues of | VarX Cov ; Cov VarY |. `(vx - vy)²` is `float`; `4.0f * covariance *
    // covariance` is `double`, so the sum, the root and the rest are `double`, narrowed at the end.
    let d = variance.x() - variance.y();
    let root = orca_crt::sqrt(f64::from(d * d) + 4.0 * covariance * covariance);
    let eigenvalue_a = (0.5 * (f64::from(variance.x() + variance.y()) + root)) as f32;
    let eigenvalue_b = (0.5 * (f64::from(variance.x() + variance.y()) - root)) as f32;
    let eigenvector_a = vec2f(
        (f64::from(eigenvalue_a - variance.y()) / covariance) as f32,
        1.0,
    );
    let eigenvector_b = vec2f(
        (f64::from(eigenvalue_b - variance.y()) / covariance) as f32,
        1.0,
    );

    if eigenvalue_a > eigenvalue_b {
        (eigenvector_a, eigenvector_b)
    } else {
        (eigenvector_b, eigenvector_a)
    }
}
