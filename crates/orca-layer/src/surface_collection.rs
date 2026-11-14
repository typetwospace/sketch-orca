// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.
//
// Carries OrcaSlicer code: src/libslic3r/SurfaceCollection.hpp lines 1-89 and
// SurfaceCollection.cpp lines 1-164 of OrcaSlicer v2.4.2, translated on 2026-11-14
// (orca-o3-layer-2). Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab), the
// PrusaSlicer authors (Prusa Research) and the Slic3r authors (Alessandro Ranellucci), under the
// GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! [`SurfaceCollection`]: a layer region's list of surfaces and the operations on it.
//!
//! `simplify` replaces each surface by one surface for each island its island simplifies to
//! (`ExPolygon::simplify`, Douglas-Peucker then a union), the fields kept. `group` puts each
//! surface in the first group whose first surface it could merge with (`surfaces_could_merge`),
//! else in a new group. The `filter_by_type` forms keep the order. `keep_type`, `keep_types`,
//! `remove_type` and `remove_types` compact the list in place as the C++ does: a kept surface is
//! swapped into the next free slot, so the kept ones stay in order, and the tail is erased.
//! `remove_type(type, &mut expolygons)` moves the removed islands out, in order.
//!
//! The templates `any_internal_contains` and `any_bottom_contains` take what `ExPolygon::contains`
//! takes ([`ExPolygonContains`]: a point, with the border counted inside, a line, a polyline or a
//! list of polylines).
//!
//! Left out: `export_to_svg`, the debugging output, which needs libslic3r's `SVG` class.

use orca_geom::expolygon::{self, ExPolygon, ExPolygons};
use orca_geom::line::Line;
use orca_geom::point::Point;
use orca_geom::polygon::Polygons;
use orca_geom::polyline::Polyline;

use crate::surface::{
    Surface, SurfaceType, Surfaces, SurfacesPtr, surfaces_append, surfaces_append_owned,
    surfaces_append_templ, surfaces_append_templ_owned, surfaces_append_typed,
    surfaces_append_typed_owned, surfaces_could_merge,
};

/// What `ExPolygon::contains` takes, for `any_internal_contains` and `any_bottom_contains`.
pub trait ExPolygonContains {
    /// `expolygon.contains(*self)`.
    fn contained_in(&self, expolygon: &ExPolygon) -> bool;
}

impl ExPolygonContains for Point {
    /// `contains(point, border_result = true)`.
    fn contained_in(&self, expolygon: &ExPolygon) -> bool {
        expolygon.contains(*self, true)
    }
}

impl ExPolygonContains for Line {
    fn contained_in(&self, expolygon: &ExPolygon) -> bool {
        expolygon.contains_line(self)
    }
}

impl ExPolygonContains for Polyline {
    fn contained_in(&self, expolygon: &ExPolygon) -> bool {
        expolygon.contains_polyline(self)
    }
}

impl ExPolygonContains for [Polyline] {
    fn contained_in(&self, expolygon: &ExPolygon) -> bool {
        expolygon.contains_polylines(self)
    }
}

/// `class SurfaceCollection`.
#[derive(Clone, Debug, Default)]
pub struct SurfaceCollection {
    /// `surfaces`.
    pub surfaces: Surfaces,
}

impl From<Surfaces> for SurfaceCollection {
    /// `SurfaceCollection(Surfaces &&)`.
    fn from(surfaces: Surfaces) -> Self {
        Self { surfaces }
    }
}

impl SurfaceCollection {
    /// `SurfaceCollection(const Surfaces &)`.
    #[must_use]
    pub fn new(surfaces: &[Surface]) -> Self {
        Self {
            surfaces: surfaces.to_vec(),
        }
    }

    /// `simplify(tolerance)`: each surface replaced by a copy for each island its island
    /// simplifies to.
    pub fn simplify(&mut self, tolerance: f64) {
        let mut ss = Surfaces::new();
        for it_s in &self.surfaces {
            let mut expp = ExPolygons::new();
            // ExPolygon::simplify(tolerance, &expp): appends simplify(tolerance).
            expp.extend(it_s.expolygon.simplify(tolerance));
            for it_e in expp {
                let mut s = it_s.clone();
                s.expolygon = it_e;
                ss.push(s);
            }
        }
        self.surfaces = ss;
    }

    /// `group(&retval)`: the surfaces by common properties, appended to `retval`'s groups.
    pub fn group<'a>(&'a self, retval: &mut Vec<SurfacesPtr<'a>>) {
        for it in &self.surfaces {
            // Find a group with the same properties.
            let mut group: Option<usize> = None;
            for (gi, git) in retval.iter().enumerate() {
                if let Some(front) = git.first()
                    && surfaces_could_merge(front, it)
                {
                    group = Some(gi);
                    break;
                }
            }
            // If no group with these properties exists, add one.
            let gi = match group {
                Some(gi) => gi,
                None => {
                    retval.push(SurfacesPtr::new());
                    retval.len() - 1
                }
            };
            // Append the surface to the group.
            retval[gi].push(it);
        }
    }

    /// `any_internal_contains(item)`: some internal surface's island contains `item`.
    #[must_use]
    pub fn any_internal_contains<T: ExPolygonContains + ?Sized>(&self, item: &T) -> bool {
        self.surfaces
            .iter()
            .any(|surface| surface.is_internal() && item.contained_in(&surface.expolygon))
    }

    /// `any_bottom_contains(item)`: some bottom surface's island contains `item`.
    #[must_use]
    pub fn any_bottom_contains<T: ExPolygonContains + ?Sized>(&self, item: &T) -> bool {
        self.surfaces
            .iter()
            .any(|surface| surface.is_bottom() && item.contained_in(&surface.expolygon))
    }

    /// `filter_by_type(type)`: the surfaces of the type, in order.
    #[must_use]
    pub fn filter_by_type(&self, surface_type: SurfaceType) -> SurfacesPtr<'_> {
        let mut ss = SurfacesPtr::new();
        for surface in &self.surfaces {
            if surface.surface_type == surface_type {
                ss.push(surface);
            }
        }
        ss
    }

    /// `filter_by_types({types})`: the surfaces of any of the types, in order.
    #[must_use]
    pub fn filter_by_types(&self, types: &[SurfaceType]) -> SurfacesPtr<'_> {
        let mut ss = SurfacesPtr::new();
        for surface in &self.surfaces {
            if types.contains(&surface.surface_type) {
                ss.push(surface);
            }
        }
        ss
    }

    /// `filter_by_type(type, &polygons)`: the contours and holes of the surfaces of the type,
    /// appended.
    pub fn filter_by_type_polygons(&self, surface_type: SurfaceType, polygons: &mut Polygons) {
        for surface in &self.surfaces {
            if surface.surface_type == surface_type {
                // polygons_append(*polygons, to_polygons(surface.expolygon)), the moving form.
                let pp = expolygon::to_polygons(&surface.expolygon);
                if polygons.is_empty() {
                    *polygons = pp;
                } else {
                    polygons.extend(pp);
                }
            }
        }
    }

    /// The compaction of `keep_type` and its kin: each surface `keep` keeps swapped into the next
    /// free slot, then the tail erased.
    fn compact(&mut self, mut keep: impl FnMut(&mut Surface) -> bool) {
        let mut j = 0usize;
        for i in 0..self.surfaces.len() {
            if keep(&mut self.surfaces[i]) {
                if j < i {
                    self.surfaces.swap(i, j);
                }
                j += 1;
            }
        }
        if j < self.surfaces.len() {
            self.surfaces.truncate(j);
        }
    }

    /// `keep_type(type)`.
    pub fn keep_type(&mut self, surface_type: SurfaceType) {
        self.compact(|s| s.surface_type == surface_type);
    }

    /// `keep_types({types})`.
    pub fn keep_types(&mut self, types: &[SurfaceType]) {
        self.compact(|s| types.contains(&s.surface_type));
    }

    /// `remove_type(type)`.
    pub fn remove_type(&mut self, surface_type: SurfaceType) {
        self.compact(|s| s.surface_type != surface_type);
    }

    /// `remove_type(type, &polygons)`: the islands of the removed surfaces moved to `polygons`,
    /// in order.
    pub fn remove_type_into(&mut self, surface_type: SurfaceType, polygons: &mut ExPolygons) {
        self.compact(|s| {
            if s.surface_type == surface_type {
                polygons.push(core::mem::take(&mut s.expolygon));
                false
            } else {
                true
            }
        });
    }

    /// `remove_types({types})`.
    pub fn remove_types(&mut self, types: &[SurfaceType]) {
        self.compact(|s| !types.contains(&s.surface_type));
    }

    /// `set_type(type)`: every surface given the type.
    pub fn set_type(&mut self, surface_type: SurfaceType) {
        for surface in &mut self.surfaces {
            surface.surface_type = surface_type;
        }
    }

    /// `change_to_new_type(old_type, new_type)`.
    pub fn change_to_new_type(&mut self, old_type: SurfaceType, new_type: SurfaceType) {
        for surface in &mut self.surfaces {
            if surface.surface_type == old_type {
                surface.surface_type = new_type;
            }
        }
    }

    /// `clear()`.
    pub fn clear(&mut self) {
        self.surfaces.clear();
    }

    /// `empty()`.
    #[must_use]
    pub fn empty(&self) -> bool {
        self.surfaces.is_empty()
    }

    /// `size()`.
    #[must_use]
    pub fn size(&self) -> usize {
        self.surfaces.len()
    }

    /// `has(type)`.
    #[must_use]
    pub fn has(&self, surface_type: SurfaceType) -> bool {
        self.surfaces.iter().any(|s| s.surface_type == surface_type)
    }

    /// `begin()` to `end()`.
    pub fn iter(&self) -> core::slice::Iter<'_, Surface> {
        self.surfaces.iter()
    }

    /// `begin()` to `end()`, to change them.
    pub fn iter_mut(&mut self) -> core::slice::IterMut<'_, Surface> {
        self.surfaces.iter_mut()
    }

    /// `set(const SurfaceCollection &)`.
    pub fn set(&mut self, coll: &SurfaceCollection) {
        self.surfaces.clone_from(&coll.surfaces);
    }

    /// `set(SurfaceCollection &&)`.
    pub fn set_owned(&mut self, coll: SurfaceCollection) {
        self.surfaces = coll.surfaces;
    }

    /// `set(const ExPolygons &, SurfaceType)`.
    pub fn set_typed(&mut self, src: &[ExPolygon], surface_type: SurfaceType) {
        self.clear();
        self.append_typed(src, surface_type);
    }

    /// `set(ExPolygons &&, SurfaceType)`.
    pub fn set_typed_owned(&mut self, src: ExPolygons, surface_type: SurfaceType) {
        self.clear();
        self.append_typed_owned(src, surface_type);
    }

    /// `set(const ExPolygons &, const Surface &surfaceTempl)`.
    pub fn set_templ(&mut self, src: &[ExPolygon], surface_templ: &Surface) {
        self.clear();
        self.append_templ(src, surface_templ);
    }

    /// `set(ExPolygons &&, const Surface &surfaceTempl)`.
    pub fn set_templ_owned(&mut self, src: ExPolygons, surface_templ: &Surface) {
        self.clear();
        self.append_templ_owned(src, surface_templ);
    }

    /// `set(const Surfaces &)`.
    pub fn set_surfaces(&mut self, src: &[Surface]) {
        self.clear();
        self.append_surfaces(src);
    }

    /// `set(Surfaces &&)`.
    pub fn set_surfaces_owned(&mut self, src: Surfaces) {
        self.clear();
        self.append_surfaces_owned(src);
    }

    /// `append(const SurfaceCollection &)`.
    pub fn append(&mut self, coll: &SurfaceCollection) {
        self.append_surfaces(&coll.surfaces);
    }

    /// `append(SurfaceCollection &&)`.
    pub fn append_owned(&mut self, coll: SurfaceCollection) {
        self.append_surfaces_owned(coll.surfaces);
    }

    /// `append(const ExPolygons &, SurfaceType)`.
    pub fn append_typed(&mut self, src: &[ExPolygon], surface_type: SurfaceType) {
        surfaces_append_typed(&mut self.surfaces, src, surface_type);
    }

    /// `append(ExPolygons &&, SurfaceType)`.
    pub fn append_typed_owned(&mut self, src: ExPolygons, surface_type: SurfaceType) {
        surfaces_append_typed_owned(&mut self.surfaces, src, surface_type);
    }

    /// `append(const ExPolygons &, const Surface &surfaceTempl)`.
    pub fn append_templ(&mut self, src: &[ExPolygon], surface_templ: &Surface) {
        surfaces_append_templ(&mut self.surfaces, src, surface_templ);
    }

    /// `append(ExPolygons &&, const Surface &surfaceTempl)`.
    pub fn append_templ_owned(&mut self, src: ExPolygons, surface_templ: &Surface) {
        surfaces_append_templ_owned(&mut self.surfaces, src, surface_templ);
    }

    /// `append(const Surfaces &)`.
    pub fn append_surfaces(&mut self, src: &[Surface]) {
        surfaces_append(&mut self.surfaces, src);
    }

    /// `append(Surfaces &&)`.
    pub fn append_surfaces_owned(&mut self, src: Surfaces) {
        surfaces_append_owned(&mut self.surfaces, src);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(t: SurfaceType, x: i64) -> Surface {
        Surface::with_expolygon(
            t,
            ExPolygon::from_points(vec![
                Point::new(x, 0),
                Point::new(x + 10, 0),
                Point::new(x + 10, 10),
                Point::new(x, 10),
            ]),
        )
    }

    #[test]
    fn compaction_keeps_the_kept_in_order() {
        let mut c = SurfaceCollection::from(vec![
            typed(SurfaceType::Top, 0),
            typed(SurfaceType::Internal, 1),
            typed(SurfaceType::Top, 2),
            typed(SurfaceType::Bottom, 3),
            typed(SurfaceType::Top, 4),
        ]);
        let mut removed = ExPolygons::new();
        c.remove_type_into(SurfaceType::Top, &mut removed);
        let xs: Vec<i64> = c.iter().map(|s| s.expolygon.contour.points[0].x).collect();
        assert_eq!(xs, [1, 3]);
        let rx: Vec<i64> = removed.iter().map(|e| e.contour.points[0].x).collect();
        assert_eq!(rx, [0, 2, 4]);
    }

    #[test]
    fn group_by_the_first_surface_of_each_group() {
        let mut a = typed(SurfaceType::Top, 0);
        a.extra_perimeters = 3;
        let c = SurfaceCollection::from(vec![
            a,
            typed(SurfaceType::Internal, 1),
            typed(SurfaceType::Top, 2),
        ]);
        let mut groups = Vec::new();
        c.group(&mut groups);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].len(), 2);
        assert!(
            c.any_internal_contains(&Point::new(5, 5)) && !c.any_bottom_contains(&Point::new(5, 5))
        );
    }
}
