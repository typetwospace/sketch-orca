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
// Carries OrcaSlicer code: src/libslic3r/PrintObject.cpp lines 84-158 (the constructor's body,
// the destructor, set_instances, all_regions), 1022-1089 (the layers and support layers),
// 3835-3859 (get_certain_layers, get_instances_shift_without_plate_offset) and 4676-4725 (the
// layer queries by print z and bottom z), and Print.cpp lines 5136-5169 (PrintInstance's
// get_bounding_box, get_convex_hull_2d and shift_without_plate_offset;
// FuzzySkinPaintedRegion's parent_print_object_region and parent_print_object_region_id), of
// OrcaSlicer v2.4.2, translated on 2026-11-14 (orca-o3-print-1). Copyright the OrcaSlicer
// authors, the BambuStudio authors (Bambu Lab), the PrusaSlicer authors (Prusa Research) and the
// Slic3r authors (Alessandro Ranellucci), under the GNU Affero General Public License, version 3
// (AGPL-3.0; LICENSE).

//! PrintObject.cpp's members of [`PrintObject`] that set an object up and query its layers, as
//! free functions over the object (orca-layer declares `PrintObject`), with Print.cpp's members of
//! [`PrintInstance`] and [`FuzzySkinPaintedRegion`] as the extension traits
//! [`PrintInstanceExt`] and [`FuzzySkinPaintedRegionExt`].
//!
//! The object's back-pointer to its print (`m_print`) is an argument: [`PrintInherited`] (the
//! print's `base`, which holds its steps, its model copy and its cancel callback) where the call
//! changes the print's steps, the [`Print`] where it reads the print's own members. The object
//! is not inside the print while its constructor runs ([`new_print_object`]), and the callers that
//! change an object of the print borrow `print.base` and `print.objects` apart.
//!
//! - [`new_print_object`]: the constructor's body, after orca-layer's `PrintObject::with_members`
//!   (the members it initialises, a new id first). The tree-support preview cache it sets to null
//!   is held beside the object (phase O5's).
//! - [`destructor_record`]: the destructor's info record, which names the object's and its shared
//!   object's addresses as MSVC prints a pointer (16 upper-case hexadecimal digits). Addresses are
//!   never the C++'s, and info records are below the command line's level unless
//!   `SKETCH_ORCA_LOG_LEVEL` asks for them. The rest of the destructor (the shared regions'
//!   count, the layers) is what dropping the object does.
//! - [`set_instances`], [`all_regions`], [`clear_layers`], [`add_layer`],
//!   [`get_support_layer_at_printz`], [`clear_support_layers`], [`add_tree_support_layer`],
//!   [`add_support_layer`], [`insert_support_layer`], [`get_certain_layers`],
//!   [`get_instances_shift_without_plate_offset`], [`get_layer_at_printz`] and its kin.
//!   A const and a non-const overload are one function and its `_mut` form; an overload with an
//!   epsilon is `_epsilon`. A layer pointer is a reference, an iterator an index.
//! - [`alloc_tree_support_preview_cache`] stops with `NotPorted` naming tree-supports: only tree
//!   supports allocate that cache (`TreeSupportData`, phase O5's).
//!
//! The steps' invalidation (`invalidate_step`, `invalidate_all_steps`,
//! `invalidate_state_by_config_options`) is in [`crate::invalidation`]; the polyholes' shapes in
//! [`crate::polyholes`].

use orca_eigen::{AngleAxisd, Transform3d, Vec3crd, Vec3d};
use orca_geom::bounding_box::{BoundingBox, BoundingBoxf3};
use orca_geom::geometry::rotation_diff_z;
use orca_geom::point::{EPSILON, Point, Points, SCALING_FACTOR, scaled};
use orca_geom::polygon::Polygon;
use orca_index::stop::not_ported;
use orca_layer::layer::{Layer, SupportInnerType, SupportLayer};
use orca_layer::print::Print;
use orca_layer::print_base::ApplyStatus;
use orca_layer::print_object::{
    FuzzySkinPaintedRegion, FuzzySkinParentType, LayerRangeRegions, PrintInstance, PrintInstances,
    PrintObject, PrintStep,
};
use orca_layer::print_region::PrintRegion;
use orca_mesh::log::{Severity, log};
use orca_model::model::{InvalidArgument, ModelObject};

use crate::invalidation::print_invalidate_step;
use crate::print::PrintInherited;

/// `PrintObject(print, model_object, trafo, instances)`: the members as orca-layer's
/// `PrintObject::with_members` initialises them (a new id, the model object, the transformation),
/// then the body. The centring offset and the size come from the model object's raw box (from its
/// first instance, without its translation), turned about Z by the difference between the first
/// model instance's rotation and the first print instance's when it passes `EPSILON`; the maximum
/// Z is the first instance's box top, scaled (a `coord_t`, kept as a `double`). Then
/// [`set_instances`].
///
/// # Errors
///
/// `ModelObject::raw_bounding_box`'s, for a model object with no instances.
pub fn new_print_object(
    print: &mut PrintInherited,
    model_object: usize,
    trafo: &Transform3d,
    instances: PrintInstances,
) -> Result<PrintObject, InvalidArgument> {
    // BBS: m_tree_support_preview_cache(nullptr) is held beside the object (phase O5's).
    let mut object = PrintObject::with_members(model_object, *trafo);

    // Compute centering offet to be applied to our meshes so that we work with smaller coordinates
    // requiring less bits to represent Clipper coordinates.

    // Snug bounding box of a rotated and scaled object by the 1st instantion, without the instance translation applied.
    // All the instances share the transformation matrix with the exception of translation in XY and rotation by Z,
    // therefore a bounding box from 1st instance of a ModelObject is good enough for calculating the object center,
    // snug height and an approximate bounding box in XY.
    let mo = &mut print.base.model.objects[model_object];
    let mut bbox: BoundingBoxf3 = mo.raw_bounding_box()?;
    let mut bbox_center: Vec3d = bbox.center();

    // We may need to rotate the bbox / bbox_center from the original instance to the current instance.
    let Some(first) = instances.first() else {
        not_ported(
            "PrintObject::PrintObject",
            "no print instance (the C++ reads instances.front() of an empty list)",
        )
    };
    let z_diff = rotation_diff_z(
        &mo.instances[0].get_rotation(),
        &mo.instances[first.model_instance].get_rotation(),
    );
    if orca_crt::fabs(z_diff) > EPSILON {
        let z_rot = AngleAxisd::new(z_diff, Vec3d::unit_z());
        bbox = bbox.transformed(&Transform3d::from_linear(&z_rot.to_rotation_matrix()));
        bbox_center = z_rot.to_rotation_matrix().mul_vec(&bbox_center);
    }

    // Center of the transformed mesh (without translation).
    object.center_offset = Point::new_scale(bbox_center.x(), bbox_center.y());
    // Size of the transformed mesh. This bounding may not be snug in XY plane, but it is snug in Z.
    let size = bbox.size() * (1. / SCALING_FACTOR);
    object.size = Vec3crd::new(
        orca_crt::f64_to_i64(size.x()),
        orca_crt::f64_to_i64(size.y()),
        orca_crt::f64_to_i64(size.z()),
    );
    // scaled() of a double gives a coord_t, kept in the double m_max_z.
    #[allow(clippy::cast_precision_loss)]
    let max_z = scaled(mo.instance_bounding_box(0, false).max.z()) as f64;
    object.max_z = max_z;

    set_instances(&mut object, print, instances);
    Ok(object)
}

/// The destructor's record, `PrintObject::~PrintObject()`: an info record naming the object's
/// address and its shared object's (null when it shares none), as MSVC prints pointers. The
/// caller then drops the object, which does the rest of the destructor: the shared regions'
/// count goes down (the `Arc`), and the layers and support layers go.
pub fn destructor_record(objects: &[PrintObject], idx: usize) {
    let object = &objects[idx];
    let this = core::ptr::from_ref(object).addr();
    let shared = object
        .shared_object
        .and_then(|s| objects.get(s))
        .map_or(0, |o| core::ptr::from_ref(o).addr());
    log(
        Severity::Info,
        &format!(
            "Slic3r::PrintObject::~PrintObject: this={this:016X}, m_shared_object {shared:016X}"
        ),
    );
}

/// `PrintObject::set_instances(instances)`: the instances, each shifted by the centring offset
/// (which slicing takes off the mesh again). When they differ from the object's (in number, or
/// in an instance's model instance or shift) the status is changed, and invalidated when that
/// invalidates the print's skirt and brim and G-code export, or else, for a change in number, its
/// wipe tower (the `||` skips the wipe tower when the first did).
pub fn set_instances(
    object: &mut PrintObject,
    print: &mut PrintInherited,
    mut instances: PrintInstances,
) -> ApplyStatus {
    for i in &mut instances {
        // Add the center offset, which will be subtracted from the mesh when slicing.
        i.shift += object.center_offset;
    }
    // Invalidate and set copies.
    let mut status = ApplyStatus::Unchanged;
    let equal_length = instances.len() == object.instances.len();
    let equal = equal_length
        && instances
            .iter()
            .zip(object.instances.iter())
            .all(|(lhs, rhs)| lhs.model_instance == rhs.model_instance && lhs.shift == rhs.shift);
    if !equal {
        status = ApplyStatus::Changed;
        if print.invalidate_steps(&[PrintStep::SkirtBrim, PrintStep::GCodeExport])
            || (!equal_length && print_invalidate_step(print, PrintStep::WipeTower))
        {
            status = ApplyStatus::Invalidated;
        }
        // Each instance's print object is the object that holds it.
        object.instances = instances;
    }
    status
}

/// `PrintObject::all_regions()`: the object's regions, none without shared regions.
#[must_use]
pub fn all_regions(object: &PrintObject) -> Vec<&PrintRegion> {
    let mut out: Vec<&PrintRegion> = Vec::new();
    let Some(shared_regions) = object.shared_regions.as_deref() else {
        return out;
    };

    out.reserve(shared_regions.all_regions.len());
    for region in &shared_regions.all_regions {
        out.push(region);
    }
    out
}

/// `PrintObject::clear_layers()`: the layers gone, unless they are another object's.
pub fn clear_layers(object: &mut PrintObject) {
    if object.shared_object.is_none() {
        object.layers.clear();
    }
}

/// `PrintObject::add_layer(id, height, print_z, slice_z)`: a new last layer (its `int` id taken
/// as a `size_t`).
pub fn add_layer(
    object: &mut PrintObject,
    id: i32,
    height: f64,
    print_z: f64,
    slice_z: f64,
) -> &mut Layer {
    #[allow(clippy::cast_sign_loss)]
    let id = id as isize as usize;
    object.layers.push(Layer::new(id, height, print_z, slice_z));
    let last = object.layers.len() - 1;
    &mut object.layers[last]
}

/// The index `lower_bound_by_predicate` finds in the support layers: the first whose print z is
/// not below `print_z - epsilon`, when it is not above `print_z + epsilon`.
fn support_layer_idx_at_printz(object: &PrintObject, print_z: f64, epsilon: f64) -> Option<usize> {
    let limit = print_z - epsilon;
    let it = orca_stl::lower_bound_by(
        &object.support_layers,
        &limit,
        |layer: &SupportLayer, limit: &f64| layer.layer.print_z < *limit,
    );
    if it == object.support_layers.len()
        || object.support_layers[it].layer.print_z > print_z + epsilon
    {
        None
    } else {
        Some(it)
    }
}

/// `PrintObject::get_support_layer_at_printz(print_z, epsilon) const`: the support layer within
/// `epsilon` of `print_z`, the first not below it.
#[must_use]
pub fn get_support_layer_at_printz(
    object: &PrintObject,
    print_z: f64,
    epsilon: f64,
) -> Option<&SupportLayer> {
    support_layer_idx_at_printz(object, print_z, epsilon).map(|i| &object.support_layers[i])
}

/// `PrintObject::get_support_layer_at_printz(print_z, epsilon)`, the non-const one.
pub fn get_support_layer_at_printz_mut(
    object: &mut PrintObject,
    print_z: f64,
    epsilon: f64,
) -> Option<&mut SupportLayer> {
    support_layer_idx_at_printz(object, print_z, epsilon).map(|i| &mut object.support_layers[i])
}

/// `PrintObject::clear_support_layers()`: unless the layers are another object's, the support
/// layers gone and each layer's sharp tails, their heights and cantilevers cleared.
pub fn clear_support_layers(object: &mut PrintObject) {
    if object.shared_object.is_none() {
        object.support_layers.clear();
        for l in &mut object.layers {
            l.sharp_tails.clear();
            l.sharp_tails_height.clear();
            l.cantilevers.clear();
        }
    }
}

/// `PrintObject::alloc_tree_support_preview_cache()`: the tree supports' preview cache
/// (`TreeSupportData`), which only tree supports allocate. Its type and the cache are phase O5's,
/// held beside the object, so this stops with `NotPorted` (tree-supports).
pub fn alloc_tree_support_preview_cache(_object: &mut PrintObject) -> ! {
    not_ported(
        "PrintObject::alloc_tree_support_preview_cache",
        "tree-supports: the tree supports' preview cache (TreeSupportData, phase O5)",
    )
}

/// `PrintObject::add_tree_support_layer(id, height, print_z, slice_z)`: a new last support layer
/// of interface 0, of the tree type.
pub fn add_tree_support_layer(
    object: &mut PrintObject,
    id: i32,
    height: f64,
    print_z: f64,
    slice_z: f64,
) -> &mut SupportLayer {
    #[allow(clippy::cast_sign_loss)]
    let id = id as isize as usize;
    object
        .support_layers
        .push(SupportLayer::new(id, 0, height, print_z, slice_z));
    let last = object.support_layers.len() - 1;
    object.support_layers[last].support_type = SupportInnerType::Tree;
    &mut object.support_layers[last]
}

/// `PrintObject::add_support_layer(id, interface_id, height, print_z)`: a new last support layer
/// with slice z -1.
pub fn add_support_layer(
    object: &mut PrintObject,
    id: i32,
    interface_id: i32,
    height: f64,
    print_z: f64,
) -> &mut SupportLayer {
    #[allow(clippy::cast_sign_loss)]
    let (id, interface_id) = (id as isize as usize, interface_id as isize as usize);
    object
        .support_layers
        .push(SupportLayer::new(id, interface_id, height, print_z, -1.));
    let last = object.support_layers.len() - 1;
    &mut object.support_layers[last]
}

/// `PrintObject::insert_support_layer(pos, id, interface_id, height, print_z, slice_z)`: a new
/// support layer inserted before `pos`; its index.
pub fn insert_support_layer(
    object: &mut PrintObject,
    pos: usize,
    id: usize,
    interface_id: usize,
    height: f64,
    print_z: f64,
    slice_z: f64,
) -> usize {
    object.support_layers.insert(
        pos,
        SupportLayer::new(id, interface_id, height, print_z, slice_z),
    );
    pos
}

/// `PrintObject::get_certain_layers(start, end, out, boundingbox_objects)` (BBS): the layers whose
/// print z is from `start` to `end` (plus `EPSILON`), in order, stopping at the first above, and
/// the box of their overhangs, appended to `out` and `boundingbox_objects`.
pub fn get_certain_layers<'a>(
    object: &'a PrintObject,
    start: f32,
    end: f32,
    out: &mut Vec<Vec<&'a Layer>>,
    boundingbox_objects: &mut Vec<BoundingBox>,
) {
    let mut temp = BoundingBox::new();
    let mut out_temp: Vec<&'a Layer> = Vec::new();
    for layer in object.layers() {
        if layer.print_z < f64::from(start) {
            continue;
        }

        if layer.print_z > f64::from(end) + EPSILON {
            break;
        }
        temp.merge_box(&layer.loverhangs_bbox);
        out_temp.push(layer);
    }
    boundingbox_objects.push(temp);
    out.push(out_temp);
}

/// `PrintObject::get_instances_shift_without_plate_offset()`: each instance's shift without the
/// plate's origin.
#[must_use]
pub fn get_instances_shift_without_plate_offset(object: &PrintObject, print: &Print) -> Points {
    let mut out: Points = Vec::with_capacity(object.instances.len());
    for instance in &object.instances {
        out.push(instance.shift_without_plate_offset(print));
    }

    out
}

/// `lower_bound_by_predicate` over the layers: the first whose print z is not below `limit`.
fn layers_lower_bound(layers: &[Layer], limit: f64) -> usize {
    orca_stl::lower_bound_by(layers, &limit, |layer: &Layer, limit: &f64| {
        layer.print_z < *limit
    })
}

/// The index [`get_layer_at_printz`] finds.
fn layer_idx_at_printz(object: &PrintObject, print_z: f64) -> Option<usize> {
    let it = layers_lower_bound(&object.layers, print_z);
    #[allow(clippy::float_cmp)] // The C++ compares the doubles with ==.
    let missing = it == object.layers.len() || object.layers[it].print_z != print_z;
    if missing { None } else { Some(it) }
}

/// `PrintObject::get_layer_at_printz(print_z) const`: the layer at exactly `print_z`.
#[must_use]
pub fn get_layer_at_printz(object: &PrintObject, print_z: f64) -> Option<&Layer> {
    layer_idx_at_printz(object, print_z).map(|i| &object.layers[i])
}

/// `PrintObject::get_layer_at_printz(print_z)`, the non-const one.
pub fn get_layer_at_printz_mut(object: &mut PrintObject, print_z: f64) -> Option<&mut Layer> {
    layer_idx_at_printz(object, print_z).map(|i| &mut object.layers[i])
}

/// The index [`get_layer_at_printz_epsilon`] finds.
fn layer_idx_at_printz_epsilon(object: &PrintObject, print_z: f64, epsilon: f64) -> Option<usize> {
    let limit = print_z - epsilon;
    let it = layers_lower_bound(&object.layers, limit);
    if it == object.layers.len() || object.layers[it].print_z > print_z + epsilon {
        None
    } else {
        Some(it)
    }
}

/// `PrintObject::get_layer_at_printz(print_z, epsilon) const`: a layer approximately at
/// `print_z`, the first not below `print_z - epsilon` when it is not above `print_z + epsilon`.
#[must_use]
pub fn get_layer_at_printz_epsilon(
    object: &PrintObject,
    print_z: f64,
    epsilon: f64,
) -> Option<&Layer> {
    layer_idx_at_printz_epsilon(object, print_z, epsilon).map(|i| &object.layers[i])
}

/// `PrintObject::get_layer_at_printz(print_z, epsilon)`, the non-const one.
pub fn get_layer_at_printz_epsilon_mut(
    object: &mut PrintObject,
    print_z: f64,
    epsilon: f64,
) -> Option<&mut Layer> {
    layer_idx_at_printz_epsilon(object, print_z, epsilon).map(|i| &mut object.layers[i])
}

/// `PrintObject::get_first_layer_bellow_printz(print_z, epsilon) const`: the layer before the
/// first whose print z is not below `print_z + epsilon`; none when that is the first.
#[must_use]
pub fn get_first_layer_bellow_printz(
    object: &PrintObject,
    print_z: f64,
    epsilon: f64,
) -> Option<&Layer> {
    let limit = print_z + epsilon;
    let it = layers_lower_bound(&object.layers, limit);
    if it == 0 {
        None
    } else {
        Some(&object.layers[it - 1])
    }
}

/// `PrintObject::get_layer_idx_get_printz(print_z, epsilon)`: the count of layers below
/// `print_z + epsilon`, -1 for none (the iterator's distance as an `int`).
#[must_use]
// std::distance's ptrdiff_t converted to int.
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
pub fn get_layer_idx_get_printz(object: &PrintObject, print_z: f64, epsilon: f64) -> i32 {
    let limit = print_z + epsilon;
    let it = layers_lower_bound(&object.layers, limit);
    if it == 0 { -1 } else { it as i32 }
}

/// The index [`get_layer_at_bottomz`] finds.
fn layer_idx_at_bottomz(object: &PrintObject, bottom_z: f64, epsilon: f64) -> Option<usize> {
    let limit_upper = bottom_z + epsilon;
    let limit_lower = bottom_z - epsilon;

    for (i, layer) in object.layers.iter().enumerate() {
        if layer.bottom_z() > limit_lower {
            return if layer.bottom_z() < limit_upper {
                Some(i)
            } else {
                None
            };
        }
    }

    None
}

/// `PrintObject::get_layer_at_bottomz(bottom_z, epsilon) const` (BBS): the first layer whose
/// bottom is above `bottom_z - epsilon`, when it is below `bottom_z + epsilon`.
#[must_use]
pub fn get_layer_at_bottomz(object: &PrintObject, bottom_z: f64, epsilon: f64) -> Option<&Layer> {
    layer_idx_at_bottomz(object, bottom_z, epsilon).map(|i| &object.layers[i])
}

/// `PrintObject::get_layer_at_bottomz(bottom_z, epsilon)`, the non-const one.
pub fn get_layer_at_bottomz_mut(
    object: &mut PrintObject,
    bottom_z: f64,
    epsilon: f64,
) -> Option<&mut Layer> {
    layer_idx_at_bottomz(object, bottom_z, epsilon).map(|i| &mut object.layers[i])
}

/// Print.cpp's members of [`PrintInstance`]. Its print object (`print_object`) is the object that
/// holds it; the calls take what they read through it.
pub trait PrintInstanceExt {
    /// `PrintInstance::get_bounding_box()`: its model object's box through its model instance,
    /// translated. `model_object` is the instance's print object's model object.
    fn get_bounding_box(&self, model_object: &ModelObject) -> BoundingBoxf3;

    /// `PrintInstance::get_convex_hull_2d()`: its model object's 2D hull through its model
    /// instance's matrix, simplified by Douglas-Peucker at 0.1 (scaled units).
    fn get_convex_hull_2d(&self, model_object: &mut ModelObject) -> Polygon;

    /// `PrintInstance::shift_without_plate_offset()` (BBS): its shift less the print's plate
    /// origin, each coordinate `scaled` (truncated).
    fn shift_without_plate_offset(&self, print: &Print) -> Point;
}

impl PrintInstanceExt for PrintInstance {
    fn get_bounding_box(&self, model_object: &ModelObject) -> BoundingBoxf3 {
        model_object.instance_bounding_box(self.model_instance, false)
    }

    fn get_convex_hull_2d(&self, model_object: &mut ModelObject) -> Polygon {
        let trafo = *model_object.instances[self.model_instance].get_matrix();
        let mut poly = model_object.convex_hull_2d(&trafo);
        poly.douglas_peucker(0.1);
        poly
    }

    fn shift_without_plate_offset(&self, print: &Print) -> Point {
        let plate_offset = print.get_plate_origin();
        self.shift - Point::new(scaled(plate_offset.x()), scaled(plate_offset.y()))
    }
}

/// Print.cpp's members of [`FuzzySkinPaintedRegion`].
pub trait FuzzySkinPaintedRegionExt {
    /// `parent_print_object_region(layer_range)`: the region of its parent, a painted region or a
    /// volume region of the layer range (an index into `all_regions`; none for a null region).
    fn parent_print_object_region(&self, layer_range: &LayerRangeRegions) -> Option<usize>;

    /// `parent_print_object_region_id(layer_range)`: that region's print object region id.
    /// `all_regions` are the object's regions the index points into.
    fn parent_print_object_region_id(
        &self,
        layer_range: &LayerRangeRegions,
        all_regions: &[PrintRegion],
    ) -> i32;
}

impl FuzzySkinPaintedRegionExt for FuzzySkinPaintedRegion {
    fn parent_print_object_region(&self, layer_range: &LayerRangeRegions) -> Option<usize> {
        let outside = || -> ! {
            not_ported(
                "PrintObjectRegions::FuzzySkinPaintedRegion::parent_print_object_region",
                "the parent index is outside its list (the C++ reads past it)",
            )
        };
        let parent = usize::try_from(self.parent).unwrap_or_else(|_| outside());

        if self.parent_type == FuzzySkinParentType::PaintedRegion {
            return layer_range
                .painted_regions
                .get(parent)
                .unwrap_or_else(|| outside())
                .region;
        }

        // The C++ asserts the parent is a volume region.
        layer_range
            .volume_regions
            .get(parent)
            .unwrap_or_else(|| outside())
            .region
    }

    fn parent_print_object_region_id(
        &self,
        layer_range: &LayerRangeRegions,
        all_regions: &[PrintRegion],
    ) -> i32 {
        let Some(region) = self.parent_print_object_region(layer_range) else {
            not_ported(
                "PrintObjectRegions::FuzzySkinPaintedRegion::parent_print_object_region_id",
                "the parent's region is null (the C++ dereferences it)",
            )
        };
        all_regions[region].print_object_region_id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orca_eigen::{Vec3f, Vec3i32};
    use orca_index::stop::NotPorted;
    use orca_layer::print_base::StepType;
    use orca_layer::print_object::PS_COUNT;
    use orca_mesh::admesh::IndexedTriangleSet;
    use orca_mesh::triangle_mesh::TriangleMesh;
    use orca_model::model::ModelVolumeType;

    /// A 10 mm cube from the origin, as one part.
    fn cube_object() -> ModelObject {
        let v = |x: f32, y: f32, z: f32| Vec3f::new(x, y, z);
        let vertices = vec![
            v(0., 0., 0.),
            v(10., 0., 0.),
            v(10., 10., 0.),
            v(0., 10., 0.),
            v(0., 0., 10.),
            v(10., 0., 10.),
            v(10., 10., 10.),
            v(0., 10., 10.),
        ];
        let f = |a: i32, b: i32, c: i32| Vec3i32::new(a, b, c);
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
        let mut o = ModelObject::default();
        o.add_volume(
            TriangleMesh::from_its(IndexedTriangleSet::new(indices, vertices)),
            ModelVolumeType::ModelPart,
            false,
        );
        let i0 = o.add_instance();
        o.instances[i0].set_offset(&Vec3d::new(50., 50., 0.));
        let i1 = o.add_instance();
        o.instances[i1].set_offset(&Vec3d::new(80., 50., 0.));
        o.instances[i1].set_rotation(&Vec3d::new(0., 0., orca_geom::point::PI / 2.));
        o
    }

    fn print_with_cube() -> Print {
        let mut print = Print::default();
        print.base.base.model.objects.push(cube_object());
        print
    }

    fn done_print() -> Print {
        let mut print = print_with_cube();
        for i in 0..PS_COUNT {
            let step = PrintStep::from_index(i);
            print.base.set_started(step);
            print.base.set_done(step);
        }
        print
    }

    fn print_done(print: &Print) -> Vec<bool> {
        (0..PS_COUNT)
            .map(|i| print.base.is_step_done_unguarded(PrintStep::from_index(i)))
            .collect()
    }

    #[test]
    fn the_constructor_centres_the_object() {
        let mut print = print_with_cube();
        let instances = vec![PrintInstance::new(0, Point::new(100, 200))];
        let object =
            new_print_object(&mut print.base, 0, &Transform3d::identity(), instances).unwrap();
        assert_eq!(object.center_offset, Point::new_scale(5., 5.));
        let size = Vec3d::new(10., 10., 10.) * (1. / SCALING_FACTOR);
        assert_eq!(
            object.size,
            Vec3crd::new(
                orca_crt::f64_to_i64(size.x()),
                orca_crt::f64_to_i64(size.y()),
                orca_crt::f64_to_i64(size.z())
            )
        );
        #[allow(clippy::cast_precision_loss)]
        let max_z = scaled(10.) as f64;
        assert!(orca_crt::fabs(object.max_z - max_z) < 1e-9);
        // The shift takes the centring offset.
        assert_eq!(
            object.instances[0].shift,
            Point::new(100, 200) + Point::new_scale(5., 5.)
        );
    }

    #[test]
    fn the_constructor_turns_the_box_to_the_first_instance() {
        let mut print = print_with_cube();
        let instances = vec![PrintInstance::new(1, Point::new(0, 0))];
        let object =
            new_print_object(&mut print.base, 0, &Transform3d::identity(), instances).unwrap();
        // Turned a quarter about Z, the centre (5, 5) goes to (-5, 5).
        assert!(
            (object.center_offset.x + 5_000_000).abs() <= 1,
            "{:?}",
            object.center_offset
        );
        assert!(
            (object.center_offset.y - 5_000_000).abs() <= 1,
            "{:?}",
            object.center_offset
        );
        assert!((object.size.x() - 10_000_000).abs() <= 2);
        assert!((object.size.y() - 10_000_000).abs() <= 2);
    }

    #[test]
    fn set_instances_reports_and_invalidates() {
        let mut print = done_print();
        let mut object = PrintObject::with_members(0, Transform3d::identity());
        object.center_offset = Point::new(10, 20);
        let two = vec![
            PrintInstance::new(0, Point::new(0, 0)),
            PrintInstance::new(1, Point::new(5, 5)),
        ];
        // New instances: the skirt and brim and the G-code export go, and the || stops there.
        assert_eq!(
            set_instances(&mut object, &mut print.base, two.clone()),
            ApplyStatus::Invalidated
        );
        assert_eq!(print_done(&print), vec![true, false, false, true]);
        assert_eq!(object.instances[1].shift, Point::new(15, 25));
        // The same instances again change nothing.
        assert_eq!(
            set_instances(&mut object, &mut print.base, two.clone()),
            ApplyStatus::Unchanged
        );
        // A moved instance with those steps already invalid only changes.
        let moved = vec![two[0], PrintInstance::new(1, Point::new(6, 5))];
        assert_eq!(
            set_instances(&mut object, &mut print.base, moved),
            ApplyStatus::Changed
        );
        assert!(print_done(&print)[PrintStep::WipeTower.index()]);
        // A new count reaches the wipe tower.
        assert_eq!(
            set_instances(&mut object, &mut print.base, vec![two[0]]),
            ApplyStatus::Invalidated
        );
        assert!(!print_done(&print)[PrintStep::WipeTower.index()]);
    }

    #[test]
    fn layers_are_found_by_print_z_and_bottom_z() {
        let mut object = PrintObject::with_members(0, Transform3d::identity());
        for (i, z) in [0.2, 0.4, 0.6].into_iter().enumerate() {
            let id = i32::try_from(i).unwrap();
            add_layer(&mut object, id, 0.2, z, z - 0.1);
        }
        assert_eq!(object.layers.len(), 3);
        assert_eq!(object.layers[2].id, 2);
        assert!(get_layer_at_printz(&object, 0.4).is_some_and(|l| l.id == 1));
        assert!(get_layer_at_printz(&object, 0.41).is_none());
        assert!(get_layer_at_printz_epsilon(&object, 0.41, 0.02).is_some_and(|l| l.id == 1));
        assert!(get_layer_at_printz_epsilon(&object, 0.7, 0.02).is_none());
        assert!(get_first_layer_bellow_printz(&object, 0.4, 0.001).is_some_and(|l| l.id == 1));
        assert!(get_first_layer_bellow_printz(&object, 0.1, 0.001).is_none());
        assert_eq!(get_layer_idx_get_printz(&object, 0.4, 0.001), 2);
        assert_eq!(get_layer_idx_get_printz(&object, 0.1, 0.), -1);
        assert!(get_layer_at_bottomz(&object, 0.2, 1e-6).is_some_and(|l| l.id == 1));
        assert!(get_layer_at_bottomz(&object, 0.3, 1e-6).is_none());
        if let Some(l) = get_layer_at_printz_mut(&mut object, 0.6) {
            l.slicing_errors = true;
        }
        assert!(object.layers[2].slicing_errors);

        let mut first = Vec::new();
        let mut boxes = Vec::new();
        get_certain_layers(&object, 0.3, 0.6, &mut first, &mut boxes);
        assert_eq!(first.len(), 1);
        assert_eq!(
            first[0].iter().map(|l| l.id).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(boxes.len(), 1);

        // Layers another object owns stay.
        object.shared_object = Some(0);
        clear_layers(&mut object);
        assert_eq!(object.layers.len(), 3);
        object.shared_object = None;
        clear_layers(&mut object);
        assert!(object.layers.is_empty());
    }

    #[test]
    fn support_layers_are_added_found_and_cleared() {
        let mut object = PrintObject::with_members(0, Transform3d::identity());
        add_layer(&mut object, 0, 0.2, 0.2, 0.1);
        object.layers[0].sharp_tails_height.push(1.);
        add_support_layer(&mut object, 0, 0, 0.2, 0.2);
        add_tree_support_layer(&mut object, 1, 0.2, 0.4, 0.3);
        assert_eq!(
            insert_support_layer(&mut object, 1, 7, 0, 0.1, 0.3, 0.25),
            1
        );
        assert_eq!(object.support_layers.len(), 3);
        assert_eq!(object.support_layers[0].layer.slice_z, -1.);
        assert_eq!(
            object.support_layers[2].support_type,
            SupportInnerType::Tree
        );
        assert!(get_support_layer_at_printz(&object, 0.3, 0.01).is_some_and(|l| l.layer.id == 7));
        assert!(get_support_layer_at_printz(&object, 0.5, 0.01).is_none());
        assert!(get_support_layer_at_printz_mut(&mut object, 0.4, 0.01).is_some());
        clear_support_layers(&mut object);
        assert!(object.support_layers.is_empty());
        assert!(object.layers[0].sharp_tails_height.is_empty());
    }

    #[test]
    fn the_tree_support_cache_stops() {
        let mut object = PrintObject::with_members(0, Transform3d::identity());
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            alloc_tree_support_preview_cache(&mut object);
        }));
        let payload = caught.expect_err("it stops");
        let stop = payload.downcast::<NotPorted>().expect("a NotPorted stop");
        assert_eq!(
            stop.function,
            "PrintObject::alloc_tree_support_preview_cache"
        );
        assert!(stop.condition.starts_with("tree-supports: "));
    }

    #[test]
    fn regions_and_instances() {
        let print = print_with_cube();
        let mut object = PrintObject::with_members(0, Transform3d::identity());
        assert!(all_regions(&object).is_empty());
        object.instances = vec![PrintInstance::new(0, Point::new(3_000_000, 4_000_000))];
        // The plate's origin is 0 here.
        assert_eq!(
            get_instances_shift_without_plate_offset(&object, &print),
            vec![Point::new(3_000_000, 4_000_000)]
        );
        let mo = &print.base.base.model.objects[0];
        let bb = object.instances[0].get_bounding_box(mo);
        assert!(orca_crt::fabs(bb.min.x() - 50.) < 1e-6 && orca_crt::fabs(bb.max.z() - 10.) < 1e-6);
        destructor_record(std::slice::from_ref(&object), 0);
    }
}
