// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Eigen code: class AlignedBox of Eigen/src/Geometry/AlignedBox.h of Eigen 5.0.1 (the
// default constructor and setEmpty, the corner constructor, extend, contains, intersects,
// diagonal, cast, isEmpty and squaredExteriorDistance of a point), translated on 2026-11-13 by
// index-1. Its copyright is the Eigen authors' (Gael Guennebaud and the Eigen contributors), under
// the Mozilla Public License, version 2.0 (MPL-2.0; licenses/MPL-2.0.txt).
//
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0. If a copy of
// the MPL was not distributed with this file, You can obtain one at https://mozilla.org/MPL/2.0/.
// (Exhibit A of the MPL-2.0: the Eigen code here stays under it; the rest of the file is
// sketch-orca's, under the AGPL-3.0, as the MPL's section 3.3 allows.)

//! `Eigen::AlignedBox<Scalar, N>`, the box of each node of an AABB tree. It belongs in orca-eigen
//! (no piece carries it there), so index-1 translates it here, for the scalars and the methods the
//! AABB trees reach: `float`, `double` and `coord_t` (`int64_t`), in 2 and 3 dimensions.
//!
//! - The default constructor makes the box empty (`setEmpty`): the minimum at the scalar's
//!   largest value and the maximum at its lowest ([`BoxScalar`]), which is what an unused node of
//!   a tree holds.
//! - `extend` takes `cwiseMin` and `cwiseMax` of the corners, which are orca-eigen's: `std::min`
//!   and `std::max` per coefficient, except that the coefficients of a whole SSE2 packet of
//!   `int64_t` go through `pmin` and `pmax` of a `Packet2l`, whose compare is wrong past 2^31
//!   (`parity/HAZARDS.md`, orca-eigen). The box's corners are `Matrix<Scalar, N, 1>`, which have
//!   packet access, so a 2D `coord_t` box takes that path in both coefficients.
//! - `contains` and `intersects` compare coefficient by coefficient with true compares: Eigen's
//!   untyped comparisons (`m_min.array() <= p.array()`) have no packet access, so `all()` visits
//!   each `bool`.
//! - `diagonal().maxCoeff(&i)` is orca-eigen's `max_coeff_index` of `max - min`.
//! - `squaredExteriorDistance(p)` of a point of another scalar ([`ExteriorDistance`]): each
//!   comparison and difference in the C++ common type of the two scalars, the difference stored in
//!   the box's scalar (`Scalar aux`, so a `float` box rounds it to `float` and a `coord_t` box
//!   truncates it with `cvttsd2si`), and the sum in the box's scalar.

use orca_eigen::{Cast, Scalar, Vector};

/// A scalar of an AABB tree's boxes: `NumTraits<Scalar>::highest()` and `lowest()`, the corners
/// of an empty box.
pub trait BoxScalar: Scalar {
    /// `NumTraits<Scalar>::highest()`: `FLT_MAX`, `DBL_MAX`, `INT64_MAX`.
    const HIGHEST: Self;
    /// `NumTraits<Scalar>::lowest()`: `-FLT_MAX`, `-DBL_MAX`, `INT64_MIN`.
    const LOWEST: Self;
}

impl BoxScalar for f32 {
    const HIGHEST: Self = f32::MAX;
    const LOWEST: Self = -f32::MAX;
}

impl BoxScalar for f64 {
    const HIGHEST: Self = f64::MAX;
    const LOWEST: Self = -f64::MAX;
}

impl BoxScalar for i64 {
    const HIGHEST: Self = i64::MAX;
    const LOWEST: Self = i64::MIN;
}

/// `Eigen::AlignedBox<S, N>`: the minimum and maximum corners.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AlignedBox<S, const N: usize> {
    /// `min()`.
    pub min: Vector<S, N>,
    /// `max()`.
    pub max: Vector<S, N>,
}

impl<S: BoxScalar, const N: usize> Default for AlignedBox<S, N> {
    /// `AlignedBox()`: empty.
    fn default() -> Self {
        Self::empty()
    }
}

impl<S: BoxScalar, const N: usize> AlignedBox<S, N> {
    /// `AlignedBox()` and `setEmpty()`: the minimum at `highest()`, the maximum at `lowest()`.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            min: Vector::constant(S::HIGHEST),
            max: Vector::constant(S::LOWEST),
        }
    }

    /// `AlignedBox(min, max)`: the corners as given, not extended.
    #[must_use]
    pub fn new(min: Vector<S, N>, max: Vector<S, N>) -> Self {
        Self { min, max }
    }

    /// `isEmpty()`: some coefficient of the minimum above the maximum's.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        (0..N).any(|k| self.min[k] > self.max[k])
    }

    /// `extend(p)`: `m_min = m_min.cwiseMin(p); m_max = m_max.cwiseMax(p)`.
    pub fn extend_point(&mut self, p: &Vector<S, N>) {
        self.min = self.min.cwise_min(p);
        self.max = self.max.cwise_max(p);
    }

    /// `extend(b)`: `m_min = m_min.cwiseMin(b.m_min); m_max = m_max.cwiseMax(b.m_max)`.
    pub fn extend_box(&mut self, b: &Self) {
        self.min = self.min.cwise_min(&b.min);
        self.max = self.max.cwise_max(&b.max);
    }

    /// `diagonal()`: `max - min` (wrapping for `coord_t`, as MSVC's arithmetic does).
    #[must_use]
    pub fn diagonal(&self) -> Vector<S, N> {
        self.max - self.min
    }

    /// `contains(p)`: `min <= p` and `p <= max` in every coefficient.
    #[must_use]
    pub fn contains_point(&self, p: &Vector<S, N>) -> bool {
        (0..N).all(|k| self.min[k] <= p[k]) && (0..N).all(|k| p[k] <= self.max[k])
    }

    /// `contains(b)`: `min <= b.min` and `b.max <= max` in every coefficient.
    #[must_use]
    pub fn contains_box(&self, b: &Self) -> bool {
        (0..N).all(|k| self.min[k] <= b.min[k]) && (0..N).all(|k| b.max[k] <= self.max[k])
    }

    /// `intersects(b)`: `min <= b.max` and `b.min <= max` in every coefficient.
    #[must_use]
    pub fn intersects(&self, b: &Self) -> bool {
        (0..N).all(|k| self.min[k] <= b.max[k]) && (0..N).all(|k| b.min[k] <= self.max[k])
    }

    /// `cast<U>()`: `static_cast<U>` of each coefficient of both corners.
    #[must_use]
    pub fn cast<U: BoxScalar>(&self) -> AlignedBox<U, N>
    where
        S: Cast<U>,
    {
        AlignedBox {
            min: self.min.cast(),
            max: self.max.cast(),
        }
    }

    /// `squaredExteriorDistance(p)`: the squared distance from `p` to the box, 0 inside it, in the
    /// box's scalar ([`ExteriorDistance`]).
    #[must_use]
    pub fn squared_exterior_distance<Q>(&self, p: &Vector<Q, N>) -> S
    where
        S: ExteriorDistance<Q>,
    {
        S::squared_exterior_distance(&self.min, &self.max, p)
    }
}

/// `AlignedBox<Self, N>::squaredExteriorDistance` of a point whose scalar is `Q`.
pub trait ExteriorDistance<Q>: BoxScalar {
    /// For each coefficient `k`: if `min[k] > p[k]`, `aux = min[k] - p[k]`; else if
    /// `p[k] > max[k]`, `aux = p[k] - max[k]`; then `dist2 += aux * aux`. The comparisons and the
    /// differences are in the C++ common type of `Self` and `Q`; `aux` and `dist2` are `Self`.
    fn squared_exterior_distance<const N: usize>(
        min: &Vector<Self, N>,
        max: &Vector<Self, N>,
        p: &Vector<Q, N>,
    ) -> Self;
}

macro_rules! exterior_distance {
    ($s:ty, $q:ty, $common:ty, $to_s:expr) => {
        impl ExteriorDistance<$q> for $s {
            fn squared_exterior_distance<const N: usize>(
                min: &Vector<$s, N>,
                max: &Vector<$s, N>,
                p: &Vector<$q, N>,
            ) -> $s {
                let to_s: fn($common) -> $s = $to_s;
                let mut dist2: $s = <$s as Scalar>::ZERO;
                for k in 0..N {
                    let lo = min[k] as $common;
                    let hi = max[k] as $common;
                    let x = p[k] as $common;
                    if lo > x {
                        let aux = to_s(Scalar::sub(lo, x));
                        dist2 = dist2.add(aux.mul(aux));
                    } else if x > hi {
                        let aux = to_s(Scalar::sub(x, hi));
                        dist2 = dist2.add(aux.mul(aux));
                    }
                }
                dist2
            }
        }
    };
}

// A `float` box and a `double` point: the compares and differences in `double`, `aux` rounded to
// `float` (`cvtsd2ss`, which `as` matches).
exterior_distance!(f32, f64, f64, |d| d as f32);
exterior_distance!(f32, f32, f32, |d| d);
exterior_distance!(f64, f64, f64, |d| d);
exterior_distance!(f64, f32, f64, |d| d);
// A `coord_t` box and a `double` point: the corner converted to `double`, `aux` truncated back to
// `coord_t` (`cvttsd2si`, through orca-crt), the products and the sum wrapping.
exterior_distance!(i64, f64, f64, orca_crt::f64_to_i64);
exterior_distance!(i64, i64, i64, |d| d);
