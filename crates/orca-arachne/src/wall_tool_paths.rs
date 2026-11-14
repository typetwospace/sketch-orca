// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Arachne/WallToolPaths.hpp lines 1-148 whole and
// WallToolPaths.cpp lines 1-895 whole (but the ARACHNE_STITCH_PATCH_DEBUG block, which is not
// compiled in), of OrcaSlicer v2.4.2, translated on 2026-11-14 by orca-o3-arachne-4. Copyright
// Ultimaker B.V. (2020 and 2022, CuraEngine), with the PrusaSlicer authors' (Prusa Research) and
// the OrcaSlicer authors' changes, under the GNU Affero General Public License, version 3
// (AGPL-3.0; LICENSE).

//! `WallToolPaths` (`Arachne/WallToolPaths.hpp` and `.cpp`): the variable-width walls of an
//! outline. [`WallToolPaths::generate`] prepares the outline for the Voronoi diagram (a triple
//! offset, then [`simplify`], [`fix_self_intersections`], [`remove_degenerate_verts`],
//! [`remove_colinear_edges`], the same two again and [`remove_small_areas`], then a union), makes
//! the beading strategy from the wall settings, runs [`SkeletalTrapezoidation`] and post-processes
//! its toolpaths: [`WallToolPaths::stitch_tool_paths`], [`WallToolPaths::remove_small_lines`],
//! [`WallToolPaths::separate_out_inner_contour`] (the zero-width insets become the inner contour),
//! [`WallToolPaths::simplify_tool_paths`] and [`WallToolPaths::remove_empty_tool_paths`].
//! [`make_paths_params`] reads the wall parameters from the configuration, and
//! [`WallToolPaths::get_region_order`] gives the order constraints between adjacent walls.
//!
//! The outline helpers are file-local functions of the C++ (non-static, so the probe declares and
//! calls them); they are public here. The protected steps are public too, for the same reason.
//! `generate`, `getToolPaths` and `getInnerContour` return the exceptions of the Voronoi
//! construction ([`VoronoiError`]), which the C++ throws out of them.
//!
//! Translated as the C++ is, quirks kept:
//!
//! - `fixSelfIntersections` calls `ClipperLib::SimplifyPolygons`, which returns its result, and
//!   drops it: the polygons are left as they are, after the call (kept, its result dropped).
//! - `removeSmallAreas` without hole removal moves the last kept polygon into its list of small
//!   holes for each small hole whose first point lies in a removed outline, then shrinks the
//!   polygons by one: the last kept polygon goes, not the hole.
//! - `generate` returns early (no inset, or an outline of no area) without marking the toolpaths
//!   generated, so `getToolPaths` and `getInnerContour` run it again each time.
//! - `separateOutInnerContour` decides whether an inset is a contour from the first junction of its
//!   last line that has one.
//! - `getRegionOrder` casts the searching radius and each pair's distance through `float`.
//!
//! The C++ keeps the outline by reference; here it is borrowed for the object's life. The grid of
//! `fixSelfIntersections` points into the polygons it moves; only the indexes of what it finds are
//! read after it is built, so here it is built over a copy taken before the points move. The pairs
//! of `getRegionOrder` are pointers to the input's lines; here they are indexes into the input
//! (the first index of each line, as the probe's `std::find` gives it), in an
//! `ankerl::unordered_dense::set`'s order ([`DenseSet`]). Sorts and `std::min`, `std::max` and
//! `std::clamp` go through `orca-stl`, the maths and the float-to-integer conversions through
//! `orca-crt`, `coord_t` arithmetic wraps.
//!
//! Stops ([`crate::stop`]): `make_paths_params` with a zero option it leaves unset
//! (`paths-params-zero`) or no nozzle (`paths-params-no-nozzle`); a point moved away from a
//! segment of zero length (`self-intersection-zero-segment`); `removeSmallLines` on an empty odd
//! open line (`small-line-empty`); `separateOutInnerContour` with no inset
//! (`inner-contour-no-insets`).

use core::f64::consts::PI;
use std::collections::BTreeMap;

use orca_config::{PrintConfig, PrintObjectConfig};
use orca_geom::clipper::{PolyFillType, area as clipper_area, simplify_polygons};
use orca_geom::clipper_utils::{DEFAULT_JOIN_TYPE, DEFAULT_MITER_LIMIT, offset, union_, union_fill};
use orca_geom::geometry::deg2rad_f32;
use orca_geom::line::Line;
use orca_geom::point::{Coord, Point, SCALING_FACTOR, scaled, scaled_f64, unscale};
use orca_geom::polygon::{Polygon, Polygons, area};
use orca_layer::flow::Flow;
use orca_voronoi::{PolygonsPointIndex, PolygonsPointIndexSegmentLocator, VoronoiError};

use crate::beading::make_strategy;
use crate::dense_set::DenseSet;
use crate::extrusion_junction::ExtrusionJunction;
use crate::extrusion_line::{ExtrusionLine, VariableWidthLines, norm_i64};
use crate::linear_alg_2d::{get_angle_left, point_is_left_of_line};
use crate::polyline_stitcher::{default_snap_distance, stitch};
use crate::skeletal_trapezoidation::SkeletalTrapezoidation;
use crate::skeletal_trapezoidation_graph::shorter_then;
use crate::sparse_line_grid::SparseLineGrid;
use crate::sparse_point_grid::SparsePointGrid;
use crate::stop::{
    INNER_CONTOUR_NO_INSETS, PATHS_PARAMS_NO_NOZZLE, PATHS_PARAMS_ZERO,
    SELF_INTERSECTION_ZERO_SEGMENT, SMALL_LINE_EMPTY, stop,
};

/// `fill_outline_gaps`: WallToolPaths always widens thin features (`print_thin_walls`).
pub const FILL_OUTLINE_GAPS: bool = true;

/// `meshfix_maximum_resolution()`: `scaled<coord_t>(0.5)`.
#[must_use]
pub fn meshfix_maximum_resolution() -> Coord {
    scaled(0.5)
}

/// `meshfix_maximum_deviation()`: `scaled<coord_t>(0.025)`.
#[must_use]
pub fn meshfix_maximum_deviation() -> Coord {
    scaled(0.025)
}

/// `meshfix_maximum_extrusion_area_deviation()`: `scaled<coord_t>(2.)`.
#[must_use]
pub fn meshfix_maximum_extrusion_area_deviation() -> Coord {
    scaled(2.)
}

/// `scaled<coord_t>(v)` of a `float`: `coord_t(v / float(SCALING_FACTOR))`, the division in
/// `float`. It belongs in `orca-geom`, whose `scaled` takes a `double`.
#[must_use]
pub fn scaled_from_f32(v: f32) -> Coord {
    orca_crt::f32_to_i64(v / SCALING_FACTOR as f32)
}

/// `unscale<float>(v)` of a `coord_t`: `float(v) * float(SCALING_FACTOR)`. It belongs in
/// `orca-geom`, whose `unscale` gives a `double`.
#[must_use]
pub fn unscale_f32(v: Coord) -> f32 {
    v as f32 * SCALING_FACTOR as f32
}

/// `WallToolPathsParams`: the wall settings of a layer.
///
/// The C++ leaves the `float`s, the count and the flag uninitialized until `make_paths_params`
/// sets them; here they start at 0 and `false` ([`Default`]), and `make_paths_params` sets each
/// one or stops (`paths-params-zero`). The resolution and the deviation default to
/// [`meshfix_maximum_resolution`] and [`meshfix_maximum_deviation`], as in the C++.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WallToolPathsParams {
    /// `min_bead_width` (mm).
    pub min_bead_width: f32,
    /// `min_feature_size` (mm).
    pub min_feature_size: f32,
    /// `min_length_factor`.
    pub min_length_factor: f32,
    /// `wall_transition_length` (mm).
    pub wall_transition_length: f32,
    /// `wall_transition_angle` (degrees).
    pub wall_transition_angle: f32,
    /// `wall_transition_filter_deviation` (mm).
    pub wall_transition_filter_deviation: f32,
    /// `wall_distribution_count`.
    pub wall_distribution_count: i32,
    /// `is_top_or_bottom_layer`.
    pub is_top_or_bottom_layer: bool,
    /// `wall_maximum_resolution`.
    pub wall_maximum_resolution: Coord,
    /// `wall_maximum_deviation`.
    pub wall_maximum_deviation: Coord,
}

impl Default for WallToolPathsParams {
    fn default() -> Self {
        Self {
            min_bead_width: 0.0,
            min_feature_size: 0.0,
            min_length_factor: 0.0,
            wall_transition_length: 0.0,
            wall_transition_angle: 0.0,
            wall_transition_filter_deviation: 0.0,
            wall_distribution_count: 0,
            is_top_or_bottom_layer: false,
            wall_maximum_resolution: meshfix_maximum_resolution(),
            wall_maximum_deviation: meshfix_maximum_deviation(),
        }
    }
}

/// `make_paths_params(layer_id, print_object_config, print_config)` (WallToolPaths.cpp 26-66): the
/// wall parameters, the percentages taken of the smallest nozzle diameter. An option is true when
/// its value is not 0 (`ConfigOptionSingle`'s `operator T()`): a minimum length factor of 0 falls
/// back to 0.5 and a resolution or deviation of 0 keeps its default, while a zero minimum feature
/// size, minimum bead width (the first layer's on layer 0), transition filter deviation or
/// transition length leaves its field unset in the C++ and stops here (`paths-params-zero`).
#[must_use]
pub fn make_paths_params(
    layer_id: i32,
    print_object_config: &PrintObjectConfig,
    print_config: &PrintConfig,
) -> WallToolPathsParams {
    const F: &str = "Slic3r::Arachne::make_paths_params";
    let mut input_params = WallToolPathsParams::default();
    {
        let nozzles = &print_config.nozzle_diameter.values;
        if nozzles.is_empty() {
            stop(F, PATHS_PARAMS_NO_NOZZLE);
        }
        let min_nozzle_diameter = nozzles[orca_stl::min_element_f64(nozzles)];
        let percent_of_nozzle = |value: f64| (value * 0.01 * min_nozzle_diameter) as f32;
        let poc = print_object_config;

        if poc.min_feature_size.value != 0.0 {
            input_params.min_feature_size = percent_of_nozzle(poc.min_feature_size.value);
        } else {
            stop(F, PATHS_PARAMS_ZERO);
        }

        if poc.min_length_factor.value != 0.0 {
            input_params.min_length_factor = poc.min_length_factor.value as f32;
        } else {
            input_params.min_length_factor = 0.5;
        }

        let min_bead_width = if layer_id == 0 {
            poc.initial_layer_min_bead_width.value
        } else {
            poc.min_bead_width.value
        };
        if min_bead_width != 0.0 {
            input_params.min_bead_width = percent_of_nozzle(min_bead_width);
        } else {
            stop(F, PATHS_PARAMS_ZERO);
        }

        if poc.wall_transition_filter_deviation.value != 0.0 {
            input_params.wall_transition_filter_deviation =
                percent_of_nozzle(poc.wall_transition_filter_deviation.value);
        } else {
            stop(F, PATHS_PARAMS_ZERO);
        }

        if poc.wall_transition_length.value != 0.0 {
            input_params.wall_transition_length =
                percent_of_nozzle(poc.wall_transition_length.value);
        } else {
            stop(F, PATHS_PARAMS_ZERO);
        }

        input_params.wall_transition_angle = poc.wall_transition_angle.value as f32;
        input_params.wall_distribution_count = poc.wall_distribution_count.value;

        input_params.is_top_or_bottom_layer = false; // Set to default value

        if poc.wall_maximum_resolution.value != 0.0 {
            input_params.wall_maximum_resolution = scaled(poc.wall_maximum_resolution.value);
        }

        if poc.wall_maximum_deviation.value != 0.0 {
            input_params.wall_maximum_deviation = scaled(poc.wall_maximum_deviation.value);
        }
    }

    input_params
}

/// `WallToolPaths::ExtrusionLineSet`: the order constraints, each pair the indexes of two lines
/// of the input, in the set's order.
pub type ExtrusionLineSet = DenseSet<(usize, usize)>;

/// `LocToLineGrid`: `SparseLineGrid<PolygonsPointIndex, PolygonsPointIndexSegmentLocator>`.
pub type LocToLineGrid<'a> = SparseLineGrid<PolygonsPointIndex<'a>, PolygonsPointIndexSegmentLocator>;

/// `simplify(thiss, smallest_line_segment_squared, allowed_error_distance_squared)` of a polygon
/// (WallToolPaths.cpp 86-201): fewer than three points are cleared, three kept; otherwise each
/// vertex is removed when its segment is under 5 microns, when it is (nearly) collinear, or when
/// its segment is shorter than the smallest segment and removing it errs less than allowed; a
/// vertex before a long segment may move to the intersection of its neighbours' lines instead.
pub fn simplify_polygon(
    thiss: &mut Polygon,
    smallest_line_segment_squared: i64,
    allowed_error_distance_squared: i64,
) {
    if thiss.points.len() < 3 {
        thiss.points.clear();
        return;
    }
    if thiss.points.len() == 3 {
        return;
    }

    let five_microns = scaled(0.005);
    let five_microns_squared = five_microns.wrapping_mul(five_microns);
    let cross = |a: Point, b: Point| a.x.wrapping_mul(b.y).wrapping_sub(a.y.wrapping_mul(b.x));

    let mut new_path = Polygon::default();
    let n = thiss.points.len();
    let mut previous = thiss.points[n - 1];
    let mut previous_previous = thiss.points[n - 2];
    let mut current = thiss.points[0];

    // When removing a vertex, we check the height of the triangle of the area being removed from
    // the original polygon by the simplification. However, when consecutively removing multiple
    // vertices the height of the previously removed vertices w.r.t. the shortcut path changes.
    // In order to not recompute the new height value of previously removed vertices we compute
    // the height of a representative triangle, which covers the same amount of area as the area
    // being cut off. We use the Shoelace formula to accumulate the area under the removed
    // segments. This works by computing the area in a 'fan' where each of the blades of the fan
    // go from the origin to one of the segments. While removing vertices the area in this fan
    // accumulates. By subtracting the area of the blade connected to the short-cutting segment
    // we obtain the total area of the cutoff region. From this area we compute the height of the
    // representative triangle using the standard formula for a triangle area: A = .5*b*h
    let mut accumulated_area_removed = cross(previous, current); // Twice the Shoelace formula for area of polygon per line segment.

    for point_idx in 0..n {
        current = thiss.points[point_idx % n];

        //Check if the accumulated area doesn't exceed the maximum.
        let next = if point_idx + 1 < n {
            thiss.points[point_idx + 1]
        } else if point_idx + 1 == n && new_path.points.len() > 1 {
            // don't spill over if the [next] vertex will then be equal to [previous]
            new_path.points[0] //Spill over to new polygon for checking removed area.
        } else {
            thiss.points[(point_idx + 1) % n]
        };
        let removed_area_next = cross(current, next); // Twice the Shoelace formula for area of polygon per line segment.
        let negative_area_closing = cross(next, previous); // area between the origin and the short-cutting segment
        accumulated_area_removed = accumulated_area_removed.wrapping_add(removed_area_next);

        let length2 = (current - previous).squared_norm();

        // Orca:
        // Checking if the segment's length is smaller than 5 microns (0.005mm).
        // The value of `length2` is scaled and squared, so we need to compare it with the squared
        // value of 5 microns
        if length2 < five_microns_squared {
            // We're allowed to always delete segments of less than 5 micron.
            continue;
        }

        let area_removed_so_far = accumulated_area_removed.wrapping_add(negative_area_closing); // close the shortcut area polygon
        let base_length_2 = (next - previous).squared_norm();

        if base_length_2 == 0 {
            //Two line segments form a line back and forth with no area.
            continue; //Remove the vertex.
        }
        //We want to check if the height of the triangle formed by previous, current and next
        //vertices is less than allowed_error_distance_squared.
        //1/2 L = A           [actual area is half of the computed shoelace value] // Shoelace formula is .5*(...) , but we simplify the computation and take out the .5
        //A = 1/2 * b * h     [triangle area formula]
        //L = b * h           [apply above two and take out the 1/2]
        //h = L / b           [divide by b]
        //h^2 = (L / b)^2     [square it]
        //h^2 = L^2 / b^2     [factor the divisor]
        let height_2 = orca_crt::f64_to_i64(
            area_removed_so_far as f64 * area_removed_so_far as f64 / base_length_2 as f64,
        );
        // Orca: The value of `height_2` is squared, so we need to compare it with the squared value
        if height_2 <= five_microns_squared //Almost exactly colinear (barring rounding errors).
            && Line::new(previous, next).distance_to_infinite(current) <= scaled_f64(0.005)
        {
            // make sure that height_2 is not small because of cancellation of positive and
            // negative areas
            continue;
        }

        if length2 < smallest_line_segment_squared
            && height_2 <= allowed_error_distance_squared
        {
            // removing the vertex doesn't introduce too much error.)
            let next_length2 = (current - next).squared_norm();
            if next_length2 > smallest_line_segment_squared.wrapping_mul(4) {
                // Special case; The next line is long. If we were to remove this, it could happen
                // that we get quite noticeable artifacts. We should instead move this point to a
                // location where both edges are kept and then remove the previous point that we
                // wanted to keep. By taking the intersection of these two lines, we get a point
                // that preserves the direction (so it makes the corner a bit more pointy). We
                // just need to be sure that the intersection point does not introduce an artifact
                // itself.
                let intersection_point = Line::new(previous_previous, previous)
                    .intersection_infinite(&Line::new(current, next));
                match intersection_point {
                    Some(intersection_point)
                        if !(Line::new(previous, current)
                            .distance_to_infinite_squared(intersection_point)
                            .0
                            > allowed_error_distance_squared as f64
                            || (intersection_point - previous).squared_norm()
                                > smallest_line_segment_squared // The intersection point is way too far from the 'previous'
                            || (intersection_point - next).squared_norm()
                                > smallest_line_segment_squared) =>
                    {
                        // and 'next' points, so it shouldn't replace 'current'
                        // New point seems like a valid one.
                        current = intersection_point;
                        // If there was a previous point added, remove it.
                        if !new_path.points.is_empty() {
                            new_path.points.pop();
                            previous = previous_previous;
                        }
                    }
                    _ => {
                        // We can't find a better spot for it, but the size of the line is more
                        // than 5 micron. So the only thing we can do here is leave it in...
                    }
                }
            } else {
                continue; //Remove the vertex.
            }
        }
        //Don't remove the vertex.
        accumulated_area_removed = removed_area_next; // so that in the next iteration it's the area between the origin, [previous] and [current]
        previous_previous = previous;
        previous = current; //Note that "previous" is only updated if we don't remove the vertex.
        new_path.points.push(current);
    }

    *thiss = new_path;
}

/// `simplify(thiss, smallest_line_segment, allowed_error_distance)` of polygons (WallToolPaths.cpp
/// 232-245; the C++'s defaults, 0.01 mm and 5 microns, are not used by its callers): each polygon
/// simplified, those left with fewer than three points removed.
pub fn simplify(thiss: &mut Polygons, smallest_line_segment: i64, allowed_error_distance: i64) {
    let allowed_error_distance_squared = allowed_error_distance.wrapping_mul(allowed_error_distance);
    let smallest_line_segment_squared = smallest_line_segment.wrapping_mul(smallest_line_segment);
    let mut p: usize = 0;
    while p < thiss.len() {
        simplify_polygon(
            &mut thiss[p],
            smallest_line_segment_squared,
            allowed_error_distance_squared,
        );
        if thiss[p].points.len() < 3 {
            thiss.remove(p);
            p = p.wrapping_sub(1);
        }
        p = p.wrapping_add(1);
    }
}

/// `createLocToLineGrid(polygons, square_size)` (WallToolPaths.cpp 247-260): a grid of every
/// segment of the polygons, each by its polygon and point index, reserved for the point count (an
/// `unsigned int`).
#[must_use]
pub fn create_loc_to_line_grid(polygons: &Polygons, square_size: i32) -> LocToLineGrid<'_> {
    let mut n_points: u32 = 0;
    for poly in polygons {
        n_points = n_points.wrapping_add(poly.points.len() as u32);
    }

    let mut ret = SparseLineGrid::new(
        Coord::from(square_size),
        n_points as usize,
        1.0,
        PolygonsPointIndexSegmentLocator,
    );

    let mut poly_idx: u32 = 0;
    while (poly_idx as usize) < polygons.len() {
        let mut point_idx: u32 = 0;
        while (point_idx as usize) < polygons[poly_idx as usize].points.len() {
            ret.insert(&PolygonsPointIndex::new(polygons, poly_idx, point_idx));
            point_idx = point_idx.wrapping_add(1);
        }
        poly_idx = poly_idx.wrapping_add(1);
    }
    ret
}

/// `ClipperLib::SimplifyPolygons(ClipperUtils::PolygonsProvider(thiss), pftEvenOdd)`, whose
/// result `fixSelfIntersections` drops.
fn simplify_polygons_dropped(thiss: &Polygons) {
    let _ = simplify_polygons(
        thiss.iter().map(|p| p.points.as_slice()),
        PolyFillType::EvenOdd,
        true,
    );
}

/// `fixSelfIntersections(epsilon, thiss)` (WallToolPaths.cpp 262-309): each point within half of
/// `epsilon` of another segment is moved away from it, perpendicular to it, by
/// `max(2, half_epsilon - 2)`; below an `epsilon` of 1 nothing moves. Either way it ends with
/// Clipper's even-odd simplification, whose result it drops.
pub fn fix_self_intersections(epsilon: Coord, thiss: &mut Polygons) {
    const F: &str = "Slic3r::Arachne::fixSelfIntersections";
    if epsilon < 1 {
        simplify_polygons_dropped(thiss);
        return;
    }

    let half_epsilon: i64 = epsilon.wrapping_add(1) / 2;

    // Points too close to line segments should be moved a little away from those line segments,
    // but less than epsilon, so at least half-epsilon distance between points can still be
    // guaranteed.
    let grid_size: Coord = scaled(2.);
    // The grid of the polygons before any point moves: only the indexes it finds are read.
    let snapshot = thiss.clone();
    let query_grid = create_loc_to_line_grid(&snapshot, grid_size as i32);

    let move_dist: i64 = orca_stl::max(2, half_epsilon.wrapping_sub(2));
    let half_epsilon_sqrd: i64 = half_epsilon.wrapping_mul(half_epsilon);

    let n = thiss.len();
    for poly_idx in 0..n {
        let pathlen = thiss[poly_idx].points.len();
        for point_idx in 0..pathlen {
            let nearby = query_grid.get_nearby(thiss[poly_idx].points[point_idx], epsilon);
            for line in &nearby {
                let line_poly_idx = line.poly_idx as usize;
                let line_point_idx = line.point_idx as usize;
                let line_next_idx =
                    line.point_idx.wrapping_add(1) as usize % thiss[line_poly_idx].points.len();
                if poly_idx == line_poly_idx
                    && (point_idx == line_point_idx || point_idx == line_next_idx)
                {
                    continue;
                }

                let segment = Line::new(
                    thiss[line_poly_idx].points[line_point_idx],
                    thiss[line_poly_idx].points[line_next_idx],
                );
                let pt = thiss[poly_idx].points[point_idx];
                let (_, segment_closest_point) = segment.distance_to_squared_closest(pt);

                if half_epsilon_sqrd >= (pt - segment_closest_point).squared_norm() {
                    let other = thiss[poly_idx].points[(point_idx + 1) % pathlen];
                    let vec = if point_is_left_of_line(other, segment.a, segment.b) > 0 {
                        segment.b - segment.a
                    } else {
                        segment.a - segment.b
                    };
                    let len: i64 = norm_i64(vec);
                    if len == 0 {
                        stop(F, SELF_INTERSECTION_ZERO_SEGMENT);
                    }
                    let pt = &mut thiss[poly_idx].points[point_idx];
                    pt.x = pt
                        .x
                        .wrapping_add(vec.y.wrapping_neg().wrapping_mul(move_dist).wrapping_div(len));
                    pt.y = pt
                        .y
                        .wrapping_add(vec.x.wrapping_mul(move_dist).wrapping_div(len));
                }
            }
        }
    }

    simplify_polygons_dropped(thiss);
}

/// `isDegenerate(last, now, next)` of `removeDegenerateVerts`: the two segments point in opposite
/// directions (their dot product is minus the product of their `int64_t` norms).
fn is_degenerate(last: Point, now: Point, next: Point) -> bool {
    let last_line = now - last;
    let next_line = next - now;
    last_line.dot(next_line)
        == (-1_i64)
            .wrapping_mul(norm_i64(last_line))
            .wrapping_mul(norm_i64(next_line))
}

/// `removeDegenerateVerts(thiss)` (WallToolPaths.cpp 311-351): removes overlapping consecutive
/// line segments which don't delimit a positive area; a polygon left with two points or fewer is
/// removed.
pub fn remove_degenerate_verts(thiss: &mut Polygons) {
    let mut poly_idx: usize = 0;
    while poly_idx < thiss.len() {
        let poly = &thiss[poly_idx];
        let mut result: Vec<Point> = Vec::new();

        let mut is_changed = false;
        let size = poly.points.len();
        for idx in 0..size {
            let last = match result.last() {
                None => poly.points[size - 1],
                Some(&p) => p,
            };
            if idx + 1 == size && result.is_empty() {
                break;
            }

            let next = if idx + 1 == size {
                result[0]
            } else {
                poly.points[idx + 1]
            };
            if is_degenerate(last, poly.points[idx], next) {
                // lines are in the opposite direction
                // don't add vert to the result
                is_changed = true;
                while result.len() > 1
                    && is_degenerate(result[result.len() - 2], result[result.len() - 1], next)
                {
                    result.pop();
                }
            } else {
                result.push(poly.points[idx]);
            }
        }

        if is_changed {
            if result.len() > 2 {
                thiss[poly_idx].points = result;
            } else {
                thiss.remove(poly_idx);
                poly_idx = poly_idx.wrapping_sub(1); // effectively the next iteration has the same poly_idx (referring to a new poly which is not yet processed)
            }
        }
        poly_idx = poly_idx.wrapping_add(1);
    }
}

/// `removeSmallAreas(thiss, min_area_size, remove_holes)` (WallToolPaths.cpp 353-405): polygons
/// of an absolute area (`ClipperLib::Area`) below `min_area_size` are removed, each replaced by
/// the last one still kept, which is checked next. Without `remove_holes` only small outlines go
/// (swapped to the end); then, for each small hole (from the last) whose first point lies in a
/// removed outline, the C++ moves the last kept polygon into its list of small holes and keeps
/// one polygon fewer: the last kept polygon goes, not the hole (kept as it is).
pub fn remove_small_areas(thiss: &mut Polygons, min_area_size: f64, remove_holes: bool) {
    let mut new_end = thiss.len();
    if remove_holes {
        let mut it = 0;
        while it < new_end {
            // All polygons smaller than target are removed by replacing them with a polygon from
            // the back of the vector.
            if orca_crt::fabs(clipper_area(&thiss[it].points)) < min_area_size {
                new_end -= 1;
                thiss.swap(it, new_end);
                continue; // Don't increment the iterator such that the polygon just swapped in is checked next.
            }
            it += 1;
        }
    } else {
        // For each polygon, computes the signed area, move small outlines at the end of the vector
        // and keep pointer on small holes
        let mut small_holes: Polygons = Vec::new();
        let mut it = 0;
        while it < new_end {
            let area = clipper_area(&thiss[it].points);
            if orca_crt::fabs(area) < min_area_size {
                if area >= 0.0 {
                    new_end -= 1;
                    if it < new_end {
                        thiss.swap(new_end, it);
                        continue;
                    }
                    // Don't self-swap the last Path
                    break;
                }
                small_holes.push(thiss[it].clone());
            }
            it += 1;
        }

        // Removes small holes that have their first point inside one of the removed outlines
        // Iterating in reverse ensures that unprocessed small holes won't be moved
        let removed_outlines_start = new_end;
        for hole_idx in (0..small_holes.len()).rev() {
            for outline_idx in removed_outlines_start..thiss.len() {
                if thiss[outline_idx].contains(small_holes[hole_idx].points[0]) {
                    new_end -= 1;
                    small_holes[hole_idx] = core::mem::take(&mut thiss[new_end]);
                    break;
                }
            }
        }
    }
    thiss.truncate(new_end);
}

/// `removeColinearEdges(poly, max_deviation_angle)` (WallToolPaths.cpp 407-469): vertices whose
/// segments are nearly parallel (the angle within `max_deviation_angle` of 0 or pi, `getAngleLeft`
/// in `float`) are removed, the vertex after each removed one skipped in that pass, the passes
/// repeated until one removes nothing; a polygon of three points or fewer is left.
pub fn remove_colinear_edges_polygon(poly: &mut Polygon, max_deviation_angle: f64) {
    // TODO: Can be made more efficient (for example, use pointer-types for process-/skip-indices,
    // so we can swap them without copy).
    let mut num_removed_in_iteration: usize;
    loop {
        num_removed_in_iteration = 0;
        let mut process_indices: Vec<bool> = vec![true; poly.points.len()];

        let mut go = true;
        while go {
            go = false;

            let rpath = &poly.points;
            let pathlen = rpath.len();
            if pathlen <= 3 {
                return;
            }

            let mut skip_indices: Vec<bool> = vec![false; poly.points.len()];

            let mut new_path: Vec<Point> = Vec::new();
            let mut point_idx = 0;
            while point_idx < pathlen {
                // Don't iterate directly over process-indices, but do it this way, because there
                // are points _in_ process-indices that should nonetheless be skipped:
                if !process_indices[point_idx] {
                    new_path.push(rpath[point_idx]);
                    point_idx += 1;
                    continue;
                }

                // Should skip the last point for this iteration if the old first was removed
                // (which can be seen from the fact that the new first was skipped):
                if point_idx == (pathlen - 1) && skip_indices[0] {
                    skip_indices[new_path.len()] = true;
                    go = true;
                    new_path.push(rpath[point_idx]);
                    break;
                }

                let prev = rpath[(point_idx.wrapping_sub(1).wrapping_add(pathlen)) % pathlen];
                let pt = rpath[point_idx];
                let next = rpath[(point_idx + 1) % pathlen];

                let mut angle: f32 = get_angle_left(prev, pt, next); // [0 : 2 * pi]
                if angle >= PI as f32 {
                    angle -= PI as f32; // map [pi : 2 * pi] to [0 : pi]
                }

                // Check if the angle is within limits for the point to 'make sense', given the
                // maximum deviation. If the angle indicates near-parallel segments ignore the
                // point 'pt'
                if f64::from(angle) > max_deviation_angle
                    && f64::from(angle) < PI - max_deviation_angle
                {
                    new_path.push(pt);
                } else if point_idx != (pathlen - 1) {
                    // Skip the next point, since the current one was removed:
                    skip_indices[new_path.len()] = true;
                    go = true;
                    new_path.push(next);
                    point_idx += 1;
                }
                point_idx += 1;
            }
            poly.points = new_path;
            num_removed_in_iteration =
                num_removed_in_iteration.wrapping_add(pathlen.wrapping_sub(poly.points.len()));

            process_indices = skip_indices;
        }
        if num_removed_in_iteration == 0 {
            break;
        }
    }
}

/// `removeColinearEdges(thiss, max_deviation_angle = 0.0005)` (WallToolPaths.cpp 471-480): each
/// polygon's nearly collinear vertices removed, those left with fewer than three points removed.
pub fn remove_colinear_edges(thiss: &mut Polygons, max_deviation_angle: f64) {
    let mut p: i32 = 0;
    while p < thiss.len() as i32 {
        remove_colinear_edges_polygon(&mut thiss[p as usize], max_deviation_angle);
        if thiss[p as usize].points.len() < 3 {
            thiss.remove(p as usize);
            p -= 1;
        }
        p += 1;
    }
}

/// `shorterThan(shape, check_length)` (WallToolPaths.cpp 671-682) of a line: whether its length
/// round the loop (from the last junction to the first, then along) stays under `check_length`.
/// A line with no junction stops (`small-line-empty`).
fn shorter_than(shape: &ExtrusionLine, check_length: Coord) -> bool {
    let Some(&back) = shape.junctions.last() else {
        stop("Slic3r::Arachne::shorterThan", SMALL_LINE_EMPTY);
    };
    let mut p0: ExtrusionJunction = back;
    let mut length: i64 = 0;
    for p1 in &shape.junctions {
        length = length.wrapping_add(norm_i64(p0.p - p1.p));
        if length >= check_length {
            return false;
        }
        p0 = *p1;
    }
    true
}

/// `WallToolPaths`: the walls of an outline as variable-width toolpaths.
#[derive(Debug)]
pub struct WallToolPaths<'a> {
    /// `outline`: the outline of the area the toolpaths are generated in.
    outline: &'a Polygons,
    /// `bead_width_0`: the nominal or first extrusion line width.
    bead_width_0: Coord,
    /// `bead_width_x`: the width of the inner walls.
    bead_width_x: Coord,
    /// `inset_count`: the maximum number of walls to generate.
    inset_count: usize,
    /// `wall_0_inset`: how far to inset the outer wall.
    wall_0_inset: Coord,
    /// `layer_height`.
    layer_height: f64,
    /// `print_thin_walls`: whether the widening meta-strategy is on for thin features.
    print_thin_walls: bool,
    /// `min_feature_size`: features thinner than this are not printed.
    min_feature_size: Coord,
    /// `min_bead_width`: the smallest bead when widening thin features.
    min_bead_width: Coord,
    /// `small_area_length`: the length of the small features filtered out, squared into an area.
    small_area_length: f64,
    /// `wall_transition_filter_deviation`: the line width deviation the filtering may make.
    wall_transition_filter_deviation: Coord,
    /// `toolpaths_generated`.
    toolpaths_generated: bool,
    /// `toolpaths`: the generated toolpaths.
    toolpaths: Vec<VariableWidthLines>,
    /// `inner_contour`: the inner contour of the generated toolpaths.
    inner_contour: Polygons,
    /// `m_params`.
    m_params: WallToolPathsParams,
}

impl<'a> WallToolPaths<'a> {
    /// `WallToolPaths(outline, bead_width_0, bead_width_x, inset_count, wall_0_inset,
    /// layer_height, params)` (WallToolPaths.cpp 68-84): the minimum feature size, minimum bead
    /// width and filter deviation scaled from the parameters' `float`s, the small area length
    /// half the first width.
    #[must_use]
    pub fn new(
        outline: &'a Polygons,
        bead_width_0: Coord,
        bead_width_x: Coord,
        inset_count: usize,
        wall_0_inset: Coord,
        layer_height: f64,
        params: &WallToolPathsParams,
    ) -> Self {
        Self {
            outline,
            bead_width_0,
            bead_width_x,
            inset_count,
            wall_0_inset,
            layer_height,
            print_thin_walls: FILL_OUTLINE_GAPS,
            min_feature_size: scaled_from_f32(params.min_feature_size),
            min_bead_width: scaled_from_f32(params.min_bead_width),
            small_area_length: bead_width_0 as f64 / 2.,
            wall_transition_filter_deviation: scaled_from_f32(
                params.wall_transition_filter_deviation,
            ),
            toolpaths_generated: false,
            toolpaths: Vec::new(),
            inner_contour: Polygons::new(),
            m_params: *params,
        }
    }

    /// `generate()` (WallToolPaths.cpp 482-571): the toolpaths, as the module's header says. With
    /// no inset, or when the prepared outline has no area, it returns the toolpaths as they are,
    /// not marked generated.
    ///
    /// # Errors
    ///
    /// The exceptions of the Voronoi construction, which the C++ throws out of it.
    pub fn generate(&mut self) -> Result<&Vec<VariableWidthLines>, VoronoiError> {
        if self.inset_count < 1 {
            return Ok(&self.toolpaths);
        }

        let smallest_segment = self.m_params.wall_maximum_resolution;
        let allowed_distance = self.m_params.wall_maximum_deviation;
        let epsilon_offset = (allowed_distance / 2).wrapping_sub(1);
        let transitioning_angle = f64::from(deg2rad_f32(self.m_params.wall_transition_angle));
        let discretization_step_size = scaled(0.8);

        // Simplify outline for boost::voronoi consumption. Absolutely no self intersections or
        // near-self intersections allowed:
        // TODO: Open question: Does this indeed fix all (or all-but-one-in-a-million) cases for
        // manifold but otherwise possibly complex polygons?
        let mut prepared_outline = offset(
            &offset(
                &offset(
                    self.outline,
                    epsilon_offset.wrapping_neg() as f32,
                    DEFAULT_JOIN_TYPE,
                    DEFAULT_MITER_LIMIT,
                ),
                epsilon_offset.wrapping_mul(2) as f32,
                DEFAULT_JOIN_TYPE,
                DEFAULT_MITER_LIMIT,
            ),
            epsilon_offset.wrapping_neg() as f32,
            DEFAULT_JOIN_TYPE,
            DEFAULT_MITER_LIMIT,
        );
        simplify(&mut prepared_outline, smallest_segment, allowed_distance);
        fix_self_intersections(epsilon_offset, &mut prepared_outline);
        remove_degenerate_verts(&mut prepared_outline);
        remove_colinear_edges(&mut prepared_outline, 0.005);
        // Removing collinear edges may introduce self intersections, so we need to fix them again
        fix_self_intersections(epsilon_offset, &mut prepared_outline);
        remove_degenerate_verts(&mut prepared_outline);
        remove_small_areas(
            &mut prepared_outline,
            self.small_area_length * self.small_area_length,
            false,
        );

        // The functions above could produce intersecting polygons that could cause a crash inside
        // Arachne. Applying Clipper union should be enough to get rid of this issue. Clipper union
        // also fixed an issue in Arachne that in post-processing Voronoi diagram, some edges
        // didn't have twin edges. (a non-planar Voronoi diagram probably caused this).
        prepared_outline = union_(&prepared_outline);

        if area(&prepared_outline) <= 0.0 {
            return Ok(&self.toolpaths);
        }

        let external_perimeter_extrusion_width = Flow::rounded_rectangle_extrusion_width_from_spacing(
            unscale_f32(self.bead_width_0),
            self.layer_height as f32,
        );
        let perimeter_extrusion_width = Flow::rounded_rectangle_extrusion_width_from_spacing(
            unscale_f32(self.bead_width_x),
            self.layer_height as f32,
        );

        let wall_transition_length = scaled_from_f32(self.m_params.wall_transition_length);

        // For an uneven nr. of lines: When to split the middle wall into two.
        let wall_split_middle_threshold = orca_stl::clamp(
            2. * unscale(self.min_bead_width) / f64::from(external_perimeter_extrusion_width) - 1.,
            0.01,
            0.99,
        );
        // For an even nr. of lines: When to add a new middle in between the innermost two walls.
        let wall_add_middle_threshold = orca_stl::clamp(
            unscale(self.min_bead_width) / f64::from(perimeter_extrusion_width),
            0.01,
            0.99,
        );

        let wall_distribution_count = self.m_params.wall_distribution_count;
        let max_bead_count: usize = if self.inset_count < (Coord::MAX / 2) as usize {
            self.inset_count.wrapping_mul(2)
        } else {
            Coord::MAX as usize
        };
        let beading_strat = make_strategy(
            self.bead_width_0,
            self.bead_width_x,
            wall_transition_length,
            transitioning_angle as f32,
            self.print_thin_walls,
            self.min_bead_width,
            self.min_feature_size,
            wall_split_middle_threshold,
            wall_add_middle_threshold,
            max_bead_count as Coord,
            self.wall_0_inset,
            wall_distribution_count,
            0.5,
        );
        let transition_filter_dist = scaled_from_f32(100.0);
        let allowed_filter_deviation = self.wall_transition_filter_deviation;
        let mut wall_maker = SkeletalTrapezoidation::new(
            &prepared_outline,
            beading_strat.as_ref(),
            beading_strat.get_transitioning_angle(),
            discretization_step_size,
            transition_filter_dist,
            allowed_filter_deviation,
            wall_transition_length,
        )?;
        wall_maker.generate_toolpaths(&mut self.toolpaths, false);
        drop(wall_maker);

        Self::stitch_tool_paths(&mut self.toolpaths, self.bead_width_x);

        let mut toolpaths = core::mem::take(&mut self.toolpaths);
        self.remove_small_lines(&mut toolpaths);
        self.toolpaths = toolpaths;

        self.separate_out_inner_contour();

        Self::simplify_tool_paths(&mut self.toolpaths, &self.m_params);

        Self::remove_empty_tool_paths(&mut self.toolpaths);
        self.toolpaths_generated = true;
        Ok(&self.toolpaths)
    }

    /// `stitchToolPaths(toolpaths, bead_width_x)` (WallToolPaths.cpp 573-669, protected): each
    /// inset's lines stitched (`PolylineStitcher::stitch`, at most a line width less 1 apart),
    /// the open ones kept and the closed ones appended, closed again when their ends are apart by
    /// less than the stitch distance.
    pub fn stitch_tool_paths(toolpaths: &mut [VariableWidthLines], bead_width_x: Coord) {
        let stitch_distance = bead_width_x.wrapping_sub(1); //In 0-width contours, junctions can cause up to 1-line-width gaps. Don't stitch more than 1 line width.

        for wall_lines in toolpaths.iter_mut() {
            let mut stitched_polylines: VariableWidthLines = Vec::new();
            let mut closed_polygons: VariableWidthLines = Vec::new();
            stitch(
                wall_lines,
                &mut stitched_polylines,
                &mut closed_polygons,
                stitch_distance,
                default_snap_distance(),
            );
            *wall_lines = stitched_polylines; // replace input toolpaths with stitched polylines

            for mut wall_polygon in closed_polygons {
                let (Some(&front), Some(&back)) =
                    (wall_polygon.junctions.first(), wall_polygon.junctions.last())
                else {
                    continue;
                };

                // PolylineStitcher, in some cases, produced closed extrusion (polygons),
                // but the endpoints differ by a small distance. So we reconnect them.
                // FIXME Lukas H.: Investigate more deeply why it is happening.
                if front.p != back.p && (back.p - front.p).cast_f64().norm() < stitch_distance as f64
                {
                    wall_polygon.junctions.push(front);
                }
                wall_polygon.is_closed = true;
                wall_lines.push(wall_polygon); // add stitched polygons to result
            }
        }
    }

    /// `removeSmallLines(toolpaths)` (WallToolPaths.cpp 684-700, protected): odd open lines
    /// shorter than their smallest width times the minimum length factor (half of it on a top or
    /// bottom layer; the `float` truncated to `coord_t`) are removed, each replaced by the inset's
    /// last line, which is checked next.
    pub fn remove_small_lines(&self, toolpaths: &mut [VariableWidthLines]) {
        for inset in toolpaths.iter_mut() {
            let mut line_idx: usize = 0;
            while line_idx < inset.len() {
                let line = &inset[line_idx];
                let mut min_width = Coord::MAX;
                for j in &line.junctions {
                    min_width = orca_stl::min(min_width, j.w);
                }
                // Only use min_length_factor for non-topmost, to prevent top gaps. Otherwise use
                // default value.
                if line.is_odd && !line.is_closed && {
                    let check_length: f32 = if self.m_params.is_top_or_bottom_layer {
                        (min_width / 2) as f32
                    } else {
                        min_width as f32 * self.m_params.min_length_factor
                    };
                    shorter_than(line, orca_crt::f32_to_i64(check_length))
                } {
                    // remove line
                    inset.swap_remove(line_idx);
                    line_idx = line_idx.wrapping_sub(1); // reconsider the current position
                }
                line_idx = line_idx.wrapping_add(1);
            }
        }
    }

    /// `simplifyToolPaths(toolpaths, params)` (WallToolPaths.cpp 702-719, protected): every line
    /// simplified with the parameters' resolution and deviation, squared, and an area deviation
    /// of 2 mm.
    pub fn simplify_tool_paths(toolpaths: &mut [VariableWidthLines], params: &WallToolPathsParams) {
        let maximum_resolution: i64 = params.wall_maximum_resolution;
        let maximum_deviation: i64 = params.wall_maximum_deviation;

        let smallest_line_segment_squared = maximum_resolution.wrapping_mul(maximum_resolution);
        let allowed_error_distance_squared = maximum_deviation.wrapping_mul(maximum_deviation);

        let maximum_extrusion_area_deviation = meshfix_maximum_extrusion_area_deviation(); // unit: μm²

        for lines in toolpaths.iter_mut() {
            for line in lines.iter_mut() {
                line.simplify(
                    smallest_line_segment_squared,
                    allowed_error_distance_squared,
                    maximum_extrusion_area_deviation,
                );
            }
        }
    }

    /// `getToolPaths()` (WallToolPaths.cpp 721-726): the toolpaths, generated first unless they
    /// have been.
    ///
    /// # Errors
    ///
    /// The exceptions of [`WallToolPaths::generate`].
    pub fn get_tool_paths(&mut self) -> Result<&Vec<VariableWidthLines>, VoronoiError> {
        if !self.toolpaths_generated {
            return self.generate();
        }
        Ok(&self.toolpaths)
    }

    /// `separateOutInnerContour()` (WallToolPaths.cpp 728-777): the insets whose last line with a
    /// junction starts with a zero width are taken out, their closed even lines kept as the inner
    /// contour (united under the even-odd rule); the rest stay the toolpaths. With no inset it
    /// stops (`inner-contour-no-insets`: the C++ divides by the inset count).
    pub fn separate_out_inner_contour(&mut self) {
        //We'll remove all 0-width paths from the original toolpaths and store them separately as
        //polygons.
        let mut actual_toolpaths: Vec<VariableWidthLines> = Vec::with_capacity(self.toolpaths.len()); //A bit too much, but the correct order of magnitude.
        // contour_paths.reserve(toolpaths.size() / inset_count): never used past its reserve.
        if self.inset_count == 0 {
            stop(
                "WallToolPaths::separateOutInnerContour",
                INNER_CONTOUR_NO_INSETS,
            );
        }
        self.inner_contour.clear();
        for inset in &self.toolpaths {
            if inset.is_empty() {
                continue;
            }
            let mut is_contour = false;
            for line in inset {
                if let Some(j) = line.junctions.first() {
                    is_contour = j.w == 0;
                }
            }

            if is_contour {
                for line in inset {
                    if line.is_odd {
                        continue; // odd lines don't contribute to the contour
                    } else if line.is_closed {
                        // sometimes an very small even polygonal wall is not stitched into a
                        // polygon
                        self.inner_contour.push(line.to_polygon());
                    }
                }
            } else {
                actual_toolpaths.push(inset.clone());
            }
        }
        if !actual_toolpaths.is_empty() {
            self.toolpaths = actual_toolpaths; // Filtered out the 0-width paths.
        } else {
            self.toolpaths.clear();
        }

        //The output walls from the skeletal trapezoidation have no known winding order,
        //especially if they are joined together from polylines. They can be in any direction,
        //clockwise or counter-clockwise, regardless of whether the shapes are positive or
        //negative. To get a correct shape, we need to make the outside contour positive and any
        //holes inside negative. This can be done by applying the even-odd rule to the shape. This
        //rule is not sensitive to the winding order of the polygon. The even-odd rule would be
        //incorrect if the polygon self-intersects, but that should never be generated by the
        //skeletal trapezoidation.
        self.inner_contour = union_fill(&self.inner_contour, PolyFillType::EvenOdd);
    }

    /// `getInnerContour()` (WallToolPaths.cpp 779-790): the inner contour, the walls generated
    /// first unless they have been; the outline itself with no inset.
    ///
    /// # Errors
    ///
    /// The exceptions of [`WallToolPaths::generate`].
    pub fn get_inner_contour(&mut self) -> Result<&Polygons, VoronoiError> {
        if !self.toolpaths_generated && self.inset_count > 0 {
            self.generate()?;
        } else if self.inset_count == 0 {
            return Ok(self.outline);
        }
        Ok(&self.inner_contour)
    }

    /// `removeEmptyToolPaths(toolpaths)` (WallToolPaths.cpp 792-799): the empty insets removed (in
    /// order, `std::remove_if`); whether none is left.
    pub fn remove_empty_tool_paths(toolpaths: &mut Vec<VariableWidthLines>) -> bool {
        toolpaths.retain(|lines| !lines.is_empty());
        toolpaths.is_empty()
    }

    /// `getRegionOrder(input, outer_to_inner)` (WallToolPaths.cpp 801-893): for each two lines of
    /// adjacent insets with vertices close together (within the mean of their widths times 1.9,
    /// found through a point grid of cells the largest width times 1.9), the pair in printing
    /// order: an odd line after its enclosing even one, even lines by `outer_to_inner`. The
    /// pairs are indexes into `input`, the first index of each line (a line given twice is one
    /// line, as the C++'s pointers are), in the order the set took them.
    #[must_use]
    pub fn get_region_order(input: &[&ExtrusionLine], outer_to_inner: bool) -> ExtrusionLineSet {
        let mut order_requirements = ExtrusionLineSet::new();
        // We build a grid where we map toolpath vertex locations to toolpaths, so that we can
        // easily find which two toolpaths are next to each other, which is the requirement for
        // there to be an order constraint.
        //
        // We use a PointGrid rather than a LineGrid to save on computation time. In very rare
        // cases two insets might lie next to each other without having neighboring vertices. However,
        // because of how Arachne works this will likely never be the case for two consecutive
        // insets. On the other hand one could imagine that two consecutive insets of a very large
        // circle could be simplify()ed such that the remaining vertices of the two insets don't
        // align. In those cases the order requirement is not captured, which means that the
        // PathOrderOptimizer *might* result in a violation of the user set path order. This
        // problem is expected to be not so severe and happen very sparsely.

        let mut max_line_w: Coord = 0;
        for line in input {
            // compute max_line_w
            for junction in &line.junctions {
                max_line_w = orca_stl::max(max_line_w, junction.w);
            }
        }
        if max_line_w == 0 {
            return order_requirements;
        }

        /// `LineLoc`: a junction and its line (the first index of the line's pointer).
        #[derive(Clone, Copy)]
        struct LineLoc {
            j: ExtrusionJunction,
            line: usize,
        }

        // The first index of each line, as its pointer.
        let mut first_index: BTreeMap<*const ExtrusionLine, usize> = BTreeMap::new();
        let canonical: Vec<usize> = input
            .iter()
            .enumerate()
            .map(|(i, l)| *first_index.entry(core::ptr::from_ref(*l)).or_insert(i))
            .collect();

        // How much farther two verts may be apart due to corners. This distance must be smaller
        // than 2, because otherwise we could create an order requirement between e.g. wall 2 of
        // one region and wall 3 of another region, while another wall 3 of the first region would
        // lie in between those two walls. However, higher values are better against the
        // limitations of using a PointGrid rather than a LineGrid.
        let diagonal_extension: f32 = 1.9;
        let searching_radius = orca_crt::f32_to_i64(max_line_w as f32 * diagonal_extension);
        let mut grid = SparsePointGrid::new(searching_radius, 0, 1.0, |elem: &LineLoc| elem.j.p);

        for (i, line) in input.iter().enumerate() {
            for junction in &line.junctions {
                grid.insert(LineLoc {
                    j: *junction,
                    line: canonical[i],
                });
            }
        }
        for (_, lineloc_here) in grid.iter() {
            let here_idx = lineloc_here.line;
            let here = input[here_idx];
            let loc_here = lineloc_here.j.p;
            let nearby_verts = grid.get_nearby(loc_here, searching_radius);
            for lineloc_nearby in &nearby_verts {
                let nearby_idx = lineloc_nearby.line;
                let nearby = input[nearby_idx];
                if nearby_idx == here_idx {
                    continue;
                }
                if nearby.inset_idx == here.inset_idx {
                    continue;
                }
                if nearby.inset_idx > here.inset_idx.wrapping_add(1) {
                    continue; // not directly adjacent
                }
                if here.inset_idx > nearby.inset_idx.wrapping_add(1) {
                    continue; // not directly adjacent
                }
                let mean_width = lineloc_here.j.w.wrapping_add(lineloc_nearby.j.w) / 2;
                if !shorter_then(
                    loc_here - lineloc_nearby.j.p,
                    orca_crt::f32_to_i64(mean_width as f32 * diagonal_extension),
                ) {
                    continue; // points are too far away from each other
                }
                if here.is_odd || nearby.is_odd {
                    if here.is_odd && !nearby.is_odd && nearby.inset_idx < here.inset_idx {
                        order_requirements.emplace((nearby_idx, here_idx));
                    }
                    if nearby.is_odd && !here.is_odd && here.inset_idx < nearby.inset_idx {
                        order_requirements.emplace((here_idx, nearby_idx));
                    }
                } else if (nearby.inset_idx < here.inset_idx) == outer_to_inner {
                    order_requirements.emplace((nearby_idx, here_idx));
                } else {
                    order_requirements.emplace((here_idx, nearby_idx));
                }
            }
        }
        order_requirements
    }
}

#[cfg(test)]
mod tests {
    use orca_geom::point::Point;
    use orca_geom::polygon::{Polygon, Polygons};

    use super::{
        WallToolPaths, WallToolPathsParams, remove_degenerate_verts, remove_small_areas,
        scaled_from_f32, simplify_polygon, unscale_f32,
    };

    fn rect(x0: i64, y0: i64, x1: i64, y1: i64) -> Polygon {
        Polygon::new(vec![
            Point::new(x0, y0),
            Point::new(x1, y0),
            Point::new(x1, y1),
            Point::new(x0, y1),
        ])
    }

    #[test]
    fn the_float_scalings_divide_in_float() {
        assert_eq!(scaled_from_f32(100.0), 100_000_000);
        assert_eq!(unscale_f32(400_000), 400_000.0_f32 * 0.000_001_f32);
    }

    #[test]
    fn simplify_keeps_three_points_and_clears_two() {
        let mut p = Polygon::new(vec![Point::new(0, 0), Point::new(5, 5)]);
        simplify_polygon(&mut p, 1, 1);
        assert!(p.points.is_empty());
        let mut t = Polygon::new(vec![Point::new(0, 0), Point::new(5, 5), Point::new(0, 9)]);
        simplify_polygon(&mut t, 1, 1);
        assert_eq!(t.points.len(), 3);
    }

    #[test]
    fn a_spike_is_degenerate() {
        let mut polys: Polygons = vec![Polygon::new(vec![
            Point::new(0, 0),
            Point::new(1_000_000, 0),
            Point::new(1_500_000, 0),
            Point::new(1_000_000, 0),
            Point::new(1_000_000, 1_000_000),
            Point::new(0, 1_000_000),
        ])];
        remove_degenerate_verts(&mut polys);
        assert_eq!(polys[0].points.len(), 4);
    }

    #[test]
    fn small_islands_go() {
        let mut polys: Polygons = vec![rect(0, 0, 5_000_000, 5_000_000), rect(6_000_000, 0, 6_100_000, 100_000)];
        remove_small_areas(&mut polys, 1e11, true);
        assert_eq!(polys.len(), 1);
    }

    #[test]
    fn a_square_gets_closed_walls() {
        let params = WallToolPathsParams {
            min_bead_width: 0.34,
            min_feature_size: 0.1,
            min_length_factor: 0.5,
            wall_transition_length: 0.4,
            wall_transition_angle: 10.0,
            wall_transition_filter_deviation: 0.1,
            wall_distribution_count: 1,
            ..WallToolPathsParams::default()
        };
        let outline: Polygons = vec![rect(0, 0, 10_000_000, 10_000_000)];
        let mut w = WallToolPaths::new(&outline, 377_080, 407_080, 3, 0, 0.2, &params);
        let toolpaths = w.get_tool_paths().expect("a square has no Voronoi error").clone();
        assert_eq!(toolpaths.len(), 3);
        for (i, inset) in toolpaths.iter().enumerate() {
            assert!(inset.iter().all(|l| l.inset_idx == i && l.is_closed));
        }
        let inner = w.get_inner_contour().expect("generated").clone();
        assert_eq!(inner.len(), 1);
    }
}
