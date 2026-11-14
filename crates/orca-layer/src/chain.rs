// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/ShortestPath.hpp (21-28 and 48-55: the declarations of
// the entity, path and instance chaining) and ShortestPath.cpp (20-41, 1026-1095 and 2015-2042)
// of OrcaSlicer v2.4.2, translated on 2026-11-14 (orca-o3-layer-3). Copyright the OrcaSlicer
// authors, the BambuStudio authors (Bambu Lab), the PrusaSlicer authors (Prusa Research) and the
// Slic3r authors, under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! The chaining of extrusion entities, of extrusion paths and of print object instances
//! (ShortestPath.cpp's parts that hold extrusion entities and print objects), on orca-paths'
//! greedy chaining of segments.
//!
//! [`chain_and_reorder_extrusion_entities`] first drops the entities without both ends
//! ([`extrusion_entity_has_endpoints`], Orca's guard for support entities that hold empty nested
//! paths), then chains the rest by their first and last points with
//! `chain_segments_greedy_constrained_reversals` (a loop counts as reversible there, and its
//! reversal is then dropped, as its start is its end) and moves them into that order, reversing
//! the ones the chain flags. The entities are the C++'s pointers, so moving them changes nothing
//! in them. [`chain_and_reorder_extrusion_paths`] chains paths by value with `chain_segments_greedy`
//! (every path free to reverse); the reorder moves each path into a new vector, so every moved
//! path's `inset_idx` is -1 (its move constructor's), through [`crate::cpp_vec`].
//!
//! [`chain_print_object_instances`] orders the instances of the given print objects by their
//! shifts, with no start for the print's own objects. The C++ returns pointers to the instances;
//! here each is the pair of its object's index (in the list given) and its index in that object's
//! instances.
//!
//! ShortestPath.hpp declares `reorder_extrusion_paths` with a chain it may change, while
//! ShortestPath.cpp defines it with a chain it only reads (no caller reaches the declared one);
//! [`reorder_extrusion_paths`] takes the chain to read.

use orca_geom::point::Point;
use orca_paths::segments::{
    Chain, chain_segments_greedy, chain_segments_greedy_constrained_reversals,
};

use crate::cpp_vec;
use crate::entity::{ExtrusionEntitiesPtr, ExtrusionEntity};
use crate::extrusion_loop::ExtrusionLoopSloped;
use crate::path::{ExtrusionPath, ExtrusionPaths};
use crate::print::Print;
use crate::print_object::PrintObject;

/// `extrusion_entity_has_endpoints(entity)` (Orca): a collection whose first and last entities
/// have ends; a path of any class with points; a multi path or loop (of either class) whose first
/// and last paths have points. (The C++'s null pointer, which has none, cannot occur here.)
#[must_use]
pub fn extrusion_entity_has_endpoints(entity: &ExtrusionEntity) -> bool {
    let paths_have_endpoints = |paths: &ExtrusionPaths| -> bool {
        !paths.is_empty()
            && !paths[0].polyline.points.is_empty()
            && !paths[paths.len() - 1].polyline.points.is_empty()
    };
    match entity {
        ExtrusionEntity::Collection(collection) => {
            !collection.entities.is_empty()
                && extrusion_entity_has_endpoints(&collection.entities[0])
                && extrusion_entity_has_endpoints(
                    &collection.entities[collection.entities.len() - 1],
                )
        }
        ExtrusionEntity::MultiPath(multipath) => paths_have_endpoints(&multipath.paths),
        ExtrusionEntity::Loop(l)
        | ExtrusionEntity::LoopSloped(ExtrusionLoopSloped { base: l, .. }) => {
            paths_have_endpoints(&l.paths)
        }
        path => path
            .as_path()
            .is_some_and(|p| !p.polyline.points.is_empty()),
    }
}

/// `chain_extrusion_entities(entities, start_near)`: the order and the reversals of the
/// entities, a loop never reversed.
///
/// # Panics
/// Where an entity's `first_point` or `last_point` stops (an entity without ends).
#[must_use]
pub fn chain_extrusion_entities(
    entities: &ExtrusionEntitiesPtr,
    start_near: Option<&Point>,
) -> Chain {
    let segment_end_point = |idx: usize, first_point: bool| -> Point {
        if first_point {
            entities[idx].first_point()
        } else {
            entities[idx].last_point()
        }
    };
    let could_reverse = |idx: usize| -> bool {
        let ee = &entities[idx];
        ee.is_loop() || ee.can_reverse()
    };
    let mut out = chain_segments_greedy_constrained_reversals(
        segment_end_point,
        could_reverse,
        entities.len(),
        start_near,
    );
    for segment in &mut out {
        let ee = &entities[segment.0];
        if ee.is_loop() {
            // Ignore reversals for loops, as the start point equals the end point.
            segment.1 = false;
        }
        // Is can_reverse() respected by the reversals?
        debug_assert!(ee.can_reverse() || !segment.1);
    }
    out
}

/// `reorder_extrusion_entities(entities, chain)`: the entities moved into the chain's order,
/// each one the chain flags reversed.
pub fn reorder_extrusion_entities(entities: &mut ExtrusionEntitiesPtr, chain: &Chain) {
    debug_assert_eq!(entities.len(), chain.len());
    let mut slots: Vec<Option<ExtrusionEntity>> = entities.drain(..).map(Some).collect();
    let mut out = ExtrusionEntitiesPtr::with_capacity(slots.len());
    for idx in chain {
        let Some(ee) = slots[idx.0].take() else {
            unreachable!("a chain names each entity once")
        };
        out.push(ee);
        if idx.1 {
            let last = out.len() - 1;
            out[last].reverse();
        }
    }
    core::mem::swap(entities, &mut out);
}

/// `chain_and_reorder_extrusion_entities(entities, start_near)`: the entities without both ends
/// dropped (the order of the rest kept), the rest chained from `start_near` and reordered.
pub fn chain_and_reorder_extrusion_entities(
    entities: &mut ExtrusionEntitiesPtr,
    start_near: Option<&Point>,
) {
    // Orca: Reordering queries first_point() / last_point(); drop entities that cannot provide
    // valid endpoints.
    entities.retain(extrusion_entity_has_endpoints);
    let chain = chain_extrusion_entities(entities, start_near);
    reorder_extrusion_entities(entities, &chain);
}

/// `chain_extrusion_paths(extrusion_paths, start_near)`: the paths' order and reversals, every
/// path free to reverse.
///
/// # Panics
/// Where a path's `first_point` or `last_point` stops (an empty path).
#[must_use]
pub fn chain_extrusion_paths(
    extrusion_paths: &[ExtrusionPath],
    start_near: Option<&Point>,
) -> Chain {
    let segment_end_point = |idx: usize, first_point: bool| -> Point {
        if first_point {
            extrusion_paths[idx].first_point()
        } else {
            extrusion_paths[idx].last_point()
        }
    };
    chain_segments_greedy(segment_end_point, extrusion_paths.len(), start_near)
}

/// `reorder_extrusion_paths(extrusion_paths, chain)`: nothing for no paths; else each path moved
/// into a new vector in the chain's order (its `inset_idx` -1, as the move constructor leaves
/// it) and reversed where the chain says, the new vector then swapped in.
pub fn reorder_extrusion_paths(extrusion_paths: &mut ExtrusionPaths, chain: &Chain) {
    debug_assert_eq!(extrusion_paths.len(), chain.len());
    if extrusion_paths.is_empty() {
        return;
    }
    let mut slots: Vec<Option<ExtrusionPath>> = extrusion_paths.drain(..).map(Some).collect();
    let mut out = ExtrusionPaths::new();
    cpp_vec::reserve(&mut out, slots.len());
    for idx in chain {
        // The C++ moves out of the slot; a chain names each path once.
        let Some(path) = slots[idx.0].take() else {
            unreachable!("a chain names each path once")
        };
        cpp_vec::push(&mut out, path);
        if idx.1 {
            let last = out.len() - 1;
            out[last].reverse();
        }
    }
    core::mem::swap(extrusion_paths, &mut out);
}

/// `chain_and_reorder_extrusion_paths(extrusion_paths, start_near)`.
///
/// # Panics
/// As [`chain_extrusion_paths`].
pub fn chain_and_reorder_extrusion_paths(
    extrusion_paths: &mut ExtrusionPaths,
    start_near: Option<&Point>,
) {
    let chain = chain_extrusion_paths(extrusion_paths, start_near);
    reorder_extrusion_paths(extrusion_paths, &chain);
}

/// `chain_print_object_instances(print_objects, start_near)` (BBS): every instance of the given
/// objects, ordered by a nearest-neighbour search over their shifts, each as (the object's index
/// in `print_objects`, the instance's index in its `instances()`).
#[must_use]
pub fn chain_print_object_instances(
    print_objects: &[&PrintObject],
    start_near: Option<&Point>,
) -> Vec<(usize, usize)> {
    // Order objects using a nearest neighbor search.
    let mut object_reference_points: Vec<Point> = Vec::new();
    let mut instances: Vec<(usize, usize)> = Vec::new();
    for (i, object) in print_objects.iter().enumerate() {
        for j in 0..object.instances().len() {
            // Sliced PrintObjects are centered, object.instances()[j].shift is the center of the
            // PrintObject in G-code coordinates.
            object_reference_points.push(object.instances()[j].shift);
            instances.push((i, j));
        }
    }
    let segment_end_point =
        |idx: usize, _first_point: bool| -> Point { object_reference_points[idx] };
    let ordered = chain_segments_greedy(segment_end_point, instances.len(), start_near);
    let mut out = Vec::with_capacity(instances.len());
    for segment_and_reversal in &ordered {
        out.push(instances[segment_and_reversal.0]);
    }
    out
}

/// `chain_print_object_instances(print)`: the print's objects' instances, with no start; the
/// first of each pair is the object's index in `print.objects()`.
#[must_use]
pub fn chain_print_object_instances_of_print(print: &Print) -> Vec<(usize, usize)> {
    let objects: Vec<&PrintObject> = print.objects().iter().collect();
    chain_print_object_instances(&objects, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collection::ExtrusionEntityCollection;
    use crate::extrusion_loop::ExtrusionLoop;
    use crate::multi_path::ExtrusionMultiPath;
    use crate::role::{ExtrusionLoopRole, ExtrusionRole};
    use orca_eigen::Transform3d;
    use orca_geom::point3::Point3;
    use orca_geom::polyline3::Polyline3;

    fn path(a: (i64, i64), b: (i64, i64)) -> ExtrusionPath {
        let mut p = ExtrusionPath::new(ExtrusionRole::Perimeter, 0.1, 0.4, 0.2);
        p.polyline = Polyline3::new(vec![Point3::new(a.0, a.1, 0), Point3::new(b.0, b.1, 0)]);
        p
    }

    #[test]
    fn entities_without_ends_are_dropped_and_the_rest_chained() {
        let mut entities: ExtrusionEntitiesPtr = vec![
            ExtrusionEntity::Path(path((100, 0), (90, 0))),
            ExtrusionEntity::Path(ExtrusionPath::new(ExtrusionRole::Perimeter, 0.1, 0.4, 0.2)),
            ExtrusionEntity::MultiPath(ExtrusionMultiPath::default()),
            ExtrusionEntity::Loop(ExtrusionLoop::new(ExtrusionLoopRole::DEFAULT)),
            ExtrusionEntity::Collection(ExtrusionEntityCollection::default()),
            ExtrusionEntity::Path(path((0, 0), (10, 0))),
        ];
        chain_and_reorder_extrusion_entities(&mut entities, Some(&Point::new(0, 0)));
        assert_eq!(entities.len(), 2);
        assert_eq!(entities[0].first_point(), Point::new(0, 0));
        // The second path is reversed to start nearest the first's end.
        assert_eq!(entities[1].first_point(), Point::new(90, 0));
    }

    #[test]
    fn reordered_paths_lose_their_inset() {
        let mut a = path((50, 0), (40, 0));
        a.inset_idx = 3;
        let mut paths = vec![a, path((0, 0), (10, 0))];
        chain_and_reorder_extrusion_paths(&mut paths, Some(&Point::new(0, 0)));
        assert_eq!(paths[0].first_point(), Point::new(0, 0));
        assert_eq!(paths[1].first_point(), Point::new(40, 0));
        assert!(paths.iter().all(|p| p.inset_idx == -1));
    }

    #[test]
    fn instances_are_chained_by_their_shifts() {
        let mut a = PrintObject::with_members(0, Transform3d::identity());
        a.instances.push(crate::print_object::PrintInstance::new(
            0,
            Point::new(100, 0),
        ));
        let mut b = PrintObject::with_members(1, Transform3d::identity());
        b.instances
            .push(crate::print_object::PrintInstance::new(0, Point::new(0, 0)));
        b.instances.push(crate::print_object::PrintInstance::new(
            1,
            Point::new(50, 0),
        ));
        let order = chain_print_object_instances(&[&a, &b], Some(&Point::new(0, 0)));
        assert_eq!(order, vec![(1, 0), (1, 1), (0, 0)]);
    }
}
