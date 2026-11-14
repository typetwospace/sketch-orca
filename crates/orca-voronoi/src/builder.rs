// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Boost code: Boost.Polygon of Boost 1.84.0, translated on 2026-11-13 by orca-o3-voronoi-2
// from boost/polygon/voronoi_builder.hpp, lines 1-521 (voronoi_builder<int32> and
// default_voronoi_builder). Copyright Andrii Sydorchuk 2010-2012, under the Boost Software
// License, version 1.0 (BSL-1.0; licenses/BSL-1.0.txt).

//! Boost.Polygon's sweep-line Voronoi builder (`voronoi_builder.hpp`) for points and segments that
//! meet only at their end points, with `int32` coordinates: `voronoi_builder<int32>`, the only
//! instantiation OrcaSlicer makes (`default_voronoi_builder`).
//!
//! - `insert_point` makes one site event, `insert_segment` three: its start point, its end point,
//!   and the segment itself, from the smaller of its points ([`VoronoiBuilder::insert_point`],
//!   [`VoronoiBuilder::insert_segment`]). Points and segments share one input index.
//! - [`VoronoiBuilder::construct`] sorts the sites with `event_comparison_predicate` through
//!   `orca-stl`'s `sort_by` (MSVC's `std::sort`), drops equal sites with `orca-stl`'s
//!   `unique_vec_by` (`std::unique`, which keeps the first of equal sites, so the order the sort
//!   leaves decides the survivor's input index and category), numbers them, starts the beach line
//!   (one site, two sites, or a run of vertical sites at the leftmost x), then takes events in
//!   order: the next site unless the top circle event goes first, dropping inactive circle events
//!   from the top after each.
//! - A site event finds the arc above it with `lower_bound` on the beach line, splits it with two
//!   new bisectors (and a temporary bisector for a segment, removed when its end point's site is
//!   processed), and checks the new triples for circle events. A circle event joins the bisectors
//!   (A, B) and (B, C) into (A, C), changing the key of (A, B) in place (the C++'s `const_cast`),
//!   and checks the triples on both sides.
//! - The output is the trait [`VoronoiOutput`], the C++'s `OUTPUT` template parameter: the five
//!   calls the builder makes (`_reserve`, `_process_single_site`, the two `_insert_new_edge` and
//!   `_build`), its edges a handle `E` the builder keeps in the beach line (the C++ keeps `void*`).
//!
//! The beach line is a `std::map` keyed by bisectors and ordered by `node_comparison_predicate`,
//! which orders only the pairs the builder compares; it is MSVC's red-black tree (module `tree`),
//! with its hinted inserts, so the builder makes the same comparisons as the C++ and gets the same
//! diagram, degenerate input included. The circle events are an [`OrderedQueue`] ordered by the
//! event comparison with its arguments swapped (`event_comparison_type`), and the temporary
//! bisectors' end points a `std::priority_queue` (`orca-stl`'s `PriorityQueue`) with the smallest
//! point on top: both pop ties in MSVC's heap order. A beach-line iterator is a node index of the
//! tree, and a circle event's place in the queue is its slot.
//!
//! [`VoronoiBuilder::site_events`] is not in the C++: it reads the sorted sites after `construct`,
//! which `tests/probes.rs` compares with the C++'s (VRSS).
//!
//! Left out: the builder is not generic over its coordinate traits or its predicates (`CTT` and
//! `VP`; OrcaSlicer uses only the defaults for `int32`), and its copy constructor and assignment,
//! disallowed in the C++, are not given. Nothing stops with NotPorted: domain A reaches all of it.

use orca_stl::PriorityQueue;

use crate::formation::CircleFormationPredicate;
use crate::geometry_type::SourceCategory;
use crate::predicates::{
    EventComparisonPredicate, NodeComparisonPredicate, PointComparisonPredicate, is_vertical,
    is_vertical_points,
};
use crate::structures::{
    BeachLineNodeData, BeachLineNodeKey, CircleEvent, OrderedQueue, Point2d, SiteEvent,
};
use crate::tree::StdMap;

/// The output a [`VoronoiBuilder`] builds (the C++'s `OUTPUT` template parameter, whose members
/// `voronoi_diagram` defines for the builder).
pub trait VoronoiOutput {
    /// An edge of the output, as the builder keeps it in the beach line (`void*` in the C++).
    type Edge: Copy;

    /// `_reserve(num_sites)`: the number of site events before duplicates are dropped.
    fn reserve(&mut self, num_sites: usize);

    /// `_process_single_site(site)`: the input has one site.
    fn process_single_site(&mut self, site: &SiteEvent);

    /// `_insert_new_edge(site1, site2)`: a new bisector of two sites; its two half-edges.
    fn insert_new_edge(&mut self, site1: &SiteEvent, site2: &SiteEvent)
    -> (Self::Edge, Self::Edge);

    /// `_insert_new_edge(site1, site3, circle, data12, data23)`: the bisector of `site1` and
    /// `site3` that starts at the circle's centre, where the bisectors `data12` and `data23` end;
    /// its two half-edges. `None` is a null `void*` (the builder never passes one).
    fn insert_new_edge_at_circle(
        &mut self,
        site1: &SiteEvent,
        site3: &SiteEvent,
        circle: &CircleEvent,
        data12: Option<Self::Edge>,
        data23: Option<Self::Edge>,
    ) -> (Self::Edge, Self::Edge);

    /// `_build()`: the sweep is done.
    fn build(&mut self);
}

/// `event_type`: a circle event and the beach-line node of its right bisector (B, C).
type Event = (CircleEvent, usize);

/// `end_point_type`: a segment's end point and its temporary bisector's beach-line node.
type EndPoint = (Point2d, usize);

/// `key_type`.
type Key = BeachLineNodeKey;

/// `beach_line_type`: `std::map<key_type, value_type, node_comparer_type>`.
type BeachLine<E> = StdMap<Key, BeachLineNodeData<E>, fn(&Key, &Key) -> bool>;

/// `event_comparison_type::operator()(lhs, rhs)`: `predicate(rhs.first, lhs.first)`, so the
/// queue's top is the event the event comparison puts first.
fn event_comparison_type(lhs: &Event, rhs: &Event) -> bool {
    EventComparisonPredicate::default().circle_circle(&rhs.0, &lhs.0)
}

/// `end_point_comparison::operator()(end1, end2)`: `point_comparison(end2.first, end1.first)`,
/// so the top is the smallest point.
fn end_point_comparison(end1: &EndPoint, end2: &EndPoint) -> bool {
    PointComparisonPredicate.call(&end2.0, &end1.0)
}

/// `node_comparer_type::operator()(node1, node2)`, the beach line's key comparison.
fn node_comparer(node1: &Key, node2: &Key) -> bool {
    NodeComparisonPredicate::default().call(node1, node2)
}

/// `voronoi_builder<int32>`: the sweep-line builder of the Voronoi diagram of points and
/// segments. `E` is the output's edge handle ([`VoronoiOutput::Edge`]).
pub struct VoronoiBuilder<E: Copy = usize> {
    point_comparison: PointComparisonPredicate,
    site_events: Vec<SiteEvent>,
    /// `site_event_iterator_`, an index into `site_events`.
    site_event_iterator: usize,
    end_points: PriorityQueue<EndPoint, fn(&EndPoint, &EndPoint) -> bool>,
    circle_events: OrderedQueue<Event, fn(&Event, &Event) -> bool>,
    beach_line: BeachLine<E>,
    circle_formation_predicate: CircleFormationPredicate,
    index: usize,
}

/// `default_voronoi_builder`: `voronoi_builder<int32>`.
pub type DefaultVoronoiBuilder<E = usize> = VoronoiBuilder<E>;

impl<E: Copy> Default for VoronoiBuilder<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Copy> VoronoiBuilder<E> {
    /// `voronoi_builder()`: no sites, the input index at 0.
    #[must_use]
    pub fn new() -> Self {
        Self {
            point_comparison: PointComparisonPredicate,
            site_events: Vec::new(),
            site_event_iterator: 0,
            end_points: PriorityQueue::with_compare(
                end_point_comparison as fn(&EndPoint, &EndPoint) -> bool,
            ),
            circle_events: OrderedQueue::new(event_comparison_type as fn(&Event, &Event) -> bool),
            beach_line: StdMap::new(node_comparer as fn(&Key, &Key) -> bool),
            circle_formation_predicate: CircleFormationPredicate::default(),
            index: 0,
        }
    }

    /// `insert_point(x, y)`: one site event; returns the point's input index.
    pub fn insert_point(&mut self, x: i32, y: i32) -> usize {
        let mut site = SiteEvent::from_xy(x, y);
        site.set_initial_index(self.index);
        site.set_source_category(SourceCategory::SINGLE_POINT);
        self.site_events.push(site);
        let index = self.index;
        self.index += 1;
        index
    }

    /// `insert_segment(x1, y1, x2, y2)`: three site events (the start point, the end point and the
    /// segment from the smaller point); returns the segment's input index.
    pub fn insert_segment(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) -> usize {
        // Set up start point site.
        let p1 = Point2d::new(x1, y1);
        let mut start = SiteEvent::from_point(p1);
        start.set_initial_index(self.index);
        start.set_source_category(SourceCategory::SEGMENT_START_POINT);
        self.site_events.push(start);

        // Set up end point site.
        let p2 = Point2d::new(x2, y2);
        let mut end = SiteEvent::from_point(p2);
        end.set_initial_index(self.index);
        end.set_source_category(SourceCategory::SEGMENT_END_POINT);
        self.site_events.push(end);

        // Set up segment site.
        let mut segment = if self.point_comparison.call(&p1, &p2) {
            let mut s = SiteEvent::from_points(p1, p2);
            s.set_source_category(SourceCategory::INITIAL_SEGMENT);
            s
        } else {
            let mut s = SiteEvent::from_points(p2, p1);
            s.set_source_category(SourceCategory::REVERSE_SEGMENT);
            s
        };
        segment.set_initial_index(self.index);
        self.site_events.push(segment);
        let index = self.index;
        self.index += 1;
        index
    }

    /// `construct(output)`: runs the sweep and fills the output.
    pub fn construct<O: VoronoiOutput<Edge = E>>(&mut self, output: &mut O) {
        // Init structures.
        output.reserve(self.site_events.len());
        self.init_sites_queue();
        self.init_beach_line(output);

        // The algorithm stops when there are no events to process.
        let event_comparison = EventComparisonPredicate::default();
        while !self.circle_events.empty() || self.site_event_iterator != self.site_events.len() {
            if self.circle_events.empty() {
                self.process_site_event(output);
            } else if self.site_event_iterator == self.site_events.len() {
                self.process_circle_event(output);
            } else if event_comparison.site_circle(
                &self.site_events[self.site_event_iterator],
                &self.circle_events.top().0,
            ) {
                self.process_site_event(output);
            } else {
                self.process_circle_event(output);
            }
            while !self.circle_events.empty() && !self.circle_events.top().0.is_active() {
                self.circle_events.pop();
            }
        }
        self.beach_line.clear();

        // Finish construction.
        output.build();
    }

    /// `clear()`: the input index back to 0 and no sites.
    pub fn clear(&mut self) {
        self.index = 0;
        self.site_events.clear();
    }

    /// The site events: as inserted before `construct`, sorted, without duplicates and numbered
    /// after it. Not in the C++ (the probe test reads the sorted sites through it).
    #[must_use]
    pub fn site_events(&self) -> &[SiteEvent] {
        &self.site_events
    }

    /// `init_sites_queue()`: sorts the sites, removes duplicates, numbers them and starts the site
    /// iterator.
    fn init_sites_queue(&mut self) {
        // Sort site events.
        let cmp = EventComparisonPredicate::default();
        orca_stl::sort_by(&mut self.site_events, |a, b| cmp.site_site(a, b));

        // Remove duplicates.
        orca_stl::unique_vec_by(&mut self.site_events, |a, b| a == b);

        // Index sites.
        for (cur, site) in self.site_events.iter_mut().enumerate() {
            site.set_sorted_index(cur);
        }

        // Init site iterator.
        self.site_event_iterator = 0;
    }

    /// `init_beach_line(output)`.
    fn init_beach_line<O: VoronoiOutput<Edge = E>>(&mut self, output: &mut O) {
        if self.site_events.is_empty() {
            return;
        }
        if self.site_events.len() == 1 {
            // Handle single site event case.
            output.process_single_site(&self.site_events[0]);
            self.site_event_iterator += 1;
        } else {
            let mut skip = 0;

            while self.site_event_iterator != self.site_events.len()
                && is_vertical_points(
                    self.site_events[self.site_event_iterator].point0(),
                    self.site_events[0].point0(),
                )
                && is_vertical(&self.site_events[self.site_event_iterator])
            {
                self.site_event_iterator += 1;
                skip += 1;
            }

            if skip == 1 {
                // Init beach line with the first two sites.
                self.init_beach_line_default(output);
            } else {
                // Init beach line with collinear vertical sites.
                self.init_beach_line_collinear_sites(output);
            }
        }
    }

    /// `init_beach_line_default(output)`: the beach line from the first two sites (the first is
    /// always a point).
    fn init_beach_line_default<O: VoronoiOutput<Edge = E>>(&mut self, output: &mut O) {
        // Get the first and the second site event.
        let it_first = self.site_events[0];
        let it_second = self.site_events[1];
        let end = self.beach_line.end();
        self.insert_new_arc(&it_first, &it_first, &it_second, end, output);
        // The second site was already processed. Move the iterator.
        self.site_event_iterator += 1;
    }

    /// `init_beach_line_collinear_sites(output)`: a bisector for each pair of consecutive sites
    /// of the leading vertical run.
    fn init_beach_line_collinear_sites<O: VoronoiOutput<Edge = E>>(&mut self, output: &mut O) {
        let mut it_first = 0;
        let mut it_second = 1;
        while it_second != self.site_event_iterator {
            let first = self.site_events[it_first];
            let second = self.site_events[it_second];
            // Create a new beach line node.
            let new_node = BeachLineNodeKey::new(&first, &second);

            // Update the output.
            let edge = output.insert_new_edge(&first, &second).0;

            // Insert a new bisector into the beach line.
            let end = self.beach_line.end();
            self.beach_line
                .insert_hint(end, new_node, BeachLineNodeData::new(Some(edge)));

            // Update iterators.
            it_first += 1;
            it_second += 1;
        }
    }

    /// `deactivate_circle_event(value)`: deactivates the node's circle event, if it has one, and
    /// forgets it.
    fn deactivate_circle_event(&mut self, node: usize) {
        if let Some(circle) = self.beach_line.value(node).circle_event() {
            self.circle_events.get_mut(circle).0.deactivate();
            self.beach_line.value_mut(node).set_circle_event(None);
        }
    }

    /// `process_site_event(output)`: the next site event, and those segments that start at the
    /// same point with it.
    fn process_site_event<O: VoronoiOutput<Edge = E>>(&mut self, output: &mut O) {
        // Get next site event to process.
        let mut site_event = self.site_events[self.site_event_iterator];

        // Move site iterator.
        let mut last = self.site_event_iterator + 1;

        // If a new site is an end point of some segment,
        // remove temporary nodes from the beach line data structure.
        if !site_event.is_segment() {
            while let Some(top) = self.end_points.top() {
                if top.0 != *site_event.point0() {
                    break;
                }
                let b_it = top.1;
                self.end_points.pop();
                self.beach_line.erase(b_it);
            }
        } else {
            while last != self.site_events.len()
                && self.site_events[last].is_segment()
                && *self.site_events[last].point0() == *site_event.point0()
            {
                last += 1;
            }
        }

        // Find the node in the binary search tree with left arc
        // lying above the new site point.
        let new_key = BeachLineNodeKey::from_site(&self.site_events[self.site_event_iterator]);
        let mut right_it = self.beach_line.lower_bound(&new_key);

        while self.site_event_iterator != last {
            site_event = self.site_events[self.site_event_iterator];
            let mut left_it = right_it;

            // Do further processing depending on the above node position.
            // For any two neighboring nodes the second site of the first node
            // is the same as the first site of the second node.
            if right_it == self.beach_line.end() {
                // The above arc corresponds to the second arc of the last node.
                // Move the iterator to the last node.
                left_it = self.beach_line.prev(left_it);

                // Get the second site of the last node
                let site_arc = *self.beach_line.key(left_it).right_site();

                // Insert new nodes into the beach line. Update the output.
                right_it = self.insert_new_arc(&site_arc, &site_arc, &site_event, right_it, output);

                // Add a candidate circle to the circle event queue.
                // There could be only one new circle event formed by
                // a new bisector and the one on the left.
                let left_key = *self.beach_line.key(left_it);
                self.activate_circle_event(
                    left_key.left_site(),
                    left_key.right_site(),
                    &site_event,
                    right_it,
                );
            } else if right_it == self.beach_line.begin() {
                // The above arc corresponds to the first site of the first node.
                let site_arc = *self.beach_line.key(right_it).left_site();

                // Insert new nodes into the beach line. Update the output.
                left_it = self.insert_new_arc(&site_arc, &site_arc, &site_event, right_it, output);

                // If the site event is a segment, update its direction.
                if site_event.is_segment() {
                    site_event.inverse();
                }

                // Add a candidate circle to the circle event queue.
                // There could be only one new circle event formed by
                // a new bisector and the one on the right.
                let right_key = *self.beach_line.key(right_it);
                self.activate_circle_event(
                    &site_event,
                    right_key.left_site(),
                    right_key.right_site(),
                    right_it,
                );
                right_it = left_it;
            } else {
                // The above arc corresponds neither to the first,
                // nor to the last site in the beach line.
                let site_arc2 = *self.beach_line.key(right_it).left_site();
                let site3 = *self.beach_line.key(right_it).right_site();

                // Remove the candidate circle from the event queue.
                self.deactivate_circle_event(right_it);
                left_it = self.beach_line.prev(left_it);
                let site_arc1 = *self.beach_line.key(left_it).right_site();
                let site1 = *self.beach_line.key(left_it).left_site();

                // Insert new nodes into the beach line. Update the output.
                let new_node_it =
                    self.insert_new_arc(&site_arc1, &site_arc2, &site_event, right_it, output);

                // Add candidate circles to the circle event queue.
                // There could be up to two circle events formed by
                // a new bisector and the one on the left or right.
                self.activate_circle_event(&site1, &site_arc1, &site_event, new_node_it);

                // If the site event is a segment, update its direction.
                if site_event.is_segment() {
                    site_event.inverse();
                }
                self.activate_circle_event(&site_event, &site_arc2, &site3, right_it);
                right_it = new_node_it;
            }
            self.site_event_iterator += 1;
        }
    }

    /// `process_circle_event(output)`: the top circle event of sites A, B and C removes the
    /// bisector (B, C), turns (A, B) into (A, C) and checks the new triples on both sides.
    fn process_circle_event<O: VoronoiOutput<Edge = E>>(&mut self, output: &mut O) {
        // Get the topmost circle event.
        let e = *self.circle_events.top();
        let circle_event = e.0;
        let mut it_first = e.1;
        let mut it_last = it_first;

        // Get the C site.
        let mut site3 = *self.beach_line.key(it_first).right_site();

        // Get the half-edge corresponding to the second bisector - (B, C).
        let bisector2 = self.beach_line.value(it_first).edge();

        // Get the half-edge corresponding to the first bisector - (A, B).
        it_first = self.beach_line.prev(it_first);
        let bisector1 = self.beach_line.value(it_first).edge();

        // Get the A site.
        let site1 = *self.beach_line.key(it_first).left_site();

        if !site1.is_segment() && site3.is_segment() && *site3.point1() == *site1.point0() {
            site3.inverse();
        }

        // Change the (A, B) bisector node to the (A, C) bisector node.
        self.beach_line.key_mut(it_first).set_right_site(&site3);

        // Insert the new bisector into the beach line.
        let edge = output
            .insert_new_edge_at_circle(&site1, &site3, &circle_event, bisector1, bisector2)
            .0;
        self.beach_line.value_mut(it_first).set_edge(Some(edge));

        // Remove the (B, C) bisector node from the beach line.
        self.beach_line.erase(it_last);
        it_last = it_first;

        // Pop the topmost circle event from the event queue.
        self.circle_events.pop();

        // Check new triplets formed by the neighboring arcs
        // to the left for potential circle events.
        if it_first != self.beach_line.begin() {
            self.deactivate_circle_event(it_first);
            it_first = self.beach_line.prev(it_first);
            let site_l1 = *self.beach_line.key(it_first).left_site();
            self.activate_circle_event(&site_l1, &site1, &site3, it_last);
        }

        // Check the new triplet formed by the neighboring arcs
        // to the right for potential circle events.
        it_last = self.beach_line.next(it_last);
        if it_last != self.beach_line.end() {
            self.deactivate_circle_event(it_last);
            let site_r1 = *self.beach_line.key(it_last).right_site();
            self.activate_circle_event(&site1, &site3, &site_r1, it_last);
        }
    }

    /// `insert_new_arc(site_arc1, site_arc2, site_event, position, output)`: the two new
    /// bisectors of a site under the arc of `site_arc1` and `site_arc2` (and a temporary one for a
    /// segment), inserted before `position`; returns the left one's node.
    fn insert_new_arc<O: VoronoiOutput<Edge = E>>(
        &mut self,
        site_arc1: &SiteEvent,
        site_arc2: &SiteEvent,
        site_event: &SiteEvent,
        mut position: usize,
        output: &mut O,
    ) -> usize {
        // Create two new bisectors with opposite directions.
        let new_left_node = BeachLineNodeKey::new(site_arc1, site_event);
        let mut new_right_node = BeachLineNodeKey::new(site_event, site_arc2);

        // Set correct orientation for the first site of the second node.
        if site_event.is_segment() {
            new_right_node.left_site_mut().inverse();
        }

        // Update the output.
        let edges = output.insert_new_edge(site_arc2, site_event);
        position = self.beach_line.insert_hint(
            position,
            new_right_node,
            BeachLineNodeData::new(Some(edges.1)),
        );

        if site_event.is_segment() {
            // Update the beach line with temporary bisector, that will
            // disappear after processing site event corresponding to the
            // second endpoint of the segment site.
            let mut new_node = BeachLineNodeKey::new(site_event, site_event);
            new_node.right_site_mut().inverse();
            position =
                self.beach_line
                    .insert_hint(position, new_node, BeachLineNodeData::new(None));

            // Update the data structure that holds temporary bisectors.
            self.end_points.push((*site_event.point1(), position));
        }

        position = self.beach_line.insert_hint(
            position,
            new_left_node,
            BeachLineNodeData::new(Some(edges.0)),
        );

        position
    }

    /// `activate_circle_event(site1, site2, site3, bisector_node)`: queues the circle event of the
    /// three sites, if they make one, and links it to the (site2, site3) bisector's node.
    fn activate_circle_event(
        &mut self,
        site1: &SiteEvent,
        site2: &SiteEvent,
        site3: &SiteEvent,
        bisector_node: usize,
    ) {
        let mut c_event = CircleEvent::default();
        // Check if the three input sites create a circle event.
        if self
            .circle_formation_predicate
            .call(site1, site2, site3, &mut c_event)
        {
            // Add the new circle event to the circle events queue.
            // Update bisector's circle event iterator to point to the
            // new circle event in the circle event queue.
            let e = self.circle_events.push((c_event, bisector_node));
            self.beach_line
                .value_mut(bisector_node)
                .set_circle_event(Some(e));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CircleEvent, SiteEvent, VoronoiBuilder, VoronoiOutput};

    /// Counts what the builder asks of its output; edges are numbered as a diagram numbers them.
    #[derive(Default)]
    struct Count {
        reserved: usize,
        single: usize,
        edges: usize,
        vertices: usize,
        built: bool,
    }

    impl VoronoiOutput for Count {
        type Edge = usize;
        fn reserve(&mut self, num_sites: usize) {
            self.reserved = num_sites;
        }
        fn process_single_site(&mut self, _site: &SiteEvent) {
            self.single += 1;
        }
        fn insert_new_edge(&mut self, _site1: &SiteEvent, _site2: &SiteEvent) -> (usize, usize) {
            self.edges += 2;
            (self.edges - 2, self.edges - 1)
        }
        fn insert_new_edge_at_circle(
            &mut self,
            _site1: &SiteEvent,
            _site3: &SiteEvent,
            _circle: &CircleEvent,
            data12: Option<usize>,
            data23: Option<usize>,
        ) -> (usize, usize) {
            assert!(data12.is_some() && data23.is_some());
            self.vertices += 1;
            self.edges += 2;
            (self.edges - 2, self.edges - 1)
        }
        fn build(&mut self) {
            self.built = true;
        }
    }

    #[test]
    fn a_square_of_points_has_one_vertex() {
        // Four corners of a square: one Voronoi vertex (the centre, which the four bisectors
        // share; the builder makes it from two circle events at the same place), four bisectors.
        let mut b = VoronoiBuilder::new();
        for (x, y) in [(0, 0), (10, 0), (0, 10), (10, 10)] {
            b.insert_point(x, y);
        }
        let mut out = Count::default();
        b.construct(&mut out);
        assert_eq!(out.reserved, 4);
        assert!(out.built);
        assert_eq!(out.single, 0);
        assert_eq!(out.vertices, 2);
        assert_eq!(out.edges, 2 * 5);
        assert_eq!(b.site_events().len(), 4);
    }

    #[test]
    fn one_segment_has_its_three_sites() {
        let mut b = VoronoiBuilder::new();
        assert_eq!(b.insert_segment(5, 5, 0, 0), 0);
        let mut out = Count::default();
        b.construct(&mut out);
        assert_eq!(out.reserved, 3);
        let s = b.site_events();
        assert_eq!(s.len(), 3);
        // The point (0, 0), the segment from it, then the point (5, 5).
        assert!(!s[0].is_segment() && s[1].is_segment() && !s[2].is_segment());
        assert_eq!((s[1].x0(), s[1].y0(), s[1].x1(), s[1].y1()), (0, 0, 5, 5));
        assert_eq!(out.vertices, 0);
        assert_eq!(out.edges, 2 * 2);
    }
}
