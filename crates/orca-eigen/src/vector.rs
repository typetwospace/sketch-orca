// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Eigen code: the fixed-size column vectors of Eigen/src/Core (Matrix.h, Dot.h for norm,
// squaredNorm, normalized and normalize, Redux.h for sum, minCoeff and maxCoeff, FindCoeff.h for
// their index forms, Fuzzy.h for isApprox, the coefficient-wise functors of
// Functors/BinaryFunctors.h and UnaryFunctors.h) and the cross products of
// Eigen/src/Geometry/OrthoMethods.h, of Eigen 5.0.1, translated on 2026-11-07. Their copyright is
// the Eigen authors' (Benoit Jacob, Gael Guennebaud and the Eigen contributors), under the Mozilla
// Public License, version 2.0 (MPL-2.0; licenses/MPL-2.0.txt).
//
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0. If a copy of
// the MPL was not distributed with this file, You can obtain one at https://mozilla.org/MPL/2.0/.
// (Exhibit A of the MPL-2.0: the Eigen code here stays under it; the rest of the file is
// sketch-orca's, under the AGPL-3.0, as the MPL's section 3.3 allows.)

//! `Eigen::Matrix<T, N, 1, Eigen::DontAlign>`: libslic3r's `Vec2d`, `Vec3d`, `Vec3f`, `Vec2crd`,
//! `Vec3crd`, `Vec3i32` and the rest (Point.hpp).
//!
//! Coefficient-wise operations (`+`, `-`, scalar `*` and `/`, `cwiseProduct`, `cwiseQuotient`,
//! `cwiseMin`, `cwiseMax`, `cwiseAbs`) compute each coefficient on its own, so SSE2 gives the same
//! values as a loop: `cwiseMin(a, b)` is `std::min(a_i, b_i)` (`_mm_min_pd(b, a)` per lane, the
//! operands reversed to match `std::min`), `cwiseMax(a, b)` `std::max(a_i, b_i)`. Except for
//! `int64_t`: the coefficients that fill whole packets (both of a `Vec2crd`, the first two of a
//! `Vec3crd`; the assignment's linear vectorised traversal does the rest one by one) go through
//! `pmin<Packet2l>` and `pmax<Packet2l>`, whose emulated compare ([`crate::pcmp_lt_i64`]) is
//! wrong past 2^31.
//!
//! Reductions follow `redux.rs`: the methods here assume the expression has packet access, as a
//! plain vector and any coefficient-wise expression of plain vectors and casts do (`DontAlign`
//! keeps packet access, through unaligned loads, as `EIGEN_UNALIGNED_VECTORIZE` is on). The
//! `*_strided` forms are for an expression without packet access: a row of a column-major matrix,
//! a diagonal, a `unaryExpr` of a lambda.

use crate::redux::{dot, find_coeff, redux, redux_packets};
use crate::scalar::{Cast, Real, Scalar};
use core::ops::{
    Add, AddAssign, Div, DivAssign, Index, IndexMut, Mul, MulAssign, Neg, Sub, SubAssign,
};

/// `Eigen::Matrix<T, N, 1, Eigen::DontAlign>`, a fixed-size column vector.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vector<T, const N: usize>(pub [T; N]);

/// `Eigen::Matrix<T, 2, 1, Eigen::DontAlign>`.
pub type Vec2<T> = Vector<T, 2>;
/// `Eigen::Matrix<T, 3, 1, Eigen::DontAlign>`.
pub type Vec3<T> = Vector<T, 3>;
/// `Eigen::Matrix<T, 4, 1, Eigen::DontAlign>`.
pub type Vec4<T> = Vector<T, 4>;
/// libslic3r's `Vec2d`.
pub type Vec2d = Vec2<f64>;
/// libslic3r's `Vec3d`.
pub type Vec3d = Vec3<f64>;
/// libslic3r's `Vec4d`.
pub type Vec4d = Vec4<f64>;
/// libslic3r's `Vec2f`.
pub type Vec2f = Vec2<f32>;
/// libslic3r's `Vec3f` (admesh's `stl_vertex` and `stl_normal`).
pub type Vec3f = Vec3<f32>;
/// libslic3r's `Vec4f`.
pub type Vec4f = Vec4<f32>;
/// libslic3r's `Vec2crd` (`coord_t` is `int64_t`); `Point` derives from it.
pub type Vec2crd = Vec2<i64>;
/// libslic3r's `Vec3crd`.
pub type Vec3crd = Vec3<i64>;
/// libslic3r's `Vec2i32`.
pub type Vec2i32 = Vec2<i32>;
/// libslic3r's `Vec3i32` (an indexed triangle).
pub type Vec3i32 = Vec3<i32>;
/// libslic3r's `Vec2i64`.
pub type Vec2i64 = Vec2<i64>;
/// libslic3r's `Vec3i64`.
pub type Vec3i64 = Vec3<i64>;

impl<T: Scalar, const N: usize> Vector<T, N> {
    /// `Zero()`.
    pub fn zero() -> Self {
        Vector([T::ZERO; N])
    }
    /// `Ones()`.
    pub fn ones() -> Self {
        Vector([T::ONE; N])
    }
    /// `Constant(v)`.
    pub fn constant(v: T) -> Self {
        Vector([v; N])
    }
    /// `Unit(i)`: 1 at `i`, 0 elsewhere.
    pub fn unit(i: usize) -> Self {
        let mut v = Self::zero();
        v.0[i] = T::ONE;
        v
    }
    /// The coefficients, in order.
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }
    /// Applies `f` to each coefficient (a coefficient-wise unary expression).
    pub fn map<U: Scalar>(&self, f: impl Fn(T) -> U) -> Vector<U, N> {
        Vector(core::array::from_fn(|i| f(self.0[i])))
    }
    /// Combines each pair of coefficients (a coefficient-wise binary expression, `self` first).
    pub fn zip<U: Scalar>(&self, o: &Self, f: impl Fn(T, T) -> U) -> Vector<U, N> {
        Vector(core::array::from_fn(|i| f(self.0[i], o.0[i])))
    }
    /// `cast<U>()`: `static_cast<U>` of each coefficient.
    pub fn cast<U: Scalar>(&self) -> Vector<U, N>
    where
        T: Cast<U>,
    {
        self.map(|x| x.cast())
    }
    /// `cwiseProduct(o)`.
    pub fn cwise_product(&self, o: &Self) -> Self {
        self.zip(o, T::mul)
    }
    /// `cwiseQuotient(o)`.
    pub fn cwise_quotient(&self, o: &Self) -> Self {
        self.zip(o, T::div)
    }
    /// `cwiseMin(o)`: `std::min(self_i, o_i)`, `self_i` when they are unordered or equal; the
    /// coefficients of whole packets by `pmin` ([`Scalar::packet_min`], which differs only for
    /// `int64_t`).
    pub fn cwise_min(&self, o: &Self) -> Self {
        let whole = N / T::PACKET * T::PACKET;
        Vector(core::array::from_fn(|i| {
            if i < whole {
                self.0[i].packet_min(o.0[i])
            } else {
                orca_stl::min(self.0[i], o.0[i])
            }
        }))
    }
    /// `cwiseMax(o)`: `std::max(self_i, o_i)`, `self_i` when they are unordered or equal; the
    /// coefficients of whole packets by `pmax` ([`Scalar::packet_max`]).
    pub fn cwise_max(&self, o: &Self) -> Self {
        let whole = N / T::PACKET * T::PACKET;
        Vector(core::array::from_fn(|i| {
            if i < whole {
                self.0[i].packet_max(o.0[i])
            } else {
                orca_stl::max(self.0[i], o.0[i])
            }
        }))
    }
    /// `cwiseAbs()`.
    pub fn cwise_abs(&self) -> Self {
        self.map(T::abs)
    }
    /// `sum()`.
    pub fn sum(&self) -> T {
        redux(&self.0, T::PACKET, true, T::add)
    }
    /// `sum()` of an expression without packet access.
    pub fn sum_strided(&self) -> T {
        redux(&self.0, T::PACKET, false, T::add)
    }
    /// `prod()`.
    pub fn prod(&self) -> T {
        redux(&self.0, T::PACKET, true, T::mul)
    }
    /// `squaredNorm()`: the sum of the squares.
    pub fn squared_norm(&self) -> T {
        let sq = self.map(|x| x.mul(x));
        redux(&sq.0, T::PACKET, true, T::add)
    }
    /// `squaredNorm()` of an expression without packet access.
    pub fn squared_norm_strided(&self) -> T {
        let sq = self.map(|x| x.mul(x));
        redux(&sq.0, T::PACKET, false, T::add)
    }
    /// `dot(o)`.
    pub fn dot(&self, o: &Self) -> T {
        dot(&self.0, &o.0, true)
    }
    /// `dot(o)` where either operand has no packet access.
    pub fn dot_strided(&self, o: &Self) -> T {
        dot(&self.0, &o.0, false)
    }
    /// `minCoeff()`: `std::min` folded in Eigen's order, whole packets joined by `pmin`.
    pub fn min_coeff(&self) -> T {
        redux_packets(&self.0, T::PACKET, true, T::packet_min, orca_stl::min)
    }
    /// `maxCoeff()`: `std::max` folded in Eigen's order, whole packets joined by `pmax`.
    pub fn max_coeff(&self) -> T {
        redux_packets(&self.0, T::PACKET, true, T::packet_max, orca_stl::max)
    }
    /// `minCoeff()` of an expression without packet access.
    pub fn min_coeff_strided(&self) -> T {
        redux(&self.0, T::PACKET, false, orca_stl::min)
    }
    /// `maxCoeff()` of an expression without packet access.
    pub fn max_coeff_strided(&self) -> T {
        redux(&self.0, T::PACKET, false, orca_stl::max)
    }
    /// `maxCoeff(&index)`: the first of the largest coefficients and its index, searched by
    /// packets as FindCoeff.h searches ([`find_coeff`]; a NaN can stop the search).
    pub fn max_coeff_index(&self) -> (T, usize) {
        find_coeff(
            &self.0,
            T::PACKET,
            true,
            |inc: T, cand: T| inc.packet_lt(cand),
            |inc, cand| cand > inc,
            orca_stl::max,
        )
    }
    /// `minCoeff(&index)`: the first of the smallest coefficients and its index, searched by
    /// packets as FindCoeff.h searches ([`find_coeff`]).
    pub fn min_coeff_index(&self) -> (T, usize) {
        find_coeff(
            &self.0,
            T::PACKET,
            true,
            |inc: T, cand: T| cand.packet_lt(inc),
            |inc, cand| cand < inc,
            orca_stl::min,
        )
    }
    /// `isApprox(o, prec)` (Fuzzy.h): `(self - o).squaredNorm() <= prec² * std::min` of the two
    /// squared norms.
    pub fn is_approx(&self, o: &Self, prec: T) -> bool {
        let d = (*self - *o).squared_norm();
        d <= prec
            .mul(prec)
            .mul(orca_stl::min(self.squared_norm(), o.squared_norm()))
    }
}

impl<T: Real, const N: usize> Vector<T, N> {
    /// `norm()`: `std::sqrt(squaredNorm())`.
    pub fn norm(&self) -> T {
        self.squared_norm().sqrt()
    }
    /// `normalized()`: each coefficient divided by the norm, or the vector itself when its
    /// squared norm is not positive.
    pub fn normalized(&self) -> Self {
        let z = self.squared_norm();
        if z > T::ZERO {
            let n = z.sqrt();
            self.map(|x| x.div(n))
        } else {
            *self
        }
    }
    /// `normalize()`, in place.
    pub fn normalize(&mut self) {
        *self = self.normalized();
    }
    /// `cwiseInverse()`: `1 / self_i`.
    pub fn cwise_inverse(&self) -> Self {
        self.map(|x| T::ONE.div(x))
    }
    /// `cwiseSqrt()`.
    pub fn cwise_sqrt(&self) -> Self {
        self.map(T::sqrt)
    }
}

impl<T: Scalar> Vector<T, 2> {
    /// `Vec2(x, y)`.
    pub const fn new(x: T, y: T) -> Self {
        Vector([x, y])
    }
    /// `x()`.
    pub fn x(&self) -> T {
        self.0[0]
    }
    /// `y()`.
    pub fn y(&self) -> T {
        self.0[1]
    }
    /// The 2D `cross(o)`: `x * o.y - y * o.x`.
    pub fn cross2(&self, o: &Self) -> T {
        self.0[0].mul(o.0[1]).sub(self.0[1].mul(o.0[0]))
    }
}

impl<T: Scalar> Vector<T, 3> {
    /// `Vec3(x, y, z)`.
    pub const fn new(x: T, y: T, z: T) -> Self {
        Vector([x, y, z])
    }
    /// `x()`.
    pub fn x(&self) -> T {
        self.0[0]
    }
    /// `y()`.
    pub fn y(&self) -> T {
        self.0[1]
    }
    /// `z()`.
    pub fn z(&self) -> T {
        self.0[2]
    }
    /// `UnitX()`.
    pub fn unit_x() -> Self {
        Self::unit(0)
    }
    /// `UnitY()`.
    pub fn unit_y() -> Self {
        Self::unit(1)
    }
    /// `UnitZ()`.
    pub fn unit_z() -> Self {
        Self::unit(2)
    }
    /// `head<2>()`.
    pub fn head2(&self) -> Vector<T, 2> {
        Vector([self.0[0], self.0[1]])
    }
    /// `cross(o)` (OrthoMethods.h): `(y o.z - z o.y, z o.x - x o.z, x o.y - y o.x)`.
    pub fn cross(&self, o: &Self) -> Self {
        let (a, b) = (&self.0, &o.0);
        Vector([
            a[1].mul(b[2]).sub(a[2].mul(b[1])),
            a[2].mul(b[0]).sub(a[0].mul(b[2])),
            a[0].mul(b[1]).sub(a[1].mul(b[0])),
        ])
    }
    /// `homogeneous()`: `(x, y, z, 1)`.
    pub fn homogeneous(&self) -> Vector<T, 4> {
        Vector([self.0[0], self.0[1], self.0[2], T::ONE])
    }
}

impl<T: Scalar> Vector<T, 4> {
    /// `Vec4(x, y, z, w)`.
    pub const fn new(x: T, y: T, z: T, w: T) -> Self {
        Vector([x, y, z, w])
    }
    /// `head<3>()`.
    pub fn head3(&self) -> Vector<T, 3> {
        Vector([self.0[0], self.0[1], self.0[2]])
    }
}

impl<T, const N: usize> Index<usize> for Vector<T, N> {
    type Output = T;
    fn index(&self, i: usize) -> &T {
        &self.0[i]
    }
}

impl<T, const N: usize> IndexMut<usize> for Vector<T, N> {
    fn index_mut(&mut self, i: usize) -> &mut T {
        &mut self.0[i]
    }
}

impl<T: Scalar, const N: usize> Add for Vector<T, N> {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        self.zip(&o, T::add)
    }
}

impl<T: Scalar, const N: usize> Sub for Vector<T, N> {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        self.zip(&o, T::sub)
    }
}

impl<T: Scalar, const N: usize> Neg for Vector<T, N> {
    type Output = Self;
    fn neg(self) -> Self {
        self.map(T::neg)
    }
}

impl<T: Scalar, const N: usize> Mul<T> for Vector<T, N> {
    type Output = Self;
    fn mul(self, s: T) -> Self {
        self.map(|x| x.mul(s))
    }
}

impl<T: Scalar, const N: usize> Div<T> for Vector<T, N> {
    type Output = Self;
    fn div(self, s: T) -> Self {
        self.map(|x| x.div(s))
    }
}

impl<T: Scalar, const N: usize> AddAssign for Vector<T, N> {
    fn add_assign(&mut self, o: Self) {
        *self = *self + o;
    }
}

impl<T: Scalar, const N: usize> SubAssign for Vector<T, N> {
    fn sub_assign(&mut self, o: Self) {
        *self = *self - o;
    }
}

impl<T: Scalar, const N: usize> MulAssign<T> for Vector<T, N> {
    fn mul_assign(&mut self, s: T) {
        *self = *self * s;
    }
}

impl<T: Scalar, const N: usize> DivAssign<T> for Vector<T, N> {
    fn div_assign(&mut self, s: T) {
        *self = *self / s;
    }
}

macro_rules! scalar_times_vector {
    ($($t:ty),+) => {
        $(impl<const N: usize> Mul<Vector<$t, N>> for $t {
            type Output = Vector<$t, N>;
            fn mul(self, v: Vector<$t, N>) -> Vector<$t, N> {
                v.map(|x| self * x)
            }
        })+
    };
}
scalar_times_vector!(f32, f64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn squared_norms_follow_the_packet_size() {
        let v = Vec3d::new(1.0, 1e-8, 1e-8);
        assert_eq!(v.squared_norm(), (1.0 + 1e-16) + 1e-16);
        assert_eq!(v.squared_norm_strided(), 1.0 + (1e-16 + 1e-16));
        let f = Vec3f::new(1.0, 2e-4, 2e-4);
        let s = 2e-4_f32 * 2e-4_f32;
        assert_eq!(f.squared_norm(), 1.0 + (s + s));
        assert_ne!(f.squared_norm(), (1.0 + s) + s);
    }

    #[test]
    fn min_and_max_keep_stds_operand_order() {
        let v = Vec2d::new(-0.0, 0.0);
        assert!(v.min_coeff().is_sign_negative());
        assert!(v.max_coeff().is_sign_negative());
        let a = Vec2d::new(0.0, f64::NAN);
        assert!(a.cwise_min(&Vec2d::new(1.0, 1.0)).0[1].is_nan());
        assert_eq!(
            Vec2d::new(f64::NAN, 2.0)
                .cwise_max(&Vec2d::new(1.0, f64::NAN))
                .0[1],
            2.0
        );
    }

    #[test]
    fn integer_vectors_wrap() {
        let p = Vec2crd::new(i64::MAX, 1);
        assert_eq!((p + Vec2crd::new(1, 0)).x(), i64::MIN);
        assert_eq!(Vec2crd::new(3, 4).squared_norm(), 25);
    }

    #[test]
    fn normalized_divides() {
        let v = Vec3d::new(3.0, 0.0, 4.0).normalized();
        assert_eq!(v, Vec3d::new(3.0 / 5.0, 0.0, 4.0 / 5.0));
        assert_eq!(Vec3d::zero().normalized(), Vec3d::zero());
    }
}
