// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/AABBTreeIndirect.hpp lines 1-992 whole and
// next_highest_power_of_2 of src/libslic3r/Utils.hpp (lines 328-339) of OrcaSlicer v2.4.2,
// translated on 2026-11-13 by index-1. Copyright the OrcaSlicer authors, the BambuStudio authors
// (Bambu Lab) and the PrusaSlicer authors (Prusa Research; the tree's balancing and traversal were
// adapted there from libigl's AABB.cpp and AABB.hpp, copyright Alec Jacobson), under the GNU Affero
// General Public License, version 3 (AGPL-3.0; LICENSE).
//
// Carries libigl code: the struct igl::Hit of igl/Hit.h of libigl, as OrcaSlicer v2.4.2 carries
// it in deps_src/libigl (its five fields, as Hit), translated on 2026-11-13 by index-1. Its
// copyright is Alec Jacobson's (2015) and Christian Schüller's (2014), under the Mozilla Public
// License, version 2.0 (MPL-2.0; licenses/MPL-2.0.txt).
//
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0. If a copy of
// the MPL was not distributed with this file, You can obtain one at https://mozilla.org/MPL/2.0/.
// (Exhibit A of the MPL-2.0: the libigl code here, Hit, stays under it; the rest of the file is
// sketch-orca's, under the AGPL-3.0, as the MPL's section 3.3 allows.)

//! `AABBTreeIndirect`: a static balanced AABB tree over items held elsewhere, addressed by index,
//! with its ray, closest-point and box queries.
//!
//! - [`Tree`] holds every node of a full binary tree in one vector (`next_highest_power_of_2(n) *
//!   2 - 1` nodes); the children of node `i` are `2i + 1` and `2i + 2`. A leaf holds its item's
//!   index, an inner node [`INNER`], an unused node [`NPOS`] and an empty box. `build_recursive`
//!   splits the items at the median (`(left + right) / 2`) along the longest side of their boxes'
//!   union, by the C++'s own quickselect (`partition_input`, a median of three then a two-sided
//!   scan), so equal centroids end where it leaves them.
//! - [`BoundingBoxWrapper`] wraps a `Slic3r::BoundingBox` grown by `SCALED_EPSILON` on each side;
//!   its centroid is `min + max / 2` in integers, as the C++ writes it (not the box's middle).
//! - The ray queries ([`intersect_ray_first_hit`], [`intersect_ray_all_hits`]) test each node's
//!   box in the ray's scalar (Williams et al.'s test, with the inverse direction) and each
//!   triangle in `double` (Moeller and Trumbore's `intersect_triangle1`); the hits keep `float`
//!   barycentrics and parameter, and [`intersect_ray_all_hits`] sorts them by `t` with
//!   `orca_stl::sort_by`, MSVC's introsort, so tied hits fall as the C++'s do.
//! - The closest-point queries ([`squared_distance_to_indexed_triangle_set`],
//!   [`is_any_triangle_in_radius`], [`all_triangles_in_radius`]) run the generic recursion
//!   [`squared_distance_to_indexed_primitives_recursive`] over an [`IndexedPrimitivesDistancer`],
//!   which AABBTreeLines' distancer shares: a child whose box holds the point is looked at first,
//!   then the nearer child by `squaredExteriorDistance` (orca-index's [`AlignedBox`]).
//! - [`traverse`] walks with a predicate ([`intersecting`], [`within`] or the caller's) and stops
//!   when the callback returns false; [`get_candidate_idxs`] collects the leaves whose boxes hold
//!   a point.
//!
//! Left out: `detail::intersect_triangle_epsilon`, whose body calls `cwiseMax()` with no argument,
//! so it does not compile once instantiated, and nothing in the engine calls it. The
//! `intersect_triangle` overloads whose origin is not `double` do not compile either (their
//! `const V edge1` is a cast expression); every overload ends in the `double` one, which
//! [`intersect_triangle`] is.
//!
//! The C++ out-parameters that are written only on success become `Option`s
//! ([`intersect_ray_first_hit`]); the distance queries keep theirs, as the C++ leaves them
//! unchanged when no item is nearer than the limit.

use core::cmp::Ordering;

use orca_eigen::{Cast, Real, Vec3d, Vec3i32, Vector};

use crate::aligned_box::{AlignedBox, BoxScalar, ExteriorDistance};

/// `Tree::npos`: an unused node.
pub const NPOS: usize = usize::MAX;
/// `Tree::inner`: an inner node.
pub const INNER: usize = usize::MAX - 1;

/// `next_highest_power_of_2(uint64_t)` (`Utils.hpp`, which no crate carries yet; it belongs in
/// the crate that takes `Utils.hpp`): the smallest power of two at or above `v`, 1 for 0.
#[must_use]
pub fn next_highest_power_of_2(v: u64) -> u64 {
    // `if (v != 0) --v;`
    let mut v = v.saturating_sub(1);
    v |= v >> 1;
    v |= v >> 2;
    v |= v >> 4;
    v |= v >> 8;
    v |= v >> 16;
    v |= v >> 32;
    v.wrapping_add(1)
}

/// `Tree::Node`: the source item's index (or [`INNER`], or [`NPOS`]) and its box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Node<S, const N: usize> {
    /// `idx`.
    pub idx: usize,
    /// `bbox`.
    pub bbox: AlignedBox<S, N>,
}

impl<S: BoxScalar, const N: usize> Default for Node<S, N> {
    /// `Node()`: unused, with an empty box.
    fn default() -> Self {
        Self {
            idx: NPOS,
            bbox: AlignedBox::empty(),
        }
    }
}

impl<S, const N: usize> Node<S, N> {
    /// `is_valid()`: not [`NPOS`].
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.idx != NPOS
    }
    /// `is_inner()`.
    #[must_use]
    pub fn is_inner(&self) -> bool {
        self.idx == INNER
    }
    /// `is_leaf()`: not inner (an unused node counts as a leaf, as in the C++).
    #[must_use]
    pub fn is_leaf(&self) -> bool {
        !self.is_inner()
    }
}

/// What `Tree::build` reads of each item (the C++'s `SourceNode`): its index, its box and the
/// centroid the tree balances on.
pub trait SourceNode<S, const N: usize> {
    /// `idx()`.
    fn idx(&self) -> usize;
    /// `bbox()`.
    fn bbox(&self) -> AlignedBox<S, N>;
    /// `centroid()`.
    fn centroid(&self) -> Vector<S, N>;
}

/// `AABBTreeIndirect::Tree<N, S>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tree<S, const N: usize> {
    nodes: Vec<Node<S, N>>,
}

/// `Tree2f`.
pub type Tree2f = Tree<f32, 2>;
/// `Tree3f`.
pub type Tree3f = Tree<f32, 3>;
/// `Tree2d`.
pub type Tree2d = Tree<f64, 2>;
/// `Tree3d`.
pub type Tree3d = Tree<f64, 3>;

impl<S: BoxScalar, const N: usize> Tree<S, N> {
    /// An empty tree.
    #[must_use]
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    /// `clear()`.
    pub fn clear(&mut self) {
        self.nodes.clear();
    }

    /// `build(std::vector<SourceNode> &&input)`: builds over the items and drops them.
    pub fn build<I: SourceNode<S, N>>(&mut self, mut input: Vec<I>) {
        self.build_modify_input(&mut input);
        input.clear();
    }

    /// `build(const std::vector<SourceNode> &input)`: builds over a copy of the items.
    pub fn build_copy<I: SourceNode<S, N> + Clone>(&mut self, input: &[I]) {
        self.build(input.to_vec());
    }

    /// `build_modify_input(input)`: builds over the items, reordering them in place as the
    /// quickselect leaves them.
    pub fn build_modify_input<I: SourceNode<S, N>>(&mut self, input: &mut [I]) {
        if input.is_empty() {
            self.clear();
        } else {
            // Allocate enough memory for a full binary tree.
            let count = next_highest_power_of_2(input.len() as u64) as usize * 2 - 1;
            self.nodes.clear();
            self.nodes.resize(count, Node::default());
            let right = input.len() - 1;
            self.build_recursive(input, 0, 0, right);
        }
    }

    /// `nodes()`.
    #[must_use]
    pub fn nodes(&self) -> &[Node<S, N>] {
        &self.nodes
    }

    /// `node(idx)`.
    #[must_use]
    pub fn node(&self, idx: usize) -> &Node<S, N> {
        &self.nodes[idx]
    }

    /// `empty()`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// `left_child_idx(idx)`: `idx * 2 + 1`.
    #[must_use]
    pub fn left_child_idx(idx: usize) -> usize {
        idx * 2 + 1
    }

    /// `right_child_idx(idx)`.
    #[must_use]
    pub fn right_child_idx(idx: usize) -> usize {
        Self::left_child_idx(idx) + 1
    }

    /// `left_child(idx)`.
    #[must_use]
    pub fn left_child(&self, idx: usize) -> &Node<S, N> {
        &self.nodes[Self::left_child_idx(idx)]
    }

    /// `right_child(idx)`.
    #[must_use]
    pub fn right_child(&self, idx: usize) -> &Node<S, N> {
        &self.nodes[Self::right_child_idx(idx)]
    }

    /// `build_recursive`: a leaf for one item, else an inner node over the union of the boxes,
    /// split at the median along the union's longest side (the first of equal sides).
    fn build_recursive<I: SourceNode<S, N>>(
        &mut self,
        input: &mut [I],
        node: usize,
        left: usize,
        right: usize,
    ) {
        if left == right {
            // Insert a node into the balanced tree.
            self.nodes[node].idx = input[left].idx();
            self.nodes[node].bbox = input[left].bbox();
            return;
        }

        // Calculate bounding box of the input.
        let mut bbox = input[left].bbox();
        for item in &input[left + 1..=right] {
            bbox.extend_box(&item.bbox());
        }
        let (_, dimension) = bbox.diagonal().max_coeff_index();

        // Partition the input to left / right pieces of the same length to produce a balanced tree.
        let center = (left + right) / 2;
        partition_input(input, dimension, left, right, center);
        // Insert an inner node into the tree. Inner node does not reference any input entity.
        self.nodes[node].idx = INNER;
        self.nodes[node].bbox = bbox;
        self.build_recursive(input, node * 2 + 1, left, center);
        self.build_recursive(input, node * 2 + 2, center + 1, right);
    }
}

/// `partition_input`: the quickselect that puts the `k`-th item of `[left, right]` (by centroid
/// along `dimension`) at `k`, lower ones before it and higher ones after.
fn partition_input<S: BoxScalar, const N: usize, I: SourceNode<S, N>>(
    input: &mut [I],
    dimension: usize,
    mut left: usize,
    mut right: usize,
    k: usize,
) {
    let value = |input: &[I], i: usize| input[i].centroid()[dimension];
    while left < right {
        let center = (left + right) / 2;
        let pivot;
        {
            // Bubble sort input[left], input[center], input[right], so that a median of the three
            // values will end up in input[center].
            let mut left_value = value(input, left);
            let mut center_value = value(input, center);
            let mut right_value = value(input, right);
            if left_value > center_value {
                input.swap(left, center);
                core::mem::swap(&mut left_value, &mut center_value);
            }
            if left_value > right_value {
                input.swap(left, right);
                right_value = left_value;
            }
            if center_value > right_value {
                input.swap(center, right);
                center_value = right_value;
            }
            pivot = center_value;
        }
        if right <= left + 2 {
            // The <left, right> interval is already sorted.
            break;
        }
        let mut i = left;
        let mut j = right - 1;
        input.swap(center, j);
        // Partition the set based on the pivot.
        loop {
            // Skip left points that are already at correct positions. The search stops at
            // (right - 1) at the latest, which holds the pivot.
            loop {
                i += 1;
                if value(input, i).partial_cmp(&pivot) != Some(Ordering::Less) {
                    break;
                }
            }
            // Skip right points that are already at correct positions.
            loop {
                j -= 1;
                let go_on = value(input, j) > pivot && i < j;
                if !go_on {
                    break;
                }
            }
            if i >= j {
                break;
            }
            input.swap(i, j);
        }
        // Restore pivot to the center of the sequence.
        input.swap(i, right - 1);
        // Which side the kth element is in?
        if k < i {
            right = i - 1;
        } else if k == i {
            // Sequence is partitioned, kth element is at its place.
            break;
        } else {
            left = i + 1;
        }
    }
}

/// `BoundingBoxWrapper`: a `Slic3r::BoundingBox` as an item of a `Tree<2, coord_t>`, grown by
/// `SCALED_EPSILON` on each side to account for numerical issues.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoundingBoxWrapper {
    idx: usize,
    bbox: AlignedBox<i64, 2>,
}

impl BoundingBoxWrapper {
    /// `BoundingBoxWrapper(idx, bbox)`: the corners moved out by `Point(SCALED_EPSILON,
    /// SCALED_EPSILON)` (each rounded, as `Point(double, double)` rounds), wrapping.
    #[must_use]
    pub fn new(idx: usize, bbox: &orca_geom::bounding_box::BoundingBox) -> Self {
        let eps = Self::epsilon();
        let min = bbox.min - eps;
        let max = bbox.max + eps;
        Self {
            idx,
            bbox: AlignedBox::new(Vector([min.x, min.y]), Vector([max.x, max.y])),
        }
    }

    /// The amount added to each side: `Point(SCALED_EPSILON, SCALED_EPSILON)`.
    #[must_use]
    pub fn epsilon() -> orca_geom::point::Point {
        orca_geom::point::Point::from_f64(
            orca_geom::point::SCALED_EPSILON,
            orca_geom::point::SCALED_EPSILON,
        )
    }
}

impl SourceNode<i64, 2> for BoundingBoxWrapper {
    fn idx(&self) -> usize {
        self.idx
    }
    fn bbox(&self) -> AlignedBox<i64, 2> {
        self.bbox
    }
    /// `m_bbox.min() + m_bbox.max() / 2`: the maximum halved toward zero, then added, wrapping.
    fn centroid(&self) -> Vector<i64, 2> {
        Vector([
            self.bbox.min[0].wrapping_add(self.bbox.max[0] / 2),
            self.bbox.min[1].wrapping_add(self.bbox.max[1] / 2),
        ])
    }
}

/// `igl::Hit<float>`: a ray's hit of a triangle.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hit {
    /// `id`: the face's index (`int(node.idx)`).
    pub id: i32,
    /// `gid`: always -1 here.
    pub gid: i32,
    /// `u`: the first barycentric coordinate.
    pub u: f32,
    /// `v`: the second.
    pub v: f32,
    /// `t`: the ray's parameter.
    pub t: f32,
}

/// `detail::ray_box_intersect_invdir(origin, inv_dir, box, t0, t1)`: whether the ray meets the box
/// with a parameter in `(t0, t1)`. The box's corners are swapped per axis where the inverse
/// direction is negative.
#[must_use]
pub fn ray_box_intersect_invdir<R: Real + BoxScalar>(
    origin: &Vector<R, 3>,
    inv_dir: &Vector<R, 3>,
    bbox: AlignedBox<R, 3>,
    t0: R,
    t1: R,
) -> bool {
    let (mut bmin, mut bmax) = (bbox.min, bbox.max);
    if inv_dir[0] < R::ZERO {
        core::mem::swap(&mut bmin.0[0], &mut bmax.0[0]);
    }
    if inv_dir[1] < R::ZERO {
        core::mem::swap(&mut bmin.0[1], &mut bmax.0[1]);
    }
    let mut tmin = bmin[0].sub(origin[0]).mul(inv_dir[0]);
    let tymax = bmax[1].sub(origin[1]).mul(inv_dir[1]);
    if tmin > tymax {
        return false;
    }
    let mut tmax = bmax[0].sub(origin[0]).mul(inv_dir[0]);
    let tymin = bmin[1].sub(origin[1]).mul(inv_dir[1]);
    if tymin > tmax {
        return false;
    }
    if tymin > tmin {
        tmin = tymin;
    }
    if tymax < tmax {
        tmax = tymax;
    }
    if inv_dir[2] < R::ZERO {
        core::mem::swap(&mut bmin.0[2], &mut bmax.0[2]);
    }
    let tzmin = bmin[2].sub(origin[2]).mul(inv_dir[2]);
    if tzmin > tmax {
        return false;
    }
    let tzmax = bmax[2].sub(origin[2]).mul(inv_dir[2]);
    if tmin > tzmax {
        return false;
    }
    if tzmin > tmin {
        tmin = tzmin;
    }
    if tzmax < tmax {
        tmax = tzmax;
    }
    tmin < t1 && tmax > t0
}

/// `detail::intersect_triangle` (Moeller's `intersect_triangle1`), in `double`: `(t, u, v)` where
/// the ray meets the triangle, or `None` (the determinant within `eps` of 0, or the hit outside).
#[must_use]
pub fn intersect_triangle(
    orig: &Vec3d,
    dir: &Vec3d,
    vert0: &Vec3d,
    vert1: &Vec3d,
    vert2: &Vec3d,
    eps: f64,
) -> Option<(f64, f64, f64)> {
    // Find vectors for two edges sharing vert0.
    let edge1 = *vert1 - *vert0;
    let edge2 = *vert2 - *vert0;
    // Begin calculating the determinant, also used to calculate the U parameter.
    let pvec = dir.cross(&edge2);
    // If the determinant is near zero, the ray lies in the plane of the triangle.
    let det = edge1.dot(&pvec);
    let (u, v, qvec);
    if det > eps {
        // Calculate the distance from vert0 to the ray origin.
        let tvec = *orig - *vert0;
        // Calculate the U parameter and test bounds.
        u = tvec.dot(&pvec);
        if u < 0.0 || u > det {
            return None;
        }
        // Prepare to test the V parameter.
        qvec = tvec.cross(&edge1);
        // Calculate the V parameter and test bounds.
        v = dir.dot(&qvec);
        if v < 0.0 || u + v > det {
            return None;
        }
    } else if det < -eps {
        let tvec = *orig - *vert0;
        u = tvec.dot(&pvec);
        if u > 0.0 || u < det {
            return None;
        }
        qvec = tvec.cross(&edge1);
        v = dir.dot(&qvec);
        if v > 0.0 || u + v < det {
            return None;
        }
    } else {
        // The ray is parallel to the plane of the triangle.
        return None;
    }
    let inv_det = 1.0 / det;
    // Calculate t: the ray intersects the triangle.
    let t = edge2.dot(&qvec) * inv_det;
    Some((t, u * inv_det, v * inv_det))
}

/// `detail::RayIntersector`: the mesh, its tree and the ray, with its inverse direction.
struct RayIntersector<'a, V, S, R> {
    vertices: &'a [Vector<V, 3>],
    faces: &'a [Vec3i32],
    tree: &'a Tree<S, 3>,
    origin: Vector<R, 3>,
    dir: Vector<R, 3>,
    invdir: Vector<R, 3>,
    eps: f64,
}

impl<V, S, R> RayIntersector<'_, V, S, R>
where
    V: BoxScalar + Cast<f64>,
    S: BoxScalar + Cast<R>,
    R: Real + BoxScalar + Cast<f64>,
    f32: Cast<R>,
{
    /// The leaf's triangle hit by the ray with `t > 0`, as an `igl::Hit<float>`.
    fn leaf_hit(&self, idx: usize) -> Option<Hit> {
        let face = self.faces[idx];
        let vertex = |k: usize| self.vertices[face[k] as usize].cast::<f64>();
        match intersect_triangle(
            &self.origin.cast(),
            &self.dir.cast(),
            &vertex(0),
            &vertex(1),
            &vertex(2),
            self.eps,
        ) {
            Some((t, u, v)) if t > 0.0 => Some(Hit {
                id: idx as i32,
                gid: -1,
                u: u as f32,
                v: v as f32,
                t: t as f32,
            }),
            _ => None,
        }
    }

    /// `detail::intersect_ray_recursive_first_hit`: the nearest hit below `min_t` in the subtree.
    fn first_hit(&self, node_idx: usize, mut min_t: R) -> Option<Hit> {
        let node = self.tree.node(node_idx);
        if !ray_box_intersect_invdir(
            &self.origin,
            &self.invdir,
            node.bbox.cast::<R>(),
            R::ZERO,
            min_t,
        ) {
            return None;
        }
        if node.is_leaf() {
            // Shoot the ray, record the hit.
            self.leaf_hit(node.idx)
        } else {
            // Left / right child node index.
            let left = node_idx * 2 + 1;
            let right = left + 1;
            let t_of = |hit: &Hit| -> R { hit.t.cast() };
            let mut hit = None;
            if let Some(left_hit) = self.first_hit(left, min_t)
                && t_of(&left_hit) < min_t
            {
                min_t = t_of(&left_hit);
                hit = Some(left_hit);
            }
            if let Some(right_hit) = self.first_hit(right, min_t)
                && t_of(&right_hit) < min_t
            {
                hit = Some(right_hit);
            }
            hit
        }
    }

    /// `detail::intersect_ray_recursive_all_hits`: every hit in the subtree, left before right.
    fn all_hits(&self, node_idx: usize, hits: &mut Vec<Hit>) {
        let node = self.tree.node(node_idx);
        if !ray_box_intersect_invdir(
            &self.origin,
            &self.invdir,
            node.bbox.cast::<R>(),
            R::ZERO,
            R::from_f64(f64::INFINITY),
        ) {
            return;
        }
        if node.is_leaf() {
            if let Some(hit) = self.leaf_hit(node.idx) {
                hits.push(hit);
            }
        } else {
            let left = node_idx * 2 + 1;
            let right = left + 1;
            self.all_hits(left, hits);
            self.all_hits(right, hits);
        }
    }
}

/// `build_aabb_tree_over_indexed_triangle_set(vertices, faces, eps)`: a tree over the faces,
/// balanced on their centroids (`(1./3.) * (v1 + v2 + v3)`, the third rounded to the vertices'
/// scalar), each box grown by `eps` on every side.
#[must_use]
pub fn build_aabb_tree_over_indexed_triangle_set<S: Real + BoxScalar>(
    vertices: &[Vector<S, 3>],
    faces: &[Vec3i32],
    eps: S,
) -> Tree<S, 3> {
    #[derive(Clone)]
    struct InputType<S> {
        idx: usize,
        bbox: AlignedBox<S, 3>,
        centroid: Vector<S, 3>,
    }
    impl<S: BoxScalar> SourceNode<S, 3> for InputType<S> {
        fn idx(&self) -> usize {
            self.idx
        }
        fn bbox(&self) -> AlignedBox<S, 3> {
            self.bbox
        }
        fn centroid(&self) -> Vector<S, 3> {
            self.centroid
        }
    }

    let third = S::from_f64(1. / 3.);
    let veps = Vector::constant(eps);
    let mut input = Vec::with_capacity(faces.len());
    for (i, face) in faces.iter().enumerate() {
        let v1 = vertices[face[0] as usize];
        let v2 = vertices[face[1] as usize];
        let v3 = vertices[face[2] as usize];
        let sum = v1 + v2 + v3;
        let mut bbox = AlignedBox::new(v1, v1);
        bbox.extend_point(&v2);
        bbox.extend_point(&v3);
        bbox.min -= veps;
        bbox.max += veps;
        input.push(InputType {
            idx: i,
            bbox,
            centroid: sum.map(|x| third.mul(x)),
        });
    }
    let mut out = Tree::new();
    out.build(input);
    out
}

/// `intersect_ray_first_hit(vertices, faces, tree, origin, dir, hit, eps)`: the hit with the
/// smallest `t`, or `None` (the C++ returns false and leaves `hit` as it was). The default `eps`
/// of the C++ is 0.000001.
#[must_use]
pub fn intersect_ray_first_hit<V, S, R>(
    vertices: &[Vector<V, 3>],
    faces: &[Vec3i32],
    tree: &Tree<S, 3>,
    origin: &Vector<R, 3>,
    dir: &Vector<R, 3>,
    eps: f64,
) -> Option<Hit>
where
    V: BoxScalar + Cast<f64>,
    S: BoxScalar + Cast<R>,
    R: Real + BoxScalar + Cast<f64>,
    f32: Cast<R>,
{
    let ray = RayIntersector {
        vertices,
        faces,
        tree,
        origin: *origin,
        dir: *dir,
        invdir: dir.cwise_inverse(),
        eps,
    };
    if tree.is_empty() {
        return None;
    }
    ray.first_hit(0, R::from_f64(f64::INFINITY))
}

/// `intersect_ray_all_hits(vertices, faces, tree, origin, dir, hits, eps)`: every hit, sorted by
/// `t` with MSVC's `std::sort` (`orca_stl::sort_by`, `l.t < r.t`), into `hits` (cleared first);
/// whether there is any.
pub fn intersect_ray_all_hits<V, S, R>(
    vertices: &[Vector<V, 3>],
    faces: &[Vec3i32],
    tree: &Tree<S, 3>,
    origin: &Vector<R, 3>,
    dir: &Vector<R, 3>,
    hits: &mut Vec<Hit>,
    eps: f64,
) -> bool
where
    V: BoxScalar + Cast<f64>,
    S: BoxScalar + Cast<R>,
    R: Real + BoxScalar + Cast<f64>,
    f32: Cast<R>,
{
    let ray = RayIntersector {
        vertices,
        faces,
        tree,
        origin: *origin,
        dir: *dir,
        invdir: dir.cwise_inverse(),
        eps,
    };
    hits.clear();
    if !tree.is_empty() {
        hits.reserve(8);
        ray.all_hits(0, hits);
        orca_stl::sort_by(hits, |l, r| l.t < r.t);
    }
    !hits.is_empty()
}

/// `detail::closest_point_to_triangle(p, a, b, c)` (Ericson, Real-time collision detection,
/// chapter 5): the point of the triangle nearest `p`, by the region `p` projects into.
#[must_use]
pub fn closest_point_to_triangle<R: Real>(
    p: &Vector<R, 3>,
    a: &Vector<R, 3>,
    b: &Vector<R, 3>,
    c: &Vector<R, 3>,
) -> Vector<R, 3> {
    let (p, a, b, c) = (*p, *a, *b, *c);
    let zero = R::ZERO;
    // Check if P in vertex region outside A.
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(&ap);
    let d2 = ac.dot(&ap);
    if d1 <= zero && d2 <= zero {
        return a;
    }
    // Check if P in vertex region outside B.
    let bp = p - b;
    let d3 = ab.dot(&bp);
    let d4 = ac.dot(&bp);
    if d3 >= zero && d4 <= d3 {
        return b;
    }
    // Check if P in edge region of AB, if so return projection of P onto AB.
    let vc = d1.mul(d4).sub(d3.mul(d2));
    let a_ne_b = (0..3).any(|k| a[k] != b[k]);
    if a_ne_b && vc <= zero && d1 >= zero && d3 <= zero {
        let v = d1.div(d1.sub(d3));
        return a.zip(&ab, |ai, abi| ai.add(v.mul(abi)));
    }
    // Check if P in vertex region outside C.
    let cp = p - c;
    let d5 = ab.dot(&cp);
    let d6 = ac.dot(&cp);
    if d6 >= zero && d5 <= d6 {
        return c;
    }
    // Check if P in edge region of AC, if so return projection of P onto AC.
    let vb = d5.mul(d2).sub(d1.mul(d6));
    if vb <= zero && d2 >= zero && d6 <= zero {
        let w = d2.div(d2.sub(d6));
        return a.zip(&ac, |ai, aci| ai.add(w.mul(aci)));
    }
    // Check if P in edge region of BC, if so return projection of P onto BC.
    let va = d3.mul(d6).sub(d5.mul(d4));
    if va <= zero && d4.sub(d3) >= zero && d5.sub(d6) >= zero {
        let w = d4.sub(d3).div(d4.sub(d3).add(d5.sub(d6)));
        let cb = c - b;
        return b.zip(&cb, |bi, cbi| bi.add(w.mul(cbi)));
    }
    // P inside face region. Compute Q through its barycentric coordinates (u, v, w).
    let denom = R::ONE.div(va.add(vb).add(vc));
    let v = vb.mul(denom);
    let w = vc.mul(denom);
    // = u*a + v*b + w*c, u = va * denom = 1.0 - v - w
    Vector(core::array::from_fn(|k| {
        a[k].add(ab[k].mul(v)).add(ac[k].mul(w))
    }))
}

/// What [`squared_distance_to_indexed_primitives_recursive`] reads (the C++'s
/// `IndexedPrimitivesDistancerType`): the tree, the point (`origin`) and the nearest point of an
/// item with its squared distance.
pub trait IndexedPrimitivesDistancer<S, R, const N: usize> {
    /// `tree`.
    fn tree(&self) -> &Tree<S, N>;
    /// `origin`.
    fn origin(&self) -> &Vector<R, N>;
    /// `closest_point_to_origin(primitive_index, squared_distance)`: the nearest point and the
    /// squared distance to it.
    fn closest_point_to_origin(&self, primitive_index: usize) -> (Vector<R, N>, R);
}

/// `detail::IndexedTriangleSetDistancer`: the triangles' nearest points, each vertex cast to the
/// point's scalar.
pub struct IndexedTriangleSetDistancer<'a, V, S, R> {
    /// `vertices`.
    pub vertices: &'a [Vector<V, 3>],
    /// `faces`.
    pub faces: &'a [Vec3i32],
    /// `tree`.
    pub tree: &'a Tree<S, 3>,
    /// `origin`.
    pub origin: Vector<R, 3>,
}

impl<V, S, R> IndexedPrimitivesDistancer<S, R, 3> for IndexedTriangleSetDistancer<'_, V, S, R>
where
    V: BoxScalar + Cast<R>,
    R: Real,
{
    fn tree(&self) -> &Tree<S, 3> {
        self.tree
    }
    fn origin(&self) -> &Vector<R, 3> {
        &self.origin
    }
    fn closest_point_to_origin(&self, primitive_index: usize) -> (Vector<R, 3>, R) {
        let triangle = self.faces[primitive_index];
        let vertex = |k: usize| self.vertices[triangle[k] as usize].cast::<R>();
        let closest_point =
            closest_point_to_triangle(&self.origin, &vertex(0), &vertex(1), &vertex(2));
        let squared_distance = (self.origin - closest_point).squared_norm();
        (closest_point, squared_distance)
    }
}

/// `detail::squared_distance_to_indexed_primitives_recursive(distancer, node_idx, low_sqr_d,
/// up_sqr_d, i, c)`: the smallest squared distance below `up_sqr_d` in the subtree, writing its
/// item to `i` and its point to `c` only when it finds one nearer; `up_sqr_d` when it finds none.
/// A child whose box holds the point (cast to the tree's scalar) is looked at first, then by the
/// boxes' exterior distances, the nearer child first (the right one on a tie).
pub fn squared_distance_to_indexed_primitives_recursive<D, S, R, const N: usize>(
    distancer: &D,
    node_idx: usize,
    low_sqr_d: R,
    mut up_sqr_d: R,
    i: &mut usize,
    c: &mut Vector<R, N>,
) -> R
where
    D: IndexedPrimitivesDistancer<S, R, N>,
    S: BoxScalar + ExteriorDistance<R> + Cast<R>,
    R: Real + Cast<S>,
{
    if low_sqr_d > up_sqr_d {
        return low_sqr_d;
    }

    // Save the best achieved hit.
    let set_min = |up_sqr_d: &mut R,
                   i: &mut usize,
                   c: &mut Vector<R, N>,
                   sqr_d_candidate: R,
                   i_candidate: usize,
                   c_candidate: Vector<R, N>| {
        if sqr_d_candidate < *up_sqr_d {
            *i = i_candidate;
            *c = c_candidate;
            *up_sqr_d = sqr_d_candidate;
        }
    };

    let tree = distancer.tree();
    let node = tree.node(node_idx);
    if node.is_leaf() {
        let (c_candidate, sqr_dist) = distancer.closest_point_to_origin(node.idx);
        set_min(&mut up_sqr_d, i, c, sqr_dist, node.idx, c_candidate);
    } else {
        let left_node_idx = node_idx * 2 + 1;
        let right_node_idx = left_node_idx + 1;
        let node_left = tree.node(left_node_idx);
        let node_right = tree.node(right_node_idx);

        let mut looked_left = false;
        let mut looked_right = false;
        // The C++ lambdas look_left and look_right: recurse with the current limit into a copy of
        // `c`, then keep what was found if it is nearer. (The copy of `i` is never read unless the
        // recursion wrote it.)
        let look = |child: usize, up_sqr_d: &mut R, i: &mut usize, c: &mut Vector<R, N>| {
            let mut i_child = NPOS;
            let mut c_child = *c;
            let sqr_d_child = squared_distance_to_indexed_primitives_recursive(
                distancer,
                child,
                low_sqr_d,
                *up_sqr_d,
                &mut i_child,
                &mut c_child,
            );
            set_min(up_sqr_d, i, c, sqr_d_child, i_child, c_child);
        };

        // Must look left or right if in box.
        let origin = *distancer.origin();
        let origin_s: Vector<S, N> = origin.cast();
        if node_left.bbox.contains_point(&origin_s) {
            look(left_node_idx, &mut up_sqr_d, i, c);
            looked_left = true;
        }
        if node_right.bbox.contains_point(&origin_s) {
            look(right_node_idx, &mut up_sqr_d, i, c);
            looked_right = true;
        }
        // If haven't looked left and could be less than current min, then look.
        let left_up_sqr_d: R = node_left.bbox.squared_exterior_distance(&origin).cast();
        let right_up_sqr_d: R = node_right.bbox.squared_exterior_distance(&origin).cast();
        if left_up_sqr_d < right_up_sqr_d {
            if !looked_left && left_up_sqr_d < up_sqr_d {
                look(left_node_idx, &mut up_sqr_d, i, c);
            }
            if !looked_right && right_up_sqr_d < up_sqr_d {
                look(right_node_idx, &mut up_sqr_d, i, c);
            }
        } else {
            if !looked_right && right_up_sqr_d < up_sqr_d {
                look(right_node_idx, &mut up_sqr_d, i, c);
            }
            if !looked_left && left_up_sqr_d < up_sqr_d {
                look(left_node_idx, &mut up_sqr_d, i, c);
            }
        }
    }
    up_sqr_d
}

/// `detail::indexed_primitives_within_distance_squared_recurisve`: the items of the subtree whose
/// nearest point is below `squared_distance_limit`, left before right, a child skipped when its
/// box's exterior distance is not below the limit.
pub fn indexed_primitives_within_distance_squared_recursive<D, S, R, const N: usize>(
    distancer: &D,
    node_idx: usize,
    squared_distance_limit: R,
    found_primitives_indices: &mut Vec<usize>,
) where
    D: IndexedPrimitivesDistancer<S, R, N>,
    S: BoxScalar + ExteriorDistance<R> + Cast<R>,
    R: Real,
{
    let tree = distancer.tree();
    let node = tree.node(node_idx);
    if node.is_leaf() {
        let (_, sqr_dist) = distancer.closest_point_to_origin(node.idx);
        if sqr_dist < squared_distance_limit {
            found_primitives_indices.push(node.idx);
        }
    } else {
        let left_node_idx = node_idx * 2 + 1;
        let right_node_idx = left_node_idx + 1;
        let origin = distancer.origin();
        let left: R = tree
            .node(left_node_idx)
            .bbox
            .squared_exterior_distance(origin)
            .cast();
        if left < squared_distance_limit {
            indexed_primitives_within_distance_squared_recursive(
                distancer,
                left_node_idx,
                squared_distance_limit,
                found_primitives_indices,
            );
        }
        let right: R = tree
            .node(right_node_idx)
            .bbox
            .squared_exterior_distance(origin)
            .cast();
        if right < squared_distance_limit {
            indexed_primitives_within_distance_squared_recursive(
                distancer,
                right_node_idx,
                squared_distance_limit,
                found_primitives_indices,
            );
        }
    }
}

/// `squared_distance_to_indexed_triangle_set(vertices, faces, tree, point, hit_idx_out,
/// hit_point_out)`: the squared distance to the nearest triangle, -1 for an empty tree; the
/// triangle and the point are written to the out-parameters when one is found.
pub fn squared_distance_to_indexed_triangle_set<V, S, R>(
    vertices: &[Vector<V, 3>],
    faces: &[Vec3i32],
    tree: &Tree<S, 3>,
    point: &Vector<R, 3>,
    hit_idx_out: &mut usize,
    hit_point_out: &mut Vector<R, 3>,
) -> R
where
    V: BoxScalar + Cast<R>,
    S: BoxScalar + ExteriorDistance<R> + Cast<R>,
    R: Real + Cast<S>,
{
    let distancer = IndexedTriangleSetDistancer {
        vertices,
        faces,
        tree,
        origin: *point,
    };
    if tree.is_empty() {
        R::from_f64(-1.0)
    } else {
        squared_distance_to_indexed_primitives_recursive(
            &distancer,
            0,
            R::ZERO,
            R::from_f64(f64::INFINITY),
            hit_idx_out,
            hit_point_out,
        )
    }
}

/// `is_any_triangle_in_radius(vertices, faces, tree, point, max_distance_squared)`: whether some
/// triangle is nearer than the limit (the search starts from a NaN point and answers whether it
/// was replaced, `allFinite()`).
#[must_use]
pub fn is_any_triangle_in_radius<V, S, R>(
    vertices: &[Vector<V, 3>],
    faces: &[Vec3i32],
    tree: &Tree<S, 3>,
    point: &Vector<R, 3>,
    max_distance_squared: R,
) -> bool
where
    V: BoxScalar + Cast<R>,
    S: BoxScalar + ExteriorDistance<R> + Cast<R>,
    R: Real + Cast<S>,
{
    let distancer = IndexedTriangleSetDistancer {
        vertices,
        faces,
        tree,
        origin: *point,
    };
    let mut hit_idx = NPOS;
    let mut hit_point = Vector::<R, 3>::ones().map(|x| x.mul(R::from_f64(f64::NAN)));
    if tree.is_empty() {
        return false;
    }
    squared_distance_to_indexed_primitives_recursive(
        &distancer,
        0,
        R::ZERO,
        max_distance_squared,
        &mut hit_idx,
        &mut hit_point,
    );
    (0..3).all(|k| hit_point[k].is_finite())
}

/// `all_triangles_in_radius(vertices, faces, tree, point, max_distance_squared)`: the triangles
/// nearer than the limit, in the tree's order (left before right).
#[must_use]
pub fn all_triangles_in_radius<V, S, R>(
    vertices: &[Vector<V, 3>],
    faces: &[Vec3i32],
    tree: &Tree<S, 3>,
    point: &Vector<R, 3>,
    max_distance_squared: R,
) -> Vec<usize>
where
    V: BoxScalar + Cast<R>,
    S: BoxScalar + ExteriorDistance<R> + Cast<R>,
    R: Real,
{
    let distancer = IndexedTriangleSetDistancer {
        vertices,
        faces,
        tree,
        origin: *point,
    };
    if tree.is_empty() {
        return Vec::new();
    }
    let mut found_triangles = Vec::new();
    indexed_primitives_within_distance_squared_recursive(
        &distancer,
        0,
        max_distance_squared,
        &mut found_triangles,
    );
    found_triangles
}

/// `get_candidate_idxs(tree, v, candidates, node_idx)`: appends the leaves under `node_idx` whose
/// boxes hold `v` (call it with node 0 for the whole tree).
pub fn get_candidate_idxs<S: BoxScalar, const N: usize>(
    tree: &Tree<S, N>,
    v: &Vector<S, N>,
    candidates: &mut Vec<usize>,
    node_idx: usize,
) {
    if tree.is_empty() || !tree.node(node_idx).bbox.contains_point(v) {
        return;
    }
    let node = tree.node(node_idx);
    if !node.is_leaf() {
        if tree.left_child(node_idx).bbox.contains_point(v) {
            get_candidate_idxs(tree, v, candidates, Tree::<S, N>::left_child_idx(node_idx));
        }
        if tree.right_child(node_idx).bbox.contains_point(v) {
            get_candidate_idxs(tree, v, candidates, Tree::<S, N>::right_child_idx(node_idx));
        }
    } else {
        candidates.push(node.idx);
    }
}

/// `intersecting(box)` (`Intersecting<AlignedBox>`): a node whose box meets `bbox`.
pub fn intersecting<S: BoxScalar, const N: usize>(
    bbox: AlignedBox<S, N>,
) -> impl Fn(&Node<S, N>) -> bool {
    move |node| bbox.intersects(&node.bbox)
}

/// `within(box)` (`Within<AlignedBox>`): a leaf whose box `bbox` contains, or an inner node whose
/// box meets it.
pub fn within<S: BoxScalar, const N: usize>(
    bbox: AlignedBox<S, N>,
) -> impl Fn(&Node<S, N>) -> bool {
    move |node| {
        if node.is_leaf() {
            bbox.contains_box(&node.bbox)
        } else {
            bbox.intersects(&node.bbox)
        }
    }
}

/// `detail::traverse_recurse`: false once the callback has asked to stop.
fn traverse_recurse<S, const N: usize, P, F>(
    tree: &Tree<S, N>,
    idx: usize,
    pred: &mut P,
    callback: &mut F,
) -> bool
where
    S: BoxScalar,
    P: FnMut(&Node<S, N>) -> bool,
    F: FnMut(&Node<S, N>) -> bool,
{
    if !pred(tree.node(idx)) {
        // Continue traversal.
        return true;
    }
    if tree.node(idx).is_leaf() {
        // The callback returns true to continue traversal, false to stop traversal.
        callback(tree.node(idx))
    } else {
        // Left / right child node index; true if both children let the traversal continue.
        traverse_recurse(tree, Tree::<S, N>::left_child_idx(idx), pred, callback)
            && traverse_recurse(tree, Tree::<S, N>::right_child_idx(idx), pred, callback)
    }
}

/// `traverse(tree, pred, callback)`: visits, left before right, the nodes `pred` accepts, calling
/// `callback` on each such leaf until it returns false.
pub fn traverse<S, const N: usize, P, F>(tree: &Tree<S, N>, mut pred: P, mut callback: F)
where
    S: BoxScalar,
    P: FnMut(&Node<S, N>) -> bool,
    F: FnMut(&Node<S, N>) -> bool,
{
    if tree.is_empty() {
        return;
    }
    traverse_recurse(tree, 0, &mut pred, &mut callback);
}
