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
//! Triangle meshes as the C++ engine reads and repairs them: admesh (`deps_src/admesh`: the STL
//! reader's passes, the facet connectivity, the repair steps and the statistics) and libslic3r's
//! `TriangleMesh` (`TriangleMesh.cpp`), with its transforms, its bounding boxes and its convex hull
//! through `orca-qhull`. admesh reads files with the C runtime's stdio (`fopen`, `fread`,
//! `fscanf`), which `orca-fmt`'s `stdio` and `scanf` modules reproduce.
//!
//! The o2-mesh task of phase O2 fills this crate; its records are in the probe file `mesh.sop1`,
//! and those of the C runtime's stdio in `cstdio.sop1`. `tests/probes.rs` replays both until
//! o2-mesh gives `cstdio.sop1` a test of its own.
//!
//! Until that task runs, the crate holds only its name, and `tests/probes.rs` passes trivially
//! while its probe file is a stub (HEAD and PRUN only).

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
