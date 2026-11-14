// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Arachne/SkeletalTrapezoidationGraph.hpp lines 1-115 and
// SkeletalTrapezoidationGraph.cpp lines 1-472 whole, of OrcaSlicer v2.4.2, with Slic3r's
// shorter_then of src/libslic3r/Point.hpp lines 477-484 and the operator<< of Point at lines
// 384-388, translated on 2026-11-14 by arachne-2. Copyright Ultimaker B.V. (2020, CuraEngine), with
// the PrusaSlicer authors' (Prusa Research) and the OrcaSlicer authors' changes, under the GNU
// Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! `SkeletalTrapezoidationGraph`: the half-edge graph of the skeletal trapezoidation, its nodes
//! [`SkeletalTrapezoidationJoint`]s and its edges [`SkeletalTrapezoidationEdge`]s, with the
//! methods of the C++'s `STHalfEdge`, `STHalfEdgeNode` and the graph.
//!
//! The graph is orca-arachne's [`HalfEdgeGraph`]: two [`crate::half_edge::StableList`]s whose
//! handles stand for the C++ pointers. `STHalfEdge` and `STHalfEdgeNode` are [`HalfEdge`] and
//! [`HalfEdgeNode`] of these data; their methods follow pointers, so they are the graph's methods
//! here and take the edge's or the node's handle:
//!
//! - `canGoUp`, `isUpward` and `distToGoUp` ([`SkeletalTrapezoidationGraph::can_go_up`],
//!   [`is_upward`](SkeletalTrapezoidationGraph::is_upward),
//!   [`dist_to_go_up`](SkeletalTrapezoidationGraph::dist_to_go_up)) compare the two nodes'
//!   distances to the outline and, between equidistant nodes, recurse over the edges leaving the
//!   far node, as the C++ does; `isUpward` breaks the last tie by the points' order.
//!   `getNextUnconnected` walks the next edges to the last and gives its twin.
//! - `isMultiIntersection`, `isCentral` and `isLocalMaximum` of a node
//!   ([`is_multi_intersection`](SkeletalTrapezoidationGraph::is_multi_intersection),
//!   [`is_central_node`](SkeletalTrapezoidationGraph::is_central_node),
//!   [`is_local_maximum`](SkeletalTrapezoidationGraph::is_local_maximum)) go round the node
//!   through each edge's twin's next edge.
//! - `collapseSmallEdges` removes the quads' middle edges shorter than the snap distance and the
//!   quads whose sides are both that short, in the edge list's order. The C++ finds a list element
//!   by its address through two ankerl maps; a handle is the element's iterator here, so the maps
//!   are not needed. Past 50 edges round one node it writes each edge to `std::cerr`, as the C++
//!   does (`orca_mesh::log::write_stderr`, the points unscaled in the stream's default format).
//! - `makeRib`, `insertRib` and `insertNode` add the ribs from a node to the outline, the new nodes
//!   and edges at the front (`makeRib`) or the back of the lists, as the C++'s `emplace_front` and
//!   `emplace_back` do. `getSource` (protected in the C++) is public here for SkeletalTrapezoidation.
//!
//! `shorter_then` (Point.hpp) belongs in `orca-geom`, which has none yet; it is translated here,
//! as is the `operator<<` of a `Point` the warning text uses.
//!
//! Stops ([`crate::stop`]): `collapseSmallEdges` meeting a middle edge with no twin loops for ever
//! on its `continue` (`collapse-no-twin`), after its warning; a null pointer the C++ follows
//! (`graph-null-walk`: a node's null incident edge in `isCentral` and `isLocalMaximum`, a null next
//! edge between equidistant nodes in `canGoUp` and `distToGoUp`, and any other null edge, twin or
//! node these functions read through). The C++'s asserts do not run in its release build; their
//! early returns (`if (!outgoing->twin) return false;` and the others) are kept.

use orca_geom::line::Line;
use orca_geom::point::{Coord, Point, unscale_};
use orca_mesh::log::{Severity, log, write_stderr};

use crate::extrusion_line::norm_i64;
use crate::half_edge::{EdgeId, HalfEdge, HalfEdgeGraph, HalfEdgeNode, NodeId};
use crate::skeletal_trapezoidation_edge::{EdgeType, SkeletalTrapezoidationEdge};
use crate::skeletal_trapezoidation_joint::SkeletalTrapezoidationJoint;
use crate::stop::{COLLAPSE_NO_TWIN, GRAPH_NULL_WALK, stop};

/// `STHalfEdge`: `HalfEdge<SkeletalTrapezoidationJoint, SkeletalTrapezoidationEdge, ...>`.
pub type STHalfEdge = HalfEdge<SkeletalTrapezoidationEdge>;
/// `STHalfEdgeNode`: `HalfEdgeNode<SkeletalTrapezoidationJoint, SkeletalTrapezoidationEdge, ...>`.
pub type STHalfEdgeNode = HalfEdgeNode<SkeletalTrapezoidationJoint>;
/// `SkeletalTrapezoidationGraph`: `HalfEdgeGraph<SkeletalTrapezoidationJoint,
/// SkeletalTrapezoidationEdge, STHalfEdgeNode, STHalfEdge>`.
pub type SkeletalTrapezoidationGraph =
    HalfEdgeGraph<SkeletalTrapezoidationJoint, SkeletalTrapezoidationEdge>;

/// `shorter_then(p0, len)` (Point.hpp 477-484): both coordinates within `len` and the squared
/// norm at most `len` squared. It belongs in `orca-geom`.
#[must_use]
pub fn shorter_then(p0: Point, len: Coord) -> bool {
    if p0.x > len || p0.x < len.wrapping_neg() {
        return false;
    }
    if p0.y > len || p0.y < len.wrapping_neg() {
        return false;
    }
    p0.squared_norm() <= len.wrapping_mul(len)
}

/// `std::ostream << pt` of a `Point` (Point.hpp 384-388): `unscale_(x) << "," << unscale_(y)`,
/// each in the stream's default format (`%g`, precision 6).
#[allow(clippy::cast_precision_loss)]
fn point_text(pt: Point) -> String {
    let mut s = orca_fmt::ostream::Stream::default();
    let x = orca_fmt::ostream::f64_to_string(&mut s, unscale_(pt.x as f64))
        .expect("a double in the default format fits");
    let y = orca_fmt::ostream::f64_to_string(&mut s, unscale_(pt.y as f64))
        .expect("a double in the default format fits");
    format!("{x},{y}")
}

/// The pointer `p`, which the C++ follows; a null one stops (`graph-null-walk`).
fn req<K>(p: Option<K>, function: &'static str) -> K {
    match p {
        Some(k) => k,
        None => stop(function, GRAPH_NULL_WALK),
    }
}

impl SkeletalTrapezoidationGraph {
    /// The distances to the outline of the edge's `to` and `from` nodes.
    fn up_down(&self, edge: EdgeId, function: &'static str) -> (Coord, Coord) {
        let e = &self.edges[edge];
        let to = self.nodes[req(e.to, function)].data.distance_to_boundary;
        let from = self.nodes[req(e.from, function)].data.distance_to_boundary;
        (to, from)
    }

    /// `STHalfEdge::canGoUp(strict = false)`: whether there is an upward edge from the
    /// `distance_to_boundary` of the edge's `from` (recursively); `strict`: whether equidistant
    /// edges can count as a local maximum.
    #[must_use]
    pub fn can_go_up(&self, edge: EdgeId, strict: bool) -> bool {
        const F: &str = "STHalfEdge::canGoUp";
        let (to, from) = self.up_down(edge, F);
        if to > from {
            return true;
        }
        if to < from || strict {
            return false;
        }

        // Edge is between equidistqant verts; recurse!
        let twin = self.edges[edge].twin;
        let mut outgoing = self.edges[edge].next;
        while outgoing != twin {
            let o = req(outgoing, F);
            if self.can_go_up(o, false) {
                return true;
            }
            let Some(o_twin) = self.edges[o].twin else {
                return false;
            };
            let Some(o_twin_next) = self.edges[o_twin].next else {
                return true; // This point is on the boundary?! Should never occur
            };
            outgoing = Some(o_twin_next);
        }
        false
    }

    /// `STHalfEdge::isUpward()`: whether the edge goes from a lower to a higher
    /// `distance_to_boundary`, looking beyond equidistant edges.
    #[must_use]
    pub fn is_upward(&self, edge: EdgeId) -> bool {
        const F: &str = "STHalfEdge::isUpward";
        let (to, from) = self.up_down(edge, F);
        if to > from {
            return true;
        }
        if to < from {
            return false;
        }

        // Equidistant edge case:
        let forward_up_dist = self.dist_to_go_up(edge);
        let backward_up_dist = self.dist_to_go_up(req(self.edges[edge].twin, F));
        if let (Some(forward), Some(backward)) = (forward_up_dist, backward_up_dist) {
            return forward < backward;
        }

        if forward_up_dist.is_some() {
            return true;
        }

        if backward_up_dist.is_some() {
            return false;
        }
        let e = &self.edges[edge];
        // Arbitrary ordering, which returns the opposite for the twin edge
        self.nodes[req(e.to, F)].p < self.nodes[req(e.from, F)].p
    }

    /// `STHalfEdge::distToGoUp()`: the distance travelled until an upward edge, the edge's own
    /// length included when it can go up; `None` when it cannot.
    #[must_use]
    pub fn dist_to_go_up(&self, edge: EdgeId) -> Option<Coord> {
        const F: &str = "STHalfEdge::distToGoUp";
        let (to, from) = self.up_down(edge, F);
        if to > from {
            return Some(0);
        }
        if to < from {
            return None;
        }

        // Edge is between equidistqant verts; recurse!
        let mut ret: Option<Coord> = None;
        let twin = self.edges[edge].twin;
        let mut outgoing = self.edges[edge].next;
        while outgoing != twin {
            let o = req(outgoing, F);
            let dist_to_up = self.dist_to_go_up(o);
            if let Some(d) = dist_to_up {
                ret = Some(match ret {
                    Some(r) => orca_stl::min(r, d),
                    None => d,
                });
            }
            // `if (!outgoing->twin) return std::optional<coord_t>();`
            let o_twin = self.edges[o].twin?;
            let Some(o_twin_next) = self.edges[o_twin].next else {
                return Some(0); // This point is on the boundary?! Should never occur
            };
            outgoing = Some(o_twin_next);
        }
        if let Some(r) = ret {
            let e = &self.edges[edge];
            let length = norm_i64(self.nodes[req(e.to, F)].p - self.nodes[req(e.from, F)].p);
            ret = Some(r.wrapping_add(length));
        }
        ret
    }

    /// `STHalfEdge::getNextUnconnected()`: the twin of the last edge the next edges lead to, or
    /// `None` when they lead back to this edge.
    #[must_use]
    pub fn get_next_unconnected(&self, edge: EdgeId) -> Option<EdgeId> {
        let mut result = edge;
        while let Some(next) = self.edges[result].next {
            result = next;
            if result == edge {
                return None;
            }
        }
        self.edges[result].twin
    }

    /// `STHalfEdgeNode::isMultiIntersection()`: more than two central edges leave the node; false
    /// for a node on the outside (a null edge on the way round).
    #[must_use]
    pub fn is_multi_intersection(&self, node: NodeId) -> bool {
        const F: &str = "STHalfEdgeNode::isMultiIntersection";
        let mut odd_path_count: i32 = 0;
        let incident_edge = self.nodes[node].incident_edge;
        let mut outgoing = incident_edge;
        loop {
            let Some(o) = outgoing else {
                // This is a node on the outside
                return false;
            };
            if self.edges[o].data.is_central() {
                odd_path_count += 1;
            }
            outgoing = self.edges[req(self.edges[o].twin, F)].next;
            if outgoing == incident_edge {
                break;
            }
        }
        odd_path_count > 2
    }

    /// `STHalfEdgeNode::isCentral()`: a central edge leaves the node.
    #[must_use]
    pub fn is_central_node(&self, node: NodeId) -> bool {
        const F: &str = "STHalfEdgeNode::isCentral";
        let incident_edge = self.nodes[node].incident_edge;
        let mut edge = incident_edge;
        loop {
            let e = req(edge, F);
            if self.edges[e].data.is_central() {
                return true;
            }
            let Some(twin) = self.edges[e].twin else {
                return false;
            };
            edge = self.edges[twin].next;
            if edge == incident_edge {
                break;
            }
        }
        false
    }

    /// `STHalfEdgeNode::isLocalMaximum(strict = false)`: whether the node's
    /// `distance_to_boundary` is locally maximal; `strict`: whether equidistant edges can count as
    /// a local maximum.
    #[must_use]
    pub fn is_local_maximum(&self, node: NodeId, strict: bool) -> bool {
        const F: &str = "STHalfEdgeNode::isLocalMaximum";
        if self.nodes[node].data.distance_to_boundary == 0 {
            return false;
        }

        let incident_edge = self.nodes[node].incident_edge;
        let mut edge = incident_edge;
        loop {
            let e = req(edge, F);
            if self.can_go_up(e, strict) {
                return false;
            }
            let Some(twin) = self.edges[e].twin else {
                return false;
            };
            let Some(twin_next) = self.edges[twin].next else {
                // This point is on the boundary
                return false;
            };
            edge = Some(twin_next);
            if edge == incident_edge {
                break;
            }
        }
        true
    }

    /// `safelyRemoveEdge` of `collapseSmallEdges`: erases `to_be_removed`, moving the loop's
    /// iterator past it when it is the edge the loop stands on.
    fn safely_remove_edge(
        &mut self,
        to_be_removed: EdgeId,
        current_edge_it: &mut Option<EdgeId>,
        edge_it_is_updated: &mut bool,
    ) {
        if *current_edge_it == Some(to_be_removed) {
            *current_edge_it = self.edges.erase(to_be_removed);
            *edge_it_is_updated = true;
        } else {
            self.edges.erase(to_be_removed);
        }
    }

    /// `collapseSmallEdges(snap_dist = 5)`: collapses each quad's middle edge (and its twin) that
    /// is shorter than `snap_dist`, and each quad whose two sides are, fixing the edges round them
    /// so that the graph stays consistent. A support edge is not collapsed alone, only with its
    /// whole quad.
    #[allow(clippy::too_many_lines)]
    pub fn collapse_small_edges(&mut self, snap_dist: Coord) {
        const F: &str = "SkeletalTrapezoidationGraph::collapseSmallEdges";
        let should_collapse =
            |g: &Self, a: NodeId, b: NodeId| shorter_then(g.nodes[a].p - g.nodes[b].p, snap_dist);

        let mut edge_it = self.edges.front();
        while let Some(it) = edge_it {
            if self.edges[it].prev.is_some() {
                edge_it = self.edges.next(it);
                continue;
            }

            let quad_start = it;
            let mut quad_end = quad_start;
            while let Some(next) = self.edges[quad_end].next {
                quad_end = next;
            }
            let quad_mid = if self.edges[quad_start].next == Some(quad_end) {
                None
            } else {
                self.edges[quad_start].next
            };

            let mut edge_it_is_updated = false;
            if let Some(mid) = quad_mid
                && should_collapse(
                    self,
                    req(self.edges[mid].from, F),
                    req(self.edges[mid].to, F),
                )
            {
                let Some(mid_twin) = self.edges[mid].twin else {
                    log(Severity::Warning, "Encountered quad edge without a twin.");
                    // The C++ `continue`s without moving on: it loops for ever.
                    stop(F, COLLAPSE_NO_TWIN);
                };
                let mut count: i32 = 0;
                let mut edge_from_3 = Some(quad_end);
                while let Some(e3) = edge_from_3 {
                    if Some(e3) == self.edges[mid].twin {
                        break;
                    }
                    let mid_from = self.edges[mid].from;
                    self.edges[e3].from = mid_from;
                    let e3_twin = req(self.edges[e3].twin, F);
                    self.edges[e3_twin].to = mid_from;
                    if count > 50 {
                        let e = &self.edges[e3];
                        let from_p = self.nodes[req(e.from, F)].p;
                        let to_p = self.nodes[req(e.to, F)].p;
                        write_stderr(&format!("{} - {}\n", point_text(from_p), point_text(to_p)));
                    }
                    count += 1;
                    if count > 1000 {
                        break;
                    }
                    edge_from_3 = self.edges[req(self.edges[e3].twin, F)].next;
                }

                // o-o > collapse top
                // | |
                // | |
                // | |
                // o o
                let mid_from = req(self.edges[mid].from, F);
                if self.nodes[mid_from].incident_edge == Some(mid) {
                    if let Some(twin_next) = self.edges[mid_twin].next {
                        self.nodes[mid_from].incident_edge = Some(twin_next);
                    } else {
                        let mid_prev = req(self.edges[mid].prev, F);
                        self.nodes[mid_from].incident_edge = self.edges[mid_prev].twin;
                    }
                }

                self.nodes.erase(req(self.edges[mid].to, F));

                let mid_next = self.edges[mid].next;
                let at = req(self.edges[mid].prev, F);
                self.edges[at].next = mid_next;
                let mid_prev = self.edges[mid].prev;
                let at = req(self.edges[mid].next, F);
                self.edges[at].prev = mid_prev;
                let twin_prev = self.edges[mid_twin].prev;
                let at = req(self.edges[mid_twin].next, F);
                self.edges[at].prev = twin_prev;
                let twin_next = self.edges[mid_twin].next;
                let at = req(self.edges[mid_twin].prev, F);
                self.edges[at].next = twin_next;

                let to_remove = req(self.edges[mid].twin, F);
                self.safely_remove_edge(to_remove, &mut edge_it, &mut edge_it_is_updated);
                self.safely_remove_edge(mid, &mut edge_it, &mut edge_it_is_updated);
            }

            //  o-o
            //  | | > collapse sides
            //  o o
            if should_collapse(
                self,
                req(self.edges[quad_start].from, F),
                req(self.edges[quad_end].to, F),
            ) && should_collapse(
                self,
                req(self.edges[quad_start].to, F),
                req(self.edges[quad_end].from, F),
            ) {
                // Collapse start and end edges and remove whole cell
                let start_twin = req(self.edges[quad_start].twin, F);
                self.edges[start_twin].to = self.edges[quad_end].to;
                let end_twin = self.edges[quad_end].twin;
                self.nodes[req(self.edges[quad_end].to, F)].incident_edge = end_twin;
                let end_from = req(self.edges[quad_end].from, F);
                if self.nodes[end_from].incident_edge == Some(quad_end) {
                    if let Some(next) = self.edges[req(self.edges[quad_end].twin, F)].next {
                        self.nodes[end_from].incident_edge = Some(next);
                    } else {
                        let end_prev = req(self.edges[quad_end].prev, F);
                        self.nodes[end_from].incident_edge = self.edges[end_prev].twin;
                    }
                }
                self.nodes.erase(req(self.edges[quad_start].from, F));

                let end_twin = self.edges[quad_end].twin;
                let at = req(self.edges[quad_start].twin, F);
                self.edges[at].twin = end_twin;
                let start_twin = self.edges[quad_start].twin;
                let at = req(self.edges[quad_end].twin, F);
                self.edges[at].twin = start_twin;
                self.safely_remove_edge(quad_start, &mut edge_it, &mut edge_it_is_updated);
                self.safely_remove_edge(quad_end, &mut edge_it, &mut edge_it_is_updated);
            }
            // If only one side had collapsable length then the cell on the other side of that edge
            // has to collapse; if we would collapse that one edge then that would change the
            // quad_start and/or quad_end of neighboring cells; this is to do with the constraint
            // that !prev == !twin.next

            if !edge_it_is_updated {
                edge_it = self.edges.next(it);
            }
        }
    }

    /// `makeRib(prev_edge, start_source_point, end_source_point)`: a rib from `prev_edge`'s `to`
    /// node to its foot on the infinite line through the two source points: a new node at the
    /// front of the nodes and two extra-VD edges at the front of the edges; `prev_edge` becomes
    /// the rib's edge back.
    pub fn make_rib(
        &mut self,
        prev_edge: &mut EdgeId,
        start_source_point: Point,
        end_source_point: Point,
    ) {
        const F: &str = "SkeletalTrapezoidationGraph::makeRib";
        let prev_to = req(self.edges[*prev_edge].to, F);
        let (_, p) = Line::new(start_source_point, end_source_point)
            .distance_to_infinite_squared(self.nodes[prev_to].p);
        let dist: Coord = norm_i64(self.nodes[prev_to].p - p);
        self.nodes[prev_to].data.distance_to_boundary = dist;

        let node = self
            .nodes
            .push_front(STHalfEdgeNode::new(SkeletalTrapezoidationJoint::new(), p));
        self.nodes[node].data.distance_to_boundary = 0;

        let forth_edge = self
            .edges
            .push_front(STHalfEdge::new(SkeletalTrapezoidationEdge::new(
                EdgeType::ExtraVd,
            )));
        let back_edge = self
            .edges
            .push_front(STHalfEdge::new(SkeletalTrapezoidationEdge::new(
                EdgeType::ExtraVd,
            )));

        self.edges[*prev_edge].next = Some(forth_edge);
        self.edges[forth_edge].prev = Some(*prev_edge);
        let prev_edge_to = self.edges[*prev_edge].to;
        self.edges[forth_edge].from = prev_edge_to;
        self.edges[forth_edge].to = Some(node);
        self.edges[forth_edge].twin = Some(back_edge);
        self.edges[back_edge].twin = Some(forth_edge);
        self.edges[back_edge].from = Some(node);
        self.edges[back_edge].to = prev_edge_to;
        self.nodes[node].incident_edge = Some(back_edge);

        *prev_edge = back_edge;
    }

    /// `insertRib(edge, mid_node)`: splits `edge` at `mid_node` with a rib to the edge's source
    /// segment: a source node and three edges at the back of the lists. Returns the first and the
    /// last edge of the edges replacing `edge` that point to the same node (`edge` itself and the
    /// new second half), their twins unset.
    pub fn insert_rib(&mut self, edge: EdgeId, mid_node: NodeId) -> (EdgeId, EdgeId) {
        const F: &str = "SkeletalTrapezoidationGraph::insertRib";
        let edge_before = self.edges[edge].prev;
        let edge_after = self.edges[edge].next;
        let node_before = self.edges[edge].from;
        let node_after = self.edges[edge].to;

        let p = self.nodes[mid_node].p;

        let source_segment = self.get_source(edge);
        let (_, px) = source_segment.distance_to_squared_closest(p);
        let dist: Coord = norm_i64(p - px);
        self.nodes[mid_node].data.distance_to_boundary = dist;
        // Both transition end should have rest = 0, because at the ends a whole number of beads
        // fits without rest
        self.nodes[mid_node].data.transition_ratio = 0.0;

        let source_node = self
            .nodes
            .push_back(STHalfEdgeNode::new(SkeletalTrapezoidationJoint::new(), px));
        self.nodes[source_node].data.distance_to_boundary = 0;

        let first = edge;
        let second = self
            .edges
            .push_back(STHalfEdge::new(SkeletalTrapezoidationEdge::default()));
        let outward_edge = self
            .edges
            .push_back(STHalfEdge::new(SkeletalTrapezoidationEdge::new(
                EdgeType::TransitionEnd,
            )));
        let inward_edge = self
            .edges
            .push_back(STHalfEdge::new(SkeletalTrapezoidationEdge::new(
                EdgeType::TransitionEnd,
            )));

        if let Some(before) = edge_before {
            self.edges[before].next = Some(first);
        }
        self.edges[first].next = Some(outward_edge);
        self.edges[outward_edge].next = None;
        self.edges[inward_edge].next = Some(second);
        self.edges[second].next = edge_after;

        if let Some(after) = edge_after {
            self.edges[after].prev = Some(second);
        }
        self.edges[second].prev = Some(inward_edge);
        self.edges[inward_edge].prev = None;
        self.edges[outward_edge].prev = Some(first);
        self.edges[first].prev = edge_before;

        self.edges[first].to = Some(mid_node);
        self.edges[outward_edge].to = Some(source_node);
        self.edges[inward_edge].to = Some(mid_node);
        self.edges[second].to = node_after;

        self.edges[first].from = node_before;
        self.edges[outward_edge].from = Some(mid_node);
        self.edges[inward_edge].from = Some(source_node);
        self.edges[second].from = Some(mid_node);

        self.nodes[req(node_before, F)].incident_edge = Some(first);
        self.nodes[mid_node].incident_edge = Some(outward_edge);
        self.nodes[source_node].incident_edge = Some(inward_edge);
        if edge_after.is_some() {
            self.nodes[req(node_after, F)].incident_edge = edge_after;
        }

        self.edges[first].data.set_is_central(true);
        // TODO verify this is always the case.
        self.edges[outward_edge].data.set_is_central(false);
        self.edges[inward_edge].data.set_is_central(false);
        self.edges[second].data.set_is_central(true);

        self.edges[outward_edge].twin = Some(inward_edge);
        self.edges[inward_edge].twin = Some(outward_edge);

        // we don't know these yet!
        self.edges[first].twin = None;
        self.edges[second].twin = None;

        (first, second)
    }

    /// `insertNode(edge, mid, mide_node_bead_count)`: a node at `mid` on `edge` and its twin,
    /// connected to the outline by ribs on both sides. Returns the last edge which replaced
    /// `edge`, which points to the same `to` node.
    pub fn insert_node(&mut self, edge: EdgeId, mid: Point, mide_node_bead_count: Coord) -> EdgeId {
        const F: &str = "SkeletalTrapezoidationGraph::insertNode";
        let mut last_edge_replacing_input = edge;

        let mid_node = self
            .nodes
            .push_back(STHalfEdgeNode::new(SkeletalTrapezoidationJoint::new(), mid));

        let twin = self.edges[last_edge_replacing_input].twin;
        self.edges[last_edge_replacing_input].twin = None;
        let twin = req(twin, F);
        self.edges[twin].twin = None;
        let left_pair = self.insert_rib(last_edge_replacing_input, mid_node);
        let right_pair = self.insert_rib(twin, mid_node);
        let first_edge_replacing_input = left_pair.0;
        last_edge_replacing_input = left_pair.1;
        let first_edge_replacing_twin = right_pair.0;
        let last_edge_replacing_twin = right_pair.1;

        self.edges[first_edge_replacing_input].twin = Some(last_edge_replacing_twin);
        self.edges[last_edge_replacing_twin].twin = Some(first_edge_replacing_input);
        self.edges[last_edge_replacing_input].twin = Some(first_edge_replacing_twin);
        self.edges[first_edge_replacing_twin].twin = Some(last_edge_replacing_input);

        self.nodes[mid_node].data.bead_count = mide_node_bead_count;

        last_edge_replacing_input
    }

    /// `getSource(edge)` (protected in the C++): the line from the `from` node of the first edge
    /// of `edge`'s quad to the `to` node of its last.
    #[must_use]
    pub fn get_source(&self, edge: EdgeId) -> Line {
        const F: &str = "SkeletalTrapezoidationGraph::getSource";
        let mut from_edge = edge;
        while let Some(prev) = self.edges[from_edge].prev {
            from_edge = prev;
        }

        let mut to_edge = edge;
        while let Some(next) = self.edges[to_edge].next {
            to_edge = next;
        }

        Line::new(
            self.nodes[req(self.edges[from_edge].from, F)].p,
            self.nodes[req(self.edges[to_edge].to, F)].p,
        )
    }
}

#[cfg(test)]
mod tests {
    use orca_geom::point::Point;

    use super::{SkeletalTrapezoidationGraph, point_text, shorter_then};

    #[test]
    fn shorter_then_takes_the_square() {
        assert!(shorter_then(Point::new(3, 4), 5));
        assert!(!shorter_then(Point::new(4, 4), 5));
        assert!(!shorter_then(Point::new(-6, 0), 5));
        assert_eq!(point_text(Point::new(1_500_000, -250)), "1.5,-0.00025");
    }

    #[test]
    fn a_rib_hangs_from_the_edge() {
        let mut g = SkeletalTrapezoidationGraph::default();
        let a = g.nodes.push_back(super::STHalfEdgeNode::new(
            super::SkeletalTrapezoidationJoint::new(),
            Point::new(0, 0),
        ));
        let b = g.nodes.push_back(super::STHalfEdgeNode::new(
            super::SkeletalTrapezoidationJoint::new(),
            Point::new(0, 1000),
        ));
        let e = g.edges.push_back(super::STHalfEdge::new(
            super::SkeletalTrapezoidationEdge::default(),
        ));
        g.edges[e].from = Some(a);
        g.edges[e].to = Some(b);
        let mut prev = e;
        g.make_rib(&mut prev, Point::new(-500, 0), Point::new(-500, 2000));
        assert_ne!(prev, e);
        let foot = g.edges[prev].from.unwrap();
        assert_eq!(g.nodes[foot].p, Point::new(-500, 1000));
        assert_eq!(g.nodes[b].data.distance_to_boundary, 500);
        assert_eq!(g.nodes.front(), Some(foot));
        assert_eq!(g.edges.front(), Some(prev));
    }
}
