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
//! The o2-geom task of phase O2 fills this crate; its records are in the probe file `geom.sop1`,
//! which `tests/probes.rs` replays.
//!
//! Until that task runs, the crate holds only its name, and `tests/probes.rs` passes trivially
//! while its probe file is a stub (HEAD and PRUN only).

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
