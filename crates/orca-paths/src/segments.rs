// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/ShortestPath.cpp lines 1-19 and 42-1025 of OrcaSlicer
// v2.4.2, translated on 2026-11-13 by paths-1. Copyright the OrcaSlicer authors, the BambuStudio
// authors (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU Affero General
// Public License, version 3 (AGPL-3.0; LICENSE).

//! ShortestPath's generic segment chaining: the templates every other chaining of the engine
//! instantiates. A segment is an index with two end points, given by `end_point_func(idx,
//! first_point)`; a chain is the segments' order, each with whether it is walked reversed.
//!
//! - [`chain_segments_closest_point`]: always the nearest free end point next, the fallback when
//!   reversal constraints defeat the greedy chaining.
//! - [`chain_segments_greedy_constrained_reversals_`]: the multi-fragment greedy chaining. A KD
//!   tree over the end points (two per segment, `2 * idx` the first) finds each one's nearest end
//!   point of another segment; a mutable priority queue keyed by the squared distance takes the
//!   shortest link that makes neither a fork nor a loop (`EquivalentChains` tracks which
//!   fragments are joined), refreshing a link whose target was taken. With `REVERSE_COULD_FAIL` a
//!   chain that walks a segment reversed which may not reverse is tried from its other end (when
//!   no start point was given), then given up for the closest-point chaining.
//! - [`chain_segments_greedy_constrained_reversals2_`]: the second version, which can also flip
//!   whole fragments (the infill lines' order): links are keyed by their length plus the flip
//!   penalty of each fragment they flip, each fragment keeps its cost both ways round, and the
//!   loop stops after `16 * num_segments` rounds.
//! - The wrappers [`chain_segments_greedy_constrained_reversals`], [`chain_segments_greedy`] (no
//!   constraint), [`chain_segments_greedy_constrained_reversals2`] and [`chain_segments_greedy2`].
//!
//! What the translation keeps of the C++:
//!
//! - Ties go as the C++'s: the KD tree keeps the first of equally near points it visits
//!   (orca-index's `find_closest_point`), the queue's sifts are `MutablePriorityQueue`'s, and the
//!   end points are pushed in index order. The first version's queue keeps a popped point's heap
//!   index (`ResetIndexWhenRemoved` false), the second's resets it.
//! - With one segment the first version reverses it when its last point is nearer `start_near`
//!   and it may reverse; the second reverses it when its first point is nearer, whatever the
//!   segment allows: the comparison is the other way round (`parity/HAZARDS.md`).
//! - The second version's loop may end by its limit before it has joined every segment; with a
//!   start point the walk then returns the chain from it, shorter than `num_segments`, as the C++
//!   does (its assert is not in the release build). Without one it stops ([`crate::stop`]).
//! - Pointers become indices: an end point's `edge_out` and `edge_candidate` and a chain's
//!   `begin` and `end` are indices into the end points, `NIL` for null; the fields the queue's
//!   index setter and predicate read while the loop changes them are [`Cell`]s.
//!
//! Left out: the debug-only checks (`EquivalentChains::validate`, `Chains::validate` and
//! `validate_graph_and_queue`, compiled only without `NDEBUG`).

use core::cell::Cell;

use orca_eigen::Vec2d;
use orca_geom::point::{Point, SCALED_EPSILON};
use orca_index::kd_tree::{self as kd, KdTreeIndirect, NPOS};
use orca_index::priority_queue::{MutablePriorityQueue, make_mutable_priority_queue};

use crate::stop::{NO_END_POINT, NO_FIRST_POINT, stop};

/// A null `EndPoint*`.
const NIL: usize = usize::MAX;

/// `std::numeric_limits<size_t>::max()`, a heap index of no slot.
const NO_HEAP_IDX: usize = usize::MAX;

/// A chain: each segment's index and whether it is walked reversed.
pub type Chain = Vec<(usize, bool)>;

/// What `chain_segments_closest_point` and the final walk read of an end point, which both
/// versions' `EndPoint` structs have.
trait ChainEndPoint {
    fn pos(&self) -> &Vec2d;
    fn chain_id(&self) -> &Cell<usize>;
    fn edge_out(&self) -> usize;
}

/// The first version's `EndPoint`.
struct EndPoint {
    pos: Vec2d,
    /// Identifier of the chain, to which this end point belongs. Zero means unassigned.
    chain_id: Cell<usize>,
    /// Link to the closest currently valid end point.
    edge_out: Cell<usize>,
    /// Distance to the next end point following the link. Zero value -> start of the final path.
    distance_out: Cell<f64>,
    heap_idx: Cell<usize>,
}

impl EndPoint {
    fn new(pos: Vec2d) -> Self {
        Self {
            pos,
            chain_id: Cell::new(0),
            edge_out: Cell::new(NIL),
            distance_out: Cell::new(f64::MAX),
            heap_idx: Cell::new(NO_HEAP_IDX),
        }
    }
}

impl ChainEndPoint for EndPoint {
    fn pos(&self) -> &Vec2d {
        &self.pos
    }
    fn chain_id(&self) -> &Cell<usize> {
        &self.chain_id
    }
    fn edge_out(&self) -> usize {
        self.edge_out.get()
    }
}

/// The KD tree over the end points: `coordinate_fn(idx, dimension)` is the end point's position.
fn end_point_tree<E: ChainEndPoint>(
    end_points: &[E],
) -> KdTreeIndirect<2, f64, impl Fn(usize, usize) -> f64 + '_> {
    KdTreeIndirect::with_count(
        move |idx: usize, dimension: usize| end_points[idx].pos()[dimension],
        end_points.len(),
    )
}

/// `find_closest_point(kdtree, point, filter)`, stopping where the C++ would take
/// `&end_points[npos]`.
fn closest<F, Filt>(
    function: &'static str,
    kdtree: &KdTreeIndirect<2, f64, F>,
    point: &Vec2d,
    filter: Filt,
) -> usize
where
    F: Fn(usize, usize) -> f64,
    Filt: FnMut(usize) -> bool,
{
    let idx = kd::find_closest_point(kdtree, point, filter);
    if idx == NPOS {
        stop(function, NO_END_POINT);
    }
    idx
}

/// `chain_segments_closest_point(end_points, kdtree, could_reverse_func, first_point)`: the naive
/// traveling salesman, always taking the next closest end point, which gives a valid result even
/// if some segments cannot reverse. `first_point` is the index of the end point to start from.
fn chain_segments_closest_point<E, F, R>(
    end_points: &[E],
    kdtree: &KdTreeIndirect<2, f64, F>,
    could_reverse_func: &mut R,
    first_point: usize,
) -> Chain
where
    E: ChainEndPoint,
    F: Fn(usize, usize) -> f64,
    R: FnMut(usize) -> bool,
{
    let num_segments = end_points.len() / 2;
    for ep in end_points {
        ep.chain_id().set(0);
    }
    let mut out = Vec::with_capacity(num_segments);
    let first_point_idx = first_point;
    out.push((first_point_idx / 2, (first_point_idx & 1) != 0));
    end_points[first_point].chain_id().set(1);
    let mut this_idx = first_point_idx ^ 1;
    let mut iter = num_segments as i32 - 2;
    while iter >= 0 {
        let this_point = &end_points[this_idx];
        this_point.chain_id().set(1);
        // Find the closest point to this end_point, which lies on a different extrusion path
        // (filtered by the lambda). Ignore the starting point as the starting point is considered
        // to be occupied, no end point coud connect to it.
        let next_idx = closest(
            "chain_segments_closest_point",
            kdtree,
            this_point.pos(),
            |idx| {
                (idx ^ this_idx) > 1
                    && end_points[idx].chain_id().get() == 0
                    && ((idx & 1) == 0 || could_reverse_func(idx >> 1))
            },
        );
        let end_point = &end_points[next_idx];
        end_point.chain_id().set(1);
        out.push((next_idx / 2, (next_idx & 1) != 0));
        this_idx = next_idx ^ 1;
        iter -= 1;
    }
    out
}

/// The first version's `EquivalentChains`: unique chain IDs are assigned to paths; when paths are
/// connected, the end points keep their chain IDs, and each ID remembers an "equivalent" one, the
/// lowest ID of the path, which is equivalent to itself.
struct EquivalentChains {
    /// Unique chain ID assigned to chains of end points of segments.
    last_chain_id: usize,
    equivalent_with: Vec<usize>,
}

impl EquivalentChains {
    /// Zero'th chain ID is invalid.
    fn new(reserve: usize) -> Self {
        let mut equivalent_with = Vec::with_capacity(reserve);
        equivalent_with.push(0);
        Self {
            last_chain_id: 0,
            equivalent_with,
        }
    }

    /// `next()`: the next equivalence class.
    fn next(&mut self) -> usize {
        self.last_chain_id += 1;
        self.equivalent_with.push(self.last_chain_id);
        self.last_chain_id
    }

    /// `operator()(chain_id)`: the equivalence class of a chain ID, remembered for that ID.
    fn get(&mut self, mut chain_id: usize) -> usize {
        if chain_id != 0 {
            let mut last = chain_id;
            loop {
                let lower = self.equivalent_with[last];
                if lower == last {
                    self.equivalent_with[chain_id] = lower;
                    chain_id = lower;
                    break;
                }
                last = lower;
            }
        }
        chain_id
    }

    /// `merge(chain_id1, chain_id2)`.
    fn merge(&mut self, chain_id1: usize, chain_id2: usize) -> usize {
        let a = self.get(chain_id1);
        let b = self.get(chain_id2);
        let chain_id = orca_stl::min(a, b);
        self.equivalent_with[chain_id1] = chain_id;
        self.equivalent_with[chain_id2] = chain_id;
        chain_id
    }
}

/// `chain_segments_greedy_constrained_reversals_<PointType, SegmentEndPointFunc,
/// REVERSE_COULD_FAIL, CouldReverseFunc>(end_point_func, could_reverse_func, num_segments,
/// start_near)`: perimeters (always closed) and thin fills (closed or open) chained by the
/// multi-fragment greedy algorithm, a shortest link added when it makes neither a fork nor a loop
/// (<https://en.wikipedia.org/wiki/Multi-fragment_algorithm>). Returns each segment's index and
/// whether it is reversed.
pub fn chain_segments_greedy_constrained_reversals_<const REVERSE_COULD_FAIL: bool, E, R>(
    mut end_point_func: E,
    mut could_reverse_func: R,
    num_segments: usize,
    start_near: Option<&Point>,
) -> Chain
where
    E: FnMut(usize, bool) -> Point,
    R: FnMut(usize) -> bool,
{
    let mut out = Vec::new();

    if num_segments == 0 {
        // Nothing to do.
    } else if num_segments == 1 {
        // Just sort the end points so that the first point visited is closest to start_near.
        let reverse = could_reverse_func(0)
            && match start_near {
                Some(start) => {
                    (end_point_func(0, false) - *start)
                        .cast_f64()
                        .squared_norm()
                        < (end_point_func(0, true) - *start).cast_f64().squared_norm()
                }
                None => false,
            };
        out.push((0, reverse));
    } else {
        // End points of segments for the KD tree closest point search.
        let mut points = Vec::with_capacity(num_segments * 2);
        for i in 0..num_segments {
            points.push(EndPoint::new(end_point_func(i, true).cast_f64()));
            points.push(EndPoint::new(end_point_func(i, false).cast_f64()));
        }
        let end_points = &points[..];

        // Construct the closest point KD tree over end points of segments.
        let kdtree = end_point_tree(end_points);

        let mut equivalent_chain = EquivalentChains::new(num_segments);

        // Find the first end point closest to start_near.
        let mut first_point = NIL;
        let mut first_point_idx = usize::MAX;
        if let Some(start) = start_near {
            // Don't start with a reverse segment, if flipping of the segment is not allowed.
            let idx = closest(
                "chain_segments_greedy_constrained_reversals_",
                &kdtree,
                &start.cast_f64(),
                |idx| (idx & 1) == 0 || could_reverse_func(idx >> 1),
            );
            first_point = idx;
            end_points[idx].distance_out.set(0.);
            end_points[idx].chain_id.set(equivalent_chain.next());
            first_point_idx = idx;
        }
        let initial_point = first_point;
        // Set by the last iteration, the loop's only way out.
        let last_point;

        // Assign the closest point and distance to the end points.
        for (this_idx, end_point) in end_points.iter().enumerate() {
            if this_idx != first_point {
                // Find the closest point to this end_point, which lies on a different extrusion
                // path (filtered by the lambda). Ignore the starting point as the starting point
                // is considered to be occupied, no end point coud connect to it.
                let next_idx = closest(
                    "chain_segments_greedy_constrained_reversals_",
                    &kdtree,
                    &end_point.pos,
                    |idx| idx != first_point_idx && (idx ^ this_idx) > 1,
                );
                let end_point2 = &end_points[next_idx];
                end_point.edge_out.set(next_idx);
                end_point
                    .distance_out
                    .set((end_point2.pos - end_point.pos).squared_norm());
            }
        }

        // Initialize a heap of end points sorted by the lowest distance to the next valid point
        // of a path.
        let mut queue = make_mutable_priority_queue::<usize, false, _, _>(
            |ep: &mut usize, idx: usize| end_points[*ep].heap_idx.set(idx),
            |l: &usize, r: &usize| {
                end_points[*l].distance_out.get() < end_points[*r].distance_out.get()
            },
        );
        queue.reserve(end_points.len() * 2 - 1);
        for ep in 0..end_points.len() {
            if first_point != ep {
                queue.push(ep);
            }
        }

        // Chain the end points: find (num_segments - 1) shortest links not forming bifurcations
        // or loops.
        let mut iter = num_segments as i32 - 2;
        loop {
            // Take the first end point, for which the link points to the currently closest valid
            // neighbor.
            let end_point1 = *queue.top();
            // Take the closest end point to the first end point,
            let end_point2 = end_points[end_point1].edge_out.get();
            let mut valid = true;
            let mut end_point1_other_chain_id = 0;
            let mut end_point2_other_chain_id = 0;
            if end_points[end_point2].chain_id.get() > 0 {
                // The other side is part of the output path. Don't connect to end_point2, update
                // end_point1 and try another one.
                valid = false;
            } else {
                // End points of the opposite ends of the segments.
                end_point1_other_chain_id =
                    equivalent_chain.get(end_points[end_point1 ^ 1].chain_id.get());
                end_point2_other_chain_id =
                    equivalent_chain.get(end_points[end_point2 ^ 1].chain_id.get());
                if end_point1_other_chain_id == end_point2_other_chain_id
                    && end_point1_other_chain_id != 0
                {
                    // This edge forms a loop. Update end_point1 and try another one.
                    valid = false;
                }
            }
            if valid {
                // Remove the first and second point from the queue.
                queue.pop();
                queue.remove(end_points[end_point2].heap_idx.get());
                end_points[end_point2].edge_out.set(end_point1);
                end_points[end_point2]
                    .distance_out
                    .set(end_points[end_point1].distance_out.get());
                // Assign chain IDs to the newly connected end points, set equivalent_chain if two
                // chains were merged.
                #[allow(clippy::if_same_then_else)] // the C++'s branches, kept one for one
                let chain_id = if end_point1_other_chain_id == 0 {
                    if end_point2_other_chain_id == 0 {
                        equivalent_chain.next()
                    } else {
                        end_point2_other_chain_id
                    }
                } else if end_point2_other_chain_id == 0 {
                    end_point1_other_chain_id
                } else if end_point1_other_chain_id == end_point2_other_chain_id {
                    end_point1_other_chain_id
                } else {
                    equivalent_chain.merge(end_point1_other_chain_id, end_point2_other_chain_id)
                };
                end_points[end_point1].chain_id.set(chain_id);
                end_points[end_point2].chain_id.set(chain_id);
                if iter == 0 {
                    // Last iteration. There shall be exactly one or two end points waiting to be
                    // connected.
                    if first_point == NIL {
                        first_point = *queue.top();
                        queue.pop();
                        end_points[first_point].edge_out.set(NIL);
                    }
                    last_point = *queue.top();
                    end_points[last_point].edge_out.set(NIL);
                    queue.pop();
                    break;
                }
            } else {
                // This edge forms a loop. Update end_point1 and try another one.
                iter += 1;
                end_points[end_point1].edge_out.set(NIL);
                // Update edge_out and distance.
                let this_idx = end_point1;
                // Find the closest point to this end_point, which lies on a different extrusion
                // path (filtered by the filter lambda).
                let next_idx = closest(
                    "chain_segments_greedy_constrained_reversals_",
                    &kdtree,
                    &end_points[end_point1].pos,
                    |idx| {
                        if (idx ^ this_idx) <= 1 || end_points[idx].chain_id.get() != 0 {
                            // Points of the same segment shall not be connected, cannot connect
                            // to an already connected point (ideally those would be removed
                            // from the KD tree, but the update is difficult).
                            return false;
                        }
                        let chain1 = equivalent_chain.get(end_points[this_idx ^ 1].chain_id.get());
                        let chain2 = equivalent_chain.get(end_points[idx ^ 1].chain_id.get());
                        chain1 != chain2 || chain1 == 0
                    },
                );
                end_points[end_point1].edge_out.set(next_idx);
                end_points[end_point1]
                    .distance_out
                    .set((end_points[next_idx].pos - end_points[end_point1].pos).squared_norm());
                // Update position of this end point in the queue based on the distance calculated
                // at the line above.
                queue.update(end_points[end_point1].heap_idx.get());
            }
            iter -= 1;
        }
        drop(queue);

        // Now interconnect pairs of segments into a chain.
        out.reserve(num_segments);
        let mut failed = walk::<REVERSE_COULD_FAIL, _>(
            "chain_segments_greedy_constrained_reversals_",
            end_points,
            first_point,
            &mut could_reverse_func,
            &mut out,
        );
        if REVERSE_COULD_FAIL {
            if failed && start_near.is_none() {
                // We may try the reverse order.
                out.clear();
                failed = walk::<true, _>(
                    "chain_segments_greedy_constrained_reversals_",
                    end_points,
                    last_point,
                    &mut could_reverse_func,
                    &mut out,
                );
            }
            if failed {
                // As a last resort, try a dumb algorithm, which is not sensitive to edge reversal
                // constraints.
                out = chain_segments_closest_point(
                    end_points,
                    &kdtree,
                    &mut could_reverse_func,
                    if initial_point != NIL {
                        initial_point
                    } else {
                        0
                    },
                );
            }
        }
    }

    out
}

/// The walk of both versions from `first_point` along the links (`do { ... } while (first_point !=
/// nullptr)`): each segment once, reversed when it is entered at its second end point. With
/// `CHECK` (the C++'s `REVERSE_COULD_FAIL`) a reversed segment that may not reverse ends it; it
/// answers whether that happened. A null `first_point` stops ([`NO_FIRST_POINT`]) in `function`.
fn walk<const CHECK: bool, E: ChainEndPoint>(
    function: &'static str,
    end_points: &[E],
    mut first_point: usize,
    could_reverse_func: &mut impl FnMut(usize) -> bool,
    out: &mut Chain,
) -> bool {
    if first_point == NIL {
        stop(function, NO_FIRST_POINT);
    }
    loop {
        let first_point_id = first_point;
        let segment_id = first_point_id >> 1;
        let reverse = (first_point_id & 1) != 0;
        let second_point = first_point_id ^ 1;
        if CHECK && reverse && !could_reverse_func(segment_id) {
            return true;
        }
        out.push((segment_id, reverse));
        first_point = end_points[second_point].edge_out();
        if first_point == NIL {
            return false;
        }
    }
}

/// The second version's `EndPoint`.
struct EndPoint2 {
    pos: Vec2d,
    /// Candidate for a new connection link.
    edge_candidate: Cell<usize>,
    /// Distance to the next end point following the link. Zero value -> start of the final path.
    distance_out: Cell<f64>,
    heap_idx: Cell<usize>,
    /// Identifier of the chain, to which this end point belongs. Zero means unassigned.
    chain_id: Cell<usize>,
    /// Double linked chain of segment end points in current path.
    edge_out: Cell<usize>,
}

impl EndPoint2 {
    fn new(pos: Vec2d) -> Self {
        Self {
            pos,
            edge_candidate: Cell::new(NIL),
            distance_out: Cell::new(f64::MAX),
            heap_idx: Cell::new(NO_HEAP_IDX),
            chain_id: Cell::new(0),
            edge_out: Cell::new(NIL),
        }
    }

    /// `heap_idx_invalid()`.
    fn heap_idx_invalid(&self) -> bool {
        self.heap_idx.get() == NO_HEAP_IDX
    }
}

impl ChainEndPoint for EndPoint2 {
    fn pos(&self) -> &Vec2d {
        &self.pos
    }
    fn chain_id(&self) -> &Cell<usize> {
        &self.chain_id
    }
    fn edge_out(&self) -> usize {
        self.edge_out.get()
    }
}

/// The second version's `Chain`: chained segments with their sum of connection lengths, both
/// ways round. The chain supports flipping all the segments, connecting the segments at the
/// opposite ends (a very useful path optimization for infill lines).
#[derive(Clone, Copy, Debug)]
struct SegmentChain {
    num_segments: usize,
    cost: f64,
    cost_flipped: f64,
    begin: usize,
    end: usize,
    equivalent_with: usize,
}

impl Default for SegmentChain {
    fn default() -> Self {
        Self {
            num_segments: 0,
            cost: 0.,
            cost_flipped: 0.,
            begin: NIL,
            end: NIL,
            equivalent_with: 0,
        }
    }
}

impl SegmentChain {
    /// `flip(endpoints)`: the chain walked the other way round; O(n).
    fn flip(&mut self, endpoints: &[EndPoint2]) {
        // Start of the current segment processed.
        let mut ept = self.begin;
        // Previous end point to connect the other side of ept to.
        let mut ept_prev = NIL;
        loop {
            let ept_end = ept ^ 1;
            let ept_next = endpoints[ept_end].edge_out.get();
            // Connect to the preceding segment.
            endpoints[ept_end].edge_out.set(ept_prev);
            if ept_prev != NIL {
                endpoints[ept_prev].edge_out.set(ept_end);
            }
            ept_prev = ept;
            ept = ept_next;
            if ept == NIL {
                break;
            }
        }
        endpoints[ept_prev].edge_out.set(NIL);
        // Swap the costs.
        core::mem::swap(&mut self.cost, &mut self.cost_flipped);
        // Swap the ends.
        let new_begin = self.begin ^ 1;
        let new_end = self.end ^ 1;
        swap_chain_id(endpoints, self.begin, new_begin);
        swap_chain_id(endpoints, self.end, new_end);
        self.begin = new_begin;
        self.end = new_end;
    }

    /// `flip_penalty()`.
    fn flip_penalty(&self) -> f64 {
        self.cost_flipped - self.cost
    }
}

/// `std::swap(a->chain_id, b->chain_id)`.
fn swap_chain_id(endpoints: &[EndPoint2], a: usize, b: usize) {
    let t = endpoints[a].chain_id.get();
    endpoints[a].chain_id.set(endpoints[b].chain_id.get());
    endpoints[b].chain_id.set(t);
}

/// The second version's `Chains`: detects loops in already connected paths and accommodates the
/// flipping of chains. Chain IDs start with 1; each remembers an equivalent ID, the lowest of its
/// path, and its costs both ways round.
struct Chains {
    chains: Vec<SegmentChain>,
    /// Unique chain ID assigned to chains of end points of segments.
    last_chain_id: usize,
}

impl Chains {
    /// Zero'th chain ID is invalid.
    fn new(reserve: usize) -> Self {
        let mut chains = Vec::with_capacity(reserve / 2);
        // Indexing starts with 1.
        chains.push(SegmentChain::default());
        Self {
            chains,
            last_chain_id: 0,
        }
    }

    /// `next_id()`: the next equivalence class.
    fn next_id(&mut self) -> usize {
        self.last_chain_id += 1;
        self.chains.push(SegmentChain {
            equivalent_with: self.last_chain_id,
            ..SegmentChain::default()
        });
        self.last_chain_id
    }

    /// `equivalent(chain_id)`: the equivalence class of a chain ID, updating its
    /// `equivalent_with`.
    fn equivalent(&mut self, mut chain_id: usize) -> usize {
        if chain_id != 0 {
            let mut last = chain_id;
            loop {
                let lower = self.chains[last].equivalent_with;
                if lower == last {
                    self.chains[chain_id].equivalent_with = lower;
                    chain_id = lower;
                    break;
                }
                last = lower;
            }
        }
        chain_id
    }

    /// `merge(chain_id1, chain_id2)`: the lower of the two chain IDs, or a new one when both are
    /// zero.
    fn merge(&mut self, chain_id1: usize, chain_id2: usize) -> usize {
        if chain_id1 == 0 {
            return if chain_id2 == 0 {
                self.next_id()
            } else {
                chain_id2
            };
        }
        if chain_id2 == 0 {
            return chain_id1;
        }
        let chain_id = orca_stl::min(chain_id1, chain_id2);
        self.chains[chain_id1].equivalent_with = chain_id;
        self.chains[chain_id2].equivalent_with = chain_id;
        chain_id
    }

    /// `chain_flip_penalty(chain_id)`.
    fn chain_flip_penalty(&mut self, chain_id: usize) -> f64 {
        let chain_id = self.equivalent(chain_id);
        self.chains[chain_id].flip_penalty()
    }
}

/// `update_end_point_in_queue(queue, kdtree, chains, end_points, end_point, first_point_idx,
/// first_point)`: a new candidate link for an end point or for the second point from a chain's
/// end, or its removal from the queue when it would flip the first segment.
fn update_end_point_in_queue<S, L, F>(
    queue: &mut MutablePriorityQueue<usize, S, L, true>,
    kdtree: &KdTreeIndirect<2, f64, F>,
    chains: &mut Chains,
    end_points: &[EndPoint2],
    end_point: usize,
    first_point_idx: usize,
    first_point: usize,
) where
    S: FnMut(&mut usize, usize),
    L: FnMut(&usize, &usize) -> bool,
    F: Fn(usize, usize) -> f64,
{
    // Updating an end point or a 2nd from an end point.
    let this_idx = end_point;
    let ep = &end_points[end_point];
    ep.edge_candidate.set(NIL);
    if first_point_idx == this_idx || (ep.chain_id.get() > 0 && first_point_idx == (this_idx ^ 1)) {
        // One may never flip the 1st edge, don't try it again.
        if !ep.heap_idx_invalid() {
            queue.remove(ep.heap_idx.get());
        }
    } else {
        // Update edge_candidate and distance.
        let chain1a = ep.chain_id.get();
        let chain1b = end_points[this_idx ^ 1].chain_id.get();
        let this_chain = chains.equivalent(orca_stl::max(chain1a, chain1b));
        // Find the closest point to this end_point, which lies on a different extrusion path
        // (filtered by the filter lambda).
        let next_idx = closest("update_end_point_in_queue", kdtree, &ep.pos, |idx| {
            if (idx ^ this_idx) <= 1 || idx == first_point_idx {
                // Points of the same segment shall not be connected. Don't connect to the first
                // point, we must not flip the 1st edge.
                return false;
            }
            let chain2a = end_points[idx].chain_id.get();
            let chain2b = end_points[idx ^ 1].chain_id.get();
            if chain2a > 0 && chain2b > 0 {
                // Only unconnected end point or a point next to an unconnected end point may be
                // connected to. Ideally those would be removed from the KD tree, but the update
                // is difficult.
                return false;
            }
            let chain2 = chains.equivalent(orca_stl::max(chain2a, chain2b));
            if this_chain == chain2 {
                // Don't connect back to the same chain, don't create a loop.
                return this_chain == 0;
            }
            // Don't connect to a segment requiring flipping if the segment starts or ends with
            // the first point.
            if chain2a > 0 {
                // Chain requires flipping.
                let chain = &chains.chains[chain2];
                if chain.begin == first_point || chain.end == first_point {
                    return false;
                }
            }
            // Everything is all right, try to connect.
            true
        });
        ep.edge_candidate.set(next_idx);
        ep.distance_out
            .set((end_points[next_idx].pos - ep.pos).norm());
        if ep.chain_id.get() > 0 {
            ep.distance_out
                .set(ep.distance_out.get() + chains.chain_flip_penalty(this_chain));
        }
        if end_points[next_idx].chain_id.get() > 0 {
            // The candidate chain is flipped.
            let penalty = chains.chain_flip_penalty(end_points[next_idx].chain_id.get());
            ep.distance_out.set(ep.distance_out.get() + penalty);
        }
        // Update position of this end point in the queue based on the distance calculated at
        // the line above.
        if ep.heap_idx_invalid() {
            queue.push(end_point);
        } else {
            queue.update(ep.heap_idx.get());
        }
    }
}

/// `chain_segments_greedy_constrained_reversals2_<PointType, SegmentEndPointFunc,
/// REVERSE_COULD_FAIL, CouldReverseFunc>(end_point_func, could_reverse_func, num_segments,
/// start_near)`: the greedy chaining that may also flip whole chains, links keyed by their length
/// plus the flip penalties of the chains they flip.
pub fn chain_segments_greedy_constrained_reversals2_<const REVERSE_COULD_FAIL: bool, E, R>(
    mut end_point_func: E,
    mut could_reverse_func: R,
    num_segments: usize,
    start_near: Option<&Point>,
) -> Chain
where
    E: FnMut(usize, bool) -> Point,
    R: FnMut(usize) -> bool,
{
    let mut out = Vec::new();

    if num_segments == 0 {
        // Nothing to do.
    } else if num_segments == 1 {
        // Just sort the end points so that the first point visited is closest to start_near.
        let reverse = match start_near {
            Some(start) => {
                (end_point_func(0, true) - *start).cast_f64().squared_norm()
                    < (end_point_func(0, false) - *start)
                        .cast_f64()
                        .squared_norm()
            }
            None => false,
        };
        out.push((0, reverse));
    } else {
        // End points of segments for the KD tree closest point search.
        let mut points = Vec::with_capacity(num_segments * 2);
        for i in 0..num_segments {
            points.push(EndPoint2::new(end_point_func(i, true).cast_f64()));
            points.push(EndPoint2::new(end_point_func(i, false).cast_f64()));
        }
        let end_points = &points[..];

        // Construct the closest point KD tree over end points of segments.
        let kdtree = end_point_tree(end_points);

        let mut chains = Chains::new(num_segments);

        // Find the first end point closest to start_near.
        let mut first_point = NIL;
        let mut first_point_idx = usize::MAX;
        if let Some(start) = start_near {
            let idx = closest(
                "chain_segments_greedy_constrained_reversals2_",
                &kdtree,
                &start.cast_f64(),
                |_| true,
            );
            first_point = idx;
            end_points[idx].distance_out.set(0.);
            end_points[idx].chain_id.set(chains.next_id());
            let chain = &mut chains.chains[end_points[idx].chain_id.get()];
            chain.begin = first_point;
            chain.end = first_point ^ 1;
            first_point_idx = idx;
        }
        let initial_point = first_point;
        let mut last_point = NIL;

        // Assign the closest point and distance to the end points.
        for (this_idx, end_point) in end_points.iter().enumerate() {
            if this_idx != first_point {
                // Find the closest point to this end_point, which lies on a different extrusion
                // path (filtered by the lambda). Ignore the starting point as the starting point
                // is considered to be occupied, no end point coud connect to it.
                let next_idx = closest(
                    "chain_segments_greedy_constrained_reversals2_",
                    &kdtree,
                    &end_point.pos,
                    |idx| idx != first_point_idx && (idx ^ this_idx) > 1,
                );
                let end_point2 = &end_points[next_idx];
                end_point.edge_candidate.set(next_idx);
                end_point
                    .distance_out
                    .set((end_point2.pos - end_point.pos).norm());
            }
        }

        // Initialize a heap of end points sorted by the lowest distance to the next valid point
        // of a path.
        let mut queue = make_mutable_priority_queue::<usize, true, _, _>(
            |ep: &mut usize, idx: usize| end_points[*ep].heap_idx.set(idx),
            |l: &usize, r: &usize| {
                end_points[*l].distance_out.get() < end_points[*r].distance_out.get()
            },
        );
        queue.reserve(end_points.len() * 2);
        for ep in 0..end_points.len() {
            if first_point != ep {
                queue.push(ep);
            }
        }

        // Chain the end points: find (num_segments - 1) shortest links not forming bifurcations
        // or loops. Some links stored onto the priority queue are being invalidated during the
        // calculation and they are not updated immediately. If such a situation is detected for
        // an end point pulled from the priority queue, the end point is being updated and
        // re-inserted into the priority queue. Therefore the number of iterations required is
        // higher than expected (it would be the number of links, num_segments - 1). The limit
        // here may not be necessary, but it guards us against an endless loop if something goes
        // wrong.
        let mut num_iter = num_segments * 16;
        let mut num_connections_to_end = num_segments - 1;
        while num_iter > 0 {
            // Take the first end point, for which the link points to the currently closest valid
            // neighbor.
            let end_point1 = *queue.top();
            let end_point1_other = end_point1 ^ 1;
            // true if end_point1 is not the end of its chain, but the 2nd point. When connecting
            // to the 2nd point, this chain needs to be flipped first.
            let chain1_flip = end_points[end_point1].chain_id.get() > 0;
            // Take the closest end point to the first end point,
            let end_point2 = end_points[end_point1].edge_candidate.get();
            let end_point2_other = end_point2 ^ 1;
            let chain2_flip = end_points[end_point2].chain_id.get() > 0;
            // Is the link from end_point1 to end_point2 still valid? If yes, the link may be
            // taken. Otherwise the link needs to be refreshed.
            let mut valid = true;
            let mut end_point1_chain_id = 0;
            let mut end_point2_chain_id = 0;
            if end_points[end_point2].chain_id.get() > 0
                && end_points[end_point2_other].chain_id.get() > 0
            {
                // The other side is part of the output path. Don't connect to end_point2, update
                // end_point1 and try another one.
                valid = false;
            } else {
                // End points of the opposite ends of the segments.
                end_point1_chain_id = chains.equivalent(
                    end_points[if chain1_flip {
                        end_point1
                    } else {
                        end_point1_other
                    }]
                    .chain_id
                    .get(),
                );
                end_point2_chain_id = chains.equivalent(
                    end_points[if chain2_flip {
                        end_point2
                    } else {
                        end_point2_other
                    }]
                    .chain_id
                    .get(),
                );
                if end_point1_chain_id == end_point2_chain_id && end_point1_chain_id != 0 {
                    // This edge forms a loop. Update end_point1 and try another one.
                    valid = false;
                } else {
                    // Verify whether end_point1.distance_out still matches the current state of
                    // the two end points to be connected and their chains. Namely, the other
                    // chain may have been flipped in the meantime.
                    let mut dist = (end_points[end_point2].pos - end_points[end_point1].pos).norm();
                    if chain1_flip {
                        dist += chains.chain_flip_penalty(end_point1_chain_id);
                    }
                    if chain2_flip {
                        dist += chains.chain_flip_penalty(end_point2_chain_id);
                    }
                    if orca_crt::fabs(dist - end_points[end_point1].distance_out.get())
                        > SCALED_EPSILON
                    {
                        // The distance changed due to flipping of one of the chains. Refresh this
                        // end point in the queue.
                        valid = false;
                    }
                }
                if valid && first_point != NIL {
                    // Verify that a chain starting or ending with the first_point does not get
                    // flipped.
                    if chain1_flip {
                        let chain = &chains.chains[end_point1_chain_id];
                        if chain.begin == first_point || chain.end == first_point {
                            valid = false;
                        }
                    }
                    if valid && chain2_flip {
                        let chain = &chains.chains[end_point2_chain_id];
                        if chain.begin == first_point || chain.end == first_point {
                            valid = false;
                        }
                    }
                }
            }
            if valid {
                // Remove the first and second point from the queue.
                queue.pop();
                queue.remove(end_points[end_point2].heap_idx.get());
                end_points[end_point1].edge_candidate.set(NIL);
                let chain1 = end_point1_chain_id;
                let chain2 = end_point2_chain_id;
                if chain1_flip {
                    chains.chains[chain1].flip(end_points);
                }
                if chain2_flip {
                    chains.chains[chain2].flip(end_points);
                }
                let chain_id = chains.merge(end_point1_chain_id, end_point2_chain_id);
                {
                    let c1 = (chain1 != 0).then(|| chains.chains[chain1]);
                    let c2 = (chain2 != 0).then(|| chains.chains[chain2]);
                    let pos = |i: usize| end_points[i].pos;
                    let chain_dst = SegmentChain {
                        begin: match c1 {
                            None => end_point1_other,
                            Some(c) if c.begin == end_point1 => c.end,
                            Some(c) => c.begin,
                        },
                        end: match c2 {
                            None => end_point2_other,
                            Some(c) if c.begin == end_point2 => c.end,
                            Some(c) => c.begin,
                        },
                        cost: c1.map_or(0., |c| c.cost)
                            + c2.map_or(0., |c| c.cost)
                            + (pos(end_point2) - pos(end_point1)).norm(),
                        cost_flipped: c1.map_or(0., |c| c.cost_flipped)
                            + c2.map_or(0., |c| c.cost_flipped)
                            + (pos(end_point2_other) - pos(end_point1_other)).norm(),
                        num_segments: c1.map_or(1, |c| c.num_segments)
                            + c2.map_or(1, |c| c.num_segments),
                        equivalent_with: chain_id,
                    };
                    chains.chains[chain_id] = chain_dst;
                }
                let chain = chains.chains[chain_id];
                if chain.begin != end_point1_other
                    && !end_points[end_point1_other].heap_idx_invalid()
                {
                    queue.remove(end_points[end_point1_other].heap_idx.get());
                }
                if chain.end != end_point2_other && !end_points[end_point2_other].heap_idx_invalid()
                {
                    queue.remove(end_points[end_point2_other].heap_idx.get());
                }
                end_points[end_point1].edge_out.set(end_point2);
                end_points[end_point2].edge_out.set(end_point1);
                end_points[end_point1].chain_id.set(chain_id);
                end_points[end_point2].chain_id.set(chain_id);
                end_points[end_point1_other].chain_id.set(chain_id);
                end_points[end_point2_other].chain_id.set(chain_id);
                if chain.begin != first_point {
                    end_points[chain.begin].chain_id.set(0);
                }
                if chain.end != first_point {
                    end_points[chain.end].chain_id.set(0);
                }
                num_connections_to_end -= 1;
                if num_connections_to_end == 0 {
                    // Last iteration. There shall be exactly one or two end points waiting to be
                    // connected.
                    if first_point == NIL {
                        // Find the first remaining end point.
                        loop {
                            first_point = *queue.top();
                            queue.pop();
                            if end_points[first_point].edge_out.get() == NIL {
                                break;
                            }
                        }
                    }
                    // Find the first remaining end point.
                    loop {
                        last_point = *queue.top();
                        queue.pop();
                        if end_points[last_point].edge_out.get() == NIL {
                            break;
                        }
                    }
                    break;
                }
                // Update end points of the flipped segments.
                let (begin, end) = (chain.begin, chain.end);
                for (go, end_point) in [
                    (true, begin ^ 1),
                    (true, end ^ 1),
                    (chain1_flip, begin),
                    (chain2_flip, end),
                ] {
                    if go {
                        update_end_point_in_queue(
                            &mut queue,
                            &kdtree,
                            &mut chains,
                            end_points,
                            end_point,
                            first_point_idx,
                            first_point,
                        );
                    }
                }
            } else {
                // This edge forms a loop. Update end_point1 and try another one.
                update_end_point_in_queue(
                    &mut queue,
                    &kdtree,
                    &mut chains,
                    end_points,
                    end_point1,
                    first_point_idx,
                    first_point,
                );
            }
            num_iter -= 1;
        }
        drop(queue);

        // Now interconnect pairs of segments into a chain.
        out.reserve(num_segments);
        let mut failed = walk::<REVERSE_COULD_FAIL, _>(
            "chain_segments_greedy_constrained_reversals2_",
            end_points,
            first_point,
            &mut could_reverse_func,
            &mut out,
        );
        if REVERSE_COULD_FAIL {
            if failed && start_near.is_none() {
                // We may try the reverse order.
                out.clear();
                failed = walk::<true, _>(
                    "chain_segments_greedy_constrained_reversals2_",
                    end_points,
                    last_point,
                    &mut could_reverse_func,
                    &mut out,
                );
            }
            if failed {
                // As a last resort, try a dumb algorithm, which is not sensitive to edge reversal
                // constraints.
                out = chain_segments_closest_point(
                    end_points,
                    &kdtree,
                    &mut could_reverse_func,
                    if initial_point != NIL {
                        initial_point
                    } else {
                        0
                    },
                );
            }
        }
    }

    out
}

/// `chain_segments_greedy_constrained_reversals(end_point_func, could_reverse_func, num_segments,
/// start_near)`: the first version, reversals refused where `could_reverse_func` says so.
pub fn chain_segments_greedy_constrained_reversals<E, R>(
    end_point_func: E,
    could_reverse_func: R,
    num_segments: usize,
    start_near: Option<&Point>,
) -> Chain
where
    E: FnMut(usize, bool) -> Point,
    R: FnMut(usize) -> bool,
{
    chain_segments_greedy_constrained_reversals_::<true, E, R>(
        end_point_func,
        could_reverse_func,
        num_segments,
        start_near,
    )
}

/// `chain_segments_greedy(end_point_func, num_segments, start_near)`: the first version, every
/// segment free to reverse.
pub fn chain_segments_greedy<E>(
    end_point_func: E,
    num_segments: usize,
    start_near: Option<&Point>,
) -> Chain
where
    E: FnMut(usize, bool) -> Point,
{
    let could_reverse_func = |_idx: usize| -> bool { true };
    chain_segments_greedy_constrained_reversals_::<false, E, _>(
        end_point_func,
        could_reverse_func,
        num_segments,
        start_near,
    )
}

/// `chain_segments_greedy_constrained_reversals2(end_point_func, could_reverse_func, num_segments,
/// start_near)`: the second version with reversals refused (no caller in the engine).
pub fn chain_segments_greedy_constrained_reversals2<E, R>(
    end_point_func: E,
    could_reverse_func: R,
    num_segments: usize,
    start_near: Option<&Point>,
) -> Chain
where
    E: FnMut(usize, bool) -> Point,
    R: FnMut(usize) -> bool,
{
    chain_segments_greedy_constrained_reversals2_::<true, E, R>(
        end_point_func,
        could_reverse_func,
        num_segments,
        start_near,
    )
}

/// `chain_segments_greedy2(end_point_func, num_segments, start_near)`: the second version, every
/// segment free to reverse.
pub fn chain_segments_greedy2<E>(
    end_point_func: E,
    num_segments: usize,
    start_near: Option<&Point>,
) -> Chain
where
    E: FnMut(usize, bool) -> Point,
{
    let could_reverse_func = |_idx: usize| -> bool { true };
    chain_segments_greedy_constrained_reversals2_::<false, E, _>(
        end_point_func,
        could_reverse_func,
        num_segments,
        start_near,
    )
}
