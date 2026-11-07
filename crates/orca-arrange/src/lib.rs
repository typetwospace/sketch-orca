// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Arrangement: libnest2d, Arrange.cpp, NLopt 2.5.0 and Boost.Geometry's R*-tree.
//!
//! Arrangement as the C++ engine places objects on the bed: libnest2d (`deps_src/libnest2d`, with
//! libslic3r's geometry and NLopt as its optimiser), libslic3r's `Arrange.cpp`, NLopt 2.5.0's
//! subplex and Nelder-Mead (`sbplx`, `nldrmd`, `redblack`, `stop` and the API) and Boost.Geometry's
//! R*-tree (Boost 1.84.0's `geometry/index`).
//!
//! The o2-arrange task of phase O2 fills this crate; its records are in the probe file
//! `arrange.sop1`, which `tests/probes.rs` replays.
//!
//! Until that task runs, the crate holds only its name, and `tests/probes.rs` passes trivially
//! while its probe file is a stub (HEAD and PRUN only).

/// The crate's name, as Cargo has it.
pub const NAME: &str = "orca-arrange";

#[cfg(test)]
mod tests {
    use super::NAME;

    #[test]
    fn name_is_the_package_name() {
        assert_eq!(NAME, env!("CARGO_PKG_NAME"));
    }
}
