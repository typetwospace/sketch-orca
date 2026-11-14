// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Arachne/utils/PolylineStitcher.hpp lines 1-242 and
// PolylineStitcher.cpp lines 1-51 whole, of OrcaSlicer v2.4.2, translated on 2026-11-14 by
// arachne-2. Copyright Ultimaker B.V. (2022, CuraEngine), with the PrusaSlicer authors' (Prusa
// Research) and the OrcaSlicer authors' changes, under the GNU Affero General Public License,
// version 3 (AGPL-3.0; LICENSE).

//! `PolylineStitcher<Paths, Path, Junction>`: stitches polylines into longer polylines, or into
//! polygons where they close.
//!
//! [`stitch`] is `PolylineStitcher<VariableWidthLines, ExtrusionLine, ExtrusionJunction>::stitch`,
//! the instance WallToolPaths uses. It puts both ends of every line into a
//! [`SparsePointGrid`] of path indexes (orca-voronoi's `PathsPointIndex`, its cell the maximum
//! stitch distance, room for two ends per line), then takes the lines in order: each one not yet
//! used starts a chain, which grows from its last junction by the nearest end the grid hands over
//! within the stitch distance (the first under the snap distance ends the search; an end that
//! would close the chain counts 10 microns more when the chain is not odd, and 10 microns less
//! when it is, and may close only a chain 3 stitch distances long with more than 2 junctions); a
//! line is joined from its start or, reversed, from its end, without its first junction when the
//! gap is under the snap distance. When the forward direction finds nothing more, the chain is
//! reversed and grows from its other end. A closed chain goes to the polygons, in its first
//! direction; any other to the lines, turned back to the first line's direction when that line
//! may not be reversed. The grid's order is MSVC's `unordered_multimap`'s
//! ([`crate::unordered_multimap`]), so the nearest of equal distances is the one the C++ takes.
//!
//! [`can_reverse`], [`can_connect`] and [`is_odd`] are the instance's three specialisations: only
//! an odd line may be reversed, only lines of the same oddness connect, and a line is odd as its
//! flag says. `ExtrusionLine` implements orca-voronoi's `PathPoints` here, so that a
//! `PathsPointIndex<'_, ExtrusionLine>` is the C++'s `PathsPointIndex<VariableWidthLines>`.
//!
//! Stops ([`crate::stop`]): `PolylineStitcher<Polygons, Polygon, Point>` (`stitch-polygons`), which
//! domain A never uses: [`stitch_polygons`]; a line with no junction, whose first and last junction
//! the C++ reads outside the vector (`stitch-empty-line`).

use orca_geom::point::{Coord, Point, scaled};
use orca_geom::polygon::Polygons;
use orca_voronoi::{PathPoints, PathsPointIndex, PathsPointIndexLocator};

use crate::extrusion_junction::make_point;
use crate::extrusion_line::{ExtrusionLine, VariableWidthLines, norm_i64};
use crate::sparse_point_grid::SparsePointGrid;
use crate::stop::{STITCH_EMPTY_LINE, STITCH_POLYGONS, stop};

/// `make_point((*polygons)[poly_idx][point_idx])` over an `ExtrusionLine`: its junctions.
impl PathPoints for ExtrusionLine {
    fn size(&self) -> usize {
        ExtrusionLine::size(self)
    }
    fn point(&self, point_idx: usize) -> Point {
        make_point(&self[point_idx])
    }
}

/// `scaled<coord_t>(0.1)`: `stitch`'s default maximum stitch distance.
#[must_use]
pub fn default_max_stitch_distance() -> Coord {
    scaled(0.1)
}

/// `scaled<coord_t>(0.01)`: `stitch`'s default snap distance.
#[must_use]
pub fn default_snap_distance() -> Coord {
    scaled(0.01)
}

/// `canReverse(polyline)` of the `VariableWidthLines` instance: whether the line the index points
/// into is odd. Panics for the empty index (the C++ follows the null pointer).
#[must_use]
pub fn can_reverse(ppi: &PathsPointIndex<'_, ExtrusionLine>) -> bool {
    ppi.get_polygon().is_odd
}

/// `canConnect(a, b)` of the `VariableWidthLines` instance: `a.is_odd == b.is_odd`.
#[must_use]
pub fn can_connect(a: &ExtrusionLine, b: &ExtrusionLine) -> bool {
    a.is_odd == b.is_odd
}

/// `isOdd(line)` of the `VariableWidthLines` instance: `line.is_odd`.
#[must_use]
pub const fn is_odd(line: &ExtrusionLine) -> bool {
    line.is_odd
}

/// `PolylineStitcher<Polygons, Polygon, Point>::stitch`, which domain A never calls: it stops
/// (`stitch-polygons`).
pub fn stitch_polygons(
    _lines: &Polygons,
    _result_lines: &mut Polygons,
    _result_polygons: &mut Polygons,
    _max_stitch_distance: Coord,
    _snap_distance: Coord,
) -> ! {
    stop(
        "PolylineStitcher<Polygons, Polygon, Point>::stitch",
        STITCH_POLYGONS,
    )
}

/// The index of `lines[line_idx][point_idx]` in `unsigned int`, as `PathsPointIndex`'s
/// constructor takes its arguments.
#[allow(clippy::cast_possible_truncation)]
fn ppi(
    lines: &VariableWidthLines,
    line_idx: usize,
    point_idx: usize,
) -> PathsPointIndex<'_, ExtrusionLine> {
    PathsPointIndex::new(lines, line_idx as u32, point_idx as u32)
}

/// `stitch(lines, result_lines, result_polygons, max_stitch_distance, snap_distance)`: the lines
/// stitched, the closed chains appended to `result_polygons` and the others to `result_lines`
/// (both may hold paths already; `lines` must not be either of them).
#[allow(clippy::too_many_lines)]
pub fn stitch(
    lines: &VariableWidthLines,
    result_lines: &mut VariableWidthLines,
    result_polygons: &mut VariableWidthLines,
    max_stitch_distance: Coord,
    snap_distance: Coord,
) {
    if lines.is_empty() {
        return;
    }

    let mut grid: SparsePointGrid<PathsPointIndex<'_, ExtrusionLine>, PathsPointIndexLocator> =
        SparsePointGrid::new(
            max_stitch_distance,
            lines.len() * 2,
            1.0,
            PathsPointIndexLocator,
        );

    // populate grid
    for (line_idx, line) in lines.iter().enumerate() {
        if line.empty() {
            // `line.size() - 1` and the reads of junction 0 fall outside the vector.
            stop("PolylineStitcher::stitch", STITCH_EMPTY_LINE);
        }
        grid.insert(ppi(lines, line_idx, 0));
        grid.insert(ppi(lines, line_idx, line.size() - 1));
    }

    let mut processed = vec![false; lines.len()];

    for line_idx in 0..lines.len() {
        if processed[line_idx] {
            continue;
        }
        processed[line_idx] = true;
        let line = &lines[line_idx];
        let mut should_close = is_odd(line);

        let mut chain = line.clone();
        let mut closest_is_closing_polygon = false;
        // first go in the unreversed direction, to try to prevent the chain.reverse() operation.
        for go_in_reverse_direction in [false, true] {
            // NOTE: Implementation only works for this order; we currently only re-reverse the
            // chain when it's closed.
            if go_in_reverse_direction {
                // try extending chain in the other direction
                chain.reverse();
            }
            let mut chain_length: i64 = chain.polyline_length();

            loop {
                let from = make_point(chain.back());

                let mut closest: PathsPointIndex<'_, ExtrusionLine> = PathsPointIndex::default();
                let mut closest_distance: Coord = Coord::MAX;
                grid.process_nearby(from, max_stitch_distance, |nearby| {
                    let mut is_closing_segment = false;
                    let mut dist: Coord = norm_i64(nearby.p() - from);
                    if dist > max_stitch_distance {
                        return true; // keep looking
                    }
                    if (nearby.p() - make_point(chain.front())).squared_norm()
                        < snap_distance.wrapping_mul(snap_distance)
                    {
                        // prevent closing of small poly, cause it might be able to continue making
                        // a larger polyline; don't make 2 vert polygons
                        if chain_length.wrapping_add(dist) < max_stitch_distance.wrapping_mul(3)
                            || chain.size() <= 2
                        {
                            return true; // look for a better next line
                        }
                        is_closing_segment = true;
                        if should_close {
                            // Prefer closing the polygon if it's 100% even lines. Used to create
                            // closed contours. Continue to see if closing segment is also the
                            // closest.
                            dist = dist.wrapping_sub(scaled(0.01));
                        } else {
                            // prefer continuing polyline over closing a polygon; avoids closed
                            // zigzags from being printed separately; continue to see if closing
                            // segment is also the closest; there might be a segment smaller than
                            // [max_stitch_distance] which closes the polygon better
                            dist = dist.wrapping_add(scaled(0.01));
                        }
                    } else if processed[nearby.poly_idx as usize] {
                        // it was already moved to output
                        return true; // keep looking for a connection
                    }
                    // flip nearby_would_be_reversed when searching in the reverse direction
                    let nearby_would_be_reversed =
                        (nearby.point_idx != 0) != go_in_reverse_direction;
                    if !can_reverse(nearby) && nearby_would_be_reversed {
                        // connecting the segment would reverse the polygon direction
                        return true; // keep looking for a connection
                    }
                    if !can_connect(&chain, nearby.get_polygon()) {
                        return true; // keep looking for a connection
                    }
                    if dist < closest_distance {
                        closest_distance = dist;
                        closest = *nearby;
                        closest_is_closing_polygon = is_closing_segment;
                    }
                    if dist < snap_distance {
                        // we have found a good enough next line
                        return false; // stop looking for alternatives
                    }
                    true // keep processing elements
                });

                if !closest.initialized() // we couldn't find any next line
                    || closest_is_closing_polygon
                // we closed the polygon
                {
                    break;
                }

                let segment_dist: Coord = norm_i64(make_point(chain.back()) - closest.p());
                // The C++ asserts segment_dist <= max_stitch_distance + 10 microns.
                let old_size = chain.size();
                let other = closest.get_polygon();
                let skip = usize::from(segment_dist < snap_distance);
                let at = chain.size();
                if closest.point_idx == 0 {
                    chain.insert_range(at, other.junctions.iter().skip(skip).copied());
                } else {
                    chain.insert_range(at, other.junctions.iter().rev().skip(skip).copied());
                }
                // Update chain length.
                for i in old_size..chain.size() {
                    chain_length = chain_length
                        .wrapping_add(norm_i64(make_point(&chain[i]) - make_point(&chain[i - 1])));
                }
                // If we connect an even to an odd line, we should no longer try to close it.
                should_close &= !is_odd(other);
                // The C++ asserts that the line was not processed yet.
                processed[closest.poly_idx as usize] = true;
            }
            if closest_is_closing_polygon {
                if go_in_reverse_direction {
                    // re-reverse chain to retain original direction
                    // NOTE: not sure if this code could ever be reached, since if a polygon can be
                    // closed that should be already possible in the forward direction
                    chain.reverse();
                }

                break; // don't consider reverse direction
            }
        }
        if closest_is_closing_polygon {
            result_polygons.push(chain);
        } else {
            let ppi_here = ppi(lines, line_idx, 0);
            if !can_reverse(&ppi_here) {
                // Since closest_is_closing_polygon is false we went through the second iterations
                // of the for-loop, where go_in_reverse_direction is true; the polyline isn't
                // allowed to be reversed, so we re-reverse it.
                chain.reverse();
            }
            result_lines.push(chain);
        }
    }
}
