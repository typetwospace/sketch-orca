// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Clipper code: class ClipperBase, class Clipper and their helpers in
// deps_src/clipper/clipper.cpp and clipper.hpp of OrcaSlicer v2.4.2 (Clipper 6.4.2 with the
// PrusaSlicer authors' changes), translated on 2026-11-07. Copyright Angus Johnson 2010-2017 and
// the PrusaSlicer authors, under the Boost Software License, version 1.0 (BSL-1.0;
// licenses/BSL-1.0.txt).

//! `ClipperBase` and `Clipper`: Vatti's clipping with Clipper's local minima, active and sorted
//! edge lists, output records and joins.
//!
//! The C++ links edges, output points and output records by pointer. Here each lives in a vector
//! and links by index, [`NIL`] for the null pointer. Output points are allocated as the C++
//! allocates them: a recycled point from the free list first, else the next new one, so that an
//! index stands for the same object the C++ address does. Each `AddPath` appends its edges to the
//! one edge vector, where the C++ keeps an array per call.
//!
//! The orders that decide results go where the C++ sends them: the sort of the local minima and of
//! the intersections (`std::sort`) and the sort of the maxima through `orca-stl`, the scanbeam
//! through `orca-stl`'s `priority_queue`. The slope tests take `Int128`'s filtered determinant
//! once a coordinate passes `loRange`.

use super::{
    ClipType, ClipperException, HI_RANGE, IO_PRESERVE_COLLINEAR, IO_REVERSE_SOLUTION,
    IO_STRICTLY_SIMPLE, IntPoint, IntRect, LO_RANGE, Path, Paths, PolyFillType, PolyTree, PolyType,
    ROOT, cross_from, round_cint,
};
use crate::int128;
use crate::point::Point;
use orca_stl::PriorityQueue;

/// The null pointer, for an index into the edges, the output points or the output records.
const NIL: u32 = u32::MAX;
/// `Unassigned`: an edge that owns no output record.
const UNASSIGNED: i32 = -1;
/// `Skip`: an edge that would otherwise close an open path.
const SKIP: i32 = -2;
/// `HORIZONTAL`: the `Dx` of a horizontal edge.
const HORIZONTAL: f64 = -1.0e40;
/// `esLeft`.
const ES_LEFT: u8 = 1;
/// `esRight`.
const ES_RIGHT: u8 = 2;

/// `Direction`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    RightToLeft,
    LeftToRight,
}

/// `TEdge`.
#[derive(Clone, Copy, Debug)]
struct TEdge {
    /// `Bot`: the end with the larger y.
    bot: IntPoint,
    /// `Curr`.
    curr: IntPoint,
    /// `Top`: the end with the smaller y.
    top: IntPoint,
    /// `Dx`: dx/dy, or `HORIZONTAL`.
    dx: f64,
    poly_typ: PolyType,
    /// `Side`: 0 as `memset` leaves it, then `ES_LEFT` or `ES_RIGHT`.
    side: u8,
    wind_delta: i32,
    wind_cnt: i32,
    wind_cnt2: i32,
    out_idx: i32,
    next: u32,
    prev: u32,
    next_in_lml: u32,
    next_in_ael: u32,
    prev_in_ael: u32,
    next_in_sel: u32,
    prev_in_sel: u32,
}

impl TEdge {
    /// An edge as `memset(e, 0, sizeof(TEdge))` leaves it, the pointers null.
    const ZERO: Self = Self {
        bot: Point::new(0, 0),
        curr: Point::new(0, 0),
        top: Point::new(0, 0),
        dx: 0.0,
        poly_typ: PolyType::Subject,
        side: 0,
        wind_delta: 0,
        wind_cnt: 0,
        wind_cnt2: 0,
        out_idx: 0,
        next: NIL,
        prev: NIL,
        next_in_lml: NIL,
        next_in_ael: NIL,
        prev_in_ael: NIL,
        next_in_sel: NIL,
        prev_in_sel: NIL,
    };
}

/// `IntersectNode`.
#[derive(Clone, Copy, Debug)]
struct IntersectNode {
    edge1: u32,
    edge2: u32,
    pt: IntPoint,
}

/// `LocalMinimum`.
#[derive(Clone, Copy, Debug)]
struct LocalMinimum {
    y: i64,
    left_bound: u32,
    right_bound: u32,
}

/// `OutPt`.
#[derive(Clone, Copy, Debug, Default)]
struct OutPt {
    idx: i32,
    pt: IntPoint,
    next: u32,
    prev: u32,
}

/// `OutRec`.
#[derive(Clone, Copy, Debug)]
struct OutRec {
    idx: i32,
    is_hole: bool,
    is_open: bool,
    first_left: u32,
    /// `PolyNd`: the node `BuildResult2` made, as a node index (`NIL` for none).
    poly_nd: u32,
    pts: u32,
    bottom_pt: u32,
}

/// `Join`.
#[derive(Clone, Copy, Debug)]
struct Join {
    out_pt1: u32,
    out_pt2: u32,
    off_pt: IntPoint,
}

/// `Clipper` (with its base class `ClipperBase`).
pub struct Clipper {
    // ClipperBase.
    /// `m_MinimaList`.
    minima_list: Vec<LocalMinimum>,
    /// `m_UseFullRange`.
    use_full_range: bool,
    /// `m_edges`, every path's edges in one vector.
    edges: Vec<TEdge>,
    /// `m_PreserveCollinear`.
    preserve_collinear: bool,
    /// `m_HasOpenPaths`.
    has_open_paths: bool,
    // Clipper.
    /// `m_PolyOuts`.
    poly_outs: Vec<OutRec>,
    /// `m_OutPts`, its chunks laid end to end.
    out_pts: Vec<OutPt>,
    /// `m_OutPtsFree`.
    out_pts_free: u32,
    /// `m_Joins`.
    joins: Vec<Join>,
    /// `m_GhostJoins`.
    ghost_joins: Vec<Join>,
    /// `m_IntersectList`.
    intersect_list: Vec<IntersectNode>,
    /// `m_ClipType`.
    clip_type: ClipType,
    /// `m_Scanbeam`.
    scanbeam: PriorityQueue<i64>,
    /// `m_Maxima`.
    maxima: Vec<i64>,
    /// `m_ActiveEdges`.
    active_edges: u32,
    /// `m_SortedEdges`.
    sorted_edges: u32,
    /// `m_ClipFillType`.
    clip_fill_type: PolyFillType,
    /// `m_SubjFillType`.
    subj_fill_type: PolyFillType,
    /// `m_ReverseOutput`.
    reverse_output: bool,
    /// `m_UsingPolyTree`.
    using_poly_tree: bool,
    /// `m_StrictSimple`.
    strict_simple: bool,
}

impl Default for Clipper {
    fn default() -> Self {
        Self::new(0)
    }
}

/// `IsHorizontal`.
fn is_horizontal(e: &TEdge) -> bool {
    e.dx == HORIZONTAL
}

/// `GetDx`.
fn get_dx(pt1: IntPoint, pt2: IntPoint) -> f64 {
    if pt1.y == pt2.y {
        HORIZONTAL
    } else {
        pt2.x.wrapping_sub(pt1.x) as f64 / pt2.y.wrapping_sub(pt1.y) as f64
    }
}

/// `TopX`: the edge's x at `current_y`.
fn top_x(edge: &TEdge, current_y: i64) -> i64 {
    if current_y == edge.top.y {
        edge.top.x
    } else {
        edge.bot.x.wrapping_add(round_cint(
            edge.dx * current_y.wrapping_sub(edge.bot.y) as f64,
        ))
    }
}

/// `SlopesEqual(dx1, dy1, dx2, dy2, UseFullInt64Range)`.
fn slopes_equal(dx1: i64, dy1: i64, dx2: i64, dy2: i64, use_full_range: bool) -> bool {
    if use_full_range {
        int128::sign_determinant_2x2_filtered(dx1, dy1, dx2, dy2) == 0
    } else {
        dy1.wrapping_mul(dx2) == dx1.wrapping_mul(dy2)
    }
}

/// `SlopesEqual(const TEdge&, const TEdge&, bool)`.
fn slopes_equal_edges(e1: &TEdge, e2: &TEdge, use_full_range: bool) -> bool {
    slopes_equal(
        e1.top.x.wrapping_sub(e1.bot.x),
        e1.top.y.wrapping_sub(e1.bot.y),
        e2.top.x.wrapping_sub(e2.bot.x),
        e2.top.y.wrapping_sub(e2.bot.y),
        use_full_range,
    )
}

/// `SlopesEqual(pt1, pt2, pt3, bool)`.
fn slopes_equal3(pt1: IntPoint, pt2: IntPoint, pt3: IntPoint, use_full_range: bool) -> bool {
    slopes_equal(
        pt1.x.wrapping_sub(pt2.x),
        pt1.y.wrapping_sub(pt2.y),
        pt2.x.wrapping_sub(pt3.x),
        pt2.y.wrapping_sub(pt3.y),
        use_full_range,
    )
}

/// `SlopesEqual(pt1, pt2, pt3, pt4, bool)`.
fn slopes_equal4(
    pt1: IntPoint,
    pt2: IntPoint,
    pt3: IntPoint,
    pt4: IntPoint,
    use_full_range: bool,
) -> bool {
    slopes_equal(
        pt1.x.wrapping_sub(pt2.x),
        pt1.y.wrapping_sub(pt2.y),
        pt3.x.wrapping_sub(pt4.x),
        pt3.y.wrapping_sub(pt4.y),
        use_full_range,
    )
}

/// `IntersectPoint`: where the two edges cross, kept within the scanbeam.
fn intersect_point(edge1: &TEdge, edge2: &TEdge) -> IntPoint {
    let mut ip = Point::new(0, 0);
    // ClipperLib_Z: ip.z() = 0 here.
    if edge1.dx == edge2.dx {
        ip.y = edge1.curr.y;
        ip.x = top_x(edge1, ip.y);
        return ip;
    }
    let mut y: i64;
    if edge1.dx == 0.0 {
        ip.x = edge1.bot.x;
        y = if is_horizontal(edge2) {
            edge2.bot.y
        } else {
            round_cint(
                ip.x as f64 / edge2.dx + edge2.bot.y as f64 - (edge2.bot.x as f64 / edge2.dx),
            )
        };
    } else if edge2.dx == 0.0 {
        ip.x = edge2.bot.x;
        y = if is_horizontal(edge1) {
            edge1.bot.y
        } else {
            round_cint(
                ip.x as f64 / edge1.dx + edge1.bot.y as f64 - (edge1.bot.x as f64 / edge1.dx),
            )
        };
    } else {
        let b1 = edge1.bot.x as f64 - edge1.bot.y as f64 * edge1.dx;
        let b2 = edge2.bot.x as f64 - edge2.bot.y as f64 * edge2.dx;
        let q = (b2 - b1) / (edge1.dx - edge2.dx);
        y = round_cint(q);
        ip.x = if orca_crt::fabs(edge1.dx) < orca_crt::fabs(edge2.dx) {
            round_cint(edge1.dx * q + b1)
        } else {
            round_cint(edge2.dx * q + b2)
        };
    }
    ip.y = y;
    if y < edge1.top.y || y < edge2.top.y {
        ip.y = if edge1.top.y > edge2.top.y {
            edge1
        } else {
            edge2
        }
        .top
        .y;
        y = ip.y;
        let e = if orca_crt::fabs(edge1.dx) < orca_crt::fabs(edge2.dx) {
            edge1
        } else {
            edge2
        };
        ip.x = top_x(e, ip.y);
    }
    // Don't allow 'ip' to be below curr.y (the bottom of the scanbeam).
    if y > edge1.curr.y {
        ip.y = edge1.curr.y;
        // The more vertical edge gives x.
        let e = if orca_crt::fabs(edge1.dx) > orca_crt::fabs(edge2.dx) {
            edge2
        } else {
            edge1
        };
        ip.x = top_x(e, ip.y);
    }
    ip
}

/// `InitEdge2`: orders the ends and computes `Dx`.
fn init_edge2(e: &mut TEdge, next_curr: IntPoint, pt: PolyType) {
    if e.curr.y >= next_curr.y {
        e.bot = e.curr;
        e.top = next_curr;
    } else {
        e.top = e.curr;
        e.bot = next_curr;
    }
    let dy = e.top.y.wrapping_sub(e.bot.y);
    e.dx = if dy == 0 {
        HORIZONTAL
    } else {
        e.top.x.wrapping_sub(e.bot.x) as f64 / dy as f64
    };
    e.poly_typ = pt;
}

/// `ReverseHorizontal`: swaps the ends' x so that the edge follows its bound.
fn reverse_horizontal(e: &mut TEdge) {
    core::mem::swap(&mut e.top.x, &mut e.bot.x);
    // ClipperLib_Z: the ends' z are swapped too.
}

/// `Pt2IsBetweenPt1AndPt3`.
fn pt2_is_between_pt1_and_pt3(pt1: IntPoint, pt2: IntPoint, pt3: IntPoint) -> bool {
    if pt1 == pt3 || pt1 == pt2 || pt3 == pt2 {
        false
    } else if pt1.x != pt3.x {
        (pt2.x > pt1.x) == (pt2.x < pt3.x)
    } else {
        (pt2.y > pt1.y) == (pt2.y < pt3.y)
    }
}

/// `HorzSegmentsOverlap`.
fn horz_segments_overlap(mut seg1a: i64, mut seg1b: i64, mut seg2a: i64, mut seg2b: i64) -> bool {
    if seg1a > seg1b {
        core::mem::swap(&mut seg1a, &mut seg1b);
    }
    if seg2a > seg2b {
        core::mem::swap(&mut seg2a, &mut seg2b);
    }
    (seg1a < seg2b) && (seg2a < seg1b)
}

/// `RangeTest`: switches to the full range past `loRange`, and throws past `hiRange`.
fn range_test(pt: IntPoint, use_full_range: &mut bool) -> Result<(), ClipperException> {
    if *use_full_range {
        if pt.x > HI_RANGE
            || pt.y > HI_RANGE
            || pt.x.wrapping_neg() > HI_RANGE
            || pt.y.wrapping_neg() > HI_RANGE
        {
            return Err(ClipperException::COORDINATE_OUTSIDE_ALLOWED_RANGE);
        }
    } else if pt.x > LO_RANGE
        || pt.y > LO_RANGE
        || pt.x.wrapping_neg() > LO_RANGE
        || pt.y.wrapping_neg() > LO_RANGE
    {
        *use_full_range = true;
        return range_test(pt, use_full_range);
    }
    Ok(())
}

/// `GetHorzDirection`.
fn get_horz_direction(e: &TEdge) -> (Direction, i64, i64) {
    if e.bot.x < e.top.x {
        (Direction::LeftToRight, e.bot.x, e.top.x)
    } else {
        (Direction::RightToLeft, e.top.x, e.bot.x)
    }
}

/// `E2InsertsBeforeE1`.
fn e2_inserts_before_e1(e1: &TEdge, e2: &TEdge) -> bool {
    if e2.curr.x == e1.curr.x {
        if e2.top.y > e1.top.y {
            e2.top.x < top_x(e1, e2.top.y)
        } else {
            e1.top.x > top_x(e2, e1.top.y)
        }
    } else {
        e2.curr.x < e1.curr.x
    }
}

/// `GetOverlap`: the overlap of two ranges, if any.
fn get_overlap(a1: i64, a2: i64, b1: i64, b2: i64) -> Option<(i64, i64)> {
    let (left, right) = if a1 < a2 {
        if b1 < b2 {
            (orca_stl::max(a1, b1), orca_stl::min(a2, b2))
        } else {
            (orca_stl::max(a1, b2), orca_stl::min(a2, b1))
        }
    } else if b1 < b2 {
        (orca_stl::max(a2, b1), orca_stl::min(a1, b2))
    } else {
        (orca_stl::max(a2, b2), orca_stl::min(a1, b1))
    };
    (left < right).then_some((left, right))
}

/// An edge's index in the edge vector.
fn ix(i: usize) -> u32 {
    i as u32
}

impl Clipper {
    /// `Clipper(initOptions)`: `ioReverseSolution`, `ioStrictlySimple`, `ioPreserveCollinear`.
    #[must_use]
    pub fn new(init_options: i32) -> Self {
        Self {
            minima_list: Vec::new(),
            use_full_range: false,
            edges: Vec::new(),
            preserve_collinear: (init_options & IO_PRESERVE_COLLINEAR) != 0,
            has_open_paths: false,
            poly_outs: Vec::new(),
            out_pts: Vec::new(),
            out_pts_free: NIL,
            joins: Vec::new(),
            ghost_joins: Vec::new(),
            intersect_list: Vec::new(),
            clip_type: ClipType::Intersection,
            scanbeam: PriorityQueue::new(),
            maxima: Vec::new(),
            active_edges: NIL,
            sorted_edges: NIL,
            clip_fill_type: PolyFillType::EvenOdd,
            subj_fill_type: PolyFillType::EvenOdd,
            reverse_output: (init_options & IO_REVERSE_SOLUTION) != 0,
            using_poly_tree: false,
            strict_simple: (init_options & IO_STRICTLY_SIMPLE) != 0,
        }
    }

    /// `PreserveCollinear()`.
    #[must_use]
    pub fn preserve_collinear(&self) -> bool {
        self.preserve_collinear
    }

    /// `PreserveCollinear(value)`.
    pub fn set_preserve_collinear(&mut self, value: bool) {
        self.preserve_collinear = value;
    }

    /// `ReverseSolution()`.
    #[must_use]
    pub fn reverse_solution(&self) -> bool {
        self.reverse_output
    }

    /// `ReverseSolution(value)`.
    pub fn set_reverse_solution(&mut self, value: bool) {
        self.reverse_output = value;
    }

    /// `StrictlySimple()`.
    #[must_use]
    pub fn strictly_simple(&self) -> bool {
        self.strict_simple
    }

    /// `StrictlySimple(value)`.
    pub fn set_strictly_simple(&mut self, value: bool) {
        self.strict_simple = value;
    }

    fn e(&self, i: u32) -> &TEdge {
        &self.edges[i as usize]
    }

    fn em(&mut self, i: u32) -> &mut TEdge {
        &mut self.edges[i as usize]
    }

    fn o(&self, i: u32) -> &OutPt {
        &self.out_pts[i as usize]
    }

    fn om(&mut self, i: u32) -> &mut OutPt {
        &mut self.out_pts[i as usize]
    }

    fn r(&self, i: u32) -> &OutRec {
        &self.poly_outs[i as usize]
    }

    fn rm(&mut self, i: u32) -> &mut OutRec {
        &mut self.poly_outs[i as usize]
    }

    /// `InitEdge`: `memset` to zero, then the links, the point and `OutIdx = Unassigned`.
    fn init_edge(&mut self, e: u32, e_next: u32, e_prev: u32, pt: IntPoint) {
        let ed = self.em(e);
        *ed = TEdge::ZERO;
        ed.next = e_next;
        ed.prev = e_prev;
        ed.curr = pt;
        ed.out_idx = UNASSIGNED;
    }

    /// `RemoveEdge`: unlinks the edge (marking it with a null `Prev`) and returns its successor.
    fn remove_edge(&mut self, e: u32) -> u32 {
        let (prev, next) = (self.e(e).prev, self.e(e).next);
        self.em(prev).next = next;
        self.em(next).prev = prev;
        self.em(e).prev = NIL;
        next
    }

    /// `AddPath(pg, PolyTyp, Closed)`: false when the path makes no edges. Throws a
    /// [`ClipperException`] for a coordinate past `hiRange` or an open clip path.
    pub fn add_path(&mut self, pg: &[IntPoint], poly_typ: PolyType, closed: bool) -> bool {
        let mut high_i = pg.len() as i32 - 1;
        if closed {
            while high_i > 0 && pg[high_i as usize] == pg[0] {
                high_i -= 1;
            }
        }
        while high_i > 0 && pg[high_i as usize] == pg[high_i as usize - 1] {
            high_i -= 1;
        }
        if (closed && high_i < 2) || (!closed && high_i < 1) {
            return false;
        }
        let start = self.edges.len();
        self.edges.resize(start + high_i as usize + 1, TEdge::ZERO);
        match self.add_path_internal(pg, high_i as usize, poly_typ, closed, start) {
            Ok(true) => true,
            Ok(false) => {
                self.edges.truncate(start);
                false
            }
            Err(e) => {
                self.edges.truncate(start);
                e.throw()
            }
        }
    }

    /// `AddPaths(paths_provider, PolyTyp, Closed)`: true when any path made edges. Throws as
    /// [`Clipper::add_path`] does.
    pub fn add_paths<'a>(
        &mut self,
        paths: impl IntoIterator<Item = &'a [IntPoint]>,
        poly_typ: PolyType,
        closed: bool,
    ) -> bool {
        let paths: Vec<&[IntPoint]> = paths.into_iter().collect();
        let num_paths = paths.len();
        if num_paths == 0 {
            return false;
        }
        if num_paths == 1 {
            return self.add_path(paths[0], poly_typ, closed);
        }
        let mut num_edges = vec![0_i32; num_paths];
        let mut num_edges_total = 0_i32;
        for (i, pg) in paths.iter().enumerate() {
            // Remove the duplicate end point of a closed path and duplicate points at the end.
            let mut high_i = pg.len() as i32 - 1;
            if closed {
                while high_i > 0 && pg[high_i as usize] == pg[0] {
                    high_i -= 1;
                }
            }
            while high_i > 0 && pg[high_i as usize] == pg[high_i as usize - 1] {
                high_i -= 1;
            }
            if (closed && high_i < 2) || (!closed && high_i < 1) {
                high_i = -1;
            }
            num_edges[i] = high_i + 1;
            num_edges_total += high_i + 1;
        }
        if num_edges_total == 0 {
            return false;
        }
        // The C++ allocates one array for all the paths and moves on only past a path that made
        // edges, so the next path reuses a failed path's slots; here the failed path's edges are
        // dropped again.
        let mut result = false;
        for (i, pg) in paths.iter().enumerate() {
            if num_edges[i] != 0 && !pg.is_empty() {
                let start = self.edges.len();
                self.edges
                    .resize(start + num_edges[i] as usize, TEdge::ZERO);
                match self.add_path_internal(pg, num_edges[i] as usize - 1, poly_typ, closed, start)
                {
                    Ok(true) => result = true,
                    Ok(false) => self.edges.truncate(start),
                    Err(e) => {
                        // In C++ the exception frees the edge array, and the local minima of the
                        // paths added before point into it: using the Clipper afterwards is
                        // undefined. Here those paths stay added.
                        self.edges.truncate(start);
                        e.throw()
                    }
                }
            }
        }
        result
    }

    /// `AddPathInternal`: links the path's edges (from `base`), drops duplicate and (closed)
    /// collinear vertices, and adds the path's bounds to the local minima.
    fn add_path_internal(
        &mut self,
        pg: &[IntPoint],
        high_i: usize,
        poly_typ: PolyType,
        closed: bool,
        base: usize,
    ) -> Result<bool, ClipperException> {
        if !closed && poly_typ == PolyType::Clip {
            return Err(ClipperException::OPEN_PATHS_MUST_BE_SUBJECT);
        }
        let e = |i: usize| ix(base + i);

        // 1. Basic (first) edge initialization.
        self.edges[base + 1].curr = pg[1];
        range_test(pg[0], &mut self.use_full_range)?;
        range_test(pg[high_i], &mut self.use_full_range)?;
        self.init_edge(e(0), e(1), e(high_i), pg[0]);
        self.init_edge(e(high_i), e(0), e(high_i - 1), pg[high_i]);
        for i in (1..high_i).rev() {
            range_test(pg[i], &mut self.use_full_range)?;
            self.init_edge(e(i), e(i + 1), e(i - 1), pg[i]);
        }
        let mut e_start = e(0);

        // 2. Remove duplicate vertices, and (when closed) collinear edges.
        let mut ed = e_start;
        let mut e_loop_stop = e_start;
        loop {
            let (curr, next, prev) = (self.e(ed).curr, self.e(ed).next, self.e(ed).prev);
            // Allows matching start and end points when not closed.
            if curr == self.e(next).curr && (closed || next != e_start) {
                if ed == next {
                    break;
                }
                if ed == e_start {
                    e_start = next;
                }
                ed = self.remove_edge(ed);
                e_loop_stop = ed;
                continue;
            }
            if prev == next {
                break; // Only two vertices.
            } else if closed
                && slopes_equal3(
                    self.e(prev).curr,
                    curr,
                    self.e(next).curr,
                    self.use_full_range,
                )
                && (!self.preserve_collinear
                    || !pt2_is_between_pt1_and_pt3(self.e(prev).curr, curr, self.e(next).curr))
            {
                // Closed paths merge adjacent collinear edges; with PreserveCollinear only the
                // overlapping ones (spikes) go.
                if ed == e_start {
                    e_start = next;
                }
                ed = self.remove_edge(ed);
                ed = self.e(ed).prev;
                e_loop_stop = ed;
                continue;
            }
            ed = next;
            if ed == e_loop_stop || (!closed && self.e(ed).next == e_start) {
                break;
            }
        }
        if (!closed && ed == self.e(ed).next) || (closed && self.e(ed).prev == self.e(ed).next) {
            return Ok(false);
        }
        if !closed {
            self.has_open_paths = true;
            let p = self.e(e_start).prev;
            self.em(p).out_idx = SKIP;
        }

        // 3. Second stage of edge initialization. IsFlat: every vertex has the same y.
        let mut is_flat = true;
        ed = e_start;
        loop {
            let next_curr = self.e(self.e(ed).next).curr;
            init_edge2(self.em(ed), next_curr, poly_typ);
            ed = self.e(ed).next;
            if is_flat && self.e(ed).curr.y != self.e(e_start).curr.y {
                is_flat = false;
            }
            if ed == e_start {
                break;
            }
        }

        // 4. Add the edge bounds to the local minima. Totally flat paths are handled apart, to
        // avoid endless loops.
        if is_flat {
            if closed {
                return Ok(false);
            }
            let p = self.e(ed).prev;
            self.em(p).out_idx = SKIP;
            let loc_min = LocalMinimum {
                y: self.e(ed).bot.y,
                left_bound: NIL,
                right_bound: ed,
            };
            self.em(ed).side = ES_RIGHT;
            self.em(ed).wind_delta = 0;
            loop {
                let prev_top_x = self.e(self.e(ed).prev).top.x;
                if self.e(ed).bot.x != prev_top_x {
                    reverse_horizontal(self.em(ed));
                }
                let next = self.e(ed).next;
                if self.e(next).out_idx == SKIP {
                    break;
                }
                self.em(ed).next_in_lml = next;
                ed = next;
            }
            self.minima_list.push(loc_min);
            return Ok(true);
        }

        let mut e_min = NIL;
        // Avoids an endless loop below when an open path's start and end points match.
        let p = self.e(ed).prev;
        if self.e(p).bot == self.e(p).top {
            ed = self.e(ed).next;
        }
        // Find the local minima; a path can have several.
        loop {
            ed = self.find_next_loc_min(ed);
            if ed == e_min {
                break;
            } else if e_min == NIL {
                e_min = ed;
            }
            // ed and its Prev share a local minimum (left aligned if horizontal); their slopes
            // say which starts which bound.
            let prev = self.e(ed).prev;
            let mut loc_min = LocalMinimum {
                y: self.e(ed).bot.y,
                left_bound: NIL,
                right_bound: NIL,
            };
            let left_bound_is_forward;
            if self.e(ed).dx < self.e(prev).dx {
                loc_min.left_bound = prev;
                loc_min.right_bound = ed;
                left_bound_is_forward = false; // Q.nextInLML = Q.prev
            } else {
                loc_min.left_bound = ed;
                loc_min.right_bound = prev;
                left_bound_is_forward = true; // Q.nextInLML = Q.next
            }
            let wd = if !closed {
                0
            } else if self.e(loc_min.left_bound).next == loc_min.right_bound {
                -1
            } else {
                1
            };
            self.em(loc_min.left_bound).wind_delta = wd;
            self.em(loc_min.right_bound).wind_delta = -wd;

            ed = self.process_bound(loc_min.left_bound, left_bound_is_forward);
            if self.e(ed).out_idx == SKIP {
                ed = self.process_bound(ed, left_bound_is_forward);
            }
            let mut e2 = self.process_bound(loc_min.right_bound, !left_bound_is_forward);
            if self.e(e2).out_idx == SKIP {
                e2 = self.process_bound(e2, !left_bound_is_forward);
            }
            if self.e(loc_min.left_bound).out_idx == SKIP {
                loc_min.left_bound = NIL;
            } else if self.e(loc_min.right_bound).out_idx == SKIP {
                loc_min.right_bound = NIL;
            }
            self.minima_list.push(loc_min);
            if !left_bound_is_forward {
                ed = e2;
            }
        }
        Ok(true)
    }

    /// `FindNextLocMin`: the next local minimum edge of the path from `e`.
    fn find_next_loc_min(&self, mut e: u32) -> u32 {
        loop {
            while self.e(e).bot != self.e(self.e(e).prev).bot || self.e(e).curr == self.e(e).top {
                e = self.e(e).next;
            }
            if !is_horizontal(self.e(e)) && !is_horizontal(self.e(self.e(e).prev)) {
                break;
            }
            while is_horizontal(self.e(self.e(e).prev)) {
                e = self.e(e).prev;
            }
            let e2 = e;
            while is_horizontal(self.e(e)) {
                e = self.e(e).next;
            }
            if self.e(e).top.y == self.e(self.e(e).prev).bot.y {
                continue; // Just an intermediate horizontal.
            }
            if self.e(self.e(e2).prev).bot.x < self.e(e).bot.x {
                e = e2;
            }
            break;
        }
        e
    }

    /// `ProcessBound`: links one bound's edges by `NextInLML` and returns the edge past it.
    fn process_bound(&mut self, mut e: u32, next_is_forward: bool) -> u32 {
        let mut result = e;
        if self.e(e).out_idx == SKIP {
            // Edges beyond the skip edge make another local minimum, and ProcessBound runs again.
            if next_is_forward {
                while self.e(e).top.y == self.e(self.e(e).next).bot.y {
                    e = self.e(e).next;
                }
                // Top horizontals are left to the opposite bound on the second pass.
                while e != result && is_horizontal(self.e(e)) {
                    e = self.e(e).prev;
                }
            } else {
                while self.e(e).top.y == self.e(self.e(e).prev).bot.y {
                    e = self.e(e).prev;
                }
                while e != result && is_horizontal(self.e(e)) {
                    e = self.e(e).next;
                }
            }
            if e == result {
                result = if next_is_forward {
                    self.e(e).next
                } else {
                    self.e(e).prev
                };
            } else {
                // More edges in the bound beyond result, starting with e.
                e = if next_is_forward {
                    self.e(result).next
                } else {
                    self.e(result).prev
                };
                let loc_min = LocalMinimum {
                    y: self.e(e).bot.y,
                    left_bound: NIL,
                    right_bound: e,
                };
                self.em(e).wind_delta = 0;
                result = self.process_bound(e, next_is_forward);
                self.minima_list.push(loc_min);
            }
            return result;
        }

        if is_horizontal(self.e(e)) {
            // Careful with open paths: e may follow a skip edge, and consecutive horizontals may
            // head left before going right.
            let e_start = if next_is_forward {
                self.e(e).prev
            } else {
                self.e(e).next
            };
            let (es, eb) = (*self.e(e_start), self.e(e).bot.x);
            if is_horizontal(&es) {
                // An adjoining horizontal skip edge.
                if es.bot.x != eb && es.top.x != eb {
                    reverse_horizontal(self.em(e));
                }
            } else if es.bot.x != eb {
                reverse_horizontal(self.em(e));
            }
        }

        let e_start = e;
        if next_is_forward {
            while self.e(result).top.y == self.e(self.e(result).next).bot.y
                && self.e(self.e(result).next).out_idx != SKIP
            {
                result = self.e(result).next;
            }
            if is_horizontal(self.e(result)) && self.e(self.e(result).next).out_idx != SKIP {
                // At the top of a bound, horizontals join the bound only when the preceding edge
                // attaches to the horizontal's left vertex, unless a skip edge is the top divide.
                let mut horz = result;
                while is_horizontal(self.e(self.e(horz).prev)) {
                    horz = self.e(horz).prev;
                }
                if self.e(self.e(horz).prev).top.x > self.e(self.e(result).next).top.x {
                    result = self.e(horz).prev;
                }
            }
            while e != result {
                let next = self.e(e).next;
                self.em(e).next_in_lml = next;
                if is_horizontal(self.e(e))
                    && e != e_start
                    && self.e(e).bot.x != self.e(self.e(e).prev).top.x
                {
                    reverse_horizontal(self.em(e));
                }
                e = next;
            }
            if is_horizontal(self.e(e))
                && e != e_start
                && self.e(e).bot.x != self.e(self.e(e).prev).top.x
            {
                reverse_horizontal(self.em(e));
            }
            result = self.e(result).next; // The edge just beyond the bound.
        } else {
            while self.e(result).top.y == self.e(self.e(result).prev).bot.y
                && self.e(self.e(result).prev).out_idx != SKIP
            {
                result = self.e(result).prev;
            }
            if is_horizontal(self.e(result)) && self.e(self.e(result).prev).out_idx != SKIP {
                let mut horz = result;
                while is_horizontal(self.e(self.e(horz).next)) {
                    horz = self.e(horz).next;
                }
                let (hx, rx) = (
                    self.e(self.e(horz).next).top.x,
                    self.e(self.e(result).prev).top.x,
                );
                if hx >= rx {
                    result = self.e(horz).next;
                }
            }
            while e != result {
                let prev = self.e(e).prev;
                self.em(e).next_in_lml = prev;
                if is_horizontal(self.e(e))
                    && e != e_start
                    && self.e(e).bot.x != self.e(self.e(e).next).top.x
                {
                    reverse_horizontal(self.em(e));
                }
                e = prev;
            }
            if is_horizontal(self.e(e))
                && e != e_start
                && self.e(e).bot.x != self.e(self.e(e).next).top.x
            {
                reverse_horizontal(self.em(e));
            }
            result = self.e(result).prev; // The edge just beyond the bound.
        }
        result
    }

    /// `Clipper::Clear()`: `ClipperBase::Clear()` (the minima, the edges, the range and the open
    /// flag; not `PreserveCollinear`), then the output records.
    pub fn clear(&mut self) {
        self.minima_list.clear();
        self.edges.clear();
        self.use_full_range = false;
        self.has_open_paths = false;
        self.dispose_all_out_recs();
    }

    /// `ClipperBase::Reset()` and `Clipper::Reset()`: sorts the minima by y (`std::sort`), resets
    /// their bounds, and loads the scanbeam.
    fn reset(&mut self) {
        if !self.minima_list.is_empty() {
            orca_stl::sort_by(&mut self.minima_list, |lm1, lm2| lm1.y < lm2.y);
            for i in 0..self.minima_list.len() {
                let lm = self.minima_list[i];
                for (e, side) in [(lm.left_bound, ES_LEFT), (lm.right_bound, ES_RIGHT)] {
                    if e != NIL {
                        let ed = self.em(e);
                        ed.curr = ed.bot;
                        ed.side = side;
                        ed.out_idx = UNASSIGNED;
                    }
                }
            }
        }
        self.scanbeam = PriorityQueue::new();
        self.maxima.clear();
        self.active_edges = NIL;
        self.sorted_edges = NIL;
        for i in (0..self.minima_list.len()).rev() {
            let y = self.minima_list[i].y;
            self.scanbeam.push(y);
        }
    }

    /// `GetBounds()`: the bounds of the edges of the local minima, (0, 0, 0, 0) for none.
    #[must_use]
    pub fn get_bounds(&self) -> IntRect {
        let Some(first) = self.minima_list.first() else {
            return IntRect::default();
        };
        let b = self.e(first.left_bound).bot;
        let mut result = IntRect {
            left: b.x,
            top: b.y,
            right: b.x,
            bottom: b.y,
        };
        for lm in &self.minima_list {
            result.bottom = orca_stl::max(result.bottom, self.e(lm.left_bound).bot.y);
            let mut e = lm.left_bound;
            loop {
                let bottom_e = e;
                while self.e(e).next_in_lml != NIL {
                    let x = self.e(e).bot.x;
                    if x < result.left {
                        result.left = x;
                    }
                    if x > result.right {
                        result.right = x;
                    }
                    e = self.e(e).next_in_lml;
                }
                let ed = self.e(e);
                result.left = orca_stl::min(result.left, ed.bot.x);
                result.right = orca_stl::max(result.right, ed.bot.x);
                result.left = orca_stl::min(result.left, ed.top.x);
                result.right = orca_stl::max(result.right, ed.top.x);
                result.top = orca_stl::min(result.top, ed.top.y);
                if bottom_e == lm.left_bound {
                    e = lm.right_bound;
                } else {
                    break;
                }
            }
        }
        result
    }

    /// `Execute(clipType, solution, subjFillType, clipFillType)` to `Paths`: false when the
    /// clipping failed (`solution` is then empty). Throws a [`ClipperException`] after an open
    /// path was added.
    pub fn execute(
        &mut self,
        clip_type: ClipType,
        solution: &mut Paths,
        subj_fill_type: PolyFillType,
        clip_fill_type: PolyFillType,
    ) -> bool {
        if self.has_open_paths {
            ClipperException::POLYTREE_NEEDED.throw();
        }
        solution.clear();
        self.subj_fill_type = subj_fill_type;
        self.clip_fill_type = clip_fill_type;
        self.clip_type = clip_type;
        self.using_poly_tree = false;
        let succeeded = self.execute_internal();
        if succeeded {
            self.build_result(solution);
        }
        self.dispose_all_out_recs();
        succeeded
    }

    /// `Execute(clipType, polytree, subjFillType, clipFillType)` to a `PolyTree`.
    pub fn execute_tree(
        &mut self,
        clip_type: ClipType,
        polytree: &mut PolyTree,
        subj_fill_type: PolyFillType,
        clip_fill_type: PolyFillType,
    ) -> bool {
        self.subj_fill_type = subj_fill_type;
        self.clip_fill_type = clip_fill_type;
        self.clip_type = clip_type;
        self.using_poly_tree = true;
        let succeeded = self.execute_internal();
        if succeeded {
            self.build_result2(polytree);
        }
        self.dispose_all_out_recs();
        succeeded
    }

    /// `ExecuteInternal()`.
    fn execute_internal(&mut self) -> bool {
        // The C++ try block; what it catches makes the clipping fail.
        let succeeded = match self.sweep() {
            Ok(None) => return true,
            Ok(Some(succeeded)) => succeeded,
            Err(_) => false,
        };
        if succeeded {
            // Fix orientations.
            for i in 0..self.poly_outs.len() {
                let rec = self.poly_outs[i];
                if rec.pts != NIL
                    && !rec.is_open
                    && (rec.is_hole ^ self.reverse_output) == (self.area_op(rec.pts) > 0.0)
                {
                    self.reverse_poly_pt_links(rec.pts);
                }
            }
            self.join_common_edges();
            // FixupOutPolygon() must come after JoinCommonEdges().
            for i in 0..self.poly_outs.len() {
                let rec = self.poly_outs[i];
                if rec.pts != NIL {
                    if rec.is_open {
                        self.fixup_out_polyline(ix(i));
                    } else {
                        self.fixup_out_polygon(ix(i));
                    }
                }
            }
            if self.strict_simple {
                self.do_simple_polygons();
            }
        }
        self.joins.clear();
        self.ghost_joins.clear();
        succeeded
    }

    /// The scanbeam sweep of `ExecuteInternal`, its try block: `None` when there is nothing to
    /// clip (the C++ returns true at once), else whether `ProcessIntersections` succeeded.
    fn sweep(&mut self) -> Result<Option<bool>, ClipperException> {
        self.reset();
        if self.minima_list.is_empty() {
            return Ok(None);
        }
        let mut bot_y = self.pop_scanbeam();
        let mut succeeded = true;
        loop {
            self.insert_local_minima_into_ael(bot_y);
            self.process_horizontals()?;
            self.ghost_joins.clear();
            if self.scanbeam.is_empty() {
                break;
            }
            let top_y = self.pop_scanbeam();
            succeeded = self.process_intersections(top_y);
            if !succeeded {
                break;
            }
            self.process_edges_at_top_of_scanbeam(top_y)?;
            bot_y = top_y;
            if self.scanbeam.is_empty() && self.minima_list.is_empty() {
                break;
            }
        }
        Ok(Some(succeeded))
    }

    /// The scanbeam's top, popped with every copy of it.
    fn pop_scanbeam(&mut self) -> i64 {
        let y = *self.scanbeam.top().expect("the scanbeam is not empty");
        loop {
            self.scanbeam.pop();
            if self.scanbeam.top() != Some(&y) {
                break;
            }
        }
        y
    }

    /// `AllocateOutPt()`: a recycled point first, then the next new one.
    fn allocate_out_pt(&mut self) -> u32 {
        if self.out_pts_free != NIL {
            let pt = self.out_pts_free;
            self.out_pts_free = self.o(pt).next;
            pt
        } else {
            self.out_pts.push(OutPt::default());
            ix(self.out_pts.len() - 1)
        }
    }

    /// `DisposeOutPt`: the point goes to the free list.
    fn dispose_out_pt(&mut self, pt: u32) {
        let free = self.out_pts_free;
        self.om(pt).next = free;
        self.out_pts_free = pt;
    }

    /// `DisposeOutPts`: the whole loop goes to the free list.
    fn dispose_out_pts(&mut self, pp: u32) {
        if pp != NIL {
            let (prev, free) = (self.o(pp).prev, self.out_pts_free);
            self.om(prev).next = free;
            self.out_pts_free = pp;
        }
    }

    /// `DisposeAllOutRecs()`.
    fn dispose_all_out_recs(&mut self) {
        self.out_pts.clear();
        self.out_pts_free = NIL;
        self.poly_outs.clear();
    }

    /// `IsEvenOddFillType`.
    fn is_even_odd_fill_type(&self, e: u32) -> bool {
        if self.e(e).poly_typ == PolyType::Subject {
            self.subj_fill_type == PolyFillType::EvenOdd
        } else {
            self.clip_fill_type == PolyFillType::EvenOdd
        }
    }

    /// `IsEvenOddAltFillType`.
    fn is_even_odd_alt_fill_type(&self, e: u32) -> bool {
        if self.e(e).poly_typ == PolyType::Subject {
            self.clip_fill_type == PolyFillType::EvenOdd
        } else {
            self.subj_fill_type == PolyFillType::EvenOdd
        }
    }

    /// `SetWindingCount`.
    fn set_winding_count(&mut self, edge: u32) {
        let (pt, wd) = (self.e(edge).poly_typ, self.e(edge).wind_delta);
        let mut e = self.e(edge).prev_in_ael;
        // The edge of the same poly type just before `edge` in the AEL.
        while e != NIL && (self.e(e).poly_typ != pt || self.e(e).wind_delta == 0) {
            e = self.e(e).prev_in_ael;
        }
        if e == NIL {
            let wc = if wd == 0 {
                let pft = if pt == PolyType::Subject {
                    self.subj_fill_type
                } else {
                    self.clip_fill_type
                };
                if pft == PolyFillType::Negative { -1 } else { 1 }
            } else {
                wd
            };
            self.em(edge).wind_cnt = wc;
            self.em(edge).wind_cnt2 = 0;
            e = self.active_edges; // Ready to compute WindCnt2.
        } else if wd == 0 && self.clip_type != ClipType::Union {
            let wc2 = self.e(e).wind_cnt2;
            self.em(edge).wind_cnt = 1;
            self.em(edge).wind_cnt2 = wc2;
            e = self.e(e).next_in_ael;
        } else if self.is_even_odd_fill_type(edge) {
            // Even-odd filling.
            let wc = if wd == 0 {
                // Inside a subject polygon?
                let mut inside = true;
                let mut e2 = self.e(e).prev_in_ael;
                while e2 != NIL {
                    if self.e(e2).poly_typ == self.e(e).poly_typ && self.e(e2).wind_delta != 0 {
                        inside = !inside;
                    }
                    e2 = self.e(e2).prev_in_ael;
                }
                if inside { 0 } else { 1 }
            } else {
                wd
            };
            let wc2 = self.e(e).wind_cnt2;
            self.em(edge).wind_cnt = wc;
            self.em(edge).wind_cnt2 = wc2;
            e = self.e(e).next_in_ael;
        } else {
            // Non-zero, positive or negative filling.
            let (ewc, ewd) = (self.e(e).wind_cnt, self.e(e).wind_delta);
            let wc = if ewc * ewd < 0 {
                // The previous edge decreases the winding count toward zero: outside it.
                if ewc.abs() > 1 {
                    // Outside the previous polygon but still inside another.
                    if ewd * wd < 0 { ewc } else { ewc + wd }
                } else if wd == 0 {
                    1
                } else {
                    wd
                }
            } else if wd == 0 {
                // The previous edge increases the winding count away from zero: inside it.
                if ewc < 0 { ewc - 1 } else { ewc + 1 }
            } else if ewd * wd < 0 {
                ewc
            } else {
                ewc + wd
            };
            let wc2 = self.e(e).wind_cnt2;
            self.em(edge).wind_cnt = wc;
            self.em(edge).wind_cnt2 = wc2;
            e = self.e(e).next_in_ael;
        }

        // Update WindCnt2.
        if self.is_even_odd_alt_fill_type(edge) {
            while e != edge {
                if self.e(e).wind_delta != 0 {
                    let w = self.e(edge).wind_cnt2;
                    self.em(edge).wind_cnt2 = if w == 0 { 1 } else { 0 };
                }
                e = self.e(e).next_in_ael;
            }
        } else {
            while e != edge {
                let d = self.e(e).wind_delta;
                self.em(edge).wind_cnt2 += d;
                e = self.e(e).next_in_ael;
            }
        }
    }

    /// `IsContributing`.
    fn is_contributing(&self, edge: u32) -> bool {
        let ed = self.e(edge);
        let (pft, pft2) = if ed.poly_typ == PolyType::Subject {
            (self.subj_fill_type, self.clip_fill_type)
        } else {
            (self.clip_fill_type, self.subj_fill_type)
        };
        match pft {
            PolyFillType::EvenOdd => {
                // A subject line flagged as inside a subject polygon.
                if ed.wind_delta == 0 && ed.wind_cnt != 1 {
                    return false;
                }
            }
            PolyFillType::NonZero => {
                if ed.wind_cnt.abs() != 1 {
                    return false;
                }
            }
            PolyFillType::Positive => {
                if ed.wind_cnt != 1 {
                    return false;
                }
            }
            PolyFillType::Negative => {
                if ed.wind_cnt != -1 {
                    return false;
                }
            }
        }
        let wc2 = ed.wind_cnt2;
        let inside = |pft2: PolyFillType| match pft2 {
            PolyFillType::EvenOdd | PolyFillType::NonZero => wc2 != 0,
            PolyFillType::Positive => wc2 > 0,
            PolyFillType::Negative => wc2 < 0,
        };
        let outside = |pft2: PolyFillType| match pft2 {
            PolyFillType::EvenOdd | PolyFillType::NonZero => wc2 == 0,
            PolyFillType::Positive => wc2 <= 0,
            PolyFillType::Negative => wc2 >= 0,
        };
        match self.clip_type {
            ClipType::Intersection => inside(pft2),
            ClipType::Union => outside(pft2),
            ClipType::Difference => {
                if ed.poly_typ == PolyType::Subject {
                    outside(pft2)
                } else {
                    inside(pft2)
                }
            }
            ClipType::Xor => {
                // Xor always contributes unless open.
                if ed.wind_delta == 0 {
                    outside(pft2)
                } else {
                    true
                }
            }
        }
    }

    /// `AddLocalMinPoly`.
    fn add_local_min_poly(&mut self, e1: u32, e2: u32, pt: IntPoint) -> u32 {
        let result;
        let e;
        let prev_e;
        if is_horizontal(self.e(e2)) || self.e(e1).dx > self.e(e2).dx {
            result = self.add_out_pt(e1, pt);
            let oi = self.e(e1).out_idx;
            self.em(e2).out_idx = oi;
            self.em(e1).side = ES_LEFT;
            self.em(e2).side = ES_RIGHT;
            e = e1;
            prev_e = if self.e(e).prev_in_ael == e2 {
                self.e(e2).prev_in_ael
            } else {
                self.e(e).prev_in_ael
            };
        } else {
            result = self.add_out_pt(e2, pt);
            let oi = self.e(e2).out_idx;
            self.em(e1).out_idx = oi;
            self.em(e1).side = ES_RIGHT;
            self.em(e2).side = ES_LEFT;
            e = e2;
            prev_e = if self.e(e).prev_in_ael == e1 {
                self.e(e1).prev_in_ael
            } else {
                self.e(e).prev_in_ael
            };
        }
        if prev_e != NIL
            && self.e(prev_e).out_idx >= 0
            && self.e(prev_e).top.y < pt.y
            && self.e(e).top.y < pt.y
        {
            let x_prev = top_x(self.e(prev_e), pt.y);
            let x_e = top_x(self.e(e), pt.y);
            if x_prev == x_e
                && self.e(e).wind_delta != 0
                && self.e(prev_e).wind_delta != 0
                && slopes_equal4(
                    Point::new(x_prev, pt.y),
                    self.e(prev_e).top,
                    Point::new(x_e, pt.y),
                    self.e(e).top,
                    self.use_full_range,
                )
            {
                let out_pt = self.add_out_pt(prev_e, pt);
                let off_pt = self.e(e).top;
                self.joins.push(Join {
                    out_pt1: result,
                    out_pt2: out_pt,
                    off_pt,
                });
            }
        }
        result
    }

    /// `AddLocalMaxPoly`.
    fn add_local_max_poly(&mut self, e1: u32, e2: u32, pt: IntPoint) {
        self.add_out_pt(e1, pt);
        if self.e(e2).wind_delta == 0 {
            self.add_out_pt(e2, pt);
        }
        let (o1, o2) = (self.e(e1).out_idx, self.e(e2).out_idx);
        if o1 == o2 {
            self.em(e1).out_idx = UNASSIGNED;
            self.em(e2).out_idx = UNASSIGNED;
        } else if o1 < o2 {
            self.append_polygon(e1, e2);
        } else {
            self.append_polygon(e2, e1);
        }
    }

    /// `AddEdgeToSEL`: pushes the edge at the front of the SEL (horizontals in no order).
    fn add_edge_to_sel(&mut self, edge: u32) {
        let sorted = self.sorted_edges;
        if sorted == NIL {
            self.sorted_edges = edge;
            self.em(edge).prev_in_sel = NIL;
            self.em(edge).next_in_sel = NIL;
        } else {
            self.em(edge).next_in_sel = sorted;
            self.em(edge).prev_in_sel = NIL;
            self.em(sorted).prev_in_sel = edge;
            self.sorted_edges = edge;
        }
    }

    /// `CopyAELToSEL`.
    fn copy_ael_to_sel(&mut self) {
        let mut e = self.active_edges;
        self.sorted_edges = e;
        while e != NIL {
            let ed = self.em(e);
            ed.prev_in_sel = ed.prev_in_ael;
            ed.next_in_sel = ed.next_in_ael;
            e = ed.next_in_ael;
        }
    }

    /// `InsertLocalMinimaIntoAEL`: inserts the bounds of the minima at `bot_y`, popped from the
    /// back of the sorted list.
    fn insert_local_minima_into_ael(&mut self, bot_y: i64) {
        while let Some(&lm) = self.minima_list.last() {
            if lm.y != bot_y {
                break;
            }
            let (lb, rb) = (lm.left_bound, lm.right_bound);
            self.minima_list.pop();

            let mut op1 = NIL;
            if lb == NIL {
                // Don't insert LB into either the AEL or the SEL.
                self.insert_edge_into_ael(rb, NIL);
                self.set_winding_count(rb);
                if self.is_contributing(rb) {
                    let b = self.e(rb).bot;
                    op1 = self.add_out_pt(rb, b);
                }
            } else if rb == NIL {
                self.insert_edge_into_ael(lb, NIL);
                self.set_winding_count(lb);
                if self.is_contributing(lb) {
                    let b = self.e(lb).bot;
                    op1 = self.add_out_pt(lb, b);
                }
                let y = self.e(lb).top.y;
                self.scanbeam.push(y);
            } else {
                self.insert_edge_into_ael(lb, NIL);
                self.insert_edge_into_ael(rb, lb);
                self.set_winding_count(lb);
                let (wc, wc2) = (self.e(lb).wind_cnt, self.e(lb).wind_cnt2);
                self.em(rb).wind_cnt = wc;
                self.em(rb).wind_cnt2 = wc2;
                if self.is_contributing(lb) {
                    let b = self.e(lb).bot;
                    op1 = self.add_local_min_poly(lb, rb, b);
                }
                let y = self.e(lb).top.y;
                self.scanbeam.push(y);
            }

            if rb != NIL {
                if is_horizontal(self.e(rb)) {
                    self.add_edge_to_sel(rb);
                    let nl = self.e(rb).next_in_lml;
                    if nl != NIL {
                        let y = self.e(nl).top.y;
                        self.scanbeam.push(y);
                    }
                } else {
                    let y = self.e(rb).top.y;
                    self.scanbeam.push(y);
                }
            }

            if lb == NIL || rb == NIL {
                continue;
            }

            // Output polygons that share an edge need joining later.
            if op1 != NIL
                && is_horizontal(self.e(rb))
                && !self.ghost_joins.is_empty()
                && self.e(rb).wind_delta != 0
            {
                let (rbx, rtx) = (self.e(rb).bot.x, self.e(rb).top.x);
                for k in 0..self.ghost_joins.len() {
                    let jr = self.ghost_joins[k];
                    // A horizontal rb that overlaps a 'ghost' horizontal makes the ghost join a
                    // real one.
                    if horz_segments_overlap(self.o(jr.out_pt1).pt.x, jr.off_pt.x, rbx, rtx) {
                        self.joins.push(Join {
                            out_pt1: jr.out_pt1,
                            out_pt2: op1,
                            off_pt: jr.off_pt,
                        });
                    }
                }
            }

            let lpa = self.e(lb).prev_in_ael;
            if self.e(lb).out_idx >= 0
                && lpa != NIL
                && self.e(lpa).curr.x == self.e(lb).bot.x
                && self.e(lpa).out_idx >= 0
                && slopes_equal4(
                    self.e(lpa).bot,
                    self.e(lpa).top,
                    self.e(lb).curr,
                    self.e(lb).top,
                    self.use_full_range,
                )
                && self.e(lb).wind_delta != 0
                && self.e(lpa).wind_delta != 0
            {
                let b = self.e(lb).bot;
                let op2 = self.add_out_pt(lpa, b);
                let t = self.e(lb).top;
                self.joins.push(Join {
                    out_pt1: op1,
                    out_pt2: op2,
                    off_pt: t,
                });
            }

            if self.e(lb).next_in_ael != rb {
                let rpa = self.e(rb).prev_in_ael;
                if self.e(rb).out_idx >= 0
                    && self.e(rpa).out_idx >= 0
                    && slopes_equal4(
                        self.e(rpa).curr,
                        self.e(rpa).top,
                        self.e(rb).curr,
                        self.e(rb).top,
                        self.use_full_range,
                    )
                    && self.e(rb).wind_delta != 0
                    && self.e(rpa).wind_delta != 0
                {
                    let b = self.e(rb).bot;
                    let op2 = self.add_out_pt(rpa, b);
                    let t = self.e(rb).top;
                    self.joins.push(Join {
                        out_pt1: op1,
                        out_pt2: op2,
                        off_pt: t,
                    });
                }

                let mut e = self.e(lb).next_in_ael;
                if e != NIL {
                    while e != rb {
                        // IntersectEdges() takes its first edge to be right of the second
                        // above the intersection: the order matters.
                        let c = self.e(lb).curr;
                        self.intersect_edges(rb, e, c);
                        e = self.e(e).next_in_ael;
                    }
                }
            }
        }
    }

    /// `DeleteFromAEL`.
    fn delete_from_ael(&mut self, e: u32) {
        let (ael_prev, ael_next) = (self.e(e).prev_in_ael, self.e(e).next_in_ael);
        if ael_prev == NIL && ael_next == NIL && e != self.active_edges {
            return; // Already deleted.
        }
        if ael_prev != NIL {
            self.em(ael_prev).next_in_ael = ael_next;
        } else {
            self.active_edges = ael_next;
        }
        if ael_next != NIL {
            self.em(ael_next).prev_in_ael = ael_prev;
        }
        self.em(e).next_in_ael = NIL;
        self.em(e).prev_in_ael = NIL;
    }

    /// `DeleteFromSEL`.
    fn delete_from_sel(&mut self, e: u32) {
        let (sel_prev, sel_next) = (self.e(e).prev_in_sel, self.e(e).next_in_sel);
        if sel_prev == NIL && sel_next == NIL && e != self.sorted_edges {
            return; // Already deleted.
        }
        if sel_prev != NIL {
            self.em(sel_prev).next_in_sel = sel_next;
        } else {
            self.sorted_edges = sel_next;
        }
        if sel_next != NIL {
            self.em(sel_next).prev_in_sel = sel_prev;
        }
        self.em(e).next_in_sel = NIL;
        self.em(e).prev_in_sel = NIL;
    }

    /// Swaps the edges' `Side`.
    fn swap_sides(&mut self, e1: u32, e2: u32) {
        let (s1, s2) = (self.e(e1).side, self.e(e2).side);
        self.em(e1).side = s2;
        self.em(e2).side = s1;
    }

    /// Swaps the edges' `Side` and `OutIdx`.
    fn swap_sides_and_idx(&mut self, e1: u32, e2: u32) {
        self.swap_sides(e1, e2);
        let (o1, o2) = (self.e(e1).out_idx, self.e(e2).out_idx);
        self.em(e1).out_idx = o2;
        self.em(e2).out_idx = o1;
    }

    /// `IntersectEdges`: updates the winding counts of two edges that cross at `pt` and adds the
    /// output points the crossing makes. `e1` is right of `e2` above the crossing.
    fn intersect_edges(&mut self, e1: u32, e2: u32, pt: IntPoint) {
        let e1_contributing = self.e(e1).out_idx >= 0;
        let e2_contributing = self.e(e2).out_idx >= 0;
        // ClipperLib_Z: SetZ(pt, e1, e2) here.

        // Either edge on an open path (use_lines).
        let (wd1, wd2) = (self.e(e1).wind_delta, self.e(e2).wind_delta);
        if wd1 == 0 || wd2 == 0 {
            // Ignore subject-subject open path intersections, unless both are open paths and
            // both are contributing maxima.
            let (pt1, pt2) = (self.e(e1).poly_typ, self.e(e2).poly_typ);
            if wd1 == 0 && wd2 == 0 {
                return;
            } else if pt1 == pt2 && wd1 != wd2 && self.clip_type == ClipType::Union {
                // A subject line crossing a subject polygon.
                if wd1 == 0 {
                    if e2_contributing {
                        self.add_out_pt(e1, pt);
                        if e1_contributing {
                            self.em(e1).out_idx = UNASSIGNED;
                        }
                    }
                } else if e1_contributing {
                    self.add_out_pt(e2, pt);
                    if e2_contributing {
                        self.em(e2).out_idx = UNASSIGNED;
                    }
                }
            } else if pt1 != pt2 {
                // Toggle a subject open path's OutIdx when Abs(clip.WndCnt) == 1.
                let (c1, c2) = (*self.e(e1), *self.e(e2));
                if wd1 == 0
                    && c2.wind_cnt.abs() == 1
                    && (self.clip_type != ClipType::Union || c2.wind_cnt2 == 0)
                {
                    self.add_out_pt(e1, pt);
                    if e1_contributing {
                        self.em(e1).out_idx = UNASSIGNED;
                    }
                } else if wd2 == 0
                    && c1.wind_cnt.abs() == 1
                    && (self.clip_type != ClipType::Union || c1.wind_cnt2 == 0)
                {
                    self.add_out_pt(e2, pt);
                    if e2_contributing {
                        self.em(e2).out_idx = UNASSIGNED;
                    }
                }
            }
            return;
        }

        // Update the winding counts.
        if self.e(e1).poly_typ == self.e(e2).poly_typ {
            if self.is_even_odd_fill_type(e1) {
                let (w1, w2) = (self.e(e1).wind_cnt, self.e(e2).wind_cnt);
                self.em(e1).wind_cnt = w2;
                self.em(e2).wind_cnt = w1;
            } else {
                let e2wd = self.e(e2).wind_delta;
                let w1 = self.e(e1).wind_cnt;
                self.em(e1).wind_cnt = if w1 + e2wd == 0 { -w1 } else { w1 + e2wd };
                let e1wd = self.e(e1).wind_delta;
                let w2 = self.e(e2).wind_cnt;
                self.em(e2).wind_cnt = if w2 - e1wd == 0 { -w2 } else { w2 - e1wd };
            }
        } else {
            if !self.is_even_odd_fill_type(e2) {
                let d = self.e(e2).wind_delta;
                self.em(e1).wind_cnt2 += d;
            } else {
                let w = self.e(e1).wind_cnt2;
                self.em(e1).wind_cnt2 = if w == 0 { 1 } else { 0 };
            }
            if !self.is_even_odd_fill_type(e1) {
                let d = self.e(e1).wind_delta;
                self.em(e2).wind_cnt2 -= d;
            } else {
                let w = self.e(e2).wind_cnt2;
                self.em(e2).wind_cnt2 = if w == 0 { 1 } else { 0 };
            }
        }

        let (c1, c2) = (*self.e(e1), *self.e(e2));
        let (e1_fill_type, e1_fill_type2) = if c1.poly_typ == PolyType::Subject {
            (self.subj_fill_type, self.clip_fill_type)
        } else {
            (self.clip_fill_type, self.subj_fill_type)
        };
        let (e2_fill_type, e2_fill_type2) = if c2.poly_typ == PolyType::Subject {
            (self.subj_fill_type, self.clip_fill_type)
        } else {
            (self.clip_fill_type, self.subj_fill_type)
        };
        let wc = |fill: PolyFillType, w: i32| -> i64 {
            match fill {
                PolyFillType::Positive => i64::from(w),
                PolyFillType::Negative => -i64::from(w),
                _ => i64::from(w.abs()),
            }
        };
        let e1_wc = wc(e1_fill_type, c1.wind_cnt);
        let e2_wc = wc(e2_fill_type, c2.wind_cnt);

        if e1_contributing && e2_contributing {
            if (e1_wc != 0 && e1_wc != 1)
                || (e2_wc != 0 && e2_wc != 1)
                || (c1.poly_typ != c2.poly_typ && self.clip_type != ClipType::Xor)
            {
                self.add_local_max_poly(e1, e2, pt);
            } else {
                self.add_out_pt(e1, pt);
                self.add_out_pt(e2, pt);
                self.swap_sides_and_idx(e1, e2);
            }
        } else if e1_contributing {
            if e2_wc == 0 || e2_wc == 1 {
                self.add_out_pt(e1, pt);
                self.swap_sides_and_idx(e1, e2);
            }
        } else if e2_contributing {
            if e1_wc == 0 || e1_wc == 1 {
                self.add_out_pt(e2, pt);
                self.swap_sides_and_idx(e1, e2);
            }
        } else if (e1_wc == 0 || e1_wc == 1) && (e2_wc == 0 || e2_wc == 1) {
            // Neither edge is contributing yet.
            let e1_wc2 = wc(e1_fill_type2, c1.wind_cnt2);
            let e2_wc2 = wc(e2_fill_type2, c2.wind_cnt2);
            if c1.poly_typ != c2.poly_typ {
                self.add_local_min_poly(e1, e2, pt);
            } else if e1_wc == 1 && e2_wc == 1 {
                match self.clip_type {
                    ClipType::Intersection => {
                        if e1_wc2 > 0 && e2_wc2 > 0 {
                            self.add_local_min_poly(e1, e2, pt);
                        }
                    }
                    ClipType::Union => {
                        if e1_wc2 <= 0 && e2_wc2 <= 0 {
                            self.add_local_min_poly(e1, e2, pt);
                        }
                    }
                    ClipType::Difference => {
                        if (c1.poly_typ == PolyType::Clip && e1_wc2 > 0 && e2_wc2 > 0)
                            || (c1.poly_typ == PolyType::Subject && e1_wc2 <= 0 && e2_wc2 <= 0)
                        {
                            self.add_local_min_poly(e1, e2, pt);
                        }
                    }
                    ClipType::Xor => {
                        self.add_local_min_poly(e1, e2, pt);
                    }
                }
            } else {
                self.swap_sides(e1, e2);
            }
        }
    }

    /// `SetHoleState`.
    fn set_hole_state(&mut self, e: u32, outrec: u32) {
        let mut e2 = self.e(e).prev_in_ael;
        let mut e_tmp = NIL;
        while e2 != NIL {
            if self.e(e2).out_idx >= 0 && self.e(e2).wind_delta != 0 {
                if e_tmp == NIL {
                    e_tmp = e2;
                } else if self.e(e_tmp).out_idx == self.e(e2).out_idx {
                    e_tmp = NIL;
                }
            }
            e2 = self.e(e2).prev_in_ael;
        }
        if e_tmp == NIL {
            self.rm(outrec).first_left = NIL;
            self.rm(outrec).is_hole = false;
        } else {
            let fl = self.e(e_tmp).out_idx as u32;
            let hole = !self.r(fl).is_hole;
            self.rm(outrec).first_left = fl;
            self.rm(outrec).is_hole = hole;
        }
    }

    /// `Area(const OutPt*)`: the loop's signed area, the sums in 64-bit integers first.
    fn area_op(&self, op: u32) -> f64 {
        if op == NIL {
            return 0.0;
        }
        let start = op;
        let mut op = op;
        let mut a = 0.0;
        loop {
            let o = self.o(op);
            let p = self.o(o.prev).pt;
            a += p.x.wrapping_add(o.pt.x) as f64 * p.y.wrapping_sub(o.pt.y) as f64;
            op = o.next;
            if op == start {
                break;
            }
        }
        a * 0.5
    }

    /// `ReversePolyPtLinks`.
    fn reverse_poly_pt_links(&mut self, pp: u32) {
        if pp == NIL {
            return;
        }
        let mut pp1 = pp;
        loop {
            let pp2 = self.o(pp1).next;
            let prev = self.o(pp1).prev;
            self.om(pp1).next = prev;
            self.om(pp1).prev = pp2;
            pp1 = pp2;
            if pp1 == pp {
                break;
            }
        }
    }

    /// `FirstIsBottomPt`.
    fn first_is_bottom_pt(&self, btm_pt1: u32, btm_pt2: u32) -> bool {
        let dxs = |btm: u32| {
            let b = self.o(btm).pt;
            let mut p = self.o(btm).prev;
            while self.o(p).pt == b && p != btm {
                p = self.o(p).prev;
            }
            let dxp = orca_crt::fabs(get_dx(b, self.o(p).pt));
            p = self.o(btm).next;
            while self.o(p).pt == b && p != btm {
                p = self.o(p).next;
            }
            let dxn = orca_crt::fabs(get_dx(b, self.o(p).pt));
            (dxp, dxn)
        };
        let (dx1p, dx1n) = dxs(btm_pt1);
        let (dx2p, dx2n) = dxs(btm_pt2);
        if orca_stl::max(dx1p, dx1n) == orca_stl::max(dx2p, dx2n)
            && orca_stl::min(dx1p, dx1n) == orca_stl::min(dx2p, dx2n)
        {
            self.area_op(btm_pt1) > 0.0 // Otherwise identical: use the orientation.
        } else {
            (dx1p >= dx2p && dx1p >= dx2n) || (dx1n >= dx2p && dx1n >= dx2n)
        }
    }

    /// `GetBottomPt`.
    fn get_bottom_pt(&self, mut pp: u32) -> u32 {
        let mut dups = NIL;
        let mut p = self.o(pp).next;
        while p != pp {
            let (pt, ppt) = (self.o(p).pt, self.o(pp).pt);
            if pt.y > ppt.y {
                pp = p;
                dups = NIL;
            } else if pt.y == ppt.y && pt.x <= ppt.x {
                if pt.x < ppt.x {
                    dups = NIL;
                    pp = p;
                } else if self.o(p).next != pp && self.o(p).prev != pp {
                    dups = p;
                }
            }
            p = self.o(p).next;
        }
        if dups != NIL {
            // At least two vertices at the bottom point.
            while dups != p {
                if !self.first_is_bottom_pt(p, dups) {
                    pp = dups;
                }
                dups = self.o(dups).next;
                while self.o(dups).pt != self.o(pp).pt {
                    dups = self.o(dups).next;
                }
            }
        }
        pp
    }

    /// `GetLowermostRec`: which fragment has the correct hole state.
    fn get_lowermost_rec(&mut self, out_rec1: u32, out_rec2: u32) -> u32 {
        for r in [out_rec1, out_rec2] {
            if self.r(r).bottom_pt == NIL {
                let b = self.get_bottom_pt(self.r(r).pts);
                self.rm(r).bottom_pt = b;
            }
        }
        let (op1, op2) = (self.r(out_rec1).bottom_pt, self.r(out_rec2).bottom_pt);
        let (p1, p2) = (self.o(op1).pt, self.o(op2).pt);
        if p1.y > p2.y {
            out_rec1
        } else if p1.y < p2.y {
            out_rec2
        } else if p1.x < p2.x {
            out_rec1
        } else if p1.x > p2.x || self.o(op1).next == op1 {
            out_rec2
        } else if self.o(op2).next == op2 || self.first_is_bottom_pt(op1, op2) {
            out_rec1
        } else {
            out_rec2
        }
    }

    /// `OutRec1RightOfOutRec2`.
    fn out_rec1_right_of_out_rec2(&self, mut out_rec1: u32, out_rec2: u32) -> bool {
        loop {
            out_rec1 = self.r(out_rec1).first_left;
            if out_rec1 == out_rec2 {
                return true;
            }
            if out_rec1 == NIL {
                return false;
            }
        }
    }

    /// `GetOutRec(Idx)`: follows the records' `Idx` to the one that owns them.
    fn get_out_rec(&self, idx: i32) -> u32 {
        let mut outrec = idx as u32;
        while outrec != self.r(outrec).idx as u32 {
            outrec = self.r(outrec).idx as u32;
        }
        outrec
    }

    /// `AppendPolygon`: joins e2's polygon onto e1's.
    fn append_polygon(&mut self, e1: u32, e2: u32) {
        let out_rec1 = self.e(e1).out_idx as u32;
        let out_rec2 = self.e(e2).out_idx as u32;
        let hole_state_rec = if self.out_rec1_right_of_out_rec2(out_rec1, out_rec2) {
            out_rec2
        } else if self.out_rec1_right_of_out_rec2(out_rec2, out_rec1) {
            out_rec1
        } else {
            self.get_lowermost_rec(out_rec1, out_rec2)
        };

        let p1_lft = self.r(out_rec1).pts;
        let p1_rt = self.o(p1_lft).prev;
        let p2_lft = self.r(out_rec2).pts;
        let p2_rt = self.o(p2_lft).prev;

        if self.e(e1).side == ES_LEFT {
            if self.e(e2).side == ES_LEFT {
                // z y x a b c
                self.reverse_poly_pt_links(p2_lft);
                self.om(p2_lft).next = p1_lft;
                self.om(p1_lft).prev = p2_lft;
                self.om(p1_rt).next = p2_rt;
                self.om(p2_rt).prev = p1_rt;
                self.rm(out_rec1).pts = p2_rt;
            } else {
                // x y z a b c
                self.om(p2_rt).next = p1_lft;
                self.om(p1_lft).prev = p2_rt;
                self.om(p2_lft).prev = p1_rt;
                self.om(p1_rt).next = p2_lft;
                self.rm(out_rec1).pts = p2_lft;
            }
        } else if self.e(e2).side == ES_RIGHT {
            // a b c z y x
            self.reverse_poly_pt_links(p2_lft);
            self.om(p1_rt).next = p2_rt;
            self.om(p2_rt).prev = p1_rt;
            self.om(p2_lft).next = p1_lft;
            self.om(p1_lft).prev = p2_lft;
        } else {
            // a b c x y z
            self.om(p1_rt).next = p2_lft;
            self.om(p2_lft).prev = p1_rt;
            self.om(p1_lft).prev = p2_rt;
            self.om(p2_rt).next = p1_lft;
        }

        self.rm(out_rec1).bottom_pt = NIL;
        if hole_state_rec == out_rec2 {
            if self.r(out_rec2).first_left != out_rec1 {
                let fl = self.r(out_rec2).first_left;
                self.rm(out_rec1).first_left = fl;
            }
            let hole = self.r(out_rec2).is_hole;
            self.rm(out_rec1).is_hole = hole;
        }
        self.rm(out_rec2).pts = NIL;
        self.rm(out_rec2).bottom_pt = NIL;
        self.rm(out_rec2).first_left = out_rec1;

        let ok_idx = self.e(e1).out_idx;
        let obsolete_idx = self.e(e2).out_idx;
        // Safe, since only AddLocalMaxPoly gets here.
        self.em(e1).out_idx = UNASSIGNED;
        self.em(e2).out_idx = UNASSIGNED;

        let mut e = self.active_edges;
        while e != NIL {
            if self.e(e).out_idx == obsolete_idx {
                let side = self.e(e1).side;
                self.em(e).out_idx = ok_idx;
                self.em(e).side = side;
                break;
            }
            e = self.e(e).next_in_ael;
        }
        let idx = self.r(out_rec1).idx;
        self.rm(out_rec2).idx = idx;
    }

    /// `CreateOutRec`.
    fn create_out_rec(&mut self) -> u32 {
        let idx = self.poly_outs.len();
        self.poly_outs.push(OutRec {
            idx: idx as i32,
            is_hole: false,
            is_open: false,
            first_left: NIL,
            poly_nd: NIL,
            pts: NIL,
            bottom_pt: NIL,
        });
        ix(idx)
    }

    /// `AddOutPt`: a new output record for an edge that has none, else the point at the
    /// record's left (front) or right end, as the edge's side says; an equal end point is reused.
    fn add_out_pt(&mut self, e: u32, pt: IntPoint) -> u32 {
        if self.e(e).out_idx < 0 {
            let out_rec = self.create_out_rec();
            let is_open = self.e(e).wind_delta == 0;
            self.rm(out_rec).is_open = is_open;
            let new_op = self.allocate_out_pt();
            self.rm(out_rec).pts = new_op;
            let idx = self.r(out_rec).idx;
            *self.om(new_op) = OutPt {
                idx,
                pt,
                next: new_op,
                prev: new_op,
            };
            if !is_open {
                self.set_hole_state(e, out_rec);
            }
            self.em(e).out_idx = idx;
            new_op
        } else {
            let out_rec = self.e(e).out_idx as u32;
            // OutRec.Pts is the left-most point, OutRec.Pts.Prev the right-most.
            let op = self.r(out_rec).pts;
            let op_prev = self.o(op).prev;
            let to_front = self.e(e).side == ES_LEFT;
            if to_front && pt == self.o(op).pt {
                return op;
            } else if !to_front && pt == self.o(op_prev).pt {
                return op_prev;
            }
            let new_op = self.allocate_out_pt();
            let idx = self.r(out_rec).idx;
            *self.om(new_op) = OutPt {
                idx,
                pt,
                next: op,
                prev: op_prev,
            };
            self.om(op_prev).next = new_op;
            self.om(op).prev = new_op;
            if to_front {
                self.rm(out_rec).pts = new_op;
            }
            new_op
        }
    }

    /// `GetLastOutPt`.
    fn get_last_out_pt(&self, e: u32) -> u32 {
        let pts = self.r(self.e(e).out_idx as u32).pts;
        if self.e(e).side == ES_LEFT {
            pts
        } else {
            self.o(pts).prev
        }
    }

    /// `ProcessHorizontals()`.
    fn process_horizontals(&mut self) -> Result<(), ClipperException> {
        let mut horz_edge = self.sorted_edges;
        while horz_edge != NIL {
            self.delete_from_sel(horz_edge);
            self.process_horizontal(horz_edge)?;
            horz_edge = self.sorted_edges;
        }
        Ok(())
    }

    /// `IsMaxima`.
    fn is_maxima(&self, e: u32, y: i64) -> bool {
        e != NIL && self.e(e).top.y == y && self.e(e).next_in_lml == NIL
    }

    /// `IsIntermediate`.
    fn is_intermediate(&self, e: u32, y: i64) -> bool {
        self.e(e).top.y == y && self.e(e).next_in_lml != NIL
    }

    /// `GetMaximaPair`.
    fn get_maxima_pair(&self, e: u32) -> u32 {
        let (next, prev, top) = (self.e(e).next, self.e(e).prev, self.e(e).top);
        if self.e(next).top == top && self.e(next).next_in_lml == NIL {
            next
        } else if self.e(prev).top == top && self.e(prev).next_in_lml == NIL {
            prev
        } else {
            NIL
        }
    }

    /// `GetMaximaPairEx`: as `GetMaximaPair`, but none when the pair is not in the AEL (unless
    /// it is horizontal).
    fn get_maxima_pair_ex(&self, e: u32) -> u32 {
        let result = self.get_maxima_pair(e);
        if result != NIL {
            let r = self.e(result);
            if r.out_idx == SKIP || (r.next_in_ael == r.prev_in_ael && !is_horizontal(r)) {
                return NIL;
            }
        }
        result
    }

    /// `SwapPositionsInAEL`.
    fn swap_positions_in_ael(&mut self, edge1: u32, edge2: u32) {
        // One or other edge already removed from the AEL?
        if self.e(edge1).next_in_ael == self.e(edge1).prev_in_ael
            || self.e(edge2).next_in_ael == self.e(edge2).prev_in_ael
        {
            return;
        }
        if self.e(edge1).next_in_ael == edge2 {
            let next = self.e(edge2).next_in_ael;
            if next != NIL {
                self.em(next).prev_in_ael = edge1;
            }
            let prev = self.e(edge1).prev_in_ael;
            if prev != NIL {
                self.em(prev).next_in_ael = edge2;
            }
            self.em(edge2).prev_in_ael = prev;
            self.em(edge2).next_in_ael = edge1;
            self.em(edge1).prev_in_ael = edge2;
            self.em(edge1).next_in_ael = next;
        } else if self.e(edge2).next_in_ael == edge1 {
            let next = self.e(edge1).next_in_ael;
            if next != NIL {
                self.em(next).prev_in_ael = edge2;
            }
            let prev = self.e(edge2).prev_in_ael;
            if prev != NIL {
                self.em(prev).next_in_ael = edge1;
            }
            self.em(edge1).prev_in_ael = prev;
            self.em(edge1).next_in_ael = edge2;
            self.em(edge2).prev_in_ael = edge1;
            self.em(edge2).next_in_ael = next;
        } else {
            let next = self.e(edge1).next_in_ael;
            let prev = self.e(edge1).prev_in_ael;
            let n2 = self.e(edge2).next_in_ael;
            self.em(edge1).next_in_ael = n2;
            if n2 != NIL {
                self.em(n2).prev_in_ael = edge1;
            }
            let p2 = self.e(edge2).prev_in_ael;
            self.em(edge1).prev_in_ael = p2;
            if p2 != NIL {
                self.em(p2).next_in_ael = edge1;
            }
            self.em(edge2).next_in_ael = next;
            if next != NIL {
                self.em(next).prev_in_ael = edge2;
            }
            self.em(edge2).prev_in_ael = prev;
            if prev != NIL {
                self.em(prev).next_in_ael = edge2;
            }
        }
        if self.e(edge1).prev_in_ael == NIL {
            self.active_edges = edge1;
        } else if self.e(edge2).prev_in_ael == NIL {
            self.active_edges = edge2;
        }
    }

    /// `SwapPositionsInSEL`.
    fn swap_positions_in_sel(&mut self, edge1: u32, edge2: u32) {
        if self.e(edge1).next_in_sel == NIL && self.e(edge1).prev_in_sel == NIL {
            return;
        }
        if self.e(edge2).next_in_sel == NIL && self.e(edge2).prev_in_sel == NIL {
            return;
        }
        if self.e(edge1).next_in_sel == edge2 {
            let next = self.e(edge2).next_in_sel;
            if next != NIL {
                self.em(next).prev_in_sel = edge1;
            }
            let prev = self.e(edge1).prev_in_sel;
            if prev != NIL {
                self.em(prev).next_in_sel = edge2;
            }
            self.em(edge2).prev_in_sel = prev;
            self.em(edge2).next_in_sel = edge1;
            self.em(edge1).prev_in_sel = edge2;
            self.em(edge1).next_in_sel = next;
        } else if self.e(edge2).next_in_sel == edge1 {
            let next = self.e(edge1).next_in_sel;
            if next != NIL {
                self.em(next).prev_in_sel = edge2;
            }
            let prev = self.e(edge2).prev_in_sel;
            if prev != NIL {
                self.em(prev).next_in_sel = edge1;
            }
            self.em(edge1).prev_in_sel = prev;
            self.em(edge1).next_in_sel = edge2;
            self.em(edge2).prev_in_sel = edge1;
            self.em(edge2).next_in_sel = next;
        } else {
            let next = self.e(edge1).next_in_sel;
            let prev = self.e(edge1).prev_in_sel;
            let n2 = self.e(edge2).next_in_sel;
            self.em(edge1).next_in_sel = n2;
            if n2 != NIL {
                self.em(n2).prev_in_sel = edge1;
            }
            let p2 = self.e(edge2).prev_in_sel;
            self.em(edge1).prev_in_sel = p2;
            if p2 != NIL {
                self.em(p2).next_in_sel = edge1;
            }
            self.em(edge2).next_in_sel = next;
            if next != NIL {
                self.em(next).prev_in_sel = edge2;
            }
            self.em(edge2).prev_in_sel = prev;
            if prev != NIL {
                self.em(prev).next_in_sel = edge2;
            }
        }
        if self.e(edge1).prev_in_sel == NIL {
            self.sorted_edges = edge1;
        } else if self.e(edge2).prev_in_sel == NIL {
            self.sorted_edges = edge2;
        }
    }

    /// The joins of a horizontal edge's last output point `op1` with the overlapping horizontals
    /// still in the SEL, then its ghost join at `ghost`.
    fn join_horizontals(&mut self, horz_edge: u32, op1: u32, ghost: IntPoint) {
        let (hb, ht) = (self.e(horz_edge).bot.x, self.e(horz_edge).top.x);
        let mut e_next_horz = self.sorted_edges;
        while e_next_horz != NIL {
            let n = *self.e(e_next_horz);
            if n.out_idx >= 0 && horz_segments_overlap(hb, ht, n.bot.x, n.top.x) {
                let op2 = self.get_last_out_pt(e_next_horz);
                self.joins.push(Join {
                    out_pt1: op2,
                    out_pt2: op1,
                    off_pt: n.top,
                });
            }
            e_next_horz = n.next_in_sel;
        }
        self.ghost_joins.push(Join {
            out_pt1: op1,
            out_pt2: NIL,
            off_pt: ghost,
        });
    }

    /// `ProcessHorizontal`. Horizontal edges at a scanline are processed as if layered: they
    /// meet other horizontals only at their `Bot.x`, and other edges; intermediate horizontals
    /// then promote the edge above them into the AEL.
    fn process_horizontal(&mut self, mut horz_edge: u32) -> Result<(), ClipperException> {
        let is_open = self.e(horz_edge).wind_delta == 0;
        let (mut dir, mut horz_left, mut horz_right) = get_horz_direction(self.e(horz_edge));

        let mut e_last_horz = horz_edge;
        let mut e_max_pair = NIL;
        loop {
            let nl = self.e(e_last_horz).next_in_lml;
            if nl == NIL || !is_horizontal(self.e(nl)) {
                break;
            }
            e_last_horz = nl;
        }
        if self.e(e_last_horz).next_in_lml == NIL {
            e_max_pair = self.get_maxima_pair(e_last_horz);
        }

        // The C++ iterators into m_Maxima: `max_it` from the front, `max_rit` from the back
        // (the element maxima[n - 1 - max_rit]); n is the end. The C++ leaves the other
        // direction's iterator uninitialised and reads it if the direction flips along a chain
        // of horizontals (undefined); here both start at the end, no maximum in range.
        let n = self.maxima.len();
        let mut max_it = n;
        let mut max_rit = n;
        if !self.maxima.is_empty() {
            // The first maximum in range (x).
            let (hbx, ltx) = (self.e(horz_edge).bot.x, self.e(e_last_horz).top.x);
            if dir == Direction::LeftToRight {
                max_it = 0;
                while max_it != n && self.maxima[max_it] <= hbx {
                    max_it += 1;
                }
                if max_it != n && self.maxima[max_it] >= ltx {
                    max_it = n;
                }
            } else {
                max_rit = 0;
                while max_rit != n && self.maxima[n - 1 - max_rit] > hbx {
                    max_rit += 1;
                }
                if max_rit != n && self.maxima[n - 1 - max_rit] <= ltx {
                    max_rit = n;
                }
            }
        }

        let mut op1 = NIL;
        loop {
            // Through consecutive horizontal edges.
            let is_last_horz = horz_edge == e_last_horz;
            let mut e = if dir == Direction::LeftToRight {
                self.e(horz_edge).next_in_ael
            } else {
                self.e(horz_edge).prev_in_ael
            };
            while e != NIL {
                // Extra coordinates in horizontal output edges wherever maxima touch them.
                if !self.maxima.is_empty() {
                    let ex = self.e(e).curr.x;
                    if dir == Direction::LeftToRight {
                        while max_it != n && self.maxima[max_it] < ex {
                            if self.e(horz_edge).out_idx >= 0 && !is_open {
                                let p = Point::new(self.maxima[max_it], self.e(horz_edge).bot.y);
                                self.add_out_pt(horz_edge, p);
                            }
                            max_it += 1;
                        }
                    } else {
                        while max_rit != n && self.maxima[n - 1 - max_rit] > ex {
                            if self.e(horz_edge).out_idx >= 0 && !is_open {
                                let p = Point::new(
                                    self.maxima[n - 1 - max_rit],
                                    self.e(horz_edge).bot.y,
                                );
                                self.add_out_pt(horz_edge, p);
                            }
                            max_rit += 1;
                        }
                    }
                }

                let ec = *self.e(e);
                if (dir == Direction::LeftToRight && ec.curr.x > horz_right)
                    || (dir == Direction::RightToLeft && ec.curr.x < horz_left)
                {
                    break;
                }
                // Also break at the end of an intermediate horizontal edge. Smaller Dx's are to
                // the right of larger Dx's above the horizontal.
                let hnl = self.e(horz_edge).next_in_lml;
                if ec.curr.x == self.e(horz_edge).top.x && hnl != NIL && ec.dx < self.e(hnl).dx {
                    break;
                }

                if self.e(horz_edge).out_idx >= 0 && !is_open {
                    // ClipperLib_Z: SetZ(e->Curr, ...) here.
                    op1 = self.add_out_pt(horz_edge, ec.curr);
                    let ghost = self.e(horz_edge).bot;
                    self.join_horizontals(horz_edge, op1, ghost);
                }

                // Still in range of the horizontal edge, and at the last of consecutive
                // horizontals when matching with e_max_pair.
                if e == e_max_pair && is_last_horz {
                    if self.e(horz_edge).out_idx >= 0 {
                        let t = self.e(horz_edge).top;
                        self.add_local_max_poly(horz_edge, e_max_pair, t);
                    }
                    self.delete_from_ael(horz_edge);
                    self.delete_from_ael(e_max_pair);
                    return Ok(());
                }

                let pt = Point::new(ec.curr.x, self.e(horz_edge).curr.y);
                if dir == Direction::LeftToRight {
                    self.intersect_edges(horz_edge, e, pt);
                } else {
                    self.intersect_edges(e, horz_edge, pt);
                }
                let e_next = if dir == Direction::LeftToRight {
                    self.e(e).next_in_ael
                } else {
                    self.e(e).prev_in_ael
                };
                self.swap_positions_in_ael(horz_edge, e);
                e = e_next;
            }

            // Out of the loop when the next edge of the bound is not horizontal too.
            let hnl = self.e(horz_edge).next_in_lml;
            if hnl == NIL || !is_horizontal(self.e(hnl)) {
                break;
            }
            self.update_edge_into_ael(&mut horz_edge)?;
            if self.e(horz_edge).out_idx >= 0 {
                let b = self.e(horz_edge).bot;
                self.add_out_pt(horz_edge, b);
            }
            (dir, horz_left, horz_right) = get_horz_direction(self.e(horz_edge));
        }

        if self.e(horz_edge).out_idx >= 0 && op1 == NIL {
            op1 = self.get_last_out_pt(horz_edge);
            let ghost = self.e(horz_edge).top;
            self.join_horizontals(horz_edge, op1, ghost);
        }

        if self.e(horz_edge).next_in_lml != NIL {
            if self.e(horz_edge).out_idx >= 0 {
                let t = self.e(horz_edge).top;
                op1 = self.add_out_pt(horz_edge, t);
                self.update_edge_into_ael(&mut horz_edge)?;
                if self.e(horz_edge).wind_delta == 0 {
                    return Ok(());
                }
                // horz_edge is no longer horizontal here.
                let h = *self.e(horz_edge);
                let (e_prev, e_next) = (h.prev_in_ael, h.next_in_ael);
                let joins_at = |c: &Self, o: u32| {
                    let n = c.e(o);
                    n.curr.x == h.bot.x
                        && n.curr.y == h.bot.y
                        && n.wind_delta != 0
                        && n.out_idx >= 0
                        && n.curr.y > n.top.y
                        && slopes_equal_edges(&h, n, c.use_full_range)
                };
                if e_prev != NIL && joins_at(self, e_prev) {
                    let op2 = self.add_out_pt(e_prev, h.bot);
                    self.joins.push(Join {
                        out_pt1: op1,
                        out_pt2: op2,
                        off_pt: h.top,
                    });
                } else if e_next != NIL && joins_at(self, e_next) {
                    let op2 = self.add_out_pt(e_next, h.bot);
                    self.joins.push(Join {
                        out_pt1: op1,
                        out_pt2: op2,
                        off_pt: h.top,
                    });
                }
            } else {
                self.update_edge_into_ael(&mut horz_edge)?;
            }
        } else {
            if self.e(horz_edge).out_idx >= 0 {
                let t = self.e(horz_edge).top;
                self.add_out_pt(horz_edge, t);
            }
            self.delete_from_ael(horz_edge);
        }
        Ok(())
    }

    /// `UpdateEdgeIntoAEL`: replaces the edge in the AEL by the next edge of its bound.
    fn update_edge_into_ael(&mut self, e: &mut u32) -> Result<(), ClipperException> {
        let cur = *self.e(*e);
        let next = cur.next_in_lml;
        if next == NIL {
            return Err(ClipperException::UPDATE_EDGE_INTO_AEL);
        }
        self.em(next).out_idx = cur.out_idx;
        if cur.prev_in_ael != NIL {
            self.em(cur.prev_in_ael).next_in_ael = next;
        } else {
            self.active_edges = next;
        }
        if cur.next_in_ael != NIL {
            self.em(cur.next_in_ael).prev_in_ael = next;
        }
        *e = next;
        let n = self.em(next);
        n.side = cur.side;
        n.wind_delta = cur.wind_delta;
        n.wind_cnt = cur.wind_cnt;
        n.wind_cnt2 = cur.wind_cnt2;
        n.curr = n.bot;
        n.prev_in_ael = cur.prev_in_ael;
        n.next_in_ael = cur.next_in_ael;
        if !is_horizontal(n) {
            let y = n.top.y;
            self.scanbeam.push(y);
        }
        Ok(())
    }

    /// `ProcessIntersections`: false when no order of the intersections keeps them between
    /// adjacent edges. The C++ catches any exception here to throw "ProcessIntersections error";
    /// nothing in it throws a `clipperException`, so that path is not translated.
    fn process_intersections(&mut self, top_y: i64) -> bool {
        if self.active_edges == NIL {
            return true;
        }
        self.build_intersect_list(top_y);
        let il_size = self.intersect_list.len();
        if il_size == 0 {
            return true;
        }
        if il_size == 1 || self.fixup_intersection_order() {
            for k in 0..self.intersect_list.len() {
                let node = self.intersect_list[k];
                self.intersect_edges(node.edge1, node.edge2, node.pt);
                self.swap_positions_in_ael(node.edge1, node.edge2);
            }
            self.intersect_list.clear();
        } else {
            return false;
        }
        self.sorted_edges = NIL;
        true
    }

    /// `BuildIntersectList`: bubble-sorts the SEL by x at `top_y`, recording each swap.
    fn build_intersect_list(&mut self, top_y: i64) {
        if self.active_edges == NIL {
            return;
        }
        // Prepare for sorting.
        let mut e = self.active_edges;
        self.sorted_edges = e;
        while e != NIL {
            let x = top_x(self.e(e), top_y);
            let ed = self.em(e);
            ed.prev_in_sel = ed.prev_in_ael;
            ed.next_in_sel = ed.next_in_ael;
            ed.curr.x = x;
            e = ed.next_in_ael;
        }
        // Bubble sort.
        loop {
            let mut is_modified = false;
            e = self.sorted_edges;
            while self.e(e).next_in_sel != NIL {
                let e_next = self.e(e).next_in_sel;
                if self.e(e).curr.x > self.e(e_next).curr.x {
                    let mut pt = intersect_point(self.e(e), self.e(e_next));
                    if pt.y < top_y {
                        pt = Point::new(top_x(self.e(e), top_y), top_y);
                    }
                    self.intersect_list.push(IntersectNode {
                        edge1: e,
                        edge2: e_next,
                        pt,
                    });
                    self.swap_positions_in_sel(e, e_next);
                    is_modified = true;
                } else {
                    e = e_next;
                }
            }
            let p = self.e(e).prev_in_sel;
            if p == NIL {
                break;
            }
            self.em(p).next_in_sel = NIL;
            if !is_modified {
                break;
            }
        }
        self.sorted_edges = NIL; // Important.
    }

    /// `EdgesAdjacent`.
    fn edges_adjacent(&self, inode: &IntersectNode) -> bool {
        self.e(inode.edge1).next_in_sel == inode.edge2
            || self.e(inode.edge1).prev_in_sel == inode.edge2
    }

    /// `FixupIntersectionOrder`: orders the intersections bottom first (`std::sort`) so that
    /// each is between adjacent edges; false when none fits.
    fn fixup_intersection_order(&mut self) -> bool {
        self.copy_ael_to_sel();
        orca_stl::sort_by(&mut self.intersect_list, |node1, node2| {
            node2.pt.y < node1.pt.y
        });
        let cnt = self.intersect_list.len();
        for i in 0..cnt {
            if !self.edges_adjacent(&self.intersect_list[i]) {
                let mut j = i + 1;
                while j < cnt && !self.edges_adjacent(&self.intersect_list[j]) {
                    j += 1;
                }
                if j == cnt {
                    return false;
                }
                self.intersect_list.swap(i, j);
            }
            let node = self.intersect_list[i];
            self.swap_positions_in_sel(node.edge1, node.edge2);
        }
        true
    }

    /// `DoMaxima`.
    fn do_maxima(&mut self, e: u32) -> Result<(), ClipperException> {
        let e_max_pair = self.get_maxima_pair_ex(e);
        if e_max_pair == NIL {
            if self.e(e).out_idx >= 0 {
                let t = self.e(e).top;
                self.add_out_pt(e, t);
            }
            self.delete_from_ael(e);
            return Ok(());
        }

        let mut e_next = self.e(e).next_in_ael;
        while e_next != NIL && e_next != e_max_pair {
            let t = self.e(e).top;
            self.intersect_edges(e, e_next, t);
            self.swap_positions_in_ael(e, e_next);
            e_next = self.e(e).next_in_ael;
        }

        if self.e(e).out_idx == UNASSIGNED && self.e(e_max_pair).out_idx == UNASSIGNED {
            self.delete_from_ael(e);
            self.delete_from_ael(e_max_pair);
        } else if self.e(e).out_idx >= 0 && self.e(e_max_pair).out_idx >= 0 {
            let t = self.e(e).top;
            self.add_local_max_poly(e, e_max_pair, t);
            self.delete_from_ael(e);
            self.delete_from_ael(e_max_pair);
        } else if self.e(e).wind_delta == 0 {
            // use_lines.
            let t = self.e(e).top;
            if self.e(e).out_idx >= 0 {
                self.add_out_pt(e, t);
                self.em(e).out_idx = UNASSIGNED;
            }
            self.delete_from_ael(e);
            if self.e(e_max_pair).out_idx >= 0 {
                self.add_out_pt(e_max_pair, t);
                self.em(e_max_pair).out_idx = UNASSIGNED;
            }
            self.delete_from_ael(e_max_pair);
        } else {
            return Err(ClipperException::DO_MAXIMA);
        }
        Ok(())
    }

    /// `ProcessEdgesAtTopOfScanbeam`.
    fn process_edges_at_top_of_scanbeam(&mut self, top_y: i64) -> Result<(), ClipperException> {
        let mut e = self.active_edges;
        while e != NIL {
            // 1. Process maxima, as if 'bent' horizontal edges, but not maxima with horizontal
            // edges. e can't be horizontal.
            let mut is_maxima_edge = self.is_maxima(e, top_y);
            if is_maxima_edge {
                let mp = self.get_maxima_pair_ex(e);
                is_maxima_edge = mp == NIL || !is_horizontal(self.e(mp));
            }
            if is_maxima_edge {
                if self.strict_simple {
                    let x = self.e(e).top.x;
                    self.maxima.push(x);
                }
                let e_prev = self.e(e).prev_in_ael;
                self.do_maxima(e)?;
                e = if e_prev == NIL {
                    self.active_edges
                } else {
                    self.e(e_prev).next_in_ael
                };
            } else {
                // 2. Promote horizontal edges, otherwise update Curr.
                if self.is_intermediate(e, top_y) && is_horizontal(self.e(self.e(e).next_in_lml)) {
                    self.update_edge_into_ael(&mut e)?;
                    if self.e(e).out_idx >= 0 {
                        let b = self.e(e).bot;
                        self.add_out_pt(e, b);
                    }
                    self.add_edge_to_sel(e);
                } else {
                    let x = top_x(self.e(e), top_y);
                    let ed = self.em(e);
                    ed.curr.x = x;
                    ed.curr.y = top_y;
                    // ClipperLib_Z: Curr's z from Top or Bot here.
                }

                // When StrictlySimple and another edge touches e, both edges get a vertex here.
                if self.strict_simple {
                    let e_prev = self.e(e).prev_in_ael;
                    let ed = *self.e(e);
                    if ed.out_idx >= 0
                        && ed.wind_delta != 0
                        && e_prev != NIL
                        && self.e(e_prev).out_idx >= 0
                        && self.e(e_prev).curr.x == ed.curr.x
                        && self.e(e_prev).wind_delta != 0
                    {
                        let pt = ed.curr;
                        // ClipperLib_Z: SetZ(pt, ePrev, e) here.
                        let op = self.add_out_pt(e_prev, pt);
                        let op2 = self.add_out_pt(e, pt);
                        // A StrictlySimple (type 3) join.
                        self.joins.push(Join {
                            out_pt1: op,
                            out_pt2: op2,
                            off_pt: pt,
                        });
                    }
                }
                e = self.e(e).next_in_ael;
            }
        }

        // 3. Process the horizontals at the top of the scanbeam.
        orca_stl::sort(&mut self.maxima);
        self.process_horizontals()?;
        self.maxima.clear();

        // 4. Promote intermediate vertices.
        e = self.active_edges;
        while e != NIL {
            if self.is_intermediate(e, top_y) {
                let mut op = NIL;
                if self.e(e).out_idx >= 0 {
                    let t = self.e(e).top;
                    op = self.add_out_pt(e, t);
                }
                self.update_edge_into_ael(&mut e)?;

                // Output polygons that share an edge need joining later.
                let ed = *self.e(e);
                let (e_prev, e_next) = (ed.prev_in_ael, ed.next_in_ael);
                let joins_at = |c: &Self, o: u32| {
                    let n = c.e(o);
                    n.curr.x == ed.bot.x
                        && n.curr.y == ed.bot.y
                        && op != NIL
                        && n.out_idx >= 0
                        && n.curr.y > n.top.y
                        && slopes_equal4(ed.curr, ed.top, n.curr, n.top, c.use_full_range)
                        && ed.wind_delta != 0
                        && n.wind_delta != 0
                };
                if e_prev != NIL && joins_at(self, e_prev) {
                    let op2 = self.add_out_pt(e_prev, ed.bot);
                    self.joins.push(Join {
                        out_pt1: op,
                        out_pt2: op2,
                        off_pt: ed.top,
                    });
                } else if e_next != NIL && joins_at(self, e_next) {
                    let op2 = self.add_out_pt(e_next, ed.bot);
                    self.joins.push(Join {
                        out_pt1: op,
                        out_pt2: op2,
                        off_pt: ed.top,
                    });
                }
            }
            e = self.e(e).next_in_ael;
        }
        Ok(())
    }

    /// `FixupOutPolyline`: removes duplicate points.
    fn fixup_out_polyline(&mut self, outrec: u32) {
        let mut pp = self.r(outrec).pts;
        let mut last_pp = self.o(pp).prev;
        while pp != last_pp {
            pp = self.o(pp).next;
            if self.o(pp).pt == self.o(self.o(pp).prev).pt {
                if pp == last_pp {
                    last_pp = self.o(pp).prev;
                }
                let tmp_pp = self.o(pp).prev;
                let next = self.o(pp).next;
                self.om(tmp_pp).next = next;
                self.om(next).prev = tmp_pp;
                self.dispose_out_pt(pp);
                pp = tmp_pp;
            }
        }
        if pp == self.o(pp).prev {
            self.dispose_out_pts(pp);
            self.rm(outrec).pts = NIL;
        }
    }

    /// `FixupOutPolygon`: removes duplicate points and the middle vertex of consecutive parallel
    /// edges.
    fn fixup_out_polygon(&mut self, outrec: u32) {
        let mut last_ok = NIL;
        self.rm(outrec).bottom_pt = NIL;
        let mut pp = self.r(outrec).pts;
        let preserve_col = self.preserve_collinear || self.strict_simple;
        loop {
            let (prev, next) = (self.o(pp).prev, self.o(pp).next);
            if prev == pp || prev == next {
                // An empty loop or a stick: release the polygon.
                self.dispose_out_pts(pp);
                self.rm(outrec).pts = NIL;
                return;
            }
            // Duplicate points and collinear edges.
            let (pv, pc, pn) = (self.o(prev).pt, self.o(pp).pt, self.o(next).pt);
            if pc == pn
                || pc == pv
                || (slopes_equal3(pv, pc, pn, self.use_full_range)
                    && (!preserve_col || !pt2_is_between_pt1_and_pt3(pv, pc, pn)))
            {
                last_ok = NIL;
                let tmp = pp;
                self.om(prev).next = next;
                self.om(next).prev = prev;
                pp = prev;
                self.dispose_out_pt(tmp);
            } else if pp == last_ok {
                break;
            } else {
                if last_ok == NIL {
                    last_ok = pp;
                }
                pp = next;
            }
        }
        self.rm(outrec).pts = pp;
    }

    /// `PointCount`: the points of a loop, 0 for none.
    fn point_count(&self, pts: u32) -> usize {
        if pts == NIL {
            return 0;
        }
        let mut result = 0;
        let mut p = pts;
        loop {
            result += 1;
            p = self.o(p).next;
            if p == pts {
                break;
            }
        }
        result
    }

    /// The loop's points from `pts.Prev` backwards, `cnt` of them, as the results take them.
    fn contour(&self, pts: u32, cnt: usize) -> Path {
        let mut pg = Path::with_capacity(cnt);
        let mut p = self.o(pts).prev;
        for _ in 0..cnt {
            pg.push(self.o(p).pt);
            p = self.o(p).prev;
        }
        pg
    }

    /// `BuildResult`: every loop of two or more points.
    fn build_result(&self, polys: &mut Paths) {
        polys.reserve(self.poly_outs.len());
        for rec in &self.poly_outs {
            if rec.pts == NIL {
                continue;
            }
            let cnt = self.point_count(self.o(rec.pts).prev);
            if cnt < 2 {
                continue;
            }
            polys.push(self.contour(rec.pts, cnt));
        }
    }

    /// `BuildResult2`: the closed loops of three or more points and the open paths of two or
    /// more, linked by their `FirstLeft` owners.
    fn build_result2(&mut self, polytree: &mut PolyTree) {
        polytree.clear();
        // Each output polygon or contour to the tree.
        for i in 0..self.poly_outs.len() {
            let rec = self.poly_outs[i];
            let cnt = self.point_count(rec.pts);
            if (rec.is_open && cnt < 2) || (!rec.is_open && cnt < 3) {
                continue; // An invalid output loop or polyline.
            }
            // Skip records that hold outermost polygons or already have the right owner.
            let fl = rec.first_left;
            if fl != NIL && (rec.is_hole == self.r(fl).is_hole || self.r(fl).pts == NIL) {
                let mut orfl = fl;
                while orfl != NIL
                    && (self.r(orfl).is_hole == rec.is_hole || self.r(orfl).pts == NIL)
                {
                    orfl = self.r(orfl).first_left;
                }
                self.poly_outs[i].first_left = orfl;
            }
            let pn = polytree.push_node(self.contour(rec.pts, cnt));
            self.poly_outs[i].poly_nd = pn as u32;
        }
        // Fix up the links.
        for rec in &self.poly_outs {
            if rec.poly_nd == NIL {
                continue;
            }
            let pn = rec.poly_nd as usize;
            if rec.is_open {
                polytree.set_open(pn);
                polytree.add_child(ROOT, pn);
            } else if rec.first_left != NIL && self.r(rec.first_left).poly_nd != NIL {
                polytree.add_child(self.r(rec.first_left).poly_nd as usize, pn);
            } else {
                polytree.add_child(ROOT, pn);
            }
        }
    }

    /// `UpdateOutPtIdxs`: every point of the record points to its `Idx`.
    fn update_out_pt_idxs(&mut self, outrec: u32) {
        let (pts, idx) = (self.r(outrec).pts, self.r(outrec).idx);
        let mut op = pts;
        loop {
            self.om(op).idx = idx;
            op = self.o(op).prev;
            if op == pts {
                break;
            }
        }
    }

    /// `InsertEdgeIntoAEL`.
    fn insert_edge_into_ael(&mut self, edge: u32, start_edge: u32) {
        let active = self.active_edges;
        if active == NIL {
            self.em(edge).prev_in_ael = NIL;
            self.em(edge).next_in_ael = NIL;
            self.active_edges = edge;
        } else if start_edge == NIL && e2_inserts_before_e1(self.e(active), self.e(edge)) {
            self.em(edge).prev_in_ael = NIL;
            self.em(edge).next_in_ael = active;
            self.em(active).prev_in_ael = edge;
            self.active_edges = edge;
        } else {
            let mut start = if start_edge == NIL {
                active
            } else {
                start_edge
            };
            loop {
                let n = self.e(start).next_in_ael;
                if n == NIL || e2_inserts_before_e1(self.e(n), self.e(edge)) {
                    break;
                }
                start = n;
            }
            let sn = self.e(start).next_in_ael;
            self.em(edge).next_in_ael = sn;
            if sn != NIL {
                self.em(sn).prev_in_ael = edge;
            }
            self.em(edge).prev_in_ael = start;
            self.em(start).next_in_ael = edge;
        }
    }

    /// `DupOutPt`.
    fn dup_out_pt(&mut self, out_pt: u32, insert_after: bool) -> u32 {
        let result = self.allocate_out_pt();
        let o = *self.o(out_pt);
        self.om(result).pt = o.pt;
        self.om(result).idx = o.idx;
        if insert_after {
            self.om(result).next = o.next;
            self.om(result).prev = out_pt;
            self.om(o.next).prev = result;
            self.om(out_pt).next = result;
        } else {
            self.om(result).prev = o.prev;
            self.om(result).next = out_pt;
            self.om(o.prev).next = result;
            self.om(out_pt).prev = result;
        }
        result
    }

    /// One side of `JoinHorz`: moves `op` along the horizontal to `pt` and duplicates it there.
    fn join_horz_side(
        &mut self,
        mut op: u32,
        dir: Direction,
        pt: IntPoint,
        discard_left: bool,
    ) -> (u32, u32) {
        let opb;
        if dir == Direction::LeftToRight {
            loop {
                let (n, c) = (self.o(self.o(op).next).pt, self.o(op).pt);
                if !(n.x <= pt.x && n.x >= c.x && n.y == pt.y) {
                    break;
                }
                op = self.o(op).next;
            }
            if discard_left && self.o(op).pt.x != pt.x {
                op = self.o(op).next;
            }
            let mut b = self.dup_out_pt(op, !discard_left);
            if self.o(b).pt != pt {
                op = b;
                self.om(op).pt = pt;
                b = self.dup_out_pt(op, !discard_left);
            }
            opb = b;
        } else {
            loop {
                let (n, c) = (self.o(self.o(op).next).pt, self.o(op).pt);
                if !(n.x >= pt.x && n.x <= c.x && n.y == pt.y) {
                    break;
                }
                op = self.o(op).next;
            }
            if !discard_left && self.o(op).pt.x != pt.x {
                op = self.o(op).next;
            }
            let mut b = self.dup_out_pt(op, discard_left);
            if self.o(b).pt != pt {
                op = b;
                self.om(op).pt = pt;
                b = self.dup_out_pt(op, discard_left);
            }
            opb = b;
        }
        (op, opb)
    }

    /// `JoinHorz`.
    fn join_horz(
        &mut self,
        op1: u32,
        op1b: u32,
        op2: u32,
        op2b: u32,
        pt: IntPoint,
        discard_left: bool,
    ) -> bool {
        let dir = |c: &Self, a: u32, b: u32| {
            if c.o(a).pt.x > c.o(b).pt.x {
                Direction::RightToLeft
            } else {
                Direction::LeftToRight
            }
        };
        let dir1 = dir(self, op1, op1b);
        let dir2 = dir(self, op2, op2b);
        if dir1 == dir2 {
            return false;
        }
        // When DiscardLeft, op1b goes left of op1, otherwise right (and likewise op2 and op2b):
        // so with DiscardLeft move at or right of pt before adding op1b, otherwise at or left.
        let (op1, op1b) = self.join_horz_side(op1, dir1, pt, discard_left);
        let (op2, op2b) = self.join_horz_side(op2, dir2, pt, discard_left);
        if (dir1 == Direction::LeftToRight) == discard_left {
            self.om(op1).prev = op2;
            self.om(op2).next = op1;
            self.om(op1b).next = op2b;
            self.om(op2b).prev = op1b;
        } else {
            self.om(op1).next = op2;
            self.om(op2).prev = op1;
            self.om(op1b).prev = op2b;
            self.om(op2b).next = op1b;
        }
        true
    }

    /// The two ways `JoinPoints` links a non-horizontal or StrictlySimple join, as `reverse` says.
    fn join_points_link(&mut self, ji: usize, op1: u32, op2: u32, reverse: bool) {
        let op1b;
        if reverse {
            op1b = self.dup_out_pt(op1, false);
            let op2b = self.dup_out_pt(op2, true);
            self.om(op1).prev = op2;
            self.om(op2).next = op1;
            self.om(op1b).next = op2b;
            self.om(op2b).prev = op1b;
        } else {
            op1b = self.dup_out_pt(op1, true);
            let op2b = self.dup_out_pt(op2, false);
            self.om(op1).next = op2;
            self.om(op2).prev = op1;
            self.om(op1b).prev = op2b;
            self.om(op2b).next = op1b;
        }
        self.joins[ji].out_pt1 = op1;
        self.joins[ji].out_pt2 = op1b;
    }

    /// `JoinPoints`: joins the join's two points, of the records `out_rec1` and `out_rec2`.
    fn join_points(&mut self, ji: usize, out_rec1: u32, out_rec2: u32) -> bool {
        let j = self.joins[ji];
        let mut op1 = j.out_pt1;
        let mut op2 = j.out_pt2;
        // Three kinds of joins: 1. horizontal, where the points are anywhere along collinear
        // horizontal edges (OffPt on the same horizontal); 2. non-horizontal, the points at the
        // bottom of the overlap (OffPt above); 3. StrictlySimple, where edges touch but are not
        // collinear and the points and OffPt coincide.
        let is_horizontal = self.o(op1).pt.y == j.off_pt.y;

        if is_horizontal && j.off_pt == self.o(op1).pt && j.off_pt == self.o(op2).pt {
            // Strictly simple join.
            if out_rec1 != out_rec2 {
                return false;
            }
            let mut op1b = self.o(op1).next;
            while op1b != op1 && self.o(op1b).pt == j.off_pt {
                op1b = self.o(op1b).next;
            }
            let reverse1 = self.o(op1b).pt.y > j.off_pt.y;
            let mut op2b = self.o(op2).next;
            while op2b != op2 && self.o(op2b).pt == j.off_pt {
                op2b = self.o(op2b).next;
            }
            let reverse2 = self.o(op2b).pt.y > j.off_pt.y;
            if reverse1 == reverse2 {
                return false;
            }
            self.join_points_link(ji, op1, op2, reverse1);
            true
        } else if is_horizontal {
            // The overlap of horizontal joins is not known yet: the points may be anywhere along
            // the horizontal edges.
            let mut op1b = op1;
            loop {
                let p = self.o(op1).prev;
                if !(self.o(p).pt.y == self.o(op1).pt.y && p != op1b && p != op2) {
                    break;
                }
                op1 = p;
            }
            loop {
                let n = self.o(op1b).next;
                if !(self.o(n).pt.y == self.o(op1b).pt.y && n != op1 && n != op2) {
                    break;
                }
                op1b = n;
            }
            let n = self.o(op1b).next;
            if n == op1 || n == op2 {
                return false; // A flat 'polygon'.
            }

            let mut op2b = op2;
            loop {
                let p = self.o(op2).prev;
                if !(self.o(p).pt.y == self.o(op2).pt.y && p != op2b && p != op1b) {
                    break;
                }
                op2 = p;
            }
            loop {
                let n = self.o(op2b).next;
                if !(self.o(n).pt.y == self.o(op2b).pt.y && n != op2 && n != op1) {
                    break;
                }
                op2b = n;
            }
            let n = self.o(op2b).next;
            if n == op2 || n == op1 {
                return false; // A flat 'polygon'.
            }

            // op1 to op1b and op2 to op2b are the extremities of the horizontal edges.
            let (p1, p1b, p2, p2b) = (
                self.o(op1).pt,
                self.o(op1b).pt,
                self.o(op2).pt,
                self.o(op2b).pt,
            );
            let Some((left, right)) = get_overlap(p1.x, p1b.x, p2.x, p2b.x) else {
                return false;
            };

            // DiscardLeftSide: joining overlapping edges makes a spike to clean up, but op1 and
            // op2 must not be caught on the discarded side, as other joins may need them.
            let (pt, discard_left_side) = if p1.x >= left && p1.x <= right {
                (p1, p1.x > p1b.x)
            } else if p2.x >= left && p2.x <= right {
                (p2, p2.x > p2b.x)
            } else if p1b.x >= left && p1b.x <= right {
                (p1b, p1b.x > p1.x)
            } else {
                (p2b, p2b.x > p2.x)
            };
            self.joins[ji].out_pt1 = op1;
            self.joins[ji].out_pt2 = op2;
            self.join_horz(op1, op1b, op2, op2b, pt, discard_left_side)
        } else {
            // Non-horizontal joins: OutPt1.y == OutPt2.y, and OutPt1.y > OffPt.y.
            // Make sure the polygons are correctly oriented.
            let ufr = self.use_full_range;
            let p1 = self.o(op1).pt;
            let mut op1b = self.o(op1).next;
            while self.o(op1b).pt == p1 && op1b != op1 {
                op1b = self.o(op1b).next;
            }
            let reverse1 =
                self.o(op1b).pt.y > p1.y || !slopes_equal3(p1, self.o(op1b).pt, j.off_pt, ufr);
            if reverse1 {
                op1b = self.o(op1).prev;
                while self.o(op1b).pt == p1 && op1b != op1 {
                    op1b = self.o(op1b).prev;
                }
                if self.o(op1b).pt.y > p1.y || !slopes_equal3(p1, self.o(op1b).pt, j.off_pt, ufr) {
                    return false;
                }
            }
            let p2 = self.o(op2).pt;
            let mut op2b = self.o(op2).next;
            while self.o(op2b).pt == p2 && op2b != op2 {
                op2b = self.o(op2b).next;
            }
            let reverse2 =
                self.o(op2b).pt.y > p2.y || !slopes_equal3(p2, self.o(op2b).pt, j.off_pt, ufr);
            if reverse2 {
                op2b = self.o(op2).prev;
                while self.o(op2b).pt == p2 && op2b != op2 {
                    op2b = self.o(op2b).prev;
                }
                if self.o(op2b).pt.y > p2.y || !slopes_equal3(p2, self.o(op2b).pt, j.off_pt, ufr) {
                    return false;
                }
            }
            if op1b == op1
                || op2b == op2
                || op1b == op2b
                || (out_rec1 == out_rec2 && reverse1 == reverse2)
            {
                return false;
            }
            self.join_points_link(ji, op1, op2, reverse1);
            true
        }
    }

    /// `ParseFirstLeft`: the first owner that still has points.
    fn parse_first_left(&self, mut first_left: u32) -> u32 {
        while first_left != NIL && self.r(first_left).pts == NIL {
            first_left = self.r(first_left).first_left;
        }
        first_left
    }

    /// `Poly2ContainsPoly1`: whether the first loop is inside the second, judged by its first
    /// point not on the second's boundary.
    fn poly2_contains_poly1(&self, out_pt1: u32, out_pt2: u32) -> bool {
        let mut op = out_pt1;
        loop {
            let res = self.point_in_polygon_op(self.o(op).pt, out_pt2);
            if res >= 0 {
                return res > 0;
            }
            op = self.o(op).next;
            if op == out_pt1 {
                break;
            }
        }
        true
    }

    /// `PointInPolygon(const IntPoint&, OutPt*)`.
    fn point_in_polygon_op(&self, pt: IntPoint, op: u32) -> i32 {
        let mut result = 0;
        let start_op = op;
        let mut op = op;
        loop {
            let o = *self.o(op);
            let n = self.o(o.next).pt;
            let c = o.pt;
            if n.y == pt.y && (n.x == pt.x || (c.y == pt.y && ((n.x > pt.x) == (c.x < pt.x)))) {
                return -1;
            }
            if (c.y < pt.y) != (n.y < pt.y) {
                if c.x >= pt.x {
                    if n.x > pt.x {
                        result = 1 - result;
                    } else {
                        let d = cross_from(pt, c, n);
                        if d == 0.0 {
                            return -1;
                        }
                        if (d > 0.0) == (n.y > c.y) {
                            result = 1 - result;
                        }
                    }
                } else if n.x > pt.x {
                    let d = cross_from(pt, c, n);
                    if d == 0.0 {
                        return -1;
                    }
                    if (d > 0.0) == (n.y > c.y) {
                        result = 1 - result;
                    }
                }
            }
            op = o.next;
            if op == start_op {
                break;
            }
        }
        result
    }

    /// `FixupFirstLefts1`: records owned by `old_out_rec` and inside `new_out_rec` move to it.
    fn fixup_first_lefts1(&mut self, old_out_rec: u32, new_out_rec: u32) {
        for i in 0..self.poly_outs.len() {
            let rec = self.poly_outs[i];
            let first_left = self.parse_first_left(rec.first_left);
            if rec.pts != NIL
                && first_left == old_out_rec
                && self.poly2_contains_poly1(rec.pts, self.r(new_out_rec).pts)
            {
                self.poly_outs[i].first_left = new_out_rec;
            }
        }
    }

    /// `FixupFirstLefts2`: after a polygon split into an inner and an outer one, the records
    /// owned by the outer one's owner, the inner or the outer one move to whichever holds them.
    fn fixup_first_lefts2(&mut self, inner_out_rec: u32, outer_out_rec: u32) {
        let orfl = self.r(outer_out_rec).first_left;
        for i in 0..self.poly_outs.len() {
            let rec = self.poly_outs[i];
            if rec.pts == NIL || ix(i) == outer_out_rec || ix(i) == inner_out_rec {
                continue;
            }
            let first_left = self.parse_first_left(rec.first_left);
            if first_left != orfl && first_left != inner_out_rec && first_left != outer_out_rec {
                continue;
            }
            if self.poly2_contains_poly1(rec.pts, self.r(inner_out_rec).pts) {
                self.poly_outs[i].first_left = inner_out_rec;
            } else if self.poly2_contains_poly1(rec.pts, self.r(outer_out_rec).pts) {
                self.poly_outs[i].first_left = outer_out_rec;
            } else if rec.first_left == inner_out_rec || rec.first_left == outer_out_rec {
                self.poly_outs[i].first_left = orfl;
            }
        }
    }

    /// `FixupFirstLefts3`: records owned by `old_out_rec` move to `new_out_rec`, untested.
    fn fixup_first_lefts3(&mut self, old_out_rec: u32, new_out_rec: u32) {
        for i in 0..self.poly_outs.len() {
            let rec = self.poly_outs[i];
            let first_left = self.parse_first_left(rec.first_left);
            if rec.pts != NIL && first_left == old_out_rec {
                self.poly_outs[i].first_left = new_out_rec;
            }
        }
    }

    /// `JoinCommonEdges`.
    fn join_common_edges(&mut self) {
        for ji in 0..self.joins.len() {
            let join = self.joins[ji];
            let out_rec1 = self.get_out_rec(self.o(join.out_pt1).idx);
            let mut out_rec2 = self.get_out_rec(self.o(join.out_pt2).idx);
            if self.r(out_rec1).pts == NIL || self.r(out_rec2).pts == NIL {
                continue;
            }
            if self.r(out_rec1).is_open || self.r(out_rec2).is_open {
                continue;
            }
            // The fragment with the correct hole state (FirstLeft), before JoinPoints().
            let hole_state_rec = if out_rec1 == out_rec2 {
                out_rec1
            } else if self.out_rec1_right_of_out_rec2(out_rec1, out_rec2) {
                out_rec2
            } else if self.out_rec1_right_of_out_rec2(out_rec2, out_rec1) {
                out_rec1
            } else {
                self.get_lowermost_rec(out_rec1, out_rec2)
            };

            if !self.join_points(ji, out_rec1, out_rec2) {
                continue;
            }
            let join = self.joins[ji];

            if out_rec1 == out_rec2 {
                // One polygon split into two.
                self.rm(out_rec1).pts = join.out_pt1;
                self.rm(out_rec1).bottom_pt = NIL;
                out_rec2 = self.create_out_rec();
                self.rm(out_rec2).pts = join.out_pt2;
                self.update_out_pt_idxs(out_rec2);

                if self.poly2_contains_poly1(self.r(out_rec2).pts, self.r(out_rec1).pts) {
                    // out_rec1 contains out_rec2.
                    let hole = !self.r(out_rec1).is_hole;
                    self.rm(out_rec2).is_hole = hole;
                    self.rm(out_rec2).first_left = out_rec1;
                    if self.using_poly_tree {
                        self.fixup_first_lefts2(out_rec2, out_rec1);
                    }
                    let r2 = *self.r(out_rec2);
                    if (r2.is_hole ^ self.reverse_output) == (self.area_op(r2.pts) > 0.0) {
                        self.reverse_poly_pt_links(r2.pts);
                    }
                } else if self.poly2_contains_poly1(self.r(out_rec1).pts, self.r(out_rec2).pts) {
                    // out_rec2 contains out_rec1.
                    let hole = self.r(out_rec1).is_hole;
                    self.rm(out_rec2).is_hole = hole;
                    self.rm(out_rec1).is_hole = !hole;
                    let fl = self.r(out_rec1).first_left;
                    self.rm(out_rec2).first_left = fl;
                    self.rm(out_rec1).first_left = out_rec2;
                    if self.using_poly_tree {
                        self.fixup_first_lefts2(out_rec1, out_rec2);
                    }
                    let r1 = *self.r(out_rec1);
                    if (r1.is_hole ^ self.reverse_output) == (self.area_op(r1.pts) > 0.0) {
                        self.reverse_poly_pt_links(r1.pts);
                    }
                } else {
                    // The two polygons are separate.
                    let (hole, fl) = (self.r(out_rec1).is_hole, self.r(out_rec1).first_left);
                    self.rm(out_rec2).is_hole = hole;
                    self.rm(out_rec2).first_left = fl;
                    if self.using_poly_tree {
                        self.fixup_first_lefts1(out_rec1, out_rec2);
                    }
                }
            } else {
                // Two polygons joined.
                self.rm(out_rec2).pts = NIL;
                self.rm(out_rec2).bottom_pt = NIL;
                let idx = self.r(out_rec1).idx;
                self.rm(out_rec2).idx = idx;
                let hole = self.r(hole_state_rec).is_hole;
                self.rm(out_rec1).is_hole = hole;
                if hole_state_rec == out_rec2 {
                    let fl = self.r(out_rec2).first_left;
                    self.rm(out_rec1).first_left = fl;
                }
                self.rm(out_rec2).first_left = out_rec1;
                if self.using_poly_tree {
                    self.fixup_first_lefts3(out_rec2, out_rec1);
                }
            }
        }
    }

    /// `DoSimplePolygons`: splits each loop at a repeated (not successive) point.
    fn do_simple_polygons(&mut self) {
        let mut i = 0;
        while i < self.poly_outs.len() {
            let outrec = ix(i);
            i += 1;
            let mut op = self.r(outrec).pts;
            if op == NIL || self.r(outrec).is_open {
                continue;
            }
            loop {
                // Each point until a duplicate is found.
                let mut op2 = self.o(op).next;
                while op2 != self.r(outrec).pts {
                    if self.o(op).pt == self.o(op2).pt
                        && self.o(op2).next != op
                        && self.o(op2).prev != op
                    {
                        // Split the polygon in two.
                        let op3 = self.o(op).prev;
                        let op4 = self.o(op2).prev;
                        self.om(op).prev = op4;
                        self.om(op4).next = op;
                        self.om(op2).prev = op3;
                        self.om(op3).next = op2;

                        self.rm(outrec).pts = op;
                        let outrec2 = self.create_out_rec();
                        self.rm(outrec2).pts = op2;
                        self.update_out_pt_idxs(outrec2);
                        if self.poly2_contains_poly1(self.r(outrec2).pts, self.r(outrec).pts) {
                            // outrec2 is inside outrec.
                            let hole = !self.r(outrec).is_hole;
                            self.rm(outrec2).is_hole = hole;
                            self.rm(outrec2).first_left = outrec;
                            if self.using_poly_tree {
                                self.fixup_first_lefts2(outrec2, outrec);
                            }
                        } else if self.poly2_contains_poly1(self.r(outrec).pts, self.r(outrec2).pts)
                        {
                            // outrec is inside outrec2.
                            let hole = self.r(outrec).is_hole;
                            self.rm(outrec2).is_hole = hole;
                            self.rm(outrec).is_hole = !hole;
                            let fl = self.r(outrec).first_left;
                            self.rm(outrec2).first_left = fl;
                            self.rm(outrec).first_left = outrec2;
                            if self.using_poly_tree {
                                self.fixup_first_lefts2(outrec, outrec2);
                            }
                        } else {
                            // The two polygons are separate.
                            let (hole, fl) = (self.r(outrec).is_hole, self.r(outrec).first_left);
                            self.rm(outrec2).is_hole = hole;
                            self.rm(outrec2).first_left = fl;
                            if self.using_poly_tree {
                                self.fixup_first_lefts1(outrec, outrec2);
                            }
                        }
                        op2 = op; // Ready for the next iteration.
                    }
                    op2 = self.o(op2).next;
                }
                op = self.o(op).next;
                if op == self.r(outrec).pts {
                    break;
                }
            }
        }
    }
}
