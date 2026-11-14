// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Geometry/VoronoiUtils.hpp lines 1-167 and VoronoiUtils.cpp
// lines 1-348 of OrcaSlicer v2.4.2 (SegmentCellRange, PointCellRange and the class VoronoiUtils:
// the vertex helpers, the source lookups, discretize_parabola and the two cell ranges), translated
// on 2026-11-14 by orca-o3-voronoi-3; with the Slic3r exceptions it throws (src/libslic3r/
// Exception.hpp). Copyright the OrcaSlicer authors and the PrusaSlicer authors (Prusa Research),
// with code adapted from CuraEngine (Copyright Ultimaker B.V.: VoronoiUtils' discretizeParabola,
// computeSegmentCellRange and computePointCellRange), under the GNU Affero General Public License,
// version 3 (AGPL-3.0; LICENSE).

//! libslic3r's Voronoi utilities (`Geometry/VoronoiUtils.hpp` and `.cpp`): what the repair, the
//! medial axis and Arachne read of a diagram's cells, vertices and edges.
//!
//! - The vertex helpers: [`VoronoiUtils::to_point`] rounds a vertex with `llround` (the UCRT's,
//!   through `orca-crt`), [`VoronoiUtils::is_finite`], [`VoronoiUtils::make_rotated_vertex`] turns
//!   a vertex about the origin with the UCRT's `cos` and `sin` (keeping its incident edge and its
//!   colour), and [`VoronoiUtils::is_in_range`] compares with the type's limits as `double`s (so
//!   2^63 is in the range of `int64_t`, whose maximum rounds up to it).
//! - The source lookups: a cell's source index names its input segment (a segment cell, or a
//!   point cell at the segment's start or end point). [`VoronoiUtils::get_source_segment`] checks
//!   the cell's geometry and the index (`InvalidArgument`, `OutOfRange`);
//!   [`VoronoiUtils::get_source_point`] and [`VoronoiUtils::get_source_point_index`] check the
//!   geometry and the category (a single point is a `RuntimeError`) but not the index, which the
//!   release build reads unchecked: past the segments it panics here, where the C++'s read is
//!   undefined. The exceptions are [`VoronoiError`]'s variants, with the C++'s texts.
//! - [`VoronoiUtils::discretize_parabola`]: points along the parabola between a point and a
//!   segment, about one per step parallel to the segment, with the apex and the two marking points
//!   where the transitioning angle starts and ends added in order, in the C++'s integer arithmetic
//!   (wrapping where MSVC's does) and its rounding (`Point::rotated`, `lround` of the step count,
//!   whose `long` is 32 bits on Windows). When both marking points but not the apex fall inside it
//!   logs the C++'s warning through `orca_mesh::log`.
//! - [`VoronoiUtils::compute_segment_cell_range`]: the first and the last edge of a segment's
//!   cell, from the last edge that starts at the segment's end point to the edge that ends at its
//!   start point; [`VoronoiUtils::compute_point_cell_range`]: the edges of a point's cell inside
//!   the polygon's corner (`is_point_inside_polygon_corner`), none for a cell outside it. They walk
//!   the cell's loop through the diagram's containers ([`VoronoiContainers`]) and return the edges'
//!   indexes. Like the C++ they read a vertex of every finite edge they meet: `to_point` of a
//!   vertex a degenerate diagram left non-finite gives `llround`'s answer for it, the UCRT's own,
//!   so the repair's cell pass over such a diagram gets the C++'s values (parity/sop1/voronoi.md
//!   lists `to_point` outside the 64-bit range as a NotPorted edge; the repair reaches it, and it
//!   has an answer). An edge the C++ follows through a null pointer is a panic here.
//!
//! The templates take any segment of [`SegmentConcept`] whose point is `Slic3r::Point`
//! (`Lines`, `VoronoiDiagram::Segment`s, `PolygonsSegmentIndex`es); `get_source_point_index` and
//! `compute_point_cell_range`, instantiated only for `PolygonsSegmentIndex`, take those, and
//! `discretize_parabola` reads the segment's `from()` and `to()` as its `LOW` and `HIGH`, which they
//! are for every segment type that has them.
//!
//! `norm_i64` (Eigen's `norm()` of an `int64_t` vector) belongs in `orca-eigen`; orca-arachne has
//! the same function, after this crate in the build order, so it is translated here too.
//!
//! Left out: the `ColoredLines` instantiations of `get_source_segment`, `get_source_point` and
//! `compute_segment_cell_range` (`MultiMaterialSegmentation.cpp`'s, the family `painted-facets`,
//! outside domain A): `ColoredLine` is not translated, so nothing can call them.

use core::fmt;

use orca_eigen::Vec2d;
use orca_geom::geometry::is_point_inside_polygon_corner;
use orca_geom::line::Line;
use orca_geom::point::{Point, Points, vec2d};
use orca_mesh::log::{Severity, log};

use crate::diagram::{BoostVoronoiDiagram, VoronoiCell, VoronoiEdge, VoronoiVertex};
use crate::geometry_type::SourceCategory;
use crate::geometry_voronoi::VoronoiDiagram;
use crate::polygons_point_index::{PolygonsPointIndex, PolygonsSegmentIndex};
use crate::voronoi::SegmentConcept;

/// The exceptions the utilities throw: `Slic3r::InvalidArgument`, `Slic3r::OutOfRange` and
/// `Slic3r::RuntimeError` (`Exception.hpp`), each with its `what()`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VoronoiError {
    /// `Slic3r::InvalidArgument`.
    InvalidArgument(&'static str),
    /// `Slic3r::OutOfRange`.
    OutOfRange(&'static str),
    /// `Slic3r::RuntimeError`.
    RuntimeError(&'static str),
}

impl VoronoiError {
    /// `what()`.
    #[must_use]
    pub fn what(&self) -> &'static str {
        match *self {
            Self::InvalidArgument(s) | Self::OutOfRange(s) | Self::RuntimeError(s) => s,
        }
    }
}

impl fmt::Display for VoronoiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.what())
    }
}

impl std::error::Error for VoronoiError {}

/// "Voronoi cell doesn't contain a source segment!"
const NO_SOURCE_SEGMENT: &str = "Voronoi cell doesn't contain a source segment!";
/// "Voronoi cell source index is out of range!"
const SOURCE_INDEX_OUT_OF_RANGE: &str = "Voronoi cell source index is out of range!";
/// "Voronoi cell doesn't contain a source point!"
const NO_SOURCE_POINT: &str = "Voronoi cell doesn't contain a source point!";
/// The `RuntimeError` of a single point's cell.
const SINGLE_POINT: &str = "Voronoi diagram is always constructed using segments, so cell.source_category() shouldn't be SOURCE_CATEGORY_SINGLE_POINT!";
/// The `InvalidArgument` of `get_source_point` on a cell of another category.
const GET_SOURCE_POINT_ONLY: &str =
    "Function get_source_point() should only be called on point cells!";
/// The `InvalidArgument` of `get_source_point_index` on a cell of another category.
const GET_SOURCE_POINT_INDEX_ONLY: &str =
    "Function get_source_point_index() should only be called on point cells!";
/// `discretize_parabola`'s warning.
const FAILING_TO_DISCRETIZE: &str =
    "Failing to discretize parabola! Must add an apex or one of the endpoints.";

/// What the cell ranges read of a diagram: the vertex and the edge containers that a cell's and an
/// edge's links index (the C++ follows pointers into them).
pub trait VoronoiContainers {
    /// `vertices()`.
    fn vertices(&self) -> &[VoronoiVertex];
    /// `edges()`.
    fn edges(&self) -> &[VoronoiEdge];
}

impl VoronoiContainers for VoronoiDiagram {
    fn vertices(&self) -> &[VoronoiVertex] {
        VoronoiDiagram::vertices(self)
    }
    fn edges(&self) -> &[VoronoiEdge] {
        VoronoiDiagram::edges(self)
    }
}

impl VoronoiContainers for BoostVoronoiDiagram {
    fn vertices(&self) -> &[VoronoiVertex] {
        BoostVoronoiDiagram::vertices(self)
    }
    fn edges(&self) -> &[VoronoiEdge] {
        BoostVoronoiDiagram::edges(self)
    }
}

/// The vertex at a link, `None` for a null link.
fn vertex_at<D: VoronoiContainers + ?Sized>(vd: &D, v: Option<usize>) -> Option<&VoronoiVertex> {
    v.map(|v| &vd.vertices()[v])
}

/// `T`'s limits as `double`s, which `is_in_range<T>` compares with: `numeric_limits<T>::lowest()`
/// and `max()`, each rounded to the nearest `double`.
pub trait InRange {
    /// `double(std::numeric_limits<T>::lowest())`.
    const LOWEST: f64;
    /// `double(std::numeric_limits<T>::max())`.
    const MAX: f64;
}

impl InRange for i32 {
    const LOWEST: f64 = i32::MIN as f64;
    const MAX: f64 = i32::MAX as f64;
}

impl InRange for i64 {
    const LOWEST: f64 = i64::MIN as f64;
    // 2^63: `INT64_MAX` rounds up to it.
    const MAX: f64 = i64::MAX as f64;
}

/// `SegmentCellRange<Point>`: the trapezoid cell around a segment, from `edge_begin` to
/// `edge_end` along the cell's loop.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SegmentCellRange {
    /// `source_segment_start_point`: the start point of the cell's source segment.
    pub source_segment_start_point: Point,
    /// `source_segment_end_point`: the end point of the cell's source segment.
    pub source_segment_end_point: Point,
    /// `edge_begin`: the edge where the loop around the cell starts.
    pub edge_begin: Option<usize>,
    /// `edge_end`: the edge where the loop around the cell ends.
    pub edge_end: Option<usize>,
}

impl SegmentCellRange {
    /// `explicit SegmentCellRange(start, end)`: no edges.
    #[must_use]
    pub fn new(source_segment_start_point: Point, source_segment_end_point: Point) -> Self {
        Self {
            source_segment_start_point,
            source_segment_end_point,
            edge_begin: None,
            edge_end: None,
        }
    }

    /// `is_valid()`: both edges set, and different.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.edge_begin.is_some() && self.edge_end.is_some() && self.edge_begin != self.edge_end
    }
}

/// `PointCellRange<Point>`: the cell around a point, from `edge_begin` to `edge_end`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PointCellRange {
    /// `source_point`: the cell's source point.
    pub source_point: Point,
    /// `edge_begin`: the edge where the loop around the cell starts.
    pub edge_begin: Option<usize>,
    /// `edge_end`: the edge where the loop around the cell ends.
    pub edge_end: Option<usize>,
}

impl PointCellRange {
    /// `explicit PointCellRange(source_point)`: no edges.
    #[must_use]
    pub fn new(source_point: Point) -> Self {
        Self {
            source_point,
            edge_begin: None,
            edge_end: None,
        }
    }

    /// `is_valid()`: both edges set, and different.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.edge_begin.is_some() && self.edge_end.is_some() && self.edge_begin != self.edge_end
    }
}

/// `(v).cast<int64_t>().norm()` of an integer vector: Eigen's `numext::sqrt` of the `int64_t`
/// squared norm (wrapping), `std::sqrt` of it as a `double`, truncated back to `int64_t`. It
/// belongs in `orca-eigen`.
#[allow(clippy::cast_precision_loss)]
fn norm_i64(v: Point) -> i64 {
    orca_crt::f64_to_i64(orca_crt::sqrt(v.squared_norm() as f64))
}

/// `class VoronoiUtils`: its static methods.
#[derive(Clone, Copy, Default, Debug)]
pub struct VoronoiUtils;

impl VoronoiUtils {
    /// `to_point(const VD::vertex_type *vertex)`: `to_point(*vertex)`. Panics for a null vertex
    /// (the C++ asserts in its debug build only, and follows the pointer).
    #[must_use]
    pub fn to_point_ptr(vertex: Option<&VoronoiVertex>) -> Point {
        VoronoiUtils::to_point(
            vertex.expect("VoronoiUtils::to_point: the C++ follows a null vertex"),
        )
    }

    /// `to_point(const VD::vertex_type &vertex)`: `{std::llround(x), std::llround(y)}`, as
    /// `Vec2i64` (the same type as `Point`'s `Vec2crd`, `coord_t` being `int64_t`).
    #[must_use]
    pub fn to_point(vertex: &VoronoiVertex) -> Point {
        let x = vertex.x();
        let y = vertex.y();
        Point::new(orca_crt::llround(x), orca_crt::llround(y))
    }

    /// `is_finite(vertex)`: both coordinates are finite.
    #[must_use]
    pub fn is_finite(vertex: &VoronoiVertex) -> bool {
        vertex.x().is_finite() && vertex.y().is_finite()
    }

    /// `make_rotated_vertex(vertex, angle)`: the vertex turned by `angle` about the origin, with
    /// its incident edge and its colour.
    #[must_use]
    pub fn make_rotated_vertex(vertex: &VoronoiVertex, angle: f64) -> VoronoiVertex {
        let cos_a = orca_crt::cos(angle);
        let sin_a = orca_crt::sin(angle);

        let rotated_x = cos_a * vertex.x() - sin_a * vertex.y();
        let rotated_y = cos_a * vertex.y() + sin_a * vertex.x();

        let mut rotated_vertex = VoronoiVertex::new(rotated_x, rotated_y);
        rotated_vertex.set_incident_edge(vertex.incident_edge());
        rotated_vertex.set_color(vertex.color());

        rotated_vertex
    }

    /// `get_source_segment(cell, segment_begin, segment_end)`: the cell's source segment.
    ///
    /// # Errors
    ///
    /// `InvalidArgument` when the cell holds no segment, `OutOfRange` when its source index is
    /// past the segments.
    pub fn get_source_segment<'s, S: SegmentConcept>(
        cell: &VoronoiCell,
        segments: &'s [S],
    ) -> Result<&'s S, VoronoiError> {
        if !cell.contains_segment() {
            return Err(VoronoiError::InvalidArgument(NO_SOURCE_SEGMENT));
        }

        if cell.source_index() >= segments.len() {
            return Err(VoronoiError::OutOfRange(SOURCE_INDEX_OUT_OF_RANGE));
        }

        Ok(&segments[cell.source_index()])
    }

    /// `get_source_point(cell, segment_begin, segment_end)`: the segment end point a point cell
    /// holds. The index is not checked (the release build has no assert): past the segments it
    /// panics.
    ///
    /// # Errors
    ///
    /// `InvalidArgument` when the cell holds no point or a category of no segment end point,
    /// `RuntimeError` for a single point's cell.
    pub fn get_source_point<S: SegmentConcept<Point = Point>>(
        cell: &VoronoiCell,
        segments: &[S],
    ) -> Result<Point, VoronoiError> {
        if !cell.contains_point() {
            return Err(VoronoiError::InvalidArgument(NO_SOURCE_POINT));
        }

        let category = cell.source_category();
        if category == SourceCategory::SEGMENT_START_POINT {
            let segment_it = &segments[cell.source_index()];
            Ok(segment_it.low())
        } else if category == SourceCategory::SEGMENT_END_POINT {
            let segment_it = &segments[cell.source_index()];
            Ok(segment_it.high())
        } else if category == SourceCategory::SINGLE_POINT {
            Err(VoronoiError::RuntimeError(SINGLE_POINT))
        } else {
            Err(VoronoiError::InvalidArgument(GET_SOURCE_POINT_ONLY))
        }
    }

    /// `get_source_point_index(cell, segment_begin, segment_end)`: the point index of the segment
    /// end point a point cell holds (the segment's own index for its start point, the next one
    /// for its end point). The index is not checked: past the segments it panics.
    ///
    /// # Errors
    ///
    /// As [`VoronoiUtils::get_source_point`].
    pub fn get_source_point_index<'a>(
        cell: &VoronoiCell,
        segments: &[PolygonsSegmentIndex<'a>],
    ) -> Result<PolygonsPointIndex<'a>, VoronoiError> {
        if !cell.contains_point() {
            return Err(VoronoiError::InvalidArgument(NO_SOURCE_POINT));
        }

        let category = cell.source_category();
        if category == SourceCategory::SEGMENT_START_POINT {
            let segment_it = &segments[cell.source_index()];
            Ok(segment_it.0)
        } else if category == SourceCategory::SEGMENT_END_POINT {
            let segment_it = &segments[cell.source_index()];
            Ok(segment_it.0.next())
        } else if category == SourceCategory::SINGLE_POINT {
            Err(VoronoiError::RuntimeError(SINGLE_POINT))
        } else {
            Err(VoronoiError::InvalidArgument(GET_SOURCE_POINT_INDEX_ONLY))
        }
    }

    /// `discretize_parabola(source_point, source_segment, start, end, approximate_step_size,
    /// transitioning_angle)`: points of the parabola between `source_point` and the segment, from
    /// `start` to `end`, about `approximate_step_size` apart measured along the segment, with the
    /// apex and the marking points of the transitioning angle where they fall between.
    #[must_use]
    pub fn discretize_parabola<S: SegmentConcept<Point = Point>>(
        source_point: Point,
        source_segment: &S,
        start: Point,
        end: Point,
        approximate_step_size: i64,
        transitioning_angle: f32,
    ) -> Points {
        let mut discretized = Points::new();
        // x is distance of point projected on the segment ab
        // xx is point projected on the segment ab
        let a = source_segment.low();
        let b = source_segment.high();
        let ab = b - a;
        let as_ = start - a;
        let ae = end - a;
        let ab_size: i64 = norm_i64(ab);
        let sx: i64 = as_.dot(ab) / ab_size;
        let ex: i64 = ae.dot(ab) / ab_size;
        let sxex: i64 = ex.wrapping_sub(sx);

        let ap = source_point - a;
        let px: i64 = ap.dot(ab) / ab_size;

        let (_, pxx) = Line::new(a, b).distance_to_infinite_squared(source_point);
        let ppxx = pxx - source_point;
        let d: i64 = norm_i64(ppxx);

        let rot: Vec2d = perp_cast_f64(ppxx).normalized();
        let rot_cos_theta = rot.x();
        let rot_sin_theta = rot.y();

        if d == 0 {
            discretized.push(start);
            discretized.push(end);
            return discretized;
        }

        let marking_bound = orca_crt::atan(f64::from(transitioning_angle) * 0.5);
        let mut msx: i64 = orca_crt::f64_to_i64(-marking_bound * i64_as_f64(d)); // projected marking_start
        let mut mex: i64 = orca_crt::f64_to_i64(marking_bound * i64_as_f64(d)); // projected marking_end

        let marking_start_end_h: i64 = msx.wrapping_mul(msx) / d.wrapping_mul(2) + d / 2;
        let mut marking_start =
            Point::new(msx, marking_start_end_h).rotated_cs(rot_cos_theta, rot_sin_theta) + pxx;
        let mut marking_end =
            Point::new(mex, marking_start_end_h).rotated_cs(rot_cos_theta, rot_sin_theta) + pxx;
        let dir: i32 = if sx > ex { -1 } else { 1 };
        if dir < 0 {
            core::mem::swap(&mut marking_start, &mut marking_end);
            core::mem::swap(&mut msx, &mut mex);
        }
        let dir = i64::from(dir);

        let mut add_marking_start = msx.wrapping_mul(dir) > sx.wrapping_sub(px).wrapping_mul(dir)
            && msx.wrapping_mul(dir) < ex.wrapping_sub(px).wrapping_mul(dir);
        let mut add_marking_end = mex.wrapping_mul(dir) > sx.wrapping_sub(px).wrapping_mul(dir)
            && mex.wrapping_mul(dir) < ex.wrapping_sub(px).wrapping_mul(dir);

        let apex = Point::new(0, d / 2).rotated_cs(rot_cos_theta, rot_sin_theta) + pxx;
        let mut add_apex =
            sx.wrapping_sub(px).wrapping_mul(dir) < 0 && ex.wrapping_sub(px).wrapping_mul(dir) > 0;

        if add_marking_start && add_marking_end && !add_apex {
            log(Severity::Warning, FAILING_TO_DISCRETIZE);
        }

        // `lround` returns `long`, 32 bits on Windows.
        let step_count: i64 = i64::from(orca_crt::lround(
            i64_as_f64(ex.wrapping_sub(sx).wrapping_abs()) / i64_as_f64(approximate_step_size),
        ));
        discretized.push(start);
        let mut step: i64 = 1;
        while step < step_count {
            let x: i64 = sx
                .wrapping_add(sxex.wrapping_mul(step) / step_count)
                .wrapping_sub(px);
            let y: i64 = x.wrapping_mul(x) / d.wrapping_mul(2) + d / 2;

            if add_marking_start && msx.wrapping_mul(dir) < x.wrapping_mul(dir) {
                discretized.push(marking_start);
                add_marking_start = false;
            }

            if add_apex && x.wrapping_mul(dir) > 0 {
                discretized.push(apex);
                add_apex = false; // only add the apex just before the
            }

            if add_marking_end && mex.wrapping_mul(dir) < x.wrapping_mul(dir) {
                discretized.push(marking_end);
                add_marking_end = false;
            }

            let result = Point::new(x, y).rotated_cs(rot_cos_theta, rot_sin_theta) + pxx;
            discretized.push(result);
            step += 1;
        }

        if add_apex {
            discretized.push(apex);
        }

        if add_marking_end {
            discretized.push(marking_end);
        }

        discretized.push(end);
        discretized
    }

    /// `compute_segment_cell_range(cell, segment_begin, segment_end)`: the range of a segment's
    /// cell, from the last edge that starts at the segment's end point (`to`) to the edge that ends
    /// at its start point (`from`), the infinite edges skipped. The range's start point is the
    /// segment's end point and its end point the start point, as the C++ builds it.
    ///
    /// # Errors
    ///
    /// The errors of [`VoronoiUtils::get_source_segment`].
    pub fn compute_segment_cell_range<S, D>(
        vd: &D,
        cell: &VoronoiCell,
        segments: &[S],
    ) -> Result<SegmentCellRange, VoronoiError>
    where
        S: SegmentConcept<Point = Point>,
        D: VoronoiContainers + ?Sized,
    {
        let source_segment = VoronoiUtils::get_source_segment(cell, segments)?;
        let from = source_segment.low();
        let to = source_segment.high();
        let from_i64 = from;
        let to_i64 = to;

        // FIXME @hejllukas: Ensure that there is no infinite edge during iteration between
        // edge_begin and edge_end.
        let mut cell_range = SegmentCellRange::new(to, from);

        // Find starting edge and end edge
        let mut seen_possible_start = false;
        let mut after_start = false;
        let mut ending_edge_is_set_before_start = false;
        let edges = vd.edges();
        let incident = cell.incident_edge();
        let mut edge = incident.expect("compute_segment_cell_range: the C++ follows a null edge");
        loop {
            let e = &edges[edge];
            if !e.is_infinite(edges) {
                let v0 = VoronoiUtils::to_point_ptr(vertex_at(vd, e.vertex0()));
                let v1 = VoronoiUtils::to_point_ptr(vertex_at(vd, e.vertex1(edges)));

                if v0 == to_i64 && !after_start {
                    // Use the last edge which starts in source_segment.to
                    cell_range.edge_begin = Some(edge);
                    seen_possible_start = true;
                } else if seen_possible_start {
                    after_start = true;
                }

                if v1 == from_i64
                    && (cell_range.edge_end.is_none() || ending_edge_is_set_before_start)
                {
                    ending_edge_is_set_before_start = !after_start;
                    cell_range.edge_end = Some(edge);
                }
            }
            edge = e
                .next()
                .expect("compute_segment_cell_range: the C++ follows a null next_");
            if Some(edge) == incident {
                break;
            }
        }

        Ok(cell_range)
    }

    /// `compute_point_cell_range(cell, segment_begin, segment_end)`: the range of a point's cell,
    /// from the edge after the one that ends at the point to that edge, when the cell's first edge
    /// is finite, inside the `coord_t` range and has its other end inside the polygon's corner at
    /// the point; else no edges.
    ///
    /// # Errors
    ///
    /// The errors of [`VoronoiUtils::get_source_point`] and
    /// [`VoronoiUtils::get_source_point_index`].
    pub fn compute_point_cell_range<D>(
        vd: &D,
        cell: &VoronoiCell,
        segments: &[PolygonsSegmentIndex<'_>],
    ) -> Result<PointCellRange, VoronoiError>
    where
        D: VoronoiContainers + ?Sized,
    {
        let source_point = VoronoiUtils::get_source_point(cell, segments)?;

        // We want to ignore (by returning PointCellRange without assigned edge_begin and
        // edge_end) cells outside the input polygon.
        let mut cell_range = PointCellRange::new(source_point);

        let edges = vd.edges();
        let incident = cell.incident_edge();
        let mut edge = incident.expect("compute_point_cell_range: the C++ follows a null edge");
        let e = &edges[edge];
        if e.is_infinite(edges) || !VoronoiUtils::is_in_range_edge::<i64, D>(vd, e) {
            // Ignore infinite edges, because they only occur outside the polygon. Also ignore
            // edges with endpoints that don't fit into CoordType, because such edges are
            // definitely outside the polygon.
            return Ok(cell_range);
        }

        let source_point_idx = VoronoiUtils::get_source_point_index(cell, segments)?;
        let edge_v0 = VoronoiUtils::to_point_ptr(vertex_at(vd, e.vertex0()));
        let edge_v1 = VoronoiUtils::to_point_ptr(vertex_at(vd, e.vertex1(edges)));
        let edge_query_point = if edge_v0 == source_point {
            edge_v1
        } else {
            edge_v0
        };

        // Check if the edge has another endpoint inside the corner of the polygon.
        if !is_point_inside_polygon_corner(
            source_point_idx.prev().p(),
            source_point_idx.p(),
            source_point_idx.next().p(),
            edge_query_point,
        ) {
            // If the endpoint isn't inside the corner of the polygon, it means that the whole
            // cell isn't inside the polygons, and we will ignore such cells.
            return Ok(cell_range);
        }

        let source_point_i64 = source_point;
        edge = incident.expect("compute_point_cell_range: the C++ follows a null edge");
        loop {
            let e = &edges[edge];
            let v1 = VoronoiUtils::to_point_ptr(vertex_at(vd, e.vertex1(edges)));
            if v1 == source_point_i64 {
                cell_range.edge_begin = e.next();
                cell_range.edge_end = Some(edge);
            }
            // Else, the C++ asserts (in its debug build only) that the edge starts at the point or
            // is primary: Arachne's input has no collinear edges.
            edge = e
                .next()
                .expect("compute_point_cell_range: the C++ follows a null next_");
            if Some(edge) == incident {
                break;
            }
        }

        Ok(cell_range)
    }

    /// `is_in_range<T>(double value)`: `double(lowest) <= value && value <= double(max)`.
    #[must_use]
    pub fn is_in_range<T: InRange>(value: f64) -> bool {
        T::LOWEST <= value && value <= T::MAX
    }

    /// `is_in_range<T>(const VD::vertex_type &vertex)`: finite and both coordinates in range.
    #[must_use]
    pub fn is_in_range_vertex<T: InRange>(vertex: &VoronoiVertex) -> bool {
        VoronoiUtils::is_finite(vertex)
            && VoronoiUtils::is_in_range::<T>(vertex.x())
            && VoronoiUtils::is_in_range::<T>(vertex.y())
    }

    /// `is_in_range<T>(const VD::edge_type &edge)`: both vertices exist and are in range.
    #[must_use]
    pub fn is_in_range_edge<T: InRange, D: VoronoiContainers + ?Sized>(
        vd: &D,
        edge: &VoronoiEdge,
    ) -> bool {
        let edges = vd.edges();
        let (Some(v0), Some(v1)) = (edge.vertex0(), edge.vertex1(edges)) else {
            return false;
        };

        VoronoiUtils::is_in_range_vertex::<T>(&vd.vertices()[v0])
            && VoronoiUtils::is_in_range_vertex::<T>(&vd.vertices()[v1])
    }
}

/// An `int64_t` converted to `double`, rounded to the nearest.
#[allow(clippy::cast_precision_loss)]
fn i64_as_f64(v: i64) -> f64 {
    v as f64
}

/// `perp(v).cast<double>()` of a `coord_t` vector: `(-v.y, v.x)` in `coord_t` (the negation
/// wrapping), then each converted to `double`.
fn perp_cast_f64(v: Point) -> Vec2d {
    vec2d(i64_as_f64(v.y.wrapping_neg()), i64_as_f64(v.x))
}

#[cfg(test)]
mod tests {
    use orca_geom::point::Point;
    use orca_geom::polygon::Polygon;

    use super::{SegmentCellRange, VoronoiError, VoronoiUtils};
    use crate::diagram::{VoronoiCell, VoronoiVertex};
    use crate::geometry_type::SourceCategory;
    use crate::polygons_point_index::PolygonsSegmentIndex;

    #[test]
    fn the_range_limits_are_doubles() {
        assert!(VoronoiUtils::is_in_range::<i64>(
            9_223_372_036_854_775_808.0
        ));
        assert!(!VoronoiUtils::is_in_range::<i32>(2_147_483_647.5));
        assert!(!VoronoiUtils::is_in_range::<i32>(f64::NAN));
        let v = VoronoiVertex::new(f64::INFINITY, 0.0);
        assert!(!VoronoiUtils::is_finite(&v) && !VoronoiUtils::is_in_range_vertex::<i64>(&v));
    }

    #[test]
    fn to_point_rounds_half_away_from_zero() {
        let v = VoronoiVertex::new(2.5, -2.5);
        assert_eq!(VoronoiUtils::to_point(&v), Point::new(3, -3));
        let w = VoronoiUtils::make_rotated_vertex(&v, 0.0);
        assert_eq!((w.x(), w.y()), (2.5, -2.5));
    }

    #[test]
    fn source_lookups_throw_as_the_cpp() {
        let polys = vec![Polygon::new(vec![
            Point::new(0, 0),
            Point::new(10, 0),
            Point::new(0, 10),
        ])];
        let segs: Vec<_> = (0..3)
            .map(|i| PolygonsSegmentIndex::new(&polys, 0, i))
            .collect();
        let segment_cell = VoronoiCell::new(2, SourceCategory::INITIAL_SEGMENT);
        assert_eq!(
            VoronoiUtils::get_source_segment(&segment_cell, &segs[..2]),
            Err(VoronoiError::OutOfRange(
                "Voronoi cell source index is out of range!"
            ))
        );
        assert_eq!(
            VoronoiUtils::get_source_point(&segment_cell, &segs),
            Err(VoronoiError::InvalidArgument(
                "Voronoi cell doesn't contain a source point!"
            ))
        );
        let end_cell = VoronoiCell::new(2, SourceCategory::SEGMENT_END_POINT);
        assert_eq!(
            VoronoiUtils::get_source_point(&end_cell, &segs),
            Ok(Point::new(0, 0))
        );
        assert_eq!(
            VoronoiUtils::get_source_point_index(&end_cell, &segs)
                .unwrap()
                .point_idx,
            0
        );
        let single = VoronoiCell::new(0, SourceCategory::SINGLE_POINT);
        assert!(matches!(
            VoronoiUtils::get_source_point(&single, &segs),
            Err(VoronoiError::RuntimeError(_))
        ));
        assert!(!SegmentCellRange::new(Point::new(0, 0), Point::new(1, 1)).is_valid());
    }

    #[test]
    fn a_parabola_on_its_point_is_its_ends() {
        let polys = vec![Polygon::new(vec![Point::new(0, 0), Point::new(100, 0)])];
        let seg = PolygonsSegmentIndex::new(&polys, 0, 0);
        let out = VoronoiUtils::discretize_parabola(
            Point::new(50, 0),
            &seg,
            Point::new(10, 5),
            Point::new(90, 5),
            10,
            0.1,
        );
        assert_eq!(out, vec![Point::new(10, 5), Point::new(90, 5)]);
        let out = VoronoiUtils::discretize_parabola(
            Point::new(50, 100),
            &seg,
            Point::new(0, 50),
            Point::new(100, 50),
            20,
            0.0,
        );
        assert_eq!(out.first(), Some(&Point::new(0, 50)));
        assert_eq!(out.last(), Some(&Point::new(100, 50)));
        assert!(out.contains(&Point::new(50, 50)));
    }
}
