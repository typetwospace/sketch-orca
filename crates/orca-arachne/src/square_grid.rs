// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Arachne/utils/SquareGrid.hpp lines 1-113 and
// SquareGrid.cpp lines 1-150 whole, of OrcaSlicer v2.4.2, translated on 2026-11-14 by arachne-2.
// Copyright Ultimaker B.V. (2021, CuraEngine), with the PrusaSlicer authors' (Prusa Research) and
// the OrcaSlicer authors' changes, under the GNU Affero General Public License, version 3
// (AGPL-3.0; LICENSE).

//! `SquareGrid`: the coordinates of an endless grid of square cells, and the walks that hand its
//! cells to a callback (a cell is a [`GridPoint`], its column and row).
//!
//! - `toGridCoord` divides by the cell size with C++'s truncating division, so the cells of
//!   column 0 and row 0 are twice as large as the others (from -cell + 1 to cell - 1);
//!   `toLowerCoord` multiplies back. The C++ says this is on purpose and the quirk is kept.
//! - `processLineCells` walks the rows a line crosses (its ends ordered by x), and in each row
//!   the columns from where the last row ended to the column of the line at the next row's
//!   nearest edge, rounded the C++'s way; so it hands over a fat line, each row's last cell
//!   again as the next row's first, and stops at the end cell or when the callback says so. A
//!   walk that passes the end cell's row without meeting it returns false (the C++ asserts it
//!   never does).
//! - `processNearby` hands over every cell of the square of half-width `radius` around the point,
//!   row by row from the lowest.
//!
//! The C++ has a const and a non-const `processLineCells` that do the same; here there is one.
//! The callbacks are closures (`std::function` in the C++), called in the same order. The integer
//! arithmetic wraps where the C++'s would overflow, as the release build does.

use orca_geom::point::{Coord, Point};

/// `SquareGrid::GridPoint`: a cell's column and row.
pub type GridPoint = Point;
/// `SquareGrid::grid_coord_t`.
pub type GridCoord = Coord;

/// `SquareGrid`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SquareGrid {
    /// `cell_size`: the side of a cell.
    cell_size: Coord,
}

impl SquareGrid {
    /// `SquareGrid(cell_size)`. The C++ asserts `cell_size > 0` (not in a release build).
    #[must_use]
    pub const fn new(cell_size: Coord) -> Self {
        Self { cell_size }
    }

    /// `getCellSize()`.
    #[must_use]
    pub const fn get_cell_size(&self) -> Coord {
        self.cell_size
    }

    /// `toGridPoint(point)`: each coordinate's [`SquareGrid::to_grid_coord`].
    #[must_use]
    pub const fn to_grid_point(&self, point: Point) -> GridPoint {
        Point::new(self.to_grid_coord(point.x), self.to_grid_coord(point.y))
    }

    /// `toGridCoord(coord)`: `coord / cell_size`, truncated toward 0, so the cells at 0 are twice
    /// as large.
    #[must_use]
    pub const fn to_grid_coord(&self, coord: i64) -> GridCoord {
        coord.wrapping_div(self.cell_size)
    }

    /// `toLowerCoord(grid_coord)`: `grid_coord * cell_size`.
    #[must_use]
    pub const fn to_lower_coord(&self, grid_coord: GridCoord) -> Coord {
        grid_coord.wrapping_mul(self.cell_size)
    }

    /// `nonzeroSign(z)`: 1 for 0 and above, -1 below.
    #[must_use]
    pub const fn nonzero_sign(z: GridCoord) -> GridCoord {
        (z >= 0) as GridCoord - (z < 0) as GridCoord
    }

    /// `processLineCells(line, process_cell_func)`: the cells along the line from `line.0` to
    /// `line.1`, each handed to `process_cell_func` until it returns false. Returns whether to go
    /// on: false when the callback stopped it (or when the walk missed the end cell), true at the
    /// end cell.
    pub fn process_line_cells(
        &self,
        line: (Point, Point),
        mut process_cell_func: impl FnMut(GridPoint) -> bool,
    ) -> bool {
        let mut start = line.0;
        let mut end = line.1;
        if end.x < start.x {
            // make sure X increases between start and end
            core::mem::swap(&mut start, &mut end);
        }

        let start_cell = self.to_grid_point(start);
        let end_cell = self.to_grid_point(end);
        let y_diff = end.y.wrapping_sub(start.y);
        let y_dir = Self::nonzero_sign(y_diff);

        // This line drawing algorithm iterates over the range of Y coordinates, and for each Y
        // coordinate computes the range of X coordinates crossed in one unit of Y. These ranges
        // are rounded to be inclusive, so effectively this creates a "fat" line, marking more
        // cells than a strict one-cell-wide path.
        let mut x_cell_start = start_cell.x;
        let mut cell_y = start_cell.y;
        while cell_y.wrapping_mul(y_dir) <= end_cell.y.wrapping_mul(y_dir) {
            // nearest y coordinate of the cells in the next row
            let step = if Self::nonzero_sign(cell_y) == y_dir || cell_y == 0 {
                y_dir
            } else {
                0
            };
            let nearest_next_y = self.to_lower_coord(cell_y.wrapping_add(step));
            // the X coord of the last cell to include from this row
            let mut x_cell_end;
            if y_diff == 0 {
                x_cell_end = end_cell.x;
            } else {
                let area = end
                    .x
                    .wrapping_sub(start.x)
                    .wrapping_mul(nearest_next_y.wrapping_sub(start.y));
                // corresponding_x: the x coordinate corresponding to nearest_next_y
                let corresponding_x = start.x.wrapping_add(area.wrapping_div(y_diff));
                let round = i64::from(corresponding_x < 0 && area.wrapping_rem(y_diff) != 0);
                x_cell_end = self.to_grid_coord(corresponding_x.wrapping_add(round));
                if x_cell_end < start_cell.x {
                    // process at least one cell!
                    x_cell_end = x_cell_start;
                }
            }

            let mut cell_x = x_cell_start;
            while cell_x <= x_cell_end {
                let grid_loc = Point::new(cell_x, cell_y);
                if !process_cell_func(grid_loc) {
                    return false;
                }
                if grid_loc == end_cell {
                    return true;
                }
                cell_x = cell_x.wrapping_add(1);
            }
            // TODO: this causes at least a one cell overlap for each row, which includes extra
            // cells when crossing precisely on the corners where positive slope where x > 0 and
            // negative slope where x < 0
            x_cell_start = x_cell_end;
            cell_y = cell_y.wrapping_add(y_dir);
        }
        // The C++ asserts "We should have returned already before here!" (not in a release build).
        false
    }

    /// `processNearby(query_pt, radius, process_func)`: every cell of the square from
    /// `query_pt - radius` to `query_pt + radius`, row by row, each handed to `process_func`
    /// until it returns false. Returns false when the callback stopped it, else true.
    pub fn process_nearby(
        &self,
        query_pt: Point,
        radius: Coord,
        mut process_func: impl FnMut(GridPoint) -> bool,
    ) -> bool {
        let min_loc = Point::new(
            query_pt.x.wrapping_sub(radius),
            query_pt.y.wrapping_sub(radius),
        );
        let max_loc = Point::new(
            query_pt.x.wrapping_add(radius),
            query_pt.y.wrapping_add(radius),
        );

        let min_grid = self.to_grid_point(min_loc);
        let max_grid = self.to_grid_point(max_loc);

        let mut grid_y = min_grid.y;
        while grid_y <= max_grid.y {
            let mut grid_x = min_grid.x;
            while grid_x <= max_grid.x {
                let grid_pt = Point::new(grid_x, grid_y);
                if !process_func(grid_pt) {
                    return false;
                }
                grid_x = grid_x.wrapping_add(1);
            }
            grid_y = grid_y.wrapping_add(1);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use orca_geom::point::Point;

    use super::SquareGrid;

    #[test]
    fn the_cells_at_zero_are_twice_as_large() {
        let g = SquareGrid::new(1000);
        assert_eq!(g.to_grid_coord(999), 0);
        assert_eq!(g.to_grid_coord(-999), 0);
        assert_eq!(g.to_grid_coord(-1000), -1);
        assert_eq!(g.to_lower_coord(-3), -3000);
        assert_eq!(SquareGrid::nonzero_sign(0), 1);
        assert_eq!(SquareGrid::nonzero_sign(-4), -1);
    }

    #[test]
    fn a_line_along_x_takes_its_row() {
        let g = SquareGrid::new(1000);
        let mut cells = Vec::new();
        let r = g.process_line_cells((Point::new(4500, 500), Point::new(500, 500)), |c| {
            cells.push(c);
            true
        });
        assert!(r);
        let want: Vec<Point> = (0..5).map(|x| Point::new(x, 0)).collect();
        assert_eq!(cells, want);
        let mut n = 0;
        assert!(!g.process_nearby(Point::new(0, 0), 2500, |_| {
            n += 1;
            n < 3
        }));
        assert_eq!(n, 3);
    }
}
