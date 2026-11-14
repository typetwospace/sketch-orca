// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: class Polyline3 of src/libslic3r/Polyline.hpp (291-353) and
// Polyline.cpp (661-1042), foot_pt of a Points3 (Polyline.cpp 615-635) and to_polyline of
// Points and of Points3 (Point.hpp 88-89, Point.cpp 263-264), of OrcaSlicer v2.4.2, translated on
// 2026-11-13. Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and the
// PrusaSlicer authors (Prusa Research), under the GNU Affero General Public License, version 3
// (AGPL-3.0; LICENSE).

//! `Polyline3`: an open path of Point3s with its arc fitting (`fitting_result`), the path an
//! `ExtrusionPath` of non-planar printing (ZAA) holds.
//!
//! It follows `Polyline` ([`crate::polyline`]) method for method: an append extends the last
//! linear move or adds one after an arc, a reversal swaps and mirrors the indices and reverses the
//! arcs, `clip_end` drops the moves past the cut and clips the last arc, the splits clip the arc
//! they cut, and `simplify` drops the fit. Where it differs from `Polyline`, the C++ does too:
//!
//! - `clear()` is `MultiPoint3::clear()`, which empties the points and keeps the fit (no override,
//!   unlike `Polyline::clear`). The splits clear their out-parameters that way, so a half that
//!   held a fit before can carry it into an append ([`Polyline3::split_at_index`] takes them as
//!   `&mut`, as the C++ does).
//! - `simplify_by_fitting_arc` fits the points' x and y and gives every point z 0.
//! - A clipped arc's new end in `split_at_index` has z 0; in `clip_end` it keeps the last point's z.
//! - `split_at` of a `Point` finds a vertex by x and y; `split_at_length` finds its point by x, y
//!   and z.
//! - `Polyline3(const Polyline&, z)` takes the points with that z and no fit.
//!
//! The indices are `size_t` and wrap as the C++'s do. Inputs the C++ reads outside its list for
//! panic, naming the call: the end points of an empty polyline (`first_point`, `last_point`, and
//! with them `split_at_index` and `clip_end` of one).

use crate::arc_fitter::{self, EMovePathType, PathFittingData};
use crate::multi_point3;
use crate::point::{Coord, Point, SCALED_EPSILON};
use crate::point3::{self, BoundingBox3, Line3, Lines3, Point3, Points3, to_points, to_points3};
use crate::polyline::Polyline;

/// `Polyline3`.
#[derive(Clone, Debug, Default)]
pub struct Polyline3 {
    /// `points`.
    pub points: Points3,
    /// `fitting_result`: the arc fitting of the points' x and y, empty unless
    /// `simplify_by_fitting_arc` or a fitted polyline appended filled it.
    pub fitting_result: Vec<PathFittingData>,
}

/// `Polylines3`.
pub type Polylines3 = Vec<Polyline3>;

/// What the C++ leaves undefined: reading outside the point list.
fn undefined(what: &str) -> ! {
    panic!("{what}: undefined in the C++ engine (it reads outside the list), not ported")
}

impl Polyline3 {
    /// `Polyline3(points)`: no fit.
    #[must_use]
    pub fn new(points: Points3) -> Self {
        Self {
            points,
            fitting_result: Vec::new(),
        }
    }

    /// `Polyline3(const Polyline &poly, coord_t z)`: the points with that z; no fit.
    #[must_use]
    pub fn from_polyline(poly: &Polyline, z: Coord) -> Self {
        let mut points = Points3::with_capacity(poly.points.len());
        for pt in &poly.points {
            points.push(Point3::new(pt.x, pt.y, z));
        }
        Self::new(points)
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

    /// `is_valid()`: two points or more.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.points.len() >= 2
    }

    /// `first_point()`.
    ///
    /// # Panics
    /// On an empty polyline, which the C++ reads outside the list for.
    #[must_use]
    pub fn first_point(&self) -> Point3 {
        match self.points.first() {
            Some(&p) => p,
            None => undefined("Polyline3::first_point of an empty polyline"),
        }
    }

    /// `last_point()`.
    ///
    /// # Panics
    /// On an empty polyline, which the C++ reads outside the list for.
    #[must_use]
    pub fn last_point(&self) -> Point3 {
        match self.points.last() {
            Some(&p) => p,
            None => undefined("Polyline3::last_point of an empty polyline"),
        }
    }

    /// `MultiPoint3::clear()`: the points only; the fit stays (`Polyline3` does not override it).
    pub fn clear(&mut self) {
        self.points.clear();
    }

    /// `MultiPoint3::translate(vector)`.
    pub fn translate(&mut self, v: &Point3) {
        multi_point3::translate(&mut self.points, v);
    }

    /// `MultiPoint3::translate(x, y, z)`.
    pub fn translate_f64(&mut self, x: f64, y: f64, z: f64) {
        multi_point3::translate_f64(&mut self.points, x, y, z);
    }

    /// `MultiPoint3::rotate(angle)`.
    pub fn rotate(&mut self, angle: f64) {
        multi_point3::rotate(&mut self.points, angle);
    }

    /// `MultiPoint3::rotate(cos_angle, sin_angle)`.
    pub fn rotate_cs(&mut self, cos_angle: f64, sin_angle: f64) {
        multi_point3::rotate_cs(&mut self.points, cos_angle, sin_angle);
    }

    /// `MultiPoint3::rotate(angle, center)`.
    pub fn rotate_around(&mut self, angle: f64, center: &Point3) {
        multi_point3::rotate_around(&mut self.points, angle, center);
    }

    /// `MultiPoint3::length()`: the sum of `lines()`' lengths.
    #[must_use]
    pub fn length(&self) -> f64 {
        multi_point3::lines_length(&self.lines())
    }

    /// `MultiPoint3::bounding_box()`.
    #[must_use]
    pub fn bounding_box(&self) -> BoundingBox3 {
        multi_point3::bounding_box(&self.points)
    }

    /// `MultiPoint3::find_point(const Point&)`: by x and y.
    #[must_use]
    pub fn find_point(&self, point: Point) -> i32 {
        multi_point3::find_point(&self.points, point)
    }

    /// `MultiPoint3::find_point(const Point&, scaled_epsilon)`.
    #[must_use]
    pub fn find_point_eps(&self, point: Point, scaled_epsilon: f64) -> i32 {
        multi_point3::find_point_eps(&self.points, point, scaled_epsilon)
    }

    /// `MultiPoint3::find_point(const Point3&)`: by x, y and z.
    #[must_use]
    pub fn find_point3(&self, point: &Point3) -> i32 {
        multi_point3::find_point3(&self.points, point)
    }

    /// `MultiPoint3::find_point(const Point3&, scaled_epsilon)`.
    #[must_use]
    pub fn find_point3_eps(&self, point: &Point3, scaled_epsilon: f64) -> i32 {
        multi_point3::find_point3_eps(&self.points, point, scaled_epsilon)
    }

    /// `MultiPoint3::remove_duplicate_points()`: the fit is left as it is.
    pub fn remove_duplicate_points(&mut self) -> bool {
        multi_point3::remove_duplicate_points(&mut self.points)
    }

    /// `lines()`: the segments; none below two points.
    #[must_use]
    pub fn lines(&self) -> Lines3 {
        let mut lines = Lines3::new();
        if self.points.len() >= 2 {
            lines.reserve(self.points.len() - 1);
            for w in self.points.windows(2) {
                lines.push(Line3::new(w[0], w[1]));
            }
        }
        lines
    }

    /// `to_polyline()`: x and y of each point; no fit.
    #[must_use]
    pub fn to_polyline(&self) -> Polyline {
        let mut out = Polyline::default();
        out.points.reserve(self.points.len());
        for point in &self.points {
            out.points.push(Point::new(point.x, point.y));
        }
        out
    }

    /// `clip_end(distance)`: removes `distance` (3D) from the end, the new end truncated to
    /// `coord_t`; the moves that start past the cut dropped, the last one ending at the new end (an
    /// arc clipped there in x and y, the end keeping its z, or made a linear move when it cannot
    /// be).
    ///
    /// # Panics
    /// On an empty polyline and a positive distance, which the C++ reads outside the list for.
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
                self.points.push(Point3::cast_from_vec3d(
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
                if back.arc_data.clip_end(self.points[n - 1].to_point()) {
                    // Clipped: the last point moves onto the arc, keeping its z.
                    self.points[n - 1] =
                        Point3::from_point(back.arc_data.end_point, self.points[n - 1].z);
                } else {
                    // Not clipped: a linear move again.
                    back.path_type = EMovePathType::LinearMove;
                }
            }
            back.end_point_index = n.wrapping_sub(1);
        }
    }

    /// `simplify(tolerance)`: the 3D Douglas-Peucker; the fit dropped.
    pub fn simplify(&mut self, tolerance: f64) {
        self.points = multi_point3::douglas_peucker(&self.points, tolerance);
        self.fitting_result.clear();
    }

    /// `simplify_by_fitting_arc(tolerance)`: the arc fitting of the points' x and y, then
    /// Douglas-Peucker on the straight parts ([`arc_fitter::do_arc_fitting_and_simplify`]); every
    /// point comes back with z 0.
    pub fn simplify_by_fitting_arc(&mut self, tolerance: f64) {
        let mut points_2d = to_points(&self.points);
        arc_fitter::do_arc_fitting_and_simplify(
            &mut points_2d,
            &mut self.fitting_result,
            tolerance,
        );
        self.points = to_points3(&points_2d);
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

    /// `split_at_index(index, p1, p2)`: false (the halves untouched) when `index` is past the last
    /// point. At the first point `p1` is cleared (its points only) and given it, `p2` this
    /// polyline; at the last the other way round. Inside, each half takes its points and the moves
    /// on its side, the move across `index` clipped at it (an arc's end or start moved onto the
    /// arc with z 0, or the arc made a linear move when it cannot be clipped).
    ///
    /// # Panics
    /// On an empty polyline, which the C++ reads outside the list for.
    pub fn split_at_index(&self, index: usize, p1: &mut Polyline3, p2: &mut Polyline3) -> bool {
        if index > self.size().wrapping_sub(1) {
            return false;
        }
        if self.points.is_empty() {
            undefined("Polyline3::split_at_index of an empty polyline");
        }
        if index == 0 {
            p1.clear();
            p1.append(self.first_point());
            *p2 = self.clone();
        } else if index == self.size() - 1 {
            p2.clear();
            p2.append(self.last_point());
            *p1 = self.clone();
        } else {
            // The first part.
            p1.clear();
            p1.points.reserve(index + 1);
            p1.points.extend_from_slice(&self.points[..=index]);
            let mut new_endpoint = Point3::default();
            if self.split_fitting_result_before_index(
                index,
                &mut new_endpoint,
                &mut p1.fitting_result,
            ) {
                let last = p1.points.len() - 1;
                p1.points[last] = new_endpoint;
            }

            p2.clear();
            p2.points.reserve(self.size() - index);
            p2.points.extend_from_slice(&self.points[index..]);
            let mut new_startpoint = Point3::default();
            if self.split_fitting_result_after_index(
                index,
                &mut new_startpoint,
                &mut p2.fitting_result,
            ) {
                p2.points[0] = new_startpoint;
            }
        }
        true
    }

    /// `append(const Point3&)`: skipped when it equals the last point (in x, y and z); the fit
    /// extended.
    pub fn append(&mut self, point: Point3) {
        if !self.points.is_empty() && self.last_point() == point {
            return;
        }
        self.points.push(point);
        self.append_fitting_result_after_append_points();
    }

    /// `append(const Polyline3&)`: nothing for an invalid polyline; to an empty one its points and
    /// fit; otherwise its first point (skipped when it equals the last), a linear move over this
    /// one's points when only `src` has a fit, then its other points and its fit, shifted.
    pub fn append_polyline3(&mut self, src: &Polyline3) {
        if !src.is_valid() {
            return;
        }
        if self.points.is_empty() {
            self.points.clone_from(&src.points);
            self.fitting_result.clone_from(&src.fitting_result);
        } else {
            // The first point joins the two, with the fit updated.
            self.append(src.points[0]);
            // A fitted polyline appended to one without a fit: a fake linear move first, so that
            // the fit of `src` is kept.
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
    }

    /// `append_before(const Point3&)`: skipped when it equals the first point. One point and the
    /// new one lose the fit; otherwise reversed, appended and reversed back.
    pub fn append_before(&mut self, point: Point3) {
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

    /// `split_at(Point &point, p1, p2)`: nothing for an empty polyline. At a vertex whose x and y
    /// equal `point`, split there and `point` becomes the end both halves share; else at the
    /// projection onto the nearest segment, `point` (with the z of the half's end) added to both
    /// halves.
    pub fn split_at_point(&self, point: &mut Point, p1: &mut Polyline3, p2: &mut Polyline3) {
        if self.points.is_empty() {
            return;
        }

        // A vertex at the point: the easy case.
        let index = self.find_point(*point);
        if index != -1 {
            self.split_at_index(index as usize, p1, p2);
            *point = if p1.is_valid() {
                p1.last_point().to_point()
            } else {
                p2.first_point().to_point()
            };
            return;
        }

        // The segment to split.
        let mut line_idx = 0;
        let mut p = self.first_point().to_point();
        let mut min = (p - *point).cast_f64().norm();
        let lines = self.lines();
        for (i, line) in lines.iter().enumerate() {
            let p_tmp = point.projection_onto_line(&line.to_line());
            if (p_tmp - *point).cast_f64().norm() < min {
                p = p_tmp;
                min = (p - *point).cast_f64().norm();
                line_idx = i;
            }
        }

        // The nearest point a vertex or not: the split index differs.
        let index = self.find_point(p);
        if index != -1 {
            self.split_at_index(index as usize, p1, p2);
            let z = p1.last_point().z;
            p1.append(Point3::from_point(*point, z));
            let z = p2.first_point().z;
            p2.append_before(Point3::from_point(*point, z));
        } else {
            let mut temp = Polyline3::default();
            self.split_at_index(line_idx, p1, &mut temp);
            let z = p1.last_point().z;
            p1.append(Point3::from_point(*point, z));
            self.split_at_index(line_idx + 1, &mut temp, p2);
            let z = p2.first_point().z;
            p2.append_before(Point3::from_point(*point, z));
        }
    }

    /// `split_at(Point3 &point, p1, p2)`: [`Polyline3::split_at_point`] of its x and y, `point`
    /// keeping its z.
    pub fn split_at_point3(&self, point: &mut Point3, p1: &mut Polyline3, p2: &mut Polyline3) {
        let mut p = point.to_point();
        self.split_at_point(&mut p, p1, p2);
        *point = Point3::from_point(p, point.z);
    }

    /// `split_at_length(length, p1, p2)`: false for an empty polyline or a length outside [0,
    /// `length()`]; at the start or the end within `SCALED_EPSILON`; else at the point that far
    /// along (truncated), at a vertex if it is one (in x, y and z), else added to both halves.
    pub fn split_at_length(&self, length: f64, p1: &mut Polyline3, p2: &mut Polyline3) -> bool {
        if self.points.is_empty() {
            return false;
        }
        if length < 0.0 || length > self.length() {
            return false;
        }

        if length < SCALED_EPSILON {
            p1.clear();
            p1.append(self.first_point());
            *p2 = self.clone();
        } else if orca_crt::fabs(length - self.length()) < SCALED_EPSILON {
            p2.clear();
            p2.append(self.last_point());
            *p1 = self.clone();
        } else {
            // The segment to split.
            let mut line_idx = 0;
            let mut acc_length = 0.0;
            let mut p = self.first_point();
            for l in &self.lines() {
                p = l.b;

                let current_length = l.length();
                if acc_length + current_length >= length {
                    p = point3::lerp(&l.a, &l.b, (length - acc_length) / current_length);
                    break;
                }
                acc_length += current_length;
                line_idx += 1;
            }

            // The point a vertex or not: the split index differs.
            let index = self.find_point3(&p);
            if index != -1 {
                self.split_at_index(index as usize, p1, p2);
            } else {
                let mut temp = Polyline3::default();
                self.split_at_index(line_idx, p1, &mut temp);
                p1.append(p);
                self.split_at_index(line_idx + 1, &mut temp, p2);
                p2.append_before(p);
            }
        }
        true
    }

    /// `split_fitting_result_before_index(index, new_endpoint, data)`: `data` cleared and
    /// `new_endpoint` set to `points[index]`; false for no fit. Else the moves that start before
    /// `index`, the last ending at `index` (an arc across it clipped there, the new end point its
    /// end with z 0, or made a linear move).
    fn split_fitting_result_before_index(
        &self,
        index: usize,
        new_endpoint: &mut Point3,
        data: &mut Vec<PathFittingData>,
    ) -> bool {
        data.clear();
        *new_endpoint = self.points[index];
        if self.fitting_result.is_empty() {
            return false;
        }
        data.reserve(self.fitting_result.len());
        // The moves before the index.
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
                if back.arc_data.clip_end(self.points[index].to_point()) {
                    *new_endpoint = Point3::from_point(back.arc_data.end_point, 0);
                } else {
                    back.path_type = EMovePathType::LinearMove;
                }
            }
            back.end_point_index = index;
        }
        data.shrink_to_fit();
        true
    }

    /// `split_fitting_result_after_index(index, new_startpoint, data)`: `data` cleared and
    /// `new_startpoint` set to `points[index]`; false for no fit. Else the moves that end after
    /// `index`, shifted back by it, the first starting at 0 (an arc across it clipped there, the
    /// new start point its start with z 0, or made a linear move).
    fn split_fitting_result_after_index(
        &self,
        index: usize,
        new_startpoint: &mut Point3,
        data: &mut Vec<PathFittingData>,
    ) -> bool {
        data.clear();
        *new_startpoint = self.points[index];
        if self.fitting_result.is_empty() {
            return false;
        }
        data.reserve(self.fitting_result.len());
        for d in &self.fitting_result {
            if d.end_point_index > index {
                data.push(*d);
            }
        }
        for (i, d) in data.iter_mut().enumerate() {
            if i != 0 {
                d.start_point_index = d.start_point_index.wrapping_sub(index);
                d.end_point_index = d.end_point_index.wrapping_sub(index);
            } else {
                d.end_point_index = d.end_point_index.wrapping_sub(index);
                // An arc across the index: clipped there, for a new start point.
                if d.is_arc_move() && d.start_point_index < index {
                    if d.arc_data.clip_start(self.points[index].to_point()) {
                        *new_startpoint = Point3::from_point(d.arc_data.start_point, 0);
                    } else {
                        d.path_type = EMovePathType::LinearMove;
                    }
                }
                d.start_point_index = 0;
            }
        }
        data.shrink_to_fit();
        true
    }

    /// `append_fitting_result_after_append_points()`: after points were appended, the last linear
    /// move extended to the new last point, or a linear move added after an arc.
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
    fn append_fitting_result_after_append_polyline(&mut self, src: &Polyline3) {
        let Some(back) = self.fitting_result.last() else {
            return;
        };
        if src.fitting_result.is_empty() {
            // `src` has no fit: a linear move.
            let new_start = back.end_point_index;
            let new_end = self.size().wrapping_sub(1);
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
}

/// `to_polyline(const Points&)`: `Polyline(points)`, no fit.
#[must_use]
pub fn to_polyline(points: &[Point]) -> Polyline {
    Polyline::new(points.to_vec())
}

/// `to_polyline(const Points3&)`: `Polyline3(points)`, no fit.
#[must_use]
pub fn to_polyline3(points: &[Point3]) -> Polyline3 {
    Polyline3::new(points.to_vec())
}

/// `foot_pt(const Points3 &polyline, const Point3 &pt)`: the index of the segment whose
/// projection ([`Point3::projection_onto_line3`]) is nearest `pt` in 3D (the first of equals) and
/// that projection; `(-1, (0, 0, 0))` below two points.
#[must_use]
pub fn foot_pt(polyline: &[Point3], pt: &Point3) -> (i32, Point3) {
    if polyline.len() < 2 {
        return (-1, Point3::new(0, 0, 0));
    }
    let mut d2_min = f64::MAX;
    let mut foot_pt_min = Point3::default();
    let mut prev = polyline[0];
    let mut it_proj = 0;
    for (i, &it) in polyline.iter().enumerate().skip(1) {
        let foot_pt = pt.projection_onto_line3(&Line3::new(prev, it));
        let d2 = (foot_pt - *pt).cast_f64().squared_norm();
        if d2 < d2_min {
            d2_min = d2;
            foot_pt_min = foot_pt;
            it_proj = i;
        }
        prev = it;
    }
    (it_proj as i32 - 1, foot_pt_min)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_keeps_the_fit() {
        let mut pl = Polyline3::new(vec![
            Point3::new(0, 0, 1),
            Point3::new(100, 0, 2),
            Point3::new(200, 0, 3),
        ]);
        pl.fitting_result.push(PathFittingData::linear(0, 2));
        pl.clear();
        assert!(pl.points.is_empty());
        assert_eq!(pl.fitting_result.len(), 1);
    }

    #[test]
    fn split_at_a_length_in_the_middle_of_a_segment() {
        let pl = Polyline3::new(vec![Point3::new(0, 0, 0), Point3::new(1000, 0, 100)]);
        let (mut a, mut b) = (Polyline3::default(), Polyline3::default());
        assert!(pl.split_at_length(pl.length() / 2.0, &mut a, &mut b));
        assert_eq!(
            a.points,
            vec![Point3::new(0, 0, 0), Point3::new(500, 0, 50)]
        );
        assert_eq!(
            b.points,
            vec![Point3::new(500, 0, 50), Point3::new(1000, 0, 100)]
        );
    }

    #[test]
    fn foot_of_a_point() {
        let pts = vec![
            Point3::new(0, 0, 0),
            Point3::new(100, 0, 0),
            Point3::new(100, 100, 50),
        ];
        assert_eq!(
            foot_pt(&pts, &Point3::new(150, 50, 0)),
            (1, Point3::new(100, 50, 22))
        );
        assert_eq!(
            foot_pt(&pts[..1], &Point3::new(1, 1, 1)),
            (-1, Point3::new(0, 0, 0))
        );
    }
}
