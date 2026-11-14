// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Arachne/utils/SparseLineGrid.hpp lines 1-76 whole, of
// OrcaSlicer v2.4.2, translated on 2026-11-14 by arachne-2. Copyright Ultimaker B.V. (2018,
// CuraEngine), with the PrusaSlicer authors' (Prusa Research) and the OrcaSlicer authors' changes,
// under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! `SparseLineGrid<ElemT, Locator>`: a [`SparseGrid`] of elements that each lie along a line
//! segment.
//!
//! `insert` puts a copy of the element into every cell `processLineCells` hands over for the
//! segment its locator gives, in that order (the C++ binds a lambda to the map through
//! `std::bind`; here the walk's closure writes the map). The C++'s `Locator` functor is a
//! [`LineLocator`]: orca-voronoi's `PolygonsPointIndexSegmentLocator` (the segment from a polygon
//! vertex to the next, as `WallToolPaths.cpp`'s `createLocToLineGrid` uses it, which is the piece
//! arachne-4's) and any `Fn(&E) -> (Point, Point)`. The grid dereferences to its [`SparseGrid`].

use core::ops::Deref;

use orca_geom::point::{Coord, Point};
use orca_voronoi::{PolygonsPointIndex, PolygonsPointIndexSegmentLocator};

use crate::sparse_grid::SparseGrid;

/// A `Locator` of `SparseLineGrid`: `std::pair<Point, Point> operator()(const ElemT &elem)
/// const`.
pub trait LineLocator<E> {
    /// The segment of `elem`, from its start to its end.
    fn locate(&self, elem: &E) -> (Point, Point);
}

impl<E, F: Fn(&E) -> (Point, Point)> LineLocator<E> for F {
    fn locate(&self, elem: &E) -> (Point, Point) {
        self(elem)
    }
}

/// `PolygonsPointIndexSegmentLocator`: the segment from the index's vertex to the next.
impl<'a> LineLocator<PolygonsPointIndex<'a>> for PolygonsPointIndexSegmentLocator {
    fn locate(&self, elem: &PolygonsPointIndex<'a>) -> (Point, Point) {
        self.call(elem)
    }
}

/// `SparseLineGrid<ElemT, Locator>`.
#[derive(Debug)]
pub struct SparseLineGrid<E, L> {
    /// The base class `SparseGrid<ElemT>`.
    grid: SparseGrid<E>,
    /// `m_locator`.
    m_locator: L,
}

impl<E, L> Deref for SparseLineGrid<E, L> {
    type Target = SparseGrid<E>;
    /// The base class `SparseGrid<ElemT>`.
    fn deref(&self) -> &SparseGrid<E> {
        &self.grid
    }
}

impl<E: Clone, L: LineLocator<E>> SparseLineGrid<E, L> {
    /// `SparseLineGrid(cell_size, elem_reserve = 0, max_load_factor = 1.0f)`, with the locator
    /// the C++ default-constructs.
    #[must_use]
    pub fn new(cell_size: Coord, elem_reserve: usize, max_load_factor: f32, locator: L) -> Self {
        Self {
            grid: SparseGrid::new(cell_size, elem_reserve, max_load_factor),
            m_locator: locator,
        }
    }

    /// `insert(elem)`: the element into every cell along its segment.
    pub fn insert(&mut self, elem: &E) {
        let line = self.m_locator.locate(elem);
        let square = self.grid.square;
        let m_grid = &mut self.grid.m_grid;
        square.process_line_cells(line, |grid_loc| {
            m_grid.emplace(grid_loc, elem.clone());
            true
        });
    }
}
