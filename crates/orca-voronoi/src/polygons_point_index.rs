// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Arachne/utils/PolygonsPointIndex.hpp lines 1-178 and
// PolygonsSegmentIndex.hpp lines 1-50 of OrcaSlicer v2.4.2 (PathsPointIndex, PolygonsPointIndex,
// the two locators, std::hash<PolygonsPointIndex>, PolygonsSegmentIndex and its segment_traits),
// with Slic3r::PointHash of src/libslic3r/Point.hpp lines 496-500, translated on 2026-11-14 by
// orca-o3-voronoi-3. Copyright Ultimaker B.V. (2018 and 2020, CuraEngine), with the PrusaSlicer
// authors' (Prusa Research) and the OrcaSlicer authors' changes, under the GNU Affero General
// Public License, version 3 (AGPL-3.0; LICENSE).

//! Arachne's indexes into a set of paths (`Arachne/utils/PolygonsPointIndex.hpp` and
//! `PolygonsSegmentIndex.hpp`): a vertex of one path, named by the path's and the vertex's
//! indexes, and the segment from that vertex to the next.
//!
//! - [`PathsPointIndex`] holds a reference to the paths (the C++'s `const Paths*`, `None` for the
//!   empty index), the path's index and the vertex's, each `unsigned int` ([`u32`]). `p()` is the
//!   vertex (through `make_point`, [`PathPoints::point`]), (0, 0) for the empty index;
//!   `next()` and `prev()` step around the path, wrapping at its ends, in `unsigned int` as the
//!   C++ does. `==` compares the paths' address and the two indexes; `<` compares the vertices
//!   (`Point`'s `operator<`: x, then y).
//! - [`PathPoints`] is what an index reads of one path: its size and its vertex as a point
//!   (`make_point`). `Polygon` implements it here; Arachne's `ExtrusionLine` implements it where
//!   it is translated (its `make_point` is the junction's point), so `PathsPointIndex<'_,
//!   ExtrusionLine>` is the C++'s `PathsPointIndex<VariableWidthLines>`.
//! - [`PolygonsPointIndex`] is the index into `Polygons`; [`PolygonsPointIndexSegmentLocator`]
//!   gives the segment from its vertex to the next and [`PathsPointIndexLocator`] its vertex;
//!   [`polygons_point_index_hash`] is `std::hash<PolygonsPointIndex>`, Slic3r's `PointHash` of the
//!   vertex.
//! - [`PolygonsSegmentIndex`] is a [`PolygonsPointIndex`] (the C++ derives from it, so it
//!   dereferences to one here) whose `from()` is the vertex and `to()` the next vertex; its
//!   segment traits ([`SegmentConcept`]) give `from` as `LOW` and `to` as `HIGH`, so it is the
//!   segment type the Voronoi diagram of Arachne's polygons is built from.
//!
//! A vertex index past its path, or a path index past the paths, is read unchecked in the C++
//! (undefined) and panics here; `next()` of an empty path divides by zero in both.
//!
//! `point_hash` (`Slic3r::PointHash`, Point.hpp) belongs in `orca-geom`, which has no `PointHash`
//! yet; it is translated here for the index's hash.
//!
//! Left out: the copy constructor and assignment, which are the C++'s defaults (`Copy`).

use core::ops::Deref;

use orca_geom::point::Point;
use orca_geom::polygon::{Polygon, Polygons};

use crate::voronoi::SegmentConcept;

/// `make_point(p)` over one path: what [`PathsPointIndex`] reads of the path it points into,
/// `(*polygons)[poly_idx].size()` and `make_point((*polygons)[poly_idx][point_idx])`.
pub trait PathPoints {
    /// The path's `size()`.
    fn size(&self) -> usize;
    /// `make_point(path[point_idx])`: the vertex as a point. Panics past the path's end.
    fn point(&self, point_idx: usize) -> Point;
}

/// A `Polygon`'s points; `make_point` of a `Point` is the point itself.
impl PathPoints for Polygon {
    fn size(&self) -> usize {
        self.points.len()
    }
    fn point(&self, point_idx: usize) -> Point {
        self.points[point_idx]
    }
}

/// `PathsPointIndex<Paths>`: a vertex of one of the paths of `Paths` (a `Vec<T>` of paths).
pub struct PathsPointIndex<'a, T> {
    /// `polygons`: the paths this index points into, `None` for the empty index.
    pub polygons: Option<&'a Vec<T>>,
    /// `poly_idx`: the index of the path in `polygons`.
    pub poly_idx: u32,
    /// `point_idx`: the index of the vertex in the path.
    pub point_idx: u32,
}

/// `PolygonsPointIndex`: `PathsPointIndex<Polygons>`.
pub type PolygonsPointIndex<'a> = PathsPointIndex<'a, Polygon>;

impl<T> Clone for PathsPointIndex<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for PathsPointIndex<'_, T> {}

impl<T> core::fmt::Debug for PathsPointIndex<'_, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PathsPointIndex")
            .field("polygons", &self.polygons.map(|p| p as *const Vec<T>))
            .field("poly_idx", &self.poly_idx)
            .field("point_idx", &self.point_idx)
            .finish()
    }
}

impl<T> Default for PathsPointIndex<'_, T> {
    /// `PathsPointIndex()`: the empty index, to no paths, both indexes 0.
    fn default() -> Self {
        Self {
            polygons: None,
            poly_idx: 0,
            point_idx: 0,
        }
    }
}

impl<'a, T: PathPoints> PathsPointIndex<'a, T> {
    /// `PathsPointIndex(polygons, poly_idx, point_idx)`. The paths are a `Vec`, as the C++'s
    /// `const Paths*` points to one: `==` compares its address.
    #[must_use]
    #[allow(clippy::ptr_arg)]
    pub fn new(polygons: &'a Vec<T>, poly_idx: u32, point_idx: u32) -> Self {
        Self {
            polygons: Some(polygons),
            poly_idx,
            point_idx,
        }
    }

    /// `p()`: the vertex, or (0, 0) for the empty index.
    #[must_use]
    pub fn p(&self) -> Point {
        match self.polygons {
            None => Point::new(0, 0),
            Some(polygons) => polygons[self.poly_idx as usize].point(self.point_idx as usize),
        }
    }

    /// `initialized()`: the index points into paths.
    #[must_use]
    pub fn initialized(&self) -> bool {
        self.polygons.is_some()
    }

    /// `getPolygon()`: the path. Panics for the empty index (the C++ follows the null pointer).
    #[must_use]
    pub fn get_polygon(&self) -> &'a T {
        &self.paths()[self.poly_idx as usize]
    }

    /// The paths, which `operator++`, `operator--` and `getPolygon` read through the pointer.
    fn paths(&self) -> &'a Vec<T> {
        self.polygons
            .expect("PathsPointIndex: the C++ follows a null polygons pointer")
    }

    /// `operator<`: `this->p() < other.p()`, `Point`'s order (x, then y).
    #[must_use]
    pub fn lt(&self, other: &Self) -> bool {
        let (l, r) = (self.p(), other.p());
        l.x < r.x || (l.x == r.x && l.y < r.y)
    }

    /// `operator++`: the next vertex, wrapping at the end of the path.
    pub fn increment(&mut self) -> &mut Self {
        let size = self.paths()[self.poly_idx as usize].size();
        // `(point_idx + 1) % size()`: the sum in `unsigned int`, the remainder in `size_t`, the
        // result narrowed back to `unsigned int`.
        self.point_idx = (self.point_idx.wrapping_add(1) as usize % size) as u32;
        self
    }

    /// `operator--`: the previous vertex, wrapping at the start of the path.
    pub fn decrement(&mut self) -> &mut Self {
        if self.point_idx == 0 {
            self.point_idx = self.paths()[self.poly_idx as usize].size() as u32;
        }
        self.point_idx = self.point_idx.wrapping_sub(1);
        self
    }

    /// `next()`: a copy moved forward.
    #[must_use]
    pub fn next(&self) -> Self {
        let mut ret = *self;
        ret.increment();
        ret
    }

    /// `prev()`: a copy moved backward.
    #[must_use]
    pub fn prev(&self) -> Self {
        let mut ret = *self;
        ret.decrement();
        ret
    }
}

impl<T> PartialEq for PathsPointIndex<'_, T> {
    /// `operator==`: the same paths (by address) and the same two indexes; `operator!=` is its
    /// negation.
    fn eq(&self, other: &Self) -> bool {
        let same_paths = match (self.polygons, other.polygons) {
            (None, None) => true,
            (Some(a), Some(b)) => core::ptr::eq(a, b),
            _ => false,
        };
        same_paths && self.poly_idx == other.poly_idx && self.point_idx == other.point_idx
    }
}

/// `PolygonsPointIndexSegmentLocator::operator()(val)`: the segment from the index's vertex to the
/// next vertex of its polygon.
#[derive(Clone, Copy, Default, Debug)]
pub struct PolygonsPointIndexSegmentLocator;

impl PolygonsPointIndexSegmentLocator {
    /// `operator()(val)`: `(poly[point_idx], poly[(point_idx + 1) % poly.size()])`. Panics for
    /// the empty index (the C++ follows the null pointer).
    #[must_use]
    pub fn call(&self, val: &PolygonsPointIndex<'_>) -> (Point, Point) {
        let poly = &val.paths()[val.poly_idx as usize];
        let start = poly.points[val.point_idx as usize];
        let next_point_idx = (val.point_idx.wrapping_add(1) as usize % poly.points.len()) as u32;
        let end = poly.points[next_point_idx as usize];
        (start, end)
    }
}

/// `PathsPointIndexLocator<Paths>::operator()(val)`: `make_point(val.p())`, the index's vertex.
#[derive(Clone, Copy, Default, Debug)]
pub struct PathsPointIndexLocator;

impl PathsPointIndexLocator {
    /// `operator()(val)`.
    #[must_use]
    pub fn call<T: PathPoints>(&self, val: &PathsPointIndex<'_, T>) -> Point {
        val.p()
    }
}

/// `Slic3r::PointHash::operator()(pt)` (Point.hpp 496-500): `coord_t((89 * 31 + int64_t(x)) * 31
/// + y)` as `size_t`, the arithmetic wrapping as MSVC's does. It belongs in `orca-geom`.
#[must_use]
pub fn point_hash(pt: Point) -> u64 {
    (89_i64 * 31)
        .wrapping_add(pt.x)
        .wrapping_mul(31)
        .wrapping_add(pt.y) as u64
}

/// `std::hash<Slic3r::Arachne::PolygonsPointIndex>::operator()(lpi)`: `PointHash{}(lpi.p())`.
#[must_use]
pub fn polygons_point_index_hash(lpi: &PolygonsPointIndex<'_>) -> u64 {
    point_hash(lpi.p())
}

/// `PolygonsSegmentIndex`: a [`PolygonsPointIndex`] read as the segment from its vertex to the
/// next.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PolygonsSegmentIndex<'a>(pub PolygonsPointIndex<'a>);

impl<'a> PolygonsSegmentIndex<'a> {
    /// `PolygonsSegmentIndex(polygons, poly_idx, point_idx)`.
    #[must_use]
    pub fn new(polygons: &'a Polygons, poly_idx: u32, point_idx: u32) -> Self {
        Self(PolygonsPointIndex::new(polygons, poly_idx, point_idx))
    }

    /// `from()`: `PolygonsPointIndex::p()`.
    #[must_use]
    pub fn from(&self) -> Point {
        self.0.p()
    }

    /// `to()`: `next().p()`.
    #[must_use]
    pub fn to(&self) -> Point {
        self.0.next().p()
    }
}

impl<'a> Deref for PolygonsSegmentIndex<'a> {
    type Target = PolygonsPointIndex<'a>;
    /// The base class `PolygonsPointIndex`.
    fn deref(&self) -> &PolygonsPointIndex<'a> {
        &self.0
    }
}

/// `segment_traits<Slic3r::Arachne::PolygonsSegmentIndex>`: `LOW` is `from()`, `HIGH` is `to()`.
impl SegmentConcept for PolygonsSegmentIndex<'_> {
    type Point = Point;
    fn low(&self) -> Point {
        self.from()
    }
    fn high(&self) -> Point {
        self.to()
    }
}

#[cfg(test)]
mod tests {
    use orca_geom::point::Point;
    use orca_geom::polygon::Polygon;

    use super::{
        PolygonsPointIndex, PolygonsPointIndexSegmentLocator, PolygonsSegmentIndex, point_hash,
    };

    #[test]
    fn indexes_wrap_around_their_polygon() {
        let polys = vec![
            Polygon::new(vec![Point::new(0, 0), Point::new(10, 0), Point::new(0, 10)]),
            Polygon::new(vec![Point::new(5, 5), Point::new(6, 5)]),
        ];
        let i = PolygonsPointIndex::new(&polys, 0, 2);
        assert_eq!(i.next().point_idx, 0);
        assert_eq!(i.prev().point_idx, 1);
        assert_eq!(PolygonsPointIndex::new(&polys, 1, 0).prev().point_idx, 1);
        assert_eq!(i.p(), Point::new(0, 10));
        let s = PolygonsSegmentIndex::new(&polys, 0, 2);
        assert_eq!((s.from(), s.to()), (Point::new(0, 10), Point::new(0, 0)));
        assert_eq!(
            PolygonsPointIndexSegmentLocator.call(&i),
            (Point::new(0, 10), Point::new(0, 0))
        );
        assert!(*s == i);
        let other = polys.clone();
        assert!(PolygonsPointIndex::new(&other, 0, 2) != i);
        assert!(!PolygonsPointIndex::new(&polys, 0, 1).lt(&i));
        assert!(i.lt(&PolygonsPointIndex::new(&polys, 0, 1)));
        let empty = PolygonsPointIndex::default();
        assert!(!empty.initialized());
        assert_eq!(empty.p(), Point::new(0, 0));
        assert_eq!(point_hash(Point::new(1, 2)), ((2759 + 1) * 31 + 2) as u64);
    }
}
