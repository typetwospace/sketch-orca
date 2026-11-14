// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.
//
// Carries OrcaSlicer code: src/libslic3r/Fill/FillBase.cpp lines 322-1242 and 1263-1264 of
// OrcaSlicer v2.4.2, with line_alg::distance_to_squared of src/libslic3r/Line.hpp (lines 41-77) on
// double points and BoundingBoxBase::overlap of src/libslic3r/BoundingBox.hpp (lines 55-58) on
// BoundingBoxf, translated on 2026-11-14 (orca-o3-fill-1). Copyright the OrcaSlicer authors, the
// BambuStudio authors (Bambu Lab), the PrusaSlicer authors (Prusa Research) and the Slic3r
// authors (Alessandro Ranellucci), under the GNU Affero General Public License, version 3
// (AGPL-3.0; LICENSE).

//! The contour walks and clipping of FillBase.cpp that `connect_infill` builds on: the T-joints of
//! infill lines with the boundary ([`ContourIntersectionPoint`]), the distances along a closed
//! contour, the pieces of contour taken onto a line ([`take_cw_full`], [`take_ccw_full`],
//! [`take_cw_limited`], [`take_ccw_limited`], [`take_points`], [`take`] and [`take_limited`]), the
//! ends of a line clipped by length ([`clip_start_segment_and_point`],
//! [`clip_end_segment_and_point`]), the overlap of a line with a thick segment
//! ([`line_rounded_thick_segment_collision`]) and the marking of the boundary that lies under the
//! infill ([`mark_boundary_segments_touching_infill`]).
//!
//! - The C++ points the T-joints at each other (`prev_on_contour`, `next_on_contour`) inside
//!   `BoundaryInfillGraph::map_infill_end_point_to_boundary`, and lists them per contour by
//!   pointer. Here they are indices into that vector (the "arena", `cps`), `None` for the C++'s
//!   null, and each function that follows a link takes the arena. A pointer difference
//!   `&cp - map_infill_end_point_to_boundary.data()` is the index itself. A link followed while
//!   still `None` stops ([`crate::stop::NULL_NEIGHBOUR`]), where the C++ reads through null.
//! - `take` marks the T-joints strictly between its two ends as consumed, but stops one short: its
//!   loop runs while the next T-joint is not the end, so the one just before the end keeps its
//!   state (a quirk kept as it is). `mark_boundary_segments_touching_infill`'s loop goes all the
//!   way to the end.
//! - `line_rounded_thick_segment_collision` writes its interval only when the line meets the
//!   thick segment, as the C++ does; with a segment shorter than `SCALED_EPSILON` it intersects the
//!   line with a circle about the segment's middle whose squared radius is `offset`, not
//!   `offset * offset` (the C++ passes `offset` where the other calls pass `offset2`; kept).
//! - `mark_boundary_segments_touching_infill` builds an `EdgeGrid` over the boundary (its box
//!   inflated by 1.43 times `distance_colliding`, cells of `max(clip_distance,
//!   distance_colliding) + 10 mm`, truncated) and traces each infill segment, clipped by
//!   `clip_distance` at both ends of its line, as two thin lines offset by `distance_colliding`
//!   to either side and lengthened by it at both ends. Each boundary segment in a cell they cross,
//!   on a contour some infill line touches, whose box overlaps the infill segment's box (inflated
//!   by `distance_colliding + SCALED_EPSILON`) and which the infill segment thickened by
//!   `distance_colliding` overlaps, trims the T-joints around the overlap and consumes those
//!   inside it. The T-joints of a contour are found by `lower_bound` over their parameters.
//! - The C++ reserves capacity before it appends contour points (`take`, `take_limited`); a
//!   `Vec` grows as it needs, and no result hangs on capacity.
//!
//! `line_alg::distance_to_squared` on `Linef` ([`distance_to_squared_linef`]) and `overlap` of a
//! `BoundingBoxf` belong in orca-geom, whose `line` and `bounding_box` modules have them for
//! `Line` and `BoundingBox` alone; no piece carries them there, so fill-1 translates them here.
//!
//! Left out: the `#ifndef NDEBUG` helpers (`inside_interval`, `interval_inside_interval`,
//! `cyclic_interval_inside_interval`, `validate_boundary_intersections`), which only asserts call;
//! `export_infill_to_svg` and the other `INFILL_DEBUG_OUTPUT` blocks (debug output); the code
//! inside `#if 0`.
//!
//! None of these is reached but through `connect_infill`, which is fill-2's: the probe's FLCN
//! records (`parity/sop1/fill.md`) prove them with it. This module's unit tests check them by hand
//! until then.

use orca_eigen::Vec2d;
use orca_geom::bounding_box::{BoundingBox, BoundingBoxf};
use orca_geom::geometry::{liang_barsky_line_clipping_interval, ray_circle_intersections_r2_lv2_c};
use orca_geom::point::{Coord, Point, Points, SCALED_EPSILON, lerp_point, perp, scale_};
use orca_geom::polyline::Polyline;
use orca_index::edge_grid::Grid;

use crate::stop::{self, stop};

/// `boundary_idx_unconnected`: the contour index of an infill end that touches no contour.
pub const BOUNDARY_IDX_UNCONNECTED: usize = usize::MAX;

/// `struct ContourIntersectionPoint`: a T-joint of an infill line with a closed contour or one of
/// its holes. Its neighbours along the contour are indices into the arena that holds every
/// T-joint (`map_infill_end_point_to_boundary`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContourIntersectionPoint {
    /// `contour_idx`: the contour the infill line meets.
    pub contour_idx: usize,
    /// `point_idx`: the point of the contour where it meets it.
    pub point_idx: usize,
    /// `param`: the Euclidean parameter of `point_idx` along its contour.
    pub param: f64,
    /// `prev_on_contour`: the previous T-joint on the contour (itself when alone), `None` for the
    /// C++'s null.
    pub prev_on_contour: Option<usize>,
    /// `next_on_contour`: the next T-joint on the contour.
    pub next_on_contour: Option<usize>,
    /// `contour_not_taken_length_prev`: the contour not yet taken going back (clockwise).
    pub contour_not_taken_length_prev: f64,
    /// `contour_not_taken_length_next`: the contour not yet taken going forward
    /// (counter-clockwise).
    pub contour_not_taken_length_next: f64,
    /// `consumed`: the infill line's end here was already connected, or could not be.
    pub consumed: bool,
    /// `prev_trimmed`: the contour going back was trimmed or taken.
    pub prev_trimmed: bool,
    /// `next_trimmed`: the contour going forward was trimmed or taken.
    pub next_trimmed: bool,
}

impl ContourIntersectionPoint {
    /// `ContourIntersectionPoint{ contour_idx, point_idx }`: the other members at their
    /// initialisers, `param` at 0 (aggregate initialisation), no neighbours, the whole contour not
    /// taken (`std::numeric_limits<double>::max()`) and nothing consumed or trimmed.
    #[must_use]
    pub fn new(contour_idx: usize, point_idx: usize) -> Self {
        Self {
            contour_idx,
            point_idx,
            param: 0.,
            prev_on_contour: None,
            next_on_contour: None,
            contour_not_taken_length_prev: f64::MAX,
            contour_not_taken_length_next: f64::MAX,
            consumed: false,
            prev_trimmed: false,
            next_trimmed: false,
        }
    }

    /// `consume_prev()`.
    pub fn consume_prev(&mut self) {
        self.contour_not_taken_length_prev = 0.;
        self.prev_trimmed = true;
        self.consumed = true;
    }

    /// `consume_next()`.
    pub fn consume_next(&mut self) {
        self.contour_not_taken_length_next = 0.;
        self.next_trimmed = true;
        self.consumed = true;
    }

    /// `trim_prev(new_len)`: shortens what is left going back.
    pub fn trim_prev(&mut self, new_len: f64) {
        if new_len < self.contour_not_taken_length_prev {
            self.contour_not_taken_length_prev = new_len;
            self.prev_trimmed = true;
        }
    }

    /// `trim_next(new_len)`: shortens what is left going forward.
    pub fn trim_next(&mut self, new_len: f64) {
        if new_len < self.contour_not_taken_length_next {
            self.contour_not_taken_length_next = new_len;
            self.next_trimmed = true;
        }
    }

    /// `could_take_prev()`: not consumed, and more than `SCALED_EPSILON` of the contour left going
    /// back.
    #[must_use]
    pub fn could_take_prev(&self) -> bool {
        !self.consumed && self.contour_not_taken_length_prev > SCALED_EPSILON
    }

    /// `could_take_next()`: not consumed, and more than `SCALED_EPSILON` of the contour left going
    /// forward.
    #[must_use]
    pub fn could_take_next(&self) -> bool {
        !self.consumed && self.contour_not_taken_length_next > SCALED_EPSILON
    }

    /// `cps[i].could_connect_prev()`: the whole arc back to the previous T-joint could be taken.
    #[must_use]
    pub fn could_connect_prev(cps: &[Self], i: usize) -> bool {
        let cp = &cps[i];
        !cp.consumed
            && cp.prev_on_contour != Some(i)
            && !cps[prev_on_contour(cps, i)].consumed
            && !cp.prev_trimmed
            && !cps[prev_on_contour(cps, i)].next_trimmed
    }

    /// `cps[i].could_connect_next()`: the whole arc on to the next T-joint could be taken.
    #[must_use]
    pub fn could_connect_next(cps: &[Self], i: usize) -> bool {
        let cp = &cps[i];
        !cp.consumed
            && cp.next_on_contour != Some(i)
            && !cps[next_on_contour(cps, i)].consumed
            && !cp.next_trimmed
            && !cps[next_on_contour(cps, i)].prev_trimmed
    }
}

/// `cps[i].prev_on_contour`, followed.
///
/// # Panics
/// With [`crate::stop::NULL_NEIGHBOUR`] when it was never set.
#[must_use]
pub fn prev_on_contour(cps: &[ContourIntersectionPoint], i: usize) -> usize {
    match cps[i].prev_on_contour {
        Some(p) => p,
        None => stop(
            "ContourIntersectionPoint::prev_on_contour",
            stop::NULL_NEIGHBOUR,
        ),
    }
}

/// `cps[i].next_on_contour`, followed.
///
/// # Panics
/// With [`crate::stop::NULL_NEIGHBOUR`] when it was never set.
#[must_use]
pub fn next_on_contour(cps: &[ContourIntersectionPoint], i: usize) -> usize {
    match cps[i].next_on_contour {
        Some(n) => n,
        None => stop(
            "ContourIntersectionPoint::next_on_contour",
            stop::NULL_NEIGHBOUR,
        ),
    }
}

/// `closed_contour_distance_ccw(param1, param2, contour_length)`: from `param1` to `param2` going
/// counter-clockwise.
#[must_use]
pub fn closed_contour_distance_ccw(param1: f64, param2: f64, contour_length: f64) -> f64 {
    let mut d = param2 - param1;
    if d < 0. {
        d += contour_length;
    }
    d
}

/// `closed_contour_distance_cw(param1, param2, contour_length)`: from `param1` to `param2` going
/// clockwise.
#[must_use]
pub fn closed_contour_distance_cw(param1: f64, param2: f64, contour_length: f64) -> f64 {
    closed_contour_distance_ccw(param2, param1, contour_length)
}

/// `path_length_along_contour_ccw(cp1, cp2, contour_length)`: along the contour from `cp1` to
/// `cp2` going counter-clockwise.
#[must_use]
pub fn path_length_along_contour_ccw(
    cp1: &ContourIntersectionPoint,
    cp2: &ContourIntersectionPoint,
    contour_length: f64,
) -> f64 {
    closed_contour_distance_ccw(cp1.param, cp2.param, contour_length)
}

/// `path_lengths_along_contour(cp1, cp2, contour_length)`: along the contour from `cp1` to `cp2`
/// going counter-clockwise, then going clockwise.
#[must_use]
pub fn path_lengths_along_contour(
    cp1: &ContourIntersectionPoint,
    cp2: &ContourIntersectionPoint,
    contour_length: f64,
) -> (f64, f64) {
    // Zero'th param is the length of the contour.
    let mut param_lo = cp1.param;
    let mut param_hi = cp2.param;
    let mut reversed = false;
    if param_lo > param_hi {
        core::mem::swap(&mut param_lo, &mut param_hi);
        reversed = true;
    }
    let mut out = (param_hi - param_lo, param_lo + contour_length - param_hi);
    if reversed {
        core::mem::swap(&mut out.0, &mut out.1);
    }
    out
}

/// `take_cw_full(pl, contour, idx_start, idx_end)`: the contour's points from before `idx_start`
/// back to `idx_end`, `idx_end` included, appended to `pl`.
pub fn take_cw_full(pl: &mut Polyline, contour: &[Point], idx_start: usize, idx_end: usize) {
    let mut i = if idx_start == 0 {
        contour.len() - 1
    } else {
        idx_start - 1
    };
    while i != idx_end {
        pl.points.push(contour[i]);
        if i == 0 {
            i = contour.len();
        }
        i -= 1;
    }
    pl.points.push(contour[i]);
}

/// `take_cw_limited(pl, contour, params, idx_start, idx_end, length_to_take)`: as
/// [`take_cw_full`], but no more than `length_to_take` along the contour, the last point
/// interpolated (`lerp`, truncated). Returns the length taken.
pub fn take_cw_limited(
    pl: &mut Polyline,
    contour: &[Point],
    params: &[f64],
    idx_start: usize,
    idx_end: usize,
    length_to_take: f64,
) -> f64 {
    // Length of the contour.
    let length = params[params.len() - 1];
    // Parameter (length from contour.front()) for the first point.
    let p0 = params[idx_start];
    // Current (2nd) point of the contour.
    let mut i = if idx_start == 0 {
        contour.len() - 1
    } else {
        idx_start - 1
    };
    // Previous point of the contour.
    let mut iprev = idx_start;
    // Length of the contour curve taken for iprev.
    let mut lprev = 0.;

    loop {
        let l = closed_contour_distance_cw(p0, params[i], length);
        if l >= length_to_take {
            // Trim the last segment.
            let t = (length_to_take - lprev) / (l - lprev);
            pl.points.push(lerp_point(contour[iprev], contour[i], t));
            return length_to_take;
        }
        // Continue with the other segments.
        pl.points.push(contour[i]);
        if i == idx_end {
            return l;
        }
        iprev = i;
        lprev = l;
        if i == 0 {
            i = contour.len();
        }
        i -= 1;
    }
}

/// `take_ccw_full(pl, contour, idx_start, idx_end)`: the contour's points from after `idx_start`
/// on to `idx_end`, `idx_end` included, appended to `pl`.
pub fn take_ccw_full(pl: &mut Polyline, contour: &[Point], idx_start: usize, idx_end: usize) {
    let mut i = idx_start + 1;
    if i == contour.len() {
        i = 0;
    }
    while i != idx_end {
        pl.points.push(contour[i]);
        i += 1;
        if i == contour.len() {
            i = 0;
        }
    }
    pl.points.push(contour[i]);
}

/// `take_ccw_limited(pl, contour, params, idx_start, idx_end, length_to_take)`: as
/// [`take_ccw_full`], but no more than `length_to_take` along the contour, the last point
/// interpolated (`lerp`, truncated). Returns the length taken.
pub fn take_ccw_limited(
    pl: &mut Polyline,
    contour: &[Point],
    params: &[f64],
    idx_start: usize,
    idx_end: usize,
    length_to_take: f64,
) -> f64 {
    // Length of the contour.
    let length = params[params.len() - 1];
    // Parameter (length from contour.front()) for the first point.
    let p0 = params[idx_start];
    // Current (2nd) point of the contour.
    let mut i = idx_start + 1;
    if i == contour.len() {
        i = 0;
    }
    // Previous point of the contour.
    let mut iprev = idx_start;
    // Length of the contour curve taken at iprev.
    let mut lprev = 0.;
    loop {
        let l = closed_contour_distance_ccw(p0, params[i], length);
        if l >= length_to_take {
            // Trim the last segment.
            let t = (length_to_take - lprev) / (l - lprev);
            pl.points.push(lerp_point(contour[iprev], contour[i], t));
            return length_to_take;
        }
        // Continue with the other segments.
        pl.points.push(contour[i]);
        if i == idx_end {
            return l;
        }
        iprev = i;
        lprev = l;
        i += 1;
        if i == contour.len() {
            i = 0;
        }
    }
}

/// `take(pl1, pl2, contour, idx_start, idx_end, clockwise)`, the form by point indices: the end
/// of `pl1` joined to the start of `pl2` along the contour, clockwise or counter-clockwise from
/// `idx_start` to `idx_end`, then `pl2`'s points after its first.
pub fn take_points(
    pl1: &mut Polyline,
    pl2: &Polyline,
    contour: &[Point],
    idx_start: usize,
    idx_end: usize,
    clockwise: bool,
) {
    if clockwise {
        take_cw_full(pl1, contour, idx_start, idx_end);
    } else {
        take_ccw_full(pl1, contour, idx_start, idx_end);
    }

    pl1.points.extend_from_slice(&pl2.points[1..]);
}

/// `take(pl1, pl2, contour, cp_start, cp_end, clockwise)`, the form by T-joints: [`take_points`]
/// between their points, then the T-joints between them (taken counter-clockwise) consumed, but
/// for the one just before the end (see the module's notes), and `cp_start`'s arc forward and
/// `cp_end`'s arc back consumed.
pub fn take(
    pl1: &mut Polyline,
    pl2: &Polyline,
    contour: &[Point],
    cps: &mut [ContourIntersectionPoint],
    mut cp_start: usize,
    mut cp_end: usize,
    clockwise: bool,
) {
    take_points(
        pl1,
        pl2,
        contour,
        cps[cp_start].point_idx,
        cps[cp_end].point_idx,
        clockwise,
    );

    // Mark the contour segments in between cp_start and cp_end as consumed.
    if clockwise {
        core::mem::swap(&mut cp_start, &mut cp_end);
    }
    if next_on_contour(cps, cp_start) != cp_end {
        let mut cp = next_on_contour(cps, cp_start);
        while next_on_contour(cps, cp) != cp_end {
            cps[cp].consume_prev();
            cps[cp].consume_next();
            cp = next_on_contour(cps, cp);
        }
    }
    cps[cp_start].consume_next();
    cps[cp_end].consume_prev();
}

/// `take_limited(pl1, contour, params, cp_start, cp_end, clockwise, take_max_length,
/// line_half_width)`: a piece of the contour, at most `take_max_length` long, taken from
/// `cp_start` toward `cp_end` (clockwise or not) onto whichever end of `pl1` lies at `cp_start`,
/// keeping `line_half_width` away from the T-joints it passes and no longer than what is left of
/// each arc, and the arcs it passes consumed or trimmed. Nothing when `cp_start` cannot take
/// that way.
///
/// # Panics
/// With [`crate::stop::EMPTY_POLYLINE`] when `pl1` has no points.
#[allow(clippy::too_many_arguments)] // The C++'s eight arguments and the arena of T-joints.
pub fn take_limited(
    pl1: &mut Polyline,
    contour: &[Point],
    params: &[f64],
    cps: &mut [ContourIntersectionPoint],
    cp_start: usize,
    cp_end: usize,
    clockwise: bool,
    take_max_length: f64,
    line_half_width: f64,
) {
    if !(if clockwise {
        cps[cp_start].could_take_prev()
    } else {
        cps[cp_start].could_take_next()
    }) {
        return;
    }

    let Some(&front) = pl1.points.first() else {
        stop("take_limited", stop::EMPTY_POLYLINE)
    };
    let add_at_start = front == contour[cps[cp_start].point_idx];
    let mut pl_tmp = Points::new();
    if add_at_start {
        pl_tmp = core::mem::take(&mut pl1.points);
    }

    let length = params[params.len() - 1];
    let mut length_to_go = take_max_length;
    cps[cp_start].consumed = true;
    if cp_start == cp_end {
        length_to_go = orca_stl::max(0., orca_stl::min(length_to_go, length - line_half_width));
        length_to_go = orca_stl::min(
            length_to_go,
            if clockwise {
                cps[cp_start].contour_not_taken_length_prev
            } else {
                cps[cp_start].contour_not_taken_length_next
            },
        );
        cps[cp_start].consume_prev();
        cps[cp_start].consume_next();
        if length_to_go > SCALED_EPSILON {
            let idx = cps[cp_start].point_idx;
            if clockwise {
                take_cw_limited(pl1, contour, params, idx, idx, length_to_go);
            } else {
                take_ccw_limited(pl1, contour, params, idx, idx, length_to_go);
            }
        }
    } else if clockwise {
        // Going clockwise from cp_start to cp_end.
        let mut cp = cp_start;
        while cp != cp_end {
            let prev = prev_on_contour(cps, cp);
            // Length of the segment from cp to cp->prev_on_contour.
            let l = closed_contour_distance_cw(cps[cp].param, cps[prev].param, length);
            length_to_go = orca_stl::min(length_to_go, cps[cp].contour_not_taken_length_prev);
            // Don't overlap with an already extruded infill line.
            length_to_go = orca_stl::max(0., orca_stl::min(length_to_go, l - line_half_width));
            cps[cp].consume_prev();
            if l >= length_to_go {
                if length_to_go > SCALED_EPSILON {
                    cps[prev].trim_next(l - length_to_go);
                    let (from, to) = (cps[cp].point_idx, cps[prev].point_idx);
                    take_cw_limited(pl1, contour, params, from, to, length_to_go);
                }
                break;
            }
            cps[prev].trim_next(0.);
            take_cw_full(pl1, contour, cps[cp].point_idx, cps[prev].point_idx);
            length_to_go -= l;
            cp = prev_on_contour(cps, cp);
        }
    } else {
        let mut cp = cp_start;
        while cp != cp_end {
            let next = next_on_contour(cps, cp);
            let l = closed_contour_distance_ccw(cps[cp].param, cps[next].param, length);
            length_to_go = orca_stl::min(length_to_go, cps[cp].contour_not_taken_length_next);
            // Don't overlap with an already extruded infill line.
            length_to_go = orca_stl::max(0., orca_stl::min(length_to_go, l - line_half_width));
            cps[cp].consume_next();
            if l >= length_to_go {
                if length_to_go > SCALED_EPSILON {
                    cps[next].trim_prev(l - length_to_go);
                    let (from, to) = (cps[cp].point_idx, cps[next].point_idx);
                    take_ccw_limited(pl1, contour, params, from, to, length_to_go);
                }
                break;
            }
            cps[next].trim_prev(0.);
            take_ccw_full(pl1, contour, cps[cp].point_idx, cps[next].point_idx);
            length_to_go -= l;
            cp = next_on_contour(cps, cp);
        }
    }

    if add_at_start {
        pl1.reverse();
        pl1.points.extend_from_slice(&pl_tmp);
    }
}

/// `struct SegmentPoint`: where a line is clipped, the start of its segment, the parameter in
/// [0, 1) along the segment and the point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SegmentPoint {
    /// `idx_segment`: the segment from this point to the next; `usize::MAX` when not found.
    pub idx_segment: usize,
    /// `t`: the parameter along the segment (left unset in the C++ until found; 0 here).
    pub t: f64,
    /// `point`: the clipping point (likewise).
    pub point: Vec2d,
}

impl Default for SegmentPoint {
    /// Not found.
    fn default() -> Self {
        Self {
            idx_segment: usize::MAX,
            t: 0.,
            point: Vec2d::zero(),
        }
    }
}

impl SegmentPoint {
    /// `valid()`: found.
    #[must_use]
    pub fn valid(&self) -> bool {
        self.idx_segment != usize::MAX
    }
}

/// `clip_start_segment_and_point(polyline, distance)`: the point `distance` along the line from
/// its start, on the first segment longer than what is left of `distance`; not found when the
/// line is no longer than that.
#[must_use]
pub fn clip_start_segment_and_point(polyline: &[Point], mut distance: f64) -> SegmentPoint {
    // Initialized to "invalid".
    let mut out = SegmentPoint::default();
    if polyline.len() >= 2 {
        let mut pt_prev = polyline[0].cast_f64();
        for (i, p) in polyline.iter().enumerate().skip(1) {
            let pt = p.cast_f64();
            let v = pt - pt_prev;
            let l = v.norm();
            if l > distance {
                out.idx_segment = i - 1;
                out.t = distance / l;
                out.point = pt_prev + out.t * v;
                break;
            }
            distance -= l;
            pt_prev = pt;
        }
    }
    out
}

/// `clip_end_segment_and_point(polyline, distance)`: the point `distance` along the line from its
/// end, its parameter taken from the segment's start.
#[must_use]
pub fn clip_end_segment_and_point(polyline: &[Point], mut distance: f64) -> SegmentPoint {
    // Initialized to "invalid".
    let mut out = SegmentPoint::default();
    if polyline.len() >= 2 {
        let mut pt_next = polyline[polyline.len() - 1].cast_f64();
        // `int(polyline.size()) - 2` down to 0.
        let mut i = polyline.len() as i64 - 2;
        while i >= 0 {
            let pt = polyline[i as usize].cast_f64();
            let v = pt - pt_next;
            let l = v.norm();
            if l > distance {
                out.idx_segment = i as usize;
                out.t = distance / l;
                out.point = pt_next + out.t * v;
                // Store the parameter referenced to the starting point of a segment.
                out.t = 1. - out.t;
                break;
            }
            distance -= l;
            pt_next = pt;
            i -= 1;
        }
    }
    out
}

/// `line_alg::distance_to_squared(Linef{a, b}, point)` (Line.hpp 41-77) on `double` points: the
/// squared distance to the nearest point of the segment. Belongs in orca-geom.
#[must_use]
pub fn distance_to_squared_linef(a: Vec2d, b: Vec2d, point: Vec2d) -> f64 {
    let v = b - a;
    let va = point - a;
    // avoid a sqrt
    let l2 = v.squared_norm();
    if l2 == 0. {
        // a == b case
        return va.squared_norm();
    }
    // The projection of the point onto the line a + t (b - a).
    let t = va.dot(&v) / l2;
    if t <= 0. {
        // beyond the 'a' end of the segment
        return va.squared_norm();
    } else if t >= 1. {
        // beyond the 'b' end of the segment
        return (point - b).squared_norm();
    }
    (t * v - va).squared_norm()
}

/// `BoundingBoxf::overlap(other)` (BoundingBox.hpp 55-58), the boundary included. Belongs in
/// orca-geom.
fn overlap_f(a: &BoundingBoxf, b: &BoundingBoxf) -> bool {
    !(a.max.x() < b.min.x()
        || a.min.x() > b.max.x()
        || a.max.y() < b.min.y()
        || a.min.y() > b.max.y())
}

/// The lambda `extend_interval` of `line_rounded_thick_segment_collision`: the interval widened
/// to take `[atmin, atmax]` in (`std::min`, `std::max`).
fn extend_interval(interval: &mut (f64, f64), atmin: f64, atmax: f64) {
    interval.0 = orca_stl::min(interval.0, atmin);
    interval.1 = orca_stl::max(interval.1, atmax);
}

/// The lambda `ray_circle_intersection_interval_extend` of `line_rounded_thick_segment_collision`:
/// where the line `line_pt + t line_vec` crosses the circle of squared radius `offset2` about
/// `segment_pt`, the parameters clamped to [0, 1], widen the interval.
fn ray_circle_intersection_interval_extend(
    interval: &mut (f64, f64),
    segment_pt: Vec2d,
    offset2: f64,
    line_pt: Vec2d,
    line_vec: Vec2d,
) {
    let mut pts = (Vec2d::zero(), Vec2d::zero());
    let p0 = line_pt - segment_pt;
    let lv2 = line_vec.squared_norm();
    if ray_circle_intersections_r2_lv2_c(
        offset2,
        line_vec.y(),
        -line_vec.x(),
        lv2,
        -line_vec.y() * p0.x() + line_vec.x() * p0.y(),
        &mut pts,
    ) != 0
    {
        let mut tmin = (pts.0 - p0).dot(&line_vec) / lv2;
        let mut tmax = (pts.1 - p0).dot(&line_vec) / lv2;
        if tmin > tmax {
            core::mem::swap(&mut tmin, &mut tmax);
        }
        tmin = orca_stl::max(tmin, 0.);
        tmax = orca_stl::min(tmax, 1.);
        if tmin <= tmax {
            extend_interval(interval, tmin, tmax);
        }
    }
}

/// `line_rounded_thick_segment_collision(line_a, line_b, segment_a, segment_b, offset,
/// out_interval)`: whether the segment `line_a`-`line_b` meets the segment `segment_a`-`segment_b`
/// thickened by `offset` with round ends; when it does, `out_interval` gets the Euclidean
/// parameters along the line of where they overlap, and is left as it was otherwise.
///
/// A line shorter than `SCALED_EPSILON` is tested by its middle point (the interval is then 0 to
/// its length). Else the interval gathers the line's crossings with the circles at the segment's
/// ends and its clip by the rectangle along the segment (Liang-Barsky in the segment's frame), or,
/// for a segment shorter than `SCALED_EPSILON`, with one circle about its middle (of squared
/// radius `offset`; see the module's notes).
pub fn line_rounded_thick_segment_collision(
    line_a: Vec2d,
    line_b: Vec2d,
    segment_a: Vec2d,
    segment_b: Vec2d,
    offset: f64,
    out_interval: &mut (f64, f64),
) -> bool {
    let line_v0 = line_b - line_a;
    let mut lv = line_v0.squared_norm();

    let segment_v = segment_b - segment_a;
    let segment_l = segment_v.norm();
    let offset2 = offset * offset;

    let intersects;
    if lv < SCALED_EPSILON * SCALED_EPSILON {
        // Very short line vector. Just test whether the center point is inside the offset line.
        let lpt = 0.5 * (line_a + line_b);
        if segment_l > SCALED_EPSILON {
            intersects = distance_to_squared_linef(segment_a, segment_b, lpt) < offset2;
        } else {
            intersects = (0.5 * (segment_a + segment_b) - lpt).squared_norm() < offset2;
        }
        if intersects {
            out_interval.0 = 0.;
            out_interval.1 = orca_crt::sqrt(lv);
        }
    } else {
        // Output interval: (tmin, tmax).
        let tmin = f64::MAX;
        let mut interval = (tmin, -tmin);

        // Intersections with the inflated segment.
        if segment_l > SCALED_EPSILON {
            ray_circle_intersection_interval_extend(
                &mut interval,
                segment_a,
                offset2,
                line_a,
                line_v0,
            );
            ray_circle_intersection_interval_extend(
                &mut interval,
                segment_b,
                offset2,
                line_a,
                line_v0,
            );
            // Clip the line segment transformed into a coordinate space of the segment, where the
            // segment spans (0, 0) to (segment_l, 0).
            let dir_x = segment_v / segment_l;
            let dir_y = Vec2d::new(-dir_x.y(), dir_x.x());
            let line_p0 = line_a - segment_a;
            if let Some(clipped) = liang_barsky_line_clipping_interval(
                Vec2d::new(line_p0.dot(&dir_x), line_p0.dot(&dir_y)),
                Vec2d::new(line_v0.dot(&dir_x), line_v0.dot(&dir_y)),
                &BoundingBoxf::from_corners(Vec2d::new(0., -offset), Vec2d::new(segment_l, offset)),
            ) {
                extend_interval(&mut interval, clipped.0, clipped.1);
            }
        } else {
            ray_circle_intersection_interval_extend(
                &mut interval,
                0.5 * (segment_a + segment_b),
                offset,
                line_a,
                line_v0,
            );
        }

        intersects = interval.0 <= interval.1;
        if intersects {
            lv = orca_crt::sqrt(lv);
            out_interval.0 = interval.0 * lv;
            out_interval.1 = interval.1 * lv;
        }
    }

    intersects
}

/// The visitor of `mark_boundary_segments_touching_infill`, which trims the T-joints with the
/// infill segment it was set to.
struct Visitor<'v> {
    /// `boundary_parameters`.
    boundary_parameters: &'v [Vec<f64>],
    /// `boundary_intersections`: the T-joints of each contour, as indices into the arena.
    boundary_intersections: &'v [Vec<usize>],
    /// `radius`: the largest distance between the boundary and an infill line at which they do
    /// not touch.
    radius: f64,
    /// `infill_pt1`.
    infill_pt1: Vec2d,
    /// `infill_pt2`.
    infill_pt2: Vec2d,
    /// `infill_bbox`.
    infill_bbox: BoundingBoxf,
}

impl Visitor<'_> {
    /// `init(infill_pt1, infill_pt2)`: a segment of an infill line, and its box inflated by the
    /// radius and `SCALED_EPSILON`.
    fn init(&mut self, infill_pt1: Vec2d, infill_pt2: Vec2d) {
        self.infill_pt1 = infill_pt1;
        self.infill_pt2 = infill_pt2;
        self.infill_bbox = BoundingBoxf::default();
        self.infill_bbox.merge(infill_pt1);
        self.infill_bbox.merge(infill_pt2);
        self.infill_bbox.offset(self.radius + SCALED_EPSILON);
    }

    /// `operator()(iy, ix)`: the boundary segments of the cell at row `iy` and column `ix` that the
    /// thick infill segment overlaps trim the T-joints around the overlap. Always goes on.
    fn visit(
        &mut self,
        grid: &Grid<'_>,
        cps: &mut [ContourIntersectionPoint],
        iy: Coord,
        ix: Coord,
    ) -> bool {
        // Called with a row and column of the grid cell, which is intersected by a line.
        for contour_and_segment in grid.cell_data_range(iy, ix) {
            // End points of the line segment and their vector.
            let segment = grid.segment(contour_and_segment);
            let intersections = &self.boundary_intersections[contour_and_segment.0];
            if intersections.is_empty() {
                // There is no infill line touching this contour, thus effort will be saved to
                // calculate overlap with other infill lines.
                continue;
            }
            let seg_pt1 = segment.0.cast_f64();
            let seg_pt2 = segment.1.cast_f64();
            let mut interval = (0., 0.);
            let mut bbox_seg = BoundingBoxf::default();
            bbox_seg.merge(seg_pt1);
            bbox_seg.merge(seg_pt2);
            if overlap_f(&self.infill_bbox, &bbox_seg)
                && line_rounded_thick_segment_collision(
                    seg_pt1,
                    seg_pt2,
                    self.infill_pt1,
                    self.infill_pt2,
                    self.radius,
                    &mut interval,
                )
            {
                // The boundary segment intersects with the infill segment thickened by radius.
                // Interval is specified in Euclidian length from seg_pt1 to seg_pt2.
                // 1) Find the Euclidian parameters of seg_pt1 and seg_pt2 on its boundary contour.
                let contour_parameters = &self.boundary_parameters[contour_and_segment.0];
                let contour_length = contour_parameters[contour_parameters.len() - 1];
                let param_seg_pt1 = contour_parameters[contour_and_segment.1];
                let param_seg_pt2 = contour_parameters[contour_and_segment.1 + 1];
                let param_overlap1 = orca_stl::min(param_seg_pt2, param_seg_pt1 + interval.0);
                let param_overlap2 = orca_stl::min(param_seg_pt2, param_seg_pt1 + interval.1);
                // 2) Find the span of ContourIntersectionPoints, that is trimmed by the interval
                // (param_overlap1, param_overlap2).
                let mut ip_low;
                let ip_high;
                if intersections.len() == 1 {
                    // Only a single infill line touches this contour.
                    ip_low = intersections[0];
                    ip_high = ip_low;
                } else {
                    let it_low =
                        orca_stl::lower_bound_by(intersections, &param_overlap1, |&l, p| {
                            cps[l].param < *p
                        });
                    let it_high =
                        orca_stl::lower_bound_by(intersections, &param_overlap2, |&l, p| {
                            cps[l].param < *p
                        });
                    ip_low = if it_low == intersections.len() {
                        intersections[0]
                    } else {
                        intersections[it_low]
                    };
                    ip_high = if it_high == intersections.len() {
                        intersections[0]
                    } else {
                        intersections[it_high]
                    };
                    if cps[ip_low].param != param_overlap1 {
                        ip_low = prev_on_contour(cps, ip_low);
                    }
                }
                // Mark all ContourIntersectionPoints between ip_low and ip_high as consumed.
                if next_on_contour(cps, ip_low) != ip_high {
                    let mut ip = next_on_contour(cps, ip_low);
                    while ip != ip_high {
                        cps[ip].consume_prev();
                        cps[ip].consume_next();
                        ip = next_on_contour(cps, ip);
                    }
                }
                // Subtract the interval from the first and last segments.
                let mut trim_l =
                    closed_contour_distance_ccw(cps[ip_low].param, param_overlap1, contour_length);
                cps[ip_low].trim_next(trim_l);
                trim_l =
                    closed_contour_distance_ccw(param_overlap2, cps[ip_high].param, contour_length);
                cps[ip_high].trim_prev(trim_l);
            }
        }
        // Continue traversing the grid along the edge.
        true
    }
}

/// `mark_boundary_segments_touching_infill(boundary, boundary_parameters,
/// boundary_intersections, boundary_bbox, infill, clip_distance, distance_colliding)`: the arcs of
/// the boundary that lie within `distance_colliding` of an infill line, away from its ends by
/// `clip_distance`, trimmed off the T-joints around them, and the T-joints inside them consumed
/// (see the module's notes). `boundary_parameters[i]` holds the parameter of each point of
/// `boundary[i]` and then the contour's length; `boundary_intersections[i]` the T-joints of
/// contour `i`, indices into `cps`, by their parameter.
#[allow(clippy::too_many_arguments)] // The C++'s seven arguments and the arena of T-joints.
pub fn mark_boundary_segments_touching_infill(
    // Boundary contour, along which the perimeter extrusions will be drawn.
    boundary: &[Points],
    // Parametrization of boundary with Euclidian length.
    boundary_parameters: &[Vec<f64>],
    // The T-joints themselves.
    cps: &mut [ContourIntersectionPoint],
    // Intersections (T-joints) of the infill lines with the boundary.
    boundary_intersections: &[Vec<usize>],
    // Bounding box around the boundary.
    boundary_bbox: &BoundingBox,
    // Infill lines, either completely inside the boundary, or touching the boundary.
    infill: &[Polyline],
    // How much of the infill ends should be ignored when marking the boundary segments?
    clip_distance: f64,
    // Roughly width of the infill line.
    distance_colliding: f64,
) {
    let mut grid = Grid::new();
    // Make sure that the the grid is big enough for queries against the thick segment.
    grid.set_bbox(boundary_bbox.inflated(distance_colliding * 1.43));
    // Inflate the bounding box by a thick line width.
    grid.create_closed_points(
        boundary,
        orca_crt::f64_to_i64(orca_stl::max(clip_distance, distance_colliding) + scale_(10.)),
    );

    // Visitor for the EdgeGrid to trim boundary_intersections with existing infill lines.
    let mut visitor = Visitor {
        boundary_parameters,
        boundary_intersections,
        radius: distance_colliding,
        infill_pt1: Vec2d::zero(),
        infill_pt2: Vec2d::zero(),
        infill_bbox: BoundingBoxf::default(),
    };

    for polyline in infill {
        // Clip the infill polyline by the Eucledian distance along the polyline.
        let start_point = clip_start_segment_and_point(&polyline.points, clip_distance);
        let end_point = clip_end_segment_and_point(&polyline.points, clip_distance);
        if start_point.valid()
            && end_point.valid()
            && (start_point.idx_segment < end_point.idx_segment
                || (start_point.idx_segment == end_point.idx_segment
                    && start_point.t < end_point.t))
        {
            // The clipped polyline is non-empty.
            for point_idx in start_point.idx_segment..=end_point.idx_segment {
                let pt1 = if point_idx == start_point.idx_segment {
                    start_point.point
                } else {
                    polyline.points[point_idx].cast_f64()
                };
                let pt2 = if point_idx == end_point.idx_segment {
                    end_point.point
                } else {
                    polyline.points[point_idx + 1].cast_f64()
                };
                visitor.init(pt1, pt2);
                // Simulate tracing of a thick line. This only works reliably if
                // distance_colliding <= grid cell size.
                let v = (pt2 - pt1).normalized() * distance_colliding;
                let vperp = perp(v);
                let mut a = pt1 - v - vperp;
                let mut b = pt2 + v - vperp;
                grid.visit_cells_intersecting_line(
                    Point::cast_from_vec2d(a),
                    Point::cast_from_vec2d(b),
                    &mut |iy, ix| visitor.visit(&grid, cps, iy, ix),
                );
                a = pt1 - v + vperp;
                b = pt2 + v + vperp;
                grid.visit_cells_intersecting_line(
                    Point::cast_from_vec2d(a),
                    Point::cast_from_vec2d(b),
                    &mut |iy, ix| visitor.visit(&grid, cps, iy, ix),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A square of side `s` from the origin, counter-clockwise, and its parameters (the length to
    /// each point, then the contour's length).
    fn square(s: Coord) -> (Points, Vec<f64>) {
        let pts = vec![
            Point::new(0, 0),
            Point::new(s, 0),
            Point::new(s, s),
            Point::new(0, s),
        ];
        let l = s as f64;
        (pts, vec![0., l, 2. * l, 3. * l, 4. * l])
    }

    /// T-joints at the given points of contour 0 with their parameters, linked into a ring.
    fn ring(at: &[(usize, f64)]) -> Vec<ContourIntersectionPoint> {
        let n = at.len();
        at.iter()
            .enumerate()
            .map(|(i, &(point_idx, param))| {
                let mut cp = ContourIntersectionPoint::new(0, point_idx);
                cp.param = param;
                cp.prev_on_contour = Some((i + n - 1) % n);
                cp.next_on_contour = Some((i + 1) % n);
                cp
            })
            .collect()
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn distances_along_a_closed_contour() {
        assert_eq!(closed_contour_distance_ccw(1., 3., 10.), 2.);
        assert_eq!(closed_contour_distance_ccw(3., 1., 10.), 8.);
        assert_eq!(closed_contour_distance_cw(1., 3., 10.), 8.);
        let cps = ring(&[(0, 1.), (2, 3.)]);
        assert_eq!(path_length_along_contour_ccw(&cps[0], &cps[1], 10.), 2.);
        assert_eq!(path_lengths_along_contour(&cps[0], &cps[1], 10.), (2., 8.));
        assert_eq!(path_lengths_along_contour(&cps[1], &cps[0], 10.), (8., 2.));
    }

    #[test]
    fn the_full_walks_take_every_point_to_the_end() {
        let (c, _) = square(1000);
        let mut pl = Polyline::new(vec![c[0]]);
        take_ccw_full(&mut pl, &c, 0, 2);
        assert_eq!(pl.points, vec![c[0], c[1], c[2]]);
        let mut pl = Polyline::new(vec![c[0]]);
        take_cw_full(&mut pl, &c, 0, 2);
        assert_eq!(pl.points, vec![c[0], c[3], c[2]]);
    }

    #[test]
    fn the_limited_walks_stop_at_the_length() {
        let (c, params) = square(1000);
        let mut pl = Polyline::new(vec![c[0]]);
        assert_eq!(take_ccw_limited(&mut pl, &c, &params, 0, 0, 1500.), 1500.);
        assert_eq!(pl.points, vec![c[0], c[1], Point::new(1000, 500)]);
        let mut pl = Polyline::new(vec![c[0]]);
        assert_eq!(take_cw_limited(&mut pl, &c, &params, 0, 0, 1500.), 1500.);
        assert_eq!(pl.points, vec![c[0], c[3], Point::new(500, 1000)]);
        // The end reached first: its length along the contour.
        let mut pl = Polyline::new(vec![c[0]]);
        assert_eq!(take_ccw_limited(&mut pl, &c, &params, 0, 1, 1500.), 1000.);
        assert_eq!(pl.points, vec![c[0], c[1]]);
    }

    #[test]
    fn take_consumes_all_but_the_t_joint_before_the_end() {
        let (c, _) = square(1000);
        let mid = Point::new(500, 500);
        let pl2 = Polyline::new(vec![c[2], mid]);
        let mut cps = ring(&[(0, 0.), (1, 1000.), (2, 2000.), (3, 3000.)]);
        let mut pl1 = Polyline::new(vec![mid, c[0]]);
        take(&mut pl1, &pl2, &c, &mut cps, 0, 2, false);
        assert_eq!(pl1.points, vec![mid, c[0], c[1], c[2], mid]);
        assert!(cps[0].consumed && cps[0].next_trimmed && !cps[0].prev_trimmed);
        assert!(cps[2].consumed && cps[2].prev_trimmed && !cps[2].next_trimmed);
        // The only T-joint between them is the one before the end: left as it was.
        assert!(!cps[1].consumed);
        let mut cps = ring(&[(0, 0.), (1, 1000.), (2, 2000.), (3, 3000.)]);
        let mut pl1 = Polyline::new(vec![mid, c[0]]);
        let pl2 = Polyline::new(vec![c[3], mid]);
        take(&mut pl1, &pl2, &c, &mut cps, 0, 3, false);
        assert!(cps[1].consumed && cps[1].contour_not_taken_length_prev == 0.);
        assert!(!cps[2].consumed);
    }

    #[test]
    fn take_limited_on_a_lone_t_joint_appends_or_prepends() {
        let (c, params) = square(1000);
        let out = Point::new(-500, 500);
        let mut cps = ring(&[(0, 0.)]);
        let mut pl = Polyline::new(vec![out, c[0]]);
        take_limited(&mut pl, &c, &params, &mut cps, 0, 0, false, 1500., 100.);
        assert_eq!(pl.points, vec![out, c[0], c[1], Point::new(1000, 500)]);
        assert!(cps[0].consumed);
        assert_eq!(cps[0].contour_not_taken_length_prev, 0.);
        assert_eq!(cps[0].contour_not_taken_length_next, 0.);
        // At the line's start: taken, reversed and put before it.
        let mut cps = ring(&[(0, 0.)]);
        let mut pl = Polyline::new(vec![c[0], out]);
        take_limited(&mut pl, &c, &params, &mut cps, 0, 0, false, 1500., 100.);
        assert_eq!(pl.points, vec![Point::new(1000, 500), c[1], c[0], out]);
        // Consumed already: nothing.
        let mut pl = Polyline::new(vec![c[0], out]);
        take_limited(&mut pl, &c, &params, &mut cps, 0, 0, false, 1500., 100.);
        assert_eq!(pl.points.len(), 2);
    }

    #[test]
    fn take_limited_trims_the_next_t_joint() {
        let (c, params) = square(1000);
        let out = Point::new(-500, 500);
        let mut cps = ring(&[(0, 0.), (2, 2000.)]);
        let mut pl = Polyline::new(vec![out, c[0]]);
        take_limited(&mut pl, &c, &params, &mut cps, 0, 1, false, 1500., 100.);
        assert_eq!(pl.points[2..], [c[1], Point::new(1000, 500)]);
        assert!(cps[0].consumed && cps[0].contour_not_taken_length_next == 0.);
        assert!(cps[1].prev_trimmed && cps[1].contour_not_taken_length_prev == 500.);
        assert!(!cps[1].consumed);
        // Longer than the arc less the half width: stops that short of the next T-joint.
        let mut cps = ring(&[(0, 0.), (2, 2000.)]);
        let mut pl = Polyline::new(vec![out, c[0]]);
        take_limited(&mut pl, &c, &params, &mut cps, 0, 1, true, 2500., 100.);
        assert_eq!(pl.points[2..], [c[3], Point::new(900, 1000)]);
        assert_eq!(cps[1].contour_not_taken_length_next, 100.);
    }

    #[test]
    fn could_connect_reads_both_ends_of_the_arc() {
        let mut cps = ring(&[(0, 0.), (2, 2000.)]);
        assert!(ContourIntersectionPoint::could_connect_next(&cps, 0));
        assert!(ContourIntersectionPoint::could_connect_prev(&cps, 0));
        cps[1].trim_prev(10.);
        assert!(!ContourIntersectionPoint::could_connect_next(&cps, 0));
        assert!(ContourIntersectionPoint::could_connect_prev(&cps, 0));
        // A lone T-joint is its own neighbour.
        let cps = ring(&[(0, 0.)]);
        assert!(!ContourIntersectionPoint::could_connect_next(&cps, 0));
        assert!(cps[0].could_take_next() && cps[0].could_take_prev());
    }

    #[test]
    fn a_missing_neighbour_stops() {
        let mut cps = vec![ContourIntersectionPoint::new(0, 0)];
        let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = next_on_contour(&cps, 0);
        }))
        .unwrap_err();
        let payload = payload.downcast::<stop::NotPorted>().unwrap();
        assert_eq!(payload.condition, stop::NULL_NEIGHBOUR);
        cps[0].next_on_contour = Some(0);
        assert_eq!(next_on_contour(&cps, 0), 0);
    }

    #[test]
    fn a_line_is_clipped_by_length_from_either_end() {
        let pl = [
            Point::new(0, 0),
            Point::new(1000, 0),
            Point::new(1000, 1000),
        ];
        let s = clip_start_segment_and_point(&pl, 1250.);
        assert!(s.valid());
        assert_eq!((s.idx_segment, s.t), (1, 0.25));
        assert_eq!(s.point, Vec2d::new(1000., 250.));
        let e = clip_end_segment_and_point(&pl, 1250.);
        assert_eq!((e.idx_segment, e.t), (0, 0.75));
        assert_eq!(e.point, Vec2d::new(750., 0.));
        assert!(!clip_start_segment_and_point(&pl, 2000.).valid());
        assert!(!clip_end_segment_and_point(&pl[..1], 10.).valid());
    }

    #[test]
    fn a_line_against_a_thick_segment() {
        let v = Vec2d::new;
        let (sa, sb) = (v(-1000., 0.), v(1000., 0.));
        let mut interval = (7., 7.);
        // Across the middle of the segment: the band |y| <= 100 of a line 1000 long.
        assert!(line_rounded_thick_segment_collision(
            v(0., -500.),
            v(0., 500.),
            sa,
            sb,
            100.,
            &mut interval
        ));
        assert!(
            near(interval.0, 400.) && near(interval.1, 600.),
            "{interval:?}"
        );
        // Past an end: the round cap, a circle of radius 100 about (1000, 0).
        assert!(line_rounded_thick_segment_collision(
            v(1060., -500.),
            v(1060., 500.),
            sa,
            sb,
            100.,
            &mut interval
        ));
        assert!(
            near(interval.0, 420.) && near(interval.1, 580.),
            "{interval:?}"
        );
        // Far away: false, the interval left as it was.
        let mut interval = (7., 7.);
        assert!(!line_rounded_thick_segment_collision(
            v(0., 500.),
            v(0., 1000.),
            sa,
            sb,
            100.,
            &mut interval
        ));
        assert_eq!(interval, (7., 7.));
        // A line shorter than SCALED_EPSILON: its middle point, the interval its length.
        assert!(line_rounded_thick_segment_collision(
            v(0., 50.),
            v(10., 50.),
            sa,
            sb,
            100.,
            &mut interval
        ));
        assert_eq!(interval, (0., 10.));
        // A segment shorter than SCALED_EPSILON: a circle about its middle whose squared radius
        // is the offset (radius 10 here), as the C++ passes it.
        assert!(line_rounded_thick_segment_collision(
            v(0., -500.),
            v(0., 500.),
            v(0., 0.),
            v(10., 0.),
            100.,
            &mut interval
        ));
        let half = orca_crt::sqrt(75.);
        assert!(
            near(interval.0, 500. - half) && near(interval.1, 500. + half),
            "{interval:?}"
        );
    }

    #[test]
    fn the_squared_distance_to_a_double_segment() {
        let v = Vec2d::new;
        assert_eq!(
            distance_to_squared_linef(v(0., 0.), v(10., 0.), v(5., 3.)),
            9.
        );
        assert_eq!(
            distance_to_squared_linef(v(0., 0.), v(10., 0.), v(-3., 4.)),
            25.
        );
        assert_eq!(
            distance_to_squared_linef(v(0., 0.), v(10., 0.), v(13., 4.)),
            25.
        );
        assert_eq!(
            distance_to_squared_linef(v(1., 1.), v(1., 1.), v(4., 5.)),
            25.
        );
    }

    #[test]
    fn infill_near_the_boundary_trims_the_t_joints_around_it() {
        let s: Coord = 10_000_000;
        let (c, params) = square(s);
        let boundary = vec![c];
        let boundary_parameters = vec![params];
        let mut cps = ring(&[(0, 0.), (2, 2. * s as f64)]);
        let boundary_intersections = vec![vec![0, 1]];
        let bbox = BoundingBox::from_corners(Point::new(0, 0), Point::new(s, s));
        // 0.2 mm above the bottom edge, 0.4 mm wide, clipped by 0.1 mm at both ends.
        let infill = vec![Polyline::new(vec![
            Point::new(2_000_000, 200_000),
            Point::new(8_000_000, 200_000),
        ])];
        mark_boundary_segments_touching_infill(
            &boundary,
            &boundary_parameters,
            &mut cps,
            &boundary_intersections,
            &bbox,
            &infill,
            100_000.,
            400_000.,
        );
        // The caps of radius 0.4 mm about (2.1, 0.2) and (7.9, 0.2) meet the edge this far
        // beyond the clipped ends.
        let cap = orca_crt::sqrt(400_000. * 400_000. - 200_000. * 200_000.);
        let next = cps[0].contour_not_taken_length_next;
        assert!(cps[0].next_trimmed && !cps[0].consumed);
        assert!((next - (2_100_000. - cap)).abs() < 1e-3, "{next}");
        let prev = cps[1].contour_not_taken_length_prev;
        assert!(cps[1].prev_trimmed);
        assert!(
            (prev - (2. * s as f64 - 7_900_000. - cap)).abs() < 1e-3,
            "{prev}"
        );
        // The other arcs are untouched.
        assert!(!cps[0].prev_trimmed && !cps[1].next_trimmed);
    }
}
