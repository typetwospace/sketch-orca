// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: the class ExtrusionMultiPath of src/libslic3r/ExtrusionEntity.hpp
// (376-441) and its definitions in ExtrusionEntity.cpp (84-139) of OrcaSlicer v2.4.2, translated
// on 2026-11-13 (orca-o3-layer-1). Copyright the OrcaSlicer authors, the BambuStudio authors
// (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU Affero General Public
// License, version 3 (AGPL-3.0; LICENSE).

//! [`ExtrusionMultiPath`]: one continuous extrusion made of several paths, each starting where
//! the one before ends, whose width, height or bridging may change from path to path.
//!
//! Its copy, like the path's, does not copy `inset_idx` (-1), and its assignment
//! ([`ExtrusionMultiPath::assign_from`]) keeps the target's; the paths it holds move as `cpp_vec`
//! says. Its ends are its first path's first point and its last path's last point: the C++ reads
//! outside its lists for a multi path without paths or with an empty path there, and these stop
//! instead.

use orca_geom::point::{Point, Points};
use orca_geom::point3::{Point3, to_points};
use orca_geom::polygon::Polygons;
use orca_geom::polyline::{Polyline, Polylines};
use orca_index::stop::not_ported;

use crate::cpp_vec;
use crate::flow::FlowError;
use crate::path::{EMPTY_PATH, ExtrusionPath, ExtrusionPathOriented, ExtrusionPaths};
use crate::role::ExtrusionRole;

/// The stop's condition for the ends of a multi path or loop without paths.
pub(crate) const NO_PATHS: &str =
    "no paths (the C++ reads its paths' front or back outside the list)";

/// `ExtrusionMultiPath`.
#[derive(Debug)]
pub struct ExtrusionMultiPath {
    /// `paths`.
    pub paths: ExtrusionPaths,
    /// `ExtrusionEntity::inset_idx`.
    pub inset_idx: i32,
    /// `m_can_reverse`.
    can_reverse: bool,
}

impl Default for ExtrusionMultiPath {
    /// `ExtrusionMultiPath()`.
    fn default() -> Self {
        Self {
            paths: ExtrusionPaths::new(),
            inset_idx: -1,
            can_reverse: true,
        }
    }
}

impl Clone for ExtrusionMultiPath {
    /// The copy constructor: the paths copied (each `inset_idx` -1), `m_can_reverse`, and an
    /// `inset_idx` of -1.
    fn clone(&self) -> Self {
        Self {
            paths: self.paths.clone(),
            inset_idx: -1,
            can_reverse: self.can_reverse,
        }
    }
}

impl ExtrusionMultiPath {
    /// `ExtrusionMultiPath(const ExtrusionPaths &paths)`: the paths copied; reversible.
    #[must_use]
    pub fn from_paths(paths: &[ExtrusionPath]) -> Self {
        Self {
            paths: paths.to_vec(),
            inset_idx: -1,
            can_reverse: true,
        }
    }

    /// `ExtrusionMultiPath(const ExtrusionPath &path)`: the one path, reversible as it is
    /// (`path.can_reverse()`).
    #[must_use]
    pub fn from_path(path: &ExtrusionPath) -> Self {
        let mut out = Self::default();
        cpp_vec::push(&mut out.paths, path.clone());
        out.can_reverse = path.can_reverse();
        out
    }

    /// `ExtrusionMultiPath(const ExtrusionPath &path)` of an oriented path: its virtual
    /// `can_reverse()` is false, so the multi path is not reversible.
    #[must_use]
    pub fn from_oriented(path: &ExtrusionPathOriented) -> Self {
        let mut out = Self::default();
        cpp_vec::push(&mut out.paths, path.path.clone());
        out.can_reverse = path.can_reverse();
        out
    }

    /// `operator=(rhs)`: the paths assigned (`cpp_vec::assign`) and `m_can_reverse`; this multi
    /// path's `inset_idx` stays.
    pub fn assign_from(&mut self, rhs: &ExtrusionMultiPath) {
        cpp_vec::assign(&mut self.paths, &rhs.paths);
        self.can_reverse = rhs.can_reverse;
    }

    /// `is_loop()`: false.
    #[must_use]
    pub fn is_loop(&self) -> bool {
        false
    }

    /// `can_reverse()`.
    #[must_use]
    pub fn can_reverse(&self) -> bool {
        self.can_reverse
    }

    /// `set_reverse()`: may no longer be reversed.
    pub fn set_reverse(&mut self) {
        self.can_reverse = false;
    }

    /// `reverse()`: each path reversed, then their order (`std::reverse`).
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
    /// As [`ExtrusionMultiPath::first_point`].
    #[must_use]
    pub fn first_point3(&self) -> Point3 {
        let Some(front) = self.paths.first() else {
            not_ported("ExtrusionMultiPath::first_point", NO_PATHS)
        };
        match front.polyline.points.first() {
            Some(&p) => p,
            None => not_ported("ExtrusionMultiPath::first_point", EMPTY_PATH),
        }
    }

    /// `last_point()`.
    ///
    /// # Panics
    /// Without paths, or when the last path is empty.
    #[must_use]
    pub fn last_point(&self) -> Point {
        self.last_point3().to_point()
    }

    /// `last_point3()`.
    ///
    /// # Panics
    /// As [`ExtrusionMultiPath::last_point`].
    #[must_use]
    pub fn last_point3(&self) -> Point3 {
        let Some(back) = self.paths.last() else {
            not_ported("ExtrusionMultiPath::last_point", NO_PATHS)
        };
        match back.polyline.points.last() {
            Some(&p) => p,
            None => not_ported("ExtrusionMultiPath::last_point", EMPTY_PATH),
        }
    }

    /// `size()`: the number of paths.
    #[must_use]
    pub fn size(&self) -> usize {
        self.paths.len()
    }

    /// `empty()`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
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

    /// `role()`: the first path's; `erNone` without paths.
    #[must_use]
    pub fn role(&self) -> ExtrusionRole {
        match self.paths.first() {
            Some(front) => front.role(),
            None => ExtrusionRole::None,
        }
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

    /// `as_polyline()`: the first point, then every path's points after its first (the points the
    /// paths share are not repeated); empty without paths.
    ///
    /// # Panics
    /// On an empty path, past whose end the C++ starts reading.
    #[must_use]
    pub fn as_polyline(&self) -> Polyline {
        let mut out = Polyline::default();
        if !self.paths.is_empty() {
            out.points.push(self.first_point());
            for path in &self.paths {
                if path.polyline.points.is_empty() {
                    not_ported("ExtrusionMultiPath::as_polyline", EMPTY_PATH);
                }
                for point in &path.polyline.points[1..] {
                    out.points.push(point.to_point());
                }
            }
        }
        out
    }

    /// `collect_polylines(dst)`: [`ExtrusionMultiPath::as_polyline`], unless empty.
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use orca_geom::polyline3::Polyline3;

    fn path(points: &[(i64, i64)]) -> ExtrusionPath {
        let mut p = ExtrusionPath::new(ExtrusionRole::Perimeter, 0.1, 0.4, 0.2);
        p.polyline = Polyline3::new(points.iter().map(|&(x, y)| Point3::new(x, y, 0)).collect());
        p
    }

    #[test]
    fn as_polyline_does_not_repeat_shared_points() {
        let m =
            ExtrusionMultiPath::from_paths(&[path(&[(0, 0), (10, 0)]), path(&[(10, 0), (10, 10)])]);
        assert_eq!(
            m.as_polyline().points,
            vec![Point::new(0, 0), Point::new(10, 0), Point::new(10, 10)]
        );
    }

    #[test]
    fn a_copy_has_no_inset_index() {
        let mut m = ExtrusionMultiPath::from_paths(&[path(&[(0, 0), (10, 0)])]);
        m.inset_idx = 3;
        m.set_reverse();
        let c = m.clone();
        assert_eq!(c.inset_idx, -1);
        assert!(!c.can_reverse());
    }
}
