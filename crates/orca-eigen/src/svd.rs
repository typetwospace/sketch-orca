// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Eigen code: JacobiSVD of Eigen/src/SVD/JacobiSVD.h (compute_impl for a square real
// matrix with ComputeFullU and ComputeFullV), real_2x2_jacobi_svd of Eigen/src/misc/RealSvd2x2.h
// and JacobiRotation, makeJacobi and apply_rotation_in_the_plane of Eigen/src/Jacobi/Jacobi.h, of
// Eigen 5.0.1, translated on 2026-11-07. Their copyright is the Eigen authors' (Benoit Jacob, Gael
// Guennebaud and the Eigen contributors), under the Mozilla Public License, version 2.0 (MPL-2.0;
// licenses/MPL-2.0.txt).
//
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0. If a copy of
// the MPL was not distributed with this file, You can obtain one at https://mozilla.org/MPL/2.0/.
// (Exhibit A of the MPL-2.0: the Eigen code here stays under it; the rest of the file is
// sketch-orca's, under the AGPL-3.0, as the MPL's section 3.3 allows.)

//! `Eigen::JacobiSVD<Matrix3d, ComputeFullU | ComputeFullV>`: the two-sided Jacobi SVD that
//! `Transform::computeRotationScaling` and libslic3r's `TransformationSVD` run on a 3x3 matrix.
//!
//! The matrix is divided by its largest magnitude; sweeps over the pairs (1, 0), (2, 0), (2, 1)
//! diagonalise each 2x2 block whose off-diagonal coefficients exceed
//! `max(DBL_MIN, 2 eps * largest diagonal)`, by a rotation on the left (`real_2x2_jacobi_svd`) and
//! one on the right (`makeJacobi`), accumulated into U and V, until a sweep changes nothing. The
//! diagonal's magnitudes, times the scale, are the singular values (U's column negated where the
//! diagonal is negative), sorted in descending order with U's and V's columns. Every rotation is
//! applied coefficient by coefficient (rows are strided, and columns of a 3x3 matrix are never
//! 16-byte aligned, so `apply_rotation_in_the_plane` takes its scalar loop).

use crate::matrix::Mat3;
use crate::scalar::Real;
use crate::vector::{Vec3, Vector};

/// `Eigen::JacobiRotation<T>`: `(c, s)`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct JacobiRotation<T> {
    c: T,
    s: T,
}

impl<T: Real> JacobiRotation<T> {
    /// `transpose()`: `(c, -s)`.
    fn transpose(self) -> Self {
        JacobiRotation {
            c: self.c,
            s: self.s.neg(),
        }
    }
    /// `self * o`: `(c c' - s s', c s' + s c')`.
    fn mul(self, o: Self) -> Self {
        JacobiRotation {
            c: self.c.mul(o.c).sub(self.s.mul(o.s)),
            s: self.c.mul(o.s).add(self.s.mul(o.c)),
        }
    }
    /// `makeJacobi(x, y, z)`: the rotation that diagonalises the symmetric `[[x, y], [y, z]]`.
    fn make_jacobi(x: T, y: T, z: T) -> Self {
        let deno = T::from_f64(2.0).mul(y.abs());
        if deno < T::MIN_POSITIVE {
            return JacobiRotation {
                c: T::ONE,
                s: T::ZERO,
            };
        }
        let tau = x.sub(z).div(deno);
        let w = tau.mul(tau).add(T::ONE).sqrt();
        let t = if tau > T::ZERO {
            T::ONE.div(tau.add(w))
        } else {
            T::ONE.div(tau.sub(w))
        };
        let sign_t = if t > T::ZERO { T::ONE } else { T::ONE.neg() };
        let n = T::ONE.div(t.mul(t).add(T::ONE).sqrt());
        JacobiRotation {
            c: n,
            s: sign_t.neg().mul(y.div(y.abs())).mul(t.abs()).mul(n),
        }
    }
}

/// `apply_rotation_in_the_plane(x, y, j)`, the scalar loop: `x' = c x + s y`, `y' = -s x + c y`,
/// skipped when `j` is the identity.
fn rotate_pair<T: Real>(x: &mut [T], y: &mut [T], j: JacobiRotation<T>) {
    if j.c == T::ONE && j.s == T::ZERO {
        return;
    }
    for (xi, yi) in x.iter_mut().zip(y.iter_mut()) {
        let (a, b) = (*xi, *yi);
        *xi = j.c.mul(a).add(j.s.mul(b));
        *yi = j.s.neg().mul(a).add(j.c.mul(b));
    }
}

/// `m.applyOnTheLeft(p, q, j)`: rows `p` and `q`.
fn apply_on_the_left<T: Real, const C: usize>(m: &mut [[T; 2]; C], j: JacobiRotation<T>) {
    let mut x: Vec<T> = m.iter().map(|col| col[0]).collect();
    let mut y: Vec<T> = m.iter().map(|col| col[1]).collect();
    rotate_pair(&mut x, &mut y, j);
    for (c, col) in m.iter_mut().enumerate() {
        col[0] = x[c];
        col[1] = y[c];
    }
}

/// `applyOnTheLeft(p, q, j)` on a 3x3 matrix: rows `p` and `q`.
fn apply_on_the_left3<T: Real>(m: &mut Mat3<T>, p: usize, q: usize, j: JacobiRotation<T>) {
    let mut x: Vec<T> = (0..3).map(|c| m.at(p, c)).collect();
    let mut y: Vec<T> = (0..3).map(|c| m.at(q, c)).collect();
    rotate_pair(&mut x, &mut y, j);
    for c in 0..3 {
        m.set(p, c, x[c]);
        m.set(q, c, y[c]);
    }
}

/// `applyOnTheRight(p, q, j)` on a 3x3 matrix: columns `p` and `q`, by `j.transpose()`.
fn apply_on_the_right3<T: Real>(m: &mut Mat3<T>, p: usize, q: usize, j: JacobiRotation<T>) {
    let mut x = m.cols[p];
    let mut y = m.cols[q];
    rotate_pair(&mut x, &mut y, j.transpose());
    m.cols[p] = x;
    m.cols[q] = y;
}

/// `real_2x2_jacobi_svd(matrix, p, q, &j_left, &j_right)`.
fn real_2x2_jacobi_svd<T: Real>(
    w: &Mat3<T>,
    p: usize,
    q: usize,
) -> (JacobiRotation<T>, JacobiRotation<T>) {
    // A 2x2 matrix, column by column: [[m00, m10], [m01, m11]].
    let mut m = [[w.at(p, p), w.at(q, p)], [w.at(p, q), w.at(q, q)]];
    let t = m[0][0].add(m[1][1]);
    let d = m[0][1].sub(m[1][0]);
    let rot1 = if d.abs() < T::MIN_POSITIVE {
        JacobiRotation {
            c: T::ONE,
            s: T::ZERO,
        }
    } else {
        let u = t.div(d);
        let tmp = T::ONE.add(u.mul(u)).sqrt();
        JacobiRotation {
            c: u.div(tmp),
            s: T::ONE.div(tmp),
        }
    };
    apply_on_the_left(&mut m, rot1);
    let j_right = JacobiRotation::make_jacobi(m[0][0], m[1][0], m[1][1]);
    let j_left = rot1.mul(j_right.transpose());
    (j_left, j_right)
}

/// The SVD of a 3x3 matrix, `matrix = U diag(singular_values) V^T`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JacobiSvd3<T> {
    /// `matrixU()`.
    pub u: Mat3<T>,
    /// `matrixV()`.
    pub v: Mat3<T>,
    /// `singularValues()`, in descending order.
    pub singular_values: Vec3<T>,
    /// `nonzeroSingularValues()`.
    pub nonzero: usize,
}

impl<T: Real> JacobiSvd3<T> {
    /// `JacobiSVD(matrix, ComputeFullU | ComputeFullV)`. `None` when the matrix holds a NaN or an
    /// infinity: Eigen then reports `InvalidInput` and leaves U, V and the singular values unset
    /// (uninitialised memory in a fixed-size SVD), so the C++ engine's results are undefined.
    pub fn compute(matrix: &Mat3<T>) -> Option<Self> {
        let precision = T::from_f64(2.0).mul(T::EPSILON);
        let consider_as_zero = T::MIN_POSITIVE;
        // cwiseAbs().maxCoeff<PropagateNaN>(): NaN if any coefficient is NaN, else the largest.
        let mut scale = T::ZERO;
        for x in matrix.storage() {
            let a = x.abs();
            if a.is_nan() {
                return None;
            }
            scale = orca_stl::max(scale, a);
        }
        if !scale.is_finite() {
            return None;
        }
        if scale == T::ZERO {
            scale = T::ONE;
        }
        let mut w = matrix.map(|x| x.div(scale));
        let mut u = Mat3::identity();
        let mut v = Mat3::identity();
        let mut max_diag = w.diagonal().cwise_abs().max_coeff_strided();
        let mut finished = false;
        while !finished {
            finished = true;
            for p in 1..3 {
                for q in 0..p {
                    let threshold = orca_stl::max(consider_as_zero, precision.mul(max_diag));
                    if w.at(p, q).abs() > threshold || w.at(q, p).abs() > threshold {
                        finished = false;
                        let (j_left, j_right) = real_2x2_jacobi_svd(&w, p, q);
                        apply_on_the_left3(&mut w, p, q, j_left);
                        apply_on_the_right3(&mut u, p, q, j_left.transpose());
                        apply_on_the_right3(&mut w, p, q, j_right);
                        apply_on_the_right3(&mut v, p, q, j_right);
                        max_diag = orca_stl::max(
                            max_diag,
                            orca_stl::max(w.at(p, p).abs(), w.at(q, q).abs()),
                        );
                    }
                }
            }
        }
        let mut sv = [T::ZERO; 3];
        for (i, s) in sv.iter_mut().enumerate() {
            let a = w.at(i, i);
            *s = a.abs();
            if a < T::ZERO {
                u.set_col(i, -u.col(i));
            }
        }
        let mut sv = Vector(sv) * scale;
        let mut nonzero = 3;
        for i in 0..3 {
            // tail(3 - i).maxCoeff(&pos): the first of the largest.
            let mut pos = 0;
            for k in 1..3 - i {
                if sv.0[i + pos] < sv.0[i + k] {
                    pos = k;
                }
            }
            if sv.0[i + pos] == T::ZERO {
                nonzero = i;
                break;
            }
            if pos != 0 {
                let pos = pos + i;
                sv.0.swap(i, pos);
                u.cols.swap(i, pos);
                v.cols.swap(i, pos);
            }
        }
        Some(JacobiSvd3 {
            u,
            v,
            singular_values: sv,
            nonzero,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagonal_matrices_sort_their_values() {
        let m = Mat3::from_rows([[1.0, 0.0, 0.0], [0.0, -3.0, 0.0], [0.0, 0.0, 2.0]]);
        let svd = JacobiSvd3::compute(&m).unwrap();
        assert_eq!(svd.singular_values, Vec3::new(3.0, 2.0, 1.0));
        assert_eq!(svd.nonzero, 3);
        let back = svd
            .u
            .mul_diagonal(&svd.singular_values)
            .mul(&svd.v.transpose());
        assert!(back.is_approx(&m, 1e-12));
        assert!(JacobiSvd3::compute(&Mat3::from_rows([[f64::NAN; 3]; 3])).is_none());
    }

    #[test]
    fn a_general_matrix_reconstructs() {
        let m = Mat3::from_rows([[0.3, -1.2, 0.5], [2.0, 0.1, -0.7], [0.4, 0.9, 1.5]]);
        let svd = JacobiSvd3::compute(&m).unwrap();
        let back = svd
            .u
            .mul_diagonal(&svd.singular_values)
            .mul(&svd.v.transpose());
        assert!(back.is_approx(&m, 1e-12));
        assert!(svd.singular_values.0[0] >= svd.singular_values.0[1]);
    }
}
