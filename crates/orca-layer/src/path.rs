// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: the classes ExtrusionPath, ExtrusionPathContoured, ExtrusionPathSloped
// and ExtrusionPathOriented of src/libslic3r/ExtrusionEntity.hpp (144-145, 151-374) and their
// definitions in ExtrusionEntity.cpp (18-82, 655-679) of OrcaSlicer v2.4.2, translated on
// 2026-11-13 (orca-o3-layer-1). Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu
// Lab) and the PrusaSlicer authors (Prusa Research), under the GNU Affero General Public License,
// version 3 (AGPL-3.0; LICENSE).

//! [`ExtrusionPath`]: one extrusion along a [`Polyline3`] with one role, flow and width, and the
//! three kinds of path built on it: [`ExtrusionPathContoured`] (with a z offset per point, never
//! simplified), [`ExtrusionPathSloped`] (the slope of a scarf seam at each end) and
//! [`ExtrusionPathOriented`] (one that may not be reversed).
//!
//! The C++ subclasses are structs holding the path (`path`); the virtual methods they override are
//! theirs, and [`crate::entity::ExtrusionEntity`] dispatches the rest to the path. A path's
//! `Clone` is the C++ copy constructor, which does not copy `inset_idx` (it comes out -1), and
//! [`ExtrusionPath::assign_from`] is its assignment, which keeps the target's (`cpp_vec` says
//! what the vector moves make of it). The subclasses' copies call the path's, so theirs come out
//! -1 too.
//!
//! The covers: `polygons_covered_by_width` offsets the 2D polyline by half the width, and
//! `polygons_covered_by_spacing` by half the spacing of a flow made from the width and height (a
//! bridge flow for a bridge role), whose making throws for a width too small: that one returns a
//! `Result`. `simplify_by_fitting_arc` is not virtual in `ExtrusionPath`: the contoured path hides
//! it with a virtual one that does nothing, so [`ExtrusionPath::simplify_by_fitting_arc`] is the
//! base's, called on a contoured path's `path` as `c->ExtrusionPath::simplify_by_fitting_arc`
//! calls it.

use orca_geom::clipper_utils::{
    DEFAULT_END_TYPE, DEFAULT_LINE_JOIN_TYPE, DEFAULT_LINE_MITER_LIMIT,
    diff_pl_polylines_expolygons, intersection_pl_polylines_expolygons, offset_polyline,
};
use orca_geom::expolygon::ExPolygon;
use orca_geom::point::{Point, Points, scale_, unscaled_f64};
use orca_geom::point3::{Point3, Points3};
use orca_geom::polygon::Polygons;
use orca_geom::polyline::{Polyline, Polylines};
use orca_geom::polyline3::Polyline3;
use orca_index::stop::not_ported;

use crate::collection::ExtrusionEntityCollection;
use crate::cpp_vec::InsetSlot;
use crate::entity::ExtrusionEntity;
use crate::flow::{Flow, FlowError};
use crate::role::{ExtrusionRole, is_bridge};

/// `ExtrusionPath`.
#[derive(Debug)]
pub struct ExtrusionPath {
    /// `polyline`.
    pub polyline: Polyline3,
    /// `overhang_degree`.
    pub overhang_degree: f64,
    /// `curve_degree`.
    pub curve_degree: i32,
    /// `mm3_per_mm`: the volume of plastic per millimetre of head travel.
    pub mm3_per_mm: f64,
    /// `width`.
    pub width: f32,
    /// `height`.
    pub height: f32,
    /// `smooth_speed`.
    pub smooth_speed: f64,
    /// `z_contoured`.
    pub z_contoured: bool,
    /// `ExtrusionEntity::inset_idx`: -1, or the wall's index for the classic perimeter generator.
    pub inset_idx: i32,
    /// `m_can_reverse`.
    can_reverse: bool,
    /// `m_role`.
    role: ExtrusionRole,
    /// `m_no_extrusion`.
    no_extrusion: bool,
}

/// `ExtrusionPaths`.
pub type ExtrusionPaths = Vec<ExtrusionPath>;

impl Clone for ExtrusionPath {
    /// The copy constructor: every field but `inset_idx`, which is -1.
    fn clone(&self) -> Self {
        Self {
            polyline: self.polyline.clone(),
            overhang_degree: self.overhang_degree,
            curve_degree: self.curve_degree,
            mm3_per_mm: self.mm3_per_mm,
            width: self.width,
            height: self.height,
            smooth_speed: self.smooth_speed,
            z_contoured: self.z_contoured,
            inset_idx: -1,
            can_reverse: self.can_reverse,
            role: self.role,
            no_extrusion: self.no_extrusion,
        }
    }
}

impl Default for ExtrusionPath {
    /// `ExtrusionPath()`: no role, -1 flow, width and height.
    fn default() -> Self {
        Self::new_no_extrusion(ExtrusionRole::None, -1.0, -1.0, -1.0, false)
    }
}

impl InsetSlot for ExtrusionPath {
    fn inset_slot(&mut self) -> &mut i32 {
        &mut self.inset_idx
    }
    fn assign_keeping_inset(&mut self, rhs: &Self) {
        self.assign_from(rhs);
    }
}

impl ExtrusionPath {
    /// `ExtrusionPath(role)`: -1 flow, width and height.
    #[must_use]
    pub fn with_role(role: ExtrusionRole) -> Self {
        Self::new_no_extrusion(role, -1.0, -1.0, -1.0, false)
    }

    /// `ExtrusionPath(role, mm3_per_mm, width, height)`.
    #[must_use]
    pub fn new(role: ExtrusionRole, mm3_per_mm: f64, width: f32, height: f32) -> Self {
        Self::new_no_extrusion(role, mm3_per_mm, width, height, false)
    }

    /// `ExtrusionPath(role, mm3_per_mm, width, height, no_extrusion)`.
    #[must_use]
    pub fn new_no_extrusion(
        role: ExtrusionRole,
        mm3_per_mm: f64,
        width: f32,
        height: f32,
        no_extrusion: bool,
    ) -> Self {
        Self {
            polyline: Polyline3::default(),
            overhang_degree: 0.,
            curve_degree: 0,
            mm3_per_mm,
            width,
            height,
            smooth_speed: 0.,
            z_contoured: false,
            inset_idx: -1,
            can_reverse: true,
            role,
            no_extrusion,
        }
    }

    /// `ExtrusionPath(polyline, rhs)`: `rhs` along another polyline (its `inset_idx` -1).
    #[must_use]
    pub fn from_polyline(polyline: Polyline3, rhs: &ExtrusionPath) -> Self {
        let mut out = rhs.clone_without_polyline();
        out.polyline = polyline;
        out
    }

    /// A copy without the polyline, for [`ExtrusionPath::from_polyline`].
    fn clone_without_polyline(&self) -> Self {
        Self {
            polyline: Polyline3::default(),
            overhang_degree: self.overhang_degree,
            curve_degree: self.curve_degree,
            mm3_per_mm: self.mm3_per_mm,
            width: self.width,
            height: self.height,
            smooth_speed: self.smooth_speed,
            z_contoured: self.z_contoured,
            inset_idx: -1,
            can_reverse: self.can_reverse,
            role: self.role,
            no_extrusion: self.no_extrusion,
        }
    }

    /// `operator=(rhs)`: every field of `rhs` but `inset_idx`, which stays this path's.
    pub fn assign_from(&mut self, rhs: &ExtrusionPath) {
        self.can_reverse = rhs.can_reverse;
        self.role = rhs.role;
        self.no_extrusion = rhs.no_extrusion;
        self.mm3_per_mm = rhs.mm3_per_mm;
        self.width = rhs.width;
        self.height = rhs.height;
        self.smooth_speed = rhs.smooth_speed;
        self.z_contoured = rhs.z_contoured;
        self.overhang_degree = rhs.overhang_degree;
        self.curve_degree = rhs.curve_degree;
        self.polyline.clone_from(&rhs.polyline);
    }

    /// `reverse()`: the polyline reversed.
    pub fn reverse(&mut self) {
        self.polyline.reverse();
    }

    /// `first_point()`: the first point's x and y.
    ///
    /// # Panics
    /// On an empty path, which the C++ reads outside its list for.
    #[must_use]
    pub fn first_point(&self) -> Point {
        self.first_point3().to_point()
    }

    /// `first_point3()`.
    ///
    /// # Panics
    /// On an empty path.
    #[must_use]
    pub fn first_point3(&self) -> Point3 {
        match self.polyline.points.first() {
            Some(&p) => p,
            None => not_ported("ExtrusionPath::first_point", EMPTY_PATH),
        }
    }

    /// `last_point()`: the last point's x and y.
    ///
    /// # Panics
    /// On an empty path.
    #[must_use]
    pub fn last_point(&self) -> Point {
        self.last_point3().to_point()
    }

    /// `last_point3()`.
    ///
    /// # Panics
    /// On an empty path.
    #[must_use]
    pub fn last_point3(&self) -> Point3 {
        match self.polyline.points.last() {
            Some(&p) => p,
            None => not_ported("ExtrusionPath::last_point", EMPTY_PATH),
        }
    }

    /// `size()`.
    #[must_use]
    pub fn size(&self) -> usize {
        self.polyline.size()
    }

    /// `empty()`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.polyline.is_empty()
    }

    /// `is_closed()`: not empty and the first point equal to the last (x, y and z).
    #[must_use]
    pub fn is_closed(&self) -> bool {
        !self.is_empty() && self.polyline.points.first() == self.polyline.points.last()
    }

    /// `intersect_expolygons(collection, retval)`: the parts of the 2D polyline inside the
    /// ExPolygons, each a path like this one, appended to `retval`.
    pub fn intersect_expolygons(
        &self,
        collection: &[ExPolygon],
        retval: &mut ExtrusionEntityCollection,
    ) {
        let parts =
            intersection_pl_polylines_expolygons(&[self.polyline.to_polyline()], collection);
        self.inflate_collection(&parts, retval);
    }

    /// `subtract_expolygons(collection, retval)`: the parts outside, as
    /// [`ExtrusionPath::intersect_expolygons`].
    pub fn subtract_expolygons(
        &self,
        collection: &[ExPolygon],
        retval: &mut ExtrusionEntityCollection,
    ) {
        let parts = diff_pl_polylines_expolygons(&[self.polyline.to_polyline()], collection);
        self.inflate_collection(&parts, retval);
    }

    /// `_inflate_collection(polylines, collection)`: a path like this one along each polyline
    /// (z 0, no fit).
    fn inflate_collection(
        &self,
        polylines: &Polylines,
        collection: &mut ExtrusionEntityCollection,
    ) {
        for polyline in polylines {
            collection
                .entities
                .push(ExtrusionEntity::Path(ExtrusionPath::from_polyline(
                    Polyline3::from_polyline(polyline, 0),
                    self,
                )));
        }
    }

    /// `clip_end(distance)`.
    pub fn clip_end(&mut self, distance: f64) {
        self.polyline.clip_end(distance);
    }

    /// `simplify(tolerance)`: nothing for a contoured path (`z_contoured`), else the polyline's.
    pub fn simplify(&mut self, tolerance: f64) {
        if self.z_contoured {
            return;
        }
        self.polyline.simplify(tolerance);
    }

    /// `simplify_by_fitting_arc(tolerance)`: nothing for `z_contoured`, else the polyline's.
    pub fn simplify_by_fitting_arc(&mut self, tolerance: f64) {
        if self.z_contoured {
            return;
        }
        self.polyline.simplify_by_fitting_arc(tolerance);
    }

    /// `length()`: the 3D length.
    #[must_use]
    pub fn length(&self) -> f64 {
        self.polyline.length()
    }

    /// `role()`.
    #[must_use]
    pub fn role(&self) -> ExtrusionRole {
        self.role
    }

    /// `polygons_covered_by_width(out, scaled_epsilon)`: the 2D polyline offset by half the
    /// width plus `scaled_epsilon` (square joins, butt ends), appended.
    #[allow(clippy::cast_possible_truncation)]
    pub fn polygons_covered_by_width(&self, out: &mut Polygons, scaled_epsilon: f32) {
        let delta = scale_(f64::from(self.width / 2.0f32)) as f32 + scaled_epsilon;
        polygons_append(
            out,
            offset_polyline(
                &self.polyline.to_polyline(),
                delta,
                DEFAULT_LINE_JOIN_TYPE,
                DEFAULT_LINE_MITER_LIMIT,
                DEFAULT_END_TYPE,
            ),
        );
    }

    /// `polygons_covered_by_spacing(out, scaled_epsilon)`: offset by half the spacing of
    /// `Flow(width, height, 0)` (`bridging_flow(width, 0)` for a bridge role) plus
    /// `scaled_epsilon`, appended.
    ///
    /// # Errors
    /// [`FlowError::NegativeSpacing`] from the flow; `out` keeps what came before.
    #[allow(clippy::cast_precision_loss)]
    pub fn polygons_covered_by_spacing(
        &self,
        out: &mut Polygons,
        scaled_epsilon: f32,
    ) -> Result<(), FlowError> {
        let bridge = is_bridge(self.role());
        let flow = if bridge {
            Flow::bridging_flow(self.width, 0.0)
        } else {
            Flow::new(self.width, self.height, 0.0)?
        };
        let delta = 0.5f32 * flow.scaled_spacing() as f32 + scaled_epsilon;
        polygons_append(
            out,
            offset_polyline(
                &self.polyline.to_polyline(),
                delta,
                DEFAULT_LINE_JOIN_TYPE,
                DEFAULT_LINE_MITER_LIMIT,
                DEFAULT_END_TYPE,
            ),
        );
        Ok(())
    }

    /// `polygons_covered_by_width(scaled_epsilon)`, returned.
    #[must_use]
    pub fn covered_by_width(&self, scaled_epsilon: f32) -> Polygons {
        let mut out = Polygons::new();
        self.polygons_covered_by_width(&mut out, scaled_epsilon);
        out
    }

    /// `polygons_covered_by_spacing(scaled_epsilon)`, returned.
    ///
    /// # Errors
    /// As [`ExtrusionPath::polygons_covered_by_spacing`].
    pub fn covered_by_spacing(&self, scaled_epsilon: f32) -> Result<Polygons, FlowError> {
        let mut out = Polygons::new();
        self.polygons_covered_by_spacing(&mut out, scaled_epsilon)?;
        Ok(out)
    }

    /// `min_mm3_per_mm()`: its own flow.
    #[must_use]
    pub fn min_mm3_per_mm(&self) -> f64 {
        self.mm3_per_mm
    }

    /// `as_polyline()`: the 2D polyline (no fit).
    #[must_use]
    pub fn as_polyline(&self) -> Polyline {
        self.polyline.to_polyline()
    }

    /// `collect_polylines(dst)`: the 2D polyline, unless the path is empty.
    pub fn collect_polylines(&self, dst: &mut Polylines) {
        if !self.polyline.is_empty() {
            dst.push(self.polyline.to_polyline());
        }
    }

    /// `collect_points(dst)`: each point's x and y.
    pub fn collect_points(&self, dst: &mut Points) {
        dst.reserve(self.polyline.points.len());
        for point in &self.polyline.points {
            dst.push(Point::new(point.x, point.y));
        }
    }

    /// `collect_points3(dst)`.
    pub fn collect_points3(&self, dst: &mut Points3) {
        dst.extend_from_slice(&self.polyline.points);
    }

    /// `total_volume()`: `mm3_per_mm * unscale<double>(length())`.
    #[must_use]
    pub fn total_volume(&self) -> f64 {
        self.mm3_per_mm * unscaled_f64(self.length())
    }

    /// `is_force_no_extrusion()`.
    #[must_use]
    pub fn is_force_no_extrusion(&self) -> bool {
        self.no_extrusion
    }

    /// `set_force_no_extrusion(no_extrusion)`.
    pub fn set_force_no_extrusion(&mut self, no_extrusion: bool) {
        self.no_extrusion = no_extrusion;
    }

    /// `set_extrusion_role(role)`.
    pub fn set_extrusion_role(&mut self, role: ExtrusionRole) {
        self.role = role;
    }

    /// `set_reverse()`: may no longer be reversed.
    pub fn set_reverse(&mut self) {
        self.can_reverse = false;
    }

    /// `can_reverse()` of this class (the member; an oriented path's is always false).
    #[must_use]
    pub fn can_reverse(&self) -> bool {
        self.can_reverse
    }
}

/// The stop's condition for the end points of an empty path.
pub(crate) const EMPTY_PATH: &str =
    "an empty path (the C++ reads its points' front or back outside the list)";

/// `polygons_append(dst, Polygons&&)`: `src` moved in, or appended.
pub(crate) fn polygons_append(dst: &mut Polygons, src: Polygons) {
    if dst.is_empty() {
        *dst = src;
    } else {
        dst.extend(src);
    }
}

/// `ExtrusionPathContoured`: a path with a z offset per point (`z_diffs`) that is never
/// simplified and reverses its offsets with its points.
#[derive(Clone, Debug, Default)]
pub struct ExtrusionPathContoured {
    /// The `ExtrusionPath` base.
    pub path: ExtrusionPath,
    /// `z_diffs`.
    pub z_diffs: Vec<f64>,
}

impl ExtrusionPathContoured {
    /// `ExtrusionPathContoured(polyline, rhs, z_diffs)`.
    #[must_use]
    pub fn new(polyline: Polyline3, rhs: &ExtrusionPath, z_diffs: Vec<f64>) -> Self {
        Self {
            path: ExtrusionPath::from_polyline(polyline, rhs),
            z_diffs,
        }
    }

    /// `simplify(tolerance)`: nothing.
    pub fn simplify(&mut self, _tolerance: f64) {}

    /// `simplify_by_fitting_arc(tolerance)`: nothing.
    pub fn simplify_by_fitting_arc(&mut self, _tolerance: f64) {}

    /// `reverse()`: the polyline and the z offsets.
    pub fn reverse(&mut self) {
        self.path.polyline.reverse();
        self.z_diffs.reverse();
    }
}

impl InsetSlot for ExtrusionPathContoured {
    fn inset_slot(&mut self) -> &mut i32 {
        &mut self.path.inset_idx
    }
    fn assign_keeping_inset(&mut self, rhs: &Self) {
        self.path.assign_from(&rhs.path);
        self.z_diffs.clone_from(&rhs.z_diffs);
    }
}

/// `ExtrusionPathSloped::Slope`: the ratios of z and of extrusion at one end of a sloped path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slope {
    /// `z_ratio`.
    pub z_ratio: f64,
    /// `e_ratio`.
    pub e_ratio: f64,
}

impl Default for Slope {
    /// `{1., 1.}`.
    fn default() -> Self {
        Self {
            z_ratio: 1.,
            e_ratio: 1.,
        }
    }
}

/// `ExtrusionPathSloped`: a path rising (or falling) from `slope_begin` to `slope_end`, the
/// pieces of a scarf seam.
#[derive(Clone, Debug, Default)]
pub struct ExtrusionPathSloped {
    /// The `ExtrusionPath` base.
    pub path: ExtrusionPath,
    /// `slope_begin`.
    pub slope_begin: Slope,
    /// `slope_end`.
    pub slope_end: Slope,
}

impl ExtrusionPathSloped {
    /// `ExtrusionPathSloped(rhs, begin, end)`: a copy of `rhs` (its `inset_idx` -1).
    #[must_use]
    pub fn new(rhs: &ExtrusionPath, begin: Slope, end: Slope) -> Self {
        Self {
            path: rhs.clone(),
            slope_begin: begin,
            slope_end: end,
        }
    }

    /// `ExtrusionPathSloped(polyline, rhs, begin, end)`.
    #[must_use]
    pub fn from_polyline(
        polyline: Polyline3,
        rhs: &ExtrusionPath,
        begin: Slope,
        end: Slope,
    ) -> Self {
        Self {
            path: ExtrusionPath::from_polyline(polyline, rhs),
            slope_begin: begin,
            slope_end: end,
        }
    }

    /// `interpolate(ratio)`: each ratio's `lerp` from the beginning to the end.
    #[must_use]
    pub fn interpolate(&self, ratio: f64) -> Slope {
        Slope {
            z_ratio: lerp(self.slope_begin.z_ratio, self.slope_end.z_ratio, ratio),
            e_ratio: lerp(self.slope_begin.e_ratio, self.slope_end.e_ratio, ratio),
        }
    }

    /// `is_flat()`: the z ratios within `EPSILON` of each other.
    #[must_use]
    pub fn is_flat(&self) -> bool {
        is_approx(self.slope_begin.z_ratio, self.slope_end.z_ratio)
    }
}

impl InsetSlot for ExtrusionPathSloped {
    fn inset_slot(&mut self) -> &mut i32 {
        &mut self.path.inset_idx
    }
    fn assign_keeping_inset(&mut self, rhs: &Self) {
        self.path.assign_from(&rhs.path);
        self.slope_begin = rhs.slope_begin;
        self.slope_end = rhs.slope_end;
    }
}

/// `ExtrusionPathOriented`: a path that may never be reversed (`can_reverse()` false whatever
/// its member says).
#[derive(Clone, Debug, Default)]
pub struct ExtrusionPathOriented {
    /// The `ExtrusionPath` base.
    pub path: ExtrusionPath,
}

impl ExtrusionPathOriented {
    /// `ExtrusionPathOriented(role, mm3_per_mm, width, height)`.
    #[must_use]
    pub fn new(role: ExtrusionRole, mm3_per_mm: f64, width: f32, height: f32) -> Self {
        Self {
            path: ExtrusionPath::new(role, mm3_per_mm, width, height),
        }
    }

    /// `can_reverse()`: false.
    #[must_use]
    pub fn can_reverse(&self) -> bool {
        false
    }
}

/// `lerp(a, b, t)` of libslic3r.h: `(1 - t) * a + t * b` (its assert does not run).
#[must_use]
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    (1. - t) * a + t * b
}

/// `is_approx(value, test_value)` of libslic3r.h: `fabs(value - test_value) < EPSILON`.
#[must_use]
pub fn is_approx(value: f64, test_value: f64) -> bool {
    orca_crt::fabs(value - test_value) < orca_geom::point::EPSILON
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_has_no_inset_index_and_an_assignment_keeps_the_targets() {
        let mut a = ExtrusionPath::new(ExtrusionRole::Perimeter, 0.1, 0.4, 0.2);
        a.inset_idx = 2;
        a.set_reverse();
        let b = a.clone();
        assert_eq!(b.inset_idx, -1);
        assert!(!b.can_reverse());
        let mut c = ExtrusionPath {
            inset_idx: 5,
            ..Default::default()
        };
        c.assign_from(&a);
        assert_eq!(c.inset_idx, 5);
        assert_eq!(c.role(), ExtrusionRole::Perimeter);
    }

    #[test]
    fn a_contoured_path_reverses_its_offsets() {
        let mut base = ExtrusionPath::new(ExtrusionRole::Perimeter, 0.1, 0.4, 0.2);
        base.polyline = Polyline3::new(vec![Point3::new(0, 0, 0), Point3::new(10, 0, 0)]);
        let mut c = ExtrusionPathContoured::new(base.polyline.clone(), &base, vec![0.1, 0.2]);
        c.reverse();
        assert_eq!(c.z_diffs, vec![0.2, 0.1]);
        assert_eq!(c.path.polyline.points[0], Point3::new(10, 0, 0));
    }
}
