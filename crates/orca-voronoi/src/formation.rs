// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Boost code: Boost.Polygon of Boost 1.84.0, translated on 2026-11-13 by orca-o3-voronoi-2
// from boost/polygon/detail/voronoi_predicates.hpp, lines 525-1533 (voronoi_predicates'
// mp_circle_formation_functor with sqrt_expr_evaluator_pss4 and pss3,
// lazy_circle_formation_functor and circle_formation_predicate). Copyright Andrii Sydorchuk
// 2010-2012, under the Boost Software License, version 1.0 (BSL-1.0; licenses/BSL-1.0.txt).

//! The rest of Boost.Polygon's `voronoi_predicates<voronoi_ctype_traits<int32>>`
//! (`voronoi_predicates.hpp` 525-1533): the functors that compute a circle event from three
//! sites, and the predicate the builder asks whether three sites make one. Module `predicates`
//! holds lines 1-524.
//!
//! - [`LazyCircleFormationFunctor`] computes the centre and the lowest x of the circle through
//!   three sites (three points, two points and a segment, a point and two segments, or three
//!   segments) in `double`, tracking each value's relative error with `robust_fpt` and
//!   `robust_dif`. A value whose error is more than `ULPS` (64) epsilons is computed again by
//!   [`MpCircleFormationFunctor`], which evaluates the same expressions exactly with
//!   `extended_int<64>` and sums of square roots (`robust_sqrt_expr`, `extended_exponent_fpt`).
//! - [`CircleFormationPredicate`] dispatches on which of the three sites are segments, asks the
//!   existence predicate first (`circle_existence_predicate`, module `predicates`), lets the
//!   formation functor compute the circle, and drops a circle whose centre lies outside one of the
//!   sites when that site is a vertical segment (`lies_outside_vertical_segment`, within `ULPS`).
//!
//! The C++ functors are templates over the site and circle types and the predicate over its
//! existence predicate and formation functor; here the functors compute on [`SiteEvent`] and
//! [`CircleEvent`] (module `structures`, the only instantiation), and the predicate is generic
//! over the traits [`CircleExistence`] and [`CircleFormationFunctor`], with the C++'s defaults.
//! Each C++ default argument (`recompute_c_x = true` and the two others) is an argument here. The
//! order of every operation is the C++'s: `extended_int` products and sums by value, in the
//! C++'s association, and each `robust_fpt` and `robust_dif` operator in the form the C++ calls
//! (`a += b` against `a = a + b`). A coordinate negated as `int32` (`-site1.y()` in `pss`) wraps
//! for `INT32_MIN`, as MSVC's does.
//!
//! Nothing of these lines is left out.

use crate::ctypes::{
    BigInt, Efpt64, TypeConverterFpt, UlpComparison, UlpResult, get_sqrt, is_neg, is_pos, is_zero,
};
use crate::predicates::{
    CircleExistencePredicate, Orientation, OrientationTest, ULPS, is_vertical,
};
use crate::robust_fpt::{RobustDif, RobustFpt, RobustSqrtExpr};
use crate::structures::{CircleEvent, SiteEvent};

/// `static_cast<int_x2_type>(v)` assigned to a `big_int_type`.
fn big(v: i64) -> BigInt {
    BigInt::from_i64(v)
}

/// `static_cast<int_x2_type>(a) - static_cast<int_x2_type>(b)`.
fn dif(a: i32, b: i32) -> i64 {
    i64::from(a) - i64::from(b)
}

/// `static_cast<int_x2_type>(a) + static_cast<int_x2_type>(b)`.
fn sum(a: i32, b: i32) -> i64 {
    i64::from(a) + i64::from(b)
}

/// `ULPS` as the `double` the `ulp() > ULPS` comparisons convert it to.
fn ulps() -> f64 {
    f64::from(ULPS)
}

// ---------------------------------------------------------------- mp_circle_formation_functor (526-982)

/// `mp_circle_formation_functor<site_event<int32>, circle_event<double>>`: recomputes the
/// parameters of a circle event exactly.
#[derive(Clone, Copy, Debug, Default)]
pub struct MpCircleFormationFunctor {
    sqrt_expr: RobustSqrtExpr,
    to_fpt: TypeConverterFpt,
}

impl MpCircleFormationFunctor {
    /// `ppp(site1, site2, site3, circle, recompute_c_x, recompute_c_y, recompute_lower_x)`.
    #[allow(clippy::too_many_arguments)] // the C++'s seven parameters
    pub fn ppp(
        &mut self,
        site1: &SiteEvent,
        site2: &SiteEvent,
        site3: &SiteEvent,
        circle: &mut CircleEvent,
        recompute_c_x: bool,
        recompute_c_y: bool,
        recompute_lower_x: bool,
    ) {
        let to_fpt = self.to_fpt;
        let dif_x = [
            big(dif(site1.x(), site2.x())),
            big(dif(site2.x(), site3.x())),
            big(dif(site1.x(), site3.x())),
        ];
        let dif_y = [
            big(dif(site1.y(), site2.y())),
            big(dif(site2.y(), site3.y())),
            big(dif(site1.y(), site3.y())),
        ];
        let sum_x = [
            big(sum(site1.x(), site2.x())),
            big(sum(site2.x(), site3.x())),
        ];
        let sum_y = [
            big(sum(site1.y(), site2.y())),
            big(sum(site2.y(), site3.y())),
        ];
        let inv_denom =
            to_fpt.call(&0.5) / to_fpt.call(&(dif_x[0] * dif_y[1] - dif_x[1] * dif_y[0]));
        let numer1 = dif_x[0] * sum_x[0] + dif_y[0] * sum_y[0];
        let numer2 = dif_x[1] * sum_x[1] + dif_y[1] * sum_y[1];

        if recompute_c_x || recompute_lower_x {
            let c_x = numer1 * dif_y[1] - numer2 * dif_y[0];
            circle.set_x(to_fpt.call(&c_x) * inv_denom);

            if recompute_lower_x {
                // Evaluate radius of the circle.
                let sqr_r = (dif_x[0] * dif_x[0] + dif_y[0] * dif_y[0])
                    * (dif_x[1] * dif_x[1] + dif_y[1] * dif_y[1])
                    * (dif_x[2] * dif_x[2] + dif_y[2] * dif_y[2]);
                let r = get_sqrt(&to_fpt.call(&sqr_r));

                // If c_x >= 0 then lower_x = c_x + r,
                // else lower_x = (c_x * c_x - r * r) / (c_x - r).
                // To guarantee epsilon relative error.
                if !is_neg(&circle.x()) {
                    if !is_neg(&inv_denom) {
                        circle.set_lower_x(circle.x() + r * inv_denom);
                    } else {
                        circle.set_lower_x(circle.x() - r * inv_denom);
                    }
                } else {
                    let numer = c_x * c_x - sqr_r;
                    let lower_x = to_fpt.call(&numer) * inv_denom / (to_fpt.call(&c_x) + r);
                    circle.set_lower_x(lower_x);
                }
            }
        }

        if recompute_c_y {
            let c_y = numer2 * dif_x[0] - numer1 * dif_x[1];
            circle.set_y(to_fpt.call(&c_y) * inv_denom);
        }
    }

    /// `pps(site1, site2, site3, segment_index, c_event, recompute_c_x, recompute_c_y,
    /// recompute_lower_x)`: recomputes the circle of two point sites and a segment site with the
    /// high-precision arithmetic.
    #[allow(clippy::too_many_arguments)] // the C++'s eight parameters
    pub fn pps(
        &mut self,
        site1: &SiteEvent,
        site2: &SiteEvent,
        site3: &SiteEvent,
        segment_index: i32,
        c_event: &mut CircleEvent,
        recompute_c_x: bool,
        recompute_c_y: bool,
        recompute_lower_x: bool,
    ) {
        let to_fpt = self.to_fpt;
        let mut c_a = [BigInt::default(); 4];
        let mut c_b = [BigInt::default(); 4];
        let line_a = big(dif(site3.y1(), site3.y0()));
        let line_b = big(dif(site3.x0(), site3.x1()));
        let segm_len = line_a * line_a + line_b * line_b;
        let vec_x = big(dif(site2.y(), site1.y()));
        let vec_y = big(dif(site1.x(), site2.x()));
        let sum_x = big(sum(site1.x(), site2.x()));
        let sum_y = big(sum(site1.y(), site2.y()));
        let teta = line_a * vec_x + line_b * vec_y;
        let mut denom = vec_x * line_b - vec_y * line_a;

        let mut dif0 = big(dif(site3.y1(), site1.y()));
        let mut dif1 = big(dif(site1.x(), site3.x1()));
        let a = line_a * dif1 - line_b * dif0;
        dif0 = big(dif(site3.y1(), site2.y()));
        dif1 = big(dif(site2.x(), site3.x1()));
        let b = line_a * dif1 - line_b * dif0;
        let sum_ab = a + b;

        if is_zero(&denom) {
            let numer = teta * teta - sum_ab * sum_ab;
            denom = teta * sum_ab;
            c_a[0] = denom * sum_x * 2_i32 + numer * vec_x;
            c_b[0] = segm_len;
            c_a[1] = denom * sum_ab * 2_i32 + numer * teta;
            c_b[1].assign_i32(1);
            c_a[2] = denom * sum_y * 2_i32 + numer * vec_y;
            let inv_denom = to_fpt.call(&1.0) / to_fpt.call(&denom);
            if recompute_c_x {
                c_event.set_x(to_fpt.call(&0.25) * to_fpt.call(&c_a[0]) * inv_denom);
            }
            if recompute_c_y {
                c_event.set_y(to_fpt.call(&0.25) * to_fpt.call(&c_a[2]) * inv_denom);
            }
            if recompute_lower_x {
                c_event.set_lower_x(
                    to_fpt.call(&0.25) * to_fpt.call(&self.sqrt_expr.eval2(&c_a, &c_b)) * inv_denom
                        / get_sqrt(&to_fpt.call(&segm_len)),
                );
            }
            return;
        }

        let det = (teta * teta + denom * denom) * a * b * 4_i32;
        let mut inv_denom_sqr = to_fpt.call(&1.0) / to_fpt.call(&denom);
        inv_denom_sqr *= inv_denom_sqr;

        if recompute_c_x || recompute_lower_x {
            c_a[0] = sum_x * denom * denom + teta * sum_ab * vec_x;
            c_b[0].assign_i32(1);
            c_a[1] = if segment_index == 2 { -vec_x } else { vec_x };
            c_b[1] = det;
            if recompute_c_x {
                c_event.set_x(
                    to_fpt.call(&0.5)
                        * to_fpt.call(&self.sqrt_expr.eval2(&c_a, &c_b))
                        * inv_denom_sqr,
                );
            }
        }

        if recompute_c_y || recompute_lower_x {
            c_a[2] = sum_y * denom * denom + teta * sum_ab * vec_y;
            c_b[2].assign_i32(1);
            c_a[3] = if segment_index == 2 { -vec_y } else { vec_y };
            c_b[3] = det;
            if recompute_c_y {
                c_event.set_y(
                    to_fpt.call(&0.5)
                        * to_fpt.call(&self.sqrt_expr.eval2(&c_a[2..], &c_b[2..]))
                        * inv_denom_sqr,
                );
            }
        }

        if recompute_lower_x {
            c_b[0] = c_b[0] * segm_len;
            c_b[1] = c_b[1] * segm_len;
            c_a[2] = sum_ab * (denom * denom + teta * teta);
            c_b[2].assign_i32(1);
            c_a[3] = if segment_index == 2 { -teta } else { teta };
            c_b[3] = det;
            c_event.set_lower_x(
                to_fpt.call(&0.5) * to_fpt.call(&self.sqrt_expr.eval4(&c_a, &c_b)) * inv_denom_sqr
                    / get_sqrt(&to_fpt.call(&segm_len)),
            );
        }
    }

    /// `pss(site1, site2, site3, point_index, c_event, recompute_c_x, recompute_c_y,
    /// recompute_lower_x)`: recomputes the circle of a point site and two segment sites with the
    /// high-precision arithmetic.
    #[allow(clippy::too_many_arguments)] // the C++'s eight parameters
    #[allow(clippy::too_many_lines)] // one C++ function, translated statement by statement
    pub fn pss(
        &mut self,
        site1: &SiteEvent,
        site2: &SiteEvent,
        site3: &SiteEvent,
        point_index: i32,
        c_event: &mut CircleEvent,
        recompute_c_x: bool,
        recompute_c_y: bool,
        recompute_lower_x: bool,
    ) {
        let to_fpt = self.to_fpt;
        let mut a = [BigInt::default(); 2];
        let mut b = [BigInt::default(); 2];
        let mut c = [BigInt::default(); 2];
        let mut c_a = [BigInt::default(); 4];
        let mut c_b = [BigInt::default(); 4];
        let segm_start1 = *site2.point1();
        let segm_end1 = *site2.point0();
        let segm_start2 = *site3.point0();
        let segm_end2 = *site3.point1();
        a[0] = big(dif(segm_end1.x(), segm_start1.x()));
        b[0] = big(dif(segm_end1.y(), segm_start1.y()));
        a[1] = big(dif(segm_end2.x(), segm_start2.x()));
        b[1] = big(dif(segm_end2.y(), segm_start2.y()));
        let orientation = a[1] * b[0] - a[0] * b[1];
        if is_zero(&orientation) {
            let denom = to_fpt.call(&2.0) * to_fpt.call(&(a[0] * a[0] + b[0] * b[0]));
            c[0] = b[0] * dif(segm_start2.x(), segm_start1.x())
                - a[0] * dif(segm_start2.y(), segm_start1.y());
            let dx =
                a[0] * dif(site1.y(), segm_start1.y()) - b[0] * dif(site1.x(), segm_start1.x());
            let dy =
                b[0] * dif(site1.x(), segm_start2.x()) - a[0] * dif(site1.y(), segm_start2.y());
            c_b[0] = dx * dy;
            c_b[1].assign_i32(1);

            if recompute_c_y {
                c_a[0] = b[0] * if point_index == 2 { 2_i32 } else { -2_i32 };
                c_a[1] = a[0] * a[0] * sum(segm_start1.y(), segm_start2.y())
                    - a[0]
                        * b[0]
                        * (i64::from(segm_start1.x()) + i64::from(segm_start2.x())
                            - i64::from(site1.x()) * 2)
                    + b[0] * b[0] * (i64::from(site1.y()) * 2);
                let c_y = to_fpt.call(&self.sqrt_expr.eval2(&c_a, &c_b));
                c_event.set_y(c_y / denom);
            }

            if recompute_c_x || recompute_lower_x {
                c_a[0] = a[0] * if point_index == 2 { 2_i32 } else { -2_i32 };
                c_a[1] = b[0] * b[0] * sum(segm_start1.x(), segm_start2.x())
                    - a[0]
                        * b[0]
                        * (i64::from(segm_start1.y()) + i64::from(segm_start2.y())
                            - i64::from(site1.y()) * 2)
                    + a[0] * a[0] * (i64::from(site1.x()) * 2);

                if recompute_c_x {
                    let c_x = to_fpt.call(&self.sqrt_expr.eval2(&c_a, &c_b));
                    c_event.set_x(c_x / denom);
                }

                if recompute_lower_x {
                    c_a[2] = if is_neg(&c[0]) { -c[0] } else { c[0] };
                    c_b[2] = a[0] * a[0] + b[0] * b[0];
                    let lower_x = to_fpt.call(&self.sqrt_expr.eval3(&c_a, &c_b));
                    c_event.set_lower_x(lower_x / denom);
                }
            }
            return;
        }
        c[0] = b[0] * segm_end1.x() - a[0] * segm_end1.y();
        c[1] = a[1] * segm_end2.y() - b[1] * segm_end2.x();
        let ix = a[0] * c[1] + a[1] * c[0];
        let iy = b[0] * c[1] + b[1] * c[0];
        let dx = ix - orientation * site1.x();
        let dy = iy - orientation * site1.y();
        if is_zero(&dx) && is_zero(&dy) {
            let denom = to_fpt.call(&orientation);
            let c_x = to_fpt.call(&ix) / denom;
            let c_y = to_fpt.call(&iy) / denom;
            *c_event = CircleEvent::new(c_x, c_y, c_x);
            return;
        }

        let sign = BigInt::from_i32(
            (if point_index == 2 { 1 } else { -1 }) * (if is_neg(&orientation) { 1 } else { -1 }),
        );
        c_a[0] = a[1] * -dx + b[1] * -dy;
        c_a[1] = a[0] * -dx + b[0] * -dy;
        c_a[2] = sign;
        c_a[3].assign_i32(0);
        c_b[0] = a[0] * a[0] + b[0] * b[0];
        c_b[1] = a[1] * a[1] + b[1] * b[1];
        c_b[2] = a[0] * a[1] + b[0] * b[1];
        c_b[3] = (a[0] * dy - b[0] * dx) * (a[1] * dy - b[1] * dx) * -2_i32;
        let temp = to_fpt.call(&self.sqrt_expr_evaluator_pss4(&c_a, &c_b));
        let denom = temp * to_fpt.call(&orientation);

        if recompute_c_y {
            c_a[0] = b[1] * (dx * dx + dy * dy) - iy * (dx * a[1] + dy * b[1]);
            c_a[1] = b[0] * (dx * dx + dy * dy) - iy * (dx * a[0] + dy * b[0]);
            c_a[2] = iy * sign;
            let cy = to_fpt.call(&self.sqrt_expr_evaluator_pss4(&c_a, &c_b));
            c_event.set_y(cy / denom);
        }

        if recompute_c_x || recompute_lower_x {
            c_a[0] = a[1] * (dx * dx + dy * dy) - ix * (dx * a[1] + dy * b[1]);
            c_a[1] = a[0] * (dx * dx + dy * dy) - ix * (dx * a[0] + dy * b[0]);
            c_a[2] = ix * sign;

            if recompute_c_x {
                let cx = to_fpt.call(&self.sqrt_expr_evaluator_pss4(&c_a, &c_b));
                c_event.set_x(cx / denom);
            }

            if recompute_lower_x {
                c_a[3] =
                    orientation * (dx * dx + dy * dy) * if is_neg(&temp) { -1_i32 } else { 1_i32 };
                let lower_x = to_fpt.call(&self.sqrt_expr_evaluator_pss4(&c_a, &c_b));
                c_event.set_lower_x(lower_x / denom);
            }
        }
    }

    /// `sss(site1, site2, site3, c_event, recompute_c_x, recompute_c_y, recompute_lower_x)`:
    /// recomputes the circle of three segment sites with the high-precision arithmetic.
    #[allow(clippy::too_many_arguments)] // the C++'s seven parameters
    #[allow(clippy::needless_range_loop)] // the C++'s index loops over a, b, c, cA and cB
    pub fn sss(
        &mut self,
        site1: &SiteEvent,
        site2: &SiteEvent,
        site3: &SiteEvent,
        c_event: &mut CircleEvent,
        recompute_c_x: bool,
        recompute_c_y: bool,
        recompute_lower_x: bool,
    ) {
        let to_fpt = self.to_fpt;
        let mut c_a = [BigInt::default(); 4];
        let mut c_b = [BigInt::default(); 4];
        // cA - corresponds to the cross product.
        // cB - corresponds to the squared length.
        let a = [
            big(dif(site1.x1(), site1.x0())),
            big(dif(site2.x1(), site2.x0())),
            big(dif(site3.x1(), site3.x0())),
        ];
        let b = [
            big(dif(site1.y1(), site1.y0())),
            big(dif(site2.y1(), site2.y0())),
            big(dif(site3.y1(), site3.y0())),
        ];
        let c = [
            big(cross_i64(site1)),
            big(cross_i64(site2)),
            big(cross_i64(site3)),
        ];

        for i in 0..3 {
            c_b[i] = a[i] * a[i] + b[i] * b[i];
        }

        for i in 0..3 {
            let j = (i + 1) % 3;
            let k = (i + 2) % 3;
            c_a[i] = a[j] * b[k] - a[k] * b[j];
        }
        let denom = to_fpt.call(&self.sqrt_expr.eval3(&c_a, &c_b));

        if recompute_c_y {
            for i in 0..3 {
                let j = (i + 1) % 3;
                let k = (i + 2) % 3;
                c_a[i] = b[j] * c[k] - b[k] * c[j];
            }
            let c_y = to_fpt.call(&self.sqrt_expr.eval3(&c_a, &c_b));
            c_event.set_y(c_y / denom);
        }

        if recompute_c_x || recompute_lower_x {
            c_a[3].assign_i32(0);
            for i in 0..3 {
                let j = (i + 1) % 3;
                let k = (i + 2) % 3;
                c_a[i] = a[j] * c[k] - a[k] * c[j];
                if recompute_lower_x {
                    c_a[3] = c_a[3] + c_a[i] * b[i];
                }
            }

            if recompute_c_x {
                let c_x = to_fpt.call(&self.sqrt_expr.eval3(&c_a, &c_b));
                c_event.set_x(c_x / denom);
            }

            if recompute_lower_x {
                c_b[3].assign_i32(1);
                let lower_x = to_fpt.call(&self.sqrt_expr.eval4(&c_a, &c_b));
                c_event.set_lower_x(lower_x / denom);
            }
        }
    }

    /// `sqrt_expr_evaluator_pss4<big_int_type, efpt_type>(A, B)`: evaluates
    /// `A[3] + A[0] * sqrt(B[0]) + A[1] * sqrt(B[1]) + A[2] * sqrt(B[3] * (sqrt(B[0] * B[1]) +
    /// B[2]))`.
    fn sqrt_expr_evaluator_pss4(&mut self, a: &[BigInt], b: &[BigInt]) -> Efpt64 {
        let mut c_a = [BigInt::default(); 4];
        let mut c_b = [BigInt::default(); 4];
        if is_zero(&a[3]) {
            let lh = self.sqrt_expr.eval2(a, b);
            c_a[0].assign_i32(1);
            c_b[0] = b[0] * b[1];
            c_a[1] = b[2];
            c_b[1].assign_i32(1);
            let rh = self.sqrt_expr.eval1(&a[2..], &b[3..])
                * get_sqrt(&self.sqrt_expr.eval2(&c_a, &c_b));
            if (!is_neg(&lh) && !is_neg(&rh)) || (!is_pos(&lh) && !is_pos(&rh)) {
                return lh + rh;
            }
            c_a[0] = a[0] * a[0] * b[0] + a[1] * a[1] * b[1] - a[2] * a[2] * b[3] * b[2];
            c_b[0].assign_i32(1);
            c_a[1] = a[0] * a[1] * 2_i32 - a[2] * a[2] * b[3];
            c_b[1] = b[0] * b[1];
            let numer = self.sqrt_expr.eval2(&c_a, &c_b);
            return numer / (lh - rh);
        }
        c_a[0].assign_i32(1);
        c_b[0] = b[0] * b[1];
        c_a[1] = b[2];
        c_b[1].assign_i32(1);
        let rh =
            self.sqrt_expr.eval1(&a[2..], &b[3..]) * get_sqrt(&self.sqrt_expr.eval2(&c_a, &c_b));
        c_a[0] = a[0];
        c_b[0] = b[0];
        c_a[1] = a[1];
        c_b[1] = b[1];
        c_a[2] = a[3];
        c_b[2].assign_i32(1);
        let lh = self.sqrt_expr.eval3(&c_a, &c_b);
        if (!is_neg(&lh) && !is_neg(&rh)) || (!is_pos(&lh) && !is_pos(&rh)) {
            return lh + rh;
        }
        c_a[0] = a[3] * a[0] * 2_i32;
        c_a[1] = a[3] * a[1] * 2_i32;
        c_a[2] = a[0] * a[0] * b[0] + a[1] * a[1] * b[1] + a[3] * a[3] - a[2] * a[2] * b[2] * b[3];
        c_a[3] = a[0] * a[1] * 2_i32 - a[2] * a[2] * b[3];
        c_b[3] = b[0] * b[1];
        let numer = self.sqrt_expr_evaluator_pss3(&c_a, &c_b);
        numer / (lh - rh)
    }

    /// `sqrt_expr_evaluator_pss3<big_int_type, efpt_type>(A, B)`: evaluates
    /// `A[0] * sqrt(B[0]) + A[1] * sqrt(B[1]) + A[2] + A[3] * sqrt(B[0] * B[1])`, with
    /// `B[3] = B[0] * B[1]`.
    fn sqrt_expr_evaluator_pss3(&mut self, a: &[BigInt], b: &[BigInt]) -> Efpt64 {
        let mut c_a = [BigInt::default(); 2];
        let mut c_b = [BigInt::default(); 2];
        let lh = self.sqrt_expr.eval2(a, b);
        let rh = self.sqrt_expr.eval2(&a[2..], &b[2..]);
        if (!is_neg(&lh) && !is_neg(&rh)) || (!is_pos(&lh) && !is_pos(&rh)) {
            return lh + rh;
        }
        c_a[0] = a[0] * a[0] * b[0] + a[1] * a[1] * b[1] - a[2] * a[2] - a[3] * a[3] * b[0] * b[1];
        c_b[0].assign_i32(1);
        c_a[1] = (a[0] * a[1] - a[2] * a[3]) * 2_i32;
        c_b[1] = b[3];
        let numer = self.sqrt_expr.eval2(&c_a, &c_b);
        numer / (lh - rh)
    }
}

/// `static_cast<int_x2_type>(s.x0()) * static_cast<int_x2_type>(s.y1()) -
/// static_cast<int_x2_type>(s.y0()) * static_cast<int_x2_type>(s.x1())`, in `int64` (it stays
/// inside the range for `int32` coordinates; MSVC would wrap past it).
fn cross_i64(s: &SiteEvent) -> i64 {
    (i64::from(s.x0()).wrapping_mul(i64::from(s.y1())))
        .wrapping_sub(i64::from(s.y0()).wrapping_mul(i64::from(s.x1())))
}

// ---------------------------------------------------------------- lazy_circle_formation_functor (984-1431)

/// `lazy_circle_formation_functor<site_event<int32>, circle_event<double>>`: the circle event's
/// parameters in `double` with their relative errors, recomputed exactly where the error is more
/// than `ULPS`.
#[derive(Clone, Copy, Debug, Default)]
pub struct LazyCircleFormationFunctor {
    exact_circle_formation_functor: MpCircleFormationFunctor,
    to_fpt: TypeConverterFpt,
}

impl LazyCircleFormationFunctor {
    /// `ppp(site1, site2, site3, c_event)`: three point sites.
    pub fn ppp(
        &mut self,
        site1: &SiteEvent,
        site2: &SiteEvent,
        site3: &SiteEvent,
        c_event: &mut CircleEvent,
    ) {
        let to_fpt = self.to_fpt;
        let dif_x1 = to_fpt.call(&site1.x()) - to_fpt.call(&site2.x());
        let dif_x2 = to_fpt.call(&site2.x()) - to_fpt.call(&site3.x());
        let dif_y1 = to_fpt.call(&site1.y()) - to_fpt.call(&site2.y());
        let dif_y2 = to_fpt.call(&site2.y()) - to_fpt.call(&site3.y());
        let orientation = robust_cross_product_fpt(
            dif(site1.x(), site2.x()),
            dif(site2.x(), site3.x()),
            dif(site1.y(), site2.y()),
            dif(site2.y(), site3.y()),
        );
        let inv_orientation =
            RobustFpt::with_error(to_fpt.call(&0.5) / orientation, to_fpt.call(&2.0));
        let sum_x1 = to_fpt.call(&site1.x()) + to_fpt.call(&site2.x());
        let sum_x2 = to_fpt.call(&site2.x()) + to_fpt.call(&site3.x());
        let sum_y1 = to_fpt.call(&site1.y()) + to_fpt.call(&site2.y());
        let sum_y2 = to_fpt.call(&site2.y()) + to_fpt.call(&site3.y());
        let dif_x3 = to_fpt.call(&site1.x()) - to_fpt.call(&site3.x());
        let dif_y3 = to_fpt.call(&site1.y()) - to_fpt.call(&site3.y());
        let mut c_x = RobustDif::new();
        let mut c_y = RobustDif::new();
        c_x += RobustFpt::with_error(dif_x1 * sum_x1 * dif_y2, to_fpt.call(&2.0));
        c_x += RobustFpt::with_error(dif_y1 * sum_y1 * dif_y2, to_fpt.call(&2.0));
        c_x -= RobustFpt::with_error(dif_x2 * sum_x2 * dif_y1, to_fpt.call(&2.0));
        c_x -= RobustFpt::with_error(dif_y2 * sum_y2 * dif_y1, to_fpt.call(&2.0));
        c_y += RobustFpt::with_error(dif_x2 * sum_x2 * dif_x1, to_fpt.call(&2.0));
        c_y += RobustFpt::with_error(dif_y2 * sum_y2 * dif_x1, to_fpt.call(&2.0));
        c_y -= RobustFpt::with_error(dif_x1 * sum_x1 * dif_x2, to_fpt.call(&2.0));
        c_y -= RobustFpt::with_error(dif_y1 * sum_y1 * dif_x2, to_fpt.call(&2.0));
        let mut lower_x = c_x;
        lower_x -= RobustFpt::with_error(
            get_sqrt(
                &((dif_x1 * dif_x1 + dif_y1 * dif_y1)
                    * (dif_x2 * dif_x2 + dif_y2 * dif_y2)
                    * (dif_x3 * dif_x3 + dif_y3 * dif_y3)),
            ),
            to_fpt.call(&5.0),
        );
        *c_event = CircleEvent::new(
            c_x.dif().fpv() * inv_orientation.fpv(),
            c_y.dif().fpv() * inv_orientation.fpv(),
            lower_x.dif().fpv() * inv_orientation.fpv(),
        );
        let recompute_c_x = c_x.dif().re() > ulps();
        let recompute_c_y = c_y.dif().re() > ulps();
        let recompute_lower_x = lower_x.dif().re() > ulps();
        if recompute_c_x || recompute_c_y || recompute_lower_x {
            self.exact_circle_formation_functor.ppp(
                site1,
                site2,
                site3,
                c_event,
                recompute_c_x,
                recompute_c_y,
                recompute_lower_x,
            );
        }
    }

    /// `pps(site1, site2, site3, segment_index, c_event)`: two point sites and a segment site.
    pub fn pps(
        &mut self,
        site1: &SiteEvent,
        site2: &SiteEvent,
        site3: &SiteEvent,
        segment_index: i32,
        c_event: &mut CircleEvent,
    ) {
        let to_fpt = self.to_fpt;
        let line_a = to_fpt.call(&site3.y1()) - to_fpt.call(&site3.y0());
        let line_b = to_fpt.call(&site3.x0()) - to_fpt.call(&site3.x1());
        let vec_x = to_fpt.call(&site2.y()) - to_fpt.call(&site1.y());
        let vec_y = to_fpt.call(&site1.x()) - to_fpt.call(&site2.x());
        let teta = RobustFpt::with_error(
            robust_cross_product_fpt(
                dif(site3.y1(), site3.y0()),
                dif(site3.x0(), site3.x1()),
                dif(site2.x(), site1.x()),
                dif(site2.y(), site1.y()),
            ),
            to_fpt.call(&1.0),
        );
        let a = RobustFpt::with_error(
            robust_cross_product_fpt(
                dif(site3.y0(), site3.y1()),
                dif(site3.x0(), site3.x1()),
                dif(site3.y1(), site1.y()),
                dif(site3.x1(), site1.x()),
            ),
            to_fpt.call(&1.0),
        );
        let b = RobustFpt::with_error(
            robust_cross_product_fpt(
                dif(site3.y0(), site3.y1()),
                dif(site3.x0(), site3.x1()),
                dif(site3.y1(), site2.y()),
                dif(site3.x1(), site2.x()),
            ),
            to_fpt.call(&1.0),
        );
        let denom = RobustFpt::with_error(
            robust_cross_product_fpt(
                dif(site1.y(), site2.y()),
                dif(site1.x(), site2.x()),
                dif(site3.y1(), site3.y0()),
                dif(site3.x1(), site3.x0()),
            ),
            to_fpt.call(&1.0),
        );
        let inv_segm_len = RobustFpt::with_error(
            to_fpt.call(&1.0) / get_sqrt(&(line_a * line_a + line_b * line_b)),
            to_fpt.call(&3.0),
        );
        let mut t = RobustDif::new();
        if OrientationTest::eval_value(&denom) == Orientation::Collinear {
            t += teta / (RobustFpt::new(to_fpt.call(&8.0)) * a);
            t -= a / (RobustFpt::new(to_fpt.call(&2.0)) * teta);
        } else {
            let det = ((teta * teta + denom * denom) * a * b).sqrt();
            if segment_index == 2 {
                t -= det / (denom * denom);
            } else {
                t += det / (denom * denom);
            }
            t += teta * (a + b) / (RobustFpt::new(to_fpt.call(&2.0)) * denom * denom);
        }
        let mut c_x = RobustDif::new();
        let mut c_y = RobustDif::new();
        c_x +=
            RobustFpt::new(to_fpt.call(&0.5) * (to_fpt.call(&site1.x()) + to_fpt.call(&site2.x())));
        c_x += RobustFpt::new(vec_x) * t;
        c_y +=
            RobustFpt::new(to_fpt.call(&0.5) * (to_fpt.call(&site1.y()) + to_fpt.call(&site2.y())));
        c_y += RobustFpt::new(vec_y) * t;
        let mut r = RobustDif::new();
        let mut lower_x = c_x;
        r -= RobustFpt::new(line_a) * RobustFpt::new(f64::from(site3.x0()));
        r -= RobustFpt::new(line_b) * RobustFpt::new(f64::from(site3.y0()));
        r += RobustFpt::new(line_a) * c_x;
        r += RobustFpt::new(line_b) * c_y;
        if r.pos().fpv() < r.neg_sum().fpv() {
            r = -r;
        }
        lower_x += r * inv_segm_len;
        *c_event = CircleEvent::new(c_x.dif().fpv(), c_y.dif().fpv(), lower_x.dif().fpv());
        let recompute_c_x = c_x.dif().re() > ulps();
        let recompute_c_y = c_y.dif().re() > ulps();
        let recompute_lower_x = lower_x.dif().re() > ulps();
        if recompute_c_x || recompute_c_y || recompute_lower_x {
            self.exact_circle_formation_functor.pps(
                site1,
                site2,
                site3,
                segment_index,
                c_event,
                recompute_c_x,
                recompute_c_y,
                recompute_lower_x,
            );
        }
    }

    /// `pss(site1, site2, site3, point_index, c_event)`: a point site and two segment sites.
    #[allow(clippy::too_many_lines)] // one C++ function, translated statement by statement
    pub fn pss(
        &mut self,
        site1: &SiteEvent,
        site2: &SiteEvent,
        site3: &SiteEvent,
        point_index: i32,
        c_event: &mut CircleEvent,
    ) {
        let to_fpt = self.to_fpt;
        let segm_start1 = *site2.point1();
        let segm_end1 = *site2.point0();
        let segm_start2 = *site3.point0();
        let segm_end2 = *site3.point1();
        let a1 = to_fpt.call(&segm_end1.x()) - to_fpt.call(&segm_start1.x());
        let b1 = to_fpt.call(&segm_end1.y()) - to_fpt.call(&segm_start1.y());
        let a2 = to_fpt.call(&segm_end2.x()) - to_fpt.call(&segm_start2.x());
        let b2 = to_fpt.call(&segm_end2.y()) - to_fpt.call(&segm_start2.y());
        let recompute_c_x;
        let recompute_c_y;
        let recompute_lower_x;
        let orientation = RobustFpt::with_error(
            robust_cross_product_fpt(
                dif(segm_end1.y(), segm_start1.y()),
                dif(segm_end1.x(), segm_start1.x()),
                dif(segm_end2.y(), segm_start2.y()),
                dif(segm_end2.x(), segm_start2.x()),
            ),
            to_fpt.call(&1.0),
        );
        if OrientationTest::eval_value(&orientation) == Orientation::Collinear {
            let a = RobustFpt::with_error(a1 * a1 + b1 * b1, to_fpt.call(&2.0));
            let c = RobustFpt::with_error(
                robust_cross_product_fpt(
                    dif(segm_end1.y(), segm_start1.y()),
                    dif(segm_end1.x(), segm_start1.x()),
                    dif(segm_start2.y(), segm_start1.y()),
                    dif(segm_start2.x(), segm_start1.x()),
                ),
                to_fpt.call(&1.0),
            );
            let det = RobustFpt::with_error(
                robust_cross_product_fpt(
                    dif(segm_end1.x(), segm_start1.x()),
                    dif(segm_end1.y(), segm_start1.y()),
                    dif(site1.x(), segm_start1.x()),
                    dif(site1.y(), segm_start1.y()),
                ) * robust_cross_product_fpt(
                    dif(segm_end1.y(), segm_start1.y()),
                    dif(segm_end1.x(), segm_start1.x()),
                    dif(site1.y(), segm_start2.y()),
                    dif(site1.x(), segm_start2.x()),
                ),
                to_fpt.call(&3.0),
            );
            let mut t = RobustDif::new();
            t -= RobustFpt::new(a1)
                * RobustFpt::new(
                    (to_fpt.call(&segm_start1.x()) + to_fpt.call(&segm_start2.x()))
                        * to_fpt.call(&0.5)
                        - to_fpt.call(&site1.x()),
                );
            t -= RobustFpt::new(b1)
                * RobustFpt::new(
                    (to_fpt.call(&segm_start1.y()) + to_fpt.call(&segm_start2.y()))
                        * to_fpt.call(&0.5)
                        - to_fpt.call(&site1.y()),
                );
            if point_index == 2 {
                t += det.sqrt();
            } else {
                t -= det.sqrt();
            }
            t /= a;
            let mut c_x = RobustDif::new();
            let mut c_y = RobustDif::new();
            c_x += RobustFpt::new(
                to_fpt.call(&0.5) * (to_fpt.call(&segm_start1.x()) + to_fpt.call(&segm_start2.x())),
            );
            c_x += RobustFpt::new(a1) * t;
            c_y += RobustFpt::new(
                to_fpt.call(&0.5) * (to_fpt.call(&segm_start1.y()) + to_fpt.call(&segm_start2.y())),
            );
            c_y += RobustFpt::new(b1) * t;
            let mut lower_x = c_x;
            if is_neg(&c) {
                lower_x -= RobustFpt::new(to_fpt.call(&0.5)) * c / a.sqrt();
            } else {
                lower_x += RobustFpt::new(to_fpt.call(&0.5)) * c / a.sqrt();
            }
            recompute_c_x = c_x.dif().re() > ulps();
            recompute_c_y = c_y.dif().re() > ulps();
            recompute_lower_x = lower_x.dif().re() > ulps();
            *c_event = CircleEvent::new(c_x.dif().fpv(), c_y.dif().fpv(), lower_x.dif().fpv());
        } else {
            let sqr_sum1 = RobustFpt::with_error(get_sqrt(&(a1 * a1 + b1 * b1)), to_fpt.call(&2.0));
            let sqr_sum2 = RobustFpt::with_error(get_sqrt(&(a2 * a2 + b2 * b2)), to_fpt.call(&2.0));
            let mut a = RobustFpt::with_error(
                robust_cross_product_fpt(
                    dif(segm_end1.x(), segm_start1.x()),
                    dif(segm_end1.y(), segm_start1.y()),
                    dif(segm_start2.y(), segm_end2.y()),
                    dif(segm_end2.x(), segm_start2.x()),
                ),
                to_fpt.call(&1.0),
            );
            if !is_neg(&a) {
                a += sqr_sum1 * sqr_sum2;
            } else {
                a = (orientation * orientation) / (sqr_sum1 * sqr_sum2 - a);
            }
            let or1 = RobustFpt::with_error(
                robust_cross_product_fpt(
                    dif(segm_end1.y(), segm_start1.y()),
                    dif(segm_end1.x(), segm_start1.x()),
                    dif(segm_end1.y(), site1.y()),
                    dif(segm_end1.x(), site1.x()),
                ),
                to_fpt.call(&1.0),
            );
            let or2 = RobustFpt::with_error(
                robust_cross_product_fpt(
                    dif(segm_end2.x(), segm_start2.x()),
                    dif(segm_end2.y(), segm_start2.y()),
                    dif(segm_end2.x(), site1.x()),
                    dif(segm_end2.y(), site1.y()),
                ),
                to_fpt.call(&1.0),
            );
            let det = RobustFpt::new(to_fpt.call(&2.0)) * a * or1 * or2;
            let c1 = RobustFpt::with_error(
                robust_cross_product_fpt(
                    dif(segm_end1.y(), segm_start1.y()),
                    dif(segm_end1.x(), segm_start1.x()),
                    i64::from(segm_end1.y()),
                    i64::from(segm_end1.x()),
                ),
                to_fpt.call(&1.0),
            );
            let c2 = RobustFpt::with_error(
                robust_cross_product_fpt(
                    dif(segm_end2.x(), segm_start2.x()),
                    dif(segm_end2.y(), segm_start2.y()),
                    i64::from(segm_end2.x()),
                    i64::from(segm_end2.y()),
                ),
                to_fpt.call(&1.0),
            );
            let inv_orientation = RobustFpt::new(to_fpt.call(&1.0)) / orientation;
            let mut t = RobustDif::new();
            let mut b = RobustDif::new();
            let mut ix = RobustDif::new();
            let mut iy = RobustDif::new();
            ix += RobustFpt::new(a2) * c1 * inv_orientation;
            ix += RobustFpt::new(a1) * c2 * inv_orientation;
            iy += RobustFpt::new(b1) * c2 * inv_orientation;
            iy += RobustFpt::new(b2) * c1 * inv_orientation;

            b += ix * (RobustFpt::new(a1) * sqr_sum2);
            b += ix * (RobustFpt::new(a2) * sqr_sum1);
            b += iy * (RobustFpt::new(b1) * sqr_sum2);
            b += iy * (RobustFpt::new(b2) * sqr_sum1);
            b -= sqr_sum1
                * RobustFpt::with_error(
                    robust_cross_product_fpt(
                        dif(segm_end2.x(), segm_start2.x()),
                        dif(segm_end2.y(), segm_start2.y()),
                        i64::from(site1.y().wrapping_neg()),
                        i64::from(site1.x()),
                    ),
                    to_fpt.call(&1.0),
                );
            b -= sqr_sum2
                * RobustFpt::with_error(
                    robust_cross_product_fpt(
                        dif(segm_end1.x(), segm_start1.x()),
                        dif(segm_end1.y(), segm_start1.y()),
                        i64::from(site1.y().wrapping_neg()),
                        i64::from(site1.x()),
                    ),
                    to_fpt.call(&1.0),
                );
            t -= b;
            if point_index == 2 {
                t += det.sqrt();
            } else {
                t -= det.sqrt();
            }
            t /= a * a;
            let mut c_x = ix;
            let mut c_y = iy;
            c_x += t * (RobustFpt::new(a1) * sqr_sum2);
            c_x += t * (RobustFpt::new(a2) * sqr_sum1);
            c_y += t * (RobustFpt::new(b1) * sqr_sum2);
            c_y += t * (RobustFpt::new(b2) * sqr_sum1);
            if t.pos().fpv() < t.neg_sum().fpv() {
                t = -t;
            }
            let mut lower_x = c_x;
            if is_neg(&orientation) {
                lower_x -= t * orientation;
            } else {
                lower_x += t * orientation;
            }
            recompute_c_x = c_x.dif().re() > ulps();
            recompute_c_y = c_y.dif().re() > ulps();
            recompute_lower_x = lower_x.dif().re() > ulps();
            *c_event = CircleEvent::new(c_x.dif().fpv(), c_y.dif().fpv(), lower_x.dif().fpv());
        }
        if recompute_c_x || recompute_c_y || recompute_lower_x {
            self.exact_circle_formation_functor.pss(
                site1,
                site2,
                site3,
                point_index,
                c_event,
                recompute_c_x,
                recompute_c_y,
                recompute_lower_x,
            );
        }
    }

    /// `sss(site1, site2, site3, c_event)`: three segment sites.
    pub fn sss(
        &mut self,
        site1: &SiteEvent,
        site2: &SiteEvent,
        site3: &SiteEvent,
        c_event: &mut CircleEvent,
    ) {
        let to_fpt = self.to_fpt;
        let a1 = RobustFpt::new(to_fpt.call(&site1.x1()) - to_fpt.call(&site1.x0()));
        let b1 = RobustFpt::new(to_fpt.call(&site1.y1()) - to_fpt.call(&site1.y0()));
        let c1 = RobustFpt::with_error(
            robust_cross_product_fpt(
                i64::from(site1.x0()),
                i64::from(site1.y0()),
                i64::from(site1.x1()),
                i64::from(site1.y1()),
            ),
            to_fpt.call(&1.0),
        );

        let a2 = RobustFpt::new(to_fpt.call(&site2.x1()) - to_fpt.call(&site2.x0()));
        let b2 = RobustFpt::new(to_fpt.call(&site2.y1()) - to_fpt.call(&site2.y0()));
        let c2 = RobustFpt::with_error(
            robust_cross_product_fpt(
                i64::from(site2.x0()),
                i64::from(site2.y0()),
                i64::from(site2.x1()),
                i64::from(site2.y1()),
            ),
            to_fpt.call(&1.0),
        );

        let a3 = RobustFpt::new(to_fpt.call(&site3.x1()) - to_fpt.call(&site3.x0()));
        let b3 = RobustFpt::new(to_fpt.call(&site3.y1()) - to_fpt.call(&site3.y0()));
        let c3 = RobustFpt::with_error(
            robust_cross_product_fpt(
                i64::from(site3.x0()),
                i64::from(site3.y0()),
                i64::from(site3.x1()),
                i64::from(site3.y1()),
            ),
            to_fpt.call(&1.0),
        );

        let len1 = (a1 * a1 + b1 * b1).sqrt();
        let len2 = (a2 * a2 + b2 * b2).sqrt();
        let len3 = (a3 * a3 + b3 * b3).sqrt();
        let cross_12 = RobustFpt::with_error(
            robust_cross_product_fpt(
                dif(site1.x1(), site1.x0()),
                dif(site1.y1(), site1.y0()),
                dif(site2.x1(), site2.x0()),
                dif(site2.y1(), site2.y0()),
            ),
            to_fpt.call(&1.0),
        );
        let cross_23 = RobustFpt::with_error(
            robust_cross_product_fpt(
                dif(site2.x1(), site2.x0()),
                dif(site2.y1(), site2.y0()),
                dif(site3.x1(), site3.x0()),
                dif(site3.y1(), site3.y0()),
            ),
            to_fpt.call(&1.0),
        );
        let cross_31 = RobustFpt::with_error(
            robust_cross_product_fpt(
                dif(site3.x1(), site3.x0()),
                dif(site3.y1(), site3.y0()),
                dif(site1.x1(), site1.x0()),
                dif(site1.y1(), site1.y0()),
            ),
            to_fpt.call(&1.0),
        );

        // denom = cross_12 * len3 + cross_23 * len1 + cross_31 * len2.
        let mut denom = RobustDif::new();
        denom += cross_12 * len3;
        denom += cross_23 * len1;
        denom += cross_31 * len2;

        // denom * r = (b2 * c_x - a2 * c_y - c2 * denom) / len2.
        let mut r = RobustDif::new();
        r -= cross_12 * c3;
        r -= cross_23 * c1;
        r -= cross_31 * c2;

        let mut c_x = RobustDif::new();
        c_x += a1 * c2 * len3;
        c_x -= a2 * c1 * len3;
        c_x += a2 * c3 * len1;
        c_x -= a3 * c2 * len1;
        c_x += a3 * c1 * len2;
        c_x -= a1 * c3 * len2;

        let mut c_y = RobustDif::new();
        c_y += b1 * c2 * len3;
        c_y -= b2 * c1 * len3;
        c_y += b2 * c3 * len1;
        c_y -= b3 * c2 * len1;
        c_y += b3 * c1 * len2;
        c_y -= b1 * c3 * len2;

        let lower_x = c_x + r;

        let denom_dif = denom.dif();
        let c_x_dif = c_x.dif() / denom_dif;
        let c_y_dif = c_y.dif() / denom_dif;
        let lower_x_dif = lower_x.dif() / denom_dif;

        let recompute_c_x = c_x_dif.re() > ulps();
        let recompute_c_y = c_y_dif.re() > ulps();
        let recompute_lower_x = lower_x_dif.re() > ulps();
        *c_event = CircleEvent::new(c_x_dif.fpv(), c_y_dif.fpv(), lower_x_dif.fpv());
        if recompute_c_x || recompute_c_y || recompute_lower_x {
            self.exact_circle_formation_functor.sss(
                site1,
                site2,
                site3,
                c_event,
                recompute_c_x,
                recompute_c_y,
                recompute_lower_x,
            );
        }
    }
}

/// `robust_cross_product(a1_, b1_, a2_, b2_)` of module `predicates`, the `int_x2_type`
/// arguments as the C++ passes them.
fn robust_cross_product_fpt(a1: i64, b1: i64, a2: i64, b2: i64) -> f64 {
    crate::predicates::robust_cross_product(a1, b1, a2, b2)
}

// ---------------------------------------------------------------- circle_formation_predicate (1433-1527)

/// The circle existence predicate `circle_formation_predicate` asks first (its template parameter
/// `CEP`, `circle_existence_predicate<site_event<int32>>` by default).
pub trait CircleExistence {
    /// `ppp(site1, site2, site3)`.
    fn ppp(&self, site1: &SiteEvent, site2: &SiteEvent, site3: &SiteEvent) -> bool;
    /// `pps(site1, site2, site3, segment_index)`.
    fn pps(
        &self,
        site1: &SiteEvent,
        site2: &SiteEvent,
        site3: &SiteEvent,
        segment_index: i32,
    ) -> bool;
    /// `pss(site1, site2, site3, point_index)`.
    fn pss(
        &self,
        site1: &SiteEvent,
        site2: &SiteEvent,
        site3: &SiteEvent,
        point_index: i32,
    ) -> bool;
    /// `sss(site1, site2, site3)`.
    fn sss(&self, site1: &SiteEvent, site2: &SiteEvent, site3: &SiteEvent) -> bool;
}

impl CircleExistence for CircleExistencePredicate {
    fn ppp(&self, site1: &SiteEvent, site2: &SiteEvent, site3: &SiteEvent) -> bool {
        CircleExistencePredicate::ppp(self, site1, site2, site3)
    }
    fn pps(
        &self,
        site1: &SiteEvent,
        site2: &SiteEvent,
        site3: &SiteEvent,
        segment_index: i32,
    ) -> bool {
        CircleExistencePredicate::pps(self, site1, site2, site3, segment_index)
    }
    fn pss(
        &self,
        site1: &SiteEvent,
        site2: &SiteEvent,
        site3: &SiteEvent,
        point_index: i32,
    ) -> bool {
        CircleExistencePredicate::pss(self, site1, site2, site3, point_index)
    }
    fn sss(&self, site1: &SiteEvent, site2: &SiteEvent, site3: &SiteEvent) -> bool {
        CircleExistencePredicate::sss(self, site1, site2, site3)
    }
}

/// The circle formation functor `circle_formation_predicate` computes the circle with (its
/// template parameter `CFF`, `lazy_circle_formation_functor` by default).
pub trait CircleFormationFunctor {
    /// `ppp(site1, site2, site3, c_event)`.
    fn ppp(
        &mut self,
        site1: &SiteEvent,
        site2: &SiteEvent,
        site3: &SiteEvent,
        c_event: &mut CircleEvent,
    );
    /// `pps(site1, site2, site3, segment_index, c_event)`.
    fn pps(
        &mut self,
        site1: &SiteEvent,
        site2: &SiteEvent,
        site3: &SiteEvent,
        segment_index: i32,
        c_event: &mut CircleEvent,
    );
    /// `pss(site1, site2, site3, point_index, c_event)`.
    fn pss(
        &mut self,
        site1: &SiteEvent,
        site2: &SiteEvent,
        site3: &SiteEvent,
        point_index: i32,
        c_event: &mut CircleEvent,
    );
    /// `sss(site1, site2, site3, c_event)`.
    fn sss(
        &mut self,
        site1: &SiteEvent,
        site2: &SiteEvent,
        site3: &SiteEvent,
        c_event: &mut CircleEvent,
    );
}

impl CircleFormationFunctor for LazyCircleFormationFunctor {
    fn ppp(
        &mut self,
        site1: &SiteEvent,
        site2: &SiteEvent,
        site3: &SiteEvent,
        c_event: &mut CircleEvent,
    ) {
        LazyCircleFormationFunctor::ppp(self, site1, site2, site3, c_event);
    }
    fn pps(
        &mut self,
        site1: &SiteEvent,
        site2: &SiteEvent,
        site3: &SiteEvent,
        segment_index: i32,
        c_event: &mut CircleEvent,
    ) {
        LazyCircleFormationFunctor::pps(self, site1, site2, site3, segment_index, c_event);
    }
    fn pss(
        &mut self,
        site1: &SiteEvent,
        site2: &SiteEvent,
        site3: &SiteEvent,
        point_index: i32,
        c_event: &mut CircleEvent,
    ) {
        LazyCircleFormationFunctor::pss(self, site1, site2, site3, point_index, c_event);
    }
    fn sss(
        &mut self,
        site1: &SiteEvent,
        site2: &SiteEvent,
        site3: &SiteEvent,
        c_event: &mut CircleEvent,
    ) {
        LazyCircleFormationFunctor::sss(self, site1, site2, site3, c_event);
    }
}

/// `circle_formation_predicate<site_event<int32>, circle_event<double>, CEP, CFF>`: whether three
/// consecutive arcs of the beach line make a circle event, and the circle when they do.
#[derive(Clone, Copy, Debug, Default)]
pub struct CircleFormationPredicate<
    CEP = CircleExistencePredicate,
    CFF = LazyCircleFormationFunctor,
> {
    to_fpt: TypeConverterFpt,
    ulp_cmp: UlpComparison,
    circle_existence_predicate: CEP,
    circle_formation_functor: CFF,
}

impl<CEP: CircleExistence, CFF: CircleFormationFunctor> CircleFormationPredicate<CEP, CFF> {
    /// The predicate with its existence predicate and formation functor (the C++ default
    /// constructs both; [`Default`] does that here).
    pub fn new(circle_existence_predicate: CEP, circle_formation_functor: CFF) -> Self {
        Self {
            to_fpt: TypeConverterFpt,
            ulp_cmp: UlpComparison,
            circle_existence_predicate,
            circle_formation_functor,
        }
    }

    /// `operator()(site1, site2, site3, circle)`: creates a circle event from the three sites;
    /// true if it exists, with the circle in `circle`, else false.
    pub fn call(
        &mut self,
        site1: &SiteEvent,
        site2: &SiteEvent,
        site3: &SiteEvent,
        circle: &mut CircleEvent,
    ) -> bool {
        if !site1.is_segment() {
            if !site2.is_segment() {
                if !site3.is_segment() {
                    // (point, point, point) sites.
                    if !self.circle_existence_predicate.ppp(site1, site2, site3) {
                        return false;
                    }
                    self.circle_formation_functor
                        .ppp(site1, site2, site3, circle);
                } else {
                    // (point, point, segment) sites.
                    if !self.circle_existence_predicate.pps(site1, site2, site3, 3) {
                        return false;
                    }
                    self.circle_formation_functor
                        .pps(site1, site2, site3, 3, circle);
                }
            } else if !site3.is_segment() {
                // (point, segment, point) sites.
                if !self.circle_existence_predicate.pps(site1, site3, site2, 2) {
                    return false;
                }
                self.circle_formation_functor
                    .pps(site1, site3, site2, 2, circle);
            } else {
                // (point, segment, segment) sites.
                if !self.circle_existence_predicate.pss(site1, site2, site3, 1) {
                    return false;
                }
                self.circle_formation_functor
                    .pss(site1, site2, site3, 1, circle);
            }
        } else if !site2.is_segment() {
            if !site3.is_segment() {
                // (segment, point, point) sites.
                if !self.circle_existence_predicate.pps(site2, site3, site1, 1) {
                    return false;
                }
                self.circle_formation_functor
                    .pps(site2, site3, site1, 1, circle);
            } else {
                // (segment, point, segment) sites.
                if !self.circle_existence_predicate.pss(site2, site1, site3, 2) {
                    return false;
                }
                self.circle_formation_functor
                    .pss(site2, site1, site3, 2, circle);
            }
        } else if !site3.is_segment() {
            // (segment, segment, point) sites.
            if !self.circle_existence_predicate.pss(site3, site1, site2, 3) {
                return false;
            }
            self.circle_formation_functor
                .pss(site3, site1, site2, 3, circle);
        } else {
            // (segment, segment, segment) sites.
            if !self.circle_existence_predicate.sss(site1, site2, site3) {
                return false;
            }
            self.circle_formation_functor
                .sss(site1, site2, site3, circle);
        }
        if self.lies_outside_vertical_segment(circle, site1)
            || self.lies_outside_vertical_segment(circle, site2)
            || self.lies_outside_vertical_segment(circle, site3)
        {
            return false;
        }
        true
    }

    /// `lies_outside_vertical_segment(c, s)`: the circle's centre is below or above a vertical
    /// segment site by more than `ULPS`.
    fn lies_outside_vertical_segment(&self, c: &CircleEvent, s: &SiteEvent) -> bool {
        if !s.is_segment() || !is_vertical(s) {
            return false;
        }
        let y0 = self
            .to_fpt
            .call(&if s.is_inverse() { s.y1() } else { s.y0() });
        let y1 = self
            .to_fpt
            .call(&if s.is_inverse() { s.y0() } else { s.y1() });
        self.ulp_cmp.call(c.y(), y0, ULPS) == UlpResult::Less
            || self.ulp_cmp.call(c.y(), y1, ULPS) == UlpResult::More
    }
}

#[cfg(test)]
mod tests {
    use super::{CircleFormationPredicate, LazyCircleFormationFunctor};
    use crate::structures::{CircleEvent, SiteEvent};

    type DefaultPredicate = CircleFormationPredicate;

    #[test]
    fn three_points_make_their_circumcircle() {
        // (0, 0), (2, 2), (4, 0): the circle of centre (2, 0) and radius 2, which fires at x 4.
        let s = [
            SiteEvent::from_xy(0, 0),
            SiteEvent::from_xy(2, 2),
            SiteEvent::from_xy(4, 0),
        ];
        let mut c = CircleEvent::default();
        LazyCircleFormationFunctor::default().ppp(&s[0], &s[1], &s[2], &mut c);
        assert_eq!((c.x(), c.y(), c.lower_x()), (2.0, 0.0, 4.0));
        let mut p = DefaultPredicate::default();
        let mut d = CircleEvent::default();
        // The three points turn right in the order (0, 0), (2, 2), (4, 0).
        assert!(p.call(&s[0], &s[1], &s[2], &mut d));
        assert!(!p.call(&s[2], &s[1], &s[0], &mut d));
    }
}
