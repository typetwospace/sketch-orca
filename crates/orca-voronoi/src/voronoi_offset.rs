// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Geometry/VoronoiOffset.hpp lines 22-103 (contour_point,
// vertex_point and the vertex, edge and cell categories kept in the colours) and
// src/libslic3r/Geometry/VoronoiOffset.cpp lines 208-252 (detail::vertex_equal_to_point and
// detail::on_site) and 646-971 (reset_inside_outside_annotations and annotate_inside_outside) of
// OrcaSlicer v2.4.2, translated on 2026-11-14 by orca-o3-voronoi-4. Copyright the OrcaSlicer
// authors and the PrusaSlicer authors (Prusa Research), under the GNU Affero General Public
// License, version 3 (AGPL-3.0; LICENSE). No piece carries VoronoiOffset; these parts are here
// because MedialAxis calls them, and they belong in this crate.

//! `Slic3r::Voronoi::annotate_inside_outside` (`Geometry/VoronoiOffset.cpp`), which MedialAxis
//! runs on its diagram: each vertex, edge and cell gets a colour saying whether it is on the input
//! contour, inside or outside the polygons the lines bound.
//!
//! - The categories are the colours as `unsigned char`s ([`VertexCategory`], [`EdgeCategory`],
//!   [`CellCategory`]): 0 on the contour (pointing to it, a boundary cell), 1 inside, 2 outside,
//!   3 unknown. A colour above 255 is read through its low byte, as the C++'s cast does.
//! - [`annotate_inside_outside`] marks everything unknown, then the vertices on the contour (a
//!   vertex within 128 ulps of a site's point, and one end of each secondary edge), then the
//!   infinite edges (outside) and each finite edge with a segment site by the side of the segment
//!   its end is on (a `double` cross product), then the edges between two point sites by one
//!   round of expansion from the cells and a seed fill over the cells.
//!
//! The diagram is read through its accessors (the copies after a repair). The C++ takes `VD&` and
//! changes only colours, which are interior-mutable here, so the function takes a shared
//! reference. Its asserts (debug build only) are left out; a null pointer it follows panics.
//!
//! Left out: the rest of VoronoiOffset.cpp (offsets, skeleton edges, signed distances), which no
//! piece carries and no domain-A caller of this crate reaches.

use orca_eigen::Vec2d;
use orca_geom::line::{Line, Lines};
use orca_geom::point::{Point, cross2, vec2d};

use crate::ctypes::{UlpComparison, UlpResult};
use crate::diagram::{VertexEqualityPredicate, VoronoiCell, VoronoiEdge, VoronoiVertex};
use crate::geometry_type::SourceCategory;
use crate::geometry_voronoi::VoronoiDiagram;

/// `enum class VertexCategory : unsigned char`, kept in a vertex's colour.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VertexCategory(pub u8);

impl VertexCategory {
    /// `OnContour`: the vertex is on the input contour.
    pub const ON_CONTOUR: Self = Self(0);
    /// `Inside`: inside the CCW input contour, holes respected.
    pub const INSIDE: Self = Self(1);
    /// `Outside`: outside the CCW input contour, holes respected.
    pub const OUTSIDE: Self = Self(2);
    /// `Unknown`: not known yet.
    pub const UNKNOWN: Self = Self(3);
}

/// `enum class EdgeCategory : unsigned char`, kept in a half-edge's colour.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EdgeCategory(pub u8);

impl EdgeCategory {
    /// `PointsToContour`: the half-edge's end is on the contour.
    pub const POINTS_TO_CONTOUR: Self = Self(0);
    /// `PointsInside`: its end is inside.
    pub const POINTS_INSIDE: Self = Self(1);
    /// `PointsOutside`: its end is outside.
    pub const POINTS_OUTSIDE: Self = Self(2);
    /// `Unknown`: not known yet.
    pub const UNKNOWN: Self = Self(3);
}

/// `enum class CellCategory : unsigned char`, kept in a cell's colour.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CellCategory(pub u8);

impl CellCategory {
    /// `Boundary`: an input segment splits the cell into an inside and an outside half.
    pub const BOUNDARY: Self = Self(0);
    /// `Inside`: the cell is completely inside.
    pub const INSIDE: Self = Self(1);
    /// `Outside`: the cell is completely outside.
    pub const OUTSIDE: Self = Self(2);
    /// `Unknown`: not known yet.
    pub const UNKNOWN: Self = Self(3);
}

/// `contour_point(cell, line)`: the line's `a` for a cell of a segment's start point, else `b`.
#[must_use]
pub fn contour_point(cell: &VoronoiCell, line: &Line) -> Point {
    if cell.source_category() == SourceCategory::SEGMENT_START_POINT {
        line.a
    } else {
        line.b
    }
}

/// `contour_point(cell, lines)`: of the cell's line. Panics past the lines (the C++ reads outside
/// the vector).
#[must_use]
pub fn contour_point_of(cell: &VoronoiCell, lines: &Lines) -> Point {
    contour_point(cell, &lines[cell.source_index()])
}

/// `vertex_point(v)`.
#[must_use]
pub fn vertex_point(v: &VoronoiVertex) -> Vec2d {
    vec2d(v.x(), v.y())
}

/// `vertex_category(v)`: the colour's low byte.
#[must_use]
pub fn vertex_category(v: &VoronoiVertex) -> VertexCategory {
    VertexCategory(v.color() as u8)
}

/// `set_vertex_category(v, c)`.
pub fn set_vertex_category(v: &VoronoiVertex, c: VertexCategory) {
    v.set_color(usize::from(c.0));
}

/// `edge_category(e)`: the colour's low byte.
#[must_use]
pub fn edge_category(e: &VoronoiEdge) -> EdgeCategory {
    EdgeCategory(e.color() as u8)
}

/// `set_edge_category(e, c)`.
pub fn set_edge_category(e: &VoronoiEdge, c: EdgeCategory) {
    e.set_color(usize::from(c.0));
}

/// `cell_category(c)`: the colour's low byte.
#[must_use]
pub fn cell_category(c: &VoronoiCell) -> CellCategory {
    CellCategory(c.color() as u8)
}

/// `set_cell_category(c, category)`.
pub fn set_cell_category(c: &VoronoiCell, category: CellCategory) {
    c.set_color(usize::from(category.0));
}

/// `detail::vertex_equal_to_point(vertex, ipt)`: both coordinates within 128 ulps of the point's
/// as `double`s (`ulp_comparison`).
#[must_use]
pub fn vertex_equal_to_point(x: f64, y: f64, ipt: Point) -> bool {
    let ulp_cmp = UlpComparison;
    let ulps = VertexEqualityPredicate::ULPS;
    ulp_cmp.call(x, ipt.x as f64, ulps) == UlpResult::Equal
        && ulp_cmp.call(y, ipt.y as f64, ulps) == UlpResult::Equal
}

/// `detail::on_site(lines, cell, pt)`: whether `pt` is the cell's point, or an end of its segment.
/// Panics past the lines (the C++ reads outside the vector).
#[must_use]
pub fn on_site(lines: &Lines, cell: &VoronoiCell, pt: Vec2d) -> bool {
    let line = &lines[cell.source_index()];
    let on_contour = |ipt: Point| vertex_equal_to_point(pt.x(), pt.y(), ipt);
    if cell.contains_point() {
        on_contour(contour_point(cell, line))
    } else {
        on_contour(line.a) || on_contour(line.b)
    }
}

/// `reset_inside_outside_annotations(vd)`: every vertex, edge and cell unknown.
pub fn reset_inside_outside_annotations(vd: &VoronoiDiagram) {
    for v in vd.vertices() {
        set_vertex_category(v, VertexCategory::UNKNOWN);
    }
    for e in vd.edges() {
        set_edge_category(e, EdgeCategory::UNKNOWN);
    }
    for c in vd.cells() {
        set_cell_category(c, CellCategory::UNKNOWN);
    }
}

/// `annotate_vertex`: sets the category (the C++ checks it in its debug build only).
fn annotate_vertex(vertex: &VoronoiVertex, new_vertex_category: VertexCategory) {
    set_vertex_category(vertex, new_vertex_category);
}

/// `annotate_edge`: sets the category (the C++ checks it in its debug build only).
fn annotate_edge(edge: &VoronoiEdge, new_edge_category: EdgeCategory) {
    set_edge_category(edge, new_edge_category);
}

/// `annotate_cell`: sets the category, a cell marked inside and outside becoming a boundary one
/// and a boundary one staying so; true when the category changed.
fn annotate_cell(cell: &VoronoiCell, new_cell_category: CellCategory) -> bool {
    let mut new_cell_category = new_cell_category;
    let cc = cell_category(cell);
    if cc == CellCategory::UNKNOWN {
        // Old category unknown, just write the new category.
    } else if cc == CellCategory::OUTSIDE {
        if new_cell_category == CellCategory::INSIDE {
            new_cell_category = CellCategory::BOUNDARY;
        }
    } else if cc == CellCategory::INSIDE {
        if new_cell_category == CellCategory::OUTSIDE {
            new_cell_category = CellCategory::BOUNDARY;
        }
    } else if cc == CellCategory::BOUNDARY {
        return false;
    }
    if cc != new_cell_category {
        set_cell_category(cell, new_cell_category);
        return true;
    }
    false
}

/// A link the C++ follows.
fn follow(i: Option<usize>, what: &str) -> usize {
    i.unwrap_or_else(|| panic!("annotate_inside_outside: the C++ follows a null {what}"))
}

/// `annotate_inside_outside(vd, lines)`: the categories of every vertex, edge and cell (the
/// module's header says how).
pub fn annotate_inside_outside(vd: &VoronoiDiagram, lines: &Lines) {
    reset_inside_outside_annotations(vd);

    let edges = vd.edges();
    let vertices = vd.vertices();
    let cells = vd.cells();
    let cell_of = |e: &VoronoiEdge| -> usize { follow(e.cell(), "cell") };
    let twin_of = |e: &VoronoiEdge| -> usize { follow(e.twin(), "twin") };

    // The next loop is supposed to annotate the "on input contour" vertices, but due to
    // merging very close Voronoi vertices together by boost::polygon Voronoi generator
    // the condition may not always be met. It should be safe to mark all Voronoi very close
    // to the input contour as on contour.
    for edge in edges {
        if let Some(v) = edge.vertex0() {
            let v = &vertices[v];
            let on_contour = on_site(lines, &cells[cell_of(edge)], vertex_point(v));
            if on_contour {
                annotate_vertex(v, VertexCategory::ON_CONTOUR);
            }
        }
    }

    // One side of a secondary edge is always on the source contour. Mark these vertices as
    // OnContour. See the comment at the loop before, the condition may not always be met.
    for edge in edges {
        if edge.is_secondary() && edge.vertex0().is_some() {
            let cell = &cells[cell_of(edge)];
            // The point associated with the point site shall be equal with one vertex of this
            // Voronoi edge.
            let pt_on_contour = if cell.contains_point() {
                contour_point_of(cell, lines)
            } else {
                contour_point_of(&cells[cell_of(&edges[twin_of(edge)])], lines)
            };
            let on_contour = |v: &VoronoiVertex| vertex_equal_to_point(v.x(), v.y(), pt_on_contour);
            let v0 = &vertices[follow(edge.vertex0(), "vertex0")];
            match edge.vertex1(edges) {
                None => annotate_vertex(v0, VertexCategory::ON_CONTOUR),
                Some(v1) => {
                    // Only one of the two vertices may lie on input contour.
                    if on_contour(v0) {
                        annotate_vertex(v0, VertexCategory::ON_CONTOUR);
                    } else {
                        annotate_vertex(&vertices[v1], VertexCategory::ON_CONTOUR);
                    }
                }
            }
        }
    }

    for edge in edges {
        if edge.vertex1(edges).is_none() {
            // Infinite Voronoi edge separating two Point sites or a Point site and a Segment site.
            // Infinite edge is always outside and it references at least one valid vertex.
            let twin = &edges[twin_of(edge)];
            let mut cell = cell_of(edge);
            let mut cell2 = cell_of(twin);
            annotate_edge(edge, EdgeCategory::POINTS_OUTSIDE);
            // Opposite edge of an infinite edge is certainly not active.
            annotate_edge(
                twin,
                if edge.is_secondary() {
                    EdgeCategory::POINTS_TO_CONTOUR
                } else {
                    EdgeCategory::POINTS_OUTSIDE
                },
            );
            annotate_vertex(
                &vertices[follow(edge.vertex0(), "vertex0")],
                if edge.is_secondary() {
                    VertexCategory::ON_CONTOUR
                } else {
                    VertexCategory::OUTSIDE
                },
            );
            // edge.vertex1() is null, it is implicitely outside.
            if cells[cell].contains_segment() {
                core::mem::swap(&mut cell, &mut cell2);
            }
            // State of a cell containing a boundary point is certainly outside.
            annotate_cell(&cells[cell], CellCategory::OUTSIDE);
            annotate_cell(
                &cells[cell2],
                if cells[cell2].contains_point() {
                    CellCategory::OUTSIDE
                } else {
                    CellCategory::BOUNDARY
                },
            );
        } else if edge.vertex0().is_some() {
            let twin = &edges[twin_of(edge)];
            let mut cell = cell_of(edge);
            let mut line: Option<&Line> = if cells[cell].contains_segment() {
                Some(&lines[cells[cell].source_index()])
            } else {
                None
            };
            if line.is_none() {
                cell = cell_of(twin);
                line = if cells[cell].contains_segment() {
                    Some(&lines[cells[cell].source_index()])
                } else {
                    None
                };
            }
            // Now classify the Voronoi vertices and edges as inside outside, if at least one
            // Voronoi site is a Segment site.
            // Inside / outside classification of Point - Point Voronoi edges will be done later
            // by a propagation (seed fill).
            if let Some(line) = line {
                let v0 = &vertices[follow(edge.vertex0(), "vertex0")];
                let v1 = &vertices[follow(edge.vertex1(edges), "vertex1")];
                let cell2 = if cell == cell_of(edge) {
                    cell_of(twin)
                } else {
                    cell_of(edge)
                };
                let v0_category = vertex_category(v0);
                let v1_category = vertex_category(v1);
                let on_contour = v0_category == VertexCategory::ON_CONTOUR
                    || v1_category == VertexCategory::ON_CONTOUR;
                if on_contour && v1_category == VertexCategory::ON_CONTOUR {
                    // Skip secondary edge pointing to a contour point.
                    annotate_edge(edge, EdgeCategory::POINTS_TO_CONTOUR);
                } else {
                    // v0 is certainly not on the input polygons.
                    // Is v1 inside or outside the input polygons?
                    // The Voronoi vertex coordinate is in doubles, calculate orientation in
                    // doubles.
                    let l0 = line.a.cast_f64();
                    let lv = (line.b - line.a).cast_f64();
                    let side = cross2(vec2d(v1.x(), v1.y()) - l0, lv);
                    // No Voronoi edge could connect two vertices of input polygons.
                    let vc = if side > 0.0 {
                        VertexCategory::OUTSIDE
                    } else {
                        VertexCategory::INSIDE
                    };
                    annotate_vertex(v1, vc);
                    let ec = if vc == VertexCategory::OUTSIDE {
                        EdgeCategory::POINTS_OUTSIDE
                    } else {
                        EdgeCategory::POINTS_INSIDE
                    };
                    annotate_edge(edge, ec);
                    // Annotate the twin edge and its vertex. As the Voronoi edge may never cross
                    // the input contour, the twin edge and its vertex will share the property of
                    // edge.
                    annotate_vertex(
                        v0,
                        if on_contour {
                            VertexCategory::ON_CONTOUR
                        } else {
                            vc
                        },
                    );
                    annotate_edge(
                        twin,
                        if on_contour {
                            EdgeCategory::POINTS_TO_CONTOUR
                        } else {
                            ec
                        },
                    );
                    annotate_cell(
                        &cells[cell],
                        if on_contour {
                            CellCategory::BOUNDARY
                        } else if vc == VertexCategory::OUTSIDE {
                            CellCategory::OUTSIDE
                        } else {
                            CellCategory::INSIDE
                        },
                    );
                    annotate_cell(
                        &cells[cell2],
                        if on_contour && cells[cell2].contains_segment() {
                            CellCategory::BOUNDARY
                        } else if vc == VertexCategory::OUTSIDE {
                            CellCategory::OUTSIDE
                        } else {
                            CellCategory::INSIDE
                        },
                    );
                }
            }
        }
    }

    // Now most Voronoi vertices, edges and cells are annotated, with the exception of some
    // edges separating two Point sites, their cells and vertices.
    // Perform one round of expansion marking Voronoi edges and cells next to boundary cells.
    let mut cell_queue: Vec<usize> = Vec::new();
    for edge in edges {
        if edge_category(edge) == EdgeCategory::UNKNOWN {
            let twin = &edges[twin_of(edge)];
            let cell = cell_of(edge);
            let cell2 = cell_of(twin);
            let cc = cell_category(&cells[cell]);
            let cc2 = cell_category(&cells[cell2]);
            let mut cc_new = cc;
            if cc_new == CellCategory::UNKNOWN {
                cc_new = cc2;
            }
            if cc_new == CellCategory::UNKNOWN {
                let vc = vertex_category(&vertices[follow(edge.vertex0(), "vertex0")]);
                if vc != VertexCategory::UNKNOWN {
                    cc_new = if vc == VertexCategory::OUTSIDE {
                        CellCategory::OUTSIDE
                    } else {
                        CellCategory::INSIDE
                    };
                }
            }
            if cc_new != CellCategory::UNKNOWN {
                let vc = if cc_new == CellCategory::OUTSIDE {
                    VertexCategory::OUTSIDE
                } else {
                    VertexCategory::INSIDE
                };
                annotate_vertex(&vertices[follow(edge.vertex0(), "vertex0")], vc);
                annotate_vertex(&vertices[follow(edge.vertex1(edges), "vertex1")], vc);
                let ec_new = if cc_new == CellCategory::OUTSIDE {
                    EdgeCategory::POINTS_OUTSIDE
                } else {
                    EdgeCategory::POINTS_INSIDE
                };
                annotate_edge(edge, ec_new);
                annotate_edge(twin, ec_new);
                if cc != cc_new {
                    annotate_cell(&cells[cell], cc_new);
                    cell_queue.push(cell);
                }
                if cc2 != cc_new {
                    annotate_cell(&cells[cell2], cc_new);
                    cell_queue.push(cell2);
                }
            }
        }
    }

    // Do a final seed fill over Voronoi cells and unmarked Voronoi edges.
    while let Some(&cell) = cell_queue.last() {
        let cc = cell_category(&cells[cell]);
        cell_queue.pop();
        let first_edge = follow(cells[cell].incident_edge(), "incident edge");
        let mut edge = first_edge;
        let ec_new = if cc == CellCategory::OUTSIDE {
            EdgeCategory::POINTS_OUTSIDE
        } else {
            EdgeCategory::POINTS_INSIDE
        };
        loop {
            let e = &edges[edge];
            let ec = edge_category(e);
            if ec == EdgeCategory::UNKNOWN {
                let twin = &edges[twin_of(e)];
                annotate_edge(e, ec_new);
                annotate_edge(twin, ec_new);
                let cell2 = cell_of(twin);
                let cc2 = cell_category(&cells[cell2]);
                if cc2 != cc {
                    annotate_cell(&cells[cell2], cc);
                    cell_queue.push(cell2);
                }
            }
            edge = follow(e.next(), "next");
            if edge == first_edge {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use orca_geom::line::Line;
    use orca_geom::point::Point;

    use super::{
        CellCategory, VertexCategory, annotate_inside_outside, cell_category, vertex_category,
        vertex_equal_to_point,
    };
    use crate::geometry_voronoi::VoronoiDiagram;

    #[test]
    fn a_square_has_its_centre_inside() {
        let p = [
            Point::new(0, 0),
            Point::new(1000, 0),
            Point::new(1000, 1000),
            Point::new(0, 1000),
        ];
        let lines: Vec<Line> = (0..4).map(|i| Line::new(p[i], p[(i + 1) % 4])).collect();
        let mut vd = VoronoiDiagram::new();
        vd.construct_voronoi_segments(&lines, true).unwrap();
        annotate_inside_outside(&vd, &lines);
        let centre = vd
            .vertices()
            .iter()
            .find(|v| (v.x(), v.y()) == (500.0, 500.0))
            .expect("the centre is a vertex");
        assert_eq!(vertex_category(centre), VertexCategory::INSIDE);
        let corners = vd
            .vertices()
            .iter()
            .filter(|v| vertex_category(v) == VertexCategory::ON_CONTOUR)
            .count();
        assert_eq!(corners, 4);
        // The segments' cells are split by their segments.
        for cell in vd.cells().iter().filter(|c| c.contains_segment()) {
            assert_eq!(cell_category(cell), CellCategory::BOUNDARY);
        }
    }

    #[test]
    fn equal_within_128_ulps() {
        let p = Point::new(1000, -3);
        assert!(vertex_equal_to_point(1000.0, -3.0, p));
        let near = f64::from_bits(1000f64.to_bits() + 128);
        let far = f64::from_bits(1000f64.to_bits() + 129);
        assert!(vertex_equal_to_point(near, -3.0, p));
        assert!(!vertex_equal_to_point(far, -3.0, p));
    }
}
