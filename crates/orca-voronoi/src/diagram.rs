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
// from boost/polygon/voronoi_diagram.hpp, lines 1-620 (voronoi_cell, voronoi_vertex, voronoi_edge,
// voronoi_diagram_traits and voronoi_diagram<double>). Copyright Andrii Sydorchuk 2010-2012, under
// the Boost Software License, version 1.0 (BSL-1.0; licenses/BSL-1.0.txt).

//! Boost.Polygon's Voronoi diagram (`voronoi_diagram.hpp`): the half-edge structure the builder
//! fills, `voronoi_diagram<double>` ([`BoostVoronoiDiagram`]), and its cells, vertices and edges
//! ([`VoronoiCell`], [`VoronoiVertex`], [`VoronoiEdge`]).
//!
//! - The C++ links the elements with pointers into the diagram's three `std::vector`s. Here a link
//!   is the element's index in its container (`None` for a null pointer), which is what the C++'s
//!   pointers stand for: the containers are reserved before the sweep (`_reserve`) and never move
//!   while it runs, and `_build` compacts them in place. An edge's `vertex1()`, `rot_next()`,
//!   `rot_prev()`, `is_finite()` and `is_infinite()` follow its twin or its previous edge, so they
//!   take the edge container. A null pointer the C++ would follow (the twin of an edge, the
//!   previous edge of an edge whose vertex the C++ reads) is a panic here, where the C++ crashes.
//! - The colours are `mutable` in the C++ and set through `const` references (the medial axis and
//!   Arachne colour a `const` diagram), so they are [`Cell`]s here. Each element keeps its 5 low
//!   bits for itself: a cell its source category, an edge whether it is linear (bit 1) and primary
//!   (bit 2). `color()` is the value shifted down by 5, `set_color(c)` keeps the low bits and puts
//!   `c << 5` above them, dropping what passes 64 bits as the C++'s `std::size_t` does.
//! - [`BoostVoronoiDiagram`] is the builder's output ([`VoronoiOutput`], the C++'s `OUTPUT`) with
//!   `usize` edge handles: `_insert_new_edge` pushes two half-edges, the first for `site1`, and the
//!   cell of a new site; the circle event's form pushes the vertex at the circle's centre, ends the
//!   two old bisectors there and links the new pair to them. `_build` removes the edges whose two
//!   vertices are equal within 128 ulps (`vertex_equality_predicate_type`), sets the incident edges
//!   of the cells and vertices, removes the vertices no edge starts at, and closes the loops of the
//!   infinite edges (a chain of lines when there is no vertex, else each boundary cell's rays).
//! - `construct` does not clear the diagram: a second construction into a diagram that holds one
//!   appends to its containers, takes no initial cell (`cells_` is not empty) and points its edges
//!   at the old cells by the new sites' sorted indexes, as the C++ does. The C++ can then move its
//!   containers past their reserved size and leave its pointers dangling; the indexes here do not
//!   dangle. `Slic3r::Geometry::VoronoiDiagram` clears its diagram where it needs a fresh one.
//!
//! Left out: nothing of these lines. The diagram's copy constructor and assignment, which the C++
//! disallows, are not given (no `Clone` for [`BoostVoronoiDiagram`]; its elements are `Clone`, as
//! their implicit copies are), and the containers' `const_iterator`s are slices.

use std::cell::Cell;

use crate::builder::VoronoiOutput;
use crate::ctypes::{UlpComparison, UlpResult};
use crate::geometry_type::{GeometryCategory, SourceCategory, belongs};
use crate::structures::{CircleEvent, SiteEvent};

/// `Bits::BITS_SHIFT`: the colour sits above the 5 reserved bits.
const BITS_SHIFT: u32 = 0x5;
/// `Bits::BITS_MASK`: the 5 reserved bits.
const BITS_MASK: usize = 0x1F;
/// `voronoi_edge::Bits::BIT_IS_LINEAR`.
const BIT_IS_LINEAR: usize = 0x1;
/// `voronoi_edge::Bits::BIT_IS_PRIMARY`.
const BIT_IS_PRIMARY: usize = 0x2;

/// `color(color) const`: `color_ &= BITS_MASK; color_ |= color << BITS_SHIFT`.
fn set_color_bits(color_: &Cell<usize>, color: usize) {
    color_.set(color_.get() & BITS_MASK);
    color_.set(color_.get() | (color << BITS_SHIFT));
}

// ---------------------------------------------------------------- voronoi_cell (28-92)

/// `voronoi_cell<double>`: the cell of one input site, with the site's index in the input, a
/// link to one of its edges and a colour that holds the site's source category in its low bits.
/// `Clone` is the C++'s implicit copy.
#[derive(Clone, Debug)]
pub struct VoronoiCell {
    source_index: usize,
    incident_edge: Option<usize>,
    color: Cell<usize>,
}

impl VoronoiCell {
    /// `voronoi_cell(source_index, source_category)`: no incident edge, the category in the
    /// colour's low bits.
    #[must_use]
    pub fn new(source_index: usize, source_category: SourceCategory) -> Self {
        Self {
            source_index,
            incident_edge: None,
            color: Cell::new(source_category.bits()),
        }
    }

    /// `contains_point()`: the source category is a point's.
    #[must_use]
    pub fn contains_point(&self) -> bool {
        let source_category = self.source_category();
        belongs(source_category, GeometryCategory::Point)
    }

    /// `contains_segment()`: the source category is a segment's.
    #[must_use]
    pub fn contains_segment(&self) -> bool {
        let source_category = self.source_category();
        belongs(source_category, GeometryCategory::Segment)
    }

    /// `source_index()`.
    #[must_use]
    pub fn source_index(&self) -> usize {
        self.source_index
    }

    /// `source_category()`: `color_ & SOURCE_CATEGORY_BITMASK`.
    #[must_use]
    pub fn source_category(&self) -> SourceCategory {
        SourceCategory((self.color.get() & SourceCategory::BITMASK.bits()) as u32)
    }

    /// `is_degenerate()`: the cell has no incident edge.
    #[must_use]
    pub fn is_degenerate(&self) -> bool {
        self.incident_edge.is_none()
    }

    /// `incident_edge()`.
    #[must_use]
    pub fn incident_edge(&self) -> Option<usize> {
        self.incident_edge
    }

    /// `incident_edge(e)`.
    pub fn set_incident_edge(&mut self, e: Option<usize>) {
        self.incident_edge = e;
    }

    /// `color()`: the colour above the 5 reserved bits.
    #[must_use]
    pub fn color(&self) -> usize {
        self.color.get() >> BITS_SHIFT
    }

    /// `color(color) const`: sets the colour, keeping the reserved bits (the source category).
    pub fn set_color(&self, color: usize) {
        set_color_bits(&self.color, color);
    }
}

// ---------------------------------------------------------------- voronoi_vertex (94-138)

/// `voronoi_vertex<double>`: a vertex of the diagram, with a link to one of the edges that start
/// at it and a colour. `Clone` is the C++'s implicit copy.
#[derive(Clone, Debug)]
pub struct VoronoiVertex {
    x: f64,
    y: f64,
    incident_edge: Option<usize>,
    color: Cell<usize>,
}

impl VoronoiVertex {
    /// `voronoi_vertex(x, y)`: no incident edge, colour 0.
    #[must_use]
    pub fn new(x: f64, y: f64) -> Self {
        Self {
            x,
            y,
            incident_edge: None,
            color: Cell::new(0),
        }
    }

    /// `x()`.
    #[must_use]
    pub fn x(&self) -> f64 {
        self.x
    }

    /// `y()`.
    #[must_use]
    pub fn y(&self) -> f64 {
        self.y
    }

    /// `is_degenerate()`: no edge starts at the vertex.
    #[must_use]
    pub fn is_degenerate(&self) -> bool {
        self.incident_edge.is_none()
    }

    /// `incident_edge()`.
    #[must_use]
    pub fn incident_edge(&self) -> Option<usize> {
        self.incident_edge
    }

    /// `incident_edge(e)`.
    pub fn set_incident_edge(&mut self, e: Option<usize>) {
        self.incident_edge = e;
    }

    /// `color()`.
    #[must_use]
    pub fn color(&self) -> usize {
        self.color.get() >> BITS_SHIFT
    }

    /// `color(color) const`.
    pub fn set_color(&self, color: usize) {
        set_color_bits(&self.color, color);
    }
}

// ---------------------------------------------------------------- voronoi_edge (140-258)

/// `voronoi_edge<double>`: a half-edge, with links to its cell, the vertex it starts at, its twin
/// and the next and previous half-edges counter-clockwise around its cell, and a colour whose low
/// bits say whether it is linear and primary. `Clone` is the C++'s implicit copy.
#[derive(Clone, Debug)]
pub struct VoronoiEdge {
    cell: Option<usize>,
    vertex: Option<usize>,
    twin: Option<usize>,
    next: Option<usize>,
    prev: Option<usize>,
    color: Cell<usize>,
}

impl VoronoiEdge {
    /// `voronoi_edge(is_linear, is_primary)`: no links, the two flags in the colour's low bits.
    #[must_use]
    pub fn new(is_linear: bool, is_primary: bool) -> Self {
        let mut color = 0;
        if is_linear {
            color |= BIT_IS_LINEAR;
        }
        if is_primary {
            color |= BIT_IS_PRIMARY;
        }
        Self {
            cell: None,
            vertex: None,
            twin: None,
            next: None,
            prev: None,
            color: Cell::new(color),
        }
    }

    /// `cell()`.
    #[must_use]
    pub fn cell(&self) -> Option<usize> {
        self.cell
    }

    /// `cell(c)`.
    pub fn set_cell(&mut self, c: Option<usize>) {
        self.cell = c;
    }

    /// `vertex0()`: the vertex the half-edge starts at.
    #[must_use]
    pub fn vertex0(&self) -> Option<usize> {
        self.vertex
    }

    /// `vertex0(v)`.
    pub fn set_vertex0(&mut self, v: Option<usize>) {
        self.vertex = v;
    }

    /// `vertex1()`: `twin_->vertex0()`, the vertex the half-edge ends at. Panics when the edge
    /// has no twin (the C++ follows the null pointer).
    #[must_use]
    pub fn vertex1(&self, edges: &[VoronoiEdge]) -> Option<usize> {
        edges[twin_of(self)].vertex0()
    }

    /// `twin()`.
    #[must_use]
    pub fn twin(&self) -> Option<usize> {
        self.twin
    }

    /// `twin(e)`.
    pub fn set_twin(&mut self, e: Option<usize>) {
        self.twin = e;
    }

    /// `next()`: the next half-edge counter-clockwise around the cell.
    #[must_use]
    pub fn next(&self) -> Option<usize> {
        self.next
    }

    /// `next(e)`.
    pub fn set_next(&mut self, e: Option<usize>) {
        self.next = e;
    }

    /// `prev()`: the previous half-edge counter-clockwise around the cell.
    #[must_use]
    pub fn prev(&self) -> Option<usize> {
        self.prev
    }

    /// `prev(e)`.
    pub fn set_prev(&mut self, e: Option<usize>) {
        self.prev = e;
    }

    /// `rot_next()`: `prev_->twin()`, the next half-edge counter-clockwise around the start
    /// vertex. Panics when the edge has no previous edge (the C++ follows the null pointer).
    #[must_use]
    pub fn rot_next(&self, edges: &[VoronoiEdge]) -> Option<usize> {
        let prev = self
            .prev
            .expect("voronoi_edge::rot_next: the C++ follows a null prev_");
        edges[prev].twin()
    }

    /// `rot_prev()`: `twin_->next()`, the previous half-edge counter-clockwise around the start
    /// vertex. Panics when the edge has no twin.
    #[must_use]
    pub fn rot_prev(&self, edges: &[VoronoiEdge]) -> Option<usize> {
        edges[twin_of(self)].next()
    }

    /// `is_finite()`: both vertices exist (a segment or a parabolic arc).
    #[must_use]
    pub fn is_finite(&self, edges: &[VoronoiEdge]) -> bool {
        self.vertex0().is_some() && self.vertex1(edges).is_some()
    }

    /// `is_infinite()`: a vertex is missing (a ray or a line).
    #[must_use]
    pub fn is_infinite(&self, edges: &[VoronoiEdge]) -> bool {
        self.vertex0().is_none() || self.vertex1(edges).is_none()
    }

    /// `is_linear()`: a segment, a ray or a line.
    #[must_use]
    pub fn is_linear(&self) -> bool {
        (self.color.get() & BIT_IS_LINEAR) != 0
    }

    /// `is_curved()`: a parabolic arc.
    #[must_use]
    pub fn is_curved(&self) -> bool {
        (self.color.get() & BIT_IS_LINEAR) == 0
    }

    /// `is_primary()`: the edge does not go through a segment's end point.
    #[must_use]
    pub fn is_primary(&self) -> bool {
        (self.color.get() & BIT_IS_PRIMARY) != 0
    }

    /// `is_secondary()`: the edge goes through a segment's end point.
    #[must_use]
    pub fn is_secondary(&self) -> bool {
        (self.color.get() & BIT_IS_PRIMARY) == 0
    }

    /// `color()`.
    #[must_use]
    pub fn color(&self) -> usize {
        self.color.get() >> BITS_SHIFT
    }

    /// `color(color) const`.
    pub fn set_color(&self, color: usize) {
        set_color_bits(&self.color, color);
    }
}

/// The edge's twin, which every half-edge the builder makes has (the C++ follows the pointer).
fn twin_of(edge: &VoronoiEdge) -> usize {
    edge.twin
        .expect("voronoi_edge: the C++ follows a null twin_")
}

// ---------------------------------------------------------------- voronoi_diagram_traits (260-278)

/// `voronoi_diagram_traits<double>::vertex_equality_predicate_type`: two vertices are equal when
/// both coordinates are within 128 ulps (`ulp_comparison` answers `EQUAL`).
#[derive(Clone, Copy, Default, Debug)]
pub struct VertexEqualityPredicate {
    ulp_cmp: UlpComparison,
}

impl VertexEqualityPredicate {
    /// `ULPS = 128`.
    pub const ULPS: u32 = 128;

    /// `operator()(v1, v2)`: x within 128 ulps, and then y.
    #[must_use]
    pub fn call(&self, v1: &VoronoiVertex, v2: &VoronoiVertex) -> bool {
        (self.ulp_cmp.call(v1.x(), v2.x(), Self::ULPS) == UlpResult::Equal)
            && (self.ulp_cmp.call(v1.y(), v2.y(), Self::ULPS) == UlpResult::Equal)
    }
}

// ---------------------------------------------------------------- voronoi_diagram (280-616)

/// `voronoi_diagram<double>`: the cells, vertices and half-edges of a Voronoi diagram, filled by
/// a [`crate::VoronoiBuilder`] through [`VoronoiOutput`]. Counter-clockwise order is used around
/// the cells and the vertices.
#[derive(Debug, Default)]
pub struct BoostVoronoiDiagram {
    cells: Vec<VoronoiCell>,
    vertices: Vec<VoronoiVertex>,
    edges: Vec<VoronoiEdge>,
    vertex_equality_predicate: VertexEqualityPredicate,
}

impl BoostVoronoiDiagram {
    /// `voronoi_diagram()`: empty.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `clear()`: empties the three containers.
    pub fn clear(&mut self) {
        self.cells.clear();
        self.vertices.clear();
        self.edges.clear();
    }

    /// `cells()`.
    #[must_use]
    pub fn cells(&self) -> &[VoronoiCell] {
        &self.cells
    }

    /// `vertices()`.
    #[must_use]
    pub fn vertices(&self) -> &[VoronoiVertex] {
        &self.vertices
    }

    /// `edges()`.
    #[must_use]
    pub fn edges(&self) -> &[VoronoiEdge] {
        &self.edges
    }

    /// `num_cells()`.
    #[must_use]
    pub fn num_cells(&self) -> usize {
        self.cells.len()
    }

    /// `num_edges()`.
    #[must_use]
    pub fn num_edges(&self) -> usize {
        self.edges.len()
    }

    /// `num_vertices()`.
    #[must_use]
    pub fn num_vertices(&self) -> usize {
        self.vertices.len()
    }

    /// `is_primary_edge(site1, site2)`: false when one site is a segment and the other is one of
    /// its end points.
    fn is_primary_edge(&self, site1: &SiteEvent, site2: &SiteEvent) -> bool {
        let flag1 = site1.is_segment();
        let flag2 = site2.is_segment();
        if flag1 && !flag2 {
            return (site1.point0() != site2.point0()) && (site1.point1() != site2.point0());
        }
        if !flag1 && flag2 {
            return (site2.point0() != site1.point0()) && (site2.point1() != site1.point0());
        }
        true
    }

    /// `is_linear_edge(site1, site2)`: a secondary edge, or one between two sites of the same
    /// geometry.
    fn is_linear_edge(&self, site1: &SiteEvent, site2: &SiteEvent) -> bool {
        if !self.is_primary_edge(site1, site2) {
            return true;
        }
        !(site1.is_segment() ^ site2.is_segment())
    }

    /// `e->rot_next()` of the edge at `e`.
    fn rot_next(&self, e: usize) -> usize {
        self.edges[e]
            .rot_next(&self.edges)
            .expect("voronoi_edge::rot_next: the C++ follows a null twin_")
    }

    /// `e->rot_prev()` of the edge at `e`.
    fn rot_prev(&self, e: usize) -> usize {
        self.edges[e]
            .rot_prev(&self.edges)
            .expect("voronoi_edge::rot_prev: the C++ follows a null next_")
    }

    /// `e->twin()` of the edge at `e`, which the C++ follows.
    fn twin(&self, e: usize) -> usize {
        twin_of(&self.edges[e])
    }

    /// `remove_edge(edge)`: the edges that start at the edge's second vertex start at its first,
    /// and the edges around it are linked past it and its twin.
    fn remove_edge(&mut self, edge: usize) {
        // Update the endpoints of the incident edges to the second vertex.
        let vertex = self.edges[edge].vertex0();
        let mut updated_edge = self.rot_next(self.twin(edge));
        while updated_edge != self.twin(edge) {
            self.edges[updated_edge].set_vertex0(vertex);
            updated_edge = self.rot_next(updated_edge);
        }

        let edge1 = edge;
        let edge2 = self.twin(edge);

        let edge1_rot_prev = self.rot_prev(edge1);
        let edge1_rot_next = self.rot_next(edge1);

        let edge2_rot_prev = self.rot_prev(edge2);
        let edge2_rot_next = self.rot_next(edge2);

        // Update prev/next pointers for the incident edges.
        let t = self.twin(edge1_rot_next);
        self.edges[t].set_next(Some(edge2_rot_prev));
        let t = self.twin(edge1_rot_next);
        self.edges[edge2_rot_prev].set_prev(Some(t));
        let t = self.twin(edge2_rot_next);
        self.edges[edge1_rot_prev].set_prev(Some(t));
        let t = self.twin(edge2_rot_next);
        self.edges[t].set_next(Some(edge1_rot_prev));
    }
}

impl VoronoiOutput for BoostVoronoiDiagram {
    type Edge = usize;

    /// `_reserve(num_sites)`: room for `num_sites` cells, twice as many vertices and six times as
    /// many half-edges.
    fn reserve(&mut self, num_sites: usize) {
        self.cells.reserve(num_sites);
        self.vertices.reserve(num_sites << 1);
        self.edges.reserve((num_sites << 2) + (num_sites << 1));
    }

    /// `_process_single_site(site)`: the cell of the one site.
    fn process_single_site(&mut self, site: &SiteEvent) {
        self.cells.push(VoronoiCell::new(
            site.initial_index(),
            site.source_category(),
        ));
    }

    /// `_insert_new_edge(site1, site2)`: two twin half-edges, the first in `site1`'s cell; the
    /// first site's cell when the diagram has none, then the second site's.
    fn insert_new_edge(&mut self, site1: &SiteEvent, site2: &SiteEvent) -> (usize, usize) {
        // Get sites' indexes.
        let site_index1 = site1.sorted_index();
        let site_index2 = site2.sorted_index();

        let is_linear = self.is_linear_edge(site1, site2);
        let is_primary = self.is_primary_edge(site1, site2);

        // Create a new half-edge that belongs to the first site.
        self.edges.push(VoronoiEdge::new(is_linear, is_primary));
        let edge1 = self.edges.len() - 1;

        // Create a new half-edge that belongs to the second site.
        self.edges.push(VoronoiEdge::new(is_linear, is_primary));
        let edge2 = self.edges.len() - 1;

        // Add the initial cell during the first edge insertion.
        if self.cells.is_empty() {
            self.cells.push(VoronoiCell::new(
                site1.initial_index(),
                site1.source_category(),
            ));
        }

        // The second site represents a new site during site event processing. Add a new cell to
        // the cell records.
        self.cells.push(VoronoiCell::new(
            site2.initial_index(),
            site2.source_category(),
        ));

        // Set up pointers to cells.
        self.edges[edge1].set_cell(Some(site_index1));
        self.edges[edge2].set_cell(Some(site_index2));

        // Set up twin pointers.
        self.edges[edge1].set_twin(Some(edge2));
        self.edges[edge2].set_twin(Some(edge1));

        // Return a pointer to the new half-edge.
        (edge1, edge2)
    }

    /// `_insert_new_edge(site1, site3, circle, data12, data23)`: the vertex at the circle's
    /// centre, where the bisectors `data12` and `data23` now start, and the bisector of `site1`
    /// and `site3` from it, linked to them.
    fn insert_new_edge_at_circle(
        &mut self,
        site1: &SiteEvent,
        site3: &SiteEvent,
        circle: &CircleEvent,
        data12: Option<usize>,
        data23: Option<usize>,
    ) -> (usize, usize) {
        let edge12 = data12.expect("_insert_new_edge: the C++ follows a null data12");
        let edge23 = data23.expect("_insert_new_edge: the C++ follows a null data23");

        // Add a new Voronoi vertex.
        self.vertices
            .push(VoronoiVertex::new(circle.x(), circle.y()));
        let new_vertex = self.vertices.len() - 1;

        // Update vertex pointers of the old edges.
        self.edges[edge12].set_vertex0(Some(new_vertex));
        self.edges[edge23].set_vertex0(Some(new_vertex));

        let is_linear = self.is_linear_edge(site1, site3);
        let is_primary = self.is_primary_edge(site1, site3);

        // Add a new half-edge.
        self.edges.push(VoronoiEdge::new(is_linear, is_primary));
        let new_edge1 = self.edges.len() - 1;
        self.edges[new_edge1].set_cell(Some(site1.sorted_index()));

        // Add a new half-edge.
        self.edges.push(VoronoiEdge::new(is_linear, is_primary));
        let new_edge2 = self.edges.len() - 1;
        self.edges[new_edge2].set_cell(Some(site3.sorted_index()));

        // Update twin pointers.
        self.edges[new_edge1].set_twin(Some(new_edge2));
        self.edges[new_edge2].set_twin(Some(new_edge1));

        // Update vertex pointer.
        self.edges[new_edge2].set_vertex0(Some(new_vertex));

        // Update Voronoi prev/next pointers.
        self.edges[edge12].set_prev(Some(new_edge1));
        self.edges[new_edge1].set_next(Some(edge12));
        let t = self.twin(edge12);
        self.edges[t].set_next(Some(edge23));
        let t = self.twin(edge12);
        self.edges[edge23].set_prev(Some(t));
        let t = self.twin(edge23);
        self.edges[t].set_next(Some(new_edge2));
        let t = self.twin(edge23);
        self.edges[new_edge2].set_prev(Some(t));

        // Return a pointer to the new half-edge.
        (new_edge1, new_edge2)
    }

    /// `_build()`: removes the degenerate edges and vertices, sets the incident edges and closes
    /// the loops of the infinite edges.
    fn build(&mut self) {
        // Remove degenerate edges.
        let mut last_edge = 0;
        let mut it = 0;
        while it != self.edges.len() {
            let v1 = self.edges[it].vertex0();
            let v2 = self.edges[it].vertex1(&self.edges);
            let degenerate = match (v1, v2) {
                (Some(v1), Some(v2)) => self
                    .vertex_equality_predicate
                    .call(&self.vertices[v1], &self.vertices[v2]),
                _ => false,
            };
            if degenerate {
                self.remove_edge(it);
            } else {
                if it != last_edge {
                    let (e1, e2) = (last_edge, last_edge + 1);
                    self.edges[e1] = self.edges[it].clone();
                    self.edges[e2] = self.edges[it + 1].clone();
                    self.edges[e1].set_twin(Some(e2));
                    self.edges[e2].set_twin(Some(e1));
                    if let Some(p) = self.edges[e1].prev() {
                        self.edges[p].set_next(Some(e1));
                        let n = self.edges[e2]
                            .next()
                            .expect("_build: the C++ follows a null next_ of e2");
                        self.edges[n].set_prev(Some(e2));
                    }
                    if let Some(p) = self.edges[e2].prev() {
                        let n = self.edges[e1]
                            .next()
                            .expect("_build: the C++ follows a null next_ of e1");
                        self.edges[n].set_prev(Some(e1));
                        self.edges[p].set_next(Some(e2));
                    }
                }
                last_edge += 2;
            }
            it += 2;
        }
        self.edges.truncate(last_edge);

        // Set up incident edge pointers for cells and vertices.
        for it in 0..self.edges.len() {
            let cell = self.edges[it]
                .cell()
                .expect("_build: the C++ follows a null cell_");
            self.cells[cell].set_incident_edge(Some(it));
            if let Some(v) = self.edges[it].vertex0() {
                self.vertices[v].set_incident_edge(Some(it));
            }
        }

        // Remove degenerate vertices.
        let mut last_vertex = 0;
        for it in 0..self.vertices.len() {
            if self.vertices[it].incident_edge().is_some() {
                if it != last_vertex {
                    self.vertices[last_vertex] = self.vertices[it].clone();
                    let v = last_vertex;
                    let mut e = self.vertices[v]
                        .incident_edge()
                        .expect("_build: a kept vertex has an incident edge");
                    loop {
                        self.edges[e].set_vertex0(Some(v));
                        e = self.rot_next(e);
                        if Some(e) == self.vertices[v].incident_edge() {
                            break;
                        }
                    }
                }
                last_vertex += 1;
            }
        }
        self.vertices.truncate(last_vertex);

        // Set up next/prev pointers for infinite edges.
        if self.vertices.is_empty() {
            if !self.edges.is_empty() {
                // Update prev/next pointers for the line edges.
                let mut edge_it = 0;
                let mut edge1 = edge_it;
                self.edges[edge1].set_next(Some(edge1));
                self.edges[edge1].set_prev(Some(edge1));
                edge_it += 1;
                edge1 = edge_it;
                edge_it += 1;

                while edge_it != self.edges.len() {
                    let edge2 = edge_it;
                    edge_it += 1;

                    self.edges[edge1].set_next(Some(edge2));
                    self.edges[edge1].set_prev(Some(edge2));
                    self.edges[edge2].set_next(Some(edge1));
                    self.edges[edge2].set_prev(Some(edge1));

                    edge1 = edge_it;
                    edge_it += 1;
                }

                self.edges[edge1].set_next(Some(edge1));
                self.edges[edge1].set_prev(Some(edge1));
            }
        } else {
            // Update prev/next pointers for the ray edges.
            for cell_it in 0..self.cells.len() {
                if self.cells[cell_it].is_degenerate() {
                    continue;
                }
                // Move to the previous edge while it is possible in the CW direction.
                let incident = self.cells[cell_it].incident_edge();
                let mut left_edge = incident.expect("a cell that is not degenerate");
                while let Some(prev) = self.edges[left_edge].prev() {
                    left_edge = prev;
                    // Terminate if this is not a boundary cell.
                    if Some(left_edge) == incident {
                        break;
                    }
                }

                if self.edges[left_edge].prev().is_some() {
                    continue;
                }

                let mut right_edge = incident.expect("a cell that is not degenerate");
                while let Some(next) = self.edges[right_edge].next() {
                    right_edge = next;
                }
                self.edges[left_edge].set_prev(Some(right_edge));
                self.edges[right_edge].set_next(Some(left_edge));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BoostVoronoiDiagram, VoronoiCell, VoronoiEdge, VoronoiVertex};
    use crate::geometry_type::SourceCategory;
    use crate::{VoronoiBuilder, VoronoiOutput};

    #[test]
    fn colours_keep_the_reserved_bits() {
        let c = VoronoiCell::new(7, SourceCategory::REVERSE_SEGMENT);
        assert_eq!(c.color(), 0);
        c.set_color(3);
        assert_eq!(c.color(), 3);
        assert_eq!(c.source_category(), SourceCategory::REVERSE_SEGMENT);
        assert!(c.contains_segment() && !c.contains_point());
        let e = VoronoiEdge::new(true, false);
        e.set_color(usize::MAX);
        assert!(e.is_linear() && e.is_secondary());
        assert_eq!(e.color(), usize::MAX >> 5);
        let v = VoronoiVertex::new(1.0, 2.0);
        v.set_color(1);
        v.set_color(2);
        assert_eq!(v.color(), 2);
    }

    #[test]
    fn a_square_of_points_has_one_vertex_after_build() {
        // The builder makes two vertices at the centre from two circle events; `_build` removes
        // the zero-length edge between them and the vertex left with no edge.
        let mut b = VoronoiBuilder::new();
        for (x, y) in [(0, 0), (10, 0), (0, 10), (10, 10)] {
            b.insert_point(x, y);
        }
        let mut vd = BoostVoronoiDiagram::new();
        b.construct(&mut vd);
        assert_eq!(vd.num_cells(), 4);
        assert_eq!(vd.num_vertices(), 1);
        assert_eq!(vd.num_edges(), 8);
        let v = &vd.vertices()[0];
        assert_eq!((v.x(), v.y()), (5.0, 5.0));
        for (i, e) in vd.edges().iter().enumerate() {
            assert_eq!(vd.edges()[e.twin().unwrap()].twin(), Some(i));
            assert_eq!(vd.edges()[e.next().unwrap()].prev(), Some(i));
        }
    }

    #[test]
    fn collinear_points_chain_their_lines() {
        let mut b = VoronoiBuilder::new();
        for x in [0, 10, 20] {
            b.insert_point(x, 0);
        }
        let mut vd = BoostVoronoiDiagram::new();
        b.construct(&mut vd);
        assert_eq!(vd.num_vertices(), 0);
        assert_eq!(vd.num_edges(), 4);
        let next: Vec<_> = vd.edges().iter().map(|e| e.next().unwrap()).collect();
        assert_eq!(next, vec![0, 2, 1, 3]);
    }

    #[test]
    fn one_site_has_one_cell() {
        let mut vd = BoostVoronoiDiagram::new();
        let mut b = VoronoiBuilder::new();
        b.insert_point(3, 4);
        b.construct(&mut vd);
        assert_eq!(vd.num_cells(), 1);
        assert!(vd.cells()[0].is_degenerate());
        vd.reserve(1);
        vd.clear();
        assert_eq!(vd.num_cells(), 0);
    }
}
