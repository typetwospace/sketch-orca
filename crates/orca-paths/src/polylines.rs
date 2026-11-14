// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/ShortestPath.cpp lines 1116-1996 (the code compiled in
// the release build) and src/libslic3r/ShortestPath.hpp lines 29-31 of OrcaSlicer v2.4.2,
// translated on 2026-11-13 by paths-1. Copyright the OrcaSlicer authors, the BambuStudio authors
// (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU Affero General Public
// License, version 3 (AGPL-3.0; LICENSE).

//! The chaining of polylines (infill lines and brim loops): [`chain_polylines`] orders them by
//! [`chain_segments_greedy2`], and, when no start point is given and there are two or more, lowers
//! the total length of the connecting moves by two-exchanges with segment flipping
//! ([`reorder_by_two_exchanges_with_segment_flipping`]).
//!
//! - A two-exchange cuts the sequence of polylines at two connections into three spans and tries
//!   each order of the spans (1-2-3, 1-3-2, 2-1-3), each span reversed or not and its polylines
//!   flipped or not (64 constellations each, [`minimum_crossover_cost`]); a reversed span of one
//!   polyline is skipped. The first cut is tried at the connections from the longest down (the
//!   lengths sorted by `std::sort`, so equal lengths go in introsort's order, through orca-stl),
//!   the second at every other one, and the first improvement found is applied; at most
//!   `min(n, 100)` rounds.
//! - After the exchanges a polyline is reversed when its edge's second point equals its first
//!   point, so a closed polyline (or a single point) is always reversed, as the C++ does.
//! - The costs add in the C++'s order: three span costs, then the two connections.
//!
//! Left out: the code the C++ compiles out (`#if 0`, or `DEBUG_SVG_OUTPUT` and the debug-only
//! checks): `svg_draw_polyline_chain`, `improve_ordering_by_segment_flipping`, the four-span
//! `minimum_crossover_cost` and `do_crossover`, `reorder_by_three_exchanges_with_segment_flipping`
//! and `reorder_by_three_exchanges_with_segment_flipping2` (and the `Matrixd` typedef only they
//! use). `FourOptCosts`, compiled but used only by that code, is here.

use core::ops::Sub;

use orca_eigen::Vec2d;
use orca_geom::point::Point;
use orca_geom::polyline::{Polyline, Polylines};

use crate::segments::chain_segments_greedy2;
use crate::stop::{EMPTY_POLYLINE, stop};

/// `FlipEdge`: a polyline's two end points and its index in the input.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlipEdge {
    /// `p1`.
    pub p1: Vec2d,
    /// `p2`.
    pub p2: Vec2d,
    /// `source_index`.
    pub source_index: usize,
}

impl FlipEdge {
    /// `FlipEdge(p1, p2, source_index)`.
    #[must_use]
    pub fn new(p1: Vec2d, p2: Vec2d, source_index: usize) -> Self {
        Self {
            p1,
            p2,
            source_index,
        }
    }

    /// `flip()`: the end points swapped.
    pub fn flip(&mut self) {
        core::mem::swap(&mut self.p1, &mut self.p2);
    }
}

/// `ConnectionCost`: the summed lengths of the connections of a run of edges, as they are and with
/// every edge flipped.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ConnectionCost {
    /// `cost`.
    pub cost: f64,
    /// `cost_flipped`.
    pub cost_flipped: f64,
}

impl ConnectionCost {
    /// `ConnectionCost(cost, cost_flipped)`.
    #[must_use]
    pub fn new(cost: f64, cost_flipped: f64) -> Self {
        Self { cost, cost_flipped }
    }

    /// `flip()`.
    pub fn flip(&mut self) {
        core::mem::swap(&mut self.cost, &mut self.cost_flipped);
    }
}

/// `operator-(lhs, rhs)`.
impl Sub for ConnectionCost {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        Self::new(self.cost - rhs.cost, self.cost_flipped - rhs.cost_flipped)
    }
}

/// A span of edges, `[first, second)`.
pub type Span = (usize, usize);

/// A span, its cost, and whether it is reversed and its edges flipped: one third of a
/// constellation of `minimum_crossover_cost`'s `connection_cost`.
#[derive(Clone, Copy)]
struct Piece<'a> {
    span: Span,
    cost: &'a ConnectionCost,
    reversed: bool,
    flipped: bool,
}

/// `first_point(span, flipped)`.
fn first_point(edges: &[FlipEdge], span: Span, flipped: bool) -> Vec2d {
    if flipped {
        edges[span.0].p2
    } else {
        edges[span.0].p1
    }
}

/// `last_point(span, flipped)`.
fn last_point(edges: &[FlipEdge], span: Span, flipped: bool) -> Vec2d {
    if flipped {
        edges[span.1 - 1].p1
    } else {
        edges[span.1 - 1].p2
    }
}

/// `point(span, start, flipped)`.
fn point(edges: &[FlipEdge], span: Span, start: bool, flipped: bool) -> Vec2d {
    if start {
        first_point(edges, span, flipped)
    } else {
        last_point(edges, span, flipped)
    }
}

/// `cost(acost, flipped)`.
fn cost(acost: &ConnectionCost, flipped: bool) -> f64 {
    if flipped {
        acost.cost_flipped
    } else {
        acost.cost
    }
}

/// `simple_span_ignore(span, reversed)`: a reversed span of a single segment.
fn simple_span_ignore(span: Span, reversed: bool) -> bool {
    span.0 + 1 == span.1 && reversed
}

/// `connection_cost(span1, cost1, reversed1, flipped1, ...)`: `DBL_MAX` when a span of one
/// segment is reversed, else the three costs and the two connections, added in that order.
fn connection_cost(edges: &[FlipEdge], s1: Piece<'_>, s2: Piece<'_>, s3: Piece<'_>) -> f64 {
    if simple_span_ignore(s1.span, s1.reversed)
        || simple_span_ignore(s2.span, s2.reversed)
        || simple_span_ignore(s3.span, s3.reversed)
    {
        // Don't perform unnecessary calculations simulating reversion of single segment spans.
        f64::MAX
    } else {
        // Calculate the cost of reverting chains and / or flipping segment orientations.
        cost(s1.cost, s1.flipped)
            + cost(s2.cost, s2.flipped)
            + cost(s3.cost, s3.flipped)
            + (point(edges, s2.span, !s2.reversed, s2.flipped)
                - point(edges, s1.span, s1.reversed, s1.flipped))
            .norm()
            + (point(edges, s3.span, !s3.reversed, s3.flipped)
                - point(edges, s2.span, s2.reversed, s2.flipped))
            .norm()
    }
}

/// The three pieces of constellation `i` over `(span, cost)` in the given order: bit 0 reverses
/// the first, bit 1 flips it, bits 2 and 3 the second, bits 4 and 5 the third.
fn pieces<'a>(
    i: usize,
    a: (Span, &'a ConnectionCost),
    b: (Span, &'a ConnectionCost),
    c: (Span, &'a ConnectionCost),
) -> [Piece<'a>; 3] {
    let bit = |k: usize| (i & (1 << k)) != 0;
    [
        Piece {
            span: a.0,
            cost: a.1,
            reversed: bit(0),
            flipped: bit(1),
        },
        Piece {
            span: b.0,
            cost: b.1,
            reversed: bit(2),
            flipped: bit(3),
        },
        Piece {
            span: c.0,
            cost: c.1,
            reversed: bit(4),
            flipped: bit(5),
        },
    ]
}

/// `minimum_crossover_cost(edges, span1, cost1, span2, cost2, span3, cost3, cost_current)`: the
/// lowest cost of the 3 orders times 64 constellations, below `cost_current`, and its code
/// (`i + (order << 6)`; 0 for no improvement).
#[must_use]
pub fn minimum_crossover_cost(
    edges: &[FlipEdge],
    piece1: (Span, &ConnectionCost),
    piece2: (Span, &ConnectionCost),
    piece3: (Span, &ConnectionCost),
    cost_current: f64,
) -> (f64, usize) {
    let mut cost_min = cost_current;
    // no flip, no improvement
    let mut flip_min = 0;
    for i in 0..(1usize << 6) {
        // From the three combinations of 1,2,3 ordering, the other three are reversals of the
        // first three.
        let c1 = if i == 0 {
            cost_current
        } else {
            let [x, y, z] = pieces(i, piece1, piece2, piece3);
            connection_cost(edges, x, y, z)
        };
        let [x, y, z] = pieces(i, piece1, piece3, piece2);
        let c2 = connection_cost(edges, x, y, z);
        let [x, y, z] = pieces(i, piece2, piece1, piece3);
        let c3 = connection_cost(edges, x, y, z);
        if c1 < cost_min {
            cost_min = c1;
            flip_min = i;
        }
        if c2 < cost_min {
            cost_min = c2;
            flip_min = i + (1 << 6);
        }
        if c3 < cost_min {
            cost_min = c3;
            flip_min = i + (2 << 6);
        }
    }
    (cost_min, flip_min)
}

/// `do_crossover(edges_in, edges_out, span1, span2, span3, i)`: `edges_out` filled with the three
/// spans in constellation `i`'s order, each reversed (`std::reverse_copy`) or copied, and its edges
/// flipped when reversed and flipped differ.
pub fn do_crossover(
    edges_in: &[FlipEdge],
    edges_out: &mut [FlipEdge],
    span1: Span,
    span2: Span,
    span3: Span,
    i: usize,
) {
    let bit = |k: usize| (i & (1 << k)) != 0;
    let (a, b, c) = match i >> 6 {
        0 => (span1, span2, span3),
        1 => (span1, span3, span2),
        _ => (span2, span1, span3),
    };
    let mut it_edges_out = 0;
    for (span, reversed, flipped) in [
        (a, bit(0), bit(1)),
        (b, bit(2), bit(3)),
        (c, bit(4), bit(5)),
    ] {
        let it = it_edges_out;
        let len = span.1 - span.0;
        if reversed {
            for (k, edge) in edges_in[span.0..span.1].iter().rev().enumerate() {
                edges_out[it_edges_out + k] = *edge;
            }
        } else {
            edges_out[it_edges_out..it_edges_out + len].copy_from_slice(&edges_in[span.0..span.1]);
        }
        it_edges_out += len;
        if reversed != flipped {
            for edge in &mut edges_out[it..it_edges_out] {
                edge.flip();
            }
        }
    }
}

/// `reorder_by_two_exchanges_with_segment_flipping(edges)`: up to `min(n, 100)` rounds, each
/// applying the first two-exchange that lowers the summed connection length, trying the first cut
/// at the connections from the longest down.
pub fn reorder_by_two_exchanges_with_segment_flipping(edges: &mut Vec<FlipEdge>) {
    if edges.len() < 2 {
        return;
    }

    let mut connections = vec![ConnectionCost::default(); edges.len()];
    let mut edges_tmp = edges.clone();
    let mut connection_lengths: Vec<(f64, usize)> = vec![(0., 0); edges.len() - 1];
    let mut connection_tried = vec![false; edges.len()];
    let max_iterations = orca_stl::min(edges.len(), 100usize);
    for _iter in 0..max_iterations {
        // Initialize connection costs and connection lengths.
        for i in 1..edges.len() {
            let e1 = &edges[i - 1];
            let e2 = &edges[i];
            let mut c = connections[i - 1];
            let l = (e2.p1 - e1.p2).norm();
            c.cost += l;
            c.cost_flipped += (e2.p2 - e1.p1).norm();
            connections[i] = c;
            connection_lengths[i - 1] = (l, i);
        }
        orca_stl::sort_by(&mut connection_lengths, |l, r| l.0 > r.0);
        connection_tried.fill(false);
        let mut crossover1_pos_final = usize::MAX;
        let mut crossover2_pos_final = usize::MAX;
        let mut crossover_flip_final = 0;
        let total = connections[connections.len() - 1];
        for first_crossover_candidate in &connection_lengths {
            let longest_connection_idx = first_crossover_candidate.1;
            connection_tried[longest_connection_idx] = true;
            // Find the second crossover connection with the lowest total chain cost.
            let mut crossover_pos_min = usize::MAX;
            let mut crossover_cost_min = total.cost;
            let mut crossover_flip_min = 0;
            #[allow(clippy::needless_range_loop)] // `j` is the C++'s index into the connections
            for j in 1..connections.len() {
                if !connection_tried[j] {
                    let mut a = j;
                    let mut b = longest_connection_idx;
                    if a > b {
                        core::mem::swap(&mut a, &mut b);
                    }
                    let cost_and_flip = minimum_crossover_cost(
                        edges,
                        ((0, a), &connections[a - 1]),
                        ((a, b), &(connections[b - 1] - connections[a])),
                        ((b, edges.len()), &(total - connections[b])),
                        total.cost,
                    );
                    if cost_and_flip.1 > 0 && cost_and_flip.0 < crossover_cost_min {
                        crossover_pos_min = j;
                        crossover_cost_min = cost_and_flip.0;
                        crossover_flip_min = cost_and_flip.1;
                    }
                }
            }
            if crossover_cost_min < total.cost {
                // The cost of the chain with the proposed two crossovers has a lower total cost
                // than the current chain. Apply the crossover.
                crossover1_pos_final = longest_connection_idx;
                crossover2_pos_final = crossover_pos_min;
                crossover_flip_final = crossover_flip_min;
                break;
            }
            // Continue with another long candidate edge.
        }
        if crossover_flip_final > 0 {
            // Pair of cross over positions and flip / reverse constellation has been found, which
            // improves the total cost of the connection. Perform a crossover.
            if crossover1_pos_final > crossover2_pos_final {
                core::mem::swap(&mut crossover1_pos_final, &mut crossover2_pos_final);
            }
            do_crossover(
                edges,
                &mut edges_tmp,
                (0, crossover1_pos_final),
                (crossover1_pos_final, crossover2_pos_final),
                (crossover2_pos_final, edges.len()),
                crossover_flip_final,
            );
            core::mem::swap(edges, &mut edges_tmp);
        } else {
            // No valid pair of cross over positions was found improving the total cost. Giving
            // up.
            break;
        }
    }
}

/// `FourOptCosts`: the costs of four pieces, read as they are or flipped (compiled in the C++ but
/// used only by its compiled-out three-exchange).
#[derive(Clone, Copy, Debug)]
pub struct FourOptCosts<'a> {
    costs: [&'a ConnectionCost; 4],
}

impl<'a> FourOptCosts<'a> {
    /// `FourOptCosts(c1, c2, c3, c4)`.
    #[must_use]
    pub fn new(
        c1: &'a ConnectionCost,
        c2: &'a ConnectionCost,
        c3: &'a ConnectionCost,
        c4: &'a ConnectionCost,
    ) -> Self {
        Self {
            costs: [c1, c2, c3, c4],
        }
    }

    /// `operator()(piece_idx, flipped)`.
    #[must_use]
    pub fn cost(&self, piece_idx: usize, flipped: bool) -> f64 {
        if flipped {
            self.costs[piece_idx].cost_flipped
        } else {
            self.costs[piece_idx].cost
        }
    }
}

/// A polyline's first point, stopping where the C++ reads one of an empty polyline.
fn first_of(function: &'static str, pl: &Polyline) -> Point {
    match pl.points.first() {
        Some(&p) => p,
        None => stop(function, EMPTY_POLYLINE),
    }
}

/// A polyline's last point, stopping where the C++ reads one of an empty polyline.
fn last_of(function: &'static str, pl: &Polyline) -> Point {
    match pl.points.last() {
        Some(&p) => p,
        None => stop(function, EMPTY_POLYLINE),
    }
}

/// `improve_ordering_by_two_exchanges_with_segment_flipping(polylines, fixed_start)`: the
/// polylines reordered and flipped to lower the total length of the connecting lines; used by the
/// infill generator when the infill is not connected with the perimeters, and to order the brim
/// lines. `fixed_start` is read only by the C++'s debug checks.
pub fn improve_ordering_by_two_exchanges_with_segment_flipping(
    polylines: &mut Polylines,
    _fixed_start: bool,
) {
    const F: &str = "improve_ordering_by_two_exchanges_with_segment_flipping";
    let mut edges = Vec::with_capacity(polylines.len());
    for (i, pl) in polylines.iter().enumerate() {
        edges.push(FlipEdge::new(
            first_of(F, pl).cast_f64(),
            last_of(F, pl).cast_f64(),
            i,
        ));
    }
    reorder_by_two_exchanges_with_segment_flipping(&mut edges);
    let mut out: Polylines = Vec::with_capacity(polylines.len());
    for edge in &edges {
        let pl = core::mem::take(&mut polylines[edge.source_index]);
        out.push(pl);
        let back = out.len() - 1;
        if edge.p2 == first_of(F, &out[back]).cast_f64() {
            // Polyline is flipped.
            out[back].reverse();
        }
        // Polyline is not flipped.
    }
    *polylines = out;
}

/// `chain_polylines(Polylines &&polylines, start_near)`: the order of infill lines and brim lines.
/// [`chain_segments_greedy2`] chains them; with no start point and two or more, the two-exchanges
/// improve the order. The C++ reads the end points of each polyline only where the chaining asks
/// for them, and stops here where it would read those of an empty one.
#[must_use]
pub fn chain_polylines(mut polylines: Polylines, start_near: Option<&Point>) -> Polylines {
    let mut out = Vec::new();
    if !polylines.is_empty() {
        let ordered = {
            let pls = &polylines;
            let segment_end_point = |idx: usize, first_point: bool| -> Point {
                if first_point {
                    first_of("chain_polylines", &pls[idx])
                } else {
                    last_of("chain_polylines", &pls[idx])
                }
            };
            chain_segments_greedy2(segment_end_point, polylines.len(), start_near)
        };
        out.reserve(polylines.len());
        for segment_and_reversal in &ordered {
            out.push(core::mem::take(&mut polylines[segment_and_reversal.0]));
            if segment_and_reversal.1 {
                let back = out.len() - 1;
                out[back].reverse();
            }
        }
        if out.len() > 1 && start_near.is_none() {
            improve_ordering_by_two_exchanges_with_segment_flipping(&mut out, start_near.is_some());
        }
    }
    out
}

/// `chain_polylines(const Polylines &src, start_near)`: a copy chained.
#[must_use]
pub fn chain_polylines_copy(src: &[Polyline], start_near: Option<&Point>) -> Polylines {
    let tmp = src.to_vec();
    chain_polylines(tmp, start_near)
}
