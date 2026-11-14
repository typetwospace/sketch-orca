// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/TriangleMeshSlicer.cpp lines 1058-2159 (the chaining of
// the intersection lines into loops, make_loops, make_slab_loops, make_expolygons, slice_mesh,
// slice_mesh_ex and slice_mesh_slabs) and the declarations of slice_mesh, slice_mesh_ex (with its
// two inline overloads) and slice_mesh_slabs of src/libslic3r/TriangleMeshSlicer.hpp lines 60-111,
// of OrcaSlicer v2.4.2, translated on 2026-11-14 by orca-o3-slice-2. Copyright the OrcaSlicer
// authors, the BambuStudio authors (Bambu Lab), the PrusaSlicer authors (Prusa Research) and the
// Slic3r authors (Alessandro Ranellucci), under the GNU Affero General Public License, version 3
// (AGPL-3.0; LICENSE).

//! The second half of libslic3r's mesh slicer (`TriangleMeshSlicer`): the intersection lines of
//! [`crate::triangle_mesh_slicer`] chained into closed loops, and the public functions that slice a
//! mesh into polygons ([`slice_mesh`], [`slice_mesh_plane`]), into expolygons ([`slice_mesh_ex`])
//! and into the tops and bottoms of slabs between heights ([`slice_mesh_slabs`]).
//!
//! - `make_loops` chains one layer's lines: first by triangle connectivity (a line goes on from the
//!   line that starts at its end's mesh edge, else at its end's vertex: the first unused one in the
//!   lines sorted by `edge_a_id` and by `a_id` with orca-stl's sort, the C++'s introsort, whose order
//!   of equal keys decides which), then the open polylines left are joined end to end through the
//!   ids of their ends, in the same direction and then in either (`chain_open_polylines_exact`),
//!   then through the nearest end within 2 mm (`chain_open_polylines_close_gaps`, through
//!   orca-paths' `ClosestPointInRadiusLookup`, which keeps each grid cell's ends in the order they
//!   went in, as MSVC's `unordered_multimap` keeps one key's values). Both seed from the open
//!   polylines sorted by length, longest first. A loop joined from pieces of either direction is
//!   made counter-clockwise when its area (MultiPoint.hpp's `area`) is negative. As in the C++, the
//!   exact chaining in one direction never moves a polyline's end reference after a join, so it
//!   keeps looking at the first polyline's own end.
//! - `make_loops` over the layers runs in a `tbb::parallel_for` (orca-tbb); each layer writes only
//!   its own polygons, so the order the threads take them in changes nothing. In Positive mode each
//!   loop is made counter-clockwise; in PositiveLargestContour mode only the loop of the largest
//!   absolute area is kept, counter-clockwise. A layer whose loops all have an area of 0 makes the
//!   C++ dereference a null pointer there; the engine never gets there (`slice_mesh_ex`, the only
//!   call PrintObjectSlice makes, slices PositiveLargestContour as Positive), and this port keeps
//!   that layer's first loop as it is (`parity/sop1/slice.md`, "The pieces and their edges").
//! - [`slice_mesh`] cuts the faces at the heights through [`slice_make_lines`]: at one height or
//!   none it transforms each vertex as it cuts (`scaled<float>` of x and y, a division by
//!   `float(SCALING_FACTOR)`, or the trafo prescaled in x and y and cast to `float`); at more it
//!   copies the vertices once, multiplied in x and y by `float(1 / SCALING_FACTOR)` or transformed,
//!   so the two paths can give other vertices for one mesh. [`slice_mesh_plane`] cuts only the faces
//!   that touch the plane, on the calling thread.
//! - [`slice_mesh_ex`] slices with PositiveLargestContour made Positive, then makes each layer's
//!   expolygons in a `tbb::parallel_for`: `union_ex` with the mode's fill type, then an offset out
//!   by the closing radius and back in by it less the extra offset (`offset2_ex`), or one offset,
//!   or the union alone; PositiveLargestContour keeps the largest contour, and a resolution above 0
//!   simplifies each expolygon (`ExPolygon::simplify`, at `scaled<float>(resolution)`).
//! - [`slice_mesh_slabs`] classifies each transformed face as up, down, vertical or degenerate by
//!   the sign of its projected cross product (times -1 under a mirroring trafo, whose sign comes
//!   from the dynamic block's LU determinant, orca-mesh's `dynamic_block_determinant`), collects the
//!   vertical faces' centroid and normal when asked, then chains each slab's lines from the plane
//!   below, between the planes and from the plane above (its edge ids moved past `num_edges`) by
//!   triangle connectivity alone; open polylines are dropped with a trace record.
//!
//! The C++'s `throw_on_cancel` is a closure called where the C++ calls it; its panic is the C++'s
//! exception, which orca-tbb carries out of its loops. The debug records are written through
//! `orca_mesh::log` at the C++'s severities and with its texts.
//!
//! Left out, as no piece carries the C++ that calls them: `make_expolygons_simple` (lines
//! 1664-1736), which only `triangulate_slice` and so `cut_mesh` call (the cut tool); and the debug
//! output under `SLIC3R_DEBUG_SLICE_PROCESSING`, `SLIC3R_TRIANGLEMESH_DEBUG`, `SLIC3R_DEBUG` and
//! `EXPENSIVE_DEBUG_CHECKS`, which the release build compiles out, with the asserts and the
//! debug-only `IntersectionLine::source`.

use std::cell::RefCell;

use orca_crt::{f32_to_i64, f64_to_i64, fabs, sqrt};
use orca_eigen::{Transform3d, Transform3f, Vec3d, Vec3f};
use orca_geom::clipper::PolyFillType;
use orca_geom::clipper_utils::{
    DEFAULT_JOIN_TYPE, DEFAULT_MITER_LIMIT, offset_ex_expolygons, offset2_ex, union_ex,
};
use orca_geom::expolygon::{ExPolygon, ExPolygons, keep_largest_contour_only};
use orca_geom::multi_point::{area2, length};
use orca_geom::point::{Coord, Point, SCALING_FACTOR, cross2_coord, scale_, scaled_f32};
use orca_geom::polygon::{Polygon, Polygons};
use orca_index::stop::not_ported;
use orca_mesh::admesh::{IndexedTriangleSet, StlVertex};
use orca_mesh::eigen_lu::dynamic_block_determinant;
use orca_mesh::face_edge_ids::{
    its_face_edge_ids, its_face_edge_ids_from_neighbors, its_face_edge_ids_masked,
};
use orca_mesh::its::its_face_neighbors_par;
use orca_mesh::log::{Severity, log};
use orca_paths::ClosestPointInRadiusLookup;
use orca_stl::{lower_bound_by, sort_by, upper_bound_by};
use orca_tbb::parallel_for_each_mut;

use crate::triangle_mesh_slicer::{
    FaceOrientation, IntersectionLine, IntersectionLines, IntersectionReference, MeshSlicingParams,
    MeshSlicingParamsEx, OpenPolyline, SlabLines, SlicingMode, slice_make_lines,
    slice_make_lines_plane, slice_slabs_make_lines,
};

/// The stop of the exact chaining in either direction (`chain_open_polylines_exact` with
/// `try_connect_reversed`) on an open polyline end that has neither a vertex nor an edge: the C++
/// leaves such an end out of its sorted ends and then dereferences the end iterator it gets back
/// when it looks for it (undefined). Every end the chaining by triangle connectivity makes holds a
/// vertex or an edge, so no mesh reaches it.
pub const OPEN_POLYLINE_END_WITHOUT_REFERENCE: &str = "open-polyline-end-without-reference";

// ---- chaining by triangle connectivity ----------------------------------------------------------

/// `chain_lines_by_triangle_connectivity(lines, loops, open_polylines)`, called by `make_loops` to
/// connect sliced triangles into closed loops and open polylines by the triangle connectivity. Only
/// connects segments crossing triangles of the same orientation. Each line used is marked skipped.
fn chain_lines_by_triangle_connectivity(
    lines: &mut [IntersectionLine],
    loops: &mut Polygons,
    open_polylines: &mut Vec<OpenPolyline>,
) {
    // Build a map of lines by edge_a_id and a_id.
    let mut by_edge_a_id: Vec<usize> = Vec::with_capacity(lines.len());
    let mut by_a_id: Vec<usize> = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        if !line.skip() {
            if line.edge_a_id != -1 {
                by_edge_a_id.push(i);
            }
            if line.a_id != -1 {
                by_a_id.push(i);
            }
        }
    }
    {
        let lines: &[IntersectionLine] = lines;
        sort_by(&mut by_edge_a_id, |&il1, &il2| {
            lines[il1].edge_a_id < lines[il2].edge_a_id
        });
        sort_by(&mut by_a_id, |&il1, &il2| lines[il1].a_id < lines[il2].a_id);
    }
    // Chain the segments with a greedy algorithm, collect the loops and unclosed polylines.
    let mut it_line_seed: usize = 0;
    loop {
        // take first spare line and start a new loop
        let mut first_line: Option<usize> = None;
        while it_line_seed < lines.len() {
            let candidate = it_line_seed;
            it_line_seed += 1;
            if lines[candidate].is_seed_candidate() {
                first_line = Some(candidate);
                break;
            }
        }
        let Some(first_line) = first_line else {
            break;
        };
        lines[first_line].set_skip();
        let mut loop_pts: Vec<Point> = vec![lines[first_line].a];
        let mut last_line = first_line;

        loop {
            // find a line starting where last one finishes
            let mut next_line: Option<usize> = None;
            if lines[last_line].edge_b_id != -1 {
                let key = lines[last_line].edge_b_id;
                next_line = first_unused(lines, &by_edge_a_id, key, |l| l.edge_a_id);
            }
            if next_line.is_none() && lines[last_line].b_id != -1 {
                let key = lines[last_line].b_id;
                next_line = first_unused(lines, &by_a_id, key, |l| l.a_id);
            }
            let Some(next_line) = next_line else {
                // Check whether we closed this loop.
                let (first, last) = (&lines[first_line], &lines[last_line]);
                if (first.edge_a_id != -1 && first.edge_a_id == last.edge_b_id)
                    || (first.a_id != -1 && first.a_id == last.b_id)
                {
                    // The current loop is complete. Add it to the output.
                    loops.push(Polygon::new(loop_pts));
                } else {
                    // This is an open polyline. Add it to the list of open polylines. These open
                    // polylines will processed later.
                    loop_pts.push(last.b);
                    open_polylines.push(OpenPolyline::new(
                        IntersectionReference::new(first.a_id, first.edge_a_id),
                        IntersectionReference::new(last.b_id, last.edge_b_id),
                        loop_pts,
                    ));
                }
                break;
            };
            loop_pts.push(lines[next_line].a);
            last_line = next_line;
            lines[next_line].set_skip();
        }
    }
}

/// The first line not skipped among those of `sorted` (sorted by `id_of`) whose id is `key`: the
/// C++'s `std::lower_bound`, then `std::upper_bound` from there when the first is not past the end,
/// then the walk over that range.
fn first_unused(
    lines: &[IntersectionLine],
    sorted: &[usize],
    key: i32,
    id_of: impl Fn(&IntersectionLine) -> i32,
) -> Option<usize> {
    let it_begin = lower_bound_by(sorted, &key, |&il, &k| id_of(&lines[il]) < k);
    if it_begin != sorted.len() {
        let it_end =
            it_begin + upper_bound_by(&sorted[it_begin..], &key, |&k, &il| k < id_of(&lines[il]));
        for &il in &sorted[it_begin..it_end] {
            if !lines[il].skip() {
                return Some(il);
            }
        }
    }
    None
}

/// `open_polylines_sorted(open_polylines, update_lengths)`: the polylines not consumed (their
/// lengths taken again when asked), by length, longest first, as indices.
fn open_polylines_sorted(open_polylines: &mut [OpenPolyline], update_lengths: bool) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::with_capacity(open_polylines.len());
    for (i, opl) in open_polylines.iter_mut().enumerate() {
        if !opl.consumed {
            if update_lengths {
                opl.length = length(&opl.points);
            }
            out.push(i);
        }
    }
    let open_polylines: &[OpenPolyline] = open_polylines;
    sort_by(&mut out, |&lhs, &rhs| {
        open_polylines[lhs].length > open_polylines[rhs].length
    });
    out
}

/// `OpenPolylineEnd` of `chain_open_polylines_exact` and of `chain_open_polylines_close_gaps`: an
/// end of an open polyline, its index and whether it is the start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OpenPolylineEnd {
    /// `polyline`, as its index.
    polyline: usize,
    /// `start`: is it the start or end point?
    start: bool,
}

impl OpenPolylineEnd {
    const fn new(polyline: usize, start: bool) -> Self {
        Self { polyline, start }
    }

    /// `ipref()`.
    fn ipref(self, open_polylines: &[OpenPolyline]) -> IntersectionReference {
        let opl = &open_polylines[self.polyline];
        if self.start { opl.start } else { opl.end }
    }

    /// `id()`: a unique id of the intersection point, its vertex id, or minus its edge id.
    fn id(self, open_polylines: &[OpenPolyline]) -> i32 {
        let r = self.ipref(open_polylines);
        if r.point_id >= 0 {
            r.point_id
        } else {
            r.edge_id.wrapping_neg()
        }
    }
}

/// The lambda `find_polyline_end(end)` of `chain_open_polylines_exact`: where `end` (the same
/// polyline and start flag) is in `by_id`.
fn find_polyline_end(
    by_id: &[OpenPolylineEnd],
    open_polylines: &[OpenPolyline],
    end: OpenPolylineEnd,
) -> Option<usize> {
    let end_id = end.id(open_polylines);
    let mut it = lower_bound_by(by_id, &end_id, |e, &k| e.id(open_polylines) < k);
    while it < by_id.len() && by_id[it].id(open_polylines) == end_id {
        if by_id[it] == end {
            return Some(it);
        }
        it += 1;
    }
    None
}

/// `chain_open_polylines_exact(open_polylines, loops, try_connect_reversed)`, called by
/// `make_loops` to connect remaining open polylines across shared triangle edges and vertices.
/// Depending on `try_connect_reversed`, it may or may not connect segments crossing triangles of
/// opposite orientation.
fn chain_open_polylines_exact(
    open_polylines: &mut [OpenPolyline],
    loops: &mut Polygons,
    try_connect_reversed: bool,
) {
    // Store the end points of open_polylines into vectors sorted
    let mut by_id: Vec<OpenPolylineEnd> = Vec::with_capacity(2 * open_polylines.len());
    for (i, opl) in open_polylines.iter().enumerate() {
        if opl.start.point_id != -1 || opl.start.edge_id != -1 {
            by_id.push(OpenPolylineEnd::new(i, true));
        }
        if try_connect_reversed && (opl.end.point_id != -1 || opl.end.edge_id != -1) {
            by_id.push(OpenPolylineEnd::new(i, false));
        }
    }
    {
        let pls: &[OpenPolyline] = open_polylines;
        sort_by(&mut by_id, |ope1, ope2| ope1.id(pls) < ope2.id(pls));
    }
    // Try to connect the loops.
    let sorted_by_length = open_polylines_sorted(open_polylines, false);
    for opl in sorted_by_length {
        if open_polylines[opl].consumed {
            continue;
        }
        open_polylines[opl].consumed = true;
        let end = OpenPolylineEnd::new(opl, false);
        loop {
            // find a line starting where last one finishes
            let next_start = {
                let pls: &[OpenPolyline] = open_polylines;
                let end_id = end.id(pls);
                let mut it = lower_bound_by(&by_id, &end_id, |e, &k| e.id(pls) < k);
                let mut found = None;
                while it < by_id.len() && by_id[it].id(pls) == end_id {
                    if !pls[by_id[it].polyline].consumed {
                        found = Some(by_id[it]);
                        break;
                    }
                    it += 1;
                }
                found
            };
            let Some(next_start) = next_start else {
                // The current loop could not be closed. Unmark the segment.
                open_polylines[opl].consumed = false;
                break;
            };
            // Attach this polyline to the end of the initial polyline.
            let next = next_start.polyline;
            let attached: Vec<Point> = if next_start.start {
                open_polylines[next].points[1..].to_vec()
            } else {
                open_polylines[next]
                    .points
                    .iter()
                    .rev()
                    .skip(1)
                    .copied()
                    .collect()
            };
            open_polylines[opl].points.extend(attached);
            open_polylines[opl].length += open_polylines[next].length;
            // Mark the next polyline as consumed.
            open_polylines[next].points.clear();
            open_polylines[next].length = 0.0;
            open_polylines[next].consumed = true;
            if try_connect_reversed {
                // Running in a mode, where the polylines may be connected by mixing their
                // orientations. Update the end point lookup structure after the end point of the
                // current polyline was extended.
                let found_end = find_polyline_end(&by_id, open_polylines, end);
                let found_next_end = find_polyline_end(
                    &by_id,
                    open_polylines,
                    OpenPolylineEnd::new(next, !next_start.start),
                );
                let (Some(it_end), Some(it_next_end)) = (found_end, found_next_end) else {
                    not_ported(
                        "chain_open_polylines_exact",
                        OPEN_POLYLINE_END_WITHOUT_REFERENCE,
                    );
                };
                // Swap the end points of the current and next polyline, but keep the polyline ptr
                // and the start flag.
                let other = by_id[it_next_end];
                let mine = open_polylines[opl].end;
                let theirs = other.ipref(open_polylines);
                open_polylines[opl].end = theirs;
                if other.start {
                    open_polylines[other.polyline].start = mine;
                } else {
                    open_polylines[other.polyline].end = mine;
                }
                // Swap the positions of OpenPolylineEnd structures in the sorted array to match
                // their respective end point positions.
                by_id.swap(it_end, it_next_end);
            }
            // Check whether we closed this loop.
            let o = &mut open_polylines[opl];
            if (o.start.edge_id != -1 && o.start.edge_id == o.end.edge_id)
                || (o.start.point_id != -1 && o.start.point_id == o.end.point_id)
            {
                // The current loop is complete. Add it to the output.
                // Remove the duplicate last point.
                o.points.pop();
                if o.points.len() >= 3 {
                    if try_connect_reversed && area2(&o.points) < 0.0 {
                        // The closed polygon is patched from pieces with messed up orientation,
                        // therefore the orientation of the patched up polygon is not known.
                        // Orient the patched up polygons CCW. This heuristic may close some holes
                        // and cavities.
                        o.points.reverse();
                    }
                    loops.push(Polygon::new(core::mem::take(&mut o.points)));
                }
                o.points.clear();
                break;
            }
            // Continue with the current loop.
        }
    }
}

/// `chain_open_polylines_close_gaps(open_polylines, loops, max_gap, try_connect_reversed)`,
/// called by `make_loops` to connect remaining open polylines across shared triangle edges and
/// vertices, possibly closing small gaps. Depending on `try_connect_reversed`, it may or may not
/// connect segments crossing triangles of opposite orientation.
///
/// The C++'s lookup reads an end's point through the polyline it points to, which changes while
/// the loop runs; here the polylines sit in a `RefCell` the lookup's accessor reads, and no borrow
/// of it is held across a call of the lookup.
#[allow(clippy::too_many_lines)]
fn chain_open_polylines_close_gaps(
    open_polylines: &mut Vec<OpenPolyline>,
    loops: &mut Polygons,
    max_gap: f64,
    try_connect_reversed: bool,
) {
    let max_gap_scaled: Coord = f64_to_i64(scale_(max_gap));

    // Sort the open polylines by their length, so the new loops will be seeded from longer chains.
    // Update the polyline lengths, return only not yet consumed polylines.
    let sorted_by_length = open_polylines_sorted(open_polylines, true);

    // Store the end points of open_polylines into ClosestPointInRadiusLookup<OpenPolylineEnd>.
    let pls = RefCell::new(core::mem::take(open_polylines));
    // `OpenPolylineEndAccessor`: no point for an end of a consumed polyline.
    let accessor = |pt: &OpenPolylineEnd| -> Option<Point> {
        let pls = pls.borrow();
        let opl = &pls[pt.polyline];
        if opl.consumed {
            None
        } else if pt.start {
            Some(opl.points[0])
        } else {
            Some(opl.points[opl.points.len() - 1])
        }
    };
    let mut closest_end_point_lookup = ClosestPointInRadiusLookup::new(max_gap_scaled, accessor);
    for &opl in &sorted_by_length {
        closest_end_point_lookup.insert(OpenPolylineEnd::new(opl, true));
        if try_connect_reversed {
            closest_end_point_lookup.insert(OpenPolylineEnd::new(opl, false));
        }
    }
    // Try to connect the loops.
    for &opl in &sorted_by_length {
        if pls.borrow()[opl].consumed {
            continue;
        }
        let end = OpenPolylineEnd::new(opl, false);
        if try_connect_reversed {
            // The end point of this polyline will be modified, thus the following entry will
            // become invalid. Remove it.
            closest_end_point_lookup.erase(&end);
        }
        pls.borrow_mut()[opl].consumed = true;
        let mut n_segments_joined: usize = 1;
        loop {
            // Find a line starting where last one finishes, only return non-consumed open
            // polylines (OpenPolylineEndAccessor returns null for consumed).
            let end_point = {
                let p = pls.borrow();
                p[opl].points[p[opl].points.len() - 1]
            };
            let (next_start, next_start_dist) = {
                let (found, dist) = closest_end_point_lookup.find(end_point);
                (found.copied(), dist)
            };
            // Check whether we closed this loop.
            let (current_loop_closing_distance2, opl_length) = {
                let p = pls.borrow();
                let points = &p[opl].points;
                let d2 = (points[points.len() - 1] - points[0])
                    .cast_f64()
                    .squared_norm();
                (d2, length(points))
            };
            let mut loop_closed =
                current_loop_closing_distance2 < max_gap_scaled as f64 * max_gap_scaled as f64;
            if next_start.is_some()
                && loop_closed
                && current_loop_closing_distance2 < next_start_dist
            {
                // Heuristics to decide, whether to close the loop, or connect another polyline.
                // One should avoid closing loops shorter than max_gap_scaled.
                loop_closed = sqrt(current_loop_closing_distance2) < 0.3 * opl_length;
            }
            if loop_closed {
                // Remove the start point of the current polyline from the lookup.
                // Mark the current segment as not consumed, otherwise the
                // closest_end_point_lookup.erase() would fail.
                pls.borrow_mut()[opl].consumed = false;
                closest_end_point_lookup.erase(&OpenPolylineEnd::new(opl, true));
                let mut p = pls.borrow_mut();
                let o = &mut p[opl];
                if current_loop_closing_distance2 == 0.0 {
                    // Remove the duplicate last point.
                    o.points.pop();
                } else {
                    // The end points are different, keep both of them.
                }
                if o.points.len() >= 3 {
                    if try_connect_reversed && n_segments_joined > 1 && area2(&o.points) < 0.0 {
                        // The closed polygon is patched from pieces with messed up orientation,
                        // therefore the orientation of the patched up polygon is not known.
                        // Orient the patched up polygons CCW. This heuristic may close some holes
                        // and cavities.
                        o.points.reverse();
                    }
                    loops.push(Polygon::new(core::mem::take(&mut o.points)));
                }
                o.points.clear();
                o.consumed = true;
                break;
            }
            let Some(next_start) = next_start else {
                // The current loop could not be closed. Unmark the segment.
                pls.borrow_mut()[opl].consumed = false;
                if try_connect_reversed {
                    // Re-insert the end point.
                    closest_end_point_lookup.insert(OpenPolylineEnd::new(opl, false));
                }
                break;
            };
            // Attach this polyline to the end of the initial polyline.
            {
                let mut p = pls.borrow_mut();
                let back = p[opl].points[p[opl].points.len() - 1];
                let src = &p[next_start.polyline].points;
                let attached: Vec<Point> = if next_start.start {
                    let skip = usize::from(src[0] == back);
                    src[skip..].to_vec()
                } else {
                    let skip = usize::from(src[src.len() - 1] == back);
                    src.iter().rev().skip(skip).copied().collect()
                };
                p[opl].points.extend(attached);
            }
            n_segments_joined += 1;
            // Remove the end points of the consumed polyline segment from the lookup.
            let opl2 = next_start.polyline;
            closest_end_point_lookup.erase(&OpenPolylineEnd::new(opl2, true));
            if try_connect_reversed {
                closest_end_point_lookup.erase(&OpenPolylineEnd::new(opl2, false));
            }
            let mut p = pls.borrow_mut();
            p[opl2].points.clear();
            p[opl2].consumed = true;
            // Continue with the current loop.
        }
    }
    drop(closest_end_point_lookup);
    *open_polylines = pls.into_inner();
}

/// `make_loops(lines)`: one layer's lines chained into loops. The lines have their flags
/// modified.
fn make_loops(lines: &mut [IntersectionLine]) -> Polygons {
    let mut loops = Polygons::new();
    // There should be no tangent edges, as the horizontal triangles are ignored and if two
    // triangles touch at a cutting plane, only the bottom triangle is considered to be cutting the
    // plane.

    let mut open_polylines: Vec<OpenPolyline> = Vec::new();
    chain_lines_by_triangle_connectivity(lines, &mut loops, &mut open_polylines);

    // Now process the open polylines.
    // Do it in two rounds, first try to connect in the same direction only,
    // then try to connect the open polylines in reversed order as well.
    chain_open_polylines_exact(&mut open_polylines, &mut loops, false);
    chain_open_polylines_exact(&mut open_polylines, &mut loops, true);

    // Try to close gaps.
    // Do it in two rounds, first try to connect in the same direction only,
    // then try to connect the open polylines in reversed order as well.
    let max_gap = 2.0; //mm
    chain_open_polylines_close_gaps(&mut open_polylines, &mut loops, max_gap, false);
    chain_open_polylines_close_gaps(&mut open_polylines, &mut loops, max_gap, true);

    loops
}

/// `make_loops(lines, params, throw_on_cancel)`: every layer's lines chained into loops in a
/// `tbb::parallel_for` over `blocked_range<size_t>`, each loop then oriented by the layer's mode
/// (`mode_below` under `slicing_mode_normal_below_layer`). `throw_on_cancel` is called at every
/// layer whose index is a multiple of 65536.
fn make_loops_layers<C: Fn() + Sync>(
    lines: Vec<IntersectionLines>,
    params: &MeshSlicingParams,
    throw_on_cancel: &C,
) -> Vec<Polygons> {
    let mut work: Vec<(usize, IntersectionLines, Polygons)> = lines
        .into_iter()
        .enumerate()
        .map(|(i, l)| (i, l, Polygons::new()))
        .collect();
    parallel_for_each_mut(&mut work, |(line_idx, lines, polygons)| {
        let line_idx = *line_idx;
        if (line_idx & 0x0ffff) == 0 {
            throw_on_cancel();
        }

        *polygons = make_loops(lines);

        let this_mode = if line_idx < params.slicing_mode_normal_below_layer {
            params.mode_below
        } else {
            params.mode
        };
        if !polygons.is_empty() {
            if this_mode == SlicingMode::Positive {
                // Reorient all loops to be CCW.
                for p in polygons.iter_mut() {
                    p.make_counter_clockwise();
                }
            } else if this_mode == SlicingMode::PositiveLargestContour {
                // Keep just the largest polygon, make it CCW.
                let mut max_area = 0.0;
                let mut max_area_polygon: Option<usize> = None;
                for (i, p) in polygons.iter().enumerate() {
                    let a = p.area();
                    if fabs(a) > fabs(max_area) {
                        max_area = a;
                        max_area_polygon = Some(i);
                    }
                }
                // Where every loop has an area of 0 the C++ dereferences its null pointer; the
                // first loop is kept as it is (the module documentation).
                let mut p = polygons.swap_remove(max_area_polygon.unwrap_or(0));
                if max_area < 0.0 {
                    p.reverse();
                }
                polygons.clear();
                polygons.push(p);
            }
        }
    });
    work.into_iter().map(|(_, _, p)| p).collect()
}

/// `make_slab_loops<ProjectionFromTop>(lines, num_edges, throw_on_cancel)`, used by
/// [`slice_mesh_slabs`] to produce loops from the lines at the slices and between them: for each
/// slab, the lines of the plane below that are not `Top` edges, the lines between the planes, and
/// the lines of the plane above that are not `Bottom` edges, reversed and with their edge ids moved
/// past `num_edges`, chained by triangle connectivity; open polylines are dropped with a trace
/// record. The slabs run in a `tbb::parallel_for` over `blocked_range<int>`, each writing its own
/// loops; `throw_on_cancel` is called at every slab whose index is a multiple of 65536.
fn make_slab_loops<const PROJECTION_FROM_TOP: bool, C: Fn() + Sync>(
    lines: &SlabLines,
    // To differentiate edge IDs of the top plane from the edge IDs of the bottom plane for
    // chaining.
    num_edges: i32,
    throw_on_cancel: &C,
) -> Vec<Polygons> {
    let count = lines.at_slice.len();
    let mut layers: Vec<(usize, Polygons)> = (0..count).map(|i| (i, Polygons::new())).collect();
    parallel_for_each_mut(&mut layers, |(line_idx, loops)| {
        let line_idx = *line_idx;
        if (line_idx & 0x0ffff) == 0 {
            throw_on_cancel();
        }
        let mut input = IntersectionLines::new();
        let mut nlines = lines.between_slices[line_idx].len();
        let slice_below = if PROJECTION_FROM_TOP {
            line_idx
        } else {
            line_idx.wrapping_sub(1)
        };
        let slice_above = if PROJECTION_FROM_TOP {
            line_idx + 1
        } else {
            line_idx
        };
        let has_slice_below = PROJECTION_FROM_TOP || line_idx > 0;
        let has_slice_above = !PROJECTION_FROM_TOP || line_idx + 1 < count;
        if has_slice_below {
            nlines += lines.at_slice[slice_below].len();
        }
        if has_slice_above {
            nlines += lines.at_slice[slice_above].len();
        }
        if nlines != 0 {
            input.reserve(nlines);
            if has_slice_below {
                for l in &lines.at_slice[slice_below] {
                    if l.edge_type != crate::triangle_mesh_slicer::FacetEdgeType::Top {
                        input.push(*l);
                    }
                }
            }
            // Edges in between slice_below and slice_above.
            // Edge IDs of end points on in-between lines that touch the layer above are already
            // increased with num_edges.
            input.extend_from_slice(&lines.between_slices[line_idx]);
            if has_slice_above {
                for lsrc in &lines.at_slice[slice_above] {
                    if lsrc.edge_type != crate::triangle_mesh_slicer::FacetEdgeType::Bottom {
                        let mut l = *lsrc;
                        l.reverse();
                        // Differentiate edge IDs of the top plane from the edge IDs of the bottom
                        // plane for chaining.
                        if l.edge_a_id >= 0 {
                            l.edge_a_id = l.edge_a_id.wrapping_add(num_edges);
                        }
                        if l.edge_b_id >= 0 {
                            l.edge_b_id = l.edge_b_id.wrapping_add(num_edges);
                        }
                        input.push(l);
                    }
                }
            }
            if !input.is_empty() {
                let mut open_polylines: Vec<OpenPolyline> = Vec::new();
                chain_lines_by_triangle_connectivity(&mut input, loops, &mut open_polylines);
                if !open_polylines.is_empty() {
                    log(
                        Severity::Trace,
                        &format!(
                            "make_slab_loops - chaining failed. #{} open polylines",
                            open_polylines.len()
                        ),
                    );
                }
            }
        }
    });
    layers.into_iter().map(|(_, l)| l).collect()
}

/// `make_expolygons(loops, closing_radius, extra_offset, fill_type, slices)`: the loops' union by
/// `fill_type`, closed by `closing_radius` (out by it, then in by it less `extra_offset`) or grown
/// by `extra_offset` when that is larger, appended to `slices`. A negative `extra_offset` shrinks
/// the contours.
fn make_expolygons(
    loops: &[Polygon],
    closing_radius: f32,
    extra_offset: f32,
    fill_type: PolyFillType,
    slices: &mut ExPolygons,
) {
    // Perform a safety offset to merge very close facets (TODO: find test case for this)
    // 0.0499 comes from https://github.com/slic3r/Slic3r/issues/959
    // 0.0001 is set to satisfy GH #520, #1029, #1364
    // Allowing negative extra_offset for shrinking a contour. This likely only makes sense if
    // slicing a single region only.
    let offset_out: f64;
    let offset_in: f64;
    if closing_radius >= extra_offset {
        offset_out = scale_(f64::from(closing_radius));
        offset_in = -scale_(f64::from(closing_radius - extra_offset));
    } else {
        offset_out = scale_(f64::from(extra_offset));
        offset_in = 0.0;
    }

    // append to the supplied collection
    let union = || union_ex(loops, fill_type);
    let result = if offset_out > 0.0 && offset_in < 0.0 {
        offset2_ex(
            &union(),
            offset_out as f32,
            offset_in as f32,
            DEFAULT_JOIN_TYPE,
            DEFAULT_MITER_LIMIT,
        )
    } else if offset_out > 0.0 {
        offset_ex_expolygons(
            &union(),
            offset_out as f32,
            DEFAULT_JOIN_TYPE,
            DEFAULT_MITER_LIMIT,
        )
    } else if offset_in < 0.0 {
        offset_ex_expolygons(
            &union(),
            offset_in as f32,
            DEFAULT_JOIN_TYPE,
            DEFAULT_MITER_LIMIT,
        )
    } else {
        union()
    };
    slices.extend(result);
}

/// `make_trafo_for_slicing(trafo)`: a trafo for transforming the vertices, scaled up in XY, not in
/// Z, cast to `float`.
fn make_trafo_for_slicing(trafo: &Transform3d) -> Transform3f {
    let mut t = *trafo;
    let s = 1.0 / SCALING_FACTOR;
    t.prescale(&Vec3d::new(s, s, 1.0));
    t.cast::<f32>()
}

/// `is_identity(trafo)`: every coefficient of the matrix equal to the identity's.
fn is_identity(trafo: &Transform3d) -> bool {
    *trafo == Transform3d::identity()
}

/// `transform_mesh_vertices_for_slicing(mesh, trafo)`: the vertices copied and scaled in XY (each
/// `x` and `y` times `float(1 / SCALING_FACTOR)`), not in Z; or transformed by the trafo prescaled
/// in XY and cast to `float`.
fn transform_mesh_vertices_for_slicing(
    mesh: &IndexedTriangleSet,
    trafo: &Transform3d,
) -> Vec<StlVertex> {
    // Copy and scale vertices in XY, don't scale in Z.
    // Possibly apply the transformation.
    let s = 1.0 / SCALING_FACTOR;
    let mut out: Vec<StlVertex> = mesh.vertices.clone();
    if is_identity(trafo) {
        // Identity.
        let sf = s as f32;
        for v in &mut out {
            // Scale just XY, leave Z unscaled.
            v.0[0] *= sf;
            v.0[1] *= sf;
        }
    } else {
        // Transform the vertices, scale up in XY, not in Y.
        let mut t = *trafo;
        t.prescale(&Vec3d::new(s, s, 1.0));
        let tf = t.cast::<f32>();
        for v in &mut out {
            *v = tf.mul_point(v);
        }
    }
    out
}

/// The lambda of the identity paths of [`slice_mesh`] and [`slice_mesh_plane`]:
/// `Vec3f(scaled<float>(p.x()), scaled<float>(p.y()), p.z())`, each a `float` division by
/// `float(SCALING_FACTOR)`.
fn scale_xy(p: Vec3f) -> Vec3f {
    let sf = SCALING_FACTOR as f32;
    Vec3f::new(p.x() / sf, p.y() / sf, p.z())
}

/// `slice_mesh(mesh, zs, params, throw_on_cancel)`: the polygons of the mesh at each height of
/// `zs` (unscaled, sorted), one list per height, each loop oriented by the layer's mode (see the
/// module documentation). `throw_on_cancel` is the C++'s default `[]{}` when given `&|| ()`.
pub fn slice_mesh<C: Fn() + Sync>(
    mesh: &IndexedTriangleSet,
    // Unscaled Zs
    zs: &[f32],
    params: &MeshSlicingParams,
    throw_on_cancel: &C,
) -> Vec<Polygons> {
    log(Severity::Debug, "slice_mesh to polygons");

    let lines: Vec<IntersectionLines>;

    {
        //FIXME facets_edges is likely not needed and quite costly to calculate.
        // Instead of edge identifiers, one shall use a sorted pair of edge vertex indices.
        // However facets_edges assigns a single edge ID to two triangles only, thus when factoring
        // facets_edges out, one will have to make sure that no code relies on it.
        let face_edge_ids = its_face_edge_ids(mesh);
        if zs.len() <= 1 {
            // It likely is not worthwile to copy the vertices. Apply the transformation in place.
            if is_identity(&params.trafo) {
                lines = slice_make_lines(
                    &mesh.vertices,
                    &scale_xy,
                    &mesh.indices,
                    &face_edge_ids,
                    zs,
                    throw_on_cancel,
                );
            } else {
                // Transform the vertices, scale up in XY, not in Z.
                let tf = make_trafo_for_slicing(&params.trafo);
                lines = slice_make_lines(
                    &mesh.vertices,
                    &|p: Vec3f| tf.mul_point(&p),
                    &mesh.indices,
                    &face_edge_ids,
                    zs,
                    throw_on_cancel,
                );
            }
        } else {
            // Copy and scale vertices in XY, don't scale in Z. Possibly apply the
            // transformation.
            lines = slice_make_lines(
                &transform_mesh_vertices_for_slicing(mesh, &params.trafo),
                &|p: Vec3f| p,
                &mesh.indices,
                &face_edge_ids,
                zs,
                throw_on_cancel,
            );
        }
    }

    throw_on_cancel();

    make_loops_layers(lines, params, throw_on_cancel)
}

/// `slice_mesh(mesh, plane_z, params)`, the specialized version for a single slicing plane only,
/// running on a single thread: the vertices marked below, on or above the plane, the faces that
/// touch it given edge ids (`its_face_edge_ids` with that mask), their lines chained into the
/// layer's polygons.
#[must_use]
pub fn slice_mesh_plane(
    mesh: &IndexedTriangleSet,
    // Unscaled Zs
    plane_z: f32,
    params: &MeshSlicingParams,
) -> Polygons {
    let mut lines: Vec<IntersectionLines> = Vec::new();

    {
        let trafo_identity = is_identity(&params.trafo);
        // The C++ leaves `tf` uninitialised on the identity path, which never reads it.
        let mut tf = Transform3f::identity();
        let mut face_mask = vec![false; mesh.indices.len()];

        {
            // 1) Mark vertices as below or above the slicing plane.
            let side = |z: f32| -> i8 {
                if z < plane_z {
                    -1
                } else if z == plane_z {
                    0
                } else {
                    1
                }
            };
            let mut vertex_side = vec![0_i8; mesh.vertices.len()];
            if trafo_identity {
                for (s, v) in vertex_side.iter_mut().zip(&mesh.vertices) {
                    *s = side(v.z());
                }
            } else {
                tf = make_trafo_for_slicing(&params.trafo);
                for (s, v) in vertex_side.iter_mut().zip(&mesh.vertices) {
                    //FIXME don't need to transform x & y, just Z.
                    *s = side(tf.mul_point(v).z());
                }
            }

            // 2) Mark faces crossing the plane.
            for (mask, face) in face_mask.iter_mut().zip(&mesh.indices) {
                let sides = [
                    i32::from(vertex_side[face[0] as usize]),
                    i32::from(vertex_side[face[1] as usize]),
                    i32::from(vertex_side[face[2] as usize]),
                ];
                *mask = sides[0] * sides[1] <= 0
                    || sides[1] * sides[2] <= 0
                    || sides[0] * sides[2] <= 0;
            }
        }

        // 3) Calculate face neighbors for just the faces in face_mask.
        let face_edge_ids = its_face_edge_ids_masked(mesh, &face_mask);

        // 4) Slice "face_mask" triangles, collect line segments.
        // It likely is not worthwile to copy the vertices. Apply the transformation in place.
        if trafo_identity {
            lines.push(slice_make_lines_plane(
                &mesh.vertices,
                &scale_xy,
                &mesh.indices,
                &face_edge_ids,
                plane_z,
                |face_idx| face_mask[face_idx as usize],
            ));
        } else {
            // Transform the vertices, scale up in XY, not in Z.
            lines.push(slice_make_lines_plane(
                &mesh.vertices,
                &|p: Vec3f| tf.mul_point(&p),
                &mesh.indices,
                &face_edge_ids,
                plane_z,
                |face_idx| face_mask[face_idx as usize],
            ));
        }
    }

    // 5) Chain the line segments.
    let layers = make_loops_layers(lines, params, &|| ());
    layers.into_iter().next().unwrap_or_default()
}

/// The fill type `slice_mesh_ex` gives `make_expolygons` for a layer's mode.
fn fill_type_of(mode: SlicingMode) -> PolyFillType {
    if mode == SlicingMode::EvenOdd {
        PolyFillType::EvenOdd
    } else if mode == SlicingMode::PositiveLargestContour {
        PolyFillType::Positive
    } else {
        PolyFillType::NonZero
    }
}

/// `slice_mesh_ex(mesh, zs, params, throw_on_cancel)`: the expolygons of the mesh at each height of
/// `zs` (unscaled, sorted), one list per height (see the module documentation).
pub fn slice_mesh_ex<C: Fn() + Sync>(
    mesh: &IndexedTriangleSet,
    zs: &[f32],
    params: &MeshSlicingParamsEx,
    throw_on_cancel: &C,
) -> Vec<ExPolygons> {
    let layers_p: Vec<Polygons>;
    {
        let mut slicing_params = params.params;
        if params.params.mode == SlicingMode::PositiveLargestContour {
            slicing_params.mode = SlicingMode::Positive;
        }
        if params.params.mode_below == SlicingMode::PositiveLargestContour {
            slicing_params.mode_below = SlicingMode::Positive;
        }
        layers_p = slice_mesh(mesh, zs, &slicing_params, throw_on_cancel);
    }

    let mut layers: Vec<(usize, ExPolygons)> = (0..layers_p.len())
        .map(|i| (i, ExPolygons::new()))
        .collect();
    parallel_for_each_mut(&mut layers, |(layer_id, expolygons)| {
        let resolution = scaled_f32(params.resolution);
        let layer_id = *layer_id;
        throw_on_cancel();
        let this_mode = if layer_id < params.params.slicing_mode_normal_below_layer {
            params.params.mode_below
        } else {
            params.params.mode
        };
        make_expolygons(
            &layers_p[layer_id],
            params.closing_radius,
            params.extra_offset,
            fill_type_of(this_mode),
            expolygons,
        );
        //FIXME simplify
        if this_mode == SlicingMode::PositiveLargestContour {
            keep_largest_contour_only(expolygons);
        }
        if resolution != 0.0 {
            let mut simplified: ExPolygons = Vec::with_capacity(expolygons.len());
            for ex in expolygons.iter() {
                simplified.extend(ExPolygon::simplify(ex, f64::from(resolution)));
            }
            *expolygons = simplified;
        }
    });

    layers.into_iter().map(|(_, l)| l).collect()
}

/// `slice_mesh_ex(mesh, zs, throw_on_cancel)`, the header's inline overload: with
/// `MeshSlicingParamsEx{}`.
pub fn slice_mesh_ex_default<C: Fn() + Sync>(
    mesh: &IndexedTriangleSet,
    zs: &[f32],
    throw_on_cancel: &C,
) -> Vec<ExPolygons> {
    slice_mesh_ex(mesh, zs, &MeshSlicingParamsEx::default(), throw_on_cancel)
}

/// `slice_mesh_ex(mesh, zs, closing_radius, throw_on_cancel)`, the header's inline overload: with
/// `MeshSlicingParamsEx{}` and the closing radius given.
pub fn slice_mesh_ex_closing<C: Fn() + Sync>(
    mesh: &IndexedTriangleSet,
    zs: &[f32],
    closing_radius: f32,
    throw_on_cancel: &C,
) -> Vec<ExPolygons> {
    let params = MeshSlicingParamsEx {
        closing_radius,
        ..MeshSlicingParamsEx::default()
    };
    slice_mesh_ex(mesh, zs, &params, throw_on_cancel)
}

/// `slice_mesh_slabs(mesh, zs, trafo, out_top, out_bottom, vertical_points, throw_on_cancel)`:
/// slices a triangle set with a set of Z slabs (thick layers). The effect is similar to producing
/// the usual top / bottom layers from a sliced mesh by subtracting `layer[i]` from `layer[i - 1]`
/// for the top surfaces resp. subtracting `layer[i]` from `layer[i + 1]` for the bottom surfaces,
/// with the exception that the triangle set this function processes may not cover the whole top
/// resp. bottom surface. Top resp. bottom surfaces are calculated only if `out_top` resp.
/// `out_bottom` is given; `vertical_points`, when given, receives the centroid and the normal of
/// every vertical face, in the order of the faces.
pub fn slice_mesh_slabs<C: Fn() + Sync>(
    mesh: &IndexedTriangleSet,
    // Unscaled Zs
    zs: &[f32],
    trafo: &Transform3d,
    out_top: Option<&mut Vec<Polygons>>,
    out_bottom: Option<&mut Vec<Polygons>>,
    mut vertical_points: Option<&mut Vec<(Vec3f, Vec3f)>>,
    throw_on_cancel: &C,
) {
    log(Severity::Debug, "slice_mesh_slabs to polygons");

    let vertices_transformed = transform_mesh_vertices_for_slicing(mesh, trafo);
    let mirrored_sign: i64 = if dynamic_block_determinant(&trafo.linear()) < 0.0 {
        -1
    } else {
        1
    };

    let mut face_orientation = vec![FaceOrientation::Up; mesh.indices.len()];
    for (fo_out, tri) in face_orientation.iter_mut().zip(&mesh.indices) {
        let fa = vertices_transformed[tri[0] as usize];
        let fb = vertices_transformed[tri[1] as usize];
        let fc = vertices_transformed[tri[2] as usize];
        let a = Point::new(f32_to_i64(fa.x()), f32_to_i64(fa.y()));
        let b = Point::new(f32_to_i64(fb.x()), f32_to_i64(fb.y()));
        let c = Point::new(f32_to_i64(fc.x()), f32_to_i64(fc.y()));
        let d: i64 = cross2_coord(b - a, c - b).wrapping_mul(mirrored_sign);
        let fo = if d > 0 {
            FaceOrientation::Up
        } else if d < 0 {
            FaceOrientation::Down
        } else {
            // Is the triangle vertical or degenerate?
            let fo = if fa == fb || fa == fc || fb == fc {
                FaceOrientation::Degenerate
            } else {
                FaceOrientation::Vertical
            };
            if let Some(vertical_points) = vertical_points.as_deref_mut()
                && fo == FaceOrientation::Vertical
            {
                let normal = (fb - fa).cross(&(fc - fa)).normalized();
                vertical_points.push(((fa + fb + fc) / 3.0, normal));
            }
            fo
        };
        *fo_out = fo;
    }

    let face_neighbors = its_face_neighbors_par(mesh);
    let mut num_edges: i32 = 0;
    let face_edge_ids =
        its_face_edge_ids_from_neighbors(mesh, &face_neighbors, true, Some(&mut num_edges));
    let lines = slice_slabs_make_lines(
        &vertices_transformed,
        &mesh.indices,
        &face_neighbors,
        &face_edge_ids,
        num_edges,
        &face_orientation,
        zs,
        out_top.is_some(),
        out_bottom.is_some(),
        throw_on_cancel,
    );

    throw_on_cancel();

    if let Some(out_top) = out_top {
        *out_top = make_slab_loops::<true, C>(&lines.0, num_edges, throw_on_cancel);
    }
    if let Some(out_bottom) = out_bottom {
        *out_bottom = make_slab_loops::<false, C>(&lines.1, num_edges, throw_on_cancel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orca_eigen::Vector;
    use orca_mesh::admesh::StlTriangleVertexIndices;
    use orca_tbb::Arena;

    fn v(x: f32, y: f32, z: f32) -> Vec3f {
        Vector([x, y, z])
    }

    fn face(a: i32, b: i32, c: i32) -> StlTriangleVertexIndices {
        Vector([a, b, c])
    }

    /// A closed axis-aligned box from `lo` to `hi`, its faces wound outwards.
    fn cuboid(lo: [f32; 3], hi: [f32; 3]) -> IndexedTriangleSet {
        let vertices = vec![
            v(lo[0], lo[1], lo[2]),
            v(hi[0], lo[1], lo[2]),
            v(hi[0], hi[1], lo[2]),
            v(lo[0], hi[1], lo[2]),
            v(lo[0], lo[1], hi[2]),
            v(hi[0], lo[1], hi[2]),
            v(hi[0], hi[1], hi[2]),
            v(lo[0], hi[1], hi[2]),
        ];
        let indices = vec![
            face(0, 2, 1),
            face(0, 3, 2),
            face(4, 5, 6),
            face(4, 6, 7),
            face(0, 1, 5),
            face(0, 5, 4),
            face(1, 2, 6),
            face(1, 6, 5),
            face(2, 3, 7),
            face(2, 7, 6),
            face(3, 0, 4),
            face(3, 4, 7),
        ];
        IndexedTriangleSet::new(indices, vertices)
    }

    #[test]
    fn a_box_slices_into_one_square_per_layer() {
        let mesh = cuboid([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let zs = [0.5_f32, 5.0, 9.5];
        let layers = slice_mesh(&mesh, &zs, &MeshSlicingParams::default(), &|| ());
        assert_eq!(layers.len(), 3);
        for l in &layers {
            assert_eq!(l.len(), 1);
            assert!(l[0].area() > 0.0);
            assert!((l[0].area() - 1e14).abs() < 1.0);
        }
        // One plane gives the same loop as the list of heights at that plane, up to its start.
        let one = slice_mesh_plane(&mesh, 5.0, &MeshSlicingParams::default());
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].area(), layers[1][0].area());
    }

    #[test]
    fn a_flipped_box_gives_clockwise_loops_unless_positive() {
        let mut mesh = cuboid([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        orca_mesh::its::its_flip_triangles(&mut mesh);
        let zs = [2.0_f32, 4.0];
        let regular = slice_mesh(&mesh, &zs, &MeshSlicingParams::default(), &|| ());
        assert!(regular.iter().all(|l| l.len() == 1 && l[0].area() < 0.0));
        let params = MeshSlicingParams {
            mode: SlicingMode::Positive,
            ..MeshSlicingParams::default()
        };
        let positive = slice_mesh(&mesh, &zs, &params, &|| ());
        assert!(positive.iter().all(|l| l.len() == 1 && l[0].area() > 0.0));
    }

    #[test]
    fn largest_contour_keeps_the_biggest_loop() {
        let mut mesh = cuboid([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let small = cuboid([20.0, 0.0, 0.0], [22.0, 2.0, 10.0]);
        let base = mesh.vertices.len() as i32;
        mesh.vertices.extend(small.vertices.iter().copied());
        mesh.indices.extend(
            small
                .indices
                .iter()
                .map(|f| face(f[0] + base, f[1] + base, f[2] + base)),
        );
        let zs = [5.0_f32];
        let all = slice_mesh(&mesh, &zs, &MeshSlicingParams::default(), &|| ());
        assert_eq!(all[0].len(), 2);
        let params = MeshSlicingParams {
            mode: SlicingMode::PositiveLargestContour,
            ..MeshSlicingParams::default()
        };
        let largest = slice_mesh(&mesh, &zs, &params, &|| ());
        assert_eq!(largest[0].len(), 1);
        assert!((largest[0][0].area() - 1e14).abs() < 1.0);
        // slice_mesh_ex keeps the largest contour itself.
        let ex = slice_mesh_ex(
            &mesh,
            &zs,
            &MeshSlicingParamsEx {
                params,
                ..MeshSlicingParamsEx::default()
            },
            &|| (),
        );
        assert_eq!(ex[0].len(), 1);
    }

    #[test]
    fn open_polylines_close_across_a_gap() {
        // Four lines of a square with a 0.5 mm gap at one corner, no shared ids: the exact rounds
        // join nothing and the gap round closes them into one loop.
        let mk = |a: (i64, i64), b: (i64, i64), ea: i32, eb: i32| {
            IntersectionLine::new(
                orca_geom::line::Line::new(Point::new(a.0, a.1), Point::new(b.0, b.1)),
                -1,
                -1,
                ea,
                eb,
                crate::triangle_mesh_slicer::FacetEdgeType::General,
            )
        };
        let m = 1_000_000;
        let mut lines = vec![
            mk((0, 0), (10 * m, 0), 1, 2),
            mk((10 * m, 0), (10 * m, 10 * m), 2, 3),
            mk((10 * m, 10 * m), (0, 10 * m), 3, 4),
            mk((0, 10 * m), (0, m / 2), 4, 5),
        ];
        let loops = make_loops(&mut lines);
        assert_eq!(loops.len(), 1);
        assert_eq!(loops[0].points.len(), 5);
        assert!(loops[0].area() > 0.0);
    }

    #[test]
    fn the_layers_are_the_same_in_any_arena() {
        let mesh = cuboid([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let zs: Vec<f32> = (0..40).map(|i| 0.1 + i as f32 * 0.25).collect();
        let params = MeshSlicingParamsEx {
            closing_radius: 0.049,
            resolution: 0.0025,
            ..MeshSlicingParamsEx::default()
        };
        let serial = Arena::serial(8).execute(|| slice_mesh_ex(&mesh, &zs, &params, &|| ()));
        let parallel = Arena::new(4)
            .unwrap()
            .execute(|| slice_mesh_ex(&mesh, &zs, &params, &|| ()));
        assert_eq!(serial, parallel);
        assert!(serial.iter().all(|l| l.len() == 1));
    }

    #[test]
    fn a_box_has_one_top_and_one_bottom_slab() {
        let mesh = cuboid([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let zs = [0.5_f32, 5.0, 9.5];
        let mut top = Vec::new();
        let mut bottom = Vec::new();
        let mut vertical = Vec::new();
        slice_mesh_slabs(
            &mesh,
            &zs,
            &Transform3d::identity(),
            Some(&mut top),
            Some(&mut bottom),
            Some(&mut vertical),
            &|| (),
        );
        assert_eq!(top.len(), 3);
        assert_eq!(bottom.len(), 3);
        // The eight side faces are vertical, each with a horizontal unit normal.
        assert_eq!(vertical.len(), 8);
        for (_, n) in &vertical {
            assert_eq!(n.z(), 0.0);
            assert!((n.squared_norm() - 1.0).abs() < 1e-6);
        }
        // Only the bottom surfaces asked: the same bottoms.
        let mut bottom_only = Vec::new();
        slice_mesh_slabs(
            &mesh,
            &zs,
            &Transform3d::identity(),
            None,
            Some(&mut bottom_only),
            None,
            &|| (),
        );
        assert_eq!(bottom_only, bottom);
    }
}
