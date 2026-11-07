// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/ClipperUtils.cpp and ClipperUtils.hpp of OrcaSlicer
// v2.4.2, translated on 2026-11-07. Copyright the OrcaSlicer authors, the BambuStudio authors
// (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU Affero General Public
// License, version 3 (AGPL-3.0; LICENSE).

//! `ClipperUtils`: libslic3r's offsets and boolean operations over [`crate::clipper`].
//!
//! The C++ passes its inputs through "paths providers" (`PolygonsProvider`, `ExPolygonsProvider`
//! and so on) that walk the point lists in place; here every operation takes the same walk as a
//! slice of point slices ([`polygons_paths`], [`expolygon_paths`], [`expolygons_paths`],
//! [`polylines_paths`]): an expolygon gives its contour, then its holes. The overloads become
//! functions named after their argument types (`diff_ex_expolygons_polygons` for
//! `diff_ex(const ExPolygons&, const Polygons&)`), and the most used ones keep the C++ name.
//!
//! The `Surface` overloads come with `Surface`, and `union_pt_chained_outside_in`,
//! `order_nodes` and `traverse_pt` with `ShortestPath`'s `chain_clipper_polynodes`; `ZPoints`
//! (the `ClipperLib_Z` build) are not here.

use orca_eigen::Vec2d;

use crate::bounding_box::BoundingBox;
use crate::clipper::{
    self, ClipType, Clipper, ClipperOffset, EndType, JoinType, Path, Paths, PolyFillType, PolyTree,
    PolyType, ROOT,
};
use crate::expolygon::{self, ExPolygon, ExPolygons};
use crate::line::{Line, Lines};
use crate::point::{Point, Pointfs, Points, SCALED_EPSILON, cross2, vec2d};
use crate::polygon::{self, Polygon, Polygons};
use crate::polyline::{Polyline, Polylines};

/// `ClipperSafetyOffset`: 10 nm, a `float`.
pub const CLIPPER_SAFETY_OFFSET: f32 = 10.0;
/// `DefaultJoinType`.
pub const DEFAULT_JOIN_TYPE: JoinType = JoinType::Miter;
/// `DefaultEndType`.
pub const DEFAULT_END_TYPE: EndType = EndType::OpenButt;
/// `DefaultMiterLimit`.
pub const DEFAULT_MITER_LIMIT: f64 = 3.0;
/// `DefaultLineJoinType`.
pub const DEFAULT_LINE_JOIN_TYPE: JoinType = JoinType::Square;
/// `DefaultLineMiterLimit`.
pub const DEFAULT_LINE_MITER_LIMIT: f64 = 0.0;
/// `ClipperOffsetShortestEdgeFactor`: an offset's `ShortestEdgeLength` is `|delta|` times this.
pub const CLIPPER_OFFSET_SHORTEST_EDGE_FACTOR: f64 = 0.005;

/// `ApplySafetyOffset`: whether the clipping paths are first offset outwards by
/// [`CLIPPER_SAFETY_OFFSET`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ApplySafetyOffset {
    /// `No`.
    #[default]
    No,
    /// `Yes`.
    Yes,
}

/// A paths provider: the point lists an operation walks, in order.
pub type PathsRef<'a> = Vec<&'a [Point]>;

/// `PolygonsProvider`.
#[must_use]
pub fn polygons_paths(polygons: &[Polygon]) -> PathsRef<'_> {
    polygons.iter().map(|p| p.points.as_slice()).collect()
}

/// `PolylinesProvider`.
#[must_use]
pub fn polylines_paths(polylines: &[Polyline]) -> PathsRef<'_> {
    polylines.iter().map(|p| p.points.as_slice()).collect()
}

/// `ExPolygonProvider`: the contour, then the holes.
#[must_use]
pub fn expolygon_paths(expolygon: &ExPolygon) -> PathsRef<'_> {
    let mut out = PathsRef::with_capacity(expolygon.holes.len() + 1);
    out.push(expolygon.contour.points.as_slice());
    out.extend(expolygon.holes.iter().map(|h| h.points.as_slice()));
    out
}

/// `ExPolygonsProvider`: each expolygon's contour, then its holes.
#[must_use]
pub fn expolygons_paths(expolygons: &[ExPolygon]) -> PathsRef<'_> {
    let mut out = PathsRef::with_capacity(expolygon::number_polygons(expolygons));
    for e in expolygons {
        out.push(e.contour.points.as_slice());
        out.extend(e.holes.iter().map(|h| h.points.as_slice()));
    }
    out
}

/// A `ClipperLib::Paths` as a provider.
#[must_use]
pub fn paths_ref(paths: &[Path]) -> PathsRef<'_> {
    paths.iter().map(Vec::as_slice).collect()
}

/// `to_polygons(ClipperLib::Paths &&)`.
#[must_use]
pub fn to_polygons(paths: Paths) -> Polygons {
    paths.into_iter().map(Polygon::new).collect()
}

/// `clip_clipper_polygon_with_subject_bbox(src, bbox, out, get_entire_polygons)`: drops the points
/// whose neighbours lie outside the box on the same side as they do, so the polygon clips the
/// same inside the box; with `get_entire_polygons` a polygon that keeps any point is kept whole.
/// Below three points, or when nothing is kept, the result is empty.
#[must_use]
pub fn clip_clipper_polygon_with_subject_bbox_points(
    src: &[Point],
    bbox: &BoundingBox,
    get_entire_polygons: bool,
) -> Points {
    let mut out = Points::new();
    let cnt = src.len();
    if cnt < 3 {
        return out;
    }
    const LEFT: i32 = 1;
    const RIGHT: i32 = 2;
    const TOP: i32 = 4;
    const BOTTOM: i32 = 8;
    let sides = |p: Point| {
        i32::from(p.x < bbox.min.x) * LEFT
            + i32::from(p.x > bbox.max.x) * RIGHT
            + i32::from(p.y < bbox.min.y) * BOTTOM
            + i32::from(p.y > bbox.max.y) * TOP
    };
    let mut sides_prev = sides(src[cnt - 1]);
    let mut sides_this = sides(src[0]);
    let last = cnt - 1;
    for i in 0..last {
        let sides_next = sides(src[i + 1]);
        if sides_this == 0 || (sides_prev & sides_this & sides_next) == 0 {
            out.push(src[i]);
            sides_prev = sides_this;
        }
        sides_this = sides_next;
    }
    // Never a single point polygon.
    if !out.is_empty() {
        if get_entire_polygons {
            out = src.to_vec();
        } else {
            let sides_next = sides(out[0]);
            if sides_this == 0 || (sides_prev & sides_this & sides_next) == 0 {
                out.push(src[cnt - 1]);
            }
        }
    }
    out
}

/// `clip_clipper_polygon_with_subject_bbox(polygon, bbox, get_entire_polygons)`.
#[must_use]
pub fn clip_clipper_polygon_with_subject_bbox(
    src: &Polygon,
    bbox: &BoundingBox,
    get_entire_polygons: bool,
) -> Polygon {
    Polygon::new(clip_clipper_polygon_with_subject_bbox_points(
        &src.points,
        bbox,
        get_entire_polygons,
    ))
}

/// `clip_clipper_polygons_with_subject_bbox(polygons, bbox)`: the empty results dropped.
#[must_use]
pub fn clip_clipper_polygons_with_subject_bbox(src: &[Polygon], bbox: &BoundingBox) -> Polygons {
    let mut out: Polygons = src
        .iter()
        .map(|p| clip_clipper_polygon_with_subject_bbox(p, bbox, false))
        .collect();
    out.retain(|p| !p.is_empty());
    out
}

/// `clip_clipper_polygons_with_subject_bbox(expolygon, bbox, get_entire_polygons)`.
#[must_use]
pub fn clip_clipper_polygons_with_subject_bbox_expolygon(
    src: &ExPolygon,
    bbox: &BoundingBox,
    get_entire_polygons: bool,
) -> Polygons {
    let mut out = Polygons::with_capacity(src.num_contours());
    out.push(clip_clipper_polygon_with_subject_bbox(
        &src.contour,
        bbox,
        get_entire_polygons,
    ));
    for p in &src.holes {
        out.push(clip_clipper_polygon_with_subject_bbox(
            p,
            bbox,
            get_entire_polygons,
        ));
    }
    out.retain(|p| !p.is_empty());
    out
}

/// `clip_clipper_polygons_with_subject_bbox(expolygons, bbox, get_entire_polygons)`.
#[must_use]
pub fn clip_clipper_polygons_with_subject_bbox_expolygons(
    src: &[ExPolygon],
    bbox: &BoundingBox,
    get_entire_polygons: bool,
) -> Polygons {
    let mut out = Polygons::with_capacity(expolygon::number_polygons(src));
    for p in src {
        out.extend(clip_clipper_polygons_with_subject_bbox_expolygon(
            p,
            bbox,
            get_entire_polygons,
        ));
    }
    out.retain(|p| !p.is_empty());
    out
}

/// `PolyTreeToExPolygons(polytree)`: each outer contour with its holes, then the islands inside
/// each hole, depth first.
#[must_use]
pub fn poly_tree_to_expolygons(mut polytree: PolyTree) -> ExPolygons {
    fn recurse(tree: &mut PolyTree, node: usize, out: &mut ExPolygons) {
        let contour = core::mem::take(&mut tree.node_mut(node).contour);
        let childs = tree.node(node).childs.clone();
        let mut holes = Polygons::with_capacity(childs.len());
        for &h in &childs {
            holes.push(Polygon::new(core::mem::take(&mut tree.node_mut(h).contour)));
        }
        out.push(ExPolygon {
            contour: Polygon::new(contour),
            holes,
        });
        for &h in &childs {
            let grandchilds = tree.node(h).childs.clone();
            for g in grandchilds {
                recurse(tree, g, out);
            }
        }
    }
    let mut out = ExPolygons::new();
    let childs = polytree.node(ROOT).childs.clone();
    for c in childs {
        recurse(&mut polytree, c, &mut out);
    }
    out
}

/// `PolyTreeToPolylines(polytree)`: every non-empty contour, depth first.
#[must_use]
pub fn poly_tree_to_polylines(polytree: &PolyTree) -> Polylines {
    fn recurse(tree: &PolyTree, node: usize, out: &mut Polylines) {
        let n = tree.node(node);
        if !n.contour.is_empty() {
            out.push(Polyline::new(n.contour.clone()));
        }
        for &c in &n.childs {
            recurse(tree, c, out);
        }
    }
    let mut out = Polylines::new();
    recurse(polytree, ROOT, &mut out);
    out
}

/// `has_duplicate_points(polytree)`: within any one contour.
#[must_use]
pub fn has_duplicate_points_tree(polytree: &PolyTree) -> bool {
    fn recurse(tree: &PolyTree, node: usize) -> bool {
        let n = tree.node(node);
        crate::point::has_duplicate_points(&n.contour) || n.childs.iter().any(|&c| recurse(tree, c))
    }
    polytree
        .node(ROOT)
        .childs
        .iter()
        .any(|&c| recurse(polytree, c))
}

/// A `ClipperOffset` with libslic3r's parameters: the miter limit is the arc tolerance for round
/// joins, and points closer than `|offset| * 0.005` merge.
fn offsetter(offset: f64, join_type: JoinType, miter_limit: f64) -> ClipperOffset {
    let mut co = ClipperOffset::default();
    if join_type == JoinType::Round {
        co.arc_tolerance = miter_limit;
    } else {
        co.miter_limit = miter_limit;
    }
    co.shortest_edge_length = orca_crt::fabs(offset * CLIPPER_OFFSET_SHORTEST_EDGE_FACTOR);
    co
}

/// `raw_offset(paths, offset, joinType, miterLimit, endType)`: each path offset on its own, a
/// clockwise closed path by `-offset` and its results reversed back; no union.
#[must_use]
pub fn raw_offset(
    paths: &[&[Point]],
    offset: f32,
    join_type: JoinType,
    miter_limit: f64,
    end_type: EndType,
) -> Paths {
    let mut co = offsetter(f64::from(offset), join_type, miter_limit);
    let mut out = Paths::with_capacity(paths.len());
    let mut out_this = Paths::new();
    for path in paths {
        co.clear();
        co.add_path(path, join_type, end_type);
        let ccw = if end_type == EndType::ClosedPolygon {
            clipper::orientation(path)
        } else {
            true
        };
        co.execute(&mut out_this, f64::from(if ccw { offset } else { -offset }));
        if !ccw {
            for p in &mut out_this {
                p.reverse();
            }
        }
        out.append(&mut out_this);
    }
    out
}

/// `safety_offset(paths)`: [`raw_offset`] by [`CLIPPER_SAFETY_OFFSET`] with the default miter.
#[must_use]
pub fn safety_offset(paths: &[&[Point]]) -> Paths {
    raw_offset(
        paths,
        CLIPPER_SAFETY_OFFSET,
        DEFAULT_JOIN_TYPE,
        DEFAULT_MITER_LIMIT,
        EndType::ClosedPolygon,
    )
}

/// `clipper_do<Paths>(clipType, subject, clip, fillType)`.
#[must_use]
pub fn clipper_do(
    clip_type: ClipType,
    subject: &[&[Point]],
    clip: &[&[Point]],
    fill_type: PolyFillType,
) -> Paths {
    let mut c = Clipper::new(0);
    c.add_paths(subject.iter().copied(), PolyType::Subject, true);
    c.add_paths(clip.iter().copied(), PolyType::Clip, true);
    let mut out = Paths::new();
    c.execute(clip_type, &mut out, fill_type, fill_type);
    out
}

/// `clipper_do<PolyTree>(clipType, subject, clip, fillType)`.
#[must_use]
pub fn clipper_do_tree(
    clip_type: ClipType,
    subject: &[&[Point]],
    clip: &[&[Point]],
    fill_type: PolyFillType,
) -> PolyTree {
    let mut c = Clipper::new(0);
    c.add_paths(subject.iter().copied(), PolyType::Subject, true);
    c.add_paths(clip.iter().copied(), PolyType::Clip, true);
    let mut out = PolyTree::new();
    c.execute_tree(clip_type, &mut out, fill_type, fill_type);
    out
}

/// `clipper_do<Paths>(..., do_safety_offset)`: with `Yes`, the clip paths safety offset first.
#[must_use]
pub fn clipper_do_safety(
    clip_type: ClipType,
    subject: &[&[Point]],
    clip: &[&[Point]],
    fill_type: PolyFillType,
    do_safety_offset: ApplySafetyOffset,
) -> Paths {
    if do_safety_offset == ApplySafetyOffset::Yes {
        let clip = safety_offset(clip);
        clipper_do(clip_type, subject, &paths_ref(&clip), fill_type)
    } else {
        clipper_do(clip_type, subject, clip, fill_type)
    }
}

/// `clipper_union<Paths>(subject, fillType)`.
#[must_use]
pub fn clipper_union(subject: &[&[Point]], fill_type: PolyFillType) -> Paths {
    let mut c = Clipper::new(0);
    c.add_paths(subject.iter().copied(), PolyType::Subject, true);
    let mut out = Paths::new();
    c.execute(ClipType::Union, &mut out, fill_type, fill_type);
    out
}

/// `clipper_union<PolyTree>(subject, fillType)`.
#[must_use]
pub fn clipper_union_tree(subject: &[&[Point]], fill_type: PolyFillType) -> PolyTree {
    let mut c = Clipper::new(0);
    c.add_paths(subject.iter().copied(), PolyType::Subject, true);
    let mut out = PolyTree::new();
    c.execute_tree(ClipType::Union, &mut out, fill_type, fill_type);
    out
}

/// `ClipperPaths_to_Slic3rExPolygons(input, do_union)`: a union by non-zero, or by even-odd when
/// `do_union` is false (its default), to expolygons.
#[must_use]
pub fn clipper_paths_to_expolygons(input: &[Path], do_union: bool) -> ExPolygons {
    let fill = if do_union {
        PolyFillType::NonZero
    } else {
        PolyFillType::EvenOdd
    };
    poly_tree_to_expolygons(clipper_union_tree(&paths_ref(input), fill))
}

/// `expand_paths<Paths>`: the union of the raw offsets.
fn expand_paths(paths: &[&[Point]], offset: f32, join_type: JoinType, miter_limit: f64) -> Paths {
    let raw = raw_offset(
        paths,
        offset,
        join_type,
        miter_limit,
        EndType::ClosedPolygon,
    );
    clipper_union(&paths_ref(&raw), PolyFillType::NonZero)
}

/// `expand_paths<PolyTree>`.
fn expand_paths_tree(
    paths: &[&[Point]],
    offset: f32,
    join_type: JoinType,
    miter_limit: f64,
) -> PolyTree {
    let raw = raw_offset(
        paths,
        offset,
        join_type,
        miter_limit,
        EndType::ClosedPolygon,
    );
    clipper_union_tree(&paths_ref(&raw), PolyFillType::NonZero)
}

/// The clipper of `shrink_paths`: the raw offsets inside a frame 10 units beyond their bounds,
/// with the solution reversed.
fn shrink_clipper(raw: &[Path]) -> Clipper {
    let mut c = Clipper::new(0);
    c.add_paths(raw.iter().map(Vec::as_slice), PolyType::Subject, true);
    let r = c.get_bounds();
    let frame = [
        Point::new(r.left.wrapping_sub(10), r.bottom.wrapping_add(10)),
        Point::new(r.right.wrapping_add(10), r.bottom.wrapping_add(10)),
        Point::new(r.right.wrapping_add(10), r.top.wrapping_sub(10)),
        Point::new(r.left.wrapping_sub(10), r.top.wrapping_sub(10)),
    ];
    c.add_path(&frame, PolyType::Subject, true);
    c.set_reverse_solution(true);
    c
}

/// `shrink_paths<Paths>`: the raw offsets by `-offset`, cleaned by a negative union inside a frame
/// that is then removed.
fn shrink_paths(paths: &[&[Point]], offset: f32, join_type: JoinType, miter_limit: f64) -> Paths {
    let mut out = Paths::new();
    let raw = raw_offset(
        paths,
        -offset,
        join_type,
        miter_limit,
        EndType::ClosedPolygon,
    );
    if !raw.is_empty() {
        let mut c = shrink_clipper(&raw);
        c.execute(
            ClipType::Union,
            &mut out,
            PolyFillType::Negative,
            PolyFillType::Negative,
        );
        if !out.is_empty() {
            out.remove(0);
        }
    }
    out
}

/// `shrink_paths<PolyTree>`.
fn shrink_paths_tree(
    paths: &[&[Point]],
    offset: f32,
    join_type: JoinType,
    miter_limit: f64,
) -> PolyTree {
    let mut out = PolyTree::new();
    let raw = raw_offset(
        paths,
        -offset,
        join_type,
        miter_limit,
        EndType::ClosedPolygon,
    );
    if !raw.is_empty() {
        let mut c = shrink_clipper(&raw);
        c.execute_tree(
            ClipType::Union,
            &mut out,
            PolyFillType::Negative,
            PolyFillType::Negative,
        );
        out.remove_outermost_polygon();
    }
    out
}

/// `offset_paths<Paths>`: expand for a positive offset, shrink otherwise.
fn offset_paths(paths: &[&[Point]], offset: f32, join_type: JoinType, miter_limit: f64) -> Paths {
    if offset > 0.0 {
        expand_paths(paths, offset, join_type, miter_limit)
    } else {
        shrink_paths(paths, -offset, join_type, miter_limit)
    }
}

/// `offset_paths<PolyTree>`.
fn offset_paths_tree(
    paths: &[&[Point]],
    offset: f32,
    join_type: JoinType,
    miter_limit: f64,
) -> PolyTree {
    if offset > 0.0 {
        expand_paths_tree(paths, offset, join_type, miter_limit)
    } else {
        shrink_paths_tree(paths, -offset, join_type, miter_limit)
    }
}

/// `offset(polygon, delta, joinType, miterLimit)`: the raw offset, no union.
#[must_use]
pub fn offset_polygon(
    polygon: &Polygon,
    delta: f32,
    join_type: JoinType,
    miter_limit: f64,
) -> Polygons {
    to_polygons(raw_offset(
        &[polygon.points.as_slice()],
        delta,
        join_type,
        miter_limit,
        EndType::ClosedPolygon,
    ))
}

/// `offset(polygons, delta, joinType, miterLimit)`.
#[must_use]
pub fn offset(polygons: &[Polygon], delta: f32, join_type: JoinType, miter_limit: f64) -> Polygons {
    to_polygons(offset_paths(
        &polygons_paths(polygons),
        delta,
        join_type,
        miter_limit,
    ))
}

/// `offset_ex(polygons, delta, joinType, miterLimit)`.
#[must_use]
pub fn offset_ex(
    polygons: &[Polygon],
    delta: f32,
    join_type: JoinType,
    miter_limit: f64,
) -> ExPolygons {
    poly_tree_to_expolygons(offset_paths_tree(
        &polygons_paths(polygons),
        delta,
        join_type,
        miter_limit,
    ))
}

/// `offset_ex(polygon, delta, joinType, miterLimit)`.
#[must_use]
pub fn offset_ex_polygon(
    polygon: &Polygon,
    delta: f32,
    join_type: JoinType,
    miter_limit: f64,
) -> ExPolygons {
    offset_ex(
        core::slice::from_ref(polygon),
        delta,
        join_type,
        miter_limit,
    )
}

/// `offset(polyline, delta, joinType, miterLimit, end_type)`: the union of the raw offset.
#[must_use]
pub fn offset_polyline(
    polyline: &Polyline,
    delta: f32,
    join_type: JoinType,
    miter_limit: f64,
    end_type: EndType,
) -> Polygons {
    let raw = raw_offset(
        &[polyline.points.as_slice()],
        delta,
        join_type,
        miter_limit,
        end_type,
    );
    to_polygons(clipper_union(&paths_ref(&raw), PolyFillType::NonZero))
}

/// `offset(polylines, delta, joinType, miterLimit, end_type)`.
#[must_use]
pub fn offset_polylines(
    polylines: &[Polyline],
    delta: f32,
    join_type: JoinType,
    miter_limit: f64,
    end_type: EndType,
) -> Polygons {
    let raw = raw_offset(
        &polylines_paths(polylines),
        delta,
        join_type,
        miter_limit,
        end_type,
    );
    to_polygons(clipper_union(&paths_ref(&raw), PolyFillType::NonZero))
}

/// `contour_to_polygons(polygon, line_width, join_type, miter_limit)`: the stroke of the closed
/// line, half the width each side.
#[must_use]
pub fn contour_to_polygons(
    polygon: &Polygon,
    line_width: f32,
    join_type: JoinType,
    miter_limit: f64,
) -> Polygons {
    contour_to_polygons_of(
        core::slice::from_ref(polygon),
        line_width,
        join_type,
        miter_limit,
    )
}

/// `contour_to_polygons(polygons, line_width, join_type, miter_limit)`.
#[must_use]
pub fn contour_to_polygons_of(
    polygons: &[Polygon],
    line_width: f32,
    join_type: JoinType,
    miter_limit: f64,
) -> Polygons {
    let raw = raw_offset(
        &polygons_paths(polygons),
        line_width / 2.0,
        join_type,
        miter_limit,
        EndType::ClosedLine,
    );
    to_polygons(clipper_union(&paths_ref(&raw), PolyFillType::NonZero))
}

/// `offset_expolygon_inner(expoly, delta, joinType, miterLimit, out)`: the contour offset, then the
/// holes one by one by `-delta`; inwards the holes are subtracted, outwards appended reversed.
/// Returns 1 when something was appended, 0 otherwise.
fn offset_expolygon_inner(
    expoly: &ExPolygon,
    delta: f32,
    join_type: JoinType,
    miter_limit: f64,
    out: &mut Paths,
) -> usize {
    let mut contours = Paths::new();
    {
        let mut co = offsetter(f64::from(delta), join_type, miter_limit);
        co.add_path(&expoly.contour.points, join_type, EndType::ClosedPolygon);
        co.execute(&mut contours, f64::from(delta));
    }
    if contours.is_empty() {
        return 0;
    }
    if expoly.holes.is_empty() {
        out.append(&mut contours);
        return 1;
    }
    let mut holes = Paths::new();
    for hole in &expoly.holes {
        let mut co = offsetter(f64::from(delta), join_type, miter_limit);
        co.add_path(&hole.points, join_type, EndType::ClosedPolygon);
        let mut out2 = Paths::new();
        co.execute(&mut out2, f64::from(-delta));
        holes.append(&mut out2);
    }
    if holes.is_empty() {
        out.append(&mut contours);
    } else if delta < 0.0 {
        let mut output = clipper_do(
            ClipType::Difference,
            &paths_ref(&contours),
            &paths_ref(&holes),
            PolyFillType::NonZero,
        );
        if output.is_empty() {
            return 0;
        }
        out.append(&mut output);
    } else {
        out.append(&mut contours);
        for h in &mut holes {
            h.reverse();
        }
        out.append(&mut holes);
    }
    1
}

/// `expolygon_offset(expolygon, delta, joinType, miterLimit)`.
#[must_use]
pub fn expolygon_offset(
    expolygon: &ExPolygon,
    delta: f32,
    join_type: JoinType,
    miter_limit: f64,
) -> Paths {
    let mut out = Paths::new();
    offset_expolygon_inner(expolygon, delta, join_type, miter_limit, &mut out);
    out
}

/// `expolygons_offset_raw`: each expolygon offset on its own, and how many gave anything.
fn expolygons_offset_raw(
    expolygons: &[ExPolygon],
    delta: f32,
    join_type: JoinType,
    miter_limit: f64,
) -> (Paths, usize) {
    let mut output = Paths::with_capacity(expolygons.len());
    let mut collected = 0;
    for e in expolygons {
        collected += offset_expolygon_inner(e, delta, join_type, miter_limit, &mut output);
    }
    (output, collected)
}

/// `expolygons_offset`: united when more than one grew outwards.
fn expolygons_offset(
    expolygons: &[ExPolygon],
    delta: f32,
    join_type: JoinType,
    miter_limit: f64,
) -> Paths {
    let (output, collected) = expolygons_offset_raw(expolygons, delta, join_type, miter_limit);
    if collected > 1 && delta > 0.0 {
        clipper_union(&paths_ref(&output), PolyFillType::NonZero)
    } else {
        output
    }
}

/// `expolygons_offset_pt`: always united, to a tree.
fn expolygons_offset_pt(
    expolygons: &[ExPolygon],
    delta: f32,
    join_type: JoinType,
    miter_limit: f64,
) -> PolyTree {
    let (output, _) = expolygons_offset_raw(expolygons, delta, join_type, miter_limit);
    clipper_union_tree(&paths_ref(&output), PolyFillType::NonZero)
}

/// `offset(expolygon, delta, joinType, miterLimit)`.
#[must_use]
pub fn offset_expolygon(
    expolygon: &ExPolygon,
    delta: f32,
    join_type: JoinType,
    miter_limit: f64,
) -> Polygons {
    to_polygons(expolygon_offset(expolygon, delta, join_type, miter_limit))
}

/// `offset(expolygons, delta, joinType, miterLimit)`.
#[must_use]
pub fn offset_expolygons(
    expolygons: &[ExPolygon],
    delta: f32,
    join_type: JoinType,
    miter_limit: f64,
) -> Polygons {
    to_polygons(expolygons_offset(expolygons, delta, join_type, miter_limit))
}

/// `offset_ex(expolygon, delta, joinType, miterLimit)`: united by even-odd
/// (`ClipperPaths_to_Slic3rExPolygons` with `do_union` false).
#[must_use]
pub fn offset_ex_expolygon(
    expolygon: &ExPolygon,
    delta: f32,
    join_type: JoinType,
    miter_limit: f64,
) -> ExPolygons {
    clipper_paths_to_expolygons(
        &expolygon_offset(expolygon, delta, join_type, miter_limit),
        false,
    )
}

/// `offset_ex(expolygons, delta, joinType, miterLimit)`.
#[must_use]
pub fn offset_ex_expolygons(
    expolygons: &[ExPolygon],
    delta: f32,
    join_type: JoinType,
    miter_limit: f64,
) -> ExPolygons {
    poly_tree_to_expolygons(expolygons_offset_pt(
        expolygons,
        delta,
        join_type,
        miter_limit,
    ))
}

/// `offset2(expolygons, delta1, delta2, joinType, miterLimit)`.
#[must_use]
pub fn offset2(
    expolygons: &[ExPolygon],
    delta1: f32,
    delta2: f32,
    join_type: JoinType,
    miter_limit: f64,
) -> Polygons {
    let first = expolygons_offset(expolygons, delta1, join_type, miter_limit);
    to_polygons(offset_paths(
        &paths_ref(&first),
        delta2,
        join_type,
        miter_limit,
    ))
}

/// `offset2_ex(expolygons, delta1, delta2, joinType, miterLimit)`.
#[must_use]
pub fn offset2_ex(
    expolygons: &[ExPolygon],
    delta1: f32,
    delta2: f32,
    join_type: JoinType,
    miter_limit: f64,
) -> ExPolygons {
    let first = expolygons_offset(expolygons, delta1, join_type, miter_limit);
    poly_tree_to_expolygons(offset_paths_tree(
        &paths_ref(&first),
        delta2,
        join_type,
        miter_limit,
    ))
}

/// `closing(polygons, delta1, delta2, joinType, miterLimit)`: out by `delta1`, back by `delta2`.
#[must_use]
pub fn closing(
    polygons: &[Polygon],
    delta1: f32,
    delta2: f32,
    join_type: JoinType,
    miter_limit: f64,
) -> Polygons {
    let grown = expand_paths(&polygons_paths(polygons), delta1, join_type, miter_limit);
    to_polygons(shrink_paths(
        &paths_ref(&grown),
        delta2,
        join_type,
        miter_limit,
    ))
}

/// `closing_ex(polygons, delta1, delta2, joinType, miterLimit)`.
#[must_use]
pub fn closing_ex(
    polygons: &[Polygon],
    delta1: f32,
    delta2: f32,
    join_type: JoinType,
    miter_limit: f64,
) -> ExPolygons {
    let grown = expand_paths(&polygons_paths(polygons), delta1, join_type, miter_limit);
    poly_tree_to_expolygons(shrink_paths_tree(
        &paths_ref(&grown),
        delta2,
        join_type,
        miter_limit,
    ))
}

/// `closing_ex(expolygons, delta, joinType, miterLimit)`: `offset2_ex(delta, -delta)`.
#[must_use]
pub fn closing_ex_expolygons(
    expolygons: &[ExPolygon],
    delta: f32,
    join_type: JoinType,
    miter_limit: f64,
) -> ExPolygons {
    offset2_ex(expolygons, delta, -delta, join_type, miter_limit)
}

/// `opening(polygons, delta1, delta2, joinType, miterLimit)`: in by `delta1`, back by `delta2`.
#[must_use]
pub fn opening(
    polygons: &[Polygon],
    delta1: f32,
    delta2: f32,
    join_type: JoinType,
    miter_limit: f64,
) -> Polygons {
    let shrunk = shrink_paths(&polygons_paths(polygons), delta1, join_type, miter_limit);
    to_polygons(expand_paths(
        &paths_ref(&shrunk),
        delta2,
        join_type,
        miter_limit,
    ))
}

/// `opening(expolygons, delta1, delta2, joinType, miterLimit)`.
#[must_use]
pub fn opening_expolygons(
    expolygons: &[ExPolygon],
    delta1: f32,
    delta2: f32,
    join_type: JoinType,
    miter_limit: f64,
) -> Polygons {
    let shrunk = shrink_paths(
        &expolygons_paths(expolygons),
        delta1,
        join_type,
        miter_limit,
    );
    to_polygons(expand_paths(
        &paths_ref(&shrunk),
        delta2,
        join_type,
        miter_limit,
    ))
}

/// `opening_ex(expolygons, delta, joinType, miterLimit)`: `offset2_ex(-delta, delta)`.
#[must_use]
pub fn opening_ex(
    expolygons: &[ExPolygon],
    delta: f32,
    join_type: JoinType,
    miter_limit: f64,
) -> ExPolygons {
    offset2_ex(expolygons, -delta, delta, join_type, miter_limit)
}

/// `expand(polygons, delta, ...)`: `offset(polygons, delta)`.
#[must_use]
pub fn expand(polygons: &[Polygon], delta: f32, join_type: JoinType, miter_limit: f64) -> Polygons {
    offset(polygons, delta, join_type, miter_limit)
}

/// `expand_ex(polygons, delta, ...)`.
#[must_use]
pub fn expand_ex(
    polygons: &[Polygon],
    delta: f32,
    join_type: JoinType,
    miter_limit: f64,
) -> ExPolygons {
    offset_ex(polygons, delta, join_type, miter_limit)
}

/// `shrink(polygons, delta, ...)`: `offset(polygons, -delta)`.
#[must_use]
pub fn shrink(polygons: &[Polygon], delta: f32, join_type: JoinType, miter_limit: f64) -> Polygons {
    offset(polygons, -delta, join_type, miter_limit)
}

/// `shrink_ex(polygons, delta, ...)`.
#[must_use]
pub fn shrink_ex(
    polygons: &[Polygon],
    delta: f32,
    join_type: JoinType,
    miter_limit: f64,
) -> ExPolygons {
    offset_ex(polygons, -delta, join_type, miter_limit)
}

/// `shrink_ex(expolygons, delta, ...)`.
#[must_use]
pub fn shrink_ex_expolygons(
    expolygons: &[ExPolygon],
    delta: f32,
    join_type: JoinType,
    miter_limit: f64,
) -> ExPolygons {
    offset_ex_expolygons(expolygons, -delta, join_type, miter_limit)
}

/// `union_safety_offset(polygons)`: `offset(polygons, ClipperSafetyOffset)`.
#[must_use]
pub fn union_safety_offset(polygons: &[Polygon]) -> Polygons {
    offset(
        polygons,
        CLIPPER_SAFETY_OFFSET,
        DEFAULT_JOIN_TYPE,
        DEFAULT_MITER_LIMIT,
    )
}

/// `union_safety_offset_ex(polygons)`.
#[must_use]
pub fn union_safety_offset_ex(polygons: &[Polygon]) -> ExPolygons {
    offset_ex(
        polygons,
        CLIPPER_SAFETY_OFFSET,
        DEFAULT_JOIN_TYPE,
        DEFAULT_MITER_LIMIT,
    )
}

/// `clipper_do_polytree(clipType, subject, clip, fillType, do_safety_offset)`: the operation to
/// paths, then a union of the result to a tree (Clipper's trees are slow on overlapping edges).
#[must_use]
pub fn clipper_do_polytree(
    clip_type: ClipType,
    subject: &[&[Point]],
    clip: &[&[Point]],
    fill_type: PolyFillType,
    do_safety_offset: ApplySafetyOffset,
) -> PolyTree {
    let output = clipper_do_safety(clip_type, subject, clip, fill_type, do_safety_offset);
    if output.is_empty() {
        return PolyTree::new();
    }
    clipper_union_tree(&paths_ref(&output), fill_type)
}

/// `_clipper(clipType, subject, clip, do_safety_offset)`: non-zero, to polygons.
#[must_use]
pub fn clipper_op(
    clip_type: ClipType,
    subject: &[&[Point]],
    clip: &[&[Point]],
    do_safety_offset: ApplySafetyOffset,
) -> Polygons {
    to_polygons(clipper_do_safety(
        clip_type,
        subject,
        clip,
        PolyFillType::NonZero,
        do_safety_offset,
    ))
}

/// `_clipper_ex(clipType, subject, clip, do_safety_offset, fill_type)`: to expolygons.
#[must_use]
pub fn clipper_op_ex(
    clip_type: ClipType,
    subject: &[&[Point]],
    clip: &[&[Point]],
    do_safety_offset: ApplySafetyOffset,
    fill_type: PolyFillType,
) -> ExPolygons {
    poly_tree_to_expolygons(clipper_do_polytree(
        clip_type,
        subject,
        clip,
        fill_type,
        do_safety_offset,
    ))
}

/// `diff(polygon, polygon, do_safety_offset)`.
#[must_use]
pub fn diff_polygon(subject: &Polygon, clip: &Polygon, safety: ApplySafetyOffset) -> Polygons {
    clipper_op(
        ClipType::Difference,
        &[subject.points.as_slice()],
        &[clip.points.as_slice()],
        safety,
    )
}

/// `diff(polygons, polygons, do_safety_offset)`.
#[must_use]
pub fn diff(subject: &[Polygon], clip: &[Polygon], safety: ApplySafetyOffset) -> Polygons {
    clipper_op(
        ClipType::Difference,
        &polygons_paths(subject),
        &polygons_paths(clip),
        safety,
    )
}

/// `diff(polygons, expolygons, do_safety_offset)`.
#[must_use]
pub fn diff_polygons_expolygons(
    subject: &[Polygon],
    clip: &[ExPolygon],
    safety: ApplySafetyOffset,
) -> Polygons {
    clipper_op(
        ClipType::Difference,
        &polygons_paths(subject),
        &expolygons_paths(clip),
        safety,
    )
}

/// `diff(expolygons, polygons, do_safety_offset)`.
#[must_use]
pub fn diff_expolygons_polygons(
    subject: &[ExPolygon],
    clip: &[Polygon],
    safety: ApplySafetyOffset,
) -> Polygons {
    clipper_op(
        ClipType::Difference,
        &expolygons_paths(subject),
        &polygons_paths(clip),
        safety,
    )
}

/// `diff(expolygons, expolygons, do_safety_offset)`.
#[must_use]
pub fn diff_expolygons(
    subject: &[ExPolygon],
    clip: &[ExPolygon],
    safety: ApplySafetyOffset,
) -> Polygons {
    clipper_op(
        ClipType::Difference,
        &expolygons_paths(subject),
        &expolygons_paths(clip),
        safety,
    )
}

/// `diff_clipped(polygons, polygons, do_safety_offset)`: the clip first trimmed to the subject's
/// box grown by `SCALED_EPSILON`.
#[must_use]
pub fn diff_clipped(subject: &[Polygon], clip: &[Polygon], safety: ApplySafetyOffset) -> Polygons {
    let bbox = polygon::get_extents_polygons(subject).inflated(SCALED_EPSILON);
    diff(
        subject,
        &clip_clipper_polygons_with_subject_bbox(clip, &bbox),
        safety,
    )
}

/// `diff_clipped(expolygons, polygons, do_safety_offset)`.
#[must_use]
pub fn diff_clipped_expolygons_polygons(
    subject: &[ExPolygon],
    clip: &[Polygon],
    safety: ApplySafetyOffset,
) -> ExPolygons {
    let bbox = expolygon::get_extents_ex(subject).inflated(SCALED_EPSILON);
    diff_ex_expolygons_polygons(
        subject,
        &clip_clipper_polygons_with_subject_bbox(clip, &bbox),
        safety,
    )
}

/// `diff_clipped(expolygons, expolygons, do_safety_offset)`.
#[must_use]
pub fn diff_clipped_expolygons(
    subject: &[ExPolygon],
    clip: &[ExPolygon],
    safety: ApplySafetyOffset,
) -> ExPolygons {
    let bbox = expolygon::get_extents_ex(subject).inflated(SCALED_EPSILON);
    diff_ex_expolygons_polygons(
        subject,
        &clip_clipper_polygons_with_subject_bbox_expolygons(clip, &bbox, false),
        safety,
    )
}

/// `intersection(polygon, polygon, do_safety_offset)`.
#[must_use]
pub fn intersection_polygon(
    subject: &Polygon,
    clip: &Polygon,
    safety: ApplySafetyOffset,
) -> Polygons {
    clipper_op(
        ClipType::Intersection,
        &[subject.points.as_slice()],
        &[clip.points.as_slice()],
        safety,
    )
}

/// `intersection(polygons, polygons, do_safety_offset)`.
#[must_use]
pub fn intersection(subject: &[Polygon], clip: &[Polygon], safety: ApplySafetyOffset) -> Polygons {
    clipper_op(
        ClipType::Intersection,
        &polygons_paths(subject),
        &polygons_paths(clip),
        safety,
    )
}

/// `intersection(polygons, polygon, do_safety_offset)`.
#[must_use]
pub fn intersection_polygons_polygon(
    subject: &[Polygon],
    clip: &Polygon,
    safety: ApplySafetyOffset,
) -> Polygons {
    intersection(subject, core::slice::from_ref(clip), safety)
}

/// `intersection(polygons, expolygon, do_safety_offset)`.
#[must_use]
pub fn intersection_polygons_expolygon(
    subject: &[Polygon],
    clip: &ExPolygon,
    safety: ApplySafetyOffset,
) -> Polygons {
    clipper_op(
        ClipType::Intersection,
        &polygons_paths(subject),
        &expolygon_paths(clip),
        safety,
    )
}

/// `intersection(expolygon, expolygon, do_safety_offset)`.
#[must_use]
pub fn intersection_expolygon(
    subject: &ExPolygon,
    clip: &ExPolygon,
    safety: ApplySafetyOffset,
) -> Polygons {
    clipper_op(
        ClipType::Intersection,
        &expolygon_paths(subject),
        &expolygon_paths(clip),
        safety,
    )
}

/// `intersection(expolygons, polygons, do_safety_offset)`.
#[must_use]
pub fn intersection_expolygons_polygons(
    subject: &[ExPolygon],
    clip: &[Polygon],
    safety: ApplySafetyOffset,
) -> Polygons {
    clipper_op(
        ClipType::Intersection,
        &expolygons_paths(subject),
        &polygons_paths(clip),
        safety,
    )
}

/// `intersection(expolygons, expolygons, do_safety_offset)`.
#[must_use]
pub fn intersection_expolygons(
    subject: &[ExPolygon],
    clip: &[ExPolygon],
    safety: ApplySafetyOffset,
) -> Polygons {
    clipper_op(
        ClipType::Intersection,
        &expolygons_paths(subject),
        &expolygons_paths(clip),
        safety,
    )
}

/// `intersection_clipped(polygons, polygons, do_safety_offset)`.
#[must_use]
pub fn intersection_clipped(
    subject: &[Polygon],
    clip: &[Polygon],
    safety: ApplySafetyOffset,
) -> Polygons {
    let bbox = polygon::get_extents_polygons(subject).inflated(SCALED_EPSILON);
    intersection(
        subject,
        &clip_clipper_polygons_with_subject_bbox(clip, &bbox),
        safety,
    )
}

/// `union_(polygons)`.
#[must_use]
pub fn union_(subject: &[Polygon]) -> Polygons {
    clipper_op(
        ClipType::Union,
        &polygons_paths(subject),
        &[],
        ApplySafetyOffset::No,
    )
}

/// `union_(expolygons)`.
#[must_use]
pub fn union_expolygons(subject: &[ExPolygon]) -> Polygons {
    clipper_op(
        ClipType::Union,
        &expolygons_paths(subject),
        &[],
        ApplySafetyOffset::No,
    )
}

/// `union_(polygons, fillType)`.
#[must_use]
pub fn union_fill(subject: &[Polygon], fill_type: PolyFillType) -> Polygons {
    to_polygons(clipper_do_safety(
        ClipType::Union,
        &polygons_paths(subject),
        &[],
        fill_type,
        ApplySafetyOffset::No,
    ))
}

/// `union_(polygons, polygons)`: of both together.
#[must_use]
pub fn union_two(subject: &[Polygon], subject2: &[Polygon]) -> Polygons {
    let mut polys = subject.to_vec();
    polys.extend_from_slice(subject2);
    union_(&polys)
}

/// `diff_ex(polygons, polygons, do_safety_offset)`.
#[must_use]
pub fn diff_ex(subject: &[Polygon], clip: &[Polygon], safety: ApplySafetyOffset) -> ExPolygons {
    clipper_op_ex(
        ClipType::Difference,
        &polygons_paths(subject),
        &polygons_paths(clip),
        safety,
        PolyFillType::NonZero,
    )
}

/// `diff_ex(polygon, polygons, do_safety_offset)`.
#[must_use]
pub fn diff_ex_polygon_polygons(
    subject: &Polygon,
    clip: &[Polygon],
    safety: ApplySafetyOffset,
) -> ExPolygons {
    diff_ex(core::slice::from_ref(subject), clip, safety)
}

/// `diff_ex(polygon, polygon, do_safety_offset)`.
#[must_use]
pub fn diff_ex_polygon(subject: &Polygon, clip: &Polygon, safety: ApplySafetyOffset) -> ExPolygons {
    diff_ex(
        core::slice::from_ref(subject),
        core::slice::from_ref(clip),
        safety,
    )
}

/// `diff_ex(polygon, expolygons, do_safety_offset)`.
#[must_use]
pub fn diff_ex_polygon_expolygons(
    subject: &Polygon,
    clip: &[ExPolygon],
    safety: ApplySafetyOffset,
) -> ExPolygons {
    clipper_op_ex(
        ClipType::Difference,
        &[subject.points.as_slice()],
        &expolygons_paths(clip),
        safety,
        PolyFillType::NonZero,
    )
}

/// `diff_ex(polygons, expolygons, do_safety_offset)`.
#[must_use]
pub fn diff_ex_polygons_expolygons(
    subject: &[Polygon],
    clip: &[ExPolygon],
    safety: ApplySafetyOffset,
) -> ExPolygons {
    clipper_op_ex(
        ClipType::Difference,
        &polygons_paths(subject),
        &expolygons_paths(clip),
        safety,
        PolyFillType::NonZero,
    )
}

/// `diff_ex(expolygon, polygon, do_safety_offset)`.
#[must_use]
pub fn diff_ex_expolygon_polygon(
    subject: &ExPolygon,
    clip: &Polygon,
    safety: ApplySafetyOffset,
) -> ExPolygons {
    clipper_op_ex(
        ClipType::Difference,
        &expolygon_paths(subject),
        &[clip.points.as_slice()],
        safety,
        PolyFillType::NonZero,
    )
}

/// `diff_ex(expolygon, polygons, do_safety_offset)`.
#[must_use]
pub fn diff_ex_expolygon_polygons(
    subject: &ExPolygon,
    clip: &[Polygon],
    safety: ApplySafetyOffset,
) -> ExPolygons {
    clipper_op_ex(
        ClipType::Difference,
        &expolygon_paths(subject),
        &polygons_paths(clip),
        safety,
        PolyFillType::NonZero,
    )
}

/// `diff_ex(expolygons, polygons, do_safety_offset)`.
#[must_use]
pub fn diff_ex_expolygons_polygons(
    subject: &[ExPolygon],
    clip: &[Polygon],
    safety: ApplySafetyOffset,
) -> ExPolygons {
    clipper_op_ex(
        ClipType::Difference,
        &expolygons_paths(subject),
        &polygons_paths(clip),
        safety,
        PolyFillType::NonZero,
    )
}

/// `diff_ex(expolygons, expolygons, do_safety_offset)`.
#[must_use]
pub fn diff_ex_expolygons(
    subject: &[ExPolygon],
    clip: &[ExPolygon],
    safety: ApplySafetyOffset,
) -> ExPolygons {
    clipper_op_ex(
        ClipType::Difference,
        &expolygons_paths(subject),
        &expolygons_paths(clip),
        safety,
        PolyFillType::NonZero,
    )
}

/// `diff_ex(expolygon, expolygon, do_safety_offset)`: the C++ inline drops `do_safety_offset`
/// and always runs without the safety offset.
#[must_use]
pub fn diff_ex_expolygon(
    subject: &ExPolygon,
    clip: &ExPolygon,
    _safety: ApplySafetyOffset,
) -> ExPolygons {
    diff_ex_expolygons(
        core::slice::from_ref(subject),
        core::slice::from_ref(clip),
        ApplySafetyOffset::No,
    )
}

/// `diff_ex(expolygon, expolygons, do_safety_offset)`.
#[must_use]
pub fn diff_ex_expolygon_expolygons(
    subject: &ExPolygon,
    clip: &[ExPolygon],
    safety: ApplySafetyOffset,
) -> ExPolygons {
    diff_ex_expolygons(core::slice::from_ref(subject), clip, safety)
}

/// `diff_ex(expolygons, expolygon, do_safety_offset)`.
#[must_use]
pub fn diff_ex_expolygons_expolygon(
    subject: &[ExPolygon],
    clip: &ExPolygon,
    safety: ApplySafetyOffset,
) -> ExPolygons {
    diff_ex_expolygons(subject, core::slice::from_ref(clip), safety)
}

/// `intersection_ex(polygons, polygons, do_safety_offset)`.
#[must_use]
pub fn intersection_ex(
    subject: &[Polygon],
    clip: &[Polygon],
    safety: ApplySafetyOffset,
) -> ExPolygons {
    clipper_op_ex(
        ClipType::Intersection,
        &polygons_paths(subject),
        &polygons_paths(clip),
        safety,
        PolyFillType::NonZero,
    )
}

/// `intersection_ex(expolygon, polygons, do_safety_offset)`.
#[must_use]
pub fn intersection_ex_expolygon_polygons(
    subject: &ExPolygon,
    clip: &[Polygon],
    safety: ApplySafetyOffset,
) -> ExPolygons {
    clipper_op_ex(
        ClipType::Intersection,
        &expolygon_paths(subject),
        &polygons_paths(clip),
        safety,
        PolyFillType::NonZero,
    )
}

/// `intersection_ex(expolygon, expolygon, do_safety_offset)`.
#[must_use]
pub fn intersection_ex_expolygon(
    subject: &ExPolygon,
    clip: &ExPolygon,
    safety: ApplySafetyOffset,
) -> ExPolygons {
    clipper_op_ex(
        ClipType::Intersection,
        &expolygon_paths(subject),
        &expolygon_paths(clip),
        safety,
        PolyFillType::NonZero,
    )
}

/// `intersection_ex(polygons, expolygons, do_safety_offset)`.
#[must_use]
pub fn intersection_ex_polygons_expolygons(
    subject: &[Polygon],
    clip: &[ExPolygon],
    safety: ApplySafetyOffset,
) -> ExPolygons {
    clipper_op_ex(
        ClipType::Intersection,
        &polygons_paths(subject),
        &expolygons_paths(clip),
        safety,
        PolyFillType::NonZero,
    )
}

/// `intersection_ex(expolygons, polygons, do_safety_offset)`.
#[must_use]
pub fn intersection_ex_expolygons_polygons(
    subject: &[ExPolygon],
    clip: &[Polygon],
    safety: ApplySafetyOffset,
) -> ExPolygons {
    clipper_op_ex(
        ClipType::Intersection,
        &expolygons_paths(subject),
        &polygons_paths(clip),
        safety,
        PolyFillType::NonZero,
    )
}

/// `intersection_ex(expolygons, expolygon, do_safety_offset)`.
#[must_use]
pub fn intersection_ex_expolygons_expolygon(
    subject: &[ExPolygon],
    clip: &ExPolygon,
    safety: ApplySafetyOffset,
) -> ExPolygons {
    clipper_op_ex(
        ClipType::Intersection,
        &expolygons_paths(subject),
        &expolygon_paths(clip),
        safety,
        PolyFillType::NonZero,
    )
}

/// `intersection_ex(expolygon, expolygons, do_safety_offset)`.
#[must_use]
pub fn intersection_ex_expolygon_expolygons(
    subject: &ExPolygon,
    clip: &[ExPolygon],
    safety: ApplySafetyOffset,
) -> ExPolygons {
    clipper_op_ex(
        ClipType::Intersection,
        &expolygon_paths(subject),
        &expolygons_paths(clip),
        safety,
        PolyFillType::NonZero,
    )
}

/// `intersection_ex(expolygons, expolygons, do_safety_offset)`.
#[must_use]
pub fn intersection_ex_expolygons(
    subject: &[ExPolygon],
    clip: &[ExPolygon],
    safety: ApplySafetyOffset,
) -> ExPolygons {
    clipper_op_ex(
        ClipType::Intersection,
        &expolygons_paths(subject),
        &expolygons_paths(clip),
        safety,
        PolyFillType::NonZero,
    )
}

/// `union_ex(polygons, fill_type)`: non-zero by default.
#[must_use]
pub fn union_ex(subject: &[Polygon], fill_type: PolyFillType) -> ExPolygons {
    clipper_op_ex(
        ClipType::Union,
        &polygons_paths(subject),
        &[],
        ApplySafetyOffset::No,
        fill_type,
    )
}

/// `union_ex(expolygons)`.
#[must_use]
pub fn union_ex_expolygons(subject: &[ExPolygon]) -> ExPolygons {
    poly_tree_to_expolygons(clipper_do_polytree(
        ClipType::Union,
        &expolygons_paths(subject),
        &[],
        PolyFillType::NonZero,
        ApplySafetyOffset::No,
    ))
}

/// `union_ex(expolygons, polygons)`: the polygons are added as clip paths.
#[must_use]
pub fn union_ex_expolygons_polygons(subject: &[ExPolygon], subject2: &[Polygon]) -> ExPolygons {
    poly_tree_to_expolygons(clipper_do_polytree(
        ClipType::Union,
        &expolygons_paths(subject),
        &polygons_paths(subject2),
        PolyFillType::NonZero,
        ApplySafetyOffset::No,
    ))
}

/// `union_ex(expolygons, expolygons, safety_offset_)`: of both together; the flag is unused.
#[must_use]
pub fn union_ex_two(poly1: &[ExPolygon], poly2: &[ExPolygon], _safety_offset: bool) -> ExPolygons {
    let mut expolys = poly1.to_vec();
    expolys.extend_from_slice(poly2);
    union_ex_expolygons(&expolys)
}

/// `xor_ex(expolygons, expolygon, do_safety_offset)`.
#[must_use]
pub fn xor_ex_expolygons_expolygon(
    subject: &[ExPolygon],
    clip: &ExPolygon,
    safety: ApplySafetyOffset,
) -> ExPolygons {
    clipper_op_ex(
        ClipType::Xor,
        &expolygons_paths(subject),
        &expolygon_paths(clip),
        safety,
        PolyFillType::NonZero,
    )
}

/// `xor_ex(expolygons, expolygons, do_safety_offset)`.
#[must_use]
pub fn xor_ex(subject: &[ExPolygon], clip: &[ExPolygon], safety: ApplySafetyOffset) -> ExPolygons {
    clipper_op_ex(
        ClipType::Xor,
        &expolygons_paths(subject),
        &expolygons_paths(clip),
        safety,
        PolyFillType::NonZero,
    )
}

/// `_clipper_pl_open(clipType, subject, clip)`: open subject paths against closed clip paths.
#[must_use]
pub fn clipper_pl_open(clip_type: ClipType, subject: &[&[Point]], clip: &[&[Point]]) -> Polylines {
    let mut c = Clipper::new(0);
    c.add_paths(subject.iter().copied(), PolyType::Subject, false);
    c.add_paths(clip.iter().copied(), PolyType::Clip, true);
    let mut tree = PolyTree::new();
    c.execute_tree(
        clip_type,
        &mut tree,
        PolyFillType::NonZero,
        PolyFillType::NonZero,
    );
    poly_tree_to_polylines(&tree)
}

/// `_clipper_pl_recombine(polylines)`: joins the pieces that share an end point, in either
/// direction.
fn clipper_pl_recombine(polylines: &mut Polylines) {
    let mut i = 0;
    while i < polylines.len() {
        let mut j = i + 1;
        while j < polylines.len() {
            let (ib, ifr) = (polylines[i].last_point(), polylines[i].first_point());
            let (jb, jfr) = (polylines[j].last_point(), polylines[j].first_point());
            if ib == jfr {
                let tail = polylines[j].points[1..].to_vec();
                polylines[i].points.extend(tail);
                polylines.remove(j);
            } else if ifr == jb {
                let n = polylines[j].points.len();
                let head = polylines[j].points[..n - 1].to_vec();
                polylines[i].points.splice(0..0, head);
                polylines.remove(j);
            } else if ifr == jfr {
                polylines[j].reverse();
                let n = polylines[j].points.len();
                let head = polylines[j].points[..n - 1].to_vec();
                polylines[i].points.splice(0..0, head);
                polylines.remove(j);
            } else if ib == jb {
                polylines[j].reverse();
                let tail = polylines[j].points[1..].to_vec();
                polylines[i].points.extend(tail);
                polylines.remove(j);
            } else {
                j += 1;
            }
        }
        i += 1;
    }
}

/// `_clipper_pl_closed(clipType, subject, clip)`: each subject polygon as an open path closed by
/// its first point, then the pieces recombined.
#[must_use]
pub fn clipper_pl_closed(
    clip_type: ClipType,
    subject: &[&[Point]],
    clip: &[&[Point]],
) -> Polylines {
    let paths: Paths = subject
        .iter()
        .map(|poly| {
            let mut path = Path::with_capacity(poly.len() + 1);
            path.extend_from_slice(poly);
            path.push(poly[0]);
            path
        })
        .collect();
    let mut retval = clipper_pl_open(clip_type, &paths_ref(&paths), clip);
    clipper_pl_recombine(&mut retval);
    retval
}

/// `diff_pl(polyline, polygons)`.
#[must_use]
pub fn diff_pl_polyline(subject: &Polyline, clip: &[Polygon]) -> Polylines {
    clipper_pl_open(
        ClipType::Difference,
        &[subject.points.as_slice()],
        &polygons_paths(clip),
    )
}

/// `diff_pl(polylines, polygons)`.
#[must_use]
pub fn diff_pl(subject: &[Polyline], clip: &[Polygon]) -> Polylines {
    clipper_pl_open(
        ClipType::Difference,
        &polylines_paths(subject),
        &polygons_paths(clip),
    )
}

/// `diff_pl(polyline, expolygon)`.
#[must_use]
pub fn diff_pl_polyline_expolygon(subject: &Polyline, clip: &ExPolygon) -> Polylines {
    clipper_pl_open(
        ClipType::Difference,
        &[subject.points.as_slice()],
        &expolygon_paths(clip),
    )
}

/// `diff_pl(polylines, expolygon)`.
#[must_use]
pub fn diff_pl_polylines_expolygon(subject: &[Polyline], clip: &ExPolygon) -> Polylines {
    clipper_pl_open(
        ClipType::Difference,
        &polylines_paths(subject),
        &expolygon_paths(clip),
    )
}

/// `diff_pl(polylines, expolygons)`.
#[must_use]
pub fn diff_pl_polylines_expolygons(subject: &[Polyline], clip: &[ExPolygon]) -> Polylines {
    clipper_pl_open(
        ClipType::Difference,
        &polylines_paths(subject),
        &expolygons_paths(clip),
    )
}

/// `diff_pl(polygons, polygons)`: the polygons' outlines outside the clip.
#[must_use]
pub fn diff_pl_polygons(subject: &[Polygon], clip: &[Polygon]) -> Polylines {
    clipper_pl_closed(
        ClipType::Difference,
        &polygons_paths(subject),
        &polygons_paths(clip),
    )
}

/// `intersection_pl(polylines, polygon)`.
#[must_use]
pub fn intersection_pl_polylines_polygon(subject: &[Polyline], clip: &Polygon) -> Polylines {
    clipper_pl_open(
        ClipType::Intersection,
        &polylines_paths(subject),
        &[clip.points.as_slice()],
    )
}

/// `intersection_pl(polyline, expolygon)`.
#[must_use]
pub fn intersection_pl_polyline_expolygon(subject: &Polyline, clip: &ExPolygon) -> Polylines {
    clipper_pl_open(
        ClipType::Intersection,
        &[subject.points.as_slice()],
        &expolygon_paths(clip),
    )
}

/// `intersection_pl(polylines, expolygon)`.
#[must_use]
pub fn intersection_pl_polylines_expolygon(subject: &[Polyline], clip: &ExPolygon) -> Polylines {
    clipper_pl_open(
        ClipType::Intersection,
        &polylines_paths(subject),
        &expolygon_paths(clip),
    )
}

/// `intersection_pl(polyline, polygons)`.
#[must_use]
pub fn intersection_pl_polyline(subject: &Polyline, clip: &[Polygon]) -> Polylines {
    clipper_pl_open(
        ClipType::Intersection,
        &[subject.points.as_slice()],
        &polygons_paths(clip),
    )
}

/// `intersection_pl(polylines, polygons)`.
#[must_use]
pub fn intersection_pl(subject: &[Polyline], clip: &[Polygon]) -> Polylines {
    clipper_pl_open(
        ClipType::Intersection,
        &polylines_paths(subject),
        &polygons_paths(clip),
    )
}

/// `intersection_pl(polylines, expolygons)`.
#[must_use]
pub fn intersection_pl_polylines_expolygons(subject: &[Polyline], clip: &[ExPolygon]) -> Polylines {
    clipper_pl_open(
        ClipType::Intersection,
        &polylines_paths(subject),
        &expolygons_paths(clip),
    )
}

/// `intersection_pl(polygons, polygons)`: the polygons' outlines inside the clip.
#[must_use]
pub fn intersection_pl_polygons(subject: &[Polygon], clip: &[Polygon]) -> Polylines {
    clipper_pl_closed(
        ClipType::Intersection,
        &polygons_paths(subject),
        &polygons_paths(clip),
    )
}

/// `_clipper_ln(clipType, lines, polygons)`: each line clipped as a two-point polyline, each
/// piece of two points or more back to a line from its first to its last point.
#[must_use]
pub fn clipper_ln(clip_type: ClipType, subject: &[Line], clip: &[Polygon]) -> Lines {
    let polylines: Polylines = subject
        .iter()
        .map(|l| Polyline::from_two(l.a, l.b))
        .collect();
    let polylines = clipper_pl_open(
        clip_type,
        &polylines_paths(&polylines),
        &polygons_paths(clip),
    );
    polylines
        .iter()
        .filter(|pl| pl.size() >= 2)
        .map(|pl| Line::new(pl.first_point(), pl.last_point()))
        .collect()
}

/// `diff_ln(lines, polygons)`.
#[must_use]
pub fn diff_ln(subject: &[Line], clip: &[Polygon]) -> Lines {
    clipper_ln(ClipType::Difference, subject, clip)
}

/// `intersection_ln(lines, polygons)`.
#[must_use]
pub fn intersection_ln(subject: &[Line], clip: &[Polygon]) -> Lines {
    clipper_ln(ClipType::Intersection, subject, clip)
}

/// `intersection_ln(line, polygons)`.
#[must_use]
pub fn intersection_ln_line(subject: &Line, clip: &[Polygon]) -> Lines {
    clipper_ln(ClipType::Intersection, core::slice::from_ref(subject), clip)
}

/// `union_pt(polygons)`: by even-odd, so contours that do not cross keep their orientation.
#[must_use]
pub fn union_pt(subject: &[Polygon]) -> PolyTree {
    clipper_do_tree(
        ClipType::Union,
        &polygons_paths(subject),
        &[],
        PolyFillType::EvenOdd,
    )
}

/// `union_pt(expolygons)`.
#[must_use]
pub fn union_pt_expolygons(subject: &[ExPolygon]) -> PolyTree {
    clipper_do_tree(
        ClipType::Union,
        &expolygons_paths(subject),
        &[],
        PolyFillType::EvenOdd,
    )
}

/// `simplify_polygons(polygons)`: a strictly simple non-zero union.
#[must_use]
pub fn simplify_polygons(subject: &[Polygon]) -> Polygons {
    let mut c = Clipper::new(0);
    c.set_strictly_simple(true);
    c.add_paths(polygons_paths(subject), PolyType::Subject, true);
    let mut output = Paths::new();
    c.execute(
        ClipType::Union,
        &mut output,
        PolyFillType::NonZero,
        PolyFillType::NonZero,
    );
    to_polygons(output)
}

/// `simplify_polygons_ex(polygons)`.
#[must_use]
pub fn simplify_polygons_ex(subject: &[Polygon]) -> ExPolygons {
    let mut c = Clipper::new(0);
    c.set_strictly_simple(true);
    c.add_paths(polygons_paths(subject), PolyType::Subject, true);
    let mut tree = PolyTree::new();
    c.execute_tree(
        ClipType::Union,
        &mut tree,
        PolyFillType::NonZero,
        PolyFillType::NonZero,
    );
    poly_tree_to_expolygons(tree)
}

/// `top_level_islands(polygons)`: the outermost contours of an even-odd union.
#[must_use]
pub fn top_level_islands(polygons: &[Polygon]) -> Polygons {
    let mut c = Clipper::new(0);
    c.clear();
    c.add_paths(polygons_paths(polygons), PolyType::Subject, true);
    let mut tree = PolyTree::new();
    c.execute_tree(
        ClipType::Union,
        &mut tree,
        PolyFillType::EvenOdd,
        PolyFillType::EvenOdd,
    );
    tree.node(ROOT)
        .childs
        .iter()
        .map(|&c| Polygon::new(tree.node(c).contour.clone()))
        .collect()
}

/// `fix_after_outer_offset(input, filltype, reverse_result)`: the union of one path.
#[must_use]
pub fn fix_after_outer_offset(
    input: &[Point],
    filltype: PolyFillType,
    reverse_result: bool,
) -> Paths {
    let mut solution = Paths::new();
    if !input.is_empty() {
        let mut c = Clipper::new(0);
        c.add_path(input, PolyType::Subject, true);
        c.set_reverse_solution(reverse_result);
        c.execute(ClipType::Union, &mut solution, filltype, filltype);
    }
    solution
}

/// `fix_after_inner_offset(input, filltype, reverse_result)`: the union of one path inside a frame
/// 10 units beyond its bounds (clockwise for positive filling), the frame then removed.
#[must_use]
pub fn fix_after_inner_offset(
    input: &[Point],
    filltype: PolyFillType,
    reverse_result: bool,
) -> Paths {
    let mut solution = Paths::new();
    if !input.is_empty() {
        let mut c = Clipper::new(0);
        c.add_path(input, PolyType::Subject, true);
        let mut r = c.get_bounds();
        r.left = r.left.wrapping_sub(10);
        r.top = r.top.wrapping_sub(10);
        r.right = r.right.wrapping_add(10);
        r.bottom = r.bottom.wrapping_add(10);
        let frame = if filltype == PolyFillType::Positive {
            [
                Point::new(r.left, r.bottom),
                Point::new(r.left, r.top),
                Point::new(r.right, r.top),
                Point::new(r.right, r.bottom),
            ]
        } else {
            [
                Point::new(r.left, r.bottom),
                Point::new(r.right, r.bottom),
                Point::new(r.right, r.top),
                Point::new(r.left, r.top),
            ]
        };
        c.add_path(&frame, PolyType::Subject, true);
        c.set_reverse_solution(reverse_result);
        c.execute(ClipType::Union, &mut solution, filltype, filltype);
        if !solution.is_empty() {
            solution.remove(0);
        }
    }
    solution
}

/// `mittered_offset_path_scaled(contour, deltas, miter_limit)`: a closed contour offset by a delta
/// per vertex, with Clipper's miter rule (the limit clamped to 2), the points rounded half away
/// from zero; empty for three points or fewer.
#[must_use]
pub fn mittered_offset_path_scaled(contour: &[Point], deltas: &[f32], miter_limit: f64) -> Path {
    let mut out = Path::new();
    if deltas.len() <= 2 {
        return out;
    }
    out.reserve(contour.len() * 2);
    let miter_limit = if miter_limit > 2.0 {
        2.0 / (miter_limit * miter_limit)
    } else {
        0.5
    };
    let perp = |v: Vec2d| vec2d(v.y(), -v.x());
    let add_offset_point = |out: &mut Path, mut pt: Vec2d| {
        pt += vec2d(
            0.5 - f64::from(u8::from(pt.x() < 0.0)),
            0.5 - f64::from(u8::from(pt.y() < 0.0)),
        );
        out.push(Point::new(
            orca_crt::f64_to_i64(pt.x()),
            orca_crt::f64_to_i64(pt.y()),
        ));
    };
    let lmin =
        f64::from(deltas[orca_stl::max_element_f32(deltas)]) * CLIPPER_OFFSET_SHORTEST_EDGE_FACTOR;
    let l2min = lmin * lmin;
    // As Clipper does.
    let sin_min_parallel = 1.0;
    let mut pt = contour[0].cast_f64();
    let mut iprev = contour.len() - 1;
    let mut ptprev = vec2d(0.0, 0.0);
    while iprev > 0 {
        ptprev = contour[iprev].cast_f64();
        if (ptprev - pt).squared_norm() > l2min {
            break;
        }
        iprev -= 1;
    }
    if iprev == 0 {
        return out;
    }
    let ilast = iprev;
    let mut nprev = perp(pt - ptprev).normalized();
    let mut i = 0;
    loop {
        // The next point further than lmin from pt.
        let mut j = i + 1;
        let mut ptnext = vec2d(0.0, 0.0);
        while j <= ilast {
            ptnext = contour[j].cast_f64();
            if (ptnext - pt).squared_norm() > l2min {
                break;
            }
            j += 1;
        }
        if j > ilast {
            // A last edge too short merges with the previous one.
            i = ilast;
            ptnext = contour[0].cast_f64();
        }
        let nnext = perp(ptnext - pt).normalized();
        let delta = f64::from(deltas[i]);
        let sin_a = orca_stl::clamp(cross2(nprev, nnext), -1.0, 1.0);
        let convex = sin_a * delta;
        if convex <= -sin_min_parallel {
            // A concave corner.
            add_offset_point(&mut out, pt + nprev * delta);
            add_offset_point(&mut out, pt);
            add_offset_point(&mut out, pt + nnext * delta);
        } else {
            let dot = nprev.dot(&nnext);
            if convex < sin_min_parallel && dot > 0.0 {
                // Nearly parallel.
                let p = if nprev.dot(&nnext) > 0.0 {
                    pt + nprev * delta
                } else {
                    pt
                };
                add_offset_point(&mut out, p);
            } else {
                // A convex corner, maybe very sharp.
                let r = 1.0 + dot;
                if r >= miter_limit {
                    add_offset_point(&mut out, pt + (nprev + nnext) * (delta / r));
                } else {
                    let dx = orca_crt::tan(orca_crt::atan2(sin_a, dot) / 4.0);
                    let newpt1 = pt + (nprev - perp(nprev) * dx) * delta;
                    let newpt2 = pt + (nnext + perp(nnext) * dx) * delta;
                    add_offset_point(&mut out, newpt1);
                    add_offset_point(&mut out, newpt2);
                }
            }
        }
        if i == ilast {
            break;
        }
        nprev = nnext;
        pt = ptnext;
        i = j;
    }
    out
}

/// The holes of a variable offset: each hole's mitered offset fixed by `fix`.
fn variable_offset_holes(
    expoly: &ExPolygon,
    deltas: &[Vec<f32>],
    miter_limit: f64,
    fix: impl Fn(&[Point]) -> Paths,
) -> Paths {
    let mut holes = Paths::with_capacity(expoly.holes.len());
    for (k, hole) in expoly.holes.iter().enumerate() {
        let mut fixed = fix(&mittered_offset_path_scaled(
            &hole.points,
            &deltas[1 + k],
            miter_limit,
        ));
        holes.append(&mut fixed);
    }
    holes
}

/// The contours less the holes, to paths.
fn subtract_holes(contours: Paths, holes: &[Path]) -> Paths {
    if holes.is_empty() {
        return contours;
    }
    let mut c = Clipper::new(0);
    c.clear();
    c.add_paths(contours.iter().map(Vec::as_slice), PolyType::Subject, true);
    c.add_paths(holes.iter().map(Vec::as_slice), PolyType::Clip, true);
    let mut output = Paths::new();
    c.execute(
        ClipType::Difference,
        &mut output,
        PolyFillType::NonZero,
        PolyFillType::NonZero,
    );
    output
}

/// The contours less the holes, to expolygons.
fn subtract_holes_ex(contours: Paths, holes: &[Path]) -> ExPolygons {
    if holes.is_empty() {
        return contours.into_iter().map(ExPolygon::from_points).collect();
    }
    let mut c = Clipper::new(0);
    c.add_paths(contours.iter().map(Vec::as_slice), PolyType::Subject, true);
    c.add_paths(holes.iter().map(Vec::as_slice), PolyType::Clip, true);
    let mut tree = PolyTree::new();
    c.execute_tree(
        ClipType::Difference,
        &mut tree,
        PolyFillType::NonZero,
        PolyFillType::NonZero,
    );
    poly_tree_to_expolygons(tree)
}

/// `variable_offset_inner(expoly, deltas, miter_limit)`: deltas per vertex, all at most zero,
/// one list for the contour and one per hole.
#[must_use]
pub fn variable_offset_inner(
    expoly: &ExPolygon,
    deltas: &[Vec<f32>],
    miter_limit: f64,
) -> Polygons {
    let contours = fix_after_inner_offset(
        &mittered_offset_path_scaled(&expoly.contour.points, &deltas[0], miter_limit),
        PolyFillType::Negative,
        true,
    );
    let holes = variable_offset_holes(expoly, deltas, miter_limit, |p| {
        fix_after_outer_offset(p, PolyFillType::Negative, false)
    });
    to_polygons(subtract_holes(contours, &holes))
}

/// `variable_offset_outer(expoly, deltas, miter_limit)`: deltas all at least zero.
#[must_use]
pub fn variable_offset_outer(
    expoly: &ExPolygon,
    deltas: &[Vec<f32>],
    miter_limit: f64,
) -> Polygons {
    let contours = fix_after_outer_offset(
        &mittered_offset_path_scaled(&expoly.contour.points, &deltas[0], miter_limit),
        PolyFillType::Positive,
        false,
    );
    let holes = variable_offset_holes(expoly, deltas, miter_limit, |p| {
        fix_after_inner_offset(p, PolyFillType::Positive, true)
    });
    to_polygons(subtract_holes(contours, &holes))
}

/// `variable_offset_outer_ex(expoly, deltas, miter_limit)`.
#[must_use]
pub fn variable_offset_outer_ex(
    expoly: &ExPolygon,
    deltas: &[Vec<f32>],
    miter_limit: f64,
) -> ExPolygons {
    let contours = fix_after_outer_offset(
        &mittered_offset_path_scaled(&expoly.contour.points, &deltas[0], miter_limit),
        PolyFillType::Positive,
        false,
    );
    let holes = variable_offset_holes(expoly, deltas, miter_limit, |p| {
        fix_after_inner_offset(p, PolyFillType::Positive, true)
    });
    subtract_holes_ex(contours, &holes)
}

/// `variable_offset_inner_ex(expoly, deltas, miter_limit)`.
#[must_use]
pub fn variable_offset_inner_ex(
    expoly: &ExPolygon,
    deltas: &[Vec<f32>],
    miter_limit: f64,
) -> ExPolygons {
    let contours = fix_after_inner_offset(
        &mittered_offset_path_scaled(&expoly.contour.points, &deltas[0], miter_limit),
        PolyFillType::Negative,
        true,
    );
    let holes = variable_offset_holes(expoly, deltas, miter_limit, |p| {
        fix_after_outer_offset(p, PolyFillType::Negative, false)
    });
    subtract_holes_ex(contours, &holes)
}

/// `make_counter_clockwise(pointfs)`: reversed when its `new_scale`d polygon is clockwise.
#[must_use]
pub fn make_counter_clockwise(pointfs: &[Vec2d]) -> Pointfs {
    let mut ps = pointfs.to_vec();
    if Polygon::new_scale(pointfs).is_clockwise() {
        ps.reverse();
    }
    ps
}
