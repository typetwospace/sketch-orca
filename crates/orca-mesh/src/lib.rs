// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Triangle meshes: admesh's reading and repair, and libslic3r's TriangleMesh with its transforms.
//!
//! Triangle meshes as the C++ engine reads and repairs them, translated statement by statement:
//!
//! - [`admesh`]: admesh (`deps_src/admesh`): the STL reader ([`admesh::stlinit`]), the facet
//!   connectivity and its repairs ([`admesh::connect`]), the facets' orientation
//!   ([`admesh::normals`]), the shared vertices ([`admesh::shared`]) and the measures and
//!   transforms of `util.cpp` ([`admesh::util`]). admesh reads files with the C runtime's stdio
//!   (`fopen`, `fread`, `fscanf`, `fgets`), which `orca-fmt`'s `stdio` and `scanf` modules
//!   reproduce.
//! - [`triangle_mesh`]: libslic3r's `TriangleMesh` (`TriangleMesh.cpp`): the repair on import, the
//!   statistics, the transforms, the bounding boxes, the split and the convex hulls (the 3D one
//!   through `orca-qhull`).
//! - [`its`]: the `its_*` helpers of `TriangleMesh.cpp`, `MeshSplitImpl.hpp` and `stl.h` that
//!   those reach (the volume, the bounding box, the face neighbours, the split into patches, the
//!   transforms, the projections for 2D hulls).
//! - [`eigen_lu`]: the determinant Eigen takes of a block of runtime size, by LU factorisation.
//! - [`log`]: the engine's `BOOST_LOG_TRIVIAL` messages these send, to a sink the caller sets.
//!
//! Floats stay `f32` where the C++ has `float`, and every mixed `float`/`double` expression keeps
//! the C++ promotions. Where the C++ reads or writes out of bounds or races, the Rust code does
//! what the oracle shows for the inputs that reach it and `parity/HAZARDS.md` (orca-mesh) records
//! the case. The probe file `mesh.sop1` records the oracle; `tests/probes.rs` replays it.

pub mod admesh;
pub mod eigen_lu;
pub mod its;
pub mod log;
pub mod triangle_mesh;

/// The crate's name, as Cargo has it.
pub const NAME: &str = "orca-mesh";

#[cfg(test)]
mod tests {
    use super::NAME;

    #[test]
    fn name_is_the_package_name() {
        assert_eq!(NAME, env!("CARGO_PKG_NAME"));
    }
}
