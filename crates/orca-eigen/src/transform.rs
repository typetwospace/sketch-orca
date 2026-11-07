// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Eigen code: Transform of Eigen/src/Geometry/Transform.h (Affine mode: translate,
// pretranslate, rotate, prerotate, scale, prescale, the products by a transform and by a vector,
// inverse, computeRotationScaling), of Eigen 5.0.1, translated on 2026-11-07. Their copyright is
// the Eigen authors' (Benoit Jacob, Gael Guennebaud, Hauke Heibel and the Eigen contributors),
// under the Mozilla Public License, version 2.0 (MPL-2.0; licenses/MPL-2.0.txt).
//
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0. If a copy of
// the MPL was not distributed with this file, You can obtain one at https://mozilla.org/MPL/2.0/.
// (Exhibit A of the MPL-2.0: the Eigen code here stays under it; the rest of the file is
// sketch-orca's, under the AGPL-3.0, as the MPL's section 3.3 allows.)

//! `Eigen::Transform<T, 3, Eigen::Affine, Eigen::DontAlign>`: libslic3r's `Transform3d` and
//! `Transform3f`, a full 4x4 matrix whose last row Eigen keeps `(0, 0, 0, 1)`.
//!
//! Each operation is Eigen's own sequence of matrix products (`matrix.rs`):
//!
//! - `translate(v)`: `translation += linear * v`, the product into a temporary first;
//! - `rotate(r)`: `linear = linear * r` (`r` a matrix, or the rotation matrix of a quaternion or
//!   an angle-axis); `prerotate(r)`: the top three rows replaced by `r` times them;
//! - `scale(v)`: each column of `linear` times `v(c)`; `prescale(v)` each top row times `v(r)`;
//! - `a * b`: `linear = a.linear * b.linear`, `translation = a.linear * b.translation +
//!   a.translation`;
//! - `t * v` for a 3-vector: the 4x4 matrix times `(v, 1)`, every row by packets (four rows fill
//!   whole packets of either type), so `((m(r,0) x + m(r,1) y) + m(r,2) z) + m(r,3)`;
//! - `inverse()`: `linear().inverse()` and the translation `-linv * translation`, with each
//!   term `(-linv(r, i)) * t(i)`.

use crate::matrix::{Mat3, Mat4, Matrix};
use crate::rotation::{AngleAxis, QuatProduct, Quaternion};
use crate::scalar::{Cast, Real};
use crate::svd::JacobiSvd3;
use crate::vector::{Vec3, Vector};

/// `Eigen::Transform<T, 3, Eigen::Affine, Eigen::DontAlign>`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform3<T> {
    /// `matrix()`.
    pub m: Mat4<T>,
}

/// libslic3r's `Transform3d`.
pub type Transform3d = Transform3<f64>;
/// libslic3r's `Transform3f`.
pub type Transform3f = Transform3<f32>;

impl<T: Real> Transform3<T> {
    /// `Identity()`.
    pub fn identity() -> Self {
        Transform3 {
            m: Mat4::identity(),
        }
    }
    /// `Transform(matrix)` from a 4x4 matrix, taken as it is.
    pub fn from_matrix(m: Mat4<T>) -> Self {
        Transform3 { m }
    }
    /// `Transform(linear)` from a 3x3 matrix: that linear part, no translation.
    pub fn from_linear(l: &Mat3<T>) -> Self {
        let mut t = Self::identity();
        t.set_linear(l);
        t
    }
    /// `linear()`: the top-left 3x3 block.
    pub fn linear(&self) -> Mat3<T> {
        Matrix::from_fn(|r, c| self.m.at(r, c))
    }
    /// `linear() = l`.
    pub fn set_linear(&mut self, l: &Mat3<T>) {
        for c in 0..3 {
            for r in 0..3 {
                self.m.set(r, c, l.at(r, c));
            }
        }
    }
    /// `translation()`: the top three coefficients of the last column.
    pub fn translation(&self) -> Vec3<T> {
        Vector([self.m.at(0, 3), self.m.at(1, 3), self.m.at(2, 3)])
    }
    /// `translation() = v`.
    pub fn set_translation(&mut self, v: &Vec3<T>) {
        for r in 0..3 {
            self.m.set(r, 3, v.0[r]);
        }
    }
    /// `makeAffine()`: the last row `(0, 0, 0, 1)`.
    pub fn make_affine(&mut self) {
        for c in 0..3 {
            self.m.set(3, c, T::ZERO);
        }
        self.m.set(3, 3, T::ONE);
    }
    /// `translate(v)`: `translation += linear * v`.
    pub fn translate(&mut self, v: &Vec3<T>) -> &mut Self {
        let p = self.linear().mul_vec(v);
        let t = self.translation() + p;
        self.set_translation(&t);
        self
    }
    /// `pretranslate(v)`: `translation += v`.
    pub fn pretranslate(&mut self, v: &Vec3<T>) -> &mut Self {
        let t = self.translation() + *v;
        self.set_translation(&t);
        self
    }
    /// `rotate(r)` by a rotation matrix: `linear = linear * r`.
    pub fn rotate(&mut self, r: &Mat3<T>) -> &mut Self {
        let l = self.linear().mul(r);
        self.set_linear(&l);
        self
    }
    /// `prerotate(r)`: the top three rows replaced by `r` times them.
    pub fn prerotate(&mut self, r: &Mat3<T>) -> &mut Self {
        let top: Matrix<T, 3, 4> = Matrix::from_fn(|i, c| self.m.at(i, c));
        let rows = r.mul(&top);
        for c in 0..4 {
            for i in 0..3 {
                self.m.set(i, c, rows.at(i, c));
            }
        }
        self
    }
    /// `scale(v)`: `linear = linear * v.asDiagonal()`.
    pub fn scale(&mut self, v: &Vec3<T>) -> &mut Self {
        let l = self.linear().mul_diagonal(v);
        self.set_linear(&l);
        self
    }
    /// `scale(s)`: `linear *= s`.
    pub fn scale_uniform(&mut self, s: T) -> &mut Self {
        let l = self.linear().scale(s);
        self.set_linear(&l);
        self
    }
    /// `prescale(v)`: `affine = v.asDiagonal() * affine`, each of the top three rows times `v(r)`.
    pub fn prescale(&mut self, v: &Vec3<T>) -> &mut Self {
        for c in 0..4 {
            for r in 0..3 {
                self.m.set(r, c, v.0[r].mul(self.m.at(r, c)));
            }
        }
        self
    }
    /// `self * o` (`transform_transform_product_impl`, Affine by Affine).
    pub fn mul(&self, o: &Self) -> Self {
        let (l1, l2) = (self.linear(), o.linear());
        let mut res = Self::identity();
        res.set_linear(&l1.mul(&l2));
        let t = l1.mul_vec(&o.translation()) + self.translation();
        res.set_translation(&t);
        res.make_affine();
        res
    }
    /// `self * v` for a point (`transform_right_product_impl<.., 2, 1>`): the 4x4 matrix times
    /// `(v, 1)`, the first three coefficients.
    pub fn mul_point(&self, v: &Vec3<T>) -> Vec3<T> {
        self.m.mul_vec(&v.homogeneous()).head3()
    }
    /// `inverse()` with the Affine hint: `linear().inverse()`, then `-linv * translation`.
    pub fn inverse(&self) -> Self {
        let linv = self.linear().inverse();
        let mut res = Self::identity();
        res.set_linear(&linv);
        let t = (-linv).mul_vec(&self.translation());
        res.set_translation(&t);
        res.make_affine();
        res
    }
    /// `cast<U>()`: each coefficient of the 4x4 matrix.
    pub fn cast<U: Real>(&self) -> Transform3<U>
    where
        T: Cast<U>,
    {
        Transform3 { m: self.m.cast() }
    }
    /// `isApprox(o, prec)` of the two 4x4 matrices.
    pub fn is_approx(&self, o: &Self, prec: T) -> bool {
        self.m.is_approx(&o.m, prec)
    }
}

impl<T: QuatProduct> Transform3<T> {
    /// `rotate(q)` by a quaternion: its `toRotationMatrix()`.
    pub fn rotate_quaternion(&mut self, q: &Quaternion<T>) -> &mut Self {
        self.rotate(&q.to_rotation_matrix())
    }
    /// `rotate(aa)` by an angle-axis: its `toRotationMatrix()`.
    pub fn rotate_angle_axis(&mut self, aa: &AngleAxis<T>) -> &mut Self {
        self.rotate(&aa.to_rotation_matrix())
    }
    /// `computeRotationScaling(&rotation, &scaling)`: the linear part as rotation times scaling,
    /// from its SVD `U S V^T`: `x = -1` when `det(U V^T) < 0` (else 1) scales the last singular
    /// value and U's last column; `scaling = (V diag(S)) V^T`, `rotation = U' V^T`. `None` when
    /// the linear part is not finite, where Eigen leaves its SVD unset (`JacobiSvd3::compute`).
    pub fn compute_rotation_scaling(&self) -> Option<(Mat3<T>, Mat3<T>)> {
        let svd = JacobiSvd3::compute(&self.linear())?;
        let vt = svd.v.transpose();
        let x = if svd.u.mul(&vt).determinant() < T::ZERO {
            T::ONE.neg()
        } else {
            T::ONE
        };
        let mut sv = svd.singular_values;
        sv.0[2] = sv.0[2].mul(x);
        let scaling = svd.v.mul_diagonal(&sv).mul(&vt);
        let mut m = svd.u;
        m.set_col(2, m.col(2) * x);
        let rotation = m.mul(&vt);
        Some((rotation, scaling))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translate_rotate_scale() {
        let mut t = Transform3d::identity();
        t.translate(&Vec3::new(1.0, 2.0, 3.0));
        t.scale(&Vec3::new(2.0, 2.0, 2.0));
        assert_eq!(
            t.mul_point(&Vec3::new(1.0, 1.0, 1.0)),
            Vec3::new(3.0, 4.0, 5.0)
        );
        let inv = t.inverse();
        assert_eq!(
            inv.mul_point(&Vec3::new(3.0, 4.0, 5.0)),
            Vec3::new(1.0, 1.0, 1.0)
        );
        let (r, s) = t.compute_rotation_scaling().unwrap();
        assert!(r.is_approx(&Mat3::identity(), 1e-12));
        assert!(s.is_approx(&Mat3::identity().scale(2.0), 1e-12));
    }
}
