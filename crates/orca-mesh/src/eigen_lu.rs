// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Eigen code: the determinant of a dynamic-size block as Eigen/src/LU/Determinant.h
// (determinant_impl, general case) and Eigen/src/LU/PartialPivLU.h (partial_lu_impl::unblocked_lu,
// PartialPivLU::determinant) of Eigen 5.0.1 compute it, with the first-maximum search of
// Eigen/src/Core/FindCoeff.h, translated on 2026-11-07. Their copyright is the Eigen authors'
// (Benoit Jacob, Gael Guennebaud and the Eigen contributors), under the Mozilla Public License,
// version 2.0 (MPL-2.0; licenses/MPL-2.0.txt).
//
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0. If a copy of
// the MPL was not distributed with this file, You can obtain one at https://mozilla.org/MPL/2.0/.
// (Exhibit A of the MPL-2.0: the Eigen code here stays under it; the rest of the file is
// sketch-orca's, under the AGPL-3.0, as the MPL's section 3.3 allows.)

//! `m.block(0, 0, 3, 3).determinant()`: a block of runtime size, whose determinant Eigen takes
//! from a partial-pivoting LU factorisation, not from the 3x3 cofactor formula a fixed-size
//! `Matrix3d` uses (`orca_eigen::Matrix::determinant`). `TriangleMesh::transform` scales the
//! volume by it.
//!
//! The factorisation, column by column: the row with the largest absolute value in the column
//! (the first of equal ones, as FindCoeff.h's packet search finds it for numbers) is swapped up
//! unless the value is zero, the column below the pivot is divided by it, and the corner below and
//! right loses the outer product of that column and the pivot row (one `a - b * c` per
//! coefficient). The determinant is the diagonal's product, `(d0 * d1) * d2`, negated after an odd
//! number of swaps.

use orca_eigen::Mat3;

/// `m.block(0, 0, 3, 3).determinant()` of a 3x3 matrix (see the module documentation).
#[must_use]
pub fn dynamic_block_determinant(m: &Mat3<f64>) -> f64 {
    let mut lu = [[0f64; 3]; 3];
    for (r, row) in lu.iter_mut().enumerate() {
        for (c, x) in row.iter_mut().enumerate() {
            *x = m.at(r, c);
        }
    }
    let mut nb_transpositions = 0u32;
    for k in 0..3 {
        // maxCoeff(&index) of the column's absolute values from row k: the first largest.
        let mut row_of_biggest = k;
        let mut biggest = lu[k][k].abs();
        for (i, row) in lu.iter().enumerate().skip(k + 1) {
            let candidate = row[k].abs();
            if candidate > biggest {
                biggest = candidate;
                row_of_biggest = i;
            }
        }
        if biggest != 0.0 {
            if k != row_of_biggest {
                lu.swap(k, row_of_biggest);
                nb_transpositions += 1;
            }
            let pivot = lu[k][k];
            for row in lu.iter_mut().skip(k + 1) {
                row[k] /= pivot;
            }
        }
        if k < 2 {
            for i in k + 1..3 {
                for j in k + 1..3 {
                    lu[i][j] -= lu[i][k] * lu[k][j];
                }
            }
        }
    }
    let det_p = if nb_transpositions % 2 == 1 {
        -1.0
    } else {
        1.0
    };
    det_p * (lu[0][0] * lu[1][1] * lu[2][2])
}

#[cfg(test)]
mod tests {
    use super::dynamic_block_determinant;
    use orca_eigen::Mat3;

    #[test]
    fn determinants_of_simple_matrices() {
        let id = Mat3::<f64>::identity();
        assert_eq!(dynamic_block_determinant(&id), 1.0);
        let swap = Mat3::from_rows([[0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]);
        assert_eq!(dynamic_block_determinant(&swap), -1.0);
        let scale = Mat3::from_rows([[2.0, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, -0.5]]);
        assert_eq!(dynamic_block_determinant(&scale), -3.0);
        let singular = Mat3::from_rows([[1.0, 2.0, 3.0], [2.0, 4.0, 6.0], [0.0, 0.0, 0.0]]);
        assert_eq!(dynamic_block_determinant(&singular), 0.0);
    }
}
