// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Arachne/utils/ExtrusionLine.hpp lines 1-286 and 293-294
// and ExtrusionLine.cpp lines 1-288, of OrcaSlicer v2.4.2, translated on 2026-11-13 by arachne-1.
// Copyright Ultimaker B.V. (2020, CuraEngine), with the PrusaSlicer authors' (Prusa Research) and
// the OrcaSlicer authors' changes, under the GNU Affero General Public License, version 3
// (AGPL-3.0; LICENSE).

//! `ExtrusionLine`: a polyline extruded with a variable width, a sequence of
//! [`ExtrusionJunction`]s with the inset it belongs to, whether it is an odd middle line (no
//! companion line on the other side) and whether it is closed. `VariableWidthLines` are the lines
//! Arachne makes.
//!
//! - `getLength` sums the segments' lengths, each `(b - a).cast<int64_t>().norm()`: Eigen's
//!   `norm` of an integer vector is `std::sqrt` of its `int64_t` squared norm, as a `double`,
//!   truncated back to `int64_t` ([`norm_i64`]). A closed line adds the closing segment.
//! - `simplify` removes junctions that make segments shorter than the smallest segment unless
//!   that moves the path more than the allowed distance, removes those that deviate less than 5
//!   microns where the extrusion area changes little, and moves a junction to the intersection of
//!   the lines before and after it when the next segment is long. It keeps the C++'s order of
//!   operations: the shoelace sums in wrapping `int64_t`, `height_2` as `int64_t(double * double /
//!   double)`, the 5-micron limits as `Slic3r::sqr(scaled<coord_t>(0.005))` and
//!   `scaled<double>(0.005)`, and `dist_greater`'s compares of `uint64_t` coordinates with the
//!   `int64_t` threshold converted to `uint64_t`. `current` and `next` are copies here, where the
//!   C++ holds references into the junctions; nothing writes through them while they are read.
//! - `calculateExtrusionAreaDeviationError` is the area a weighted average width loses when the
//!   middle junction of three goes.
//! - `is_contour` is a closed line oriented clockwise (Arachne makes contours clockwise, holes
//!   counter-clockwise), by Clipper's orientation; `area` is the shoelace area in `double`.
//! - `to_thick_polyline` (a template of the C++, for these lines and for ClipperLib_Z's paths,
//!   whose `z` is the width; [`ThickPathPoint`]), `to_polygon` and `to_points` convert a line.
//!   `extrusion_paths_append` (`ExtrusionLine.hpp` 287-292, `ExtrusionLine.cpp` 289-303), which
//!   builds orca-layer's extrusion paths from these lines, is the piece walls-1's in orca-perimeter;
//!   the `#if 0` block of the header (`get_extents` and the `to_points` of line pointers) is not
//!   compiled and not carried.
//!
//! Stops ([`crate::stop`]): `area` of an open line, `to_thick_polyline` of fewer than two points
//! and `to_polygon` of fewer than three junctions assert in the C++ (`line-shape`; the release
//! build reads outside the vector for the last two); `calculateExtrusionAreaDeviationError` of
//! junctions at one point whose widths differ by more than 1 divides by zero
//! (`area-error-no-length`).

use core::ops::{Index, IndexMut};

use orca_geom::clipper_z;
use orca_geom::line::Line;
use orca_geom::point::{Coord, Point, Points, cross2, scaled, scaled_f64};
use orca_geom::polygon::Polygon;
use orca_geom::polyline::ThickPolyline;

use crate::extrusion_junction::ExtrusionJunction;
use crate::stop::{AREA_ERROR_NO_LENGTH, LINE_SHAPE, stop};

/// `(v).cast<int64_t>().norm()` of an integer vector: `int64_t(std::sqrt(double(v.squaredNorm())))`,
/// the squared norm in wrapping `int64_t`, the root truncated.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn norm_i64(v: Point) -> i64 {
    orca_crt::f64_to_i64(orca_crt::sqrt(v.squared_norm() as f64))
}

/// `Slic3r::sqr(scaled<coord_t>(0.005))`: 5 microns, squared, in `coord_t`.
fn five_microns_squared() -> i64 {
    let five = scaled(0.005);
    five.wrapping_mul(five)
}

/// `ExtrusionLine`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtrusionLine {
    /// `inset_idx`: the inset the path is, counted from the outside inward (0 the outer wall).
    pub inset_idx: usize,
    /// `is_odd`: a middle line of an odd number of walls, with no companion line going back on
    /// the other side, and not a loop.
    pub is_odd: bool,
    /// `is_closed`: a closed polygonal path.
    pub is_closed: bool,
    /// `junctions`: the vertices the path runs along, each with its width.
    pub junctions: Vec<ExtrusionJunction>,
}

/// `ExtrusionLine()`: inset `-1` (the largest `size_t`), odd, open.
impl Default for ExtrusionLine {
    fn default() -> Self {
        Self {
            inset_idx: usize::MAX,
            is_odd: true,
            is_closed: false,
            junctions: Vec::new(),
        }
    }
}

/// `operator[](unsigned int index)`: the index is truncated to `unsigned int`, as the C++
/// parameter takes it.
impl Index<usize> for ExtrusionLine {
    type Output = ExtrusionJunction;

    #[allow(clippy::cast_possible_truncation)]
    fn index(&self, index: usize) -> &ExtrusionJunction {
        &self.junctions[index as u32 as usize]
    }
}

impl IndexMut<usize> for ExtrusionLine {
    #[allow(clippy::cast_possible_truncation)]
    fn index_mut(&mut self, index: usize) -> &mut ExtrusionJunction {
        &mut self.junctions[index as u32 as usize]
    }
}

impl ExtrusionLine {
    /// `ExtrusionLine(inset_idx, is_odd)`: open, no junction.
    #[must_use]
    pub const fn new(inset_idx: usize, is_odd: bool) -> Self {
        Self {
            inset_idx,
            is_odd,
            is_closed: false,
            junctions: Vec::new(),
        }
    }

    /// `size()`: the number of junctions.
    #[must_use]
    pub fn size(&self) -> usize {
        self.junctions.len()
    }

    /// `empty()`.
    #[must_use]
    pub fn empty(&self) -> bool {
        self.junctions.is_empty()
    }

    /// `front()`.
    #[must_use]
    pub fn front(&self) -> &ExtrusionJunction {
        &self.junctions[0]
    }

    /// `back()`.
    #[must_use]
    pub fn back(&self) -> &ExtrusionJunction {
        &self.junctions[self.junctions.len() - 1]
    }

    /// `front()`, to change it.
    pub fn front_mut(&mut self) -> &mut ExtrusionJunction {
        &mut self.junctions[0]
    }

    /// `back()`, to change it.
    pub fn back_mut(&mut self) -> &mut ExtrusionJunction {
        let last = self.junctions.len() - 1;
        &mut self.junctions[last]
    }

    /// `emplace_back(...)`.
    pub fn emplace_back(&mut self, junction: ExtrusionJunction) {
        self.junctions.push(junction);
    }

    /// `remove(unsigned int index)`.
    #[allow(clippy::cast_possible_truncation)]
    pub fn remove(&mut self, index: usize) {
        self.junctions.remove(index as u32 as usize);
    }

    /// `insert(index, p)`.
    pub fn insert(&mut self, index: usize, p: ExtrusionJunction) {
        self.junctions.insert(index, p);
    }

    /// `insert(pos, first, last)`: the junctions inserted before `pos`; the index of the first
    /// inserted (the iterator the C++ returns).
    pub fn insert_range(
        &mut self,
        pos: usize,
        junctions: impl IntoIterator<Item = ExtrusionJunction>,
    ) -> usize {
        self.junctions.splice(pos..pos, junctions);
        pos
    }

    /// `clear()`.
    pub fn clear(&mut self) {
        self.junctions.clear();
    }

    /// `reverse()`.
    pub fn reverse(&mut self) {
        self.junctions.reverse();
    }

    /// `getLength()`: the segments' lengths summed, each [`norm_i64`], the closing segment too
    /// for a closed line.
    #[must_use]
    pub fn get_length(&self) -> i64 {
        if self.junctions.is_empty() {
            return 0;
        }
        let mut len: i64 = 0;
        let mut prev = self.junctions[0];
        for next in &self.junctions {
            len = len.wrapping_add(norm_i64(next.p - prev.p));
            prev = *next;
        }
        if self.is_closed {
            len = len.wrapping_add(norm_i64(self.front().p - self.back().p));
        }
        len
    }

    /// `polylineLength()`: [`ExtrusionLine::get_length`].
    #[must_use]
    pub fn polyline_length(&self) -> i64 {
        self.get_length()
    }

    /// `toPolygon()`: every junction's point; an open line's polygon is a polyline.
    #[must_use]
    pub fn to_polygon(&self) -> Polygon {
        Polygon::new(self.junctions.iter().map(|j| j.p).collect())
    }

    /// `simplify(smallest_line_segment_squared, allowed_error_distance_squared,
    /// maximum_extrusion_area_deviation)`: removes junctions that are too dense (the module's
    /// header says how), keeping the first and the last.
    #[allow(
        clippy::too_many_lines,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss
    )]
    pub fn simplify(
        &mut self,
        smallest_line_segment_squared: i64,
        allowed_error_distance_squared: i64,
        maximum_extrusion_area_deviation: i64,
    ) {
        let junctions = &self.junctions;
        let size = junctions.len();
        let min_path_size = if self.is_closed { 3 } else { 2 };
        if size <= min_path_size {
            return;
        }

        // A closed line's first and last points are (almost) the same, so neither is touched:
        // the simplification starts at index 1.
        let mut new_junctions: Vec<ExtrusionJunction> = Vec::new();
        // The first junction always stays.
        new_junctions.push(junctions[0]);

        let mut previous = junctions[0];
        // An open line's last junction cannot count at index 1; a closed line's first and last
        // are the same, so the one before the last is taken.
        let mut previous_previous = if self.is_closed {
            junctions[size - 2]
        } else {
            junctions[0]
        };

        // The area the simplification cuts off is accumulated with the shoelace formula (twice
        // the area of each segment's fan blade from the origin), the blade of the shortcut
        // subtracted, so the height of a representative triangle is the area over the base.
        let initial = junctions[1];
        let mut accumulated_area_removed = previous
            .p
            .x
            .wrapping_mul(initial.p.y)
            .wrapping_sub(previous.p.y.wrapping_mul(initial.p.x));

        let five2 = five_microns_squared();
        let end = size - usize::from(!self.is_closed);
        // A closed line processes its last point, which is the first one.
        for point_idx in 1..end {
            // The last point of a closed line is the first point of the new line, which may
            // have moved.
            let is_last = point_idx + 1 == size;
            let current = if is_last {
                new_junctions[0]
            } else {
                junctions[point_idx]
            };

            // A closed line keeps at least 3 junctions.
            if self.is_closed && new_junctions.len() + (size - point_idx) <= 3 {
                new_junctions.push(current);
                continue;
            }

            // Spill over at the end of a closed line, unless [next] would then be [previous].
            let spill_over = self.is_closed
                && point_idx + 2 >= size
                && point_idx + 2 - size < new_junctions.len();
            let next = if spill_over {
                new_junctions[point_idx + 2 - size]
            } else {
                junctions[point_idx + 1]
            };

            let removed_area_next = current
                .p
                .x
                .wrapping_mul(next.p.y)
                .wrapping_sub(current.p.y.wrapping_mul(next.p.x));
            // The area between the origin and the shortcut.
            let negative_area_closing = next
                .p
                .x
                .wrapping_mul(previous.p.y)
                .wrapping_sub(next.p.y.wrapping_mul(previous.p.x));
            accumulated_area_removed = accumulated_area_removed.wrapping_add(removed_area_next);

            let length2 = (current - previous).squared_norm();

            // A segment under 5 microns (OrcaSlicer compares the squares) always goes; its width
            // does not matter much.
            if length2 < five2 {
                continue;
            }

            // The shortcut's area polygon, closed.
            let area_removed_so_far = accumulated_area_removed.wrapping_add(negative_area_closing);
            let base_length_2 = (next - previous).squared_norm();

            // Two segments back and forth, with no area.
            if base_length_2 == 0 {
                continue;
            }
            // The triangle's height squared: h^2 = L^2 / b^2, L twice its area.
            let height_2 = orca_crt::f64_to_i64(
                area_removed_so_far as f64 * area_removed_so_far as f64 / base_length_2 as f64,
            );
            let extrusion_area_error =
                Self::calculate_extrusion_area_deviation_error(previous, current, next);
            // Almost exactly collinear (the distance check stops a small height made of positive
            // and negative areas cancelling), and the area of the shortcut's width not changed
            // too much.
            if (height_2 <= five2
                && Line::new(previous.p, next.p).distance_to_infinite(current.p)
                    <= scaled_f64(0.005))
                && extrusion_area_error <= maximum_extrusion_area_deviation
            {
                continue;
            }

            if length2 < smallest_line_segment_squared && height_2 <= allowed_error_distance_squared
            {
                let next_length2 = (current - next).squared_norm();
                if next_length2 > smallest_line_segment_squared.wrapping_mul(4) {
                    // The next line is long: removing this junction could leave an artifact, so
                    // it moves to the intersection of the line before and the line after (a
                    // sharper corner) and the previous junction goes, unless that point is an
                    // artifact itself.
                    // `!has_intersection || ... || ...` rejects it.
                    let accepted = Line::new(previous_previous.p, previous.p)
                        .intersection_infinite(&Line::new(current.p, next.p))
                        .filter(|&intersection_point| {
                            let rejected = Line::new(previous.p, current.p)
                                .distance_to_infinite_squared(intersection_point)
                                .0
                                > allowed_error_distance_squared as f64
                                // Too far from 'previous' and 'current' to replace 'current'.
                                || dist_greater(
                                    intersection_point,
                                    previous.p,
                                    smallest_line_segment_squared,
                                )
                                || dist_greater(
                                    intersection_point,
                                    current.p,
                                    smallest_line_segment_squared,
                                );
                            !rejected
                        });
                    if let Some(intersection_point) = accepted {
                        // The new point is valid.
                        let new_to_add = ExtrusionJunction {
                            p: intersection_point,
                            w: current.w,
                            perimeter_index: current.perimeter_index,
                        };
                        // The previous point added goes.
                        if !new_junctions.is_empty() {
                            new_junctions.pop();
                            previous = previous_previous;
                        }

                        // The junction is replaced by the new one.
                        accumulated_area_removed = removed_area_next;
                        previous_previous = previous;
                        previous = new_to_add;
                        new_junctions.push(new_to_add);
                        continue;
                    }
                    // No better spot, and the segment is over 5 microns: it stays.
                } else {
                    // Removing the junction adds little error.
                    continue;
                }
            }
            // The junction stays.
            accumulated_area_removed = removed_area_next;
            previous_previous = previous;
            previous = current;
            new_junctions.push(current);
        }

        if self.is_closed {
            // A closed line's first and last points are the same: the last was processed above.
            let last = new_junctions[new_junctions.len() - 1].p;
            new_junctions[0].p = last;
        } else {
            // The last junction always stays.
            new_junctions.push(junctions[size - 1]);
        }

        self.junctions = new_junctions;
    }

    /// `calculateExtrusionAreaDeviationError(A, B, C)`: the area error of the segments AB and BC
    /// when B goes and AC takes the length-weighted average width. Widths within 1 of each other
    /// take the width difference times the shorter segment.
    #[must_use]
    pub fn calculate_extrusion_area_deviation_error(
        a: ExtrusionJunction,
        b: ExtrusionJunction,
        c: ExtrusionJunction,
    ) -> i64 {
        let ab_length = norm_i64(b.p - a.p);
        let bc_length = norm_i64(c.p - b.p);
        let width_diff = orca_stl::max(
            b.w.wrapping_sub(a.w).wrapping_abs(),
            c.w.wrapping_sub(b.w).wrapping_abs(),
        );
        if width_diff > 1 {
            // The width changes only when the widths differ, or rounding may give a wrong average.
            let ab_weight = a.w.wrapping_add(b.w) / 2;
            let bc_weight = b.w.wrapping_add(c.w) / 2;
            let total = ab_length.wrapping_add(bc_length);
            if total == 0 {
                stop(
                    "ExtrusionLine::calculateExtrusionAreaDeviationError",
                    AREA_ERROR_NO_LENGTH,
                );
            }
            let weighted_average_width = ab_length
                .wrapping_mul(ab_weight)
                .wrapping_add(bc_length.wrapping_mul(bc_weight))
                .wrapping_div(total);
            let ac_length = norm_i64(c.p - a.p);
            ab_weight
                .wrapping_mul(ab_length)
                .wrapping_add(bc_weight.wrapping_mul(bc_length))
                .wrapping_sub(weighted_average_width.wrapping_mul(ac_length))
                .wrapping_abs()
        } else if ab_length > bc_length {
            // A small difference: the width of the longer segment.
            width_diff.wrapping_mul(bc_length)
        } else {
            width_diff.wrapping_mul(ab_length)
        }
    }

    /// `is_contour()`: a closed line oriented clockwise.
    #[must_use]
    pub fn is_contour(&self) -> bool {
        if !self.is_closed {
            return false;
        }
        // Arachne makes contours clockwise and holes counter-clockwise.
        Polygon::new(self.junctions.iter().map(|j| j.p).collect()).is_clockwise()
    }

    /// `area()` of a closed line: half the shoelace sum in `double`, 0 below three junctions.
    #[must_use]
    pub fn area(&self) -> f64 {
        if !self.is_closed {
            stop("ExtrusionLine::area", LINE_SHAPE);
        }
        let mut a = 0.0;
        if self.junctions.len() >= 3 {
            let mut p1 = self.back().p.cast_f64();
            for junction in &self.junctions {
                let p2 = junction.p.cast_f64();
                a += cross2(p1, p2);
                p1 = p2;
            }
        }
        0.5 * a
    }
}

/// A point of a path `to_thick_polyline` takes: `x()`, `y()` and `z()`, the width.
pub trait ThickPathPoint {
    /// `x()`.
    fn x(&self) -> Coord;
    /// `y()`.
    fn y(&self) -> Coord;
    /// `z()`: the width.
    fn z(&self) -> Coord;
}

impl ThickPathPoint for ExtrusionJunction {
    fn x(&self) -> Coord {
        self.p.x
    }
    fn y(&self) -> Coord {
        self.p.y
    }
    fn z(&self) -> Coord {
        self.w
    }
}

/// ClipperLib_Z's point, whose `z` carries the width (`clip_extrusion`).
impl ThickPathPoint for clipper_z::IntPoint {
    fn x(&self) -> Coord {
        self.x
    }
    fn y(&self) -> Coord {
        self.y
    }
    fn z(&self) -> Coord {
        self.z
    }
}

/// `to_thick_polyline(path)` of an [`ExtrusionLine`]'s junctions or a ClipperLib_Z path: the
/// points, and each segment's two widths (the `coord_t` widths as `double`s). The end flags stay
/// `(false, false)`.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn to_thick_polyline<P: ThickPathPoint>(path: &[P]) -> ThickPolyline {
    if path.len() < 2 {
        stop("Arachne::to_thick_polyline", LINE_SHAPE);
    }
    let mut out = ThickPolyline::default();
    out.points.push(Point::new(path[0].x(), path[0].y()));
    out.width.push(path[0].z() as f64);
    out.points.push(Point::new(path[1].x(), path[1].y()));
    out.width.push(path[1].z() as f64);

    let mut it_prev = &path[1];
    for it in &path[2..] {
        out.points.push(Point::new(it.x(), it.y()));
        out.width.push(it_prev.z() as f64);
        out.width.push(it.z() as f64);
        it_prev = it;
    }
    out
}

/// `to_polygon(line)`: a closed line's points without the last, which repeats the first.
#[must_use]
pub fn to_polygon(line: &ExtrusionLine) -> Polygon {
    if line.junctions.len() < 3 {
        stop("Arachne::to_polygon", LINE_SHAPE);
    }
    // The C++ asserts that the first and the last point are the same, which a release build does
    // not check.
    let n = line.junctions.len() - 1;
    let mut out = Polygon::default();
    out.points.reserve(n);
    for j in &line.junctions[..n] {
        out.points.push(j.p);
    }
    out
}

/// `to_points(extrusion_line)`: every junction's point.
#[must_use]
pub fn to_points(extrusion_line: &ExtrusionLine) -> Points {
    extrusion_line.junctions.iter().map(|j| j.p).collect()
}

/// `dist_greater` of `simplify`: whether a coordinate of `|p1 - p2|` (as `uint64_t`) passes the
/// threshold, else whether its squared norm (wrapping `uint64_t`) does, the threshold converted
/// to `uint64_t` as the comparison does.
#[allow(clippy::cast_sign_loss)]
fn dist_greater(p1: Point, p2: Point, threshold: i64) -> bool {
    let v = (p1 - p2).cwise_abs();
    let (x, y) = (v.x as u64, v.y as u64);
    let t = threshold as u64;
    if x > t || y > t {
        // Certainly greater: no squared norm, which could overflow.
        return true;
    }
    x.wrapping_mul(x).wrapping_add(y.wrapping_mul(y)) > t
}

/// `VariableWidthLines`: the lines Arachne makes.
pub type VariableWidthLines = Vec<ExtrusionLine>;

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::*;
    use crate::stop::NotPorted;

    fn j(x: Coord, y: Coord, w: Coord) -> ExtrusionJunction {
        ExtrusionJunction::new(Point::new(x, y), w, 0)
    }

    #[test]
    fn length_adds_the_closing_segment() {
        let mut l = ExtrusionLine::new(0, false);
        l.junctions = vec![j(0, 0, 1), j(3, 4, 1), j(3, 0, 1)];
        assert_eq!(l.get_length(), 9);
        l.is_closed = true;
        assert_eq!(l.get_length(), 12);
        assert_eq!(ExtrusionLine::default().inset_idx, usize::MAX);
    }

    #[test]
    fn a_short_line_is_not_simplified() {
        let mut l = ExtrusionLine::new(0, false);
        l.junctions = vec![j(0, 0, 1), j(1, 0, 1)];
        let before = l.clone();
        l.simplify(1, 1, 1);
        assert_eq!(l, before);
    }

    #[test]
    fn line_shape_edges_stop() {
        let l = ExtrusionLine::new(0, false);
        let r = catch_unwind(AssertUnwindSafe(|| l.area()));
        let p = r.expect_err("area of an open line");
        assert_eq!(
            p.downcast_ref::<NotPorted>().map(|n| n.condition),
            Some(LINE_SHAPE)
        );
        let r = catch_unwind(AssertUnwindSafe(|| to_thick_polyline(&l.junctions)));
        assert!(r.is_err());
        let r = catch_unwind(AssertUnwindSafe(|| to_polygon(&l)));
        assert!(r.is_err());
        let r = catch_unwind(|| {
            ExtrusionLine::calculate_extrusion_area_deviation_error(
                j(0, 0, 1),
                j(0, 0, 5),
                j(0, 0, 9),
            )
        });
        let p = r.expect_err("no length");
        assert_eq!(
            p.downcast_ref::<NotPorted>().map(|n| n.condition),
            Some(AREA_ERROR_NO_LENGTH)
        );
    }

    #[test]
    fn thick_polyline_doubles_the_inner_widths() {
        let path = [j(0, 0, 10), j(5, 0, 20), j(9, 0, 30)];
        let t = to_thick_polyline(&path);
        assert_eq!(t.points.len(), 3);
        assert_eq!(t.width, vec![10.0, 20.0, 20.0, 30.0]);
        assert_eq!(t.endpoints, (false, false));
        let z = [
            clipper_z::IntPoint::new(0, 0, 7),
            clipper_z::IntPoint::new(1, 1, 8),
        ];
        assert_eq!(to_thick_polyline(&z).width, vec![7.0, 8.0]);
    }
}
