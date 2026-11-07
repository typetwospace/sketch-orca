// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Boost code: Boost.Geometry of Boost 1.84.0, translated on 2026-11-07 from
// boost/geometry/index/rtree.hpp (insert, clear, empty, size, bounds, query with an intersects
// predicate), index/detail/rtree/visitors/children_box.hpp, algorithms/detail/disjoint/box_box.hpp
// and algorithms/assign.hpp (assign_inverse). Copyright (c) 2008 Federico J. Fernandez, Copyright
// (c) 2011-2023 Adam Wulkiewicz, Copyright (c) 2020 Caian Benedicto, Copyright (c) 2007-2015
// Barend Gehrels and modifications copyright (c) 2014-2023 Oracle and/or its affiliates, under the
// Boost Software License, version 1.0 (licenses/BSL-1.0.txt).

//! `Arrange.cpp`'s spatial index, `bgi::rtree<SpatElement, bgi::rstar<16, 4>>`, for what it asks
//! of it: insert, clear, empty, size, the bounds and the boxes that intersect a box.
//!
//! The index is kept as a list in insertion order. Its answers are those of the R*-tree:
//!
//! - `bounds()` is the envelope of the root's children, and the R*-tree's node boxes are the
//!   envelopes of what they hold (elements are only added and moved by forced reinsertion, never
//!   removed), so it is the exact envelope of every box inserted; an empty tree gives the inverse
//!   box (`assign_inverse`: the highest corner low, the lowest high).
//! - A query with `bgi::intersects(box)` returns the elements whose box is not disjoint from it,
//!   the boundary included (Boost's box-box `disjoint`: one box's maximum below the other's
//!   minimum on an axis). A node whose box misses the query box holds no element that meets it,
//!   so the set is the same; only its order is the tree's, which `Arrange.cpp` never reads (it
//!   takes the smallest alignment score with a strict `<` and counts the results).

use crate::geometry::Box;
use orca_geom::point::Point;

/// `SpatElement`: an item's bounding box and its index among the packed items.
pub type SpatElement = (Box, u32);

/// `SpatIndex`.
#[derive(Clone, Debug, Default)]
pub struct SpatIndex {
    elements: Vec<SpatElement>,
}

/// `geometry::intersects(box1, box2)`: not disjoint, the boundary included.
#[must_use]
pub fn boxes_intersect(a: &Box, b: &Box) -> bool {
    !(a.max.x < b.min.x || a.min.x > b.max.x || a.max.y < b.min.y || a.min.y > b.max.y)
}

impl SpatIndex {
    /// An empty index.
    #[must_use]
    pub fn new() -> SpatIndex {
        SpatIndex::default()
    }

    /// `insert(value)`.
    pub fn insert(&mut self, value: SpatElement) {
        self.elements.push(value);
    }

    /// `clear()`.
    pub fn clear(&mut self) {
        self.elements.clear();
    }

    /// `empty()`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// `size()`.
    #[must_use]
    pub fn len(&self) -> usize {
        self.elements.len()
    }

    /// `bounds()`, converted to a libnest2d box (not defined): the envelope of every box, or the
    /// inverse box of an empty index.
    #[must_use]
    pub fn bounds(&self) -> Box {
        let mut min = Point::new(i64::MAX, i64::MAX);
        let mut max = Point::new(i64::MIN, i64::MIN);
        for (b, _) in &self.elements {
            min.x = orca_stl::min(min.x, b.min.x);
            min.y = orca_stl::min(min.y, b.min.y);
            max.x = orca_stl::max(max.x, b.max.x);
            max.y = orca_stl::max(max.y, b.max.y);
        }
        Box {
            min,
            max,
            defined: false,
        }
    }

    /// `query(bgi::intersects(b), out)`: the elements whose box meets `b`, in insertion order.
    #[must_use]
    pub fn query_intersects(&self, b: &Box) -> Vec<SpatElement> {
        self.elements
            .iter()
            .filter(|(e, _)| boxes_intersect(e, b))
            .copied()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touching_boxes_intersect_and_bounds_cover_all() {
        let mut t = SpatIndex::new();
        assert!(t.is_empty());
        assert_eq!(t.bounds().min, Point::new(i64::MAX, i64::MAX));
        t.insert((Box::new(Point::new(0, 0), Point::new(10, 10)), 0));
        t.insert((Box::new(Point::new(20, 0), Point::new(30, 5)), 1));
        let q = t.query_intersects(&Box::new(Point::new(10, 10), Point::new(15, 15)));
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].1, 0);
        let b = t.bounds();
        assert_eq!(
            (b.min, b.max, b.defined),
            (Point::new(0, 0), Point::new(30, 10), false)
        );
        assert_eq!(t.len(), 2);
    }
}
