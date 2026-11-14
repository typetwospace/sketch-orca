// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: classes Model, ModelObject, ModelVolume and ModelInstance of
// src/libslic3r/Model.hpp and Model.cpp (the parts the command line's loading, print volume check
// and arrangement use), of OrcaSlicer v2.4.2, translated on 2026-11-07 (Model::delete_object,
// Model::get_backup_path and ModelProfileInfo, ModelDesignInfo and ModelInfo, for the 3MF reader,
// on 2026-11-10; Model::add_object(const ModelObject&), ModelObject::delete_last_instance and
// add_volume_with_shared_mesh, ModelVolume::CutInfo, the parts' cut information, text
// configuration and emboss shape, the objects' cut id, CutObjectBase of
// src/libslic3r/ObjectID.hpp and CutConnectorType, for the 3MF reader's assembly, on
// 2026-11-11; Model::assign_copy in both forms (Model.cpp:59-152), Model::is_mm_painted and
// is_fuzzy_skin_painted (1086-1094), ModelObject::assign_copy in both forms (1121-1196),
// ModelObject::is_mm_painted and is_fuzzy_skin_painted (1356-1364) and ModelObject::raw_mesh
// (1518-1529), with the helpers no other piece carries: Model::clear_objects (571-581),
// clear_materials (629-634), is_fdm_support_painted and is_seam_painted (1076-1084),
// ModelObject::clear_volumes (1336-1344), is_fdm_support_painted and is_seam_painted
// (1346-1354), assign_new_unique_ids_recursive (1198-1206, 2844-2852), copy_transformation_caches,
// ModelVolume's painted queries (Model.hpp:1012-1015), ModelMaterial and the ids of
// ObjectBase, on 2026-11-13 by orca-o3-front-2). Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and the
// PrusaSlicer authors (Prusa Research), under the GNU Affero General Public License, version 3
// (AGPL-3.0; LICENSE).

//! The model: objects, their parts (volumes) and their placed copies (instances).
//!
//! The C++ links a part and an instance back to its object; here an object owns its parts and
//! instances, and what needs the object (an instance's bounding box, its print volume state, its
//! arrange polygon) is a method of the object given the instance's index. The C++ caches it
//! fills in `const` methods (the object's bounding boxes, a part's 2D hull) are filled here by
//! methods that take `&mut self`, at the same moments, so a cache that goes stale in the C++ goes
//! stale here too (`ModelObject::translate_instance` invalidates the boxes, setting an instance's
//! offset directly does not).
//!
//! A part with fewer than two triangles gets no convex hull (`ModelVolume`'s constructor computes
//! it from two up), and the C++ dereferences the missing hull wherever it reads it; here
//! [`ModelVolume::get_convex_hull`] panics there. The command line refuses such parts before any
//! such read (`check_solid_objects`). A model, an object, a part and an instance hold their
//! object id (`ObjectBase`, [`crate::object_id`]; added by orca-o3-front-2, 2026-11-13): a new one
//! when made, kept by a copy, renewed by a clone ([`Model::add_object_copy`]), as PrintApply reads
//! them; the ids of configs and paintings are not kept. The model's backup folder
//! (`Model::get_backup_path`) is named by an id of its own (`parity/HAZARDS.md`, orca-model).
//!
//! What PrintApply and the slicer call on a model (orca-o3-front-2, 2026-11-13): the copies
//! ([`Model::assign_copy`], [`Model::assign_move`], [`ModelObject::assign_copy`],
//! [`ModelObject::assign_move`]; `Clone` of a model or an object is the C++'s copy constructor,
//! which runs `assign_copy`, so a copied object's bounding boxes are invalid, as the C++'s
//! `clear_volumes` and `clear_instances` inside it leave them), the `clear_` functions, the
//! painted queries and [`ModelObject::raw_mesh`]. The comparisons PrintApply makes between two
//! models are [`crate::compare`]'s.
//!
//! The model's information (`design_info`, `profile_info`, `model_info`) is what a 3MF file's
//! metadata give; the C++ holds each in a `shared_ptr`, here an `Option`. `~Model` asks the backup
//! thread to remove the backup folder (`remove_backup`, `bbs_3mf.cpp:8652`); that thread is not
//! translated, so the folder stays in the temporary folder.
//!
//! A part's mesh is shared, as the C++'s `shared_ptr<const TriangleMesh>` is: a copied part and a
//! part made with [`ModelObject::add_volume_with_shared_mesh`] hold the same mesh (an `Arc`).
//! `center_geometry_after_creation` moves the mesh in place through a `const_cast` in the C++, so
//! every part that shares it moves; here a shared mesh is copied first (`Arc::make_mut`). Only a
//! new part, whose mesh nothing shares yet, is centred on what the command line runs.
//!
//! The cut information (an object's `cut_id`, a part's `cut_info`) and a part's text
//! configuration and emboss shape are what a 3MF file gives; the command line reads them again
//! only when it writes a `.gcode.3mf` (`cli/run.cpp`, `export_bambu_3mf`: `store_bbs_3mf` writes
//! the cut information file and each part's text and shape, `bbs_3mf.cpp:7419-7462`,
//! `:7882-7887`).

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use orca_eigen::{Transform3d, Vec3d};
use orca_geom::bounding_box::BoundingBoxf3;
use orca_geom::convex_hull::convex_hull;
use orca_geom::multi_point::translate_f64;
use orca_geom::point::{Point, Points, scale_};
use orca_geom::polygon::Polygon;
use orca_mesh::log::{Severity, log};
use orca_mesh::triangle_mesh::TriangleMesh;

use crate::config::ModelConfig;
use crate::emboss::{EmbossShape, TextConfiguration};
use crate::facets::FacetsAnnotation;
use crate::object_id::ObjectID;
use crate::transformation::{Transformation, X, Y, Z};

/// `SINKING_Z_THRESHOLD`, a `float`.
pub const SINKING_Z_THRESHOLD: f32 = -0.001;
/// `SINKING_MIN_Z_THRESHOLD`.
pub const SINKING_MIN_Z_THRESHOLD: f64 = 0.05;

/// `ModelVolumeType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelVolumeType {
    /// `INVALID` (-1).
    Invalid,
    /// `MODEL_PART` (0).
    ModelPart,
    /// `NEGATIVE_VOLUME` (1).
    NegativeVolume,
    /// `PARAMETER_MODIFIER` (2).
    ParameterModifier,
    /// `SUPPORT_BLOCKER` (3).
    SupportBlocker,
    /// `SUPPORT_ENFORCER` (4).
    SupportEnforcer,
}

impl ModelVolumeType {
    /// The enum's value.
    #[must_use]
    pub fn value(self) -> i32 {
        match self {
            ModelVolumeType::Invalid => -1,
            ModelVolumeType::ModelPart => 0,
            ModelVolumeType::NegativeVolume => 1,
            ModelVolumeType::ParameterModifier => 2,
            ModelVolumeType::SupportBlocker => 3,
            ModelVolumeType::SupportEnforcer => 4,
        }
    }

    /// `ModelVolume::type_from_string(s)` (Model.cpp:2717-2733): the subtypes the 3MF files name,
    /// `MODEL_PART` for any other string.
    #[must_use]
    pub fn from_string(s: &str) -> ModelVolumeType {
        // New type (supporting the support enforcers & blockers)
        match s {
            "normal_part" => ModelVolumeType::ModelPart,
            "negative_part" => ModelVolumeType::NegativeVolume,
            "modifier_part" => ModelVolumeType::ParameterModifier,
            "support_enforcer" => ModelVolumeType::SupportEnforcer,
            "support_blocker" => ModelVolumeType::SupportBlocker,
            // Default value if invalud type string received.
            _ => ModelVolumeType::ModelPart,
        }
    }

    /// `ModelVolume::type_to_string(t)` (Model.cpp:2736-2748): `normal_part` for `INVALID`, as
    /// the release build's `default` answers.
    #[must_use]
    pub fn to_str(self) -> &'static str {
        match self {
            ModelVolumeType::ModelPart | ModelVolumeType::Invalid => "normal_part",
            ModelVolumeType::NegativeVolume => "negative_part",
            ModelVolumeType::ParameterModifier => "modifier_part",
            ModelVolumeType::SupportEnforcer => "support_enforcer",
            ModelVolumeType::SupportBlocker => "support_blocker",
        }
    }
}

/// `ModelInstanceEPrintVolumeState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrintVolumeState {
    /// `ModelInstancePVS_Inside` (0).
    Inside,
    /// `ModelInstancePVS_Limited` (1).
    Limited,
    /// `ModelInstancePVS_Partly_Outside` (2).
    PartlyOutside,
    /// `ModelInstancePVS_Fully_Outside` (3).
    FullyOutside,
}

impl PrintVolumeState {
    /// The enum's value.
    #[must_use]
    pub fn value(self) -> u8 {
        match self {
            PrintVolumeState::Inside => 0,
            PrintVolumeState::Limited => 1,
            PrintVolumeState::PartlyOutside => 2,
            PrintVolumeState::FullyOutside => 3,
        }
    }
}

/// `ModelVolume::Source`: where a part came from.
#[derive(Clone, Debug, PartialEq)]
pub struct VolumeSource {
    /// `input_file`.
    pub input_file: String,
    /// `object_idx`.
    pub object_idx: i32,
    /// `volume_idx`.
    pub volume_idx: i32,
    /// `mesh_offset`.
    pub mesh_offset: Vec3d,
    /// `transform`.
    pub transform: Transformation,
    /// `is_converted_from_inches`.
    pub is_converted_from_inches: bool,
    /// `is_converted_from_meters`.
    pub is_converted_from_meters: bool,
    /// `is_from_builtin_objects`.
    pub is_from_builtin_objects: bool,
}

impl Default for VolumeSource {
    fn default() -> Self {
        VolumeSource {
            input_file: String::new(),
            object_idx: -1,
            volume_idx: -1,
            mesh_offset: Vec3d::zero(),
            transform: Transformation::default(),
            is_converted_from_inches: false,
            is_converted_from_meters: false,
            is_from_builtin_objects: false,
        }
    }
}

/// `CutConnectorType`, an `enum class : int` (Plug 0, Dowel 1, Snap 2, Undef 3): the value as the
/// cast `CutConnectorType(type)` keeps it, any `int` included.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CutConnectorType(pub i32);

impl CutConnectorType {
    /// `Plug`.
    pub const PLUG: CutConnectorType = CutConnectorType(0);
    /// `Dowel`.
    pub const DOWEL: CutConnectorType = CutConnectorType(1);
    /// `Snap`.
    pub const SNAP: CutConnectorType = CutConnectorType(2);
    /// `Undef`.
    pub const UNDEF: CutConnectorType = CutConnectorType(3);
}

/// `ModelVolume::CutInfo`: whether a part is a cut's connector, and its type and tolerances.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CutInfo {
    /// `is_from_upper`.
    pub is_from_upper: bool,
    /// `is_connector`.
    pub is_connector: bool,
    /// `is_processed`.
    pub is_processed: bool,
    /// `connector_type`.
    pub connector_type: CutConnectorType,
    /// `radius_tolerance`, a `float`.
    pub radius_tolerance: f32,
    /// `height_tolerance`, a `float`.
    pub height_tolerance: f32,
}

impl Default for CutInfo {
    /// `CutInfo()`.
    fn default() -> Self {
        CutInfo {
            is_from_upper: true,
            is_connector: false,
            is_processed: true,
            connector_type: CutConnectorType::PLUG,
            radius_tolerance: 0.0,
            height_tolerance: 0.0,
        }
    }
}

impl CutInfo {
    /// `CutInfo(type, rad_tolerance, h_tolerance, processed)`: a connector.
    #[must_use]
    pub fn connector(
        connector_type: CutConnectorType,
        radius_tolerance: f32,
        height_tolerance: f32,
        is_processed: bool,
    ) -> Self {
        CutInfo {
            is_connector: true,
            is_processed,
            connector_type,
            radius_tolerance,
            height_tolerance,
            ..CutInfo::default()
        }
    }
}

/// `CutObjectBase` (`ObjectID.hpp`): the cut's id, 0 (invalid) for an object that is no cut's
/// part, its parts' check sum and its connectors' count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CutObjectBase {
    /// `id().id`.
    pub id: u64,
    /// `check_sum()`.
    pub check_sum: u64,
    /// `connectors_cnt()`.
    pub connectors_cnt: u64,
}

impl Default for CutObjectBase {
    /// `CutObjectBase()`: `ObjectBase(-1)`, the `int` overload, which gives the id 0.
    fn default() -> Self {
        CutObjectBase {
            id: 0,
            check_sum: 1,
            connectors_cnt: 0,
        }
    }
}

/// `ModelVolume`: one part of an object, its mesh in the object's coordinates.
#[derive(Clone, Debug)]
pub struct ModelVolume {
    /// `name`.
    pub name: String,
    /// `source`.
    pub source: VolumeSource,
    /// `config`: the part's own settings.
    pub config: ModelConfig,
    /// `supported_facets`: painted supports.
    pub supported_facets: FacetsAnnotation,
    /// `seam_facets`: painted seams.
    pub seam_facets: FacetsAnnotation,
    /// `mmu_segmentation_facets`: painted colours.
    pub mmu_segmentation_facets: FacetsAnnotation,
    /// `fuzzy_skin_facets`: painted fuzzy skin.
    pub fuzzy_skin_facets: FacetsAnnotation,
    /// `cut_info`.
    pub cut_info: CutInfo,
    /// `text_configuration`: set only for a text part.
    pub text_configuration: Option<TextConfiguration>,
    /// `emboss_shape`: set only for an embossed shape.
    pub emboss_shape: Option<EmbossShape>,
    m_mesh: Arc<TriangleMesh>,
    m_convex_hull: Option<TriangleMesh>,
    m_type: ModelVolumeType,
    m_transformation: Transformation,
    m_convex_hull_2d: Polygon,
    m_cached_trans_matrix: Option<Transform3d>,
    m_cached_2d_polygon: Polygon,
    m_id: ObjectID,
}

impl ModelVolume {
    /// `id()` (`ObjectBase`): a new one when the part is made, kept by a copy (`Clone`, the C++'s
    /// `ModelVolume(const ModelVolume&)`).
    #[must_use]
    pub fn id(&self) -> ObjectID {
        self.m_id
    }

    /// The id set as the C++'s serialization loads it (`ObjectBase::serialize`: `ar(m_id)`); the
    /// probe replays set a recorded one so.
    pub fn load_id(&mut self, id: ObjectID) {
        self.m_id = id;
    }

    /// `assign_new_unique_ids_recursive()`: a new id (the C++ gives the config and the four
    /// paintings new ones too, which are not kept: [`crate::object_id`]).
    pub fn assign_new_unique_ids_recursive(&mut self) {
        self.m_id = ObjectID::generate_new_id();
    }

    /// `is_fdm_support_painted()` (`Model.hpp:1012`): support painting.
    #[must_use]
    pub fn is_fdm_support_painted(&self) -> bool {
        !self.supported_facets.empty()
    }

    /// `is_seam_painted()` (`Model.hpp:1013`): seam painting.
    #[must_use]
    pub fn is_seam_painted(&self) -> bool {
        !self.seam_facets.empty()
    }

    /// `is_mm_painted()` (`Model.hpp:1014`): colour painting.
    #[must_use]
    pub fn is_mm_painted(&self) -> bool {
        !self.mmu_segmentation_facets.empty()
    }

    /// `is_fuzzy_skin_painted()` (`Model.hpp:1015`): fuzzy-skin painting.
    #[must_use]
    pub fn is_fuzzy_skin_painted(&self) -> bool {
        !self.fuzzy_skin_facets.empty()
    }

    /// `ModelVolume(object, mesh, type)`: the convex hull computed when the mesh has more than one
    /// triangle, none otherwise.
    #[must_use]
    pub fn new(mesh: TriangleMesh, volume_type: ModelVolumeType) -> Self {
        let mut v = ModelVolume::with_shared_mesh(&Arc::new(mesh), volume_type);
        if v.m_mesh.facets_count() > 1 {
            v.calculate_convex_hull();
        }
        v
    }

    /// `ModelVolume(object, mesh, type)` with the mesh's `shared_ptr`: the part shares the mesh
    /// and gets no convex hull.
    #[must_use]
    pub fn with_shared_mesh(mesh: &Arc<TriangleMesh>, volume_type: ModelVolumeType) -> Self {
        ModelVolume {
            name: String::new(),
            source: VolumeSource::default(),
            config: ModelConfig::new(),
            supported_facets: FacetsAnnotation::default(),
            seam_facets: FacetsAnnotation::default(),
            mmu_segmentation_facets: FacetsAnnotation::default(),
            fuzzy_skin_facets: FacetsAnnotation::default(),
            cut_info: CutInfo::default(),
            text_configuration: None,
            emboss_shape: None,
            m_mesh: Arc::clone(mesh),
            m_convex_hull: None,
            m_type: volume_type,
            m_transformation: Transformation::default(),
            m_convex_hull_2d: Polygon::default(),
            m_cached_trans_matrix: None,
            m_cached_2d_polygon: Polygon::default(),
            m_id: ObjectID::generate_new_id(),
        }
    }

    /// `ModelVolume(object, mesh, convex_hull, type)`: with the hull given.
    #[must_use]
    pub fn with_convex_hull(
        mesh: TriangleMesh,
        convex_hull: TriangleMesh,
        volume_type: ModelVolumeType,
    ) -> Self {
        let mut v = ModelVolume::new(TriangleMesh::default(), volume_type);
        v.m_mesh = Arc::new(mesh);
        v.m_convex_hull = Some(convex_hull);
        v
    }

    /// `mesh()`.
    #[must_use]
    pub fn mesh(&self) -> &TriangleMesh {
        &self.m_mesh
    }

    /// `mesh_ptr()`: the mesh's `shared_ptr`.
    #[must_use]
    pub fn mesh_ptr(&self) -> Arc<TriangleMesh> {
        Arc::clone(&self.m_mesh)
    }

    /// `set_mesh(mesh)`: the hull is left as it was, as in the C++.
    pub fn set_mesh(&mut self, mesh: TriangleMesh) {
        self.m_mesh = Arc::new(mesh);
    }

    /// `get_convex_hull()`.
    ///
    /// # Panics
    ///
    /// When the part has no hull (fewer than two triangles), where the C++ dereferences a null
    /// pointer.
    #[must_use]
    pub fn get_convex_hull(&self) -> &TriangleMesh {
        self.m_convex_hull.as_ref().unwrap_or_else(|| {
            panic!(
                "ModelVolume \"{}\" has no convex hull (fewer than two triangles): the C++ \
                 dereferences a null pointer here",
                self.name
            )
        })
    }

    /// The hull, or `None` where the C++ holds a null pointer.
    #[must_use]
    pub fn try_get_convex_hull(&self) -> Option<&TriangleMesh> {
        self.m_convex_hull.as_ref()
    }

    /// `calculate_convex_hull()`: `mesh().convex_hull_3d()`.
    pub fn calculate_convex_hull(&mut self) {
        self.m_convex_hull = Some(self.m_mesh.convex_hull_3d());
    }

    /// `type()`.
    #[must_use]
    pub fn volume_type(&self) -> ModelVolumeType {
        self.m_type
    }

    /// `set_type(t)`.
    pub fn set_type(&mut self, t: ModelVolumeType) {
        self.m_type = t;
    }

    /// `is_model_part()`.
    #[must_use]
    pub fn is_model_part(&self) -> bool {
        self.m_type == ModelVolumeType::ModelPart
    }

    /// `is_modifier()`.
    #[must_use]
    pub fn is_modifier(&self) -> bool {
        self.m_type == ModelVolumeType::ParameterModifier
    }

    /// `get_transformation()`.
    #[must_use]
    pub fn get_transformation(&self) -> &Transformation {
        &self.m_transformation
    }

    /// `set_transformation(transformation)`.
    pub fn set_transformation(&mut self, transformation: Transformation) {
        self.m_transformation = transformation;
    }

    /// `get_matrix()`.
    #[must_use]
    pub fn get_matrix(&self) -> &Transform3d {
        self.m_transformation.get_matrix()
    }

    /// `get_offset()`.
    #[must_use]
    pub fn get_offset(&self) -> Vec3d {
        self.m_transformation.get_offset()
    }

    /// `set_offset(offset)`.
    pub fn set_offset(&mut self, offset: &Vec3d) {
        self.m_transformation.set_offset(offset);
    }

    /// `translate(displacement)`: `set_offset(get_offset() + displacement)`.
    pub fn translate(&mut self, displacement: &Vec3d) {
        let offset = self.get_offset() + *displacement;
        self.set_offset(&offset);
    }

    /// `center_geometry_after_creation(update_source_offset)`: moves the mesh (and its hull) so
    /// that its bounding box is centred on the origin, and the part's offset the other way. A
    /// mesh another part shares is copied first, where the C++ moves it for both (the module's
    /// header).
    pub fn center_geometry_after_creation(&mut self, update_source_offset: bool) {
        let shift = self.m_mesh.bounding_box().center();
        if !shift.is_approx(&Vec3d::zero(), 1e-12) {
            let (x, y, z) = (-(shift[0] as f32), -(shift[1] as f32), -(shift[2] as f32));
            let mesh = Arc::make_mut(&mut self.m_mesh);
            mesh.translate_xyz(x, y, z);
            mesh.set_init_shift(shift);
            if let Some(hull) = self.m_convex_hull.as_mut() {
                hull.translate_xyz(x, y, z);
            }
            self.translate(&shift);
        }
        if update_source_offset {
            self.source.mesh_offset = shift;
        }
    }

    /// `get_repaired_errors_count()`.
    #[must_use]
    pub fn get_repaired_errors_count(&self) -> i32 {
        let e = &self.m_mesh.stats().repaired_errors;
        e.degenerate_facets
            + e.edges_fixed
            + e.facets_removed
            + e.facets_reversed
            + e.backwards_edges
    }

    /// `calculate_convex_hull_2d(transformation)`: the hull's vertices through the matrix without
    /// the offset, truncated to scaled coordinates, their 2D hull, then translated by the scaled
    /// offset; nothing when the hull has no vertices.
    fn calculate_convex_hull_2d(&mut self, transformation: &Transformation) {
        let hull = self.get_convex_hull();
        if hull.its.vertices.is_empty() {
            return;
        }
        let new_matrix = transformation.get_matrix_no_offset();
        let mut pts: Points = Vec::with_capacity(hull.its.vertices.len());
        for v in &hull.its.vertices {
            let p = new_matrix.mul_point(&v.cast::<f64>());
            pts.push(Point::new(
                orca_crt::f64_to_i64(scale_(p.x())),
                orca_crt::f64_to_i64(scale_(p.y())),
            ));
        }
        self.m_cached_2d_polygon = convex_hull(pts);
        self.m_convex_hull_2d = self.m_cached_2d_polygon.clone();
        translate_f64(
            &mut self.m_convex_hull_2d.points,
            scale_(transformation.get_offset_axis(X)),
            scale_(transformation.get_offset_axis(Y)),
        );
    }

    /// `get_convex_hull_2d(trafo_instance)`: the 2D hull of the part placed by the instance's
    /// matrix, recomputed when the rotation, mirror or scale changed (or none is held), else the
    /// held one moved to the new offset.
    pub fn get_convex_hull_2d(&mut self, trafo_instance: &Transform3d) -> &Polygon {
        let new_matrix = trafo_instance.mul(self.m_transformation.get_matrix());
        let changed = match &self.m_cached_trans_matrix {
            None => true,
            Some(cached) => matrix_differs(&new_matrix, cached),
        };
        if changed || !self.m_convex_hull_2d.is_valid() {
            let new_trans = Transformation::new(&new_matrix);
            let recompute = match &self.m_cached_trans_matrix {
                // The C++ compares with a matrix it never set; the hull is invalid then, so it
                // recomputes either way.
                None => true,
                Some(cached) => {
                    let old_trans = Transformation::new(cached);
                    old_trans.get_scaling_factor() != new_trans.get_scaling_factor()
                        || old_trans.get_rotation() != new_trans.get_rotation()
                        || old_trans.get_mirror() != new_trans.get_mirror()
                }
            };
            if recompute || !self.m_convex_hull_2d.is_valid() {
                self.calculate_convex_hull_2d(&new_trans);
            } else {
                self.m_convex_hull_2d = self.m_cached_2d_polygon.clone();
                translate_f64(
                    &mut self.m_convex_hull_2d.points,
                    scale_(new_trans.get_offset_axis(X)),
                    scale_(new_trans.get_offset_axis(Y)),
                );
            }
            self.m_cached_trans_matrix = Some(new_matrix);
        }
        &self.m_convex_hull_2d
    }

    /// `extruder_id()`: the part's `extruder`, or its object's when the part has none or 0, or 1
    /// when neither has one.
    #[must_use]
    pub fn extruder_id(&self, object_config: &ModelConfig) -> i32 {
        let mut opt = self.config.option("extruder");
        if opt.is_none_or(|o| option_get_int(o) == 0) {
            opt = object_config.option("extruder");
        }
        opt.map_or(1, option_get_int)
    }

    /// `get_extruders()`: the painted colours (each state from 1 up that the painting uses, in
    /// order), then the part's extruder when it is above 0; none for a negative volume, a support
    /// blocker or enforcer, or an invalid part.
    ///
    /// The C++ builds a `TriangleSelector` from the painting and lists the states whose triangles
    /// are not empty: [`TriangleSplittingData::leaf_states`](crate::facets::TriangleSplittingData::leaf_states)
    /// of the part's mesh. Its cache (`mmuseg_ts`, never initialized) is left out: the reader
    /// touches the painting after loading it, so the C++ builds the list on the first call. So a
    /// painted triangle id past the mesh logs `deserialize`'s info line at each call here, and at
    /// the first only in the C++; the 3MF reader paints only the part's own triangles, so no file
    /// reaches it.
    #[must_use]
    pub fn get_extruders(&self, object_config: &ModelConfig) -> Vec<i32> {
        if matches!(
            self.m_type,
            ModelVolumeType::Invalid
                | ModelVolumeType::NegativeVolume
                | ModelVolumeType::SupportBlocker
                | ModelVolumeType::SupportEnforcer
        ) {
            return Vec::new();
        }
        let mut volume_extruders = self
            .mmu_segmentation_facets
            .get_data()
            .leaf_states(self.mesh().its.indices.len());
        let volume_extruder_id = self.extruder_id(object_config);
        if volume_extruder_id > 0 {
            volume_extruders.push(volume_extruder_id);
        }
        volume_extruders
    }
}

/// `matrix() != other.matrix()`: true when any coefficient differs (or is NaN).
fn matrix_differs(a: &Transform3d, b: &Transform3d) -> bool {
    for r in 0..4 {
        for c in 0..4 {
            if a.m.at(r, c) != b.m.at(r, c) {
                return true;
            }
        }
    }
    false
}

/// `ConfigOption::getInt()` of an option the C++ reads as an int.
pub(crate) fn option_get_int(o: &orca_config::OptionValue) -> i32 {
    use orca_config::OptionValue;
    match o {
        OptionValue::Int(v) => *v,
        OptionValue::Bool(b) => i32::from(*b),
        OptionValue::Enum { value, .. } => *value,
        other => panic!("getInt() of an option that is not an int: {other:?}"),
    }
}

/// `ModelInstance`: one placed copy of an object.
#[derive(Clone, Debug)]
pub struct ModelInstance {
    m_transformation: Transformation,
    m_assemble_transformation: Transformation,
    m_offset_to_assembly: Vec3d,
    m_assemble_initialized: bool,
    /// `print_volume_state`.
    pub print_volume_state: PrintVolumeState,
    /// `printable`.
    pub printable: bool,
    /// `auto_drop`.
    pub auto_drop: bool,
    /// `use_loaded_id_for_label`.
    pub use_loaded_id_for_label: bool,
    /// `arrange_order`.
    pub arrange_order: i32,
    /// `loaded_id`.
    pub loaded_id: usize,
    convex_hull: Polygon,
    m_id: ObjectID,
}

impl Default for ModelInstance {
    /// `ModelInstance(object)`: a new id.
    fn default() -> Self {
        ModelInstance {
            m_transformation: Transformation::default(),
            m_assemble_transformation: Transformation::default(),
            m_offset_to_assembly: Vec3d::zero(),
            m_assemble_initialized: false,
            print_volume_state: PrintVolumeState::Inside,
            printable: true,
            auto_drop: true,
            use_loaded_id_for_label: false,
            arrange_order: 0,
            loaded_id: 0,
            convex_hull: Polygon::default(),
            m_id: ObjectID::generate_new_id(),
        }
    }
}

impl ModelInstance {
    /// `id()` (`ObjectBase`): a new one when the instance is made (`ModelInstance(object)`,
    /// `ModelInstance(object, other)`), kept by a copy (`Clone`, the C++'s
    /// `ModelInstance(const ModelInstance&)`).
    #[must_use]
    pub fn id(&self) -> ObjectID {
        self.m_id
    }

    /// `assign_new_unique_ids_recursive()`: a new id.
    pub fn assign_new_unique_ids_recursive(&mut self) {
        self.m_id = ObjectID::generate_new_id();
    }

    /// `get_transformation()`.
    #[must_use]
    pub fn get_transformation(&self) -> &Transformation {
        &self.m_transformation
    }

    /// `set_transformation(t)`.
    pub fn set_transformation(&mut self, t: Transformation) {
        self.m_transformation = t;
    }

    /// `get_assemble_transformation()`.
    #[must_use]
    pub fn get_assemble_transformation(&self) -> &Transformation {
        &self.m_assemble_transformation
    }

    /// `set_assemble_transformation(t)`.
    pub fn set_assemble_transformation(&mut self, t: Transformation) {
        self.m_assemble_initialized = true;
        self.m_assemble_transformation = t;
    }

    /// `set_assemble_from_transform(transform)`.
    pub fn set_assemble_from_transform(&mut self, transform: &Transform3d) {
        self.m_assemble_initialized = true;
        self.m_assemble_transformation.set_matrix(transform);
    }

    /// `is_assemble_initialized()`.
    #[must_use]
    pub fn is_assemble_initialized(&self) -> bool {
        self.m_assemble_initialized
    }

    /// `set_offset_to_assembly(offset)`.
    pub fn set_offset_to_assembly(&mut self, offset: Vec3d) {
        self.m_offset_to_assembly = offset;
    }

    /// `get_offset_to_assembly()`.
    #[must_use]
    pub fn get_offset_to_assembly(&self) -> Vec3d {
        self.m_offset_to_assembly
    }

    /// `get_offset()`.
    #[must_use]
    pub fn get_offset(&self) -> Vec3d {
        self.m_transformation.get_offset()
    }

    /// `get_offset(axis)`.
    #[must_use]
    pub fn get_offset_axis(&self, axis: usize) -> f64 {
        self.m_transformation.get_offset_axis(axis)
    }

    /// `set_offset(offset)`.
    pub fn set_offset(&mut self, offset: &Vec3d) {
        self.m_transformation.set_offset(offset);
    }

    /// `set_offset(axis, offset)`.
    pub fn set_offset_axis(&mut self, axis: usize, offset: f64) {
        self.m_transformation.set_offset_axis(axis, offset);
    }

    /// `get_rotation()`.
    #[must_use]
    pub fn get_rotation(&self) -> Vec3d {
        self.m_transformation.get_rotation()
    }

    /// `get_rotation(axis)`.
    #[must_use]
    pub fn get_rotation_axis(&self, axis: usize) -> f64 {
        self.m_transformation.get_rotation_axis(axis)
    }

    /// `set_rotation(rotation)`.
    pub fn set_rotation(&mut self, rotation: &Vec3d) {
        self.m_transformation.set_rotation(rotation);
    }

    /// `set_rotation(axis, rotation)`.
    pub fn set_rotation_axis(&mut self, axis: usize, rotation: f64) {
        self.m_transformation.set_rotation_axis(axis, rotation);
    }

    /// `get_scaling_factor()`.
    #[must_use]
    pub fn get_scaling_factor(&self) -> Vec3d {
        self.m_transformation.get_scaling_factor()
    }

    /// `set_scaling_factor(scaling_factor)`.
    pub fn set_scaling_factor(&mut self, scaling_factor: &Vec3d) {
        self.m_transformation.set_scaling_factor(scaling_factor);
    }

    /// `get_mirror()`.
    #[must_use]
    pub fn get_mirror(&self) -> Vec3d {
        self.m_transformation.get_mirror()
    }

    /// `set_mirror(mirror)`.
    pub fn set_mirror(&mut self, mirror: &Vec3d) {
        self.m_transformation.set_mirror(mirror);
    }

    /// `is_left_handed()`.
    #[must_use]
    pub fn is_left_handed(&self) -> bool {
        self.m_transformation.is_left_handed()
    }

    /// `get_matrix()`.
    #[must_use]
    pub fn get_matrix(&self) -> &Transform3d {
        self.m_transformation.get_matrix()
    }

    /// `get_matrix_no_offset()`.
    #[must_use]
    pub fn get_matrix_no_offset(&self) -> Transform3d {
        self.m_transformation.get_matrix_no_offset()
    }

    /// `transform_bounding_box(bbox, dont_translate)`.
    #[must_use]
    pub fn transform_bounding_box(
        &self,
        bbox: &BoundingBoxf3,
        dont_translate: bool,
    ) -> BoundingBoxf3 {
        if dont_translate {
            bbox.transformed(&self.get_matrix_no_offset())
        } else {
            bbox.transformed(self.get_matrix())
        }
    }

    /// `is_printable()`, given the object's `printable`.
    #[must_use]
    pub fn is_printable(&self, object_printable: bool) -> bool {
        object_printable && self.printable && self.print_volume_state == PrintVolumeState::Inside
    }

    /// `invalidate_convex_hull_2d()`.
    pub fn invalidate_convex_hull_2d(&mut self) {
        self.convex_hull.points.clear();
    }
}

/// A brim ear: `BrimPoint`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrimPoint {
    /// `pos`.
    pub pos: orca_eigen::Vec3f,
    /// `head_front_radius`.
    pub head_front_radius: f32,
}

/// `t_layer_config_ranges`: a height range's settings, by the range, in order.
pub type LayerConfigRanges = Vec<((f64, f64), ModelConfig)>;

/// `ModelObject`. A copy (`Clone`) is the C++'s copy constructor: [`ModelObject::assign_copy`]
/// onto an object of no id, so it keeps the ids and leaves the bounding boxes invalid.
#[derive(Debug)]
pub struct ModelObject {
    /// `name`.
    pub name: String,
    /// `module_name`.
    pub module_name: String,
    /// `input_file`.
    pub input_file: String,
    /// `instances`.
    pub instances: Vec<ModelInstance>,
    /// `volumes`.
    pub volumes: Vec<ModelVolume>,
    /// `config`.
    pub config: ModelConfig,
    /// `layer_config_ranges`.
    pub layer_config_ranges: LayerConfigRanges,
    /// `layer_height_profile`.
    pub layer_height_profile: Vec<f64>,
    /// `printable`.
    pub printable: bool,
    /// `brim_points`.
    pub brim_points: Vec<BrimPoint>,
    /// `origin_translation`.
    pub origin_translation: Vec3d,
    /// `cut_id`.
    pub cut_id: CutObjectBase,
    m_bounding_box_approx: BoundingBoxf3,
    m_bounding_box_approx_valid: bool,
    m_bounding_box_exact: BoundingBoxf3,
    m_bounding_box_exact_valid: bool,
    m_min_max_z_valid: bool,
    m_raw_bounding_box: BoundingBoxf3,
    m_raw_bounding_box_valid: bool,
    m_raw_mesh_bounding_box: BoundingBoxf3,
    m_raw_mesh_bounding_box_valid: bool,
    m_id: ObjectID,
}

impl Default for ModelObject {
    /// `ModelObject(model)`: a new id.
    fn default() -> Self {
        ModelObject::constructed(ObjectID::generate_new_id())
    }
}

impl Clone for ModelObject {
    /// `ModelObject(const ModelObject&)`: `ObjectBase(-1)`, then `assign_copy`.
    fn clone(&self) -> Self {
        let mut o = ModelObject::constructed(ObjectID::default());
        o.assign_copy(self);
        o
    }
}

impl ModelObject {
    /// The members as the constructors leave them, with the id given.
    fn constructed(id: ObjectID) -> Self {
        ModelObject {
            name: String::new(),
            module_name: String::new(),
            input_file: String::new(),
            instances: Vec::new(),
            volumes: Vec::new(),
            config: ModelConfig::new(),
            layer_config_ranges: Vec::new(),
            layer_height_profile: Vec::new(),
            printable: true,
            brim_points: Vec::new(),
            origin_translation: Vec3d::zero(),
            cut_id: CutObjectBase::default(),
            m_bounding_box_approx: BoundingBoxf3::default(),
            m_bounding_box_approx_valid: false,
            m_bounding_box_exact: BoundingBoxf3::default(),
            m_bounding_box_exact_valid: false,
            m_min_max_z_valid: false,
            m_raw_bounding_box: BoundingBoxf3::default(),
            m_raw_bounding_box_valid: false,
            m_raw_mesh_bounding_box: BoundingBoxf3::default(),
            m_raw_mesh_bounding_box_valid: false,
            m_id: id,
        }
    }

    /// `id()` (`ObjectBase`).
    #[must_use]
    pub fn id(&self) -> ObjectID {
        self.m_id
    }

    /// The id set as the C++'s serialization loads it (`ObjectBase::serialize`: `ar(m_id)`); the
    /// probe replays set a recorded one so.
    pub fn load_id(&mut self, id: ObjectID) {
        self.m_id = id;
    }

    /// `assign_new_unique_ids_recursive()`: new ids for the object, its parts and its instances
    /// (the C++ renews the config's and the layer height profile's too, which are not kept:
    /// [`crate::object_id`]).
    pub fn assign_new_unique_ids_recursive(&mut self) {
        self.m_id = ObjectID::generate_new_id();
        for model_volume in &mut self.volumes {
            model_volume.assign_new_unique_ids_recursive();
        }
        for model_instance in &mut self.instances {
            model_instance.assign_new_unique_ids_recursive();
        }
    }

    /// `copy_transformation_caches(src)`: the bounding boxes and their flags.
    fn copy_transformation_caches(&mut self, src: &ModelObject) {
        self.m_bounding_box_approx = src.m_bounding_box_approx;
        self.m_bounding_box_approx_valid = src.m_bounding_box_approx_valid;
        self.m_bounding_box_exact = src.m_bounding_box_exact;
        self.m_bounding_box_exact_valid = src.m_bounding_box_exact_valid;
        self.m_min_max_z_valid = src.m_min_max_z_valid;
        self.m_raw_bounding_box = src.m_raw_bounding_box;
        self.m_raw_bounding_box_valid = src.m_raw_bounding_box_valid;
        self.m_raw_mesh_bounding_box = src.m_raw_mesh_bounding_box;
        self.m_raw_mesh_bounding_box_valid = src.m_raw_mesh_bounding_box_valid;
    }

    /// `assign_copy(const ModelObject&)`: the id, the members (the config with its settings), the
    /// cut id, the bounding boxes, then the parts and the instances copied with their ids; clearing
    /// the old parts and instances invalidates the boxes just copied, as in the C++. The SLA
    /// members (`sla_support_points`, `sla_points_status`, `sla_drain_holes`) are not held.
    pub fn assign_copy(&mut self, rhs: &ModelObject) -> &mut Self {
        self.m_id = rhs.m_id;

        self.name.clone_from(&rhs.name);
        //BBS: add module name
        self.module_name.clone_from(&rhs.module_name);
        self.input_file.clone_from(&rhs.input_file);
        // Copies the config's ID
        self.config = rhs.config.clone();
        self.brim_points.clone_from(&rhs.brim_points);
        self.layer_config_ranges
            .clone_from(&rhs.layer_config_ranges);
        self.layer_height_profile
            .clone_from(&rhs.layer_height_profile);
        self.printable = rhs.printable;
        self.origin_translation = rhs.origin_translation;
        self.cut_id = rhs.cut_id;
        self.copy_transformation_caches(rhs);

        self.clear_volumes();
        self.volumes.reserve(rhs.volumes.len());
        for model_volume in &rhs.volumes {
            self.volumes.push(model_volume.clone());
        }
        self.clear_instances();
        self.instances.reserve(rhs.instances.len());
        for model_instance in &rhs.instances {
            self.instances.push(model_instance.clone());
        }

        self
    }

    /// `assign_copy(ModelObject&&)`: as [`ModelObject::assign_copy`], the members, parts and
    /// instances moved out of `rhs` (left empty, as MSVC leaves a moved string, map or vector);
    /// the cut id is not taken, as the C++ leaves it.
    pub fn assign_move(&mut self, rhs: &mut ModelObject) -> &mut Self {
        self.m_id = rhs.m_id;

        self.name = std::mem::take(&mut rhs.name);
        //BBS: add module name
        self.module_name = std::mem::take(&mut rhs.module_name);
        self.input_file = std::mem::take(&mut rhs.input_file);
        // Moves the config's ID
        self.config = std::mem::replace(&mut rhs.config, ModelConfig::new());
        self.brim_points = std::mem::take(&mut rhs.brim_points);
        self.layer_config_ranges = std::mem::take(&mut rhs.layer_config_ranges);
        self.layer_height_profile = std::mem::take(&mut rhs.layer_height_profile);
        self.printable = rhs.printable;
        self.origin_translation = rhs.origin_translation;
        self.copy_transformation_caches(rhs);

        self.clear_volumes();
        self.volumes = std::mem::take(&mut rhs.volumes);
        self.clear_instances();
        self.instances = std::mem::take(&mut rhs.instances);

        self
    }

    /// `clear_volumes()`: the parts removed, the bounding boxes invalidated.
    pub fn clear_volumes(&mut self) {
        self.volumes.clear();
        self.invalidate_bounding_box();
    }

    /// `is_fdm_support_painted()` (`Model.cpp:1346-1349`): some part has support painting.
    #[must_use]
    pub fn is_fdm_support_painted(&self) -> bool {
        self.volumes.iter().any(ModelVolume::is_fdm_support_painted)
    }

    /// `is_seam_painted()` (`Model.cpp:1351-1354`): some part has seam painting.
    #[must_use]
    pub fn is_seam_painted(&self) -> bool {
        self.volumes.iter().any(ModelVolume::is_seam_painted)
    }

    /// `is_mm_painted()`: some part has colour painting.
    #[must_use]
    pub fn is_mm_painted(&self) -> bool {
        self.volumes.iter().any(ModelVolume::is_mm_painted)
    }

    /// `is_fuzzy_skin_painted()`: some part has fuzzy-skin painting.
    #[must_use]
    pub fn is_fuzzy_skin_painted(&self) -> bool {
        self.volumes.iter().any(ModelVolume::is_fuzzy_skin_painted)
    }

    /// `raw_mesh()`: the model parts' meshes, each through its part's matrix, merged in order (no
    /// instance transformation).
    #[must_use]
    pub fn raw_mesh(&self) -> TriangleMesh {
        let mut mesh = TriangleMesh::default();
        for v in &self.volumes {
            if v.is_model_part() {
                let mut vol_mesh = v.mesh().clone();
                vol_mesh.transform(v.get_matrix(), false);
                mesh.merge(&vol_mesh);
            }
        }
        mesh
    }
}

/// The error of `raw_bounding_box()` on an object with no instance (`InvalidArgument`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidArgument(pub String);

impl std::fmt::Display for InvalidArgument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for InvalidArgument {}

impl ModelObject {
    /// `invalidate_bounding_box()`.
    pub fn invalidate_bounding_box(&mut self) {
        self.m_bounding_box_approx_valid = false;
        self.m_bounding_box_exact_valid = false;
        self.m_min_max_z_valid = false;
        self.m_raw_bounding_box_valid = false;
        self.m_raw_mesh_bounding_box_valid = false;
    }

    /// `add_volume(mesh, type, modify_to_center_geometry)`: the new part's index.
    pub fn add_volume(
        &mut self,
        mesh: TriangleMesh,
        volume_type: ModelVolumeType,
        modify_to_center_geometry: bool,
    ) -> usize {
        self.volumes.push(ModelVolume::new(mesh, volume_type));
        let idx = self.volumes.len() - 1;
        if modify_to_center_geometry {
            self.volumes[idx].center_geometry_after_creation(true);
            self.invalidate_bounding_box();
        }
        idx
    }

    /// `add_volume_with_shared_mesh(other, type)`, given `other.m_mesh` (its
    /// [`ModelVolume::mesh_ptr`]): a part that shares the mesh, with no convex hull, not centred
    /// (it was when it was made); its type set when `volume_type` is not `INVALID` and differs.
    /// The new part's index.
    pub fn add_volume_with_shared_mesh(
        &mut self,
        other_mesh: &Arc<TriangleMesh>,
        volume_type: ModelVolumeType,
    ) -> usize {
        let mut v = ModelVolume::with_shared_mesh(other_mesh, ModelVolumeType::ModelPart);
        if volume_type != ModelVolumeType::Invalid && v.volume_type() != volume_type {
            v.set_type(volume_type);
        }
        self.volumes.push(v);
        // The volume should already be centered at this point of time when copying shared
        // pointers of the triangle mesh and convex hull.
        self.volumes.len() - 1
    }

    /// `add_instance()`: the new instance's index.
    pub fn add_instance(&mut self) -> usize {
        self.instances.push(ModelInstance::default());
        self.invalidate_bounding_box();
        self.instances.len() - 1
    }

    /// `add_instance(other)`: a copy of `other` made by `ModelInstance(object, other)`, which
    /// copies the transformations, the offset to the assembly, `printable` and `auto_drop`; the
    /// print volume state is `Inside`, the assembly not initialised, the rest as a new instance's.
    pub fn add_instance_copy(&mut self, other: &ModelInstance) -> usize {
        self.instances.push(ModelInstance {
            m_transformation: other.m_transformation,
            m_assemble_transformation: other.m_assemble_transformation,
            m_offset_to_assembly: other.m_offset_to_assembly,
            printable: other.printable,
            auto_drop: other.auto_drop,
            ..ModelInstance::default()
        });
        self.invalidate_bounding_box();
        self.instances.len() - 1
    }

    /// `add_instance(offset, scaling_factor, rotation, mirror)`.
    pub fn add_instance_with(
        &mut self,
        offset: &Vec3d,
        scaling_factor: &Vec3d,
        rotation: &Vec3d,
        mirror: &Vec3d,
    ) -> usize {
        let idx = self.add_instance();
        let i = &mut self.instances[idx];
        i.set_offset(offset);
        i.set_scaling_factor(scaling_factor);
        i.set_rotation(rotation);
        i.set_mirror(mirror);
        idx
    }

    /// `delete_instance(idx)`.
    pub fn delete_instance(&mut self, idx: usize) {
        self.instances.remove(idx);
        self.invalidate_bounding_box();
    }

    /// `delete_last_instance()`.
    ///
    /// # Panics
    ///
    /// When the object has no instance, where the C++ erases at `begin() - 1`.
    pub fn delete_last_instance(&mut self) {
        self.delete_instance(self.instances.len() - 1);
    }

    /// `clear_instances()`.
    pub fn clear_instances(&mut self) {
        self.instances.clear();
        self.invalidate_bounding_box();
    }

    /// `raw_mesh_bounding_box()`: the parts' meshes through their own matrices (cached).
    pub fn raw_mesh_bounding_box(&mut self) -> BoundingBoxf3 {
        if !self.m_raw_mesh_bounding_box_valid {
            self.m_raw_mesh_bounding_box_valid = true;
            let mut bb = BoundingBoxf3::default();
            for v in &self.volumes {
                if v.is_model_part() {
                    bb.merge_box(&v.mesh().transformed_bounding_box(v.get_matrix()));
                }
            }
            self.m_raw_mesh_bounding_box = bb;
        }
        self.m_raw_mesh_bounding_box
    }

    /// `full_raw_mesh_bounding_box()`: every part's, modifiers included.
    #[must_use]
    pub fn full_raw_mesh_bounding_box(&self) -> BoundingBoxf3 {
        let mut bb = BoundingBoxf3::default();
        for v in &self.volumes {
            bb.merge_box(&v.mesh().transformed_bounding_box(v.get_matrix()));
        }
        bb
    }

    /// `raw_bounding_box()`: the parts through the first instance's matrix without its offset
    /// (cached).
    ///
    /// # Errors
    ///
    /// With no instance: "Can't call raw_bounding_box() with no instances". The cache counts as
    /// filled then, as in the C++, so a second call returns the empty box.
    pub fn raw_bounding_box(&mut self) -> Result<BoundingBoxf3, InvalidArgument> {
        if !self.m_raw_bounding_box_valid {
            self.m_raw_bounding_box_valid = true;
            self.m_raw_bounding_box = BoundingBoxf3::default();
            let Some(first) = self.instances.first() else {
                return Err(InvalidArgument(
                    "Can't call raw_bounding_box() with no instances".to_owned(),
                ));
            };
            let inst_matrix = first.get_transformation().get_matrix_no_offset();
            let mut bb = BoundingBoxf3::default();
            for v in &self.volumes {
                if v.is_model_part() {
                    bb.merge_box(
                        &v.mesh()
                            .transformed_bounding_box(&inst_matrix.mul(v.get_matrix())),
                    );
                }
            }
            self.m_raw_bounding_box = bb;
        }
        Ok(self.m_raw_bounding_box)
    }

    /// `instance_bounding_box(instance_idx, dont_translate)`: the parts' meshes through the
    /// instance's matrix and their own.
    #[must_use]
    pub fn instance_bounding_box(
        &self,
        instance_idx: usize,
        dont_translate: bool,
    ) -> BoundingBoxf3 {
        let instance = &self.instances[instance_idx];
        let inst_matrix = if dont_translate {
            instance.get_transformation().get_matrix_no_offset()
        } else {
            *instance.get_transformation().get_matrix()
        };
        let mut bb = BoundingBoxf3::default();
        for v in &self.volumes {
            if v.is_model_part() {
                bb.merge_box(
                    &v.mesh()
                        .transformed_bounding_box(&inst_matrix.mul(v.get_matrix())),
                );
            }
        }
        bb
    }

    /// `instance_convex_hull_bounding_box(instance_idx, dont_translate)`: as
    /// [`ModelObject::instance_bounding_box`], of the parts' hulls.
    #[must_use]
    pub fn instance_convex_hull_bounding_box(
        &self,
        instance_idx: usize,
        dont_translate: bool,
    ) -> BoundingBoxf3 {
        let instance = &self.instances[instance_idx];
        let inst_matrix = if dont_translate {
            instance.get_transformation().get_matrix_no_offset()
        } else {
            *instance.get_transformation().get_matrix()
        };
        let mut bb = BoundingBoxf3::default();
        for v in &self.volumes {
            if v.is_model_part() {
                bb.merge_box(
                    &v.get_convex_hull()
                        .transformed_bounding_box(&inst_matrix.mul(v.get_matrix())),
                );
            }
        }
        bb
    }

    /// `bounding_box_approx()`: the raw mesh box through each instance (cached).
    pub fn bounding_box_approx(&mut self) -> BoundingBoxf3 {
        if !self.m_bounding_box_approx_valid {
            self.m_bounding_box_approx_valid = true;
            let raw_bbox = self.raw_mesh_bounding_box();
            let mut bb = BoundingBoxf3::default();
            for i in &self.instances {
                bb.merge_box(&i.transform_bounding_box(&raw_bbox, false));
            }
            self.m_bounding_box_approx = bb;
        }
        self.m_bounding_box_approx
    }

    /// `bounding_box_exact()`: each instance's box (cached; fills the minimum and maximum z
    /// too).
    pub fn bounding_box_exact(&mut self) -> BoundingBoxf3 {
        if !self.m_bounding_box_exact_valid {
            self.m_bounding_box_exact_valid = true;
            self.m_min_max_z_valid = true;
            let mut bb = BoundingBoxf3::default();
            for i in 0..self.instances.len() {
                bb.merge_box(&self.instance_bounding_box(i, false));
            }
            self.m_bounding_box_exact = bb;
        }
        self.m_bounding_box_exact
    }

    /// `min_z()`.
    pub fn min_z(&mut self) -> f64 {
        self.update_min_max_z();
        self.m_bounding_box_exact.min.z()
    }

    /// `max_z()`.
    pub fn max_z(&mut self) -> f64 {
        self.update_min_max_z();
        self.m_bounding_box_exact.max.z()
    }

    /// `update_min_max_z()`: the parts' lowest and highest z through the first instance, into the
    /// exact box's z alone.
    pub fn update_min_max_z(&mut self) {
        if !self.m_min_max_z_valid && !self.instances.is_empty() {
            self.m_min_max_z_valid = true;
            let mat_instance = *self.instances[0].get_transformation().get_matrix();
            let mut global_min_z = f64::MAX;
            let mut global_max_z = -f64::MAX;
            for v in &self.volumes {
                if v.is_model_part() {
                    let m = mat_instance.mul(v.get_matrix());
                    let row_z = m.linear().row(2);
                    let shift_z = m.translation().z();
                    let mut this_min_z = f64::MAX;
                    let mut this_max_z = -f64::MAX;
                    for p in &v.mesh().its.vertices {
                        let z = row_z.dot(&p.cast::<f64>());
                        this_min_z = orca_stl::min(this_min_z, z);
                        this_max_z = orca_stl::max(this_max_z, z);
                    }
                    this_min_z += shift_z;
                    this_max_z += shift_z;
                    global_min_z = orca_stl::min(global_min_z, this_min_z);
                    global_max_z = orca_stl::max(global_max_z, this_max_z);
                }
            }
            self.m_bounding_box_exact.min[2] = global_min_z;
            self.m_bounding_box_exact.max[2] = global_max_z;
        }
    }

    /// `facets_count()`: the parts' triangles.
    #[must_use]
    pub fn facets_count(&self) -> usize {
        self.volumes
            .iter()
            .filter(|v| v.is_model_part())
            .map(|v| v.mesh().facets_count())
            .sum()
    }

    /// `parts_count()`.
    #[must_use]
    pub fn parts_count(&self) -> usize {
        self.volumes.iter().filter(|v| v.is_model_part()).count()
    }

    /// `convex_hull_2d(trafo_instance)`: the 2D hull of the parts' 2D hulls.
    pub fn convex_hull_2d(&mut self, trafo_instance: &Transform3d) -> Polygon {
        let mut pts: Points = Vec::new();
        for v in &mut self.volumes {
            if v.is_model_part() {
                let volume_hull = v.get_convex_hull_2d(trafo_instance);
                pts.extend_from_slice(&volume_hull.points);
            }
        }
        convex_hull(pts)
    }

    /// `ModelInstance::convex_hull_2d()` of the instance: through its full matrix (kept on the
    /// instance as the C++ keeps it).
    pub fn instance_convex_hull_2d(&mut self, instance_idx: usize) -> Polygon {
        let trafo = *self.instances[instance_idx].get_matrix();
        let hull = self.convex_hull_2d(&trafo);
        self.instances[instance_idx].convex_hull = hull.clone();
        hull
    }

    /// `center_around_origin(include_modifiers)`.
    pub fn center_around_origin(&mut self, include_modifiers: bool) {
        let bb = if include_modifiers {
            self.full_raw_mesh_bounding_box()
        } else {
            self.raw_mesh_bounding_box()
        };
        let shift = -bb.center();
        self.translate(shift.x(), shift.y(), shift.z());
        self.origin_translation += shift;
    }

    /// `ensure_on_bed(allow_negative_z)`: lifts or lowers each instance that drops automatically
    /// so the object stands on the bed.
    pub fn ensure_on_bed(&mut self, allow_negative_z: bool) {
        let mut z_offset = 0.0;
        if allow_negative_z {
            if self.parts_count() == 1 {
                let min_z = self.min_z();
                let max_z = self.max_z();
                if min_z >= f64::from(SINKING_Z_THRESHOLD) || max_z < 0.0 {
                    z_offset = -min_z;
                }
            } else {
                let max_z = self.max_z();
                if max_z < SINKING_MIN_Z_THRESHOLD {
                    z_offset = SINKING_MIN_Z_THRESHOLD - max_z;
                }
            }
        } else {
            z_offset = -self.min_z();
        }
        if z_offset != 0.0 {
            let v = Vec3d::unit_z() * z_offset;
            for i in 0..self.instances.len() {
                if !self.instances[i].auto_drop {
                    continue;
                }
                self.translate_instance(i, &v);
            }
        }
    }

    /// `translate_instances(vector)`.
    pub fn translate_instances(&mut self, vector: &Vec3d) {
        for i in 0..self.instances.len() {
            self.translate_instance(i, vector);
        }
    }

    /// `translate_instance(instance_idx, vector)`.
    pub fn translate_instance(&mut self, instance_idx: usize, vector: &Vec3d) {
        let i = &mut self.instances[instance_idx];
        let offset = i.get_offset() + *vector;
        i.set_offset(&offset);
        self.invalidate_bounding_box();
    }

    /// `translate(x, y, z)`: every part, and the held boxes that are valid.
    pub fn translate(&mut self, x: f64, y: f64, z: f64) {
        let d = Vec3d::new(x, y, z);
        for v in &mut self.volumes {
            v.translate(&d);
        }
        if self.m_bounding_box_approx_valid {
            self.m_bounding_box_approx.translate(d);
        }
        if self.m_bounding_box_exact_valid {
            self.m_bounding_box_exact.translate(d);
        }
    }

    /// `get_instance_min_z(instance_idx)`: the lowest z of the parts' hulls (of the meshes when a
    /// hull has no triangle) through the instance without its offset, 0 when nothing is found,
    /// plus the offset's z.
    #[must_use]
    pub fn get_instance_min_z(&self, instance_idx: usize) -> f64 {
        let mut min_z = f64::MAX;
        let inst = &self.instances[instance_idx];
        let mi = inst.get_matrix_no_offset();
        for v in &self.volumes {
            if !v.is_model_part() {
                continue;
            }
            let mv = mi.mul(v.get_matrix());
            let hull = v.get_convex_hull();
            let its = if hull.its.indices.is_empty() {
                &v.mesh().its
            } else {
                &hull.its
            };
            for facet in &its.indices {
                for i in 0..3 {
                    let p = its.vertices[facet[i] as usize].cast::<f64>();
                    min_z = orca_stl::min(min_z, mv.mul_point(&p).z());
                }
            }
        }
        if min_z == f64::MAX {
            min_z = 0.0;
        }
        min_z + inst.get_offset_axis(Z)
    }

    /// `get_instance_max_z(instance_idx)`: the highest z of the parts' hulls through the instance
    /// without its offset, plus the offset's z.
    #[must_use]
    pub fn get_instance_max_z(&self, instance_idx: usize) -> f64 {
        let mut max_z = -f64::MAX;
        let inst = &self.instances[instance_idx];
        let mi = inst.get_matrix_no_offset();
        for v in &self.volumes {
            if !v.is_model_part() {
                continue;
            }
            let mv = mi.mul(v.get_matrix());
            let hull = v.get_convex_hull();
            for facet in &hull.its.indices {
                for i in 0..3 {
                    let p = hull.its.vertices[facet[i] as usize].cast::<f64>();
                    max_z = orca_stl::max(max_z, mv.mul_point(&p).z());
                }
            }
        }
        max_z + inst.get_offset_axis(Z)
    }

    /// `ModelInstance::apply_arrange_result(offs, rotation)` of the instance: the rotation about
    /// z, the offset's x and y unscaled, and the object's boxes invalidated.
    pub fn apply_arrange_result(&mut self, instance_idx: usize, offs: (f64, f64), rotation: f64) {
        let i = &mut self.instances[instance_idx];
        i.set_rotation_axis(Z, rotation);
        i.set_offset_axis(X, orca_geom::point::unscale_(offs.0));
        i.set_offset_axis(Y, orca_geom::point::unscale_(offs.1));
        self.invalidate_bounding_box();
    }

    /// `get_config_value<T>(global_config, key)`: the object's own option, or the global one.
    #[must_use]
    pub fn get_config_value<'a>(
        &'a self,
        global_config: &'a dyn crate::arrange::OptionSource,
        key: &str,
    ) -> Option<&'a orca_config::OptionValue> {
        if self.config.has(key) {
            self.config.option(key)
        } else {
            global_config.option(key)
        }
    }
}

/// `ModelProfileInfo`: the profile's data a 3MF file's metadata give.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelProfileInfo {
    /// `ProfileTile`.
    pub profile_tile: String,
    /// `ProfileCover`.
    pub profile_cover: String,
    /// `ProfileDescription`.
    pub profile_description: String,
    /// `ProfileUserId`.
    pub profile_user_id: String,
    /// `ProfileUserName`.
    pub profile_user_name: String,
}

/// `ModelDesignInfo`: the design's data a 3MF file's metadata give.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelDesignInfo {
    /// `DesignId`.
    pub design_id: String,
    /// `Designer`.
    pub designer: String,
    /// `DesignerUserId`.
    pub designer_user_id: String,
}

/// `ModelInfo`: the model's data a 3MF file's metadata give, and every named metadata item.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelInfo {
    /// `cover_file`.
    pub cover_file: String,
    /// `license`.
    pub license: String,
    /// `description`.
    pub description: String,
    /// `copyright`.
    pub copyright: String,
    /// `model_name`.
    pub model_name: String,
    /// `origin`.
    pub origin: String,
    /// `metadata_items`: by name (`std::map<std::string, std::string>`, byte order).
    pub metadata_items: BTreeMap<String, String>,
}

impl ModelInfo {
    /// `load(info)`: a copy of `info`'s fields.
    pub fn load(&mut self, info: &ModelInfo) {
        self.cover_file = info.cover_file.clone();
        self.license = info.license.clone();
        self.description = info.description.clone();
        self.copyright = info.copyright.clone();
        self.model_name = info.model_name.clone();
        self.origin = info.origin.clone();
        self.metadata_items = info.metadata_items.clone();
    }
}

/// The ids `Model::get_backup_path` names its folders by: one for each model that asks, in the
/// order they ask (the C++ uses the model's `ObjectID`, which counts every `ObjectBase` made in
/// the process; neither is recorded, `parity/HAZARDS.md`).
static NEXT_MODEL_ID: AtomicU64 = AtomicU64::new(1);

/// `ModelMaterial`: a material's attributes and settings, which only the AMF reader (not
/// translated) adds; the command line's models have none.
#[derive(Clone, Debug)]
pub struct ModelMaterial {
    /// `attributes`.
    pub attributes: BTreeMap<String, String>,
    /// `config`.
    pub config: ModelConfig,
    m_id: ObjectID,
}

impl Default for ModelMaterial {
    /// `ModelMaterial(model)`: a new id.
    fn default() -> Self {
        ModelMaterial {
            attributes: BTreeMap::new(),
            config: ModelConfig::new(),
            m_id: ObjectID::generate_new_id(),
        }
    }
}

impl ModelMaterial {
    /// `id()` (`ObjectBase`), kept by a copy (`ModelMaterial(const ModelMaterial&)`).
    #[must_use]
    pub fn id(&self) -> ObjectID {
        self.m_id
    }
}

/// `Model`. A copy (`Clone`) is the C++'s copy constructor: [`Model::assign_copy`] onto a model
/// of no id, which keeps the ids and does not copy the backup folder.
///
/// Members the command line's model never fills are not held: `plates_custom_gcodes` (the custom
/// G-code of each plate, which phase O4's G-code export reads), `calib_pa_pattern`,
/// `stl_design_id`, `stl_design_country`, `mk_name`, `mk_version`, `md_name`, `md_value`,
/// `object_backup_id_map` and `next_object_backup_id`; `assign_copy` and `assign_move` copy the
/// members that are.
#[derive(Debug)]
pub struct Model {
    /// `objects`.
    pub objects: Vec<ModelObject>,
    /// `materials`, by id (a `std::map`).
    pub materials: BTreeMap<String, ModelMaterial>,
    /// `curr_plate_index`.
    pub curr_plate_index: i32,
    /// `design_info` (a `shared_ptr`, null until a 3MF file with a Designer sets it).
    pub design_info: Option<ModelDesignInfo>,
    /// `model_info` (null until a 3MF file sets it).
    pub model_info: Option<ModelInfo>,
    /// `profile_info` (null until a 3MF file sets it).
    pub profile_info: Option<ModelProfileInfo>,
    /// `backup_path`: the model's backup folder, empty until asked for.
    pub backup_path: String,
    m_id: ObjectID,
}

impl Default for Model {
    /// `Model()`: a new id.
    fn default() -> Self {
        Model::constructed(ObjectID::generate_new_id())
    }
}

impl Clone for Model {
    /// `Model(const Model&)`: `ObjectBase(-1)`, then `assign_copy`.
    fn clone(&self) -> Self {
        let mut m = Model::constructed(ObjectID::default());
        m.assign_copy(self);
        m
    }
}

impl Model {
    /// The members as the constructors leave them, with the id given.
    fn constructed(id: ObjectID) -> Self {
        Model {
            objects: Vec::new(),
            materials: BTreeMap::new(),
            curr_plate_index: 0,
            design_info: None,
            model_info: None,
            profile_info: None,
            backup_path: String::new(),
            m_id: id,
        }
    }

    /// `id()` (`ObjectBase`).
    #[must_use]
    pub fn id(&self) -> ObjectID {
        self.m_id
    }

    /// `assign_copy(const Model&)`: the id, the materials and the objects copied with their ids,
    /// the plate index and the model's information (`Model.cpp:59-111`). The backup folder is not
    /// copied.
    pub fn assign_copy(&mut self, rhs: &Model) -> &mut Self {
        self.m_id = rhs.m_id;
        // copy materials
        self.clear_materials();
        // Copy including the ID and m_model.
        self.materials.clone_from(&rhs.materials);
        // copy objects
        self.clear_objects();
        self.objects.reserve(rhs.objects.len());
        for model_object in &rhs.objects {
            // Copy including the ID, leave ID set to invalid (zero).
            self.objects.push(model_object.clone());
        }

        // copy custom code per height
        // BBS
        self.curr_plate_index = rhs.curr_plate_index;

        // BBS: for design info
        self.design_info.clone_from(&rhs.design_info);
        self.model_info.clone_from(&rhs.model_info);
        self.profile_info.clone_from(&rhs.profile_info);

        self
    }

    /// `assign_copy(Model&&)`: the id, the materials and the objects moved out of `rhs` (left
    /// with none), the plate index, the backup folder, and the model's information moved (`rhs`
    /// left with none) (`Model.cpp:113-152`).
    pub fn assign_move(&mut self, rhs: &mut Model) -> &mut Self {
        self.m_id = rhs.m_id;
        // Move materials, adjust the parent pointer.
        self.clear_materials();
        self.materials = std::mem::take(&mut rhs.materials);
        // Move objects, adjust the parent pointer.
        self.clear_objects();
        self.objects = std::mem::take(&mut rhs.objects);

        // copy custom code per height
        // BBS
        self.curr_plate_index = rhs.curr_plate_index;

        //BBS: add auxiliary path logic
        // BBS: backup, all in one temp dir
        self.backup_path = std::mem::take(&mut rhs.backup_path);
        self.design_info = rhs.design_info.take();
        self.model_info = rhs.model_info.take();
        self.profile_info = rhs.profile_info.take();
        self
    }

    /// `clear_objects()`: every object removed (`delete_object_mesh` does nothing,
    /// `bbs_3mf.cpp:9026-9030`; the backup ids are not held).
    pub fn clear_objects(&mut self) {
        self.objects.clear();
    }

    /// `clear_materials()`: every material removed.
    pub fn clear_materials(&mut self) {
        self.materials.clear();
    }

    /// `is_fdm_support_painted()` (`Model.cpp:1076-1079`): some object has support painting.
    #[must_use]
    pub fn is_fdm_support_painted(&self) -> bool {
        self.objects.iter().any(ModelObject::is_fdm_support_painted)
    }

    /// `is_seam_painted()` (`Model.cpp:1081-1084`): some object has seam painting.
    #[must_use]
    pub fn is_seam_painted(&self) -> bool {
        self.objects.iter().any(ModelObject::is_seam_painted)
    }

    /// `is_mm_painted()`: some object has colour painting.
    #[must_use]
    pub fn is_mm_painted(&self) -> bool {
        self.objects.iter().any(ModelObject::is_mm_painted)
    }

    /// `is_fuzzy_skin_painted()`: some object has fuzzy-skin painting.
    #[must_use]
    pub fn is_fuzzy_skin_painted(&self) -> bool {
        self.objects.iter().any(ModelObject::is_fuzzy_skin_painted)
    }

    /// `add_object()`: an empty object's index.
    pub fn add_empty_object(&mut self) -> usize {
        self.objects.push(ModelObject::default());
        self.objects.len() - 1
    }

    /// `add_object(other)`: a copy of `other` (`ModelObject::new_clone`, `assign_copy`: its parts
    /// sharing their meshes, its instances copied, then new ids for it, its parts and its
    /// instances), its extruder set to 1 when it has none or 0. The backup a model that needs one
    /// makes (`need_backup`) is not translated: a loaded model needs none. The new object's
    /// index.
    pub fn add_object_copy(&mut self, other: &ModelObject) -> usize {
        let mut new_object = other.clone();
        new_object.assign_new_unique_ids_recursive();
        // BBS: set default extruder id to 1
        if !new_object.config.has("extruder") || new_object.config.extruder() == 0 {
            new_object
                .config
                .set_key_value("extruder", orca_config::OptionValue::Int(1));
        }
        self.objects.push(new_object);
        self.objects.len() - 1
    }

    /// `delete_object(idx)` and `delete_object(ModelObject*)`: the object at `idx` removed (its
    /// `delete_object_mesh` does nothing, `bbs_3mf.cpp:9026-9030`); whether there was one.
    pub fn delete_object(&mut self, idx: usize) -> bool {
        if idx < self.objects.len() {
            self.objects.remove(idx);
            return true;
        }
        false
    }

    /// `set_backup_path(path)`.
    pub fn set_backup_path(&mut self, path: &str) {
        self.backup_path = path.to_owned();
    }

    /// `get_backup_path()`: the model's backup folder,
    /// `<temporary_dir>/orcaslicer_model/<%a_%b_%d>/<%H_%M_%S>#<pid>#<id>` in local time, made
    /// with `3D/Objects`, `Metadata` and `lock.txt` (the process id) when it does not exist. A
    /// folder of the same name left from before is removed first. A failure to make it is logged,
    /// not returned.
    ///
    /// The C++ `remove_all` of an old folder throws on a failure; here that failure is ignored
    /// (the name holds the process id and a fresh id, so no record reaches it). The error log's
    /// text after the path is the Rust error's, not Boost's `filesystem_error` (not recorded).
    pub fn get_backup_path(&mut self) -> String {
        if self.backup_path.is_empty() {
            let pid = crate::utils::get_current_pid();
            let parent_path = crate::utils::temporary_dir();
            let now_time = crate::utils::local_time_now();
            let id = NEXT_MODEL_ID.fetch_add(1, Ordering::Relaxed);
            let mut buf = String::from("/orcaslicer_model/");
            buf.push_str(&crate::utils::put_time(&now_time, "%a_%b_%d/%H_%M_%S#"));
            buf.push_str(&format!("{pid}#{id}"));
            self.backup_path = parent_path + &buf;
            let this = std::ptr::from_ref(self) as usize;
            log(
                Severity::Info,
                &format!(
                    "model {this:016X}, id {id}, backup_path empty, set to {}",
                    self.backup_path
                ),
            );
            let temp_path = Path::new(&self.backup_path);
            if temp_path.exists() {
                log(
                    Severity::Info,
                    &format!(
                        "model {this:016X}, id {id}, remove previous {}",
                        self.backup_path
                    ),
                );
                let _ = if temp_path.is_dir() {
                    std::fs::remove_dir_all(temp_path)
                } else {
                    std::fs::remove_file(temp_path)
                };
            }
        }
        let temp_path = crate::utils::quoted(&self.backup_path);
        if !Path::new(&self.backup_path).exists() {
            let made = (|| -> Result<(), String> {
                log(
                    Severity::Info,
                    &format!("create /3D/Objects in {temp_path}"),
                );
                std::fs::create_dir_all(format!("{}/3D/Objects", self.backup_path))
                    .map_err(|e| e.to_string())?;
                log(Severity::Info, &format!("create /Metadata in {temp_path}"));
                std::fs::create_dir_all(format!("{}/Metadata", self.backup_path))
                    .map_err(|e| e.to_string())?;
                log(Severity::Info, &format!("create /lock.txt in {temp_path}"));
                crate::utils::save_string_file(
                    &format!("{}/lock.txt", self.backup_path),
                    &crate::utils::get_current_pid().to_string(),
                )
            })();
            if let Err(what) = made {
                log(
                    Severity::Error,
                    &format!("Failed to create backup path{temp_path}: {what}"),
                );
            }
        }
        self.backup_path.clone()
    }

    /// `add_object(name, path, mesh)`: an object of one part, the mesh centred (the part offset
    /// back), its extruder set to 1 when it has none or 0.
    pub fn add_object(&mut self, name: &str, path: &str, mesh: TriangleMesh) -> usize {
        self.objects.push(ModelObject::default());
        let object_idx = self.objects.len() - 1;
        let o = &mut self.objects[object_idx];
        o.name = name.to_owned();
        o.input_file = path.to_owned();
        let v = o.add_volume(mesh, ModelVolumeType::ModelPart, true);
        let volume_count = o.volumes.len();
        let volume = &mut o.volumes[v];
        volume.name = name.to_owned();
        volume.source.input_file = path.to_owned();
        volume.source.object_idx = object_idx as i32;
        volume.source.volume_idx = volume_count as i32 - 1;
        if !o.config.has("extruder") || o.config.extruder() == 0 {
            o.config
                .set_key_value("extruder", orca_config::OptionValue::Int(1));
        }
        o.invalidate_bounding_box();
        object_idx
    }

    /// `add_default_instances()`: an instance for each object that has none.
    pub fn add_default_instances(&mut self) -> bool {
        for o in &mut self.objects {
            if o.instances.is_empty() {
                o.add_instance();
            }
        }
        true
    }

    /// `bounding_box_approx()`.
    pub fn bounding_box_approx(&mut self) -> BoundingBoxf3 {
        let mut bb = BoundingBoxf3::default();
        for o in &mut self.objects {
            bb.merge_box(&o.bounding_box_approx());
        }
        bb
    }

    /// `bounding_box_exact()`.
    pub fn bounding_box_exact(&mut self) -> BoundingBoxf3 {
        let mut bb = BoundingBoxf3::default();
        for o in &mut self.objects {
            bb.merge_box(&o.bounding_box_exact());
        }
        bb
    }

    /// `max_z()`.
    pub fn max_z(&mut self) -> f64 {
        let mut z = 0.0;
        for o in &mut self.objects {
            z = orca_stl::max(z, o.max_z());
        }
        z
    }

    /// `update_print_volume_state(build_volume)`: each instance's state, and how many are inside
    /// (logged at info, as the C++ logs it).
    pub fn update_print_volume_state(
        &mut self,
        build_volume: &crate::build_volume::BuildVolume,
    ) -> u32 {
        let mut num_printable = 0;
        for o in &mut self.objects {
            num_printable += o.update_instances_print_volume_state(build_volume);
        }
        let pv = build_volume.bounding_volume();
        let f = |v: f64| {
            orca_fmt::ostream::f64_to_string(&mut orca_fmt::ostream::Stream::default(), v)
                .unwrap_or_default()
        };
        log(
            Severity::Info,
            &format!(
                "Slic3r::Model::update_print_volume_state, print_volume {{{}, {}, {}}} to {{{}, {}, {}}}, got {} printable istances",
                f(pv.min.x()),
                f(pv.min.y()),
                f(pv.min.z()),
                f(pv.max.x()),
                f(pv.max.y()),
                f(pv.max.z()),
                num_printable
            ),
        );
        num_printable
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orca_eigen::Vec3f;
    use orca_mesh::admesh::IndexedTriangleSet;

    fn cube(o: [f32; 3], s: f32) -> TriangleMesh {
        let v = |x: f32, y: f32, z: f32| Vec3f::new(o[0] + x * s, o[1] + y * s, o[2] + z * s);
        let vertices = vec![
            v(0.0, 0.0, 0.0),
            v(1.0, 0.0, 0.0),
            v(1.0, 1.0, 0.0),
            v(0.0, 1.0, 0.0),
            v(0.0, 0.0, 1.0),
            v(1.0, 0.0, 1.0),
            v(1.0, 1.0, 1.0),
            v(0.0, 1.0, 1.0),
        ];
        let f = |a: i32, b: i32, c: i32| orca_eigen::Vec3i32::new(a, b, c);
        let indices = vec![
            f(0, 2, 1),
            f(0, 3, 2),
            f(4, 5, 6),
            f(4, 6, 7),
            f(0, 1, 5),
            f(0, 5, 4),
            f(1, 2, 6),
            f(1, 6, 5),
            f(2, 3, 7),
            f(2, 7, 6),
            f(3, 0, 4),
            f(3, 4, 7),
        ];
        TriangleMesh::from_its(IndexedTriangleSet::new(indices, vertices))
    }

    #[test]
    fn an_object_is_centred_and_dropped_on_the_bed() {
        let mut model = Model::default();
        model.add_object("cube.stl", "C:\\cube.stl", cube([10.0, 10.0, 5.0], 20.0));
        model.add_default_instances();
        let o = &mut model.objects[0];
        assert_eq!(o.volumes[0].get_offset(), Vec3d::new(20.0, 20.0, 15.0));
        assert_eq!(o.config.extruder(), 1);
        o.ensure_on_bed(false);
        assert_eq!(o.instances[0].get_offset(), Vec3d::new(0.0, 0.0, -5.0));
        assert_eq!(o.min_z(), 0.0);
        assert_eq!(o.get_instance_min_z(0), 0.0);
        let hull = o.instance_convex_hull_2d(0);
        assert_eq!(hull.points.len(), 4);
    }

    // Expected values read from Model.cpp:2717-2748: the 3MF subtypes' names, MODEL_PART for any
    // other string, "normal_part" for INVALID.
    #[test]
    fn volume_types_by_their_3mf_names() {
        let t = ModelVolumeType::from_string;
        assert_eq!(t("normal_part"), ModelVolumeType::ModelPart);
        assert_eq!(t("negative_part"), ModelVolumeType::NegativeVolume);
        assert_eq!(t("modifier_part"), ModelVolumeType::ParameterModifier);
        assert_eq!(t("support_enforcer"), ModelVolumeType::SupportEnforcer);
        assert_eq!(t("support_blocker"), ModelVolumeType::SupportBlocker);
        assert_eq!(t("ModelPart"), ModelVolumeType::ModelPart);
        assert_eq!(t(""), ModelVolumeType::ModelPart);
        assert_eq!(t("Support_Blocker"), ModelVolumeType::ModelPart);
        for ty in [
            ModelVolumeType::ModelPart,
            ModelVolumeType::NegativeVolume,
            ModelVolumeType::ParameterModifier,
            ModelVolumeType::SupportEnforcer,
            ModelVolumeType::SupportBlocker,
        ] {
            assert_eq!(t(ty.to_str()), ty);
        }
        assert_eq!(ModelVolumeType::Invalid.to_str(), "normal_part");
    }
}
