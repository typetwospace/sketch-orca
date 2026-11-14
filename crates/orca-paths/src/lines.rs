// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/ShortestPath.cpp lines 2043-2099 and
// src/libslic3r/ShortestPath.hpp lines 56-62, with src/libslic3r/Point.hpp lines 506-636
// (ClosestPointInRadiusLookup), of OrcaSlicer v2.4.2, translated on 2026-11-13 by paths-1.
// Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and the PrusaSlicer
// authors (Prusa Research), under the GNU Affero General Public License, version 3 (AGPL-3.0;
// LICENSE).

//! [`chain_lines`]: lines joined into polylines where their ends meet within an epsilon, and
//! [`ClosestPointInRadiusLookup`], the grid it finds the ends in.
//!
//! - `chain_lines` keeps the square of the epsilon of the process's first call in a
//!   function-local static (`static const double point_distance_epsilon2`) and compares every
//!   later call's distances against it, while its lookup uses each call's own epsilon (truncated
//!   to `coord_t`). [`chain_lines`] keeps that static too; [`chain_lines_against`] takes the
//!   squared epsilon as an argument, so a caller (the probe replay) can name the one the C++
//!   process had.
//! - After `erase(*line_end)` the C++ goes on reading `line_end`, a pointer into the node it has
//!   just freed (its `other_end()` and its line); the node still holds the value then, so the
//!   translation reads a copy taken before the erase (`parity/HAZARDS.md`).
//! - `ClosestPointInRadiusLookup` keeps its values in a `std::unordered_multimap` keyed by grid
//!   cell and only ever reads one cell's equal range: MSVC keeps the values of one key in the order
//!   they were inserted (a new one after the last of its key), and `erase` takes the first equal
//!   one. A [`BTreeMap`] of per-cell lists in insertion order gives the same equal ranges; no
//!   iteration over the whole map, so no hash order, reaches a result.
//!
//! `ClosestPointInRadiusLookup` belongs in orca-geom, beside `Point`; no piece carries it there.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use orca_geom::line::Line;
use orca_geom::point::{Coord, Point};
use orca_geom::polyline::{Polyline, Polylines};

/// `ClosestPointInRadiusLookup<ValueType, PointAccessor>`: values by the grid cell of their
/// point, the cells twice the search radius wide (and more, to a power of two), each query
/// looking in the four cells about the point's nearest cell corner. The accessor gives a value's
/// point, or `None` for a value with none (the C++'s null pointer). Belongs in orca-geom.
pub struct ClosestPointInRadiusLookup<V, A> {
    point_accessor: A,
    map: BTreeMap<(Coord, Coord), Vec<V>>,
    search_radius: Coord,
    grid_resolution: Coord,
    grid_log2: Coord,
}

impl<V: PartialEq, A: Fn(&V) -> Option<Point>> ClosestPointInRadiusLookup<V, A> {
    /// `ClosestPointInRadiusLookup(search_radius, point_accessor)`.
    pub fn new(search_radius: Coord, point_accessor: A) -> Self {
        let mut grid_log2: Coord = 0;
        // Resolution of a grid, twice the search radius + some epsilon.
        let gridres = search_radius.wrapping_mul(2).wrapping_add(4);
        let mut grid_resolution = gridres;
        // Compute m_grid_log2 = log2(m_grid_resolution)
        if grid_resolution > 32767 {
            grid_resolution >>= 16;
            grid_log2 += 16;
        }
        if grid_resolution > 127 {
            grid_resolution >>= 8;
            grid_log2 += 8;
        }
        if grid_resolution > 7 {
            grid_resolution >>= 4;
            grid_log2 += 4;
        }
        if grid_resolution > 1 {
            grid_resolution >>= 2;
            grid_log2 += 2;
        }
        if grid_resolution > 0 {
            grid_log2 += 1;
        }
        // `1 << m_grid_log2` shifts the int 1, as x64's shl does.
        grid_resolution = Coord::from(1i32.wrapping_shl(grid_log2 as u32));
        Self {
            point_accessor,
            map: BTreeMap::new(),
            search_radius,
            grid_resolution,
            grid_log2,
        }
    }

    /// The grid cell of a point: each coordinate shifted right by `m_grid_log2`.
    fn cell(&self, pt: Point) -> (Coord, Coord) {
        (pt.x >> self.grid_log2, pt.y >> self.grid_log2)
    }

    /// `insert(value)`.
    pub fn insert(&mut self, value: V) {
        if let Some(pt) = (self.point_accessor)(&value) {
            let key = self.cell(pt);
            self.map.entry(key).or_default().push(value);
        }
    }

    /// `erase(value)`: the first value of its cell equal to `value` removed; whether there was
    /// one.
    pub fn erase(&mut self, value: &V) -> bool {
        if let Some(pt) = (self.point_accessor)(value) {
            // Range of fragment starts around grid_corner, close to pt.
            let key = self.cell(pt);
            if let Some(range) = self.map.get_mut(&key) {
                // Remove the first item.
                if let Some(k) = range.iter().position(|v| v == value) {
                    range.remove(k);
                    if range.is_empty() {
                        self.map.remove(&key);
                    }
                    return true;
                }
            }
        }
        false
    }

    /// The corner of the grid cell nearest `pt`.
    fn grid_corner(&self, pt: Point) -> (Coord, Coord) {
        (
            pt.x.wrapping_add(self.grid_resolution >> 1) >> self.grid_log2,
            pt.y.wrapping_add(self.grid_resolution >> 1) >> self.grid_log2,
        )
    }

    /// `find(pt)`: the value nearest `pt` in the four cells about it and its squared distance, the
    /// first of equally near ones, when nearer than the search radius; else `(None, DBL_MAX)`.
    pub fn find(&self, pt: Point) -> (Option<&V>, f64) {
        // Iterate over 4 closest grid cells around pt, find the closest start point inside these
        // cells to pt.
        let mut value_min = None;
        let mut dist_min = f64::MAX;
        // Round pt to a closest grid_cell corner.
        let grid_corner = self.grid_corner(pt);
        // For four neighbors of grid_corner:
        for neighbor_y in -1..1 {
            for neighbor_x in -1..1 {
                // Range of fragment starts around grid_corner, close to pt.
                let key = (grid_corner.0 + neighbor_x, grid_corner.1 + neighbor_y);
                let Some(range) = self.map.get(&key) else {
                    continue;
                };
                // Find the map entry closest to pt.
                for value in range {
                    if let Some(pt2) = (self.point_accessor)(value) {
                        let d2 = (pt - pt2).cast_f64().squared_norm();
                        if d2 < dist_min {
                            dist_min = d2;
                            value_min = Some(value);
                        }
                    }
                }
            }
        }
        if value_min.is_some() && dist_min < self.search_radius as f64 * self.search_radius as f64 {
            (value_min, dist_min)
        } else {
            (None, f64::MAX)
        }
    }

    /// `find_all(pt)`: every value in the four cells about `pt` within the search radius
    /// (`d2 <= r2`), with its squared distance, in the cells' order.
    pub fn find_all(&self, pt: Point) -> Vec<(&V, f64)> {
        // Round pt to a closest grid_cell corner.
        let grid_corner = self.grid_corner(pt);
        // For four neighbors of grid_corner:
        let mut out = Vec::new();
        let r2 = self.search_radius as f64 * self.search_radius as f64;
        for neighbor_y in -1..1 {
            for neighbor_x in -1..1 {
                // Range of fragment starts around grid_corner, close to pt.
                let key = (grid_corner.0 + neighbor_x, grid_corner.1 + neighbor_y);
                let Some(range) = self.map.get(&key) else {
                    continue;
                };
                // Find the map entry closest to pt.
                for value in range {
                    if let Some(pt2) = (self.point_accessor)(value) {
                        let d2 = (pt - pt2).cast_f64().squared_norm();
                        if d2 <= r2 {
                            out.push((value, d2));
                        }
                    }
                }
            }
        }
        out
    }
}

/// `chain_lines`' `LineEnd`: a line's index and whether it is its start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LineEnd {
    line: usize,
    // Is it the start or end point?
    start: bool,
}

impl LineEnd {
    /// `point()`.
    fn point(self, lines: &[Line]) -> Point {
        if self.start {
            lines[self.line].a
        } else {
            lines[self.line].b
        }
    }

    /// `other_point()`.
    fn other_point(self, lines: &[Line]) -> Point {
        if self.start {
            lines[self.line].b
        } else {
            lines[self.line].a
        }
    }

    /// `other_end()`.
    fn other_end(self) -> Self {
        Self {
            line: self.line,
            start: !self.start,
        }
    }
}

/// `static const double point_distance_epsilon2`: the square of the first call's epsilon.
static POINT_DISTANCE_EPSILON2: OnceLock<f64> = OnceLock::new();

/// `chain_lines(lines, point_distance_epsilon)`: the lines joined into polylines, the squared
/// distances compared against the square of the epsilon of this process's first call.
#[must_use]
pub fn chain_lines(lines: &[Line], point_distance_epsilon: f64) -> Polylines {
    let point_distance_epsilon2 =
        *POINT_DISTANCE_EPSILON2.get_or_init(|| point_distance_epsilon * point_distance_epsilon);
    chain_lines_against(lines, point_distance_epsilon, point_distance_epsilon2)
}

/// `chain_lines` with the function-local static given: each line not yet used seeds a polyline,
/// which grows at its end by the nearest free line end within the lookup's radius whose squared
/// distance is below `point_distance_epsilon2` (the joint averaged, truncated to `coord_t`), then
/// the same at its start.
#[must_use]
pub fn chain_lines_against(
    lines: &[Line],
    point_distance_epsilon: f64,
    point_distance_epsilon2: f64,
) -> Polylines {
    // Create line end point lookup.
    let mut closest_end_point_lookup = ClosestPointInRadiusLookup::new(
        orca_crt::f64_to_i64(point_distance_epsilon),
        |pt: &LineEnd| Some(pt.point(lines)),
    );
    for line in 0..lines.len() {
        closest_end_point_lookup.insert(LineEnd { line, start: true });
        closest_end_point_lookup.insert(LineEnd { line, start: false });
    }

    // Chain the lines.
    let mut line_consumed = vec![false; lines.len()];
    let mut out = Vec::new();
    for (seed_idx, seed) in lines.iter().enumerate() {
        if !line_consumed[seed_idx] {
            line_consumed[seed_idx] = true;
            closest_end_point_lookup.erase(&LineEnd {
                line: seed_idx,
                start: false,
            });
            closest_end_point_lookup.erase(&LineEnd {
                line: seed_idx,
                start: true,
            });
            let mut pl = Polyline::new(vec![seed.a, seed.b]);
            for _round in 0..2 {
                loop {
                    let (line_end, dist2) = closest_end_point_lookup.find(pl.last_point());
                    let Some(&line_end) = line_end else {
                        // Cannot extent in this direction.
                        break;
                    };
                    if dist2 >= point_distance_epsilon2 {
                        // Cannot extent in this direction.
                        break;
                    }
                    // Average the last point.
                    let back = pl.points.len() - 1;
                    pl.points[back] = Point::cast_from_vec2d(
                        0.5 * (pl.points[back].cast_f64() + line_end.point(lines).cast_f64()),
                    );
                    // and extend with the new line segment.
                    pl.points.push(line_end.other_point(lines));
                    closest_end_point_lookup.erase(&line_end);
                    closest_end_point_lookup.erase(&line_end.other_end());
                    line_consumed[line_end.line] = true;
                }
                // reverse and try the oter direction.
                pl.reverse();
            }
            out.push(pl);
        }
    }
    out
}
