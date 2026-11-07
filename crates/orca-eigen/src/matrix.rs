// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Eigen code: the fixed-size matrices of Eigen/src/Core (Matrix.h, the products of
// ProductEvaluators.h and GeneralProduct.h, Transpose.h, DiagonalProduct.h, Fuzzy.h), the 3x3
// determinant of Eigen/src/LU/Determinant.h and the 3x3 inverse of Eigen/src/LU/InverseImpl.h, of
// Eigen 5.0.1, translated on 2026-11-07. Their copyright is the Eigen authors' (Benoit Jacob, Gael
// Guennebaud and the Eigen contributors), under the Mozilla Public License, version 2.0 (MPL-2.0;
// licenses/MPL-2.0.txt).
//
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0. If a copy of
// the MPL was not distributed with this file, You can obtain one at https://mozilla.org/MPL/2.0/.
// (Exhibit A of the MPL-2.0: the Eigen code here stays under it; the rest of the file is
// sketch-orca's, under the AGPL-3.0, as the MPL's section 3.3 allows.)

//! `Eigen::Matrix<T, R, C, Eigen::DontAlign>`, column-major: libslic3r's `Matrix3d`, `Matrix3f`,
//! `Matrix4d` and the linear parts of its transforms.
//!
//! A product of two small fixed-size matrices (or a matrix and a vector) is coefficient-based
//! (`CoeffBasedProductMode`, evaluated through `lazyProduct`): each coefficient is a sum of
//! `lhs(r, i) * rhs(i, c)`, added in the order `redux.rs` gives for its row: the packet path (left
//! to right) for the rows [`packet_rows`] names, the coefficient path (a tree) for the others.
//! [`Matrix::mul`] is that product for a column-major lhs with packet access, which every product
//! of libslic3r's geometry has (a plain matrix, a block of one such as `Transform::linear()`, or
//! a negation of one); the rhs may be anything, a transpose included, as only its coefficients are
//! read. Products by a diagonal (`asDiagonal()`, `Eigen::Scaling`) are one multiplication per
//! coefficient.

use crate::redux::{packet_rows, product_coeff, redux};
use crate::scalar::{Cast, Real, Scalar};
use crate::vector::Vector;
use core::ops::{Add, Index, IndexMut, Neg, Sub};

/// `Eigen::Matrix<T, R, C, Eigen::DontAlign>`, stored column by column (`cols[c][r]`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix<T, const R: usize, const C: usize> {
    /// The columns.
    pub cols: [[T; R]; C],
}

/// `Eigen::Matrix<T, 2, 2, Eigen::DontAlign>`.
pub type Mat2<T> = Matrix<T, 2, 2>;
/// `Eigen::Matrix<T, 3, 3, Eigen::DontAlign>`.
pub type Mat3<T> = Matrix<T, 3, 3>;
/// `Eigen::Matrix<T, 4, 4, Eigen::DontAlign>`.
pub type Mat4<T> = Matrix<T, 4, 4>;
/// libslic3r's `Matrix3d`.
pub type Matrix3d = Mat3<f64>;
/// libslic3r's `Matrix3f`.
pub type Matrix3f = Mat3<f32>;
/// libslic3r's `Matrix4d`.
pub type Matrix4d = Mat4<f64>;
/// libslic3r's `Matrix2d`.
pub type Matrix2d = Mat2<f64>;

impl<T: Scalar, const R: usize, const C: usize> Matrix<T, R, C> {
    /// `Zero()`.
    pub fn zero() -> Self {
        Matrix {
            cols: [[T::ZERO; R]; C],
        }
    }
    /// `Identity()`: ones on the diagonal.
    pub fn identity() -> Self {
        let mut m = Self::zero();
        for i in 0..R.min(C) {
            m.cols[i][i] = T::ONE;
        }
        m
    }
    /// The matrix whose coefficient `(r, c)` is `f(r, c)`.
    pub fn from_fn(f: impl Fn(usize, usize) -> T) -> Self {
        Matrix {
            cols: core::array::from_fn(|c| core::array::from_fn(|r| f(r, c))),
        }
    }
    /// From rows, as `m << a, b, c, ...` fills a matrix row by row.
    pub fn from_rows(rows: [[T; C]; R]) -> Self {
        Self::from_fn(|r, c| rows[r][c])
    }
    /// `coeff(r, c)`.
    pub fn at(&self, r: usize, c: usize) -> T {
        self.cols[c][r]
    }
    /// `coeffRef(r, c) = v`.
    pub fn set(&mut self, r: usize, c: usize, v: T) {
        self.cols[c][r] = v;
    }
    /// `col(c)`.
    pub fn col(&self, c: usize) -> Vector<T, R> {
        Vector(self.cols[c])
    }
    /// `col(c) = v`.
    pub fn set_col(&mut self, c: usize, v: Vector<T, R>) {
        self.cols[c] = v.0;
    }
    /// `row(r)`, as a column vector (a strided expression: no packet access).
    pub fn row(&self, r: usize) -> Vector<T, C> {
        Vector(core::array::from_fn(|c| self.cols[c][r]))
    }
    /// `row(r) = v`.
    pub fn set_row(&mut self, r: usize, v: Vector<T, C>) {
        for c in 0..C {
            self.cols[c][r] = v.0[c];
        }
    }
    /// The coefficients in storage order (column by column).
    pub fn storage(&self) -> Vec<T> {
        self.cols.iter().flatten().copied().collect()
    }
    /// `transpose()`.
    pub fn transpose(&self) -> Matrix<T, C, R> {
        Matrix::from_fn(|r, c| self.cols[r][c])
    }
    /// Applies `f` to each coefficient.
    pub fn map<U: Scalar>(&self, f: impl Fn(T) -> U) -> Matrix<U, R, C> {
        Matrix {
            cols: core::array::from_fn(|c| core::array::from_fn(|r| f(self.cols[c][r]))),
        }
    }
    /// Combines each pair of coefficients, `self` first.
    pub fn zip(&self, o: &Self, f: impl Fn(T, T) -> T) -> Self {
        Self::from_fn(|r, c| f(self.cols[c][r], o.cols[c][r]))
    }
    /// `cast<U>()`.
    pub fn cast<U: Scalar>(&self) -> Matrix<U, R, C>
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
    /// `cwiseAbs()`.
    pub fn cwise_abs(&self) -> Self {
        self.map(T::abs)
    }
    /// `* s`, each coefficient.
    pub fn scale(&self, s: T) -> Self {
        self.map(|x| x.mul(s))
    }
    /// `sum()`: a plain matrix has packet and linear access, so a vectorised reduction over its
    /// storage order.
    pub fn sum(&self) -> T {
        redux(&self.storage(), T::PACKET, true, T::add)
    }
    /// `squaredNorm()`.
    pub fn squared_norm(&self) -> T {
        let sq: Vec<T> = self.storage().into_iter().map(|x| x.mul(x)).collect();
        redux(&sq, T::PACKET, true, T::add)
    }
    /// `isApprox(o, prec)` (Fuzzy.h), as for vectors, over all coefficients.
    pub fn is_approx(&self, o: &Self, prec: T) -> bool {
        let d = (*self - *o).squared_norm();
        d <= prec
            .mul(prec)
            .mul(orca_stl::min(self.squared_norm(), o.squared_norm()))
    }
    /// `self * other` for a column-major `self` with packet access (module docs): each column's
    /// first [`packet_rows`] rows left to right, the rest as a tree.
    pub fn mul<const K: usize>(&self, other: &Matrix<T, C, K>) -> Matrix<T, R, K> {
        let packet = packet_rows(R, T::PACKET);
        Matrix::from_fn(|r, c| {
            product_coeff(C, r < packet, |i| self.cols[i][r].mul(other.cols[c][i]))
        })
    }
    /// `self * v` for a column-major `self` with packet access: the vector's first
    /// [`packet_rows`] coefficients left to right, the rest as a tree.
    pub fn mul_vec(&self, v: &Vector<T, C>) -> Vector<T, R> {
        let packet = packet_rows(R, T::PACKET);
        Vector(core::array::from_fn(|r| {
            product_coeff(C, r < packet, |i| self.cols[i][r].mul(v.0[i]))
        }))
    }
    /// `self * d.asDiagonal()`: column `c` times `d(c)`.
    pub fn mul_diagonal(&self, d: &Vector<T, C>) -> Self {
        Self::from_fn(|r, c| self.cols[c][r].mul(d.0[c]))
    }
    /// `d.asDiagonal() * self`: row `r` times `d(r)`.
    pub fn diagonal_mul(d: &Vector<T, R>, m: &Self) -> Self {
        Self::from_fn(|r, c| d.0[r].mul(m.cols[c][r]))
    }
}

impl<T: Scalar, const N: usize> Matrix<T, N, N> {
    /// `diagonal()`.
    pub fn diagonal(&self) -> Vector<T, N> {
        Vector(core::array::from_fn(|i| self.cols[i][i]))
    }
    /// `trace()`: the diagonal's sum, a strided expression (no packet access), so a tree.
    pub fn trace(&self) -> T {
        self.diagonal().sum_strided()
    }
    /// `v.asDiagonal()` as a matrix.
    pub fn from_diagonal(v: &Vector<T, N>) -> Self {
        let mut m = Self::zero();
        for i in 0..N {
            m.cols[i][i] = v.0[i];
        }
        m
    }
    /// `isDiagonal(prec)` (DiagonalMatrix.h; libslic3r passes the default `dummy_precision`):
    /// the largest absolute diagonal coefficient (from -1, so a NaN diagonal is skipped), then each
    /// off-diagonal coefficient much smaller than it (`isMuchSmallerThan`: `|x| <= |max| * prec`,
    /// false for NaN).
    pub fn is_diagonal(&self, prec: T) -> bool {
        let mut max_abs_on_diagonal = T::ONE.neg();
        for j in 0..N {
            let a = self.cols[j][j].abs();
            if a > max_abs_on_diagonal {
                max_abs_on_diagonal = a;
            }
        }
        let small = |x: T| x.abs() <= max_abs_on_diagonal.abs().mul(prec);
        for j in 0..N {
            for i in 0..j {
                if !small(self.cols[j][i]) || !small(self.cols[i][j]) {
                    return false;
                }
            }
        }
        true
    }
}

impl<T: Scalar> Matrix<T, 3, 3> {
    /// `cofactor_3x3<i, j>` (InverseImpl.h).
    fn cofactor(&self, i: usize, j: usize) -> T {
        let (i1, i2, j1, j2) = ((i + 1) % 3, (i + 2) % 3, (j + 1) % 3, (j + 2) % 3);
        self.at(i1, j1)
            .mul(self.at(i2, j2))
            .sub(self.at(i1, j2).mul(self.at(i2, j1)))
    }
    /// `determinant()` (Determinant.h): `m(0,0) (m(1,1) m(2,2) - m(1,2) m(2,1))`, minus the same
    /// for column 1, plus the same for column 2, in that order.
    pub fn determinant(&self) -> T {
        let h = |a: usize, b: usize, c: usize| {
            self.at(0, a).mul(
                self.at(1, b)
                    .mul(self.at(2, c))
                    .sub(self.at(1, c).mul(self.at(2, b))),
            )
        };
        h(0, 1, 2).sub(h(1, 0, 2)).add(h(2, 0, 1))
    }
}

impl<T: Real> Matrix<T, 3, 3> {
    /// `inverse()` (InverseImpl.h, `compute_inverse<.., 3>`): the cofactors of column 0, the
    /// determinant as their dot product with column 0 (a vectorised reduction: both have packet
    /// access), and each coefficient `(r, c)` as the cofactor `(c, r)` times `1 / det`.
    pub fn inverse(&self) -> Self {
        let col0 = Vector([
            self.cofactor(0, 0),
            self.cofactor(1, 0),
            self.cofactor(2, 0),
        ]);
        let det = col0.cwise_product(&self.col(0)).sum();
        let invdet = T::ONE.div(det);
        let mut res = Self::zero();
        res.set(1, 2, self.cofactor(2, 1).mul(invdet));
        res.set(2, 1, self.cofactor(1, 2).mul(invdet));
        res.set(2, 2, self.cofactor(2, 2).mul(invdet));
        res.set(1, 0, self.cofactor(0, 1).mul(invdet));
        res.set(1, 1, self.cofactor(1, 1).mul(invdet));
        res.set(2, 0, self.cofactor(0, 2).mul(invdet));
        res.set_row(0, col0 * invdet);
        res
    }
}

impl<T, const R: usize, const C: usize> Index<(usize, usize)> for Matrix<T, R, C> {
    type Output = T;
    fn index(&self, (r, c): (usize, usize)) -> &T {
        &self.cols[c][r]
    }
}

impl<T, const R: usize, const C: usize> IndexMut<(usize, usize)> for Matrix<T, R, C> {
    fn index_mut(&mut self, (r, c): (usize, usize)) -> &mut T {
        &mut self.cols[c][r]
    }
}

impl<T: Scalar, const R: usize, const C: usize> Add for Matrix<T, R, C> {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        self.zip(&o, T::add)
    }
}

impl<T: Scalar, const R: usize, const C: usize> Sub for Matrix<T, R, C> {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        self.zip(&o, T::sub)
    }
}

impl<T: Scalar, const R: usize, const C: usize> Neg for Matrix<T, R, C> {
    type Output = Self;
    fn neg(self) -> Self {
        self.map(T::neg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_by_three_double_products_split_their_rows() {
        let a = Matrix3d::from_rows([[1.0, 1e-16, 1e-16]; 3]);
        let b = Matrix3d::identity().scale(1.0);
        let ones = Matrix3d::from_fn(|_, _| 1.0);
        let p = a.mul(&ones);
        // Rows 0 and 1 by packets, left to right; row 2 by the coefficient path, a tree.
        assert_eq!(p.at(0, 0), (1.0 + 1e-16) + 1e-16);
        assert_eq!(p.at(2, 0), 1.0 + (1e-16 + 1e-16));
        assert_eq!(a.mul(&b), a);
        let f = Matrix3f::from_rows([[1.0, 4e-8, 4e-8]; 3]);
        let pf = f.mul(&Matrix3f::from_fn(|_, _| 1.0));
        assert_eq!(pf.at(0, 0), 1.0 + (4e-8 + 4e-8));
    }

    #[test]
    fn inverse_and_determinant() {
        let m = Matrix3d::from_rows([[2.0, 0.0, 0.0], [0.0, 4.0, 0.0], [0.0, 0.0, 8.0]]);
        assert_eq!(m.determinant(), 64.0);
        assert_eq!(
            m.inverse(),
            Matrix3d::from_rows([[0.5, 0.0, 0.0], [0.0, 0.25, 0.0], [0.0, 0.0, 0.125]])
        );
        assert!(Matrix3d::identity().is_diagonal(1e-12));
    }
}
