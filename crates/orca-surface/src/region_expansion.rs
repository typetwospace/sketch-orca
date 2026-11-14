// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Algorithm/RegionExpansion.hpp lines 1-118 and
// src/libslic3r/Algorithm/RegionExpansion.cpp lines 1-597 of OrcaSlicer v2.4.2, whole, translated
// on 2026-11-13 by orca-o3-surfaces-1. Copyright the OrcaSlicer authors, the BambuStudio authors
// (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU Affero General Public
// License, version 3 (AGPL-3.0; LICENSE).

//! `Slic3r::Algorithm` region expansion: grows source regions into the boundary regions they touch
//! by waves, as `LayerRegion::process_external_surfaces` grows bridges and bottoms into the solid
//! infill, the sparse infill and the tops around them.
//!
//! - [`RegionExpansionParameters::build`] splits an expansion into a tiny first expansion and
//!   equal steps, in `float` where the C++ is `float` and `double` where it mixes in a `double`.
//! - [`wave_seeds`] expands each source contour by the tiny expansion (a square-joined
//!   `ClipperOffset`), opens it (the first point repeated at the end), gives every point the z of
//!   its source (boundaries from 1, then the sources) and clips the open paths by the closed
//!   boundaries with ClipperLib_Z; the Z fill visitor (`crate::clipper_z_utils`) records which
//!   boundary and which source cross at each new point. `merge_splits` joins the pieces cut at the
//!   seam where a contour was opened. Each piece becomes a [`WaveSeed`] tagged with its source and
//!   boundary: from an intersection point at one of its ends; from the z of its points when both
//!   ends are vertices of the input; or, for a closed piece that touches no boundary, from the
//!   boundary whose contour holds its first point, found with an AABB tree over the boundaries'
//!   boxes (`orca_index::aabb_tree`, built once when first needed).
//! - [`propagate_waves`] takes the seeds of one source and boundary at a time, inflates them by
//!   the initial step (round joins; a closed seed as a closed line, an open one with round ends),
//!   clips with the boundary trimmed to the seeds' box grown by the maximum inflation, then
//!   inflates and clips again for each further step (each polygon by its own orientation, the
//!   holes' results reversed). [`propagate_waves_ex`] unites each group's polygons into
//!   expolygons; [`expand_expolygons`] groups the polygons by source.
//! - [`merge_expansions_into_expolygons`] sorts the expansions by source (`std::sort`, not stable:
//!   ties fall as MSVC's introsort leaves them, `orca_stl::sort_by`) and unites each source with
//!   its expansions by a safety offset; when that gives more than one expolygon, the one holding
//!   the source contour's first point is kept (the AABB tree again).
//!
//! The overloads become functions named for their arguments: [`propagate_waves`] (the seeds),
//! [`propagate_waves_src`] (the source and the parameters), [`propagate_waves_steps`] (the source,
//! an expansion, a step and a most number of steps), and [`propagate_waves_ex`] and
//! [`propagate_waves_ex_steps`]. [`clipper_round_offset_error`] is translated though nothing in
//! the engine calls it (its one use is commented out in `build`). The `reserve` and
//! `reserve_more_power_of_2` calls change nothing that can be read and are left out. The input
//! the C++ has no answer for panics: an empty path (`front()` of an empty vector), a seed index
//! or source id past its vector, and an expansion whose source has no contour.
//!
//! Domain A reaches this code only through LayerRegion's bridge and surface expansion
//! (orca-o3-surfaces-3), which reads no setting here, so nothing in it stops with `NotPorted`.

use orca_eigen::Vector;
use orca_geom::bounding_box::get_extents_vec;
use orca_geom::clipper::{
    self, ClipType, Clipper, ClipperOffset, EndType, JoinType, Path, Paths, PolyFillType, PolyType,
};
use orca_geom::clipper_utils::{
    CLIPPER_OFFSET_SHORTEST_EDGE_FACTOR, clip_clipper_polygons_with_subject_bbox_expolygon,
    to_polygons, union_ex, union_safety_offset_ex,
};
use orca_geom::clipper_z;
use orca_geom::expolygon::{self, ExPolygon, ExPolygons};
use orca_geom::point::{Coord, PI, Point, Points, SCALING_FACTOR};
use orca_geom::polygon::{self, Polygon, Polygons};
use orca_index::aabb_tree::{self, BoundingBoxWrapper, Tree};

use crate::clipper_z_utils::{
    ClipperZIntersectionVisitor, Intersections, ZPaths, ZPoint, expolygons_to_zpaths, from_zpath,
    polylines_merge_z, to_zpaths, zpoint_lower,
};

/// `clipper_round_offset_error(offset, arc_tolerance)`: the radius error of ClipperLib's round
/// offset, as `ClipperOffset::DoOffset` discretises it. Nothing in the engine calls it.
#[must_use]
pub fn clipper_round_offset_error(offset: f64, arc_tolerance: f64) -> f64 {
    const DEF_ARC_TOLERANCE: f64 = 0.25;
    let y = if arc_tolerance <= 0. {
        DEF_ARC_TOLERANCE
    } else if arc_tolerance > offset * DEF_ARC_TOLERANCE {
        offset * DEF_ARC_TOLERANCE
    } else {
        arc_tolerance
    };
    let steps = orca_stl::min(PI / orca_crt::acos(1. - y / offset), offset * PI);
    offset * (1. - orca_crt::cos(PI / steps))
}

/// `RegionExpansionParameters`. The C++ leaves a default-made one uninitialised; `Default` gives
/// zeros.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RegionExpansionParameters {
    /// `tiny_expansion`: the first expansion of the source, so that it overlaps the boundary a bit.
    pub tiny_expansion: f32,
    /// `initial_step`: how much the seed lines are inflated into the first wave.
    pub initial_step: f32,
    /// `other_step`: how much each later wave is inflated.
    pub other_step: f32,
    /// `num_other_steps`: the number of steps after the first.
    pub num_other_steps: usize,
    /// `max_inflation`: how far the waves can reach past the seeds, to trim the boundary by.
    pub max_inflation: f32,
    /// `arc_tolerance`: the offsetter's accuracy during the propagation.
    pub arc_tolerance: f64,
    /// `shortest_edge_length`.
    pub shortest_edge_length: f64,
}

impl RegionExpansionParameters {
    /// `RegionExpansionParameters::build(full_expansion, expansion_step, max_nr_expansion_steps)`:
    /// all three scaled; `max_nr_expansion_steps` caps the number of steps for a small step.
    #[must_use]
    pub fn build(full_expansion: f32, expansion_step: f32, max_nr_expansion_steps: usize) -> Self {
        let mut out = Self::default();
        // Initial expansion of src to make the source regions intersect with boundary regions just
        // a bit: scaled<float>(0.05f), the division in float.
        let tiny_cap = 0.05_f32 / SCALING_FACTOR as f32;
        out.tiny_expansion = orca_stl::min(0.25_f32 * full_expansion, tiny_cap);
        let mut nsteps = orca_crt::f32_to_u64(orca_crt::ceilf(
            (full_expansion - out.tiny_expansion) / expansion_step,
        )) as usize;
        if max_nr_expansion_steps > 0 {
            nsteps = orca_stl::min(nsteps, max_nr_expansion_steps);
        }
        out.initial_step = (full_expansion - out.tiny_expansion) / nsteps as f32;
        if nsteps > 1 && 0.25 * f64::from(out.initial_step) < f64::from(out.tiny_expansion) {
            // Decrease the step size by lowering number of steps.
            let steps = orca_crt::floor(
                f64::from(full_expansion - out.tiny_expansion)
                    / (4. * f64::from(out.tiny_expansion)),
            );
            nsteps = orca_stl::max(1_usize, orca_crt::f64_to_u64(steps) as usize);
            out.initial_step = (full_expansion - out.tiny_expansion) / nsteps as f32;
        }
        if 0.25 * f64::from(out.initial_step) < f64::from(out.tiny_expansion) || nsteps == 1 {
            out.tiny_expansion = 0.2_f32 * full_expansion;
            out.initial_step = 0.8_f32 * full_expansion;
        }
        out.other_step = out.initial_step;
        out.num_other_steps = nsteps.wrapping_sub(1);

        // Accuracy of the offsetter for wave propagation.
        out.arc_tolerance = 0.1 / SCALING_FACTOR;
        out.shortest_edge_length =
            f64::from(out.initial_step) * CLIPPER_OFFSET_SHORTEST_EDGE_FACTOR;

        // Maximum inflation of seed contours over the boundary, with a little added.
        out.max_inflation =
            (f64::from(out.tiny_expansion + nsteps as f32 * out.initial_step) * 1.1) as f32;

        out
    }
}

/// `WaveSeed`: a seed path of the source `src` touching the boundary `boundary`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WaveSeed {
    /// `src`.
    pub src: u32,
    /// `boundary`.
    pub boundary: u32,
    /// `path`.
    pub path: Points,
}

/// `WaveSeeds`.
pub type WaveSeeds = Vec<WaveSeed>;

/// `lower_by_boundary_and_src(l, r)`.
#[must_use]
pub fn lower_by_boundary_and_src(l: &WaveSeed, r: &WaveSeed) -> bool {
    l.boundary < r.boundary || (l.boundary == r.boundary && l.src < r.src)
}

/// `lower_by_src_and_boundary(l, r)`.
#[must_use]
pub fn lower_by_src_and_boundary(l: &WaveSeed, r: &WaveSeed) -> bool {
    l.src < r.src || (l.src == r.src && l.boundary < r.boundary)
}

/// `RegionExpansion`: a polygon grown from the source `src_id` into the boundary `boundary_id`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RegionExpansion {
    /// `polygon`.
    pub polygon: Polygon,
    /// `src_id`.
    pub src_id: u32,
    /// `boundary_id`.
    pub boundary_id: u32,
}

/// `RegionExpansionEx`: an expolygon grown from the source `src_id` into the boundary
/// `boundary_id`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RegionExpansionEx {
    /// `expolygon`.
    pub expolygon: ExPolygon,
    /// `src_id`.
    pub src_id: u32,
    /// `boundary_id`.
    pub boundary_id: u32,
}

/// `expolygons_to_zpaths_expanded_opened(src, expansion, base_idx)` (static): each contour of each
/// expolygon offset by `expansion` (a hole by `-expansion`, as `Execute` reorients the paths
/// first) with square joins and a shortest edge of `expansion` times
/// `ClipperOffsetShortestEdgeFactor`, then opened, with the z `base_idx`, which goes up by one for
/// each expolygon.
fn expolygons_to_zpaths_expanded_opened(
    src: &[ExPolygon],
    expansion: f32,
    base_idx: &mut Coord,
) -> ZPaths {
    let mut out = ZPaths::with_capacity(2 * src.iter().map(ExPolygon::num_contours).sum::<usize>());
    let mut offsetter = ClipperOffset::default();
    offsetter.shortest_edge_length = f64::from(expansion) * CLIPPER_OFFSET_SHORTEST_EDGE_FACTOR;
    let mut expansion_cache = Paths::new();
    for expoly in src {
        for icontour in 0..expoly.num_contours() {
            // Execute reorients the contours so that the outer most contour has a positive area.
            // Thus the output contours will be CCW oriented even though the input paths are CW
            // oriented. Offset is applied after contour reorientation, thus the signum of the
            // offset value is reversed.
            offsetter.clear();
            offsetter.add_path(
                &expoly.contour_or_hole(icontour).points,
                JoinType::Square,
                EndType::ClosedPolygon,
            );
            expansion_cache.clear();
            let delta = if icontour == 0 { expansion } else { -expansion };
            offsetter.execute(&mut expansion_cache, f64::from(delta));
            out.extend(to_zpaths(&expansion_cache, *base_idx, true));
        }
        *base_idx += 1;
    }
    out
}

/// `merge_splits(paths, splits)` (static): the clipped pieces whose ends lie at the seams where the
/// source contours were opened (`splits`, sorted by `zpoint_lower`, each with the index of the
/// first piece found there or -1) are joined to that first piece; a joined piece's place takes the
/// last path, which is looked at next.
fn merge_splits(paths: &mut ZPaths, splits: &mut [(ZPoint, i32)]) {
    let find_end = |splits: &[(ZPoint, i32)], pt: &ZPoint| -> Option<usize> {
        let it = orca_stl::lower_bound_by(splits, pt, |l, r| zpoint_lower(&l.0, r));
        (it != splits.len() && splits[it].0 == *pt).then_some(it)
    };
    let mut it_path = 0;
    while it_path < paths.len() {
        let mut merged = false;
        if paths[it_path].len() >= 2 {
            let front = paths[it_path][0];
            let back = paths[it_path][paths[it_path].len() - 1];
            // The path before clipping was supposed to cross the clipping boundary or be fully out
            // of it. Thus the clipped contour is supposed to become open, with one exception: the
            // anchor expands into a closed hole.
            if front.x != back.x || front.y != back.y {
                // Look up the ends in "splits", possibly join the contours.
                let mut end = find_end(splits, &front);
                let mut end_front = true;
                if end.is_none() {
                    end_front = false;
                    end = find_end(splits, &back);
                }
                if let Some(e) = end {
                    // This segment ends at a split point of the source closed contour before
                    // clipping.
                    if splits[e].1 == -1 {
                        // Open end was found, not matched yet.
                        splits[e].1 = it_path as i32;
                    } else {
                        // Open end was found and matched with end->second.
                        let other = usize::try_from(splits[e].1).unwrap_or_else(|_| {
                            panic!("merge_splits: a split's index is negative")
                        });
                        let path = core::mem::take(&mut paths[it_path]);
                        let other_front = paths[other][0] == splits[e].0;
                        polylines_merge_z(&mut paths[other], other_front, path, end_front);
                        if it_path + 1 == paths.len() {
                            paths.pop();
                            break;
                        }
                        if let Some(last) = paths.pop() {
                            paths[it_path] = last;
                        }
                        merged = true;
                    }
                }
            }
        }
        if !merged {
            it_path += 1;
        }
    }
}

/// `AABBTreeBBoxes`: `AABBTreeIndirect::Tree<2, coord_t>`.
pub type AabbTreeBBoxes = Tree<i64, 2>;

/// `build_aabb_tree_over_expolygons(expolygons)` (static): a tree over the boxes of the contours,
/// each grown by `SCALED_EPSILON`.
fn build_aabb_tree_over_expolygons(expolygons: &[ExPolygon]) -> AabbTreeBBoxes {
    // Calculate bounding boxes of internal slices.
    let mut bboxes: Vec<BoundingBoxWrapper> = expolygons
        .iter()
        .enumerate()
        .map(|(i, e)| BoundingBoxWrapper::new(i, &polygon::get_extents(&e.contour)))
        .collect();
    // Build AABB tree over bounding boxes of boundary expolygons.
    let mut out = AabbTreeBBoxes::new();
    out.build_modify_input(&mut bboxes);
    out
}

/// `sample_in_expolygons(aabb_tree, expolygons, sample)` (static): the index of the first
/// expolygon, in the tree's order, whose box and whose area (the boundary included) hold `sample`;
/// -1 for none.
fn sample_in_expolygons(
    aabb_tree: &AabbTreeBBoxes,
    expolygons: &[ExPolygon],
    sample: Point,
) -> i32 {
    let mut out = -1;
    let s = Vector([sample.x, sample.y]);
    aabb_tree::traverse(
        aabb_tree,
        |node| node.bbox.contains_point(&s),
        |node| {
            if expolygons[node.idx].contains(sample, true) {
                out = node.idx as i32;
                // Stop traversal.
                return false;
            }
            // Continue traversal.
            true
        },
    );
    out
}

/// `wave_seeds(src, boundary, tiny_expansion, sorted)`: the source contours expanded by
/// `tiny_expansion`, opened and trimmed by the boundaries, each piece tagged with its source and
/// boundary; sorted by boundary, then source (`std::sort`) when `sorted`.
#[must_use]
pub fn wave_seeds(
    src: &[ExPolygon],
    boundary: &[ExPolygon],
    tiny_expansion: f32,
    sorted: bool,
) -> WaveSeeds {
    if src.is_empty() || boundary.is_empty() {
        return WaveSeeds::new();
    }

    let mut intersections = Intersections::new();
    let idx_boundary_begin: Coord = 1;
    let mut idx_boundary_end: Coord = idx_boundary_begin;
    let mut idx_src_end: Coord;

    let mut segments: ZPaths;
    {
        let mut visitor = ClipperZIntersectionVisitor::new(&mut intersections);
        let mut zclipper = clipper_z::Clipper::new(0);
        zclipper.z_fill_function(move |e1bot, e1top, e2bot, e2top, pt| {
            visitor.visit(e1bot, e1top, e2bot, e2top, pt);
        });
        // As closed contours.
        let zboundary = expolygons_to_zpaths(boundary, &mut idx_boundary_end, false);
        zclipper.add_paths(zboundary.iter().map(Vec::as_slice), PolyType::Clip, true);
        // As open contours.
        let mut zsrc_splits: Vec<(ZPoint, i32)>;
        {
            idx_src_end = idx_boundary_end;
            let zsrc = expolygons_to_zpaths_expanded_opened(src, tiny_expansion, &mut idx_src_end);
            zclipper.add_paths(zsrc.iter().map(Vec::as_slice), PolyType::Subject, false);
            zsrc_splits = Vec::with_capacity(zsrc.len());
            for path in &zsrc {
                zsrc_splits.push((front_of(path), -1));
            }
            orca_stl::sort_by(&mut zsrc_splits, |l, r| zpoint_lower(&l.0, &r.0));
        }
        let mut polytree = clipper_z::PolyTree::new();
        zclipper.execute_tree(
            ClipType::Intersection,
            &mut polytree,
            PolyFillType::NonZero,
            PolyFillType::NonZero,
        );
        segments = clipper_z::poly_tree_to_paths(&polytree);
        merge_splits(&mut segments, &mut zsrc_splits);
    }

    // AABBTree over bounding boxes of boundaries. Only built if necessary, that is if any of the
    // seed contours is closed, thus there is no intersection point with the boundary and all Z
    // coordinates of the closed contour point to the source contour.
    let mut aabb_tree = AabbTreeBBoxes::new();

    // Sort paths into their respective islands. Each src x boundary will be processed (wave
    // expanded) independently. Multiple pieces of a single src may intersect the same boundary.
    let mut out = WaveSeeds::with_capacity(segments.len());
    for path in &segments {
        let mut front = front_of(path);
        let mut back = path[path.len() - 1];
        // Both ends of a seed segment are supposed to be inside a single boundary expolygon. With
        // complex geometry both end points may coincide with existing polygon vertices (z >= 0).
        if front != back && front.z >= 0 && back.z >= 0 {
            // Both end points coincide with existing polygon vertices, so the ZFillFunction
            // callback was never called. Determine src_id / boundary_id from the Z coordinates,
            // falling back to an AABB-tree point-in-polygon test for the boundary.
            let mut src_z: Coord = -1;
            let mut boundary_z: Coord = -1;
            // Scan all path points for the information we need.
            for point in path {
                if point.z >= idx_boundary_end && point.z < idx_src_end && src_z < 0 {
                    src_z = point.z;
                } else if point.z >= idx_boundary_begin
                    && point.z < idx_boundary_end
                    && boundary_z < 0
                {
                    boundary_z = point.z;
                }
                if src_z >= 0 && boundary_z >= 0 {
                    break;
                }
            }
            if src_z >= 0 {
                let src_id = (src_z - idx_boundary_end) as u32;
                if boundary_z >= 0 {
                    out.push(WaveSeed {
                        src: src_id,
                        boundary: (boundary_z - 1) as u32,
                        path: from_zpath(path, false),
                    });
                } else {
                    // Source ID known but boundary unknown: use the AABB tree.
                    if aabb_tree.is_empty() {
                        aabb_tree = build_aabb_tree_over_expolygons(boundary);
                    }
                    let boundary_id =
                        sample_in_expolygons(&aabb_tree, boundary, Point::new(front.x, front.y));
                    if boundary_id >= 0 {
                        out.push(WaveSeed {
                            src: src_id,
                            boundary: boundary_id as u32,
                            path: from_zpath(path, false),
                        });
                    }
                }
                continue;
            }
            // Unable to determine source ID: drop the segment.
            continue;
        } else if front == back && front.z < idx_boundary_end {
            // This should be a very rare exception (PrusaSlicer issue 12469): the segment is open,
            // yet its first point seems to be part of a boundary polygon. Take the last point with
            // a src polygon index.
            for point in path {
                if point.z >= idx_boundary_end {
                    front = *point;
                    back = *point;
                }
            }
        }

        let intersection_point_valid = |is: &(Coord, Coord)| {
            is.0 >= 1 && is.0 < idx_boundary_end && is.1 >= idx_boundary_end && is.1 < idx_src_end
        };
        let mut intersection: Option<(Coord, Coord)> = None;
        if front.z < 0 {
            let is = intersections[intersection_index(front.z)];
            if intersection_point_valid(&is) {
                intersection = Some(is);
            }
        }
        if intersection.is_none() && back.z < 0 {
            let is = intersections[intersection_index(back.z)];
            if intersection_point_valid(&is) {
                intersection = Some(is);
            }
        }
        if let Some(is) = intersection {
            // The path intersects the boundary contour at least at one side.
            out.push(WaveSeed {
                src: (is.1 - idx_boundary_end) as u32,
                boundary: (is.0 - 1) as u32,
                path: from_zpath(path, false),
            });
        } else {
            // This should be a closed contour. Find a source boundary expolygon of one sample of
            // this closed path.
            if aabb_tree.is_empty() {
                aabb_tree = build_aabb_tree_over_expolygons(boundary);
            }
            let boundary_id =
                sample_in_expolygons(&aabb_tree, boundary, Point::new(front.x, front.y));
            // Boundary that contains the sample point was found.
            if boundary_id >= 0 {
                out.push(WaveSeed {
                    src: (front.z - idx_boundary_end) as u32,
                    boundary: boundary_id as u32,
                    path: from_zpath(path, false),
                });
            }
        }
    }

    if sorted {
        // Sort the seeds by their intersection boundary and source contour.
        orca_stl::sort_by(&mut out, lower_by_boundary_and_src);
    }
    out
}

/// `path.front()`, or a panic where the C++ reads an empty vector's first element.
fn front_of(path: &[ZPoint]) -> ZPoint {
    *path
        .first()
        .unwrap_or_else(|| panic!("wave_seeds: an empty path (the C++ reads front() of it)"))
}

/// `intersections[-z - 1]`: the index of the intersection a negative z names.
fn intersection_index(z: Coord) -> usize {
    usize::try_from(z.wrapping_neg().wrapping_sub(1))
        .unwrap_or_else(|_| panic!("wave_seeds: the z {z} names no intersection"))
}

/// `wavefront_initial(co, polylines, offset)` (static): each seed inflated by `offset` with round
/// joins, a closed seed as a closed line and an open one with round ends.
fn wavefront_initial(co: &mut ClipperOffset, polylines: &[Path], offset: f32) -> Paths {
    let mut out = Paths::with_capacity(polylines.len());
    let mut out_this = Paths::new();
    for path in polylines {
        co.clear();
        assert!(
            !path.is_empty(),
            "wavefront_initial: an empty seed (the C++ reads front() of it)"
        );
        let closed = path[0] == path[path.len() - 1];
        co.add_path(
            path,
            JoinType::Round,
            if closed {
                EndType::ClosedLine
            } else {
                EndType::OpenRound
            },
        );
        co.execute(&mut out_this, f64::from(offset));
        out.append(&mut out_this);
    }
    out
}

/// `wavefront_step(co, polygons, offset)` (static): each polygon inflated by `offset` (by
/// `-offset` and its results reversed when it is clockwise) with round joins. Overlaps are left
/// for the clip that follows.
fn wavefront_step(co: &mut ClipperOffset, polygons: &[Path], offset: f32) -> Paths {
    let mut out = Paths::with_capacity(polygons.len());
    let mut out_this = Paths::new();
    for polygon in polygons {
        co.clear();
        // Execute reorients the contours so that the outer most contour has a positive area. Thus
        // the output contours will be CCW oriented even though the input paths are CW oriented.
        // Offset is applied after contour reorientation, thus the signum of the offset value is
        // reversed.
        co.add_path(polygon, JoinType::Round, EndType::ClosedPolygon);
        let ccw = clipper::orientation(polygon);
        co.execute(&mut out_this, f64::from(if ccw { offset } else { -offset }));
        if !ccw {
            // Reverse the resulting contours.
            for path in &mut out_this {
                path.reverse();
            }
        }
        out.append(&mut out_this);
    }
    out
}

/// `wavefront_clip(wavefront, clipping)` (static): the intersection, both fill types positive.
fn wavefront_clip(wavefront: &[Path], clipping: &[Polygon]) -> Paths {
    let mut clipper = Clipper::new(0);
    clipper.add_paths(wavefront.iter().map(Vec::as_slice), PolyType::Subject, true);
    clipper.add_paths(
        clipping.iter().map(|p| p.points.as_slice()),
        PolyType::Clip,
        true,
    );
    let mut out = Paths::new();
    clipper.execute(
        ClipType::Intersection,
        &mut out,
        PolyFillType::Positive,
        PolyFillType::Positive,
    );
    out
}

/// `propagate_wave_from_boundary(co, seed, boundary, initial_step, other_step, num_other_steps,
/// max_inflation)` (static): the seed's first wave and each further wave, clipped by the boundary
/// trimmed to the seed's box (the boundary included) grown by `max_inflation`.
fn propagate_wave_from_boundary(
    co: &mut ClipperOffset,
    seed: &[Path],
    boundary: &ExPolygon,
    initial_step: f32,
    other_step: f32,
    num_other_steps: usize,
    max_inflation: f32,
) -> Polygons {
    let bbox = get_extents_vec(seed, true).inflated(f64::from(max_inflation));
    let clipping = clip_clipper_polygons_with_subject_bbox_expolygon(boundary, &bbox, false);
    let mut polygons = wavefront_clip(&wavefront_initial(co, seed, initial_step), &clipping);
    // Now offset the remaining.
    for _ in 0..num_other_steps {
        polygons = wavefront_clip(&wavefront_step(co, &polygons, other_step), &clipping);
    }
    to_polygons(polygons)
}

/// `propagate_waves(seeds, boundary, params)`: the waves of each run of seeds with one boundary and
/// one source, in the seeds' order (sorted by boundary and source, as [`wave_seeds`] sorts them).
#[must_use]
pub fn propagate_waves(
    seeds: &[WaveSeed],
    boundary: &[ExPolygon],
    params: &RegionExpansionParameters,
) -> Vec<RegionExpansion> {
    let mut out = Vec::new();
    let mut paths = Paths::new();
    let mut co = ClipperOffset::default();
    co.arc_tolerance = params.arc_tolerance;
    co.shortest_edge_length = params.shortest_edge_length;
    let mut it_seed = 0;
    while it_seed < seeds.len() {
        let mut it = it_seed;
        paths.clear();
        while it < seeds.len()
            && seeds[it].boundary == seeds[it_seed].boundary
            && seeds[it].src == seeds[it_seed].src
        {
            paths.push(seeds[it].path.clone());
            it += 1;
        }
        // Propagate the wavefront while clipping it with the trimmed boundary.
        // Collect the expanded polygons, merge them with the source polygons.
        let seed = &seeds[it_seed];
        for polygon in propagate_wave_from_boundary(
            &mut co,
            &paths,
            &boundary[seed.boundary as usize],
            params.initial_step,
            params.other_step,
            params.num_other_steps,
            params.max_inflation,
        ) {
            out.push(RegionExpansion {
                polygon,
                src_id: seed.src,
                boundary_id: seed.boundary,
            });
        }
        it_seed = it;
    }
    out
}

/// `propagate_waves(src, boundary, params)`: from the sorted [`wave_seeds`].
#[must_use]
pub fn propagate_waves_src(
    src: &[ExPolygon],
    boundary: &[ExPolygon],
    params: &RegionExpansionParameters,
) -> Vec<RegionExpansion> {
    propagate_waves(
        &wave_seeds(src, boundary, params.tiny_expansion, true),
        boundary,
        params,
    )
}

/// `propagate_waves(src, boundary, expansion, expansion_step, max_nr_steps)`: with the parameters
/// [`RegionExpansionParameters::build`] makes.
#[must_use]
pub fn propagate_waves_steps(
    src: &[ExPolygon],
    boundary: &[ExPolygon],
    expansion: f32,
    expansion_step: f32,
    max_nr_steps: usize,
) -> Vec<RegionExpansion> {
    propagate_waves_src(
        src,
        boundary,
        &RegionExpansionParameters::build(expansion, expansion_step, max_nr_steps),
    )
}

/// `propagate_waves_ex(seeds, boundary, params)`: each run of one boundary and one source as one
/// expolygon when it is one polygon, else the union of its polygons (non-zero).
#[must_use]
pub fn propagate_waves_ex(
    seeds: &[WaveSeed],
    boundary: &[ExPolygon],
    params: &RegionExpansionParameters,
) -> Vec<RegionExpansionEx> {
    let mut expanded = propagate_waves(seeds, boundary, params);
    let mut acc = Polygons::new();
    let mut out = Vec::new();
    let mut it = 0;
    while it < expanded.len() {
        let mut it2 = it;
        acc.clear();
        while it2 < expanded.len()
            && expanded[it2].boundary_id == expanded[it].boundary_id
            && expanded[it2].src_id == expanded[it].src_id
        {
            acc.push(core::mem::take(&mut expanded[it2].polygon));
            it2 += 1;
        }
        let (src_id, boundary_id) = (expanded[it].src_id, expanded[it].boundary_id);
        let size = it2 - it;
        if size == 1 {
            out.push(RegionExpansionEx {
                expolygon: ExPolygon::new(core::mem::take(&mut acc[0])),
                src_id,
                boundary_id,
            });
        } else {
            let expolys = union_ex(&acc, PolyFillType::NonZero);
            for ex in expolys {
                out.push(RegionExpansionEx {
                    expolygon: ex,
                    src_id,
                    boundary_id,
                });
            }
        }
        it = it2;
    }
    out
}

/// `propagate_waves_ex(src, boundary, full_expansion, expansion_step, max_nr_expansion_steps)`.
#[must_use]
pub fn propagate_waves_ex_steps(
    src: &[ExPolygon],
    boundary: &[ExPolygon],
    full_expansion: f32,
    expansion_step: f32,
    max_nr_expansion_steps: usize,
) -> Vec<RegionExpansionEx> {
    let params =
        RegionExpansionParameters::build(full_expansion, expansion_step, max_nr_expansion_steps);
    propagate_waves_ex(
        &wave_seeds(src, boundary, params.tiny_expansion, true),
        boundary,
        &params,
    )
}

/// `expand_expolygons(src, boundary, expansion, expansion_step, max_nr_steps)`: the expanded
/// polygons of each source, by source.
#[must_use]
pub fn expand_expolygons(
    src: &[ExPolygon],
    boundary: &[ExPolygon],
    expansion: f32,
    expansion_step: f32,
    max_nr_steps: usize,
) -> Vec<Polygons> {
    let mut out = vec![Polygons::new(); src.len()];
    for r in propagate_waves_steps(src, boundary, expansion, expansion_step, max_nr_steps) {
        out[r.src_id as usize].push(r.polygon);
    }
    out
}

/// `merge_expansions_into_expolygons(src, expanded)`: each source united with its expansions
/// (`union_safety_offset_ex`), the sources without expansions as they are, in the sources' order.
/// Where a union falls apart, the part holding the source contour's first point is kept; a source
/// whose union holds none of its parts is dropped.
#[must_use]
pub fn merge_expansions_into_expolygons(
    mut src: ExPolygons,
    mut expanded: Vec<RegionExpansion>,
) -> ExPolygons {
    // Expanded regions will be merged into source regions, thus they will be re-sorted by source
    // id.
    orca_stl::sort_by(&mut expanded, |l, r| l.src_id < r.src_id);
    let mut last: u32 = 0;
    let mut acc = Polygons::new();
    let mut out = ExPolygons::with_capacity(src.len());
    let mut it = 0;
    while it < expanded.len() {
        while last < expanded[it].src_id {
            out.push(core::mem::take(&mut src[last as usize]));
            last += 1;
        }
        acc.clear();
        while it < expanded.len() && expanded[it].src_id == last {
            acc.push(core::mem::take(&mut expanded[it].polygon));
            it += 1;
        }
        let src_ex = core::mem::take(&mut src[last as usize]);
        last += 1;
        let sample = *src_ex.contour.points.first().unwrap_or_else(|| {
            panic!("merge_expansions_into_expolygons: a source without a contour (the C++ reads front() of it)")
        });
        acc.extend(expolygon::to_polygons(&src_ex));
        let mut merged = union_safety_offset_ex(&acc);
        // Expanding one expolygon by waves should not change connectivity of the source expolygon:
        // a single expolygon should be produced, possibly with more holes.
        if merged.len() > 1 {
            // There is something wrong with the initial waves. Most likely the bridge was not valid
            // at all or the boundary region was very close to some bridge edge, but not really
            // touching. Pick only a single merged expolygon, which contains one sample point of the
            // source expolygon.
            let aabb_tree = build_aabb_tree_over_expolygons(&merged);
            let id = sample_in_expolygons(&aabb_tree, &merged, sample);
            if id != -1 {
                out.push(core::mem::take(&mut merged[id as usize]));
            }
        } else if merged.len() == 1 {
            out.push(merged.swap_remove(0));
        }
    }
    while (last as usize) < src.len() {
        out.push(core::mem::take(&mut src[last as usize]));
        last += 1;
    }
    out
}

/// `expand_merge_expolygons(src, boundary, params)`: [`propagate_waves_src`], then
/// [`merge_expansions_into_expolygons`].
#[must_use]
pub fn expand_merge_expolygons(
    src: ExPolygons,
    boundary: &[ExPolygon],
    params: &RegionExpansionParameters,
) -> ExPolygons {
    // Expanded regions are sorted by boundary id and source id.
    let expanded = propagate_waves_src(&src, boundary, params);
    merge_expansions_into_expolygons(src, expanded)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: i64, y0: i64, x1: i64, y1: i64) -> ExPolygon {
        ExPolygon::from_points(vec![
            Point::new(x0, y0),
            Point::new(x1, y0),
            Point::new(x1, y1),
            Point::new(x0, y1),
        ])
    }

    /// The two-wall bridge expansion of `process_external_surfaces` in five steps of 0.1 mm.
    #[test]
    fn build_splits_the_expansion() {
        let p = RegionExpansionParameters::build(1_103_637.0, 100_000.0, 5);
        assert_eq!(p.tiny_expansion, 50_000.0);
        assert_eq!(p.num_other_steps, 4);
        assert_eq!(p.other_step, p.initial_step);
        assert_eq!(p.initial_step, (1_103_637.0_f32 - 50_000.0) / 5.0);
    }

    /// A source beside its boundary seeds one wave into it, and the expansion reaches into it.
    #[test]
    fn touching_regions_expand() {
        let src = vec![rect(0, 0, 10_000_000, 10_000_000)];
        let boundary = vec![rect(10_000_000, 0, 20_000_000, 10_000_000)];
        let seeds = wave_seeds(&src, &boundary, 50_000.0, true);
        assert!(!seeds.is_empty());
        assert!(seeds.iter().all(|s| s.src == 0 && s.boundary == 0));
        let out = propagate_waves_steps(&src, &boundary, 1_103_637.0, 100_000.0, 5);
        assert!(!out.is_empty());
        let reach = out
            .iter()
            .flat_map(|r| r.polygon.points.iter().map(|p| p.x))
            .fold(i64::MIN, |a, b| if a < b { b } else { a });
        assert!(reach > 10_900_000 && reach <= 11_103_637);
        let merged = merge_expansions_into_expolygons(src.clone(), out);
        assert_eq!(merged.len(), 1);
    }

    /// No source or no boundary seeds nothing.
    #[test]
    fn nothing_to_expand() {
        let r = vec![rect(0, 0, 10, 10)];
        assert!(wave_seeds(&[], &r, 1.0, false).is_empty());
        assert!(wave_seeds(&r, &[], 1.0, false).is_empty());
        assert_eq!(merge_expansions_into_expolygons(r.clone(), Vec::new()), r);
    }
}
