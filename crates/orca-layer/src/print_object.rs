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
// Carries OrcaSlicer code: src/libslic3r/Print.hpp lines 1-101 and 154-592 of OrcaSlicer v2.4.2
// (its lines 102-153, PrintRegion, are in print_region.rs), with the values of PrintObject's two
// static flags from PrintObjectSlice.cpp lines 21-22, translated on 2026-11-14 (orca-o3-layer-2).
// Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab), the PrusaSlicer authors
// (Prusa Research) and the Slic3r authors (Alessandro Ranellucci), under the GNU Affero General
// Public License, version 3 (AGPL-3.0; LICENSE).

//! Print.hpp up to `PrintObject`: the print and object steps, the volumes' first-layer slices,
//! [`PrintInstance`], [`PrintObjectRegions`] and [`PrintObject`]'s declaration with the members
//! the header defines.
//!
//! How the C++ pointers are held:
//!
//! - A print object's regions ([`PrintObjectRegions`]) are shared by the print objects of one
//!   model object: an `Arc`, which also does the C++'s manual count (`ref_cnt_inc`,
//!   `ref_cnt_dec`, the `delete` at zero); `Arc::ptr_eq` is the C++'s pointer comparison.
//! - The object's model object (`m_model_object`) is an index into the print's `model.objects`;
//!   an instance's model instance an index into that model object's `instances`; a volume
//!   region's model volume an index into its `volumes`; its region and a painted region's an
//!   index into `all_regions`; its `bbox` an index into its layer range's `volumes`;
//!   `prev_same_region` an index into the layer range's `volume_regions`; `m_shared_object` an
//!   index into the print's objects. An instance's print object is the object holding it.
//! - A layer range's config, which the C++ points at in the model object, is a copy (only read).
//! - `ConstLayerPtrsAdaptor` and `ConstSupportLayerPtrsAdaptor` are slices.
//!
//! Held beside the object, not in it, as their types are later phases': the tree-support preview
//! cache (`m_tree_support_preview_cache`, `alloc_tree_support_preview_cache`,
//! `clear_tree_support_preview_cache`; phase O5), the adaptive and support infill octrees
//! (`m_adaptive_fill_octrees`, `prepare_adaptive_infill_data`; phase O5) and the lightning
//! generator (`m_lightning_generator`, `prepare_lightning_infill_data`; outside domain A's fill
//! patterns, phase O5).
//!
//! The methods with bodies in PrintObject.cpp, PrintObjectSlice.cpp, PrintApply.cpp, Print.cpp
//! and the support code are written with their bodies, as free functions or extension traits of
//! the crates that carry them (orca-print, orca-perimeter, orca-surface, orca-fill,
//! orca-support, orca-plate, orca-process): the constructor's body, `clear_layers`,
//! `get_layer_at_printz` and its kin, `add_layer`, the support layer functions,
//! `update_layer_height_profile`, `slicing_parameters(config, …)`, `all_regions`,
//! `object_extruders`, `slice`, `slice_support_volumes`, `project_and_append_custom_facets`,
//! `get_first_layer_bbox`, `get_certain_layers`, `get_instances_shift_without_plate_offset`,
//! `set_shared_object`, `clear_shared_object`, `copy_layers_from_shared_object`,
//! `copy_layers_overhang_from_shared_object`, `generate_support_preview`,
//! `remove_bridges_from_contacts`, `set_instances`, `invalidate_step`, `invalidate_all_steps`,
//! `invalidate_state_by_config_options`, `update_slicing_parameters`,
//! `object_config_from_model_object`, the steps (`make_perimeters` to `simplify_extrusion_path`),
//! `detect_extruder_geometric_unprintables`, `slice_volumes`, `_shrink_contour_holes`,
//! `detect_overhangs_for_lift`, `clear_overhangs_for_lift`, `_transform_hole_to_polyholes`, the
//! surface steps, `is_support_necessary` and `apply_conical_overhang`; `PrintInstance`'s
//! `get_bounding_box`, `get_convex_hull_2d` and `shift_without_plate_offset`; and
//! `FuzzySkinPaintedRegion`'s `parent_print_object_region` and `parent_print_object_region_id`.

use std::sync::Arc;

use orca_config::config::Config;
use orca_config::print_config::BrimType;
use orca_config::{ConfigBase, ConfigError, PrintObjectConfig};
use orca_eigen::{Transform3d, Vec3crd, Vec3d};
use orca_geom::bounding_box::BoundingBox;
use orca_geom::expolygon::ExPolygons;
use orca_geom::point::{Point, unscale};
use orca_index::aligned_box::AlignedBox;
use orca_index::stop::not_ported;
use orca_model::model::{Model, ModelObject};
use orca_model::object_id::ObjectID;
use orca_slice::slicing::{LayerHeightRange, SlicingParameters};

use crate::collection::ExtrusionEntityCollection;
use crate::layer::{Layer, LayerPtrs, SupportLayer, SupportLayerPtrs};
use crate::print_base::{PrintObjectBaseWithState, StepType};
use crate::print_region::PrintRegion;

/// `MAX_OUTER_NOZZLE_DIAMETER`.
pub const MAX_OUTER_NOZZLE_DIAMETER: f64 = 4.;

/// `struct VolumeSlices` (BBS): a volume's slices.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VolumeSlices {
    /// `volume_id`.
    pub volume_id: ObjectID,
    /// `slices`.
    pub slices: Vec<ExPolygons>,
}

/// `struct groupedVolumeSlices`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupedVolumeSlices {
    /// `groupId`.
    pub group_id: i32,
    /// `volume_ids`.
    pub volume_ids: Vec<ObjectID>,
    /// `slices`.
    pub slices: ExPolygons,
}

impl Default for GroupedVolumeSlices {
    fn default() -> Self {
        Self {
            group_id: -1,
            volume_ids: Vec::new(),
            slices: ExPolygons::new(),
        }
    }
}

/// `enum SupportNecessaryType`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum SupportNecessaryType {
    /// `NoNeedSupp`.
    #[default]
    NoNeedSupp = 0,
    /// `SharpTail`.
    SharpTail = 1,
    /// `Cantilever`.
    Cantilever = 2,
    /// `LargeOverhang`.
    LargeOverhang = 3,
}

/// `enum PrintStep`: the print's steps, in their order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(i32)]
pub enum PrintStep {
    /// `psWipeTower`, also `psToolOrdering`.
    WipeTower = 0,
    /// `psSkirtBrim`, also `psSlicingFinished`, the last step before the G-code export.
    SkirtBrim = 1,
    /// `psGCodeExport`.
    GCodeExport = 2,
    /// `psConflictCheck`.
    ConflictCheck = 3,
}

impl PrintStep {
    /// `psToolOrdering`, a synonym of `psWipeTower`.
    pub const TOOL_ORDERING: Self = Self::WipeTower;
    /// `psSlicingFinished`, a synonym of `psSkirtBrim`.
    pub const SLICING_FINISHED: Self = Self::SkirtBrim;
}

/// `psCount`.
pub const PS_COUNT: usize = 4;

impl StepType for PrintStep {
    fn index(self) -> usize {
        self as usize
    }
    fn from_index(index: usize) -> Self {
        match index {
            0 => Self::WipeTower,
            1 => Self::SkirtBrim,
            2 => Self::GCodeExport,
            _ => Self::ConflictCheck,
        }
    }
}

/// `enum PrintObjectStep`: an object's steps, in their order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(i32)]
pub enum PrintObjectStep {
    /// `posSlice`.
    Slice = 0,
    /// `posPerimeters`.
    Perimeters = 1,
    /// `posEstimateCurledExtrusions`.
    EstimateCurledExtrusions = 2,
    /// `posPrepareInfill`.
    PrepareInfill = 3,
    /// `posInfill`.
    Infill = 4,
    /// `posIroning`.
    Ironing = 5,
    /// `posContouring`.
    Contouring = 6,
    /// `posSupportMaterial`.
    SupportMaterial = 7,
    /// `posSimplifyPath`.
    SimplifyPath = 8,
    /// `posSimplifySupportPath`.
    SimplifySupportPath = 9,
    /// `posDetectOverhangsForLift` (BBS).
    DetectOverhangsForLift = 10,
    /// `posSimplifyWall`.
    SimplifyWall = 11,
    /// `posSimplifyInfill`.
    SimplifyInfill = 12,
}

/// `posCount`.
pub const POS_COUNT: usize = 13;

impl StepType for PrintObjectStep {
    fn index(self) -> usize {
        self as usize
    }
    fn from_index(index: usize) -> Self {
        match index {
            0 => Self::Slice,
            1 => Self::Perimeters,
            2 => Self::EstimateCurledExtrusions,
            3 => Self::PrepareInfill,
            4 => Self::Infill,
            5 => Self::Ironing,
            6 => Self::Contouring,
            7 => Self::SupportMaterial,
            8 => Self::SimplifyPath,
            9 => Self::SimplifySupportPath,
            10 => Self::DetectOverhangsForLift,
            11 => Self::SimplifyWall,
            _ => Self::SimplifyInfill,
        }
    }
}

/// `struct PrintInstance`: one instance of a print object. Its print object is the object that
/// holds it. The aggregate initialisation the C++ uses leaves `id` and `unique_id` 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrintInstance {
    /// `model_instance`: the index of its model instance in the model object's `instances`.
    pub model_instance: usize,
    /// `shift`: the shift of the instance's centre into world coordinates.
    pub shift: Point,
    /// `id` (SoftFever): the instance id.
    pub id: usize,
    /// `unique_id` (Orca): the id the firmware's cancel-object feature uses.
    pub unique_id: usize,
}

impl PrintInstance {
    /// `PrintInstance{ print_object, model_instance, shift }`.
    #[must_use]
    pub fn new(model_instance: usize, shift: Point) -> Self {
        Self {
            model_instance,
            shift,
            id: 0,
            unique_id: 0,
        }
    }
}

/// `PrintInstances`.
pub type PrintInstances = Vec<PrintInstance>;

/// `PrintObjectRegions::BoundingBox`: `Eigen::AlignedBox<float, 3>`.
pub type RegionsBoundingBox = AlignedBox<f32, 3>;

/// `PrintObjectRegions::VolumeExtents`: a volume's box in the object's space, maybe clipped by a
/// layer range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VolumeExtents {
    /// `volume_id`.
    pub volume_id: ObjectID,
    /// `bbox`.
    pub bbox: RegionsBoundingBox,
}

/// `PrintObjectRegions::VolumeRegion`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VolumeRegion {
    /// `model_volume`: the index of its volume in the model object's `volumes`; none for null.
    pub model_volume: Option<usize>,
    /// `parent`: the index of the parent volume region, -1 for none.
    pub parent: i32,
    /// `region`: the index in `all_regions`; none for a negative volume.
    pub region: Option<usize>,
    /// `bbox`: the index of its `VolumeExtents` in the layer range's `volumes`.
    pub bbox: Option<usize>,
    /// `prev_same_region`: the index of the previous volume region of the same region.
    pub prev_same_region: Option<usize>,
}

impl Default for VolumeRegion {
    fn default() -> Self {
        Self {
            model_volume: None,
            parent: -1,
            region: None,
            bbox: None,
            prev_same_region: None,
        }
    }
}

/// `PrintObjectRegions::PaintedRegion`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaintedRegion {
    /// `extruder_id`: 1-based.
    pub extruder_id: u32,
    /// `parent`: the index of the parent volume region, -1 for none.
    pub parent: i32,
    /// `region`: the index in `all_regions`.
    pub region: Option<usize>,
}

impl Default for PaintedRegion {
    fn default() -> Self {
        Self {
            extruder_id: 0,
            parent: -1,
            region: None,
        }
    }
}

/// `PrintObjectRegions::FuzzySkinPaintedRegion::ParentType`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FuzzySkinParentType {
    /// `VolumeRegion`.
    #[default]
    VolumeRegion,
    /// `PaintedRegion`.
    PaintedRegion,
}

/// `PrintObjectRegions::FuzzySkinPaintedRegion`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FuzzySkinPaintedRegion {
    /// `parent_type`.
    pub parent_type: FuzzySkinParentType,
    /// `parent`: the index of the parent volume or painted region, -1 for none.
    pub parent: i32,
    /// `region`: the index in `all_regions`.
    pub region: Option<usize>,
}

impl Default for FuzzySkinPaintedRegion {
    fn default() -> Self {
        Self {
            parent_type: FuzzySkinParentType::VolumeRegion,
            parent: -1,
            region: None,
        }
    }
}

/// `PrintObjectRegions::LayerRangeRegions`: one slab of the object, its volumes and regions.
#[derive(Clone, Debug, Default)]
pub struct LayerRangeRegions {
    /// `layer_height_range`.
    pub layer_height_range: LayerHeightRange,
    /// `config`: the layer range's config, none for one range without an override (a copy of
    /// the model object's).
    pub config: Option<Config>,
    /// `volumes`: sorted by volume id.
    pub volumes: Vec<VolumeExtents>,
    /// `volume_regions`: in the order of their volumes.
    pub volume_regions: Vec<VolumeRegion>,
    /// `painted_regions`.
    pub painted_regions: Vec<PaintedRegion>,
    /// `fuzzy_skin_painted_regions`.
    pub fuzzy_skin_painted_regions: Vec<FuzzySkinPaintedRegion>,
}

impl LayerRangeRegions {
    /// `has_volume(id)`: by `lower_bound_by_predicate` over the sorted volumes.
    #[must_use]
    pub fn has_volume(&self, id: ObjectID) -> bool {
        let it =
            orca_stl::lower_bound_by(&self.volumes, &id, |l: &VolumeExtents, id: &ObjectID| {
                l.volume_id < *id
            });
        it != self.volumes.len() && self.volumes[it].volume_id == id
    }
}

/// `class PrintObjectRegions`: the regions of the print objects of one model object.
#[derive(Clone, Debug)]
pub struct PrintObjectRegions {
    /// `all_regions`.
    pub all_regions: Vec<PrintRegion>,
    /// `layer_ranges`.
    pub layer_ranges: Vec<LayerRangeRegions>,
    /// `trafo_bboxes`: the model object into one of its print objects, for the volume extents.
    pub trafo_bboxes: Transform3d,
    /// `cached_volume_ids`.
    pub cached_volume_ids: Vec<ObjectID>,
}

impl Default for PrintObjectRegions {
    /// The C++ leaves `trafo_bboxes` uninitialised; here it is the identity.
    fn default() -> Self {
        Self {
            all_regions: Vec::new(),
            layer_ranges: Vec::new(),
            trafo_bboxes: Transform3d::identity(),
            cached_volume_ids: Vec::new(),
        }
    }
}

impl PrintObjectRegions {
    /// `clear()`.
    pub fn clear(&mut self) {
        self.all_regions.clear();
        self.layer_ranges.clear();
        self.cached_volume_ids.clear();
    }
}

/// `class PrintObject`.
#[derive(Clone, Debug)]
pub struct PrintObject {
    /// The `PrintObjectBaseWithState<Print, PrintObjectStep, posCount>` it derives from.
    pub base: PrintObjectBaseWithState<PrintObjectStep, POS_COUNT>,
    /// `m_size`: XYZ in scaled coordinates (zero until the constructor's body sets it).
    pub size: Vec3crd,
    /// `m_max_z` (0 until the constructor's body sets it).
    pub max_z: f64,
    /// `m_config`.
    pub config: PrintObjectConfig,
    /// `m_trafo`: translation in Z, rotation, scaling and mirroring.
    pub trafo: Transform3d,
    /// `m_instances`: in scaled G-code coordinates.
    pub instances: PrintInstances,
    /// `m_center_offset`: the shift of the object's system to the print object's.
    pub center_offset: Point,
    /// `m_shared_regions`: shared by the print objects of one model object.
    pub shared_regions: Option<Arc<PrintObjectRegions>>,
    /// `m_slicing_params`.
    pub slicing_params: SlicingParameters,
    /// `m_layers`.
    pub layers: LayerPtrs,
    /// `m_support_layers`.
    pub support_layers: SupportLayerPtrs,
    /// `m_typed_slices`: the region slices are split into top, internal and bottom.
    pub typed_slices: bool,
    /// `firstLayerObjSliceByVolume`.
    pub first_layer_obj_slice_by_volume: Vec<VolumeSlices>,
    /// `firstLayerObjSliceByGroups`.
    pub first_layer_obj_slice_by_groups: Vec<GroupedVolumeSlices>,
    /// `m_skirt` (BBS): the object's own skirt.
    pub skirt: ExtrusionEntityCollection,
    /// `m_shared_object`: the index of the print object whose layers this one shares.
    pub shared_object: Option<usize>,
    /// `m_id` (SoftFever): the object id (uninitialised in the C++ until `set_id`; here 0).
    pub id: usize,
    /// `firstLayerObjectBrimBoundingBox` (BBS).
    pub first_layer_object_brim_bounding_box: BoundingBox,
    /// `object_first_layer_wall_extruders` (BBS): 1-based.
    pub object_first_layer_wall_extruders: Vec<i32>,
}

impl PrintObject {
    /// `clip_multipart_objects` (PrintObjectSlice.cpp line 21; never assigned).
    pub const CLIP_MULTIPART_OBJECTS: bool = true;
    /// `infill_only_where_needed` (PrintObjectSlice.cpp line 22; never assigned).
    pub const INFILL_ONLY_WHERE_NEEDED: bool = false;

    /// The members as `PrintObject(print, model_object, trafo, instances)` initialises them before
    /// its body: the base (a new id, the model object), the transformation, the rest defaulted.
    /// The body (PrintObject.cpp) is orca-print's, and runs after this.
    #[must_use]
    pub fn with_members(model_object: usize, trafo: Transform3d) -> Self {
        Self {
            base: PrintObjectBaseWithState::new(model_object),
            size: Vec3crd::new(0, 0, 0),
            max_z: 0.,
            config: PrintObjectConfig::default(),
            trafo,
            instances: PrintInstances::new(),
            center_offset: Point::default(),
            shared_regions: None,
            slicing_params: SlicingParameters::default(),
            layers: LayerPtrs::new(),
            support_layers: SupportLayerPtrs::new(),
            typed_slices: false,
            first_layer_obj_slice_by_volume: Vec::new(),
            first_layer_obj_slice_by_groups: Vec::new(),
            skirt: ExtrusionEntityCollection::default(),
            shared_object: None,
            id: 0,
            first_layer_object_brim_bounding_box: BoundingBox::new(),
            object_first_layer_wall_extruders: Vec::new(),
        }
    }

    /// `ObjectBase::id()`.
    #[must_use]
    pub fn object_id(&self) -> ObjectID {
        self.base.base.id()
    }

    /// `model_object()`: found in the print's model.
    #[must_use]
    pub fn model_object<'a>(&self, model: &'a Model) -> &'a ModelObject {
        self.base.base.model_object(model)
    }

    /// `size()`.
    #[must_use]
    pub fn size(&self) -> &Vec3crd {
        &self.size
    }

    /// `config()`.
    #[must_use]
    pub fn config(&self) -> &PrintObjectConfig {
        &self.config
    }

    /// `configBrimWidth(m)`.
    pub fn config_brim_width(&mut self, m: f64) {
        self.config.brim_width.value = m;
    }

    /// `layers()`.
    #[must_use]
    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    /// `layers()`, the non-const form that lets its caller change them.
    pub fn layers_mut(&mut self) -> &mut LayerPtrs {
        &mut self.layers
    }

    /// `support_layers()`.
    #[must_use]
    pub fn support_layers(&self) -> &[SupportLayer] {
        &self.support_layers
    }

    /// `support_layers()`, the non-const form.
    pub fn support_layers_mut(&mut self) -> &mut SupportLayerPtrs {
        &mut self.support_layers
    }

    /// `trafo()`.
    #[must_use]
    pub fn trafo(&self) -> &Transform3d {
        &self.trafo
    }

    /// `trafo_centered()`: the transformation, then the centring offset taken off in XY.
    #[must_use]
    pub fn trafo_centered(&self) -> Transform3d {
        let mut t = self.trafo;
        t.pretranslate(&Vec3d::new(
            -unscale(self.center_offset.x),
            -unscale(self.center_offset.y),
            0.,
        ));
        t
    }

    /// `instances()`.
    #[must_use]
    pub fn instances(&self) -> &PrintInstances {
        &self.instances
    }

    /// `instances()`, the non-const form.
    pub fn instances_mut(&mut self) -> &mut PrintInstances {
        &mut self.instances
    }

    /// `bounding_box()`: half the size each way from the origin, by integer division.
    #[must_use]
    pub fn bounding_box(&self) -> BoundingBox {
        BoundingBox::from_corners(
            Point::new(
                self.size.x().wrapping_neg() / 2,
                self.size.y().wrapping_neg() / 2,
            ),
            Point::new(self.size.x() / 2, self.size.y() / 2),
        )
    }

    /// `height()`: the snug height, scaled.
    #[must_use]
    pub fn height(&self) -> i64 {
        self.size.z()
    }

    /// `max_z()`.
    #[must_use]
    pub fn max_z(&self) -> f64 {
        self.max_z
    }

    /// `center_offset()`.
    #[must_use]
    pub fn center_offset(&self) -> &Point {
        &self.center_offset
    }

    /// `firstLayerObjSlice()`.
    #[must_use]
    pub fn first_layer_obj_slice(&self) -> &[VolumeSlices] {
        &self.first_layer_obj_slice_by_volume
    }

    /// `firstLayerObjSliceMod()`.
    pub fn first_layer_obj_slice_mod(&mut self) -> &mut Vec<VolumeSlices> {
        &mut self.first_layer_obj_slice_by_volume
    }

    /// `firstLayerObjGroups()`.
    #[must_use]
    pub fn first_layer_obj_groups(&self) -> &[GroupedVolumeSlices] {
        &self.first_layer_obj_slice_by_groups
    }

    /// `firstLayerObjGroupsMod()`.
    pub fn first_layer_obj_groups_mod(&mut self) -> &mut Vec<GroupedVolumeSlices> {
        &mut self.first_layer_obj_slice_by_groups
    }

    /// `has_brim()`: a brim of some width, an automatic brim, or a painted brim with points;
    /// never with a raft. The model object is the print's (for its brim points).
    #[must_use]
    pub fn has_brim(&self, model_object: &ModelObject) -> bool {
        let brim_type = self.config().brim_type.value;
        ((brim_type != BrimType::btNoBrim && self.config().brim_width.value > 0.)
            || brim_type == BrimType::btAutoBrim
            || (brim_type == BrimType::btPainted && !model_object.brim_points.is_empty()))
            && !self.has_raft()
    }

    /// `object_skirt()`.
    #[must_use]
    pub fn object_skirt(&self) -> &ExtrusionEntityCollection {
        &self.skirt
    }

    /// `total_layer_count()`: layers and support layers.
    #[must_use]
    pub fn total_layer_count(&self) -> usize {
        self.layer_count() + self.support_layer_count()
    }

    /// `layer_count()`.
    #[must_use]
    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }

    /// `get_layer(idx)` (the C++ takes an `int`; a negative one is undefined).
    #[must_use]
    pub fn get_layer(&self, idx: usize) -> &Layer {
        &self.layers[idx]
    }

    /// `get_layer(idx)`, to change it.
    pub fn get_layer_mut(&mut self, idx: usize) -> &mut Layer {
        &mut self.layers[idx]
    }

    /// `support_layer_count()`.
    #[must_use]
    pub fn support_layer_count(&self) -> usize {
        self.support_layers.len()
    }

    /// `get_support_layer(idx)`: none when `idx`, converted to `size_t` as the C++ compares it
    /// (a negative one wraps to a huge index), is past the support layers.
    pub fn get_support_layer(&mut self, idx: i32) -> Option<&mut SupportLayer> {
        #[allow(clippy::cast_sign_loss)] // The C++ converts the int to size_t.
        let i = idx as isize as usize;
        if i < self.support_layers.len() {
            Some(&mut self.support_layers[i])
        } else {
            None
        }
    }

    /// `slicing_parameters()`.
    #[must_use]
    pub fn slicing_parameters(&self) -> &SlicingParameters {
        &self.slicing_params
    }

    /// The shared regions, which the C++ dereferences unchecked.
    fn regions(&self) -> &PrintObjectRegions {
        match &self.shared_regions {
            Some(r) => r,
            None => not_ported(
                "PrintObject::num_printing_regions, PrintObject::printing_region",
                "the object has no regions (the C++ dereferences a null m_shared_regions)",
            ),
        }
    }

    /// `num_printing_regions()`.
    #[must_use]
    pub fn num_printing_regions(&self) -> usize {
        self.regions().all_regions.len()
    }

    /// `printing_region(idx)`.
    #[must_use]
    pub fn printing_region(&self, idx: usize) -> &PrintRegion {
        &self.regions().all_regions[idx]
    }

    /// `shared_regions()`.
    #[must_use]
    pub fn shared_regions(&self) -> Option<&PrintObjectRegions> {
        self.shared_regions.as_deref()
    }

    /// `has_support()`: supports on, or enforced support layers.
    #[must_use]
    pub fn has_support(&self) -> bool {
        self.config.enable_support.value || self.config.enforce_support_layers.value > 0
    }

    /// `has_raft()`.
    #[must_use]
    pub fn has_raft(&self) -> bool {
        self.config.raft_layers.value > 0
    }

    /// `has_support_material()`.
    #[must_use]
    pub fn has_support_material(&self) -> bool {
        self.has_support() || self.has_raft()
    }

    /// `is_mm_painted()`: the model object is painted with the multi-material gizmo.
    #[must_use]
    pub fn is_mm_painted(&self, model_object: &ModelObject) -> bool {
        model_object.is_mm_painted()
    }

    /// `is_fuzzy_skin_painted()`.
    #[must_use]
    pub fn is_fuzzy_skin_painted(&self, model_object: &ModelObject) -> bool {
        model_object.is_fuzzy_skin_painted()
    }

    /// `get_shared_object()`.
    #[must_use]
    pub fn get_shared_object(&self) -> Option<usize> {
        self.shared_object
    }

    /// `get_id()`.
    #[must_use]
    pub fn get_id(&self) -> usize {
        self.id
    }

    /// `set_id(id)`.
    pub fn set_id(&mut self, id: usize) {
        self.id = id;
    }

    /// `config_apply(other, ignore_nonexistent)`.
    ///
    /// # Errors
    ///
    /// What `ConfigBase::apply` refuses.
    pub fn config_apply(
        &mut self,
        other: &dyn ConfigBase,
        ignore_nonexistent: bool,
    ) -> Result<(), ConfigError> {
        self.config.apply_config(other, ignore_nonexistent)
    }

    /// `config_apply_only(other, keys, ignore_nonexistent)`.
    ///
    /// # Errors
    ///
    /// What `ConfigBase::apply_only` refuses.
    pub fn config_apply_only(
        &mut self,
        other: &dyn ConfigBase,
        keys: &[String],
        ignore_nonexistent: bool,
    ) -> Result<(), ConfigError> {
        self.config.apply_only(other, keys, ignore_nonexistent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orca_config::ConfigOptionEnum;

    #[test]
    fn the_steps_count_and_alias() {
        assert_eq!(PrintStep::TOOL_ORDERING, PrintStep::WipeTower);
        assert_eq!(PrintStep::SLICING_FINISHED.index(), 1);
        assert_eq!(
            PrintStep::from_index(PS_COUNT - 1),
            PrintStep::ConflictCheck
        );
        assert_eq!(
            PrintObjectStep::from_index(POS_COUNT - 1),
            PrintObjectStep::SimplifyInfill
        );
        for i in 0..POS_COUNT {
            assert_eq!(PrintObjectStep::from_index(i).index(), i);
        }
    }

    #[test]
    fn an_object_reads_its_regions_and_box() {
        let mut o = PrintObject::with_members(0, Transform3d::identity());
        o.size = Vec3crd::new(10_000_001, 4, 7);
        let bb = o.bounding_box();
        assert_eq!(
            (bb.min, bb.max),
            (Point::new(-5_000_000, -2), Point::new(5_000_000, 2))
        );
        assert_eq!(o.height(), 7);
        let mut regions = PrintObjectRegions::default();
        regions.all_regions.push(PrintRegion::default());
        o.shared_regions = Some(Arc::new(regions));
        assert_eq!(o.num_printing_regions(), 1);
        let mut l = Layer::new(0, 0.2, 0.2, 0.1);
        l.regions.push(crate::layer::LayerRegion::new(0));
        assert_eq!(l.regions[0].region(&o).print_object_region_id(), -1);
        assert!(o.get_support_layer(-1).is_none());
        o.config.brim_type = ConfigOptionEnum::new(BrimType::btNoBrim);
        assert!(!o.has_support_material());
    }

    #[test]
    fn has_volume_finds_the_sorted_ids() {
        let r = LayerRangeRegions {
            volumes: vec![
                VolumeExtents {
                    volume_id: ObjectID::new(3),
                    bbox: RegionsBoundingBox::default(),
                },
                VolumeExtents {
                    volume_id: ObjectID::new(9),
                    bbox: RegionsBoundingBox::default(),
                },
            ],
            ..LayerRangeRegions::default()
        };
        assert!(r.has_volume(ObjectID::new(9)) && !r.has_volume(ObjectID::new(4)));
    }
}
