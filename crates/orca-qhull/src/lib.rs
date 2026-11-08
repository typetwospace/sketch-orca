// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Qhull 2015.2 (libqhull_r 7.2.0 and libqhullcpp), as OrcaSlicer builds it from deps_src/qhull.
//!
//! Qhull 2015.2 as the C++ engine links it (`qhullstatic`, built from OrcaSlicer's
//! `deps_src/qhull`), translated statement by statement for the one run the engine makes:
//! `its_convex_hull` (`TriangleMesh.cpp`) calls `orgQhull::Qhull`'s
//! `runQhull("", 3, n, points, "Qt")`, a 3-d convex hull with Qhull's default merging (`C-0`:
//! the facets of each new cone that are not clearly convex merge as they appear) and its facets
//! triangulated. [`convex_hull`] makes that run and returns what the wrapper's `QhullFacetList`
//! walks: each facet's vertices (id and input point) and normal, and the messages `~Qhull` prints.
//!
//! - `set`: Qhull's sets (`qset_r.c`) over arena indices.
//! - `qh`: the data structures and the facet and vertex lists (`libqhull_r.h`, `poly_r.c`).
//! - `geom`: distances, hyperplanes, centrums and the searches for the best facet (`geom_r.c`,
//!   `geom2_r.c`).
//! - `poly`: the initial simplex, the new facets of a point and their matching, the final check
//!   of the outer planes and the triangulation (`poly_r.c`, `poly2_r.c`).
//! - `libqhull`: the build, a point at a time, and the partition of the points (`libqhull_r.c`).
//! - `merge`: the merging of the new facets (`merge_r.c`).
//! - `hull`: the set-up, the run and its result (`global_r.c`, `libqhullcpp`'s `Qhull.cpp`).
//!
//! The options the run uses decide which code exists here: what they never reach is left out
//! where it stands, with the option named, and the few branches of reached functions that they
//! never take return [`Error::Unported`]. Qhull's statistics and traces print nothing in this run
//! and are left out, but for the two counters the build reads (`Ztotmerge`, `Zsetplane`).
//! Qhull's `qsort` calls go through `orca-crt`'s `qsort`, the static UCRT's own, so equal keys
//! keep the C's order. Facets, vertices and ridges live in arenas indexed in the order Qhull
//! makes them; Qhull's memory pool and the addresses it hands out play no part, but in the two
//! places `parity/HAZARDS.md` (orca-qhull) records: the hash of a duplicate ridge, and the freed
//! normal of a triangulated facet with no owner. The probe file `qhull.sop1` records the oracle;
//! `tests/probes.rs` replays it.

mod geom;
mod hull;
mod libqhull;
mod merge;
mod poly;
mod qh;
mod set;

pub use hull::{Facet, Hull, Vertex, convex_hull};
pub use qh::Error;

/// The crate's name, as Cargo has it.
pub const NAME: &str = "orca-qhull";

#[cfg(test)]
mod tests {
    use super::NAME;

    #[test]
    fn name_is_the_package_name() {
        assert_eq!(NAME, env!("CARGO_PKG_NAME"));
    }
}
