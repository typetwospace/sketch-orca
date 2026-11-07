// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: class ExPolygon and its helpers of src/libslic3r/ExPolygon.hpp and
// ExPolygon.cpp of OrcaSlicer v2.4.2, translated on 2026-11-07. Copyright the OrcaSlicer authors,
// the BambuStudio authors (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU
// Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! `ExPolygon`: a counter-clockwise contour with clockwise holes. The medial axis (`medial_axis`,
//! Voronoi based) comes with the perimeter generator, not here.

use crate::bounding_box::BoundingBox;
use crate::clipper_utils;
use crate::line::{Line, Lines};
use crate::multi_point;
use crate::point::{Coord, Point, Points};
use crate::polygon::{self, Polygon, Polygons};
use crate::polyline::{Polyline, Polylines};

/// `ExPolygon`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExPolygon {
    /// `contour`: counter-clockwise.
    pub contour: Polygon,
    /// `holes`: clockwise.
    pub holes: Polygons,
}

/// `ExPolygons`.
pub type ExPolygons = Vec<ExPolygon>;

impl ExPolygon {
    /// `ExPolygon(contour)`.
    #[must_use]
    pub fn new(contour: Polygon) -> Self {
        Self {
            contour,
            holes: Polygons::new(),
        }
    }

    /// `ExPolygon(Points contour)`.
    #[must_use]
    pub fn from_points(contour: Points) -> Self {
        Self::new(Polygon::new(contour))
    }

    /// `ExPolygon(contour, hole)`.
    #[must_use]
    pub fn with_hole(contour: Polygon, hole: Polygon) -> Self {
        Self {
            contour,
            holes: vec![hole],
        }
    }

    /// `clear()`.
    pub fn clear(&mut self) {
        self.contour.points.clear();
        self.holes.clear();
    }

    /// `empty()`: no contour points.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.contour.points.is_empty()
    }

    /// `num_contours()`.
    #[must_use]
    pub fn num_contours(&self) -> usize {
        self.holes.len() + 1
    }

    /// `contour_or_hole(idx)`.
    #[must_use]
    pub fn contour_or_hole(&self, idx: usize) -> &Polygon {
        if idx == 0 {
            &self.contour
        } else {
            &self.holes[idx - 1]
        }
    }

    /// `scale(factor)`.
    pub fn scale(&mut self, factor: f64) {
        self.contour.scale(factor);
        for hole in &mut self.holes {
            hole.scale(factor);
        }
    }

    /// `scale(factor_x, factor_y)`.
    pub fn scale_xy(&mut self, factor_x: f64, factor_y: f64) {
        self.contour.scale_xy(factor_x, factor_y);
        for hole in &mut self.holes {
            hole.scale_xy(factor_x, factor_y);
        }
    }

    /// `translate(x, y)`: by `Point(coord_t(x), coord_t(y))`, truncated.
    pub fn translate_f64(&mut self, x: f64, y: f64) {
        self.translate(Point::new(orca_crt::f64_to_i64(x), orca_crt::f64_to_i64(y)));
    }

    /// `translate(vector)`.
    pub fn translate(&mut self, p: Point) {
        self.contour.translate(p);
        for hole in &mut self.holes {
            hole.translate(p);
        }
    }

    /// `rotate(angle)`.
    pub fn rotate(&mut self, angle: f64) {
        self.contour.rotate(angle);
        for hole in &mut self.holes {
            hole.rotate(angle);
        }
    }

    /// `rotate(angle, center)`.
    pub fn rotate_around(&mut self, angle: f64, center: Point) {
        self.contour.rotate_around(angle, center);
        for hole in &mut self.holes {
            hole.rotate_around(angle, center);
        }
    }

    /// `area()`: the contour's area, each hole's taken as `a -= -hole.area()`.
    #[must_use]
    pub fn area(&self) -> f64 {
        let mut a = self.contour.area();
        for hole in &self.holes {
            a -= -hole.area();
        }
        a
    }

    /// `is_valid()`: a valid counter-clockwise contour, valid clockwise holes.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        if !self.contour.is_valid() || !self.contour.is_counter_clockwise() {
            return false;
        }
        self.holes
            .iter()
            .all(|h| h.is_valid() && !h.is_counter_clockwise())
    }

    /// `douglas_peucker(tolerance)`.
    pub fn douglas_peucker(&mut self, tolerance: f64) {
        self.contour.douglas_peucker(tolerance);
        for poly in &mut self.holes {
            poly.douglas_peucker(tolerance);
        }
    }

    /// `contains(line)`.
    #[must_use]
    pub fn contains_line(&self, line: &Line) -> bool {
        self.contains_polyline(&Polyline::from_two(line.a, line.b))
    }

    /// `contains(polyline)`: the boxes overlap and nothing of the polyline is left outside.
    ///
    /// The C++ computes `bbox2.inflated(1)` and drops the result, so the test uses the polyline's
    /// own box.
    #[must_use]
    pub fn contains_polyline(&self, polyline: &Polyline) -> bool {
        let bbox1 = get_extents(self);
        let bbox2 = crate::polyline::get_extents(polyline);
        if !bbox1.overlap(&bbox2) {
            return false;
        }
        clipper_utils::diff_pl_polyline_expolygon(polyline, self).is_empty()
    }

    /// `contains(polylines)`.
    #[must_use]
    pub fn contains_polylines(&self, polylines: &[Polyline]) -> bool {
        clipper_utils::diff_pl_polylines_expolygon(polylines, self).is_empty()
    }

    /// `contains(point, border_result)`: in the contour and in no hole, `border_result` on the
    /// boundary.
    #[must_use]
    pub fn contains(&self, point: Point, border_result: bool) -> bool {
        if !polygon::contains(&self.contour, point, border_result) {
            return false;
        }
        !self
            .holes
            .iter()
            .any(|hole| polygon::contains(hole, point, !border_result))
    }

    /// `on_boundary(point, eps)`.
    #[must_use]
    pub fn on_boundary(&self, point: Point, eps: f64) -> bool {
        self.contour.on_boundary(point, eps) || self.holes.iter().any(|h| h.on_boundary(point, eps))
    }

    /// `point_projection(point)`: the nearest outline point; with holes the nearest of each
    /// contour's projections by squared distance.
    #[must_use]
    pub fn point_projection(&self, point: Point) -> Point {
        if self.holes.is_empty() {
            return self.contour.point_projection(point);
        }
        let mut dist_min2 = f64::MAX;
        let mut closest_pt_min = Point::default();
        for i in 0..self.num_contours() {
            let closest_pt = self.contour_or_hole(i).point_projection(point);
            let d2 = (closest_pt - point).cast_f64().squared_norm();
            if d2 < dist_min2 {
                dist_min2 = d2;
                closest_pt_min = closest_pt;
            }
        }
        closest_pt_min
    }

    /// `symmetric_y(y_axis)`.
    pub fn symmetric_y(&mut self, y_axis: Coord) {
        multi_point::symmetric_y(&mut self.contour.points, y_axis);
        for hole in &mut self.holes {
            multi_point::symmetric_y(&mut hole.points, y_axis);
        }
    }

    /// `overlaps(other)`: `other`'s outline meets this one, or this lies inside `other`.
    #[must_use]
    pub fn overlaps(&self, other: &ExPolygon) -> bool {
        if self.is_empty() || other.is_empty() {
            return false;
        }
        let pl_out = clipper_utils::intersection_pl_polylines_expolygon(
            &polygon::to_polylines(&to_polygons(other)),
            self,
        );
        !pl_out.is_empty() || other.contains(self.contour.points[0], true)
    }

    /// `simplify_p(tolerance)`: each contour Douglas-Peucker simplified as a closed path, then
    /// `simplify_polygons`.
    #[must_use]
    pub fn simplify_p(&self, tolerance: f64) -> Polygons {
        let mut pp = Polygons::with_capacity(self.holes.len() + 1);
        for p in core::iter::once(&self.contour).chain(&self.holes) {
            let mut points = p.points.clone();
            points.push(points[0]);
            let mut points = multi_point::douglas_peucker(&points, tolerance);
            points.pop();
            pp.push(Polygon::new(points));
        }
        clipper_utils::simplify_polygons(&pp)
    }

    /// `simplify(tolerance)`: `union_ex` of `simplify_p`.
    #[must_use]
    pub fn simplify(&self, tolerance: f64) -> ExPolygons {
        clipper_utils::union_ex(
            &self.simplify_p(tolerance),
            crate::clipper::PolyFillType::NonZero,
        )
    }

    /// `lines()`: the contour's lines, then each hole's.
    #[must_use]
    pub fn lines(&self) -> Lines {
        let mut lines = self.contour.lines();
        for h in &self.holes {
            lines.extend(h.lines());
        }
        lines
    }
}

/// `to_polygons(expolygon)`: the contour, then the holes.
#[must_use]
pub fn to_polygons(src: &ExPolygon) -> Polygons {
    let mut polygons = Polygons::with_capacity(src.holes.len() + 1);
    polygons.push(src.contour.clone());
    polygons.extend(src.holes.iter().cloned());
    polygons
}

/// `to_polygons(expolygons)`.
#[must_use]
pub fn to_polygons_ex(src: &[ExPolygon]) -> Polygons {
    let mut polygons = Polygons::with_capacity(number_polygons(src));
    for e in src {
        polygons.push(e.contour.clone());
        polygons.extend(e.holes.iter().cloned());
    }
    polygons
}

/// `to_expolygons(polygons)`: each polygon a contour without holes.
#[must_use]
pub fn to_expolygons(polys: Polygons) -> ExPolygons {
    polys.into_iter().map(ExPolygon::new).collect()
}

/// `count_points(expolygon)`.
#[must_use]
pub fn count_points(expoly: &ExPolygon) -> usize {
    expoly.contour.points.len() + expoly.holes.iter().map(|h| h.points.len()).sum::<usize>()
}

/// `count_points(expolygons)`.
#[must_use]
pub fn count_points_ex(expolys: &[ExPolygon]) -> usize {
    expolys.iter().map(count_points).sum()
}

/// `number_polygons(expolygons)`.
#[must_use]
pub fn number_polygons(expolys: &[ExPolygon]) -> usize {
    expolys.iter().map(|e| e.holes.len() + 1).sum()
}

/// `to_lines(expolygon)`: each contour's edges, the closing one last.
///
/// Unlike `Polygon::lines()` it has no size check: an empty contour or hole is undefined in the
/// C++ (`end() - 1` before `begin()`); here it panics.
#[must_use]
pub fn to_lines(src: &ExPolygon) -> Lines {
    let mut lines = Lines::with_capacity(count_points(src));
    for i in 0..=src.holes.len() {
        let poly = src.contour_or_hole(i);
        let pts = &poly.points;
        for w in pts.windows(2) {
            lines.push(Line::new(w[0], w[1]));
        }
        lines.push(Line::new(pts[pts.len() - 1], pts[0]));
    }
    lines
}

/// `to_lines(expolygons)`.
#[must_use]
pub fn to_lines_ex(src: &[ExPolygon]) -> Lines {
    let mut lines = Lines::with_capacity(count_points_ex(src));
    for e in src {
        lines.extend(to_lines(e));
    }
    lines
}

/// `to_polylines(expolygon)`: each contour as a closed polyline.
#[must_use]
pub fn to_polylines(src: &ExPolygon) -> Polylines {
    polygon::to_polylines(&to_polygons(src))
}

/// `get_extents(expolygon)`: the contour's box.
#[must_use]
pub fn get_extents(expolygon: &ExPolygon) -> BoundingBox {
    polygon::get_extents(&expolygon.contour)
}

/// `get_extents(expolygons)`: the boxes of the non-empty contours merged.
#[must_use]
pub fn get_extents_ex(expolygons: &[ExPolygon]) -> BoundingBox {
    let mut bbox = BoundingBox::new();
    for e in expolygons {
        if !e.contour.points.is_empty() {
            bbox.merge_box(&get_extents(e));
        }
    }
    bbox
}

/// `get_extents_rotated(expolygon, angle)`.
#[must_use]
pub fn get_extents_rotated(expolygon: &ExPolygon, angle: f64) -> BoundingBox {
    multi_point::get_extents_rotated(&expolygon.contour.points, angle)
}

/// `get_extents_rotated(expolygons, angle)`.
#[must_use]
pub fn get_extents_rotated_ex(expolygons: &[ExPolygon], angle: f64) -> BoundingBox {
    let mut bbox = BoundingBox::new();
    if let Some((first, rest)) = expolygons.split_first() {
        bbox = get_extents_rotated(first, angle);
        for e in rest {
            bbox.merge_box(&get_extents_rotated(e, angle));
        }
    }
    bbox
}

/// `get_extents_vector(expolygons)`.
#[must_use]
pub fn get_extents_vector(polygons: &[ExPolygon]) -> Vec<BoundingBox> {
    polygons.iter().map(get_extents).collect()
}

/// `has_duplicate_points(expolygon)`: over all its points together.
#[must_use]
pub fn has_duplicate_points(expoly: &ExPolygon) -> bool {
    let mut allpts = Points::with_capacity(count_points(expoly));
    allpts.extend_from_slice(&expoly.contour.points);
    for hole in &expoly.holes {
        allpts.extend_from_slice(&hole.points);
    }
    crate::point::has_duplicate_points(&allpts)
}

/// `has_duplicate_points(expolygons)`: over all their points together.
#[must_use]
pub fn has_duplicate_points_ex(expolys: &[ExPolygon]) -> bool {
    let mut allpts = Points::with_capacity(count_points_ex(expolys));
    for e in expolys {
        allpts.extend_from_slice(&e.contour.points);
        for hole in &e.holes {
            allpts.extend_from_slice(&hole.points);
        }
    }
    crate::point::has_duplicate_points(&allpts)
}

/// `remove_same_neighbor(expolygons)`: each contour and its holes, then the expolygons whose
/// contour has two points or fewer dropped when a contour changed.
pub fn remove_same_neighbor(expolygons: &mut ExPolygons) -> bool {
    if expolygons.is_empty() {
        return false;
    }
    let mut remove_from_holes = false;
    let mut remove_from_contour = false;
    for e in expolygons.iter_mut() {
        remove_from_contour |= polygon::remove_same_neighbor(&mut e.contour);
        remove_from_holes |= polygon::remove_same_neighbor_polygons(&mut e.holes);
    }
    if remove_from_contour {
        expolygons.retain(|p| p.contour.points.len() > 2);
    }
    remove_from_holes || remove_from_contour
}

/// `remove_sticks(expolygon)`: the contour, and the holes only when the contour did not change
/// (the C++ `||` short-circuits).
pub fn remove_sticks(poly: &mut ExPolygon) -> bool {
    polygon::remove_sticks(&mut poly.contour) || polygon::remove_sticks_polygons(&mut poly.holes)
}

/// `remove_small_and_small_holes(expolygons, min_area)`.
pub fn remove_small_and_small_holes(expolygons: &mut ExPolygons, min_area: f64) -> bool {
    let mut modified = false;
    let mut free_idx = 0;
    for expoly_idx in 0..expolygons.len() {
        if orca_crt::fabs(expolygons[expoly_idx].area()) >= min_area {
            modified |= polygon::remove_small(&mut expolygons[expoly_idx].holes, min_area);
            if free_idx < expoly_idx {
                expolygons.swap(expoly_idx, free_idx);
            }
            free_idx += 1;
        } else {
            modified = true;
        }
    }
    expolygons.truncate(free_idx);
    modified
}

/// `keep_largest_contour_only(expolygons)`: the first of largest contour area (above 0) kept.
///
/// # Panics
/// When no contour has a positive area: the C++ asserts and then dereferences a null pointer.
pub fn keep_largest_contour_only(polygons: &mut ExPolygons) {
    if polygons.len() > 1 {
        let mut max_area = 0.0;
        let mut max_idx = None;
        for (i, p) in polygons.iter().enumerate() {
            let a = p.contour.area();
            if a > max_area {
                max_area = a;
                max_idx = Some(i);
            }
        }
        let Some(i) = max_idx else {
            panic!("keep_largest_contour_only: no contour of positive area");
        };
        let p = polygons.swap_remove(i);
        polygons.clear();
        polygons.push(p);
    }
}

/// `overlaps(expolys1, expolys2)`.
#[must_use]
pub fn overlaps(expolys1: &[ExPolygon], expolys2: &[ExPolygon]) -> bool {
    expolys1
        .iter()
        .any(|e1| expolys2.iter().any(|e2| e1.overlaps(e2)))
}

/// `projection_onto(expolygons, from)`: the nearest projection onto any contour or hole.
#[must_use]
pub fn projection_onto(polygons: &[ExPolygon], from: Point) -> Point {
    let mut projected_pt = Point::default();
    let mut min_dist = f64::MAX;
    for poly in polygons {
        for i in 0..poly.num_contours() {
            let c = poly.contour_or_hole(i);
            let p = from.projection_onto_lines(c.first_point(), &c.lines());
            let dist = (from - p).cast_f64().squared_norm();
            if dist < min_dist {
                projected_pt = p;
                min_dist = dist;
            }
        }
    }
    projected_pt
}
