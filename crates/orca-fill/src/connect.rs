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
// Carries OrcaSlicer code: src/libslic3r/Fill/FillBase.cpp lines 1265-1833 of OrcaSlicer v2.4.2,
// translated on 2026-11-14 (orca-o3-fill-2). Copyright the OrcaSlicer authors, the BambuStudio
// authors (Bambu Lab), the PrusaSlicer authors (Prusa Research) and the Slic3r authors (Alessandro
// Ranellucci), under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! The connection of infill lines along the boundary (FillBase.cpp): the boundary with the lines'
//! ends inserted and linked ([`BoundaryInfillGraph`], [`create_boundary_infill_graph`]), the third
//! `Fill::connect_infill` ([`connect_infill`]), `Fill::chain_or_connect_infill`
//! ([`chain_or_connect_infill`]) and `Fill::extended_object_bounding_box`
//! ([`FillBase::extended_object_bounding_box`]).
//!
//! - The T-joints are the arena `map_infill_end_point_to_boundary`, two per line (its first point,
//!   then its last), linked by index as in [`crate::contour`]. The C++'s pointer differences
//!   (`&cp - map_infill_end_point_to_boundary.data()`) are those indices; a line's index is half
//!   its T-joint's.
//! - `create_boundary_infill_graph` projects each line's two ends onto the boundary through an
//!   `EdgeGrid` (cells of 10 mm, a search radius of `SCALED_EPSILON`), sorts the hits by contour,
//!   segment and parameter with orca-stl's `std::sort`, inserts them into a copy of each contour
//!   (a point equal to the last one copied is not repeated; a hit at the contour's first point
//!   after its last segment reuses point 0) and links them along the contour into a ring. It
//!   closes the ring after every point of the contour once the first T-joint is in, as the C++
//!   does. A line of one point projects its one point twice, both as its first end (the C++
//!   compares the address of the last point with the first's), so its second T-joint stays
//!   unconnected and its first is overwritten by the second hit. The contours are parametrised by
//!   their Euclidean length (each step the difference of two points cast to `double`), and the
//!   boundary under the lines is marked with a clip distance of 1.7 and a collision distance of 0.8
//!   times the scaled spacing ([`crate::contour::mark_boundary_segments_touching_infill`]).
//! - `connect_infill` scales the anchors (`float` millimetres, as `double`), takes the arcs from
//!   each T-joint to the next whose whole arc could be taken, sorted by length with `std::sort`
//!   (with `dont_sort` it takes no arcs at all, as the C++ builds the list only without it),
//!   joins two lines along an arc shorter than the longest anchor (skipping
//!   arcs shorter than the multiline count times the spacing when multiline is above 1) or hooks
//!   both ends by an anchor's length, then joins each end left over to its nearer neighbour within
//!   the longest anchor, or hooks it along its longer free side. The merged lines are kept under
//!   the lowest index of a union-find; the lines left with points are moved out in order.
//! - `chain_or_connect_infill` chains the lines with `chain_polylines` when they are not to be
//!   connected (more than one), else hands them to [`connect_infill_expolygon`].
//! - `extended_object_bounding_box` merges the box's corners with x and y swapped (the second
//!   from the box the first merge left) and scales it by `sqrt(2.)` (orca-crt).
//!
//! What stops ([`crate::stop`]): a boundary contour without points
//! ([`crate::stop::EMPTY_CONTOUR`]) and an infill line without points
//! ([`crate::stop::EMPTY_POLYLINE`]), where the C++ reads outside a vector.
//!
//! Left out: `mark_boundary_segments_overlapping_infill` (FillBase.cpp 1352-1430), whose one
//! caller is `connect_base_support` (the support base pattern, family `infill-supportbase`);
//! `BoundaryInfillGraph::interpolate_contour_point`, which nothing calls; the code inside `#if 0`
//! (the older connection by `ConnectionCost`) and the `reserve` calls. `base_support_extend_infill_lines`
//! (from line 1834) is the support base's.

use orca_geom::bounding_box::BoundingBox;
use orca_geom::expolygon::ExPolygon;
use orca_geom::point::{Point, Points, SCALED_EPSILON, scale_};
use orca_geom::polygon::Polygon;
use orca_geom::polyline::{Polyline, Polylines};
use orca_index::edge_grid::{ClosestPointResult, Grid};
use orca_paths::chain_polylines;

use crate::base::{FillBase, FillParams, connect_infill_expolygon};
use crate::contour::{
    BOUNDARY_IDX_UNCONNECTED, ContourIntersectionPoint, closed_contour_distance_ccw,
    mark_boundary_segments_touching_infill, next_on_contour, path_length_along_contour_ccw,
    prev_on_contour, take, take_limited,
};
use crate::stop::{self, stop};

/// `BoundaryInfillGraph::Direction`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// `Left`.
    Left,
    /// `Right`.
    Right,
    /// `Up`.
    Up,
    /// `Down`.
    Down,
    /// `Taken`.
    Taken,
}

/// `struct BoundaryInfillGraph`: the boundary's contours with the infill lines' ends inserted,
/// their parameters, and the T-joints, two per infill line.
#[derive(Clone, Debug, Default)]
pub struct BoundaryInfillGraph {
    /// `boundary`: each contour, with the T-joints' points inserted.
    pub boundary: Vec<Points>,
    /// `boundary_params`: each contour's points' parameters, then its length.
    pub boundary_params: Vec<Vec<f64>>,
    /// `map_infill_end_point_to_boundary`: the T-joints, line `i`'s first end at `2 i` and its last
    /// at `2 i + 1`.
    pub map_infill_end_point_to_boundary: Vec<ContourIntersectionPoint>,
}

impl BoundaryInfillGraph {
    /// `point(cp)`: the T-joint's point on its contour.
    #[must_use]
    pub fn point(&self, cp: &ContourIntersectionPoint) -> Point {
        self.boundary[cp.contour_idx][cp.point_idx]
    }

    /// `infill_end_point(infill_end_point_idx)`.
    #[must_use]
    pub fn infill_end_point(&self, infill_end_point_idx: usize) -> Point {
        self.point(&self.map_infill_end_point_to_boundary[infill_end_point_idx])
    }

    /// `dir(p1, p2)`: up or down when the x are equal, else right or left.
    #[must_use]
    pub fn dir(p1: Point, p2: Point) -> Direction {
        if p1.x == p2.x {
            if p1.y < p2.y {
                Direction::Up
            } else {
                Direction::Down
            }
        } else if p1.x < p2.x {
            Direction::Right
        } else {
            Direction::Left
        }
    }

    /// `dir_prev(cp)` of T-joint `i`: toward the previous T-joint, or taken.
    #[must_use]
    pub fn dir_prev(&self, i: usize) -> Direction {
        let cps = &self.map_infill_end_point_to_boundary;
        if cps[i].could_take_prev() {
            Self::dir(
                self.point(&cps[i]),
                self.point(&cps[prev_on_contour(cps, i)]),
            )
        } else {
            Direction::Taken
        }
    }

    /// `dir_next(cp)` of T-joint `i`: toward the next T-joint, or taken.
    #[must_use]
    pub fn dir_next(&self, i: usize) -> Direction {
        let cps = &self.map_infill_end_point_to_boundary;
        if cps[i].could_take_next() {
            Self::dir(
                self.point(&cps[i]),
                self.point(&cps[next_on_contour(cps, i)]),
            )
        } else {
            Direction::Taken
        }
    }

    /// `first(cp)` of T-joint `i`: the first end of its line.
    #[must_use]
    pub fn first(i: usize) -> bool {
        (i & 1) == 0
    }

    /// `other(cp)` of T-joint `i`: the T-joint at the line's other end.
    #[must_use]
    pub fn other(i: usize) -> usize {
        i ^ 1
    }

    /// `prev_vertical(cp)` of T-joint `i`: the previous T-joint has the same x.
    #[must_use]
    pub fn prev_vertical(&self, i: usize) -> bool {
        let cps = &self.map_infill_end_point_to_boundary;
        self.point(&cps[i]).x == self.point(&cps[prev_on_contour(cps, i)]).x
    }

    /// `next_vertical(cp)` of T-joint `i`: the next T-joint has the same x.
    #[must_use]
    pub fn next_vertical(&self, i: usize) -> bool {
        let cps = &self.map_infill_end_point_to_boundary;
        self.point(&cps[i]).x == self.point(&cps[next_on_contour(cps, i)]).x
    }
}

/// `create_boundary_infill_graph(infill_ordered, boundary_src, bbox, spacing)`: the boundary
/// with the ends of the infill lines inserted as linked T-joints, parametrised, and the boundary
/// under the lines marked (see the module's notes).
///
/// # Panics
/// With [`crate::stop::EMPTY_POLYLINE`] for an infill line without points and
/// [`crate::stop::EMPTY_CONTOUR`] for a boundary contour without points.
#[must_use]
pub fn create_boundary_infill_graph(
    infill_ordered: &[Polyline],
    boundary_src: &[&Polygon],
    bbox: &BoundingBox,
    spacing: f64,
) -> BoundaryInfillGraph {
    const FUNCTION: &str = "create_boundary_infill_graph";
    let mut out = BoundaryInfillGraph {
        boundary: vec![Points::new(); boundary_src.len()],
        boundary_params: vec![Vec::new(); boundary_src.len()],
        map_infill_end_point_to_boundary: vec![
            ContourIntersectionPoint::new(
                BOUNDARY_IDX_UNCONNECTED,
                BOUNDARY_IDX_UNCONNECTED
            );
            infill_ordered.len() * 2
        ],
    };
    {
        // Project the infill_ordered end points onto boundary_src.
        let mut intersection_points: Vec<(ClosestPointResult, usize)> = Vec::new();
        {
            let mut grid = Grid::new();
            grid.set_bbox(bbox.inflated(SCALED_EPSILON));
            grid.create_polygon_refs(boundary_src, orca_crt::f64_to_i64(scale_(10.)));
            for (pl_idx, pl) in infill_ordered.iter().enumerate() {
                let (Some(&front), Some(&back)) = (pl.points.first(), pl.points.last()) else {
                    stop(FUNCTION, stop::EMPTY_POLYLINE)
                };
                // The second is the last point, unless it is the first (a line of one point).
                for (pt, is_front) in [(front, true), (back, pl.points.len() == 1)] {
                    let cp = grid
                        .closest_point_signed_distance(pt, orca_crt::f64_to_i64(SCALED_EPSILON));
                    if cp.valid() {
                        // The infill end point shall lie on the contour.
                        intersection_points.push((cp, pl_idx * 2 + usize::from(!is_front)));
                    }
                }
            }
            orca_stl::sort_by(&mut intersection_points, |cp1, cp2| {
                cp1.0.contour_idx < cp2.0.contour_idx
                    || (cp1.0.contour_idx == cp2.0.contour_idx
                        && (cp1.0.start_point_idx < cp2.0.start_point_idx
                            || (cp1.0.start_point_idx == cp2.0.start_point_idx
                                && cp1.0.t < cp2.0.t)))
            });
        }
        let BoundaryInfillGraph {
            boundary,
            boundary_params,
            map_infill_end_point_to_boundary: cps,
        } = &mut out;
        let mut it = 0_usize;
        let it_end = intersection_points.len();
        let mut boundary_intersection_points: Vec<Vec<usize>> = vec![Vec::new(); boundary.len()];
        for idx_contour in 0..boundary_src.len() {
            // Copy contour_src to contour_dst while adding intersection points.
            // Map infill end points map_infill_end_point_to_boundary to the newly inserted
            // boundary points of contour_dst.
            // chain the points of map_infill_end_point_to_boundary along their respective
            // contours.
            let contour_src = boundary_src[idx_contour];
            let contour_dst = &mut boundary[idx_contour];
            let contour_intersection_points = &mut boundary_intersection_points[idx_contour];
            let mut pfirst: Option<usize> = None;
            let mut pprev: Option<usize> = None;
            for (idx_point, &ipt) in contour_src.points.iter().enumerate() {
                if contour_dst.last() != Some(&ipt) {
                    contour_dst.push(ipt);
                }
                while it != it_end
                    && intersection_points[it].0.contour_idx == idx_contour
                    && intersection_points[it].0.start_point_idx == idx_point
                {
                    // Add these points to the destination contour.
                    let end_idx = intersection_points[it].1;
                    let infill_line = &infill_ordered[end_idx / 2];
                    let pt = if end_idx & 1 != 0 {
                        infill_line.points[infill_line.points.len() - 1]
                    } else {
                        infill_line.points[0]
                    };
                    let mut idx_tjoint_pt = 0;
                    if idx_point + 1 < contour_src.points.len() || pt != contour_dst[0] {
                        if contour_dst.last() != Some(&pt) {
                            contour_dst.push(pt);
                        }
                        idx_tjoint_pt = contour_dst.len() - 1;
                    }
                    cps[end_idx] = ContourIntersectionPoint::new(idx_contour, idx_tjoint_pt);
                    let pthis = end_idx;
                    if let Some(pp) = pprev {
                        cps[pp].next_on_contour = Some(pthis);
                        cps[pthis].prev_on_contour = Some(pp);
                    } else {
                        pfirst = Some(pthis);
                    }
                    contour_intersection_points.push(pthis);
                    pprev = Some(pthis);
                    it += 1;
                }
                if let (Some(pf), Some(pp)) = (pfirst, pprev) {
                    cps[pp].next_on_contour = Some(pf);
                    cps[pf].prev_on_contour = Some(pp);
                }
            }
            if contour_dst.is_empty() {
                stop(FUNCTION, stop::EMPTY_CONTOUR);
            }
            // Parametrize the new boundary with the intersection points inserted.
            let contour_params = &mut boundary_params[idx_contour];
            *contour_params = vec![0.; contour_dst.len() + 1];
            for i in 1..contour_dst.len() {
                contour_params[i] = contour_params[i - 1]
                    + (contour_dst[i].cast_f64() - contour_dst[i - 1].cast_f64()).norm();
            }
            let n = contour_params.len();
            contour_params[n - 1] = contour_params[n - 2]
                + (contour_dst[contour_dst.len() - 1].cast_f64() - contour_dst[0].cast_f64())
                    .norm();
            // Map parameters from contour_params to boundary_intersection_points.
            for &ip in contour_intersection_points.iter() {
                cps[ip].param = contour_params[cps[ip].point_idx];
            }
            // and measure distance to the previous and next intersection point.
            let contour_length = contour_params[n - 1];
            for &ip in contour_intersection_points.iter() {
                if cps[ip].next_on_contour == Some(ip) {
                    cps[ip].contour_not_taken_length_prev = contour_length;
                    cps[ip].contour_not_taken_length_next = contour_length;
                } else {
                    let prev = prev_on_contour(cps, ip);
                    cps[ip].contour_not_taken_length_prev =
                        closed_contour_distance_ccw(cps[prev].param, cps[ip].param, contour_length);
                    let next = next_on_contour(cps, ip);
                    cps[ip].contour_not_taken_length_next =
                        closed_contour_distance_ccw(cps[ip].param, cps[next].param, contour_length);
                }
            }
        }

        // Mark the points and segments of split out.boundary as consumed if they are very close
        // to some of the infill line.
        {
            // @supermerill used 2. * scale_(spacing)
            let clip_distance = 1.7 * scale_(spacing);
            // Allow a bit of overlap. This value must be slightly higher than the overlap of
            // FillAdaptive, otherwise the anchors of the adaptive infill will mask the other side
            // of the perimeter line. (see connect_lines_using_hooks() in FillAdaptive.cpp)
            let distance_colliding = 0.8 * scale_(spacing);
            mark_boundary_segments_touching_infill(
                boundary,
                boundary_params,
                cps,
                &boundary_intersection_points,
                bbox,
                infill_ordered,
                clip_distance,
                distance_colliding,
            );
        }
    }

    out
}

impl FillBase<'_> {
    /// `Fill::extended_object_bounding_box()` (FillBase.cpp 1569-1578): the object's box grown to
    /// cover any turn of every layer: its corners merged with x and y swapped (the second from the
    /// box the first merge left), then scaled by `sqrt(2.)`.
    #[must_use]
    pub fn extended_object_bounding_box(&self) -> BoundingBox {
        let mut out = self.bounding_box;
        let p = Point::new(out.min.y, out.min.x);
        out.merge(p);
        let p = Point::new(out.max.y, out.max.x);
        out.merge(p);

        // The bounding box is scaled by sqrt(2.) to ensure that the bounding box
        // covers any possible rotations.
        out.scaled(orca_crt::sqrt(2.))
    }
}

/// `get_and_update_merged_with(polyline_idx)`, `connect_infill`'s lambda: the line `polyline_idx`
/// was merged into (the root of its chain), remembered for it.
fn get_and_update_merged_with(merged_with: &mut [usize], polyline_idx: usize) -> usize {
    let mut last = polyline_idx;
    loop {
        let lower = merged_with[last];
        if lower == last {
            merged_with[polyline_idx] = last;
            return last;
        }
        last = lower;
    }
}

/// Two different lines of `v`, to change both.
fn pair_mut(v: &mut [Polyline], a: usize, b: usize) -> (&mut Polyline, &mut Polyline) {
    if a < b {
        let (lo, hi) = v.split_at_mut(b);
        (&mut lo[a], &mut hi[0])
    } else {
        let (lo, hi) = v.split_at_mut(a);
        (&mut hi[0], &mut lo[b])
    }
}

/// `Arc`, `connect_infill`'s: a T-joint and the length of the boundary to the next.
#[derive(Clone, Copy, Debug)]
struct Arc {
    intersection: usize,
    arc_length: f64,
}

/// `Fill::connect_infill(infill_ordered, const std::vector<const Polygon*> &boundary, bbox,
/// polylines_out, spacing, params)` (FillBase.cpp 1580-1818): the infill lines joined along the
/// boundary, or hooked onto it by the anchors, appended to `polylines_out` (see the module's
/// notes).
///
/// # Panics
/// With [`crate::stop::EMPTY_POLYLINE`] for an infill line without points and
/// [`crate::stop::EMPTY_CONTOUR`] for a boundary contour without points.
pub fn connect_infill(
    infill_ordered: Polylines,
    boundary_src: &[&Polygon],
    bbox: &BoundingBox,
    polylines_out: &mut Polylines,
    spacing: f64,
    params: &FillParams<'_>,
) {
    let mut infill_ordered = infill_ordered;
    let anchor_length = scale_(f64::from(params.anchor_length));
    let anchor_length_max = scale_(f64::from(params.anchor_length_max));

    let mut graph = create_boundary_infill_graph(&infill_ordered, boundary_src, bbox, spacing);

    let mut merged_with: Vec<usize> = (0..infill_ordered.len()).collect();

    let line_half_width = 0.5 * scale_(spacing);

    let BoundaryInfillGraph {
        boundary,
        boundary_params,
        map_infill_end_point_to_boundary: cps,
    } = &mut graph;
    let boundary: &[Points] = boundary;
    let boundary_params: &[Vec<f64>] = boundary_params;

    let mut arches: Vec<Arc> = Vec::new();
    if !params.dont_sort {
        for i in 0..cps.len() {
            let cp = &cps[i];
            if cp.contour_idx != BOUNDARY_IDX_UNCONNECTED
                && cp.next_on_contour != Some(i)
                && ContourIntersectionPoint::could_connect_next(cps, i)
            {
                let next = next_on_contour(cps, i);
                let contour_length =
                    boundary_params[cp.contour_idx][boundary_params[cp.contour_idx].len() - 1];
                arches.push(Arc {
                    intersection: i,
                    arc_length: path_length_along_contour_ccw(cp, &cps[next], contour_length),
                });
            }
        }
        orca_stl::sort_by(&mut arches, |l, r| l.arc_length < r.arc_length);
    }

    //FIXME improve the Traveling Salesman problem with 2-opt and 3-opt local optimization.
    for arc in &arches {
        if !cps[arc.intersection].consumed && !cps[next_on_contour(cps, arc.intersection)].consumed
        {
            // Indices of the polylines to be connected by a perimeter segment.
            let cp1 = arc.intersection;
            let cp2 = next_on_contour(cps, arc.intersection);
            let polyline_idx1 = get_and_update_merged_with(&mut merged_with, cp1 / 2);
            let polyline_idx2 = get_and_update_merged_with(&mut merged_with, cp2 / 2);
            let contour = &boundary[cps[cp1].contour_idx];

            // Orca: If multiline infill is requested, skip connections that are too short.
            if params.multiline > 1
                && arc.arc_length < scale_(spacing) * f64::from(params.multiline)
            {
                continue;
            }

            let contour_params = &boundary_params[cps[cp1].contour_idx];
            if polyline_idx1 != polyline_idx2 {
                if arc.arc_length < anchor_length_max {
                    {
                        let (polyline1, polyline2) =
                            pair_mut(&mut infill_ordered, polyline_idx1, polyline_idx2);
                        // Not closing a loop, connecting the lines.
                        if polyline1.points.first() == Some(&contour[cps[cp1].point_idx]) {
                            polyline1.reverse();
                        }
                        if polyline2.points.last() == Some(&contour[cps[cp2].point_idx]) {
                            polyline2.reverse();
                        }
                        take(polyline1, polyline2, contour, cps, cp1, cp2, false);
                    }
                    // Mark the second polygon as merged with the first one.
                    if polyline_idx2 < polyline_idx1 {
                        infill_ordered[polyline_idx2] =
                            core::mem::take(&mut infill_ordered[polyline_idx1]);
                        infill_ordered[polyline_idx1].points.clear();
                        merged_with[polyline_idx1] = merged_with[polyline_idx2];
                    } else {
                        infill_ordered[polyline_idx2].points.clear();
                        merged_with[polyline_idx2] = merged_with[polyline_idx1];
                    }
                } else if anchor_length > SCALED_EPSILON {
                    // Move along the perimeter, but don't take the whole arc.
                    take_limited(
                        &mut infill_ordered[polyline_idx1],
                        contour,
                        contour_params,
                        cps,
                        cp1,
                        cp2,
                        false,
                        anchor_length,
                        line_half_width,
                    );
                    take_limited(
                        &mut infill_ordered[polyline_idx2],
                        contour,
                        contour_params,
                        cps,
                        cp2,
                        cp1,
                        true,
                        anchor_length,
                        line_half_width,
                    );
                }
            }
        }
    }

    // Connect the remaining open infill lines to the perimeter lines if possible.
    for contour_point in 0..cps.len() {
        if !cps[contour_point].consumed
            && cps[contour_point].contour_idx != BOUNDARY_IDX_UNCONNECTED
        {
            let contour = &boundary[cps[contour_point].contour_idx];
            let contour_params = &boundary_params[cps[contour_point].contour_idx];
            let contour_length = contour_params[contour_params.len() - 1];

            let lprev = if ContourIntersectionPoint::could_connect_prev(cps, contour_point) {
                path_length_along_contour_ccw(
                    &cps[prev_on_contour(cps, contour_point)],
                    &cps[contour_point],
                    contour_length,
                )
            } else {
                f64::MAX
            };
            let lnext = if ContourIntersectionPoint::could_connect_next(cps, contour_point) {
                path_length_along_contour_ccw(
                    &cps[contour_point],
                    &cps[next_on_contour(cps, contour_point)],
                    contour_length,
                )
            } else {
                f64::MAX
            };
            let polyline_idx = get_and_update_merged_with(&mut merged_with, contour_point / 2);
            let mut connected = false;
            for l in [orca_stl::min(lprev, lnext), orca_stl::max(lprev, lnext)] {
                if l == f64::MAX || l > anchor_length_max {
                    break;
                }
                // Take the complete contour.
                let reversed = l == lprev;
                let cp2 = if reversed {
                    prev_on_contour(cps, contour_point)
                } else {
                    next_on_contour(cps, contour_point)
                };
                // Identify which end of the polyline touches the boundary.
                let polyline_idx2 = get_and_update_merged_with(&mut merged_with, cp2 / 2);
                if polyline_idx == polyline_idx2 {
                    // Try the other side.
                    continue;
                }
                {
                    let (polyline, polyline2) =
                        pair_mut(&mut infill_ordered, polyline_idx, polyline_idx2);
                    // Not closing a loop.
                    if polyline.points.first() == Some(&contour[cps[contour_point].point_idx]) {
                        polyline.reverse();
                    }
                    if polyline2.points.last() == Some(&contour[cps[cp2].point_idx]) {
                        polyline2.reverse();
                    }
                    take(
                        polyline,
                        polyline2,
                        contour,
                        cps,
                        contour_point,
                        cp2,
                        reversed,
                    );
                }
                if polyline_idx < polyline_idx2 {
                    // Mark the second polyline as merged with the first one.
                    merged_with[polyline_idx2] = polyline_idx;
                    infill_ordered[polyline_idx2].points.clear();
                } else {
                    // Mark the first polyline as merged with the second one.
                    merged_with[polyline_idx] = polyline_idx2;
                    infill_ordered[polyline_idx2] =
                        core::mem::take(&mut infill_ordered[polyline_idx]);
                    infill_ordered[polyline_idx].points.clear();
                }
                connected = true;
                break;
            }
            if !connected && anchor_length > SCALED_EPSILON {
                // Which to take? One could optimize for:
                // 1) Shortest path
                // 2) Hook length
                // ...
                // Let's take the longer now, as this improves the chance of another hook to be
                // placed on the other side of this contour point.
                let l = orca_stl::max(
                    cps[contour_point].contour_not_taken_length_prev,
                    cps[contour_point].contour_not_taken_length_next,
                );
                if l > SCALED_EPSILON {
                    if cps[contour_point].contour_not_taken_length_prev
                        > cps[contour_point].contour_not_taken_length_next
                    {
                        let prev = prev_on_contour(cps, contour_point);
                        take_limited(
                            &mut infill_ordered[polyline_idx],
                            contour,
                            contour_params,
                            cps,
                            contour_point,
                            prev,
                            true,
                            anchor_length,
                            line_half_width,
                        );
                    } else {
                        let next = next_on_contour(cps, contour_point);
                        take_limited(
                            &mut infill_ordered[polyline_idx],
                            contour,
                            contour_params,
                            cps,
                            contour_point,
                            next,
                            false,
                            anchor_length,
                            line_half_width,
                        );
                    }
                }
            }
        }
    }

    for pl in infill_ordered {
        if !pl.is_empty() {
            polylines_out.push(pl);
        }
    }
}

/// `Fill::chain_or_connect_infill(infill_ordered, boundary, polylines_out, spacing, params)`
/// (FillBase.cpp 1820-1830): nothing for no lines; else, when the lines are not to be connected
/// (`params.dont_connect()`), the lines chained by `chain_polylines` (more than one) and appended,
/// or else joined along the boundary by [`connect_infill_expolygon`].
pub fn chain_or_connect_infill(
    infill_ordered: Polylines,
    boundary: &ExPolygon,
    polylines_out: &mut Polylines,
    spacing: f64,
    params: &FillParams<'_>,
) {
    if !infill_ordered.is_empty() {
        if params.dont_connect() {
            let mut infill_ordered = infill_ordered;
            if infill_ordered.len() > 1 {
                infill_ordered = chain_polylines(infill_ordered, None);
            }
            polylines_out.extend(infill_ordered);
        } else {
            connect_infill_expolygon(infill_ordered, boundary, polylines_out, spacing, params);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A counter-clockwise square from 0 to `size`.
    fn square(size: i64) -> ExPolygon {
        ExPolygon::new(Polygon::new(vec![
            Point::new(0, 0),
            Point::new(size, 0),
            Point::new(size, size),
            Point::new(0, size),
        ]))
    }

    /// Vertical lines across the 10 mm square at the given x (mm), bottom to top.
    fn lines(xs: &[f64]) -> Polylines {
        xs.iter()
            .map(|&x| {
                let x = orca_crt::f64_to_i64(scale_(x));
                Polyline::new(vec![Point::new(x, 0), Point::new(x, 10_000_000)])
            })
            .collect()
    }

    fn params(anchor_length: f32, anchor_length_max: f32) -> FillParams<'static> {
        FillParams {
            density: 0.2,
            anchor_length,
            anchor_length_max,
            ..FillParams::default()
        }
    }

    #[test]
    fn the_extended_box_covers_the_swapped_corners_turned() {
        let mut f = FillBase::new();
        f.bounding_box = BoundingBox::from_corners(Point::new(10, 0), Point::new(20, 40));
        let b = f.extended_object_bounding_box();
        // Merged with (0, 10) and then (40, 40): from 0 to 40 both ways, then times sqrt(2).
        assert_eq!(b.min, Point::new(0, 0));
        assert_eq!(b.max.x, b.max.y);
        assert!((56..=57).contains(&b.max.x));
    }

    #[test]
    fn two_lines_close_together_are_joined_along_the_boundary() {
        let mut out = Polylines::new();
        connect_infill_expolygon(
            lines(&[3., 4.]),
            &square(10_000_000),
            &mut out,
            0.45,
            &params(1000., 1000.),
        );
        assert_eq!(out.len(), 1);
        // Both lines are in it, and the piece of boundary between them.
        assert!(out[0].points.len() >= 4);
        let length: f64 = out[0].length();
        assert!(length > 20_000_000.);
    }

    #[test]
    fn unconnected_lines_are_only_chained() {
        let mut out = Polylines::new();
        chain_or_connect_infill(
            lines(&[2., 5., 8.]),
            &square(10_000_000),
            &mut out,
            0.45,
            &params(0., 0.),
        );
        assert_eq!(out.len(), 3);
        assert!(out.iter().all(|pl| pl.points.len() == 2));
        // Nothing in, nothing out, whether connected or not.
        let mut none = Polylines::new();
        chain_or_connect_infill(
            Polylines::new(),
            &square(10_000_000),
            &mut none,
            0.45,
            &params(1000., 1000.),
        );
        assert!(none.is_empty());
    }

    #[test]
    fn a_line_without_points_stops() {
        let mut infill = lines(&[3.]);
        infill.push(Polyline::new(Vec::new()));
        let payload = std::panic::catch_unwind(|| {
            let mut out = Polylines::new();
            connect_infill_expolygon(
                infill,
                &square(10_000_000),
                &mut out,
                0.45,
                &params(1000., 1000.),
            );
        })
        .unwrap_err();
        let payload = payload.downcast::<stop::NotPorted>().unwrap();
        assert_eq!(payload.condition, stop::EMPTY_POLYLINE);
    }
}
