// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/AABBTreeLines.hpp lines 1-372 whole, and class Linef with
// the double instances of line_alg::distance_to_squared and line_alg::intersection of
// src/libslic3r/Line.hpp (lines 43-148, 251-267) of OrcaSlicer v2.4.2, translated on 2026-11-13 by
// index-1. Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and the
// PrusaSlicer authors (Prusa Research), under the GNU Affero General Public License, version 3
// (AGPL-3.0; LICENSE).

//! `AABBTreeLines`: an AABB tree over line segments ([`build_aabb_tree_over_indexed_lines`]) and
//! its queries: the nearest line, the lines within a distance, the inside test of closed contours
//! and the intersections with a segment; [`LinesDistancer`] holds the lines with their tree.
//!
//! - A line's centroid is `(a + b) * 0.5`: for `Line` that is `Point`'s free `operator*`, the
//!   integer sum (wrapping) as a `double` times 0.5, truncated toward zero; for `Linef` plain
//!   `double`s ([`IndexedLine::centroid`]). Its box is `AlignedBox(a, a).extend(b)`, with no
//!   epsilon.
//! - The nearest-line search is AABBTreeIndirect's recursion
//!   ([`crate::aabb_tree::squared_distance_to_indexed_primitives_recursive`]). For `Line` the tree
//!   is over `coord_t`, so the point is truncated to `coord_t` for each line's distance and for the
//!   boxes' `contains`, and a box's exterior distance is computed in `double` and truncated to
//!   `coord_t` coordinate by coordinate (orca-index's `AlignedBox`); the limit defaults to
//!   infinity, and when nothing is nearer than the limit the search returns the limit and leaves
//!   the out-parameters as they were.
//! - [`point_outside_closed_contours`] counts the crossings of a ray along x from the point, each
//!   way (a crossing at the point itself answers 0); when the two counts disagree in parity it
//!   tries a ray along y, and answers 0 if those disagree too.
//! - [`get_intersections_with_line`] collects, left before right in the tree, each line that
//!   `line_alg::intersection` finds crossing the query line (the point on the query line, cast to
//!   the line's scalar); the sorted form orders them by the squared distance from the query
//!   line's start with MSVC's `std::sort` (`orca_stl::sort_by`), so tied points fall as the C++'s.
//!
//! [`Linef`] belongs in orca-geom, beside `Line`; no piece carries it there, so index-1 translates
//! it here with the two `line_alg` functions its tree reads. `Line`'s are orca-geom's.

use orca_eigen::{Cast, Real, Scalar, Vec2d, Vector};
use orca_geom::line::{Line, distance_to_squared_nearest};
use orca_geom::point::{EPSILON, Point, cross2};

use crate::aabb_tree::{
    IndexedPrimitivesDistancer, NPOS, SourceNode, Tree,
    indexed_primitives_within_distance_squared_recursive,
    squared_distance_to_indexed_primitives_recursive,
};
use crate::aligned_box::{AlignedBox, BoxScalar, ExteriorDistance};

/// A line type the tree is built over (`LineType`, with `line_alg::Traits`): its ends, its
/// centroid and the `line_alg` distance to it.
pub trait IndexedLine<const N: usize> {
    /// `LineType::Scalar`.
    type Scalar: BoxScalar + Cast<Self::Floating> + ExteriorDistance<Self::Floating>;
    /// `std::conditional<std::is_floating_point<Scalar>::value, Scalar, double>`.
    type Floating: Real + BoxScalar + Cast<Self::Scalar>;
    /// `line.a`.
    fn a(&self) -> Vector<Self::Scalar, N>;
    /// `line.b`.
    fn b(&self) -> Vector<Self::Scalar, N>;
    /// `(line.a + line.b) * 0.5`, as the line type computes it.
    fn centroid(&self) -> Vector<Self::Scalar, N>;
    /// `line_alg::distance_to_squared(line, point, &nearest_point)`: the squared distance and the
    /// nearest point.
    fn distance_to_squared(
        &self,
        point: &Vector<Self::Scalar, N>,
    ) -> (f64, Vector<Self::Scalar, N>);
}

/// A 2D line type with `line_alg::intersection`.
pub trait IntersectingLine: IndexedLine<2> {
    /// `line_alg::intersection(self, other, &pt)`: the crossing of the two segments, a point on
    /// `self`, or `None` (parallel within `EPSILON`, or past an end).
    fn intersection(&self, other: &Self) -> Option<Vector<Self::Scalar, 2>>;
}

/// An intersection of `get_intersections_with_line`: the point and the index of the line it is on
/// (the C++'s `std::pair<VectorType, size_t>`).
pub type LineHit<L> = (Vector<<L as IndexedLine<2>>::Scalar, 2>, usize);

fn point_vec(p: Point) -> Vector<i64, 2> {
    Vector([p.x, p.y])
}

fn vec_point(v: &Vector<i64, 2>) -> Point {
    Point::new(v[0], v[1])
}

impl IndexedLine<2> for Line {
    type Scalar = i64;
    type Floating = f64;
    fn a(&self) -> Vector<i64, 2> {
        point_vec(self.a)
    }
    fn b(&self) -> Vector<i64, 2> {
        point_vec(self.b)
    }
    /// `(line.a + line.b) * 0.5`: `Point`'s free `operator*(const Point&, const double&)`,
    /// `coord_t(x * 0.5)`.
    fn centroid(&self) -> Vector<i64, 2> {
        point_vec((self.a + self.b).mul_trunc(0.5))
    }
    fn distance_to_squared(&self, point: &Vector<i64, 2>) -> (f64, Vector<i64, 2>) {
        let (d, nearest) = distance_to_squared_nearest(self.a, self.b, vec_point(point));
        (d, point_vec(nearest))
    }
}

impl IntersectingLine for Line {
    fn intersection(&self, other: &Self) -> Option<Vector<i64, 2>> {
        Line::intersection(self, other).map(point_vec)
    }
}

/// `Linef`: a segment of `double` points (`Line.hpp`; it belongs in orca-geom).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Linef {
    /// `a`.
    pub a: Vec2d,
    /// `b`.
    pub b: Vec2d,
}

impl Default for Linef {
    /// `Linef()`: both ends at the origin.
    fn default() -> Self {
        Self {
            a: Vec2d::zero(),
            b: Vec2d::zero(),
        }
    }
}

impl Linef {
    /// `Linef(a, b)`.
    #[must_use]
    pub const fn new(a: Vec2d, b: Vec2d) -> Self {
        Self { a, b }
    }
}

impl IndexedLine<2> for Linef {
    type Scalar = f64;
    type Floating = f64;
    fn a(&self) -> Vec2d {
        self.a
    }
    fn b(&self) -> Vec2d {
        self.b
    }
    fn centroid(&self) -> Vec2d {
        (self.a + self.b) * 0.5
    }
    /// `line_alg::distance_to_squared` of a `Linef`, in `double`.
    fn distance_to_squared(&self, point: &Vec2d) -> (f64, Vec2d) {
        let (a, b, point) = (self.a, self.b, *point);
        let v = b - a;
        let va = point - a;
        // Avoid a sqrt.
        let l2 = v.squared_norm();
        if l2 == 0.0 {
            // The a == b case.
            return (va.squared_norm(), a);
        }
        // Consider the line extending the segment, parameterized as a + t (b - a). The projection
        // of the point onto it falls where t = [(point - a) . (b - a)] / |b - a|^2.
        let t = va.dot(&v) / l2;
        if t <= 0.0 {
            // Beyond the 'a' end of the segment.
            return (va.squared_norm(), a);
        } else if t >= 1.0 {
            // Beyond the 'b' end of the segment.
            return ((point - b).squared_norm(), b);
        }
        let nearest = a.zip(&v, |ai, vi| ai + t * vi);
        let tv_va = v.zip(&va, |vi, vai| t * vi - vai);
        (tv_va.squared_norm(), nearest)
    }
}

impl IntersectingLine for Linef {
    /// `line_alg::intersection` of two `Linef`s, in `double`.
    fn intersection(&self, other: &Self) -> Option<Vec2d> {
        let v1 = self.b - self.a;
        let v2 = other.b - other.a;
        let denom = cross2(v1, v2);
        if orca_crt::fabs(denom) < EPSILON {
            return None;
        }
        let v12 = self.a - other.a;
        let nume_a = cross2(v2, v12);
        let nume_b = cross2(v1, v12);
        let t1 = nume_a / denom;
        let t2 = nume_b / denom;
        if (0.0..=1.0).contains(&t1) && (0.0..=1.0).contains(&t2) {
            // Get the intersection point.
            return Some(self.a.zip(&v1, |ai, vi| ai + t1 * vi));
        }
        // Not intersecting.
        None
    }
}

/// `detail::IndexedLinesDistancer`: each line's nearest point, the point cast to the line's
/// scalar.
pub struct IndexedLinesDistancer<'a, L: IndexedLine<N>, R, const N: usize> {
    /// `lines`.
    pub lines: &'a [L],
    /// `tree`.
    pub tree: &'a Tree<L::Scalar, N>,
    /// `origin`.
    pub origin: Vector<R, N>,
}

impl<L, R, const N: usize> IndexedPrimitivesDistancer<L::Scalar, R, N>
    for IndexedLinesDistancer<'_, L, R, N>
where
    L: IndexedLine<N>,
    L::Scalar: Cast<R>,
    R: Real + Cast<L::Scalar>,
{
    fn tree(&self) -> &Tree<L::Scalar, N> {
        self.tree
    }
    fn origin(&self) -> &Vector<R, N> {
        &self.origin
    }
    fn closest_point_to_origin(&self, primitive_index: usize) -> (Vector<R, N>, R) {
        let line = &self.lines[primitive_index];
        let (squared_distance, nearest_point) = line.distance_to_squared(&self.origin.cast());
        (nearest_point.cast(), R::from_f64(squared_distance))
    }
}

/// `build_aabb_tree_over_indexed_lines(lines)`: a tree over the lines, balanced on their
/// centroids, each box the line's own (no epsilon).
#[must_use]
pub fn build_aabb_tree_over_indexed_lines<L: IndexedLine<N>, const N: usize>(
    lines: &[L],
) -> Tree<L::Scalar, N> {
    struct InputType<S, const N: usize> {
        idx: usize,
        bbox: AlignedBox<S, N>,
        centroid: Vector<S, N>,
    }
    impl<S: BoxScalar, const N: usize> SourceNode<S, N> for InputType<S, N> {
        fn idx(&self) -> usize {
            self.idx
        }
        fn bbox(&self) -> AlignedBox<S, N> {
            self.bbox
        }
        fn centroid(&self) -> Vector<S, N> {
            self.centroid
        }
    }

    let mut input = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        let a = line.a();
        let mut bbox = AlignedBox::new(a, a);
        bbox.extend_point(&line.b());
        input.push(InputType {
            idx: i,
            bbox,
            centroid: line.centroid(),
        });
    }
    let mut out = Tree::new();
    out.build(input);
    out
}

/// `squared_distance_to_indexed_lines(lines, tree, point, hit_idx_out, hit_point_out,
/// max_sqr_dist)`: the squared distance to the nearest line, -1 for an empty tree, `max_sqr_dist`
/// when no line is nearer (the C++'s default limit is infinity); the line and its nearest point go
/// to the out-parameters only when one is found.
pub fn squared_distance_to_indexed_lines<L, R, const N: usize>(
    lines: &[L],
    tree: &Tree<L::Scalar, N>,
    point: &Vector<R, N>,
    hit_idx_out: &mut usize,
    hit_point_out: &mut Vector<R, N>,
    max_sqr_dist: R,
) -> R
where
    L: IndexedLine<N>,
    L::Scalar: Cast<R> + ExteriorDistance<R>,
    R: Real + Cast<L::Scalar>,
{
    if tree.is_empty() {
        return R::from_f64(-1.0);
    }
    let distancer = IndexedLinesDistancer {
        lines,
        tree,
        origin: *point,
    };
    squared_distance_to_indexed_primitives_recursive(
        &distancer,
        0,
        R::ZERO,
        max_sqr_dist,
        hit_idx_out,
        hit_point_out,
    )
}

/// `all_lines_in_radius(lines, tree, point, max_distance_squared)`: the lines nearer than the
/// limit, in the tree's order.
#[must_use]
pub fn all_lines_in_radius<L, R, const N: usize>(
    lines: &[L],
    tree: &Tree<L::Scalar, N>,
    point: &Vector<R, N>,
    max_distance_squared: R,
) -> Vec<usize>
where
    L: IndexedLine<N>,
    L::Scalar: Cast<R> + ExteriorDistance<R>,
    R: Real + Cast<L::Scalar>,
{
    let distancer = IndexedLinesDistancer {
        lines,
        tree,
        origin: *point,
    };
    if tree.is_empty() {
        return Vec::new();
    }
    let mut found_lines = Vec::new();
    indexed_primitives_within_distance_squared_recursive(
        &distancer,
        0,
        max_distance_squared,
        &mut found_lines,
    );
    found_lines
}

/// `detail::coordinate_aligned_ray_hit_count<..., coordinate>`: the crossings of the ray from
/// `ray_origin` along `COORD`, `(positive way, negative way)`, or `(-1, -1)` when the origin lies
/// on a line. A line counts when the origin's other coordinate is in `[min, max)` of its ends.
fn coordinate_aligned_ray_hit_count<L: IndexedLine<N>, const N: usize, const COORD: usize>(
    node_idx: usize,
    tree: &Tree<L::Scalar, N>,
    lines: &[L],
    ray_origin: &Vector<L::Scalar, N>,
) -> (i32, i32) {
    let other = (COORD + 1) % 2;
    let node = tree.node(node_idx);
    if node.is_leaf() {
        let line = &lines[node.idx];
        let (a, b) = (line.a(), line.b());
        if ray_origin[other] < orca_stl::min(a[other], b[other])
            || ray_origin[other] >= orca_stl::max(a[other], b[other])
        {
            // The second inequality is not sharp for a reason: without it, the contour's border
            // may be counted twice where lines meet exactly at the crossing.
            return (0, 0);
        }
        let line_max = orca_stl::max(a[COORD], b[COORD]);
        let line_min = orca_stl::min(a[COORD], b[COORD]);
        if ray_origin[COORD] > line_max {
            (1, 0)
        } else if ray_origin[COORD] < line_min {
            (0, 1)
        } else {
            // The crossing of the ray with the line: t = (origin[oc] - a[oc]) / (b[oc] - a[oc]),
            // then val_c = a[c] + t * (b[c] - a[c]). The differences are in the line's scalar.
            let floating = |s: L::Scalar| -> L::Floating { s.cast() };
            let distance_oc = floating(b[other].sub(a[other]));
            let t = floating(ray_origin[other].sub(a[other])).div(distance_oc);
            let val_c = floating(a[COORD]).add(t.mul(floating(b[COORD].sub(a[COORD]))));
            let origin_c = floating(ray_origin[COORD]);
            if origin_c > val_c {
                (1, 0)
            } else if origin_c < val_c {
                (0, 1)
            } else {
                // The ray origin is on the boundary.
                (-1, -1)
            }
        }
    } else {
        let mut intersections_above = 0i32;
        let mut intersections_below = 0i32;
        let left_node_idx = node_idx * 2 + 1;
        let right_node_idx = left_node_idx + 1;
        for child in [left_node_idx, right_node_idx] {
            let bbox = &tree.node(child).bbox;
            if bbox.min[other] <= ray_origin[other] && bbox.max[other] >= ray_origin[other] {
                let (above, below) =
                    coordinate_aligned_ray_hit_count::<L, N, COORD>(child, tree, lines, ray_origin);
                if above < 0 || below < 0 {
                    return (-1, -1);
                }
                intersections_above = intersections_above.wrapping_add(above);
                intersections_below = intersections_below.wrapping_add(below);
            }
        }
        (intersections_above, intersections_below)
    }
}

/// `point_outside_closed_contours(lines, tree, point)`: 1 outside, -1 inside, 0 on a contour or
/// when it cannot tell; 1 for an empty tree.
#[must_use]
pub fn point_outside_closed_contours<L: IndexedLine<N>, const N: usize>(
    lines: &[L],
    tree: &Tree<L::Scalar, N>,
    point: &Vector<L::Scalar, N>,
) -> i32 {
    if tree.is_empty() {
        return 1;
    }
    let (hits_above, hits_below) =
        coordinate_aligned_ray_hit_count::<L, N, 0>(0, tree, lines, point);
    if hits_above < 0 || hits_below < 0 {
        0
    } else if hits_above % 2 == 1 && hits_below % 2 == 1 {
        -1
    } else if hits_above % 2 == 0 && hits_below % 2 == 0 {
        1
    } else {
        // This should not happen with closed contours: check in the Y direction.
        let (hits_above, hits_below) =
            coordinate_aligned_ray_hit_count::<L, N, 1>(0, tree, lines, point);
        if hits_above < 0 || hits_below < 0 {
            0
        } else if hits_above % 2 == 1 && hits_below % 2 == 1 {
            -1
        } else if hits_above % 2 == 0 && hits_below % 2 == 0 {
            1
        } else {
            // Both results were unclear.
            0
        }
    }
}

/// `detail::get_intersections_with_line`: the crossings in the subtree, left before right, a
/// child skipped when its box does not meet the query line's.
fn intersections_recursive<L: IntersectingLine>(
    node_idx: usize,
    tree: &Tree<L::Scalar, 2>,
    lines: &[L],
    line: &L,
    line_bb: &AlignedBox<L::Scalar, 2>,
) -> Vec<LineHit<L>> {
    let node = tree.node(node_idx);
    if node.is_leaf() {
        match line.intersection(&lines[node.idx]) {
            Some(pt) => vec![(pt, node.idx)],
            None => Vec::new(),
        }
    } else {
        let left_node_idx = node_idx * 2 + 1;
        let right_node_idx = left_node_idx + 1;
        let mut result = Vec::new();
        if tree.node(left_node_idx).bbox.intersects(line_bb) {
            result.extend(intersections_recursive(
                left_node_idx,
                tree,
                lines,
                line,
                line_bb,
            ));
        }
        if tree.node(right_node_idx).bbox.intersects(line_bb) {
            result.extend(intersections_recursive(
                right_node_idx,
                tree,
                lines,
                line,
                line_bb,
            ));
        }
        result
    }
}

/// `get_intersections_with_line<sorted>(lines, tree, line)`: the crossings of `line` with the
/// tree's lines, each the point and the line's index; when `SORTED`, ordered by the squared
/// distance (in `Floating`) from `line.a` with `std::sort`.
#[must_use]
pub fn get_intersections_with_line<const SORTED: bool, L: IntersectingLine>(
    lines: &[L],
    tree: &Tree<L::Scalar, 2>,
    line: &L,
) -> Vec<LineHit<L>> {
    if tree.is_empty() {
        return Vec::new();
    }
    let a = line.a();
    let mut line_bb = AlignedBox::new(a, a);
    line_bb.extend_point(&line.b());

    let mut intersections = intersections_recursive(0, tree, lines, line, &line_bb);
    if SORTED {
        let mut points_with_sq_distance: Vec<(L::Floating, LineHit<L>)> =
            Vec::with_capacity(intersections.len());
        for p in &intersections {
            let d: Vector<L::Floating, 2> = (p.0 - a).cast();
            points_with_sq_distance.push((d.squared_norm(), *p));
        }
        orca_stl::sort_by(&mut points_with_sq_distance, |left, right| left.0 < right.0);
        for (i, p) in points_with_sq_distance.iter().enumerate() {
            intersections[i] = p.1;
        }
    }
    intersections
}

/// `LinesDistancer<LineType>`: the lines, copied, with the tree built over them.
#[derive(Clone, Debug)]
pub struct LinesDistancer<L: IndexedLine<N>, const N: usize> {
    lines: Vec<L>,
    tree: Tree<L::Scalar, N>,
}

impl<L: IndexedLine<N>, const N: usize> Default for LinesDistancer<L, N> {
    /// `LinesDistancer()`: no lines, an empty tree.
    fn default() -> Self {
        Self {
            lines: Vec::new(),
            tree: Tree::new(),
        }
    }
}

impl<L: IndexedLine<N>, const N: usize> LinesDistancer<L, N> {
    /// `LinesDistancer(std::vector<LineType> &&lines)`.
    #[must_use]
    pub fn new(lines: Vec<L>) -> Self {
        let tree = build_aabb_tree_over_indexed_lines(&lines);
        Self { lines, tree }
    }

    /// `LinesDistancer(const std::vector<LineType> &lines)`.
    #[must_use]
    pub fn from_slice(lines: &[L]) -> Self
    where
        L: Clone,
    {
        Self::new(lines.to_vec())
    }

    /// `outside(point)`: 1 outside, -1 inside, 0 when it cannot tell.
    #[must_use]
    pub fn outside(&self, point: &Vector<L::Scalar, N>) -> i32 {
        point_outside_closed_contours(&self.lines, &self.tree, point)
    }

    /// `distance_from_lines_extra<SIGNED_DISTANCE>(point)`: the distance to the nearest line
    /// (infinity when there is none; negative inside when signed, 0 when `outside` cannot tell),
    /// the line's index ([`NPOS`] when none) and the nearest point (zero when none).
    #[must_use]
    pub fn distance_from_lines_extra<const SIGNED_DISTANCE: bool>(
        &self,
        point: &Vector<L::Scalar, N>,
    ) -> (L::Floating, usize, Vector<L::Floating, N>) {
        let mut nearest_line_index_out = NPOS;
        let mut nearest_point_out = Vector::<L::Floating, N>::zero();
        let p: Vector<L::Floating, N> = point.cast();
        let mut distance = squared_distance_to_indexed_lines(
            &self.lines,
            &self.tree,
            &p,
            &mut nearest_line_index_out,
            &mut nearest_point_out,
            L::Floating::from_f64(f64::INFINITY),
        );
        if distance < L::Floating::ZERO {
            return (
                L::Floating::from_f64(f64::INFINITY),
                nearest_line_index_out,
                nearest_point_out,
            );
        }
        distance = distance.sqrt();
        if SIGNED_DISTANCE {
            distance = distance.mul(L::Floating::from_f64(f64::from(self.outside(point))));
        }
        (distance, nearest_line_index_out, nearest_point_out)
    }

    /// `distance_from_lines<SIGNED_DISTANCE>(point)`.
    #[must_use]
    pub fn distance_from_lines<const SIGNED_DISTANCE: bool>(
        &self,
        point: &Vector<L::Scalar, N>,
    ) -> L::Floating {
        self.distance_from_lines_extra::<SIGNED_DISTANCE>(point).0
    }

    /// `all_lines_in_radius(point, radius)`: the lines nearer than `radius`.
    #[must_use]
    pub fn all_lines_in_radius(
        &self,
        point: &Vector<L::Scalar, N>,
        radius: L::Floating,
    ) -> Vec<usize> {
        all_lines_in_radius(&self.lines, &self.tree, &point.cast(), radius.mul(radius))
    }

    /// `get_line(line_idx)`.
    #[must_use]
    pub fn get_line(&self, line_idx: usize) -> &L {
        &self.lines[line_idx]
    }

    /// `get_lines()`.
    #[must_use]
    pub fn get_lines(&self) -> &[L] {
        &self.lines
    }
}

impl<L: IntersectingLine> LinesDistancer<L, 2> {
    /// `intersections_with_line<sorted>(line)`.
    #[must_use]
    pub fn intersections_with_line<const SORTED: bool>(&self, line: &L) -> Vec<LineHit<L>> {
        get_intersections_with_line::<SORTED, L>(&self.lines, &self.tree, line)
    }
}
