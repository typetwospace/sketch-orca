// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Geometry/MedialAxis.hpp lines 1-53 and MedialAxis.cpp
// lines 1-707 (the class MedialAxis: its constructor, both build methods, process_edge_neighbors
// and validate_edge) and src/libslic3r/ExPolygon.cpp lines 261-378 (ExPolygon::medial_axis of
// ThickPolylines and of Polylines) of OrcaSlicer v2.4.2, translated on 2026-11-14 by
// orca-o3-voronoi-4. Copyright the OrcaSlicer authors and the PrusaSlicer authors (Prusa
// Research), under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! libslic3r's medial axis: the skeleton of a thin region, as polylines with a width at each end
//! of each segment, which thin walls and gap fill extrude.
//!
//! - [`MedialAxis::build`] constructs the Voronoi diagram of the region's lines with the repair
//!   on. When that is not valid it builds again, on the same object, from the lines of the
//!   region's morphological closing by `2 * SCALED_EPSILON` (an error is logged when that is not
//!   valid either). It annotates the diagram ([`crate::voronoi_offset::annotate_inside_outside`]),
//!   keeps each primary finite edge with an end inside that [`MedialAxis::validate_edge`] accepts,
//!   and chains the kept edges into polylines, forward from each seed edge and then backward from
//!   its twin.
//! - `validate_edge` measures the width at both ends (twice the distance to the other cell's
//!   segment or point), drops edges between two segments that do not face each other within π/8
//!   unless they are short, edges thinner than `SCALED_EPSILON`, and edges outside the widths.
//! - [`ExPolygonMedialAxis::medial_axis`] (`ExPolygon::medial_axis`) extends the free ends of the
//!   build's polylines to the contour, drops those shorter than twice the largest width and, when
//!   it dropped any, joins polylines that meet end to end.
//!
//! Behaviours the C++ has and this keeps (HAZARDS.md): the backward chain's polyline lives across
//! seeds and its `clear()` keeps its end flags, so once a backward chain ended free every later
//! polyline's first end is marked free; the largest width is taken with `fmaxf`, so through
//! `float`s; the second construction after a failed repair detects on the stale diagram (module
//! `repair`). The edges are walked two at a time by index (each edge with its twin).
//!
//! Errors: a construction's [`VoronoiError`] passes through, as the C++'s exception does. A null
//! pointer the C++ follows, and a site index past the lines (the C++ reads outside the vector),
//! panic.
//!
//! Left out: the `SLIC3R_DEBUG` dump (MedialAxis.cpp 15-395), the templates no code instantiates
//! (`retrieve_cell_point`, `measure_edge_thickness`, `Lines2VDSegments`) and the commented-out
//! code.

use orca_geom::clipper_utils::{DEFAULT_JOIN_TYPE, DEFAULT_MITER_LIMIT, closing_ex_expolygons};
use orca_geom::expolygon::{ExPolygon, to_lines_ex};
use orca_geom::line::{Line, Lines};
use orca_geom::point::{PI, Point, SCALED_EPSILON};
use orca_geom::polyline::{Polyline, Polylines, ThickPolyline, ThickPolylines};
use orca_mesh::log::{Severity, log};

use crate::diagram::VoronoiCell;
use crate::geometry_type::SourceCategory;
use crate::geometry_voronoi::VoronoiDiagram;
use crate::voronoi_offset::{VertexCategory, annotate_inside_outside, vertex_category};
use crate::voronoi_utils::VoronoiError;

/// The error after the closing.
const INVALID_AFTER_CLOSING: &str =
    "MedialAxis - Invalid Voronoi diagram even after morphological closing.";

/// `MedialAxis::EdgeData`: whether a skeleton edge is kept, and its widths at its two ends (of
/// the even half-edge of the pair).
#[derive(Clone, Copy, Debug, Default)]
struct EdgeData {
    active: bool,
    width_start: f64,
    width_end: f64,
}

/// `class Slic3r::Geometry::MedialAxis`.
#[derive(Debug)]
pub struct MedialAxis<'a> {
    /// `m_expolygon`.
    expolygon: &'a ExPolygon,
    /// `m_lines`.
    lines: Lines,
    /// `m_min_width`.
    min_width: f64,
    /// `m_max_width`.
    max_width: f64,
    /// `m_vd`.
    vd: VoronoiDiagram,
    /// `m_edge_data`: one per pair of half-edges.
    edge_data: Vec<EdgeData>,
}

/// The link the C++ follows.
fn follow(i: Option<usize>, what: &str) -> usize {
    i.unwrap_or_else(|| panic!("MedialAxis: the C++ follows a null {what}"))
}

impl<'a> MedialAxis<'a> {
    /// `MedialAxis(min_width, max_width, expolygon)`: the lines of the region.
    #[must_use]
    pub fn new(min_width: f64, max_width: f64, expolygon: &'a ExPolygon) -> Self {
        Self {
            expolygon,
            lines: expolygon.lines(),
            min_width,
            max_width,
            vd: VoronoiDiagram::new(),
            edge_data: Vec::new(),
        }
    }

    /// `build(ThickPolylines*)`: appends the skeleton's polylines (the module's header says how).
    ///
    /// # Errors
    ///
    /// A construction's error (the second construction's, on a stale diagram).
    pub fn build(&mut self, polylines: &mut ThickPolylines) -> Result<(), VoronoiError> {
        self.vd.construct_voronoi_segments(&self.lines, true)?;

        // For several ExPolygons in SPE-1729, an invalid Voronoi diagram was produced that wasn't
        // fixable by rotating input data. Those ExPolygons contain very thin lines and holes
        // formed by very close (1-5nm) vertices that are on the edge of our resolution. Those thin
        // lines and holes are both unprintable and cause the Voronoi diagram to be invalid. So we
        // filter out such thin lines and holes and try to compute the Voronoi diagram again.
        if !self.vd.is_valid() {
            self.lines = to_lines_ex(&closing_ex_expolygons(
                core::slice::from_ref(self.expolygon),
                (2.0 * SCALED_EPSILON) as f32,
                DEFAULT_JOIN_TYPE,
                DEFAULT_MITER_LIMIT,
            ));
            self.vd.construct_voronoi_segments(&self.lines, true)?;

            if !self.vd.is_valid() {
                log(Severity::Error, INVALID_AFTER_CLOSING);
            }
        }

        annotate_inside_outside(&self.vd, &self.lines);

        // collect valid edges (i.e. prune those not belonging to MAT)
        // note: this keeps twins, so it inserts twice the number of the valid edges
        let num_edges = self.vd.edges().len();
        self.edge_data = vec![EdgeData::default(); num_edges / 2];
        for edge in (0..num_edges).step_by(2) {
            let edges = self.vd.edges();
            let vertices = self.vd.vertices();
            let e = &edges[edge];
            if e.is_primary()
                && e.is_finite(edges)
                && (vertex_category(&vertices[follow(e.vertex0(), "vertex0")])
                    == VertexCategory::INSIDE
                    || vertex_category(&vertices[follow(e.vertex1(edges), "vertex1")])
                        == VertexCategory::INSIDE)
                && self.validate_edge(edge)
            {
                // Valid skeleton edge.
                self.edge_data[edge / 2].active = true;
            }
        }

        // iterate through the valid edges to build polylines
        let mut reverse_polyline = ThickPolyline::default();
        for seed_edge in (0..num_edges).step_by(2) {
            if self.edge_data[seed_edge / 2].active {
                // Mark this edge as visited.
                self.edge_data[seed_edge / 2].active = false;
                let seed_edge_data = self.edge_data[seed_edge / 2];

                // Start a polyline.
                let mut polyline = ThickPolyline::default();
                {
                    let edges = self.vd.edges();
                    let vertices = self.vd.vertices();
                    let e = &edges[seed_edge];
                    let v0 = &vertices[follow(e.vertex0(), "vertex0")];
                    let v1 = &vertices[follow(e.vertex1(edges), "vertex1")];
                    polyline.points.push(Point::from_f64(v0.x(), v0.y()));
                    polyline.points.push(Point::from_f64(v1.x(), v1.y()));
                }
                polyline.width.push(seed_edge_data.width_start);
                polyline.width.push(seed_edge_data.width_end);
                // Grow the polyline in a forward direction.
                self.process_edge_neighbors(seed_edge, &mut polyline);

                // Grow the polyline in a backward direction.
                reverse_polyline.clear();
                let twin = follow(self.vd.edges()[seed_edge].twin(), "twin");
                self.process_edge_neighbors(twin, &mut reverse_polyline);
                polyline
                    .points
                    .splice(0..0, reverse_polyline.points.iter().rev().copied());
                polyline
                    .width
                    .splice(0..0, reverse_polyline.width.iter().rev().copied());
                polyline.endpoints.0 = reverse_polyline.endpoints.1;

                // Prevent loop endpoints from being extended.
                if polyline.first_point() == polyline.last_point() {
                    polyline.endpoints.0 = false;
                    polyline.endpoints.1 = false;
                }

                // Append polyline to result.
                polylines.push(polyline);
            }
        }
        Ok(())
    }

    /// `build(Polylines*)`: the skeleton's polylines without their widths.
    ///
    /// # Errors
    ///
    /// As [`MedialAxis::build`].
    pub fn build_polylines(&mut self, polylines: &mut Polylines) -> Result<(), VoronoiError> {
        let mut tp = ThickPolylines::new();
        self.build(&mut tp)?;
        polylines.reserve(tp.len());
        for pl in &tp {
            polylines.push(Polyline::new(pl.points.clone()));
        }
        Ok(())
    }

    /// `process_edge_neighbors(edge, polyline)`: extends the polyline while the edge's end has
    /// exactly one kept neighbour; marks the end free when it has none.
    fn process_edge_neighbors(&mut self, edge: usize, polyline: &mut ThickPolyline) {
        let mut edge = edge;
        loop {
            let edges = self.vd.edges();
            let vertices = self.vd.vertices();
            // Since rot_next() works on the edge starting point but we want
            // to find neighbors on the ending point, we just swap edge with
            // its twin.
            let twin = follow(edges[edge].twin(), "twin");

            // count neighbors for this edge
            let mut num_neighbors = 0usize;
            let mut first_neighbor: Option<usize> = None;
            let mut neighbor = follow(edges[twin].rot_next(edges), "rot_next");
            while neighbor != twin {
                if self.edge_data[neighbor / 2].active {
                    if num_neighbors == 0 {
                        first_neighbor = Some(neighbor);
                    }
                    num_neighbors += 1;
                }
                neighbor = follow(edges[neighbor].rot_next(edges), "rot_next");
            }

            // if we have a single neighbor then we can continue recursively
            if num_neighbors == 1 {
                let first_neighbor = follow(first_neighbor, "neighbor");
                let reversed = (first_neighbor & 1) != 0;
                let neighbor_data = &mut self.edge_data[first_neighbor / 2];
                if neighbor_data.active {
                    neighbor_data.active = false;
                    let v1 = &vertices[follow(edges[first_neighbor].vertex1(edges), "vertex1")];
                    polyline.points.push(Point::from_f64(v1.x(), v1.y()));
                    if reversed {
                        polyline.width.push(neighbor_data.width_end);
                        polyline.width.push(neighbor_data.width_start);
                    } else {
                        polyline.width.push(neighbor_data.width_start);
                        polyline.width.push(neighbor_data.width_end);
                    }
                    edge = first_neighbor;
                    // Continue chaining.
                    continue;
                }
            } else if num_neighbors == 0 {
                polyline.endpoints.1 = true;
            } else {
                // T-shaped or star-shaped joint
            }
            // Stop chaining.
            break;
        }
    }

    /// `validate_edge(edge)`: whether the edge belongs to the skeleton, its widths stored when
    /// it does (the module's header says which).
    fn validate_edge(&mut self, edge: usize) -> bool {
        let edges = self.vd.edges();
        let vertices = self.vd.vertices();
        let cells = self.vd.cells();
        let lines = &self.lines;
        let retrieve_segment = |cell: &VoronoiCell| -> Line { lines[cell.source_index()] };
        let retrieve_endpoint = |cell: &VoronoiCell| -> Point {
            let line = retrieve_segment(cell);
            if cell.source_category() == SourceCategory::SEGMENT_START_POINT {
                line.a
            } else {
                line.b
            }
        };

        // construct the line representing this edge of the Voronoi diagram
        let e = &edges[edge];
        let v0 = &vertices[follow(e.vertex0(), "vertex0")];
        let v1 = &vertices[follow(e.vertex1(edges), "vertex1")];
        let line = Line::new(
            Point::from_f64(v0.x(), v0.y()),
            Point::from_f64(v1.x(), v1.y()),
        );

        // retrieve the original line segments which generated the edge we're checking
        let cell_l = &cells[follow(e.cell(), "cell")];
        let cell_r = &cells[follow(edges[follow(e.twin(), "twin")].cell(), "cell")];
        let segment_l = retrieve_segment(cell_l);
        let segment_r = retrieve_segment(cell_r);

        // Calculate thickness of the cross-section at both the endpoints of this edge.
        // Our Voronoi edge is part of a CCW sequence going around its Voronoi cell
        // located on the left side. (segment_l).
        // This edge's twin goes around segment_r. Thus, segment_r is
        // oriented in the same direction as our main edge, and segment_l is oriented
        // in the same direction as our twin edge.
        // We used to only consider the (half-)distances to segment_r, and that works
        // whenever segment_l and segment_r are almost specular and facing. However,
        // at curves they are staggered and they only face for a very little length
        // (our very short edge represents such visibility).
        // Both w0 and w1 can be calculated either towards cell_l or cell_r with equal
        // results by Voronoi definition.
        // When cell_l or cell_r don't refer to the segment but only to an endpoint, we
        // calculate the distance to that endpoint instead.

        let mut w0 = if cell_r.contains_segment() {
            segment_r.distance_to(line.a) * 2.0
        } else {
            (retrieve_endpoint(cell_r) - line.a).cast_f64().norm() * 2.0
        };

        let mut w1 = if cell_l.contains_segment() {
            segment_l.distance_to(line.b) * 2.0
        } else {
            (retrieve_endpoint(cell_l) - line.b).cast_f64().norm() * 2.0
        };

        if cell_l.contains_segment() && cell_r.contains_segment() {
            // calculate the relative angle between the two boundary segments
            let mut angle = orca_crt::fabs(segment_r.orientation() - segment_l.orientation());
            if angle > PI {
                angle = 2.0 * PI - angle;
            }

            // fabs(angle) ranges from 0 (collinear, same direction) to PI (collinear, opposite
            // direction) we're interested only in segments close to the second case (facing
            // segments) so we allow some tolerance.
            // this filter ensures that we're dealing with a narrow/oriented area (longer than
            // thick) we don't run it on edges not generated by two segments (thus generated by
            // one segment and the endpoint of another segment), since their orientation would not
            // be meaningful
            if PI - angle > PI / 8.0 {
                // angle is not narrow enough
                // only apply this filter to segments that are not too short otherwise their
                // angle could possibly be not meaningful
                if w0 < SCALED_EPSILON || w1 < SCALED_EPSILON || line.length() >= self.min_width {
                    return false;
                }
            }
        } else if w0 < SCALED_EPSILON || w1 < SCALED_EPSILON {
            return false;
        }

        if (w0 >= self.min_width || w1 >= self.min_width)
            && (w0 <= self.max_width || w1 <= self.max_width)
        {
            let reversed = (edge & 1) != 0;
            if reversed {
                core::mem::swap(&mut w0, &mut w1);
            }
            let ed = &mut self.edge_data[edge / 2];
            ed.width_start = w0;
            ed.width_end = w1;
            return true;
        }

        false
    }
}

/// C's `fmaxf(x, y)`: the larger, the other one when one is a NaN. The two zeros never meet here
/// (the widths are distances, never -0), so which one it returns for them does not matter. It
/// belongs in `orca-crt`, which has no `fmaxf` yet.
fn fmaxf(x: f32, y: f32) -> f32 {
    if x.is_nan() {
        y
    } else if y.is_nan() || y < x {
        x
    } else {
        y
    }
}

/// `ExPolygon::medial_axis`, both overloads (`ExPolygon.cpp` 261-378): `ExPolygon` is
/// `orca-geom`'s, so they are a trait here.
pub trait ExPolygonMedialAxis {
    /// `medial_axis(min_width, max_width, ThickPolylines*)`: the build's polylines with their
    /// free ends extended to the contour, the short ones dropped and the rest joined (the
    /// module's header says how), appended.
    ///
    /// # Errors
    ///
    /// As [`MedialAxis::build`].
    fn medial_axis(
        &self,
        min_width: f64,
        max_width: f64,
        polylines: &mut ThickPolylines,
    ) -> Result<(), VoronoiError>;

    /// `medial_axis(min_width, max_width, Polylines*)`: the same without the widths.
    ///
    /// # Errors
    ///
    /// As [`MedialAxis::build`].
    fn medial_axis_polylines(
        &self,
        min_width: f64,
        max_width: f64,
        polylines: &mut Polylines,
    ) -> Result<(), VoronoiError>;
}

impl ExPolygonMedialAxis for ExPolygon {
    fn medial_axis(
        &self,
        min_width: f64,
        max_width: f64,
        polylines: &mut ThickPolylines,
    ) -> Result<(), VoronoiError> {
        // init helper object
        let mut ma = MedialAxis::new(min_width, max_width, self);

        // compute the Voronoi diagram and extract medial axis polylines
        let mut pp = ThickPolylines::new();
        ma.build(&mut pp)?;

        // Find the maximum width returned; we're going to use this for validating and
        // filtering the output segments.
        let mut max_w = 0.0_f64;
        for it in &pp {
            let largest = it.width[orca_stl::max_element_f64(&it.width)];
            max_w = f64::from(fmaxf(max_w as f32, largest as f32));
        }

        // Loop through all returned polylines in order to extend their endpoints to the
        // expolygon boundaries
        let mut removed = false;
        let mut i = 0usize;
        while i < pp.len() {
            let polyline = &mut pp[i];

            // extend initial and final segments of each polyline if they're actual endpoints
            // We assign new endpoints to temporary variables because in case of a single-line
            // polyline, after we extend the start point it will be caught by the intersection()
            // call, so we keep the inner point until we perform the second intersection() as
            // well
            let mut new_front = polyline.points[0];
            let mut new_back = polyline.points[polyline.points.len() - 1];
            if polyline.endpoints.0 && !self.on_boundary(new_front, SCALED_EPSILON) {
                let mut p1 = polyline.points[0].cast_f64();
                let mut p2 = polyline.points[1].cast_f64();
                // prevent the line from touching on the other side, otherwise intersection()
                // might return that solution
                if polyline.points.len() == 2 {
                    p2 = (p1 + p2) * 0.5;
                }
                // Extend the start of the segment.
                p1 -= (p2 - p1).normalized() * max_width;
                if let Some(ip) = self.contour.intersection(&Line::new(
                    Point::cast_from_vec2d(p1),
                    Point::cast_from_vec2d(p2),
                )) {
                    new_front = ip;
                }
            }
            if polyline.endpoints.1 && !self.on_boundary(new_back, SCALED_EPSILON) {
                let n = polyline.points.len();
                let mut p1 = polyline.points[n - 2].cast_f64();
                let mut p2 = polyline.points[n - 1].cast_f64();
                // prevent the line from touching on the other side, otherwise intersection()
                // might return that solution
                if polyline.points.len() == 2 {
                    p1 = (p1 + p2) * 0.5;
                }
                // Extend the start of the segment.
                p2 += (p2 - p1).normalized() * max_width;
                if let Some(ip) = self.contour.intersection(&Line::new(
                    Point::cast_from_vec2d(p1),
                    Point::cast_from_vec2d(p2),
                )) {
                    new_back = ip;
                }
            }
            polyline.points[0] = new_front;
            let n = polyline.points.len();
            polyline.points[n - 1] = new_back;

            // remove too short polylines
            // (we can't do this check before endpoints extension and clipping because we don't
            // know how long will the endpoints be extended since it depends on polygon thickness
            // which is variable - extension will be <= max_width/2 on each side)
            if (polyline.endpoints.0 || polyline.endpoints.1) && polyline.length() < max_w * 2.0 {
                pp.remove(i);
                removed = true;
                continue;
            }
            i += 1;
        }

        // If we removed any short polylines we now try to connect consecutive polylines
        // in order to allow loop detection. Note that this algorithm is greedier than
        // MedialAxis::process_edge_neighbors() as it will connect random pairs of
        // polylines even when more than two start from the same point. This has no
        // drawbacks since we optimize later using nearest-neighbor which would do the
        // same, but should we use a more sophisticated optimization algorithm we should
        // not connect polylines when more than two meet.
        if removed {
            let mut i = 0usize;
            while i < pp.len() {
                if pp[i].endpoints.0 && pp[i].endpoints.1 {
                    i += 1;
                    continue; // optimization
                }

                // find another polyline starting here
                let mut j = i + 1;
                while j < pp.len() {
                    let (head, tail) = pp.split_at_mut(j);
                    let polyline = &mut head[i];
                    let other = &mut tail[0];
                    if polyline.last_point() == other.last_point() {
                        other.reverse();
                    } else if polyline.first_point() == other.last_point() {
                        polyline.reverse();
                        other.reverse();
                    } else if polyline.first_point() == other.first_point() {
                        polyline.reverse();
                    } else if polyline.last_point() != other.first_point() {
                        j += 1;
                        continue;
                    }

                    polyline.points.extend_from_slice(&other.points[1..]);
                    polyline.width.extend_from_slice(&other.width);
                    polyline.endpoints.1 = other.endpoints.1;

                    pp.remove(j);
                    j = i; // restart search from i+1
                    j += 1;
                }
                i += 1;
            }
        }

        polylines.extend(pp);
        Ok(())
    }

    fn medial_axis_polylines(
        &self,
        min_width: f64,
        max_width: f64,
        polylines: &mut Polylines,
    ) -> Result<(), VoronoiError> {
        let mut tp = ThickPolylines::new();
        self.medial_axis(min_width, max_width, &mut tp)?;
        polylines.reserve(tp.len());
        for pl in &tp {
            polylines.push(Polyline::new(pl.points.clone()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use orca_geom::expolygon::ExPolygon;
    use orca_geom::point::Point;
    use orca_geom::polyline::{Polylines, ThickPolylines};

    use super::{ExPolygonMedialAxis, MedialAxis, fmaxf};

    /// A thin rectangle 10 mm long and 0.4 mm wide, in scaled units.
    fn strip() -> ExPolygon {
        ExPolygon::from_points(vec![
            Point::new(0, 0),
            Point::new(10_000_000, 0),
            Point::new(10_000_000, 400_000),
            Point::new(0, 400_000),
        ])
    }

    #[test]
    fn a_strip_has_one_axis_along_its_middle() {
        let ex = strip();
        let mut tp = ThickPolylines::new();
        MedialAxis::new(100_000.0, 800_000.0, &ex)
            .build(&mut tp)
            .unwrap();
        assert!(!tp.is_empty());
        for t in &tp {
            assert_eq!(t.width.len(), t.points.len() * 2 - 2);
        }
        let mut medial = ThickPolylines::new();
        ex.medial_axis(100_000.0, 800_000.0, &mut medial).unwrap();
        assert_eq!(medial.len(), 1);
        // The axis runs along y = 200000, extended to the short sides.
        let axis = &medial[0];
        assert!(axis.points.iter().all(|p| p.y == 200_000));
        assert_eq!(axis.first_point().x.min(axis.last_point().x), 0);
        let mut lines = Polylines::new();
        ex.medial_axis_polylines(100_000.0, 800_000.0, &mut lines)
            .unwrap();
        assert_eq!(lines[0].points, axis.points);
    }

    #[test]
    fn fmaxf_skips_nan() {
        assert_eq!(fmaxf(f32::NAN, 1.0), 1.0);
        assert_eq!(fmaxf(2.0, f32::NAN), 2.0);
        assert_eq!(fmaxf(2.0, 3.0), 3.0);
    }
}
