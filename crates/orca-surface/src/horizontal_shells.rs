// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/PrintObject.cpp lines 3869-4288 of OrcaSlicer v2.4.2
// (PrintObject::clip_fill_surfaces, discover_horizontal_shells and combine_infill), translated on
// 2026-11-14 by orca-o3-surfaces-2. Copyright the OrcaSlicer authors, the BambuStudio authors
// (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU Affero General Public
// License, version 3 (AGPL-3.0; LICENSE).

//! Three serial steps of `PrintObject::prepare_infill` over the fill surfaces:
//!
//! - [`clip_fill_surfaces`]: with `infill_only_where_needed`, the sparse infill is kept only
//!   under what needs support (solid surfaces and perimeters a full loop thick, closed by 2 mm and
//!   smoothed outward), top-down, the rest made internal void. The engine's
//!   `PrintObject::infill_only_where_needed` is a `false` constant, so the step returns at once;
//!   its body is translated all the same.
//! - [`discover_horizontal_shells`]: marks the internal infill of the layers the
//!   `extra_solid_infills` pattern names solid (`crate::utils::check_layer_id_pattern`), then, for
//!   each region without `ensure_vertical_shell_thickness` = all, scatters each layer's top and
//!   bottom (bridge) slices and fill surfaces down or up through `top_shell_layers` or
//!   `top_shell_thickness` (`bottom_` for bottoms): where they cover a neighbour's internal or
//!   internal solid infill, that becomes internal solid. With no sparse infill, or `evstNone` or
//!   `evstCriticalOnly`, the scattering stops at the first layer it reaches nothing on, and parts
//!   narrower than an external perimeter width (times 1, 0.5 or 0.2) are dropped and the shape
//!   carried on shrinks with them; parts narrower than one (`evstNone`) or three solid infill
//!   widths are grown into the neighbour's internal, non-bridge infill.
//! - [`combine_infill`]: with `infill_combination`, groups the layers above the first print layer
//!   into runs no thicker than the smaller of the two infill nozzles (or
//!   `infill_combination_max_layer_height`), and in each run prints the infill (solid at 100 %
//!   density) common to all its layers once on the top layer, as thick as the run, larger than
//!   the solid infill spacing squared; the layers below keep internal void there, and the common
//!   area grown by the clearance is cut from every layer's infill.
//!
//! Signatures: each member is a free function `(print, object_idx)` that takes the object's
//! layers out for its length ([`crate::layers_out::LayersOut`]); a flow the C++ throws on
//! (`FlowError`) is returned. `LayerRegion::infill_area_threshold` is
//! [`crate::layer_region::LayerRegionFill`]'s.
//!
//! In `discover_horizontal_shells`, the C++ `goto EXTERNAL` leaves the scattering of one surface
//! type, which is a `break` of the neighbour loop here; the loop's step runs before its body,
//! which no step reads. `SurfaceCollection backup = std::move(fill_surfaces)` leaves the fill
//! surfaces empty (MSVC's moved-from vector) before they are set anew. The trace message is
//! kept; the SVG exports of `SLIC3R_DEBUG_SLICE_PROCESSING` are not built and are left out.
//!
//! `NotPorted`: in `combine_infill`, a first layer whose id is not 0 (a raft below the object,
//! which domain A has none of) that is as thick as the combined height (the C++ writes
//! `combine[-1]`), and an object with no layers (it writes `combine[size() - 1]`); an empty
//! `nozzle_diameter` (`get_at` reads its front).

use orca_config::print_config::{EnsureVerticalShellThickness, InfillPattern};
use orca_geom::clipper::{JoinType, PolyFillType};
use orca_geom::clipper_utils::{
    ApplySafetyOffset, CLIPPER_SAFETY_OFFSET, DEFAULT_JOIN_TYPE, DEFAULT_MITER_LIMIT, closing,
    diff, diff_ex, diff_ex_expolygons_polygons, diff_expolygons_polygons, expand, intersection,
    intersection_ex, intersection_ex_expolygons, offset_expolygon, opening, union_ex,
};
use orca_geom::expolygon::{self, ExPolygon, ExPolygons};
use orca_geom::point::{EPSILON, SCALING_FACTOR, scaled};
use orca_geom::polygon::Polygons;
use orca_index::stop::not_ported;
use orca_layer::flow::{FlowError, FlowRole};
use orca_layer::layer::Layer;
use orca_layer::print::Print;
use orca_layer::print_object::PrintObject;
use orca_layer::surface::{Surface, SurfaceType, SurfacesPtr, to_expolygons};
use orca_layer::surface_collection::SurfaceCollection;
use orca_mesh::log::{Severity, log};

use crate::layer_region::LayerRegionFill;
use crate::layers_out::{LayersOut, nozzle_diameter_of_filament};
use crate::mutable_polygon::smooth_outward_polygons;
use crate::utils::check_layer_id_pattern;

/// The C++ name `NotPorted` gives for `combine_infill`.
const COMBINE_INFILL: &str = "PrintObject::combine_infill";

/// `PrintObject::clip_fill_surfaces()` of `print.objects[object_idx]`: only active with
/// `infill_only_where_needed`, which the engine fixes off, so it returns at once. The body
/// trims the sparse infill top-down to what the layer above needs supported, turning the rest
/// into internal void, so the fill surfaces' boundaries stay as they were.
///
/// # Errors
/// A flow the C++ throws on.
///
/// # Panics
/// When the print has no object `object_idx`.
pub fn clip_fill_surfaces(print: &mut Print, object_idx: usize) -> Result<(), FlowError> {
    if !PrintObject::INFILL_ONLY_WHERE_NEEDED {
        return Ok(());
    }
    let mut out = LayersOut::new(print, object_idx);
    let (layers, object, print) = out.parts();
    clip_fill_surfaces_in_layers(layers, object, print)
}

/// The body of `clip_fill_surfaces` past its `infill_only_where_needed` check.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss
)]
fn clip_fill_surfaces_in_layers(
    layers: &mut [Layer],
    object: &PrintObject,
    print: &Print,
) -> Result<(), FlowError> {
    let mut has_infill = false;
    for i in 0..object.num_printing_regions() {
        if object
            .printing_region(i)
            .config()
            .sparse_infill_density
            .value
            > 0.
        {
            has_infill = true;
            break;
        }
    }
    if !has_infill {
        return Ok(());
    }

    // We only want infill under ceilings; this is almost like an
    // internal support material.
    // Proceed top-down, skipping the bottom layer.
    let mut upper_internal = Polygons::new();
    let mut layer_id = (layers.len() as i32).wrapping_sub(1);
    while layer_id > 0 {
        let lid = layer_id as usize;
        // Detect things that we need to support.
        // Cummulative fill surfaces.
        let mut fill_surfaces = Polygons::new();
        // Solid surfaces to be supported.
        let mut overhangs = Polygons::new();
        for layerm in &layers[lid].regions {
            for surface in &layerm.fill_surfaces.surfaces {
                let polygons = expolygon::to_polygons(&surface.expolygon);
                if surface.is_solid() {
                    overhangs.extend(polygons.iter().cloned());
                }
                fill_surfaces.extend(polygons);
            }
        }
        let mut lower_layer_fill_surfaces = Polygons::new();
        let mut lower_layer_internal_surfaces = Polygons::new();
        for layerm in &layers[lid - 1].regions {
            for surface in &layerm.fill_surfaces.surfaces {
                let polygons = expolygon::to_polygons(&surface.expolygon);
                if surface.surface_type == SurfaceType::Internal
                    || surface.surface_type == SurfaceType::InternalVoid
                {
                    lower_layer_internal_surfaces.extend(polygons.iter().cloned());
                }
                lower_layer_fill_surfaces.extend(polygons);
            }
        }
        // We also need to support perimeters when there's at least one full unsupported loop
        {
            // Get perimeters area as the difference between slices and fill_surfaces
            // Only consider the area that is not supported by lower perimeters
            let perimeters = intersection(
                &diff_expolygons_polygons(
                    &layers[lid].lslices,
                    &fill_surfaces,
                    ApplySafetyOffset::No,
                ),
                &lower_layer_fill_surfaces,
                ApplySafetyOffset::No,
            );
            // Only consider perimeter areas that are at least one extrusion width thick.
            //FIXME Offset2 eats out from both sides, while the perimeters are create outside in.
            //Should the pw not be half of the current value?
            let mut pw = f32::MAX;
            for layerm in &layers[lid].regions {
                pw = orca_stl::min(
                    pw,
                    layerm
                        .flow(&layers[lid], object, print, FlowRole::Perimeter)?
                        .scaled_width() as f32,
                );
            }
            // Append such thick perimeters to the areas that need support
            overhangs.extend(opening(
                &perimeters,
                pw,
                pw,
                DEFAULT_JOIN_TYPE,
                DEFAULT_MITER_LIMIT,
            ));
        }
        // Merge the new overhangs, find new internal infill.
        upper_internal.extend(overhangs);
        // scaled<float>(2.f): a float division by float(SCALING_FACTOR).
        let closing_radius = 2.0f32 / SCALING_FACTOR as f32;
        upper_internal = intersection(
            // Regularize the overhang regions, so that the infill areas will not become
            // excessively jagged.
            &smooth_outward_polygons(
                closing(
                    &upper_internal,
                    closing_radius,
                    closing_radius,
                    JoinType::Square,
                    0.,
                ),
                scaled(0.1),
            ),
            &lower_layer_internal_surfaces,
            ApplySafetyOffset::No,
        );
        // Apply new internal infill to regions.
        for layerm in &mut layers[lid - 1].regions {
            if layerm.region(object).config().sparse_infill_density.value == 0. {
                continue;
            }
            let mut internal = Polygons::new();
            for surface in &layerm.fill_surfaces.surfaces {
                if surface.surface_type == SurfaceType::Internal
                    || surface.surface_type == SurfaceType::InternalVoid
                {
                    internal.extend(expolygon::to_polygons(&surface.expolygon));
                }
            }
            layerm
                .fill_surfaces
                .remove_types(&[SurfaceType::Internal, SurfaceType::InternalVoid]);
            layerm.fill_surfaces.append_typed_owned(
                intersection_ex(&internal, &upper_internal, ApplySafetyOffset::Yes),
                SurfaceType::Internal,
            );
            layerm.fill_surfaces.append_typed_owned(
                diff_ex(&internal, &upper_internal, ApplySafetyOffset::Yes),
                SurfaceType::InternalVoid,
            );
            // If there are voids it means that our internal infill is not adjacent to
            // perimeters. In this case it would be nice to add a loop around infill to
            // make it more robust and nicer. TODO.
        }
        print.throw_if_canceled();
        layer_id -= 1;
    }
    Ok(())
}

/// `PrintObject::discover_horizontal_shells()` of `print.objects[object_idx]`.
///
/// # Errors
/// A flow the C++ throws on.
///
/// # Panics
/// When the print has no object `object_idx`.
pub fn discover_horizontal_shells(print: &mut Print, object_idx: usize) -> Result<(), FlowError> {
    log(Severity::Trace, "discover_horizontal_shells()");

    let mut out = LayersOut::new(print, object_idx);
    let (layers, object, print) = out.parts();
    for region_id in 0..object.num_printing_regions() {
        for i in 0..layers.len() {
            print.throw_if_canceled();
            let region_config = layers[i].regions[region_id].region(object).config();

            // The C++ passes the size_t index as int.
            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            let layer_id = i as i32;
            if !region_config.extra_solid_infills.value.is_empty()
                && check_layer_id_pattern(
                    region_config.extra_solid_infills.value.as_bytes(),
                    layer_id,
                )
            {
                // Insert a solid internal layer. Mark stInternal surfaces as stInternalSolid.
                for surface in &mut layers[i].regions[region_id].fill_surfaces.surfaces {
                    if surface.surface_type == SurfaceType::Internal {
                        surface.surface_type = SurfaceType::InternalSolid;
                    }
                }
            }

            // If ensure_vertical_shell_thickness, then the rest has already been performed by
            // discover_vertical_shells().
            if region_config.ensure_vertical_shell_thickness.value
                == EnsureVerticalShellThickness::evstAll
            {
                continue;
            }

            let print_z = layers[i].print_z;
            let bottom_z = layers[i].bottom_z();
            for idx_surface_type in 0..3 {
                print.throw_if_canceled();
                let surface_type = match idx_surface_type {
                    0 => SurfaceType::Top,
                    1 => SurfaceType::Bottom,
                    _ => SurfaceType::BottomBridge,
                };
                scatter_surface_type(
                    layers,
                    object,
                    print,
                    region_id,
                    i,
                    surface_type,
                    print_z,
                    bottom_z,
                )?;
            } // foreach type (stTop, stBottom, stBottomBridge)
        } // for each layer
    } // for each region
    Ok(())
}

/// One surface type of layer `i` in `discover_horizontal_shells`: its slices and fill surfaces of
/// that type scattered to the layers below (a top) or above (a bottom).
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn scatter_surface_type(
    layers: &mut [Layer],
    object: &PrintObject,
    print: &Print,
    region_id: usize,
    i: usize,
    surface_type: SurfaceType,
    print_z: f64,
    bottom_z: f64,
) -> Result<(), FlowError> {
    let region_config = layers[i].regions[region_id].region(object).config();
    let num_solid_layers = if surface_type == SurfaceType::Top {
        region_config.top_shell_layers.value
    } else {
        region_config.bottom_shell_layers.value
    };
    if num_solid_layers == 0 {
        return Ok(());
    }
    // Find slices of current type for current layer.
    // Use slices instead of fill_surfaces, because they also include the perimeter area,
    // which needs to be propagated in shells; we need to grow slices like we did for
    // fill_surfaces though. Using both ungrown slices and grown fill_surfaces will
    // not work in some situations, as there won't be any grown region in the perimeter
    // area (this was seen in a model where the top layer had one extra perimeter, thus
    // its fill_surfaces were thinner than the lower layer's infill), however it's the best
    // solution so far. Growing the external slices by EXTERNAL_INFILL_MARGIN will put
    // too much solid infill inside nearly-vertical slopes.

    // Surfaces including the area of perimeters. Everything, that is visible from the top /
    // bottom (not covered by a layer above / below).
    // This does not contain the areas covered by perimeters!
    let mut solid = Polygons::new();
    {
        let layerm = &layers[i].regions[region_id];
        for surface in &layerm.slices.surfaces {
            if surface.surface_type == surface_type {
                solid.extend(expolygon::to_polygons(&surface.expolygon));
            }
        }
        // Infill areas (slices without the perimeters).
        for surface in &layerm.fill_surfaces.surfaces {
            if surface.surface_type == surface_type {
                solid.extend(expolygon::to_polygons(&surface.expolygon));
            }
        }
    }
    if solid.is_empty() {
        return Ok(());
    }

    // Scatter top / bottom regions to other layers. Scattering process is inherently serial, it
    // is difficult to parallelize without locking.
    let is_top = surface_type == SurfaceType::Top;
    let step: i32 = if is_top { -1 } else { 1 };
    let i_int = i as i32;
    let num_layers_int = layers.len() as i32;
    let mut n = i_int.wrapping_add(step);
    loop {
        let in_range = if is_top {
            n >= 0
                && (i_int.wrapping_sub(n) < num_solid_layers
                    || print_z - layers[n as usize].print_z
                        < region_config.top_shell_thickness.value - EPSILON)
        } else {
            n < num_layers_int
                && (n.wrapping_sub(i_int) < num_solid_layers
                    || layers[n as usize].bottom_z() - bottom_z
                        < region_config.bottom_shell_thickness.value - EPSILON)
        };
        if !in_range {
            break;
        }
        // The loop's step (-- n or ++ n), taken first: the body reads only `neighbor`.
        let neighbor = n as usize;
        n = n.wrapping_add(step);

        // Reference to the lower layer of a TOP surface, or an upper layer of a BOTTOM surface.
        // find intersection between neighbor and current layer's surfaces
        // intersections have contours and holes
        // we update $solid so that we limit the next neighbor layer to the areas that were
        // found on this one - in other words, solid shells on one layer (for a given external
        // surface) are always a subset of the shells found on the previous shell layer
        // this approach allows for DWIM in hollow sloping vases, where we want bottom
        // shells to be generated in the base but not in the walls (where there are many
        // narrow bottom surfaces): reassigning $solid will consider the 'shadow' of the
        // upper perimeter as an obstacle and shell will not be propagated to more upper layers
        //FIXME How does it work for stInternalBRIDGE? This is set for sparse infill. Likely this
        // does not work.
        let mut new_internal_solid: Polygons;
        {
            let mut internal = Polygons::new();
            for surface in &layers[neighbor].regions[region_id].fill_surfaces.surfaces {
                if surface.surface_type == SurfaceType::Internal
                    || surface.surface_type == SurfaceType::InternalSolid
                {
                    internal.extend(expolygon::to_polygons(&surface.expolygon));
                }
            }
            new_internal_solid = intersection(&solid, &internal, ApplySafetyOffset::Yes);
        }
        let evst = region_config.ensure_vertical_shell_thickness.value;
        if new_internal_solid.is_empty() {
            // No internal solid needed on this layer. In order to decide whether to continue
            // searching on the next neighbor (thus enforcing the configured number of solid
            // layers, use different strategies according to configured infill density:

            // Orca: Also use the same strategy if the user has selected to further reduce
            // the amount of solid infill on walls.
            if region_config.sparse_infill_density.value == 0.
                || evst == EnsureVerticalShellThickness::evstCriticalOnly
                || evst == EnsureVerticalShellThickness::evstNone
            {
                // If user expects the object to be void (for example a hollow sloping vase),
                // don't continue the search. In this case, we only generate the external solid
                // shell if the object would otherwise show a hole (gap between perimeters of
                // the two layers), and internal solid shells are a subset of the shells found
                // on each previous layer.
                break; // goto EXTERNAL
            }
            // If we have internal infill, we can generate internal solid shells freely.
            continue;
        }

        let mut factor = 0.0f32;
        if region_config.sparse_infill_density.value == 0. {
            factor = 1.0f32;
        } else if evst == EnsureVerticalShellThickness::evstNone {
            factor = 0.5f32;
        } else if evst == EnsureVerticalShellThickness::evstCriticalOnly {
            factor = 0.2f32;
        }
        if factor > 0.0f32 {
            // if we're printing a hollow object we discard any solid shell thinner
            // than a perimeter width, since it's probably just crossing a sloping wall
            // and it's not wanted in a hollow print even if it would make sense when
            // obeying the solid shell count option strictly (DWIM!)

            // Orca: Also use the same strategy if the user has selected to reduce
            // the amount of solid infill on walls. However reduce the margin to 20% overhang
            // as we want to generate infill on sloped vertical surfaces but still keep a small
            // amount of filtering. This is an arbitrary value to make this option safe
            // by ensuring that top surfaces, especially slanted ones dont go **completely**
            // unsupported especially when using single perimeter top layers.
            let neighbor_layer = &layers[neighbor];
            let margin = neighbor_layer.regions[region_id]
                .flow(neighbor_layer, object, print, FlowRole::ExternalPerimeter)?
                .scaled_width() as f32
                * factor;
            let too_narrow = diff(
                &new_internal_solid,
                &opening(
                    &new_internal_solid,
                    margin,
                    margin + CLIPPER_SAFETY_OFFSET,
                    JoinType::Miter,
                    5.,
                ),
                ApplySafetyOffset::No,
            );
            // Trim the regularized region by the original region.
            if !too_narrow.is_empty() {
                solid = diff(&new_internal_solid, &too_narrow, ApplySafetyOffset::No);
                new_internal_solid.clone_from(&solid);
            }
        }

        // make sure the new internal solid is wide enough, as it might get collapsed
        // when spacing is added in Fill.pm
        {
            //FIXME Vojtech: Disable this and you will be sorry.
            // require at least this size
            let layer = &layers[i];
            let margin = (if evst == EnsureVerticalShellThickness::evstNone {
                1.0f32
            } else {
                3.0f32
            }) * layer.regions[region_id]
                .flow(layer, object, print, FlowRole::SolidInfill)?
                .scaled_width() as f32;
            // we use a higher miterLimit here to handle areas with acute angles
            // in those cases, the default miterLimit would cut the corner and we'd
            // get a triangle in $too_narrow; if we grow it below then the shell
            // would have a different shape from the external surface and we'd still
            // have the same angle, so the next shell would be grown even more and so on.
            let too_narrow = diff(
                &new_internal_solid,
                &opening(
                    &new_internal_solid,
                    margin,
                    margin + CLIPPER_SAFETY_OFFSET,
                    JoinType::Miter,
                    5.,
                ),
                ApplySafetyOffset::No,
            );
            if !too_narrow.is_empty() {
                // grow the collapsing parts and add the extra area to  the neighbor layer
                // as well as to our original surfaces so that we support this
                // additional area in the next shell too
                // make sure our grown surfaces don't exceed the fill area
                let mut internal = Polygons::new();
                for surface in &layers[neighbor].regions[region_id].fill_surfaces.surfaces {
                    if surface.is_internal() && !surface.is_bridge() {
                        internal.extend(expolygon::to_polygons(&surface.expolygon));
                    }
                }
                new_internal_solid.extend(intersection(
                    &expand(&too_narrow, margin, DEFAULT_JOIN_TYPE, DEFAULT_MITER_LIMIT),
                    // Discard bridges as they are grown for anchoring and we can't
                    // remove such anchors. (This may happen when a bridge is being
                    // anchored onto a wall where little space remains after the bridge
                    // is grown, and that little space is an internal solid shell so
                    // it triggers this too_narrow logic.)
                    &internal,
                    ApplySafetyOffset::No,
                ));
                // solid = new_internal_solid;
            }
        }

        // internal-solid are the union of the existing internal-solid surfaces
        // and new ones
        let neighbor_layerm = &mut layers[neighbor].regions[region_id];
        let mut backup: SurfaceCollection = std::mem::take(&mut neighbor_layerm.fill_surfaces);
        new_internal_solid.extend(orca_layer::surface::to_polygons_surfaces(
            backup.filter_by_type(SurfaceType::InternalSolid),
        ));
        let internal_solid = union_ex(&new_internal_solid, PolyFillType::NonZero);
        // assign new internal-solid surfaces to layer
        neighbor_layerm
            .fill_surfaces
            .set_typed(&internal_solid, SurfaceType::InternalSolid);
        // subtract intersections from layer surfaces to get resulting internal surfaces
        let mut polygons_internal = expolygon::to_polygons_ex(&internal_solid);
        let internal: ExPolygons = diff_ex_expolygons_polygons(
            &to_expolygons(backup.filter_by_type(SurfaceType::Internal)),
            &polygons_internal,
            ApplySafetyOffset::Yes,
        );
        // assign resulting internal surfaces to layer
        neighbor_layerm
            .fill_surfaces
            .append_typed(&internal, SurfaceType::Internal);
        polygons_internal.extend(expolygon::to_polygons_ex(&internal));
        // assign top and bottom surfaces to layer
        backup.keep_types(&[
            SurfaceType::Top,
            SurfaceType::Bottom,
            SurfaceType::BottomBridge,
        ]);
        let mut top_bottom_groups: Vec<SurfacesPtr<'_>> = Vec::new();
        backup.group(&mut top_bottom_groups);
        for group in &top_bottom_groups {
            neighbor_layerm.fill_surfaces.append_templ_owned(
                diff_ex_expolygons_polygons(
                    &to_expolygons(group.iter().copied()),
                    &polygons_internal,
                    ApplySafetyOffset::No,
                ),
                // Use an existing surface as a template, it carries the bridge angle etc.
                group[0],
            );
        }
    }
    // EXTERNAL:
    Ok(())
}

/// `PrintObject::combine_infill()` of `print.objects[object_idx]`: combines fill surfaces across
/// layers to honor the "infill every N layers" option. Idempotence of this method is guaranteed
/// by the fact that we don't remove things from fill_surfaces but we only turn them into VOID
/// surfaces, thus preserving the boundaries.
///
/// # Errors
/// A flow the C++ throws on.
///
/// # Panics
/// With `NotPorted` as the module header says; when the print has no object `object_idx`.
#[allow(clippy::too_many_lines)]
pub fn combine_infill(print: &mut Print, object_idx: usize) -> Result<(), FlowError> {
    let mut out = LayersOut::new(print, object_idx);
    let (layers, object, print) = out.parts();
    // Work on each region separately.
    for region_id in 0..object.num_printing_regions() {
        let region = object.printing_region(region_id);
        //BBS
        let enable_combine_infill = region.config().infill_combination.value;
        if !enable_combine_infill || region.config().sparse_infill_density.value == 0. {
            continue;
        }

        // Support internal solid infill when sparse_infill_density is 100%
        let use_solid_infill =
            orca_crt::fabs(region.config().sparse_infill_density.value - 100.) < EPSILON;
        let surface_type = if use_solid_infill {
            SurfaceType::InternalSolid
        } else {
            SurfaceType::Internal
        };
        let infill_pattern = if use_solid_infill {
            region.config().internal_solid_infill_pattern.value
        } else {
            region.config().sparse_infill_pattern.value
        };

        // Limit the number of combined layers to the maximum height allowed by this regions'
        // nozzle.
        //FIXME limit the layer height to max_layer_height
        let mut nozzle_diameter = orca_stl::min(
            nozzle_diameter_of_filament(
                print,
                region.config().sparse_infill_filament_id.value,
                COMBINE_INFILL,
            ),
            nozzle_diameter_of_filament(
                print,
                region.config().internal_solid_filament_id.value,
                COMBINE_INFILL,
            ),
        );

        //Orca: Limit combination of infill to up to infill_combination_max_layer_height
        let infill_combination_max_layer_height = region
            .config()
            .infill_combination_max_layer_height
            .get_abs_value(nozzle_diameter);
        nozzle_diameter = if infill_combination_max_layer_height > 0. {
            orca_stl::min(infill_combination_max_layer_height, nozzle_diameter)
        } else {
            nozzle_diameter
        };

        // define the combinations
        let mut combine: Vec<usize> = vec![0; layers.len()];
        {
            let mut current_height = 0.0f64;
            let mut num_layers = 0usize;
            for layer_idx in 0..layers.len() {
                print.throw_if_canceled();
                let layer = &layers[layer_idx];
                if layer.id() == 0 {
                    // Skip first print layer (which may not be first layer in array because of
                    // raft).
                    continue;
                }
                // Check whether the combination of this layer with the lower layers' buffer
                // would exceed max layer height or max combined layer count.
                // BBS: automatically calculate how many layers should be combined
                if current_height + layer.height >= nozzle_diameter + EPSILON {
                    if layer_idx == 0 {
                        not_ported(
                            COMBINE_INFILL,
                            "raft: the object's first layer is not print layer 0 and is as thick as the combined height (combine[-1] is written)",
                        );
                    }
                    // Append combination to lower layer.
                    combine[layer_idx - 1] = num_layers;
                    current_height = 0.;
                    num_layers = 0;
                }
                current_height += layer.height;
                num_layers += 1;
            }

            // Append lower layers (if any) to uppermost layer.
            if layers.is_empty() {
                not_ported(
                    COMBINE_INFILL,
                    "an object with no layers (combine[m_layers.size() - 1] is written)",
                );
            }
            combine[layers.len() - 1] = num_layers;
        }

        // loop through layers to which we have assigned layers to combine
        for (layer_idx, &num_layers) in combine.iter().enumerate() {
            print.throw_if_canceled();
            if num_layers <= 1 {
                continue;
            }
            combine_layers(
                layers,
                object,
                print,
                region_id,
                layer_idx + 1 - num_layers..=layer_idx,
                surface_type,
                infill_pattern,
            )?;
        }
    }
    Ok(())
}

/// One run of `combine_infill`: the layers `run` (bottom to top) of region `region_id`.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn combine_layers(
    layers: &mut [Layer],
    object: &PrintObject,
    print: &Print,
    region_id: usize,
    run: std::ops::RangeInclusive<usize>,
    surface_type: SurfaceType,
    infill_pattern: InfillPattern,
) -> Result<(), FlowError> {
    let (first, last) = (*run.start(), *run.end());
    // Get all the LayerRegion objects to be combined: layers[first..=last], region region_id.
    // We need to perform a multi-layer intersection, so let's split it in pairs.
    // Initialize the intersection with the candidates of the lowest layer.
    let mut intersection: ExPolygons = to_expolygons(
        layers[first].regions[region_id]
            .fill_surfaces
            .filter_by_type(surface_type),
    );
    // Start looping from the second layer and intersect the current intersection with it.
    for layer in &layers[first + 1..=last] {
        intersection = intersection_ex_expolygons(
            &to_expolygons(
                layer.regions[region_id]
                    .fill_surfaces
                    .filter_by_type(surface_type),
            ),
            &intersection,
            ApplySafetyOffset::No,
        );
    }
    let area_threshold =
        layers[first].regions[region_id].infill_area_threshold(&layers[first], object, print)?;
    if !intersection.is_empty() && area_threshold > 0. {
        intersection.retain(|expoly: &ExPolygon| expoly.area() > area_threshold);
    }
    if intersection.is_empty() {
        return Ok(());
    }
    // intersection now contains the regions that can be combined across the full amount of
    // layers, so let's remove those areas from all layers.
    let mut intersection_with_clearance = Polygons::with_capacity(intersection.len());
    let top_layer = &layers[last];
    let top_layerm = &top_layer.regions[region_id];
    let perimeter_width = top_layerm
        .flow(top_layer, object, print, FlowRole::Perimeter)?
        .scaled_width();
    // Because fill areas for rectilinear and honeycomb are grown
    // later to overlap perimeters, we need to counteract that too.
    let grown_factor = if infill_pattern == InfillPattern::ipRectilinear
        || infill_pattern == InfillPattern::ipMonotonic
        || infill_pattern == InfillPattern::ipGrid
        || infill_pattern == InfillPattern::ipLateralLattice
        || infill_pattern == InfillPattern::ipLine
        || infill_pattern == InfillPattern::ipHoneycomb
        || infill_pattern == InfillPattern::ipLateralHoneycomb
    {
        1.5f32
    } else {
        0.5f32
    };
    let solid_infill_width = top_layerm
        .flow(top_layer, object, print, FlowRole::SolidInfill)?
        .scaled_width();
    let clearance_offset =
        0.5f32 * perimeter_width as f32 + grown_factor * solid_infill_width as f32;
    for expoly in &intersection {
        intersection_with_clearance.extend(offset_expolygon(
            expoly,
            clearance_offset,
            DEFAULT_JOIN_TYPE,
            DEFAULT_MITER_LIMIT,
        ));
    }
    // The run's thickness: the sum of its layers' heights from 0, bottom to top.
    let mut thickness = 0.0f64;
    for layer in &layers[first..=last] {
        thickness += layer.height;
    }
    let num_layerms = last + 1 - first;
    for (k, layer) in layers[first..=last].iter_mut().enumerate() {
        let idx = first + k;
        let layerm = &mut layer.regions[region_id];
        let internal = orca_layer::surface::to_polygons_surfaces(
            layerm.fill_surfaces.filter_by_type(surface_type),
        );
        layerm.fill_surfaces.remove_type(surface_type);
        layerm.fill_surfaces.append_typed_owned(
            diff_ex(
                &internal,
                &intersection_with_clearance,
                ApplySafetyOffset::No,
            ),
            surface_type,
        );
        if idx == last {
            // Apply surfaces back with adjusted depth to the uppermost layer.
            let mut templ = Surface::with_expolygon(surface_type, ExPolygon::default());
            templ.thickness = thickness;
            templ.thickness_layers = num_layerms as u16;
            layerm.fill_surfaces.append_templ(&intersection, &templ);
        } else {
            // Save void surfaces.
            layerm.fill_surfaces.append_typed_owned(
                intersection_ex(
                    &internal,
                    &intersection_with_clearance,
                    ApplySafetyOffset::No,
                ),
                SurfaceType::InternalVoid,
            );
        }
    }
    Ok(())
}
