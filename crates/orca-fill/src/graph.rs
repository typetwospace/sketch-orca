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
// Carries OrcaSlicer code: src/libslic3r/Fill/FillRectilinear.cpp lines 1354-1588 of OrcaSlicer
// v2.4.2, translated on 2026-11-14 (orca-o3-fill-3). Copyright the OrcaSlicer authors, the
// BambuStudio authors (Bambu Lab), the PrusaSlicer authors (Prusa Research) and the Slic3r authors
// (Alessandro Ranellucci), under the GNU Affero General Public License, version 3 (AGPL-3.0;
// LICENSE).

//! The rectilinear infill's graph walk (FillRectilinear.cpp): `traverse_graph_generate_polylines`
//! ([`traverse_graph_generate_polylines`]), which turns the vertical lines' intersections, linked
//! along the contours by [`crate::rectilinear::connect_segment_intersections_by_contours`], into
//! zig-zag lines, sweeping from left to right. The monotonic fill takes another way
//! ([`crate::monotonic`]).
//!
//! - The C++ walks the intersections of a line with a pointer `it` beside the index
//!   `i_intersection`; the two move together, so here one index is kept, and the intersection one
//!   step past it is named where the C++ moves `it` alone at the end.
//! - The current line is the last of `polylines_out` (the C++'s `polyline_current` always points
//!   at `polylines_out.back()`).
//! - A link followed while it is -1 stops ([`crate::stop::NEGATIVE_LINK`]) where the C++ would
//!   index a vector with it; the links the walk follows are found, so it never does.
//! - A finished line drops its repeated points (`remove_duplicate_points`) and is dropped when it
//!   has one point or two closer than `SCALED_EPSILON` on both axes (`std::abs` of the integer
//!   differences against the `double`).
//!
//! Left out: the walk's `pointLast`, which the C++ writes and never reads (it reads
//! `polylines_out.back().points.back()` before the loop, which on a line without points would read
//! outside it); the commented-out `measure_outer_contour_slab`; the asserts.

use orca_geom::multi_point::remove_duplicate_points;
use orca_geom::point::{Point, SCALED_EPSILON};
use orca_geom::polyline::{Polyline, Polylines};

use crate::base::FillParams;
use crate::rectilinear::{
    ExPolygonWithOffset, LinkQuality, SegmentIntersectionType, SegmentedIntersectionLine,
    emit_perimeter_prev_next_segment, emit_perimeter_segment_on_vertical_line,
    intersection_on_next_vertical_line_valid, intersection_on_prev_vertical_line_valid,
    measure_perimeter_horizontal_segment_length,
};
use crate::stop::{self, stop};

const FUNCTION: &str = "traverse_graph_generate_polylines";

/// An intersection link (`int`, -1 for none) as an index.
///
/// # Panics
/// With [`crate::stop::NEGATIVE_LINK`] when it is negative.
fn link(idx: i32) -> usize {
    match usize::try_from(idx) {
        Ok(i) => i,
        Err(_) => stop(FUNCTION, stop::NEGATIVE_LINK),
    }
}

/// The line being drawn: the last of `polylines_out`, as the C++'s `polyline_current`.
fn current(polylines_out: &mut Polylines) -> &mut Polyline {
    let n = polylines_out.len();
    &mut polylines_out[n - 1]
}

/// Whether a finished line is dropped: one point or none, or two points closer than
/// `SCALED_EPSILON` on both axes.
pub(crate) fn nearly_empty(points: &[Point]) -> bool {
    points.len() <= 1
        || (points.len() == 2
            && (points[0].x.wrapping_sub(points[1].x).wrapping_abs() as f64) < SCALED_EPSILON
            && (points[0].y.wrapping_sub(points[1].y).wrapping_abs() as f64) < SCALED_EPSILON)
}

/// `traverse_graph_generate_polylines(poly_with_offset, params, segs, consistent_pattern,
/// polylines_out)`: the outer-only chords marked consumed, then, from the first line that still
/// has an unconsumed piece (each line from the bottom, or for a consistent pattern every odd line
/// from the top), a line drawn up or down each vertical piece and on along the inner contour to
/// the line on the left or right (the shorter perimeter when both are free) or to the next piece
/// on the same line, until no way goes on; then the next start. The lines are appended to
/// `polylines_out`.
///
/// `_params` is read only by the commented-out code that measures the outer chords.
///
/// # Panics
/// With [`crate::stop::NEGATIVE_LINK`] when it would follow a link of -1.
pub fn traverse_graph_generate_polylines(
    poly_with_offset: &ExPolygonWithOffset,
    _params: &FillParams<'_>,
    segs: &mut [SegmentedIntersectionLine],
    consistent_pattern: bool,
    polylines_out: &mut Polylines,
) {
    // For each outer only chords, measure their maximum distance to the bow of the outer contour.
    // Mark an outer only chord as consumed, if the distance is low.
    for vline in segs.iter_mut() {
        let n = vline.intersections.len() as i32;
        let mut i_intersection = 0_i32;
        while i_intersection + 1 < n {
            let i = i_intersection as usize;
            if vline.intersections[i].kind == SegmentIntersectionType::OuterLow
                && vline.intersections[i + 1].kind == SegmentIntersectionType::OuterHigh
            {
                // if (params.full_infill()) measure_outer_contour_slab(...) else
                let consumed = true;
                vline.intersections[i].consumed_vertical_up = consumed;
            }
            i_intersection += 1;
        }
    }

    // Now construct a graph.
    // Find the first point.
    // Naively one would expect to achieve best results by chaining the paths by the shortest
    // distance, but that procedure does not create the longest continuous paths.
    // A simple "sweep left to right" procedure achieves better results.
    let mut i_vline: i32 = 0;
    let mut i_intersection: i32 = -1;
    // Follow the line, connect the lines into a graph.
    // Until no new line could be added to the output path:
    loop {
        if i_intersection == -1 {
            // The path has been interrupted. Find a next starting point.
            'search: for i_vline2 in 0..segs.len() as i32 {
                let vline = &segs[i_vline2 as usize];
                if !vline.intersections.is_empty() {
                    // For infill that needs to be consistent between layers (like Zig Zag),
                    // we are switching between forward and backward passes based on the line
                    // index.
                    let forward_pass = !consistent_pattern || (i_vline2 % 2 == 0);
                    let n = vline.intersections.len() as i32;
                    for i in 0..n {
                        let intrsctn_idx = if forward_pass { i } else { n - i - 1 };
                        let intrsctn = &vline.intersections[intrsctn_idx as usize];
                        if intrsctn.is_outer() {
                            let consumed = if intrsctn.is_low() {
                                intrsctn.consumed_vertical_up
                            } else {
                                vline.intersections[(intrsctn_idx - 1) as usize]
                                    .consumed_vertical_up
                            };
                            if !consumed {
                                i_vline = i_vline2;
                                i_intersection = intrsctn_idx;
                                break 'search;
                            }
                        }
                    }
                }
            }
            if i_intersection == -1 {
                // We are finished.
                break;
            }
            // found:
            // Start a new path.
            polylines_out.push(Polyline::default());
            // Emit the first point of a path.
            let vline = &segs[i_vline as usize];
            let point_last = Point::new(
                vline.pos,
                vline.intersections[i_intersection as usize].pos(),
            );
            current(polylines_out).points.push(point_last);
        }

        // From the initial point (i_vline, i_intersection), follow a path.
        let iv = i_vline as usize;
        let going_up = segs[iv].intersections[i_intersection as usize].is_low();
        let mut try_connect = false;
        if going_up {
            // Step back to the beginning of the vertical segment to mark it as consumed.
            if segs[iv].intersections[i_intersection as usize].is_inner() {
                i_intersection -= 1;
            }
            // Consume the complete vertical segment up to the outer contour.
            loop {
                segs[iv].intersections[i_intersection as usize].consumed_vertical_up = true;
                i_intersection += 1;
                if segs[iv].intersections[i_intersection as usize].kind
                    == SegmentIntersectionType::OuterHigh
                {
                    break;
                }
            }
            if segs[iv].intersections[(i_intersection - 1) as usize].is_inner() {
                // Step back.
                i_intersection -= 1;
                try_connect = true;
            }
        } else {
            // Going down.
            // Consume the complete vertical segment up to the outer contour.
            if segs[iv].intersections[i_intersection as usize].is_inner() {
                segs[iv].intersections[i_intersection as usize].consumed_vertical_up = true;
            }
            loop {
                i_intersection -= 1;
                segs[iv].intersections[i_intersection as usize].consumed_vertical_up = true;
                if segs[iv].intersections[i_intersection as usize].kind
                    == SegmentIntersectionType::OuterLow
                {
                    break;
                }
            }
            if segs[iv].intersections[(i_intersection + 1) as usize].is_inner() {
                // Step back.
                i_intersection += 1;
                try_connect = true;
            }
        }
        // Where the C++'s `it` points when the line is finished.
        let mut it = i_intersection as usize;
        if try_connect {
            let ii = it;
            // Decide, whether to finish the segment, or whether to follow the perimeter.
            // 1) Find possible connection points on the previous / next vertical line.
            let i_prev = segs[iv].intersections[ii].left_horizontal();
            let i_next = segs[iv].intersections[ii].right_horizontal();

            // To ensure pattern consistency between layers for Zig Zag infill, we always
            // try to connect to the next vertical line and never to the previous vertical line.
            let intersection_prev_valid =
                intersection_on_prev_vertical_line_valid(segs, iv, ii) && !consistent_pattern;
            let intersection_next_valid = intersection_on_next_vertical_line_valid(segs, iv, ii);
            let intersection_horizontal_valid = intersection_prev_valid || intersection_next_valid;

            // Mark both the left and right connecting segment as consumed, because one cannot go
            // to this intersection point as it has been consumed.
            if i_prev != -1 {
                segs[iv - 1].intersections[i_prev as usize].consumed_perimeter_right = true;
            }
            if i_next != -1 {
                segs[iv].intersections[ii].consumed_perimeter_right = true;
            }

            // Try to connect to a previous or next vertical line, making a zig-zag pattern.
            if intersection_horizontal_valid {
                // A horizontal connection along the perimeter line exists.
                let mut take_next = intersection_next_valid;
                if intersection_prev_valid && intersection_next_valid {
                    // Take the shorter segment. This greedy heuristics may not be the best.
                    let dist_prev = measure_perimeter_horizontal_segment_length(
                        poly_with_offset,
                        segs,
                        iv - 1,
                        link(i_prev),
                        ii,
                    );
                    let dist_next = measure_perimeter_horizontal_segment_length(
                        poly_with_offset,
                        segs,
                        iv,
                        ii,
                        link(i_next),
                    );
                    take_next = dist_next < dist_prev;
                }
                let vline = &segs[iv];
                let itsct = &vline.intersections[ii];
                let polyline_current = current(polylines_out);
                polyline_current
                    .points
                    .push(Point::new(vline.pos, itsct.pos()));
                emit_perimeter_prev_next_segment(
                    poly_with_offset,
                    segs,
                    iv,
                    itsct.i_contour,
                    ii,
                    link(if take_next { i_next } else { i_prev }),
                    polyline_current,
                    take_next,
                );
                // FIXME consume the left / right connecting segments at the other end of this
                // line? Currently it is not critical because a perimeter segment is not followed
                // if the vertical segment at the other side has already been consumed.
                // Advance to the neighbor line.
                if take_next {
                    i_vline += 1;
                    i_intersection = i_next;
                } else {
                    i_vline -= 1;
                    i_intersection = i_prev;
                }
                continue;
            }

            // Try to connect to a previous or next point on the same vertical line.
            let i_vertical = segs[iv].intersections[ii].vertical_outside();
            let vertical_link_quality = if i_vertical == -1
                || segs[iv].intersections[link(i_vertical + if going_up { 0 } else { -1 })]
                    .consumed_vertical_up
            {
                LinkQuality::Invalid
            } else {
                segs[iv].intersections[ii].vertical_outside_quality()
            };
            // (#if 0: also follow a link too long when there is no horizontal link.)
            if vertical_link_quality != LinkQuality::Invalid {
                let vline = &segs[iv];
                let itsct = &vline.intersections[ii];
                current(polylines_out)
                    .points
                    .push(Point::new(vline.pos, itsct.pos()));
                if vertical_link_quality == LinkQuality::Valid {
                    // Consume the connecting contour and the next segment.
                    emit_perimeter_segment_on_vertical_line(
                        poly_with_offset,
                        segs,
                        iv,
                        itsct.i_contour,
                        ii,
                        link(i_vertical),
                        current(polylines_out),
                        if going_up {
                            itsct.has_left_vertical_up()
                        } else {
                            itsct.has_right_vertical_down()
                        },
                    );
                } else {
                    // Just skip the connecting contour and start a new path.
                    polylines_out.push(Polyline::default());
                    current(polylines_out).points.push(Point::new(
                        vline.pos,
                        vline.intersections[link(i_vertical)].pos(),
                    ));
                }
                // Mark both the left and right connecting segment as consumed, because one cannot
                // go to this intersection point as it has been consumed.
                // If there are any outer intersection points skipped (bypassed) by the contour,
                // mark them as processed.
                if going_up {
                    for i in i_intersection..i_vertical {
                        segs[iv].intersections[i as usize].consumed_vertical_up = true;
                    }
                } else {
                    for i in i_vertical..i_intersection {
                        segs[iv].intersections[i as usize].consumed_vertical_up = true;
                    }
                }
                // seg.intersections[going_up ? i_intersection : i_intersection - 1]
                //     .consumed_vertical_up = true;
                segs[iv].intersections[ii].consumed_perimeter_right = true;
                let beside = if going_up { ii + 1 } else { ii - 1 };
                segs[iv].intersections[beside].consumed_perimeter_right = true;
                i_intersection = i_vertical;
                continue;
            }

            // No way to continue the current polyline. Take the rest of the line up to the outer
            // contour. This will finish the polyline, starting another polyline at a new point.
            it = if going_up { ii + 1 } else { ii - 1 };
        }

        // Finish the current vertical line,
        // reset the current vertical line to pick a new starting point in the next round.
        let vline = &segs[iv];
        let point_last = Point::new(vline.pos, vline.intersections[it].pos());
        let polyline_current = current(polylines_out);
        polyline_current.points.push(point_last);
        // Handle duplicate points and zero length segments.
        remove_duplicate_points(&mut polyline_current.points);
        // Handle nearly zero length edges.
        if nearly_empty(&polyline_current.points) {
            polylines_out.pop();
        }
        i_intersection = -1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use orca_geom::expolygon::ExPolygon;
    use orca_geom::polygon::Polygon;

    use crate::rectilinear::{
        connect_segment_intersections_by_contours, slice_region_by_vertical_lines,
    };

    fn square(size: i64) -> ExPolygon {
        ExPolygon::new(Polygon::new(vec![
            Point::new(0, 0),
            Point::new(size, 0),
            Point::new(size, size),
            Point::new(0, size),
        ]))
    }

    #[test]
    fn a_square_is_one_zig_zag() {
        let pwo = ExPolygonWithOffset::new(&square(1000), 0., -100, -200);
        // Lines at x = 250, 350, ..., 750: all inside the inner offset.
        let mut segs = slice_region_by_vertical_lines(&pwo, 6, 250, 100).unwrap();
        let params = FillParams::default();
        connect_segment_intersections_by_contours(&pwo, &mut segs, &params, 0);
        let mut out = Polylines::new();
        traverse_graph_generate_polylines(&pwo, &params, &mut segs, false, &mut out);
        assert_eq!(out.len(), 1);
        let pl = &out[0];
        // Starts at the outer low of the first line, ends at the outer contour of the last.
        assert_eq!(pl.points[0], Point::new(250, 100));
        assert_eq!(*pl.points.last().unwrap(), Point::new(750, 900));
        // Up, across, down, across ...: every line's x is visited in turn.
        let xs: Vec<i64> = pl.points.iter().map(|p| p.x).collect();
        for x in [250, 350, 450, 550, 650, 750] {
            assert!(xs.contains(&x), "x = {x} in {xs:?}");
        }
        // Every piece was consumed.
        for il in &segs {
            assert!(il.intersections[0].consumed_vertical_up);
        }
    }

    #[test]
    fn lines_that_cannot_connect_stay_apart() {
        let pwo = ExPolygonWithOffset::new(&square(1000), 0., -100, -200);
        let mut segs = slice_region_by_vertical_lines(&pwo, 3, 250, 200).unwrap();
        let params = FillParams {
            anchor_length_max: 0.,
            ..FillParams::default()
        };
        connect_segment_intersections_by_contours(&pwo, &mut segs, &params, 0);
        let mut out = Polylines::new();
        traverse_graph_generate_polylines(&pwo, &params, &mut segs, false, &mut out);
        // The links are too long to follow: each line is a line of its own, outer to outer.
        assert!(out.len() > 1);
        assert_eq!(out[0].points[0], Point::new(250, 100));
        assert_eq!(out[0].points[1], Point::new(250, 900));
    }

    #[test]
    fn a_short_line_is_dropped() {
        assert!(nearly_empty(&[Point::new(0, 0)]));
        assert!(nearly_empty(&[Point::new(0, 0), Point::new(99, -99)]));
        assert!(!nearly_empty(&[Point::new(0, 0), Point::new(100, 0)]));
        assert!(!nearly_empty(&[
            Point::new(0, 0),
            Point::new(1, 0),
            Point::new(2, 0)
        ]));
    }
}
