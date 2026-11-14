// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/BridgeDetector.hpp (1-131) and BridgeDetector.cpp
// (1-474) of OrcaSlicer v2.4.2, with Polygon.hpp's polygons_rotate (162-168), translated on
// 2026-11-14 (orca-o3-walls-1). Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu
// Lab), the PrusaSlicer authors (Prusa Research) and the Slic3r authors, under the GNU Affero
// General Public License, version 3 (AGPL-3.0; LICENSE).

//! The bridge detector: the direction to lay a bridge over a region, measured counter-clockwise
//! from the x axis, and the parts of the region a bridge in a direction covers.
//!
//! [`BridgeDetector`] grows the region by the spacing and takes its anchors where it meets the
//! lower slices. [`BridgeDetector::detect_angle`] tries every 5° and the directions of the
//! region's edges and of each open supporting edge: for each it lays lines `spacing` apart over
//! the anchors' rotated box, clips them to the region grown by half the spacing, and sums the
//! lengths of those whose two ends are anchored. The direction of most coverage wins, unless
//! another within one spacing of it has shorter lines. [`BridgeDetector::coverage`] cuts the
//! rotated region into vertical trapezoids (every x of its points, or every spacing when precise)
//! and keeps those anchored twice; [`BridgeDetector::unsupported_edges`] gives the region's edges
//! off the grown lower slices that are not parallel to the bridge.
//!
//! [`detect_bridging_direction_lines`] and [`detect_bridging_direction`] are the header's newer
//! detector: the second principal component of the overhang when nothing floats, else the
//! direction across the floating edges' normals that the edges cross least. Its normals are kept
//! in an `std::unordered_map<double, Vec2d>` under `ceil(atan2 * 1000)`, the first of each key, and
//! walked in MSVC's order ([`orca_stl::UnorderedMap`]), which decides between equal costs.
//!
//! The detector holds its expolygons as a [`Cow`]: the C++ one of an `ExPolygon` keeps its own
//! copy, the one of `ExPolygons` a reference. A spacing not above 0 makes the C++ divide by zero or
//! loop forever in `detect_angle` and `get_trapezoids3_half`; those stop with [`NotPorted`]
//! ([`crate::stop`]).
//!
//! [`NotPorted`]: crate::stop::NotPorted

use std::borrow::Cow;

use orca_eigen::{Vec2d, Vec2f};
use orca_geom::bounding_box::BoundingBox;
use orca_geom::clipper_utils::{
    ApplySafetyOffset, CLIPPER_SAFETY_OFFSET, DEFAULT_JOIN_TYPE, DEFAULT_MITER_LIMIT, diff,
    diff_pl, expand, intersection, intersection_ex, intersection_ln, intersection_pl,
    offset_ex_expolygon, offset_expolygons, union_,
};
use orca_geom::expolygon::{self, ExPolygon, ExPolygons, get_extents_rotated_ex, to_lines_ex};
use orca_geom::geometry::{contains as expolygons_contain, directions_parallel};
use orca_geom::line::{Line, Lines};
use orca_geom::point::{Coord, PI, Point, SCALED_EPSILON, vec2d};
use orca_geom::polygon::{self, Polygon, Polygons};
use orca_geom::polyline::{Polyline, Polylines, to_lines_polylines};
use orca_stl::UnorderedMap;

use crate::principal_components::compute_principal_components;
use crate::stop::{SPACING_NOT_POSITIVE, not_ported};

/// `polygons_rotate(polys, angle)` (Polygon.hpp 162-168): the cosine and sine taken once, then
/// each polygon rotated by them. It belongs in `orca-geom`, whose `polygon` has no such function;
/// translated here (walls-1).
pub fn polygons_rotate(polys: &mut [Polygon], angle: f64) {
    let cos_angle = orca_crt::cos(angle);
    let sin_angle = orca_crt::sin(angle);
    for p in polys {
        p.rotate_cs(cos_angle, sin_angle);
    }
}

/// `(coord_t)round(v)`.
fn round_coord(v: f64) -> Coord {
    orca_crt::f64_to_i64(orca_crt::round(v))
}

/// `BridgeDirection`: a candidate angle and what it covers.
#[derive(Clone, Copy, Debug)]
struct BridgeDirection {
    angle: f64,
    coverage: f64,
    max_length: f64,
    archored_percent: f64,
}

impl BridgeDirection {
    /// `BridgeDirection(a)`: nothing covered yet.
    fn new(angle: f64) -> Self {
        Self {
            angle,
            coverage: 0.,
            max_length: 0.,
            archored_percent: 0.,
        }
    }
}

/// `BridgeDetector`.
#[derive(Clone, Debug)]
pub struct BridgeDetector<'a> {
    /// `expolygons` (or `expolygons_owned`): the bridge's region, not grown.
    pub expolygons: Cow<'a, [ExPolygon]>,
    /// `lower_slices`: the lower layer's slices, all regions.
    pub lower_slices: &'a [ExPolygon],
    /// `spacing`: the scaled extrusion spacing of the infill.
    pub spacing: Coord,
    /// `resolution`: the angle step of the search, π/36.
    pub resolution: f64,
    /// `angle`: the angle found, -1 until one is.
    pub angle: f64,
    /// `_edges`: the open lines of the grown region over the lower slices' contours.
    edges: Polylines,
    /// `_anchor_regions`: the grown region over the lower slices.
    anchor_regions: ExPolygons,
}

impl<'a> BridgeDetector<'a> {
    /// `BridgeDetector(ExPolygon, lower_slices, spacing)`: the detector keeps its own copy of the
    /// region.
    #[must_use]
    pub fn from_expolygon(
        expolygon: ExPolygon,
        lower_slices: &'a [ExPolygon],
        spacing: Coord,
    ) -> Self {
        let mut out = Self {
            expolygons: Cow::Owned(vec![expolygon]),
            lower_slices,
            spacing,
            resolution: 0.,
            angle: 0.,
            edges: Polylines::new(),
            anchor_regions: ExPolygons::new(),
        };
        out.initialize();
        out
    }

    /// `BridgeDetector(const ExPolygons &, lower_slices, spacing)`.
    #[must_use]
    pub fn new(expolygons: &'a [ExPolygon], lower_slices: &'a [ExPolygon], spacing: Coord) -> Self {
        let mut out = Self {
            expolygons: Cow::Borrowed(expolygons),
            lower_slices,
            spacing,
            resolution: 0.,
            angle: 0.,
            edges: Polylines::new(),
            anchor_regions: ExPolygons::new(),
        };
        out.initialize();
        out
    }

    /// `initialize()`: the anchors and supporting edges of the region grown by the spacing.
    #[allow(clippy::cast_precision_loss)]
    fn initialize(&mut self) {
        // 5 degrees stepping
        self.resolution = PI / 36.0;
        // output angle not known
        self.angle = -1.;

        // Outset our bridge by an arbitrary amout; we'll use this outer margin for detecting
        // anchors.
        let grown = offset_expolygons(
            &self.expolygons,
            self.spacing as f32,
            DEFAULT_JOIN_TYPE,
            DEFAULT_MITER_LIMIT,
        );

        // Detect what edges lie on lower slices by turning bridge contour and holes into
        // polylines and then clipping them with each lower slice's contour.
        let mut contours = Polygons::with_capacity(self.lower_slices.len());
        for expoly in self.lower_slices {
            contours.push(expoly.contour.clone());
        }
        self.edges = intersection_pl(&polygon::to_polylines(&grown), &contours);

        // detect anchors as intersection between our bridge expolygon and the lower slices
        // (`union_safety_offset` of expolygons: `offset(expolygons, ClipperSafetyOffset)`)
        self.anchor_regions = intersection_ex(
            &grown,
            &offset_expolygons(
                self.lower_slices,
                CLIPPER_SAFETY_OFFSET,
                DEFAULT_JOIN_TYPE,
                DEFAULT_MITER_LIMIT,
            ),
            ApplySafetyOffset::No,
        );
    }

    /// `detect_angle(bridge_direction_override)`: whether a direction was found, which is then
    /// `angle`; the override (when not 0) is the only candidate.
    #[allow(clippy::cast_precision_loss, clippy::float_cmp)]
    pub fn detect_angle(&mut self, bridge_direction_override: f64) -> bool {
        if self.edges.is_empty() || self.anchor_regions.is_empty() {
            // The bridging region is completely in the air, there are no anchors available at
            // the layer below.
            return false;
        }

        let mut candidates: Vec<BridgeDirection> = Vec::new();
        if bridge_direction_override == 0. {
            let angles = self.bridge_direction_candidates();
            candidates.reserve(angles.len());
            for a in angles {
                candidates.push(BridgeDirection::new(a));
            }
        } else {
            candidates.push(BridgeDirection::new(bridge_direction_override));
        }

        // Outset the bridge expolygon by half the amount we used for detecting anchors; we'll use
        // this one to clip our test lines and be sure that their endpoints are inside the anchors
        // and not on their contours leading to false negatives.
        let clip_area = offset_expolygons(
            &self.expolygons,
            0.5f32 * self.spacing as f32,
            DEFAULT_JOIN_TYPE,
            DEFAULT_MITER_LIMIT,
        );

        // `lines.reserve((max.y - min.y + spacing) / spacing)` divides by the spacing, and the
        // lines below step by it.
        if self.spacing <= 0 {
            not_ported("BridgeDetector::detect_angle", SPACING_NOT_POSITIVE);
        }

        let mut have_coverage = false;
        for candidate in &mut candidates {
            let angle = candidate.angle;

            let mut lines = Lines::new();
            {
                // Get an oriented bounding box around _anchor_regions.
                let bbox = get_extents_rotated_ex(&self.anchor_regions, -angle);
                // Cover the region with line segments.
                let s = orca_crt::sin(angle);
                let c = orca_crt::cos(angle);
                let mut y = bbox.min.y;
                while y <= bbox.max.y {
                    let yf = y as f64;
                    lines.push(Line {
                        a: Point::new(
                            round_coord(c * bbox.min.x as f64 - s * yf),
                            round_coord(c * yf + s * bbox.min.x as f64),
                        ),
                        b: Point::new(
                            round_coord(c * bbox.max.x as f64 - s * yf),
                            round_coord(c * yf + s * bbox.max.x as f64),
                        ),
                    });
                    y += self.spacing;
                }
            }

            let mut total_length = 0.;
            let mut max_length = 0.;
            {
                let clipped_lines = intersection_ln(&lines, &clip_area);
                let mut archored_line_num = 0usize;
                for line in &clipped_lines {
                    if expolygons_contain(&self.anchor_regions, line.a)
                        && expolygons_contain(&self.anchor_regions, line.b)
                    {
                        // This line could be anchored.
                        let len = line.length();
                        total_length += len;
                        max_length = orca_stl::max(max_length, len);
                        archored_line_num += 1;
                    }
                }
                if !clipped_lines.is_empty() && archored_line_num > 0 {
                    candidate.archored_percent =
                        archored_line_num as f64 / clipped_lines.len() as f64;
                }
            }
            if total_length == 0. {
                continue;
            }

            have_coverage = true;
            // Sum length of bridged lines.
            candidate.coverage = total_length;
            // max length of bridged lines
            candidate.max_length = max_length;
        }

        // if no direction produced coverage, then there's no bridge direction
        if !have_coverage {
            return false;
        }

        // sort directions by coverage - most coverage first
        orca_stl::sort_by(&mut candidates, |a, b| a.coverage > b.coverage);

        // if any other direction is within extrusion width of coverage, prefer it if shorter
        let mut i_best = 0usize;
        let mut i = 1usize;
        while i < candidates.len()
            && candidates[i_best].coverage - candidates[i].coverage < self.spacing as f64
        {
            if candidates[i].max_length < candidates[i_best].max_length {
                i_best = i;
            }
            i += 1;
        }

        self.angle = candidates[i_best].angle;
        if self.angle >= PI {
            self.angle -= PI;
        }

        true
    }

    /// `bridge_direction_candidates()`: every `resolution` step from 0 to π, the directions of the
    /// region's edges and of each open supporting edge, sorted, those within 1° of the one before
    /// dropped, and the last dropped when parallel to the first.
    fn bridge_direction_candidates(&self) -> Vec<f64> {
        // we test angles according to configured resolution
        let mut angles: Vec<f64> = Vec::new();
        let mut i: i32 = 0;
        while f64::from(i) <= PI / self.resolution {
            angles.push(f64::from(i) * self.resolution);
            i += 1;
        }

        // we also test angles of each bridge contour
        for line in &to_lines_ex(&self.expolygons) {
            angles.push(line.direction());
        }

        // we also test angles of each open supporting edge (this finds the optimal angle for
        // C-shaped supports)
        for edge in &self.edges {
            if edge.first_point() != edge.last_point() {
                angles.push(
                    Line {
                        a: edge.first_point(),
                        b: edge.last_point(),
                    }
                    .direction(),
                );
            }
        }

        // remove duplicates
        let min_resolution = PI / 180.0; // 1 degree
        orca_stl::sort(&mut angles);
        let mut i = 1usize;
        while i < angles.len() {
            if directions_parallel(angles[i], angles[i - 1], min_resolution) {
                angles.remove(i);
            } else {
                i += 1;
            }
        }
        // compare first value with last one and remove the greatest one (PI) in case they are
        // parallel (PI, 0)
        if directions_parallel(angles[0], angles[angles.len() - 1], min_resolution) {
            angles.pop();
        }

        angles
    }

    /// `coverage(angle, precise)`: the trapezoids of the region a bridge at `angle` (-1: the one
    /// found) covers, united; none without an angle.
    #[must_use]
    #[allow(clippy::cast_precision_loss, clippy::float_cmp)]
    pub fn coverage(&self, angle: f64, precise: bool) -> Polygons {
        let angle = if angle == -1. { self.angle } else { angle };

        let mut covered = Polygons::new();

        if angle != -1. {
            // Get anchors, convert them to Polygons and rotate them.
            let mut anchors = expolygon::to_polygons_ex(&self.anchor_regions);
            polygons_rotate(&mut anchors, PI / 2.0 - angle);

            for expolygon in self.expolygons.iter() {
                // Clone our expolygon and rotate it so that we work with vertical lines.
                let mut expolygon = expolygon.clone();
                expolygon.rotate(PI / 2.0 - angle);
                // Outset the bridge expolygon by half the amount we used for detecting anchors;
                // we'll use this one to generate our trapezoids and be sure that their vertices
                // are inside the anchors and not on their contours leading to false negatives.
                for expoly in offset_ex_expolygon(
                    &expolygon,
                    0.5f32 * self.spacing as f32,
                    DEFAULT_JOIN_TYPE,
                    DEFAULT_MITER_LIMIT,
                ) {
                    // Compute trapezoids according to a vertical orientation
                    let mut trapezoids = Polygons::new();
                    if precise {
                        get_trapezoids3_half(&expoly, &mut trapezoids, self.spacing as f32);
                    } else {
                        get_trapezoids2_rotated(&expoly, &mut trapezoids, PI / 2.);
                    }
                    for mut trapezoid in trapezoids {
                        let mut n_supported = 0usize;
                        if precise {
                            let intersects = intersection(
                                core::slice::from_ref(&trapezoid),
                                &anchors,
                                ApplySafetyOffset::No,
                            );
                            n_supported = intersects.len();

                            if n_supported >= 2 {
                                // trim it to not allow to go outside of the intersections
                                let mut center_bound = intersects[0].bounding_box();
                                let mut min_y = center_bound.center().y;
                                let mut max_y = center_bound.center().y;
                                for poly_bound in &intersects {
                                    center_bound = poly_bound.bounding_box();
                                    if min_y > center_bound.center().y {
                                        min_y = center_bound.center().y;
                                    }
                                    if max_y < center_bound.center().y {
                                        max_y = center_bound.center().y;
                                    }
                                }
                                let mut min_x = trapezoid.points[0].x;
                                let mut max_x = trapezoid.points[0].x;
                                for p in &trapezoid.points {
                                    if min_x > p.x {
                                        min_x = p.x;
                                    }
                                    if max_x < p.x {
                                        max_x = p.x;
                                    }
                                }
                                // add what get_trapezoids3 has removed (+EPSILON)
                                min_x -= self.spacing / 4 + 1;
                                max_x += self.spacing / 4 + 1;
                                let mid_x = (min_x + max_x) / 2;
                                for p in &mut trapezoid.points {
                                    if p.y < min_y {
                                        p.y = min_y;
                                    }
                                    if p.y > max_y {
                                        p.y = max_y;
                                    }
                                    if p.x > min_x && p.x < mid_x {
                                        p.x = min_x;
                                    }
                                    if p.x < max_x && p.x > mid_x {
                                        p.x = max_x;
                                    }
                                }
                            }
                        } else {
                            // not nice, we need a more robust non-numeric check
                            for supported_line in intersection_ln(&trapezoid.lines(), &anchors) {
                                if supported_line.length() >= self.spacing as f64 {
                                    n_supported += 1;
                                }
                            }
                        }

                        if n_supported >= 2 {
                            //add it
                            covered.push(trapezoid);
                        }
                    }
                }
            }

            // Unite the trapezoids before rotation, as the rotation creates tiny gaps and
            // intersections between the trapezoids instead of exact overlaps.
            covered = union_(&covered);
            // Intersect trapezoids with actual bridge area to remove extra margins and append it
            // to result.
            polygons_rotate(&mut covered, -(PI / 2.0 - angle));
        }
        covered
    }

    /// `unsupported_edges(angle, unsupported)`: the region's edges off the lower slices grown by
    /// the spacing that are not parallel to the bridge at `angle` (-1: the one found), each a
    /// two-point polyline appended; nothing without an angle.
    #[allow(clippy::cast_precision_loss, clippy::float_cmp)]
    pub fn unsupported_edges_into(&self, angle: f64, unsupported: &mut Polylines) {
        let angle = if angle == -1. { self.angle } else { angle };
        if angle == -1. {
            return;
        }

        let grown_lower = offset_expolygons(
            self.lower_slices,
            self.spacing as f32,
            DEFAULT_JOIN_TYPE,
            DEFAULT_MITER_LIMIT,
        );

        for expoly in self.expolygons.iter() {
            // get unsupported bridge edges (both contour and holes)
            let unsupported_lines =
                to_lines_polylines(&diff_pl(&expolygon::to_polylines(expoly), &grown_lower));
            // Split into individual segments and filter out edges parallel to the bridging angle
            for line in &unsupported_lines {
                if !directions_parallel(line.direction(), angle, 0.) {
                    unsupported.push(Polyline::new(vec![line.a, line.b]));
                }
            }
        }
    }

    /// `unsupported_edges(angle)`.
    #[must_use]
    pub fn unsupported_edges(&self, angle: f64) -> Polylines {
        let mut pp = Polylines::new();
        self.unsupported_edges_into(angle, &mut pp);
        pp
    }
}

/// `get_trapezoids2(expoly, polygons)`: the expolygon cut at every x of its points, each slice
/// between two different x's the intersection of their box-high rectangle with it, appended.
fn get_trapezoids2(expoly: &ExPolygon, polygons: &mut Polygons) {
    let src_polygons = expolygon::to_polygons(expoly);
    // get all points of this ExPolygon
    let pp = polygon::to_points(&src_polygons);

    // build our bounding box
    let bb = BoundingBox::from_points(&pp);

    // get all x coordinates
    let mut xx: Vec<Coord> = Vec::with_capacity(pp.len());
    for p in &pp {
        xx.push(p.x);
    }
    orca_stl::sort(&mut xx);

    // find trapezoids by looping from first to next-to-last coordinate
    let mut rectangle: Polygons = vec![Polygon::default()];
    for i in 0..xx.len().saturating_sub(1) {
        let x = xx[i];
        let next_x = xx[i + 1];
        if x != next_x {
            // intersect with rectangle; append results to return value
            rectangle[0] = Polygon::new(vec![
                Point::new(x, bb.min.y),
                Point::new(next_x, bb.min.y),
                Point::new(next_x, bb.max.y),
                Point::new(x, bb.max.y),
            ]);
            polygons.extend(intersection(
                &rectangle,
                &src_polygons,
                ApplySafetyOffset::No,
            ));
        }
    }
}

/// `get_trapezoids2(expoly, polygons, angle)`: [`get_trapezoids2`] of the expolygon rotated by
/// `π/2 - angle` about the origin, every polygon of the list rotated back.
fn get_trapezoids2_rotated(expoly: &ExPolygon, polygons: &mut Polygons, angle: f64) {
    let mut clone = expoly.clone();
    clone.rotate_around(PI / 2. - angle, Point::new(0, 0));
    get_trapezoids2(&clone, polygons);
    for polygon in polygons.iter_mut() {
        polygon.rotate_around(-(PI / 2. - angle), Point::new(0, 0));
    }
}

/// `get_trapezoids3_half(expoly, polygons, spacing)`: the expolygon cut every `spacing` from its
/// least x, each slice's rectangle narrowed by a quarter spacing on both sides and intersected
/// with it, appended.
pub fn get_trapezoids3_half(expoly: &ExPolygon, polygons: &mut Polygons, spacing: f32) {
    // get all points of this ExPolygon
    let pp = polygon::to_points(&expolygon::to_polygons(expoly));

    if pp.is_empty() {
        return;
    }

    // build our bounding box
    let bb = BoundingBox::from_points(&pp);

    // get all x coordinates
    let mut min_x = pp[0].x;
    let mut max_x = pp[0].x;
    let mut xx: Vec<Coord> = Vec::new();
    for p in &pp {
        if min_x > p.x {
            min_x = p.x;
        }
        if max_x < p.x {
            max_x = p.x;
        }
    }
    let half = orca_crt::f32_to_i64(spacing / 2.);
    let step = orca_crt::f32_to_i64(spacing);
    let mut x = min_x;
    if step <= 0 && x < max_x - half {
        // The C++ never leaves this loop.
        not_ported("get_trapezoids3_half", SPACING_NOT_POSITIVE);
    }
    while x < max_x - half {
        xx.push(x);
        x += step;
    }
    xx.push(max_x);

    // find trapezoids by looping from first to next-to-last coordinate
    let src = expolygon::to_polygons(expoly);
    for i in 0..xx.len() - 1 {
        let x = xx[i];
        let next_x = xx[i + 1];
        if x == next_x {
            continue;
        }

        // build rectangle
        let quarter = step / 4;
        let poly = Polygon::new(vec![
            Point::new(x + quarter, bb.min.y),
            Point::new(next_x - quarter, bb.min.y),
            Point::new(next_x - quarter, bb.max.y),
            Point::new(x + quarter, bb.max.y),
        ]);

        // intersect with this expolygon; append results to return value
        polygons.extend(intersection(&[poly], &src, ApplySafetyOffset::No));
    }
}

/// `detect_bridging_direction(floating_edges, overhang_area)`: the bridge direction and the
/// length of its unsupported ends. With no floating edges the overhang is anchored all round:
/// its second principal component, or (1, 0) when it has none.
#[must_use]
pub fn detect_bridging_direction_lines(
    floating_edges: &[Line],
    overhang_area: &[Polygon],
) -> (Vec2d, f64) {
    if floating_edges.is_empty() {
        // consider this area anchored from all sides, pick bridging direction that will likely
        // yield shortest bridges
        let (_pc1, pc2) = compute_principal_components(overhang_area);
        if pc2 == Vec2f::zero() {
            // overhang may be smaller than resolution. In this case, any direction is ok
            return (vec2d(1.0, 0.0), 0.0);
        }
        let n = pc2.normalized();
        return (vec2d(f64::from(n.x()), f64::from(n.y())), 0.0);
    }

    // Overhang is not fully surrounded by anchors, in that case, find such direction that will
    // minimize the number of bridge ends/180turns in the air
    let mut directions: UnorderedMap<f64, Vec2d> = UnorderedMap::new();
    for l in floating_edges {
        let normal = l.normal().cast_f64().normalized();
        let quantized_angle = orca_crt::ceil(orca_crt::atan2(normal.y(), normal.x()) * 1000.0);
        directions.insert(quantized_angle, normal);
    }
    let mut direction_costs: Vec<(Vec2d, f64)> = Vec::new();
    // it is acutally cost of a perpendicular bridge direction - we find the minimal cost and then
    // return the perpendicular dir
    for (_, d) in directions.iter() {
        direction_costs.push((*d, 0.0));
    }

    for l in floating_edges {
        let line = (l.b - l.a).cast_f64();
        for dir_cost in &mut direction_costs {
            // the dot product already contains the length of the line. dir_cost.first is
            // normalized.
            dir_cost.1 += orca_crt::fabs(line.dot(&dir_cost.0));
        }
    }

    let mut result_dir = Vec2d::ones();
    let mut min_cost = f64::MAX;
    for cost in &direction_costs {
        if cost.1 < min_cost {
            // now flip the orientation back and return the direction of the bridge extrusions
            result_dir = vec2d(cost.0.y(), -cost.0.x());
            min_cost = cost.1;
        }
    }

    (result_dir, min_cost)
}

/// `detect_bridging_direction(to_cover, anchors_area)`: of the part of `to_cover` off the
/// anchors, whose floating edges are its outline off the anchors grown by `SCALED_EPSILON`.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn detect_bridging_direction(to_cover: &[Polygon], anchors_area: &[Polygon]) -> (Vec2d, f64) {
    let overhang_area = diff(to_cover, anchors_area, ApplySafetyOffset::No);
    let floating_edges = to_lines_polylines(&diff_pl(
        &polygon::to_polylines(&overhang_area),
        &expand(
            anchors_area,
            SCALED_EPSILON as f32,
            DEFAULT_JOIN_TYPE,
            DEFAULT_MITER_LIMIT,
        ),
    ));
    detect_bridging_direction_lines(&floating_edges, &overhang_area)
}
