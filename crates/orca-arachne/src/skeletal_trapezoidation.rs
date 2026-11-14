// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Arachne/SkeletalTrapezoidation.hpp lines 1-558 and
// SkeletalTrapezoidation.cpp lines 1-2098, both whole, of OrcaSlicer v2.4.2: lines 1-1268, with
// isEndOfCentral of lines 1336-1355 (the class SkeletalTrapezoidation, its construction from the
// Voronoi diagram, the central edges, the bead counts and the transitions), translated on
// 2026-11-14 by orca-o3-arachne-3; lines 1269-2098 but isEndOfCentral (the transitions applied,
// the extra ribs, the beading propagation, the junctions and the toolpaths), translated on
// 2026-11-14 by orca-o3-arachne-4. Copyright Ultimaker B.V. (2020 and 2021, CuraEngine), with the
// PrusaSlicer authors' (Prusa Research) and the OrcaSlicer authors' changes, under the GNU Affero
// General Public License, version 3 (AGPL-3.0; LICENSE).

//! `SkeletalTrapezoidation` (`Arachne/SkeletalTrapezoidation.hpp` and `.cpp`): the graph built
//! from the Voronoi diagram of the outline, the central edges, the bead counts and the transitions
//! between bead counts (arachne-3), then the transitions applied to the graph, the extra ribs, the
//! beadings propagated through the graph, the junctions on its edges and the toolpaths connected
//! from them (arachne-4).
//!
//! - [`SkeletalTrapezoidation::new`] is the constructor: `constructFromPolygons` builds the
//!   Voronoi diagram of the polygons' segments through orca-voronoi
//!   ([`VoronoiDiagram::construct_voronoi_segments`] with the repair on, the C++'s default, which
//!   runs the detection of known issues, `VoronoiUtilsCgal::is_voronoi_diagram_planar_angle`
//!   among them, and rotates the input when it finds one), then walks each cell's range
//!   ([`VoronoiUtils::compute_point_cell_range`], [`VoronoiUtils::compute_segment_cell_range`])
//!   and transfers its edges into the half-edge graph (`transferEdge`), discretizing parabolic and
//!   point-point edges (`discretize`) and adding a rib to the outline after each piece
//!   (`makeRib`). An edge whose twin was transferred first is made by walking the twin's pieces
//!   back. Then `separatePointyQuadEndNodes`, `collapseSmallEdges` (snap distance 5) and each
//!   quad's first edge as its node's incident edge. The C++ keeps two ankerl maps from a Voronoi
//!   edge's and vertex's address to the graph's edge and node; here they are keyed by the element's
//!   index in the diagram's containers ([`BTreeMap`]s: only `find`, `emplace` and `operator[]` are
//!   used, never their order). The `ARACHNE_DEBUG` check of `is_voronoi_diagram_planar_intersection`
//!   and the self-intersection assert are not compiled in the C++'s build and are not carried.
//! - The transitioning steps of `generateToolpaths`, in its order: `updateIsCentral`,
//!   `filterCentral`, `updateBeadCount`, `filterNoncentralRegions` and, inside
//!   `generateTransitioningRibs`, `generateTransitionMids`, `filterTransitionMids` (with
//!   `dissolveNearbyTransitions`, `dissolveBeadCountRegion` and `filterEndOfCentralTransition`)
//!   and `generateAllTransitionEnds` (with `generateTransitionEnds`, `generateTransitionEnd` and
//!   `isGoingDown`). They are public here (protected in the C++) so that the probe can run them
//!   one at a time, as the probe's derived class does.
//! - `isEndOfCentral` (SkeletalTrapezoidation.cpp 1336-1355, among arachne-4's lines) was
//!   translated by arachne-3, since `filterCentral` and `filterNoncentralRegions` call it.
//!   `normal` (1261-1267) is the file-local helper of `applyTransitions`.
//! - The rest of `generateToolpaths` (arachne-4): `applyTransitions` (each transition end moved
//!   from the twin to the edge it lies on, then a rib inserted at each, the bead counts of the new
//!   nodes set), `generateExtraRibs` (a rib wherever the strategy puts a bead's switch on an edge
//!   long enough), and `generateSegments`: the beadings stored at the central nodes, propagated
//!   upward and downward (`propagateBeadingsUpward`, `propagateBeadingsDownward`, `interpolate`,
//!   `getOrCreateBeading`, `getNearestBeading`), the junctions of each edge
//!   (`generateJunctions`), the toolpath segments connected quad by quad (`connectJunctions`,
//!   `addToolpathSegment`) and the single beads of local maxima
//!   (`generateLocalMaximaSingleBeads`). These are public too, for the probe's stages.
//! - A range-for over an edge list that inserts edges (`applyTransitions`, `generateExtraRibs`)
//!   walks the list live, handle to next handle, so that it meets the new edges, as the C++'s
//!   `std::list` iteration does. `std::list::sort` is a stable sort of the list's values
//!   ([`orca_stl::stable_sort_by`]), `std::sort` MSVC's ([`orca_stl::sort_by`]), the
//!   `std::priority_queue` of `getNearestBeading` [`orca_stl::PriorityQueue`]; the
//!   `ankerl::unordered_dense::set` of `connectJunctions` keeps its order through [`DenseSet`].
//!
//! The C++'s pointers are handles of the graph's lists ([`EdgeId`], [`NodeId`]); a `std::list`
//! iterator into an edge's transitions is a [`ListIt`]; `ptr_vector_t<T>`, the vectors of
//! `shared_ptr`s that own the lists while the edges point to them weakly, is [`PtrVector`]. The
//! beading strategy is borrowed for the object's life, as the C++'s reference is. A loop over the
//! graph's edges or nodes that neither adds nor erases one walks the handles taken before it, the
//! same elements in the same order. `coord_t` arithmetic wraps, as MSVC's does; `float`s are
//! `f32`, mixed with `int64_t` and `double` as the C++ promotes them; float-to-integer conversions
//! go through `orca-crt`, `std::min` and `std::max` through `orca-stl`, the logs through
//! `orca_mesh::log` at the C++'s severities and texts. The exceptions of the Voronoi lookups are
//! returned ([`VoronoiError`]): the constructor returns them, where the C++ throws out of it.
//!
//! Left out, each for the reason given:
//!
//! - `filterOuterCentral` (and `generateToolpaths` with `filter_outermost_central_edges`), which
//!   domain A never calls: it stops ([`crate::stop::FILTER_OUTER_CENTRAL`]).
//! - The `ARACHNE_DEBUG` code (`export_graph_to_svg`, the SVG dumps, `outline`) and the asserts.
//!
//! Stops ([`crate::stop`]): a null pointer the C++ follows (`graph-null-walk`), the twin walk of
//! `transferEdge` meeting a null twin (`transfer-no-twin`, after its warning), the front, back or
//! pop of an empty transition list or the erase of a transition no longer in its list
//! (`transitions-empty`), `interpolate` reading a beading past its end
//! (`interpolate-short-right`), `connectJunctions` meeting an edge with no junctions
//! (`junctions-missing`) and `addToolpathSegment` an empty line (`toolpath-empty-line`).

use core::cell::RefCell;
use core::f64::consts::PI;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use orca_geom::point::{Coord, Point, Points, scaled};
use orca_geom::polygon::Polygons;
use orca_mesh::log::{Severity, log};
use orca_voronoi::{PolygonsSegmentIndex, VoronoiDiagram, VoronoiError, VoronoiUtils};

use crate::beading::{Beading, BeadingStrategy};
use crate::dense_set::DenseSet;
use crate::extrusion_junction::{ExtrusionJunction, LineJunctions};
use crate::extrusion_line::{ExtrusionLine, VariableWidthLines, norm_i64};
use crate::half_edge::{EdgeId, NodeId};
use crate::skeletal_trapezoidation_edge::{
    EdgeType, ListIt, Shared, SkeletalTrapezoidationEdge, TransitionEnd, TransitionEnds,
    TransitionMiddle, TransitionMiddles,
};
use crate::skeletal_trapezoidation_graph::{
    STHalfEdge, STHalfEdgeNode, SkeletalTrapezoidationGraph, shorter_then,
};
use crate::skeletal_trapezoidation_joint::{BeadingPropagation, SkeletalTrapezoidationJoint};
use crate::stop::{
    FILTER_OUTER_CENTRAL, GRAPH_NULL_WALK, INTERPOLATE_SHORT_RIGHT, JUNCTIONS_MISSING,
    TOOLPATH_EMPTY_LINE, TRANSFER_NO_TWIN, TRANSITIONS_EMPTY, stop,
};

/// `ptr_vector_t<T>`: `std::vector<std::shared_ptr<T>>`, the storage the graph's weak pointers
/// point to.
pub type PtrVector<T> = Vec<Shared<T>>;

/// `SkeletalTrapezoidation::NodeSet`: `ankerl::unordered_dense::set<node_t*>`, of which only
/// membership is read (`None` is `nullptr`).
pub type NodeSet = BTreeSet<Option<NodeId>>;

/// `SkeletalTrapezoidation::TransitionMidRef`: one transition along an edge which may hold several.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransitionMidRef {
    /// `edge`.
    pub edge: EdgeId,
    /// `transition_it`: the transition's iterator in the edge's list.
    pub transition_it: ListIt,
}

impl TransitionMidRef {
    /// `TransitionMidRef(edge, transition_it)`.
    #[must_use]
    pub const fn new(edge: EdgeId, transition_it: ListIt) -> Self {
        Self {
            edge,
            transition_it,
        }
    }
}

/// The pointer `p`, which the C++ follows; a null one stops (`graph-null-walk`).
fn req<K>(p: Option<K>, function: &'static str) -> K {
    match p {
        Some(k) => k,
        None => stop(function, GRAPH_NULL_WALK),
    }
}

/// `VoronoiUtils::to_point(vertex)` of a vertex link (`.cast<coord_t>()` is the same point).
fn vertex_point(vd: &VoronoiDiagram, v: Option<usize>) -> Point {
    VoronoiUtils::to_point_ptr(v.map(|v| &vd.vertices()[v]))
}

/// `perp(const Point&)` (`Point.hpp`): `(-v.y, v.x)`, the negation wrapping. It belongs in
/// orca-geom, whose `perp` takes a `Vec2d`.
fn perp(v: Point) -> Point {
    Point::new(v.y.wrapping_neg(), v.x)
}

/// A copy of the first transition of a list; an empty list stops (`std::list::front` of it is
/// undefined).
fn front_of(list: &Shared<TransitionMiddles>, function: &'static str) -> TransitionMiddle {
    let l = list.borrow();
    match l.front() {
        Some(k) => l[k],
        None => stop(function, TRANSITIONS_EMPTY),
    }
}

/// A copy of the last transition of a list; an empty list stops.
fn back_of(list: &Shared<TransitionMiddles>, function: &'static str) -> TransitionMiddle {
    let l = list.borrow();
    match l.back() {
        Some(k) => l[k],
        None => stop(function, TRANSITIONS_EMPTY),
    }
}

/// `normal(p0, len)` (SkeletalTrapezoidation.cpp 1261-1267, file-local): `p0` scaled to the length
/// `len` in `int64_t`, truncated; `(len, 0)` when `p0` is shorter than 1. `applyTransitions` (the
/// piece arachne-4's) calls it.
#[must_use]
pub fn normal(p0: Point, len: Coord) -> Point {
    let len_ = norm_i64(p0);
    if len_ < 1 {
        return Point::new(len, 0);
    }
    Point::new(
        p0.x.wrapping_mul(len).wrapping_div(len_),
        p0.y.wrapping_mul(len).wrapping_div(len_),
    )
}

/// `SkeletalTrapezoidation`: the dynamic beading strategies' main class. The outline is decomposed
/// into trapezoids held in a half-edge graph; the central edges get a bead count and the
/// transitions between bead counts are placed along them.
pub struct SkeletalTrapezoidation<'a> {
    /// `transitioning_angle`: how pointy a region should be before the method applies (180
    /// degrees minus the limit bisector angle).
    pub transitioning_angle: f64,
    /// `discretization_step_size`: the approximate length of the pieces of a discretized parabolic
    /// or point-point edge.
    pub discretization_step_size: Coord,
    /// `transition_filter_dist`: transition middles closer together than this are filtered.
    pub transition_filter_dist: Coord,
    /// `allowed_filter_deviation`: the line width deviation the filtering may make.
    pub allowed_filter_deviation: Coord,
    /// `beading_propagation_transition_dist`: the transitioning distance between beadings
    /// propagated from below and from above.
    pub beading_propagation_transition_dist: Coord,
    /// `beading_strategy`.
    pub beading_strategy: &'a dyn BeadingStrategy,
    /// `graph`: the skeletal graph through the polygons.
    pub graph: SkeletalTrapezoidationGraph,
    /// `vd_edge_to_he_edge`: each Voronoi edge (by its index) to the graph's edge it maps to, the
    /// last piece of a discretized one (`None` stands for the `nullptr` the C++ may store).
    pub vd_edge_to_he_edge: BTreeMap<usize, Option<EdgeId>>,
    /// `vd_node_to_he_node`: each Voronoi vertex (by its index) to the graph's node it maps to.
    pub vd_node_to_he_node: BTreeMap<usize, NodeId>,
    /// `*p_generated_toolpaths`: the toolpaths `generateToolpaths` fills. The C++ points to the
    /// caller's vector; [`SkeletalTrapezoidation::generate_toolpaths`] moves the caller's vector in
    /// here and back out at its end.
    pub p_generated_toolpaths: Vec<VariableWidthLines>,
}

impl<'a> SkeletalTrapezoidation<'a> {
    /// `SkeletalTrapezoidation(polys, beading_strategy, transitioning_angle,
    /// discretization_step_size, transition_filter_dist, allowed_filter_deviation,
    /// beading_propagation_transition_dist)`: stores the parameters and runs
    /// `constructFromPolygons(polys)`.
    ///
    /// # Errors
    ///
    /// The exceptions of the Voronoi construction and lookups, which the C++ throws out of the
    /// constructor.
    pub fn new(
        polys: &Polygons,
        beading_strategy: &'a dyn BeadingStrategy,
        transitioning_angle: f64,
        discretization_step_size: Coord,
        transition_filter_dist: Coord,
        allowed_filter_deviation: Coord,
        beading_propagation_transition_dist: Coord,
    ) -> Result<Self, VoronoiError> {
        let mut st = Self {
            transitioning_angle,
            discretization_step_size,
            transition_filter_dist,
            allowed_filter_deviation,
            beading_propagation_transition_dist,
            beading_strategy,
            graph: SkeletalTrapezoidationGraph::default(),
            vd_edge_to_he_edge: BTreeMap::new(),
            vd_node_to_he_node: BTreeMap::new(),
            p_generated_toolpaths: Vec::new(),
        };
        st.construct_from_polygons(polys)?;
        Ok(st)
    }

    /// `central_filter_dist()`: central areas smaller than this are filtered,
    /// `scaled<coord_t>(0.02)`.
    #[must_use]
    pub fn central_filter_dist(&self) -> Coord {
        scaled(0.02)
    }

    /// `snap_dist()`: the arithmetic inaccuracy, `scaled<coord_t>(0.02)`; only used to decide
    /// whether a transition needs an extra edge.
    #[must_use]
    pub fn snap_dist(&self) -> Coord {
        scaled(0.02)
    }

    // ------------------------------------------------------------------ the graph's fields

    /// `edge->from`, followed.
    fn edge_from(&self, edge: EdgeId, function: &'static str) -> NodeId {
        req(self.graph.edges[edge].from, function)
    }

    fn edge_to(&self, edge: EdgeId, function: &'static str) -> NodeId {
        req(self.graph.edges[edge].to, function)
    }

    fn edge_twin(&self, edge: EdgeId, function: &'static str) -> EdgeId {
        req(self.graph.edges[edge].twin, function)
    }

    /// `edge->twin->next`, the next edge round the edge's `to` node.
    fn next_round(&self, edge: EdgeId, function: &'static str) -> Option<EdgeId> {
        self.graph.edges[self.edge_twin(edge, function)].next
    }

    fn dist(&self, node: NodeId) -> Coord {
        self.graph.nodes[node].data.distance_to_boundary
    }

    fn bead_count(&self, node: NodeId) -> Coord {
        self.graph.nodes[node].data.bead_count
    }

    fn p(&self, node: NodeId) -> Point {
        self.graph.nodes[node].p
    }

    fn is_central(&self, edge: EdgeId) -> bool {
        self.graph.edges[edge].data.is_central()
    }

    fn set_is_central(&mut self, edge: EdgeId, b: bool) {
        self.graph.edges[edge].data.set_is_central(b);
    }

    /// `(edge->from->p - edge->to->p).cast<int64_t>().norm()`.
    fn edge_length(&self, edge: EdgeId, function: &'static str) -> Coord {
        let from = self.edge_from(edge, function);
        let to = self.edge_to(edge, function);
        norm_i64(self.p(from) - self.p(to))
    }

    /// The handles of the graph's edges, in the list's order.
    fn edge_keys(&self) -> Vec<EdgeId> {
        self.graph.edges.keys().collect()
    }

    // ------------------------------------------------------------------ initialization

    /// `makeNode(vd_node, p)`: the node the Voronoi vertex maps to, or a new node at `p` at the
    /// front of the nodes, mapped to it.
    fn make_node(&mut self, vd_node: usize, p: Point) -> NodeId {
        if let Some(&node) = self.vd_node_to_he_node.get(&vd_node) {
            return node;
        }
        let node = self
            .graph
            .nodes
            .push_front(STHalfEdgeNode::new(SkeletalTrapezoidationJoint::new(), p));
        self.vd_node_to_he_node.entry(vd_node).or_insert(node);
        node
    }

    /// `transferEdge(from, to, vd_edge, prev_edge, start_source_point, end_source_point,
    /// segments)`: the graph's edges for a Voronoi edge. When its twin was transferred, its pieces
    /// are the twin's walked back, a rib after each but the last; else the discretized edge's
    /// pieces, a rib after each but the last (the caller makes that one), mapped from the Voronoi
    /// edge by the last. `prev_edge` is the edge before them on entry (may be `None`) and the last
    /// edge made on return.
    #[allow(clippy::too_many_arguments)] // The C++'s seven parameters.
    fn transfer_edge(
        &mut self,
        vd: &VoronoiDiagram,
        from: Point,
        to: Point,
        vd_edge: usize,
        prev_edge: &mut Option<EdgeId>,
        start_source_point: Point,
        end_source_point: Point,
        segments: &[PolygonsSegmentIndex<'_>],
    ) -> Result<(), VoronoiError> {
        const F: &str = "SkeletalTrapezoidation::transferEdge";
        let vd_edges = vd.edges();
        let e = &vd_edges[vd_edge];
        let he_edge_it = e
            .twin()
            .and_then(|t| self.vd_edge_to_he_edge.get(&t).copied());
        if let Some(source_twin) = he_edge_it {
            // Twin segment(s) have already been made
            let end_node = match e
                .vertex1(vd_edges)
                .and_then(|v| self.vd_node_to_he_node.get(&v).copied())
            {
                Some(node) => node,
                None => stop(F, GRAPH_NULL_WALK),
            };
            let mut twin = source_twin;
            loop {
                let Some(tw) = twin else {
                    log(
                        Severity::Warning,
                        "Encountered a voronoi edge without twin.",
                    );
                    // The C++ `continue`s, and its loop step follows the null twin.
                    stop(F, TRANSFER_NO_TWIN);
                };
                let edge = self
                    .graph
                    .edges
                    .push_front(STHalfEdge::new(SkeletalTrapezoidationEdge::default()));
                let (tw_to, tw_from) = (self.graph.edges[tw].to, self.graph.edges[tw].from);
                self.graph.edges[edge].from = tw_to;
                self.graph.edges[edge].to = tw_from;
                self.graph.edges[edge].twin = Some(tw);
                self.graph.edges[tw].twin = Some(edge);
                let edge_from = req(tw_to, F);
                self.graph.nodes[edge_from].incident_edge = Some(edge);

                if let Some(prev) = *prev_edge {
                    self.graph.edges[edge].prev = Some(prev);
                    self.graph.edges[prev].next = Some(edge);
                }

                *prev_edge = Some(edge);

                if self.graph.edges[edge].to == Some(end_node) {
                    return Ok(());
                }

                let twin_prev = self.graph.edges[tw].prev;
                let twin_prev_twin = twin_prev.and_then(|x| self.graph.edges[x].twin);
                let twin_prev_twin_prev = twin_prev_twin.and_then(|x| self.graph.edges[x].prev);
                if twin_prev.is_none() || twin_prev_twin.is_none() || twin_prev_twin_prev.is_none()
                {
                    log(Severity::Error, "Discretized segment behaves oddly!");
                    return Ok(());
                }

                let mut rib = edge;
                self.graph
                    .make_rib(&mut rib, start_source_point, end_source_point);
                *prev_edge = Some(rib);

                // twin = twin->prev->twin->prev
                let tp = req(self.graph.edges[tw].prev, F);
                let tpt = req(self.graph.edges[tp].twin, F);
                twin = self.graph.edges[tpt].prev;
            }
        }

        let discretized = self.discretize(vd, vd_edge, segments)?;
        if discretized.len() < 2 {
            log(Severity::Warning, "Discretized Voronoi edge is degenerate.");
        }

        if let Some(prev) = *prev_edge
            && self.graph.edges[prev].to.is_none()
        {
            log(Severity::Warning, "Previous edge doesn't go anywhere.");
        }
        // TODO: investigate whether boost:voronoi can produce multiple verts and violates
        // consistency
        let mut v0 = match *prev_edge {
            Some(prev) => self.graph.edges[prev].to,
            None => Some(self.make_node(req(e.vertex0(), F), from)),
        };
        let last = discretized.len().wrapping_sub(1);
        for (p1_idx, &p1) in discretized.iter().enumerate().skip(1) {
            let v1 = if p1_idx < last {
                self.graph
                    .nodes
                    .push_front(STHalfEdgeNode::new(SkeletalTrapezoidationJoint::new(), p1))
            } else {
                self.make_node(req(e.vertex1(vd_edges), F), to)
            };

            let edge = self
                .graph
                .edges
                .push_front(STHalfEdge::new(SkeletalTrapezoidationEdge::default()));
            self.graph.edges[edge].from = v0;
            self.graph.edges[edge].to = Some(v1);
            let edge_from = req(v0, F);
            self.graph.nodes[edge_from].incident_edge = Some(edge);

            if let Some(prev) = *prev_edge {
                self.graph.edges[edge].prev = Some(prev);
                self.graph.edges[prev].next = Some(edge);
            }

            *prev_edge = Some(edge);
            v0 = Some(v1);

            if p1_idx < last {
                // Rib for last segment gets introduced outside this function!
                let mut rib = edge;
                self.graph
                    .make_rib(&mut rib, start_source_point, end_source_point);
                *prev_edge = Some(rib);
            }
        }
        self.vd_edge_to_he_edge.entry(vd_edge).or_insert(*prev_edge);
        Ok(())
    }

    /// `discretize(segment, segments)`: the points along a Voronoi edge where it is cut into
    /// pieces with a straight medial axis and a linear width: the two ends of an edge between two
    /// segments or of a secondary edge; `discretize_parabola` between a point and a segment; and
    /// between two points, even steps with an extra point at each marking of the transitioning
    /// angle the edge passes.
    ///
    /// # Errors
    ///
    /// The source lookups' exceptions.
    pub fn discretize(
        &self,
        vd: &VoronoiDiagram,
        vd_edge: usize,
        segments: &[PolygonsSegmentIndex<'_>],
    ) -> Result<Points, VoronoiError> {
        const F: &str = "SkeletalTrapezoidation::discretize";
        // Terminology in this function assumes that the edge moves horizontally from left to
        // right. This is not necessarily the case; the edge can go in any direction, but it helps
        // to picture it in a certain direction in your head.
        let vd_edges = vd.edges();
        let e = &vd_edges[vd_edge];
        let left_cell = &vd.cells()[req(e.cell(), F)];
        let right_cell = &vd.cells()[req(vd_edges[req(e.twin(), F)].cell(), F)];

        let start = vertex_point(vd, e.vertex0());
        let end = vertex_point(vd, e.vertex1(vd_edges));

        let point_left = left_cell.contains_point();
        let point_right = right_cell.contains_point();
        if (!point_left && !point_right) || e.is_secondary() {
            // Source vert is directly connected to source segment
            return Ok(vec![start, end]);
        } else if point_left != point_right {
            // This is a parabolic edge between a point and a line.
            let p = VoronoiUtils::get_source_point(
                if point_left { left_cell } else { right_cell },
                segments,
            )?;
            let s = VoronoiUtils::get_source_segment(
                if point_left { right_cell } else { left_cell },
                segments,
            )?;
            #[allow(clippy::cast_possible_truncation)] // `double` to `float`, rounded.
            let angle = self.transitioning_angle as f32;
            return Ok(VoronoiUtils::discretize_parabola(
                p,
                s,
                start,
                end,
                self.discretization_step_size,
                angle,
            ));
        }

        // This is a straight edge between two points.
        // While the edge is straight, it is still discretized since the part becomes narrower
        // between the two points. As such it may need different beadings along the way.
        let left_point = VoronoiUtils::get_source_point(left_cell, segments)?;
        let right_point = VoronoiUtils::get_source_point(right_cell, segments)?;
        let d: Coord = norm_i64(right_point - left_point);
        let middle = (left_point + right_point) / 2;
        let x_axis_dir = perp(right_point - left_point);
        let x_axis_length: Coord = norm_i64(x_axis_dir);

        // Project a point on the edge.
        let projected_x = |from: Point| -> Coord {
            let vec = from - middle;
            vec.dot(x_axis_dir).wrapping_div(x_axis_length)
        };

        let start_x = projected_x(start);
        let end_x = projected_x(end);

        // Part of the edge will be bound to the markings on the endpoints of the edge. Calculate
        // how far that is.
        #[allow(clippy::cast_possible_truncation)] // `double` to `float`, rounded.
        let bound = (0.5 / orca_crt::tan((PI - self.transitioning_angle) * 0.5)) as f32;
        #[allow(clippy::cast_precision_loss)] // `int64_t * float` is a `float` product.
        let mut marking_start_x: i64 = orca_crt::f32_to_i64(d.wrapping_neg() as f32 * bound);
        #[allow(clippy::cast_precision_loss)]
        let mut marking_end_x: i64 = orca_crt::f32_to_i64(d as f32 * bound);

        let marking_of = |marking_x: i64| -> Point {
            middle
                + Point::new(
                    x_axis_dir
                        .x
                        .wrapping_mul(marking_x)
                        .wrapping_div(x_axis_length),
                    x_axis_dir
                        .y
                        .wrapping_mul(marking_x)
                        .wrapping_div(x_axis_length),
                )
        };
        let mut marking_start = marking_of(marking_start_x);
        let mut marking_end = marking_of(marking_end_x);
        let mut direction: i64 = 1;

        if start_x > end_x {
            // Oops, the Voronoi edge is the other way around.
            direction = -1;
            core::mem::swap(&mut marking_start, &mut marking_end);
            core::mem::swap(&mut marking_start_x, &mut marking_end_x);
        }

        // Start generating points along the edge.
        let a = start;
        let b = end;
        let mut ret = Points::new();
        ret.push(a);

        // Introduce an extra edge at the borders of the markings?
        let mut add_marking_start =
            marking_start_x.wrapping_mul(direction) > start_x.wrapping_mul(direction);
        let mut add_marking_end =
            marking_end_x.wrapping_mul(direction) > start_x.wrapping_mul(direction);

        // The edge's length may not be divisible by the step size, so calculate an integer step
        // count and evenly distribute the vertices among those.
        let ab = b - a;
        let ab_size: Coord = norm_i64(ab);
        let mut step_count: Coord = ab_size
            .wrapping_add(self.discretization_step_size / 2)
            .wrapping_div(self.discretization_step_size);
        if step_count % 2 == 1 {
            step_count += 1; // enforce a discretization point being added in the middle
        }
        let mut step: Coord = 1;
        while step < step_count {
            // Now simply interpolate the coordinates to get the new vertices!
            let here = a + Point::new(
                ab.x.wrapping_mul(step).wrapping_div(step_count),
                ab.y.wrapping_mul(step).wrapping_div(step_count),
            );
            // If we've surpassed the position of the extra markings, we may need to insert them
            // first.
            let x_here = projected_x(here);
            if add_marking_start
                && marking_start_x.wrapping_mul(direction) < x_here.wrapping_mul(direction)
            {
                ret.push(marking_start);
                add_marking_start = false;
            }
            if add_marking_end
                && marking_end_x.wrapping_mul(direction) < x_here.wrapping_mul(direction)
            {
                ret.push(marking_end);
                add_marking_end = false;
            }
            ret.push(here);
            step += 1;
        }
        if add_marking_end && marking_end_x.wrapping_mul(direction) < end_x.wrapping_mul(direction)
        {
            ret.push(marking_end);
        }
        ret.push(b);
        Ok(ret)
    }

    /// `constructFromPolygons(polys)`: the Voronoi diagram of the polygons' segments, each cell's
    /// range of edges transferred into the graph with a rib after each, then
    /// `separatePointyQuadEndNodes`, `collapseSmallEdges` and each quad's first edge as its `from`
    /// node's incident edge, so that every edge is reachable from a node's incident edge without
    /// going backward.
    fn construct_from_polygons(&mut self, polys: &Polygons) -> Result<(), VoronoiError> {
        const F: &str = "SkeletalTrapezoidation::constructFromPolygons";
        // The check of self intersections is an assert (EdgeGrid), not in the release build.

        self.vd_edge_to_he_edge.clear();
        self.vd_node_to_he_node.clear();

        let mut segments: Vec<PolygonsSegmentIndex<'_>> = Vec::new();
        for (poly_idx, poly) in polys.iter().enumerate() {
            for point_idx in 0..poly.points.len() {
                // `size_t` to `unsigned int`, the low 32 bits.
                #[allow(clippy::cast_possible_truncation)]
                segments.push(PolygonsSegmentIndex::new(
                    polys,
                    poly_idx as u32,
                    point_idx as u32,
                ));
            }
        }

        let mut voronoi_diagram = VoronoiDiagram::new();
        voronoi_diagram.construct_voronoi_segments(&segments, true)?;

        let vd = &voronoi_diagram;
        for cell in vd.cells() {
            if cell.incident_edge().is_none() {
                continue; // There is no spoon
            }

            let start_source_point: Point;
            let end_source_point: Point;
            let starting_voronoi_edge: Option<usize>;
            let ending_voronoi_edge: Option<usize>;
            // Compute and store result in above variables

            if cell.contains_point() {
                let cell_range = VoronoiUtils::compute_point_cell_range(vd, cell, &segments)?;
                start_source_point = cell_range.source_point;
                end_source_point = cell_range.source_point;
                starting_voronoi_edge = cell_range.edge_begin;
                ending_voronoi_edge = cell_range.edge_end;

                if !cell_range.is_valid() {
                    continue;
                }
            } else {
                let cell_range = VoronoiUtils::compute_segment_cell_range(vd, cell, &segments)?;
                start_source_point = cell_range.source_segment_start_point;
                end_source_point = cell_range.source_segment_end_point;
                starting_voronoi_edge = cell_range.edge_begin;
                ending_voronoi_edge = cell_range.edge_end;
            }

            let (Some(starting_voronoi_edge), Some(ending_voronoi_edge)) =
                (starting_voronoi_edge, ending_voronoi_edge)
            else {
                // The C++ asserts that each cell starts and ends in a polygon vertex.
                continue;
            };

            // Copy start to end edge to graph
            let vd_edges = vd.edges();
            let mut prev_edge: Option<EdgeId> = None;
            let starting_to = vertex_point(vd, vd_edges[starting_voronoi_edge].vertex1(vd_edges));
            self.transfer_edge(
                vd,
                start_source_point,
                starting_to,
                starting_voronoi_edge,
                &mut prev_edge,
                start_source_point,
                end_source_point,
                &segments,
            )?;
            // `vd_node_to_he_node[vertex0]`: the C++ follows what `operator[]` gives, `nullptr`
            // for a vertex not in the map.
            let starting_node = match vd_edges[starting_voronoi_edge]
                .vertex0()
                .and_then(|v| self.vd_node_to_he_node.get(&v).copied())
            {
                Some(node) => node,
                None => stop(F, GRAPH_NULL_WALK),
            };
            self.graph.nodes[starting_node].data.distance_to_boundary = 0;

            let mut rib = req(prev_edge, F);
            self.graph
                .make_rib(&mut rib, start_source_point, end_source_point);
            prev_edge = Some(rib);
            let mut vd_edge = req(vd_edges[starting_voronoi_edge].next(), F);
            while vd_edge != ending_voronoi_edge {
                let v1 = vertex_point(vd, vd_edges[vd_edge].vertex0());
                let v2 = vertex_point(vd, vd_edges[vd_edge].vertex1(vd_edges));
                self.transfer_edge(
                    vd,
                    v1,
                    v2,
                    vd_edge,
                    &mut prev_edge,
                    start_source_point,
                    end_source_point,
                    &segments,
                )?;
                let mut rib = req(prev_edge, F);
                self.graph
                    .make_rib(&mut rib, start_source_point, end_source_point);
                prev_edge = Some(rib);
                vd_edge = req(vd_edges[vd_edge].next(), F);
            }

            let ending_from = vertex_point(vd, vd_edges[ending_voronoi_edge].vertex0());
            self.transfer_edge(
                vd,
                ending_from,
                end_source_point,
                ending_voronoi_edge,
                &mut prev_edge,
                start_source_point,
                end_source_point,
                &segments,
            )?;
            let last_to = self.edge_to(req(prev_edge, F), F);
            self.graph.nodes[last_to].data.distance_to_boundary = 0;
        }

        self.separate_pointy_quad_end_nodes();

        self.graph.collapse_small_edges(5);

        // Set [incident_edge] the first possible edge that way we can iterate over all reachable
        // edges from node.incident_edge, without needing to iterate backward
        for edge in self.edge_keys() {
            if self.graph.edges[edge].prev.is_none() {
                let from = self.edge_from(edge, F);
                self.graph.nodes[from].incident_edge = Some(edge);
            }
        }
        Ok(())
    }

    /// `separatePointyQuadEndNodes()`: a node that starts two quads (the cell of an input vertex)
    /// is duplicated at the back of the nodes for the second, so that both quads' ends are
    /// reachable from a node's incident edge.
    pub fn separate_pointy_quad_end_nodes(&mut self) {
        const F: &str = "SkeletalTrapezoidation::separatePointyQuadEndNodes";
        let mut visited_nodes = NodeSet::new();
        for edge in self.edge_keys() {
            if self.graph.edges[edge].prev.is_some() {
                continue;
            }
            let quad_start = edge;
            let from = self.graph.edges[quad_start].from;
            if !visited_nodes.contains(&from) {
                visited_nodes.insert(from);
            } else {
                // Needs to be duplicated
                let copy = self.graph.nodes[req(from, F)].clone();
                let new_node = self.graph.nodes.push_back(copy);
                self.graph.nodes[new_node].incident_edge = Some(quad_start);
                self.graph.edges[quad_start].from = Some(new_node);
                let twin = self.edge_twin(quad_start, F);
                self.graph.edges[twin].to = Some(new_node);
            }
        }
    }

    // ------------------------------------------------------------------ transitioning

    /// `generateToolpaths(generated_toolpaths, filter_outermost_central_edges = false)`: the
    /// paths to extrude, appended to `generated_toolpaths` (moved into
    /// [`SkeletalTrapezoidation::p_generated_toolpaths`] while the steps run).
    pub fn generate_toolpaths(
        &mut self,
        generated_toolpaths: &mut Vec<VariableWidthLines>,
        filter_outermost_central_edges: bool,
    ) {
        self.p_generated_toolpaths = core::mem::take(generated_toolpaths);

        self.update_is_central();

        self.filter_central(self.central_filter_dist());

        if filter_outermost_central_edges {
            self.filter_outer_central();
        }

        self.update_bead_count();

        self.filter_noncentral_regions();

        self.generate_transitioning_ribs();

        self.generate_extra_ribs();

        self.generate_segments();

        *generated_toolpaths = core::mem::take(&mut self.p_generated_toolpaths);
    }

    /// `updateIsCentral()`: each edge is central when the remaining diameter change along it is
    /// smaller than its length times `sin(transitioning_angle / 2)`; a twin takes its twin's
    /// answer, an extra-VD edge and an edge near the outline (both ends within half the first
    /// transition thickness) are not central.
    pub fn update_is_central(&mut self) {
        //                                            _.-'^`      A and B are the endpoints of an edge we're checking.
        //                                      _.-'^`            Part of the line AB will be used as a cap,
        //                                _.-'^` \                because the polygon is too narrow there.
        //                          _.-'^`        \               If |AB| minus the cap is still bigger than dR,
        //                    _.-'^`               \ R2           the edge AB is considered central. It's then
        //              _.-'^` \              _.-'\`\             significant compared to the edges around it.
        //        _.-'^`        \R1     _.-'^`     '`\ dR
        //  _.-'^`a/2            \_.-'^`a             \           Line AR2 is parallel to the polygon contour.
        //  `^'-._````````````````A```````````v````````B```````   dR is the remaining diameter at B.
        //        `^'-._                     dD = |AB|            As a result, AB is less often central if the polygon
        //              `^'-._                                    corner is obtuse.
        //                             sin a = dR / dD
        const F: &str = "SkeletalTrapezoidation::updateIsCentral";

        let outer_edge_filter_length: Coord = self.beading_strategy.get_transition_thickness(0) / 2;

        // = cos(bisector_angle / 2)
        #[allow(clippy::cast_possible_truncation)] // `double` to `float`, rounded.
        let cap = orca_crt::sin(self.beading_strategy.get_transitioning_angle() * 0.5) as f32;
        for edge in self.edge_keys() {
            let Some(twin) = self.graph.edges[edge].twin else {
                log(
                    Severity::Warning,
                    "Encountered a Voronoi edge without twin!",
                );
                continue;
            };
            if self.graph.edges[twin].data.central_is_set() {
                let central = self.graph.edges[twin].data.is_central();
                self.set_is_central(edge, central);
            } else if self.graph.edges[edge].data.edge_type == EdgeType::ExtraVd {
                self.set_is_central(edge, false);
            } else {
                let from = self.edge_from(edge, F);
                let to = self.edge_to(edge, F);
                if orca_stl::max(self.dist(from), self.dist(to)) < outer_edge_filter_length {
                    self.set_is_central(edge, false);
                } else {
                    let a = self.p(from);
                    let b = self.p(to);
                    let ab = b - a;
                    let d_r: Coord = self.dist(to).wrapping_sub(self.dist(from)).wrapping_abs();
                    let d_d: Coord = norm_i64(ab);
                    // `dR < dD * cap`: both `int64_t`s converted to `float`.
                    #[allow(clippy::cast_precision_loss)]
                    let central = (d_r as f32) < (d_d as f32) * cap;
                    self.set_is_central(edge, central);
                }
            }
        }
    }

    /// `filterCentral(max_length)`: for each edge that ends a central region at a node that is
    /// and is not a local maximum (never: the C++'s test is `x && !x`), the region behind it.
    pub fn filter_central(&mut self, max_length: Coord) {
        const F: &str = "SkeletalTrapezoidation::filterCentral";
        for edge in self.edge_keys() {
            if self.is_end_of_central(edge)
                && self.graph.is_local_maximum(self.edge_to(edge, F), false)
                && !self.graph.is_local_maximum(self.edge_to(edge, F), false)
            {
                let twin = self.edge_twin(edge, F);
                self.filter_central_from(twin, 0, max_length);
            }
        }
    }

    /// `filterCentral(starting_edge, traveled_dist, max_length)`: unmarks the central region
    /// reached through `starting_edge` when it ends within `max_length` without a local maximum;
    /// returns whether it did, on the way back out of the recursion.
    pub fn filter_central_from(
        &mut self,
        starting_edge: EdgeId,
        traveled_dist: Coord,
        max_length: Coord,
    ) -> bool {
        const F: &str = "SkeletalTrapezoidation::filterCentral";
        let length = self.edge_length(starting_edge, F);
        if traveled_dist.wrapping_add(length) > max_length {
            return false;
        }

        let mut should_dissolve = true; // Should we unmark this as central and propagate that?
        let mut next_edge = self.graph.edges[starting_edge].next;
        while let Some(ne) = next_edge {
            if Some(ne) == self.graph.edges[starting_edge].twin {
                break;
            }
            if self.is_central(ne) {
                should_dissolve &=
                    self.filter_central_from(ne, traveled_dist.wrapping_add(length), max_length);
            }
            next_edge = self.next_round(ne, F);
        }

        // Don't filter central regions with a local maximum!
        should_dissolve &= !self
            .graph
            .is_local_maximum(self.edge_to(starting_edge, F), false);
        if should_dissolve {
            self.set_is_central(starting_edge, false);
            let twin = self.edge_twin(starting_edge, F);
            self.set_is_central(twin, false);
        }
        should_dissolve
    }

    /// `filterOuterCentral()`: unmarks the edges on the outline as central. Domain A never calls
    /// it (`generateToolpaths` with `filter_outermost_central_edges`): it stops
    /// (`filter-outer-central`).
    pub fn filter_outer_central(&mut self) {
        stop(
            "SkeletalTrapezoidation::filterOuterCentral",
            FILTER_OUTER_CENTRAL,
        );
    }

    /// `updateBeadCount()`: the optimal bead count of its width at the end of each central edge,
    /// and at each local maximum, whose missing distance to the outline is first taken from its
    /// neighbours.
    pub fn update_bead_count(&mut self) {
        const F: &str = "SkeletalTrapezoidation::updateBeadCount";
        for edge in self.edge_keys() {
            if self.is_central(edge) {
                let to = self.edge_to(edge, F);
                self.graph.nodes[to].data.bead_count = self
                    .beading_strategy
                    .get_optimal_bead_count(self.dist(to).wrapping_mul(2));
            }
        }

        // Fix bead count at locally maximal R, also for central regions!! See TODO s in
        // generateTransitionEnd(.)
        let nodes: Vec<NodeId> = self.graph.nodes.keys().collect();
        for node in nodes {
            if self.graph.is_local_maximum(node, false) {
                if self.dist(node) < 0 {
                    log(
                        Severity::Warning,
                        "Distance to boundary not yet computed for local maximum!",
                    );
                    self.graph.nodes[node].data.distance_to_boundary = Coord::MAX;
                    let incident_edge = self.graph.nodes[node].incident_edge;
                    let mut edge = req(incident_edge, F);
                    loop {
                        let to = self.edge_to(edge, F);
                        let from = self.edge_from(edge, F);
                        let through = self
                            .dist(to)
                            .wrapping_add(norm_i64(self.p(from) - self.p(to)));
                        self.graph.nodes[node].data.distance_to_boundary =
                            orca_stl::min(self.dist(node), through);
                        let next = self.next_round(edge, F);
                        if next == incident_edge {
                            break;
                        }
                        edge = req(next, F);
                    }
                }
                let bead_count = self
                    .beading_strategy
                    .get_optimal_bead_count(self.dist(node).wrapping_mul(2));
                self.graph.nodes[node].data.bead_count = bead_count;
            }
        }
    }

    /// `filterNoncentralRegions()`: from each end of a central region, the upward edges to the
    /// next central region become central when that region has the same bead count, or one more
    /// or less within 0.4 mm.
    pub fn filter_noncentral_regions(&mut self) {
        const F: &str = "SkeletalTrapezoidation::filterNoncentralRegions";
        for edge in self.edge_keys() {
            if !self.is_end_of_central(edge) {
                continue;
            }
            let to = self.edge_to(edge, F);
            if self.bead_count(to) < 0 && self.dist(to) != 0 {
                log(
                    Severity::Warning,
                    "Encountered an uninitialized bead at the boundary!",
                );
            }
            let max_dist: Coord = scaled(0.4);
            self.filter_noncentral_regions_from(edge, self.bead_count(to), 0, max_dist);
        }
    }

    /// `filterNoncentralRegions(to_edge, bead_count, traveled_dist, max_dist)`: follows the one
    /// upward edge from `to_edge`'s `to` node; returns whether it was made central (with its
    /// node's optimal bead count and a transition ratio of 0).
    pub fn filter_noncentral_regions_from(
        &mut self,
        to_edge: EdgeId,
        bead_count: Coord,
        traveled_dist: Coord,
        max_dist: Coord,
    ) -> bool {
        const F: &str = "SkeletalTrapezoidation::filterNoncentralRegions";
        let r = self.dist(self.edge_to(to_edge, F));

        let to_edge_twin = self.graph.edges[to_edge].twin;
        let mut next_edge = self.graph.edges[to_edge].next;
        while let Some(ne) = next_edge {
            if Some(ne) == to_edge_twin {
                break;
            }
            let ne_to = self.edge_to(ne, F);
            let ne_from = self.edge_from(ne, F);
            if self.dist(ne_to) >= r
                || crate::skeletal_trapezoidation_graph::shorter_then(
                    self.p(ne_to) - self.p(ne_from),
                    scaled(0.01),
                )
            {
                break; // Only walk upward
            }
            next_edge = self.next_round(ne, F);
        }
        let Some(next_edge) = next_edge else {
            return false;
        };
        if Some(next_edge) == to_edge_twin {
            return false;
        }

        let ne_to = self.edge_to(next_edge, F);
        let ne_from = self.edge_from(next_edge, F);
        let length: Coord = norm_i64(self.p(ne_to) - self.p(ne_from));

        let dissolve = if self.bead_count(ne_to) == bead_count {
            true
        } else if self.bead_count(ne_to) < 0 {
            self.filter_noncentral_regions_from(
                next_edge,
                bead_count,
                traveled_dist.wrapping_add(length),
                max_dist,
            )
        } else {
            // Upward bead count is different
            // Dissolve if two central regions with different bead count are closer together than
            // the max_dist (= transition distance)
            traveled_dist.wrapping_add(length) < max_dist
                && self
                    .bead_count(ne_to)
                    .wrapping_sub(bead_count)
                    .wrapping_abs()
                    == 1
        };

        if dissolve {
            self.set_is_central(next_edge, true);
            let twin = self.edge_twin(next_edge, F);
            self.set_is_central(twin, true);
            self.graph.nodes[ne_to].data.bead_count = self
                .beading_strategy
                .get_optimal_bead_count(self.dist(ne_to).wrapping_mul(2));
            self.graph.nodes[ne_to].data.transition_ratio = 0.0;
        }
        // Dissolving only depend on the one edge going upward. There cannot be multiple edges
        // going upward.
        dissolve
    }

    /// `generateTransitioningRibs()`: the transition middles, filtered, their ends, and the ends
    /// applied as ribs (`applyTransitions`, the piece arachne-4's). The two lists of storage live
    /// until it returns; the edges' weak pointers to them die then.
    pub fn generate_transitioning_ribs(&mut self) {
        // Store the upward edges to the transitions.
        // We only store the halfedge for which the distance_to_boundary is higher at the end than
        // at the beginning.
        let mut edge_transitions: PtrVector<TransitionMiddles> = Vec::new();
        self.generate_transition_mids(&mut edge_transitions);

        // The loop that checks a transition between nodes of different bead counts holds only an
        // assert: nothing of it runs in the release build.

        self.filter_transition_mids();

        // We only map the half edge in the upward direction. mapped items are not sorted
        let mut edge_transition_ends: PtrVector<TransitionEnds> = Vec::new();
        self.generate_all_transition_ends(&mut edge_transition_ends);

        self.apply_transitions(&mut edge_transition_ends);
        // Note that the shared pointer lists will be out of scope and thus destroyed here, since
        // the remaining refs are weak_ptr.
    }

    /// `generateTransitionMids(edge_transitions)`: on each central edge going up between two
    /// different bead counts, a transition middle for each bead count between, at the position
    /// where its transition thickness falls; the lists are stored in `edge_transitions` and the
    /// edges point to them.
    pub fn generate_transition_mids(
        &mut self,
        edge_transitions: &mut PtrVector<TransitionMiddles>,
    ) {
        const F: &str = "SkeletalTrapezoidation::generateTransitionMids";
        for edge in self.edge_keys() {
            if !self.is_central(edge) {
                // Only central regions introduce transitions
                continue;
            }
            let from = self.edge_from(edge, F);
            let to = self.edge_to(edge, F);
            let start_r: Coord = self.dist(from);
            let end_r: Coord = self.dist(to);
            // `int` from `coord_t`: the low 32 bits.
            #[allow(clippy::cast_possible_truncation)]
            let start_bead_count = self.bead_count(from) as i32;
            #[allow(clippy::cast_possible_truncation)]
            let end_bead_count = self.bead_count(to) as i32;

            if start_r == end_r {
                // No transitions occur when both end points have the same distance_to_boundary
                if self.bead_count(from) != self.bead_count(to) {
                    log(
                        Severity::Warning,
                        &format!(
                            "Bead count {} is different from {} even though distance to boundary is the same.",
                            self.bead_count(from),
                            self.bead_count(to)
                        ),
                    );
                }
                continue;
            } else if start_r > end_r {
                // Only consider those half-edges which are going from a lower to a higher
                // distance_to_boundary
                continue;
            }

            if self.bead_count(from) == self.bead_count(to) {
                // No transitions should occur according to the enforced bead counts
                continue;
            }

            if i64::from(start_bead_count)
                > self
                    .beading_strategy
                    .get_optimal_bead_count(start_r.wrapping_mul(2))
                || i64::from(end_bead_count)
                    > self
                        .beading_strategy
                        .get_optimal_bead_count(end_r.wrapping_mul(2))
            {
                // Wasn't the case earlier in this function because of already introduced
                // transitions
                log(Severity::Error, "transitioning segment overlap! (?)");
            }
            if start_r >= end_r {
                log(
                    Severity::Warning,
                    &format!(
                        "Transitioning the wrong way around! This function expects to transition from small R to big R, but was transitioning from {start_r} to {end_r}"
                    ),
                );
            }
            let edge_size: Coord = norm_i64(self.p(from) - self.p(to));
            let mut transition_lower_bead_count = start_bead_count;
            while transition_lower_bead_count < end_bead_count {
                let mut mid_r: Coord = self
                    .beading_strategy
                    .get_transition_thickness(i64::from(transition_lower_bead_count))
                    / 2;
                if mid_r > end_r {
                    log(
                        Severity::Error,
                        "transition on segment lies outside of segment!",
                    );
                    mid_r = end_r;
                }
                if mid_r < start_r {
                    log(
                        Severity::Error,
                        "transition on segment lies outside of segment!",
                    );
                    mid_r = start_r;
                }
                let mid_pos: Coord = edge_size
                    .wrapping_mul(mid_r.wrapping_sub(start_r))
                    .wrapping_div(end_r.wrapping_sub(start_r));

                if mid_pos < 0 || mid_pos > edge_size {
                    log(
                        Severity::Warning,
                        "Transition mid is out of bounds of the edge.",
                    );
                }
                let mut transitions = self.graph.edges[edge].data.get_transitions();
                let ignore_empty = true;
                if !self.graph.edges[edge].data.has_transitions(ignore_empty) {
                    let storage: Shared<TransitionMiddles> =
                        Rc::new(RefCell::new(TransitionMiddles::new()));
                    edge_transitions.push(storage);
                    let back = edge_transitions.last().expect("the storage just pushed");
                    self.graph.edges[edge].data.set_transitions(back); // initialization
                    transitions = self.graph.edges[edge].data.get_transitions();
                }
                req(transitions, F)
                    .borrow_mut()
                    .push_back(TransitionMiddle::new(
                        mid_pos,
                        transition_lower_bead_count,
                        mid_r,
                    ));
                transition_lower_bead_count += 1;
            }
        }
    }

    /// `filterTransitionMids()`: on each edge with transitions, the last and then the first
    /// transition is dissolved when nearby transitions of opposite effect are (and those with it)
    /// or when the central region ends within its half length.
    pub fn filter_transition_mids(&mut self) {
        const F: &str = "SkeletalTrapezoidation::filterTransitionMids";
        for edge in self.edge_keys() {
            if !self.graph.edges[edge].data.has_transitions(false) {
                continue;
            }
            let transitions = req(self.graph.edges[edge].data.get_transitions(), F);

            // The C++ asserts how the transitions are stored here.

            let a = self.p(self.edge_from(edge, F));
            let b = self.p(self.edge_to(edge, F));
            let ab = b - a;
            let ab_size: Coord = norm_i64(ab);

            let mut going_up = true;
            let back = back_of(&transitions, F);
            let to_be_dissolved_back = self.dissolve_nearby_transitions(
                edge,
                back,
                ab_size.wrapping_sub(back.pos),
                self.transition_filter_dist,
                going_up,
            );
            let mut should_dissolve_back = !to_be_dissolved_back.is_empty();
            for r in &to_be_dissolved_back {
                let back = back_of(&transitions, F);
                self.dissolve_bead_count_region(
                    edge,
                    i64::from(back.lower_bead_count.wrapping_add(1)),
                    i64::from(back.lower_bead_count),
                );
                self.erase_transition(*r, F);
            }

            {
                let back = back_of(&transitions, F);
                let trans_bead_count: Coord = i64::from(back.lower_bead_count);
                // `(1.0 - float) * coord_t`, a `double`, truncated.
                #[allow(clippy::cast_precision_loss)]
                let upper_transition_half_length: Coord = orca_crt::f64_to_i64(
                    (1.0 - f64::from(
                        self.beading_strategy
                            .get_transition_anchor_pos(trans_bead_count),
                    )) * self
                        .beading_strategy
                        .get_transitioning_length(trans_bead_count) as f64,
                );
                should_dissolve_back |= self.filter_end_of_central_transition(
                    edge,
                    ab_size.wrapping_sub(back.pos),
                    upper_transition_half_length,
                    trans_bead_count,
                );
            }

            if should_dissolve_back {
                Self::pop_transition(&transitions, false, F);
            }
            if transitions.borrow().is_empty() {
                // FilterEndOfCentralTransition gives inconsistent new bead count when executing
                // for the same transition in two directions.
                continue;
            }

            going_up = false;
            let twin = self.edge_twin(edge, F);
            let front = front_of(&transitions, F);
            let to_be_dissolved_front = self.dissolve_nearby_transitions(
                twin,
                front,
                front.pos,
                self.transition_filter_dist,
                going_up,
            );
            let mut should_dissolve_front = !to_be_dissolved_front.is_empty();
            for r in &to_be_dissolved_front {
                let front = front_of(&transitions, F);
                self.dissolve_bead_count_region(
                    twin,
                    i64::from(front.lower_bead_count),
                    i64::from(front.lower_bead_count.wrapping_add(1)),
                );
                self.erase_transition(*r, F);
            }

            {
                let front = front_of(&transitions, F);
                let trans_bead_count: Coord = i64::from(front.lower_bead_count);
                // `float * coord_t`, a `float`, truncated.
                #[allow(clippy::cast_precision_loss)]
                let lower_transition_half_length: Coord = orca_crt::f32_to_i64(
                    self.beading_strategy
                        .get_transition_anchor_pos(trans_bead_count)
                        * self
                            .beading_strategy
                            .get_transitioning_length(trans_bead_count)
                            as f32,
                );
                should_dissolve_front |= self.filter_end_of_central_transition(
                    twin,
                    front.pos,
                    lower_transition_half_length,
                    trans_bead_count.wrapping_add(1),
                );
            }

            if should_dissolve_front {
                Self::pop_transition(&transitions, true, F);
            }
            if transitions.borrow().is_empty() {
                // FilterEndOfCentralTransition gives inconsistent new bead count when executing
                // for the same transition in two directions.
                continue;
            }
        }
    }

    /// `ref.edge->data.getTransitions()->erase(ref.transition_it)`; a dead list (the C++ follows
    /// the null `shared_ptr`) or a transition no longer in it stops.
    fn erase_transition(&mut self, r: TransitionMidRef, function: &'static str) {
        let list = req(self.graph.edges[r.edge].data.get_transitions(), function);
        let mut l = list.borrow_mut();
        if !l.contains(r.transition_it) {
            stop(function, TRANSITIONS_EMPTY);
        }
        l.erase(r.transition_it);
    }

    /// `pop_front()` or `pop_back()` of a transition list; an empty one stops.
    fn pop_transition(list: &Shared<TransitionMiddles>, front: bool, function: &'static str) {
        let mut l = list.borrow_mut();
        let k = if front { l.front() } else { l.back() };
        match k {
            Some(k) => {
                l.erase(k);
            }
            None => stop(function, TRANSITIONS_EMPTY),
        }
    }

    /// `dissolveNearbyTransitions(edge_to_start, origin_transition, traveled_dist, max_dist,
    /// going_up)`: the transitions of the same lower bead count within `max_dist` of the origin
    /// in every central direction from `edge_to_start`'s `to` node, when the width deviation of
    /// dissolving them stays within the allowed deviation; empty when one direction has none.
    pub fn dissolve_nearby_transitions(
        &self,
        edge_to_start: EdgeId,
        origin_transition: TransitionMiddle,
        traveled_dist: Coord,
        max_dist: Coord,
        going_up: bool,
    ) -> Vec<TransitionMidRef> {
        const F: &str = "SkeletalTrapezoidation::dissolveNearbyTransitions";
        let mut to_be_dissolved: Vec<TransitionMidRef> = Vec::new();
        if traveled_dist > max_dist {
            return to_be_dissolved;
        }

        let mut should_dissolve = true;
        let start_twin = self.graph.edges[edge_to_start].twin;
        let mut next = self.graph.edges[edge_to_start].next;
        while let Some(edge) = next {
            if Some(edge) == start_twin {
                break;
            }
            if self.is_central(edge) {
                let a = self.p(self.edge_from(edge, F));
                let b = self.p(self.edge_to(edge, F));
                let ab = b - a;
                let ab_size: Coord = norm_i64(ab);
                let is_aligned = self.graph.is_upward(edge);
                let aligned_edge = if is_aligned {
                    Some(edge)
                } else {
                    self.graph.edges[edge].twin
                };
                let mut seen_transition_on_this_edge = false;

                let origin_radius: Coord = origin_transition.feature_radius;
                let radius_here: Coord = self.dist(self.edge_from(edge, F));
                let dissolve_result_is_odd =
                    (origin_transition.lower_bead_count % 2 != 0) == going_up;
                // times by two because the deviation happens at both sides of the significant
                // edge
                let width_deviation: Coord = origin_radius
                    .wrapping_sub(radius_here)
                    .wrapping_abs()
                    .wrapping_mul(2);
                // assume the deviation will be split over either 1 or 2 lines, i.e. assume
                // wall_distribution_count = 1
                let line_width_deviation: Coord = if dissolve_result_is_odd {
                    width_deviation
                } else {
                    width_deviation / 2
                };
                if line_width_deviation > self.allowed_filter_deviation {
                    should_dissolve = false;
                }

                if should_dissolve {
                    let aligned = req(aligned_edge, F);
                    if self.graph.edges[aligned].data.has_transitions(false) {
                        let transitions = req(self.graph.edges[aligned].data.get_transitions(), F);
                        let list = transitions.borrow();
                        // Note: this is not necessarily iterating in the traveling direction!
                        for (transition_it, t) in list.iter() {
                            // Check whether we should dissolve
                            let pos = if is_aligned {
                                t.pos
                            } else {
                                ab_size.wrapping_sub(t.pos)
                            };
                            if traveled_dist.wrapping_add(pos) < max_dist
                                && t.lower_bead_count == origin_transition.lower_bead_count
                            {
                                // Only dissolve local optima. (The C++ asserts that
                                // consecutive transitions both in- or decreasing in bead count
                                // are never closer together than the transition distance.)
                                to_be_dissolved.push(TransitionMidRef::new(aligned, transition_it));
                                seen_transition_on_this_edge = true;
                            }
                        }
                    }
                }
                if should_dissolve && !seen_transition_on_this_edge {
                    let to_be_dissolved_here = self.dissolve_nearby_transitions(
                        edge,
                        origin_transition,
                        traveled_dist.wrapping_add(ab_size),
                        max_dist,
                        going_up,
                    );
                    if to_be_dissolved_here.is_empty() {
                        // The region is too long to be dissolved in this direction, so it cannot
                        // be dissolved in any direction.
                        to_be_dissolved.clear();
                        return to_be_dissolved;
                    }
                    // Transfer to_be_dissolved_here into to_be_dissolved
                    to_be_dissolved.extend(to_be_dissolved_here);
                    should_dissolve = should_dissolve && !to_be_dissolved.is_empty();
                }
            }
            next = self.next_round(edge, F);
        }

        if !should_dissolve {
            to_be_dissolved.clear();
        }

        to_be_dissolved
    }

    /// `dissolveBeadCountRegion(edge_to_start, from_bead_count, to_bead_count)`: the region of
    /// central edges reached from `edge_to_start` whose nodes have `from_bead_count` gets
    /// `to_bead_count`.
    pub fn dissolve_bead_count_region(
        &mut self,
        edge_to_start: EdgeId,
        from_bead_count: Coord,
        to_bead_count: Coord,
    ) {
        const F: &str = "SkeletalTrapezoidation::dissolveBeadCountRegion";
        let to = self.edge_to(edge_to_start, F);
        if self.bead_count(to) != from_bead_count {
            return;
        }

        self.graph.nodes[to].data.bead_count = to_bead_count;
        let mut next = self.graph.edges[edge_to_start].next;
        while let Some(edge) = next {
            if Some(edge) == self.graph.edges[edge_to_start].twin {
                break;
            }
            if self.is_central(edge) {
                self.dissolve_bead_count_region(edge, from_bead_count, to_bead_count);
            }
            next = self.next_round(edge, F);
        }
    }

    /// `filterEndOfCentralTransition(edge_to_start, traveled_dist, max_dist,
    /// replacing_bead_count)`: when a central region ends within `max_dist` beyond
    /// `edge_to_start`, the nodes on the way get `replacing_bead_count`; returns whether
    /// `edge_to_start`'s `to` node did.
    pub fn filter_end_of_central_transition(
        &mut self,
        edge_to_start: EdgeId,
        traveled_dist: Coord,
        max_dist: Coord,
        replacing_bead_count: Coord,
    ) -> bool {
        const F: &str = "SkeletalTrapezoidation::filterEndOfCentralTransition";
        if traveled_dist > max_dist {
            return false;
        }

        let mut is_end_of_central = true;
        let mut should_dissolve = false;
        let mut next = self.graph.edges[edge_to_start].next;
        while let Some(next_edge) = next {
            if Some(next_edge) == self.graph.edges[edge_to_start].twin {
                break;
            }
            if self.is_central(next_edge) {
                let to = self.edge_to(next_edge, F);
                let from = self.edge_from(next_edge, F);
                let length: Coord = norm_i64(self.p(to) - self.p(from));
                should_dissolve |= self.filter_end_of_central_transition(
                    next_edge,
                    traveled_dist.wrapping_add(length),
                    max_dist,
                    replacing_bead_count,
                );
                is_end_of_central = false;
            }
            next = self.next_round(next_edge, F);
        }
        if is_end_of_central && traveled_dist < max_dist {
            should_dissolve = true;
        }

        if should_dissolve {
            let to = self.edge_to(edge_to_start, F);
            self.graph.nodes[to].data.bead_count = replacing_bead_count;
        }
        should_dissolve
    }

    /// `generateAllTransitionEnds(edge_transition_ends)`: the ends of every transition middle of
    /// every edge, in the edges' and the lists' order; new lists are stored in
    /// `edge_transition_ends`.
    pub fn generate_all_transition_ends(
        &mut self,
        edge_transition_ends: &mut PtrVector<TransitionEnds>,
    ) {
        const F: &str = "SkeletalTrapezoidation::generateAllTransitionEnds";
        for edge in self.edge_keys() {
            if !self.graph.edges[edge].data.has_transitions(false) {
                continue;
            }
            let transition_positions = req(self.graph.edges[edge].data.get_transitions(), F);

            // The list is walked while the ends are made, as the C++'s range loop does.
            let mut it = transition_positions.borrow().front();
            while let Some(k) = it {
                let transition_middle = transition_positions.borrow()[k];
                self.generate_transition_ends(
                    edge,
                    transition_middle.pos,
                    i64::from(transition_middle.lower_bead_count),
                    edge_transition_ends,
                );
                it = transition_positions.borrow().next(k);
            }
        }
    }

    /// `generateTransitionEnds(edge, mid_pos, lower_bead_count, edge_transition_ends)`: the lower
    /// end of a transition, down the twin from the middle by its anchor's share of the transition
    /// length, and its upper end, up the edge by the rest.
    pub fn generate_transition_ends(
        &mut self,
        edge: EdgeId,
        mid_pos: Coord,
        lower_bead_count: Coord,
        edge_transition_ends: &mut PtrVector<TransitionEnds>,
    ) {
        const F: &str = "SkeletalTrapezoidation::generateTransitionEnds";
        let a = self.p(self.edge_from(edge, F));
        let b = self.p(self.edge_to(edge, F));
        let ab = b - a;
        let ab_size: Coord = norm_i64(ab);

        let transition_length: Coord = self
            .beading_strategy
            .get_transitioning_length(lower_bead_count);
        let transition_mid_position: f32 = self
            .beading_strategy
            .get_transition_anchor_pos(lower_bead_count);
        const INNER_BEAD_WIDTH_RATIO_AFTER_TRANSITION: f32 = 1.0;

        const START_REST: Coord = 0;
        let mid_rest: f32 = transition_mid_position * INNER_BEAD_WIDTH_RATIO_AFTER_TRANSITION;
        const END_REST: f32 = INNER_BEAD_WIDTH_RATIO_AFTER_TRANSITION;

        {
            // Lower bead count transition end
            let start_pos: Coord = ab_size.wrapping_sub(mid_pos);
            // `float * int64_t`, a `float`, truncated.
            #[allow(clippy::cast_precision_loss)]
            let transition_half_length: Coord =
                orca_crt::f32_to_i64(transition_mid_position * transition_length as f32);
            let end_pos: Coord = start_pos.wrapping_add(transition_half_length);
            let twin = self.edge_twin(edge, F);
            #[allow(clippy::cast_precision_loss)]
            self.generate_transition_end(
                twin,
                start_pos,
                end_pos,
                transition_half_length,
                f64::from(mid_rest),
                START_REST as f64,
                lower_bead_count,
                edge_transition_ends,
            );
        }

        {
            // Upper bead count transition end
            let start_pos: Coord = mid_pos;
            // `(1.0 - float) * coord_t`, a `double`, truncated.
            #[allow(clippy::cast_precision_loss)]
            let transition_half_length: Coord = orca_crt::f64_to_i64(
                (1.0 - f64::from(transition_mid_position)) * transition_length as f64,
            );
            let end_pos: Coord = mid_pos.wrapping_add(transition_half_length);
            // The release build has no `DEBUG`, so the result is not checked.
            self.generate_transition_end(
                edge,
                start_pos,
                end_pos,
                transition_half_length,
                f64::from(mid_rest),
                f64::from(END_REST),
                lower_bead_count,
                edge_transition_ends,
            );
        }
    }

    /// `generateTransitionEnd(edge, start_pos, end_pos, transition_half_length, start_rest,
    /// end_rest, lower_bead_count, edge_transition_ends)`: one end of a transition, put on the
    /// upward half of the edge where it falls, or carried over the central edges beyond, setting
    /// the passed node's transition ratio and bead count; returns whether the edge goes down
    /// (towards a thinner region).
    #[allow(clippy::too_many_arguments)] // The C++'s eight parameters.
    pub fn generate_transition_end(
        &mut self,
        edge: EdgeId,
        start_pos: Coord,
        end_pos: Coord,
        transition_half_length: Coord,
        start_rest: f64,
        end_rest: f64,
        lower_bead_count: Coord,
        edge_transition_ends: &mut PtrVector<TransitionEnds>,
    ) -> bool {
        const F: &str = "SkeletalTrapezoidation::generateTransitionEnd";
        let a = self.p(self.edge_from(edge, F));
        let b = self.p(self.edge_to(edge, F));
        let ab = b - a;
        let ab_size: Coord = norm_i64(ab); // TODO: prevent recalculation of these values

        if start_pos > ab_size {
            log(
                Severity::Warning,
                "Start position of edge is beyond edge range.",
            );
        }

        let going_up = end_rest > start_rest;

        if !self.is_central(edge) {
            log(
                Severity::Warning,
                "This function shouldn't generate ends in or beyond non-central regions.",
            );
            return false;
        }

        if end_pos > ab_size {
            // Recurse on all further edges
            // `end_rest - (start_rest - end_rest) * (end_pos - ab_size) / (start_pos - end_pos)`
            // in `double`, stored as a `float`.
            #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
            let rest = (end_rest
                - (start_rest - end_rest) * end_pos.wrapping_sub(ab_size) as f64
                    / start_pos.wrapping_sub(end_pos) as f64) as f32;

            let mut central_edge_count: Coord = 0;
            let edge_twin = self.graph.edges[edge].twin;
            let mut outgoing = self.graph.edges[edge].next;
            while let Some(o) = outgoing {
                if Some(o) == edge_twin {
                    break;
                }
                if self.is_central(o) {
                    central_edge_count += 1;
                }
                outgoing = self.next_round(o, F);
            }

            let mut is_only_going_down = true;
            let mut has_recursed = false;
            let mut outgoing = self.graph.edges[edge].next;
            while let Some(o) = outgoing {
                if Some(o) == self.graph.edges[edge].twin {
                    break;
                }
                let next = self.next_round(o, F); // Before we change the outgoing edge itself
                if !self.is_central(o) {
                    outgoing = next;
                    continue; // Don't put transition ends in non-central regions
                }
                if central_edge_count > 1
                    && going_up
                    && self.is_going_down(
                        o,
                        0,
                        end_pos
                            .wrapping_sub(ab_size)
                            .wrapping_add(transition_half_length),
                        lower_bead_count,
                    )
                {
                    // We're after a 3-way_all-central_junction-node and going in the direction of
                    // lower bead count; don't introduce a transition end along this central
                    // direction, because this direction is the downward direction while we are
                    // supposed to be [going_up]
                    outgoing = next;
                    continue;
                }
                let is_going_down = self.generate_transition_end(
                    o,
                    0,
                    end_pos.wrapping_sub(ab_size),
                    transition_half_length,
                    f64::from(rest),
                    end_rest,
                    lower_bead_count,
                    edge_transition_ends,
                );
                is_only_going_down &= is_going_down;
                outgoing = next;
                has_recursed = true;
            }
            if !going_up || (has_recursed && !is_only_going_down) {
                let to = self.edge_to(edge, F);
                self.graph.nodes[to].data.transition_ratio = rest;
                self.graph.nodes[to].data.bead_count = lower_bead_count;
            }
            is_only_going_down
        } else {
            // end_pos < ab_size
            // Add transition end point here
            // TODO collapse this parameter into the bool for which it is used here!
            let is_lower_end = end_rest == 0.0;

            let (upward_edge, pos) = if self.graph.is_upward(edge) {
                (edge, end_pos)
            } else {
                (self.edge_twin(edge, F), ab_size.wrapping_sub(end_pos))
            };

            if !self.graph.edges[upward_edge]
                .data
                .has_transition_ends(false)
            {
                // This edge doesn't have a data structure yet for the transition ends. Make one.
                let storage: Shared<TransitionEnds> = Rc::new(RefCell::new(TransitionEnds::new()));
                edge_transition_ends.push(storage);
                let back = edge_transition_ends
                    .last()
                    .expect("the storage just pushed");
                self.graph.edges[upward_edge].data.set_transition_ends(back);
            }
            let transitions = req(self.graph.edges[upward_edge].data.get_transition_ends(), F);
            let mut t = transitions.borrow_mut();

            // Add a transition to it (on the correct side).
            // `int` from `coord_t`: the low 32 bits.
            #[allow(clippy::cast_possible_truncation)]
            let end = TransitionEnd::new(pos, lower_bead_count as i32, is_lower_end);
            let before_front = match t.front() {
                None => true,
                Some(front) => pos < t[front].pos,
            };
            if before_front {
                // Preorder so that sorting later on is faster
                t.push_front(end);
            } else {
                t.push_back(end);
            }
            false
        }
    }

    /// `isGoingDown(outgoing, traveled_dist, max_dist, lower_bead_count)`: whether the edge goes
    /// down (towards a thinner region) within `max_dist`, looking through the central edges beyond
    /// it.
    #[must_use]
    pub fn is_going_down(
        &self,
        outgoing: EdgeId,
        traveled_dist: Coord,
        max_dist: Coord,
        lower_bead_count: Coord,
    ) -> bool {
        const F: &str = "SkeletalTrapezoidation::isGoingDown";
        // NOTE: the logic below is not fully thought through.
        // TODO: take transition mids into account
        let to = self.edge_to(outgoing, F);
        let from = self.edge_from(outgoing, F);
        if self.dist(to) == 0 {
            return true;
        }
        let is_upward = self.dist(to) >= self.dist(from);
        let upward_edge = if is_upward {
            Some(outgoing)
        } else {
            self.graph.edges[outgoing].twin
        };
        if self.bead_count(to) > lower_bead_count.wrapping_add(1) {
            if !self.graph.edges[req(upward_edge, F)]
                .data
                .has_transitions(false)
            {
                log(
                    Severity::Warning,
                    "If the bead count is going down there has to be a transition mid!",
                );
            }
            return false;
        }
        let length: Coord = norm_i64(self.p(to) - self.p(from));
        let upward = req(upward_edge, F);
        if self.graph.edges[upward].data.has_transitions(false) {
            let transition_mids = req(self.graph.edges[upward].data.get_transitions(), F);
            let mid = if is_upward {
                front_of(&transition_mids, F)
            } else {
                back_of(&transition_mids, F)
            };
            if i64::from(mid.lower_bead_count) == lower_bead_count
                && ((is_upward && mid.pos.wrapping_add(traveled_dist) < max_dist)
                    || (!is_upward
                        && length.wrapping_sub(mid.pos).wrapping_add(traveled_dist) < max_dist))
            {
                return true;
            }
        }
        if traveled_dist.wrapping_add(length) > max_dist {
            return false;
        }
        if self.bead_count(to) <= lower_bead_count
            && !(self.bead_count(to) == lower_bead_count
                && f64::from(self.graph.nodes[to].data.transition_ratio) > 0.0)
        {
            return true;
        }

        let mut is_only_going_down = true;
        let mut has_recursed = false;
        let mut next = self.graph.edges[outgoing].next;
        while let Some(n) = next {
            if Some(n) == self.graph.edges[outgoing].twin {
                break;
            }
            if self.is_central(n) {
                let is_going_down = self.is_going_down(
                    n,
                    traveled_dist.wrapping_add(length),
                    max_dist,
                    lower_bead_count,
                );
                is_only_going_down &= is_going_down;
                has_recursed = true;
            }
            next = self.next_round(n, F);
        }
        has_recursed && is_only_going_down
    }

    /// `isEndOfCentral(edge_to)` (SkeletalTrapezoidation.cpp 1336-1355): the edge is central and
    /// no central edge leaves its `to` node (or it has no next edge).
    #[must_use]
    pub fn is_end_of_central(&self, edge_to: EdgeId) -> bool {
        const F: &str = "SkeletalTrapezoidation::isEndOfCentral";
        if !self.is_central(edge_to) {
            return false;
        }
        if self.graph.edges[edge_to].next.is_none() {
            return true;
        }
        let mut next = self.graph.edges[edge_to].next;
        while let Some(edge) = next {
            if Some(edge) == self.graph.edges[edge_to].twin {
                break;
            }
            if self.is_central(edge) {
                return false;
            }
            next = self.next_round(edge, F);
        }
        true
    }

    // ------------------------------------------------------------------ the second half (arachne-4)

    /// `applyTransitions(edge_transition_ends)` (SkeletalTrapezoidation.cpp 1269-1334): each
    /// edge takes its twin's transition ends (measured from its own `from` node), then each edge's
    /// ends, sorted by position, become nodes on it (`insertNode`), except an end within the snap
    /// distance of a node of its bead count, which sets that node's transition ratio to 0.
    ///
    /// Both loops walk the edge list as it grows, as the C++'s range-for does: `insertNode`
    /// appends edges, and the loop reaches them.
    pub fn apply_transitions(&mut self, edge_transition_ends: &mut PtrVector<TransitionEnds>) {
        const F: &str = "SkeletalTrapezoidation::applyTransitions";
        let snap_dist = self.snap_dist();
        let mut it = self.graph.edges.front();
        while let Some(edge) = it {
            let twin = self.edge_twin(edge, F);
            if self.graph.edges[twin].data.has_transition_ends(false) {
                let length = self.edge_length(edge, F);
                let twin_transition_ends = req(self.graph.edges[twin].data.get_transition_ends(), F);
                if !self.graph.edges[edge].data.has_transition_ends(false) {
                    let storage = Rc::new(RefCell::new(TransitionEnds::new()));
                    self.graph.edges[edge].data.set_transition_ends(&storage);
                    edge_transition_ends.push(storage);
                }
                let transition_ends = req(self.graph.edges[edge].data.get_transition_ends(), F);
                let ends: Vec<TransitionEnd> = twin_transition_ends.borrow().values().copied().collect();
                {
                    let mut transition_ends = transition_ends.borrow_mut();
                    for end in ends {
                        transition_ends.push_back(TransitionEnd::new(
                            length.wrapping_sub(end.pos),
                            end.lower_bead_count,
                            end.is_lower_end,
                        ));
                    }
                }
                twin_transition_ends.borrow_mut().clear();
            }
            it = self.graph.edges.next(edge);
        }

        let mut it = self.graph.edges.front();
        while let Some(edge) = it {
            if self.graph.edges[edge].data.has_transition_ends(false) {
                self.apply_transitions_on(edge, snap_dist);
            }
            it = self.graph.edges.next(edge);
        }
    }

    /// The second loop's body of `applyTransitions` on one edge with transition ends.
    fn apply_transitions_on(&mut self, edge: EdgeId, snap_dist: Coord) {
        const F: &str = "SkeletalTrapezoidation::applyTransitions";
        let transitions = req(self.graph.edges[edge].data.get_transition_ends(), F);
        // std::list::sort, a stable merge sort: the ends in a stable order of their positions,
        // written back along the list.
        let sorted: Vec<TransitionEnd> = {
            let mut list = transitions.borrow_mut();
            let mut values: Vec<TransitionEnd> = list.values().copied().collect();
            orca_stl::stable_sort_by(&mut values, |a, b| a.pos < b.pos);
            let keys: Vec<ListIt> = list.keys().collect();
            for (k, v) in keys.into_iter().zip(&values) {
                list[k] = *v;
            }
            values
        };

        let from = self.edge_from(edge, F);
        let to = self.edge_to(edge, F);
        let a = self.p(from);
        let b = self.p(to);
        let ab = b - a;
        let ab_size = norm_i64(ab);

        let mut last_edge_replacing_input = edge;
        for transition_end in sorted {
            let new_node_bead_count = Coord::from(if transition_end.is_lower_end {
                transition_end.lower_bead_count
            } else {
                transition_end.lower_bead_count.wrapping_add(1)
            });
            let end_pos = transition_end.pos;
            let close_node = if end_pos < ab_size / 2 { from } else { to };
            if (end_pos < snap_dist || end_pos > ab_size.wrapping_sub(snap_dist))
                && self.bead_count(close_node) == new_node_bead_count
            {
                self.graph.nodes[close_node].data.transition_ratio = 0.0;
                continue;
            }
            let mid = a + normal(ab, end_pos);

            last_edge_replacing_input =
                self.graph
                    .insert_node(last_edge_replacing_input, mid, new_node_bead_count);
        }
    }

    /// `generateExtraRibs()` (SkeletalTrapezoidation.cpp 1357-1420): on each central upward edge
    /// longer than the discretization step, a node (`insertNode`) where each of the strategy's
    /// nonlinear thicknesses for the `from` node's bead count falls strictly between its ends,
    /// unless it falls within the snap distance of a node of the new node's bead count.
    ///
    /// The loop walks the edge list as it grows, as the C++'s iterator does, and reads the edge's
    /// nodes again for each new node's bead count: `insertNode` makes the edge end at the new node.
    pub fn generate_extra_ribs(&mut self) {
        const F: &str = "SkeletalTrapezoidation::generateExtraRibs";
        let snap_dist = self.snap_dist();
        let mut it = self.graph.edges.front();
        while let Some(edge) = it {
            let skip = !self.is_central(edge) || {
                let from = self.edge_from(edge, F);
                let to = self.edge_to(edge, F);
                shorter_then(self.p(to) - self.p(from), self.discretization_step_size)
                    || self.dist(from) >= self.dist(to)
            };
            if !skip {
                self.generate_extra_ribs_on(edge, snap_dist);
            }
            it = self.graph.edges.next(edge);
        }
    }

    /// The loop's body of `generateExtraRibs` on one edge it does not skip.
    fn generate_extra_ribs_on(&mut self, edge: EdgeId, snap_dist: Coord) {
        const F: &str = "SkeletalTrapezoidation::generateExtraRibs";
        let rib_thicknesses = self
            .beading_strategy
            .get_nonlinear_thicknesses(self.bead_count(self.edge_from(edge, F)));

        if rib_thicknesses.is_empty() {
            return;
        }

        // Preload some variables before [edge] gets changed
        let from = self.edge_from(edge, F);
        let to = self.edge_to(edge, F);
        let a = self.p(from);
        let b = self.p(to);
        let ab = b - a;
        let ab_size = norm_i64(ab);
        let a_r = self.dist(from);
        let b_r = self.dist(to);

        let mut last_edge_replacing_input = edge;
        for rib_thickness in rib_thicknesses {
            if rib_thickness / 2 <= a_r {
                continue;
            }
            if rib_thickness / 2 >= b_r {
                break;
            }

            let new_node_bead_count = orca_stl::min(
                self.bead_count(self.edge_from(edge, F)),
                self.bead_count(self.edge_to(edge, F)),
            );
            let end_pos = ab_size
                .wrapping_mul((rib_thickness / 2).wrapping_sub(a_r))
                .wrapping_div(b_r.wrapping_sub(a_r));
            let close_node = if end_pos < ab_size / 2 { from } else { to };
            if (end_pos < snap_dist || end_pos > ab_size.wrapping_sub(snap_dist))
                && self.bead_count(close_node) == new_node_bead_count
            {
                self.graph.nodes[close_node].data.transition_ratio = 0.0;
                continue;
            }
            let mid = a + normal(ab, end_pos);

            last_edge_replacing_input =
                self.graph
                    .insert_node(last_edge_replacing_input, mid, new_node_bead_count);
        }
    }

    /// The comparator of `generateSegments`' sort of the upward quad middles: higher `to` nodes
    /// first; between two of the same height, a flat one (both ends equidistant) first, two flat
    /// ones by their distance to go up less their length. It is not a strict weak order, so the
    /// order of the sort ([`orca_stl::sort_by`], MSVC's) is part of the result.
    fn upward_quad_mid_less(&self, a: EdgeId, b: EdgeId) -> bool {
        const F: &str = "SkeletalTrapezoidation::generateSegments";
        let a_to = self.edge_to(a, F);
        let b_to = self.edge_to(b, F);
        if self.dist(a_to) == self.dist(b_to) {
            // Ordering between two 'upward' edges of the same distance is important when one of
            // the edges is flat and connected to the other
            let a_from = self.edge_from(a, F);
            let b_from = self.edge_from(b, F);
            if self.dist(a_from) == self.dist(a_to) && self.dist(b_from) == self.dist(b_to) {
                let max = Coord::MAX;
                let a_dist_from_up = orca_stl::min(
                    self.graph.dist_to_go_up(a).unwrap_or(max),
                    self.graph
                        .dist_to_go_up(self.edge_twin(a, F))
                        .unwrap_or(max),
                )
                .wrapping_sub(norm_i64(self.p(a_to) - self.p(a_from)));
                let b_dist_from_up = orca_stl::min(
                    self.graph.dist_to_go_up(b).unwrap_or(max),
                    self.graph
                        .dist_to_go_up(self.edge_twin(b, F))
                        .unwrap_or(max),
                )
                .wrapping_sub(norm_i64(self.p(b_to) - self.p(b_from)));
                return a_dist_from_up < b_dist_from_up;
            } else if self.dist(a_from) == self.dist(a_to) {
                return true; // Edge a might be 'above' edge b
            } else if self.dist(b_from) == self.dist(b_to) {
                return false; // Edge b might be 'above' edge a
            }
            // Ordering is not important
        }
        self.dist(a_to) > self.dist(b_to)
    }

    /// `generateSegments()` (SkeletalTrapezoidation.cpp 1430-1534): the upward quad middles,
    /// sorted; a beading for each node with a positive bead count (two beadings interpolated at
    /// a node inside a transition); the beadings propagated up and down; the junctions along each
    /// upward edge; the junctions connected into toolpaths; and a small circle at each local
    /// maximum with an odd beading. The beadings and the junctions live until it returns, as the
    /// C++'s local `ptr_vector_t`s do; the graph's weak pointers to them die then.
    pub fn generate_segments(&mut self) {
        let mut upward_quad_mids: Vec<EdgeId> = Vec::new();
        for edge in self.edge_keys() {
            let e = &self.graph.edges[edge];
            if e.prev.is_some() && e.next.is_some() && self.graph.is_upward(edge) {
                upward_quad_mids.push(edge);
            }
        }

        orca_stl::sort_by(&mut upward_quad_mids, |&a, &b| {
            self.upward_quad_mid_less(a, b)
        });

        let mut node_beadings: PtrVector<BeadingPropagation> = Vec::new();
        // Store beading
        let node_keys: Vec<NodeId> = self.graph.nodes.keys().collect();
        for node in node_keys {
            let bead_count = self.bead_count(node);
            if bead_count <= 0 {
                continue;
            }
            let thickness = self.dist(node).wrapping_mul(2);
            let transition_ratio = self.graph.nodes[node].data.transition_ratio;
            if transition_ratio == 0.0 {
                let storage = Rc::new(RefCell::new(BeadingPropagation::new(
                    self.beading_strategy.compute(thickness, bead_count),
                )));
                self.graph.nodes[node].data.set_beading(&storage);
                let total_thickness = storage.borrow().beading.total_thickness;
                node_beadings.push(storage);
                if total_thickness != thickness {
                    log(
                        Severity::Warning,
                        "If transitioning to an endpoint (ratio 0), the node should be exactly in the middle.",
                    );
                }
            } else {
                let low_count_beading = self.beading_strategy.compute(thickness, bead_count);
                let high_count_beading = self
                    .beading_strategy
                    .compute(thickness, bead_count.wrapping_add(1));
                let merged = Self::interpolate(
                    &low_count_beading,
                    1.0 - f64::from(transition_ratio),
                    &high_count_beading,
                );
                let total_thickness = merged.total_thickness;
                let storage = Rc::new(RefCell::new(BeadingPropagation::new(merged)));
                self.graph.nodes[node].data.set_beading(&storage);
                node_beadings.push(storage);
                if total_thickness != thickness {
                    log(
                        Severity::Warning,
                        "If merging two beads, the new bead must be exactly in the middle.",
                    );
                }
            }
        }

        self.propagate_beadings_upward(&upward_quad_mids, &mut node_beadings);

        self.propagate_beadings_downward(&upward_quad_mids, &mut node_beadings);

        let mut edge_junctions: PtrVector<LineJunctions> = Vec::new(); // junctions ordered high R to low R
        self.generate_junctions(&mut node_beadings, &mut edge_junctions);

        self.connect_junctions(&mut edge_junctions);

        self.generate_local_maxima_single_beads();

        // The beadings and the junctions die here, as the C++'s locals do.
        drop((node_beadings, edge_junctions));
    }

    /// `getQuadMaxRedgeTo(quad_start_edge)` (SkeletalTrapezoidation.cpp 1536-1559): the edge of
    /// the quad leading to its node of the highest radius (the first of equals), or the edge
    /// before it when that is the quad's last edge and only less than 5 microns higher than its
    /// `from` node. `None` stands for the null pointer the C++ may return.
    #[must_use]
    pub fn get_quad_max_redge_to(&self, quad_start_edge: EdgeId) -> Option<EdgeId> {
        const F: &str = "SkeletalTrapezoidation::getQuadMaxRedgeTo";
        let mut max_r: Coord = -1;
        let mut ret: Option<EdgeId> = None;
        let mut edge = Some(quad_start_edge);
        while let Some(e) = edge {
            let r = self.dist(self.edge_to(e, F));
            if r > max_r {
                max_r = r;
                ret = Some(e);
            }
            edge = self.graph.edges[e].next;
        }

        let r = req(ret, F);
        if self.graph.edges[r].next.is_none()
            && self
                .dist(self.edge_to(r, F))
                .wrapping_sub(scaled(0.005))
                < self.dist(self.edge_from(r, F))
        {
            ret = self.graph.edges[r].prev;
        }
        ret
    }

    /// `propagateBeadingsUpward(upward_quad_mids, node_beadings)` (SkeletalTrapezoidation.cpp
    /// 1561-1589): from the lowest upward quad middle up, a node with no bead count and no
    /// beading takes its lower node's beading, its distance to the bottom source grown by the
    /// edge's length and marked as propagated upward only.
    pub fn propagate_beadings_upward(
        &mut self,
        upward_quad_mids: &[EdgeId],
        node_beadings: &mut PtrVector<BeadingPropagation>,
    ) {
        const F: &str = "SkeletalTrapezoidation::propagateBeadingsUpward";
        for &upward_edge in upward_quad_mids.iter().rev() {
            let to = self.edge_to(upward_edge, F);
            if self.bead_count(to) >= 0 {
                // Don't override local beading
                continue;
            }
            let from = self.edge_from(upward_edge, F);
            let Some(lower_beading) = self.graph.nodes[from].data.get_beading() else {
                // Only propagate if we have something to propagate
                continue;
            };
            if self.graph.nodes[to].data.has_beading() {
                // Only propagate to places where there is place
                continue;
            }
            let length = norm_i64(self.p(to) - self.p(from));
            let mut upper_beading = lower_beading.borrow().clone();
            upper_beading.dist_to_bottom_source =
                upper_beading.dist_to_bottom_source.wrapping_add(length);
            upper_beading.is_upward_propagated_only = true;
            let storage = Rc::new(RefCell::new(upper_beading));
            self.graph.nodes[to].data.set_beading(&storage);
            node_beadings.push(storage);
        }
    }

    /// `propagateBeadingsDownward(upward_quad_mids, node_beadings)` (SkeletalTrapezoidation.cpp
    /// 1591-1612): each upward quad middle that is not central hands its top beading down
    /// ([`SkeletalTrapezoidation::propagate_beadings_downward_edge`]); an equidistant one whose
    /// `from` node alone has a beading hands it the other way, along its twin.
    pub fn propagate_beadings_downward(
        &mut self,
        upward_quad_mids: &[EdgeId],
        node_beadings: &mut PtrVector<BeadingPropagation>,
    ) {
        const F: &str = "SkeletalTrapezoidation::propagateBeadingsDownward";
        for &upward_quad_mid in upward_quad_mids {
            // Transfer beading information to lower nodes
            if !self.is_central(upward_quad_mid) {
                let from = self.edge_from(upward_quad_mid, F);
                let to = self.edge_to(upward_quad_mid, F);
                // for equidistant edge: propagate from known beading to node with unknown beading
                if self.dist(from) == self.dist(to)
                    && self.graph.nodes[from].data.has_beading()
                    && !self.graph.nodes[to].data.has_beading()
                {
                    let twin = self.edge_twin(upward_quad_mid, F);
                    self.propagate_beadings_downward_edge(twin, node_beadings);
                } else {
                    self.propagate_beadings_downward_edge(upward_quad_mid, node_beadings);
                }
            }
        }
    }

    /// `propagateBeadingsDownward(edge_to_peak, node_beadings)` (SkeletalTrapezoidation.cpp
    /// 1614-1660): the `from` node takes the `to` node's beading (made when it has none), its
    /// distance to the top source grown by the edge's length; a `from` node with a beading of
    /// its own keeps a mix of the two, by the share of the propagation distance its bottom source
    /// is away, or the top one past it.
    ///
    /// The top beading may be the bottom one itself (`getOrCreateBeading` may return another
    /// node's beading): it is copied before the bottom one is written, as the C++ reads it before
    /// it assigns.
    pub fn propagate_beadings_downward_edge(
        &mut self,
        edge_to_peak: EdgeId,
        node_beadings: &mut PtrVector<BeadingPropagation>,
    ) {
        const F: &str = "SkeletalTrapezoidation::propagateBeadingsDownward";
        let to = self.edge_to(edge_to_peak, F);
        let from = self.edge_from(edge_to_peak, F);
        let length = norm_i64(self.p(to) - self.p(from));
        let top_storage = self.get_or_create_beading(to, node_beadings);
        let top_beading = top_storage.borrow().clone();
        if top_beading.beading.total_thickness < self.dist(to).wrapping_mul(2) {
            log(
                Severity::Warning,
                "Top bead is beyond the center of the total width.",
            );
        }

        match self.graph.nodes[from].data.get_beading() {
            None => {
                // Set new beading if there is no beading associated with the node yet
                let mut propagated_beading = top_beading;
                propagated_beading.dist_from_top_source =
                    propagated_beading.dist_from_top_source.wrapping_add(length);
                let total_thickness = propagated_beading.beading.total_thickness;
                let storage = Rc::new(RefCell::new(propagated_beading));
                self.graph.nodes[from].data.set_beading(&storage);
                node_beadings.push(storage);
                if total_thickness < self.dist(from).wrapping_mul(2) {
                    log(
                        Severity::Warning,
                        "Propagated bead is beyond the center of the total width.",
                    );
                }
            }
            Some(bottom_storage) => {
                let bottom = bottom_storage.borrow().clone();
                let total_dist = top_beading
                    .dist_from_top_source
                    .wrapping_add(length)
                    .wrapping_add(bottom.dist_to_bottom_source);
                let mut ratio_of_top = f64::from(
                    bottom.dist_to_bottom_source as f32
                        / orca_stl::min(total_dist, self.beading_propagation_transition_dist)
                            as f32,
                );
                ratio_of_top = orca_stl::max(0.0, ratio_of_top);
                if ratio_of_top >= 1.0 {
                    let mut b = top_beading;
                    b.dist_from_top_source = b.dist_from_top_source.wrapping_add(length);
                    *bottom_storage.borrow_mut() = b;
                } else {
                    let merged_beading = Self::interpolate_at(
                        &top_beading.beading,
                        ratio_of_top,
                        &bottom.beading,
                        self.dist(from),
                    );
                    let total_thickness = merged_beading.total_thickness;
                    let mut b = BeadingPropagation::new(merged_beading);
                    b.is_upward_propagated_only = false;
                    *bottom_storage.borrow_mut() = b;
                    if total_thickness < self.dist(from).wrapping_mul(2) {
                        log(
                            Severity::Warning,
                            "Merged bead is beyond the center of the total width.",
                        );
                    }
                }
            }
        }
    }

    /// `interpolate(left, ratio_left_to_whole, right, switching_radius)`
    /// (SkeletalTrapezoidation.cpp 1663-1703): the three-argument mix, unless the inset before
    /// the switching radius disappeared in it, when the mix is taken again at the ratio that puts
    /// that inset at the radius, plus 0.1 (at most 1). A `const` member of the C++ that reads
    /// nothing of the object, so it takes none here.
    ///
    /// It reads the mix and the right beading at the inset before the radius: a right beading
    /// more than one location shorter than the left one stops there (`interpolate-short-right`).
    #[must_use]
    pub fn interpolate_at(
        left: &Beading,
        ratio_left_to_whole: f64,
        right: &Beading,
        switching_radius: Coord,
    ) -> Beading {
        const F: &str = "SkeletalTrapezoidation::interpolate";
        let ret = Self::interpolate(left, ratio_left_to_whole, right);

        // TODO: don't use toolpath locations past the middle!
        // TODO: stretch bead widths and locations of the higher bead count beading to fit in the
        // left over space
        // `size() - 1` in `size_t`, assigned to a `coord_t`: -1 for no location.
        let mut next_inset_idx: Coord = left.toolpath_locations.len().wrapping_sub(1) as Coord;
        while next_inset_idx >= 0 {
            if switching_radius > left.toolpath_locations[next_inset_idx as usize] {
                break;
            }
            next_inset_idx -= 1;
        }
        if next_inset_idx < 0 {
            // There is no next inset, because there is only one
            return ret;
        }
        if next_inset_idx + 1 == left.toolpath_locations.len() as Coord {
            // We cant adjust to fit the next edge because there is no previous one?!
            return ret;
        }
        let idx = next_inset_idx as usize;
        let at = |v: &[Coord]| match v.get(idx) {
            Some(&x) => x,
            None => stop(F, INTERPOLATE_SHORT_RIGHT),
        };
        if at(&ret.toolpath_locations) > switching_radius {
            // One inset disappeared between left and the merged one
            // solve for ratio f:
            // f*l + (1-f)*r = s
            // f*l + r - f*r = s
            // f*(l-r) + r = s
            // f*(l-r) = s - r
            // f = (s-r) / (l-r)
            let right_location = at(&right.toolpath_locations);
            let mut new_ratio = switching_radius.wrapping_sub(right_location) as f32
                / left.toolpath_locations[idx].wrapping_sub(right_location) as f32;
            new_ratio = orca_stl::min(1.0, f64::from(new_ratio) + 0.1) as f32;
            return Self::interpolate(left, f64::from(new_ratio), right);
        }
        ret
    }

    /// `interpolate(left, ratio_left_to_whole, right)` (SkeletalTrapezoidation.cpp 1706-1725):
    /// the thicker beading, its first widths and locations (as many as both have widths) the mix
    /// of the two: the left share in `double`, the right share a `float` (`1 - ratio`), each
    /// product in its own type, the sum truncated; a zero width stays zero. A `const` member of
    /// the C++ that reads nothing of the object, so it takes none here.
    ///
    /// A location past the end of either beading's locations stops there
    /// (`interpolate-short-right`; the strategies give as many locations as widths).
    #[must_use]
    pub fn interpolate(left: &Beading, ratio_left_to_whole: f64, right: &Beading) -> Beading {
        const F: &str = "SkeletalTrapezoidation::interpolate";
        let ratio_right_to_whole = (1.0 - ratio_left_to_whole) as f32;

        let mut ret = if left.total_thickness > right.total_thickness {
            left.clone()
        } else {
            right.clone()
        };
        let mix = |l: Coord, r: Coord| -> Coord {
            let v = ratio_left_to_whole * l as f64 + f64::from(ratio_right_to_whole * r as f32);
            orca_crt::f64_to_i64(v)
        };
        let at = |v: &[Coord], i: usize| match v.get(i) {
            Some(&x) => x,
            None => stop(F, INTERPOLATE_SHORT_RIGHT),
        };
        let n = orca_stl::min(left.bead_widths.len(), right.bead_widths.len());
        for inset_idx in 0..n {
            if left.bead_widths[inset_idx] == 0 || right.bead_widths[inset_idx] == 0 {
                ret.bead_widths[inset_idx] = 0; //0-width wall markers stay 0-width.
            } else {
                ret.bead_widths[inset_idx] =
                    mix(left.bead_widths[inset_idx], right.bead_widths[inset_idx]);
            }
            let location = mix(
                at(&left.toolpath_locations, inset_idx),
                at(&right.toolpath_locations, inset_idx),
            );
            match ret.toolpath_locations.get_mut(inset_idx) {
                Some(x) => *x = location,
                None => stop(F, INTERPOLATE_SHORT_RIGHT),
            }
        }
        ret
    }

    /// `generateJunctions(node_beadings, edge_junctions)` (SkeletalTrapezoidation.cpp
    /// 1727-1802): along each upward edge between different bead counts (or of an unknown one),
    /// a junction for each toolpath location of its top node's beading between its two radii,
    /// from the top down, each in a list stored in `edge_junctions` that the edge points to.
    pub fn generate_junctions(
        &mut self,
        node_beadings: &mut PtrVector<BeadingPropagation>,
        edge_junctions: &mut PtrVector<LineJunctions>,
    ) {
        const F: &str = "SkeletalTrapezoidation::generateJunctions";
        for edge in self.edge_keys() {
            let from = self.edge_from(edge, F);
            let to = self.edge_to(edge, F);
            if self.dist(from) > self.dist(to) {
                // Only consider the upward half-edges
                continue;
            }

            let start_r = self.dist(to); // higher R
            let end_r = self.dist(from); // lower R

            if (self.bead_count(from) == self.bead_count(to) && self.bead_count(from) >= 0)
                || end_r >= start_r
            {
                // No beads to generate
                continue;
            }

            let beading_storage = self.get_or_create_beading(to, node_beadings);
            let storage: Shared<LineJunctions> = Rc::new(RefCell::new(Vec::new()));
            self.graph.edges[edge]
                .data
                .set_extrusion_junctions(&storage); // initialization
            edge_junctions.push(Rc::clone(&storage));
            let mut ret = storage.borrow_mut();
            let propagation = beading_storage.borrow();
            let beading = &propagation.beading;

            if beading.total_thickness < self.dist(to).wrapping_mul(2) {
                log(
                    Severity::Warning,
                    "Generated junction is beyond the center of total width.",
                );
            }

            let a = self.p(to);
            let b = self.p(from);
            let ab = b - a;

            let num_junctions = beading.toolpath_locations.len();
            // Compute starting junction_idx for this segment
            let mut junction_idx = (orca_stl::max(1, beading.toolpath_locations.len()) - 1) / 2;
            while junction_idx < num_junctions {
                let bead_r = beading.toolpath_locations[junction_idx];
                // toolpath_locations computed inside DistributedBeadingStrategy could be off by 1
                // because of rounding errors. In GH issue #8472, these roundings errors caused
                // missing the middle extrusion. Adding small epsilon should help resolve those
                // cases.
                if bead_r <= start_r.wrapping_add(1) {
                    // Junction coinciding with start node is used in this function call
                    break;
                }
                junction_idx = junction_idx.wrapping_sub(1);
            }

            // Robustness against odd segments which might lie just slightly outside of the range
            // due to rounding errors not sure if this is really needed (TODO)
            if junction_idx.wrapping_add(1) < num_junctions
                && beading.toolpath_locations[junction_idx.wrapping_add(1)]
                    <= start_r.wrapping_add(scaled(0.005))
                && beading.total_thickness < start_r.wrapping_add(scaled(0.005))
            {
                junction_idx = junction_idx.wrapping_add(1);
            }

            //When junction_idx underflows, it'll be more than num_junctions too.
            while junction_idx < num_junctions {
                let bead_r = beading.toolpath_locations[junction_idx];
                if bead_r < end_r {
                    // Junction coinciding with a node is handled by the next segment
                    break;
                }
                let k = bead_r.wrapping_sub(start_r);
                let d = end_r.wrapping_sub(start_r);
                let mut junction = a + Point::new(
                    ab.x.wrapping_mul(k).wrapping_div(d),
                    ab.y.wrapping_mul(k).wrapping_div(d),
                );
                if bead_r > start_r.wrapping_sub(scaled(0.005)) {
                    // Snap to start node if it is really close, in order to be able to see 3-way
                    // intersection later on more robustly
                    junction = a;
                }
                ret.push(ExtrusionJunction::new(
                    junction,
                    beading.bead_widths[junction_idx],
                    junction_idx as Coord,
                ));
                junction_idx = junction_idx.wrapping_sub(1);
            }
        }
    }

    /// `getOrCreateBeading(node, node_beadings)` (SkeletalTrapezoidation.cpp 1804-1844): the
    /// node's beading, made when it has none: a node of unknown bead count takes the nearest
    /// beading within 0.1 mm (not set on it), else gets the optimal bead count of twice its
    /// smallest radius through a neighbour (an error logged when no edge of it is central).
    pub fn get_or_create_beading(
        &mut self,
        node: NodeId,
        node_beadings: &mut PtrVector<BeadingPropagation>,
    ) -> Shared<BeadingPropagation> {
        const F: &str = "SkeletalTrapezoidation::getOrCreateBeading";
        if !self.graph.nodes[node].data.has_beading() {
            if self.bead_count(node) == -1 {
                // This bug is due to too small central edges
                let nearby_dist = scaled(0.1);
                if let Some(nearest_beading) = self.get_nearest_beading(node, nearby_dist) {
                    return nearest_beading;
                }

                // Else make a new beading:
                let mut has_central_edge = false;
                let mut first = true;
                let mut dist = Coord::MAX;
                let incident_edge = self.graph.nodes[node].incident_edge;
                let mut edge = incident_edge;
                while let Some(e) = edge {
                    if !(first || edge != incident_edge) {
                        break;
                    }
                    if self.is_central(e) {
                        has_central_edge = true;
                    }
                    let to = self.edge_to(e, F);
                    let from = self.edge_from(e, F);
                    dist = orca_stl::min(
                        dist,
                        self.dist(to)
                            .wrapping_add(norm_i64(self.p(to) - self.p(from))),
                    );
                    first = false;
                    edge = self.next_round(e, F);
                }
                if !has_central_edge {
                    log(Severity::Error, "Unknown beading for non-central node!");
                }
                self.graph.nodes[node].data.bead_count = self
                    .beading_strategy
                    .get_optimal_bead_count(dist.wrapping_mul(2));
            }
            let storage = Rc::new(RefCell::new(BeadingPropagation::new(
                self.beading_strategy
                    .compute(self.dist(node).wrapping_mul(2), self.bead_count(node)),
            )));
            self.graph.nodes[node].data.set_beading(&storage);
            node_beadings.push(storage);
        }
        req(self.graph.nodes[node].data.get_beading(), F)
    }

    /// `getNearestBeading(node, max_dist)` (SkeletalTrapezoidation.cpp 1846-1885): the beading of
    /// the nearest node along the graph, within `max_dist`, searched outward through a heap of
    /// the edges by their distance (`std::priority_queue`, [`orca_stl::PriorityQueue`]) for at
    /// most 1000 steps.
    #[must_use]
    pub fn get_nearest_beading(
        &self,
        node: NodeId,
        max_dist: Coord,
    ) -> Option<Shared<BeadingPropagation>> {
        const F: &str = "SkeletalTrapezoidation::getNearestBeading";
        /// `DistEdge`.
        struct DistEdge {
            edge_to: EdgeId,
            dist: Coord,
        }

        let mut further_edges =
            orca_stl::PriorityQueue::with_compare(|l: &DistEdge, r: &DistEdge| l.dist > r.dist);
        let mut first = true;
        let incident_edge = self.graph.nodes[node].incident_edge;
        let mut outgoing = incident_edge;
        while let Some(o) = outgoing {
            if !(first || outgoing != incident_edge) {
                break;
            }
            further_edges.push(DistEdge {
                edge_to: o,
                dist: norm_i64(self.p(self.edge_to(o, F)) - self.p(self.edge_from(o, F))),
            });
            first = false;
            outgoing = self.next_round(o, F);
        }

        for _counter in 0..SKELETAL_TRAPEZOIDATION_BEAD_SEARCH_MAX {
            // Prevent endless recursion
            if further_edges.is_empty() {
                return None;
            }
            let here = further_edges.pop()?;
            if here.dist > max_dist {
                return None;
            }
            let here_to = self.edge_to(here.edge_to, F);
            if let Some(beading) = self.graph.nodes[here_to].data.get_beading() {
                return Some(beading);
            }
            // recurse
            let here_twin = self.graph.edges[here.edge_to].twin;
            let mut further_edge = self.graph.edges[here.edge_to].next;
            while let Some(f) = further_edge {
                if Some(f) == here_twin {
                    break;
                }
                further_edges.push(DistEdge {
                    edge_to: f,
                    dist: here.dist.wrapping_add(norm_i64(
                        self.p(self.edge_to(f, F)) - self.p(self.edge_from(f, F)),
                    )),
                });
                further_edge = self.next_round(f, F);
            }
        }
        None
    }

    /// `addToolpathSegment(from, to, is_odd, force_new_path, from_is_3way, to_is_3way)`
    /// (SkeletalTrapezoidation.cpp 1887-1932): the segment appended to the last line of its
    /// inset when one of its ends meets that line's end (within 10 microns of place and width,
    /// not at a 3-way intersection), else a new line of the two junctions.
    ///
    /// The last line of an inset with no junction stops (`toolpath-empty-line`): the C++ reads
    /// the last junction of an empty vector, and no line it makes is empty.
    pub fn add_toolpath_segment(
        &mut self,
        from: &ExtrusionJunction,
        to: &ExtrusionJunction,
        is_odd: bool,
        mut force_new_path: bool,
        from_is_3way: bool,
        to_is_3way: bool,
    ) {
        const F: &str = "SkeletalTrapezoidation::addToolpathSegment";
        if from == to {
            return;
        }

        let generated_toolpaths = &mut self.p_generated_toolpaths;

        let inset_idx = from.perimeter_index;
        if inset_idx >= generated_toolpaths.len() {
            generated_toolpaths.resize_with(inset_idx.wrapping_add(1), Vec::new);
        }
        let inset = &mut generated_toolpaths[inset_idx];
        let back_junction = |inset: &VariableWidthLines| -> ExtrusionJunction {
            match inset.last().and_then(|l| l.junctions.last()) {
                Some(&j) => j,
                None => stop(F, TOOLPATH_EMPTY_LINE),
            }
        };
        if inset.is_empty()
            || inset.last().is_some_and(|l| l.is_odd != is_odd)
            || back_junction(inset).perimeter_index != inset_idx
        // inset_idx should always be consistent
        {
            force_new_path = true;
        }
        if !force_new_path
            && shorter_then(back_junction(inset).p - from.p, scaled(0.010))
            && back_junction(inset).w.wrapping_sub(from.w).wrapping_abs() < scaled(0.010)
            && !from_is_3way
        // force new path at 3way intersection
        {
            if let Some(line) = inset.last_mut() {
                line.junctions.push(*to);
            }
        } else if !force_new_path
            && shorter_then(back_junction(inset).p - to.p, scaled(0.010))
            && back_junction(inset).w.wrapping_sub(to.w).wrapping_abs() < scaled(0.010)
            && !to_is_3way
        // force new path at 3way intersection
        {
            if !is_odd {
                log(
                    Severity::Error,
                    "Reversing even wall line causes it to be printed CCW instead of CW!",
                );
            }
            if let Some(line) = inset.last_mut() {
                line.junctions.push(*from);
            }
        } else {
            let mut line = ExtrusionLine::new(inset_idx, is_odd);
            line.junctions.push(*from);
            line.junctions.push(*to);
            inset.push(line);
        }
    }

    /// The junctions an edge points to, copied; a list that is gone stops (`junctions-missing`:
    /// the C++ dereferences the empty `shared_ptr`).
    fn junctions_of(&self, edge: EdgeId, function: &'static str) -> LineJunctions {
        match self.graph.edges[edge].data.get_extrusion_junctions() {
            Some(j) => j.borrow().clone(),
            None => stop(function, JUNCTIONS_MISSING),
        }
    }

    /// The edge's junctions, a new empty list stored in `edge_junctions` first when it has none
    /// (or an empty one), copied.
    fn junctions_or_new(
        &mut self,
        edge: EdgeId,
        edge_junctions: &mut PtrVector<LineJunctions>,
        function: &'static str,
    ) -> LineJunctions {
        if !self.graph.edges[edge].data.has_extrusion_junctions(false) {
            let storage: Shared<LineJunctions> = Rc::new(RefCell::new(Vec::new()));
            self.graph.edges[edge]
                .data
                .set_extrusion_junctions(&storage);
            edge_junctions.push(storage);
        }
        self.junctions_of(edge, function)
    }

    /// `connectJunctions(edge_junctions)` (SkeletalTrapezoidation.cpp 1934-2054): quad by quad,
    /// each polygon domain walked from a quad start the set still holds, the junctions on both
    /// sides of the quad's peak are joined pairwise from the top down into toolpath segments; an
    /// odd single bead segment is made once.
    ///
    /// The quad starts are an `ankerl::unordered_dense::set` whose first element starts each
    /// domain: [`DenseSet`] keeps its order (insertion order, an erased element's place taken by
    /// the last). The odd edges passed are only asked for membership.
    pub fn connect_junctions(&mut self, edge_junctions: &mut PtrVector<LineJunctions>) {
        const F: &str = "SkeletalTrapezoidation::connectJunctions";
        let mut unprocessed_quad_starts: DenseSet<EdgeId> = DenseSet::new();
        for edge in self.edge_keys() {
            if self.graph.edges[edge].prev.is_none() {
                unprocessed_quad_starts.emplace(edge);
            }
        }

        let mut passed_odd_edges: BTreeSet<Option<EdgeId>> = BTreeSet::new();

        while let Some(&poly_domain_start) = unprocessed_quad_starts.first() {
            let mut quad_start = poly_domain_start;
            let mut new_domain_start = true;
            loop {
                let mut quad_end = quad_start;
                while let Some(next) = self.graph.edges[quad_end].next {
                    quad_end = next;
                }

                let edge_to_peak = req(self.get_quad_max_redge_to(quad_start), F);
                // walk down on both sides and connect junctions
                let edge_from_peak = req(self.graph.edges[edge_to_peak].next, F);

                unprocessed_quad_starts.erase(&quad_start);

                // The junctions on the edge(s) from the start of the quad to the node with
                // highest R
                let mut from_junctions = self.junctions_or_new(edge_to_peak, edge_junctions, F);
                let from_peak_twin = self.edge_twin(edge_from_peak, F);
                // The junctions on the edge(s) from the end of the quad to the node with highest
                // R
                let mut to_junctions = self.junctions_or_new(from_peak_twin, edge_junctions, F);
                if let Some(prev) = self.graph.edges[edge_to_peak].prev {
                    let from_prev_junctions = self.junctions_of(prev, F);
                    while let (Some(back), Some(front)) =
                        (from_junctions.last(), from_prev_junctions.first())
                    {
                        if back.perimeter_index > front.perimeter_index {
                            break;
                        }
                        from_junctions.pop();
                    }
                    from_junctions.extend_from_slice(&from_prev_junctions);
                    if self.graph.edges[prev].prev.is_some() {
                        log(
                            Severity::Warning,
                            "The edge we're about to connect is already connected.",
                        );
                    }
                }
                if let Some(next) = self.graph.edges[edge_from_peak].next {
                    let next_twin = self.edge_twin(next, F);
                    let to_next_junctions = self.junctions_of(next_twin, F);
                    while let (Some(back), Some(front)) =
                        (to_junctions.last(), to_next_junctions.first())
                    {
                        if back.perimeter_index > front.perimeter_index {
                            break;
                        }
                        to_junctions.pop();
                    }
                    to_junctions.extend_from_slice(&to_next_junctions);
                    if self.graph.edges[next].next.is_some() {
                        log(
                            Severity::Warning,
                            "The edge we're about to connect is already connected!",
                        );
                    }
                }
                if (from_junctions.len() as i32)
                    .wrapping_sub(to_junctions.len() as i32)
                    .wrapping_abs()
                    > 1
                {
                    log(
                        Severity::Warning,
                        &format!(
                            "Can't create a transition when connecting two perimeters where the number of beads differs too much! {} vs. {}",
                            from_junctions.len(),
                            to_junctions.len()
                        ),
                    );
                }

                let segment_count = orca_stl::min(from_junctions.len(), to_junctions.len());
                for junction_rev_idx in 0..segment_count {
                    let from = from_junctions[from_junctions.len() - 1 - junction_rev_idx];
                    let to = to_junctions[to_junctions.len() - 1 - junction_rev_idx];
                    if from.perimeter_index != to.perimeter_index {
                        log(
                            Severity::Warning,
                            &format!(
                                "Connecting two perimeters with different indices! Perimeter {} and {}",
                                from.perimeter_index, to.perimeter_index
                            ),
                        );
                    }
                    let quad_start_to = self.edge_to(quad_start, F);
                    let from_is_odd = self.bead_count(quad_start_to) > 0
                        && self.bead_count(quad_start_to) % 2 == 1 // quad contains single bead segment
                        && self.graph.nodes[quad_start_to].data.transition_ratio == 0.0 // We're not in a transition
                        && junction_rev_idx == segment_count - 1 // Is single bead segment
                        && shorter_then(from.p - self.p(quad_start_to), scaled(0.005));
                    let quad_end_from = self.edge_from(quad_end, F);
                    let to_is_odd = self.bead_count(quad_end_from) > 0
                        && self.bead_count(quad_end_from) % 2 == 1 // quad contains single bead segment
                        && self.graph.nodes[quad_end_from].data.transition_ratio == 0.0 // We're not in a transition
                        && junction_rev_idx == segment_count - 1 // Is single bead segment
                        && shorter_then(to.p - self.p(quad_end_from), scaled(0.005));
                    let is_odd_segment = from_is_odd && to_is_odd;
                    if is_odd_segment
                        && passed_odd_edges.contains(
                            &self.graph.edges[req(self.graph.edges[quad_start].next, F)].twin,
                        )
                    {
                        // Only generate toolpath for odd segments once
                        continue; // Prevent duplication of single bead segments
                    }
                    let from_is_3way =
                        from_is_odd && self.graph.is_multi_intersection(quad_start_to);
                    let to_is_3way = to_is_odd && self.graph.is_multi_intersection(quad_end_from);
                    passed_odd_edges.insert(self.graph.edges[quad_start].next);

                    self.add_toolpath_segment(
                        &from,
                        &to,
                        is_odd_segment,
                        new_domain_start,
                        from_is_3way,
                        to_is_3way,
                    );
                }
                new_domain_start = false;
                match self.graph.get_next_unconnected(quad_start) {
                    Some(q) if q == poly_domain_start => break,
                    Some(q) => quad_start = q,
                    None => stop(F, GRAPH_NULL_WALK),
                }
            }
        }
    }

    /// `generateLocalMaximaSingleBeads()` (SkeletalTrapezoidation.cpp 2056-2090): at each node
    /// with an odd beading that is a strict local maximum and not central, a closed hexagon of
    /// radius an eighth of the middle bead's width, as the odd line of the middle inset (each
    /// vertex `node.p + Point(r * cos(a), r * sin(a))` with `a` a `float`: the `float` overloads,
    /// a `float` product, rounded to the point).
    pub fn generate_local_maxima_single_beads(&mut self) {
        let node_keys: Vec<NodeId> = self.graph.nodes.keys().collect();
        for node in node_keys {
            let Some(beading) = self.graph.nodes[node].data.get_beading() else {
                continue;
            };
            let beading = beading.borrow().beading.clone();
            if beading.bead_widths.len() % 2 == 1
                && self.graph.is_local_maximum(node, true)
                && !self.graph.is_central_node(node)
            {
                let inset_index = beading.bead_widths.len() / 2;
                let is_odd = true;
                let generated_toolpaths = &mut self.p_generated_toolpaths;
                if inset_index >= generated_toolpaths.len() {
                    generated_toolpaths.resize_with(inset_index + 1, Vec::new);
                }
                let mut line = ExtrusionLine::new(inset_index, is_odd);
                let width = beading.bead_widths[inset_index];
                // total area to be extruded is pi*(w/2)^2 = pi*w*w/4
                // Width a constant extrusion width w, that would be a length of pi*w/4
                // If we make a small circle to fill up the hole, then that circle would have a
                // circumference of 2*pi*r
                // So our circle needs to be such that r=w/8
                let r = width / 8;
                let n_segments: Coord = 6;
                let p = self.graph.nodes[node].p;
                for segment in 0..n_segments {
                    let a = (2.0 * PI / n_segments as f64 * segment as f64) as f32;
                    line.junctions.push(ExtrusionJunction::new(
                        p + Point::from_f64(
                            f64::from(r as f32 * orca_crt::cosf(a)),
                            f64::from(r as f32 * orca_crt::sinf(a)),
                        ),
                        width,
                        inset_index as Coord,
                    ));
                }
                self.p_generated_toolpaths[inset_index].push(line);
            }
        }
    }
}

/// `SKELETAL_TRAPEZOIDATION_BEAD_SEARCH_MAX` (SkeletalTrapezoidation.cpp 28): how many steps
/// `getNearestBeading` searches for a beading nearby.
pub const SKELETAL_TRAPEZOIDATION_BEAD_SEARCH_MAX: Coord = 1000;

#[cfg(test)]
mod tests {
    use orca_geom::point::Point;
    use orca_geom::polygon::{Polygon, Polygons};

    use super::{SkeletalTrapezoidation, normal, perp};
    use crate::beading::make_strategy;

    fn rect(x0: i64, y0: i64, x1: i64, y1: i64) -> Polygon {
        Polygon::new(vec![
            Point::new(x0, y0),
            Point::new(x1, y0),
            Point::new(x1, y1),
            Point::new(x0, y1),
        ])
    }

    #[test]
    fn normal_scales_and_falls_back() {
        assert_eq!(normal(Point::new(3, 4), 10), Point::new(6, 8));
        assert_eq!(normal(Point::new(0, 0), 7), Point::new(7, 0));
        assert_eq!(perp(Point::new(2, 5)), Point::new(-5, 2));
    }

    #[test]
    fn a_rectangle_builds_a_consistent_graph() {
        let strategy = make_strategy(
            400_000,
            400_000,
            400_000,
            0.174_532_92,
            false,
            100_000,
            100_000,
            0.5,
            0.75,
            4,
            0,
            1,
            0.5,
        );
        let polys: Polygons = vec![rect(0, 0, 3_000_000, 1_000_000)];
        let mut st = SkeletalTrapezoidation::new(
            &polys,
            strategy.as_ref(),
            strategy.get_transitioning_angle(),
            200_000,
            100_000,
            25_000,
            400_000,
        )
        .expect("a rectangle's diagram has no lookup error");
        assert!(!st.graph.edges.is_empty() && !st.graph.nodes.is_empty());
        for (k, e) in st.graph.edges.iter() {
            let twin = e.twin.expect("every edge has a twin");
            assert_eq!(st.graph.edges[twin].twin, Some(k));
            assert_eq!(st.graph.edges[twin].from, e.to);
            assert!(e.from.is_some_and(|n| st.graph.nodes.contains(n)));
        }
        st.update_is_central();
        assert!(st.graph.edges.values().all(|e| e.data.central_is_set()));
        st.filter_central(st.central_filter_dist());
        st.update_bead_count();
        st.filter_noncentral_regions();
        let mut mids = Vec::new();
        st.generate_transition_mids(&mut mids);
        st.filter_transition_mids();
        let mut ends = Vec::new();
        st.generate_all_transition_ends(&mut ends);
        // A 1 mm wide strip holds central edges with a bead count.
        assert!(
            st.graph
                .edges
                .values()
                .any(|e| e.data.is_central() && st.graph.nodes[e.to.unwrap()].data.bead_count > 0)
        );
    }
}
