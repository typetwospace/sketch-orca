// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: class Polyline and its helpers of src/libslic3r/Polyline.hpp and
// Polyline.cpp of OrcaSlicer v2.4.2, translated on 2026-11-07. Copyright the OrcaSlicer authors,
// the BambuStudio authors (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU
// Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! `Polyline`: an open path.
//!
//! The C++ class also carries `fitting_result`, the arc-fitting data of `ArcFitter.hpp` that only
//! `simplify_by_fitting_arc` fills (G-code export). Every method here is the C++ one with that
//! data empty, which is what it is for every polyline geometry code makes; the arc fitting and
//! `Polyline3` come with the G-code export.

use orca_eigen::Vec2d;

use crate::bounding_box::BoundingBox;
use crate::line::{Line, Lines};
use crate::multi_point;
use crate::point::{Point, Points, SCALED_EPSILON, lerp_point};

/// `Polyline`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Polyline {
    /// `points`.
    pub points: Points,
}

/// `Polylines`.
pub type Polylines = Vec<Polyline>;

impl Polyline {
    /// `Polyline(points)`.
    #[must_use]
    pub fn new(points: Points) -> Self {
        Self { points }
    }

    /// `Polyline(p1, p2)`.
    #[must_use]
    pub fn from_two(p1: Point, p2: Point) -> Self {
        Self::new(vec![p1, p2])
    }

    /// `Polyline::new_scale(points)`: each point `Point::new_scale`d (truncated).
    #[must_use]
    pub fn new_scale(points: &[Vec2d]) -> Self {
        Self::new(
            points
                .iter()
                .map(|p| Point::new_scale(p.x(), p.y()))
                .collect(),
        )
    }

    /// `size()`.
    #[must_use]
    pub fn size(&self) -> usize {
        self.points.len()
    }

    /// `empty()`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// `is_valid()`: two points at least.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.points.len() >= 2
    }

    /// `first_point()`.
    #[must_use]
    pub fn first_point(&self) -> Point {
        self.points[0]
    }

    /// `last_point()`.
    #[must_use]
    pub fn last_point(&self) -> Point {
        self.points[self.points.len() - 1]
    }

    /// `is_closed()`: the first point equals the last.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.points[0] == self.points[self.points.len() - 1]
    }

    /// `append(point)`: skipped when it equals the last point.
    pub fn append(&mut self, point: Point) {
        if !self.points.is_empty() && self.last_point() == point {
            return;
        }
        self.points.push(point);
    }

    /// `append_before(point)`: skipped when it equals the first point.
    pub fn append_before(&mut self, point: Point) {
        if !self.points.is_empty() && self.first_point() == point {
            return;
        }
        if self.points.len() == 1 {
            self.points.push(point);
            self.points.reverse();
        } else {
            self.reverse();
            self.append(point);
            self.reverse();
        }
    }

    /// `append(points)`: the first of them skipped when it equals the last point.
    pub fn append_points(&mut self, src: &[Point]) {
        if !self.points.is_empty() && !src.is_empty() && self.last_point() == src[0] {
            self.append_range(&src[1..]);
        } else {
            self.append_range(src);
        }
    }

    /// `append(begin, end)`.
    pub fn append_range(&mut self, src: &[Point]) {
        if !self.points.is_empty() && !src.is_empty() && self.last_point() == src[0] {
            self.points.extend_from_slice(&src[1..]);
        } else {
            self.points.extend_from_slice(src);
        }
    }

    /// `append(Points &&)`: every point, none skipped.
    pub fn append_moved(&mut self, src: Points) {
        self.points.extend(src);
    }

    /// `append(const Polyline &)`: nothing for an invalid polyline; otherwise the first point
    /// (skipped when it equals the last) and then the others.
    pub fn append_polyline(&mut self, src: &Polyline) {
        if !src.is_valid() {
            return;
        }
        if self.points.is_empty() {
            self.points.clone_from(&src.points);
        } else {
            self.append(src.points[0]);
            self.points.extend_from_slice(&src.points[1..]);
        }
    }

    /// `leftmost_point()`: the first point of smallest x.
    #[must_use]
    pub fn leftmost_point(&self) -> Point {
        let mut p = self.points[0];
        for &it in &self.points[1..] {
            if it.x < p.x {
                p = it;
            }
        }
        p
    }

    /// `lines()`: the segments; none below two points.
    #[must_use]
    pub fn lines(&self) -> Lines {
        to_lines(self)
    }

    /// `length()`: `MultiPoint::length()`, the sum of `lines()`' lengths.
    #[must_use]
    pub fn length(&self) -> f64 {
        multi_point::lines_length(&self.lines())
    }

    /// `bounding_box()`.
    #[must_use]
    pub fn bounding_box(&self) -> BoundingBox {
        BoundingBox::from_points(&self.points)
    }

    /// `reverse()`.
    pub fn reverse(&mut self) {
        self.points.reverse();
    }

    /// `clip_end(distance)`: removes `distance` from the end, the new end truncated to `coord_t`.
    pub fn clip_end(&mut self, mut distance: f64) {
        while distance > 0.0 {
            let last_point = self.last_point().cast_f64();
            self.points.pop();
            if self.points.is_empty() {
                return;
            }
            let v = self.last_point().cast_f64() - last_point;
            let lsqr = v.squared_norm();
            if lsqr > distance * distance {
                self.points.push(Point::cast_from_vec2d(
                    last_point + v * (distance / orca_crt::sqrt(lsqr)),
                ));
                break;
            }
            distance -= orca_crt::sqrt(lsqr);
        }
    }

    /// `clip_start(distance)`: `clip_end` of the reversed polyline, reversed back when two points
    /// or more remain.
    pub fn clip_start(&mut self, distance: f64) {
        self.reverse();
        self.clip_end(distance);
        if self.points.len() >= 2 {
            self.reverse();
        }
    }

    /// `extend_end(distance)`: a new last point along the last segment, the step truncated.
    pub fn extend_end(&mut self, distance: f64) {
        let n = self.points.len();
        let v = (self.points[n - 1] - self.points[n - 2])
            .cast_f64()
            .normalized();
        let new_last_point = self.points[n - 1] + Point::cast_from_vec2d(v * distance);
        self.append(new_last_point);
    }

    /// `extend_start(distance)`.
    pub fn extend_start(&mut self, distance: f64) {
        self.reverse();
        self.extend_end(distance);
        self.reverse();
    }

    /// `equally_spaced_points(distance)`: points `distance` apart along the path, from the first.
    #[must_use]
    pub fn equally_spaced_points(&self, distance: f64) -> Points {
        let mut points = vec![self.first_point()];
        let mut len = 0.0;
        let mut i = 1;
        while i < self.points.len() {
            let p1 = self.points[i - 1].cast_f64();
            let v = self.points[i].cast_f64() - p1;
            let segment_length = v.norm();
            len += segment_length;
            if len < distance {
                i += 1;
                continue;
            }
            if len == distance {
                points.push(self.points[i]);
                len = 0.0;
                i += 1;
                continue;
            }
            // How much of this segment is taken.
            let take = segment_length - (len - distance);
            points.push(Point::cast_from_vec2d(p1 + v * (take / v.norm())));
            // The C++ steps its iterator back, so the same segment comes again.
            len = -take;
        }
        points
    }

    /// `equally_spaced_lines(distance)`: the path cut into pieces `distance` long.
    #[must_use]
    pub fn equally_spaced_lines(&self, distance: f64) -> Polylines {
        let mut lines = Polylines::new();
        let mut line = Polyline::default();
        line.append(self.first_point());
        let mut len = 0.0;
        let mut i = 1;
        while i < self.points.len() {
            let p1 = line.last_point().cast_f64();
            let v = self.points[i].cast_f64() - p1;
            let segment_length = v.norm();
            len += segment_length;
            if len < distance {
                i += 1;
                continue;
            }
            if len == distance {
                line.append(self.points[i]);
                lines.push(line.clone());
                line.points.clear();
                line.append(self.points[i]);
                len = 0.0;
                i += 1;
                continue;
            }
            let take = distance;
            line.append(Point::cast_from_vec2d(p1 + v * (take / v.norm())));
            lines.push(line.clone());
            line.points.clear();
            let last = lines[lines.len() - 1].last_point();
            line.append(last);
            len = -take;
        }
        // The remainder.
        if line.points.len() == 1 {
            line.append(self.last_point());
            if line.first_point() != line.last_point() {
                lines.push(line);
            }
        }
        lines
    }

    /// `simplify(tolerance)`: Douglas-Peucker.
    pub fn simplify(&mut self, tolerance: f64) {
        self.points = multi_point::douglas_peucker(&self.points, tolerance);
    }

    /// `split_at_index(index, p1, p2)`: false when `index` is past the last point.
    #[must_use]
    pub fn split_at_index(&self, index: usize) -> Option<(Polyline, Polyline)> {
        let n = self.points.len();
        if index > n.wrapping_sub(1) {
            return None;
        }
        let mut p1 = Polyline::default();
        let mut p2 = Polyline::default();
        if index == 0 {
            p1.append(self.first_point());
            p2 = self.clone();
        } else if index == n - 1 {
            p2.append(self.last_point());
            p1 = self.clone();
        } else {
            p1.points.extend_from_slice(&self.points[..=index]);
            p2.points.extend_from_slice(&self.points[index..]);
        }
        Some((p1, p2))
    }

    /// `split_at(point, p1, p2)`: at a vertex equal to `point`, or at its projection onto the
    /// nearest segment, with `point` itself added to both halves. Returns the halves and the new
    /// `point` (the C++ writes it back only when `point` is a vertex).
    #[must_use]
    pub fn split_at(&self, point: Point) -> Option<(Polyline, Polyline, Point)> {
        if self.points.is_empty() {
            return None;
        }
        let index = multi_point::find_point(&self.points, point);
        if index != -1 {
            let (p1, p2) = self
                .split_at_index(index as usize)
                .unwrap_or_else(|| unreachable!("a vertex index is in range"));
            let point = if p1.is_valid() {
                p1.last_point()
            } else {
                p2.first_point()
            };
            return Some((p1, p2, point));
        }
        // The segment to split.
        let mut line_idx = 0;
        let mut p = self.first_point();
        let mut min = (p - point).cast_f64().norm();
        let lines = self.lines();
        for (i, line) in lines.iter().enumerate() {
            let p_tmp = point.projection_onto_line(line);
            if (p_tmp - point).cast_f64().norm() < min {
                p = p_tmp;
                min = (p - point).cast_f64().norm();
                line_idx = i;
            }
        }
        let index = multi_point::find_point(&self.points, p);
        let (p1, p2) = if index != -1 {
            let (mut p1, mut p2) = self
                .split_at_index(index as usize)
                .unwrap_or_else(|| unreachable!("a vertex index is in range"));
            p1.append(point);
            p2.append_before(point);
            (p1, p2)
        } else {
            let (mut p1, _) = self
                .split_at_index(line_idx)
                .unwrap_or_else(|| unreachable!("a segment's first index is in range"));
            p1.append(point);
            let (_, mut p2) = self
                .split_at_index(line_idx + 1)
                .unwrap_or_else(|| unreachable!("a segment's second index is in range"));
            p2.append_before(point);
            (p1, p2)
        };
        Some((p1, p2, point))
    }

    /// `split_at_length(length, p1, p2)`.
    #[must_use]
    pub fn split_at_length(&self, length: f64) -> Option<(Polyline, Polyline)> {
        if self.points.is_empty() {
            return None;
        }
        if length < 0.0 || length > self.length() {
            return None;
        }
        if length < SCALED_EPSILON {
            let mut p1 = Polyline::default();
            p1.append(self.first_point());
            return Some((p1, self.clone()));
        }
        if orca_crt::fabs(length - self.length()) < SCALED_EPSILON {
            let mut p2 = Polyline::default();
            p2.append(self.last_point());
            return Some((self.clone(), p2));
        }
        // The segment to split.
        let mut line_idx = 0;
        let mut acc_length = 0.0;
        let mut p = self.first_point();
        for l in &self.lines() {
            p = l.b;
            let current_length = l.length();
            if acc_length + current_length >= length {
                p = lerp_point(l.a, l.b, (length - acc_length) / current_length);
                break;
            }
            acc_length += current_length;
            line_idx += 1;
        }
        let index = multi_point::find_point(&self.points, p);
        if index != -1 {
            return self.split_at_index(index as usize);
        }
        let (mut p1, _) = self.split_at_index(line_idx)?;
        p1.append(p);
        let (_, mut p2) = self.split_at_index(line_idx + 1)?;
        p2.append_before(p);
        Some((p1, p2))
    }

    /// `is_straight()`: every segment parallel to the first-to-last direction.
    #[must_use]
    pub fn is_straight(&self) -> bool {
        let dir = Line::new(self.first_point(), self.last_point()).direction();
        self.lines().iter().all(|line| line.parallel_to_angle(dir))
    }
}

/// `get_extents(polyline)`.
#[must_use]
pub fn get_extents(polyline: &Polyline) -> BoundingBox {
    polyline.bounding_box()
}

/// `get_extents(polylines)`: the first one's box, merged with the other points.
#[must_use]
pub fn get_extents_polylines(polylines: &[Polyline]) -> BoundingBox {
    let mut bb = BoundingBox::new();
    if let Some((first, rest)) = polylines.split_first() {
        bb = first.bounding_box();
        for p in rest {
            bb.merge_points(&p.points);
        }
    }
    bb
}

/// `remove_same_neighbor(polyline)`: `std::unique`.
pub fn remove_same_neighbor(polyline: &mut Polyline) -> bool {
    let points = &mut polyline.points;
    if points.is_empty() {
        return false;
    }
    let last = orca_stl::unique(points);
    if last == points.len() {
        return false;
    }
    points.truncate(last);
    true
}

/// `remove_same_neighbor(polylines)`: each one, then those of one point or none dropped.
pub fn remove_same_neighbor_polylines(polylines: &mut Polylines) -> bool {
    if polylines.is_empty() {
        return false;
    }
    let mut exist = false;
    for p in polylines.iter_mut() {
        exist |= remove_same_neighbor(p);
    }
    polylines.retain(|p| p.points.len() > 1);
    exist
}

/// `leftmost_point(polylines)`.
///
/// # Panics
/// On no polylines, as the C++ throws `InvalidArgument`.
#[must_use]
pub fn leftmost_point(polylines: &[Polyline]) -> Point {
    let Some((first, rest)) = polylines.split_first() else {
        panic!("leftmost_point() called on empty PolylineCollection");
    };
    let mut p = first.leftmost_point();
    for pl in rest {
        let p2 = pl.leftmost_point();
        if p2.x < p.x {
            p = p2;
        }
    }
    p
}

/// `remove_degenerate(polylines)`: those below two points dropped.
pub fn remove_degenerate(polylines: &mut Polylines) -> bool {
    let n = polylines.len();
    polylines.retain(|p| p.points.len() >= 2);
    polylines.len() != n
}

/// `foot_pt(polyline, pt)`: the index of the nearest segment's first point and the foot on it;
/// `(-1, (0, 0))` below two points.
#[must_use]
pub fn foot_pt(polyline: &[Point], pt: Point) -> (i32, Point) {
    if polyline.len() < 2 {
        return (-1, Point::new(0, 0));
    }
    let mut d2_min = f64::MAX;
    let mut foot_pt_min = Point::default();
    let mut prev = polyline[0];
    let mut it_proj = 0_usize;
    for (i, &p) in polyline.iter().enumerate().skip(1) {
        let foot = pt.projection_onto_line(&Line::new(prev, p));
        let d2 = (foot - pt).cast_f64().squared_norm();
        if d2 < d2_min {
            d2_min = d2;
            foot_pt_min = foot;
            it_proj = i;
        }
        prev = p;
    }
    (it_proj as i32 - 1, foot_pt_min)
}

/// `total_length(polylines)`.
#[must_use]
pub fn total_length(polylines: &[Polyline]) -> f64 {
    let mut total = 0.0;
    for pl in polylines {
        total += pl.length();
    }
    total
}

/// `to_lines(polyline)`.
#[must_use]
pub fn to_lines(poly: &Polyline) -> Lines {
    let mut lines = Lines::new();
    if poly.points.len() >= 2 {
        for w in poly.points.windows(2) {
            lines.push(Line::new(w[0], w[1]));
        }
    }
    lines
}

/// `to_lines(polylines)`.
///
/// The C++ loops `begin() .. end() - 1` without a size check, which is undefined for an empty
/// polyline; this gives it no lines.
#[must_use]
pub fn to_lines_polylines(polys: &[Polyline]) -> Lines {
    let mut lines = Lines::new();
    for poly in polys {
        lines.extend(to_lines(poly));
    }
    lines
}

/// `to_polylines(paths)`.
#[must_use]
pub fn to_polylines(paths: Vec<Points>) -> Polylines {
    paths.into_iter().map(Polyline::new).collect()
}

/// `polylines_append(dst, src)`.
pub fn polylines_append(dst: &mut Polylines, src: Polylines) {
    dst.extend(src);
}

/// `polylines_merge(dst, dst_first, src, src_first)`: joins `src` to `dst` at the given ends.
pub fn polylines_merge(dst: &mut Points, dst_first: bool, mut src: Points, src_first: bool) {
    if dst_first {
        if src_first {
            dst.reverse();
        } else {
            core::mem::swap(dst, &mut src);
        }
    } else if !src_first {
        src.reverse();
    }
    dst.extend(src);
}
