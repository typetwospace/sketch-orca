// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: model_object_list_equal, model_object_list_extended,
// model_volume_list_changed (the template and its two overloads), model_property_changed,
// model_custom_supports_data_changed, model_custom_seam_data_changed,
// model_mmu_segmentation_data_changed, model_fuzzy_skin_data_changed,
// model_brim_points_data_changed, model_has_multi_part_objects and model_has_advanced_features of
// src/libslic3r/Model.cpp (lines 3602-3757), with BrimPoint::operator== of BrimEarsPoint.hpp
// (lines 50-56), of OrcaSlicer v2.4.2, translated on 2026-11-13 by orca-o3-front-2. Copyright the
// OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and the PrusaSlicer authors (Prusa
// Research), under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! The comparisons PrintApply makes between the model it was given and its own copy of the last
//! one, to decide what to keep: the objects by id and order, each object's parts of some types by
//! type, id and matrix, the painting by timestamp, the brim ears by value, and whether a model has
//! objects of several parts or settings beyond the extruder.
//!
//! A part's matrices are compared with Eigen's `isApprox` at its default precision (1e-12): the
//! squared norm of the difference of the two 4x4 matrices against 1e-24 times the smaller squared
//! norm, so a move of 1e-13 mm can pass as no change. A brim ear is equal to another at the same
//! position (each float exactly) with radii less than `float(EPSILON)` apart.
//!
//! `check_model_ids_validity` and `check_model_ids_equal` (`Model.cpp:3759-3817`) only assert,
//! under `#ifndef NDEBUG`, which the release build leaves out: they are not translated.

use crate::config::ModelConfig;
use crate::model::{BrimPoint, Model, ModelObject, ModelVolume, ModelVolumeType};

/// Eigen's `NumTraits<double>::dummy_precision()`, `isApprox`'s default.
const DUMMY_PRECISION: f64 = 1e-12;

/// `EPSILON` as the brim ears compare it: `float(EPSILON)`.
const BRIM_EPSILON: f32 = orca_geom::point::EPSILON as f32;

/// `model_object_list_equal(model_old, model_new)`: the same number of objects with the same ids
/// in the same order.
#[must_use]
pub fn model_object_list_equal(model_old: &Model, model_new: &Model) -> bool {
    if model_old.objects.len() != model_new.objects.len() {
        return false;
    }
    for i in 0..model_old.objects.len() {
        if model_old.objects[i].id() != model_new.objects[i].id() {
            return false;
        }
    }
    true
}

/// `model_object_list_extended(model_old, model_new)`: the new model's objects are the old ones,
/// by id and in order, with more after them.
#[must_use]
pub fn model_object_list_extended(model_old: &Model, model_new: &Model) -> bool {
    if model_old.objects.len() >= model_new.objects.len() {
        return false;
    }
    for i in 0..model_old.objects.len() {
        if model_old.objects[i].id() != model_new.objects[i].id() {
            return false;
        }
    }
    true
}

/// The template `model_volume_list_changed(model_object_old, model_object_new, type_filter)`: the
/// parts the filter keeps, walked in step, differ in number, type, id or matrix.
fn model_volume_list_changed_by(
    model_object_old: &ModelObject,
    model_object_new: &ModelObject,
    type_filter: &dyn Fn(ModelVolumeType) -> bool,
) -> bool {
    let mut i_old = 0;
    let mut i_new = 0;
    while i_old < model_object_old.volumes.len() && i_new < model_object_new.volumes.len() {
        let mv_old = &model_object_old.volumes[i_old];
        let mv_new = &model_object_new.volumes[i_new];
        if !type_filter(mv_old.volume_type()) {
            i_old += 1;
            continue;
        }
        if !type_filter(mv_new.volume_type()) {
            i_new += 1;
            continue;
        }
        if mv_old.volume_type() != mv_new.volume_type() || mv_old.id() != mv_new.id() {
            return true;
        }
        //FIXME test for the content of the mesh!
        if !mv_old
            .get_matrix()
            .is_approx(mv_new.get_matrix(), DUMMY_PRECISION)
        {
            return true;
        }
        i_old += 1;
        i_new += 1;
    }
    while i_old < model_object_old.volumes.len() {
        let mv_old = &model_object_old.volumes[i_old];
        if type_filter(mv_old.volume_type()) {
            // ModelVolume was deleted.
            return true;
        }
        i_old += 1;
    }
    while i_new < model_object_new.volumes.len() {
        let mv_new = &model_object_new.volumes[i_new];
        if type_filter(mv_new.volume_type()) {
            // ModelVolume was added.
            return true;
        }
        i_new += 1;
    }
    false
}

/// `model_volume_list_changed(model_object_old, model_object_new, type)`: for the parts of one
/// type.
#[must_use]
pub fn model_volume_list_changed(
    model_object_old: &ModelObject,
    model_object_new: &ModelObject,
    volume_type: ModelVolumeType,
) -> bool {
    model_volume_list_changed_by(model_object_old, model_object_new, &|t| t == volume_type)
}

/// `model_volume_list_changed(model_object_old, model_object_new, types)`: for the parts of any of
/// the types (`std::find` in the initializer list).
#[must_use]
pub fn model_volume_list_changed_of(
    model_object_old: &ModelObject,
    model_object_new: &ModelObject,
    types: &[ModelVolumeType],
) -> bool {
    model_volume_list_changed_by(model_object_old, model_object_new, &|t| types.contains(&t))
}

/// `model_property_changed(model_object_old, model_object_new, type_filter, compare)`: the parts
/// the filter keeps, walked in step, differ by `compare`. The C++ asserts first that the list of
/// those parts did not change (a release build does not check), and walks the two lists to the
/// shorter's end.
fn model_property_changed(
    model_object_old: &ModelObject,
    model_object_new: &ModelObject,
    type_filter: &dyn Fn(ModelVolumeType) -> bool,
    compare: &dyn Fn(&ModelVolume, &ModelVolume) -> bool,
) -> bool {
    let mut i_old = 0;
    let mut i_new = 0;
    while i_old < model_object_old.volumes.len() && i_new < model_object_new.volumes.len() {
        let mv_old = &model_object_old.volumes[i_old];
        let mv_new = &model_object_new.volumes[i_new];
        if !type_filter(mv_old.volume_type()) {
            i_old += 1;
            continue;
        }
        if !type_filter(mv_new.volume_type()) {
            i_new += 1;
            continue;
        }
        if !compare(mv_old, mv_new) {
            return true;
        }
        i_old += 1;
        i_new += 1;
    }
    false
}

/// The filter of the four painting comparisons: model parts.
fn is_model_part_type(t: ModelVolumeType) -> bool {
    t == ModelVolumeType::ModelPart
}

/// `model_custom_supports_data_changed(mo, mo_new)`: a model part's support painting has another
/// timestamp.
#[must_use]
pub fn model_custom_supports_data_changed(mo: &ModelObject, mo_new: &ModelObject) -> bool {
    model_property_changed(mo, mo_new, &is_model_part_type, &|mv_old, mv_new| {
        mv_old
            .supported_facets
            .timestamp_matches(&mv_new.supported_facets)
    })
}

/// `model_custom_seam_data_changed(mo, mo_new)`: a model part's seam painting has another
/// timestamp.
#[must_use]
pub fn model_custom_seam_data_changed(mo: &ModelObject, mo_new: &ModelObject) -> bool {
    model_property_changed(mo, mo_new, &is_model_part_type, &|mv_old, mv_new| {
        mv_old.seam_facets.timestamp_matches(&mv_new.seam_facets)
    })
}

/// `model_mmu_segmentation_data_changed(mo, mo_new)`: a model part's colour painting has another
/// timestamp.
#[must_use]
pub fn model_mmu_segmentation_data_changed(mo: &ModelObject, mo_new: &ModelObject) -> bool {
    model_property_changed(mo, mo_new, &is_model_part_type, &|mv_old, mv_new| {
        mv_old
            .mmu_segmentation_facets
            .timestamp_matches(&mv_new.mmu_segmentation_facets)
    })
}

/// `model_fuzzy_skin_data_changed(mo, mo_new)`: a model part's fuzzy-skin painting has another
/// timestamp.
#[must_use]
pub fn model_fuzzy_skin_data_changed(mo: &ModelObject, mo_new: &ModelObject) -> bool {
    model_property_changed(mo, mo_new, &is_model_part_type, &|mv_old, mv_new| {
        mv_old
            .fuzzy_skin_facets
            .timestamp_matches(&mv_new.fuzzy_skin_facets)
    })
}

/// `BrimPoint::operator==`: the same position, each float exactly, and radii less than
/// `float(EPSILON)` apart. `operator!=` asks it of the other point: `!(sp == *this)`.
fn brim_point_equal(this: &BrimPoint, sp: &BrimPoint) -> bool {
    let rdiff = (this.head_front_radius - sp.head_front_radius).abs();
    this.pos.x() == sp.pos.x()
        && this.pos.y() == sp.pos.y()
        && this.pos.z() == sp.pos.z()
        && rdiff < BRIM_EPSILON
}

/// `model_brim_points_data_changed(mo, mo_new)`: the brim ears differ in number or in one ear.
#[must_use]
pub fn model_brim_points_data_changed(mo: &ModelObject, mo_new: &ModelObject) -> bool {
    if mo.brim_points.len() != mo_new.brim_points.len() {
        return true;
    }
    for i in 0..mo.brim_points.len() {
        // mo.brim_points[i] != mo_new.brim_points[i]
        if !brim_point_equal(&mo_new.brim_points[i], &mo.brim_points[i]) {
            return true;
        }
    }
    false
}

/// `model_has_multi_part_objects(model)`: an object has other than one part, or its one part is
/// not a model part.
#[must_use]
pub fn model_has_multi_part_objects(model: &Model) -> bool {
    for model_object in &model.objects {
        if model_object.volumes.len() != 1 || !model_object.volumes[0].is_model_part() {
            return true;
        }
    }
    false
}

/// `config_is_advanced` of `model_has_advanced_features`: settings other than none or the
/// extruder alone.
fn config_is_advanced(config: &ModelConfig) -> bool {
    !(config.is_empty()
        || (config.iter().count() == 1
            && config.iter().next().is_some_and(|(k, _)| k == "extruder")))
}

/// `model_has_advanced_features(model)`: an object with more than one instance, a part that is not
/// a model part, or an object or part with settings beyond the extruder.
#[must_use]
pub fn model_has_advanced_features(model: &Model) -> bool {
    for model_object in &model.objects {
        // Is there more than one instance or advanced config data?
        if model_object.instances.len() > 1 || config_is_advanced(&model_object.config) {
            return true;
        }
        // Is there any modifier or advanced config data?
        for model_volume in &model_object.volumes {
            if !model_volume.is_model_part() || config_is_advanced(&model_volume.config) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use orca_eigen::{Vec3d, Vec3f};
    use orca_mesh::triangle_mesh::TriangleMesh;

    fn tetrahedron() -> TriangleMesh {
        TriangleMesh::from_vertices_faces(
            vec![
                Vec3f::new(0.0, 0.0, 0.0),
                Vec3f::new(1.0, 0.0, 0.0),
                Vec3f::new(0.0, 1.0, 0.0),
                Vec3f::new(0.0, 0.0, 1.0),
            ],
            vec![
                orca_eigen::Vec3i32::new(0, 2, 1),
                orca_eigen::Vec3i32::new(0, 1, 3),
                orca_eigen::Vec3i32::new(0, 3, 2),
                orca_eigen::Vec3i32::new(1, 2, 3),
            ],
        )
    }

    fn one_object_model() -> Model {
        let mut a = Model::default();
        let o = a.add_empty_object();
        a.objects[o].add_volume(tetrahedron(), ModelVolumeType::ModelPart, false);
        a.objects[o].add_instance();
        a
    }

    #[test]
    fn a_copy_compares_equal_and_a_clone_does_not() {
        let a = one_object_model();
        let b = a.clone();
        assert!(model_object_list_equal(&a, &b));
        assert!(!model_object_list_extended(&a, &b));
        assert!(!model_volume_list_changed(
            &a.objects[0],
            &b.objects[0],
            ModelVolumeType::ModelPart
        ));
        let mut c = a.clone();
        c.add_object_copy(&a.objects[0]);
        assert!(model_object_list_extended(&a, &c));
        assert!(!model_object_list_equal(&a, &c));
        assert_ne!(c.objects[1].id(), a.objects[0].id());
    }

    #[test]
    fn a_tiny_move_is_no_change() {
        let a = one_object_model();
        let mut b = a.clone();
        let off = b.objects[0].volumes[0].get_offset();
        b.objects[0].volumes[0].set_offset(&(off + Vec3d::new(1e-13, 0.0, 0.0)));
        assert!(!model_volume_list_changed(
            &a.objects[0],
            &b.objects[0],
            ModelVolumeType::ModelPart
        ));
        b.objects[0].volumes[0].set_offset(&(off + Vec3d::new(1.0, 0.0, 0.0)));
        assert!(model_volume_list_changed(
            &a.objects[0],
            &b.objects[0],
            ModelVolumeType::ModelPart
        ));
    }

    #[test]
    fn a_reset_changes_the_timestamp() {
        let a = one_object_model();
        let mut b = a.clone();
        assert!(!model_custom_seam_data_changed(
            &a.objects[0],
            &b.objects[0]
        ));
        b.objects[0].volumes[0].seam_facets.reset();
        assert!(model_custom_seam_data_changed(&a.objects[0], &b.objects[0]));
    }
}
