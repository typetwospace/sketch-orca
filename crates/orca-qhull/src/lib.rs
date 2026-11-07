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
//! Qhull 2015.2 (`libqhull_r` 7.2.0 and `libqhullcpp`) as the C++ engine links it, built from
//! OrcaSlicer's `deps_src/qhull` as `qhullstatic`: the convex hulls `TriangleMesh` and the model
//! take, with Qhull's merging and its `qsort` calls (`merge_r.c`, `mem_r.c`, `io_r.c`), which go
//! through `orca-crt`'s `qsort`, the static UCRT's own.
//!
//! The o2-mesh task of phase O2 fills this crate; its records are in the probe file `qhull.sop1`,
//! which `tests/probes.rs` replays.
//!
//! Until that task runs, the crate holds only its name, and `tests/probes.rs` passes trivially
//! while its probe file is a stub (HEAD and PRUN only).

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
