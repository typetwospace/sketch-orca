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
// Carries OrcaSlicer code: src/libslic3r/Fill/FillRectilinear.hpp lines 1-232 and
// src/libslic3r/Fill/FillRectilinear.cpp lines 1-1353 of OrcaSlicer v2.4.2, translated on
// 2026-11-14 (orca-o3-fill-2). Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu
// Lab), the PrusaSlicer authors (Prusa Research) and the Slic3r authors (Alessandro Ranellucci),
// under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! The rectilinear infill's set-up (FillRectilinear.hpp and the first part of
//! FillRectilinear.cpp): the region sliced by equally spaced vertical lines
//! ([`slice_region_by_vertical_lines`]), the intersections of each line with the contours
//! ([`SegmentIntersection`], [`SegmentedIntersectionLine`]) linked along the contours
//! ([`connect_segment_intersections_by_contours`]), the pinched inner contours split by phony
//! outer intersections ([`pinch_contours_insert_phony_outer_intersections`]), the region with its
//! inner offset ([`ExPolygonWithOffset`]) and the helpers that measure and emit pieces of a
//! perimeter. The graph walk that turns them into lines, and the fillers' `fill_surface`, are
//! fill-3's (FillRectilinear.cpp from line 1354).
//!
//! - An intersection's y is the rational `pos_p / pos_q` (`i64` over `u32`). `operator<` and
//!   `operator==` ([`SegmentIntersection::less`], [`SegmentIntersection::equal`]) compare the
//!   cross products as 96-bit numbers out of 32-bit halves, in unsigned 64-bit arithmetic that
//!   wraps as the C++'s does; [`SegmentIntersection::pos`] rounds half away from zero.
//! - The links between intersections are `int` indices (`-1` for none), as the C++ holds them;
//!   one followed while it is -1 stops ([`crate::stop::NEGATIVE_LINK`]) where the C++ indexes a
//!   vector with it.
//! - `slice_region_by_vertical_lines` truncates a segment's x span to the denominator's 32 bits
//!   (`uint32_t pos_q`), computes the vertical lines' indices in `int`, sorts each line's
//!   intersections with orca-stl's `std::sort` and throws `InfillFailedException`
//!   ([`FillError::InfillFailed`]) when a line's intersections do not pair up as outer low, inner
//!   pairs, outer high. It ignores strictly vertical segments and contour points that touch a line
//!   from one side, and keeps one of two intersections where a contour meets a line at a vertex.
//! - `connect_segment_intersections_by_contours` measures a link's length only while it is still
//!   valid, as the C++'s `&&` does, and compares it with `link_max_length` in `double`.
//! - `end_of_vertical_run_raw` and `end_of_vertical_run` return an index into the line's
//!   intersections where the C++ returns a reference (it walks by pointer from the start).
//! - `ExPolygonWithOffset` rotates the region by the angle (a `float`, as a `double`) when it is
//!   not 0, removes sticks, offsets it by `aoffset1` (mitred, the default miter limit of 3; or takes
//!   it as it is when `aoffset1` is 0) and shrinks that by `aoffset1 - aoffset2` when `aoffset2` is
//!   below 0, drops contours under `0.01 * aoffset2 * aoffset2` in area and sticks, removes
//!   duplicate points and keeps each contour's orientation.
//! - `FillRectilinear`, `FillMonotonic` and `FillMonotonicLines` are the rectilinear family of
//!   domain A (rectilinear, monotonic and monotonicline): structs over [`FillBase`] whose
//!   `fill_surface` stops with [`crate::stop::FILL_3`] until fill-3 writes it. `SweepParams` is
//!   `FillRectilinear::SweepParams`. The protected `fill_surface_by_lines`,
//!   `fill_surface_by_multilines`, `fill_surface_trapezoidal` and the class's own
//!   `extended_object_bounding_box` (FillRectilinear.cpp 2741) are fill-3's.
//!
//! Left out: the fillers outside domain A (`FillAlignedRectilinear`, `FillGrid`,
//! `FillLateralLattice`, `FillTriangles`, `FillStars`, `FillCubic`, `FillQuarterCubic`,
//! `FillLateralHoneycomb`, `FillSupportBase`, `FillZigZag`, `FillCrossZag`, `FillLockedZag`) and
//! `FillMonotonicLine`, which no `Fill::new_from_type` case makes; the three `sample_grid_pattern`
//! forms (the lightning infill's, outside domain A); `validate_segment_intersection_connectivity`
//! and the `SLIC3R_DEBUG` checks, which only asserts call; the `INFILL_DEBUG_OUTPUT` SVG export;
//! the commented-out `measure_outer_contour_slab` and the code inside `#if 0`; the `reserve` calls.

use core::cmp::Ordering;

use orca_geom::clipper::JoinType;
use orca_geom::clipper_utils::{DEFAULT_MITER_LIMIT, offset_expolygon, shrink};
use orca_geom::expolygon::{self, ExPolygon};
use orca_geom::geometry::is_ccw;
use orca_geom::multi_point::remove_duplicate_points;
use orca_geom::point::{Coord, Coordf, Point, Points};
use orca_geom::polygon::{self, Polygon, Polygons};
use orca_geom::polyline::{Polyline, Polylines};
use orca_layer::extrusion_loop::{next_idx_modulo, prev_idx_modulo};
use orca_layer::surface::Surface;

use crate::base::{Fill, FillBase, FillError, FillParams};
use crate::stop::{self, stop};

/// An intersection link (`int`, -1 for none) as an index.
///
/// # Panics
/// With [`crate::stop::NEGATIVE_LINK`] when it is negative.
fn link(function: &'static str, idx: i32) -> usize {
    match usize::try_from(idx) {
        Ok(i) => i,
        Err(_) => stop(function, stop::NEGATIVE_LINK),
    }
}

/// `segment_length(poly, seg1, p1, seg2, p2)`: the Euclidean length of the piece of the closed
/// polygon from `p1` (on the edge that ends at point `seg1`) forward to `p2` (on the edge that
/// ends at point `seg2`). Each step is the integer difference of two points, as a `double`.
#[must_use]
pub fn segment_length(poly: &Polygon, seg1: usize, p1: Point, seg2: usize, p2: Point) -> Coordf {
    let mut p_prev = p1;
    let mut len: Coordf = 0.;
    if seg1 <= seg2 {
        for &p_this in &poly.points[seg1..seg2] {
            len += (p_prev - p_this).cast_f64().norm();
            p_prev = p_this;
        }
    } else {
        for &p_this in poly.points[seg1..].iter().chain(&poly.points[..seg2]) {
            len += (p_prev - p_this).cast_f64().norm();
            p_prev = p_this;
        }
    }
    len += (p_prev - p2).cast_f64().norm();
    len
}

/// `polygon_segment_append(out, polygon, seg1, seg2)`: the polygon's points from `seg1` forward
/// to before `seg2` appended (around the end when `seg1 > seg2`; none when they are equal).
pub fn polygon_segment_append(out: &mut Points, polygon: &Polygon, seg1: usize, seg2: usize) {
    match seg1.cmp(&seg2) {
        // Nothing to append from this segment.
        Ordering::Equal => {}
        // Do not append a point pointed to by seg2.
        Ordering::Less => out.extend_from_slice(&polygon.points[seg1..seg2]),
        Ordering::Greater => {
            out.extend_from_slice(&polygon.points[seg1..]);
            // Do not append a point pointed to by seg2.
            out.extend_from_slice(&polygon.points[..seg2]);
        }
    }
}

/// `polygon_segment_append_reversed(out, polygon, seg1, seg2)`: the polygon's points from before
/// `seg1` backward to `seg2` appended; the whole loop around when `seg1 < seg2`.
pub fn polygon_segment_append_reversed(
    out: &mut Points,
    polygon: &Polygon,
    seg1: usize,
    seg2: usize,
) {
    if seg1 >= seg2 {
        out.extend(polygon.points[seg2..seg1].iter().rev());
    } else {
        // it could be, that seg1 == seg2. In that case, append the complete loop.
        out.extend(polygon.points[..seg1].iter().rev());
        out.extend(polygon.points[seg2..].iter().rev());
    }
}

/// `SegmentIntersection::SegmentIntersectionType` (a `char`): the contour an intersection is with
/// (outer, the region's first offset, or inner, its second) and whether the contour goes up
/// (`*_LOW`, the region lies above) or down there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentIntersectionType {
    /// `UNKNOWN`.
    Unknown,
    /// `OUTER_LOW`.
    OuterLow,
    /// `OUTER_HIGH`.
    OuterHigh,
    /// `INNER_LOW`.
    InnerLow,
    /// `INNER_HIGH`.
    InnerHigh,
}

/// `SegmentIntersection::LinkType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkType {
    /// `Horizontal`: a link to the line on the left or right.
    Horizontal,
    /// `Up`: a link up the same line.
    Up,
    /// `Down`: a link down the same line.
    Down,
    /// `Phony`: a phony intersection, without links.
    Phony,
}

/// `SegmentIntersection::LinkQuality`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkQuality {
    /// `Invalid`.
    Invalid,
    /// `Valid`.
    Valid,
    /// `TooLong`: valid, but too long to be followed.
    TooLong,
}

/// `SegmentIntersection::Side`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// `Left`.
    Left,
    /// `Right`.
    Right,
}

/// `SegmentIntersection::Direction`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// `Up`.
    Up,
    /// `Down`.
    Down,
}

/// `struct SegmentIntersection`: where a vertical line meets a segment of a contour of an
/// [`ExPolygonWithOffset`], and its links to the next intersections along that contour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegmentIntersection {
    /// `iContour`: the contour in the [`ExPolygonWithOffset`].
    pub i_contour: usize,
    /// `iSegment`: the segment of the contour (the index of its end point).
    pub i_segment: usize,
    /// `pos_p`: the numerator of the intersection's y.
    pub pos_p: i64,
    /// `pos_q`: the denominator of the intersection's y.
    pub pos_q: u32,
    /// `prev_on_contour`: the previous intersection along the contour, on the line to the left or
    /// on this line.
    pub prev_on_contour: i32,
    /// `next_on_contour`: the next intersection along the contour, on the line to the right or on
    /// this line.
    pub next_on_contour: i32,
    /// `type`: the kind of intersection.
    pub kind: SegmentIntersectionType,
    /// `prev_on_contour_type`.
    pub prev_on_contour_type: LinkType,
    /// `next_on_contour_type`.
    pub next_on_contour_type: LinkType,
    /// `prev_on_contour_quality`.
    pub prev_on_contour_quality: LinkQuality,
    /// `next_on_contour_quality`.
    pub next_on_contour_quality: LinkQuality,
    /// `consumed_vertical_up`: the piece of the line up from here was taken.
    pub consumed_vertical_up: bool,
    /// `consumed_perimeter_right`: the piece of the inner contour to the right was taken.
    pub consumed_perimeter_right: bool,
}

impl Default for SegmentIntersection {
    /// The C++'s member initialisers: contour and segment 0, y 0/1, links 0, the type unknown,
    /// horizontal and valid links, nothing consumed.
    fn default() -> Self {
        Self {
            i_contour: 0,
            i_segment: 0,
            pos_p: 0,
            pos_q: 1,
            prev_on_contour: 0,
            next_on_contour: 0,
            kind: SegmentIntersectionType::Unknown,
            prev_on_contour_type: LinkType::Horizontal,
            next_on_contour_type: LinkType::Horizontal,
            prev_on_contour_quality: LinkQuality::Valid,
            next_on_contour_quality: LinkQuality::Valid,
            consumed_vertical_up: false,
            consumed_perimeter_right: false,
        }
    }
}

impl SegmentIntersection {
    /// `pos()`: the y, `pos_p / pos_q` rounded half away from zero.
    #[must_use]
    pub fn pos(&self) -> Coord {
        // Division rounds both positive and negative down to zero.
        // Add half of q for an arithmetic rounding effect.
        let mut p = self.pos_p;
        if p < 0 {
            p = p.wrapping_sub(i64::from(self.pos_q >> 1));
        } else {
            p = p.wrapping_add(i64::from(self.pos_q >> 1));
        }
        p / i64::from(self.pos_q)
    }

    /// `is_inner()`.
    #[must_use]
    pub fn is_inner(&self) -> bool {
        self.kind == SegmentIntersectionType::InnerLow
            || self.kind == SegmentIntersectionType::InnerHigh
    }

    /// `is_outer()`.
    #[must_use]
    pub fn is_outer(&self) -> bool {
        self.kind == SegmentIntersectionType::OuterLow
            || self.kind == SegmentIntersectionType::OuterHigh
    }

    /// `is_low()`.
    #[must_use]
    pub fn is_low(&self) -> bool {
        self.kind == SegmentIntersectionType::InnerLow
            || self.kind == SegmentIntersectionType::OuterLow
    }

    /// `is_high()`.
    #[must_use]
    pub fn is_high(&self) -> bool {
        self.kind == SegmentIntersectionType::InnerHigh
            || self.kind == SegmentIntersectionType::OuterHigh
    }

    /// `has_left_horizontal()`.
    #[must_use]
    pub fn has_left_horizontal(&self) -> bool {
        self.prev_on_contour_type == LinkType::Horizontal
    }

    /// `has_right_horizontal()`.
    #[must_use]
    pub fn has_right_horizontal(&self) -> bool {
        self.next_on_contour_type == LinkType::Horizontal
    }

    /// `has_horizontal(side)`.
    #[must_use]
    pub fn has_horizontal(&self, side: Side) -> bool {
        if side == Side::Left {
            self.has_left_horizontal()
        } else {
            self.has_right_horizontal()
        }
    }

    /// `has_left_vertical_up()`.
    #[must_use]
    pub fn has_left_vertical_up(&self) -> bool {
        self.prev_on_contour_type == LinkType::Up
    }

    /// `has_left_vertical_down()`.
    #[must_use]
    pub fn has_left_vertical_down(&self) -> bool {
        self.prev_on_contour_type == LinkType::Down
    }

    /// `has_left_vertical(dir)`.
    #[must_use]
    pub fn has_left_vertical_dir(&self, dir: Direction) -> bool {
        if dir == Direction::Up {
            self.has_left_vertical_up()
        } else {
            self.has_left_vertical_down()
        }
    }

    /// `has_left_vertical()`.
    #[must_use]
    pub fn has_left_vertical(&self) -> bool {
        self.has_left_vertical_up() || self.has_left_vertical_down()
    }

    /// `has_left_vertical_outside()`.
    #[must_use]
    pub fn has_left_vertical_outside(&self) -> bool {
        if self.is_low() {
            self.has_left_vertical_down()
        } else {
            self.has_left_vertical_up()
        }
    }

    /// `has_right_vertical_up()`.
    #[must_use]
    pub fn has_right_vertical_up(&self) -> bool {
        self.next_on_contour_type == LinkType::Up
    }

    /// `has_right_vertical_down()`.
    #[must_use]
    pub fn has_right_vertical_down(&self) -> bool {
        self.next_on_contour_type == LinkType::Down
    }

    /// `has_right_vertical(dir)`.
    #[must_use]
    pub fn has_right_vertical_dir(&self, dir: Direction) -> bool {
        if dir == Direction::Up {
            self.has_right_vertical_up()
        } else {
            self.has_right_vertical_down()
        }
    }

    /// `has_right_vertical()`.
    #[must_use]
    pub fn has_right_vertical(&self) -> bool {
        self.has_right_vertical_up() || self.has_right_vertical_down()
    }

    /// `has_right_vertical_outside()`.
    #[must_use]
    pub fn has_right_vertical_outside(&self) -> bool {
        if self.is_low() {
            self.has_right_vertical_down()
        } else {
            self.has_right_vertical_up()
        }
    }

    /// `has_vertical()`.
    #[must_use]
    pub fn has_vertical(&self) -> bool {
        self.has_left_vertical() || self.has_right_vertical()
    }

    /// `has_vertical(side)`.
    #[must_use]
    pub fn has_vertical_side(&self, side: Side) -> bool {
        if side == Side::Left {
            self.has_left_vertical()
        } else {
            self.has_right_vertical()
        }
    }

    /// `has_vertical_up()`.
    #[must_use]
    pub fn has_vertical_up(&self) -> bool {
        self.has_left_vertical_up() || self.has_right_vertical_up()
    }

    /// `has_vertical_down()`.
    #[must_use]
    pub fn has_vertical_down(&self) -> bool {
        self.has_left_vertical_down() || self.has_right_vertical_down()
    }

    /// `has_vertical(dir)`.
    #[must_use]
    pub fn has_vertical_dir(&self, dir: Direction) -> bool {
        if dir == Direction::Up {
            self.has_vertical_up()
        } else {
            self.has_vertical_down()
        }
    }

    /// `left_horizontal()`: -1 when there is none.
    #[must_use]
    pub fn left_horizontal(&self) -> i32 {
        if self.has_left_horizontal() {
            self.prev_on_contour
        } else {
            -1
        }
    }

    /// `right_horizontal()`: -1 when there is none.
    #[must_use]
    pub fn right_horizontal(&self) -> i32 {
        if self.has_right_horizontal() {
            self.next_on_contour
        } else {
            -1
        }
    }

    /// `horizontal(side)`.
    #[must_use]
    pub fn horizontal(&self, side: Side) -> i32 {
        if side == Side::Left {
            self.left_horizontal()
        } else {
            self.right_horizontal()
        }
    }

    /// `horizontal_quality(side)`.
    #[must_use]
    pub fn horizontal_quality(&self, side: Side) -> LinkQuality {
        if side == Side::Left {
            self.prev_on_contour_quality
        } else {
            self.next_on_contour_quality
        }
    }

    /// `left_vertical_up()`: -1 when there is none.
    #[must_use]
    pub fn left_vertical_up(&self) -> i32 {
        if self.has_left_vertical_up() {
            self.prev_on_contour
        } else {
            -1
        }
    }

    /// `left_vertical_down()`: -1 when there is none.
    #[must_use]
    pub fn left_vertical_down(&self) -> i32 {
        if self.has_left_vertical_down() {
            self.prev_on_contour
        } else {
            -1
        }
    }

    /// `left_vertical(dir)`: -1 when there is none.
    #[must_use]
    pub fn left_vertical_dir(&self, dir: Direction) -> i32 {
        if self.has_left_vertical_dir(dir) {
            self.prev_on_contour
        } else {
            -1
        }
    }

    /// `left_vertical()`: -1 when there is none.
    #[must_use]
    pub fn left_vertical(&self) -> i32 {
        if self.has_left_vertical() {
            self.prev_on_contour
        } else {
            -1
        }
    }

    /// `left_vertical_outside()`.
    #[must_use]
    pub fn left_vertical_outside(&self) -> i32 {
        if self.is_low() {
            self.left_vertical_down()
        } else {
            self.left_vertical_up()
        }
    }

    /// `right_vertical_up()`: -1 when there is none.
    #[must_use]
    pub fn right_vertical_up(&self) -> i32 {
        if self.has_right_vertical_up() {
            self.next_on_contour
        } else {
            -1
        }
    }

    /// `right_vertical_down()`: -1 when there is none.
    #[must_use]
    pub fn right_vertical_down(&self) -> i32 {
        if self.has_right_vertical_down() {
            self.next_on_contour
        } else {
            -1
        }
    }

    /// `right_vertical(dir)`: -1 when there is none.
    #[must_use]
    pub fn right_vertical_dir(&self, dir: Direction) -> i32 {
        if self.has_right_vertical_dir(dir) {
            self.next_on_contour
        } else {
            -1
        }
    }

    /// `right_vertical()`: -1 when there is none.
    #[must_use]
    pub fn right_vertical(&self) -> i32 {
        if self.has_right_vertical() {
            self.next_on_contour
        } else {
            -1
        }
    }

    /// `right_vertical_outside()`.
    #[must_use]
    pub fn right_vertical_outside(&self) -> i32 {
        if self.is_low() {
            self.right_vertical_down()
        } else {
            self.right_vertical_up()
        }
    }

    /// `vertical_up(side)`.
    #[must_use]
    pub fn vertical_up_side(&self, side: Side) -> i32 {
        if side == Side::Left {
            self.left_vertical_up()
        } else {
            self.right_vertical_up()
        }
    }

    /// `vertical_down(side)`.
    #[must_use]
    pub fn vertical_down_side(&self, side: Side) -> i32 {
        if side == Side::Left {
            self.left_vertical_down()
        } else {
            self.right_vertical_down()
        }
    }

    /// `vertical_outside(side)`.
    #[must_use]
    pub fn vertical_outside_side(&self, side: Side) -> i32 {
        if side == Side::Left {
            self.left_vertical_outside()
        } else {
            self.right_vertical_outside()
        }
    }

    /// `vertical_up()`: -1 when there is no link up.
    #[must_use]
    pub fn vertical_up(&self) -> i32 {
        if self.has_left_vertical_up() {
            self.left_vertical_up()
        } else {
            self.right_vertical_up()
        }
    }

    /// `vertical_up_quality()`.
    #[must_use]
    pub fn vertical_up_quality(&self) -> LinkQuality {
        if self.has_left_vertical_up() {
            self.prev_on_contour_quality
        } else {
            self.next_on_contour_quality
        }
    }

    /// `vertical_down()`: -1 when there is no link down.
    #[must_use]
    pub fn vertical_down(&self) -> i32 {
        if self.has_left_vertical_down() {
            self.left_vertical_down()
        } else {
            self.right_vertical_down()
        }
    }

    /// `vertical_down_quality()`.
    #[must_use]
    pub fn vertical_down_quality(&self) -> LinkQuality {
        if self.has_left_vertical_down() {
            self.prev_on_contour_quality
        } else {
            self.next_on_contour_quality
        }
    }

    /// `vertical_outside()`.
    #[must_use]
    pub fn vertical_outside(&self) -> i32 {
        if self.is_low() {
            self.vertical_down()
        } else {
            self.vertical_up()
        }
    }

    /// `vertical_outside_quality()`.
    #[must_use]
    pub fn vertical_outside_quality(&self) -> LinkQuality {
        if self.is_low() {
            self.vertical_down_quality()
        } else {
            self.vertical_up_quality()
        }
    }

    /// `operator<`: `pos_p * other.pos_q < other.pos_p * pos_q`, the products as 96-bit numbers
    /// out of the 32-bit halves of the numerators' absolute values (unsigned, wrapping).
    #[must_use]
    pub fn less(&self, other: &Self) -> bool {
        if self.pos_p == 0 || other.pos_p == 0 {
            // Because the denominators are positive and one of the nominators is zero,
            // following simple statement holds.
            self.pos_p < other.pos_p
        } else {
            // None of the nominators is zero.
            let sign1: i32 = if self.pos_p > 0 { 1 } else { -1 };
            let sign2: i32 = if other.pos_p > 0 { 1 } else { -1 };
            let signs = sign1 * sign2;
            if signs < 0 {
                // The nominators have different signs.
                sign1 < 0
            } else {
                // The nominators have the same sign.
                // Absolute values
                let (p1, p2) = if sign1 > 0 {
                    (self.pos_p as u64, other.pos_p as u64)
                } else {
                    (
                        self.pos_p.wrapping_neg() as u64,
                        other.pos_p.wrapping_neg() as u64,
                    )
                };
                // Multiply low and high 32bit words of p1 by other_pos.q
                // 32bit x 32bit => 64bit
                // l_hi and l_lo overlap by 32 bits.
                let mut l_hi = (p1 >> 32).wrapping_mul(u64::from(other.pos_q));
                let mut l_lo = (p1 & 0xffff_ffff).wrapping_mul(u64::from(other.pos_q));
                l_hi = l_hi.wrapping_add(l_lo >> 32);
                let mut r_hi = (p2 >> 32).wrapping_mul(u64::from(self.pos_q));
                let mut r_lo = (p2 & 0xffff_ffff).wrapping_mul(u64::from(self.pos_q));
                r_hi = r_hi.wrapping_add(r_lo >> 32);
                // Compare the high 64 bits.
                if l_hi == r_hi {
                    // Compare the low 32 bits.
                    l_lo &= 0xffff_ffff;
                    r_lo &= 0xffff_ffff;
                    return if sign1 < 0 { l_lo > r_lo } else { l_lo < r_lo };
                }
                if sign1 < 0 { l_hi > r_hi } else { l_hi < r_hi }
            }
        }
    }

    /// `operator==`: the two rationals equal, by the same 96-bit products as [`Self::less`].
    #[must_use]
    pub fn equal(&self, other: &Self) -> bool {
        if self.pos_p == 0 || other.pos_p == 0 {
            // Because the denominators are positive and one of the nominators is zero,
            // following simple statement holds.
            return self.pos_p == other.pos_p;
        }

        // None of the nominators is zero, none of the denominators is zero.
        let positive = self.pos_p > 0;
        if positive != (other.pos_p > 0) {
            return false;
        }
        // The nominators have the same sign.
        // Absolute values
        let p1 = if positive {
            self.pos_p as u64
        } else {
            self.pos_p.wrapping_neg() as u64
        };
        let p2 = if positive {
            other.pos_p as u64
        } else {
            other.pos_p.wrapping_neg() as u64
        };
        // Multiply low and high 32bit words of p1 by other_pos.q
        // 32bit x 32bit => 64bit
        // l_hi and l_lo overlap by 32 bits.
        let l_lo = (p1 & 0xffff_ffff).wrapping_mul(u64::from(other.pos_q));
        let r_lo = (p2 & 0xffff_ffff).wrapping_mul(u64::from(self.pos_q));
        if l_lo != r_lo {
            return false;
        }
        let l_hi = (p1 >> 32).wrapping_mul(u64::from(other.pos_q));
        let r_hi = (p2 >> 32).wrapping_mul(u64::from(self.pos_q));
        l_hi.wrapping_add(l_lo >> 32) == r_hi.wrapping_add(r_lo >> 32)
    }
}

/// `struct SegmentedIntersectionLine`: a vertical line and its intersections with the contours,
/// by y.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SegmentedIntersectionLine {
    /// `idx`: the line's index.
    pub idx: usize,
    /// `pos`: the line's x.
    pub pos: Coord,
    /// `intersections`: sorted by y.
    pub intersections: Vec<SegmentIntersection>,
}

/// `phony_outer_intersection(type, pos)`: an outer intersection at y `pos` on no contour (contour
/// and segment `size_t(-1)`), with phony, invalid links of -1.
#[must_use]
pub fn phony_outer_intersection(kind: SegmentIntersectionType, pos: Coord) -> SegmentIntersection {
    SegmentIntersection {
        // Invalid contour & segment.
        i_contour: usize::MAX,
        i_segment: usize::MAX,
        pos_p: pos,
        kind,
        // Invalid prev / next.
        prev_on_contour: -1,
        next_on_contour: -1,
        prev_on_contour_type: LinkType::Phony,
        next_on_contour_type: LinkType::Phony,
        prev_on_contour_quality: LinkQuality::Invalid,
        next_on_contour_quality: LinkQuality::Invalid,
        ..SegmentIntersection::default()
    }
}

/// `struct ExPolygonWithOffset`: a region, turned, with its first (outer) and second (inner)
/// offsets; the inner contours give the segments that connect the infill lines. Contour `i` is
/// outer for `i < n_contours_outer`, else inner.
#[derive(Clone, Debug)]
pub struct ExPolygonWithOffset {
    /// `polygons_src`: the region, turned.
    pub polygons_src: ExPolygon,
    /// `polygons_outer`: offset by `aoffset1`.
    pub polygons_outer: Polygons,
    /// `polygons_inner`: offset by `aoffset2`.
    pub polygons_inner: Polygons,
    /// `n_contours_outer`.
    pub n_contours_outer: usize,
    /// `n_contours_inner`.
    pub n_contours_inner: usize,
    /// `n_contours`.
    pub n_contours: usize,
    /// `polygons_ccw` (protected): each contour's orientation.
    polygons_ccw: Vec<bool>,
}

impl ExPolygonWithOffset {
    /// `ExPolygonWithOffset(expolygon, angle, aoffset1, aoffset2)`: when `aoffset2` is 0, only the
    /// outer contours (and so only outer intersections).
    #[must_use]
    pub fn new(expolygon: &ExPolygon, angle: f32, aoffset1: Coord, aoffset2: Coord) -> Self {
        // Copy and rotate the source polygons.
        let mut polygons_src = expolygon.clone();
        if angle != 0. {
            polygons_src.contour.rotate(f64::from(angle));
            for hole in &mut polygons_src.holes {
                hole.rotate(f64::from(angle));
            }
        }

        let miter_limit = DEFAULT_MITER_LIMIT;
        // for the infill pattern, don't cut the corners.
        // default miterLimt = 3
        expolygon::remove_sticks(&mut polygons_src);
        let mut polygons_outer = if aoffset1 == 0 {
            expolygon::to_polygons(&polygons_src)
        } else {
            offset_expolygon(&polygons_src, aoffset1 as f32, JoinType::Miter, miter_limit)
        };
        let mut polygons_inner = Polygons::new();
        if aoffset2 < 0 {
            polygons_inner = shrink(
                &polygons_outer,
                (aoffset1 - aoffset2) as f32,
                JoinType::Miter,
                miter_limit,
            );
        }
        // Filter out contours with zero area or small area, contours with 2 points only.
        let min_area_threshold = 0.01 * aoffset2 as f64 * aoffset2 as f64;
        polygon::remove_small(&mut polygons_outer, min_area_threshold);
        polygon::remove_small(&mut polygons_inner, min_area_threshold);
        polygon::remove_sticks_polygons(&mut polygons_outer);
        polygon::remove_sticks_polygons(&mut polygons_inner);
        let n_contours_outer = polygons_outer.len();
        let n_contours_inner = polygons_inner.len();
        let n_contours = n_contours_outer + n_contours_inner;
        let mut out = Self {
            polygons_src,
            polygons_outer,
            polygons_inner,
            n_contours_outer,
            n_contours_inner,
            n_contours,
            polygons_ccw: vec![false; n_contours],
        };
        for i in 0..n_contours {
            remove_duplicate_points(&mut out.contour_mut(i).points);
            out.polygons_ccw[i] = is_ccw(out.contour(i));
        }
        out
    }

    /// `ExPolygonWithOffset(rhs, angle)`: a copy whose region and offsets are turned by `angle`
    /// more (the orientations kept from `rhs`).
    #[must_use]
    pub fn rotated(rhs: &Self, angle: f32) -> Self {
        let mut out = rhs.clone();
        if angle != 0. {
            out.polygons_src.contour.rotate(f64::from(angle));
            for hole in &mut out.polygons_src.holes {
                hole.rotate(f64::from(angle));
            }
            for poly in &mut out.polygons_outer {
                poly.rotate(f64::from(angle));
            }
            for poly in &mut out.polygons_inner {
                poly.rotate(f64::from(angle));
            }
        }
        out
    }

    /// `is_contour_outer(idx)`: any contour of the first offset.
    #[must_use]
    pub fn is_contour_outer(&self, idx: usize) -> bool {
        idx < self.n_contours_outer
    }

    /// `is_contour_inner(idx)`: any contour of the second offset.
    #[must_use]
    pub fn is_contour_inner(&self, idx: usize) -> bool {
        idx >= self.n_contours_outer
    }

    /// `contour(idx)`.
    #[must_use]
    pub fn contour(&self, idx: usize) -> &Polygon {
        if self.is_contour_outer(idx) {
            &self.polygons_outer[idx]
        } else {
            &self.polygons_inner[idx - self.n_contours_outer]
        }
    }

    /// `contour(idx)`, to change.
    pub fn contour_mut(&mut self, idx: usize) -> &mut Polygon {
        if self.is_contour_outer(idx) {
            &mut self.polygons_outer[idx]
        } else {
            &mut self.polygons_inner[idx - self.n_contours_outer]
        }
    }

    /// `is_contour_ccw(idx)`.
    #[must_use]
    pub fn is_contour_ccw(&self, idx: usize) -> bool {
        self.polygons_ccw[idx]
    }

    /// `bounding_box_src()`.
    #[must_use]
    pub fn bounding_box_src(&self) -> orca_geom::bounding_box::BoundingBox {
        expolygon::get_extents(&self.polygons_src)
    }

    /// `bounding_box_outer()`.
    #[must_use]
    pub fn bounding_box_outer(&self) -> orca_geom::bounding_box::BoundingBox {
        polygon::get_extents_polygons(&self.polygons_outer)
    }

    /// `bounding_box_inner()`.
    #[must_use]
    pub fn bounding_box_inner(&self) -> orca_geom::bounding_box::BoundingBox {
        polygon::get_extents_polygons(&self.polygons_inner)
    }
}

/// `distance_of_segmens(poly, seg1, seg2, forward)`: how many segments from `seg1` to `seg2`
/// along the polygon, forward or backward, in `int`.
#[must_use]
pub fn distance_of_segmens(poly: &Polygon, seg1: usize, seg2: usize, forward: bool) -> i32 {
    let mut d = (seg2 as i32).wrapping_sub(seg1 as i32);
    if !forward {
        d = d.wrapping_neg();
    }
    if d < 0 {
        d = d.wrapping_add(poly.points.len() as i32);
    }
    d
}

/// `intersection_on_prev_next_vertical_line_valid(segs, iVerticalLine, iIntersection, side)`:
/// whether the perimeter from this intersection to the line on `side` may be followed: the first
/// intersection along the contour, a valid link that lands at the end of a vertical run, and
/// neither the perimeter piece nor that run consumed.
///
/// # Panics
/// With [`crate::stop::NEGATIVE_LINK`] when it would index with -1.
#[must_use]
pub fn intersection_on_prev_next_vertical_line_valid(
    segs: &[SegmentedIntersectionLine],
    i_vertical_line: usize,
    i_intersection: usize,
    side: Side,
) -> bool {
    const FUNCTION: &str = "intersection_on_prev_next_vertical_line_valid";
    let vline_this = &segs[i_vertical_line];
    let it_this = &vline_this.intersections[i_intersection];
    if it_this.has_vertical_side(side) {
        // Not the first intersection along the contor. This intersection point
        // has been preceded by an intersection point along the vertical line.
        return false;
    }
    let i_intersection_other = it_this.horizontal(side);
    if i_intersection_other == -1 {
        return false;
    }
    let vline_other = &segs[if side == Side::Right {
        i_vertical_line + 1
    } else {
        i_vertical_line - 1
    }];
    let it_other = &vline_other.intersections[link(FUNCTION, i_intersection_other)];
    // Is iIntersectionOther at the boundary of a vertical segment?
    let it_other2 = &vline_other.intersections[link(
        FUNCTION,
        if it_other.is_low() {
            i_intersection_other.wrapping_sub(1)
        } else {
            i_intersection_other.wrapping_add(1)
        },
    )];
    if it_other2.is_inner() {
        // Cannot follow a perimeter segment into the middle of another vertical segment.
        // Only perimeter segments connecting to the end of a vertical segment are followed.
        return false;
    }
    if it_this.horizontal_quality(side) != LinkQuality::Valid {
        return false;
    }
    let perimeter_consumed = if side == Side::Right {
        it_this.consumed_perimeter_right
    } else {
        it_other.consumed_perimeter_right
    };
    if perimeter_consumed {
        // This perimeter segment was already consumed.
        return false;
    }
    let vertical_consumed = if it_other.is_low() {
        it_other.consumed_vertical_up
    } else {
        vline_other.intersections[link(FUNCTION, i_intersection_other.wrapping_sub(1))]
            .consumed_vertical_up
    };
    if vertical_consumed {
        // This vertical segment was already consumed.
        return false;
    }
    true
}

/// `intersection_on_prev_vertical_line_valid(segs, iVerticalLine, iIntersection)`: on the left.
#[must_use]
pub fn intersection_on_prev_vertical_line_valid(
    segs: &[SegmentedIntersectionLine],
    i_vertical_line: usize,
    i_intersection: usize,
) -> bool {
    intersection_on_prev_next_vertical_line_valid(segs, i_vertical_line, i_intersection, Side::Left)
}

/// `intersection_on_next_vertical_line_valid(segs, iVerticalLine, iIntersection)`: on the right.
#[must_use]
pub fn intersection_on_next_vertical_line_valid(
    segs: &[SegmentedIntersectionLine],
    i_vertical_line: usize,
    i_intersection: usize,
) -> bool {
    intersection_on_prev_next_vertical_line_valid(
        segs,
        i_vertical_line,
        i_intersection,
        Side::Right,
    )
}

/// `measure_perimeter_horizontal_segment_length(poly_with_offset, segs, iVerticalLine,
/// iIntersection, iIntersection2)`: the length of the perimeter from an intersection on line
/// `i_vertical_line` to one on the next line (forward from the low one).
#[must_use]
pub fn measure_perimeter_horizontal_segment_length(
    poly_with_offset: &ExPolygonWithOffset,
    segs: &[SegmentedIntersectionLine],
    i_vertical_line: usize,
    i_intersection: usize,
    i_intersection2: usize,
) -> Coordf {
    let i_vertical_line_other = i_vertical_line + 1;
    let vline = &segs[i_vertical_line];
    let it = &vline.intersections[i_intersection];
    let vline2 = &segs[i_vertical_line_other];
    let it2 = &vline2.intersections[i_intersection2];
    let poly = poly_with_offset.contour(it.i_contour);

    let p1 = Point::new(vline.pos, it.pos());
    let p2 = Point::new(vline2.pos, it2.pos());
    if it.is_low() {
        segment_length(poly, it.i_segment, p1, it2.i_segment, p2)
    } else {
        segment_length(poly, it2.i_segment, p2, it.i_segment, p1)
    }
}

/// `emit_perimeter_prev_next_segment(poly_with_offset, segs, iVerticalLine, iInnerContour,
/// iIntersection, iIntersection2, out, dir_is_next)`: the points of the inner perimeter from an
/// intersection to one on the next (or previous) line appended to `out`, without the first and
/// with the last.
#[allow(clippy::too_many_arguments)] // The C++'s eight arguments.
pub fn emit_perimeter_prev_next_segment(
    poly_with_offset: &ExPolygonWithOffset,
    segs: &[SegmentedIntersectionLine],
    i_vertical_line: usize,
    i_inner_contour: usize,
    i_intersection: usize,
    i_intersection2: usize,
    out: &mut Polyline,
    dir_is_next: bool,
) {
    let mut i_vertical_line_other = i_vertical_line;
    if dir_is_next {
        i_vertical_line_other += 1;
    } else {
        i_vertical_line_other -= 1;
    }

    let il = &segs[i_vertical_line];
    let itsct = &il.intersections[i_intersection];
    let il2 = &segs[i_vertical_line_other];
    let itsct2 = &il2.intersections[i_intersection2];
    let poly = poly_with_offset.contour(i_inner_contour);
    let forward = itsct.is_low() == dir_is_next;
    // Do not append the first point.
    if forward {
        polygon_segment_append(&mut out.points, poly, itsct.i_segment, itsct2.i_segment);
    } else {
        polygon_segment_append_reversed(&mut out.points, poly, itsct.i_segment, itsct2.i_segment);
    }
    // Append the last point.
    out.points.push(Point::new(il2.pos, itsct2.pos()));
}

/// `measure_perimeter_segment_on_vertical_line_length(poly_with_offset, segs, iVerticalLine,
/// iIntersection, iIntersection2, forward)`: the length of the perimeter between two
/// intersections on the same line.
#[must_use]
pub fn measure_perimeter_segment_on_vertical_line_length(
    poly_with_offset: &ExPolygonWithOffset,
    segs: &[SegmentedIntersectionLine],
    i_vertical_line: usize,
    i_intersection: usize,
    i_intersection2: usize,
    forward: bool,
) -> Coordf {
    let il = &segs[i_vertical_line];
    let itsct = &il.intersections[i_intersection];
    let itsct2 = &il.intersections[i_intersection2];
    let poly = poly_with_offset.contour(itsct.i_contour);
    let p1 = Point::new(il.pos, itsct.pos());
    let p2 = Point::new(il.pos, itsct2.pos());
    if forward {
        segment_length(poly, itsct.i_segment, p1, itsct2.i_segment, p2)
    } else {
        segment_length(poly, itsct2.i_segment, p2, itsct.i_segment, p1)
    }
}

/// `emit_perimeter_segment_on_vertical_line(poly_with_offset, segs, iVerticalLine, iInnerContour,
/// iIntersection, iIntersection2, out, forward)`: the points of the inner perimeter between two
/// intersections on the same line appended to `out`, without the first and with the last.
#[allow(clippy::too_many_arguments)] // The C++'s eight arguments.
pub fn emit_perimeter_segment_on_vertical_line(
    poly_with_offset: &ExPolygonWithOffset,
    segs: &[SegmentedIntersectionLine],
    i_vertical_line: usize,
    i_inner_contour: usize,
    i_intersection: usize,
    i_intersection2: usize,
    out: &mut Polyline,
    forward: bool,
) {
    let il = &segs[i_vertical_line];
    let itsct = &il.intersections[i_intersection];
    let itsct2 = &il.intersections[i_intersection2];
    let poly = poly_with_offset.contour(i_inner_contour);
    // Do not append the first point.
    if forward {
        polygon_segment_append(&mut out.points, poly, itsct.i_segment, itsct2.i_segment);
    } else {
        polygon_segment_append_reversed(&mut out.points, poly, itsct.i_segment, itsct2.i_segment);
    }
    // Append the last point.
    out.points.push(Point::new(il.pos, itsct2.pos()));
}

/// `DIR_FORWARD` of `enum DirectionMask`.
pub const DIR_FORWARD: i32 = 1;
/// `DIR_BACKWARD` of `enum DirectionMask`.
pub const DIR_BACKWARD: i32 = 2;

/// `slice_region_by_vertical_lines(poly_with_offset, n_vlines, x0, line_spacing)`: `n_vlines`
/// vertical lines from `x0`, `line_spacing` apart, each with its intersections with the contours,
/// sorted by y, typed and with the doubles at the contours' vertices removed.
///
/// # Errors
/// [`FillError::InfillFailed`] (the C++'s `InfillFailedException`) when a line's intersections
/// are odd in number or do not run outer low, pairs of inner, outer high.
pub fn slice_region_by_vertical_lines(
    poly_with_offset: &ExPolygonWithOffset,
    n_vlines: usize,
    x0: Coord,
    line_spacing: Coord,
) -> Result<Vec<SegmentedIntersectionLine>, FillError> {
    // Allocate storage for the segments.
    let mut segs: Vec<SegmentedIntersectionLine> =
        vec![SegmentedIntersectionLine::default(); n_vlines];
    for (i, seg) in segs.iter_mut().enumerate() {
        seg.idx = i;
        seg.pos = x0.wrapping_add((i as Coord).wrapping_mul(line_spacing));
    }
    // For each contour
    for i_contour in 0..poly_with_offset.n_contours {
        let contour = &poly_with_offset.contour(i_contour).points;
        if contour.len() < 2 {
            continue;
        }
        // For each segment
        for i_segment in 0..contour.len() {
            let i_prev = (if i_segment == 0 {
                contour.len()
            } else {
                i_segment
            }) - 1;
            let p1 = contour[i_prev];
            let p2 = contour[i_segment];
            // Which of the equally spaced vertical lines is intersected by this segment?
            let mut l = p1.x;
            let mut r = p2.x;
            if l > r {
                core::mem::swap(&mut l, &mut r);
            }
            // il, ir are the left / right indices of vertical lines intersecting a segment
            let mut il = ((l - x0) / line_spacing) as i32;
            while i64::from(il) * line_spacing + x0 < l {
                il += 1;
            }
            il = orca_stl::max(0_i32, il);
            let mut ir = ((r - x0 + line_spacing) / line_spacing) as i32;
            while i64::from(ir) * line_spacing + x0 > r {
                ir -= 1;
            }
            ir = orca_stl::min((segs.len() as i32).wrapping_sub(1), ir);
            if il > ir {
                // No vertical line intersects this segment.
                continue;
            }
            for i in il..=ir {
                let this_x = segs[i as usize].pos;
                let mut is = SegmentIntersection {
                    i_contour,
                    i_segment,
                    ..SegmentIntersection::default()
                };
                // Calculate the intersection position in y axis. x is known.
                if p1.x == this_x {
                    if p2.x == this_x {
                        // Ignore strictly vertical segments.
                        continue;
                    }
                    let p0 = contour[prev_idx_modulo(i_prev, contour.len())];
                    if (p0.x - p1.x).wrapping_mul(p2.x - p1.x) > 0 {
                        // Ignore points of a contour touching the infill line from one side.
                        continue;
                    }
                    is.pos_p = p1.y;
                    is.pos_q = 1;
                } else if p2.x == this_x {
                    let p3 = contour[next_idx_modulo(i_segment, contour.len())];
                    if (p3.x - p2.x).wrapping_mul(p1.x - p2.x) > 0 {
                        // Ignore points of a contour touching the infill line from one side.
                        continue;
                    }
                    is.pos_p = p2.y;
                    is.pos_q = 1;
                } else {
                    // First calculate the intersection parameter 't' as a rational number with
                    // non negative denominator.
                    if p2.x > p1.x {
                        is.pos_p = this_x - p1.x;
                        is.pos_q = (p2.x - p1.x) as u32;
                    } else {
                        is.pos_p = p1.x - this_x;
                        is.pos_q = (p1.x - p2.x) as u32;
                    }
                    // Make an intersection point from the 't'.
                    is.pos_p = is.pos_p.wrapping_mul(p2.y - p1.y);
                    is.pos_p = is
                        .pos_p
                        .wrapping_add(p1.y.wrapping_mul(i64::from(is.pos_q)));
                }
                segs[i as usize].intersections.push(is);
            }
        }
    }

    // Sort the intersections along their segments, specify the intersection types.
    for sil in &mut segs {
        // Sort the intersection points using exact rational arithmetic.
        orca_stl::sort_by(&mut sil.intersections, SegmentIntersection::less);
        // Assign the intersection types, remove duplicate or overlapping intersection points.
        // When a loop vertex touches a vertical line, intersection point is generated for both
        // segments. If such two segments are oriented equally, then one of them is removed.
        // Otherwise the vertex is tangential to the vertical line and both segments are removed.
        // The same rule applies, if the loop is pinched into a single point and this point
        // touches the vertical line: the loop has a zero vertical size at the vertical line,
        // therefore the intersection point is removed.
        let mut j = 0_usize;
        for i in 0..sil.intersections.len() {
            // What is the orientation of the segment at the intersection point?
            let i_contour = sil.intersections[i].i_contour;
            let contour = &poly_with_offset.contour(i_contour).points;
            let i_segment = sil.intersections[i].i_segment;
            let i_prev = prev_idx_modulo(i_segment, contour.len());
            let dir = contour[i_segment].x - contour[i_prev].x;
            let low = dir > 0;
            sil.intersections[i].kind = if poly_with_offset.is_contour_outer(i_contour) {
                if low {
                    SegmentIntersectionType::OuterLow
                } else {
                    SegmentIntersectionType::OuterHigh
                }
            } else if low {
                SegmentIntersectionType::InnerLow
            } else {
                SegmentIntersectionType::InnerHigh
            };
            let mut take_next = true;
            if j > 0 {
                let is = sil.intersections[i];
                let is2 = sil.intersections[j - 1];
                if i_contour == is2.i_contour && is.pos_q == 1 && is2.pos_q == 1 {
                    // Two successive intersection points on a vertical line with the same
                    // contour, both points are end points of their respective contour segments.
                    if is.pos_p == is2.pos_p {
                        // Two successive segments of the same direction (both to the right or
                        // both to the left) meet exactly at the vertical line.
                        // Remove the second intersection point.
                        take_next = false;
                    } else if is.kind == is2.kind {
                        // Two non successive segments of the same direction (both to the right
                        // or both to the left) meet exactly at the vertical line. That means
                        // there is a Z shaped path, where the center segment of the Z shaped path
                        // is aligned with this vertical line.
                        // Remove one of the intersection points while maximizing the vertical
                        // segment length. When low, keep the first intersection point.
                        if !low {
                            // Remove the first intersection point, keep the second intersection
                            // point.
                            sil.intersections[j - 1] = sil.intersections[i];
                        }
                        take_next = false;
                    }
                }
            }
            if take_next {
                // Vertical line intersects a contour segment at a general position (not at one
                // of its end points).
                if j < i {
                    sil.intersections[j] = sil.intersections[i];
                }
                j += 1;
            }
        }
        // Shrink the list of intersections, if any of the intersection was removed during the
        // classification.
        if j < sil.intersections.len() {
            sil.intersections.truncate(j);
        }
    }

    // Verify the segments. If something is wrong, give up.
    for sil in &segs {
        let its = &sil.intersections;
        // The intersection points have to be even.
        if its.len() & 1 != 0 {
            return Err(FillError::InfillFailed);
        }
        let mut i = 0_usize;
        while i < its.len() {
            // An intersection segment crossing the bigger contour may cross the inner offsetted
            // contour even number of times.
            if its[i].kind != SegmentIntersectionType::OuterLow {
                return Err(FillError::InfillFailed);
            }
            let mut j = i + 1;
            if j >= its.len() {
                return Err(FillError::InfillFailed);
            }
            if !(its[j].kind == SegmentIntersectionType::InnerLow
                || its[j].kind == SegmentIntersectionType::OuterHigh)
            {
                return Err(FillError::InfillFailed);
            }
            while j < its.len() && its[j].is_inner() {
                j += 1;
            }
            if j >= its.len() {
                return Err(FillError::InfillFailed);
            }
            if j & 1 != 1 {
                return Err(FillError::InfillFailed);
            }
            if its[j].kind != SegmentIntersectionType::OuterHigh {
                return Err(FillError::InfillFailed);
            }
            if !(i + 1 == j || its[j - 1].kind == SegmentIntersectionType::InnerHigh) {
                return Err(FillError::InfillFailed);
            }
            i = j + 1;
        }
    }
    Ok(segs)
}

/// `connect_segment_intersections_by_contours(poly_with_offset, segs, params, link_max_length)`:
/// each intersection linked to the previous and next intersection along its contour, on the
/// neighbouring lines or on its own (the nearest by the number of segments between), with the
/// link's type and quality: invalid when a vertical link skips an inner intersection or both links
/// go the same way along the line, too long when the lines are not connected or the perimeter is
/// longer than `link_max_length` (when that is above 0). An invalid vertical link is made invalid
/// at its other end too.
///
/// # Panics
/// With [`crate::stop::NEGATIVE_LINK`] when it measures a link that found no intersection.
pub fn connect_segment_intersections_by_contours(
    poly_with_offset: &ExPolygonWithOffset,
    segs: &mut [SegmentedIntersectionLine],
    params: &FillParams<'_>,
    link_max_length: Coord,
) {
    const FUNCTION: &str = "connect_segment_intersections_by_contours";
    for i_vline in 0..segs.len() {
        let n_intersections = segs[i_vline].intersections.len() as i32;
        for i_intersection in 0..n_intersections {
            let itsct = segs[i_vline].intersections[i_intersection as usize];
            let poly = poly_with_offset.contour(itsct.i_contour);
            let forward = itsct.is_low(); // == poly_with_offset.is_contour_ccw(intrsctn->iContour);

            // 1) Find possible connection points on the previous / next vertical line.
            // Find an intersection point on il_prev, intersecting i_intersection
            // at the same orientation as i_intersection, and being closest to i_intersection
            // in the number of contour segments, when following the direction of the contour.
            let mut iprev: i32 = -1;
            let mut d_prev = i32::MAX;
            if i_vline > 0 {
                let il_prev = &segs[i_vline - 1];
                for (i, itsct2) in il_prev.intersections.iter().enumerate() {
                    if itsct.i_contour == itsct2.i_contour && itsct.kind == itsct2.kind {
                        // The intersection points lie on the same contour and have the same
                        // orientation. Find the intersection point with a shortest path in the
                        // direction of the contour.
                        let d =
                            distance_of_segmens(poly, itsct2.i_segment, itsct.i_segment, forward);
                        if d < d_prev {
                            iprev = i as i32;
                            d_prev = d;
                        }
                    }
                }
            }

            // The same for il_next.
            let mut inext: i32 = -1;
            let mut d_next = i32::MAX;
            if i_vline + 1 < segs.len() {
                let il_next = &segs[i_vline + 1];
                for (i, itsct2) in il_next.intersections.iter().enumerate() {
                    if itsct.i_contour == itsct2.i_contour && itsct.kind == itsct2.kind {
                        // The intersection points lie on the same contour and have the same
                        // orientation. Find the intersection point with a shortest path in the
                        // direction of the contour.
                        let d =
                            distance_of_segmens(poly, itsct.i_segment, itsct2.i_segment, forward);
                        if d < d_next {
                            inext = i as i32;
                            d_next = d;
                        }
                    }
                }
            }

            // 2) Find possible connection points on the same vertical line.
            let mut same_prev = false;
            let mut same_next = false;
            // Does the perimeter intersect the current vertical line above intrsctn?
            for (i, it2) in segs[i_vline].intersections.iter().enumerate() {
                if i as i32 != i_intersection
                    && it2.i_contour == itsct.i_contour
                    && it2.kind != itsct.kind
                {
                    let mut d = distance_of_segmens(poly, it2.i_segment, itsct.i_segment, forward);
                    if d < d_prev {
                        iprev = i as i32;
                        d_prev = d;
                        same_prev = true;
                    }
                    d = distance_of_segmens(poly, itsct.i_segment, it2.i_segment, forward);
                    if d < d_next {
                        inext = i as i32;
                        d_next = d;
                        same_next = true;
                    }
                }
            }

            let mut out = itsct;
            out.prev_on_contour = iprev;
            out.prev_on_contour_type = if same_prev {
                if iprev < i_intersection {
                    LinkType::Down
                } else {
                    LinkType::Up
                }
            } else {
                LinkType::Horizontal
            };
            out.next_on_contour = inext;
            out.next_on_contour_type = if same_next {
                if inext < i_intersection {
                    LinkType::Down
                } else {
                    LinkType::Up
                }
            } else {
                LinkType::Horizontal
            };

            let il = &segs[i_vline];
            if same_prev {
                // Only follow a vertical perimeter segment if it skips just the outer
                // intersections.
                let (lo, hi) = ordered(i_intersection, iprev);
                if il.intersections[lo + 1..hi]
                    .iter()
                    .any(SegmentIntersection::is_inner)
                {
                    out.prev_on_contour_quality = LinkQuality::Invalid;
                }
            }

            if same_next {
                // Only follow a vertical perimeter segment if it skips just the outer
                // intersections.
                let (lo, hi) = ordered(i_intersection, inext);
                if il.intersections[lo + 1..hi]
                    .iter()
                    .any(SegmentIntersection::is_inner)
                {
                    out.next_on_contour_quality = LinkQuality::Invalid;
                }
            }

            // If both iprev and inext are on this vline, then there must not be any intersection
            // with the previous or next contour and we will not trace this contour when
            // generating infill.
            if same_prev && same_next && (iprev > i_intersection) == (inext > i_intersection) {
                // Both closest intersections of this contour are on the same vertical line and
                // at the same side of this point. Ignore them when tracing the infill.
                out.prev_on_contour_quality = LinkQuality::Invalid;
                out.next_on_contour_quality = LinkQuality::Invalid;
            }

            if params.dont_connect() {
                if out.prev_on_contour_quality == LinkQuality::Valid {
                    out.prev_on_contour_quality = LinkQuality::TooLong;
                }
                if out.next_on_contour_quality == LinkQuality::Valid {
                    out.next_on_contour_quality = LinkQuality::TooLong;
                }
            } else if link_max_length > 0 {
                // Measure length of the links.
                if out.prev_on_contour_quality == LinkQuality::Valid
                    && (if same_prev {
                        measure_perimeter_segment_on_vertical_line_length(
                            poly_with_offset,
                            segs,
                            i_vline,
                            link(FUNCTION, iprev),
                            i_intersection as usize,
                            forward,
                        )
                    } else {
                        measure_perimeter_horizontal_segment_length(
                            poly_with_offset,
                            segs,
                            i_vline.wrapping_sub(1),
                            link(FUNCTION, iprev),
                            i_intersection as usize,
                        )
                    }) > link_max_length as f64
                {
                    out.prev_on_contour_quality = LinkQuality::TooLong;
                }
                if out.next_on_contour_quality == LinkQuality::Valid
                    && (if same_next {
                        measure_perimeter_segment_on_vertical_line_length(
                            poly_with_offset,
                            segs,
                            i_vline,
                            i_intersection as usize,
                            link(FUNCTION, inext),
                            forward,
                        )
                    } else {
                        measure_perimeter_horizontal_segment_length(
                            poly_with_offset,
                            segs,
                            i_vline,
                            i_intersection as usize,
                            link(FUNCTION, inext),
                        )
                    }) > link_max_length as f64
                {
                    out.next_on_contour_quality = LinkQuality::TooLong;
                }
            }
            segs[i_vline].intersections[i_intersection as usize] = out;
        }

        // Make the LinkQuality::Invalid symmetric on vertical connections.
        for i_intersection in 0..segs[i_vline].intersections.len() {
            let it = segs[i_vline].intersections[i_intersection];
            if it.has_left_vertical() && it.prev_on_contour_quality == LinkQuality::Invalid {
                let k = link(FUNCTION, it.left_vertical());
                segs[i_vline].intersections[k].prev_on_contour_quality = LinkQuality::Invalid;
            }
            if it.has_right_vertical() && it.next_on_contour_quality == LinkQuality::Invalid {
                let k = link(FUNCTION, it.right_vertical());
                segs[i_vline].intersections[k].next_on_contour_quality = LinkQuality::Invalid;
            }
        }
    }
}

/// The two indices of a vertical link, the lower first (the C++ swaps two pointers into the line).
fn ordered(a: i32, b: i32) -> (usize, usize) {
    let (a, b) = (
        link("connect_segment_intersections_by_contours", a),
        link("connect_segment_intersections_by_contours", b),
    );
    if a > b { (b, a) } else { (a, b) }
}

/// `pinch_contours_insert_phony_outer_intersections(segs)`: where an inner contour runs inner
/// high then inner low along a line without the vertical link between them (the outer contour
/// squeezed it into two loops), a phony outer high and outer low pair inserted between them at the
/// mean of their y, and the links into the line renumbered. The first line is left as it is.
///
/// # Panics
/// With [`crate::stop::NEGATIVE_LINK`] when it renumbers a link of -1.
pub fn pinch_contours_insert_phony_outer_intersections(segs: &mut [SegmentedIntersectionLine]) {
    const FUNCTION: &str = "pinch_contours_insert_phony_outer_intersections";
    // Keep the vector outside the loops, so they will not be reallocated.
    // Where to insert new outer points.
    let mut insert_after: Vec<usize> = Vec::new();
    // Mapping of indices of current intersection line after inserting new outer points.
    let mut map: Vec<i32> = Vec::new();
    let mut temp_intersections: Vec<SegmentIntersection> = Vec::new();

    for i_vline in 1..segs.len() {
        if segs[i_vline].intersections.is_empty() {
            continue;
        }
        {
            let its = &segs[i_vline].intersections;
            let end = its.len() - 1;
            insert_after.clear();
            let mut it = 1_usize;
            while it != end {
                if its[it].kind == SegmentIntersectionType::OuterHigh {
                    it += 2;
                } else {
                    it += 1;
                    let hi = it;
                    it += 1;
                    let lo2 = it;
                    if its[lo2].kind == SegmentIntersectionType::InnerLow {
                        // INNER_HIGH followed by INNER_LOW. The outer contour may have squeezed
                        // the inner contour into two separate loops. In that case one shall insert
                        // a phony OUTER_HIGH / OUTER_LOW pair.
                        let up = its[hi].vertical_up();
                        let dn = its[lo2].vertical_down();
                        let pinched = dn.wrapping_add(1) != up;
                        if pinched {
                            // hi is not connected with its inner contour to lo2.
                            // Insert a phony OUTER_HIGH / OUTER_LOW pair.
                            insert_after.push(hi);
                        }
                    }
                }
            }
        }

        if insert_after.is_empty() {
            continue;
        }
        // Insert phony OUTER_HIGH / OUTER_LOW pairs, adjust indices pointing to intersection
        // points on this contour.
        map.clear();
        {
            let il = &mut segs[i_vline];
            let mut i = 0_usize;
            temp_intersections.clear();
            for &idx_inset_after in &insert_after {
                while i <= idx_inset_after {
                    map.push(temp_intersections.len() as i32);
                    temp_intersections.push(il.intersections[i]);
                    i += 1;
                }
                let last = temp_intersections[temp_intersections.len() - 1].pos();
                let pos = (last + il.intersections[i].pos()) / 2;
                temp_intersections.push(phony_outer_intersection(
                    SegmentIntersectionType::OuterHigh,
                    pos,
                ));
                temp_intersections.push(phony_outer_intersection(
                    SegmentIntersectionType::OuterLow,
                    pos,
                ));
            }
            while i < il.intersections.len() {
                map.push(temp_intersections.len() as i32);
                temp_intersections.push(il.intersections[i]);
                i += 1;
            }
            core::mem::swap(&mut temp_intersections, &mut il.intersections);
        }
        // Reindex references on current intersection line.
        for ip in &mut segs[i_vline].intersections {
            if ip.has_left_vertical() {
                ip.prev_on_contour = map[link(FUNCTION, ip.prev_on_contour)];
            }
            if ip.has_right_vertical() {
                ip.next_on_contour = map[link(FUNCTION, ip.next_on_contour)];
            }
        }
        // Reindex references on previous intersection line.
        for ip in &mut segs[i_vline - 1].intersections {
            if ip.has_right_horizontal() {
                ip.next_on_contour = map[link(FUNCTION, ip.next_on_contour)];
            }
        }
        if i_vline + 1 < segs.len() {
            // Reindex references on next intersection line.
            for ip in &mut segs[i_vline + 1].intersections {
                if ip.has_left_horizontal() {
                    ip.prev_on_contour = map[link(FUNCTION, ip.prev_on_contour)];
                }
            }
        }
    }
}

/// `end_of_vertical_run_raw(start)`: from an inner low intersection `start` of `intersections`
/// (a line's), the index of the last inner high before the next outer high, or of that outer high
/// when the intersection before it is outer.
#[must_use]
pub fn end_of_vertical_run_raw(intersections: &[SegmentIntersection], start: usize) -> usize {
    // Step back to the beginning of the vertical segment to mark it as consumed.
    let mut it = start;
    loop {
        it += 1;
        if intersections[it].kind == SegmentIntersectionType::OuterHigh {
            break;
        }
    }
    if intersections[it - 1].is_inner() {
        // Step back.
        it -= 1;
    }
    it
}

/// `end_of_vertical_run(il, start)`: as [`end_of_vertical_run_raw`], then on up through each
/// valid vertical link up to the next run. The C++'s `const` and non-`const` forms are this one,
/// returning an index into `il.intersections`.
///
/// # Panics
/// Never by a link of -1: it follows a link up only when there is one.
#[must_use]
pub fn end_of_vertical_run(il: &SegmentedIntersectionLine, start: usize) -> usize {
    let mut end = end_of_vertical_run_raw(&il.intersections, start);
    loop {
        let e = &il.intersections[end];
        let up = e.vertical_up();
        if up == -1
            || (if e.has_left_vertical_up() {
                e.prev_on_contour_quality
            } else {
                e.next_on_contour_quality
            }) != LinkQuality::Valid
        {
            break;
        }
        let new_start = link("end_of_vertical_run", up);
        end = end_of_vertical_run_raw(&il.intersections, new_start);
    }
    end
}

/// `FillRectilinear::SweepParams`: one sweep of a fill by several directions.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SweepParams {
    /// `angle_base`.
    pub angle_base: f32,
    /// `pattern_shift`.
    pub pattern_shift: f32,
}

/// `class FillRectilinear`: lines in one direction, joined along the perimeters.
#[derive(Clone, Debug, Default)]
pub struct FillRectilinear<'a> {
    /// The fields of `class Fill`.
    pub base: FillBase<'a>,
}

impl<'a> Fill<'a> for FillRectilinear<'a> {
    fn base(&self) -> &FillBase<'a> {
        &self.base
    }
    fn base_mut(&mut self) -> &mut FillBase<'a> {
        &mut self.base
    }
    fn clone_box(&self) -> Box<dyn Fill<'a> + 'a> {
        Box::new(self.clone())
    }
    fn is_self_crossing(&self) -> bool {
        false
    }
    /// `FillRectilinear::fill_surface`, fill-3's.
    ///
    /// # Panics
    /// Until fill-3 writes it, with [`crate::stop::FILL_3`].
    fn fill_surface(
        &mut self,
        _surface: &Surface,
        _params: &FillParams<'_>,
    ) -> Result<Polylines, FillError> {
        stop("FillRectilinear::fill_surface", stop::FILL_3)
    }
}

/// `class FillMonotonic`: the rectilinear lines, all printed the same way, unsorted.
#[derive(Clone, Debug, Default)]
pub struct FillMonotonic<'a> {
    /// The fields of `class Fill`.
    pub base: FillBase<'a>,
}

impl<'a> Fill<'a> for FillMonotonic<'a> {
    fn base(&self) -> &FillBase<'a> {
        &self.base
    }
    fn base_mut(&mut self) -> &mut FillBase<'a> {
        &mut self.base
    }
    fn clone_box(&self) -> Box<dyn Fill<'a> + 'a> {
        Box::new(self.clone())
    }
    fn no_sort(&self) -> bool {
        true
    }
    fn is_self_crossing(&self) -> bool {
        false
    }
    /// `FillMonotonic::fill_surface`, fill-3's.
    ///
    /// # Panics
    /// Until fill-3 writes it, with [`crate::stop::FILL_3`].
    fn fill_surface(
        &mut self,
        _surface: &Surface,
        _params: &FillParams<'_>,
    ) -> Result<Polylines, FillError> {
        stop("FillMonotonic::fill_surface", stop::FILL_3)
    }
}

/// `class FillMonotonicLines` (Orca's, from PrusaSlicer, for `ipMonotonicLine`): the monotonic
/// lines, unsorted.
#[derive(Clone, Debug, Default)]
pub struct FillMonotonicLines<'a> {
    /// The fields of `class Fill`.
    pub base: FillBase<'a>,
}

impl<'a> Fill<'a> for FillMonotonicLines<'a> {
    fn base(&self) -> &FillBase<'a> {
        &self.base
    }
    fn base_mut(&mut self) -> &mut FillBase<'a> {
        &mut self.base
    }
    fn clone_box(&self) -> Box<dyn Fill<'a> + 'a> {
        Box::new(self.clone())
    }
    fn no_sort(&self) -> bool {
        true
    }
    fn is_self_crossing(&self) -> bool {
        false
    }
    /// `FillMonotonicLines::fill_surface`, fill-3's.
    ///
    /// # Panics
    /// Until fill-3 writes it, with [`crate::stop::FILL_3`].
    fn fill_surface(
        &mut self,
        _surface: &Surface,
        _params: &FillParams<'_>,
    ) -> Result<Polylines, FillError> {
        stop("FillMonotonicLines::fill_surface", stop::FILL_3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use SegmentIntersectionType::{InnerHigh, InnerLow, OuterHigh, OuterLow};

    fn polygon(points: &[(Coord, Coord)]) -> Polygon {
        Polygon::new(points.iter().map(|&(x, y)| Point::new(x, y)).collect())
    }

    /// A counter-clockwise square from 0 to `size`.
    fn square(size: Coord) -> Polygon {
        polygon(&[(0, 0), (size, 0), (size, size), (0, size)])
    }

    fn at(pos_p: i64, pos_q: u32) -> SegmentIntersection {
        SegmentIntersection {
            pos_p,
            pos_q,
            ..SegmentIntersection::default()
        }
    }

    #[test]
    fn segment_length_runs_forward_and_around_the_end() {
        let sq = square(10);
        // From (5, 0) on the edge ending at point 1, to (10, 5) on the edge ending at point 2.
        let forward = segment_length(&sq, 1, Point::new(5, 0), 2, Point::new(10, 5));
        assert_eq!(forward, 10.);
        // From (0, 5) on the edge ending at point 0 (from point 3), around to (5, 0).
        let around = segment_length(&sq, 0, Point::new(0, 5), 1, Point::new(5, 0));
        assert_eq!(around, 10.);
        let wrapped = segment_length(&sq, 3, Point::new(5, 10), 1, Point::new(5, 0));
        assert_eq!(wrapped, 20.);
    }

    #[test]
    fn the_appends_take_the_points_between_the_segments() {
        let sq = square(10);
        let mut out = Points::new();
        polygon_segment_append(&mut out, &sq, 1, 3);
        assert_eq!(out, vec![Point::new(10, 0), Point::new(10, 10)]);
        out.clear();
        polygon_segment_append(&mut out, &sq, 3, 1);
        assert_eq!(out, vec![Point::new(0, 10), Point::new(0, 0)]);
        out.clear();
        polygon_segment_append(&mut out, &sq, 2, 2);
        assert!(out.is_empty());
        out.clear();
        polygon_segment_append_reversed(&mut out, &sq, 3, 1);
        assert_eq!(out, vec![Point::new(10, 10), Point::new(10, 0)]);
        out.clear();
        // seg1 below seg2: back to the start, then from the end down to seg2.
        polygon_segment_append_reversed(&mut out, &sq, 1, 3);
        assert_eq!(out, vec![Point::new(0, 0), Point::new(0, 10)]);
        out.clear();
        // Equal: nothing (seg1 >= seg2 takes the empty range).
        polygon_segment_append_reversed(&mut out, &sq, 2, 2);
        assert!(out.is_empty());
    }

    #[test]
    fn pos_rounds_half_away_from_zero() {
        assert_eq!(at(7, 2).pos(), 4);
        assert_eq!(at(-7, 2).pos(), -4);
        assert_eq!(at(5, 4).pos(), 1);
        assert_eq!(at(-5, 4).pos(), -1);
        assert_eq!(at(6, 4).pos(), 2);
        assert_eq!(at(0, 9).pos(), 0);
    }

    #[test]
    fn the_rationals_compare_exactly() {
        assert!(at(1, 3).equal(&at(2, 6)));
        assert!(!at(1, 3).less(&at(2, 6)) && !at(2, 6).less(&at(1, 3)));
        assert!(at(1, 3).less(&at(1, 2)));
        assert!(at(-1, 2).less(&at(-1, 3)));
        assert!(at(-1, 2).less(&at(1, 9)));
        assert!(!at(1, 9).less(&at(-1, 2)));
        assert!(at(0, 1).less(&at(1, u32::MAX)));
        assert!(at(-1, u32::MAX).less(&at(0, 5)));
        // Numerators past 32 bits, where only the low words differ.
        let big = 1_i64 << 40;
        assert!(at(big, 3).less(&at(big + 1, 3)));
        assert!(at(-big - 1, 3).less(&at(-big, 3)));
        assert!(at(big * 3, 3).equal(&at(big, 1)));
        assert!(!at(big * 3 + 1, 3).equal(&at(big, 1)));
        // The low word's product carries into the high word.
        assert!(at(0xffff_ffff, 2).less(&at(0x1_0000_0000, 2)));
        assert!(!at(0x1_0000_0000, 2).less(&at(0xffff_ffff, 2)));
    }

    #[test]
    fn a_square_and_its_offsets_slice_into_paired_intersections() {
        let region = ExPolygon::new(square(1000));
        let pwo = ExPolygonWithOffset::new(&region, 0., -100, -200);
        assert_eq!((pwo.n_contours_outer, pwo.n_contours_inner), (1, 1));
        assert!(pwo.is_contour_ccw(0) && pwo.is_contour_ccw(1));
        assert_eq!(pwo.bounding_box_outer().min, Point::new(100, 100));
        assert_eq!(pwo.bounding_box_inner().max, Point::new(800, 800));
        // Lines at x = 150, 250, ..., 850.
        let segs = slice_region_by_vertical_lines(&pwo, 8, 150, 100).unwrap();
        assert_eq!(segs.len(), 8);
        assert_eq!(segs[3].pos, 450);
        // Outside the inner offset: the outer contour only.
        for i in [0, 7] {
            let kinds: Vec<_> = segs[i].intersections.iter().map(|s| s.kind).collect();
            assert_eq!(kinds, vec![OuterLow, OuterHigh]);
            assert_eq!(segs[i].intersections[0].pos(), 100);
            assert_eq!(segs[i].intersections[1].pos(), 900);
        }
        for il in &segs[1..7] {
            let kinds: Vec<_> = il.intersections.iter().map(|s| s.kind).collect();
            assert_eq!(kinds, vec![OuterLow, InnerLow, InnerHigh, OuterHigh]);
            let ys: Vec<_> = il
                .intersections
                .iter()
                .map(SegmentIntersection::pos)
                .collect();
            assert_eq!(ys, vec![100, 200, 800, 900]);
        }
    }

    #[test]
    fn a_line_through_a_vertex_keeps_one_intersection() {
        // A diamond whose left and right vertices lie on lines; the top and bottom vertices on
        // the middle line.
        let region = ExPolygon::new(polygon(&[(0, 50), (50, 0), (100, 50), (50, 100)]));
        let pwo = ExPolygonWithOffset::new(&region, 0., 0, 0);
        let segs = slice_region_by_vertical_lines(&pwo, 3, 0, 50).unwrap();
        // The side vertices touch their lines from one side: nothing.
        assert!(segs[0].intersections.is_empty());
        assert!(segs[2].intersections.is_empty());
        // The middle line meets the bottom and top vertices, each once.
        let ys: Vec<_> = segs[1]
            .intersections
            .iter()
            .map(SegmentIntersection::pos)
            .collect();
        assert_eq!(ys, vec![0, 100]);
        let kinds: Vec<_> = segs[1].intersections.iter().map(|s| s.kind).collect();
        assert_eq!(kinds, vec![OuterLow, OuterHigh]);
    }

    #[test]
    fn a_clockwise_contour_fails_the_infill() {
        let mut cw = square(1000);
        cw.points.reverse();
        let pwo = ExPolygonWithOffset::new(&ExPolygon::new(cw), 0., 0, 0);
        assert!(!pwo.is_contour_ccw(0));
        assert_eq!(
            slice_region_by_vertical_lines(&pwo, 3, 250, 250),
            Err(FillError::InfillFailed)
        );
    }

    #[test]
    fn the_links_follow_the_contours() {
        let region = ExPolygon::new(square(1000));
        let pwo = ExPolygonWithOffset::new(&region, 0., -100, -200);
        let mut segs = slice_region_by_vertical_lines(&pwo, 8, 150, 100).unwrap();
        connect_segment_intersections_by_contours(&pwo, &mut segs, &FillParams::default(), 0);
        // The outer contour's bottom runs right: each outer low links to its neighbours'.
        for il in &segs[1..7] {
            let low = &il.intersections[0];
            assert!(low.has_left_horizontal() && low.has_right_horizontal());
            assert_eq!((low.prev_on_contour, low.next_on_contour), (0, 0));
        }
        // The inner contour's bottom ends at x = 800, between lines 6 and 7: up its right side
        // to the inner high on the same line.
        let last = &segs[6].intersections[1];
        assert_eq!(last.kind, InnerLow);
        assert!(last.has_right_vertical_up());
        assert_eq!(last.next_on_contour, 2);
        assert_eq!(last.vertical_up(), 2);
        assert_eq!(last.vertical_up_quality(), LinkQuality::Valid);
        // The inner low on line 3 runs on along the bottom to the next line's.
        let mid = &segs[3].intersections[1];
        assert!(mid.has_left_horizontal() && mid.has_right_horizontal());
        assert_eq!(mid.vertical_up(), -1);
        // From line 6's inner low the run goes up to its inner high.
        assert_eq!(end_of_vertical_run_raw(&segs[6].intersections, 1), 2);
        assert_eq!(end_of_vertical_run(&segs[6], 1), 2);
    }

    #[test]
    fn a_pinched_inner_contour_gets_a_phony_outer_pair() {
        let mut segs = vec![SegmentedIntersectionLine::default(); 2];
        let kinds = [
            OuterLow, InnerLow, InnerHigh, InnerLow, InnerHigh, OuterHigh,
        ];
        for (i, kind) in kinds.into_iter().enumerate() {
            segs[1].intersections.push(SegmentIntersection {
                kind,
                pos_p: 100 * i as i64,
                ..SegmentIntersection::default()
            });
        }
        // Line 0's intersection points right at line 1's fourth (the second inner low).
        segs[0].intersections.push(SegmentIntersection {
            kind: OuterLow,
            next_on_contour: 3,
            ..SegmentIntersection::default()
        });
        pinch_contours_insert_phony_outer_intersections(&mut segs);
        let kinds: Vec<_> = segs[1].intersections.iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            vec![
                OuterLow, InnerLow, InnerHigh, OuterHigh, OuterLow, InnerLow, InnerHigh, OuterHigh
            ]
        );
        let phony = &segs[1].intersections[3];
        assert_eq!(phony.pos(), 250);
        assert_eq!(phony.i_contour, usize::MAX);
        assert_eq!(phony.prev_on_contour_type, LinkType::Phony);
        assert_eq!(phony.next_on_contour_quality, LinkQuality::Invalid);
        assert_eq!(segs[0].intersections[0].next_on_contour, 5);
    }

    #[test]
    fn the_rectilinear_fillers_answer_and_their_fill_waits_for_fill_3() {
        let mut f = FillMonotonic::default();
        assert!(f.no_sort() && !f.is_self_crossing());
        assert!(!FillRectilinear::default().no_sort());
        assert!(FillMonotonicLines::default().no_sort());
        let s = Surface::with_expolygon(
            orca_layer::surface::SurfaceType::Internal,
            ExPolygon::new(square(1000)),
        );
        let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = f.fill_surface(&s, &FillParams::default());
        }))
        .unwrap_err();
        let payload = payload.downcast::<stop::NotPorted>().unwrap();
        assert_eq!(payload.condition, stop::FILL_3);
    }
}
