// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Arachne/utils/SparseGrid.hpp lines 1-132 whole, of
// OrcaSlicer v2.4.2, translated on 2026-11-14 by arachne-2. Copyright Scott Lenser (2016) and
// Ultimaker B.V. (2018, CuraEngine), with the PrusaSlicer authors' (Prusa Research) and the
// OrcaSlicer authors' changes, under the GNU Affero General Public License, version 3 (AGPL-3.0;
// LICENSE).

//! `SparseGrid<ElemT>`: elements kept by the cell of a [`SquareGrid`] they lie in, so that the
//! elements near a point are found by visiting the cells around it.
//!
//! The C++ derives it from `SquareGrid`; here it holds one and dereferences to it, so the grid's
//! coordinates and walks are its own. Its map is `std::unordered_multimap<GridPoint, Elem,
//! PointHash>`: [`UnorderedMultimap`] in MSVC's order, hashed with Slic3r's `PointHash`
//! (`orca_voronoi::point_hash`). `processNearby` walks the cells of the square around the point
//! row by row and, in each, the cell's elements in the map's order (`equal_range`); `getNearby`
//! collects what it hands over. A grid is filled by [`crate::sparse_point_grid`] or
//! [`crate::sparse_line_grid`], which hold one of these.

use core::ops::Deref;

use orca_geom::point::{Coord, Point};

use crate::square_grid::{GridPoint, SquareGrid};
use crate::unordered_multimap::{Iter, UnorderedMultimap};

/// `PointHash` of a grid cell, as the map's hash function.
fn cell_hash(p: &GridPoint) -> u64 {
    orca_voronoi::point_hash(*p)
}

/// `SparseGrid<ElemT>`.
#[derive(Debug)]
pub struct SparseGrid<E> {
    /// The base class `SquareGrid`.
    pub(crate) square: SquareGrid,
    /// `m_grid`: the elements by cell.
    pub(crate) m_grid: UnorderedMultimap<GridPoint, E>,
}

impl<E> Deref for SparseGrid<E> {
    type Target = SquareGrid;
    /// The base class `SquareGrid`.
    fn deref(&self) -> &SquareGrid {
        &self.square
    }
}

impl<E> SparseGrid<E> {
    /// `SparseGrid(cell_size, elem_reserve = 0, max_load_factor = 1.0f)`: the map's maximum load
    /// factor set first, then room for `elem_reserve` elements when it is not 0.
    #[must_use]
    pub fn new(cell_size: Coord, elem_reserve: usize, max_load_factor: f32) -> Self {
        let mut m_grid = UnorderedMultimap::new(cell_hash);
        // Must be before the reserve call.
        m_grid.set_max_load_factor(max_load_factor);
        if elem_reserve != 0 {
            m_grid.reserve(elem_reserve);
        }
        Self {
            square: SquareGrid::new(cell_size),
            m_grid,
        }
    }

    /// `begin()` to `end()`: each element with its cell, in the map's order.
    #[must_use]
    pub fn iter(&self) -> Iter<'_, GridPoint, E> {
        self.m_grid.iter()
    }

    /// The map's bucket count, as `m_grid.bucket_count()` gives it.
    #[must_use]
    pub const fn bucket_count(&self) -> usize {
        self.m_grid.bucket_count()
    }

    /// `processFromCell(grid_pt, process_func)`: the elements of one cell, each handed to
    /// `process_func` until it returns false. Returns whether to go on to the next cell.
    pub fn process_from_cell(
        &self,
        grid_pt: GridPoint,
        process_func: &mut impl FnMut(&E) -> bool,
    ) -> bool {
        for (_, e) in self.m_grid.equal_range(&grid_pt) {
            if !process_func(e) {
                return false;
            }
        }
        true
    }

    /// `processNearby(query_pt, radius, process_func)`: the elements of every cell that may hold
    /// one within `radius` of `query_pt` (some up to `radius` plus a cell further), each handed to
    /// `process_func` until it returns false. Returns false when the callback stopped it.
    pub fn process_nearby(
        &self,
        query_pt: Point,
        radius: Coord,
        mut process_func: impl FnMut(&E) -> bool,
    ) -> bool {
        self.square.process_nearby(query_pt, radius, |grid_pt| {
            self.process_from_cell(grid_pt, &mut process_func)
        })
    }

    /// `getNearby(query_pt, radius)`: what [`SparseGrid::process_nearby`] hands over, in its
    /// order.
    #[must_use]
    pub fn get_nearby(&self, query_pt: Point, radius: Coord) -> Vec<E>
    where
        E: Clone,
    {
        let mut ret = Vec::new();
        self.process_nearby(query_pt, radius, |elem| {
            ret.push(elem.clone());
            true
        });
        ret
    }
}
