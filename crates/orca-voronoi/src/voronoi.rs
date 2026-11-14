// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Boost code: Boost.Polygon of Boost 1.84.0, translated on 2026-11-14 by orca-o3-voronoi-3
// from boost/polygon/voronoi.hpp, lines 1-157 (insert and construct_voronoi for points, segments,
// and points with segments). Copyright Andrii Sydorchuk 2010-2012, under the Boost Software
// License, version 1.0 (BSL-1.0; licenses/BSL-1.0.txt). The concepts the functions take stand for
// the point and segment traits of boost/polygon/point_concept.hpp and segment_concept.hpp and for
// OrcaSlicer's specialisations of them (src/libslic3r/Point.hpp 785-791 and Line.hpp 328-335 of
// OrcaSlicer v2.4.2; Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and the
// PrusaSlicer authors (Prusa Research), under the GNU Affero General Public License, version 3
// (AGPL-3.0; LICENSE)).

//! Boost.Polygon's public entry to the Voronoi builder (`voronoi.hpp`): `insert` a point or a
//! segment, or a range of them, into a builder, and `construct_voronoi` of points, of segments, or
//! of points and then segments, with a fresh `default_voronoi_builder` each time.
//!
//! - A point is read through Boost's point concept ([`PointConcept`], `x(point)` and `y(point)`)
//!   and a segment through its segment concept ([`SegmentConcept`], `low(segment)` and
//!   `high(segment)`, the `segment_traits<Segment>::get` of `LOW` and `HIGH`). OrcaSlicer's
//!   specialisations are implemented here: `Slic3r::Point` is a point (its two coordinates) and
//!   `Slic3r::Line` a segment (`a` low, `b` high). `Slic3r::Geometry::VoronoiDiagram::Segment` and
//!   `Arachne::PolygonsSegmentIndex` implement [`SegmentConcept`] in their own modules.
//! - `insert_point` and `insert_segment` take `int32` coordinates, so `insert` narrows each
//!   coordinate it reads to `int32` as the C++'s implicit conversion does: a `coord_t` outside the
//!   32-bit range keeps its low 32 bits (`as i32`). parity/sop1/voronoi.md lists such coordinates
//!   among the edges where a port may stop with NotPorted; this translation gives the C++'s answer
//!   instead, since the narrowing is defined and the same bits here.
//! - `construct_voronoi` inserts the points first, then the segments, so the input indexes the
//!   cells carry count the points before the segments, and runs the builder into the diagram,
//!   which it does not clear first ([`crate::diagram`]).
//!
//! Left out: the `enable_if` and `gtl_if` dispatch of the C++ overloads, which pick by the
//! concept of the iterator's value: the overloads are functions of their own names here.

use orca_geom::line::Line;
use orca_geom::point::Point;

use crate::builder::{VoronoiBuilder, VoronoiOutput};

/// Boost.Polygon's point concept, as `insert` reads a point: `x(point)` and `y(point)`, the
/// `point_traits<Point>::get` of `HORIZONTAL` and `VERTICAL`, widened to `i64` here (every
/// coordinate type OrcaSlicer gives the builder fits it).
pub trait PointConcept {
    /// `x(point)`.
    fn x(&self) -> i64;
    /// `y(point)`.
    fn y(&self) -> i64;
}

/// Boost.Polygon's segment concept, as `insert` and libslic3r's Voronoi code read a segment: its
/// two ends, `segment_traits<Segment>::get(segment, LOW)` and `get(segment, HIGH)`.
pub trait SegmentConcept {
    /// `segment_traits<Segment>::point_type`.
    type Point: PointConcept;
    /// `low(segment)`: `get(segment, LOW)`.
    fn low(&self) -> Self::Point;
    /// `high(segment)`: `get(segment, HIGH)`.
    fn high(&self) -> Self::Point;
}

/// `point_traits<Slic3r::Point>` (Point.hpp 785-791): `point(0)` and `point(1)`.
impl PointConcept for Point {
    fn x(&self) -> i64 {
        self.x
    }
    fn y(&self) -> i64 {
        self.y
    }
}

/// `segment_traits<Slic3r::Line>` (Line.hpp 328-335): `LOW` is `line.a`, `HIGH` is `line.b`.
impl SegmentConcept for Line {
    type Point = Point;
    fn low(&self) -> Point {
        self.a
    }
    fn high(&self) -> Point {
        self.b
    }
}

/// A coordinate as `insert_point` and `insert_segment` take it: the implicit conversion to
/// `int32`, which keeps the low 32 bits.
#[allow(clippy::cast_possible_truncation)]
fn narrow(v: i64) -> i32 {
    v as i32
}

/// `insert(point, vb)`: `vb->insert_point(x(point), y(point))`; the point's input index.
pub fn insert_point<P: PointConcept, E: Copy>(point: &P, vb: &mut VoronoiBuilder<E>) -> usize {
    vb.insert_point(narrow(point.x()), narrow(point.y()))
}

/// `insert(first, last, vb)` of points: each point in order.
pub fn insert_points<P: PointConcept, E: Copy>(points: &[P], vb: &mut VoronoiBuilder<E>) {
    for it in points {
        insert_point(it, vb);
    }
}

/// `insert(segment, vb)`: `vb->insert_segment(x(low(segment)), y(low(segment)),
/// x(high(segment)), y(high(segment)))`; the segment's input index.
pub fn insert_segment<S: SegmentConcept, E: Copy>(
    segment: &S,
    vb: &mut VoronoiBuilder<E>,
) -> usize {
    let low = segment.low();
    let high = segment.high();
    vb.insert_segment(
        narrow(low.x()),
        narrow(low.y()),
        narrow(high.x()),
        narrow(high.y()),
    )
}

/// `insert(first, last, vb)` of segments: each segment in order.
pub fn insert_segments<S: SegmentConcept, E: Copy>(segments: &[S], vb: &mut VoronoiBuilder<E>) {
    for it in segments {
        insert_segment(it, vb);
    }
}

/// `construct_voronoi(first, last, vd)` of points: a fresh `default_voronoi_builder`, the points
/// inserted, then `construct(vd)`.
pub fn construct_voronoi_points<P: PointConcept, O: VoronoiOutput>(points: &[P], vd: &mut O) {
    let mut builder = VoronoiBuilder::<O::Edge>::new();
    insert_points(points, &mut builder);
    builder.construct(vd);
}

/// `construct_voronoi(first, last, vd)` of segments: a fresh `default_voronoi_builder`, the
/// segments inserted, then `construct(vd)`.
pub fn construct_voronoi_segments<S: SegmentConcept, O: VoronoiOutput>(segments: &[S], vd: &mut O) {
    let mut builder = VoronoiBuilder::<O::Edge>::new();
    insert_segments(segments, &mut builder);
    builder.construct(vd);
}

/// `construct_voronoi(p_first, p_last, s_first, s_last, vd)`: a fresh `default_voronoi_builder`,
/// the points inserted, then the segments, then `construct(vd)`.
pub fn construct_voronoi_points_segments<P: PointConcept, S: SegmentConcept, O: VoronoiOutput>(
    points: &[P],
    segments: &[S],
    vd: &mut O,
) {
    let mut builder = VoronoiBuilder::<O::Edge>::new();
    insert_points(points, &mut builder);
    insert_segments(segments, &mut builder);
    builder.construct(vd);
}

#[cfg(test)]
mod tests {
    use orca_geom::line::Line;
    use orca_geom::point::Point;

    use super::{construct_voronoi_points, construct_voronoi_points_segments, insert_point};
    use crate::diagram::BoostVoronoiDiagram;
    use crate::{SourceCategory, VoronoiBuilder};

    #[test]
    fn coordinates_are_narrowed_to_32_bits() {
        let mut b = VoronoiBuilder::<usize>::new();
        assert_eq!(insert_point(&Point::new((1_i64 << 32) + 5, -1), &mut b), 0);
        let s = b.site_events()[0];
        assert_eq!((s.x(), s.y()), (5, -1));
    }

    #[test]
    fn points_come_before_segments() {
        let mut vd = BoostVoronoiDiagram::new();
        let points = [Point::new(0, 10)];
        let lines = [Line::new(Point::new(-10, 0), Point::new(10, 0))];
        construct_voronoi_points_segments(&points, &lines, &mut vd);
        let point_cell = vd
            .cells()
            .iter()
            .find(|c| c.source_category() == SourceCategory::SINGLE_POINT)
            .unwrap();
        assert_eq!(point_cell.source_index(), 0);
        assert!(
            vd.cells()
                .iter()
                .filter(|c| c.source_category() != SourceCategory::SINGLE_POINT)
                .all(|c| c.source_index() == 1)
        );
        let mut vd = BoostVoronoiDiagram::new();
        construct_voronoi_points(&[Point::new(0, 0), Point::new(4, 0)], &mut vd);
        assert_eq!((vd.num_cells(), vd.num_edges()), (2, 2));
    }
}
