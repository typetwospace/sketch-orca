// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/PrintObject.cpp lines 2007-2448 of OrcaSlicer v2.4.2
// (PrintObject::discover_vertical_shells), translated on 2026-11-14 by orca-o3-surfaces-2.
// Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and the PrusaSlicer
// authors (Prusa Research), under the GNU Affero General Public License, version 3 (AGPL-3.0;
// LICENSE).

//! `PrintObject::discover_vertical_shells()`: for each region with
//! `ensure_vertical_shell_thickness` = all, makes solid the internal infill of each layer that the
//! top and bottom surfaces of the layers within the shell thickness above and below project onto,
//! so that sloping walls keep their full shell thickness.
//!
//! - A cache holds, for each layer, its top and bottom surfaces grown by 5 % of the solid infill
//!   spacing, and its holes (the fill areas). For a print of several regions without
//!   `interface_shells` it is made once over all the regions, the holes then also simulating one
//!   set of perimeters over the merged regions (the lslices grown by 0.3 of the narrowest
//!   perimeter spacing and shrunk by the widest perimeter offset); otherwise it is made for each
//!   region, the holes collected once. A print of several regions, none with the setting, stops
//!   there.
//! - For each layer, the shell is the union of the top surfaces of the layers above within
//!   `top_shell_layers` or `top_shell_thickness` and of the bottom surfaces of those below, the
//!   holes their intersection; where no layer projects, the layer's own tops or bottoms grown by
//!   an external perimeter spacing and clipped by the next layer's lslices anchor it. The shell is
//!   clipped by the internal, internal void and internal solid surfaces, the internal surfaces
//!   outside the holes added; with the internal solids it is opened, closed and grown a little by
//!   square joins (0.325, 0.6 and 0.2 times 1.05 solid infill spacings), and the islands that are
//!   small (or not much larger and inside the object) and cover no internal polygon whole are
//!   dropped. What is left is the new internal solid; the internal and internal void surfaces
//!   lose it.
//!
//! Signatures: the member is the free function [`discover_vertical_shells`]`(print,
//! object_idx)`, which takes the object's layers out for its length
//! ([`crate::layers_out::LayersOut`]). A flow the C++ throws on (`FlowError`) is returned.
//!
//! The parallel loops (`crate::layers_out::map_range`, applied in index order): the cache loops
//! read a layer's slices, fill areas and lslices and write only that layer's cache entry; the
//! thickness loop reads the cache, the layers' lslices and heights, and its own layer's fill
//! surfaces, and writes only its own layer's fill surfaces, which nothing else in the loop reads.
//!
//! The info message `"Discovering vertical shells..."` leaves out `log_memory_info()`
//! (HAZARDS.md). `PROFILE_FUNC()` is empty in the engine. The SVG exports of
//! `SLIC3R_DEBUG_SLICE_PROCESSING`, the `#if 0` blocks and the branches of
//! `one_more_layer_below_top_bottom_surfaces` (a `false` constant) are not built and are left out.
//! Nothing here reads a setting domain A leaves out, so nothing stops with `NotPorted`.

use orca_config::print_config::EnsureVerticalShellThickness;
use orca_geom::clipper::{JoinType, PolyFillType};
use orca_geom::clipper_utils::{
    ApplySafetyOffset, DEFAULT_JOIN_TYPE, DEFAULT_MITER_LIMIT, closing, diff, diff_ex_expolygons,
    expand, intersection, intersection_ex_polygons_expolygons, offset_expolygons, offset2,
    offset2_ex, shrink_ex_expolygons, union_, union_ex,
};
use orca_geom::expolygon::{ExPolygon, ExPolygons, to_polygons, to_polygons_ex};
use orca_geom::point::{EPSILON, SCALED_EPSILON, scaled};
use orca_geom::polygon::Polygons;
use orca_layer::flow::{FlowError, FlowRole};
use orca_layer::layer::Layer;
use orca_layer::print::Print;
use orca_layer::print_object::PrintObject;
use orca_layer::surface::{SurfaceType, to_expolygons, to_polygons_surfaces};
use orca_mesh::log::{Severity, log};

use crate::layers_out::{LayersOut, map_range};

/// `DiscoverVerticalShellsCacheEntry`: a layer's collected polygons, offsetted.
#[derive(Clone, Debug, Default)]
struct CacheEntry {
    top_surfaces: Polygons,
    bottom_surfaces: Polygons,
    holes: Polygons,
}

/// Just a tiny fraction of an infill extrusion width to merge neighbor regions reliably.
const TOP_BOTTOM_EXPANSION_COEFF: f32 = 0.05;

/// The bottom surface types the cache collects.
const SURFACES_BOTTOM: [SurfaceType; 2] = [SurfaceType::Bottom, SurfaceType::BottomBridge];

/// What the thickness loop gives a layer: its new internal, internal void and internal solid
/// surfaces, or `None` where it leaves the layer as it is (`continue`).
type NewInternal = Option<(ExPolygons, ExPolygons, ExPolygons)>;

/// `PrintObject::discover_vertical_shells()` of `print.objects[object_idx]`.
///
/// # Errors
/// A flow the C++ throws on, from the first leaf of a loop to give one.
///
/// # Panics
/// When the print has no object `object_idx`, or in spiral vase mode when the object has no
/// region (the C++ reads past its regions).
pub fn discover_vertical_shells(print: &mut Print, object_idx: usize) -> Result<(), FlowError> {
    log(Severity::Info, "Discovering vertical shells...");
    let mut out = LayersOut::new(print, object_idx);
    let (layers, object, print) = out.parts();
    vertical_shells_in_layers(layers, object, print)
}

/// The body of `discover_vertical_shells` over the layers taken out of `object`.
fn vertical_shells_in_layers(
    layers: &mut [Layer],
    object: &PrintObject,
    print: &Print,
) -> Result<(), FlowError> {
    let spiral_mode = print.config().spiral_mode.value;
    let num_layers = if spiral_mode {
        #[allow(clippy::cast_sign_loss)]
        let bottom_shell_layers =
            object.printing_region(0).config().bottom_shell_layers.value as usize;
        orca_stl::min(bottom_shell_layers, layers.len())
    } else {
        layers.len()
    };
    let mut cache_top_botom_regions: Vec<CacheEntry> = vec![CacheEntry::default(); num_layers];
    let top_bottom_surfaces_all_regions =
        object.num_printing_regions() > 1 && !object.config().interface_shells.value;
    if top_bottom_surfaces_all_regions {
        // This is a multi-material print and interface_shells are disabled, meaning that the
        // vertical shell thickness is calculated over all materials.
        // Is the "ensure vertical wall thickness" applicable to any region?
        let mut has_extra_layers = false;
        for region_id in 0..object.num_printing_regions() {
            let config = object.printing_region(region_id).config();
            if config.ensure_vertical_shell_thickness.value == EnsureVerticalShellThickness::evstAll
            {
                has_extra_layers = true;
                break;
            }
        }
        if !has_extra_layers {
            // The "ensure vertical wall thickness" feature is not applicable to any of the
            // regions. Quit.
            return Ok(());
        }
        log(
            Severity::Debug,
            "Discovering vertical shells in parallel - start : cache top / bottom",
        );
        //FIXME Improve the heuristics for a grain size.
        let grain_size = orca_stl::max(num_layers / 16, 1usize);
        cache_top_botom_regions = {
            let layers: &[Layer] = layers;
            map_range(0, num_layers, grain_size, |idx_layer| {
                cache_over_all_regions(layers, object, print, idx_layer)
            })?
        };
        print.throw_if_canceled();
        log(
            Severity::Debug,
            "Discovering vertical shells in parallel - end : cache top / bottom",
        );
    }

    for region_id in 0..object.num_printing_regions() {
        let region = object.printing_region(region_id);
        if region.config().ensure_vertical_shell_thickness.value
            != EnsureVerticalShellThickness::evstAll
        {
            // This region will be handled by discover_horizontal_shells().
            continue;
        }

        //FIXME Improve the heuristics for a grain size.
        let mut grain_size = orca_stl::max(num_layers / 16, 1usize);

        if !top_bottom_surfaces_all_regions {
            // This is either a single material print, or a multi-material print and
            // interface_shells are enabled, meaning that the vertical shell thickness is
            // calculated over a single material.
            log(
                Severity::Debug,
                &format!(
                    "Discovering vertical shells for region {region_id} in parallel - start : cache top / bottom"
                ),
            );
            let entries = {
                let layers: &[Layer] = layers;
                let cache: &[CacheEntry] = &cache_top_botom_regions;
                map_range(0, num_layers, grain_size, |idx_layer| {
                    cache_of_region(layers, object, print, region_id, idx_layer, cache)
                })?
            };
            for (cache, (top_surfaces, bottom_surfaces, holes)) in
                cache_top_botom_regions.iter_mut().zip(entries)
            {
                cache.top_surfaces = top_surfaces;
                cache.bottom_surfaces = bottom_surfaces;
                if let Some(holes) = holes {
                    cache.holes = holes;
                }
            }
            print.throw_if_canceled();
            log(
                Severity::Debug,
                &format!(
                    "Discovering vertical shells for region {region_id} in parallel - end : cache top / bottom"
                ),
            );
        }

        log(
            Severity::Debug,
            &format!(
                "Discovering vertical shells for region {region_id} in parallel - start : ensure vertical wall thickness"
            ),
        );
        grain_size = 1;
        let new_internals = {
            let layers: &[Layer] = layers;
            let cache: &[CacheEntry] = &cache_top_botom_regions;
            map_range(0, num_layers, grain_size, |idx_layer| {
                ensure_vertical_wall_thickness(layers, object, print, region_id, idx_layer, cache)
            })?
        };
        for (idx_layer, new_internal) in new_internals.into_iter().enumerate() {
            if let Some((new_internal, new_internal_void, new_internal_solid)) = new_internal {
                // Assign resulting internal surfaces to layer.
                let fill_surfaces = &mut layers[idx_layer].regions[region_id].fill_surfaces;
                fill_surfaces.keep_types(&[
                    SurfaceType::Top,
                    SurfaceType::Bottom,
                    SurfaceType::BottomBridge,
                ]);
                fill_surfaces.append_typed_owned(new_internal, SurfaceType::Internal);
                fill_surfaces.append_typed_owned(new_internal_void, SurfaceType::InternalVoid);
                fill_surfaces.append_typed_owned(new_internal_solid, SurfaceType::InternalSolid);
            }
        }
        print.throw_if_canceled();
        log(
            Severity::Debug,
            &format!("Discovering vertical shells for region {region_id} in parallel - end"),
        );
    } // for each region
    Ok(())
}

/// The cache entry of layer `idx_layer` over all the regions (a print of several regions without
/// `interface_shells`).
fn cache_over_all_regions(
    layers: &[Layer],
    object: &PrintObject,
    print: &Print,
    idx_layer: usize,
) -> Result<CacheEntry, FlowError> {
    let num_regions = object.num_printing_regions();
    print.throw_if_canceled();
    let layer = &layers[idx_layer];
    let mut cache = CacheEntry::default();
    // Simulate single set of perimeters over all merged regions.
    let mut perimeter_offset = 0.0f32;
    let mut perimeter_min_spacing = f32::MAX;
    for region_id in 0..num_regions {
        let layerm = &layer.regions[region_id];
        #[allow(clippy::cast_precision_loss)]
        let top_bottom_expansion = layerm
            .flow(layer, object, print, FlowRole::SolidInfill)?
            .scaled_spacing() as f32
            * TOP_BOTTOM_EXPANSION_COEFF;
        // Top surfaces.
        cache.top_surfaces.extend(offset_expolygons(
            &to_expolygons(layerm.slices.filter_by_type(SurfaceType::Top)),
            top_bottom_expansion,
            DEFAULT_JOIN_TYPE,
            DEFAULT_MITER_LIMIT,
        ));
        // Bottom surfaces.
        cache.bottom_surfaces.extend(offset_expolygons(
            &to_expolygons(layerm.slices.filter_by_types(&SURFACES_BOTTOM)),
            top_bottom_expansion,
            DEFAULT_JOIN_TYPE,
            DEFAULT_MITER_LIMIT,
        ));
        // Calculate the maximum perimeter offset as if the slice was extruded with a single
        // extruder only. First find the maxium number of perimeters per region slice.
        let mut perimeters: u32 = 0;
        for s in &layerm.slices.surfaces {
            perimeters = orca_stl::max(perimeters, u32::from(s.extra_perimeters));
        }
        #[allow(clippy::cast_sign_loss)]
        let wall_loops = layerm.region(object).config().wall_loops.value as u32;
        perimeters = perimeters.wrapping_add(wall_loops);
        // Then calculate the infill offset.
        if perimeters > 0 {
            let extflow = layerm.flow(layer, object, print, FlowRole::ExternalPerimeter)?;
            let flow = layerm.flow(layer, object, print, FlowRole::Perimeter)?;
            #[allow(clippy::cast_precision_loss)]
            {
                perimeter_offset = orca_stl::max(
                    perimeter_offset,
                    0.5f32
                        * extflow
                            .scaled_width()
                            .wrapping_add(extflow.scaled_spacing())
                            as f32
                        + (perimeters as f32 - 1.0f32) * flow.scaled_spacing() as f32,
                );
                perimeter_min_spacing = orca_stl::min(
                    perimeter_min_spacing,
                    orca_stl::min(extflow.scaled_spacing(), flow.scaled_spacing()) as f32,
                );
            }
        }
        cache.holes.extend(to_polygons_ex(&layerm.fill_expolygons));
    }
    // Save some computing time by reducing the number of polygons.
    cache.top_surfaces = union_(&cache.top_surfaces);
    cache.bottom_surfaces = union_(&cache.bottom_surfaces);
    // For a multi-material print, simulate perimeter / infill split as if only a single extruder
    // has been used for the whole print.
    if f64::from(perimeter_offset) > 0. {
        // The layer.lslices are forced to merge by expanding them first.
        cache.holes.extend(offset2(
            &layer.lslices,
            0.3f32 * perimeter_min_spacing,
            -perimeter_offset - 0.3f32 * perimeter_min_spacing,
            DEFAULT_JOIN_TYPE,
            DEFAULT_MITER_LIMIT,
        ));
    }
    cache.holes = union_(&cache.holes);
    Ok(cache)
}

/// The cache of layer `idx_layer` for region `region_id` alone: its top and bottom surfaces, and
/// the holes over all regions where the entry has none yet (they are valid for every region).
fn cache_of_region(
    layers: &[Layer],
    object: &PrintObject,
    print: &Print,
    region_id: usize,
    idx_layer: usize,
    cache: &[CacheEntry],
) -> Result<(Polygons, Polygons, Option<Polygons>), FlowError> {
    print.throw_if_canceled();
    let layer = &layers[idx_layer];
    let layerm = &layer.regions[region_id];
    #[allow(clippy::cast_precision_loss)]
    let top_bottom_expansion = layerm
        .flow(layer, object, print, FlowRole::SolidInfill)?
        .scaled_spacing() as f32
        * TOP_BOTTOM_EXPANSION_COEFF;
    // Top surfaces.
    let top_surfaces = offset_expolygons(
        &to_expolygons(layerm.slices.filter_by_type(SurfaceType::Top)),
        top_bottom_expansion,
        DEFAULT_JOIN_TYPE,
        DEFAULT_MITER_LIMIT,
    );
    // Bottom surfaces.
    let bottom_surfaces = offset_expolygons(
        &to_expolygons(layerm.slices.filter_by_types(&SURFACES_BOTTOM)),
        top_bottom_expansion,
        DEFAULT_JOIN_TYPE,
        DEFAULT_MITER_LIMIT,
    );
    // Holes over all regions. Only collect them once, they are valid for all region_id
    // iterations.
    let holes = if cache[idx_layer].holes.is_empty() {
        let mut holes = Polygons::new();
        for layerm in &layer.regions {
            holes.extend(to_polygons_ex(&layerm.fill_expolygons));
        }
        Some(holes)
    } else {
        None
    };
    Ok((top_surfaces, bottom_surfaces, holes))
}

/// `combine_holes`: the holes cleared when either is empty, else their intersection.
fn combine_holes(holes: &mut Polygons, holes2: &Polygons) {
    if holes.is_empty() || holes2.is_empty() {
        holes.clear();
    } else {
        *holes = intersection(holes, holes2, ApplySafetyOffset::No);
    }
}

/// `combine_shells`: a copy of `shells2` into an empty shell, else their union.
fn combine_shells(shell: &mut Polygons, shells2: &Polygons) {
    if shell.is_empty() {
        shell.clone_from(shells2);
    } else if !shells2.is_empty() {
        shell.extend(shells2.iter().cloned());
        // Running the union_ using the Clipper library piece by piece is cheaper than running the
        // union_ all at once.
        *shell = union_(shell);
    }
}

/// One iteration of the thickness loop: layer `idx_layer`'s new internal surfaces.
#[allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn ensure_vertical_wall_thickness(
    layers: &[Layer],
    object: &PrintObject,
    print: &Print,
    region_id: usize,
    idx_layer: usize,
    cache: &[CacheEntry],
) -> Result<NewInternal, FlowError> {
    print.throw_if_canceled();
    let layer = &layers[idx_layer];
    let layerm = &layer.regions[region_id];
    let region_config = layerm.region(object).config();

    let solid_infill_flow = layerm.flow(layer, object, print, FlowRole::SolidInfill)?;
    let infill_line_spacing = solid_infill_flow.scaled_spacing();
    // Find a union of perimeters below / above this surface to guarantee a minimum shell
    // thickness.
    let mut shell = Polygons::new();
    let mut holes = Polygons::new();
    let min_perimeter_infill_spacing = infill_line_spacing as f32 * 1.05f32;
    holes.extend(cache[idx_layer].holes.iter().cloned());
    // The C++ indexes the layers and the cache with int.
    let cache_size = cache.len() as i32;
    let n_top_layers = region_config.top_shell_layers.value;
    if n_top_layers > 0 {
        // Gather top regions projected to this layer.
        let print_z = layer.print_z;
        let mut i = (idx_layer as i32).wrapping_add(1);
        let itop = (idx_layer as i32).wrapping_add(n_top_layers);
        let mut at_least_one_top_projected = false;
        while i < cache_size
            && (i < itop
                || layers[i as usize].print_z - print_z
                    < region_config.top_shell_thickness.value - EPSILON)
        {
            at_least_one_top_projected = true;
            let cache = &cache[i as usize];
            combine_holes(&mut holes, &cache.holes);
            combine_shells(&mut shell, &cache.top_surfaces);
            i += 1;
        }
        if !at_least_one_top_projected && i < cache_size {
            // Lets consider this a special case - with only 1 top solid and minimal shell
            // thickness settings, the boundaries of solid layers are not anchored over/under
            // perimeters, so lets fix it by adding at least one perimeter width of area
            let anchor_area = intersection(
                &expand(
                    &cache[idx_layer].top_surfaces,
                    layerm
                        .flow(layer, object, print, FlowRole::ExternalPerimeter)?
                        .scaled_spacing() as f32,
                    DEFAULT_JOIN_TYPE,
                    DEFAULT_MITER_LIMIT,
                ),
                &to_polygons_ex(&layers[i as usize].lslices),
                ApplySafetyOffset::No,
            );
            combine_shells(&mut shell, &anchor_area);
        }
    }
    let n_bottom_layers = region_config.bottom_shell_layers.value;
    if n_bottom_layers > 0 {
        // Gather bottom regions projected to this layer.
        let bottom_z = layer.bottom_z();
        let mut i = (idx_layer as i32).wrapping_sub(1);
        let ibottom = (idx_layer as i32).wrapping_sub(n_bottom_layers);
        let mut at_least_one_bottom_projected = false;
        while i >= 0
            && (i > ibottom
                || bottom_z - layers[i as usize].bottom_z()
                    < region_config.bottom_shell_thickness.value - EPSILON)
        {
            at_least_one_bottom_projected = true;
            let cache = &cache[i as usize];
            combine_holes(&mut holes, &cache.holes);
            combine_shells(&mut shell, &cache.bottom_surfaces);
            i -= 1;
        }

        if !at_least_one_bottom_projected && i >= 0 {
            let anchor_area = intersection(
                &expand(
                    &cache[idx_layer].bottom_surfaces,
                    layerm
                        .flow(layer, object, print, FlowRole::ExternalPerimeter)?
                        .scaled_spacing() as f32,
                    DEFAULT_JOIN_TYPE,
                    DEFAULT_MITER_LIMIT,
                ),
                &to_polygons_ex(&layers[i as usize].lslices),
                ApplySafetyOffset::No,
            );
            combine_shells(&mut shell, &anchor_area);
        }
    }

    // Trim the shells region by the internal & internal void surfaces.
    let polygons_internal = to_polygons_surfaces(layerm.fill_surfaces.filter_by_types(&[
        SurfaceType::Internal,
        SurfaceType::InternalVoid,
        SurfaceType::InternalSolid,
    ]));
    shell = intersection(&shell, &polygons_internal, ApplySafetyOffset::Yes);
    shell.extend(diff(&polygons_internal, &holes, ApplySafetyOffset::No));
    if shell.is_empty() {
        return Ok(None);
    }

    // Append the internal solids, so they will be merged with the new ones.
    shell.extend(to_polygons_surfaces(
        layerm
            .fill_surfaces
            .filter_by_type(SurfaceType::InternalSolid),
    ));

    // These regions will be filled by a rectilinear full infill. Currently this type of infill
    // only fills regions, which fit at least a single line. To avoid gaps in the sparse infill,
    // make sure that this region does not contain parts narrower than the infill spacing width.
    let mut regularized_shell: ExPolygons;
    {
        // Open to remove (filter out) regions narrower than a bit less than an infill extrusion
        // line width. Such narrow regions are difficult to fill in with a gap fill algorithm (or
        // Arachne), however they are most likely not needed for print stability / quality.
        let narrow_ensure_vertical_wall_thickness_region_radius =
            0.5f32 * 0.65f32 * min_perimeter_infill_spacing;
        // Then close gaps narrower than 1.2 * line width, such gaps are difficult to fill in with
        // sparse infill, thus they will be merged into the solid infill.
        let narrow_sparse_infill_region_radius = 0.5f32 * 1.2f32 * min_perimeter_infill_spacing;
        // Finally expand the infill a bit to remove tiny gaps between solid infill and the other
        // regions.
        let tiny_overlap_radius = 0.2f32 * min_perimeter_infill_spacing;
        regularized_shell = shrink_ex_expolygons(
            &offset2_ex(
                &union_ex(&shell, PolyFillType::NonZero),
                // Open to remove (filter out) regions narrower than an infill extrusion line
                // width.
                -narrow_ensure_vertical_wall_thickness_region_radius,
                // Then close gaps narrower than 1.2 * line width, such gaps are difficult to fill
                // in with sparse infill.
                narrow_ensure_vertical_wall_thickness_region_radius
                    + narrow_sparse_infill_region_radius,
                JoinType::Square,
                DEFAULT_MITER_LIMIT,
            ),
            // Finally expand the infill a bit to remove tiny gaps between solid infill and the
            // other regions.
            narrow_sparse_infill_region_radius - tiny_overlap_radius,
            JoinType::Square,
            DEFAULT_MITER_LIMIT,
        );

        let object_volume: Polygons;
        let internal_volume: Polygons;
        {
            let shrinked_bottom_slice = if idx_layer > 0 {
                to_polygons_ex(&layers[idx_layer - 1].lslices)
            } else {
                Polygons::new()
            };
            let shrinked_upper_slice = if idx_layer + 1 < layers.len() {
                to_polygons_ex(&layers[idx_layer + 1].lslices)
            } else {
                Polygons::new()
            };
            object_volume = intersection(
                &shrinked_bottom_slice,
                &shrinked_upper_slice,
                ApplySafetyOffset::No,
            );
            internal_volume = closing(
                &polygons_internal,
                SCALED_EPSILON as f32,
                SCALED_EPSILON as f32,
                DEFAULT_JOIN_TYPE,
                DEFAULT_MITER_LIMIT,
            );
        }

        // The regularization operation may cause scattered tiny drops on the smooth parts of the
        // model, filter them out. If the region checks both following conditions, it is removed:
        //   1. the area is very small,
        //      OR the area is quite small and it is fully wrapped in model (not visible)
        //      the in-model condition is there due to small sloping surfaces, e.g. top of the
        //      hull of the benchy
        //   2. the area does not fully cover an internal polygon
        //      This is there mainly for a very thin parts, where the solid layers would be
        //      missing if the part area is quite small
        // (The thresholds are float products of a coord_t, compared in double.)
        regularized_shell.retain(|p: &ExPolygon| {
            let remove = (p.area() < f64::from(min_perimeter_infill_spacing * scaled(1.5) as f32)
                || (p.area() < f64::from(min_perimeter_infill_spacing * scaled(8.0) as f32)
                    && diff(&to_polygons(p), &object_volume, ApplySafetyOffset::No).is_empty()))
                && diff(
                    &internal_volume,
                    &expand(
                        &to_polygons(p),
                        min_perimeter_infill_spacing,
                        DEFAULT_JOIN_TYPE,
                        DEFAULT_MITER_LIMIT,
                    ),
                    ApplySafetyOffset::No,
                )
                .len()
                    >= internal_volume.len();
            !remove
        });
    }
    if regularized_shell.is_empty() {
        return Ok(None);
    }

    let new_internal_solid = intersection_ex_polygons_expolygons(
        &polygons_internal,
        &regularized_shell,
        ApplySafetyOffset::No,
    );

    // Trim the internal & internalvoid by the shell.
    let new_internal = diff_ex_expolygons(
        &to_expolygons(layerm.fill_surfaces.filter_by_type(SurfaceType::Internal)),
        &regularized_shell,
        ApplySafetyOffset::No,
    );
    let new_internal_void = diff_ex_expolygons(
        &to_expolygons(
            layerm
                .fill_surfaces
                .filter_by_type(SurfaceType::InternalVoid),
        ),
        &regularized_shell,
        ApplySafetyOffset::No,
    );

    Ok(Some((new_internal, new_internal_void, new_internal_solid)))
}
