// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/EdgeGrid.hpp lines 1-426 and src/libslic3r/EdgeGrid.cpp
// lines 1-1634 (all but the debug and SVG output and the code inside #if 0, named below), with
// sort_remove_duplicates of src/libslic3r/libslic3r.h (lines 217-222), of OrcaSlicer v2.4.2,
// translated on 2026-11-13 by index-2. Copyright the OrcaSlicer authors, the BambuStudio authors
// (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU Affero General Public
// License, version 3 (AGPL-3.0; LICENSE).

//! `EdgeGrid`: a regular grid of square cells over a set of contours, each cell listing the
//! contour segments that cross it ([`Grid`]), with its cell walks, the signed distance to the
//! contours near a point, a rough signed distance field over the cell corners and the contours it
//! extracts from the rasterised cells; and the free [`intersecting_edges`], which finds the
//! crossing segments of a set of polygons.
//!
//! - A [`Contour`] is a view of a run of points in the caller's container (the C++ keeps two
//!   pointers into it), so a [`Grid`] borrows what it was created from. A contour's segment `i`
//!   runs from point `i` to point `i + 1`, the last of a closed contour back to its first.
//!   Segment 0 of an open contour is past the engine's asserts: the release build gives the last
//!   index and the last point there ([`Contour::segment_idx_prev`], [`Contour::segment_prev`]),
//!   and `calculate_sdf` and the distances read it so. [`Contour::get_segments`] lists nothing
//!   for a contour of two segments or fewer, as the C++ does.
//! - `create` merges every point into the box it was given (an undefined box is replaced by the
//!   first point), then widens it by 16 on each side; the columns and rows are the box's sides
//!   divided by the resolution, rounded up. Each segment is rasterised twice, by the same integer
//!   walk (the crossing of a cell side is decided by exact `int64_t` products, ties at a corner
//!   going both ways, or to x where the C++ says so): once to count the segments of each cell,
//!   then, after a prefix sum, to fill the cells' contour and segment pairs. `create` does not
//!   clear the box or the field: a second `create` merges into the widened box and keeps the old
//!   field, as the C++ does.
//! - The distances look at the segments of the cells within the search radius: the nearest
//!   segment by its perpendicular distance, a vertex only when the point lies in the wedge where
//!   the previous segment also points away; the sign is the side of the segment, or the turn at
//!   the vertex. `calculate_sdf` seeds the cell corners within one cell of each segment the same
//!   way (`float` distances and vectors), spreads the sign by sweeps down and up, then the
//!   distance by Danielsson's chamfer sweeps; `signed_distance_bilinear` interpolates it in
//!   `float`, adding the distance outside the grid.
//! - `contours_simplified` marks the cells holding a segment (or a corner of the field at or
//!   below 0), optionally fills one-cell gaps, and chains the cell sides between marked and
//!   unmarked cells into polygons, scaled back to the world, each corner moved in by the offset
//!   and the collinear points dropped. The chaining reads an `std::unordered_multimap`; its
//!   result does not hang on the order of equal keys (at a corner the left turn wins, and a loop
//!   closes when it meets its first line, whichever comes first), so an ordered map stands in.
//! - `intersecting_edges` names a contour by its index in `contours()`, where the C++ keeps a
//!   pointer to it: the pointers lie in one vector, so they sort as the indices do. Its pairs are
//!   sorted and deduplicated by `sort_remove_duplicates` (`orca_stl::sort` and `unique_vec`).
//!
//! Where the engine's release build divides by zero, overflows a signed product, reads outside a
//! vector or reads a resolution never set, the grid stops with a [`crate::NotPorted`] payload
//! naming the functions and the condition as `parity/sop1/index.md`'s IXNP rows for index-2 give
//! them. A product that overflows in `visit_cells_intersecting_line` called by a caller stops with
//! the overflow row too, and `create`'s first raster pass stops with the cell walk's row when a
//! segment's end lies outside the cells (only a box missed by Eigen's compare past 2^31 can do
//! that, and `create`'s second pass would stop there anyway). A grid whose box was given with its
//! minimum above its maximum and no columns, or one created twice with a larger grid after
//! `calculate_sdf`, panics on its index (the C++ reads past a vector).
//!
//! Left out: `Grid::intersect` (every overload), `Grid::inside`, `Grid::line_cell_intersect` and
//! `div_floor`, which sit inside `#if 0`; `PropagateDanielssonSingleVStep3`, which nothing calls
//! (its two calls are commented out); the `EDGE_GRID_DEBUG_OUTPUT` and `#ifndef NDEBUG` blocks;
//! `EdgeGrid::save_png` and `Slic3r::export_intersections_to_svg` (debug output).
//!
//! `sort_remove_duplicates` belongs in orca-geom, beside the other helpers of `libslic3r.h`; no
//! piece carries it there, so index-2 translates it here.

use std::collections::BTreeMap;

use orca_geom::bounding_box::BoundingBox;
use orca_geom::expolygon::ExPolygon;
use orca_geom::geometry::segments_intersect;
use orca_geom::line::{Line, Lines};
use orca_geom::point::{Coord, Point, Points};
use orca_geom::polygon::{Polygon, Polygons};
use orca_geom::polyline::Polyline;

use crate::stop::not_ported;

const CREATE: &str = "EdgeGrid::Grid::create";
const RESOLUTION_ZERO: &str = "the resolution is 0: create_from_m_contours divides by it";
const RESOLUTION_NEGATIVE: &str = "the resolution is below 0: rows and columns come out negative and the cell vector's size wraps";
const CELLS_TOO_MANY: &str = "rows times columns overflows or cannot be allocated";
const OVERFLOW_FUNCTIONS: &str = "EdgeGrid::Grid::create, closest_point_signed_distance, signed_distance_edges, signed_distance, calculate_sdf";
const OVERFLOW: &str = "a product of a coordinate difference and the resolution, or of two coordinate differences, passes 2^63 (signed overflow)";
const VISIT_LINE: &str = "EdgeGrid::Grid::visit_cells_intersecting_line";
const VISIT_OUTSIDE: &str = "an end point lies outside the grid's cells: below the box's minimum, or at or past the minimum plus columns (rows) times the resolution; callers' visitors read that cell";
const BILINEAR: &str = "EdgeGrid::Grid::signed_distance_bilinear";
const BILINEAR_NO_FIELD: &str = "calculate_sdf has not run: it reads the empty field";
const QUERIES: &str = "EdgeGrid::Grid queries";
const NOT_CREATED: &str = "create has not run: the resolution was never set";
const INTERSECTING_EDGES: &str = "Slic3r::intersecting_edges(const Polygons &)";
const MEAN_EDGE_BELOW_ONE: &str = "the mean edge length over the polygons of two or more points is below 1, so the resolution (a double turned into coord_t) is 0";

/// `int64_t(a) * int64_t(b)`, stopping where the C++ product overflows.
fn mul(a: i64, b: i64) -> i64 {
    match a.checked_mul(b) {
        Some(v) => v,
        None => not_ported(OVERFLOW_FUNCTIONS, OVERFLOW),
    }
}

/// `a + b` of two such products, stopping where it overflows.
fn add(a: i64, b: i64) -> i64 {
    match a.checked_add(b) {
        Some(v) => v,
        None => not_ported(OVERFLOW_FUNCTIONS, OVERFLOW),
    }
}

/// `a - b` of two such products, stopping where it overflows.
fn sub(a: i64, b: i64) -> i64 {
    match a.checked_sub(b) {
        Some(v) => v,
        None => not_ported(OVERFLOW_FUNCTIONS, OVERFLOW),
    }
}

/// `row * m_cols + col`: a `coord_t` times a `size_t`, in `size_t`.
fn cell_index(row: Coord, col: Coord, cols: usize) -> usize {
    (row as usize).wrapping_mul(cols).wrapping_add(col as usize)
}

/// `sort_remove_duplicates(vec)` of `libslic3r.h`: `std::sort` by `operator<`, then
/// `std::unique` and `erase`. It belongs in orca-geom, beside the other helpers of `libslic3r.h`.
pub fn sort_remove_duplicates<T: PartialOrd>(vec: &mut Vec<T>) {
    orca_stl::sort(vec);
    orca_stl::unique_vec(vec);
}

/// `EdgeGrid::Contour`: a run of points of the caller's container and whether it is open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Contour<'a> {
    points: &'a [Point],
    open: bool,
}

impl<'a> Contour<'a> {
    /// `Contour(begin, end, open)`, `Contour(data, size, open)` and `Contour(pts, open)`.
    #[must_use]
    pub fn new(points: &'a [Point], open: bool) -> Self {
        Self { points, open }
    }

    /// `begin()` to `end()`: the contour's points.
    #[must_use]
    pub fn points(&self) -> &'a [Point] {
        self.points
    }

    /// `open()`.
    #[must_use]
    pub fn open(&self) -> bool {
        self.open
    }

    /// `closed()`.
    #[must_use]
    pub fn closed(&self) -> bool {
        !self.open
    }

    /// `front()`: `*m_begin`.
    #[must_use]
    pub fn front(&self) -> Point {
        self.points[0]
    }

    /// `back()`: `*(m_end - 1)`.
    #[must_use]
    pub fn back(&self) -> Point {
        self.points[self.size() - 1]
    }

    /// `segment_start(idx)`: `m_begin[idx]`.
    #[must_use]
    pub fn segment_start(&self, idx: usize) -> Point {
        self.points[idx]
    }

    /// The index of the point `segment_end(idx)` returns: `idx + 1`, or the first point when that
    /// is the end (the C++ compares the points' addresses).
    fn segment_end_index(&self, idx: usize) -> usize {
        let next = idx + 1;
        if next == self.size() { 0 } else { next }
    }

    /// `segment_end(idx)`: the next point, the first after the last.
    #[must_use]
    pub fn segment_end(&self, idx: usize) -> Point {
        self.points[self.segment_end_index(idx)]
    }

    /// `segment_prev(idx)`: `idx == 0 ? m_end[-1] : m_begin[idx - 1]`, the last point for segment
    /// 0 of an open contour too (the release build skips the assert).
    #[must_use]
    pub fn segment_prev(&self, idx: usize) -> Point {
        if idx == 0 {
            self.points[self.size() - 1]
        } else {
            self.points[idx - 1]
        }
    }

    /// `segment_idx_prev(idx)`: `(idx == 0 ? size() : idx) - 1`.
    #[must_use]
    pub fn segment_idx_prev(&self, idx: usize) -> usize {
        (if idx == 0 { self.size() } else { idx }).wrapping_sub(1)
    }

    /// `segment_idx_next(idx)`: `idx + 1`, or 0 where that is the end.
    #[must_use]
    pub fn segment_idx_next(&self, idx: usize) -> usize {
        let idx = idx + 1;
        if idx == self.size() { 0 } else { idx }
    }

    /// `num_segments()`: `size() - (m_open ? 1 : 0)`.
    #[must_use]
    pub fn num_segments(&self) -> usize {
        self.size().wrapping_sub(usize::from(self.open))
    }

    /// `get_segment(idx)`.
    #[must_use]
    pub fn get_segment(&self, idx: usize) -> Line {
        Line::new(self.segment_start(idx), self.segment_end(idx))
    }

    /// `get_segments()`: each pair of successive points and, closed, the last to the first; empty
    /// unless the contour has more than two segments.
    #[must_use]
    pub fn get_segments(&self) -> Lines {
        let mut lines = Lines::new();
        if self.num_segments() > 2 {
            for w in self.points.windows(2) {
                lines.push(Line::new(w[0], w[1]));
            }
            if !self.open {
                lines.push(Line::new(self.back(), self.front()));
            }
        }
        lines
    }

    /// `size()` (private in the C++): `m_end - m_begin`.
    #[must_use]
    pub fn size(&self) -> usize {
        self.points.len()
    }
}

/// `Grid::Cell`: the cell's range in the cell data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    /// `begin`.
    pub begin: usize,
    /// `end`.
    pub end: usize,
}

/// `Grid::ClosestPointResult`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClosestPointResult {
    /// `contour_idx`: `size_t(-1)` when nothing was found.
    pub contour_idx: usize,
    /// `start_point_idx`: the segment.
    pub start_point_idx: usize,
    /// `distance`: the signed distance to the closest point, `DBL_MAX` when nothing was found.
    pub distance: f64,
    /// `t`: where the closest point lies on the segment, in [0, 1].
    pub t: f64,
}

impl Default for ClosestPointResult {
    fn default() -> Self {
        Self {
            contour_idx: usize::MAX,
            start_point_idx: usize::MAX,
            distance: f64::MAX,
            t: 0.0,
        }
    }
}

impl ClosestPointResult {
    /// `valid()`.
    #[must_use]
    pub fn valid(&self) -> bool {
        self.contour_idx != usize::MAX
    }
}

/// `Grid::ContourPoint`: a contour (its index in `contours()`, the C++'s pointer) and a point.
pub type ContourPoint = (usize, usize);
/// `Grid::ContourEdge`: a contour (its index in `contours()`, the C++'s pointer) and a segment.
pub type ContourEdge = (usize, usize);

/// What the cell walk reads of a grid: the box's minimum, the resolution and the grid's size.
#[derive(Clone, Copy)]
struct Dims {
    min: Point,
    resolution: Coord,
    rows: usize,
    cols: usize,
}

impl Dims {
    /// The asserts of `visit_cells_intersecting_line` on an end point less the box's minimum:
    /// at or above 0 and below the columns (rows) times the resolution, in `size_t`.
    fn check_inside(self, p: Point) {
        let w = (self.cols as u64).wrapping_mul(self.resolution as u64);
        let h = (self.rows as u64).wrapping_mul(self.resolution as u64);
        if p.x < 0 || p.x as u64 >= w || p.y < 0 || p.y as u64 >= h {
            not_ported(VISIT_LINE, VISIT_OUTSIDE);
        }
    }
}

/// `EdgeGrid::Grid`.
#[derive(Clone, Debug, Default)]
pub struct Grid<'a> {
    /// `m_bbox`: the box around the contours.
    bbox: BoundingBox,
    /// `m_resolution`: never set until `create` runs (the C++ leaves it uninitialised).
    resolution: Option<Coord>,
    /// `m_rows`.
    rows: usize,
    /// `m_cols`.
    cols: usize,
    /// `m_contours`.
    contours: Vec<Contour<'a>>,
    /// `m_cell_data`: a contour and a segment of it.
    cell_data: Vec<(usize, usize)>,
    /// `m_cells`, row by row.
    cells: Vec<Cell>,
    /// `m_signed_distance_field`: (rows + 1) by (cols + 1) corners, empty until `calculate_sdf`.
    signed_distance_field: Vec<f32>,
}

impl<'a> Grid<'a> {
    /// `Grid()`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `Grid(bbox)`.
    #[must_use]
    pub fn with_bbox(bbox: BoundingBox) -> Self {
        Self {
            bbox,
            ..Self::default()
        }
    }

    /// `set_bbox(bbox)`.
    pub fn set_bbox(&mut self, bbox: BoundingBox) {
        self.bbox = bbox;
    }

    /// `create(const std::vector<Points> &, coord_t, bool open)`: each list of two points or more;
    /// with `open_polylines` a list whose first point is its last is closed, that point dropped.
    pub fn create_points(
        &mut self,
        polylines_or_polygons: &'a [Points],
        resolution: Coord,
        open_polylines: bool,
    ) {
        self.contours.clear();
        self.contours
            .reserve(polylines_or_polygons.iter().filter(|p| p.len() > 1).count());
        for points in polylines_or_polygons {
            if points.len() > 1 {
                let mut end = points.len();
                let mut open = open_polylines;
                if open_polylines && points[0] == points[end - 1] {
                    open = false;
                    end -= 1;
                }
                self.contours.push(Contour::new(&points[..end], open));
            }
        }
        self.create_from_m_contours(resolution);
    }

    /// `create(const Polygons &, const Polylines &, coord_t)`: the polylines of two points or more
    /// first (closed where the first point is the last, that point dropped), then the polygons of
    /// two points or more.
    pub fn create_polygons_polylines(
        &mut self,
        polygons: &'a [Polygon],
        polylines: &'a [Polyline],
        resolution: Coord,
    ) {
        self.contours.clear();
        self.contours.reserve(
            polygons.iter().filter(|p| p.points.len() > 1).count()
                + polylines.iter().filter(|p| p.points.len() > 1).count(),
        );
        for polyline in polylines {
            if polyline.points.len() > 1 {
                let mut end = polyline.points.len();
                let mut open = true;
                if polyline.points[0] == polyline.points[end - 1] {
                    open = false;
                    end -= 1;
                }
                self.contours
                    .push(Contour::new(&polyline.points[..end], open));
            }
        }
        for polygon in polygons {
            if polygon.points.len() > 1 {
                self.contours.push(Contour::new(&polygon.points, false));
            }
        }
        self.create_from_m_contours(resolution);
    }

    /// `create(const Polygons &, coord_t)`: each polygon that is not empty, closed.
    pub fn create_polygons(&mut self, polygons: &'a [Polygon], resolution: Coord) {
        self.contours.clear();
        self.contours
            .reserve(polygons.iter().filter(|p| !p.points.is_empty()).count());
        for polygon in polygons {
            if !polygon.points.is_empty() {
                self.contours.push(Contour::new(&polygon.points, false));
            }
        }
        self.create_from_m_contours(resolution);
    }

    /// `create(const std::vector<const Polygon *> &, coord_t)`.
    pub fn create_polygon_refs(&mut self, polygons: &[&'a Polygon], resolution: Coord) {
        self.contours.clear();
        self.contours
            .reserve(polygons.iter().filter(|p| !p.points.is_empty()).count());
        for polygon in polygons {
            if !polygon.points.is_empty() {
                self.contours.push(Contour::new(&polygon.points, false));
            }
        }
        self.create_from_m_contours(resolution);
    }

    /// `create(const std::vector<Points> &, coord_t)`: `create(polygons, resolution, false)`.
    pub fn create_closed_points(&mut self, polygons: &'a [Points], resolution: Coord) {
        self.create_points(polygons, resolution, false);
    }

    /// `create(const ExPolygon &, coord_t)`: its contour and holes that are not empty, closed.
    pub fn create_expolygon(&mut self, expoly: &'a ExPolygon, resolution: Coord) {
        self.contours.clear();
        self.contours.reserve(
            usize::from(!expoly.contour.points.is_empty())
                + expoly.holes.iter().filter(|p| !p.points.is_empty()).count(),
        );
        if !expoly.contour.points.is_empty() {
            self.contours
                .push(Contour::new(&expoly.contour.points, false));
        }
        for hole in &expoly.holes {
            if !hole.points.is_empty() {
                self.contours.push(Contour::new(&hole.points, false));
            }
        }
        self.create_from_m_contours(resolution);
    }

    /// `create(const ExPolygons &, coord_t)`: each ExPolygon's contour, then its holes.
    pub fn create_expolygons(&mut self, expolygons: &'a [ExPolygon], resolution: Coord) {
        let mut ncontours = 0;
        for expoly in expolygons {
            if !expoly.contour.points.is_empty() {
                ncontours += 1;
            }
            ncontours += expoly.holes.iter().filter(|p| !p.points.is_empty()).count();
        }
        self.contours.clear();
        self.contours.reserve(ncontours);
        for expoly in expolygons {
            if !expoly.contour.points.is_empty() {
                self.contours
                    .push(Contour::new(&expoly.contour.points, false));
            }
            for hole in &expoly.holes {
                if !hole.points.is_empty() {
                    self.contours.push(Contour::new(&hole.points, false));
                }
            }
        }
        self.create_from_m_contours(resolution);
    }

    /// `contours()`.
    #[must_use]
    pub fn contours(&self) -> &[Contour<'a>] {
        &self.contours
    }

    /// `m_resolution`, stopping where the C++ reads it uninitialised.
    fn res(&self) -> Coord {
        match self.resolution {
            Some(r) => r,
            None => not_ported(QUERIES, NOT_CREATED),
        }
    }

    fn dims(&self, resolution: Coord) -> Dims {
        Dims {
            min: self.bbox.min,
            resolution,
            rows: self.rows,
            cols: self.cols,
        }
    }

    /// `create_from_m_contours(resolution)`: the box, the grid's size, then both raster passes.
    fn create_from_m_contours(&mut self, resolution: Coord) {
        // 1) Measure the bounding box.
        for contour in &self.contours {
            for &pt in contour.points() {
                self.bbox.merge(pt);
            }
        }
        let eps: Coord = 16;
        self.bbox.min.x = self.bbox.min.x.wrapping_sub(eps);
        self.bbox.min.y = self.bbox.min.y.wrapping_sub(eps);
        self.bbox.max.x = self.bbox.max.x.wrapping_add(eps);
        self.bbox.max.y = self.bbox.max.y.wrapping_add(eps);

        // 2) Initialize the edge grid.
        if resolution == 0 {
            not_ported(CREATE, RESOLUTION_ZERO);
        }
        if resolution < 0 {
            not_ported(CREATE, RESOLUTION_NEGATIVE);
        }
        self.resolution = Some(resolution);
        self.cols = (self
            .bbox
            .max
            .x
            .wrapping_sub(self.bbox.min.x)
            .wrapping_add(resolution)
            .wrapping_sub(1)
            / resolution) as usize;
        self.rows = (self
            .bbox
            .max
            .y
            .wrapping_sub(self.bbox.min.y)
            .wrapping_add(resolution)
            .wrapping_sub(1)
            / resolution) as usize;
        // m_cells.assign(m_rows * m_cols, Cell()): the product wraps in size_t, and a vector past
        // max_size() or the memory throws.
        let ncells = match self.rows.checked_mul(self.cols) {
            Some(n) if n <= isize::MAX as usize / core::mem::size_of::<Cell>() => n,
            _ => not_ported(CREATE, CELLS_TOO_MANY),
        };
        let mut cells = Vec::new();
        if cells.try_reserve_exact(ncells).is_err() {
            not_ported(CREATE, CELLS_TOO_MANY);
        }
        cells.resize(ncells, Cell::default());
        self.cells = cells;

        // 3) First round of contour rasterization, count the edges per grid cell.
        let dims = self.dims(resolution);
        let cols = self.cols;
        for contour in &self.contours {
            for j in 0..contour.num_segments() {
                // End points of the line segment.
                let mut p1 = contour.segment_start(j);
                let mut p2 = contour.segment_end(j);
                p1.x = p1.x.wrapping_sub(self.bbox.min.x);
                p1.y = p1.y.wrapping_sub(self.bbox.min.y);
                p2.x = p2.x.wrapping_sub(self.bbox.min.x);
                p2.y = p2.y.wrapping_sub(self.bbox.min.y);
                // The second round walks the same segment through visit_cells_intersecting_line,
                // which stops on an end point outside the cells; this round would read past them.
                dims.check_inside(p1);
                dims.check_inside(p2);
                // Get the cells of the end points.
                let mut ix = p1.x / resolution;
                let mut iy = p1.y / resolution;
                let ixb = p2.x / resolution;
                let iyb = p2.y / resolution;
                // Account for the end points.
                self.cells[cell_index(iy, ix, cols)].end += 1;
                if ix == ixb && iy == iyb {
                    // Both ends fall into the same cell.
                    continue;
                }
                // Raster the centeral part of the line.
                let dx = p2.x.wrapping_sub(p1.x).wrapping_abs();
                let dy = p2.y.wrapping_sub(p1.y).wrapping_abs();
                if p1.x < p2.x {
                    let mut ex = mul(sub(mul(ix + 1, resolution), p1.x), dy);
                    if p1.y < p2.y {
                        // x positive, y positive
                        let mut ey = mul(sub(mul(iy + 1, resolution), p1.y), dx);
                        loop {
                            if ex < ey {
                                ey = sub(ey, ex);
                                ex = mul(dy, resolution);
                                ix += 1;
                            } else if ex == ey {
                                ex = mul(dy, resolution);
                                ey = mul(dx, resolution);
                                ix += 1;
                                iy += 1;
                            } else {
                                ex = sub(ex, ey);
                                ey = mul(dx, resolution);
                                iy += 1;
                            }
                            self.cells[cell_index(iy, ix, cols)].end += 1;
                            if ix == ixb && iy == iyb {
                                break;
                            }
                        }
                    } else {
                        // x positive, y non positive
                        let mut ey = mul(sub(p1.y, mul(iy, resolution)), dx);
                        loop {
                            if ex <= ey {
                                ey = sub(ey, ex);
                                ex = mul(dy, resolution);
                                ix += 1;
                            } else {
                                ex = sub(ex, ey);
                                ey = mul(dx, resolution);
                                iy -= 1;
                            }
                            self.cells[cell_index(iy, ix, cols)].end += 1;
                            if ix == ixb && iy == iyb {
                                break;
                            }
                        }
                    }
                } else {
                    let mut ex = mul(sub(p1.x, mul(ix, resolution)), dy);
                    if p1.y < p2.y {
                        // x non positive, y positive
                        let mut ey = mul(sub(mul(iy + 1, resolution), p1.y), dx);
                        loop {
                            if ex < ey {
                                ey = sub(ey, ex);
                                ex = mul(dy, resolution);
                                ix -= 1;
                            } else {
                                ex = sub(ex, ey);
                                ey = mul(dx, resolution);
                                iy += 1;
                            }
                            self.cells[cell_index(iy, ix, cols)].end += 1;
                            if ix == ixb && iy == iyb {
                                break;
                            }
                        }
                    } else {
                        // x non positive, y non positive
                        let mut ey = mul(sub(p1.y, mul(iy, resolution)), dx);
                        loop {
                            if ex < ey {
                                ey = sub(ey, ex);
                                ex = mul(dy, resolution);
                                ix -= 1;
                            } else if ex == ey {
                                // The lower edge of a grid cell belongs to the cell.
                                if dx > 0 {
                                    ex = mul(dy, resolution);
                                    ix -= 1;
                                }
                                if dy > 0 {
                                    ey = mul(dx, resolution);
                                    iy -= 1;
                                }
                            } else {
                                ex = sub(ex, ey);
                                ey = mul(dx, resolution);
                                iy -= 1;
                            }
                            self.cells[cell_index(iy, ix, cols)].end += 1;
                            if ix == ixb && iy == iyb {
                                break;
                            }
                        }
                    }
                }
            }
        }

        // 4) Prefix sum the numbers of hits per cells to get an index into m_cell_data. With no
        // cells, m_cells.front() reads past the empty vector: the index panics here.
        let mut cnt = self.cells[0].end;
        for cell in self.cells.iter_mut().skip(1) {
            cell.begin = cnt;
            cnt += cell.end;
            cell.end = cnt;
        }

        // 5) Allocate the cell data.
        self.cell_data.clear();
        self.cell_data.resize(cnt, (usize::MAX, usize::MAX));

        // 6) Finally fill in m_cell_data by rasterizing the lines once again.
        for cell in &mut self.cells {
            cell.end = cell.begin;
        }
        let cells = &mut self.cells;
        let cell_data = &mut self.cell_data;
        for (i, contour) in self.contours.iter().enumerate() {
            for j in 0..contour.num_segments() {
                visit_line(
                    dims,
                    contour.segment_start(j),
                    contour.segment_end(j),
                    &mut |iy: Coord, ix: Coord| {
                        let cell = &mut cells[cell_index(iy, ix, cols)];
                        cell_data[cell.end] = (i, j);
                        cell.end += 1;
                        // Continue traversing the grid along the edge.
                        true
                    },
                );
            }
        }
    }

    /// `calculate_sdf()`: the rough signed distance field over the cell corners.
    pub fn calculate_sdf(&mut self) {
        // 1) Initialize a signum and an unsigned vector to a zero iso surface.
        let nrows = self.rows + 1;
        let ncols = self.cols + 1;
        // Unsigned vectors towards the closest point on the surface.
        let mut l = vec![f32::MAX; nrows * ncols * 2];
        // Bit 0 set - negative. Bit 1 set - original value, the distance value shall not be
        // changed by the Danielsson propagation. Bit 2 set - signum not propagated yet.
        let mut signs = vec![4_u8; nrows * ncols];
        let resolution = self.res();
        let search_radius = (resolution << 1) as f32;
        self.signed_distance_field.clear();
        self.signed_distance_field
            .resize(nrows * ncols, search_radius);
        // For each cell:
        for r in 0..(self.rows as i32) {
            for c in 0..(self.cols as i32) {
                let cell = self.cells[cell_index(Coord::from(r), Coord::from(c), self.cols)];
                // For each segment in the cell:
                for i in cell.begin..cell.end {
                    let contour = self.contours[self.cell_data[i].0];
                    let ipt = self.cell_data[i].1;
                    // End points of the line segment.
                    let p1 = contour.segment_start(ipt);
                    let p2 = contour.segment_end(ipt);
                    // Segment vector
                    let v_seg = p2 - p1;
                    // l2 of v_seg
                    let l2_seg = add(mul(v_seg.x, v_seg.x), mul(v_seg.y, v_seg.y));
                    // For each corner of this cell and its 1 ring neighbours:
                    for corner_y in -1..3 {
                        let corner_r = Coord::from(r + corner_y);
                        if corner_r < 0 || corner_r as usize >= nrows {
                            continue;
                        }
                        for corner_x in -1..3 {
                            let corner_c = Coord::from(c + corner_x);
                            if corner_c < 0 || corner_c as usize >= ncols {
                                continue;
                            }
                            let addr = cell_index(corner_r, corner_c, ncols);
                            let pt = Point::new(
                                self.bbox.min.x.wrapping_add(mul(corner_c, resolution)),
                                self.bbox.min.y.wrapping_add(mul(corner_r, resolution)),
                            );
                            let v_pt = pt - p1;
                            // dot(p2-p1, pt-p1)
                            let t_pt = add(mul(v_seg.x, v_pt.x), mul(v_seg.y, v_pt.y));
                            if t_pt < 0 {
                                // Closest to p1.
                                let dabs = orca_crt::sqrt(add(
                                    mul(v_pt.x, v_pt.x),
                                    mul(v_pt.y, v_pt.y),
                                ) as f64);
                                if dabs < f64::from(self.signed_distance_field[addr]) {
                                    // Previous point.
                                    let p0 = contour.segment_prev(ipt);
                                    let v_seg_prev = p1 - p0;
                                    let t2_pt =
                                        add(mul(v_seg_prev.x, v_pt.x), mul(v_seg_prev.y, v_pt.y));
                                    if t2_pt > 0 {
                                        // Inside the wedge between the previous and the next
                                        // segment. Set the signum depending on whether the
                                        // vertex is convex or reflex.
                                        let det = sub(
                                            mul(v_seg_prev.x, v_seg.y),
                                            mul(v_seg_prev.y, v_seg.x),
                                        );
                                        self.signed_distance_field[addr] = dabs as f32;
                                        // Fill in an unsigned vector towards the zero iso surface.
                                        l[addr << 1] = v_pt.x.wrapping_abs() as f32;
                                        l[(addr << 1) + 1] = v_pt.y.wrapping_abs() as f32;
                                        signs[addr] = u8::from(det < 0) | 2;
                                    }
                                }
                            } else if t_pt > l2_seg {
                                // Closest to p2. Then p2 is the starting point of another
                                // segment, which shall be discovered in the same cell.
                                continue;
                            } else {
                                // Closest to the segment.
                                let d_seg = sub(mul(v_seg.y, v_pt.x), mul(v_seg.x, v_pt.y));
                                let d = d_seg as f64 / orca_crt::sqrt(l2_seg as f64);
                                let dabs = orca_crt::fabs(d);
                                if dabs < f64::from(self.signed_distance_field[addr]) {
                                    self.signed_distance_field[addr] = dabs as f32;
                                    // Fill in an unsigned vector towards the zero iso surface.
                                    let linv = d_seg as f32 / l2_seg as f32;
                                    l[addr << 1] = orca_crt::fabsf(v_seg.y as f32 * linv);
                                    l[(addr << 1) + 1] = orca_crt::fabsf(v_seg.x as f32 * linv);
                                    signs[addr] = u8::from(d_seg < 0) | 2;
                                }
                            }
                        }
                    }
                }
            }
        }

        // 2) Propagate the signum.
        let propagate_signum_single_step = |signs: &mut [u8], addr: usize, delta: isize| {
            if signs[addr] & 4 != 0 {
                let old_val = signs[addr.wrapping_add_signed(delta)];
                if old_val & 4 == 0 {
                    signs[addr] = old_val & 1;
                }
            }
        };
        let ncols_delta = ncols as isize;
        // Top to bottom propagation.
        for r in 0..nrows {
            if r > 0 {
                for c in 0..ncols {
                    propagate_signum_single_step(&mut signs, r * ncols + c, -ncols_delta);
                }
            }
            for c in 1..ncols {
                propagate_signum_single_step(&mut signs, r * ncols + c, -1);
            }
            let mut c = ncols as i32 - 2;
            while c >= 0 {
                propagate_signum_single_step(&mut signs, r * ncols + c as usize, 1);
                c -= 1;
            }
        }
        // Bottom to top propagation.
        let mut r = nrows as i32 - 2;
        while r >= 0 {
            let row = r as usize * ncols;
            for c in 0..ncols {
                propagate_signum_single_step(&mut signs, row + c, ncols_delta);
            }
            for c in 1..ncols {
                propagate_signum_single_step(&mut signs, row + c, -1);
            }
            let mut c = ncols as i32 - 2;
            while c >= 0 {
                propagate_signum_single_step(&mut signs, row + c as usize, 1);
                c -= 1;
            }
            r -= 1;
        }

        // 3) Propagate the distance by the Danielsson chamfer metric.
        let danielsson_hstep = PropagateDanielssonSingleStep::<1, 0> {
            stride: ncols,
            resolution,
        };
        let danielsson_vstep = PropagateDanielssonSingleStep::<0, 1> {
            stride: ncols,
            resolution,
        };
        let ncols_int = ncols as i32;
        // Top to bottom propagation.
        for r in 0..nrows {
            if r > 0 {
                for c in 0..ncols {
                    danielsson_vstep.step(&mut l, &signs, r as i32, c as i32, -ncols_int);
                }
            }
            for c in 1..ncols {
                danielsson_hstep.step(&mut l, &signs, r as i32, c as i32, -1);
            }
            let mut c = ncols as i32 - 2;
            while c >= 0 {
                danielsson_hstep.step(&mut l, &signs, r as i32, c, 1);
                c -= 1;
            }
        }
        // Bottom to top propagation.
        let mut r = nrows as i32 - 2;
        while r >= 0 {
            for c in 0..ncols {
                danielsson_vstep.step(&mut l, &signs, r, c as i32, ncols_int);
            }
            for c in 1..ncols {
                danielsson_hstep.step(&mut l, &signs, r, c as i32, -1);
            }
            let mut c = ncols as i32 - 2;
            while c >= 0 {
                danielsson_hstep.step(&mut l, &signs, r, c, 1);
                c -= 1;
            }
            r -= 1;
        }

        // Update signed distance field from absolte vectors to the iso-surface.
        for r in 0..nrows {
            for c in 0..ncols {
                let addr = r * ncols + c;
                let v = addr << 1;
                let mut d = orca_crt::sqrtf(l[v] * l[v] + l[v + 1] * l[v + 1]);
                if signs[addr] & 1 != 0 {
                    d = -d;
                }
                self.signed_distance_field[addr] = d;
            }
        }
    }

    /// `signed_distance_bilinear(pt)`: the field interpolated in `float`; outside the grid the
    /// point is clamped to it and its distance to the grid added (subtracted where the
    /// interpolation is not above 0).
    #[must_use]
    pub fn signed_distance_bilinear(&self, pt: Point) -> f32 {
        let x = pt.x.wrapping_sub(self.bbox.min.x);
        let y = pt.y.wrapping_sub(self.bbox.min.y);
        let resolution = self.res();
        let w = (resolution as u64).wrapping_mul(self.cols as u64) as Coord;
        let h = (resolution as u64).wrapping_mul(self.rows as u64) as Coord;
        let mut clamped = false;
        let mut xcl = x;
        let mut ycl = y;
        if x < 0 {
            xcl = 0;
            clamped = true;
        } else if x >= w {
            xcl = w.wrapping_sub(1);
            clamped = true;
        }
        if y < 0 {
            ycl = 0;
            clamped = true;
        } else if y >= h {
            ycl = h.wrapping_sub(1);
            clamped = true;
        }

        let cell_c = orca_crt::f64_to_i64(orca_crt::floor((xcl / resolution) as f64));
        let cell_r = orca_crt::f64_to_i64(orca_crt::floor((ycl / resolution) as f64));
        let tx = xcl.wrapping_sub(cell_c.wrapping_mul(resolution)) as f32 / resolution as f32;
        let ty = ycl.wrapping_sub(cell_r.wrapping_mul(resolution)) as f32 / resolution as f32;
        if self.signed_distance_field.is_empty() {
            not_ported(BILINEAR, BILINEAR_NO_FIELD);
        }
        let mut addr = cell_index(cell_r, cell_c, self.cols + 1);
        let f00 = self.signed_distance_field[addr];
        let f01 = self.signed_distance_field[addr + 1];
        addr += self.cols + 1;
        let f10 = self.signed_distance_field[addr];
        let f11 = self.signed_distance_field[addr + 1];
        let f0 = (1.0 - tx) * f00 + tx * f01;
        let f1 = (1.0 - tx) * f10 + tx * f11;
        let mut f = (1.0 - ty) * f0 + ty * f1;

        if clamped {
            if f > 0.0 {
                if x < 0 {
                    f += x.wrapping_neg() as f32;
                } else if x >= w {
                    f += x.wrapping_sub(w).wrapping_add(1) as f32;
                }
                if y < 0 {
                    f += y.wrapping_neg() as f32;
                } else if y >= h {
                    f += y.wrapping_sub(h).wrapping_add(1) as f32;
                }
            } else {
                if x < 0 {
                    f -= x.wrapping_neg() as f32;
                } else if x >= w {
                    f -= x.wrapping_sub(w).wrapping_add(1) as f32;
                }
                if y < 0 {
                    f -= y.wrapping_neg() as f32;
                } else if y >= h {
                    f -= y.wrapping_sub(h).wrapping_add(1) as f32;
                }
            }
        }
        f
    }

    /// The cells within `search_radius` of `pt`, as the distance queries bound them: the box of
    /// cells (rows then columns), or `None` where it is empty. `None` before the resolution is
    /// read when the box lies wholly below the grid.
    fn search_cells(&self, pt: Point, search_radius: Coord) -> Option<BoundingBox> {
        let mut bbox = BoundingBox::new();
        let p = Point::new(
            pt.x.wrapping_sub(self.bbox.min.x),
            pt.y.wrapping_sub(self.bbox.min.y),
        );
        bbox.min = p;
        bbox.max = p;
        bbox.defined = true;
        // Upper boundary, round to grid and test validity.
        bbox.max.x = bbox.max.x.wrapping_add(search_radius);
        bbox.max.y = bbox.max.y.wrapping_add(search_radius);
        if bbox.max.x < 0 || bbox.max.y < 0 {
            return None;
        }
        let resolution = self.res();
        bbox.max.x /= resolution;
        bbox.max.y /= resolution;
        if bbox.max.x as usize >= self.cols {
            bbox.max.x = self.cols.wrapping_sub(1) as Coord;
        }
        if bbox.max.y as usize >= self.rows {
            bbox.max.y = self.rows.wrapping_sub(1) as Coord;
        }
        // Lower boundary, round to grid and test validity.
        bbox.min.x = bbox.min.x.wrapping_sub(search_radius);
        bbox.min.y = bbox.min.y.wrapping_sub(search_radius);
        if bbox.min.x < 0 {
            bbox.min.x = 0;
        }
        if bbox.min.y < 0 {
            bbox.min.y = 0;
        }
        bbox.min.x /= resolution;
        bbox.min.y /= resolution;
        // Is the interval empty?
        if bbox.min.x > bbox.max.x || bbox.min.y > bbox.max.y {
            return None;
        }
        Some(bbox)
    }

    /// Calls `f` with each contour index, contour and segment of the cells in `bbox`, rows then
    /// columns (the C++'s `int` loops), in each cell's order.
    fn for_each_segment(&self, bbox: &BoundingBox, mut f: impl FnMut(usize, &Contour<'a>, usize)) {
        let mut r = bbox.min.y as i32;
        while Coord::from(r) <= bbox.max.y {
            let mut c = bbox.min.x as i32;
            while Coord::from(c) <= bbox.max.x {
                let cell = self.cells[cell_index(Coord::from(r), Coord::from(c), self.cols)];
                for i in cell.begin..cell.end {
                    let contour_idx = self.cell_data[i].0;
                    f(
                        contour_idx,
                        &self.contours[contour_idx],
                        self.cell_data[i].1,
                    );
                }
                c += 1;
            }
            r += 1;
        }
    }

    /// `closest_point_signed_distance(pt, search_radius)`.
    #[must_use]
    pub fn closest_point_signed_distance(
        &self,
        pt: Point,
        search_radius: Coord,
    ) -> ClosestPointResult {
        let mut result = ClosestPointResult::default();
        let Some(bbox) = self.search_cells(pt, search_radius) else {
            return result;
        };
        // Traverse all cells in the bounding box.
        let mut d_min = search_radius as f64;
        // Signum of the distance field at pt.
        let mut sign_min: i32 = 0;
        let mut l2_seg_min = 1.0_f64;
        self.for_each_segment(&bbox, |contour_idx, contour, ipt| {
            // End points of the line segment.
            let p1 = contour.segment_start(ipt);
            let p2 = contour.segment_end(ipt);
            let v_seg = p2 - p1;
            let v_pt = pt - p1;
            // dot(p2-p1, pt-p1)
            let t_pt = add(mul(v_seg.x, v_pt.x), mul(v_seg.y, v_pt.y));
            // l2 of seg
            let l2_seg = add(mul(v_seg.x, v_seg.x), mul(v_seg.y, v_seg.y));
            if t_pt < 0 {
                // Closest to p1.
                let dabs = orca_crt::sqrt(add(mul(v_pt.x, v_pt.x), mul(v_pt.y, v_pt.y)) as f64);
                if dabs < d_min {
                    // Previous point.
                    let p0 = contour.segment_prev(ipt);
                    let v_seg_prev = p1 - p0;
                    let t2_pt = add(mul(v_seg_prev.x, v_pt.x), mul(v_seg_prev.y, v_pt.y));
                    if t2_pt > 0 {
                        // Inside the wedge between the previous and the next segment.
                        d_min = dabs;
                        // Set the signum depending on whether the vertex is convex or reflex.
                        let det = sub(mul(v_seg_prev.x, v_seg.y), mul(v_seg_prev.y, v_seg.x));
                        sign_min = if det > 0 { 1 } else { -1 };
                        result.contour_idx = contour_idx;
                        result.start_point_idx = ipt;
                        result.t = 0.0;
                    }
                }
            } else if t_pt > l2_seg {
                // Closest to p2. Then p2 is the starting point of another segment, which shall be
                // discovered in the same cell.
            } else {
                // Closest to the segment.
                let d_seg = sub(mul(v_seg.y, v_pt.x), mul(v_seg.x, v_pt.y));
                let d = d_seg as f64 / orca_crt::sqrt(l2_seg as f64);
                let dabs = orca_crt::fabs(d);
                if dabs < d_min {
                    d_min = dabs;
                    sign_min = d_seg.signum() as i32;
                    l2_seg_min = l2_seg as f64;
                    result.contour_idx = contour_idx;
                    result.start_point_idx = ipt;
                    result.t = t_pt as f64;
                }
            }
        });
        if result.contour_idx != usize::MAX && d_min <= search_radius as f64 {
            result.distance = d_min * f64::from(sign_min);
            result.t /= l2_seg_min;
        } else {
            result = ClosestPointResult::default();
        }
        result
    }

    /// `signed_distance_edges(pt, search_radius, result_min_dist, pon_segment)`: true, with the
    /// signed distance (and whether it is to a segment rather than a vertex), when a segment lies
    /// nearer than `search_radius`; `result_min_dist` and `pon_segment` are left alone otherwise.
    pub fn signed_distance_edges(
        &self,
        pt: Point,
        search_radius: Coord,
        result_min_dist: &mut f64,
        pon_segment: Option<&mut bool>,
    ) -> bool {
        let Some(bbox) = self.search_cells(pt, search_radius) else {
            return false;
        };
        // Traverse all cells in the bounding box.
        let mut d_min = search_radius as f64;
        // Signum of the distance field at pt.
        let mut sign_min: i32 = 0;
        let mut on_segment = false;
        self.for_each_segment(&bbox, |_, contour, ipt| {
            // End points of the line segment.
            let p1 = contour.segment_start(ipt);
            let p2 = contour.segment_end(ipt);
            let v_seg = p2 - p1;
            let v_pt = pt - p1;
            // dot(p2-p1, pt-p1)
            let t_pt = add(mul(v_seg.x, v_pt.x), mul(v_seg.y, v_pt.y));
            // l2 of seg
            let l2_seg = add(mul(v_seg.x, v_seg.x), mul(v_seg.y, v_seg.y));
            if t_pt < 0 {
                // Closest to p1.
                let dabs = orca_crt::sqrt(add(mul(v_pt.x, v_pt.x), mul(v_pt.y, v_pt.y)) as f64);
                if dabs < d_min {
                    // Previous point.
                    let p0 = contour.segment_prev(ipt);
                    let v_seg_prev = p1 - p0;
                    let t2_pt = add(mul(v_seg_prev.x, v_pt.x), mul(v_seg_prev.y, v_pt.y));
                    if t2_pt > 0 {
                        // Inside the wedge between the previous and the next segment.
                        d_min = dabs;
                        // Set the signum depending on whether the vertex is convex or reflex.
                        let det = sub(mul(v_seg_prev.x, v_seg.y), mul(v_seg_prev.y, v_seg.x));
                        sign_min = if det > 0 { 1 } else { -1 };
                        on_segment = false;
                    }
                }
            } else if t_pt > l2_seg {
                // Closest to p2. Then p2 is the starting point of another segment, which shall be
                // discovered in the same cell.
            } else {
                // Closest to the segment.
                let d_seg = sub(mul(v_seg.y, v_pt.x), mul(v_seg.x, v_pt.y));
                let d = d_seg as f64 / orca_crt::sqrt(l2_seg as f64);
                let dabs = orca_crt::fabs(d);
                if dabs < d_min {
                    d_min = dabs;
                    sign_min = d_seg.signum() as i32;
                    on_segment = true;
                }
            }
        });
        if d_min >= search_radius as f64 {
            return false;
        }
        *result_min_dist = d_min * f64::from(sign_min);
        if let Some(on) = pon_segment {
            *on = on_segment;
        }
        true
    }

    /// `signed_distance(pt, search_radius, result_min_dist)`: `signed_distance_edges`, else the
    /// field interpolated, when there is one.
    pub fn signed_distance(
        &self,
        pt: Point,
        search_radius: Coord,
        result_min_dist: &mut f64,
    ) -> bool {
        if self.signed_distance_edges(pt, search_radius, result_min_dist, None) {
            return true;
        }
        if self.signed_distance_field.is_empty() {
            return false;
        }
        *result_min_dist = f64::from(self.signed_distance_bilinear(pt));
        true
    }

    /// `bbox()`.
    #[must_use]
    pub fn bbox(&self) -> &BoundingBox {
        &self.bbox
    }

    /// `resolution()`, which the C++ returns uninitialised before `create` (a stop here).
    #[must_use]
    pub fn resolution(&self) -> Coord {
        self.res()
    }

    /// `rows()`.
    #[must_use]
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// `cols()`.
    #[must_use]
    pub fn cols(&self) -> usize {
        self.cols
    }

    /// `m_cells` (protected in the C++, read by the probe through a subclass).
    #[must_use]
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    /// `m_cell_data` (protected in the C++, read by the probe through a subclass).
    #[must_use]
    pub fn cell_data(&self) -> &[(usize, usize)] {
        &self.cell_data
    }

    /// `m_signed_distance_field` (protected in the C++, read by the probe through a subclass).
    #[must_use]
    pub fn signed_distance_field(&self) -> &[f32] {
        &self.signed_distance_field
    }

    /// `cell_inside_or_crossing(r, c)`: a cell of the grid that holds a segment, or whose corner of
    /// the field is at or below 0.
    fn cell_inside_or_crossing(&self, r: i32, c: i32) -> bool {
        if r < 0 || r as usize >= self.rows || c < 0 || c as usize >= self.cols {
            // The cell is outside the domain.
            return false;
        }
        let cell = &self.cells[cell_index(Coord::from(r), Coord::from(c), self.cols)];
        (cell.begin < cell.end)
            || (!self.signed_distance_field.is_empty()
                && self.signed_distance_field
                    [cell_index(Coord::from(r), Coord::from(c), self.cols + 1)]
                    <= 0.0)
    }

    /// `contours_simplified(offset, fill_holes)`: for supports, the contours enclosing the
    /// rasterised edges.
    #[must_use]
    pub fn contours_simplified(&self, offset: Coord, fill_holes: bool) -> Polygons {
        // 0) Prepare a binary grid.
        let cell_rows = self.rows + 2;
        let cell_cols = self.cols + 2;
        let mut cell_inside = vec![false; cell_rows * cell_cols];
        for r in 0..(cell_rows as i32) {
            for c in 0..(cell_cols as i32) {
                cell_inside[(r as usize) * cell_cols + c as usize] =
                    self.cell_inside_or_crossing(r - 1, c - 1);
            }
        }
        // Fill in empty cells, which have a left / right neighbor filled. Fill in empty cells,
        // which have the top / bottom neighbor filled.
        if fill_holes {
            let cell_inside2 = cell_inside.clone();
            let mut r = 1;
            while r + 1 < cell_rows as i32 {
                let mut c = 1;
                while c + 1 < cell_cols as i32 {
                    let addr = (r as usize) * cell_cols + c as usize;
                    if (cell_inside2[addr - 1] && cell_inside2[addr + 1])
                        || (cell_inside2[addr - cell_cols] && cell_inside2[addr + cell_cols])
                    {
                        cell_inside[addr] = true;
                    }
                    c += 1;
                }
                r += 1;
            }
        }

        // 1) Collect the lines.
        let mut lines: Vec<Line> = Vec::new();
        // std::unordered_multimap<Point, int, PointHash>: the chaining below does not depend on
        // the order of equal keys (the module's comment).
        let mut start_point_to_line_idx: BTreeMap<Point, Vec<i32>> = BTreeMap::new();
        for r in 0..=(self.rows as Coord) {
            for c in 0..=(self.cols as Coord) {
                let addr = ((r + 1) as usize) * cell_cols + c as usize + 1;
                let left = cell_inside[addr - 1];
                let top = cell_inside[addr - cell_cols];
                let current = cell_inside[addr];
                if left != current {
                    lines.push(if left {
                        Line::new(Point::new(c, r + 1), Point::new(c, r))
                    } else {
                        Line::new(Point::new(c, r), Point::new(c, r + 1))
                    });
                    let last = lines[lines.len() - 1];
                    start_point_to_line_idx
                        .entry(last.a)
                        .or_default()
                        .push(lines.len() as i32 - 1);
                }
                if top != current {
                    lines.push(if top {
                        Line::new(Point::new(c, r), Point::new(c + 1, r))
                    } else {
                        Line::new(Point::new(c + 1, r), Point::new(c, r))
                    });
                    let last = lines[lines.len() - 1];
                    start_point_to_line_idx
                        .entry(last.a)
                        .or_default()
                        .push(lines.len() as i32 - 1);
                }
            }
        }

        // 2) Chain the lines.
        let mut line_processed = vec![false; lines.len()];
        let mut out = Polygons::new();
        for i_candidate in 0..(lines.len() as i32) {
            if line_processed[i_candidate as usize] {
                continue;
            }
            let mut poly = Polygon::default();
            line_processed[i_candidate as usize] = true;
            poly.points.push(lines[i_candidate as usize].b);
            let mut i_line_current = i_candidate;
            'chain: loop {
                // There shall be at least one line continuing the current one.
                let line_range = start_point_to_line_idx
                    .get(&lines[i_line_current as usize].b)
                    .map_or(&[][..], Vec::as_slice);
                let mut i_next: i32 = -1;
                for &it in line_range {
                    if it == i_candidate {
                        // closing the loop.
                        break 'chain;
                    }
                    if line_processed[it as usize] {
                        continue;
                    }
                    if i_next == -1 {
                        i_next = it;
                    } else {
                        // This is a corner, where two lines meet exactly. Pick the line, which
                        // encloses a smallest angle with the current edge.
                        let line_current = &lines[i_line_current as usize];
                        let line_next = &lines[it as usize];
                        let v1 = line_current.b - line_current.a;
                        let v2 = line_next.b - line_next.a;
                        let cross =
                            v1.x.wrapping_mul(v2.y)
                                .wrapping_sub(v2.x.wrapping_mul(v1.y));
                        if cross > 0 {
                            // This has to be a convex right angle. There is no better next line.
                            i_next = it;
                            break;
                        }
                    }
                }
                line_processed[i_next as usize] = true;
                i_line_current = i_next;
                poly.points.push(lines[i_line_current as usize].b);
            }
            out.push(poly);
        }

        // 3) Scale the polygons back into world, shrink slightly and remove collinear points.
        for poly in &mut out {
            let resolution = self.res();
            for p in &mut poly.points {
                p.x = p.x.wrapping_mul(resolution);
                p.y = p.y.wrapping_mul(resolution);
                p.x = p.x.wrapping_add(self.bbox.min.x);
                p.y = p.y.wrapping_add(self.bbox.min.y);
            }
            // Shrink the contour slightly, so if the same contour gets discretized and simplified
            // again, one will get the same result. Remove collinear points.
            let n = poly.points.len();
            let mut pts = Points::with_capacity(n);
            for j in 0..n {
                let j0 = if j == 0 { n - 1 } else { j - 1 };
                let j2 = if j + 1 == n { 0 } else { j + 1 };
                let v = poly.points[j2] - poly.points[j0];
                if v.x != 0 && v.y != 0 {
                    // This is a corner point. Copy it to the output contour.
                    let mut p = poly.points[j];
                    p.y = p.y.wrapping_add(if v.x < 0 {
                        offset.wrapping_neg()
                    } else {
                        offset
                    });
                    p.x = p.x.wrapping_add(if v.y > 0 {
                        offset.wrapping_neg()
                    } else {
                        offset
                    });
                    pts.push(p);
                }
            }
            poly.points = pts;
        }
        out
    }

    /// `intersecting_edges()`: each pair of segments of one cell that cross or touch, other than
    /// two neighbours of one contour, sorted and without repeats. The pair keeps the first
    /// segment's contour first: the C++'s `jfirst` picks between two equal expressions.
    #[must_use]
    pub fn intersecting_edges(&self) -> Vec<(ContourEdge, ContourEdge)> {
        let mut out = Vec::new();
        // For each cell:
        for r in 0..(self.rows as i32) {
            for c in 0..(self.cols as i32) {
                let cell = self.cells[cell_index(Coord::from(r), Coord::from(c), self.cols)];
                // For each pair of segments in the cell:
                for i in cell.begin..cell.end {
                    let ic = self.cell_data[i].0;
                    let icontour = &self.contours[ic];
                    let ipt = self.cell_data[i].1;
                    // End points of the line segment and their vector.
                    let ip1 = icontour.segment_start(ipt);
                    let ip2 = icontour.segment_end(ipt);
                    for j in i + 1..cell.end {
                        let jc = self.cell_data[j].0;
                        let jcontour = &self.contours[jc];
                        let jpt = self.cell_data[j].1;
                        // End points of the line segment and their vector.
                        let jp1 = jcontour.segment_start(jpt);
                        let jp2 = jcontour.segment_end(jpt);
                        if ic == jc
                            && (ipt == jcontour.segment_end_index(jpt)
                                || jpt == icontour.segment_end_index(ipt))
                        {
                            // Segments of the same contour share a common vertex.
                            continue;
                        }
                        if segments_intersect(ip1, ip2, jp1, jp2) {
                            // The two segments intersect. Add them to the output.
                            out.push(((ic, ipt), (jc, jpt)));
                        }
                    }
                }
            }
        }
        sort_remove_duplicates(&mut out);
        out
    }

    /// `has_intersecting_edges()`: whether `intersecting_edges()` would find a pair.
    #[must_use]
    pub fn has_intersecting_edges(&self) -> bool {
        // For each cell:
        for r in 0..(self.rows as i32) {
            for c in 0..(self.cols as i32) {
                let cell = self.cells[cell_index(Coord::from(r), Coord::from(c), self.cols)];
                // For each pair of segments in the cell:
                for i in cell.begin..cell.end {
                    let ic = self.cell_data[i].0;
                    let icontour = &self.contours[ic];
                    let ipt = self.cell_data[i].1;
                    let ip1 = icontour.segment_start(ipt);
                    let ip2 = icontour.segment_end(ipt);
                    for j in i + 1..cell.end {
                        let jc = self.cell_data[j].0;
                        let jcontour = &self.contours[jc];
                        let jpt = self.cell_data[j].1;
                        let jp1 = jcontour.segment_start(jpt);
                        let jp2 = jcontour.segment_end(jpt);
                        if !(ic == jc
                            && (ipt == jcontour.segment_end_index(jpt)
                                || jpt == icontour.segment_end_index(ipt)))
                            && segments_intersect(ip1, ip2, jp1, jp2)
                        {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// `visit_cells_intersecting_line(p1, p2, visitor)`: the cells the segment crosses, from the
    /// cell of `p1` to that of `p2`, each given to `visitor` as (row, column) until it returns
    /// false. Both ends must lie in the grid's cells (a stop otherwise).
    pub fn visit_cells_intersecting_line<V: FnMut(Coord, Coord) -> bool>(
        &self,
        p1: Point,
        p2: Point,
        visitor: &mut V,
    ) {
        let resolution = self.res();
        visit_line(self.dims(resolution), p1, p2, visitor);
    }

    /// `visit_cells_intersecting_box(bbox, visitor)`: the cells of the box (its maximum less one,
    /// each corner divided by the resolution toward zero, then trimmed to the grid), row by row,
    /// until `visitor` returns false.
    pub fn visit_cells_intersecting_box<V: FnMut(Coord, Coord) -> bool>(
        &self,
        mut bbox: BoundingBox,
        visitor: &mut V,
    ) {
        // End points of the line segment.
        bbox.min -= self.bbox.min;
        bbox.max -= self.bbox.min + Point::new(1, 1);
        // Get the cells of the end points.
        let resolution = self.res();
        bbox.min = bbox.min / resolution;
        bbox.max = bbox.max / resolution;
        // Trim with the cells.
        bbox.min.x = orca_stl::max(bbox.min.x, 0);
        bbox.min.y = orca_stl::max(bbox.min.y, 0);
        bbox.max.x = orca_stl::min(bbox.max.x, (self.cols as Coord).wrapping_sub(1));
        bbox.max.y = orca_stl::min(bbox.max.y, (self.rows as Coord).wrapping_sub(1));
        let mut iy = bbox.min.y;
        while iy <= bbox.max.y {
            let mut ix = bbox.min.x;
            while ix <= bbox.max.x {
                if !visitor(iy, ix) {
                    return;
                }
                ix += 1;
            }
            iy += 1;
        }
    }

    /// `cell_data_range(row, col)`: the contour and segment pairs of a cell.
    #[must_use]
    pub fn cell_data_range(&self, row: Coord, col: Coord) -> &[(usize, usize)] {
        let cell = &self.cells[cell_index(row, col, self.cols)];
        &self.cell_data[cell.begin..cell.end]
    }

    /// `segment(contour_and_segment_idx)`: its start and end points.
    #[must_use]
    pub fn segment(&self, contour_and_segment_idx: &(usize, usize)) -> (Point, Point) {
        let contour = &self.contours[contour_and_segment_idx.0];
        let iseg = contour_and_segment_idx.1;
        (contour.segment_start(iseg), contour.segment_end(iseg))
    }

    /// `line(contour_and_segment_idx)`.
    #[must_use]
    pub fn line(&self, contour_and_segment_idx: &(usize, usize)) -> Line {
        let contour = &self.contours[contour_and_segment_idx.0];
        let iseg = contour_and_segment_idx.1;
        Line::new(contour.segment_start(iseg), contour.segment_end(iseg))
    }
}

/// The body of `visit_cells_intersecting_line` over the grid's box minimum, resolution and size,
/// so that `create` can walk while it fills the cells.
fn visit_line<V: FnMut(Coord, Coord) -> bool>(
    d: Dims,
    mut p1: Point,
    mut p2: Point,
    visitor: &mut V,
) {
    let resolution = d.resolution;
    // End points of the line segment.
    p1 -= d.min;
    p2 -= d.min;
    d.check_inside(p1);
    d.check_inside(p2);
    // Get the cells of the end points.
    let mut ix = p1.x / resolution;
    let mut iy = p1.y / resolution;
    let ixb = p2.x / resolution;
    let iyb = p2.y / resolution;
    // Account for the end points.
    if !visitor(iy, ix) || (ix == ixb && iy == iyb) {
        // Both ends fall into the same cell.
        return;
    }
    let cols = d.cols as i64;
    let rows = d.rows as i64;
    let outside = |ix: Coord, iy: Coord| ix < 0 || iy < 0 || ix >= cols || iy >= rows;
    // Raster the centeral part of the line.
    let dx = p2.x.wrapping_sub(p1.x).wrapping_abs();
    let dy = p2.y.wrapping_sub(p1.y).wrapping_abs();
    if p1.x < p2.x {
        let mut ex = mul(sub(mul(ix + 1, resolution), p1.x), dy);
        if p1.y < p2.y {
            // x positive, y positive
            let mut ey = mul(sub(mul(iy + 1, resolution), p1.y), dx);
            loop {
                if ex < ey {
                    ey = sub(ey, ex);
                    ex = mul(dy, resolution);
                    ix += 1;
                } else if ex == ey {
                    ex = mul(dy, resolution);
                    ey = mul(dx, resolution);
                    ix += 1;
                    iy += 1;
                } else {
                    ex = sub(ex, ey);
                    ey = mul(dx, resolution);
                    iy += 1;
                }
                if outside(ix, iy) || !visitor(iy, ix) {
                    return;
                }
                if ix == ixb && iy == iyb {
                    break;
                }
            }
        } else {
            // x positive, y non positive
            let mut ey = mul(sub(p1.y, mul(iy, resolution)), dx);
            loop {
                if ex <= ey {
                    ey = sub(ey, ex);
                    ex = mul(dy, resolution);
                    ix += 1;
                } else {
                    ex = sub(ex, ey);
                    ey = mul(dx, resolution);
                    iy -= 1;
                }
                if outside(ix, iy) || !visitor(iy, ix) {
                    return;
                }
                if ix == ixb && iy == iyb {
                    break;
                }
            }
        }
    } else {
        let mut ex = mul(sub(p1.x, mul(ix, resolution)), dy);
        if p1.y < p2.y {
            // x non positive, y positive
            let mut ey = mul(sub(mul(iy + 1, resolution), p1.y), dx);
            loop {
                if ex < ey {
                    ey = sub(ey, ex);
                    ex = mul(dy, resolution);
                    ix -= 1;
                } else {
                    ex = sub(ex, ey);
                    ey = mul(dx, resolution);
                    iy += 1;
                }
                if outside(ix, iy) || !visitor(iy, ix) {
                    return;
                }
                if ix == ixb && iy == iyb {
                    break;
                }
            }
        } else {
            // x non positive, y non positive
            let mut ey = mul(sub(p1.y, mul(iy, resolution)), dx);
            loop {
                if ex < ey {
                    ey = sub(ey, ex);
                    ex = mul(dy, resolution);
                    ix -= 1;
                } else if ex == ey {
                    // The lower edge of a grid cell belongs to the cell. Handle the case where the
                    // ray may cross the lower left corner of a cell in a general case, or a left or
                    // lower edge in a degenerate case (horizontal or vertical line).
                    if dx > 0 {
                        ex = mul(dy, resolution);
                        ix -= 1;
                    }
                    if dy > 0 {
                        ey = mul(dx, resolution);
                        iy -= 1;
                    }
                } else {
                    ex = sub(ex, ey);
                    ey = mul(dx, resolution);
                    iy -= 1;
                }
                if outside(ix, iy) || !visitor(iy, ix) {
                    return;
                }
                if ix == ixb && iy == iyb {
                    break;
                }
            }
        }
    }
}

/// `PropagateDanielssonSingleStep<INCX, INCY>`: one step of Danielsson's chamfer sweep from the
/// corner `addr_delta` away, offset by `(INCX, INCY)` cells.
struct PropagateDanielssonSingleStep<const INCX: i64, const INCY: i64> {
    stride: usize,
    resolution: Coord,
}

impl<const INCX: i64, const INCY: i64> PropagateDanielssonSingleStep<INCX, INCY> {
    /// `operator()(r, c, addr_delta)`: a corner that is not a seed takes its neighbour's vector
    /// plus the offset where that is shorter, in `float`.
    fn step(&self, l: &mut [f32], signs: &[u8], r: i32, c: i32, addr_delta: i32) {
        let addr = (r as usize)
            .wrapping_mul(self.stride)
            .wrapping_add(c as usize);
        if signs[addr] & 2 == 0 {
            let v = addr << 1;
            let len = l[v] * l[v] + l[v + 1] * l[v + 1];
            let v2s = v.wrapping_add_signed((addr_delta as isize) << 1);
            let v2 = [
                l[v2s] + (INCX * self.resolution) as f32,
                l[v2s + 1] + (INCY * self.resolution) as f32,
            ];
            let l2 = v2[0] * v2[0] + v2[1] * v2[1];
            if l2 < len {
                l[v] = v2[0];
                l[v + 1] = v2[1];
            }
        }
    }
}

/// `Slic3r::intersecting_edges(polygons)`: the crossing segments of the polygons, on a grid of
/// the polygons' box widened by 20 whose resolution is their mean edge length (over the polygons
/// of two points or more). A pair's contour is the index of the polygon among those that are not
/// empty (the C++'s pointer into its own grid, freed when it returns).
#[must_use]
pub fn intersecting_edges(polygons: &[Polygon]) -> Vec<(ContourEdge, ContourEdge)> {
    let mut len = 0.0_f64;
    let mut cnt: usize = 0;
    let mut bbox = BoundingBox::new();
    for poly in polygons {
        if poly.points.len() < 2 {
            continue;
        }
        for i in 0..poly.points.len() {
            bbox.merge(poly.points[i]);
            let j = if i == 0 { poly.points.len() - 1 } else { i - 1 };
            len += (poly.points[j] - poly.points[i]).cast_f64().norm();
            cnt += 1;
        }
    }

    let mut out = Vec::new();
    if cnt > 0 {
        len /= cnt as f64;
        bbox.offset(20.0);
        let mut grid = Grid::new();
        grid.set_bbox(bbox);
        // create(const Polygons &, coord_t) takes the double as a coord_t.
        let resolution = orca_crt::f64_to_i64(len);
        if resolution == 0 {
            not_ported(INTERSECTING_EDGES, MEAN_EDGE_BELOW_ONE);
        }
        grid.create_polygons(polygons, resolution);
        out = grid.intersecting_edges();
    }
    out
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::*;
    use crate::NotPorted;

    fn square() -> Vec<Polygon> {
        vec![Polygon::new(vec![
            Point::new(0, 0),
            Point::new(1000, 0),
            Point::new(1000, 1000),
            Point::new(0, 1000),
        ])]
    }

    fn stop(f: impl FnOnce()) -> Option<NotPorted> {
        match catch_unwind(AssertUnwindSafe(f)) {
            Ok(()) => None,
            Err(payload) => payload.downcast::<NotPorted>().ok().map(|b| *b),
        }
    }

    /// A query whose products of coordinate differences pass 2^63 stops with the overflow row, as
    /// `create` does (the probe's IXNP example covers `create`).
    #[test]
    fn a_query_that_overflows_stops() {
        let polygons = square();
        let mut grid = Grid::new();
        grid.create_polygons(&polygons, 100);
        let far = Point::new(1 << 61, 0);
        let want = Some(NotPorted {
            function: OVERFLOW_FUNCTIONS,
            condition: OVERFLOW,
        });
        let got = stop(|| {
            let _ = grid.closest_point_signed_distance(far, 1 << 62);
        });
        assert_eq!(got, want);
        let mut d = 0.0;
        let got = stop(|| {
            let _ = grid.signed_distance_edges(far, 1 << 62, &mut d, None);
        });
        assert_eq!(got, want);
        // A point beyond the radius reads no segment.
        assert!(!grid.signed_distance_edges(far, 100, &mut d, None));
    }

    /// The pairs of the bow tie: the crossing of its first and third segments.
    #[test]
    fn a_bow_tie_crosses_itself() {
        let polygons = vec![Polygon::new(vec![
            Point::new(0, 0),
            Point::new(1000, 1000),
            Point::new(1000, 0),
            Point::new(0, 1000),
        ])];
        assert_eq!(intersecting_edges(&polygons), vec![((0, 0), (0, 2))]);
        let mut grid = Grid::new();
        grid.create_polygons(&polygons, 100);
        assert!(grid.has_intersecting_edges());
    }
}
