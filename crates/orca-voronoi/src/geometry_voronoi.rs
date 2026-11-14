// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Geometry/Voronoi.hpp lines 1-212 of OrcaSlicer v2.4.2
// (the class Slic3r::Geometry::VoronoiDiagram, its issue and state, its accessors, the point and
// mixed overloads of construct_voronoi, its Segment and Segment's segment_traits), translated on
// 2026-11-14 by orca-o3-voronoi-3. Copyright the OrcaSlicer authors and the PrusaSlicer authors
// (Prusa Research), under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! `Slic3r::Geometry::VoronoiDiagram` (`Geometry/Voronoi.hpp`): Boost.Polygon's diagram
//! ([`BoostVoronoiDiagram`]) with libslic3r's detection of its known issues and its repair by
//! rotating the input.
//!
//! - The object holds the builder's diagram and, once a repair has run, its own copies of the
//!   cells, vertices and edges (`m_is_modified`): the accessors ([`VoronoiDiagram::cells`] and the
//!   rest) read the copies then, and the builder's diagram otherwise. A later construction builds
//!   into the builder's diagram while the accessors still read the copies, as the C++ does.
//! - [`IssueType`] and [`State`] say what the detection found and what the repair did;
//!   [`VoronoiDiagram::is_valid`] is false only after a repair that failed.
//! - The point and the mixed overloads of `construct_voronoi` ([`VoronoiDiagram::construct_voronoi_points`],
//!   [`VoronoiDiagram::construct_voronoi_points_segments`]) build into the builder's diagram
//!   without clearing it and never repair: the state and the issue are `UNKNOWN`.
//! - [`Segment`] is the rotated input the repair builds from (`segments_rotated`); its segment
//!   traits give `from` as `LOW` and `to` as `HIGH`.
//!
//! The members are `pub(crate)` for the methods whose bodies are in `Geometry/Voronoi.cpp`, which
//! voronoi-4 carries in its own module of this crate: `construct_voronoi` of segments (with the
//! detection and the repair), `clear`, `copy_to_local`, `detect_known_issues`,
//! `detect_known_voronoi_cell_issues`, `detect_known_voronoi_edge_issues`,
//! `try_to_repair_degenerated_voronoi_diagram` and
//! `try_to_repair_degenerated_voronoi_diagram_by_rotation`; this module declares none of them.
//! Left out besides: the virtual destructor (nothing derives from the class), the type aliases
//! (`point_type`, `segment_type`, `rect_type` and the container and iterator types; the elements
//! are [`VoronoiCell`], [`VoronoiVertex`] and [`VoronoiEdge`], the containers slices) and
//! `SegmentIt` (a slice of [`Segment`]s).

use orca_geom::point::Point;

use crate::diagram::{BoostVoronoiDiagram, VoronoiCell, VoronoiEdge, VoronoiVertex};
use crate::voronoi::{
    PointConcept, SegmentConcept, construct_voronoi_points, construct_voronoi_points_segments,
};

/// `VoronoiDiagram::IssueType`: what the detection of known issues found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum IssueType {
    /// `NO_ISSUE_DETECTED`.
    NoIssueDetected = 0,
    /// `FINITE_EDGE_WITH_NON_FINITE_VERTEX`.
    FiniteEdgeWithNonFiniteVertex = 1,
    /// `MISSING_VORONOI_VERTEX`.
    MissingVoronoiVertex = 2,
    /// `NON_PLANAR_VORONOI_DIAGRAM`.
    NonPlanarVoronoiDiagram = 3,
    /// `VORONOI_EDGE_INTERSECTING_INPUT_SEGMENT`.
    VoronoiEdgeIntersectingInputSegment = 4,
    /// `PARABOLIC_VORONOI_EDGE_WITHOUT_FOCUS_POINT`.
    ParabolicVoronoiEdgeWithoutFocusPoint = 5,
    /// `UNKNOWN`: repairs are disabled in the constructor.
    Unknown = 6,
}

/// `VoronoiDiagram::State`: what the repair did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum State {
    /// `REPAIR_NOT_NEEDED`: the original Voronoi diagram doesn't have any issue.
    RepairNotNeeded = 0,
    /// `REPAIR_SUCCESSFUL`: the original Voronoi diagram has some issues, but it was repaired.
    RepairSuccessful = 1,
    /// `REPAIR_UNSUCCESSFUL`: the original Voronoi diagram has some issues, but it wasn't
    /// repaired.
    RepairUnsuccessful = 2,
    /// `UNKNOWN`: repairs are disabled in the constructor.
    Unknown = 3,
}

/// `Slic3r::Geometry::VoronoiDiagram`.
#[derive(Debug)]
pub struct VoronoiDiagram {
    /// `m_voronoi_diagram`: the builder's diagram.
    pub(crate) voronoi_diagram: BoostVoronoiDiagram,
    /// `m_vertices`: the repair's copy.
    pub(crate) vertices: Vec<VoronoiVertex>,
    /// `m_edges`: the repair's copy.
    pub(crate) edges: Vec<VoronoiEdge>,
    /// `m_cells`: the repair's copy.
    pub(crate) cells: Vec<VoronoiCell>,
    /// `m_is_modified`: the accessors read the copies.
    pub(crate) is_modified: bool,
    /// `m_state`.
    pub(crate) state: State,
    /// `m_issue_type`.
    pub(crate) issue_type: IssueType,
}

impl Default for VoronoiDiagram {
    /// `VoronoiDiagram() = default`: an empty diagram, not modified, state and issue `UNKNOWN`.
    fn default() -> Self {
        Self {
            voronoi_diagram: BoostVoronoiDiagram::new(),
            vertices: Vec::new(),
            edges: Vec::new(),
            cells: Vec::new(),
            is_modified: false,
            state: State::Unknown,
            issue_type: IssueType::Unknown,
        }
    }
}

impl VoronoiDiagram {
    /// `VoronoiDiagram()`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `get_issue_type()`.
    #[must_use]
    pub fn get_issue_type(&self) -> IssueType {
        self.issue_type
    }

    /// `get_state()`.
    #[must_use]
    pub fn get_state(&self) -> State {
        self.state
    }

    /// `is_valid()`: the state is not `REPAIR_UNSUCCESSFUL`.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.state != State::RepairUnsuccessful
    }

    /// `vertices()`: the copies once modified, else the builder's.
    #[must_use]
    pub fn vertices(&self) -> &[VoronoiVertex] {
        if self.is_modified {
            &self.vertices
        } else {
            self.voronoi_diagram.vertices()
        }
    }

    /// `edges()`: the copies once modified, else the builder's.
    #[must_use]
    pub fn edges(&self) -> &[VoronoiEdge] {
        if self.is_modified {
            &self.edges
        } else {
            self.voronoi_diagram.edges()
        }
    }

    /// `cells()`: the copies once modified, else the builder's.
    #[must_use]
    pub fn cells(&self) -> &[VoronoiCell] {
        if self.is_modified {
            &self.cells
        } else {
            self.voronoi_diagram.cells()
        }
    }

    /// `num_vertices()`.
    #[must_use]
    pub fn num_vertices(&self) -> usize {
        if self.is_modified {
            self.vertices.len()
        } else {
            self.voronoi_diagram.num_vertices()
        }
    }

    /// `num_edges()`.
    #[must_use]
    pub fn num_edges(&self) -> usize {
        if self.is_modified {
            self.edges.len()
        } else {
            self.voronoi_diagram.num_edges()
        }
    }

    /// `num_cells()`.
    #[must_use]
    pub fn num_cells(&self) -> usize {
        if self.is_modified {
            self.cells.len()
        } else {
            self.voronoi_diagram.num_cells()
        }
    }

    /// `construct_voronoi(first, last)` of points: Boost's `construct_voronoi` into the builder's
    /// diagram; the state and the issue `UNKNOWN` (no repair).
    pub fn construct_voronoi_points<P: PointConcept>(&mut self, points: &[P]) {
        construct_voronoi_points(points, &mut self.voronoi_diagram);
        self.state = State::Unknown;
        self.issue_type = IssueType::Unknown;
    }

    /// `construct_voronoi(p_first, p_last, s_first, s_last)`: Boost's `construct_voronoi` of the
    /// points and then the segments into the builder's diagram; the state and the issue `UNKNOWN`
    /// (no repair).
    pub fn construct_voronoi_points_segments<P: PointConcept, S: SegmentConcept>(
        &mut self,
        points: &[P],
        segments: &[S],
    ) {
        construct_voronoi_points_segments(points, segments, &mut self.voronoi_diagram);
        self.state = State::Unknown;
        self.issue_type = IssueType::Unknown;
    }
}

/// `VoronoiDiagram::Segment`: a segment of two points, the repair's rotated input.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Segment {
    /// `from`.
    pub from: Point,
    /// `to`.
    pub to: Point,
}

impl Segment {
    /// `explicit Segment(from, to)` (the default constructor is deleted).
    #[must_use]
    pub fn new(from: Point, to: Point) -> Self {
        Self { from, to }
    }
}

/// `segment_traits<Slic3r::Geometry::VoronoiDiagram::Segment>`: `LOW` is `from`, `HIGH` is `to`.
impl SegmentConcept for Segment {
    type Point = Point;
    fn low(&self) -> Point {
        self.from
    }
    fn high(&self) -> Point {
        self.to
    }
}

#[cfg(test)]
mod tests {
    use orca_geom::line::Line;
    use orca_geom::point::Point;

    use super::{IssueType, State, VoronoiDiagram};

    #[test]
    fn the_point_overloads_never_repair() {
        let mut vd = VoronoiDiagram::new();
        assert_eq!(vd.get_state(), State::Unknown);
        vd.construct_voronoi_points(&[Point::new(0, 0), Point::new(10, 0), Point::new(0, 10)]);
        assert_eq!(
            (vd.get_state(), vd.get_issue_type()),
            (State::Unknown, IssueType::Unknown)
        );
        assert!(vd.is_valid());
        assert_eq!(
            (vd.num_cells(), vd.num_vertices(), vd.num_edges()),
            (3, 1, 6)
        );
        assert_eq!(vd.cells().len(), 3);
        let mut vd = VoronoiDiagram::new();
        vd.construct_voronoi_points_segments(
            &[Point::new(0, 10)],
            &[Line::new(Point::new(-10, 0), Point::new(10, 0))],
        );
        assert_eq!(vd.num_cells(), 4);
        assert_eq!(vd.edges().len(), vd.num_edges());
        assert_eq!(vd.vertices().len(), vd.num_vertices());
    }
}
