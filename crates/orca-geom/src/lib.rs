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
//! `Line`, `MultiPoint` (Douglas-Peucker and Visvalingam), `ExPolygon` (without the medial axis),
//! `BoundingBox`, the convex hulls of `Geometry.cpp` and `Geometry/ConvexHull.cpp`,
//! `Geometry::Transformation`, and `scale_` and `unscale` with the command line's `SCALING_FACTOR`
//! of 1e-6. `coord_t` is `i64`.
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
//! - [`point`]: `coord_t` (`i64`) and `Point`.

pub mod clipper;
pub mod int128;
pub mod point;

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
