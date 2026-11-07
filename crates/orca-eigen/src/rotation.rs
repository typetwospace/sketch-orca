// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Eigen code: AngleAxis of Eigen/src/Geometry/AngleAxis.h, Quaternion of
// Eigen/src/Geometry/Quaternion.h with the SSE2 quaternion products of
// Eigen/src/Geometry/arch/Geometry_SIMD.h, eulerAngles of Eigen/src/Geometry/EulerAngles.h and
// stableNorm of Eigen/src/Core/StableNorm.h, of Eigen 5.0.1, translated on 2026-11-07. Their
// copyright is the Eigen authors' (Benoit Jacob, Gael Guennebaud, Rohit Garg and the Eigen
// contributors), under the Mozilla Public License, version 2.0 (MPL-2.0; licenses/MPL-2.0.txt).
//
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0. If a copy of
// the MPL was not distributed with this file, You can obtain one at https://mozilla.org/MPL/2.0/.
// (Exhibit A of the MPL-2.0: the Eigen code here stays under it; the rest of the file is
// sketch-orca's, under the AGPL-3.0, as the MPL's section 3.3 allows.)

//! Rotations: `Eigen::AngleAxis`, `Eigen::Quaternion` and `eulerAngles`, as libslic3r's Geometry
//! builds and takes apart its rotation matrices.
//!
//! `Geometry::rotation_transform` and `assemble_transform` rotate by
//! `AngleAxisd(z, UnitZ()) * AngleAxisd(y, UnitY()) * AngleAxisd(x, UnitX())`: each `AngleAxis`
//! becomes a quaternion (`w = cos(angle / 2)`, `xyz = sin(angle / 2) * axis`), the product of two
//! is a quaternion, and `Transform::rotate` multiplies by its rotation matrix. With SSE, Eigen
//! multiplies quaternions with packets (Geometry_SIMD.h), in its own order of the sixteen
//! products, one order for `double` and another for `float` ([`Real::quat_product`]).

use crate::matrix::Mat3;
use crate::scalar::{Real, hypot};
use crate::vector::{Vec3, Vector};

/// The SSE2 quaternion product of each floating-point type (Geometry_SIMD.h), `[x, y, z, w]`.
pub trait QuatProduct: Real {
    /// `a * b`.
    fn quat_product(a: &[Self; 4], b: &[Self; 4]) -> [Self; 4];
}

impl QuatProduct for f64 {
    /// Two `Packet2d` halves: `t1 = aw*b.xy + ay*b.zw`, `t2 = az*b.xy - ax*b.zw`,
    /// `xy = t1 -/+ swap(t2)` (`paddsub`), then `t1 = aw*b.zw - ay*b.xy`, `t2 = az*b.zw + ax*b.xy`,
    /// `zw = swap(swap(t1) -/+ t2)`.
    fn quat_product(a: &[f64; 4], b: &[f64; 4]) -> [f64; 4] {
        let [ax, ay, az, aw] = *a;
        let [bx, by, bz, bw] = *b;
        [
            (aw * bx + ay * bz) - (az * by - ax * bw),
            (aw * by + ay * bw) + (az * bx - ax * bz),
            (aw * bz - ay * bx) + (az * bw + ax * by),
            (aw * bw - ay * by) - (az * bz + ax * bx),
        ]
    }
}

impl QuatProduct for f32 {
    /// One `Packet4f`: `a * b.wwww - a.zxyx * b.yzxx`, plus `a.yzxz * b.zxyz + a.wwwy * b.xyzy`
    /// with the last lane's sign flipped.
    fn quat_product(a: &[f32; 4], b: &[f32; 4]) -> [f32; 4] {
        let [ax, ay, az, aw] = *a;
        let [bx, by, bz, bw] = *b;
        [
            (ax * bw - az * by) + (ay * bz + aw * bx),
            (ay * bw - ax * bz) + (az * bx + aw * by),
            (az * bw - ay * bx) + (ax * by + aw * bz),
            (aw * bw - ax * bx) + -(az * bz + ay * by),
        ]
    }
}

/// `Eigen::Quaternion<T>`, its coefficients stored `[x, y, z, w]` as Eigen stores them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quaternion<T> {
    /// `coeffs()`: `x, y, z, w`.
    pub coeffs: [T; 4],
}

/// `Eigen::Quaterniond`.
pub type Quaterniond = Quaternion<f64>;

impl<T: QuatProduct> Quaternion<T> {
    /// `Quaternion(w, x, y, z)`, Eigen's argument order.
    pub fn new(w: T, x: T, y: T, z: T) -> Self {
        Quaternion {
            coeffs: [x, y, z, w],
        }
    }
    /// `Identity()`.
    pub fn identity() -> Self {
        Self::new(T::ONE, T::ZERO, T::ZERO, T::ZERO)
    }
    /// `x()`.
    pub fn x(&self) -> T {
        self.coeffs[0]
    }
    /// `y()`.
    pub fn y(&self) -> T {
        self.coeffs[1]
    }
    /// `z()`.
    pub fn z(&self) -> T {
        self.coeffs[2]
    }
    /// `w()`.
    pub fn w(&self) -> T {
        self.coeffs[3]
    }
    /// `vec()`: `(x, y, z)`.
    pub fn vec(&self) -> Vec3<T> {
        Vector([self.coeffs[0], self.coeffs[1], self.coeffs[2]])
    }
    /// `Quaternion(aa)` (`operator=(const AngleAxisType&)`): `w = cos(angle / 2)` and
    /// `xyz = sin(angle / 2) * axis`, the half angle `T(0.5) * angle`.
    pub fn from_angle_axis(aa: &AngleAxis<T>) -> Self {
        let ha = T::from_f64(0.5).mul(aa.angle);
        let s = ha.sin();
        Quaternion {
            coeffs: [
                s.mul(aa.axis.0[0]),
                s.mul(aa.axis.0[1]),
                s.mul(aa.axis.0[2]),
                ha.cos(),
            ],
        }
    }
    /// `Quaternion(mat)` from a rotation matrix (`quaternionbase_assign_impl<Other, 3, 3>`, Ken
    /// Shoemake's algorithm): through the trace when it is positive, else through the largest
    /// diagonal coefficient.
    pub fn from_rotation_matrix(mat: &Mat3<T>) -> Self {
        let half = T::from_f64(0.5);
        let mut q = [T::ZERO; 4];
        let mut t = mat.trace();
        if t > T::ZERO {
            t = t.add(T::ONE).sqrt();
            q[3] = half.mul(t);
            t = half.div(t);
            q[0] = mat.at(2, 1).sub(mat.at(1, 2)).mul(t);
            q[1] = mat.at(0, 2).sub(mat.at(2, 0)).mul(t);
            q[2] = mat.at(1, 0).sub(mat.at(0, 1)).mul(t);
        } else {
            let mut i = 0;
            if mat.at(1, 1) > mat.at(0, 0) {
                i = 1;
            }
            if mat.at(2, 2) > mat.at(i, i) {
                i = 2;
            }
            let j = (i + 1) % 3;
            let k = (j + 1) % 3;
            t = mat
                .at(i, i)
                .sub(mat.at(j, j))
                .sub(mat.at(k, k))
                .add(T::ONE)
                .sqrt();
            q[i] = half.mul(t);
            t = half.div(t);
            q[3] = mat.at(k, j).sub(mat.at(j, k)).mul(t);
            q[j] = mat.at(j, i).add(mat.at(i, j)).mul(t);
            q[k] = mat.at(k, i).add(mat.at(i, k)).mul(t);
        }
        Quaternion { coeffs: q }
    }
    /// `self * o`, the SSE2 product of the scalar type.
    pub fn mul(&self, o: &Self) -> Self {
        Quaternion {
            coeffs: T::quat_product(&self.coeffs, &o.coeffs),
        }
    }
    /// `self * AngleAxis`: `self * Quaternion(aa)`.
    pub fn mul_angle_axis(&self, aa: &AngleAxis<T>) -> Self {
        self.mul(&Self::from_angle_axis(aa))
    }
    /// `toRotationMatrix()` (also `matrix()`).
    pub fn to_rotation_matrix(&self) -> Mat3<T> {
        let [x, y, z, w] = self.coeffs;
        let two = T::from_f64(2.0);
        let (tx, ty, tz) = (two.mul(x), two.mul(y), two.mul(z));
        let (twx, twy, twz) = (tx.mul(w), ty.mul(w), tz.mul(w));
        let (txx, txy, txz) = (tx.mul(x), ty.mul(x), tz.mul(x));
        let (tyy, tyz, tzz) = (ty.mul(y), tz.mul(y), tz.mul(z));
        Mat3::from_rows([
            [T::ONE.sub(tyy.add(tzz)), txy.sub(twz), txz.add(twy)],
            [txy.add(twz), T::ONE.sub(txx.add(tzz)), tyz.sub(twx)],
            [txz.sub(twy), tyz.add(twx), T::ONE.sub(txx.add(tyy))],
        ])
    }
    /// `squaredNorm()` of the four coefficients: `(x² + z²) + (y² + w²)` for both types (two
    /// `Packet2d` or one `Packet4f`).
    pub fn squared_norm(&self) -> T {
        Vector(self.coeffs).squared_norm()
    }
    /// `norm()`.
    pub fn norm(&self) -> T {
        self.squared_norm().sqrt()
    }
    /// `normalized()`: each coefficient divided by the norm, unless the squared norm is not
    /// positive.
    pub fn normalized(&self) -> Self {
        Quaternion {
            coeffs: Vector(self.coeffs).normalized().0,
        }
    }
    /// `normalize()`, in place.
    pub fn normalize(&mut self) {
        *self = self.normalized();
    }
}

/// `Eigen::AngleAxis<T>`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AngleAxis<T> {
    /// `angle()`, in radians.
    pub angle: T,
    /// `axis()`.
    pub axis: Vec3<T>,
}

/// `Eigen::AngleAxisd`.
pub type AngleAxisd = AngleAxis<f64>;

impl<T: QuatProduct> AngleAxis<T> {
    /// `AngleAxis(angle, axis)`.
    pub fn new(angle: T, axis: Vec3<T>) -> Self {
        AngleAxis { angle, axis }
    }
    /// `AngleAxis(q)` (`operator=(const QuaternionBase&)`): `n = |q.vec()|` (`stableNorm` when it
    /// is below epsilon); the angle `2 atan2(n, |w|)` and the axis `vec / ±n`; angle 0 about X when
    /// `n` is 0.
    pub fn from_quaternion(q: &Quaternion<T>) -> Self {
        let v = q.vec();
        let mut n = v.norm();
        if n < T::EPSILON {
            n = stable_norm3(&v);
        }
        if n != T::ZERO {
            let angle = T::from_f64(2.0).mul(n.atan2(q.w().abs()));
            if q.w() < T::ZERO {
                n = n.neg();
            }
            AngleAxis { angle, axis: v / n }
        } else {
            AngleAxis {
                angle: T::ZERO,
                axis: Vector([T::ONE, T::ZERO, T::ZERO]),
            }
        }
    }
    /// `AngleAxis(mat)` (`fromRotationMatrix`): through the quaternion.
    pub fn from_rotation_matrix(mat: &Mat3<T>) -> Self {
        Self::from_quaternion(&Quaternion::from_rotation_matrix(mat))
    }
    /// `toRotationMatrix()`.
    pub fn to_rotation_matrix(&self) -> Mat3<T> {
        let a = &self.axis.0;
        let s = self.angle.sin();
        let sin_axis = self.axis.map(|x| s.mul(x));
        let c = self.angle.cos();
        let one_minus_c = T::ONE.sub(c);
        let cos1_axis = self.axis.map(|x| one_minus_c.mul(x));
        let mut res = Mat3::zero();
        let mut tmp = cos1_axis.0[0].mul(a[1]);
        res.set(0, 1, tmp.sub(sin_axis.0[2]));
        res.set(1, 0, tmp.add(sin_axis.0[2]));
        tmp = cos1_axis.0[0].mul(a[2]);
        res.set(0, 2, tmp.add(sin_axis.0[1]));
        res.set(2, 0, tmp.sub(sin_axis.0[1]));
        tmp = cos1_axis.0[1].mul(a[2]);
        res.set(1, 2, tmp.sub(sin_axis.0[0]));
        res.set(2, 1, tmp.add(sin_axis.0[0]));
        for (i, (&k, &x)) in cos1_axis.0.iter().zip(a).enumerate() {
            res.set(i, i, k.mul(x).add(c));
        }
        res
    }
    /// `self * o`: the product of their quaternions.
    pub fn mul(&self, o: &Self) -> Quaternion<T> {
        Quaternion::from_angle_axis(self).mul(&Quaternion::from_angle_axis(o))
    }
}

/// `stableNorm()` of three coefficients (StableNorm.h, one kernel over a dynamic segment): the
/// largest magnitude as the scale, then `scale * sqrt` of the scaled squares added left to right.
pub fn stable_norm3<T: Real>(v: &Vec3<T>) -> T {
    let mut scale = T::ZERO;
    let mut inv_scale = T::ONE;
    let mut ssq = T::ZERO;
    let mut max_coeff = v.0[0].abs();
    for x in &v.0[1..] {
        max_coeff = orca_stl::max(max_coeff, x.abs());
    }
    if max_coeff > scale {
        let r = scale.div(max_coeff);
        ssq = ssq.mul(r.mul(r));
        let tmp = T::ONE.div(max_coeff);
        if tmp > T::HIGHEST {
            inv_scale = T::HIGHEST;
            scale = T::ONE.div(inv_scale);
        } else if max_coeff > T::HIGHEST {
            inv_scale = T::ONE;
            scale = max_coeff;
        } else {
            scale = max_coeff;
            inv_scale = tmp;
        }
    } else if max_coeff.is_nan() {
        scale = max_coeff;
    }
    if scale > T::ZERO {
        let s = v.map(|x| x.mul(inv_scale));
        let sq = s.0[0]
            .mul(s.0[0])
            .add(s.0[1].mul(s.0[1]))
            .add(s.0[2].mul(s.0[2]));
        ssq = ssq.add(sq);
    }
    scale.mul(ssq.sqrt())
}

/// `mat.eulerAngles(a0, a1, a2)` (EulerAngles.h, Graphics Gems IV), the first angle in `[0, pi]`.
pub fn euler_angles<T: Real>(mat: &Mat3<T>, a0: usize, a1: usize, a2: usize) -> Vec3<T> {
    let m = |r: usize, c: usize| mat.at(r, c);
    let mut res = [T::ZERO; 3];
    let odd = (a0 + 1) % 3 != a1;
    let i = a0;
    let j = (a0 + 1 + usize::from(odd)) % 3;
    let k = (a0 + 2 - usize::from(odd)) % 3;
    if a0 == a2 {
        res[0] = m(j, i).atan2(m(k, i));
        if (odd && res[0] < T::ZERO) || (!odd && res[0] > T::ZERO) {
            if res[0] > T::ZERO {
                res[0] = res[0].sub(T::PI);
            } else {
                res[0] = res[0].add(T::PI);
            }
            let s2 = hypot(m(j, i), m(k, i));
            res[1] = s2.atan2(m(i, i)).neg();
        } else {
            let s2 = hypot(m(j, i), m(k, i));
            res[1] = s2.atan2(m(i, i));
        }
        let s1 = res[0].sin();
        let c1 = res[0].cos();
        res[2] = c1
            .mul(m(j, k))
            .sub(s1.mul(m(k, k)))
            .atan2(c1.mul(m(j, j)).sub(s1.mul(m(k, j))));
    } else {
        res[0] = m(j, k).atan2(m(k, k));
        let c2 = hypot(m(i, i), m(i, j));
        if (odd && res[0] < T::ZERO) || (!odd && res[0] > T::ZERO) {
            if res[0] > T::ZERO {
                res[0] = res[0].sub(T::PI);
            } else {
                res[0] = res[0].add(T::PI);
            }
            res[1] = m(i, k).neg().atan2(c2.neg());
        } else {
            res[1] = m(i, k).neg().atan2(c2);
        }
        let s1 = res[0].sin();
        let c1 = res[0].cos();
        res[2] = s1
            .mul(m(k, i))
            .sub(c1.mul(m(j, i)))
            .atan2(c1.mul(m(j, j)).sub(s1.mul(m(k, j))));
    }
    if !odd {
        res = res.map(T::neg);
    }
    Vector(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quarter_turn_about_z() {
        let aa = AngleAxisd::new(core::f64::consts::FRAC_PI_2, Vec3::unit_z());
        let m = aa.to_rotation_matrix();
        assert!((m.at(0, 1) + 1.0).abs() < 1e-15 && (m.at(1, 0) - 1.0).abs() < 1e-15);
        let q = Quaterniond::from_angle_axis(&aa);
        let back = AngleAxisd::from_quaternion(&q);
        assert!((back.angle - aa.angle).abs() < 1e-15);
        let e = euler_angles(&m, 2, 1, 0);
        assert!((e.x() - aa.angle).abs() < 1e-15 && e.y().abs() < 1e-15);
    }

    #[test]
    fn identity_products() {
        let q = Quaterniond::new(0.5, 0.5, 0.5, 0.5);
        assert_eq!(q.mul(&Quaterniond::identity()), q);
        let f = Quaternion::<f32>::new(0.5, 0.5, 0.5, 0.5);
        assert_eq!(f.mul(&Quaternion::identity()), f);
    }
}
