// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The Eigen 5.0.1 operations libslic3r's geometry reaches, evaluated as MSVC 14.42 compiles them.
//!
//! The Eigen operations libslic3r's geometry reaches, evaluated as MSVC 14.42 compiles Eigen 5.0.1
//! with the C++ engine's flags (`/O2`, `/fp:precise`, SSE2): fixed-size vectors and matrices of
//! `float`, `double` and `coord_t` (`Vec2`, `Vec3`, `Matrix3d`, `Matrix4d`), `Transform3d` and
//! `Affine3d` products, inverses and `linear()`, `AngleAxis` and the rotation matrices `Geometry`
//! builds from Euler angles, and `norm`, `squaredNorm`, `dot`, `cross`, `normalized`, the
//! coefficient-wise operations, `minCoeff`, `maxCoeff` and the casts. Where Eigen vectorises, the
//! SSE2 packets and their reduction order are the result (`parity/HAZARDS.md`, "Geometry kernels").
//!
//! The dependency build pins Eigen 5.0.1 (`<work>/deps/dep_Eigen-prefix/src/dep_Eigen`), whose
//! headers number it 3.5.0. The o2-geom task of phase O2 fills this crate; its records are in the
//! probe file `geom.sop1`, which `tests/probes.rs` replays.
//!
//! Until that task runs, the crate holds only its name, and `tests/probes.rs` passes trivially
//! while its probe file is a stub (HEAD and PRUN only).

/// The crate's name, as Cargo has it.
pub const NAME: &str = "orca-eigen";

#[cfg(test)]
mod tests {
    use super::NAME;

    #[test]
    fn name_is_the_package_name() {
        assert_eq!(NAME, env!("CARGO_PKG_NAME"));
    }
}
