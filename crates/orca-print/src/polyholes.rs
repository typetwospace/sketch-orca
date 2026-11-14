// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.
//
// Carries OrcaSlicer code: src/libslic3r/PrintObject.cpp lines 160-186 (create_polyholes) of
// OrcaSlicer v2.4.2, translated on 2026-11-14 (orca-o3-print-1). Copyright the OrcaSlicer
// authors, the BambuStudio authors (Bambu Lab), the PrusaSlicer authors (Prusa Research), the
// SuperSlicer authors and the Slic3r authors (Alessandro Ranellucci), under the GNU Affero General
// Public License, version 3 (AGPL-3.0; LICENSE).

//! `create_polyholes`: the polygons that stand in for a round hole (SuperSlicer's polyholes),
//! a regular polygon of `max(3, round(4 r 0.4 / d))` edges circumscribing the circle, or five of
//! them each turned a fifth of an edge's angle when `multiple`.
//!
//! The arithmetic keeps the C++'s types: the turn and each corner's angle are `float`, so a
//! corner's cosine and sine are the UCRT's `cosf` and `sinf` (MSVC's `cos(float)` overload), the
//! radius is the scaled radius over `float(cos(PI / n))`, and each coordinate is the centre's
//! `coord_t` taken to `float` plus the `float` offset, rounded by `Point(double, double)`.
//! `PrintObject::_transform_hole_to_polyholes`, which calls it, is piece print-2's.

use orca_geom::point::{PI, Point, unscale};
use orca_geom::polygon::{Polygon, Polygons};

/// `create_polyholes(center, radius, nozzle_diameter, multiple)`: the polyhole of a hole of
/// `radius` around `center` for a nozzle of `nozzle_diameter` (both scaled), one polygon or, when
/// `multiple`, five turned copies in the order 0, 2, 4, 1, 3 of their turns. Each is clockwise.
#[must_use]
// The C++'s conversions: int to size_t, size_t and coord_t to float, double to float.
#[allow(
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation
)]
pub fn create_polyholes(
    center: Point,
    radius: i64,
    nozzle_diameter: i64,
    multiple: bool,
) -> Polygons {
    // n = max(round(2 * d), 3); // for 0.4mm nozzle
    let rounded = orca_crt::f64_to_i32(orca_crt::round(
        4.0 * unscale(radius) * 0.4 / unscale(nozzle_diameter),
    ));
    // std::max(3, (int)...) taken to an int, then to a size_t.
    let nb_edges: usize = orca_stl::max(3, rounded) as usize;
    // cylinder(h = h, r = d / cos (180 / n), $fn = n);
    //create x polyholes by rotation if multiple
    let mut nb_polyhole: i32 = 1;
    let mut rotation: f32 = 0.;
    if multiple {
        nb_polyhole = 5;
        // size_t * int is a size_t, which the float division takes to a float.
        rotation = 2. * (PI as f32) / (nb_edges.wrapping_mul(nb_polyhole as usize) as f32);
    }
    let mut list: Polygons = Vec::new();
    for _i_poly in 0..nb_polyhole {
        list.push(Polygon::default());
    }
    for i_poly in 0..nb_polyhole {
        let idx = if (i_poly % 2) == 0 {
            i_poly / 2
        } else {
            (nb_polyhole + 1) / 2 + i_poly / 2
        };
        let pts = &mut list[idx as usize];
        // The scaled radius (a coord_t) over a float, in float.
        let new_radius: f32 = radius as f32 / (orca_crt::cos(PI / nb_edges as f64) as f32);
        for i_edge in 0..nb_edges {
            let angle: f32 = rotation * (i_poly as f32)
                + ((PI as f32) * 2. * (i_edge as f32)) / (nb_edges as f32);
            // The coord_t plus the float is a float, which Point(double, double) rounds.
            pts.points.push(Point::from_f64(
                f64::from(center.x as f32 + new_radius * orca_crt::cosf(angle)),
                f64::from(center.y as f32 + new_radius * orca_crt::sinf(angle)),
            ));
        }
        pts.make_clockwise();
    }
    //alternate
    list
}

#[cfg(test)]
mod tests {
    use super::create_polyholes;
    use orca_geom::point::{Point, scaled};

    #[test]
    fn edge_count_follows_the_radius_over_the_nozzle() {
        // round(4 * 2.5 * 0.4 / 0.4) = 10 edges.
        let holes = create_polyholes(Point::new(0, 0), scaled(2.5), scaled(0.4), false);
        assert_eq!(holes.len(), 1);
        assert_eq!(holes[0].points.len(), 10);
        // A tiny hole still has three edges.
        let tiny = create_polyholes(Point::new(0, 0), scaled(0.01), scaled(0.4), false);
        assert_eq!(tiny[0].points.len(), 3);
    }

    #[test]
    fn multiple_gives_five_clockwise_polygons() {
        let holes = create_polyholes(Point::new(1000, -2000), scaled(1.0), scaled(0.4), true);
        assert_eq!(holes.len(), 5);
        for h in &holes {
            assert_eq!(h.points.len(), 4);
            assert!(h.is_clockwise());
        }
    }

    #[test]
    fn corners_sit_on_the_circumscribed_circle() {
        let r = scaled(2.0);
        let holes = create_polyholes(Point::new(0, 0), r, scaled(0.4), false);
        let n = holes[0].points.len();
        assert_eq!(n, 8);
        #[allow(clippy::cast_precision_loss)]
        let expected = r as f64 / orca_crt::cos(orca_geom::point::PI / n as f64);
        for p in &holes[0].points {
            #[allow(clippy::cast_precision_loss)]
            let d = orca_crt::sqrt((p.x as f64) * (p.x as f64) + (p.y as f64) * (p.y as f64));
            assert!(orca_crt::fabs(d - expected) < 2.0, "{d} vs {expected}");
        }
    }
}
