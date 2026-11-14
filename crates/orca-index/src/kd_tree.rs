// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/KDTreeIndirect.hpp lines 1-374 whole, of OrcaSlicer
// v2.4.2, translated on 2026-11-13 by index-1. Copyright the OrcaSlicer authors, the BambuStudio
// authors (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU Affero General
// Public License, version 3 (AGPL-3.0; LICENSE).

//! `KDTreeIndirect<D, CoordType, CoordinateFn>`: a balanced KD tree over points held elsewhere,
//! addressed by index through a coordinate function, with its nearest and range searches.
//!
//! - The tree is one vector of `next_highest_power_of_2(n + 1)` slots, each a point's index or
//!   [`NPOS`]; the children of slot `i` are `2i + 1` and `2i + 2`. `build_recursive` puts the
//!   median (`(left + right) / 2`, found by the same quickselect as the AABB tree's) at each slot,
//!   cycling the dimension with the depth.
//! - [`KdTreeIndirect::visit`] calls the visitor on a slot, then on the children its mask names
//!   ([`VisitorReturnMask`]), left first; `STOP` ends that branch.
//! - [`find_closest_points`] keeps the `K` nearest in a sorted array (a later point at the same
//!   distance goes before the ones kept, by `std::lower_bound` through orca-stl, then
//!   `std::rotate`) and descends by [`KdTreeIndirect::descent_mask`] against the nearest distance
//!   so far (the first of the `K`, not the last, as the C++ does: for `K` above 1 it can miss some
//!   of the `K` nearest); the searches run in the tree's coordinate type, `float` or `double`, with
//!   `CoordType(EPSILON)` added to the squared radius in the descent test.

use core::ops::Index;

use orca_eigen::Real;

use crate::aabb_tree::next_highest_power_of_2;

/// `KDTreeIndirect::npos`: an empty slot, and "not found".
pub const NPOS: usize = usize::MAX;

/// `VisitorReturnMask`: what a visitor answers for each slot, as a bit mask.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum VisitorReturnMask {
    /// Visit the left child.
    ContinueLeft = 1,
    /// Visit the right child.
    ContinueRight = 2,
    /// Visit neither.
    Stop = 4,
}

/// Both children: `CONTINUE_LEFT | CONTINUE_RIGHT`.
const BOTH: u32 = VisitorReturnMask::ContinueLeft as u32 | VisitorReturnMask::ContinueRight as u32;

/// `KDTreeIndirect<D, C, F>`.
#[derive(Clone, Debug)]
pub struct KdTreeIndirect<const D: usize, C, F> {
    /// `coordinate`: `coordinate(idx, dimension)` of the point `idx`.
    pub coordinate: F,
    nodes: Vec<usize>,
    _coord: core::marker::PhantomData<C>,
}

impl<const D: usize, C: Real, F: Fn(usize, usize) -> C> KdTreeIndirect<D, C, F> {
    /// `KDTreeIndirect(coordinate)`: an empty tree.
    pub fn new(coordinate: F) -> Self {
        Self {
            coordinate,
            nodes: Vec::new(),
            _coord: core::marker::PhantomData,
        }
    }

    /// `KDTreeIndirect(coordinate, std::vector<size_t> indices)`: over the given points.
    pub fn with_indices(coordinate: F, mut indices: Vec<usize>) -> Self {
        let mut tree = Self::new(coordinate);
        tree.build(&mut indices);
        tree
    }

    /// `KDTreeIndirect(coordinate, size_t num_indices)`: over the points `0..num_indices`.
    pub fn with_count(coordinate: F, num_indices: usize) -> Self {
        let mut tree = Self::new(coordinate);
        tree.build_count(num_indices);
        tree
    }

    /// `clear()`.
    pub fn clear(&mut self) {
        self.nodes.clear();
    }

    /// `build(size_t num_indices)`.
    pub fn build_count(&mut self, num_indices: usize) {
        let mut indices: Vec<usize> = (0..num_indices).collect();
        self.build(&mut indices);
    }

    /// `build(std::vector<size_t> &indices)`: builds over the points and clears `indices`, which
    /// the quickselect reorders on the way.
    pub fn build(&mut self, indices: &mut Vec<usize>) {
        if indices.is_empty() {
            self.clear();
        } else {
            // Allocate enough memory for a full binary tree.
            let count = next_highest_power_of_2(indices.len() as u64 + 1) as usize;
            self.nodes.clear();
            self.nodes.resize(count, NPOS);
            let right = indices.len() - 1;
            self.build_recursive(indices, 0, 0, 0, right);
        }
        indices.clear();
    }

    /// `descent_mask(point_coord, search_radius, idx, dimension)`: both children when the plane
    /// of the point `idx` is within the radius (`dist * dist < search_radius + C(EPSILON)`), else
    /// the side the point lies on.
    pub fn descent_mask(
        &self,
        point_coord: C,
        search_radius: C,
        idx: usize,
        dimension: usize,
    ) -> u32 {
        let dist = point_coord.sub((self.coordinate)(idx, dimension));
        if dist.mul(dist) < search_radius.add(C::from_f64(orca_geom::point::EPSILON)) {
            // The plane intersects a hypersphere centered at point_coord of search_radius.
            BOTH
        } else if dist > C::ZERO {
            // The plane does not intersect the hypersphere.
            VisitorReturnMask::ContinueRight as u32
        } else {
            VisitorReturnMask::ContinueLeft as u32
        }
    }

    /// `visit(visitor)`: the visitor takes a point's index and the slot's dimension and answers a
    /// [`VisitorReturnMask`] bit mask.
    pub fn visit<V: FnMut(usize, usize) -> u32>(&self, visitor: &mut V) {
        self.visit_recursive(0, 0, visitor);
    }

    /// The slots, each a point's index or [`NPOS`] (the C++'s private `m_nodes`).
    #[must_use]
    pub fn nodes(&self) -> &[usize] {
        &self.nodes
    }

    /// `build_recursive`: the median of `[left, right]` along `dimension` at `node`, then the
    /// halves below it with the next dimension.
    fn build_recursive(
        &mut self,
        input: &mut [usize],
        node: usize,
        dimension: usize,
        left: usize,
        right: usize,
    ) {
        if left > right {
            return;
        }
        if left == right {
            // Insert a node into the balanced tree.
            self.nodes[node] = input[left];
            return;
        }
        // Partition the input to left / right pieces of the same length to produce a balanced tree.
        let center = (left + right) / 2;
        self.partition_input(input, dimension, left, right, center);
        // Insert a node into the tree.
        self.nodes[node] = input[center];
        // Build up the left / right subtrees.
        let mut next_dimension = dimension + 1;
        if next_dimension == D {
            next_dimension = 0;
        }
        if center > left {
            self.build_recursive(input, node * 2 + 1, next_dimension, left, center - 1);
        }
        self.build_recursive(input, node * 2 + 2, next_dimension, center + 1, right);
    }

    /// `partition_input`: the quickselect of `AABBTreeIndirect`'s `partition_input`, over indices
    /// by their coordinate along `dimension`.
    fn partition_input(
        &self,
        input: &mut [usize],
        dimension: usize,
        mut left: usize,
        mut right: usize,
        k: usize,
    ) {
        let coord = |i: usize| (self.coordinate)(i, dimension);
        while left < right {
            let center = (left + right) / 2;
            let pivot;
            {
                // Bubble sort input[left], input[center], input[right], so that a median of the
                // three values will end up in input[center].
                let mut left_value = coord(input[left]);
                let mut center_value = coord(input[center]);
                let mut right_value = coord(input[right]);
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
                    if coord(input[i]).partial_cmp(&pivot) != Some(core::cmp::Ordering::Less) {
                        break;
                    }
                }
                // Skip right points that are already at correct positions.
                loop {
                    j -= 1;
                    let go_on = coord(input[j]) > pivot && i < j;
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

    /// `visit_recursive(node, dimension, visitor)`.
    fn visit_recursive<V: FnMut(usize, usize) -> u32>(
        &self,
        node: usize,
        dimension: usize,
        visitor: &mut V,
    ) {
        if node >= self.nodes.len() || self.nodes[node] == NPOS {
            return;
        }
        // Left / right child node index.
        let left = node * 2 + 1;
        let right = left + 1;
        let mask = visitor(self.nodes[node], dimension);
        if mask & VisitorReturnMask::Stop as u32 == 0 {
            let next_dimension = if dimension + 1 == D { 0 } else { dimension + 1 };
            if mask & VisitorReturnMask::ContinueLeft as u32 != 0 {
                self.visit_recursive(left, next_dimension, visitor);
            }
            if mask & VisitorReturnMask::ContinueRight as u32 != 0 {
                self.visit_recursive(right, next_dimension, visitor);
            }
        }
    }
}

/// `find_closest_points<K>(kdtree, point, filter)`: the `K` nearest points that `filter` keeps,
/// nearest first, [`NPOS`] where there are fewer. Euclidean metric.
pub fn find_closest_points<const K: usize, const D: usize, C, F, P, Filt>(
    kdtree: &KdTreeIndirect<D, C, F>,
    point: &P,
    mut filter: Filt,
) -> [usize; K]
where
    C: Real,
    F: Fn(usize, usize) -> C,
    P: Index<usize, Output = C> + ?Sized,
    Filt: FnMut(usize) -> bool,
{
    let mut results = [(NPOS, C::HIGHEST); K];
    let mut visitor = |idx: usize, dimension: usize| -> u32 {
        if filter(idx) {
            let mut dist = C::ZERO;
            for i in 0..D {
                let d = point[i].sub((kdtree.coordinate)(idx, i));
                dist = dist.add(d.mul(d));
            }
            let res = (idx, dist);
            let it = orca_stl::lower_bound_by(&results, &res, |r1, r2| r1.1 < r2.1);
            if it != K {
                results[it..].rotate_right(1);
                results[it] = res;
            }
        }
        kdtree.descent_mask(point[dimension], results[0].1, idx, dimension)
    };
    kdtree.visit(&mut visitor);
    core::array::from_fn(|i| results[i].0)
}

/// `find_closest_points<K>(kdtree, point)`: with no filter.
pub fn find_closest_points_all<const K: usize, const D: usize, C, F, P>(
    kdtree: &KdTreeIndirect<D, C, F>,
    point: &P,
) -> [usize; K]
where
    C: Real,
    F: Fn(usize, usize) -> C,
    P: Index<usize, Output = C> + ?Sized,
{
    find_closest_points::<K, D, C, F, P, _>(kdtree, point, |_| true)
}

/// `find_closest_point(kdtree, point, filter)`: the nearest point `filter` keeps, or [`NPOS`].
pub fn find_closest_point<const D: usize, C, F, P, Filt>(
    kdtree: &KdTreeIndirect<D, C, F>,
    point: &P,
    filter: Filt,
) -> usize
where
    C: Real,
    F: Fn(usize, usize) -> C,
    P: Index<usize, Output = C> + ?Sized,
    Filt: FnMut(usize) -> bool,
{
    find_closest_points::<1, D, C, F, P, Filt>(kdtree, point, filter)[0]
}

/// `find_closest_point(kdtree, point)`: with no filter.
pub fn find_closest_point_all<const D: usize, C, F, P>(
    kdtree: &KdTreeIndirect<D, C, F>,
    point: &P,
) -> usize
where
    C: Real,
    F: Fn(usize, usize) -> C,
    P: Index<usize, Output = C> + ?Sized,
{
    find_closest_point(kdtree, point, |_| true)
}

/// `find_nearby_points(kdtree, center, max_distance, filter)`: the points `filter` keeps whose
/// squared distance is below `max_distance * max_distance`, in the order the tree visits them.
pub fn find_nearby_points<const D: usize, C, F, P, Filt>(
    kdtree: &KdTreeIndirect<D, C, F>,
    center: &P,
    max_distance: C,
    mut filter: Filt,
) -> Vec<usize>
where
    C: Real,
    F: Fn(usize, usize) -> C,
    P: Index<usize, Output = C> + ?Sized,
    Filt: FnMut(usize) -> bool,
{
    let max_distance_squared = max_distance.mul(max_distance);
    let mut result = Vec::new();
    let mut visitor = |idx: usize, dimension: usize| -> u32 {
        if filter(idx) {
            let mut dist = C::ZERO;
            for i in 0..D {
                let d = center[i].sub((kdtree.coordinate)(idx, i));
                dist = dist.add(d.mul(d));
            }
            if dist < max_distance_squared {
                result.push(idx);
            }
        }
        kdtree.descent_mask(center[dimension], max_distance_squared, idx, dimension)
    };
    kdtree.visit(&mut visitor);
    result
}

/// `find_nearby_points(kdtree, center, max_distance)`: with no filter.
pub fn find_nearby_points_all<const D: usize, C, F, P>(
    kdtree: &KdTreeIndirect<D, C, F>,
    center: &P,
    max_distance: C,
) -> Vec<usize>
where
    C: Real,
    F: Fn(usize, usize) -> C,
    P: Index<usize, Output = C> + ?Sized,
{
    find_nearby_points(kdtree, center, max_distance, |_| true)
}

/// `find_nearby_points(kdtree, bb_min, bb_max, filter)`: the points `filter` keeps inside the box
/// (bounds included), in the order the tree visits them; a slot whose point the filter keeps
/// descends only toward the box along its dimension.
pub fn find_nearby_points_in_box<const D: usize, C, F, P, Filt>(
    kdtree: &KdTreeIndirect<D, C, F>,
    bb_min: &P,
    bb_max: &P,
    mut filter: Filt,
) -> Vec<usize>
where
    C: Real,
    F: Fn(usize, usize) -> C,
    P: Index<usize, Output = C> + ?Sized,
    Filt: FnMut(usize) -> bool,
{
    let mut result = Vec::new();
    let mut visitor = |idx: usize, dimension: usize| -> u32 {
        let mut ret = BOTH;
        if filter(idx) {
            let mut p = [C::ZERO; D];
            let mut contains = true;
            for (i, pi) in p.iter_mut().enumerate() {
                *pi = (kdtree.coordinate)(idx, i);
                contains = contains && bb_min[i] <= *pi && *pi <= bb_max[i];
            }
            if p[dimension] < bb_min[dimension] {
                ret = VisitorReturnMask::ContinueRight as u32;
            }
            if p[dimension] > bb_max[dimension] {
                ret = VisitorReturnMask::ContinueLeft as u32;
            }
            if contains {
                result.push(idx);
            }
        }
        ret
    };
    kdtree.visit(&mut visitor);
    result
}
