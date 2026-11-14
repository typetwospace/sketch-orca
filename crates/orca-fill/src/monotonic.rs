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
// Carries OrcaSlicer code: src/libslic3r/Fill/FillRectilinear.cpp lines 1590-2738 of OrcaSlicer
// v2.4.2, translated on 2026-11-14 (orca-o3-fill-3). Copyright the OrcaSlicer authors, the
// BambuStudio authors (Bambu Lab), the PrusaSlicer authors (Prusa Research) and the Slic3r authors
// (Alessandro Ranellucci), under the GNU Affero General Public License, version 3 (AGPL-3.0;
// LICENSE).

//! The monotonic infill (FillRectilinear.cpp): the vertical lines' pieces grouped into monotonic
//! regions ([`generate_montonous_regions`]), the regions linked to their neighbours and measured
//! ([`connect_monotonic_regions`]), an order of the regions found by an ant colony
//! ([`chain_monotonic_regions`]) and the lines drawn in that order ([`polylines_from_paths`]).
//!
//! - The C++'s pointers become indices: an intersection is its index in its line's
//!   `intersections`, a region its index in the regions' vector, an [`AntPath`] its index in the
//!   [`AntPathMatrix`]. A pointer that may be null is an `Option`.
//! - `connect_monotonic_regions` sorts its two maps by the intersection's address, which across
//!   the lines' vectors is the heap's order. Each key is a different intersection, and every look
//!   up finds its own key, so the maps are sorted here by (line, index) and give the same answers.
//!   A key that is not there stops ([`crate::stop::REGION_LINK_MISSING`]), where the C++ takes
//!   whatever entry the address order puts next.
//! - The neighbour lists are sorted by the regions' addresses, which are their indices.
//! - `AntPathMatrix::operator()` measures the perimeter between two regions only when
//!   `region_from.right.vline + 1 == region_from.left.vline`, which a region's own two sides never
//!   are; so every length is the Euclidean distance of the two end points, in `float`
//!   (`Vec2f(...).norm()`, unscaled as a `float`). The code is kept as the C++ has it.
//! - `chain_monotonic_regions` draws from the caller's `std::mt19937_64` (orca-stl's
//!   [`Mt19937_64`]): `std::uniform_int_distribution<>` for the first region (orca-stl's
//!   [`UniformIntDistribution`], MSVC's algorithm), `rng() > rng.max() / 2` for its direction and
//!   `float(rng()) / float(rng.max())` for each dice, each conversion of the 64-bit draw to `float`
//!   correctly rounded as the C++'s. `pow(float, float)` is MSVC's `float` overload, `powf`.
//! - After each round the pheromones are reinforced along `path`, which after a better path's
//!   `std::swap` holds the earlier best path (empty after the first), and otherwise the last ant's
//!   path: the C++ does that, not the best path its comment names.
//! - Where the C++ would follow a null region or step from an empty candidate list (its asserts
//!   compiled out), it stops ([`crate::stop::REGION_LINK_MISSING`]).
//!
//! Left out: the `SLIC3R_DEBUG_MONOTONOUS_REGIONS` overlap test and `validate_unprocessed`, which
//! only asserts call; `export_monotonous_regions_to_svg` (`INFILL_DEBUG_OUTPUT`); `print_ant`,
//! empty without `SLIC3R_DEBUG_ANTS`; the `#if 0` printing; the `reserve` calls.
//! `monotonic_3_opt` has an empty body in the C++ and is kept empty.

use orca_eigen::vector::Vec2f;
use orca_geom::multi_point::remove_duplicate_points;
use orca_geom::point::{EPSILON, Point, SCALED_EPSILON, SCALING_FACTOR};
use orca_geom::polyline::{Polyline, Polylines};
use orca_stl::{Mt19937_64, UniformIntDistribution};

use crate::graph::nearly_empty;
use crate::rectilinear::{
    ExPolygonWithOffset, LinkQuality, SegmentIntersectionType, SegmentedIntersectionLine, Side,
    emit_perimeter_prev_next_segment, emit_perimeter_segment_on_vertical_line, end_of_vertical_run,
    measure_perimeter_horizontal_segment_length,
};
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

/// `MonotonicRegion::Boundary`: a line and the low and high intersections of its run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Boundary {
    /// `vline`.
    pub vline: i32,
    /// `low`.
    pub low: i32,
    /// `high`.
    pub high: i32,
}

/// `struct MonotonicRegion`: runs of neighbouring lines, each overlapping only the next.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MonotonicRegion {
    /// `left`.
    pub left: Boundary,
    /// `right`.
    pub right: Boundary,
    /// `len1`: the length when starting at `left.low`.
    pub len1: f32,
    /// `len2`: the length when starting at `left.high`.
    pub len2: f32,
    /// `flips`: starting at `left.low` ends at `right.high` and the other way round; else on the
    /// same side.
    pub flips: bool,
    /// `left_neighbors`: the regions that must be printed before this one.
    pub left_neighbors: Vec<usize>,
    /// `right_neighbors`: the regions the ants may take next.
    pub right_neighbors: Vec<usize>,
}

impl MonotonicRegion {
    /// `length(region_flipped)`.
    #[must_use]
    pub fn length(&self, region_flipped: bool) -> f32 {
        if region_flipped { self.len2 } else { self.len1 }
    }

    /// `left_intersection_point(region_flipped)`.
    #[must_use]
    pub fn left_intersection_point(&self, region_flipped: bool) -> i32 {
        if region_flipped {
            self.left.high
        } else {
            self.left.low
        }
    }

    /// `right_intersection_point(region_flipped)`.
    #[must_use]
    pub fn right_intersection_point(&self, region_flipped: bool) -> i32 {
        if region_flipped == self.flips {
            self.right.low
        } else {
            self.right.high
        }
    }
}

/// `struct AntPath`: a link from the end of one region to the start of another.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AntPath {
    /// `length`: of the link, -1 until measured.
    pub length: f32,
    /// `visibility`: `1 / length`.
    pub visibility: f32,
    /// `pheromone`.
    pub pheromone: f32,
}

/// `struct MonotonicRegionLink`: a region on a path, its direction and its links to the next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MonotonicRegionLink {
    /// `region`.
    pub region: usize,
    /// `flipped`.
    pub flipped: bool,
    /// `next`: the [`AntPathMatrix`] entry to the next region, as the two regions' directions are.
    pub next: Option<usize>,
    /// `next_flipped`: the entry to the next region with both directions turned.
    pub next_flipped: Option<usize>,
}

impl MonotonicRegionLink {
    fn new(region: usize, flipped: bool) -> Self {
        Self {
            region,
            flipped,
            next: None,
            next_flipped: None,
        }
    }
}

/// `class AntPathMatrix`: the links between the regions' ends, both directions of each, measured
/// when first asked for.
#[derive(Debug)]
pub struct AntPathMatrix<'r> {
    regions: &'r [MonotonicRegion],
    poly_with_offset: &'r ExPolygonWithOffset,
    segs: &'r [SegmentedIntersectionLine],
    /// `m_matrix`.
    pub matrix: Vec<AntPath>,
}

impl<'r> AntPathMatrix<'r> {
    /// `AntPathMatrix(regions, poly_with_offset, segs, initial_pheromone)`: `4 n²` links of length
    /// -1.
    #[must_use]
    pub fn new(
        regions: &'r [MonotonicRegion],
        poly_with_offset: &'r ExPolygonWithOffset,
        segs: &'r [SegmentedIntersectionLine],
        initial_pheromone: f32,
    ) -> Self {
        Self {
            regions,
            poly_with_offset,
            segs,
            matrix: vec![
                AntPath {
                    length: -1.,
                    visibility: -1.,
                    pheromone: initial_pheromone,
                };
                regions.len() * regions.len() * 4
            ],
        }
    }

    /// `update_inital_pheromone(initial_pheromone)`.
    pub fn update_inital_pheromone(&mut self, initial_pheromone: f32) {
        for ap in &mut self.matrix {
            ap.pheromone = initial_pheromone;
        }
    }

    /// `operator()(region_from, flipped_from, region_to, flipped_to)`: the index of the link, its
    /// length and visibility worked out the first time.
    pub fn at(&mut self, from: usize, flipped_from: bool, to: usize, flipped_to: bool) -> usize {
        let row = 2 * from as i32 + i32::from(flipped_from);
        let col = 2 * to as i32 + i32::from(flipped_to);
        let idx = row as usize * self.regions.len() * 2 + col as usize;
        if self.matrix[idx].length == -1. {
            // This path is accessed for the first time. Update the length and cost.
            let region_from = &self.regions[from];
            let region_to = &self.regions[to];
            let i_from = link("AntPathMatrix", region_from.right_intersection_point(flipped_from));
            let i_to = link("AntPathMatrix", region_to.left_intersection_point(flipped_to));
            let vline_from = &self.segs[region_from.right.vline as usize];
            let vline_to = &self.segs[region_to.left.vline as usize];
            let mut length = self.matrix[idx].length;
            if region_from.right.vline + 1 == region_from.left.vline {
                let i_right = vline_from.intersections[i_from].right_horizontal();
                if i_right == i_to as i32
                    && vline_from.intersections[i_from].next_on_contour_quality
                        == LinkQuality::Valid
                {
                    // Measure length along the contour.
                    length = (measure_perimeter_horizontal_segment_length(
                        self.poly_with_offset,
                        self.segs,
                        region_from.right.vline as usize,
                        i_from,
                        i_to,
                    ) as f32)
                        * SCALING_FACTOR as f32;
                }
            }
            if length == -1. {
                // Just apply the Eucledian distance of the end points.
                let v = Vec2f::new(
                    vline_to.pos.wrapping_sub(vline_from.pos) as f32,
                    vline_to.intersections[i_to]
                        .pos()
                        .wrapping_sub(vline_from.intersections[i_from].pos())
                        as f32,
                );
                length = v.norm() * SCALING_FACTOR as f32;
            }
            let path = &mut self.matrix[idx];
            path.length = length;
            path.visibility = 1. / (length + EPSILON as f32);
        }
        idx
    }
}

/// `vertical_run_bottom(vline, start)`: from an inner intersection, the lowest `INNER_LOW` of its
/// run, going down through the runs that valid vertical links join.
#[must_use]
pub fn vertical_run_bottom(vline: &SegmentedIntersectionLine, start: usize) -> usize {
    let mut it = start;
    // Find the lowest SegmentIntersection::INNER_LOW starting with right.
    loop {
        while vline.intersections[it].kind != SegmentIntersectionType::InnerLow {
            it -= 1;
        }
        if vline.intersections[it - 1].kind == SegmentIntersectionType::InnerHigh {
            it -= 1;
        } else {
            let down = vline.intersections[it].vertical_down();
            if down == -1 || vline.intersections[it].vertical_down_quality() != LinkQuality::Valid
            {
                break;
            }
            it = down as usize;
        }
    }
    it
}

/// `vertical_run_top(vline, start)`: from an inner intersection, the highest `INNER_HIGH` of its
/// run, going up through the runs that valid vertical links join.
#[must_use]
pub fn vertical_run_top(vline: &SegmentedIntersectionLine, start: usize) -> usize {
    let mut it = start;
    // Find the lowest SegmentIntersection::INNER_LOW starting with right.
    loop {
        while vline.intersections[it].kind != SegmentIntersectionType::InnerHigh {
            it += 1;
        }
        if vline.intersections[it + 1].kind == SegmentIntersectionType::InnerLow {
            it += 1;
        } else {
            let up = vline.intersections[it].vertical_up();
            if up == -1 || vline.intersections[it].vertical_up_quality() != LinkQuality::Valid {
                break;
            }
            it = up as usize;
        }
    }
    it
}

/// `overlap_bottom(start, end, vline_this, vline_other, side)`: walking up the run from `start`
/// to `end` on `vline_this`, the first horizontal link to `side`, as the bottom of its run on
/// `vline_other`; none when there is no such link.
#[must_use]
pub fn overlap_bottom(
    start: usize,
    end: usize,
    vline_this: &SegmentedIntersectionLine,
    vline_other: &SegmentedIntersectionLine,
    side: Side,
) -> Option<usize> {
    let mut other = None;
    let mut it = start;
    loop {
        let itsct = &vline_this.intersections[it];
        if itsct.is_inner() {
            let i = itsct.horizontal(side);
            if i != -1 {
                other = Some(i as usize);
                break;
            }
            if it == end {
                break;
            }
        }
        if itsct.kind != SegmentIntersectionType::InnerHigh
            || vline_this.intersections[it + 1].kind == SegmentIntersectionType::InnerLow
        {
            it += 1;
        } else {
            let up = itsct.vertical_up();
            if up == -1 || itsct.vertical_up_quality() != LinkQuality::Valid {
                break;
            }
            it = up as usize;
        }
    }
    other.map(|o| vertical_run_bottom(vline_other, o))
}

/// `overlap_top(start, end, vline_this, vline_other, side)`: walking down the run from `end` to
/// `start` on `vline_this`, the first horizontal link to `side`, as the top of its run on
/// `vline_other`; none when there is no such link.
#[must_use]
pub fn overlap_top(
    start: usize,
    end: usize,
    vline_this: &SegmentedIntersectionLine,
    vline_other: &SegmentedIntersectionLine,
    side: Side,
) -> Option<usize> {
    let mut other = None;
    let mut it = end;
    loop {
        let itsct = &vline_this.intersections[it];
        if itsct.is_inner() {
            let i = itsct.horizontal(side);
            if i != -1 {
                other = Some(i as usize);
                break;
            }
            if it == start {
                break;
            }
        }
        if itsct.kind != SegmentIntersectionType::InnerLow
            || vline_this.intersections[it - 1].kind == SegmentIntersectionType::InnerHigh
        {
            it -= 1;
        } else {
            let down = itsct.vertical_down();
            if down == -1 || itsct.vertical_down_quality() != LinkQuality::Valid {
                break;
            }
            it = down as usize;
        }
    }
    other.map(|o| vertical_run_top(vline_other, o))
}

/// A run of a line by its bottom and top intersections, either none when the C++'s pointers are
/// null.
pub type Run = (Option<usize>, Option<usize>);

/// `left_overlap(start, end, vline_this, vline_left)`: the runs on the line to the left that the
/// run from `start` to `end` overlaps, from the bottom of the lowest to the top of the highest.
#[must_use]
pub fn left_overlap(
    start: usize,
    end: usize,
    vline_this: &SegmentedIntersectionLine,
    vline_left: &SegmentedIntersectionLine,
) -> Run {
    let first = overlap_bottom(start, end, vline_this, vline_left, Side::Left);
    let second = if first.is_some() {
        overlap_top(start, end, vline_this, vline_left, Side::Left)
    } else {
        None
    };
    (first, second)
}

/// `right_overlap(start, end, vline_this, vline_right)`: as [`left_overlap`], to the right.
#[must_use]
pub fn right_overlap(
    start: usize,
    end: usize,
    vline_this: &SegmentedIntersectionLine,
    vline_right: &SegmentedIntersectionLine,
) -> Run {
    let first = overlap_bottom(start, end, vline_this, vline_right, Side::Right);
    let second = if first.is_some() {
        overlap_top(start, end, vline_this, vline_right, Side::Right)
    } else {
        None
    };
    (first, second)
}

/// The run's two ends, where the C++ dereferences both.
///
/// # Panics
/// With [`crate::stop::REGION_LINK_MISSING`] when only one is there.
fn both_ends(function: &'static str, start_end: Run) -> Option<(usize, usize)> {
    match start_end {
        (None, _) => None,
        (Some(s), Some(e)) => Some((s, e)),
        (Some(_), None) => stop(function, stop::REGION_LINK_MISSING),
    }
}

/// `left_overlap(start_end, vline_this, vline_left)`: none for none.
#[must_use]
pub fn left_overlap_run(
    start_end: Run,
    vline_this: &SegmentedIntersectionLine,
    vline_left: &SegmentedIntersectionLine,
) -> Run {
    match both_ends("left_overlap", start_end) {
        None => start_end,
        Some((s, e)) => left_overlap(s, e, vline_this, vline_left),
    }
}

/// `right_overlap(start_end, vline_this, vline_right)`: none for none.
#[must_use]
pub fn right_overlap_run(
    start_end: Run,
    vline_this: &SegmentedIntersectionLine,
    vline_right: &SegmentedIntersectionLine,
) -> Run {
    match both_ends("right_overlap", start_end) {
        None => start_end,
        Some((s, e)) => right_overlap(s, e, vline_this, vline_right),
    }
}

/// `generate_montonous_regions(segs)`: from each run not yet taken, line by line from the left, a
/// region grown to the right while its run overlaps exactly one run on the next line and that run
/// overlaps exactly it; each run taken is marked consumed. A region flips when it has an odd
/// number of lines.
#[must_use]
pub fn generate_montonous_regions(segs: &mut [SegmentedIntersectionLine]) -> Vec<MonotonicRegion> {
    let mut monotonic_regions = Vec::new();

    for i_vline_seed in 0..segs.len() as i32 {
        let seed = i_vline_seed as usize;
        let n_seed = segs[seed].intersections.len() as i32;
        let mut i_intersection_seed: i32 = 1;
        while i_intersection_seed + 1 < n_seed {
            while i_intersection_seed < n_seed
                && segs[seed].intersections[i_intersection_seed as usize].kind
                    != SegmentIntersectionType::InnerLow
            {
                i_intersection_seed += 1;
            }
            if i_intersection_seed == n_seed {
                break;
            }
            let start = i_intersection_seed as usize;
            let end = end_of_vertical_run(&segs[seed], start);
            if !segs[seed].intersections[start].consumed_vertical_up {
                // Draw a new monotonic region starting with this segment.
                // while there is only a single right neighbor
                let mut i_vline = i_vline_seed;
                let mut left: Run = (Some(start), Some(end));
                let mut region = MonotonicRegion::default();
                region.left.vline = i_vline;
                region.left.low = start as i32;
                region.left.high = end as i32;
                region.right = region.left;
                segs[seed].intersections[start].consumed_vertical_up = true;
                let mut num_lines = 1;
                loop {
                    i_vline += 1;
                    if i_vline >= segs.len() as i32 {
                        break;
                    }
                    let iv = i_vline as usize;
                    let vline_left = &segs[iv - 1];
                    let vline_right = &segs[iv];
                    let right = right_overlap_run(left, vline_left, vline_right);
                    let Some(right_first) = right.0 else {
                        // No neighbor at the right side of the current segment.
                        break;
                    };
                    let right_top_first = vertical_run_top(vline_right, right_first);
                    if Some(right_top_first) != right.1 {
                        // This segment overlaps with multiple segments at its right side.
                        break;
                    }
                    let right_left = left_overlap_run(right, vline_right, vline_left);
                    if left != right_left {
                        // Left & right draws don't overlap exclusively, right neighbor segment
                        // overlaps with multiple segments at its left.
                        break;
                    }
                    region.right.vline = i_vline;
                    region.right.low = right_first as i32;
                    region.right.high = right_top_first as i32;
                    segs[iv].intersections[right_first].consumed_vertical_up = true;
                    num_lines += 1;
                    left = right;
                }
                // Even number of lines makes the infill zig-zag to exit on the other side of the
                // region than where it starts.
                region.flips = (num_lines & 1) != 0;
                monotonic_regions.push(region);
            }
            i_intersection_seed = end as i32 + 1;
        }
    }

    monotonic_regions
}

/// `montonous_region_path_length(region, dir, poly_with_offset, segs)`: the length of the moves
/// drawing the region starting at the left low (or, `dir`, high) end needs, in `float`: half of
/// each perimeter piece that joins two lines, and each travel from the end of a line to the start
/// of the next where none does; unscaled.
#[must_use]
pub fn montonous_region_path_length(
    region: &MonotonicRegion,
    dir: bool,
    poly_with_offset: &ExPolygonWithOffset,
    segs: &[SegmentedIntersectionLine],
) -> f32 {
    const FUNCTION: &str = "montonous_region_path_length";
    // From the initial point (i_vline, i_intersection), follow a path.
    let mut i_intersection = region.left_intersection_point(dir);
    let mut i_vline = region.left.vline;
    let mut total_length: f32 = 0.;
    let mut no_perimeter = false;
    let mut last_point = Vec2f::new(0., 0.);

    loop {
        let vline = &segs[i_vline as usize];
        let its = &vline.intersections;
        let mut it = link(FUNCTION, i_intersection);
        let going_up = its[it].is_low();

        if no_perimeter {
            let beside = if going_up { it - 1 } else { it + 1 };
            total_length += (last_point - Vec2f::new(vline.pos as f32, its[beside].pos() as f32))
                .norm();
        }

        let mut iright = its[it].right_horizontal();
        if going_up {
            // Traverse the complete vertical segment up to the inner contour.
            loop {
                loop {
                    it += 1;
                    iright = orca_stl::max(iright, its[it].right_horizontal());
                    if its[it].kind == SegmentIntersectionType::InnerHigh
                        && its[it + 1].kind == SegmentIntersectionType::OuterHigh
                    {
                        break;
                    }
                }
                let inext = its[it].vertical_up();
                if inext == -1 || its[it].vertical_up_quality() != LinkQuality::Valid {
                    break;
                }
                it = inext as usize;
            }
        } else {
            // Going down.
            loop {
                loop {
                    it -= 1;
                    let iright_new = its[it].right_horizontal();
                    if iright_new != -1 {
                        iright = iright_new;
                    }
                    if its[it].kind == SegmentIntersectionType::InnerLow
                        && its[it - 1].kind == SegmentIntersectionType::OuterLow
                    {
                        break;
                    }
                }
                let inext = its[it].vertical_down();
                if inext == -1 || its[it].vertical_down_quality() != LinkQuality::Valid {
                    break;
                }
                it = inext as usize;
            }
        }

        if i_vline == region.right.vline {
            break;
        }

        let inext = its[it].right_horizontal();

        // Find the end of the next overlapping vertical segment.
        let vline_right = &segs[i_vline as usize + 1];
        let right = if going_up {
            vertical_run_top(vline_right, link(FUNCTION, iright))
        } else {
            vertical_run_bottom(vline_right, link(FUNCTION, iright))
        };
        i_intersection = right as i32;

        if inext == i_intersection && its[it].next_on_contour_quality == LinkQuality::Valid {
            // Summarize length of the connection line along the perimeter.
            // FIXME should it be weighted with a lower weight than non-extruding connection line?
            // What weight? Taking half of the length.
            total_length += 0.5_f32
                * measure_perimeter_horizontal_segment_length(
                    poly_with_offset,
                    segs,
                    i_vline as usize,
                    it,
                    inext as usize,
                ) as f32;
            // Don't add distance to the next vertical line start to the total length.
            no_perimeter = false;
        } else {
            // Finish the current vertical line,
            it = if going_up { it + 1 } else { it - 1 };
            // Mark the end of this vertical line.
            last_point = Vec2f::new(vline.pos as f32, its[it].pos() as f32);
            // Remember to add distance to the last point.
            no_perimeter = true;
        }

        i_vline += 1;
    }

    total_length * SCALING_FACTOR as f32
}

/// The key of a map entry: the intersection's line and index, standing for its address.
type Key = (usize, usize);

/// The region of the map entry whose key is `key`.
///
/// # Panics
/// With [`crate::stop::REGION_LINK_MISSING`] when no entry has it.
fn find_region(map: &[(Key, usize)], key: Key) -> usize {
    let it = orca_stl::lower_bound_by(map, &key, |entry: &(Key, usize), k: &Key| entry.0 < *k);
    if it == map.len() || map[it].0 != key {
        stop("connect_monotonic_regions", stop::REGION_LINK_MISSING);
    }
    map[it].1
}

/// `connect_monotonic_regions(regions, poly_with_offset, segs)`: each region's neighbours on the
/// left and on the right (the regions whose runs its outer runs overlap), made symmetric and
/// sorted; then each region's two lengths ([`montonous_region_path_length`]), the smaller
/// subtracted from both.
///
/// # Panics
/// With [`crate::stop::REGION_LINK_MISSING`] when an overlapped run is no region's end.
pub fn connect_monotonic_regions(
    regions: &mut [MonotonicRegion],
    poly_with_offset: &ExPolygonWithOffset,
    segs: &[SegmentedIntersectionLine],
) {
    const FUNCTION: &str = "connect_monotonic_regions";
    // Map from low intersection to left / right side of a monotonic region.
    let mut map_intersection_to_region_start: Vec<(Key, usize)> = Vec::new();
    let mut map_intersection_to_region_end: Vec<(Key, usize)> = Vec::new();
    for (i, region) in regions.iter().enumerate() {
        map_intersection_to_region_start.push((
            (region.left.vline as usize, region.left.low as usize),
            i,
        ));
        map_intersection_to_region_end.push((
            (region.right.vline as usize, region.right.low as usize),
            i,
        ));
    }
    let intersections_lower = |l: &(Key, usize), r: &(Key, usize)| l.0 < r.0;
    orca_stl::sort_by(&mut map_intersection_to_region_start, intersections_lower);
    orca_stl::sort_by(&mut map_intersection_to_region_end, intersections_lower);

    // Scatter links to neighboring regions.
    for i_region in 0..regions.len() {
        let left = regions[i_region].left;
        let right = regions[i_region].right;
        if left.vline > 0 {
            let vline = &segs[left.vline as usize];
            let vline_left = &segs[left.vline as usize - 1];
            let (lbegin, lend) =
                left_overlap(left.low as usize, left.high as usize, vline, vline_left);
            if let Some(mut lbegin) = lbegin {
                loop {
                    let found =
                        find_region(&map_intersection_to_region_end, (left.vline as usize - 1, lbegin));
                    regions[found].right_neighbors.push(i_region);
                    let mut lnext = vertical_run_top(vline_left, lbegin);
                    if Some(lnext) == lend {
                        break;
                    }
                    if lend.is_none() {
                        stop(FUNCTION, stop::REGION_LINK_MISSING);
                    }
                    while vline_left.intersections[lnext].kind != SegmentIntersectionType::InnerLow
                    {
                        lnext += 1;
                    }
                    lbegin = lnext;
                }
            }
        }
        if right.vline + 1 < segs.len() as i32 {
            let vline = &segs[right.vline as usize];
            let vline_right = &segs[right.vline as usize + 1];
            let (rbegin, rend) =
                right_overlap(right.low as usize, right.high as usize, vline, vline_right);
            if let Some(mut rbegin) = rbegin {
                loop {
                    let found = find_region(
                        &map_intersection_to_region_start,
                        (right.vline as usize + 1, rbegin),
                    );
                    regions[found].left_neighbors.push(i_region);
                    let mut rnext = vertical_run_top(vline_right, rbegin);
                    if Some(rnext) == rend {
                        break;
                    }
                    if rend.is_none() {
                        stop(FUNCTION, stop::REGION_LINK_MISSING);
                    }
                    while vline_right.intersections[rnext].kind
                        != SegmentIntersectionType::InnerLow
                    {
                        rnext += 1;
                    }
                    rbegin = rnext;
                }
            }
        }
    }

    // Sometimes a segment may indicate that it connects to a segment on the other side while the
    // other does not. This may be a valid case if one side contains runs of OUTER_LOW, INNER_LOW,
    // {INNER_HIGH, INNER_LOW}*, INNER_HIGH, OUTER_HIGH, where the part in the middle does not
    // connect to the other side, but it will be extruded through.
    for region in regions.iter_mut() {
        orca_stl::sort(&mut region.left_neighbors);
        orca_stl::sort(&mut region.right_neighbors);
    }
    for i_region in 0..regions.len() {
        let left_neighbors = regions[i_region].left_neighbors.clone();
        for neighbor in left_neighbors {
            let list = &mut regions[neighbor].right_neighbors;
            let it = orca_stl::lower_bound(list, &i_region);
            if it == list.len() || list[it] != i_region {
                list.insert(it, i_region);
            }
        }
        let right_neighbors = regions[i_region].right_neighbors.clone();
        for neighbor in right_neighbors {
            let list = &mut regions[neighbor].left_neighbors;
            let it = orca_stl::lower_bound(list, &i_region);
            if it == list.len() || list[it] != i_region {
                list.insert(it, i_region);
            }
        }
    }

    // Fill in sum length of connecting lines of a region. This length is used for optimizing the
    // infill path for minimum length.
    for region in regions.iter_mut() {
        region.len1 = montonous_region_path_length(region, false, poly_with_offset, segs);
        region.len2 = montonous_region_path_length(region, true, poly_with_offset, segs);
        // Subtract the smaller length from the longer one, so we will optimize just with the
        // positive difference of the two.
        if region.len1 > region.len2 {
            region.len1 -= region.len2;
            region.len2 = 0.;
        } else {
            region.len2 -= region.len1;
            region.len1 = 0.;
        }
    }
}

/// `monotonic_3_opt(path, segs)`: the path optimized keeping the regions' order constraints. Its
/// body is empty in the C++ (only a comment on how it might be done).
pub fn monotonic_3_opt(_path: &mut [MonotonicRegionLink], _segs: &[SegmentedIntersectionLine]) {}

/// `struct NextCandidate` of `chain_monotonic_regions`.
#[derive(Clone, Copy, Debug)]
struct NextCandidate {
    region: Option<usize>,
    link: usize,
    link_flipped: usize,
    probability: f32,
    dir: bool,
}

impl NextCandidate {
    /// `NextCandidate next_candidate; next_candidate.probability = 0;`: no region; the C++ leaves
    /// the links uninitialised and never reads them before it sets them.
    fn none() -> Self {
        Self {
            region: None,
            link: 0,
            link_flipped: 0,
            probability: 0.,
            dir: false,
        }
    }
}

/// How many times to repeat the ant simulation (number of ant generations).
const NUM_ROUNDS: i32 = 25;
/// After how many rounds without an improvement to exit?
const NUM_ROUNDS_NO_CHANGE_EXIT: i32 = 8;
/// Evaporation rate of pheromones.
const PHEROMONE_EVAPORATION: f32 = 0.1;
/// Evaporation rate to diversify paths taken by individual ants.
const PHEROMONE_DIVERSIFICATION: f32 = 0.1;
/// Probability at which to take the next best path. Otherwise take the the path based on the cost
/// distribution.
const PROBABILITY_TAKE_BEST: f32 = 0.9;
/// Exponents of the cost function: pheromone exponent.
const PHEROMONE_ALPHA: f32 = 1.;
/// Attractiveness weighted towards edge length.
const PHEROMONE_BETA: f32 = 2.;

/// The region a candidate or path names.
///
/// # Panics
/// With [`crate::stop::REGION_LINK_MISSING`] when it is none (the C++'s null region).
fn region_of(region: Option<usize>) -> usize {
    match region {
        Some(r) => r,
        None => stop("chain_monotonic_regions", stop::REGION_LINK_MISSING),
    }
}

/// `std::find` of `region` in `queue`, the last element moved into its place and popped.
///
/// # Panics
/// With [`crate::stop::REGION_LINK_MISSING`] when it is not there (the C++ writes past the end).
fn remove_from_queue(queue: &mut Vec<usize>, region: Option<usize>) {
    match queue.iter().position(|&q| Some(q) == region) {
        Some(i) => {
            let last = queue[queue.len() - 1];
            queue[i] = last;
            queue.pop();
        }
        None => stop("chain_monotonic_regions", stop::REGION_LINK_MISSING),
    }
}

/// `chain_monotonic_regions(regions, poly_with_offset, segs, rng)`: an order of the regions that
/// prints every region after its left neighbours, with each region's direction, short in travel:
/// a greedy path sets the pheromones' base level, then up to 25 rounds of up to 10 ants each walk
/// the regions following the pheromones, the shortest path kept; a round without a better path
/// counts towards the exit after 8, and a path of length 0 ends the search.
///
/// # Panics
/// With [`crate::stop::REGION_LINK_MISSING`] when a step finds no region to go to, where the C++
/// dereferences a null region or an empty list's end.
#[allow(clippy::too_many_lines)] // One C++ function, kept whole.
pub fn chain_monotonic_regions(
    regions: &[MonotonicRegion],
    poly_with_offset: &ExPolygonWithOffset,
    segs: &[SegmentedIntersectionLine],
    rng: &mut Mt19937_64,
) -> Vec<MonotonicRegionLink> {
    // Number of left neighbors (regions that this region depends on, this region cannot be printed
    // before the regions left of it are printed) + self.
    let mut left_neighbors_unprocessed: Vec<i32> = vec![1; regions.len()];
    // Queue of regions, which have their left neighbors already printed.
    let mut queue: Vec<usize> = Vec::new();
    for (i, region) in regions.iter().enumerate() {
        if region.left_neighbors.is_empty() {
            queue.push(i);
        } else {
            left_neighbors_unprocessed[i] += region.left_neighbors.len() as i32;
        }
    }
    // Make copy of structures that need to be initialized at each ant iteration.
    let left_neighbors_unprocessed_initial = left_neighbors_unprocessed.clone();
    let queue_initial = queue.clone();

    let mut path: Vec<MonotonicRegionLink> = Vec::new();
    let mut best_path: Vec<MonotonicRegionLink> = Vec::new();
    let mut best_path_length = f32::MAX;

    let mut next_candidates: Vec<NextCandidate> = Vec::new();

    let num_ants = orca_stl::min(regions.len() as i32, 10);
    // Base (initial) pheromone level. This value will be adjusted based on the length of the first
    // greedy path found.
    let mut pheromone_initial_deposit: f32 = 0.5;

    let mut path_matrix =
        AntPathMatrix::new(regions, poly_with_offset, segs, pheromone_initial_deposit);

    // Find an initial path in a greedy way, set the initial pheromone value to 10% of the cost of
    // the greedy path.
    {
        // Construct the first path in a greedy way to calculate an initial value of the pheromone
        // value.
        queue.clone_from(&queue_initial);
        left_neighbors_unprocessed.clone_from(&left_neighbors_unprocessed_initial);
        // Pick the last of the queue.
        let mut path_end = MonotonicRegionLink::new(region_of(queue.last().copied()), false);
        queue.pop();
        left_neighbors_unprocessed[path_end.region] -= 1;

        let mut total_length = regions[path_end.region].length(false);
        while !queue.is_empty() || !regions[path_end.region].right_neighbors.is_empty() {
            // Chain.
            let region = path_end.region;
            let dir = path_end.flipped;
            let mut next_candidate = NextCandidate::none();
            for &next in &regions[region].right_neighbors {
                if left_neighbors_unprocessed[next] == 2 {
                    // Dependencies of the successive blocks are satisfied.
                    let path1 = path_matrix.at(region, dir, next, false);
                    let path2 = path_matrix.at(region, dir, next, true);
                    let v1 = path_matrix.matrix[path1].visibility;
                    if v1 > next_candidate.probability {
                        next_candidate = NextCandidate {
                            region: Some(next),
                            link: path1,
                            link_flipped: path1,
                            probability: v1,
                            dir: false,
                        };
                    }
                    let v2 = path_matrix.matrix[path2].visibility;
                    if v2 > next_candidate.probability {
                        next_candidate = NextCandidate {
                            region: Some(next),
                            link: path2,
                            link_flipped: path2,
                            probability: v2,
                            dir: true,
                        };
                    }
                }
            }
            let from_queue = next_candidate.probability == 0.;
            if from_queue {
                for &next in &queue {
                    let path1 = path_matrix.at(region, dir, next, false);
                    let path2 = path_matrix.at(region, dir, next, true);
                    let v1 = path_matrix.matrix[path1].visibility;
                    if v1 > next_candidate.probability {
                        next_candidate = NextCandidate {
                            region: Some(next),
                            link: path1,
                            link_flipped: path1,
                            probability: v1,
                            dir: false,
                        };
                    }
                    let v2 = path_matrix.matrix[path2].visibility;
                    if v2 > next_candidate.probability {
                        next_candidate = NextCandidate {
                            region: Some(next),
                            link: path2,
                            link_flipped: path2,
                            probability: v2,
                            dir: true,
                        };
                    }
                }
            }
            // Move the other right neighbors with satisified constraints to the queue.
            for &next in &regions[region].right_neighbors {
                left_neighbors_unprocessed[next] -= 1;
                if left_neighbors_unprocessed[next] == 1 && next_candidate.region != Some(next) {
                    queue.push(next);
                }
            }
            if from_queue {
                // Remove the selected path from the queue.
                remove_from_queue(&mut queue, next_candidate.region);
            }
            // Extend the path.
            let next_region = region_of(next_candidate.region);
            let next_dir = next_candidate.dir;
            let link_length = path_matrix.matrix
                [path_matrix.at(path_end.region, path_end.flipped, next_region, next_dir)]
            .length;
            total_length += regions[next_region].length(next_dir) + link_length;
            path_end = MonotonicRegionLink::new(next_region, next_dir);
            left_neighbors_unprocessed[next_region] = 0;
        }

        // Set an initial pheromone value to 10% of the greedy path's value.
        pheromone_initial_deposit = 0.1 / total_length;
        path_matrix.update_inital_pheromone(pheromone_initial_deposit);
    }

    // Probability (unnormalized) of traversing a link between two monotonic regions.
    let path_probability = |path: &AntPath| {
        orca_crt::powf(path.pheromone, PHEROMONE_ALPHA)
            * orca_crt::powf(path.visibility, PHEROMONE_BETA)
    };

    let mut num_rounds_no_change = 0;
    let mut round = 0;
    while round < NUM_ROUNDS && num_rounds_no_change < NUM_ROUNDS_NO_CHANGE_EXIT {
        let mut improved = false;
        for _ant in 0..num_ants {
            // Find a new path following the pheromones deposited by the previous ants.
            path.clear();
            queue.clone_from(&queue_initial);
            left_neighbors_unprocessed.clone_from(&left_neighbors_unprocessed_initial);
            // Pick randomly the first from the queue at random orientation.
            // FIXME picking the 1st monotonic region should likely be done based on accumulated
            // pheromone level as well, but the inefficiency caused by the random pick of the 1st
            // monotonic region is likely insignificant.
            let first_idx = UniformIntDistribution::<i32>::new(0, queue.len() as i32 - 1).sample(rng);
            let first = region_of(queue.get(first_idx as usize).copied());
            let flipped = rng.next() > u64::MAX / 2;
            path.push(MonotonicRegionLink::new(first, flipped));
            let last = queue[queue.len() - 1];
            queue[first_idx as usize] = last;
            queue.pop();
            left_neighbors_unprocessed[first] -= 1;

            while !queue.is_empty()
                || !regions[path[path.len() - 1].region].right_neighbors.is_empty()
            {
                // Chain.
                let back = path[path.len() - 1];
                let region = back.region;
                let dir = back.flipped;
                // Sort by distance to pt.
                next_candidates.clear();
                for &next in &regions[region].right_neighbors {
                    let unprocessed = &mut left_neighbors_unprocessed[next];
                    *unprocessed -= 1;
                    if *unprocessed == 1 {
                        // Dependencies of the successive blocks are satisfied.
                        let path1 = path_matrix.at(region, dir, next, false);
                        let path1_flipped = path_matrix.at(region, !dir, next, true);
                        let path2 = path_matrix.at(region, dir, next, true);
                        let path2_flipped = path_matrix.at(region, !dir, next, false);
                        next_candidates.push(NextCandidate {
                            region: Some(next),
                            link: path1,
                            link_flipped: path1_flipped,
                            probability: path_probability(&path_matrix.matrix[path1]),
                            dir: false,
                        });
                        next_candidates.push(NextCandidate {
                            region: Some(next),
                            link: path2,
                            link_flipped: path2_flipped,
                            probability: path_probability(&path_matrix.matrix[path2]),
                            dir: true,
                        });
                    }
                }
                let num_direct_neighbors = next_candidates.len();
                // FIXME add the queue items to the candidates? These are valid moves as well.
                if num_direct_neighbors == 0 {
                    // Add the queue candidates.
                    for &next in &queue {
                        let path1 = path_matrix.at(region, dir, next, false);
                        let path1_flipped = path_matrix.at(region, !dir, next, true);
                        let path2 = path_matrix.at(region, dir, next, true);
                        let path2_flipped = path_matrix.at(region, !dir, next, false);
                        next_candidates.push(NextCandidate {
                            region: Some(next),
                            link: path1,
                            link_flipped: path1_flipped,
                            probability: path_probability(&path_matrix.matrix[path1]),
                            dir: false,
                        });
                        next_candidates.push(NextCandidate {
                            region: Some(next),
                            link: path2,
                            link_flipped: path2_flipped,
                            probability: path_probability(&path_matrix.matrix[path2]),
                            dir: true,
                        });
                    }
                }
                if next_candidates.is_empty() {
                    // The C++ dereferences the end of the empty candidate list.
                    stop("chain_monotonic_regions", stop::REGION_LINK_MISSING);
                }
                let dice = rng.next() as f32 / u64::MAX as f32;
                let take_path = if dice < PROBABILITY_TAKE_BEST {
                    // Take the highest probability path.
                    orca_stl::max_element_by(&next_candidates, |l, r| l.probability < r.probability)
                } else {
                    // Take the path based on the probability.
                    // Calculate the total probability.
                    let total_probability = next_candidates
                        .iter()
                        .fold(0_f32, |l, r| l + r.probability);
                    // Take a random path based on the probability.
                    let mut probability_threshold =
                        rng.next() as f32 * total_probability / u64::MAX as f32;
                    let mut take_path = next_candidates.len() - 1;
                    for (i, candidate) in next_candidates.iter().enumerate() {
                        probability_threshold -= candidate.probability;
                        if probability_threshold <= 0. {
                            take_path = i;
                            break;
                        }
                    }
                    take_path
                };
                let taken = next_candidates[take_path];
                // Move the other right neighbors with satisified constraints to the queue.
                for candidate in &next_candidates[..num_direct_neighbors] {
                    if (queue.is_empty() || candidate.region != Some(queue[queue.len() - 1]))
                        && candidate.region != taken.region
                    {
                        queue.push(region_of(candidate.region));
                    }
                }
                if take_path >= num_direct_neighbors {
                    // Remove the selected path from the queue.
                    remove_from_queue(&mut queue, taken.region);
                }
                // Extend the path.
                let next_region = region_of(taken.region);
                let next_dir = taken.dir;
                let n = path.len();
                path[n - 1].next = Some(taken.link);
                path[n - 1].next_flipped = Some(taken.link_flipped);
                path.push(MonotonicRegionLink::new(next_region, next_dir));
                left_neighbors_unprocessed[next_region] = 0;

                // Update pheromones along this link, see Ant Colony System (ACS) update rule.
                // http://www.scholarpedia.org/article/Ant_colony_optimization
                // The goal here is to lower the pheromone trace for paths taken to diversify the
                // next path picked in the same batch of ants.
                let ap = &mut path_matrix.matrix[taken.link];
                ap.pheromone = (1. - PHEROMONE_DIVERSIFICATION) * ap.pheromone
                    + PHEROMONE_DIVERSIFICATION * pheromone_initial_deposit;
            }

            // Perform 3-opt local optimization of the path.
            monotonic_3_opt(&mut path, segs);

            // Measure path length.
            let back = path[path.len() - 1];
            let mut path_length = regions[back.region].length(back.flipped);
            for i in 0..path.len() - 1 {
                let r = path[i];
                let next = path[i + 1];
                let link_length = path_matrix.matrix
                    [path_matrix.at(r.region, r.flipped, next.region, next.flipped)]
                .length;
                path_length = path_length + regions[r.region].length(r.flipped) + link_length;
            }
            // Save the shortest path.
            if path_length < best_path_length {
                best_path_length = path_length;
                core::mem::swap(&mut best_path, &mut path);
                if path_length == 0. {
                    // Perfect path found.
                    return best_path;
                }
                improved = true;
            }
        }

        // Reinforce the path pheromones with the best path.
        let total_cost = best_path_length + EPSILON as f32;
        let mut i = 0;
        while i + 1 < path.len() {
            let Some(next) = path[i].next else {
                stop("chain_monotonic_regions", stop::REGION_LINK_MISSING)
            };
            let ap = &mut path_matrix.matrix[next];
            ap.pheromone =
                (1. - PHEROMONE_EVAPORATION) * ap.pheromone + PHEROMONE_EVAPORATION / total_cost;
            i += 1;
        }

        if improved {
            num_rounds_no_change = 0;
        } else {
            num_rounds_no_change += 1;
        }
        round += 1;
    }

    best_path
}

/// `finish_polyline` of `polylines_from_paths`: the last line's repeated points dropped, the line
/// dropped when it is too short ([`nearly_empty`]), or else joined to the line before when it
/// starts within `SCALED_EPSILON` of that line's end (on both axes): the two points become their
/// midpoint (integer halves, truncated).
fn finish_polyline(polylines_out: &mut Polylines) {
    let n = polylines_out.len();
    remove_duplicate_points(&mut polylines_out[n - 1].points);
    // Handle duplicate points and zero length segments.
    if nearly_empty(&polylines_out[n - 1].points) {
        // Handle nearly zero length edges.
        polylines_out.pop();
    } else if n >= 2 {
        // Merge the two last polylines. An extrusion may have been split by an introduction of
        // phony outer points on intersection lines to cope with pinching of inner offset contours.
        let front = polylines_out[n - 1].points[0];
        let prev_back = polylines_out[n - 2].last_point();
        if (front.x.wrapping_sub(prev_back.x).wrapping_abs() as f64) < SCALED_EPSILON
            && (front.y.wrapping_sub(prev_back.y).wrapping_abs() as f64) < SCALED_EPSILON
        {
            let polyline = polylines_out.pop().unwrap_or_default();
            let pl_prev = &mut polylines_out[n - 2];
            let last = pl_prev.points.len() - 1;
            pl_prev.points[last] = (prev_back + front) / 2;
            pl_prev.points.extend_from_slice(&polyline.points[1..]);
        }
    }
}

/// `polylines_from_paths(path, poly_with_offset, segs, polylines_out)`: the regions drawn in the
/// path's order, each from its left line to its right, up and down each run and along the inner
/// contour between runs (valid vertical links) and between lines (valid horizontal links); a line
/// that cannot go on is taken out to the outer contour and finished, and the next starts at the
/// outer contour. The lines are appended to `polylines_out`; a finished line may join the line
/// before it, one appended before this call included, as the C++'s does.
pub fn polylines_from_paths(
    path: &[MonotonicRegionLink],
    regions: &[MonotonicRegion],
    poly_with_offset: &ExPolygonWithOffset,
    segs: &[SegmentedIntersectionLine],
    polylines_out: &mut Polylines,
) {
    const FUNCTION: &str = "polylines_from_paths";
    // Whether the C++'s `polyline` points at the last of `polylines_out`.
    let mut polyline = false;

    for (i_segment, path_segment) in path.iter().enumerate() {
        let region = &regions[path_segment.region];
        let dir = path_segment.flipped;

        // From the initial point (i_vline, i_intersection), follow a path.
        let mut i_intersection = region.left_intersection_point(dir);
        let mut i_vline = region.left.vline;

        if polyline && i_segment != 0 {
            // Connect previous path segment with the new one.
            let path_segment_prev = &path[i_segment - 1];
            let region_prev = &regions[path_segment_prev.region];
            let dir_prev = path_segment_prev.flipped;
            let i_vline_prev = region_prev.right.vline;
            let vline_prev = &segs[i_vline_prev as usize];
            let i_intersection_prev =
                link(FUNCTION, region_prev.right_intersection_point(dir_prev));
            let ip_prev = &vline_prev.intersections[i_intersection_prev];
            let mut extended = false;
            if i_vline_prev + 1 == i_vline
                && ip_prev.right_horizontal() == i_intersection
                && ip_prev.next_on_contour_quality == LinkQuality::Valid
            {
                // Emit a horizontal connection contour.
                let n = polylines_out.len();
                emit_perimeter_prev_next_segment(
                    poly_with_offset,
                    segs,
                    i_vline_prev as usize,
                    ip_prev.i_contour,
                    i_intersection_prev,
                    link(FUNCTION, i_intersection),
                    &mut polylines_out[n - 1],
                    true,
                );
                extended = true;
            }
            if !extended {
                // Finish the current vertical line,
                let outer = if ip_prev.is_low() {
                    i_intersection_prev - 1
                } else {
                    i_intersection_prev + 1
                };
                let n = polylines_out.len();
                let pl = &mut polylines_out[n - 1];
                let last = pl.points.len() - 1;
                pl.points[last] = Point::new(vline_prev.pos, vline_prev.intersections[outer].pos());
                finish_polyline(polylines_out);
                polyline = false;
            }
        }

        loop {
            let vline = &segs[i_vline as usize];
            let its = &vline.intersections;
            let mut it = link(FUNCTION, i_intersection);
            let going_up = its[it].is_low();
            if polyline {
                let n = polylines_out.len();
                polylines_out[n - 1]
                    .points
                    .push(Point::new(vline.pos, its[it].pos()));
            } else {
                polylines_out.push(Polyline::default());
                polyline = true;
                // Extend the infill line up to the outer contour.
                let beside = if going_up { it - 1 } else { it + 1 };
                let n = polylines_out.len();
                polylines_out[n - 1]
                    .points
                    .push(Point::new(vline.pos, its[beside].pos()));
            }

            let mut iright = its[it].right_horizontal();
            if going_up {
                // Consume the complete vertical segment up to the inner contour.
                loop {
                    loop {
                        it += 1;
                        iright = orca_stl::max(iright, its[it].right_horizontal());
                        if its[it].kind == SegmentIntersectionType::InnerHigh
                            && its[it + 1].kind == SegmentIntersectionType::OuterHigh
                        {
                            break;
                        }
                    }
                    let n = polylines_out.len();
                    polylines_out[n - 1]
                        .points
                        .push(Point::new(vline.pos, its[it].pos()));
                    let inext = its[it].vertical_up();
                    if inext == -1 || its[it].vertical_up_quality() != LinkQuality::Valid {
                        break;
                    }
                    emit_perimeter_segment_on_vertical_line(
                        poly_with_offset,
                        segs,
                        i_vline as usize,
                        its[it].i_contour,
                        it,
                        inext as usize,
                        &mut polylines_out[n - 1],
                        its[it].has_left_vertical_up(),
                    );
                    it = inext as usize;
                }
            } else {
                // Going down.
                loop {
                    loop {
                        it -= 1;
                        let iright_new = its[it].right_horizontal();
                        if iright_new != -1 {
                            iright = iright_new;
                        }
                        if its[it].kind == SegmentIntersectionType::InnerLow
                            && its[it - 1].kind == SegmentIntersectionType::OuterLow
                        {
                            break;
                        }
                    }
                    let n = polylines_out.len();
                    polylines_out[n - 1]
                        .points
                        .push(Point::new(vline.pos, its[it].pos()));
                    let inext = its[it].vertical_down();
                    if inext == -1 || its[it].vertical_down_quality() != LinkQuality::Valid {
                        break;
                    }
                    emit_perimeter_segment_on_vertical_line(
                        poly_with_offset,
                        segs,
                        i_vline as usize,
                        its[it].i_contour,
                        it,
                        inext as usize,
                        &mut polylines_out[n - 1],
                        its[it].has_right_vertical_down(),
                    );
                    it = inext as usize;
                }
            }

            if i_vline == region.right.vline {
                break;
            }

            let inext = its[it].right_horizontal();

            // Find the end of the next overlapping vertical segment.
            let vline_right = &segs[i_vline as usize + 1];
            let right = if going_up {
                vertical_run_top(vline_right, link(FUNCTION, iright))
            } else {
                vertical_run_bottom(vline_right, link(FUNCTION, iright))
            };
            i_intersection = right as i32;

            if inext == i_intersection && its[it].next_on_contour_quality == LinkQuality::Valid {
                // Emit a horizontal connection contour.
                let n = polylines_out.len();
                emit_perimeter_prev_next_segment(
                    poly_with_offset,
                    segs,
                    i_vline as usize,
                    its[it].i_contour,
                    it,
                    inext as usize,
                    &mut polylines_out[n - 1],
                    true,
                );
            } else {
                // Finish the current vertical line,
                it = if going_up { it + 1 } else { it - 1 };
                let n = polylines_out.len();
                let pl = &mut polylines_out[n - 1];
                let last = pl.points.len() - 1;
                pl.points[last] = Point::new(vline.pos, its[it].pos());
                finish_polyline(polylines_out);
                polyline = false;
            }

            i_vline += 1;
        }
    }

    if polyline {
        // Finish the current vertical line,
        let back = &path[path.len() - 1];
        let region = &regions[back.region];
        let vline = &segs[region.right.vline as usize];
        let ip = link(FUNCTION, region.right_intersection_point(back.flipped));
        let outer = if vline.intersections[ip].is_low() {
            ip - 1
        } else {
            ip + 1
        };
        let n = polylines_out.len();
        let pl = &mut polylines_out[n - 1];
        let last = pl.points.len() - 1;
        pl.points[last] = Point::new(vline.pos, vline.intersections[outer].pos());
        finish_polyline(polylines_out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use orca_geom::expolygon::ExPolygon;
    use orca_geom::polygon::Polygon;

    use crate::base::FillParams;
    use crate::rectilinear::{
        connect_segment_intersections_by_contours, pinch_contours_insert_phony_outer_intersections,
        slice_region_by_vertical_lines,
    };

    fn polygon(points: &[(i64, i64)]) -> Polygon {
        Polygon::new(points.iter().map(|&(x, y)| Point::new(x, y)).collect())
    }

    /// A square with a square hole in its middle: the lines through the hole split in two.
    fn square_with_hole() -> ExPolygon {
        let mut ex = ExPolygon::new(polygon(&[(0, 0), (3000, 0), (3000, 3000), (0, 3000)]));
        ex.holes
            .push(polygon(&[(1000, 1000), (1000, 2000), (2000, 2000), (2000, 1000)]));
        ex
    }

    fn sliced(ex: &ExPolygon, n: usize, x0: i64, spacing: i64) -> (ExPolygonWithOffset, Vec<SegmentedIntersectionLine>) {
        let pwo = ExPolygonWithOffset::new(ex, 0., -50, -100);
        let mut segs = slice_region_by_vertical_lines(&pwo, n, x0, spacing).unwrap();
        connect_segment_intersections_by_contours(&pwo, &mut segs, &FillParams::default(), 0);
        pinch_contours_insert_phony_outer_intersections(&mut segs);
        (pwo, segs)
    }

    #[test]
    fn a_square_is_one_region_of_every_line() {
        let ex = ExPolygon::new(polygon(&[(0, 0), (1000, 0), (1000, 1000), (0, 1000)]));
        let (pwo, mut segs) = sliced(&ex, 5, 300, 100);
        let mut regions = generate_montonous_regions(&mut segs);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].left.vline, 0);
        assert_eq!(regions[0].right.vline, 4);
        // Five lines: an odd number, it flips.
        assert!(regions[0].flips);
        connect_monotonic_regions(&mut regions, &pwo, &segs);
        assert!(regions[0].left_neighbors.is_empty() && regions[0].right_neighbors.is_empty());
        // One of the two lengths is 0 after the subtraction.
        assert!(regions[0].len1 == 0. || regions[0].len2 == 0.);
        let mut rng = Mt19937_64::default();
        let path = chain_monotonic_regions(&regions, &pwo, &segs, &mut rng);
        assert_eq!(path.len(), 1);
        let mut out = Polylines::new();
        polylines_from_paths(&path, &regions, &pwo, &segs, &mut out);
        assert_eq!(out.len(), 1);
        // A zig-zag over the five lines, from the outer contour to the outer contour.
        let xs: Vec<i64> = out[0].points.iter().map(|p| p.x).collect();
        for x in [300, 400, 500, 600, 700] {
            assert!(xs.contains(&x), "x = {x} in {xs:?}");
        }
        assert_eq!(out[0].points[0].x, 300);
        assert_eq!(out[0].points[out[0].points.len() - 1].x, 700);
    }

    #[test]
    fn a_hole_splits_the_lines_into_linked_regions() {
        let (pwo, mut segs) = sliced(&square_with_hole(), 10, 250, 300);
        let mut regions = generate_montonous_regions(&mut segs);
        // Left of the hole, below it, above it and right of it.
        assert_eq!(regions.len(), 4);
        connect_monotonic_regions(&mut regions, &pwo, &segs);
        // The neighbours are symmetric.
        for (i, r) in regions.iter().enumerate() {
            for &n in &r.right_neighbors {
                assert!(regions[n].left_neighbors.contains(&i));
            }
            for &n in &r.left_neighbors {
                assert!(regions[n].right_neighbors.contains(&i));
            }
        }
        let mut rng = Mt19937_64::default();
        let path = chain_monotonic_regions(&regions, &pwo, &segs, &mut rng);
        // Every region once, each after its left neighbours.
        assert_eq!(path.len(), 4);
        for (k, l) in path.iter().enumerate() {
            for &left in &regions[l.region].left_neighbors {
                assert!(path[..k].iter().any(|p| p.region == left));
            }
        }
        let mut out = Polylines::new();
        polylines_from_paths(&path, &regions, &pwo, &segs, &mut out);
        assert!(!out.is_empty());
        for pl in &out {
            assert!(pl.points.len() >= 2);
        }
    }

    #[test]
    fn the_runs_go_through_valid_vertical_links() {
        let (_, segs) = sliced(&square_with_hole(), 10, 250, 300);
        for line in &segs {
            for (i, itsct) in line.intersections.iter().enumerate() {
                if itsct.kind == SegmentIntersectionType::InnerLow {
                    let top = vertical_run_top(line, i);
                    assert_eq!(line.intersections[top].kind, SegmentIntersectionType::InnerHigh);
                    assert_eq!(vertical_run_bottom(line, top), vertical_run_bottom(line, i));
                }
            }
        }
    }

    #[test]
    fn the_matrix_measures_the_straight_distance() {
        let regions = vec![
            MonotonicRegion {
                left: Boundary { vline: 0, low: 1, high: 2 },
                right: Boundary { vline: 0, low: 1, high: 2 },
                ..MonotonicRegion::default()
            },
            MonotonicRegion {
                left: Boundary { vline: 1, low: 1, high: 2 },
                right: Boundary { vline: 1, low: 1, high: 2 },
                ..MonotonicRegion::default()
            },
        ];
        let ex = ExPolygon::new(polygon(&[(0, 0), (4_000_000, 0), (4_000_000, 4_000_000), (0, 4_000_000)]));
        let (pwo, segs) = sliced(&ex, 2, 1_000_000, 3_000_000);
        let mut m = AntPathMatrix::new(&regions, &pwo, &segs, 0.5);
        let idx = m.at(0, false, 1, false);
        assert_eq!(idx, 2);
        let ap = m.matrix[idx];
        // From (1 mm, the top of line 0) to (4 mm, the bottom of line 1).
        let dy = (segs[1].intersections[1].pos() - segs[0].intersections[2].pos()) as f32;
        let expect = Vec2f::new(3_000_000., dy).norm() * SCALING_FACTOR as f32;
        assert_eq!(ap.length, expect);
        assert_eq!(ap.visibility, 1. / (expect + EPSILON as f32));
        assert_eq!(ap.pheromone, 0.5);
    }
}
