// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: the classes ExtrusionLoop and ExtrusionLoopSloped of
// src/libslic3r/ExtrusionEntity.hpp (443-549) and their definitions in ExtrusionEntity.cpp
// (16, 141-581), with prev_idx_modulo and next_idx_modulo of src/libslic3r/Utils.hpp (375-389),
// of OrcaSlicer v2.4.2, translated on 2026-11-13 (orca-o3-layer-1). Copyright the OrcaSlicer
// authors, the BambuStudio authors (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under
// the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! [`ExtrusionLoop`]: a closed extrusion made of paths, the last ending where the first starts,
//! and [`ExtrusionLoopSloped`], a loop whose seam is a scarf: slopes at its start (`starts`) and
//! its end (`ends`) around its flat paths.
//!
//! A loop's copy is the compiler's: it copies `inset_idx` (the paths it holds come out -1, as
//! every copied path does). Its ends are its first path's first point, both of them (the C++
//! asserts the loop closes, which its release build does not check). The splits:
//! [`ExtrusionLoop::split_at_vertex`] makes a loop start at the vertex within `scaled_epsilon` of
//! a point (by changing the order of its points when it has one path, else by rebuilding its paths
//! from the one split there); [`ExtrusionLoop::split_at`] splits the path nearest the point at its
//! foot (snapped to the segment's end within `scaled_epsilon`), the halves made new from the path's
//! role, flow, width, height and `z_contoured` only, then splits at that vertex.
//!
//! `is_smooth` follows the C++ number types: the edge lengths are `float`s (the closing edge at
//! least 0.1 mm, as a `double` before it is stored) and the arm lengths too, each compared with
//! the `double` arm length after promotion. A sloped loop's methods are the loop's (they see its
//! flat `paths` only), but for its own `clip_end`, which clips the end slopes, and `clip_front`,
//! which clips the start slopes: for an inner wall (`erPerimeter`) by 0.4 times the slopes'
//! length, scaled, and while the distance it was given (not the one left) is positive. Its copy
//! through `ExtrusionEntity::clone()` is a plain loop: the C++ class does not override `clone`.
//!
//! The C++ reads outside its lists for a loop without paths, with an empty path, or whose nearest
//! path has fewer than two points; these stop with [`not_ported`] instead.

use orca_geom::point::{Point, Points, scale_, scaled_f64, unscale_, unscale_point};
use orca_geom::point3::{Line3, Point3, to_points};
use orca_geom::polygon::{Polygon, Polygons};
use orca_geom::polyline::{Polyline, Polylines};
use orca_geom::polyline3::{Polyline3, foot_pt};
use orca_index::stop::not_ported;

use crate::cpp_vec;
use crate::flow::FlowError;
use crate::multi_path::NO_PATHS;
use crate::path::{
    EMPTY_PATH, ExtrusionPath, ExtrusionPathSloped, ExtrusionPaths, Slope, is_approx, lerp,
};
use crate::role::{ExtrusionLoopRole, ExtrusionRole, is_bridge};

/// `slope_inner_outer_wall_gap`: the share of an inner wall's slopes `clip_front` clips.
const SLOPE_INNER_OUTER_WALL_GAP: f64 = 0.4;

/// `prev_idx_modulo(idx, count)` of `Utils.hpp`: `idx - 1`, from 0 to `count - 1`.
#[must_use]
pub fn prev_idx_modulo(idx: usize, count: usize) -> usize {
    let idx = if idx == 0 { count } else { idx };
    idx.wrapping_sub(1)
}

/// `next_idx_modulo(idx, count)` of `Utils.hpp`: `idx + 1`, from `count - 1` to 0.
#[must_use]
pub fn next_idx_modulo(idx: usize, count: usize) -> usize {
    let idx = idx.wrapping_add(1);
    if idx == count { 0 } else { idx }
}

/// `ExtrusionLoop::ClosestPathPoint`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClosestPathPoint {
    /// `path_idx`.
    pub path_idx: usize,
    /// `segment_idx`: the segment's first point; `size_t(-1)` for a path below two points.
    pub segment_idx: usize,
    /// `foot_pt`.
    pub foot_pt: Point,
}

/// `ExtrusionLoop`.
#[derive(Clone, Debug)]
pub struct ExtrusionLoop {
    /// `paths`.
    pub paths: ExtrusionPaths,
    /// `ExtrusionEntity::inset_idx`.
    pub inset_idx: i32,
    /// `m_loop_role`.
    loop_role: ExtrusionLoopRole,
}

impl Default for ExtrusionLoop {
    /// `ExtrusionLoop()`: no paths, `elrDefault`.
    fn default() -> Self {
        Self::new(ExtrusionLoopRole::DEFAULT)
    }
}

impl ExtrusionLoop {
    /// `ExtrusionLoop(role)`: no paths.
    #[must_use]
    pub fn new(role: ExtrusionLoopRole) -> Self {
        Self {
            paths: ExtrusionPaths::new(),
            inset_idx: -1,
            loop_role: role,
        }
    }

    /// `ExtrusionLoop(ExtrusionPaths &&paths, role)`: the paths moved in, as they are.
    #[must_use]
    pub fn from_paths(paths: ExtrusionPaths, role: ExtrusionLoopRole) -> Self {
        Self {
            paths,
            inset_idx: -1,
            loop_role: role,
        }
    }

    /// `ExtrusionLoop(const ExtrusionPaths &paths, role)`: the paths copied.
    #[must_use]
    pub fn from_paths_copied(paths: &[ExtrusionPath], role: ExtrusionLoopRole) -> Self {
        Self::from_paths(paths.to_vec(), role)
    }

    /// `ExtrusionLoop(const ExtrusionPath &path, role)` (and its `&&` form, which copies too).
    #[must_use]
    pub fn from_path(path: &ExtrusionPath, role: ExtrusionLoopRole) -> Self {
        let mut out = Self::new(role);
        cpp_vec::push(&mut out.paths, path.clone());
        out
    }

    /// `is_loop()`: true.
    #[must_use]
    pub fn is_loop(&self) -> bool {
        true
    }

    /// `can_reverse()`: false.
    #[must_use]
    pub fn can_reverse(&self) -> bool {
        false
    }

    /// `make_clockwise()`: reversed when its polygon is counter-clockwise; true then.
    ///
    /// # Panics
    /// As [`ExtrusionLoop::polygon`].
    pub fn make_clockwise(&mut self) -> bool {
        let was_ccw = self.polygon().is_counter_clockwise();
        if was_ccw {
            self.reverse();
        }
        was_ccw
    }

    /// `make_counter_clockwise()`: reversed when its polygon is clockwise; true then.
    ///
    /// # Panics
    /// As [`ExtrusionLoop::polygon`].
    pub fn make_counter_clockwise(&mut self) -> bool {
        let was_cw = self.polygon().is_clockwise();
        if was_cw {
            self.reverse();
        }
        was_cw
    }

    /// `is_clockwise()`.
    ///
    /// # Panics
    /// As [`ExtrusionLoop::polygon`].
    #[must_use]
    pub fn is_clockwise(&self) -> bool {
        self.polygon().is_clockwise()
    }

    /// `is_counter_clockwise()`.
    ///
    /// # Panics
    /// As [`ExtrusionLoop::polygon`].
    #[must_use]
    pub fn is_counter_clockwise(&self) -> bool {
        self.polygon().is_counter_clockwise()
    }

    /// `reverse()`: each path reversed, then their order.
    pub fn reverse(&mut self) {
        for path in &mut self.paths {
            path.reverse();
        }
        cpp_vec::reverse(&mut self.paths);
    }

    /// `first_point()`.
    ///
    /// # Panics
    /// Without paths, or when the first path is empty.
    #[must_use]
    pub fn first_point(&self) -> Point {
        self.first_point3().to_point()
    }

    /// `first_point3()`.
    ///
    /// # Panics
    /// As [`ExtrusionLoop::first_point`].
    #[must_use]
    pub fn first_point3(&self) -> Point3 {
        let Some(front) = self.paths.first() else {
            not_ported("ExtrusionLoop::first_point", NO_PATHS)
        };
        match front.polyline.points.first() {
            Some(&p) => p,
            None => not_ported("ExtrusionLoop::first_point", EMPTY_PATH),
        }
    }

    /// `last_point()`: the first point.
    ///
    /// # Panics
    /// As [`ExtrusionLoop::first_point`].
    #[must_use]
    pub fn last_point(&self) -> Point {
        self.first_point()
    }

    /// `last_point3()`: the first point.
    ///
    /// # Panics
    /// As [`ExtrusionLoop::first_point`].
    #[must_use]
    pub fn last_point3(&self) -> Point3 {
        self.first_point3()
    }

    /// `polygon()`: each path's points but its last, in x and y.
    ///
    /// # Panics
    /// On an empty path, before whose start the C++ reads.
    #[must_use]
    pub fn polygon(&self) -> Polygon {
        let mut polygon = Polygon::default();
        for path in &self.paths {
            let Some((_, init)) = path.polyline.points.split_last() else {
                not_ported("ExtrusionLoop::polygon", EMPTY_PATH)
            };
            for point in init {
                polygon.points.push(point.to_point());
            }
        }
        polygon
    }

    /// `length()`: the paths' lengths summed in order.
    #[must_use]
    pub fn length(&self) -> f64 {
        let mut len = 0.;
        for path in &self.paths {
            len += path.polyline.length();
        }
        len
    }

    /// `split_at_vertex(point, scaled_epsilon)`: at the first path with a vertex within
    /// `scaled_epsilon` of `point`, the loop made to start there; false when no path has one.
    pub fn split_at_vertex(&mut self, point: &Point, scaled_epsilon: f64) -> bool {
        for path_i in 0..self.paths.len() {
            let idx = self.paths[path_i]
                .polyline
                .find_point_eps(*point, scaled_epsilon);
            if idx == -1 {
                continue;
            }
            #[allow(clippy::cast_sign_loss)]
            let idx = idx as usize;
            if self.paths.len() == 1 {
                // Just the order of the points changes.
                let mut p1 = Polyline3::default();
                let mut p2 = Polyline3::default();
                self.paths[path_i]
                    .polyline
                    .split_at_index(idx, &mut p1, &mut p2);
                if p1.is_valid() && p2.is_valid() {
                    p2.append_polyline3(&p1);
                    let polyline = &mut self.paths[path_i].polyline;
                    core::mem::swap(&mut polyline.points, &mut p2.points);
                    core::mem::swap(&mut polyline.fitting_result, &mut p2.fitting_result);
                }
            } else {
                // The new list: the second half of this path, the paths after it, the paths
                // before it, then the first half.
                let path = &self.paths[path_i];
                let mut p1 = Polyline3::default();
                let mut p2 = Polyline3::default();
                path.polyline.split_at_index(idx, &mut p1, &mut p2);
                let mut new_paths = ExtrusionPaths::with_capacity(self.paths.len() + 1);
                {
                    let mut p = path.clone();
                    core::mem::swap(&mut p.polyline.points, &mut p2.points);
                    core::mem::swap(&mut p.polyline.fitting_result, &mut p2.fitting_result);
                    if p.polyline.is_valid() {
                        cpp_vec::push(&mut new_paths, p);
                    }
                }
                for after in &self.paths[path_i + 1..] {
                    cpp_vec::push(&mut new_paths, after.clone());
                }
                for before in &self.paths[..path_i] {
                    cpp_vec::push(&mut new_paths, before.clone());
                }
                {
                    let mut p = path.clone();
                    core::mem::swap(&mut p.polyline.points, &mut p1.points);
                    core::mem::swap(&mut p.polyline.fitting_result, &mut p1.fitting_result);
                    if p.polyline.is_valid() {
                        cpp_vec::push(&mut new_paths, p);
                    }
                }
                core::mem::swap(&mut self.paths, &mut new_paths);
            }
            return true;
        }
        false
    }

    /// `get_closest_path_and_point(point, prefer_non_overhang)`: the path whose foot (in x and y)
    /// is nearest `point`, the first of equals; with `prefer_non_overhang`, the nearest of the
    /// paths that are not bridges when there is one.
    #[must_use]
    pub fn get_closest_path_and_point(
        &self,
        point: &Point,
        prefer_non_overhang: bool,
    ) -> ClosestPathPoint {
        let mut out = ClosestPathPoint::default();
        let mut min2 = f64::MAX;
        let mut best_non_overhang = ClosestPathPoint::default();
        let mut min2_non_overhang = f64::MAX;
        for (path_idx, path) in self.paths.iter().enumerate() {
            let (segment, foot) = foot_pt(&path.polyline.points, &Point3::from_point(*point, 0));
            let foot_pt_2d = Point::new(foot.x, foot.y);
            let d2 = (foot_pt_2d - *point).cast_f64().squared_norm();
            // `size_t(int)`: -1 becomes `SIZE_MAX`.
            #[allow(clippy::cast_sign_loss)]
            let segment_idx = segment as isize as usize;
            if d2 < min2 {
                out.foot_pt = foot_pt_2d;
                out.path_idx = path_idx;
                out.segment_idx = segment_idx;
                min2 = d2;
            }
            if prefer_non_overhang && !is_bridge(path.role()) && d2 < min2_non_overhang {
                best_non_overhang.foot_pt = foot_pt_2d;
                best_non_overhang.path_idx = path_idx;
                best_non_overhang.segment_idx = segment_idx;
                min2_non_overhang = d2;
            }
        }
        if prefer_non_overhang && min2_non_overhang != f64::MAX {
            // The non-overhang point only when there is one.
            out = best_non_overhang;
        }
        out
    }

    /// `split_at(point, prefer_non_overhang, scaled_epsilon)`: nothing without paths; else the
    /// nearest path split at its foot (snapped to the nearer end of its segment within
    /// `scaled_epsilon`), then [`ExtrusionLoop::split_at_vertex`] at the split point with the
    /// default epsilon.
    ///
    /// # Panics
    /// When the nearest path has fewer than two points: the C++ reads outside its points.
    pub fn split_at(&mut self, point: &Point, prefer_non_overhang: bool, scaled_epsilon: f64) {
        if self.paths.is_empty() {
            return;
        }

        let ClosestPathPoint {
            path_idx,
            segment_idx,
            foot_pt: mut p,
        } = self.get_closest_path_and_point(point, prefer_non_overhang);

        // Snap p to the start or end of segment_idx if closer than scaled_epsilon.
        {
            let points = &self.paths[path_idx].polyline.points;
            if segment_idx
                .checked_add(1)
                .is_none_or(|next| next >= points.len())
            {
                not_ported(
                    "ExtrusionLoop::split_at",
                    "the nearest path has fewer than two points (the C++ reads outside its points)",
                );
            }
            let p1_2d = points[segment_idx].to_point();
            let p2_2d = points[segment_idx + 1].to_point();
            let d2_1 = (*point - p1_2d).cast_f64().squared_norm();
            let d2_2 = (*point - p2_2d).cast_f64().squared_norm();
            let thr2 = scaled_epsilon * scaled_epsilon;
            if d2_1 < d2_2 {
                if d2_1 < thr2 {
                    p = p1_2d;
                }
            } else if d2_2 < thr2 {
                p = p2_2d;
            }
        }

        // Now split path_idx in two parts.
        let path = &self.paths[path_idx];
        let mut p1 = ExtrusionPath::new(path.role(), path.mm3_per_mm, path.width, path.height);
        let mut p2 = ExtrusionPath::new(path.role(), path.mm3_per_mm, path.width, path.height);
        p1.z_contoured = path.z_contoured;
        p2.z_contoured = path.z_contoured;
        path.polyline
            .split_at_point(&mut p, &mut p1.polyline, &mut p2.polyline);

        if self.paths.len() == 1 {
            let front = &mut self.paths[0].polyline;
            if !p1.polyline.is_valid() {
                core::mem::swap(&mut front.points, &mut p2.polyline.points);
                core::mem::swap(&mut front.fitting_result, &mut p2.polyline.fitting_result);
            } else if !p2.polyline.is_valid() {
                core::mem::swap(&mut front.points, &mut p1.polyline.points);
                core::mem::swap(&mut front.fitting_result, &mut p1.polyline.fitting_result);
            } else {
                p2.polyline.append_polyline3(&p1.polyline);
                core::mem::swap(&mut front.points, &mut p2.polyline.points);
                core::mem::swap(&mut front.fitting_result, &mut p2.polyline.fitting_result);
            }
        } else {
            // Install the two paths.
            cpp_vec::erase(&mut self.paths, path_idx);
            if p2.polyline.is_valid() {
                cpp_vec::insert(&mut self.paths, path_idx, p2);
            }
            if p1.polyline.is_valid() {
                cpp_vec::insert(&mut self.paths, path_idx, p1);
            }
        }

        // Split at the new vertex.
        self.split_at_vertex(&p, scaled_f64(0.001));
    }

    /// `clip_end(distance, paths)`: `paths` assigned this loop's paths (`cpp_vec::assign`), then
    /// `distance` taken off their end, whole paths popped while they are no longer than what is
    /// left.
    pub fn clip_end(&self, mut distance: f64, paths: &mut ExtrusionPaths) {
        cpp_vec::assign(paths, &self.paths);
        while distance > 0. {
            let Some(last) = paths.last_mut() else {
                break;
            };
            let len = last.length();
            if len <= distance {
                paths.pop();
                distance -= len;
            } else {
                last.polyline.clip_end(distance);
                break;
            }
        }
    }

    /// `has_overhang_point(point)`: whether the first path with a vertex at `point` is a bridge
    /// and the vertex is not one of its ends.
    #[must_use]
    pub fn has_overhang_point(&self, point: &Point) -> bool {
        for path in &self.paths {
            let pos = path.polyline.find_point(*point);
            if pos != -1 {
                // The point belongs to this path: an overhang only if it is not an end point.
                #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
                let last = path.polyline.points.len() as i32 - 1;
                return is_bridge(path.role()) && pos > 0 && pos != last;
            }
        }
        false
    }

    /// `role()`: the first path's; `erNone` without paths.
    #[must_use]
    pub fn role(&self) -> ExtrusionRole {
        match self.paths.first() {
            Some(front) => front.role(),
            None => ExtrusionRole::None,
        }
    }

    /// `loop_role()`.
    #[must_use]
    pub fn loop_role(&self) -> ExtrusionLoopRole {
        self.loop_role
    }

    /// `set_loop_role(role)`.
    pub fn set_loop_role(&mut self, role: ExtrusionLoopRole) {
        self.loop_role = role;
    }

    /// `polygons_covered_by_width(out, scaled_epsilon)`: each path's, appended in order.
    pub fn polygons_covered_by_width(&self, out: &mut Polygons, scaled_epsilon: f32) {
        for path in &self.paths {
            path.polygons_covered_by_width(out, scaled_epsilon);
        }
    }

    /// `polygons_covered_by_spacing(out, scaled_epsilon)`: each path's, appended in order.
    ///
    /// # Errors
    /// The first path's [`FlowError`]; `out` keeps the paths before it.
    pub fn polygons_covered_by_spacing(
        &self,
        out: &mut Polygons,
        scaled_epsilon: f32,
    ) -> Result<(), FlowError> {
        for path in &self.paths {
            path.polygons_covered_by_spacing(out, scaled_epsilon)?;
        }
        Ok(())
    }

    /// `min_mm3_per_mm()`: the least of the paths' flows (`DBL_MAX` without paths).
    #[must_use]
    pub fn min_mm3_per_mm(&self) -> f64 {
        let mut min_mm3_per_mm = f64::MAX;
        for path in &self.paths {
            min_mm3_per_mm = orca_stl::minmax::min(min_mm3_per_mm, path.mm3_per_mm);
        }
        min_mm3_per_mm
    }

    /// `as_polyline()`: empty without paths or length; else the polygon opened at its first
    /// point (which it repeats at the end).
    ///
    /// # Panics
    /// As [`ExtrusionLoop::polygon`].
    #[must_use]
    pub fn as_polyline(&self) -> Polyline {
        if self.paths.is_empty() || self.length() <= 0. {
            return Polyline::default();
        }
        self.polygon().split_at_first_point()
    }

    /// `collect_polylines(dst)`: [`ExtrusionLoop::as_polyline`], unless empty.
    ///
    /// # Panics
    /// As [`ExtrusionLoop::polygon`].
    pub fn collect_polylines(&self, dst: &mut Polylines) {
        let pl = self.as_polyline();
        if !pl.is_empty() {
            dst.push(pl);
        }
    }

    /// `collect_points(dst)`: every path's points' x and y.
    pub fn collect_points(&self, dst: &mut Points) {
        for path in &self.paths {
            dst.extend(to_points(&path.polyline.points));
        }
    }

    /// `total_volume()`: the paths' volumes summed in order.
    #[must_use]
    pub fn total_volume(&self) -> f64 {
        let mut volume = 0.;
        for path in &self.paths {
            volume += path.total_volume();
        }
        volume
    }

    /// `is_smooth(angle_threshold, min_arm_length)`: false when the angle at a vertex, between
    /// arms at least `min_arm_length` (mm) long, is within `angle_threshold` (radians) of
    /// straight back; the defaults are 0.174 and 0.025.
    ///
    /// # Panics
    /// On a loop with no points: the C++ counts down from `size() - 1` past its lengths.
    #[must_use]
    pub fn is_smooth(&self, angle_threshold: f64, min_arm_length: f64) -> bool {
        // Go through all the points in the loop and check the angle between AB and BC.
        let mut idx_prev: usize = 0;
        let mut idx_curr: usize = 0;
        let mut idx_next: usize = 0;

        let mut distance_to_prev: f32 = 0.;
        let mut distance_to_next: f32 = 0.;

        let polygon = self.polygon();
        let points = &polygon.points;
        if points.is_empty() {
            not_ported(
                "ExtrusionLoop::is_smooth",
                "a loop with no points (the C++ reads past its lengths)",
            );
        }

        let mut lengths: Vec<f32> = Vec::new();
        for point_idx in 0..points.len() - 1 {
            #[allow(clippy::cast_possible_truncation)]
            lengths.push(
                (unscale_point(points[point_idx]) - unscale_point(points[point_idx + 1])).norm()
                    as f32,
            );
        }
        #[allow(clippy::cast_possible_truncation)]
        lengths.push(orca_stl::minmax::max(
            (unscale_point(points[0]) - unscale_point(points[points.len() - 1])).norm(),
            0.1,
        ) as f32);

        // Push idx_prev far enough back as initialization.
        while f64::from(distance_to_prev) < min_arm_length {
            idx_prev = prev_idx_modulo(idx_prev, points.len());
            distance_to_prev += lengths[idx_prev];
        }

        for _ in 0..points.len() {
            // Pull idx_prev to current as much as possible, keeping min_arm_length.
            while f64::from(distance_to_prev - lengths[idx_prev]) > min_arm_length {
                distance_to_prev -= lengths[idx_prev];
                idx_prev = next_idx_modulo(idx_prev, points.len());
            }

            // Push idx_next forward as far as needed.
            while f64::from(distance_to_next) < min_arm_length {
                distance_to_next += lengths[idx_next];
                idx_next = next_idx_modulo(idx_next, points.len());
            }

            // The angle between idx_prev, idx_curr and idx_next.
            let p0 = points[idx_prev];
            let p1 = points[idx_curr];
            let p2 = points[idx_next];
            let a = orca_geom::point::angle((p0 - p1).cast_f64(), (p2 - p1).cast_f64());
            if if a > 0. {
                a < angle_threshold
            } else {
                a > -angle_threshold
            } {
                return false;
            }

            // Increase idx_curr by one.
            let curr_distance = lengths[idx_curr];
            idx_curr += 1;
            distance_to_prev += curr_distance;
            distance_to_next -= curr_distance;
        }

        true
    }
}

/// `ExtrusionLoopSloped`: a loop whose seam is a scarf, the slopes at its start in `starts`, its
/// flat paths in `paths` (of `base`) and the slopes at its end in `ends`.
#[derive(Clone, Debug, Default)]
pub struct ExtrusionLoopSloped {
    /// The `ExtrusionLoop` base.
    pub base: ExtrusionLoop,
    /// `starts`.
    pub starts: Vec<ExtrusionPathSloped>,
    /// `ends`.
    pub ends: Vec<ExtrusionPathSloped>,
}

impl ExtrusionLoopSloped {
    /// `ExtrusionLoopSloped(original_paths, seam_gap, slope_min_length, slope_max_segment_length,
    /// start_slope_ratio, role)`: the first `slope_min_length` (mm) of the paths made start
    /// slopes rising from `start_slope_ratio` to 1, each cut into segments no longer than
    /// `slope_max_segment_length` (scaled), with an end slope over each, less `seam_gap` (scaled)
    /// at the end of the one that reaches 1; the rest the flat paths.
    #[must_use]
    pub fn new(
        original_paths: &[ExtrusionPath],
        seam_gap: f64,
        slope_min_length: f64,
        slope_max_segment_length: f64,
        start_slope_ratio: f64,
        role: ExtrusionLoopRole,
    ) -> Self {
        let mut out = Self {
            base: ExtrusionLoop::new(role),
            starts: Vec::new(),
            ends: Vec::new(),
        };

        let mut remaining_length = slope_min_length;
        let mut path = 0;
        let mut start_ratio = start_slope_ratio;
        while path < original_paths.len() && remaining_length > 0. {
            let current = &original_paths[path];
            let path_len = unscale_(current.length());
            if path_len > remaining_length {
                // The path split into its slope and flat parts.
                let mut slope_path = Polyline3::default();
                let mut flat_path = Polyline3::default();
                current.polyline.split_at_length(
                    scale_(remaining_length),
                    &mut slope_path,
                    &mut flat_path,
                );

                out.add_slope(
                    current,
                    &slope_path,
                    start_ratio,
                    1.,
                    slope_max_segment_length,
                    seam_gap,
                );
                start_ratio = 1.;

                cpp_vec::push(
                    &mut out.base.paths,
                    ExtrusionPath::from_polyline(flat_path, current),
                );
                remaining_length = 0.;
            } else {
                remaining_length -= path_len;
                let end_ratio = lerp(1.0, start_slope_ratio, remaining_length / slope_min_length);
                out.add_slope(
                    current,
                    &current.polyline,
                    start_ratio,
                    end_ratio,
                    slope_max_segment_length,
                    seam_gap,
                );
                start_ratio = end_ratio;
            }
            path += 1;
        }

        // The remaining flat paths.
        cpp_vec::extend_from_slice(&mut out.base.paths, &original_paths[path..]);
        out
    }

    /// The constructor's `add_slop`: a start slope over `poly` (cut so that no segment is longer
    /// than `slope_max_segment_length`) from `ratio_begin` to `ratio_end`, and an end slope over
    /// it less `seam_gap` at its end when it reaches 1; nothing for an empty polyline.
    fn add_slope(
        &mut self,
        path: &ExtrusionPath,
        poly: &Polyline3,
        ratio_begin: f64,
        mut ratio_end: f64,
        slope_max_segment_length: f64,
        seam_gap: f64,
    ) {
        if poly.is_empty() {
            return;
        }

        // Ensure slope_max_segment_length.
        let mut detailed_poly = Polyline3::default();
        detailed_poly.append(poly.first_point());
        for line in &poly.lines() {
            handle_line(line, slope_max_segment_length, &mut detailed_poly);
        }

        cpp_vec::push(
            &mut self.starts,
            ExtrusionPathSloped::from_polyline(
                detailed_poly.clone(),
                path,
                Slope {
                    z_ratio: ratio_begin,
                    e_ratio: ratio_begin,
                },
                Slope {
                    z_ratio: ratio_end,
                    e_ratio: ratio_end,
                },
            ),
        );

        if is_approx(ratio_end, 1.) && seam_gap > 0. {
            // Remove the segments that have no extrusion.
            let seg_length = detailed_poly.length();
            if seg_length > seam_gap {
                // Split the segment and remove the last seam_gap bit.
                let orig = detailed_poly.clone();
                let mut tmp = Polyline3::default();
                orig.split_at_length(seg_length - seam_gap, &mut detailed_poly, &mut tmp);

                ratio_end = lerp(ratio_begin, ratio_end, (seg_length - seam_gap) / seg_length);
            } else {
                // Remove the entire segment.
                detailed_poly.clear();
            }
        }
        if !detailed_poly.is_empty() {
            cpp_vec::push(
                &mut self.ends,
                ExtrusionPathSloped::from_polyline(
                    detailed_poly,
                    path,
                    Slope {
                        z_ratio: 1.,
                        e_ratio: 1. - ratio_begin,
                    },
                    Slope {
                        z_ratio: 1.,
                        e_ratio: 1. - ratio_end,
                    },
                ),
            );
        }
    }

    /// `get_all_paths()`: the start slopes, the flat paths and the end slopes, in that order.
    #[must_use]
    pub fn get_all_paths(&self) -> Vec<&ExtrusionPath> {
        let mut r = Vec::with_capacity(self.starts.len() + self.base.paths.len() + self.ends.len());
        for p in &self.starts {
            r.push(&p.path);
        }
        for p in &self.base.paths {
            r.push(p);
        }
        for p in &self.ends {
            r.push(&p.path);
        }
        r
    }

    /// `clip_slope(distance, inter_perimeter)`: [`ExtrusionLoopSloped::clip_end`] by `distance`,
    /// then [`ExtrusionLoopSloped::clip_front`] by twice it (`inter_perimeter` is not read).
    pub fn clip_slope(&mut self, distance: f64, _inter_perimeter: bool) {
        self.clip_end(distance);
        self.clip_front(distance * 2.);
    }

    /// `clip_end(distance)`: `distance` taken off the end slopes, whole slopes popped while they
    /// are no longer than what is left.
    pub fn clip_end(&mut self, distance: f64) {
        let mut clip_dist = distance;
        while clip_dist > 0. {
            let Some(last_path) = self.ends.last_mut() else {
                break;
            };
            let len = last_path.path.length();
            if len <= clip_dist {
                self.ends.pop();
                clip_dist -= len;
            } else {
                last_path.path.polyline.clip_end(clip_dist);
                break;
            }
        }
    }

    /// `clip_front(distance)`: the start slopes clipped from the front by `distance`, or for an
    /// inner wall by 0.4 times the slopes' length (scaled); whole slopes erased while they are no
    /// longer than what is left, and the loop runs while `distance` itself is positive.
    pub fn clip_front(&mut self, distance: f64) {
        let mut clip_dist = distance;
        if self.base.role() == ExtrusionRole::Perimeter {
            clip_dist = scale_(self.slope_path_length()) * SLOPE_INNER_OUTER_WALL_GAP;
        }

        while distance > 0. && !self.starts.is_empty() {
            let len = self.starts[0].path.length();
            if len <= clip_dist {
                cpp_vec::erase(&mut self.starts, 0);
                clip_dist -= len;
            } else {
                let first_path = &mut self.starts[0].path.polyline;
                first_path.reverse();
                first_path.clip_end(clip_dist);
                first_path.reverse();
                break;
            }
        }
    }

    /// `slope_path_length()`: the start slopes' lengths, unscaled, summed in order.
    #[must_use]
    pub fn slope_path_length(&self) -> f64 {
        let mut total_length = 0.0;
        for start_ep in &self.starts {
            total_length += unscale_(start_ep.path.length());
        }
        total_length
    }
}

/// The constructor's `handle_line`: `line.b` appended when the line is no longer than
/// `slope_max_segment_length`, else each half in turn.
fn handle_line(line: &Line3, slope_max_segment_length: f64, detailed_poly: &mut Polyline3) {
    if line.length() <= slope_max_segment_length {
        detailed_poly.append(line.b);
    } else {
        // The left half, then the right half.
        handle_line(
            &Line3::new(line.a, line.midpoint()),
            slope_max_segment_length,
            detailed_poly,
        );
        handle_line(
            &Line3::new(line.midpoint(), line.b),
            slope_max_segment_length,
            detailed_poly,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square_loop() -> ExtrusionLoop {
        let mut path = ExtrusionPath::new(ExtrusionRole::Perimeter, 0.1, 0.4, 0.2);
        path.polyline = Polyline3::new(vec![
            Point3::new(0, 0, 0),
            Point3::new(1_000_000, 0, 0),
            Point3::new(1_000_000, 1_000_000, 0),
            Point3::new(0, 1_000_000, 0),
            Point3::new(0, 0, 0),
        ]);
        ExtrusionLoop::from_paths(vec![path], ExtrusionLoopRole::DEFAULT)
    }

    #[test]
    fn modulo_steps_wrap() {
        assert_eq!(prev_idx_modulo(0, 4), 3);
        assert_eq!(next_idx_modulo(3, 4), 0);
        assert_eq!(next_idx_modulo(1, 4), 2);
    }

    #[test]
    fn a_square_is_counter_clockwise_and_smooth() {
        // Its corners turn by a right angle: none folds back within the threshold.
        let l = square_loop();
        assert!(l.is_counter_clockwise());
        assert!(l.is_smooth(0.174, 0.025));
        assert_eq!(l.polygon().points.len(), 4);
    }

    #[test]
    fn split_at_a_vertex_starts_the_loop_there() {
        let mut l = square_loop();
        assert!(l.split_at_vertex(&Point::new(1_000_000, 1_000_000), 1000.));
        assert_eq!(l.first_point(), Point::new(1_000_000, 1_000_000));
        assert_eq!(l.paths[0].polyline.points.len(), 5);
    }

    #[test]
    fn a_copy_keeps_the_loops_inset_index() {
        let mut l = square_loop();
        l.inset_idx = 2;
        l.paths[0].inset_idx = 1;
        let c = l.clone();
        assert_eq!(c.inset_idx, 2);
        assert_eq!(c.paths[0].inset_idx, -1);
    }
}
