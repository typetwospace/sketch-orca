// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Geometry/Voronoi.cpp lines 1-364 of OrcaSlicer v2.4.2
// (VoronoiDiagram's construct_voronoi of segments with the detection of known issues and the repair
// by rotation, clear, copy_to_local, the three detections, the two repairs and the encoding of
// segment end points in vertex colours), translated on 2026-11-14 by orca-o3-voronoi-4. Copyright
// the OrcaSlicer authors and the PrusaSlicer authors (Prusa Research), under the GNU Affero
// General Public License, version 3 (AGPL-3.0; LICENSE).

//! The methods of `Slic3r::Geometry::VoronoiDiagram` whose bodies are in `Geometry/Voronoi.cpp`:
//! the construction from segments, with the detection of Boost.Polygon's known issues and the
//! repair by rotating the input.
//!
//! - [`VoronoiDiagram::construct_voronoi_segments`] builds Boost's diagram into the builder's
//!   diagram. With the repair on it runs [`VoronoiDiagram::detect_known_issues`]; on an issue it
//!   logs a warning naming it (an error for an unknown one) and tries
//!   [`VoronoiDiagram::try_to_repair_degenerated_voronoi_diagram`]: the input turned by π/6, π/5,
//!   π/7 and π/11 in turn until a turn's diagram shows no issue. A repair that fails logs an error
//!   and leaves the state `REPAIR_UNSUCCESSFUL`, which [`VoronoiDiagram::is_valid`] reports.
//! - One rotation ([`VoronoiDiagram::try_to_repair_degenerated_voronoi_diagram_by_rotation`])
//!   builds the diagram of the turned segments, copies it into the object
//!   ([`VoronoiDiagram::copy_to_local`], after which the accessors read the copies and the
//!   builder's diagram is empty), runs the detection on it and turns the vertices back. A vertex
//!   at an end of a segment is not turned back but put on the original end exactly: the end is
//!   written in the vertex's colour first (`(index + 1) << 1 | end`, 1 for `to`), and every vertex
//!   colour is 0 again at the end.
//! - The detection: an edge issue first (a finite edge with a vertex that is not finite, a
//!   parabolic edge with no point site on either side), then a cell issue (a segment's cell whose
//!   range is not valid or reaches an infinite edge: a missing vertex; a vertex of the range on the
//!   right of the segment: an edge crossing the input), then a diagram that is not planar by
//!   `VoronoiUtilsCgal::is_voronoi_diagram_planar_angle`.
//!
//! A construction after a repair builds into the builder's diagram while the accessors still read
//! the copies of the last rotation, and the detection then runs on those with the new segments, as
//! the C++ does; a lookup there can fail (the stale diagram, HAZARDS.md). The exceptions the
//! lookups throw are returned ([`VoronoiError`]); a field the C++ assigns from a call that throws
//! is left as it was. A null pointer the C++ follows panics.
//!
//! Left out: the `ColoredLines` instantiation (multi-material segmentation, outside domain A:
//! `ColoredLine` is not translated, so nothing can call it).

use orca_geom::point::{PI, Point, cross2, vec2d};
use orca_mesh::log::{Severity, log};

use crate::diagram::{BoostVoronoiDiagram, VoronoiCell, VoronoiEdge, VoronoiVertex};
use crate::geometry_voronoi::{IssueType, Segment, State, VoronoiDiagram};
use crate::voronoi::{SegmentConcept, construct_voronoi_segments as boost_construct_voronoi};
use crate::voronoi_utils::{VoronoiError, VoronoiUtils};
use crate::voronoi_utils_cgal::VoronoiUtilsCgal;

/// The warning of a missing vertex before the repair.
const WARN_MISSING_VERTEX: &str =
    "Detected missing Voronoi vertex, input polygons will be rotated back and forth.";
/// The warning of a diagram that is not planar before the repair.
const WARN_NON_PLANAR: &str =
    "Detected non-planar Voronoi diagram, input polygons will be rotated back and forth.";
/// The warning of an edge crossing the input before the repair.
const WARN_INTERSECTING: &str = "Detected Voronoi edge intersecting input segment, input polygons will be rotated back and forth.";
/// The warning of a finite edge with a vertex that is not finite before the repair.
const WARN_NON_FINITE_VERTEX: &str = "Detected finite Voronoi vertex with non finite vertex, input polygons will be rotated back and forth.";
/// The warning of a parabolic edge with no focus before the repair.
const WARN_NO_FOCUS: &str = "Detected parabolic Voronoi edges without focus point, input polygons will be rotated back and forth.";
/// The error of an unknown issue before the repair.
const ERROR_UNKNOWN: &str =
    "Detected unknown Voronoi diagram issue, input polygons will be rotated back and forth.";
/// The error of a missing vertex after the repair.
const AFTER_MISSING_VERTEX: &str =
    "Detected missing Voronoi vertex even after the rotation of input.";
/// The error of a diagram that is not planar after the repair.
const AFTER_NON_PLANAR: &str =
    "Detected non-planar Voronoi diagram even after the rotation of input.";
/// The error of an edge crossing the input after the repair.
const AFTER_INTERSECTING: &str =
    "Detected Voronoi edge intersecting input segment even after the rotation of input.";
/// The error of a finite edge with a vertex that is not finite after the repair.
const AFTER_NON_FINITE_VERTEX: &str =
    "Detected finite Voronoi vertex with non finite vertex even after the rotation of input.";
/// The error of a parabolic edge with no focus after the repair.
const AFTER_NO_FOCUS: &str =
    "Detected parabolic Voronoi edges without focus point even after the rotation of input.";
/// The error of an unknown issue after the repair.
const AFTER_UNKNOWN: &str =
    "Detected unknown Voronoi diagram issue even after the rotation of input.";

/// `encode_input_segment_endpoint(cell_source_index, dir)`: `(index + 1) << 1`, with bit 0 set for
/// `HIGH` (the segment's `to`).
fn encode_input_segment_endpoint(cell_source_index: usize, dir_high: bool) -> usize {
    ((cell_source_index + 1) << 1) | usize::from(dir_high)
}

/// `decode_input_segment_endpoint(color, segment_begin, segment_end)`: the end of the original
/// segment a colour names.
fn decode_input_segment_endpoint<S: SegmentConcept<Point = Point>>(
    color: usize,
    segments: &[S],
) -> Point {
    let segment_idx = (color >> 1) - 1;
    let segment_it = &segments[segment_idx];
    if (color & 1) != 0 {
        segment_it.high()
    } else {
        segment_it.low()
    }
}

/// The vertex at a link the C++ follows.
fn vertex_of(vertices: &[VoronoiVertex], v: Option<usize>) -> &VoronoiVertex {
    &vertices[v.expect("VoronoiDiagram: the C++ follows a null vertex")]
}

impl VoronoiDiagram {
    /// `construct_voronoi(segment_begin, segment_end, try_to_repair_if_needed)` of segments
    /// (`true` is the C++'s default): Boost's `construct_voronoi` into the builder's diagram and,
    /// with the repair on, the detection and the repair (the module's header says how).
    ///
    /// # Errors
    ///
    /// The lookups' exceptions out of the detection (a stale diagram's, HAZARDS.md).
    pub fn construct_voronoi_segments<S: SegmentConcept<Point = Point>>(
        &mut self,
        segments: &[S],
        try_to_repair_if_needed: bool,
    ) -> Result<(), VoronoiError> {
        boost_construct_voronoi(segments, &mut self.voronoi_diagram);
        if try_to_repair_if_needed {
            self.issue_type = Self::detect_known_issues(self, segments)?;
            if self.issue_type != IssueType::NoIssueDetected {
                if self.issue_type == IssueType::MissingVoronoiVertex {
                    log(Severity::Warning, WARN_MISSING_VERTEX);
                } else if self.issue_type == IssueType::NonPlanarVoronoiDiagram {
                    log(Severity::Warning, WARN_NON_PLANAR);
                } else if self.issue_type == IssueType::VoronoiEdgeIntersectingInputSegment {
                    log(Severity::Warning, WARN_INTERSECTING);
                } else if self.issue_type == IssueType::FiniteEdgeWithNonFiniteVertex {
                    log(Severity::Warning, WARN_NON_FINITE_VERTEX);
                } else if self.issue_type == IssueType::ParabolicVoronoiEdgeWithoutFocusPoint {
                    log(Severity::Warning, WARN_NO_FOCUS);
                } else {
                    log(Severity::Error, ERROR_UNKNOWN);
                }

                self.issue_type = self.try_to_repair_degenerated_voronoi_diagram(segments)?;
                if self.issue_type != IssueType::NoIssueDetected {
                    if self.issue_type == IssueType::MissingVoronoiVertex {
                        log(Severity::Error, AFTER_MISSING_VERTEX);
                    } else if self.issue_type == IssueType::NonPlanarVoronoiDiagram {
                        log(Severity::Error, AFTER_NON_PLANAR);
                    } else if self.issue_type == IssueType::VoronoiEdgeIntersectingInputSegment {
                        log(Severity::Error, AFTER_INTERSECTING);
                    } else if self.issue_type == IssueType::FiniteEdgeWithNonFiniteVertex {
                        log(Severity::Error, AFTER_NON_FINITE_VERTEX);
                    } else if self.issue_type == IssueType::ParabolicVoronoiEdgeWithoutFocusPoint {
                        log(Severity::Error, AFTER_NO_FOCUS);
                    } else {
                        log(Severity::Error, AFTER_UNKNOWN);
                    }

                    self.state = State::RepairUnsuccessful;
                } else {
                    self.state = State::RepairSuccessful;
                }
            } else {
                self.state = State::RepairNotNeeded;
                self.issue_type = IssueType::NoIssueDetected;
            }
        } else {
            self.state = State::Unknown;
            self.issue_type = IssueType::Unknown;
        }
        Ok(())
    }

    /// `clear()`: the copies when the diagram was modified (no longer modified then), else the
    /// builder's diagram; the state and the issue `UNKNOWN`.
    pub fn clear(&mut self) {
        if self.is_modified {
            self.vertices.clear();
            self.edges.clear();
            self.cells.clear();
            self.is_modified = false;
        } else {
            self.voronoi_diagram.clear();
        }

        self.state = State::Unknown;
        self.issue_type = IssueType::Unknown;
    }

    /// `copy_to_local(voronoi_diagram)`: copies the edges, cells and vertices of a diagram with
    /// their colours and links (the same indexes), empties the builder's diagram and marks the
    /// object modified.
    pub fn copy_to_local(&mut self, voronoi_diagram: &BoostVoronoiDiagram) {
        self.edges.clear();
        self.cells.clear();
        self.vertices.clear();

        // Copy Voronoi edges.
        self.edges.reserve(voronoi_diagram.num_edges());
        for edge in voronoi_diagram.edges() {
            self.edges
                .push(VoronoiEdge::new(edge.is_linear(), edge.is_primary()));
            self.edges[self.edges.len() - 1].set_color(edge.color());
        }

        // Copy Voronoi cells.
        self.cells.reserve(voronoi_diagram.num_cells());
        for cell in voronoi_diagram.cells() {
            self.cells.push(VoronoiCell::new(
                cell.source_index(),
                cell.source_category(),
            ));
            let last = self.cells.len() - 1;
            self.cells[last].set_color(cell.color());

            if let Some(incident_edge_idx) = cell.incident_edge() {
                self.cells[last].set_incident_edge(Some(incident_edge_idx));
            }
        }

        // Copy Voronoi vertices.
        self.vertices.reserve(voronoi_diagram.num_vertices());
        for vertex in voronoi_diagram.vertices() {
            self.vertices
                .push(VoronoiVertex::new(vertex.x(), vertex.y()));
            let last = self.vertices.len() - 1;
            self.vertices[last].set_color(vertex.color());

            if let Some(incident_edge_idx) = vertex.incident_edge() {
                self.vertices[last].set_incident_edge(Some(incident_edge_idx));
            }
        }

        // Assign all pointers for each Voronoi edge.
        for (edge_idx, old_edge) in voronoi_diagram.edges().iter().enumerate() {
            let new_edge = &mut self.edges[edge_idx];

            if let Some(cell_idx) = old_edge.cell() {
                new_edge.set_cell(Some(cell_idx));
            }

            if let Some(vertex0_idx) = old_edge.vertex0() {
                new_edge.set_vertex0(Some(vertex0_idx));
            }

            if let Some(twin_edge_idx) = old_edge.twin() {
                new_edge.set_twin(Some(twin_edge_idx));
            }

            if let Some(next_edge_idx) = old_edge.next() {
                new_edge.set_next(Some(next_edge_idx));
            }

            if let Some(prev_edge_idx) = old_edge.prev() {
                new_edge.set_prev(Some(prev_edge_idx));
            }
        }

        self.voronoi_diagram.clear();
        self.is_modified = true;
    }

    /// `detect_known_issues(voronoi_diagram, segment_begin, segment_end)`: an edge issue, else a
    /// cell issue, else `NON_PLANAR_VORONOI_DIAGRAM` when the angle check fails, else none.
    ///
    /// # Errors
    ///
    /// The errors of the cell ranges and of the angle check's lookups.
    pub fn detect_known_issues<S: SegmentConcept<Point = Point>>(
        voronoi_diagram: &VoronoiDiagram,
        segments: &[S],
    ) -> Result<IssueType, VoronoiError> {
        let edge_issue_type = Self::detect_known_voronoi_edge_issues(voronoi_diagram);
        if edge_issue_type != IssueType::NoIssueDetected {
            return Ok(edge_issue_type);
        }
        let cell_issue_type = Self::detect_known_voronoi_cell_issues(voronoi_diagram, segments)?;
        if cell_issue_type != IssueType::NoIssueDetected {
            return Ok(cell_issue_type);
        } else if !VoronoiUtilsCgal::is_voronoi_diagram_planar_angle(voronoi_diagram, segments)? {
            // Detection of non-planar Voronoi diagram detects at least GH issues #8474, #8514 and
            // #8446.
            return Ok(IssueType::NonPlanarVoronoiDiagram);
        }

        Ok(IssueType::NoIssueDetected)
    }

    /// `detect_known_voronoi_cell_issues(voronoi_diagram, segment_begin, segment_end)`: on each
    /// segment's cell that is not degenerate, a range that is not valid or that reaches an
    /// infinite edge (a missing vertex), or an end of a range edge on the right of the segment (an
    /// edge crossing the input).
    ///
    /// # Errors
    ///
    /// The errors of `compute_segment_cell_range` and `get_source_segment`.
    pub fn detect_known_voronoi_cell_issues<S: SegmentConcept<Point = Point>>(
        voronoi_diagram: &VoronoiDiagram,
        segments: &[S],
    ) -> Result<IssueType, VoronoiError> {
        let edges = voronoi_diagram.edges();
        let vertices = voronoi_diagram.vertices();
        for cell in voronoi_diagram.cells() {
            if cell.is_degenerate() || !cell.contains_segment() {
                continue; // Skip degenerated cell that has no spoon. Also, skip a cell that doesn't contain a segment.
            }

            let cell_range =
                VoronoiUtils::compute_segment_cell_range(voronoi_diagram, cell, segments)?;
            if cell_range.is_valid() {
                // Detection if Voronoi edge is intersecting input segment.
                // It detects this type of issue at least in GH issues #8446, #8474 and #8514.

                let source_segment = VoronoiUtils::get_source_segment(cell, segments)?;
                let source_segment_from = source_segment.low().cast_f64();
                let source_segment_to = source_segment.high().cast_f64();
                let source_segment_vec = source_segment_to - source_segment_from;

                // All Voronoi vertices must be on the left side of the source segment, otherwise
                // the Voronoi diagram is invalid.
                let mut edge = cell_range.edge_begin;
                while edge != cell_range.edge_end {
                    let e = &edges[edge.expect("VoronoiDiagram: the C++ follows a null edge")];
                    if e.is_infinite(edges) {
                        // When there is a missing Voronoi vertex, we may encounter an infinite
                        // Voronoi edge. This happens, for example, in GH issue #8846.
                        return Ok(IssueType::MissingVoronoiVertex);
                    }
                    let v1 = vertex_of(vertices, e.vertex1(edges));
                    let edge_v1 = vec2d(v1.x(), v1.y());
                    if cross2(source_segment_vec, edge_v1 - source_segment_from) < 0.0 {
                        return Ok(IssueType::VoronoiEdgeIntersectingInputSegment);
                    }
                    edge = e.next();
                }
            } else {
                // When there is a missing Voronoi vertex (especially at one of the endpoints of
                // the input segment), the returned cell_range is marked as invalid.
                // It detects this type of issue at least in GH issue #8846.
                return Ok(IssueType::MissingVoronoiVertex);
            }
        }

        Ok(IssueType::NoIssueDetected)
    }

    /// `detect_known_voronoi_edge_issues(voronoi_diagram)`: a finite edge with a vertex that is
    /// not finite, or a parabolic edge with no point site on either side.
    #[must_use]
    pub fn detect_known_voronoi_edge_issues(voronoi_diagram: &VoronoiDiagram) -> IssueType {
        let edges = voronoi_diagram.edges();
        let vertices = voronoi_diagram.vertices();
        let cells = voronoi_diagram.cells();
        for edge in edges {
            if edge.is_finite(edges) {
                let (v0, v1) = (edge.vertex0(), edge.vertex1(edges));
                if v0.is_none()
                    || v1.is_none()
                    || !VoronoiUtils::is_finite(vertex_of(vertices, v0))
                    || !VoronoiUtils::is_finite(vertex_of(vertices, v1))
                {
                    return IssueType::FiniteEdgeWithNonFiniteVertex;
                }

                if edge.is_curved() {
                    let cell = &cells[edge
                        .cell()
                        .expect("VoronoiDiagram: the C++ follows a null cell")];
                    let twin = &edges[edge
                        .twin()
                        .expect("VoronoiDiagram: the C++ follows a null twin")];
                    let twin_cell = &cells[twin
                        .cell()
                        .expect("VoronoiDiagram: the C++ follows a null cell")];
                    if !cell.contains_point() && !twin_cell.contains_point() {
                        return IssueType::ParabolicVoronoiEdgeWithoutFocusPoint;
                    }
                }
            }
        }

        IssueType::NoIssueDetected
    }

    /// `try_to_repair_degenerated_voronoi_diagram(segment_begin, segment_end)`: the rotations by
    /// π/6, π/5, π/7 and π/11 in turn until one shows no issue; the last one's issue.
    ///
    /// # Errors
    ///
    /// A rotation's detection errors.
    pub fn try_to_repair_degenerated_voronoi_diagram<S: SegmentConcept<Point = Point>>(
        &mut self,
        segments: &[S],
    ) -> Result<IssueType, VoronoiError> {
        let mut issue_type = self.issue_type;

        let fix_angles = [PI / 6.0, PI / 5.0, PI / 7.0, PI / 11.0];
        for fix_angle in fix_angles {
            issue_type =
                self.try_to_repair_degenerated_voronoi_diagram_by_rotation(segments, fix_angle)?;
            if issue_type == IssueType::NoIssueDetected {
                return Ok(issue_type);
            }
        }

        Ok(issue_type)
    }

    /// `try_to_repair_degenerated_voronoi_diagram_by_rotation(segment_begin, segment_end,
    /// fix_angle)`: the diagram of the segments turned by `fix_angle` (`Point::rotated`), copied
    /// into the object, its issue detected, and its vertices turned back, those at segment ends put
    /// on the original ends (the module's header says how).
    ///
    /// # Errors
    ///
    /// The detection's errors (the copies are in place then, not turned back) and the cell
    /// ranges'.
    pub fn try_to_repair_degenerated_voronoi_diagram_by_rotation<
        S: SegmentConcept<Point = Point>,
    >(
        &mut self,
        segments: &[S],
        fix_angle: f64,
    ) -> Result<IssueType, VoronoiError> {
        // Copy all segments and rotate their vertices.
        let mut segments_rotated: Vec<Segment> = Vec::with_capacity(segments.len());
        for segment_it in segments {
            let from = segment_it.low();
            let to = segment_it.high();
            segments_rotated.push(Segment::new(from.rotated(fix_angle), to.rotated(fix_angle)));
        }

        let mut voronoi_diagram_rotated = BoostVoronoiDiagram::new();
        boost_construct_voronoi(&segments_rotated, &mut voronoi_diagram_rotated);

        self.copy_to_local(&voronoi_diagram_rotated);
        let issue_type = Self::detect_known_issues(self, &segments_rotated)?;

        // We want to remap all Voronoi vertices at the endpoints of input segments
        // to ensure that Voronoi vertices at endpoints will be preserved after rotation.
        // So we assign every Voronoi vertices color to map this Vertex into input segments.
        for cell in &self.cells {
            if cell.is_degenerate() {
                continue;
            }

            if cell.contains_segment() {
                let cell_range =
                    VoronoiUtils::compute_segment_cell_range(&*self, cell, &segments_rotated)?;
                if cell_range.is_valid() {
                    let edge_end =
                        &self.edges[cell_range.edge_end.expect("a valid range has its end edge")];
                    let edge_end_v1 = vertex_of(&self.vertices, edge_end.vertex1(&self.edges));
                    if edge_end_v1.color() == 0 {
                        // Vertex 1 of edge_end points to the starting endpoint of the input
                        // segment (from() or line.a).
                        let color = encode_input_segment_endpoint(cell.source_index(), false);
                        edge_end_v1.set_color(color);
                    }

                    let edge_begin = &self.edges[cell_range
                        .edge_begin
                        .expect("a valid range has its begin edge")];
                    let edge_begin_v0 = vertex_of(&self.vertices, edge_begin.vertex0());
                    if edge_begin_v0.color() == 0 {
                        // Vertex 0 of edge_end points to the ending endpoint of the input segment
                        // (to() or line.b).
                        let color = encode_input_segment_endpoint(cell.source_index(), true);
                        edge_begin_v0.set_color(color);
                    }
                }
                // Else: this could happen when there is a missing Voronoi vertex even after
                // rotation (the C++ asserts in its debug build only).
            }

            // FIXME @hejllukas: Implement mapping also for source points and not just for source
            // segments.
        }

        // Rotate all Voronoi vertices back.
        // When a Voronoi vertex can be mapped to the input segment endpoint, then we don't need to
        // do rotation back.
        for vertex in &mut self.vertices {
            if vertex.color() == 0 {
                // This vertex isn't mapped to any vertex, so we rotate it back.
                *vertex = VoronoiUtils::make_rotated_vertex(vertex, -fix_angle);
            } else {
                // This vertex can be mapped to the input segment endpoint.
                let endpoint = decode_input_segment_endpoint(vertex.color(), segments);
                let mut endpoint_vertex = VoronoiVertex::new(endpoint.x as f64, endpoint.y as f64);
                endpoint_vertex.set_incident_edge(vertex.incident_edge());
                endpoint_vertex.set_color(vertex.color());
                *vertex = endpoint_vertex;
            }
        }

        // We have to clear all marked vertices because some algorithms expect that all vertices
        // have a color equal to 0.
        for vertex in &self.vertices {
            vertex.set_color(0);
        }

        Ok(issue_type)
    }
}

#[cfg(test)]
mod tests {
    use orca_geom::line::Line;
    use orca_geom::point::Point;

    use super::{decode_input_segment_endpoint, encode_input_segment_endpoint};
    use crate::geometry_voronoi::{IssueType, State, VoronoiDiagram};

    fn square(size: i64) -> Vec<Line> {
        let p = [
            Point::new(0, 0),
            Point::new(size, 0),
            Point::new(size, size),
            Point::new(0, size),
        ];
        (0..4).map(|i| Line::new(p[i], p[(i + 1) % 4])).collect()
    }

    #[test]
    fn a_square_needs_no_repair() {
        let lines = square(1000);
        let mut vd = VoronoiDiagram::new();
        vd.construct_voronoi_segments(&lines, true).unwrap();
        assert_eq!(
            (vd.get_state(), vd.get_issue_type()),
            (State::RepairNotNeeded, IssueType::NoIssueDetected)
        );
        assert!(vd.is_valid());
        vd.construct_voronoi_segments(&lines, false).unwrap();
        assert_eq!(
            (vd.get_state(), vd.get_issue_type()),
            (State::Unknown, IssueType::Unknown)
        );
        vd.clear();
        assert_eq!(vd.num_edges(), 0);
    }

    #[test]
    fn a_rotation_puts_segment_ends_back_exactly() {
        let lines = square(1_000_000);
        let mut vd = VoronoiDiagram::new();
        let issue = vd
            .try_to_repair_degenerated_voronoi_diagram_by_rotation(
                &lines,
                core::f64::consts::PI / 7.0,
            )
            .unwrap();
        assert_eq!(issue, IssueType::NoIssueDetected);
        // The corners are the square's own, not turned back by sines and cosines.
        for corner in [(0.0, 0.0), (1e6, 0.0), (1e6, 1e6), (0.0, 1e6)] {
            assert!(vd.vertices().iter().any(|v| (v.x(), v.y()) == corner));
        }
        assert!(vd.vertices().iter().all(|v| v.color() == 0));
        assert_eq!(encode_input_segment_endpoint(2, true), 7);
        assert_eq!(decode_input_segment_endpoint(7, &lines), lines[2].b);
        assert_eq!(decode_input_segment_endpoint(6, &lines), lines[2].a);
    }
}
