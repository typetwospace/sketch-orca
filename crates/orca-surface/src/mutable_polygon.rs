// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/MutablePolygon.hpp lines 1-372 and
// src/libslic3r/MutablePolygon.cpp lines 1-387 of OrcaSlicer v2.4.2, whole, translated on
// 2026-11-13 by orca-o3-surfaces-1. Copyright the OrcaSlicer authors, the BambuStudio authors
// (Bambu Lab) and the PrusaSlicer authors (Prusa Research; clip_narrow_corner and smooth_outward
// were adapted there from Cura's ConstPolygonRef::smooth_corner_complex and smooth_outward by Tim
// Kuipers, Ultimaker, also under the GNU Affero General Public License, version 3), under the GNU
// Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! `MutablePolygon`: a polygon kept as a loop of doubly linked points in one vector, so that points
//! can be removed and inserted while it is walked, and the two passes that edit polygons with it:
//! `remove_duplicates` and `smooth_outward`.
//!
//! - [`MutablePolygon`] holds its points in one vector of `LinkedPoint`s (a point and the indices of
//!   the previous and the next), its size, the index of its first point and the head of its free
//!   list. A removed point's slot goes on the free list (its `next` becomes the old head) and the
//!   next insertion takes it back, as the C++ does; a slot keeps its `prev` while it is free.
//! - The C++ `iterator` and `const_iterator` hold the polygon and an index; here an [`Iter`] holds
//!   the index alone and the polygon is the argument of every step ([`MutablePolygon::next`],
//!   [`MutablePolygon::prev`], [`MutablePolygon::point`], [`MutablePolygon::point_mut`],
//!   [`MutablePolygon::remove`], [`MutablePolygon::insert`]). `end()` is the last point before the
//!   loop rolls over, not one past it; an index of -1 is not valid.
//! - [`Range`] is `MutablePolygon::range`, the inclusive range of points `smooth_outward` has not
//!   processed yet. Its begin and end are both -1 once it is empty.
//! - `remove_duplicates` in its three forms ([`remove_duplicates`], [`remove_duplicates_eps`],
//!   [`remove_duplicates_angle`]) and the ExPolygons one ([`remove_duplicates_expolygons`]), which
//!   only MultiMaterialSegmentation calls.
//! - [`smooth_outward`] (and its forms for a `Polygon`, `Polygons` and `ExPolygons`) clips sharp
//!   concave corners and fills narrow cracks and tiny clockwise triangles, with the static
//!   `clip_narrow_corner` inside it. Only `PrintObject::clip_fill_surfaces` (which returns at once,
//!   as `infill_only_where_needed` is false) and the supports call it.
//!
//! The arithmetic is the C++'s: `Vec2i64` products and sums in wrapping `i64`, `Vec2d` dot products
//! and squared norms as Eigen's two-lane packets give them (`orca_eigen`), `sqrt` and `cos` through
//! `orca-crt`, a `double` vector cast to `coord_t` truncated (`Point::cast_from_vec2d`).
//!
//! Left out: `capacity()`, which no engine code calls (Rust's vector grows by its own rule, so it
//! would not give MSVC's number), and the C++ iterators' `size()` and `polygon()`, which the
//! polygon argument makes redundant. The asserts and the `NDEBUG` checks of `clip_narrow_corner`
//! are not translated (the engine's release build drops them). Reading through an iterator that is
//! not valid is undefined behaviour in C++; here it panics.

use orca_eigen::Vec2d;
use orca_geom::expolygon::ExPolygons;
use orca_geom::point::{Coord, Point, SCALED_EPSILON, cross2_coord, scaled_f64};
use orca_geom::polygon::{Polygon, Polygons};

/// `MutablePolygon::IndexType`.
pub type IndexType = i32;

/// `MutablePolygon::iterator` and `const_iterator`: the index of a point in the polygon's vector
/// (`m_idx`), -1 when not valid. `==` compares the indices, as the release build does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Iter(IndexType);

impl Iter {
    /// An iterator at the index `idx`.
    #[must_use]
    pub const fn new(idx: IndexType) -> Self {
        Self(idx)
    }

    /// `m_idx`.
    #[must_use]
    pub const fn idx(self) -> IndexType {
        self.0
    }

    /// `valid()`: `m_idx >= 0`.
    #[must_use]
    pub const fn valid(self) -> bool {
        self.0 >= 0
    }
}

/// `MutablePolygon::LinkedPoint`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LinkedPoint {
    point: Point,
    prev: IndexType,
    next: IndexType,
}

/// `MutablePolygon`: a polygon as a loop of doubly linked points in one vector, addressed by index
/// so that the indices survive the vector's reallocation.
#[derive(Clone, Debug)]
pub struct MutablePolygon {
    /// `m_data`.
    data: Vec<LinkedPoint>,
    /// `m_size`: the number of points in the loop.
    size: IndexType,
    /// `m_head`.
    head: IndexType,
    /// `m_head_free`: the head of the free list.
    head_free: IndexType,
}

impl Default for MutablePolygon {
    /// `MutablePolygon()`.
    fn default() -> Self {
        Self {
            data: Vec::new(),
            size: 0,
            head: -1,
            head_free: -1,
        }
    }
}

impl MutablePolygon {
    /// `MutablePolygon()`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `MutablePolygon(const Polygon &rhs, size_t reserve)`.
    #[must_use]
    pub fn from_polygon(rhs: &Polygon, reserve: usize) -> Self {
        Self::from_points(&rhs.points, reserve)
    }

    /// `MutablePolygon(IT begin, IT end, size_t reserve)` and the `initializer_list` form.
    #[must_use]
    pub fn from_points(points: &[Point], reserve: usize) -> Self {
        let mut out = Self::default();
        out.assign_inner(points, reserve);
        out
    }

    /// `assign(IT begin, IT end, size_t reserve)`.
    pub fn assign(&mut self, points: &[Point], reserve: usize) {
        self.data.clear();
        self.head = -1;
        self.head_free = -1;
        self.assign_inner(points, reserve);
    }

    /// `assign(const Polygon &rhs, size_t reserve)`.
    pub fn assign_polygon(&mut self, rhs: &Polygon, reserve: usize) {
        self.assign(&rhs.points, reserve);
    }

    /// `polygon(Polygon &out)`: the points from `begin()` around the loop, none below 3 points.
    pub fn polygon_into(&self, out: &mut Polygon) {
        out.points.clear();
        if self.valid() {
            out.points.reserve(self.size());
            let begin = self.begin();
            let mut it = begin;
            out.points.push(self.point(it));
            it = self.next(it);
            while it != begin {
                out.points.push(self.point(it));
                it = self.next(it);
            }
        }
    }

    /// `polygon()`.
    #[must_use]
    pub fn polygon(&self) -> Polygon {
        let mut out = Polygon::default();
        self.polygon_into(&mut out);
        out
    }

    /// `empty()`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// `size()`: `m_size` as a `size_t`.
    #[must_use]
    pub fn size(&self) -> usize {
        self.size as usize
    }

    /// `valid()`: at least 3 points.
    #[must_use]
    pub fn valid(&self) -> bool {
        self.size >= 3
    }

    /// `clear()`.
    pub fn clear(&mut self) {
        self.data.clear();
        self.size = 0;
        self.head = -1;
        self.head_free = -1;
    }

    /// `begin()` and `cbegin()`.
    #[must_use]
    pub fn begin(&self) -> Iter {
        Iter(self.head)
    }

    /// `end()` and `cend()`: the last point before the loop rolls over (-1 when empty).
    #[must_use]
    pub fn end(&self) -> Iter {
        Iter(if self.is_empty() {
            -1
        } else {
            self.at(self.head).prev
        })
    }

    /// `remove(it)` and `it.remove()`: the iterator following the removed point, not valid once the
    /// last point is removed. Removing `begin()` makes the next point the new `begin()`.
    pub fn remove(&mut self, it: Iter) -> Iter {
        Iter(self.remove_idx(it.0))
    }

    /// `insert(it, pt)` and `it.insert(pt)`: inserts `pt` before `it` and returns its iterator.
    /// `begin()` does not change; `end()` may become the new point.
    pub fn insert(&mut self, it: Iter, pt: Point) -> Iter {
        Iter(self.insert_idx(it.0, pt))
    }

    /// `++it` and `it.next()`.
    #[must_use]
    pub fn next(&self, it: Iter) -> Iter {
        Iter(self.at(it.0).next)
    }

    /// `--it` and `it.prev()`.
    #[must_use]
    pub fn prev(&self, it: Iter) -> Iter {
        Iter(self.at(it.0).prev)
    }

    /// `*it`.
    #[must_use]
    pub fn point(&self, it: Iter) -> Point {
        self.at(it.0).point
    }

    /// `*it` to write through.
    pub fn point_mut(&mut self, it: Iter) -> &mut Point {
        &mut self.at_mut(it.0).point
    }

    /// `at(i)`: `m_data[i]`.
    fn at(&self, i: IndexType) -> &LinkedPoint {
        &self.data[slot(i)]
    }

    /// `at(i)` to write through.
    fn at_mut(&mut self, i: IndexType) -> &mut LinkedPoint {
        let i = slot(i);
        &mut self.data[i]
    }

    /// `assign_inner(begin, end, reserve)`: the points linked in order, the first after the last.
    fn assign_inner(&mut self, points: &[Point], reserve: usize) {
        self.size = points.len() as IndexType;
        if self.size > 0 {
            self.head = 0;
            let want = orca_stl::max(self.size as usize, reserve);
            if want > self.data.capacity() {
                self.data.reserve_exact(want - self.data.len());
            }
            let mut i: IndexType = -1;
            let mut j: IndexType = 1;
            for &point in points {
                self.data.push(LinkedPoint {
                    point,
                    prev: i,
                    next: j,
                });
                i = i.wrapping_add(1);
                j = j.wrapping_add(1);
            }
            let last = self.size - 1;
            if let Some(front) = self.data.first_mut() {
                front.prev = last;
            }
            if let Some(back) = self.data.last_mut() {
                back.next = 0;
            }
        }
    }

    /// `remove(IndexType i)`: unlinks the point and puts its slot at the head of the free list.
    fn remove_idx(&mut self, i: IndexType) -> IndexType {
        let head_free = self.head_free;
        let lp = self.at_mut(i);
        let prev = lp.prev;
        let next = lp.next;
        lp.next = head_free;
        self.head_free = i;
        self.size -= 1;
        if self.size == 0 {
            self.head = -1;
        } else if self.head == i {
            self.head = next;
        }
        if self.is_empty() {
            return -1;
        }
        self.at_mut(prev).next = next;
        self.at_mut(next).prev = prev;
        next
    }

    /// `insert(IndexType i, Point pt)`: a new slot, or the head of the free list, linked before `i`.
    fn insert_idx(&mut self, i: IndexType, pt: Point) -> IndexType {
        let j = self.at(i).prev;
        let n;
        if self.head_free == -1 {
            // Allocate a new item.
            n = self.data.len() as IndexType;
            self.data.push(LinkedPoint {
                point: pt,
                prev: j,
                next: i,
            });
        } else {
            n = self.head_free;
            let nlp = self.at_mut(n);
            let next_free = nlp.next;
            *nlp = LinkedPoint {
                point: pt,
                prev: j,
                next: i,
            };
            self.head_free = next_free;
        }
        self.at_mut(j).next = n;
        self.at_mut(i).prev = n;
        self.size += 1;
        n
    }
}

/// An index of the vector, or a panic where the C++ would read outside it.
fn slot(i: IndexType) -> usize {
    usize::try_from(i).unwrap_or_else(|_| {
        panic!("MutablePolygon: an iterator that is not valid was read (index {i}; the C++ reads outside its vector)")
    })
}

impl PartialEq for MutablePolygon {
    /// `operator==(p1, p2)`: the same size and the same points from each `begin()` on.
    fn eq(&self, p2: &Self) -> bool {
        if self.size() != p2.size() {
            return false;
        }
        if self.is_empty() {
            return true;
        }
        let begin = self.begin();
        let mut it = begin;
        let mut it2 = p2.begin();
        loop {
            if self.point(it) != p2.point(it2) {
                return false;
            }
            it = self.next(it);
            if it == begin {
                return true;
            }
            it2 = p2.next(it2);
        }
    }
}

/// `MutablePolygon::range`: an inclusive range of points not processed yet, see [`smooth_outward`].
/// Empty when its begin is not valid; then both ends are -1. The C++ range holds iterators into
/// the polygon; here the polygon is the argument of the steps that walk or remove.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    /// `m_begin`.
    begin: Iter,
    /// `m_end`.
    end: Iter,
}

impl Range {
    /// `range(MutablePolygon &poly)`: from `begin()` to `end()`.
    #[must_use]
    pub fn new(poly: &MutablePolygon) -> Self {
        Self::from_iters(poly.begin(), poly.end())
    }

    /// `range(begin, end)`.
    #[must_use]
    pub const fn from_iters(begin: Iter, end: Iter) -> Self {
        Self { begin, end }
    }

    /// `begin()`: the first point, inclusive.
    #[must_use]
    pub const fn begin(&self) -> Iter {
        self.begin
    }

    /// `end()`: the last point, inclusive.
    #[must_use]
    pub const fn end(&self) -> Iter {
        self.end
    }

    /// `empty()`.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        !self.begin.valid()
    }

    /// `process_next()`: returns `begin()` and advances the front.
    pub fn process_next(&mut self, poly: &MutablePolygon) -> Iter {
        let out = self.begin;
        self.advance_front(poly);
        out
    }

    /// `advance_front()`.
    pub fn advance_front(&mut self, poly: &MutablePolygon) {
        if self.begin == self.end {
            self.make_empty();
        } else {
            self.begin = poly.next(self.begin);
        }
    }

    /// `retract_back()`.
    pub fn retract_back(&mut self, poly: &MutablePolygon) {
        if self.begin == self.end {
            self.make_empty();
        } else {
            self.end = poly.prev(self.end);
        }
    }

    /// `remove_front(it)`: advances the front first when `it` is the range's begin.
    pub fn remove_front(&mut self, poly: &mut MutablePolygon, it: Iter) -> Iter {
        if !self.is_empty() && self.begin == it {
            self.advance_front(poly);
        }
        poly.remove(it)
    }

    /// `remove_back(it)`: retracts the back first when `it` is the range's end.
    pub fn remove_back(&mut self, poly: &mut MutablePolygon, it: Iter) -> Iter {
        if !self.is_empty() && self.end == it {
            self.retract_back(poly);
        }
        poly.remove(it)
    }

    /// `make_empty()`.
    fn make_empty(&mut self) {
        self.begin = Iter(-1);
        self.end = Iter(-1);
    }
}

/// `remove_duplicates(MutablePolygon&)`: removes exact duplicate points; may leave fewer than 3.
pub fn remove_duplicates(polygon: &mut MutablePolygon) {
    if !polygon.is_empty() {
        let begin = polygon.begin();
        let mut it = polygon.next(begin);
        while it != begin {
            let prev = polygon.prev(it);
            if polygon.point(prev) == polygon.point(it) {
                it = polygon.remove(it);
            } else {
                it = polygon.next(it);
            }
        }
    }
}

/// `remove_duplicates(MutablePolygon&, double eps)`: removes points closer than `eps` to the one
/// before; may leave fewer than 3.
pub fn remove_duplicates_eps(polygon: &mut MutablePolygon, eps: f64) {
    if !polygon.is_empty() {
        let eps2 = eps * eps;
        let begin = polygon.begin();
        let mut it = polygon.next(begin);
        while it != begin {
            let prev = polygon.prev(it);
            if (polygon.point(it) - polygon.point(prev))
                .cast_f64()
                .squared_norm()
                < eps2
            {
                it = polygon.remove(it);
            } else {
                it = polygon.next(it);
            }
        }
    }
}

/// `remove_duplicates(MutablePolygon&, coord_t scaled_eps, double max_angle)`: removes a point
/// closer than `scaled_eps` to the one before when the turn there is within `max_angle`; clears a
/// polygon left with fewer than 3 points.
pub fn remove_duplicates_angle(polygon: &mut MutablePolygon, scaled_eps: Coord, max_angle: f64) {
    if polygon.size() >= 3 {
        let cos_max_angle = orca_crt::cos(max_angle);
        let cos_max_angle_2 = cos_max_angle * cos_max_angle;
        let scaled_eps_sqr = scaled_eps.wrapping_mul(scaled_eps);
        let begin = polygon.begin();
        let mut it = polygon.next(begin);
        while it != begin {
            let prev = polygon.prev(it);
            let next = polygon.next(it);
            let v1 = polygon.point(it) - polygon.point(prev);
            let v1_sqr_norm = v1.squared_norm();
            if v1_sqr_norm < scaled_eps_sqr {
                let v2 = polygon.point(next) - polygon.point(prev);
                let d = v1.dot(v2) as f64;
                if d * d > cos_max_angle_2 * v1_sqr_norm as f64 * v2.squared_norm() as f64 {
                    it = polygon.remove(it);
                    continue;
                }
            }
            it = next;
        }
    }

    if polygon.size() < 3 {
        polygon.clear();
    }
}

/// `remove_duplicates(ExPolygons, coord_t scaled_eps, double max_angle)`: each contour and hole
/// through [`remove_duplicates_angle`] (reserving twice its points), the emptied holes and the
/// expolygons left without a contour dropped.
#[must_use]
pub fn remove_duplicates_expolygons(
    mut expolygons: ExPolygons,
    scaled_eps: Coord,
    max_angle: f64,
) -> ExPolygons {
    let mut mp = MutablePolygon::new();
    for expolygon in &mut expolygons {
        mp.assign_polygon(&expolygon.contour, expolygon.contour.points.len() * 2);
        remove_duplicates_angle(&mut mp, scaled_eps, max_angle);
        mp.polygon_into(&mut expolygon.contour);
        for hole in &mut expolygon.holes {
            mp.assign_polygon(hole, hole.points.len() * 2);
            remove_duplicates_angle(&mut mp, scaled_eps, max_angle);
            mp.polygon_into(hole);
        }
        expolygon.holes.retain(|p| !p.is_empty());
    }
    expolygons.retain(|p| !p.is_empty());
    expolygons
}

/// The state of each side of `clip_narrow_corner`'s walk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Free,
    Blocked,
    Far,
}

/// `int64_t(SCALED_EPSILON)`.
fn scaled_epsilon_i64() -> i64 {
    orca_crt::f64_to_i64(SCALED_EPSILON)
}

/// `clip_narrow_corner` (static, MutablePolygon.cpp 74-255): after the caller removed the concave
/// corner `p1` between `it0` and `it2`, where `|p2 - p0| < shortcut_length`, closes the crack by
/// walking back from `it0` and on from `it2` while the clipping edge stays shorter than
/// `shortcut_length` and inside the polygon, then clips the last trapezoid with an edge of
/// `shortcut_length`. True when the polygon was closed to nothing or left as one counter-clockwise
/// triangle; `it0` and `it2` end on the final clipping edge. The swapped targets of the far and
/// blocked case (`*it0` moves along `it2`'s edge and the other way round) are the C++'s.
fn clip_narrow_corner(
    p1: Point,
    it0: &mut Iter,
    it2: &mut Iter,
    polygon: &mut MutablePolygon,
    unprocessed_range: &mut Range,
    mut dist2_current: i64,
    shortcut_length: i64,
) -> bool {
    let shortcut_length2 = shortcut_length.wrapping_mul(shortcut_length);

    let mut forward = Status::Free;
    let mut backward = Status::Free;

    let mut p0 = polygon.point(*it0);
    let mut p2 = polygon.point(*it2);
    // Uninitialised in the C++; every read follows a write.
    let mut p02 = Point::default();
    let mut p22 = Point::default();
    let mut dist2_next: i64 = 0;

    // As long as there is at least a single triangle left in the polygon.
    while polygon.size() >= 3 {
        if forward == Status::Far && backward == Status::Far {
            p02 = polygon.point(polygon.prev(*it0));
            p22 = polygon.point(polygon.next(*it2));
            let d2 = (p22 - p02).squared_norm();
            if d2 <= shortcut_length2 {
                // The region was narrow until now and it is still narrow. Trim at both sides.
                let after = unprocessed_range.remove_back(polygon, *it0);
                *it0 = polygon.prev(after);
                *it2 = unprocessed_range.remove_front(polygon, *it2);
                if polygon.size() <= 2 {
                    // A hole degenerated to an empty polygon.
                    return true;
                }
                forward = Status::Free;
                backward = Status::Free;
                dist2_current = d2;
                p0 = p02;
                p2 = p22;
            } else {
                // The region is widening. Stop traversal and trim the final trapezoid.
                dist2_next = d2;
                break;
            }
        } else if forward != Status::Free && backward != Status::Free {
            // One of the corners is blocked, the other is blocked or too far. Stop traversal.
            break;
        }
        // Try to proceed by flipping a diagonal.
        // Progress by keeping the distance of the clipping edge end points equal to initial p1.
        if forward == Status::Free
            && (backward != Status::Free || (p2 - p1).squared_norm() < (p0 - p1).squared_norm())
        {
            p22 = polygon.point(polygon.next(*it2));
            if cross2_coord(p2 - p0, p22 - p0) > 0 {
                forward = Status::Blocked;
            } else {
                // New clipping edge length.
                let d2 = (p22 - p0).squared_norm();
                if d2 > shortcut_length2 {
                    forward = Status::Far;
                    dist2_next = d2;
                } else {
                    forward = Status::Free;
                    // Make one step in the forward direction.
                    *it2 = unprocessed_range.remove_front(polygon, *it2);
                    p2 = p22;
                    dist2_current = d2;
                }
            }
        } else {
            p02 = polygon.point(polygon.prev(*it0));
            if cross2_coord(p02 - p2, p0 - p2) > 0 {
                backward = Status::Blocked;
            } else {
                // New clipping edge length.
                let d2 = (p2 - p02).squared_norm();
                if d2 > shortcut_length2 {
                    backward = Status::Far;
                    dist2_next = d2;
                } else {
                    backward = Status::Free;
                    // Make one step in the backward direction.
                    let after = unprocessed_range.remove_back(polygon, *it0);
                    *it0 = polygon.prev(after);
                    p0 = p02;
                    dist2_current = d2;
                }
            }
        }
    }

    if polygon.size() <= 3 {
        // A hole degenerated to an empty polygon, or a tiny triangle remained.
        if polygon.size() < 3 || (forward == Status::Far && backward == Status::Far) {
            polygon.clear();
        }
        // Otherwise the remaining triangle is CCW oriented: keep it.
        return true;
    }

    let short = shortcut_length.wrapping_sub(scaled_epsilon_i64());
    let short2 = short.wrapping_mul(short);
    if (forward == Status::Blocked && backward == Status::Blocked) || dist2_current > short2 {
        // The crack is filled, keep the last clipping edge.
    } else if dist2_next < short2 {
        // To avoid creating tiny edges.
        if forward == Status::Far {
            let after = unprocessed_range.remove_back(polygon, *it0);
            *it0 = polygon.prev(after);
        }
        if backward == Status::Far {
            *it2 = unprocessed_range.remove_front(polygon, *it2);
        }
        if polygon.size() <= 2 {
            // A hole degenerated to an empty polygon.
            return true;
        }
    } else if forward == Status::Blocked || backward == Status::Blocked {
        // One side is far, the other blocked.
        if forward == Status::Far {
            // Sort, so we will clip the 1st edge.
            core::mem::swap(&mut p0, &mut p2);
            core::mem::swap(&mut p02, &mut p22);
        }
        // Find point on (p0, p02) at distance shortcut_length from p2.
        let v: Vec2d = (p02 - p0).cast_f64();
        let d: Vec2d = (p0 - p2).cast_f64();
        let a = v.squared_norm();
        let b = 2. * d.dot(&v);
        let mut u = b * b - 4. * a * (d.squared_norm() - shortcut_length2 as f64);
        u = orca_crt::sqrt(u);
        let t = (-b + u) / (2. * a);
        let target = if backward == Status::Far { *it2 } else { *it0 };
        *polygon.point_mut(target) += Point::cast_from_vec2d(v * t);
    } else {
        // The trapezoid (it0.prev(), it0, it2, it2.next()) is widening. Trim it.
        let dcurrent = orca_crt::sqrt(dist2_current as f64);
        let t =
            (shortcut_length as f64 - dcurrent) / (orca_crt::sqrt(dist2_next as f64) - dcurrent);
        *polygon.point_mut(*it0) += Point::cast_from_vec2d((p02 - p0).cast_f64() * t);
        *polygon.point_mut(*it2) += Point::cast_from_vec2d((p22 - p2).cast_f64() * t);
    }
    false
}

/// `smooth_outward(MutablePolygon&, coord_t clip_dist_scaled)`, adapted by its authors from Cura's
/// `smooth_outward`: after [`remove_duplicates_eps`] by scaled 0.01 mm, visits each point once
/// and, at a concave corner sharper than 135 degrees, removes it and either closes the crack with
/// `clip_narrow_corner` (the corner's neighbours closer than `clip_dist_scaled`) or cuts the
/// corner by an edge of `clip_dist_scaled`; a polygon left as a low clockwise triangle, or with
/// fewer than 3 points, is cleared.
pub fn smooth_outward(polygon: &mut MutablePolygon, clip_dist_scaled: Coord) {
    remove_duplicates_eps(polygon, scaled_f64(0.01));

    let clip_dist_scaled2 = clip_dist_scaled.wrapping_mul(clip_dist_scaled);
    let grown = clip_dist_scaled.wrapping_add(scaled_epsilon_i64());
    let clip_dist_scaled2eps = grown.wrapping_mul(grown);
    let foot_dist_min2 = SCALED_EPSILON * SCALED_EPSILON;

    // Each source point will be visited exactly once.
    let mut unprocessed_range = Range::new(polygon);
    while !unprocessed_range.is_empty() && polygon.size() > 2 {
        let it1 = unprocessed_range.process_next(polygon);
        let mut it0 = polygon.prev(it1);
        let mut it2 = polygon.next(it1);
        let p0 = polygon.point(it0);
        let p1 = polygon.point(it1);
        let p2 = polygon.point(it2);
        let v1 = p0 - p1;
        let v2 = p2 - p1;
        if cross2_coord(v1, v2) > 0 {
            // Concave corner.
            let dot = v1.dot(v2);
            let mut l2v1 = v1.squared_norm() as f64;
            let mut l2v2 = v2.squared_norm() as f64;
            if dot > 0 || (dot as f64) * (dot as f64) * 2. < l2v1 * l2v2 {
                // Angle between v1 and v2 bigger than 135 degrees.
                // Simplify the sharp angle.
                let v02 = p2 - p0;
                let l2v02 = v02.squared_norm();
                polygon.remove(it1);
                if l2v02 < clip_dist_scaled2 {
                    // (p0, p2) is short.
                    // Clip a sharp concave corner by possibly expanding the trimming region left of
                    // it0 and right of it2.
                    if clip_narrow_corner(
                        p1,
                        &mut it0,
                        &mut it2,
                        polygon,
                        &mut unprocessed_range,
                        l2v02,
                        clip_dist_scaled,
                    ) {
                        // Trimmed down to an empty polygon or to a single CCW triangle.
                        return;
                    }
                } else if l2v02 > clip_dist_scaled2eps {
                    // Clip an obtuse corner.
                    let mut v1d = v1.cast_f64();
                    let mut v2d = v2.cast_f64();
                    // Sort v1d, v2d, shorter first.
                    let swap = l2v1 > l2v2;
                    if swap {
                        core::mem::swap(&mut v1d, &mut v2d);
                        core::mem::swap(&mut l2v1, &mut l2v2);
                    }
                    let lv1 = orca_crt::sqrt(l2v1);
                    let lv2 = orca_crt::sqrt(l2v2);
                    // Bisector between v1 and v2.
                    let bisector = v1d / lv1 + v2d / lv2;
                    let l2bisector = bisector.squared_norm();
                    // Squared distance of the end point of v1 to the bisector.
                    let foot = v1d.dot(&bisector);
                    let d2 = l2v1 - foot * foot / l2bisector;
                    if d2 < foot_dist_min2 {
                        // Height of the p1, p0, p2 triangle is tiny. Just remove p1.
                    } else if d2 < 0.25 * clip_dist_scaled2 as f64 + SCALED_EPSILON {
                        // The shorter vector is too close to the bisector. Trim the shorter vector
                        // fully, trim the longer vector partially: the intersection of a circle at
                        // p2 of radius clip_dist_scaled with the ray (p1, p0), after the foot point.
                        let b = -2. * v1d.dot(&v2d);
                        let u = b * b - 4. * l2v2 * (l2v1 - clip_dist_scaled2 as f64);
                        // Take the second intersection along v2.
                        let t = (-b + orca_crt::sqrt(u)) / (2. * l2v2);
                        let pt_new = p1 + Point::cast_from_vec2d(t * v2d);
                        polygon.insert(it2, pt_new);
                    } else {
                        // Cut the corner with a line perpendicular to the bisector.
                        let t = orca_crt::sqrt(0.25 * clip_dist_scaled2 as f64 / d2);
                        let t2 = t * lv1 / lv2;
                        let mut q0 = p1 + Point::cast_from_vec2d(v1d * t);
                        let mut q2 = p1 + Point::cast_from_vec2d(v2d * t2);
                        if swap {
                            core::mem::swap(&mut q0, &mut q2);
                        }
                        let at_q2 = polygon.insert(it2, q2);
                        polygon.insert(at_q2, q0);
                    }
                } else {
                    // Just remove p1.
                }
                // The C++ sets it1 = it2 here and ++it1 in the other branches; the loop takes its
                // next point from the range, so neither is read.
            }
        }
    }

    if polygon.size() == 3 {
        // Check whether the last triangle is clockwise oriented (it is a hole) and its height is
        // below clip_dist_scaled. If so, fill in the hole.
        let begin = polygon.begin();
        let p0 = polygon.point(polygon.prev(begin));
        let p1 = polygon.point(begin);
        let p2 = polygon.point(polygon.next(begin));
        let mut v1 = p0 - p1;
        let mut v2 = p2 - p1;
        if cross2_coord(v1, v2) > 0 {
            // CW triangle. Measure its height.
            let v3 = p2 - p0;
            let mut l12 = v1.squared_norm();
            let mut l22 = v2.squared_norm();
            let l32 = v3.squared_norm();
            if l22 > l12 && l22 > l32 {
                core::mem::swap(&mut v1, &mut v2);
                core::mem::swap(&mut l12, &mut l22);
            } else if l32 > l12 && l32 > l22 {
                v1 = v3;
                l12 = l32;
            }
            let dot = v1.dot(v2) as f64;
            let h2 = l22 as f64 - dot * dot / l12 as f64;
            if h2 < clip_dist_scaled2 as f64 {
                // CW triangle with a low height. Close the hole.
                polygon.clear();
            }
        }
    } else if polygon.size() < 3 {
        polygon.clear();
    }
}

/// `smooth_outward(Polygon, coord_t)`: through a [`MutablePolygon`] reserving twice its points.
#[must_use]
pub fn smooth_outward_polygon(mut polygon: Polygon, clip_dist_scaled: Coord) -> Polygon {
    let mut mp = MutablePolygon::from_polygon(&polygon, polygon.points.len() * 2);
    smooth_outward(&mut mp, clip_dist_scaled);
    mp.polygon_into(&mut polygon);
    polygon
}

/// `smooth_outward(Polygons, coord_t)`: each polygon through one [`MutablePolygon`], the emptied
/// ones dropped.
#[must_use]
pub fn smooth_outward_polygons(mut polygons: Polygons, clip_dist_scaled: Coord) -> Polygons {
    let mut mp = MutablePolygon::new();
    for polygon in &mut polygons {
        mp.assign_polygon(polygon, polygon.points.len() * 2);
        smooth_outward(&mut mp, clip_dist_scaled);
        mp.polygon_into(polygon);
    }
    polygons.retain(|p| !p.is_empty());
    polygons
}

/// `smooth_outward(ExPolygons, coord_t)`: each contour and hole through one [`MutablePolygon`],
/// the emptied holes and the expolygons left without a contour dropped.
#[must_use]
pub fn smooth_outward_expolygons(
    mut expolygons: ExPolygons,
    clip_dist_scaled: Coord,
) -> ExPolygons {
    let mut mp = MutablePolygon::new();
    for expolygon in &mut expolygons {
        mp.assign_polygon(&expolygon.contour, expolygon.contour.points.len() * 2);
        smooth_outward(&mut mp, clip_dist_scaled);
        mp.polygon_into(&mut expolygon.contour);
        for hole in &mut expolygon.holes {
            mp.assign_polygon(hole, hole.points.len() * 2);
            smooth_outward(&mut mp, clip_dist_scaled);
            mp.polygon_into(hole);
        }
        expolygon.holes.retain(|p| !p.is_empty());
    }
    expolygons.retain(|p| !p.is_empty());
    expolygons
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pts(v: &[(i64, i64)]) -> Vec<Point> {
        v.iter().map(|&(x, y)| Point::new(x, y)).collect()
    }

    /// A removed slot goes on the free list and the next insertion takes it back, linked before
    /// the iterator it is given.
    #[test]
    fn insert_reuses_the_freed_slot() {
        let mut mp = MutablePolygon::from_points(&pts(&[(0, 0), (10, 0), (10, 10), (0, 10)]), 0);
        let b = mp.begin();
        let second = mp.next(b);
        let after = mp.remove(second);
        assert_eq!(after.idx(), 2);
        assert_eq!(mp.size(), 3);
        let n = mp.insert(after, Point::new(5, -5));
        assert_eq!(n.idx(), 1);
        assert_eq!(
            mp.polygon().points,
            pts(&[(0, 0), (5, -5), (10, 10), (0, 10)])
        );
        assert_eq!(mp.end().idx(), 3);
    }

    /// Removing `begin()` moves it on; removing the last point leaves no valid iterator.
    #[test]
    fn removing_every_point_empties_the_loop() {
        let mut mp = MutablePolygon::from_points(&pts(&[(1, 1), (2, 2)]), 4);
        let it = mp.remove(mp.begin());
        assert_eq!(mp.begin(), it);
        assert!(!mp.valid());
        assert!(mp.polygon().points.is_empty());
        let it = mp.remove(it);
        assert!(!it.valid());
        assert!(mp.is_empty());
        assert!(!mp.end().valid());
    }

    /// `==` compares from each begin, so a turned loop differs.
    #[test]
    fn equality_starts_at_begin() {
        let a = MutablePolygon::from_points(&pts(&[(0, 0), (1, 0), (1, 1)]), 0);
        let b = MutablePolygon::from_points(&pts(&[(1, 0), (1, 1), (0, 0)]), 0);
        assert!(a != b);
        assert!(a == a.clone());
        assert!(MutablePolygon::new() == MutablePolygon::from_points(&[], 3));
    }

    /// A crack narrower than the clip distance is closed.
    #[test]
    fn a_narrow_crack_is_closed() {
        let p = Polygon::new(pts(&[
            (0, 0),
            (10_000_000, 0),
            (10_000_000, 10_000_000),
            (5_025_000, 10_000_000),
            (5_000_000, 2_000_000),
            (4_975_000, 10_000_000),
            (0, 10_000_000),
        ]));
        let out = smooth_outward_polygon(p, 100_000);
        assert!(out.points.iter().all(|q| q.y == 0 || q.y >= 9_000_000));
    }
}
