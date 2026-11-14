// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Boost code: Boost.Polygon of Boost 1.84.0, translated on 2026-11-13 by orca-o3-voronoi-1
// from boost/polygon/detail/voronoi_predicates.hpp, lines 1-524 (voronoi_predicates' is_vertical,
// robust_cross_product, orientation_test, point_comparison_predicate,
// event_comparison_predicate, distance_predicate, node_comparison_predicate and
// circle_existence_predicate). Copyright Andrii Sydorchuk 2010-2012, under the Boost Software
// License, version 1.0 (BSL-1.0; licenses/BSL-1.0.txt).

//! The first part of Boost.Polygon's `voronoi_predicates<voronoi_ctype_traits<int32>>`
//! (`voronoi_predicates.hpp` 1-524): the orientation test, the event and node orders of the sweep
//! line, the distance predicate the beach line's comparisons rest on, and the circle existence
//! tests. Module `formation` (voronoi-2) carries the circle formation functors and predicate
//! (lines 525-1533).
//!
//! - [`robust_cross_product`] is `a1·b2 − b1·a2` of four 64-bit integers (each a difference of two
//!   32-bit coordinates) with a relative error of 1 EPS: the magnitudes' products in `uint64`, then
//!   their sum or difference by the signs, converted to `double`. A sum past 2^64 wraps, as the
//!   C++'s `uint64` does.
//! - [`OrientationTest`] is `orientation_test`: the sign of a value or of that cross product.
//! - [`EventComparisonPredicate`] orders the sweep line's events: sites by x, then points before
//!   segments and segments by y and by the orientation of their far points; sites against circles
//!   by `ulp_comparison` within 64 ulps (`ULPS`) of the circle's lowest x; circles by lowest x,
//!   then centre y.
//! - [`DistancePredicate`] decides whether a horizontal line through a new point meets the right
//!   arc first, with a fast test for point-segment pairs and distances in `double` otherwise;
//!   [`NodeComparisonPredicate`] orders the beach line's nodes with it.
//! - [`CircleExistencePredicate`] is the cheap test that three sites can form a circle at all.
//!
//! The C++ predicates are templates over the point, site, circle and beach-line node types, which
//! `voronoi_structures.hpp` defines and voronoi-2 carries: here they are generic over the traits
//! [`VoronoiPoint`], [`VoronoiSite`], [`VoronoiCircle`] and [`BeachLineNodeKey`], which name what
//! the predicates read of `point_2d<int32>`, `site_event<int32>`, `circle_event<double>` and
//! `beach_line_node_key<site_event<int32>>`, so that voronoi-2's types implement them. The
//! coordinate type is `int32` throughout, the only one OrcaSlicer instantiates. Each overload of
//! a predicate's `operator()` is a method of its own.
//!
//! Nothing of these lines is left out.

use crate::ctypes::{Sign, TypeConverterFpt, UlpComparison, UlpResult, get_sqrt};

// ---------------------------------------------------------------- what the predicates read

/// What the predicates read of `point_2d<int32>`: its coordinates, and `operator!=` through
/// [`PartialEq`] (`point_2d`'s `!=` is `x != x || y != y`, which is `!(a == b)`).
pub trait VoronoiPoint: PartialEq {
    /// `x()`.
    fn x(&self) -> i32;
    /// `y()`.
    fn y(&self) -> i32;
}

/// What the predicates read of `site_event<int32>`.
pub trait VoronoiSite {
    /// `point_type`, `point_2d<int32>`.
    type Point: VoronoiPoint;
    /// `x()`: `point0_.x()`.
    fn x(&self) -> i32;
    /// `y()`: `point0_.y()`.
    fn y(&self) -> i32;
    /// `x0()`: `point0_.x()`.
    fn x0(&self) -> i32;
    /// `y0()`: `point0_.y()`.
    fn y0(&self) -> i32;
    /// `x1()`: `point1_.x()`.
    fn x1(&self) -> i32;
    /// `y1()`: `point1_.y()`.
    fn y1(&self) -> i32;
    /// `point0()`.
    fn point0(&self) -> &Self::Point;
    /// `point1()`.
    fn point1(&self) -> &Self::Point;
    /// `sorted_index()`.
    fn sorted_index(&self) -> usize;
    /// `is_inverse()`.
    fn is_inverse(&self) -> bool;
    /// `is_segment()`: the two points differ.
    fn is_segment(&self) -> bool;
}

/// What the predicates read of `circle_event<double>`.
pub trait VoronoiCircle {
    /// `lower_x()`: the x of the circle's lowest point (its rightmost, on the sweep line).
    fn lower_x(&self) -> f64;
    /// `y()`: the centre's y.
    fn y(&self) -> f64;
}

/// What the node comparison reads of `beach_line_node_key<site_event<int32>>`.
pub trait BeachLineNodeKey {
    /// `site_type`.
    type Site: VoronoiSite;
    /// `left_site()`.
    fn left_site(&self) -> &Self::Site;
    /// `right_site()`.
    fn right_site(&self) -> &Self::Site;
}

// ---------------------------------------------------------------- voronoi_predicates (23-118)

/// `ULPS`: the ulps within which a site and a circle event are not ordered.
pub const ULPS: u32 = 64;
/// `ULPSx2`.
pub const ULPS_X2: u32 = 128;

/// `is_vertical(point1, point2)`: the same x.
pub fn is_vertical_points<P: VoronoiPoint>(point1: &P, point2: &P) -> bool {
    point1.x() == point2.x()
}

/// `is_vertical(site)`: the site's two points have the same x.
pub fn is_vertical<S: VoronoiSite>(site: &S) -> bool {
    is_vertical_points(site.point0(), site.point1())
}

/// `robust_cross_product(a1_, b1_, a2_, b2_)`: `a1 * b2 - b1 * a2` with an epsilon relative error
/// of 1 EPS.
#[must_use]
pub fn robust_cross_product(a1_: i64, b1_: i64, a2_: i64, b2_: i64) -> f64 {
    let to_fpt = TypeConverterFpt;
    let a1 = (if a1_.is_neg() {
        a1_.wrapping_neg()
    } else {
        a1_
    }) as u64;
    let b1 = (if b1_.is_neg() {
        b1_.wrapping_neg()
    } else {
        b1_
    }) as u64;
    let a2 = (if a2_.is_neg() {
        a2_.wrapping_neg()
    } else {
        a2_
    }) as u64;
    let b2 = (if b2_.is_neg() {
        b2_.wrapping_neg()
    } else {
        b2_
    }) as u64;

    let l = a1.wrapping_mul(b2);
    let r = b1.wrapping_mul(a2);

    if a1_.is_neg() ^ b2_.is_neg() {
        if a2_.is_neg() ^ b1_.is_neg() {
            if l > r {
                -to_fpt.call(&(l - r))
            } else {
                to_fpt.call(&(r - l))
            }
        } else {
            -to_fpt.call(&l.wrapping_add(r))
        }
    } else if a2_.is_neg() ^ b1_.is_neg() {
        to_fpt.call(&l.wrapping_add(r))
    } else if l < r {
        -to_fpt.call(&(r - l))
    } else {
        to_fpt.call(&(l - r))
    }
}

/// `orientation_test::Orientation`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(i32)]
pub enum Orientation {
    /// `RIGHT = -1`.
    Right = -1,
    /// `COLLINEAR = 0`.
    Collinear = 0,
    /// `LEFT = 1`.
    Left = 1,
}

/// `orientation_test` (`ot`): the orientation from the sign of a determinant.
#[derive(Clone, Copy, Default, Debug)]
pub struct OrientationTest;

impl OrientationTest {
    /// `eval(T value)`: collinear for zero, else right for a negative value and left for a
    /// positive one.
    pub fn eval_value<T: Sign + ?Sized>(value: &T) -> Orientation {
        if value.is_zero() {
            return Orientation::Collinear;
        }
        if value.is_neg() {
            Orientation::Right
        } else {
            Orientation::Left
        }
    }

    /// `eval(dif_x1_, dif_y1_, dif_x2_, dif_y2_)`: the sign of their robust cross product.
    #[must_use]
    pub fn eval_dif(dif_x1_: i64, dif_y1_: i64, dif_x2_: i64, dif_y2_: i64) -> Orientation {
        Self::eval_value(&robust_cross_product(dif_x1_, dif_y1_, dif_x2_, dif_y2_))
    }

    /// `eval(point1, point2, point3)`: the orientation of the turn point1, point2, point3.
    pub fn eval_points<P: VoronoiPoint>(point1: &P, point2: &P, point3: &P) -> Orientation {
        let dx1 = i64::from(point1.x()) - i64::from(point2.x());
        let dx2 = i64::from(point2.x()) - i64::from(point3.x());
        let dy1 = i64::from(point1.y()) - i64::from(point2.y());
        let dy2 = i64::from(point2.y()) - i64::from(point3.y());
        Self::eval_value(&robust_cross_product(dx1, dy1, dx2, dy2))
    }
}

// ---------------------------------------------------------------- point_comparison_predicate (121-131)

/// `point_comparison_predicate<point_2d<int32>>`: by x, then by y.
#[derive(Clone, Copy, Default, Debug)]
pub struct PointComparisonPredicate;

impl PointComparisonPredicate {
    /// `operator()(lhs, rhs)`.
    pub fn call<P: VoronoiPoint>(&self, lhs: &P, rhs: &P) -> bool {
        if lhs.x() == rhs.x() {
            return lhs.y() < rhs.y();
        }
        lhs.x() < rhs.x()
    }
}

// ---------------------------------------------------------------- event_comparison_predicate (133-184)

/// `event_comparison_predicate<site_event<int32>, circle_event<double>>`: the order of the sweep
/// line's events.
#[derive(Clone, Copy, Default, Debug)]
pub struct EventComparisonPredicate {
    ulp_cmp: UlpComparison,
    to_fpt: TypeConverterFpt,
}

impl EventComparisonPredicate {
    /// `operator()(const site_type&, const site_type&)`.
    pub fn site_site<S: VoronoiSite>(&self, lhs: &S, rhs: &S) -> bool {
        if lhs.x0() != rhs.x0() {
            return lhs.x0() < rhs.x0();
        }
        if !lhs.is_segment() {
            if !rhs.is_segment() {
                return lhs.y0() < rhs.y0();
            }
            if is_vertical(rhs) {
                return lhs.y0() <= rhs.y0();
            }
            true
        } else {
            if is_vertical(rhs) {
                if is_vertical(lhs) {
                    return lhs.y0() < rhs.y0();
                }
                return false;
            }
            if is_vertical(lhs) {
                return true;
            }
            if lhs.y0() != rhs.y0() {
                return lhs.y0() < rhs.y0();
            }
            OrientationTest::eval_points(lhs.point1(), lhs.point0(), rhs.point1())
                == Orientation::Left
        }
    }

    /// `operator()(const site_type&, const circle_type&)`: the site's x less than the circle's
    /// lowest x by more than `ULPS`.
    pub fn site_circle<S: VoronoiSite, C: VoronoiCircle>(&self, lhs: &S, rhs: &C) -> bool {
        let x_cmp = self.ulp_cmp.call(
            self.to_fpt.call(&lhs.x0()),
            self.to_fpt.call(&rhs.lower_x()),
            ULPS,
        );
        x_cmp == UlpResult::Less
    }

    /// `operator()(const circle_type&, const site_type&)`.
    pub fn circle_site<C: VoronoiCircle, S: VoronoiSite>(&self, lhs: &C, rhs: &S) -> bool {
        let x_cmp = self.ulp_cmp.call(
            self.to_fpt.call(&lhs.lower_x()),
            self.to_fpt.call(&rhs.x0()),
            ULPS,
        );
        x_cmp == UlpResult::Less
    }

    /// `operator()(const circle_type&, const circle_type&)`: by lowest x, then by centre y.
    pub fn circle_circle<C: VoronoiCircle>(&self, lhs: &C, rhs: &C) -> bool {
        if lhs.lower_x() != rhs.lower_x() {
            return lhs.lower_x() < rhs.lower_x();
        }
        lhs.y() < rhs.y()
    }
}

// ---------------------------------------------------------------- distance_predicate (186-371)

/// `distance_predicate::kPredicateResult`: the answer of an epsilon-robust test.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(i32)]
enum KPredicateResult {
    Less = -1,
    Undefined = 0,
    More = 1,
}

/// `distance_predicate<site_event<int32>>`.
#[derive(Clone, Copy, Default, Debug)]
pub struct DistancePredicate {
    ulp_cmp: UlpComparison,
    to_fpt: TypeConverterFpt,
}

impl DistancePredicate {
    /// `operator()(left_site, right_site, new_point)`: true if a horizontal line through the new
    /// point meets the right arc first; false also when it goes through the arcs' intersection.
    pub fn call<S: VoronoiSite>(
        &self,
        left_site: &S,
        right_site: &S,
        new_point: &S::Point,
    ) -> bool {
        if !left_site.is_segment() {
            if !right_site.is_segment() {
                self.pp(left_site, right_site, new_point)
            } else {
                self.ps(left_site, right_site, new_point, false)
            }
        } else if !right_site.is_segment() {
            self.ps(right_site, left_site, new_point, true)
        } else {
            self.ss(left_site, right_site, new_point)
        }
    }

    /// `pp`: two point sites.
    fn pp<S: VoronoiSite>(&self, left_site: &S, right_site: &S, new_point: &S::Point) -> bool {
        let left_point = left_site.point0();
        let right_point = right_site.point0();
        if left_point.x() > right_point.x() {
            if new_point.y() <= left_point.y() {
                return false;
            }
        } else if left_point.x() < right_point.x() {
            if new_point.y() >= right_point.y() {
                return true;
            }
        } else {
            return i64::from(left_point.y()) + i64::from(right_point.y())
                < i64::from(new_point.y()) * 2;
        }

        let dist1 = self.find_distance_to_point_arc(left_site, new_point);
        let dist2 = self.find_distance_to_point_arc(right_site, new_point);

        // The undefined ulp range is equal to 3EPS + 3EPS <= 6ULP.
        dist1 < dist2
    }

    /// `ps`: a point site and a segment site.
    fn ps<S: VoronoiSite>(
        &self,
        left_site: &S,
        right_site: &S,
        new_point: &S::Point,
        reverse_order: bool,
    ) -> bool {
        let fast_res = self.fast_ps(left_site, right_site, new_point, reverse_order);
        if fast_res != KPredicateResult::Undefined {
            return fast_res == KPredicateResult::Less;
        }

        let dist1 = self.find_distance_to_point_arc(left_site, new_point);
        let dist2 = self.find_distance_to_segment_arc(right_site, new_point);

        // The undefined ulp range is equal to 3EPS + 7EPS <= 10ULP.
        reverse_order ^ (dist1 < dist2)
    }

    /// `ss`: two segment sites.
    fn ss<S: VoronoiSite>(&self, left_site: &S, right_site: &S, new_point: &S::Point) -> bool {
        // Handle temporary segment sites.
        if left_site.sorted_index() == right_site.sorted_index() {
            return OrientationTest::eval_points(left_site.point0(), left_site.point1(), new_point)
                == Orientation::Left;
        }

        let dist1 = self.find_distance_to_segment_arc(left_site, new_point);
        let dist2 = self.find_distance_to_segment_arc(right_site, new_point);

        // The undefined ulp range is equal to 7EPS + 7EPS <= 14ULP.
        dist1 < dist2
    }

    /// `find_distance_to_point_arc`: relative error at most 3EPS.
    fn find_distance_to_point_arc<S: VoronoiSite>(&self, site: &S, point: &S::Point) -> f64 {
        let dx = self.to_fpt.call(&site.x()) - self.to_fpt.call(&point.x());
        let dy = self.to_fpt.call(&site.y()) - self.to_fpt.call(&point.y());
        (dx * dx + dy * dy) / (self.to_fpt.call(&2.0) * dx)
    }

    /// `find_distance_to_segment_arc`: relative error at most 7EPS.
    fn find_distance_to_segment_arc<S: VoronoiSite>(&self, site: &S, point: &S::Point) -> f64 {
        if is_vertical(site) {
            (self.to_fpt.call(&site.x()) - self.to_fpt.call(&point.x())) * self.to_fpt.call(&0.5)
        } else {
            let segment0 = site.point0();
            let segment1 = site.point1();
            let a1 = self.to_fpt.call(&segment1.x()) - self.to_fpt.call(&segment0.x());
            let b1 = self.to_fpt.call(&segment1.y()) - self.to_fpt.call(&segment0.y());
            let mut k = get_sqrt(&(a1 * a1 + b1 * b1));
            // Avoid subtraction while computing k.
            if !b1.is_neg() {
                k = self.to_fpt.call(&1.0) / (b1 + k);
            } else {
                k = (k - b1) / (a1 * a1);
            }
            k * robust_cross_product(
                i64::from(segment1.x()) - i64::from(segment0.x()),
                i64::from(segment1.y()) - i64::from(segment0.y()),
                i64::from(point.x()) - i64::from(segment0.x()),
                i64::from(point.y()) - i64::from(segment0.y()),
            )
        }
    }

    /// `fast_ps`: the epsilon-robust test of a point site against a segment site.
    fn fast_ps<S: VoronoiSite>(
        &self,
        left_site: &S,
        right_site: &S,
        new_point: &S::Point,
        reverse_order: bool,
    ) -> KPredicateResult {
        let site_point = left_site.point0();
        let segment_start = right_site.point0();
        let segment_end = right_site.point1();

        if OrientationTest::eval_points(segment_start, segment_end, new_point) != Orientation::Right
        {
            return if !right_site.is_inverse() {
                KPredicateResult::Less
            } else {
                KPredicateResult::More
            };
        }

        let dif_x = self.to_fpt.call(&new_point.x()) - self.to_fpt.call(&site_point.x());
        let dif_y = self.to_fpt.call(&new_point.y()) - self.to_fpt.call(&site_point.y());
        let a = self.to_fpt.call(&segment_end.x()) - self.to_fpt.call(&segment_start.x());
        let b = self.to_fpt.call(&segment_end.y()) - self.to_fpt.call(&segment_start.y());

        if is_vertical(right_site) {
            if new_point.y() < site_point.y() && !reverse_order {
                return KPredicateResult::More;
            } else if new_point.y() > site_point.y() && reverse_order {
                return KPredicateResult::Less;
            }
            return KPredicateResult::Undefined;
        } else {
            let orientation = OrientationTest::eval_dif(
                i64::from(segment_end.x()) - i64::from(segment_start.x()),
                i64::from(segment_end.y()) - i64::from(segment_start.y()),
                i64::from(new_point.x()) - i64::from(site_point.x()),
                i64::from(new_point.y()) - i64::from(site_point.y()),
            );
            if orientation == Orientation::Left {
                if !right_site.is_inverse() {
                    return if reverse_order {
                        KPredicateResult::Less
                    } else {
                        KPredicateResult::Undefined
                    };
                }
                return if reverse_order {
                    KPredicateResult::Undefined
                } else {
                    KPredicateResult::More
                };
            }
        }

        let fast_left_expr = a * (dif_y + dif_x) * (dif_y - dif_x);
        let fast_right_expr = (self.to_fpt.call(&2.0) * b) * dif_x * dif_y;
        let expr_cmp = self.ulp_cmp.call(fast_left_expr, fast_right_expr, 4);
        if expr_cmp != UlpResult::Equal {
            if (expr_cmp == UlpResult::More) ^ reverse_order {
                return if reverse_order {
                    KPredicateResult::Less
                } else {
                    KPredicateResult::More
                };
            }
            return KPredicateResult::Undefined;
        }
        KPredicateResult::Undefined
    }
}

// ---------------------------------------------------------------- node_comparison_predicate (373-458)

/// `node_comparison_predicate<beach_line_node_key<site_event<int32>>>`: the order of the beach
/// line's nodes, by the y of their arcs' intersection on the sweep line. It is only called while
/// a new site is processed, so one of the two nodes always lies on the sweep line.
#[derive(Clone, Copy, Default, Debug)]
pub struct NodeComparisonPredicate {
    point_comparison: PointComparisonPredicate,
    distance_predicate: DistancePredicate,
}

impl NodeComparisonPredicate {
    /// `operator()(node1, node2)`.
    pub fn call<N: BeachLineNodeKey>(&self, node1: &N, node2: &N) -> bool {
        // Get x coordinate of the rightmost site from both nodes.
        let site1 = self.get_comparison_site(node1);
        let site2 = self.get_comparison_site(node2);
        let point1 = self.get_comparison_point(site1);
        let point2 = self.get_comparison_point(site2);

        if point1.x() < point2.x() {
            // The second node contains a new site.
            self.distance_predicate
                .call(node1.left_site(), node1.right_site(), point2)
        } else if point1.x() > point2.x() {
            // The first node contains a new site.
            !self
                .distance_predicate
                .call(node2.left_site(), node2.right_site(), point1)
        } else {
            // These checks were evaluated experimentally.
            if site1.sorted_index() == site2.sorted_index() {
                // Both nodes are new (inserted during same site event processing).
                self.get_comparison_y(node1, true) < self.get_comparison_y(node2, true)
            } else if site1.sorted_index() < site2.sorted_index() {
                let y1 = self.get_comparison_y(node1, false);
                let y2 = self.get_comparison_y(node2, true);
                if y1.0 != y2.0 {
                    return y1.0 < y2.0;
                }
                if !site1.is_segment() { y1.1 < 0 } else { false }
            } else {
                let y1 = self.get_comparison_y(node1, true);
                let y2 = self.get_comparison_y(node2, false);
                if y1.0 != y2.0 {
                    return y1.0 < y2.0;
                }
                if !site2.is_segment() { y2.1 > 0 } else { true }
            }
        }
    }

    /// `get_comparison_site`: the newer site.
    fn get_comparison_site<'a, N: BeachLineNodeKey>(&self, node: &'a N) -> &'a N::Site {
        if node.left_site().sorted_index() > node.right_site().sorted_index() {
            return node.left_site();
        }
        node.right_site()
    }

    /// `get_comparison_point`: the smaller of the site's points.
    fn get_comparison_point<'a, S: VoronoiSite>(&self, site: &'a S) -> &'a S::Point {
        if self.point_comparison.call(site.point0(), site.point1()) {
            site.point0()
        } else {
            site.point1()
        }
    }

    /// `get_comparison_y(node, is_new_node = true)`: the y and the direction of the newer site.
    fn get_comparison_y<N: BeachLineNodeKey>(&self, node: &N, is_new_node: bool) -> (i32, i32) {
        if node.left_site().sorted_index() == node.right_site().sorted_index() {
            return (node.left_site().y0(), 0);
        }
        if node.left_site().sorted_index() > node.right_site().sorted_index() {
            if !is_new_node && node.left_site().is_segment() && is_vertical(node.left_site()) {
                return (node.left_site().y0(), 1);
            }
            return (node.left_site().y1(), 1);
        }
        (node.right_site().y0(), -1)
    }
}

// ---------------------------------------------------------------- circle_existence_predicate (460-524)

/// `circle_existence_predicate<site_event<int32>>`: whether three sites can form a circle event.
#[derive(Clone, Copy, Default, Debug)]
pub struct CircleExistencePredicate;

impl CircleExistencePredicate {
    /// `ppp`: three point sites turn right.
    pub fn ppp<S: VoronoiSite>(&self, site1: &S, site2: &S, site3: &S) -> bool {
        OrientationTest::eval_points(site1.point0(), site2.point0(), site3.point0())
            == Orientation::Right
    }

    /// `pps`: two point sites and a segment site at `segment_index` (1 to 3).
    pub fn pps<S: VoronoiSite>(&self, site1: &S, site2: &S, site3: &S, segment_index: i32) -> bool {
        if segment_index != 2 {
            let orient1 =
                OrientationTest::eval_points(site1.point0(), site2.point0(), site3.point0());
            let orient2 =
                OrientationTest::eval_points(site1.point0(), site2.point0(), site3.point1());
            if segment_index == 1 && site1.x0() >= site2.x0() {
                if orient1 != Orientation::Right {
                    return false;
                }
            } else if segment_index == 3 && site2.x0() >= site1.x0() {
                if orient2 != Orientation::Right {
                    return false;
                }
            } else if orient1 != Orientation::Right && orient2 != Orientation::Right {
                return false;
            }
        } else {
            return (site3.point0() != site1.point0()) || (site3.point1() != site2.point0());
        }
        true
    }

    /// `pss`: a point site and two segment sites, the point at `point_index` (1 to 3).
    pub fn pss<S: VoronoiSite>(&self, site1: &S, site2: &S, site3: &S, point_index: i32) -> bool {
        if site2.sorted_index() == site3.sorted_index() {
            return false;
        }
        if point_index == 2 {
            if !site2.is_inverse() && site3.is_inverse() {
                return false;
            }
            if site2.is_inverse() == site3.is_inverse()
                && OrientationTest::eval_points(site2.point0(), site1.point0(), site3.point1())
                    != Orientation::Right
            {
                return false;
            }
        }
        true
    }

    /// `sss`: three segment sites, two of them not the same segment.
    pub fn sss<S: VoronoiSite>(&self, site1: &S, site2: &S, site3: &S) -> bool {
        (site1.sorted_index() != site2.sorted_index())
            && (site2.sorted_index() != site3.sorted_index())
    }
}
