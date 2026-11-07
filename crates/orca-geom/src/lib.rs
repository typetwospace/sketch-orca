// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Clipper 6.4.2, ClipperUtils and libslic3r's geometry: points, polygons, ExPolygons, boxes, hulls.
//!
//! The geometry everything after the model's reading stands on: Clipper 6.4.2 as OrcaSlicer vendors
//! it (`deps_src/clipper`, compiled with `Slic3r::Point` as its `IntPoint`), with `Int128` as
//! `i128`; libslic3r's `ClipperUtils`; and libslic3r's geometry: `Point`, `Polygon`, `Polyline`,
//! `Line`, `MultiPoint` (Douglas-Peucker; Visvalingam is left out, as nothing calls it and its
//! code is undefined behaviour), `ExPolygon` (without the medial axis), `BoundingBox`, the
//! predicates, angles, transform builders and convex hulls of `Geometry.cpp` and
//! `Geometry/ConvexHull.cpp`, and `scale_` and `unscale` with the command line's
//! `SCALING_FACTOR` of 1e-6. `coord_t` is `i64`.
//! `Geometry::Transformation`, `Geometry::arrange` and the arc fitting of `Polyline` are not
//! here yet (`parity/HAZARDS.md`, orca-geom).
//!
//! Each module gives the results OrcaSlicer v2.4.2's code gives, bit for bit, which the records of
//! the probe file `geom.sop1` prove (`tests/probes.rs` replays them):
//!
//! - [`clipper`]: Clipper 6.4.2 whole: `Clipper` with every clip and fill type and `PolyTree`
//!   results, `ClipperOffset` with every join and end type, Minkowski sums, `SimplifyPolygon(s)`,
//!   `CleanPolygon(s)`, `Area`, `Orientation` and `PointInPolygon`. Its sorts and scanbeam queue go
//!   through `orca-stl`, its trigonometry and float-to-integer casts through `orca-crt`. What
//!   Clipper throws to its caller is a panic whose payload is a `ClipperException`.
//! - [`int128`]: `Int128`'s filtered predicates (Clipper's `SlopesEqual`, `Point.cpp`), as MSVC
//!   compiles them, with `i128` for the exact products.
//! - [`point`]: `coord_t` (`i64`), `Point`, `scale_` and `unscale`. `cwiseMin` and `cwiseMax`
//!   are Eigen's SSE2 ones, whose 64-bit compare takes the low halves signed, so a
//!   `BoundingBox` of coordinates past 2^31 can miss a point exactly as the engine's does.
//! - [`line`], [`multi_point`], [`polygon`], [`polyline`], [`expolygon`], [`bounding_box`]: the
//!   point containers and their helpers.
//! - [`clipper_utils`]: the offsets and boolean operations of `ClipperUtils`, on polygons,
//!   ExPolygons, polylines and lines, the variable-width offsets and `top_level_islands`. The
//!   C++ overloads are functions named for their argument types (`diff_ex_expolygons`); a
//!   path provider is a list of point slices ([`clipper_utils::PathsRef`]).
//! - [`geometry`], [`convex_hull`]: `Slic3r::Geometry`'s predicates, angles, transform builders
//!   and convex hulls, and the rotating calipers of `convex_polygons_intersect`.

pub mod bounding_box;
pub mod clipper;
pub mod clipper_utils;
pub mod convex_hull;
pub mod expolygon;
pub mod geometry;
pub mod int128;
pub mod line;
pub mod multi_point;
pub mod point;
pub mod polygon;
pub mod polyline;

/// The crate's name, as Cargo has it.
pub const NAME: &str = "orca-geom";

#[cfg(test)]
mod tests {
    use super::NAME;

    #[test]
    fn name_is_the_package_name() {
        assert_eq!(NAME, env!("CARGO_PKG_NAME"));
    }
}
