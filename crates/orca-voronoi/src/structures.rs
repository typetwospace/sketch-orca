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
// from boost/polygon/detail/voronoi_structures.hpp, lines 1-450 (point_2d, site_event,
// circle_event, ordered_queue, beach_line_node_key and beach_line_node_data). Copyright Andrii
// Sydorchuk 2010-2012, under the Boost Software License, version 1.0 (BSL-1.0;
// licenses/BSL-1.0.txt).

//! The data structures of Boost.Polygon's Voronoi sweep (`voronoi_structures.hpp`), for the only
//! instantiation OrcaSlicer makes: `int32` input coordinates and `double` circle events
//! (`voronoi_builder<int32>`).
//!
//! - [`Point2d`] is `point_2d<int32>`.
//! - [`SiteEvent`] is `site_event<int32>`: a point site, or a segment from `point0` to `point1`,
//!   with its index among the sorted sites, its index among the input and its flags (the source
//!   category in the low five bits, `IS_INVERSE` = 0x20). `inverse()` swaps the two points and
//!   flips the inverse bit; `source_category(c)` ORs `c` into the flags, as the C++ does, and the
//!   getter reads the flags back through `SOURCE_CATEGORY_BITMASK`. Equality is that of the two
//!   points only (`std::unique` in `init_sites_queue` relies on it).
//! - [`CircleEvent`] is `circle_event<double>`: the centre, the x of the circle's rightmost point
//!   (`lower_x`, the sweep line's position when the event fires) and whether it is still active.
//! - [`OrderedQueue`] is `ordered_queue<T, Predicate>`: a `std::list` that holds the events, so a
//!   reference to one stays valid, and a `std::priority_queue` of list iterators ordered by the
//!   predicate on the elements. Here the list is a slab of slots (a popped element's slot is
//!   reused by a later push, as a freed list node's memory may be), a list iterator is a slot
//!   index, and the priority queue is a vector of slot indices that `orca-stl`'s
//!   `push_heap_by` and `pop_heap_by` order as MSVC's `std::priority_queue` does, so events that
//!   compare equal pop in MSVC's order.
//! - [`BeachLineNodeKey`] is `beach_line_node_key<site_event<int32>>`, a bisector of two sites,
//!   and [`BeachLineNodeData`] is `beach_line_node_data<void, circle_event<double>>`, the edge
//!   that bisector traces and the circle event it is the right bisector of. The C++ holds two
//!   pointers; here the circle event is the [`OrderedQueue`] slot that holds it and the edge is
//!   the output's own handle `E` (module `builder`, `VoronoiOutput`); `None` is the null pointer.
//!
//! The four types the predicates read implement the traits of module `predicates`
//! ([`VoronoiPoint`], [`VoronoiSite`], [`VoronoiCircle`] and its `BeachLineNodeKey` trait). Each
//! C++ getter and setter pair is a getter of the C++ name and a setter `set_<name>`.
//!
//! Left out: the constructors that leave members unset (`point_2d()`, `circle_event()`'s
//! coordinates, every `site_event` constructor's `initial_index_`) set them to zero here; the C++
//! reads none of them before it writes them. `ordered_queue`'s copy constructor and assignment
//! are disallowed in the C++, and no `Clone` is given here.

use crate::geometry_type::SourceCategory;
use crate::predicates::{self, VoronoiCircle, VoronoiPoint, VoronoiSite};

// ---------------------------------------------------------------- point_2d (22-63)

/// `point_2d<int32>`: a point of the input.
#[derive(Clone, Copy, Debug, Default)]
pub struct Point2d {
    x: i32,
    y: i32,
}

impl Point2d {
    /// `point_2d(x, y)`.
    #[must_use]
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// `x()`.
    #[must_use]
    pub fn x(&self) -> i32 {
        self.x
    }

    /// `y()`.
    #[must_use]
    pub fn y(&self) -> i32 {
        self.y
    }

    /// `x(coordinate_type)`.
    pub fn set_x(&mut self, x: i32) -> &mut Self {
        self.x = x;
        self
    }

    /// `y(coordinate_type)`.
    pub fn set_y(&mut self, y: i32) -> &mut Self {
        self.y = y;
        self
    }
}

impl PartialEq for Point2d {
    /// `operator==`: both coordinates equal; `operator!=` (`x != x || y != y`) is its negation.
    fn eq(&self, that: &Self) -> bool {
        (self.x == that.x()) && (self.y == that.y())
    }
}

impl VoronoiPoint for Point2d {
    fn x(&self) -> i32 {
        Point2d::x(self)
    }
    fn y(&self) -> i32 {
        Point2d::y(self)
    }
}

// ---------------------------------------------------------------- site_event (65-220)

/// `site_event::Bits::IS_INVERSE`.
const IS_INVERSE: usize = 0x20;

/// `site_event<int32>`: a site of the sweep (a point, or a segment whose start point compares
/// less than its end point until `inverse()` swaps them).
#[derive(Clone, Copy, Debug, Default)]
pub struct SiteEvent {
    point0: Point2d,
    point1: Point2d,
    sorted_index: usize,
    initial_index: usize,
    flags: usize,
}

impl SiteEvent {
    /// `site_event(x, y)`: a point site.
    #[must_use]
    pub fn from_xy(x: i32, y: i32) -> Self {
        Self {
            point0: Point2d::new(x, y),
            point1: Point2d::new(x, y),
            sorted_index: 0,
            initial_index: 0,
            flags: 0,
        }
    }

    /// `explicit site_event(const point_type&)`: a point site.
    #[must_use]
    pub fn from_point(point: Point2d) -> Self {
        Self {
            point0: point,
            point1: point,
            sorted_index: 0,
            initial_index: 0,
            flags: 0,
        }
    }

    /// `site_event(x1, y1, x2, y2)`: a segment site.
    #[must_use]
    pub fn from_coords(x1: i32, y1: i32, x2: i32, y2: i32) -> Self {
        Self {
            point0: Point2d::new(x1, y1),
            point1: Point2d::new(x2, y2),
            sorted_index: 0,
            initial_index: 0,
            flags: 0,
        }
    }

    /// `site_event(const point_type&, const point_type&)`: a segment site.
    #[must_use]
    pub fn from_points(point1: Point2d, point2: Point2d) -> Self {
        Self {
            point0: point1,
            point1: point2,
            sorted_index: 0,
            initial_index: 0,
            flags: 0,
        }
    }

    /// `x()`: `point0_.x()`.
    #[must_use]
    pub fn x(&self) -> i32 {
        self.point0.x()
    }

    /// `y()`: `point0_.y()`.
    #[must_use]
    pub fn y(&self) -> i32 {
        self.point0.y()
    }

    /// `x0()`.
    #[must_use]
    pub fn x0(&self) -> i32 {
        self.point0.x()
    }

    /// `y0()`.
    #[must_use]
    pub fn y0(&self) -> i32 {
        self.point0.y()
    }

    /// `x1()`.
    #[must_use]
    pub fn x1(&self) -> i32 {
        self.point1.x()
    }

    /// `y1()`.
    #[must_use]
    pub fn y1(&self) -> i32 {
        self.point1.y()
    }

    /// `point0()`.
    #[must_use]
    pub fn point0(&self) -> &Point2d {
        &self.point0
    }

    /// `point1()`.
    #[must_use]
    pub fn point1(&self) -> &Point2d {
        &self.point1
    }

    /// `sorted_index()`.
    #[must_use]
    pub fn sorted_index(&self) -> usize {
        self.sorted_index
    }

    /// `sorted_index(std::size_t)`.
    pub fn set_sorted_index(&mut self, index: usize) -> &mut Self {
        self.sorted_index = index;
        self
    }

    /// `initial_index()`.
    #[must_use]
    pub fn initial_index(&self) -> usize {
        self.initial_index
    }

    /// `initial_index(std::size_t)`.
    pub fn set_initial_index(&mut self, index: usize) -> &mut Self {
        self.initial_index = index;
        self
    }

    /// `is_inverse()`: the `IS_INVERSE` bit of the flags.
    #[must_use]
    pub fn is_inverse(&self) -> bool {
        (self.flags & IS_INVERSE) != 0
    }

    /// `inverse()`: swaps the two points and flips the `IS_INVERSE` bit.
    pub fn inverse(&mut self) -> &mut Self {
        std::mem::swap(&mut self.point0, &mut self.point1);
        self.flags ^= IS_INVERSE;
        self
    }

    /// `source_category()`: `static_cast<SourceCategory>(flags_ & SOURCE_CATEGORY_BITMASK)`.
    #[must_use]
    pub fn source_category(&self) -> SourceCategory {
        SourceCategory((self.flags & SourceCategory::BITMASK.bits()) as u32)
    }

    /// `source_category(SourceCategory)`: `flags_ |= source_category`.
    pub fn set_source_category(&mut self, source_category: SourceCategory) -> &mut Self {
        self.flags |= source_category.bits();
        self
    }

    /// `is_point()`: the two points are equal.
    #[must_use]
    pub fn is_point(&self) -> bool {
        (self.point0.x() == self.point1.x()) && (self.point0.y() == self.point1.y())
    }

    /// `is_segment()`: the two points differ.
    #[must_use]
    pub fn is_segment(&self) -> bool {
        (self.point0.x() != self.point1.x()) || (self.point0.y() != self.point1.y())
    }
}

impl PartialEq for SiteEvent {
    /// `operator==`: the same two points, whatever the indexes and flags; `operator!=` is its
    /// negation.
    fn eq(&self, that: &Self) -> bool {
        (self.point0 == that.point0) && (self.point1 == that.point1)
    }
}

impl VoronoiSite for SiteEvent {
    type Point = Point2d;
    fn x(&self) -> i32 {
        SiteEvent::x(self)
    }
    fn y(&self) -> i32 {
        SiteEvent::y(self)
    }
    fn x0(&self) -> i32 {
        SiteEvent::x0(self)
    }
    fn y0(&self) -> i32 {
        SiteEvent::y0(self)
    }
    fn x1(&self) -> i32 {
        SiteEvent::x1(self)
    }
    fn y1(&self) -> i32 {
        SiteEvent::y1(self)
    }
    fn point0(&self) -> &Point2d {
        SiteEvent::point0(self)
    }
    fn point1(&self) -> &Point2d {
        SiteEvent::point1(self)
    }
    fn sorted_index(&self) -> usize {
        SiteEvent::sorted_index(self)
    }
    fn is_inverse(&self) -> bool {
        SiteEvent::is_inverse(self)
    }
    fn is_segment(&self) -> bool {
        SiteEvent::is_segment(self)
    }
}

// ---------------------------------------------------------------- circle_event (222-294)

/// `circle_event<double>`: the circle through three sites, which fires when the sweep line
/// reaches its rightmost point. Its lowest point's y is the centre's (`lower_y()`).
#[derive(Clone, Copy, Debug)]
pub struct CircleEvent {
    center_x: f64,
    center_y: f64,
    lower_x: f64,
    is_active: bool,
}

impl Default for CircleEvent {
    /// `circle_event()`: active; the C++ leaves the coordinates unset, and they are zero here.
    fn default() -> Self {
        Self {
            center_x: 0.0,
            center_y: 0.0,
            lower_x: 0.0,
            is_active: true,
        }
    }
}

impl CircleEvent {
    /// `circle_event(c_x, c_y, lower_x)`: active.
    #[must_use]
    pub fn new(c_x: f64, c_y: f64, lower_x: f64) -> Self {
        Self {
            center_x: c_x,
            center_y: c_y,
            lower_x,
            is_active: true,
        }
    }

    /// `x()`: the centre's x.
    #[must_use]
    pub fn x(&self) -> f64 {
        self.center_x
    }

    /// `x(coordinate_type)`.
    pub fn set_x(&mut self, center_x: f64) -> &mut Self {
        self.center_x = center_x;
        self
    }

    /// `y()`: the centre's y.
    #[must_use]
    pub fn y(&self) -> f64 {
        self.center_y
    }

    /// `y(coordinate_type)`.
    pub fn set_y(&mut self, center_y: f64) -> &mut Self {
        self.center_y = center_y;
        self
    }

    /// `lower_x()`: the x of the circle's rightmost point.
    #[must_use]
    pub fn lower_x(&self) -> f64 {
        self.lower_x
    }

    /// `lower_x(coordinate_type)`.
    pub fn set_lower_x(&mut self, lower_x: f64) -> &mut Self {
        self.lower_x = lower_x;
        self
    }

    /// `lower_y()`: the centre's y.
    #[must_use]
    pub fn lower_y(&self) -> f64 {
        self.center_y
    }

    /// `is_active()`.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.is_active
    }

    /// `deactivate()`.
    pub fn deactivate(&mut self) -> &mut Self {
        self.is_active = false;
        self
    }
}

impl VoronoiCircle for CircleEvent {
    fn lower_x(&self) -> f64 {
        CircleEvent::lower_x(self)
    }
    fn y(&self) -> f64 {
        CircleEvent::y(self)
    }
}

// ---------------------------------------------------------------- ordered_queue (296-353)

/// `ordered_queue<T, Predicate>`: the circle event queue. `pred(a, b)` is the C++ `Predicate`,
/// "a goes after b" in the queue (the queue's top is the element no other goes after, as in a
/// `std::priority_queue` ordered by `pred`).
pub struct OrderedQueue<T, P> {
    /// `c_`, the `std::priority_queue`'s vector of list iterators (slot indices).
    c: Vec<usize>,
    /// `c_list_`, the `std::list` that holds the elements: a slot per element, `None` when free.
    list: Vec<Option<T>>,
    /// The free slots of `list`, the last freed first.
    free: Vec<usize>,
    /// `comparison::cmp_`.
    cmp: P,
}

impl<T, P: Fn(&T, &T) -> bool> OrderedQueue<T, P> {
    /// `ordered_queue()`, with its predicate.
    pub fn new(cmp: P) -> Self {
        Self {
            c: Vec::new(),
            list: Vec::new(),
            free: Vec::new(),
            cmp,
        }
    }

    /// `empty()`.
    #[must_use]
    pub fn empty(&self) -> bool {
        self.c.is_empty()
    }

    /// `top()`: the element on top. The C++ is undefined on an empty queue; this panics.
    #[must_use]
    pub fn top(&self) -> &T {
        self.get(self.c[0])
    }

    /// The slot of the element on top (`c_.top()`, the list iterator).
    #[must_use]
    pub fn top_slot(&self) -> usize {
        self.c[0]
    }

    /// `pop()`: takes the top iterator, pops the heap, then erases the element from the list.
    pub fn pop(&mut self) {
        let it = self.c[0];
        let list = &self.list;
        let cmp = &self.cmp;
        orca_stl::pop_heap_by(&mut self.c, |it1, it2| {
            cmp(slot(list, *it1), slot(list, *it2))
        });
        self.c.pop();
        self.list[it] = None;
        self.free.push(it);
    }

    /// `push(e)`: `c_list_.push_front(e)`, then the new iterator into the heap; returns the slot
    /// of the element (the C++ returns a reference to it, which stays valid until it is popped).
    pub fn push(&mut self, e: T) -> usize {
        let it = if let Some(it) = self.free.pop() {
            self.list[it] = Some(e);
            it
        } else {
            self.list.push(Some(e));
            self.list.len() - 1
        };
        self.c.push(it);
        let list = &self.list;
        let cmp = &self.cmp;
        orca_stl::push_heap_by(&mut self.c, |it1, it2| {
            cmp(slot(list, *it1), slot(list, *it2))
        });
        it
    }

    /// The element in a slot `push` returned (the reference it returned).
    #[must_use]
    pub fn get(&self, it: usize) -> &T {
        slot(&self.list, it)
    }

    /// The element in a slot `push` returned, to change it (the reference it returned).
    pub fn get_mut(&mut self, it: usize) -> &mut T {
        self.list[it]
            .as_mut()
            .expect("ordered_queue: an element that was popped")
    }

    /// `clear()`: pops every iterator, then clears the list.
    pub fn clear(&mut self) {
        self.c.clear();
        self.list.clear();
        self.free.clear();
    }
}

/// The element a list iterator points at.
fn slot<T>(list: &[Option<T>], it: usize) -> &T {
    list[it]
        .as_ref()
        .expect("ordered_queue: an element that was popped")
}

// ---------------------------------------------------------------- beach_line_node_key (355-411)

/// `beach_line_node_key<site_event<int32>>`: the bisector of a left and a right site; the order
/// of the two sites picks one of the two opposite bisectors.
#[derive(Clone, Copy, Debug)]
pub struct BeachLineNodeKey {
    left_site: SiteEvent,
    right_site: SiteEvent,
}

impl BeachLineNodeKey {
    /// `explicit beach_line_node_key(const site_type& new_site)`: the degenerate bisector the
    /// beach line is searched with for the arc above a new site.
    #[must_use]
    pub fn from_site(new_site: &SiteEvent) -> Self {
        Self {
            left_site: *new_site,
            right_site: *new_site,
        }
    }

    /// `beach_line_node_key(left_site, right_site)`.
    #[must_use]
    pub fn new(left_site: &SiteEvent, right_site: &SiteEvent) -> Self {
        Self {
            left_site: *left_site,
            right_site: *right_site,
        }
    }

    /// `left_site() const`.
    #[must_use]
    pub fn left_site(&self) -> &SiteEvent {
        &self.left_site
    }

    /// `left_site()`, the non-const overload.
    pub fn left_site_mut(&mut self) -> &mut SiteEvent {
        &mut self.left_site
    }

    /// `left_site(const site_type&)`.
    pub fn set_left_site(&mut self, site: &SiteEvent) -> &mut Self {
        self.left_site = *site;
        self
    }

    /// `right_site() const`.
    #[must_use]
    pub fn right_site(&self) -> &SiteEvent {
        &self.right_site
    }

    /// `right_site()`, the non-const overload.
    pub fn right_site_mut(&mut self) -> &mut SiteEvent {
        &mut self.right_site
    }

    /// `right_site(const site_type&)`.
    pub fn set_right_site(&mut self, site: &SiteEvent) -> &mut Self {
        self.right_site = *site;
        self
    }
}

impl predicates::BeachLineNodeKey for BeachLineNodeKey {
    type Site = SiteEvent;
    fn left_site(&self) -> &SiteEvent {
        BeachLineNodeKey::left_site(self)
    }
    fn right_site(&self) -> &SiteEvent {
        BeachLineNodeKey::right_site(self)
    }
}

// ---------------------------------------------------------------- beach_line_node_data (413-445)

/// `beach_line_node_data<void, circle_event<double>>`: the output edge a bisector traces and the
/// circle event (its slot in the [`OrderedQueue`]) whose right bisector it is.
#[derive(Clone, Copy, Debug)]
pub struct BeachLineNodeData<E> {
    circle_event: Option<usize>,
    edge: Option<E>,
}

impl<E: Copy> BeachLineNodeData<E> {
    /// `explicit beach_line_node_data(Edge* new_edge)`: no circle event.
    #[must_use]
    pub fn new(new_edge: Option<E>) -> Self {
        Self {
            circle_event: None,
            edge: new_edge,
        }
    }

    /// `circle_event()`.
    #[must_use]
    pub fn circle_event(&self) -> Option<usize> {
        self.circle_event
    }

    /// `circle_event(Circle*)`.
    pub fn set_circle_event(&mut self, circle_event: Option<usize>) -> &mut Self {
        self.circle_event = circle_event;
        self
    }

    /// `edge()`.
    #[must_use]
    pub fn edge(&self) -> Option<E> {
        self.edge
    }

    /// `edge(Edge*)`.
    pub fn set_edge(&mut self, new_edge: Option<E>) -> &mut Self {
        self.edge = new_edge;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{OrderedQueue, Point2d, SiteEvent, SourceCategory};

    #[test]
    fn inverse_swaps_points_and_keeps_the_category() {
        let mut s = SiteEvent::from_coords(1, 2, 3, 4);
        s.set_source_category(SourceCategory::REVERSE_SEGMENT);
        s.inverse();
        assert!(s.is_inverse());
        assert_eq!((s.x0(), s.y0(), s.x1(), s.y1()), (3, 4, 1, 2));
        assert_eq!(s.source_category(), SourceCategory::REVERSE_SEGMENT);
        s.inverse();
        assert!(!s.is_inverse());
        assert!(SiteEvent::from_point(Point2d::new(5, 5)).is_point());
    }

    #[test]
    fn the_queue_reuses_a_popped_slot() {
        let mut q = OrderedQueue::new(|a: &i32, b: &i32| a > b);
        let a = q.push(3);
        q.push(1);
        q.push(2);
        assert_eq!(*q.top(), 1);
        q.pop();
        let c = q.push(0);
        assert_eq!(*q.top(), 0);
        assert_ne!(a, c);
        assert_eq!(*q.get(a), 3);
    }
}
