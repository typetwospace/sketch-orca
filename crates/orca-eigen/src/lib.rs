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
//! MSVC 14.42 compiles Eigen 5.0.1 (whose headers number it 3.5.0) with the C++ engine's flags
//! (`/O2`, `/fp:precise`, SSE2, no FMA): Eigen vectorises with SSE2 packets (two `double`s or four
//! `float`s), and where it does, the packets and their reduction order are the result, as
//! floating-point addition is not associative. Each operation here computes every coefficient
//! with the additions in Eigen's order (`redux.rs` says which), so the results match the engine's
//! bit for bit:
//!
//! - [`Vector`]: libslic3r's `Vec2d`, `Vec3d`, `Vec3f`, `Vec2crd`, `Vec3crd`, `Vec3i32` and the
//!   rest, with the coefficient-wise operations, `dot`, `squaredNorm`, `norm`, `normalized`,
//!   `cross`, `minCoeff`, `maxCoeff` and the casts (`vector.rs`);
//! - [`Matrix`]: `Matrix3d`, `Matrix3f`, `Matrix4d`, with products, `transpose`, `determinant`
//!   and the 3x3 `inverse` (`matrix.rs`);
//! - [`Transform3`]: `Transform3d` and `Transform3f` (`Affine`), with `translate`, `rotate`,
//!   `scale`, products, `inverse`, `linear()` and `computeRotationScaling` (`transform.rs`);
//! - [`AngleAxis`], [`Quaternion`] and [`euler_angles`]: the rotation matrices Geometry builds
//!   from Euler angles and takes apart again (`rotation.rs`);
//! - [`JacobiSvd3`]: the 3x3 SVD behind `computeRotationScaling` and `TransformationSVD`
//!   (`svd.rs`).
//!
//! Maths functions go through orca-crt (the static UCRT's `sin`, `cos`, `atan2`, `pow`), float
//! min and max through orca-stl (`std::min` and `std::max`, whose operand order SSE2's `minpd` and
//! `maxpd` reproduce), and float-to-integer casts through orca-crt's conversions (`cvttsd2si`).
//!
//! The files that carry Eigen's code say so in a dated block of their header, which names the
//! Eigen sources, and stay under the Mozilla Public License, version 2.0 (MPL-2.0;
//! `licenses/MPL-2.0.txt`) as its section 3 asks, inside this AGPL-3.0 program as its section 3.3
//! allows (`NOTICE`, the orca-eigen section). The probe file `geom.sop1` records what the engine's own Eigen computes, and
//! `tests/probes.rs` replays its Eigen records (`parity/SOP1.md`, "geom.sop1").

pub mod matrix;
pub mod redux;
pub mod rotation;
pub mod scalar;
pub mod svd;
pub mod transform;
pub mod vector;

pub use matrix::{Mat2, Mat3, Mat4, Matrix, Matrix2d, Matrix3d, Matrix3f, Matrix4d};
pub use rotation::{
    AngleAxis, AngleAxisd, QuatProduct, Quaternion, Quaterniond, euler_angles, stable_norm3,
};
pub use scalar::{Cast, Real, Scalar, hypot};
pub use svd::JacobiSvd3;
pub use transform::{Transform3, Transform3d, Transform3f};
pub use vector::{
    Vec2, Vec2crd, Vec2d, Vec2f, Vec2i32, Vec2i64, Vec3, Vec3crd, Vec3d, Vec3f, Vec3i32, Vec3i64,
    Vec4, Vec4d, Vec4f, Vector,
};

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
