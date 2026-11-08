// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: class Slic3r::Geometry::Transformation of src/libslic3r/Geometry.hpp
// and Geometry.cpp (extract_rotation_matrix, extract_scale, extract_rotation_scale, the getters
// and setters of the offset, rotation, scaling factor and mirror, get_matrix_no_offset and the
// product) and transform3d_from_string, of OrcaSlicer v2.4.2, translated on 2026-11-07. Copyright
// the OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and the PrusaSlicer authors (Prusa
// Research), under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! `Geometry::Transformation`: an affine matrix with its offset, rotation, scaling factor and
//! mirror taken apart and put back as the C++ does, through Eigen's `computeRotationScaling`
//! (orca-eigen's 3x3 Jacobi SVD).
//!
//! The class holds only the matrix; every getter but the offset decomposes it again, so a setter
//! changes the bits of the whole linear part even where it sets a value the matrix already has.
//! Instance and volume matrices go into the front end's dumps bit for bit, so each setter keeps
//! the C++ order of products. A matrix whose linear part is not finite has no SVD in orca-eigen;
//! where Eigen would compute with an unset SVD, [`decompose`] gives NaN matrices, which compare
//! and propagate as the C++ garbage would not be guaranteed to: such a matrix never reaches the
//! front end from a model the readers accept (`parity/HAZARDS.md`, orca-model).

use orca_eigen::{Matrix3d, Transform3d, Vec3d, Vector};
use orca_geom::geometry::{
    angle_to_0_2pi, extract_euler_angles_of, rotation_transform, scale_transform,
};
use orca_geom::point::PI;

/// The axes of `Slic3r::Axis`.
pub const X: usize = 0;
/// `Y`.
pub const Y: usize = 1;
/// `Z`.
pub const Z: usize = 2;

/// `computeRotationScaling(&rotation, &scale)` of the matrix's linear part: (rotation, scale).
fn decompose(trafo: &Transform3d) -> (Matrix3d, Matrix3d) {
    trafo.compute_rotation_scaling().unwrap_or_else(|| {
        let nan = Matrix3d::from_fn(|_, _| f64::NAN);
        (nan, nan)
    })
}

/// `extract_rotation_matrix(trafo)`: `Transform3d(rotation)`.
fn extract_rotation_matrix(trafo: &Transform3d) -> Transform3d {
    Transform3d::from_linear(&decompose(trafo).0)
}

/// `extract_scale(trafo)`: `Transform3d(scale)`.
fn extract_scale(trafo: &Transform3d) -> Transform3d {
    Transform3d::from_linear(&decompose(trafo).1)
}

/// `extract_rotation_scale(trafo)`.
fn extract_rotation_scale(trafo: &Transform3d) -> (Transform3d, Transform3d) {
    let (r, s) = decompose(trafo);
    (Transform3d::from_linear(&r), Transform3d::from_linear(&s))
}

/// libslic3r's `is_approx(value, test_value)` with the default `EPSILON`.
fn is_approx(value: f64, test_value: f64) -> bool {
    (value - test_value).abs() < orca_geom::point::EPSILON
}

/// `Geometry::Transformation`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transformation {
    m_matrix: Transform3d,
}

impl Default for Transformation {
    /// `Transformation()`: the identity.
    fn default() -> Self {
        Transformation {
            m_matrix: Transform3d::identity(),
        }
    }
}

impl Transformation {
    /// `Transformation(transform)`.
    #[must_use]
    pub fn new(transform: &Transform3d) -> Self {
        Transformation {
            m_matrix: *transform,
        }
    }

    /// `get_offset()`.
    #[must_use]
    pub fn get_offset(&self) -> Vec3d {
        self.m_matrix.translation()
    }

    /// `get_offset(axis)`.
    #[must_use]
    pub fn get_offset_axis(&self, axis: usize) -> f64 {
        self.get_offset()[axis]
    }

    /// `set_offset(offset)`.
    pub fn set_offset(&mut self, offset: &Vec3d) {
        self.m_matrix.set_translation(offset);
    }

    /// `set_offset(axis, offset)`.
    pub fn set_offset_axis(&mut self, axis: usize, offset: f64) {
        self.m_matrix.m.set(axis, 3, offset);
    }

    /// `get_rotation()`: the Euler angles of the rotation of `computeRotationScaling`, through
    /// the `Transform3d` overload of `extract_euler_angles` (its columns normalized first).
    #[must_use]
    pub fn get_rotation(&self) -> Vec3d {
        extract_euler_angles_of(&extract_rotation_matrix(&self.m_matrix))
    }

    /// `get_rotation(axis)`.
    #[must_use]
    pub fn get_rotation_axis(&self, axis: usize) -> f64 {
        self.get_rotation()[axis]
    }

    /// `get_rotation_matrix()`.
    #[must_use]
    pub fn get_rotation_matrix(&self) -> Transform3d {
        extract_rotation_matrix(&self.m_matrix)
    }

    /// `set_rotation(rotation)`: `rotation_transform(rotation) * extract_scale(m_matrix)`, the
    /// offset kept.
    pub fn set_rotation(&mut self, rotation: &Vec3d) {
        let offset = self.get_offset();
        self.m_matrix = rotation_transform(rotation).mul(&extract_scale(&self.m_matrix));
        self.m_matrix.set_translation(&offset);
    }

    /// `set_rotation(axis, rotation)`: the angle brought to [0, 2 pi) (a full turn is 0), the
    /// other angles of the current rotation kept.
    pub fn set_rotation_axis(&mut self, axis: usize, rotation: f64) {
        let mut rotation = angle_to_0_2pi(rotation);
        if is_approx(rotation.abs(), 2.0 * PI) {
            rotation = 0.0;
        }
        let (curr_rotation, scale) = extract_rotation_scale(&self.m_matrix);
        let mut angles = extract_euler_angles_of(&curr_rotation);
        angles[axis] = rotation;
        let offset = self.get_offset();
        self.m_matrix = rotation_transform(&angles).mul(&scale);
        self.m_matrix.set_translation(&offset);
    }

    /// `get_scaling_factor()`: the absolute diagonal of the scale.
    #[must_use]
    pub fn get_scaling_factor(&self) -> Vec3d {
        let scale = extract_scale(&self.m_matrix);
        Vector([
            scale.m.at(0, 0).abs(),
            scale.m.at(1, 1).abs(),
            scale.m.at(2, 2).abs(),
        ])
    }

    /// `get_scaling_factor(axis)`.
    #[must_use]
    pub fn get_scaling_factor_axis(&self, axis: usize) -> f64 {
        self.get_scaling_factor()[axis]
    }

    /// `is_scaling_uniform()`.
    #[must_use]
    pub fn is_scaling_uniform(&self) -> bool {
        let scale = self.get_scaling_factor();
        (scale.x() - scale.y()).abs() < 1e-8 && (scale.x() - scale.z()).abs() < 1e-8
    }

    /// `set_scaling_factor(scaling_factor)`: `extract_rotation_matrix(m_matrix) *
    /// scale_transform(scaling_factor)`, the offset kept.
    pub fn set_scaling_factor(&mut self, scaling_factor: &Vec3d) {
        let offset = self.get_offset();
        self.m_matrix =
            extract_rotation_matrix(&self.m_matrix).mul(&scale_transform(scaling_factor));
        self.m_matrix.set_translation(&offset);
    }

    /// `set_scaling_factor(axis, scaling_factor)`.
    pub fn set_scaling_factor_axis(&mut self, axis: usize, scaling_factor: f64) {
        let (rotation, mut scale) = extract_rotation_scale(&self.m_matrix);
        scale.m.set(axis, axis, scaling_factor);
        let offset = self.get_offset();
        self.m_matrix = rotation.mul(&scale);
        self.m_matrix.set_translation(&offset);
    }

    /// `get_mirror()`: the signs of the scale's diagonal (`s / |s|`, NaN for 0).
    #[must_use]
    pub fn get_mirror(&self) -> Vec3d {
        let scale = extract_scale(&self.m_matrix);
        let d = |k: usize| scale.m.at(k, k) / scale.m.at(k, k).abs();
        Vector([d(0), d(1), d(2)])
    }

    /// `get_mirror(axis)`.
    #[must_use]
    pub fn get_mirror_axis(&self, axis: usize) -> f64 {
        self.get_mirror()[axis]
    }

    /// `is_left_handed()`: the linear part's determinant below 0.
    #[must_use]
    pub fn is_left_handed(&self) -> bool {
        self.m_matrix.linear().determinant() < 0.0
    }

    /// `set_mirror(mirror)`.
    pub fn set_mirror(&mut self, mirror: &Vec3d) {
        let mut copy = *mirror;
        let abs_mirror = copy.cwise_abs();
        for i in 0..3 {
            if abs_mirror[i] == 0.0 {
                copy[i] = 1.0;
            } else if abs_mirror[i] != 1.0 {
                copy[i] /= abs_mirror[i];
            }
        }
        let (rotation, mut scale) = extract_rotation_scale(&self.m_matrix);
        let curr_scales = Vector([scale.m.at(0, 0), scale.m.at(1, 1), scale.m.at(2, 2)]);
        let signs = curr_scales.cwise_product(&copy);
        for k in 0..3 {
            if signs[k] < 0.0 {
                let v = scale.m.at(k, k);
                scale.m.set(k, k, -v);
            }
        }
        let offset = self.get_offset();
        self.m_matrix = rotation.mul(&scale);
        self.m_matrix.set_translation(&offset);
    }

    /// `set_mirror(axis, mirror)`.
    pub fn set_mirror_axis(&mut self, axis: usize, mut mirror: f64) {
        let abs_mirror = mirror.abs();
        if abs_mirror == 0.0 {
            mirror = 1.0;
        } else if abs_mirror != 1.0 {
            mirror /= abs_mirror;
        }
        let (rotation, mut scale) = extract_rotation_scale(&self.m_matrix);
        let curr_scale = scale.m.at(axis, axis);
        let sign = curr_scale * mirror;
        if sign < 0.0 {
            scale.m.set(axis, axis, -curr_scale);
        }
        let offset = self.get_offset();
        self.m_matrix = rotation.mul(&scale);
        self.m_matrix.set_translation(&offset);
    }

    /// `reset()`.
    pub fn reset(&mut self) {
        self.m_matrix = Transform3d::identity();
    }

    /// `reset_offset()`.
    pub fn reset_offset(&mut self) {
        self.set_offset(&Vec3d::zero());
    }

    /// `get_matrix()`.
    #[must_use]
    pub fn get_matrix(&self) -> &Transform3d {
        &self.m_matrix
    }

    /// `get_matrix_no_offset()`: a copy with the offset reset.
    #[must_use]
    pub fn get_matrix_no_offset(&self) -> Transform3d {
        let mut copy = *self;
        copy.reset_offset();
        copy.m_matrix
    }

    /// `set_matrix(transform)`.
    pub fn set_matrix(&mut self, transform: &Transform3d) {
        self.m_matrix = *transform;
    }

    /// `operator *`: the product of the matrices.
    #[must_use]
    pub fn mul(&self, other: &Transformation) -> Transformation {
        Transformation::new(&self.m_matrix.mul(&other.m_matrix))
    }
}

/// `transform3d_from_string(transform_str)`: sixteen numbers split at runs of spaces
/// (`boost::split` with `token_compress_on`), read with `atof` row by row; anything else gives
/// the identity.
#[must_use]
pub fn transform3d_from_string(transform_str: &str) -> Transform3d {
    let mut transform = Transform3d::identity();
    if !transform_str.is_empty() {
        let elements = split_compress_spaces(transform_str);
        if elements.len() == 16 {
            let mut i = 0;
            for r in 0..4 {
                for c in 0..4 {
                    transform
                        .m
                        .set(r, c, orca_fmt::parse::atof(elements[i].as_bytes()));
                    i += 1;
                }
            }
        }
    }
    transform
}

/// `boost::split(out, s, is_any_of(" "), token_compress_on)`: a leading or trailing space gives
/// an empty first or last token, and a run of spaces counts as one.
#[must_use]
pub fn split_compress_spaces(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b' ' {
            out.push(&s[start..i]);
            while i < bytes.len() && bytes[i] == b' ' {
                i += 1;
            }
            start = i;
        } else {
            i += 1;
        }
    }
    out.push(&s[start..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_keeps_empty_ends() {
        assert_eq!(split_compress_spaces("a  b"), vec!["a", "b"]);
        assert_eq!(split_compress_spaces(" a b "), vec!["", "a", "b", ""]);
        assert_eq!(split_compress_spaces(""), vec![""]);
    }

    #[test]
    fn offsets_are_the_translation() {
        let mut t = Transformation::default();
        t.set_offset(&Vec3d::new(1.0, 2.0, 3.0));
        assert_eq!(t.get_offset(), Vec3d::new(1.0, 2.0, 3.0));
        assert_eq!(t.get_matrix_no_offset(), Transform3d::identity());
    }
}
