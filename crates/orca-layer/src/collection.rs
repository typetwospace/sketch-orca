// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/ExtrusionEntityCollection.hpp (1-173: the class
// ExtrusionEntityCollection and filter_by_extrusion_role) and ExtrusionEntityCollection.cpp
// (1-157) of OrcaSlicer v2.4.2, translated on 2026-11-13 (orca-o3-layer-1), the chaining in
// chained_path_from joined on 2026-11-14 (orca-o3-layer-3). Copyright the OrcaSlicer authors, the
// BambuStudio authors (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU
// Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! [`ExtrusionEntityCollection`]: entities that a generator or a layer owns together, in the
//! order they print, which may be fixed (`no_sort`) or left to the chaining.
//!
//! Its copy constructor clones each entity (the virtual `clone()`) and copies `no_sort` and
//! `is_reverse`, its `inset_idx` -1; its assignment ([`ExtrusionEntityCollection::assign_from`])
//! clones the entities and copies `no_sort` only, so the target keeps its `is_reverse` (and its
//! `inset_idx`). `can_reverse()` is false for a collection that may not be sorted, else its
//! `is_reverse`, which `set_reverse()` clears. Its role is the one role of its entities, `erNone`
//! for none and `erMixed` for several. `reverse()` reverses every entity but loops, then their
//! order. `flatten` copies the entities out of nested collections, keeping a collection that may
//! not be sorted whole when asked to preserve the order. `ExtrusionPaths(collection)`
//! ([`ExtrusionEntityCollection::to_paths`]) copies the paths it holds directly, each as a plain
//! path, and skips the rest.
//!
//! `chained_path_from` returns a copy of a collection that may not be sorted; otherwise it copies
//! the entities of the role asked (all for `erMixed`) and chains them with
//! `chain_and_reorder_extrusion_entities` of ShortestPath.cpp ([`crate::chain`], orca-o3-layer-3,
//! 2026-11-14), which first drops the entities without ends. A collection without entities has no
//! ends (the C++ reads outside its list); `first_point` and `last_point` stop there.

use core::borrow::Borrow;

use orca_geom::point::{Point, Points};
use orca_geom::point3::Point3;
use orca_geom::polygon::Polygons;
use orca_geom::polyline::{Polyline, Polylines};
use orca_index::stop::not_ported;

use crate::chain::chain_and_reorder_extrusion_entities;
use crate::cpp_vec;
use crate::entity::{ExtrusionEntitiesPtr, ExtrusionEntity, RuntimeError};
use crate::flow::FlowError;
use crate::path::{ExtrusionPath, ExtrusionPaths};
use crate::role::{ExtrusionRole, is_infill, is_perimeter, is_solid_infill};

/// The stop's condition for the ends of a collection without entities.
const NO_ENTITIES: &str =
    "no entities (the C++ reads its entities' front or back outside the list)";

/// `filter_by_extrusion_role_in_place(extrusion_entities, role)`: the entities of another role
/// removed, the order kept; nothing for `erMixed`.
pub fn filter_by_extrusion_role_in_place<E: Borrow<ExtrusionEntity>>(
    extrusion_entities: &mut Vec<E>,
    role: ExtrusionRole,
) {
    if role != ExtrusionRole::Mixed {
        extrusion_entities.retain(|ee| ee.borrow().role() == role);
    }
}

/// `filter_by_extrusion_role(extrusion_entities, role)`: the entities of that role (all for
/// `erMixed`), shared with the source, not cloned.
#[must_use]
pub fn filter_by_extrusion_role(
    extrusion_entities: &[ExtrusionEntity],
    role: ExtrusionRole,
) -> Vec<&ExtrusionEntity> {
    let mut out: Vec<&ExtrusionEntity> = extrusion_entities.iter().collect();
    filter_by_extrusion_role_in_place(&mut out, role);
    out
}

/// `ExtrusionEntityCollection`.
#[derive(Debug)]
pub struct ExtrusionEntityCollection {
    /// `entities`: owned.
    pub entities: ExtrusionEntitiesPtr,
    /// `no_sort`: the order is fixed.
    pub no_sort: bool,
    /// `ExtrusionEntity::inset_idx`.
    pub inset_idx: i32,
    /// `is_reverse`.
    is_reverse: bool,
}

impl Default for ExtrusionEntityCollection {
    /// `ExtrusionEntityCollection()`.
    fn default() -> Self {
        Self {
            entities: ExtrusionEntitiesPtr::new(),
            no_sort: false,
            inset_idx: -1,
            is_reverse: true,
        }
    }
}

impl Clone for ExtrusionEntityCollection {
    /// The copy constructor: `no_sort` and `is_reverse` copied, each entity cloned, the
    /// `inset_idx` -1.
    fn clone(&self) -> Self {
        let mut out = Self {
            entities: ExtrusionEntitiesPtr::new(),
            no_sort: self.no_sort,
            inset_idx: -1,
            is_reverse: self.is_reverse,
        };
        out.append_entities(&self.entities);
        out
    }
}

impl ExtrusionEntityCollection {
    /// `ExtrusionEntityCollection(const ExtrusionPaths &paths)`: each path cloned in.
    #[must_use]
    pub fn from_paths(paths: &[ExtrusionPath]) -> Self {
        let mut out = Self::default();
        out.append_paths(paths);
        out
    }

    /// `operator=(const ExtrusionEntityCollection &)`: the entities cloned and `no_sort` copied;
    /// `is_reverse` and `inset_idx` stay this collection's.
    pub fn assign_from(&mut self, other: &ExtrusionEntityCollection) {
        self.clear();
        self.entities = other.entities.clone();
        self.no_sort = other.no_sort;
    }

    /// `operator=(ExtrusionEntityCollection &&)`: the entities moved in, `no_sort` and
    /// `is_reverse` copied; `inset_idx` stays.
    pub fn assign_move(&mut self, other: ExtrusionEntityCollection) {
        self.entities = other.entities;
        self.no_sort = other.no_sort;
        self.is_reverse = other.is_reverse;
    }

    /// `explicit operator ExtrusionPaths()`: a copy of each entity that is a path (of any of the
    /// four path classes), as a plain path; the others skipped.
    #[must_use]
    pub fn to_paths(&self) -> ExtrusionPaths {
        let mut paths = ExtrusionPaths::new();
        for ptr in &self.entities {
            if let Some(path) = ptr.as_path() {
                cpp_vec::push(&mut paths, path.clone());
            }
        }
        paths
    }

    /// `is_collection()`: true.
    #[must_use]
    pub fn is_collection(&self) -> bool {
        true
    }

    /// `role()`: the entities' one role; `erNone` without entities, `erMixed` for several.
    #[must_use]
    pub fn role(&self) -> ExtrusionRole {
        let mut out = ExtrusionRole::None;
        for ee in &self.entities {
            let er = ee.role();
            out = if out == ExtrusionRole::None || out == er {
                er
            } else {
                ExtrusionRole::Mixed
            };
        }
        out
    }

    /// `has_perimeters()`: an entity whose role is a wall.
    #[must_use]
    pub fn has_perimeters(&self) -> bool {
        self.entities.iter().any(|ee| is_perimeter(ee.role()))
    }

    /// `has_infill()`.
    #[must_use]
    pub fn has_infill(&self) -> bool {
        self.entities.iter().any(|ee| is_infill(ee.role()))
    }

    /// `has_solid_infill()`.
    #[must_use]
    pub fn has_solid_infill(&self) -> bool {
        self.entities.iter().any(|ee| is_solid_infill(ee.role()))
    }

    /// `can_sort()`: not `no_sort`.
    #[must_use]
    pub fn can_sort(&self) -> bool {
        !self.no_sort
    }

    /// `can_reverse()`: false when `no_sort`, else `is_reverse`.
    #[must_use]
    pub fn can_reverse(&self) -> bool {
        if self.no_sort { false } else { self.is_reverse }
    }

    /// `set_reverse()`: `is_reverse` cleared.
    pub fn set_reverse(&mut self) {
        self.is_reverse = false;
    }

    /// `empty()`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// `clear()`: the entities dropped.
    pub fn clear(&mut self) {
        self.entities.clear();
    }

    /// `swap(c)`: the entities and `no_sort` swapped (not `is_reverse`).
    pub fn swap(&mut self, c: &mut ExtrusionEntityCollection) {
        core::mem::swap(&mut self.entities, &mut c.entities);
        core::mem::swap(&mut self.no_sort, &mut c.no_sort);
    }

    /// `append(const ExtrusionEntity &)`: a clone appended.
    pub fn append(&mut self, entity: &ExtrusionEntity) {
        self.entities.push(entity.clone());
    }

    /// `append(ExtrusionEntity &&)`: the entity's `clone_move()` appended.
    pub fn append_move(&mut self, entity: ExtrusionEntity) {
        self.entities.push(entity.clone_move());
    }

    /// `append(const ExtrusionEntitiesPtr &)`: a clone of each appended.
    pub fn append_entities(&mut self, entities: &[ExtrusionEntity]) {
        self.entities.reserve(entities.len());
        for ptr in entities {
            self.entities.push(ptr.clone());
        }
    }

    /// `append(ExtrusionEntitiesPtr &&)`: the entities moved in as they are.
    pub fn append_entities_move(&mut self, src: ExtrusionEntitiesPtr) {
        if self.entities.is_empty() {
            self.entities = src;
        } else {
            self.entities.extend(src);
        }
    }

    /// `append(const ExtrusionPaths &)`: a clone of each path (index -1).
    pub fn append_paths(&mut self, paths: &[ExtrusionPath]) {
        self.entities.reserve(paths.len());
        for path in paths {
            self.entities.push(ExtrusionEntity::Path(path.clone()));
        }
    }

    /// `append(ExtrusionPaths &&)`: each path moved in (index -1).
    pub fn append_paths_move(&mut self, paths: ExtrusionPaths) {
        self.entities.reserve(paths.len());
        for mut path in paths {
            path.inset_idx = -1;
            self.entities.push(ExtrusionEntity::Path(path));
        }
    }

    /// `replace(i, entity)`: entity `i` replaced by a clone.
    ///
    /// # Panics
    /// When `i` is past the end.
    pub fn replace(&mut self, i: usize, entity: &ExtrusionEntity) {
        self.entities[i] = entity.clone();
    }

    /// `remove(i)`: entity `i` dropped, the rest shifted down.
    ///
    /// # Panics
    /// When `i` is past the end.
    pub fn remove(&mut self, i: usize) {
        self.entities.remove(i);
    }

    /// `chained_path_from(extrusion_entities, start_near, role)`: the entities of `role` (all for
    /// `erMixed`) cloned into a new collection and chained from `start_near`
    /// ([`chain_and_reorder_extrusion_entities`], which drops the entities without ends).
    #[must_use]
    pub fn chained_path_from_entities(
        extrusion_entities: &[ExtrusionEntity],
        start_near: &Point,
        role: ExtrusionRole,
    ) -> ExtrusionEntityCollection {
        // A filtered copy of the collection, each entity cloned.
        let mut out = ExtrusionEntityCollection {
            entities: filter_by_extrusion_role(extrusion_entities, role)
                .into_iter()
                .cloned()
                .collect(),
            ..Default::default()
        };
        chain_and_reorder_extrusion_entities(&mut out.entities, Some(start_near));
        out
    }

    /// `chained_path_from(start_near, role)`: a copy when `no_sort`, else
    /// [`ExtrusionEntityCollection::chained_path_from_entities`] of the entities.
    #[must_use]
    pub fn chained_path_from(
        &self,
        start_near: &Point,
        role: ExtrusionRole,
    ) -> ExtrusionEntityCollection {
        if self.no_sort {
            self.clone()
        } else {
            Self::chained_path_from_entities(&self.entities, start_near, role)
        }
    }

    /// `reverse()`: every entity but loops reversed (a loop's winding is kept), then their order.
    pub fn reverse(&mut self) {
        for ptr in &mut self.entities {
            // A loop is not reversed: its order of elements stays and the caller may rely on its
            // winding.
            if !ptr.is_loop() {
                ptr.reverse();
            }
        }
        self.entities.reverse();
    }

    /// `first_point()`: the first entity's.
    ///
    /// # Panics
    /// Without entities, or where the first entity's stops.
    #[must_use]
    pub fn first_point(&self) -> Point {
        match self.entities.first() {
            Some(e) => e.first_point(),
            None => not_ported("ExtrusionEntityCollection::first_point", NO_ENTITIES),
        }
    }

    /// `first_point3()`.
    ///
    /// # Panics
    /// As [`ExtrusionEntityCollection::first_point`].
    #[must_use]
    pub fn first_point3(&self) -> Point3 {
        match self.entities.first() {
            Some(e) => e.first_point3(),
            None => not_ported("ExtrusionEntityCollection::first_point", NO_ENTITIES),
        }
    }

    /// `last_point()`: the last entity's.
    ///
    /// # Panics
    /// Without entities, or where the last entity's stops.
    #[must_use]
    pub fn last_point(&self) -> Point {
        match self.entities.last() {
            Some(e) => e.last_point(),
            None => not_ported("ExtrusionEntityCollection::last_point", NO_ENTITIES),
        }
    }

    /// `last_point3()`.
    ///
    /// # Panics
    /// As [`ExtrusionEntityCollection::last_point`].
    #[must_use]
    pub fn last_point3(&self) -> Point3 {
        match self.entities.last() {
            Some(e) => e.last_point3(),
            None => not_ported("ExtrusionEntityCollection::last_point", NO_ENTITIES),
        }
    }

    /// `polygons_covered_by_width(out, scaled_epsilon)`: each entity's, appended in order.
    pub fn polygons_covered_by_width(&self, out: &mut Polygons, scaled_epsilon: f32) {
        for entity in &self.entities {
            entity.polygons_covered_by_width(out, scaled_epsilon);
        }
    }

    /// `polygons_covered_by_spacing(out, scaled_epsilon)`: each entity's, appended in order.
    ///
    /// # Errors
    /// The first entity's [`FlowError`]; `out` keeps what came before it.
    pub fn polygons_covered_by_spacing(
        &self,
        out: &mut Polygons,
        scaled_epsilon: f32,
    ) -> Result<(), FlowError> {
        for entity in &self.entities {
            entity.polygons_covered_by_spacing(out, scaled_epsilon)?;
        }
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
    /// As [`ExtrusionEntityCollection::polygons_covered_by_spacing`].
    pub fn covered_by_spacing(&self, scaled_epsilon: f32) -> Result<Polygons, FlowError> {
        let mut out = Polygons::new();
        self.polygons_covered_by_spacing(&mut out, scaled_epsilon)?;
        Ok(out)
    }

    /// `items_count()`: the paths and loops held, nested collections counted through.
    #[must_use]
    pub fn items_count(&self) -> usize {
        let mut count = 0;
        for entity in &self.entities {
            if let ExtrusionEntity::Collection(c) = entity {
                count += c.items_count();
            } else {
                count += 1;
            }
        }
        count
    }

    /// `size()`: the number of entities.
    #[must_use]
    pub fn size(&self) -> usize {
        self.entities.len()
    }

    /// `flatten(preserve_ordering)`: a new collection of clones of every entity that is not a
    /// collection, nested ones included; with `preserve_ordering`, a collection that may not be
    /// sorted (this one included) is cloned whole instead.
    #[must_use]
    pub fn flatten(&self, preserve_ordering: bool) -> ExtrusionEntityCollection {
        let mut out = ExtrusionEntityCollection::default();
        flatten_recursive(&mut out, self, preserve_ordering);
        // `return flatten.out;` of a member: a copy.
        out.clone()
    }

    /// `min_mm3_per_mm()`: the least of the entities' (`DBL_MAX` without entities).
    #[must_use]
    pub fn min_mm3_per_mm(&self) -> f64 {
        let mut min_mm3_per_mm = f64::MAX;
        for entity in &self.entities {
            min_mm3_per_mm = orca_stl::minmax::min(min_mm3_per_mm, entity.min_mm3_per_mm());
        }
        min_mm3_per_mm
    }

    /// `total_volume()`: the entities' volumes summed in order.
    #[must_use]
    pub fn total_volume(&self) -> f64 {
        let mut volume = 0.;
        for ent in &self.entities {
            volume += ent.total_volume();
        }
        volume
    }

    /// `as_polyline()`: never called on a collection.
    ///
    /// # Errors
    /// Always: "Calling as_polyline() on a ExtrusionEntityCollection".
    pub fn as_polyline(&self) -> Result<Polyline, RuntimeError> {
        Err(RuntimeError(
            "Calling as_polyline() on a ExtrusionEntityCollection".to_string(),
        ))
    }

    /// `collect_polylines(dst)`: each entity's.
    pub fn collect_polylines(&self, dst: &mut Polylines) {
        for extrusion_entity in &self.entities {
            extrusion_entity.collect_polylines(dst);
        }
    }

    /// `collect_points(dst)`: each entity's.
    pub fn collect_points(&self, dst: &mut Points) {
        for extrusion_entity in &self.entities {
            extrusion_entity.collect_points(dst);
        }
    }

    /// `length()`: never called on a collection.
    ///
    /// # Errors
    /// Always: "Calling length() on a ExtrusionEntityCollection".
    pub fn length(&self) -> Result<f64, RuntimeError> {
        Err(RuntimeError(
            "Calling length() on a ExtrusionEntityCollection".to_string(),
        ))
    }
}

/// `Flatten::recursive_do(collection)`.
fn flatten_recursive(
    out: &mut ExtrusionEntityCollection,
    collection: &ExtrusionEntityCollection,
    preserve_ordering: bool,
) {
    if collection.no_sort && preserve_ordering {
        // Whatever is below this level stays as it is.
        out.entities
            .push(ExtrusionEntity::Collection(collection.clone()));
    } else {
        for entity in &collection.entities {
            if let ExtrusionEntity::Collection(c) = entity {
                flatten_recursive(out, c, preserve_ordering);
            } else {
                out.append(entity);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extrusion_loop::ExtrusionLoop;
    use crate::role::ExtrusionLoopRole;
    use orca_geom::point3::Point3;
    use orca_geom::polyline3::Polyline3;

    fn path(role: ExtrusionRole) -> ExtrusionEntity {
        let mut p = ExtrusionPath::new(role, 0.1, 0.4, 0.2);
        p.polyline = Polyline3::new(vec![Point3::new(0, 0, 0), Point3::new(10, 0, 0)]);
        ExtrusionEntity::Path(p)
    }

    #[test]
    fn an_assignment_keeps_is_reverse() {
        let mut c = ExtrusionEntityCollection::default();
        c.entities.push(path(ExtrusionRole::Perimeter));
        c.set_reverse();
        let copy = c.clone();
        assert!(!copy.can_reverse());
        let mut a = ExtrusionEntityCollection::default();
        a.assign_from(&c);
        assert!(a.can_reverse());
    }

    #[test]
    fn role_and_counts() {
        let mut inner = ExtrusionEntityCollection::default();
        inner.entities.push(path(ExtrusionRole::GapFill));
        inner
            .entities
            .push(ExtrusionEntity::Loop(ExtrusionLoop::new(
                ExtrusionLoopRole::DEFAULT,
            )));
        let mut c = ExtrusionEntityCollection::default();
        c.entities.push(path(ExtrusionRole::Perimeter));
        c.entities.push(ExtrusionEntity::Collection(inner));
        assert_eq!(c.items_count(), 3);
        assert_eq!(c.role(), ExtrusionRole::Mixed);
        assert_eq!(c.flatten(false).entities.len(), 3);
        let kept = filter_by_extrusion_role(&c.entities, ExtrusionRole::Perimeter);
        assert_eq!(kept.len(), 1);
    }
}
