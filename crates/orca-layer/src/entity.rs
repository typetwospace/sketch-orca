// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: the class ExtrusionEntity and the typedef ExtrusionEntitiesPtr of
// src/libslic3r/ExtrusionEntity.hpp (105-151), its append helpers (551-671) and the virtual
// methods of its subclasses as one dispatch, of OrcaSlicer v2.4.2, translated on 2026-11-13
// (orca-o3-layer-1). Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and
// the PrusaSlicer authors (Prusa Research), under the GNU Affero General Public License, version
// 3 (AGPL-3.0; LICENSE).

//! [`ExtrusionEntity`]: the abstract C++ class as an enum of its eight concrete classes, each
//! method its virtual dispatch.
//!
//! The vector of owning pointers `ExtrusionEntitiesPtr` is a `Vec<ExtrusionEntity>`: the entities
//! never move when it grows, so their `inset_idx` is not touched. Copying an entity is the virtual
//! `clone()` (this enum's `Clone`), which is the class's own copy constructor but for two classes
//! that do not override it: a sloped path clones as a plain path and a sloped loop as a plain loop
//! (the C++ slices them). [`ExtrusionEntity::clone_move`] is `clone_move()`, the same with the move
//! constructors (which keep the held paths' indices). `role`, `can_reverse` and the rest answer as
//! the class's override does: an oriented path never reverses, a loop never reverses and ignores
//! `set_reverse`, and a collection answers from what it holds.
//!
//! The C++ throws `RuntimeError` for a collection's `as_polyline()` and `length()`; those return
//! a [`RuntimeError`] here, and the covers by spacing return a `FlowError` for a width too small.
//!
//! The append helpers build paths and loops from polylines and polygons as the generators do: a
//! path along each valid polyline (oriented when it may not be reversed), multi paths joined by a
//! wipe (a move that does not extrude) where a polyline starts within three widths of the last
//! one's end, a loop along each valid polygon, and a loop or a path for each polyline as it is
//! closed or not.

use core::fmt;

use orca_geom::point::{Point, Points, SCALING_FACTOR, vec2d};
use orca_geom::point3::Point3;
use orca_geom::polygon::Polygons;
use orca_geom::polyline::{Polyline, Polylines};
use orca_geom::polyline3::Polyline3;

use crate::collection::ExtrusionEntityCollection;
use crate::cpp_vec;
use crate::extrusion_loop::ExtrusionLoop;
use crate::extrusion_loop::ExtrusionLoopSloped;
use crate::flow::FlowError;
use crate::multi_path::ExtrusionMultiPath;
use crate::path::{
    ExtrusionPath, ExtrusionPathContoured, ExtrusionPathOriented, ExtrusionPathSloped,
    ExtrusionPaths,
};
use crate::role::{ExtrusionLoopRole, ExtrusionRole};

/// `Slic3r::RuntimeError`: the text of the exception.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeError(pub String);

impl RuntimeError {
    /// `what()`.
    #[must_use]
    pub fn what(&self) -> String {
        self.0.clone()
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RuntimeError {}

/// `ExtrusionEntity`: one of its concrete classes.
#[derive(Debug)]
pub enum ExtrusionEntity {
    /// `ExtrusionPath`.
    Path(ExtrusionPath),
    /// `ExtrusionPathContoured`.
    PathContoured(ExtrusionPathContoured),
    /// `ExtrusionPathSloped`.
    PathSloped(ExtrusionPathSloped),
    /// `ExtrusionPathOriented`.
    PathOriented(ExtrusionPathOriented),
    /// `ExtrusionMultiPath`.
    MultiPath(ExtrusionMultiPath),
    /// `ExtrusionLoop`.
    Loop(ExtrusionLoop),
    /// `ExtrusionLoopSloped`.
    LoopSloped(ExtrusionLoopSloped),
    /// `ExtrusionEntityCollection`.
    Collection(ExtrusionEntityCollection),
}

/// `ExtrusionEntitiesPtr`: the entities a collection or a generator owns.
pub type ExtrusionEntitiesPtr = Vec<ExtrusionEntity>;

impl Clone for ExtrusionEntity {
    /// `clone()`: the class's copy, a sloped path's as a plain path and a sloped loop's as a
    /// plain loop.
    fn clone(&self) -> Self {
        match self {
            Self::Path(p) => Self::Path(p.clone()),
            Self::PathContoured(p) => Self::PathContoured(p.clone()),
            Self::PathSloped(p) => Self::Path(p.path.clone()),
            Self::PathOriented(p) => Self::PathOriented(p.clone()),
            Self::MultiPath(m) => Self::MultiPath(m.clone()),
            Self::Loop(l) => Self::Loop(l.clone()),
            Self::LoopSloped(l) => Self::Loop(l.base.clone()),
            Self::Collection(c) => Self::Collection(c.clone()),
        }
    }
}

impl ExtrusionEntity {
    /// `clone_move()`: the class's move constructor, a sloped path's as a plain path and a sloped
    /// loop's as a plain loop. A moved path's or multi path's `inset_idx` is -1, a loop keeps its
    /// own, and the paths a multi path or loop holds move with theirs.
    #[must_use]
    pub fn clone_move(self) -> Self {
        match self {
            Self::Path(mut p) | Self::PathSloped(ExtrusionPathSloped { path: mut p, .. }) => {
                p.inset_idx = -1;
                Self::Path(p)
            }
            Self::PathContoured(mut p) => {
                p.path.inset_idx = -1;
                Self::PathContoured(p)
            }
            Self::PathOriented(mut p) => {
                p.path.inset_idx = -1;
                Self::PathOriented(p)
            }
            Self::MultiPath(mut m) => {
                m.inset_idx = -1;
                Self::MultiPath(m)
            }
            Self::Loop(l) | Self::LoopSloped(ExtrusionLoopSloped { base: l, .. }) => Self::Loop(l),
            Self::Collection(mut c) => {
                c.inset_idx = -1;
                Self::Collection(c)
            }
        }
    }

    /// `dynamic_cast<const ExtrusionPath*>`: the path of the four path classes.
    #[must_use]
    pub fn as_path(&self) -> Option<&ExtrusionPath> {
        match self {
            Self::Path(p) => Some(p),
            Self::PathContoured(p) => Some(&p.path),
            Self::PathSloped(p) => Some(&p.path),
            Self::PathOriented(p) => Some(&p.path),
            _ => None,
        }
    }

    /// `dynamic_cast<ExtrusionPath*>`: the path of the four path classes, to change.
    pub fn as_path_mut(&mut self) -> Option<&mut ExtrusionPath> {
        match self {
            Self::Path(p) => Some(p),
            Self::PathContoured(p) => Some(&mut p.path),
            Self::PathSloped(p) => Some(&mut p.path),
            Self::PathOriented(p) => Some(&mut p.path),
            _ => None,
        }
    }

    /// `inset_idx`.
    #[must_use]
    pub fn inset_idx(&self) -> i32 {
        match self {
            Self::Path(p) => p.inset_idx,
            Self::PathContoured(p) => p.path.inset_idx,
            Self::PathSloped(p) => p.path.inset_idx,
            Self::PathOriented(p) => p.path.inset_idx,
            Self::MultiPath(m) => m.inset_idx,
            Self::Loop(l) => l.inset_idx,
            Self::LoopSloped(l) => l.base.inset_idx,
            Self::Collection(c) => c.inset_idx,
        }
    }

    /// `inset_idx = value`.
    pub fn set_inset_idx(&mut self, value: i32) {
        match self {
            Self::Path(p) => p.inset_idx = value,
            Self::PathContoured(p) => p.path.inset_idx = value,
            Self::PathSloped(p) => p.path.inset_idx = value,
            Self::PathOriented(p) => p.path.inset_idx = value,
            Self::MultiPath(m) => m.inset_idx = value,
            Self::Loop(l) => l.inset_idx = value,
            Self::LoopSloped(l) => l.base.inset_idx = value,
            Self::Collection(c) => c.inset_idx = value,
        }
    }

    /// `role()`.
    #[must_use]
    pub fn role(&self) -> ExtrusionRole {
        match self {
            Self::MultiPath(m) => m.role(),
            Self::Loop(l) | Self::LoopSloped(ExtrusionLoopSloped { base: l, .. }) => l.role(),
            Self::Collection(c) => c.role(),
            path => path
                .as_path()
                .map_or(ExtrusionRole::None, ExtrusionPath::role),
        }
    }

    /// `is_collection()`.
    #[must_use]
    pub fn is_collection(&self) -> bool {
        matches!(self, Self::Collection(_))
    }

    /// `is_loop()`: the two loop classes.
    #[must_use]
    pub fn is_loop(&self) -> bool {
        matches!(self, Self::Loop(_) | Self::LoopSloped(_))
    }

    /// `can_reverse()`.
    #[must_use]
    pub fn can_reverse(&self) -> bool {
        match self {
            Self::Path(p) => p.can_reverse(),
            Self::PathContoured(p) => p.path.can_reverse(),
            Self::PathSloped(p) => p.path.can_reverse(),
            Self::PathOriented(p) => p.can_reverse(),
            Self::MultiPath(m) => m.can_reverse(),
            Self::Loop(_) | Self::LoopSloped(_) => false,
            Self::Collection(c) => c.can_reverse(),
        }
    }

    /// `can_sort()`: true but for a collection that may not be sorted.
    #[must_use]
    pub fn can_sort(&self) -> bool {
        match self {
            Self::Collection(c) => c.can_sort(),
            _ => true,
        }
    }

    /// `set_reverse()`: nothing for a loop.
    pub fn set_reverse(&mut self) {
        match self {
            Self::Path(p) => p.set_reverse(),
            Self::PathContoured(p) => p.path.set_reverse(),
            Self::PathSloped(p) => p.path.set_reverse(),
            Self::PathOriented(p) => p.path.set_reverse(),
            Self::MultiPath(m) => m.set_reverse(),
            Self::Loop(_) | Self::LoopSloped(_) => {}
            Self::Collection(c) => c.set_reverse(),
        }
    }

    /// `reverse()`.
    pub fn reverse(&mut self) {
        match self {
            Self::Path(p) => p.reverse(),
            Self::PathContoured(p) => p.reverse(),
            Self::PathSloped(p) => p.path.reverse(),
            Self::PathOriented(p) => p.path.reverse(),
            Self::MultiPath(m) => m.reverse(),
            Self::Loop(l) | Self::LoopSloped(ExtrusionLoopSloped { base: l, .. }) => l.reverse(),
            Self::Collection(c) => c.reverse(),
        }
    }

    /// `first_point()`.
    ///
    /// # Panics
    /// Where the class's stops (an empty path, no paths, an empty collection).
    #[must_use]
    pub fn first_point(&self) -> Point {
        match self {
            Self::MultiPath(m) => m.first_point(),
            Self::Loop(l) | Self::LoopSloped(ExtrusionLoopSloped { base: l, .. }) => {
                l.first_point()
            }
            Self::Collection(c) => c.first_point(),
            path => path.path_ref().first_point(),
        }
    }

    /// `last_point()`.
    ///
    /// # Panics
    /// As [`ExtrusionEntity::first_point`].
    #[must_use]
    pub fn last_point(&self) -> Point {
        match self {
            Self::MultiPath(m) => m.last_point(),
            Self::Loop(l) | Self::LoopSloped(ExtrusionLoopSloped { base: l, .. }) => l.last_point(),
            Self::Collection(c) => c.last_point(),
            path => path.path_ref().last_point(),
        }
    }

    /// `first_point3()`.
    ///
    /// # Panics
    /// As [`ExtrusionEntity::first_point`].
    #[must_use]
    pub fn first_point3(&self) -> Point3 {
        match self {
            Self::MultiPath(m) => m.first_point3(),
            Self::Loop(l) | Self::LoopSloped(ExtrusionLoopSloped { base: l, .. }) => {
                l.first_point3()
            }
            Self::Collection(c) => c.first_point3(),
            path => path.path_ref().first_point3(),
        }
    }

    /// `last_point3()`.
    ///
    /// # Panics
    /// As [`ExtrusionEntity::first_point`].
    #[must_use]
    pub fn last_point3(&self) -> Point3 {
        match self {
            Self::MultiPath(m) => m.last_point3(),
            Self::Loop(l) | Self::LoopSloped(ExtrusionLoopSloped { base: l, .. }) => {
                l.last_point3()
            }
            Self::Collection(c) => c.last_point3(),
            path => path.path_ref().last_point3(),
        }
    }

    /// The path of a path class; never called on the others.
    fn path_ref(&self) -> &ExtrusionPath {
        match self.as_path() {
            Some(p) => p,
            None => unreachable!("path_ref of an entity that is not a path"),
        }
    }

    /// `polygons_covered_by_width(out, scaled_epsilon)`.
    pub fn polygons_covered_by_width(&self, out: &mut Polygons, scaled_epsilon: f32) {
        match self {
            Self::MultiPath(m) => m.polygons_covered_by_width(out, scaled_epsilon),
            Self::Loop(l) | Self::LoopSloped(ExtrusionLoopSloped { base: l, .. }) => {
                l.polygons_covered_by_width(out, scaled_epsilon);
            }
            Self::Collection(c) => c.polygons_covered_by_width(out, scaled_epsilon),
            path => path
                .path_ref()
                .polygons_covered_by_width(out, scaled_epsilon),
        }
    }

    /// `polygons_covered_by_spacing(out, scaled_epsilon)`.
    ///
    /// # Errors
    /// The [`FlowError`] of the first path whose width is too small.
    pub fn polygons_covered_by_spacing(
        &self,
        out: &mut Polygons,
        scaled_epsilon: f32,
    ) -> Result<(), FlowError> {
        match self {
            Self::MultiPath(m) => m.polygons_covered_by_spacing(out, scaled_epsilon),
            Self::Loop(l) | Self::LoopSloped(ExtrusionLoopSloped { base: l, .. }) => {
                l.polygons_covered_by_spacing(out, scaled_epsilon)
            }
            Self::Collection(c) => c.polygons_covered_by_spacing(out, scaled_epsilon),
            path => path
                .path_ref()
                .polygons_covered_by_spacing(out, scaled_epsilon),
        }
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
    /// As [`ExtrusionEntity::polygons_covered_by_spacing`].
    pub fn covered_by_spacing(&self, scaled_epsilon: f32) -> Result<Polygons, FlowError> {
        let mut out = Polygons::new();
        self.polygons_covered_by_spacing(&mut out, scaled_epsilon)?;
        Ok(out)
    }

    /// `min_mm3_per_mm()`.
    #[must_use]
    pub fn min_mm3_per_mm(&self) -> f64 {
        match self {
            Self::MultiPath(m) => m.min_mm3_per_mm(),
            Self::Loop(l) | Self::LoopSloped(ExtrusionLoopSloped { base: l, .. }) => {
                l.min_mm3_per_mm()
            }
            Self::Collection(c) => c.min_mm3_per_mm(),
            path => path.path_ref().min_mm3_per_mm(),
        }
    }

    /// `as_polyline()`.
    ///
    /// # Errors
    /// For a collection: "Calling as_polyline() on a ExtrusionEntityCollection".
    pub fn as_polyline(&self) -> Result<Polyline, RuntimeError> {
        Ok(match self {
            Self::MultiPath(m) => m.as_polyline(),
            Self::Loop(l) | Self::LoopSloped(ExtrusionLoopSloped { base: l, .. }) => {
                l.as_polyline()
            }
            Self::Collection(c) => return c.as_polyline(),
            path => path.path_ref().as_polyline(),
        })
    }

    /// `collect_polylines(dst)`.
    pub fn collect_polylines(&self, dst: &mut Polylines) {
        match self {
            Self::MultiPath(m) => m.collect_polylines(dst),
            Self::Loop(l) | Self::LoopSloped(ExtrusionLoopSloped { base: l, .. }) => {
                l.collect_polylines(dst);
            }
            Self::Collection(c) => c.collect_polylines(dst),
            path => path.path_ref().collect_polylines(dst),
        }
    }

    /// `collect_points(dst)`.
    pub fn collect_points(&self, dst: &mut Points) {
        match self {
            Self::MultiPath(m) => m.collect_points(dst),
            Self::Loop(l) | Self::LoopSloped(ExtrusionLoopSloped { base: l, .. }) => {
                l.collect_points(dst);
            }
            Self::Collection(c) => c.collect_points(dst),
            path => path.path_ref().collect_points(dst),
        }
    }

    /// `as_polylines()`: [`ExtrusionEntity::collect_polylines`] into a new list.
    #[must_use]
    pub fn as_polylines(&self) -> Polylines {
        let mut dst = Polylines::new();
        self.collect_polylines(&mut dst);
        dst
    }

    /// `length()`.
    ///
    /// # Errors
    /// For a collection: "Calling length() on a ExtrusionEntityCollection".
    pub fn length(&self) -> Result<f64, RuntimeError> {
        Ok(match self {
            Self::MultiPath(m) => m.length(),
            Self::Loop(l) | Self::LoopSloped(ExtrusionLoopSloped { base: l, .. }) => l.length(),
            Self::Collection(c) => return c.length(),
            path => path.path_ref().length(),
        })
    }

    /// `total_volume()`.
    #[must_use]
    pub fn total_volume(&self) -> f64 {
        match self {
            Self::MultiPath(m) => m.total_volume(),
            Self::Loop(l) | Self::LoopSloped(ExtrusionLoopSloped { base: l, .. }) => {
                l.total_volume()
            }
            Self::Collection(c) => c.total_volume(),
            path => path.path_ref().total_volume(),
        }
    }
}

/// `extrusion_paths_append(dst, polylines, role, mm3_per_mm, width, height)`: a path along each
/// valid polyline (z 0), appended.
pub fn extrusion_paths_append(
    dst: &mut ExtrusionPaths,
    polylines: &[Polyline],
    role: ExtrusionRole,
    mm3_per_mm: f64,
    width: f32,
    height: f32,
) {
    cpp_vec::reserve(dst, dst.len() + polylines.len());
    for polyline in polylines {
        if polyline.is_valid() {
            cpp_vec::push(dst, ExtrusionPath::new(role, mm3_per_mm, width, height));
            if let Some(back) = dst.last_mut() {
                back.polyline = Polyline3::from_polyline(polyline, 0);
            }
        }
    }
}

/// `extrusion_paths_append(dst, Polylines &&polylines, ...)`: as [`extrusion_paths_append`], the
/// polylines cleared after.
pub fn extrusion_paths_append_moved(
    dst: &mut ExtrusionPaths,
    polylines: &mut Polylines,
    role: ExtrusionRole,
    mm3_per_mm: f64,
    width: f32,
    height: f32,
) {
    extrusion_paths_append(dst, polylines, role, mm3_per_mm, width, height);
    polylines.clear();
}

/// `extrusion_paths_append(dst, Polyline &&polyline, ...)`: a path along the polyline when it is
/// valid.
pub fn extrusion_paths_append_polyline(
    dst: &mut ExtrusionPaths,
    polyline: &Polyline,
    role: ExtrusionRole,
    mm3_per_mm: f64,
    width: f32,
    height: f32,
) {
    cpp_vec::reserve(dst, dst.len() + 1);
    if polyline.is_valid() {
        cpp_vec::push(dst, ExtrusionPath::new(role, mm3_per_mm, width, height));
        if let Some(back) = dst.last_mut() {
            back.polyline = Polyline3::from_polyline(polyline, 0);
        }
    }
}

/// A path, or an oriented path when it may not be reversed, along a polyline (z 0).
fn path_entity(
    polyline: &Polyline,
    role: ExtrusionRole,
    mm3_per_mm: f64,
    width: f32,
    height: f32,
    can_reverse: bool,
) -> ExtrusionEntity {
    if can_reverse {
        let mut path = ExtrusionPath::new(role, mm3_per_mm, width, height);
        path.polyline = Polyline3::from_polyline(polyline, 0);
        ExtrusionEntity::Path(path)
    } else {
        let mut path = ExtrusionPathOriented::new(role, mm3_per_mm, width, height);
        path.path.polyline = Polyline3::from_polyline(polyline, 0);
        ExtrusionEntity::PathOriented(path)
    }
}

/// `extrusion_entities_append_paths(dst, polylines, role, mm3_per_mm, width, height,
/// can_reverse)`: an entity along each valid polyline, a path or (when it may not be reversed)
/// an oriented path.
pub fn extrusion_entities_append_paths(
    dst: &mut ExtrusionEntitiesPtr,
    polylines: &[Polyline],
    role: ExtrusionRole,
    mm3_per_mm: f64,
    width: f32,
    height: f32,
    can_reverse: bool,
) {
    dst.reserve(polylines.len());
    for polyline in polylines {
        if polyline.is_valid() {
            dst.push(path_entity(
                polyline,
                role,
                mm3_per_mm,
                width,
                height,
                can_reverse,
            ));
        }
    }
}

/// `extrusion_entities_append_paths(dst, Polylines &&polylines, ...)`: as
/// [`extrusion_entities_append_paths`], the polylines cleared after.
pub fn extrusion_entities_append_paths_moved(
    dst: &mut ExtrusionEntitiesPtr,
    polylines: &mut Polylines,
    role: ExtrusionRole,
    mm3_per_mm: f64,
    width: f32,
    height: f32,
    can_reverse: bool,
) {
    extrusion_entities_append_paths(dst, polylines, role, mm3_per_mm, width, height, can_reverse);
    polylines.clear();
}

/// `extrusion_entities_append_paths_with_wipe(dst, polylines, role, mm3_per_mm, width, height)`:
/// the valid polylines as paths of multi paths, a polyline that starts within three widths
/// (`3 * scaled(width)`) of the last one's end joined to it by a path that does not extrude, one
/// farther off starting a new multi path.
pub fn extrusion_entities_append_paths_with_wipe(
    dst: &mut ExtrusionEntitiesPtr,
    polylines: Polylines,
    role: ExtrusionRole,
    mm3_per_mm: f64,
    width: f32,
    height: f32,
) {
    dst.reserve(polylines.len());
    let mut last_end_point = Point::default();
    let mut last_end_point_valid = false;
    let mut multi_path = ExtrusionMultiPath::default();
    // `scaled(width)` of a float: `coord_t(width / float(SCALING_FACTOR))`.
    #[allow(clippy::cast_possible_truncation)]
    let wipe_reach = 3 * orca_crt::f32_to_i64(width / SCALING_FACTOR as f32);
    for polyline in &polylines {
        if polyline.is_valid() {
            if last_end_point_valid {
                let temp = polyline.first_point() - last_end_point;
                #[allow(clippy::cast_precision_loss)]
                let near = vec2d(temp.x as f64, temp.y as f64).norm() <= wipe_reach as f64;
                if near {
                    cpp_vec::push(
                        &mut multi_path.paths,
                        ExtrusionPath::new_no_extrusion(role, mm3_per_mm, width, height, true),
                    );
                    if let Some(back) = multi_path.paths.last_mut() {
                        back.polyline = Polyline3::from_polyline(
                            &Polyline::from_two(last_end_point, polyline.first_point()),
                            0,
                        );
                    }
                } else {
                    dst.push(ExtrusionEntity::MultiPath(core::mem::take(&mut multi_path)));
                }
            }

            cpp_vec::push(
                &mut multi_path.paths,
                ExtrusionPath::new(role, mm3_per_mm, width, height),
            );
            if let Some(back) = multi_path.paths.last_mut() {
                back.polyline = Polyline3::from_polyline(polyline, 0);
                last_end_point_valid = true;
                last_end_point = back.polyline.last_point().to_point();
            }
        }
    }
    if !multi_path.is_empty() {
        dst.push(ExtrusionEntity::MultiPath(multi_path));
    }
}

/// `extrusion_entities_append_loops(dst, loops, role, mm3_per_mm, width, height)`: a loop along
/// each valid polygon (z 0), its first point repeated at the end.
pub fn extrusion_entities_append_loops(
    dst: &mut ExtrusionEntitiesPtr,
    loops: Polygons,
    role: ExtrusionRole,
    mm3_per_mm: f64,
    width: f32,
    height: f32,
) {
    dst.reserve(loops.len());
    for poly in &loops {
        if poly.is_valid() {
            let mut path = ExtrusionPath::new(role, mm3_per_mm, width, height);
            path.polyline.points.reserve(poly.points.len() + 1);
            for pt in &poly.points {
                path.polyline.points.push(Point3::from_point(*pt, 0));
            }
            let front = path.polyline.points[0];
            path.polyline.points.push(front);
            dst.push(ExtrusionEntity::Loop(ExtrusionLoop::from_path(
                &path,
                ExtrusionLoopRole::DEFAULT,
            )));
        }
    }
}

/// `extrusion_entities_append_loops_and_paths(dst, polylines, role, mm3_per_mm, width, height)`:
/// each valid polyline a loop when it is closed, else a path.
pub fn extrusion_entities_append_loops_and_paths(
    dst: &mut ExtrusionEntitiesPtr,
    polylines: Polylines,
    role: ExtrusionRole,
    mm3_per_mm: f64,
    width: f32,
    height: f32,
) {
    dst.reserve(polylines.len());
    for polyline in &polylines {
        if polyline.is_valid() {
            let mut path = ExtrusionPath::new(role, mm3_per_mm, width, height);
            path.polyline = Polyline3::from_polyline(polyline, 0);
            if polyline.is_closed() {
                dst.push(ExtrusionEntity::Loop(ExtrusionLoop::from_path(
                    &path,
                    ExtrusionLoopRole::DEFAULT,
                )));
            } else {
                dst.push(ExtrusionEntity::Path(path));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::Slope;

    #[test]
    fn a_sloped_path_clones_as_a_plain_path() {
        let s = ExtrusionEntity::PathSloped(ExtrusionPathSloped::new(
            &ExtrusionPath::new(ExtrusionRole::Perimeter, 0.1, 0.4, 0.2),
            Slope::default(),
            Slope::default(),
        ));
        assert!(matches!(s.clone(), ExtrusionEntity::Path(_)));
    }

    #[test]
    fn an_oriented_path_does_not_reverse() {
        let o = ExtrusionEntity::PathOriented(ExtrusionPathOriented::new(
            ExtrusionRole::Perimeter,
            0.1,
            0.4,
            0.2,
        ));
        assert!(!o.can_reverse());
        assert!(o.as_path().is_some_and(ExtrusionPath::can_reverse));
    }

    #[test]
    fn wipes_join_near_polylines() {
        let pls = vec![
            Polyline::new(vec![Point::new(0, 0), Point::new(1_000_000, 0)]),
            Polyline::new(vec![Point::new(1_100_000, 0), Point::new(2_000_000, 0)]),
            Polyline::new(vec![Point::new(9_000_000, 0), Point::new(9_500_000, 0)]),
        ];
        let mut dst = ExtrusionEntitiesPtr::new();
        extrusion_entities_append_paths_with_wipe(
            &mut dst,
            pls,
            ExtrusionRole::GapFill,
            0.1,
            0.4,
            0.2,
        );
        assert_eq!(dst.len(), 2);
        let ExtrusionEntity::MultiPath(m) = &dst[0] else {
            panic!("not a multi path");
        };
        assert_eq!(m.paths.len(), 3);
        assert!(m.paths[1].is_force_no_extrusion());
    }
}
