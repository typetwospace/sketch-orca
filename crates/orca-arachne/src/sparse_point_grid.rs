// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Arachne/utils/SparsePointGrid.hpp lines 1-63 whole, of
// OrcaSlicer v2.4.2, translated on 2026-11-14 by arachne-2. Copyright Scott Lenser (2016) and
// Ultimaker B.V. (2020, CuraEngine), with the PrusaSlicer authors' (Prusa Research) and the
// OrcaSlicer authors' changes, under the GNU Affero General Public License, version 3 (AGPL-3.0;
// LICENSE).

//! `SparsePointGrid<ElemT, Locator>`: a [`SparseGrid`] of elements that each lie at one point.
//!
//! `insert` puts the element into the cell of the point its locator gives (`toGridPoint`), after
//! the elements already at that cell ([`crate::unordered_multimap`]). The C++'s `Locator` functor
//! is a [`PointLocator`]: orca-voronoi's `PathsPointIndexLocator` (the vertex of a path index, as
//! `PolylineStitcher` uses it) and any `Fn(&E) -> Point`. The grid dereferences to its
//! [`SparseGrid`], which the C++ derives it from.

use core::ops::Deref;

use orca_geom::point::{Coord, Point};
use orca_voronoi::{PathPoints, PathsPointIndex, PathsPointIndexLocator};

use crate::sparse_grid::SparseGrid;

/// A `Locator` of `SparsePointGrid`: `Point operator()(const ElemT &elem) const`.
pub trait PointLocator<E> {
    /// The point of `elem`.
    fn locate(&self, elem: &E) -> Point;
}

impl<E, F: Fn(&E) -> Point> PointLocator<E> for F {
    fn locate(&self, elem: &E) -> Point {
        self(elem)
    }
}

/// `PathsPointIndexLocator<Paths>`: the index's vertex.
impl<T: PathPoints> PointLocator<PathsPointIndex<'_, T>> for PathsPointIndexLocator {
    fn locate(&self, elem: &PathsPointIndex<'_, T>) -> Point {
        self.call(elem)
    }
}

/// `SparsePointGrid<ElemT, Locator>`.
#[derive(Debug)]
pub struct SparsePointGrid<E, L> {
    /// The base class `SparseGrid<ElemT>`.
    grid: SparseGrid<E>,
    /// `m_locator`.
    m_locator: L,
}

impl<E, L> Deref for SparsePointGrid<E, L> {
    type Target = SparseGrid<E>;
    /// The base class `SparseGrid<ElemT>`.
    fn deref(&self) -> &SparseGrid<E> {
        &self.grid
    }
}

impl<E, L: PointLocator<E>> SparsePointGrid<E, L> {
    /// `SparsePointGrid(cell_size, elem_reserve = 0, max_load_factor = 1.0f)`, with the locator
    /// the C++ default-constructs.
    #[must_use]
    pub fn new(cell_size: Coord, elem_reserve: usize, max_load_factor: f32, locator: L) -> Self {
        Self {
            grid: SparseGrid::new(cell_size, elem_reserve, max_load_factor),
            m_locator: locator,
        }
    }

    /// `insert(elem)`: the element into the cell of its point.
    pub fn insert(&mut self, elem: E) {
        let loc = self.m_locator.locate(&elem);
        let grid_loc = self.grid.square.to_grid_point(loc);
        self.grid.m_grid.emplace(grid_loc, elem);
    }
}
