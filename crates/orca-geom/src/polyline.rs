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
// Polyline.cpp of OrcaSlicer v2.4.2, translated on 2026-11-07; and Polyline's fitting_result with
// its upkeep, Polyline.hpp 19-141 and Polyline.cpp 152-156 and 408-517 (with the fitting parts of
// reverse, clip_end and the appends, Polyline.cpp 34-92 and 360-406), and ThickPolyline,
// Polyline.cpp 637-659 with the class of Polyline.hpp 256-289 and ThickLine of Line.hpp 202-210,
// translated on 2026-11-13. Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab)
// and the PrusaSlicer authors (Prusa Research), under the GNU Affero General Public License,
// version 3 (AGPL-3.0; LICENSE).

//! `Polyline`: an open path, with `fitting_result`, the arc fitting of its points
//! ([`crate::arc_fitter`]) that `simplify_by_fitting_arc` fills for the G-code's arcs.
//!
//! Every method keeps the fitting result in step with the points as the C++ does: an append
//! extends the last linear move or adds one after an arc, a reversal swaps and mirrors the indices
//! and reverses the arcs, `clip_end` drops the moves past the cut and clips the last arc, the
//! splits clip the arc they cut, and `simplify` and `clear` drop the fit. Appending a fitted
//! polyline to one without a fit first gives this one a linear move over its points. Two polylines
//! are equal when their points are (`operator==`), whatever their fits.
//!
//! The indices are `size_t` and wrap as the C++'s do. `split_at_index` of an empty polyline, which
//! the C++ reads outside the list for (`parity/sop1/paths.md`, "Edges"), panics.
//!
//! `ThickPolyline` and `ThickLine` are here as well: no other piece carries them, and
//! `ThickPolyline` is a `Polyline` with a width at each end of each segment. `Polyline3` and its
//! fitting are geom-2's.

use core::ops::{Deref, DerefMut};

use orca_eigen::Vec2d;

use crate::arc_fitter::{self, EMovePathType, PathFittingData};
use crate::bounding_box::BoundingBox;
use crate::line::{Line, Lines};
use crate::multi_point;
use crate::point::{Point, Points, SCALED_EPSILON, lerp_point};

/// `Polyline`.
#[derive(Clone, Debug, Default)]
pub struct Polyline {
    /// `points`.
    pub points: Points,
    /// `fitting_result`: the arc fitting of the points, empty unless `simplify_by_fitting_arc` or
    /// a fitted polyline appended filled it.
    pub fitting_result: Vec<PathFittingData>,
}

/// `operator==`: the points alone.
impl PartialEq for Polyline {
    fn eq(&self, other: &Self) -> bool {
        self.points == other.points
    }
}

impl Eq for Polyline {}

/// `Polylines`.
pub type Polylines = Vec<Polyline>;

impl Polyline {
    /// `Polyline(points)`: no fit.
    #[must_use]
    pub fn new(points: Points) -> Self {
        Self {
            points,
            fitting_result: Vec::new(),
        }
    }

    /// `Polyline(p1, p2)`.
    #[must_use]
    pub fn from_two(p1: Point, p2: Point) -> Self {
        Self::new(vec![p1, p2])
    }

    /// `Polyline::new_scale(points)`: each point `Point::new_scale`d (truncated); no fit.
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

    /// `append(point)`: skipped when it equals the last point; the fit extended.
    pub fn append(&mut self, point: Point) {
        if !self.points.is_empty() && self.last_point() == point {
            return;
        }
        self.points.push(point);
        self.append_fitting_result_after_append_points();
    }

    /// `append_before(point)`: skipped when it equals the first point. One point and the new one
    /// lose the fit; otherwise reversed, appended and reversed back.
    pub fn append_before(&mut self, point: Point) {
        if !self.points.is_empty() && self.first_point() == point {
            return;
        }
        if self.points.len() == 1 {
            self.fitting_result.clear();
            self.points.push(point);
            self.points.reverse();
        } else {
            self.reverse();
            self.append(point);
            self.reverse();
        }
    }

    /// `append(const Points &)`: the first of them skipped when it equals the last point, then
    /// [`Polyline::append_range`] (which may skip the next one too).
    pub fn append_points(&mut self, src: &[Point]) {
        if !self.points.is_empty() && !src.is_empty() && self.last_point() == src[0] {
            self.append_range(&src[1..]);
        } else {
            self.append_range(src);
        }
    }

    /// `append(begin, end)`: the first skipped when it equals the last point; the fit extended.
    pub fn append_range(&mut self, src: &[Point]) {
        if !self.points.is_empty() && !src.is_empty() && self.last_point() == src[0] {
            self.points.extend_from_slice(&src[1..]);
        } else {
            self.points.extend_from_slice(src);
        }
        self.append_fitting_result_after_append_points();
    }

    /// `append(Points &&)`: every point, none skipped; the fit extended.
    pub fn append_moved(&mut self, src: Points) {
        if self.points.is_empty() {
            self.points = src;
        } else {
            self.points.extend(src);
        }
        self.append_fitting_result_after_append_points();
    }

    /// `append(const Polyline &)`: nothing for an invalid polyline; to an empty one its points and
    /// fit; otherwise its first point (skipped when it equals the last), a linear move over this
    /// one's points when only `src` has a fit, then its other points and its fit, shifted.
    pub fn append_polyline(&mut self, src: &Polyline) {
        if !src.is_valid() {
            return;
        }
        if self.points.is_empty() {
            self.points.clone_from(&src.points);
            self.fitting_result.clone_from(&src.fitting_result);
        } else {
            self.append_polyline_tail(src);
        }
    }

    /// `append(Polyline &&)`: as [`Polyline::append_polyline`], `src` left with no points and no
    /// fit (unless it was invalid, when nothing changes).
    pub fn append_polyline_moved(&mut self, src: &mut Polyline) {
        if !src.is_valid() {
            return;
        }
        if self.points.is_empty() {
            self.points = core::mem::take(&mut src.points);
            self.fitting_result = core::mem::take(&mut src.fitting_result);
        } else {
            self.append_polyline_tail(src);
            src.points.clear();
            src.fitting_result.clear();
        }
    }

    /// The common part of the two `append(Polyline)`s for a polyline with points.
    fn append_polyline_tail(&mut self, src: &Polyline) {
        // The first point joins the two, with the fit updated.
        self.append(src.points[0]);
        // A fitted polyline appended to one without a fit: a fake linear move first, so that the
        // fit of `src` is kept.
        if self.fitting_result.is_empty() && !src.fitting_result.is_empty() {
            self.fitting_result.push(PathFittingData::linear(
                0,
                self.points.len().wrapping_sub(1),
            ));
        }
        // The other points, then the fit.
        self.points.extend_from_slice(&src.points[1..]);
        self.append_fitting_result_after_append_polyline(src);
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

    /// `clear()`: the points and the fit.
    pub fn clear(&mut self) {
        self.points.clear();
        self.fitting_result.clear();
    }

    /// `reverse()`: the points reversed; each move's ends swapped and mirrored, its arc reversed,
    /// and the moves in reverse order.
    pub fn reverse(&mut self) {
        self.points.reverse();
        if !self.fitting_result.is_empty() {
            let last = self.points.len().wrapping_sub(1);
            for data in &mut self.fitting_result {
                core::mem::swap(&mut data.start_point_index, &mut data.end_point_index);
                data.start_point_index = last.wrapping_sub(data.start_point_index);
                data.end_point_index = last.wrapping_sub(data.end_point_index);
                if data.is_arc_move() {
                    data.reverse_arc_path();
                }
            }
            self.fitting_result.reverse();
        }
    }

    /// `clip_end(distance)`: removes `distance` from the end, the new end truncated to `coord_t`;
    /// the moves that start past the cut dropped, the last one ending at the new end (an arc
    /// clipped there, or made a linear move when it cannot be).
    pub fn clip_end(&mut self, mut distance: f64) {
        let mut remove_after_index = self.points.len();
        while distance > 0.0 {
            let last_point = self.last_point().cast_f64();
            self.points.pop();
            remove_after_index = remove_after_index.wrapping_sub(1);
            if self.points.is_empty() {
                self.fitting_result.clear();
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

        // No fit to clip.
        if self.fitting_result.is_empty() {
            return;
        }
        while let Some(last) = self.fitting_result.last() {
            if last.start_point_index >= remove_after_index {
                self.fitting_result.pop();
            } else {
                break;
            }
        }
        let n = self.points.len();
        if let Some(back) = self.fitting_result.last_mut() {
            // The last move left an arc: clipped at the last point.
            if back.path_type == EMovePathType::ArcMoveCcw
                || back.path_type == EMovePathType::ArcMoveCw
            {
                if back.arc_data.clip_end(self.points[n - 1]) {
                    // Clipped: the last point moves onto the arc.
                    self.points[n - 1] = back.arc_data.end_point;
                } else {
                    // Not clipped: a linear move again.
                    back.path_type = EMovePathType::LinearMove;
                }
            }
            back.end_point_index = n - 1;
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
                line.clear();
                line.append(self.points[i]);
                len = 0.0;
                i += 1;
                continue;
            }
            let take = distance;
            line.append(Point::cast_from_vec2d(p1 + v * (take / v.norm())));
            lines.push(line.clone());
            line.clear();
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

    /// `simplify(tolerance)`: Douglas-Peucker; the fit dropped.
    pub fn simplify(&mut self, tolerance: f64) {
        self.points = multi_point::douglas_peucker(&self.points, tolerance);
        self.fitting_result.clear();
    }

    /// `simplify_by_fitting_arc(tolerance)`: the arc fitting, then Douglas-Peucker on the straight
    /// parts ([`arc_fitter::do_arc_fitting_and_simplify`], into this polyline's own fit).
    pub fn simplify_by_fitting_arc(&mut self, tolerance: f64) {
        arc_fitter::do_arc_fitting_and_simplify(
            &mut self.points,
            &mut self.fitting_result,
            tolerance,
        );
    }

    /// `split_at_index(index, p1, p2)`: false when `index` is past the last point. Split inside,
    /// the moves on each side go with each half, the move across `index` clipped at it (an arc's
    /// end or start moved onto the arc, or the arc made a linear move when it cannot be clipped).
    ///
    /// # Panics
    /// On an empty polyline, which the C++ reads outside the list for.
    #[must_use]
    pub fn split_at_index(&self, index: usize) -> Option<(Polyline, Polyline)> {
        let n = self.points.len();
        if index > n.wrapping_sub(1) {
            return None;
        }
        if n == 0 {
            panic!(
                "Polyline::split_at_index of an empty polyline: undefined in the C++ engine (it \
                 reads outside the list), not ported"
            );
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
            // The first part.
            p1.points.extend_from_slice(&self.points[..=index]);
            if let Some((new_endpoint, data)) = self.split_fitting_result_before_index(index) {
                p1.fitting_result = data;
                let last = p1.points.len() - 1;
                p1.points[last] = new_endpoint;
            }
            p2.points.extend_from_slice(&self.points[index..]);
            if let Some((new_startpoint, data)) = self.split_fitting_result_after_index(index) {
                p2.fitting_result = data;
                p2.points[0] = new_startpoint;
            }
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

    /// `append_fitting_result_after_append_points()`: after points were appended, the last
    /// linear move extended to the new last point, or a linear move added after an arc.
    fn append_fitting_result_after_append_points(&mut self) {
        let last = self.points.len().wrapping_sub(1);
        if let Some(back) = self.fitting_result.last_mut() {
            if back.is_linear_move() {
                back.end_point_index = last;
            } else {
                let new_start = back.end_point_index;
                let new_end = last;
                if new_start != new_end {
                    self.fitting_result
                        .push(PathFittingData::linear(new_start, new_end));
                }
            }
        }
    }

    /// `append_fitting_result_after_append_polyline(src)`: after `src`'s points were appended,
    /// its fit shifted by this fit's last end, or a linear move to the new last point when `src`
    /// has none. Nothing when this polyline has no fit.
    fn append_fitting_result_after_append_polyline(&mut self, src: &Polyline) {
        let Some(back) = self.fitting_result.last() else {
            return;
        };
        if src.fitting_result.is_empty() {
            // `src` has no fit: a linear move.
            let new_start = back.end_point_index;
            let new_end = self.points.len().wrapping_sub(1);
            if new_start != new_end {
                self.fitting_result
                    .push(PathFittingData::linear(new_start, new_end));
            }
        } else {
            // `src`'s fit, shifted.
            let old_size = self.fitting_result.len();
            let index_offset = back.end_point_index;
            self.fitting_result.extend_from_slice(&src.fitting_result);
            for data in &mut self.fitting_result[old_size..] {
                data.start_point_index = data.start_point_index.wrapping_add(index_offset);
                data.end_point_index = data.end_point_index.wrapping_add(index_offset);
            }
        }
    }

    /// `reset_to_linear_move()`: the fit one linear move over every point. (Nothing in the engine
    /// calls it.)
    pub fn reset_to_linear_move(&mut self) {
        self.fitting_result.clear();
        self.fitting_result.push(PathFittingData::linear(
            0,
            self.points.len().wrapping_sub(1),
        ));
    }

    /// `split_fitting_result_before_index(index, new_endpoint, data)`: `None` for no fit; else the
    /// moves that start before `index`, the last ending at `index` (an arc across it clipped there,
    /// or made a linear move), and the new end point (`points[index]`, or the clipped arc's end).
    fn split_fitting_result_before_index(
        &self,
        index: usize,
    ) -> Option<(Point, Vec<PathFittingData>)> {
        let mut new_endpoint = self.points[index];
        if self.fitting_result.is_empty() {
            return None;
        }
        // The moves before the index.
        let mut data = Vec::with_capacity(self.fitting_result.len());
        for d in &self.fitting_result {
            if d.start_point_index < index {
                data.push(*d);
            } else {
                break;
            }
        }
        if let Some(back) = data.last_mut() {
            // An arc across the index: clipped there, for a new end point.
            if back.is_arc_move() && back.end_point_index > index {
                if back.arc_data.clip_end(self.points[index]) {
                    new_endpoint = back.arc_data.end_point;
                } else {
                    back.path_type = EMovePathType::LinearMove;
                }
            }
            back.end_point_index = index;
        }
        Some((new_endpoint, data))
    }

    /// `split_fitting_result_after_index(index, new_startpoint, data)`: `None` for no fit; else the
    /// moves that end after `index`, shifted back by it, the first starting at 0 (an arc across it
    /// clipped there, or made a linear move), and the new start point.
    fn split_fitting_result_after_index(
        &self,
        index: usize,
    ) -> Option<(Point, Vec<PathFittingData>)> {
        let mut new_startpoint = self.points[index];
        if self.fitting_result.is_empty() {
            return None;
        }
        let mut data: Vec<PathFittingData> = self
            .fitting_result
            .iter()
            .filter(|d| d.end_point_index > index)
            .copied()
            .collect();
        for (i, d) in data.iter_mut().enumerate() {
            if i != 0 {
                d.start_point_index = d.start_point_index.wrapping_sub(index);
                d.end_point_index = d.end_point_index.wrapping_sub(index);
            } else {
                d.end_point_index = d.end_point_index.wrapping_sub(index);
                // An arc across the index: clipped there, for a new start point.
                if d.is_arc_move() && d.start_point_index < index {
                    if d.arc_data.clip_start(self.points[index]) {
                        new_startpoint = d.arc_data.start_point;
                    } else {
                        d.path_type = EMovePathType::LinearMove;
                    }
                }
                d.start_point_index = 0;
            }
        }
        Some((new_startpoint, data))
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

/// `remove_same_neighbor(polyline)`: `std::unique` of the points (the fit as it was).
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

/// `remove_degenerate(polylines)`: those below two points dropped. The C++ swaps only the points
/// of the polylines it keeps into place, so a kept polyline takes the fit of the slot it moves to.
pub fn remove_degenerate(polylines: &mut Polylines) -> bool {
    let mut modified = false;
    let mut j = 0;
    for i in 0..polylines.len() {
        if polylines[i].points.len() >= 2 {
            if j < i {
                let (head, tail) = polylines.split_at_mut(i);
                core::mem::swap(&mut head[j].points, &mut tail[0].points);
            }
            j += 1;
        } else {
            modified = true;
        }
    }
    if j < polylines.len() {
        polylines.truncate(j);
    }
    modified
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

/// `ThickLine` (Line.hpp 202-210, a helper no piece carries): a segment with a width at each end.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ThickLine {
    /// `a`.
    pub a: Point,
    /// `b`.
    pub b: Point,
    /// `a_width`.
    pub a_width: f64,
    /// `b_width`.
    pub b_width: f64,
}

impl ThickLine {
    /// `ThickLine(a, b, wa, wb)`.
    #[must_use]
    pub fn new(a: Point, b: Point, a_width: f64, b_width: f64) -> Self {
        Self {
            a,
            b,
            a_width,
            b_width,
        }
    }
}

/// `ThickLines`.
pub type ThickLines = Vec<ThickLine>;

/// `ThickPolyline` (Polyline.hpp 256-289, a helper no piece carries): a `Polyline` (its points and
/// fit through `Deref`) with two widths per segment, one at each end, and whether each end is an
/// end of the medial axis it came from.
#[derive(Clone, Debug, Default)]
pub struct ThickPolyline {
    /// The `Polyline` base.
    pub polyline: Polyline,
    /// `width`: `2 * (size() - 1)` widths, segment by segment.
    pub width: Vec<f64>,
    /// `endpoints`.
    pub endpoints: (bool, bool),
}

impl Deref for ThickPolyline {
    type Target = Polyline;

    fn deref(&self) -> &Polyline {
        &self.polyline
    }
}

impl DerefMut for ThickPolyline {
    fn deref_mut(&mut self) -> &mut Polyline {
        &mut self.polyline
    }
}

/// `ThickPolylines`.
pub type ThickPolylines = Vec<ThickPolyline>;

impl ThickPolyline {
    /// `thicklines()`: each segment with its two widths; none below two points.
    #[must_use]
    pub fn thicklines(&self) -> ThickLines {
        let mut lines = ThickLines::new();
        let points = &self.polyline.points;
        if points.len() >= 2 {
            for (i, w) in points.windows(2).enumerate() {
                lines.push(ThickLine::new(
                    w[0],
                    w[1],
                    self.width[2 * i],
                    self.width[2 * i + 1],
                ));
            }
        }
        lines
    }

    /// `reverse()`: the polyline, the widths and the end flags.
    pub fn reverse(&mut self) {
        self.polyline.reverse();
        self.width.reverse();
        core::mem::swap(&mut self.endpoints.0, &mut self.endpoints.1);
    }

    /// `clear()`: the polyline and the widths.
    pub fn clear(&mut self) {
        self.polyline.clear();
        self.width.clear();
    }

    /// `start_at_index(index)`: a closed polyline (first point and width equal to the last)
    /// rotated to start at `index`; nothing for index 0, the last index or an open polyline.
    ///
    /// # Panics
    /// Where the C++ goes outside its lists: `index` outside [0, size) on a closed polyline,
    /// points with no widths, or no points with an index other than 0 and -1.
    pub fn start_at_index(&mut self, index: i32) {
        let n = self.polyline.points.len();
        if index == 0 || index.wrapping_add(1) == n as i32 {
            return;
        }
        let points = &mut self.polyline.points;
        if n == 0 || points[0] != points[n - 1] {
            if n == 0 {
                undefined_thick("ThickPolyline::start_at_index of no points");
            }
            return;
        }
        if self.width.is_empty() {
            undefined_thick("ThickPolyline::start_at_index with no widths");
        }
        if self.width[0] != self.width[self.width.len() - 1] {
            return;
        }
        let shift = usize::try_from(index).unwrap_or(usize::MAX);
        if shift >= n - 1 || 2 * shift > self.width.len() {
            undefined_thick("ThickPolyline::start_at_index outside [0, size)");
        }
        points.pop();
        points.rotate_left(shift);
        self.width.rotate_left(2 * shift);
        let first = points[0];
        points.push(first);
    }
}

/// Stops on an input the C++ reads outside its lists for (`parity/sop1/paths.md`, "Edges").
fn undefined_thick(what: &str) -> ! {
    panic!("{what}: undefined in the C++ engine (it reads outside its lists), not ported")
}

/// `to_thick_polylines(polylines, width)`: each polyline's points with `width` at both ends of
/// every segment.
///
/// # Panics
/// On a polyline with no points, whose `(size() - 1) * 2` widths the C++ cannot allocate.
#[must_use]
pub fn to_thick_polylines(polylines: Polylines, width: f64) -> ThickPolylines {
    let mut out = ThickPolylines::with_capacity(polylines.len());
    for polyline in polylines {
        let Some(segments) = polyline.points.len().checked_sub(1) else {
            panic!("to_thick_polylines of a polyline with no points: the C++ throws length_error");
        };
        out.push(ThickPolyline {
            polyline: Polyline::new(polyline.points),
            width: vec![width; segments * 2],
            endpoints: (false, false),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circle::{ArcDirection, ArcSegment};

    fn arc_fit(
        start: usize,
        end: usize,
        center: Point,
        radius: f64,
        s: Point,
        e: Point,
    ) -> PathFittingData {
        PathFittingData::new(
            start,
            end,
            EMovePathType::ArcMoveCcw,
            ArcSegment::new(center, radius, s, e, ArcDirection::Ccw),
        )
    }

    /// Appending points after an arc adds a linear move; a reversal mirrors and reverses it.
    #[test]
    fn the_fit_follows_appends_and_reversal() {
        let mut pl = Polyline::new(vec![
            Point::new(1000, 0),
            Point::new(707, 707),
            Point::new(0, 1000),
        ]);
        pl.fitting_result = vec![arc_fit(
            0,
            2,
            Point::new(0, 0),
            1000.0,
            Point::new(1000, 0),
            Point::new(0, 1000),
        )];
        pl.append(Point::new(-500, 1000));
        assert_eq!(pl.fitting_result.len(), 2);
        assert_eq!(pl.fitting_result[1], PathFittingData::linear(2, 3));
        pl.reverse();
        assert_eq!(pl.fitting_result[0], PathFittingData::linear(0, 1));
        assert_eq!(pl.fitting_result[1].start_point_index, 1);
        assert_eq!(pl.fitting_result[1].end_point_index, 3);
        assert_eq!(pl.fitting_result[1].path_type, EMovePathType::ArcMoveCw);
        // Equality is of the points alone.
        assert_eq!(pl, Polyline::new(pl.points.clone()));
    }

    #[test]
    fn remove_degenerate_moves_points_not_fits() {
        let mut v = vec![
            Polyline::new(vec![Point::new(0, 0)]),
            Polyline::new(vec![Point::new(0, 0), Point::new(1, 0)]),
        ];
        v[0].reset_to_linear_move();
        assert!(remove_degenerate(&mut v));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].points.len(), 2);
        assert_eq!(v[0].fitting_result, vec![PathFittingData::linear(0, 0)]);
    }
}
