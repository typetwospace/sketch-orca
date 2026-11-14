// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/PrintObject.cpp lines 1520-1925 of OrcaSlicer v2.4.2
// (PrintObject::detect_surfaces_type), translated on 2026-11-14 by orca-o3-surfaces-2.
// Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and the PrusaSlicer
// authors (Prusa Research), under the GNU Affero General Public License, version 3 (AGPL-3.0;
// LICENSE).

//! `PrintObject::detect_surfaces_type()`: splits each region's slices of each layer into top,
//! bottom (bridging or not) and internal surfaces, then makes the regions' fill surfaces from
//! them.
//!
//! For each region in turn:
//!
//! - Each layer's slices (with its fill surfaces added for `counterbore_hole_bridging` = filled)
//!   less the layer above (its lslices, or the region's slices with `interface_shells`), opened
//!   by a tenth of the external perimeter width, are its top; less the layer below, its bottom,
//!   `stBottomBridge` unless supports carry it; the first and last layers are all bottom and all
//!   top. Where top and bottom overlap (a thin membrane) the overlap is bottom, save for small
//!   cracks on a layer above the first: a crack that vanishes when shrunk by 1.5 external
//!   perimeter widths, unless a bottom surface twice its area holds it, is taken out of the
//!   bottoms (grown by the same) and stays top. The rest of the slices is internal; the slices
//!   become the internal, then the top, then the bottom surfaces. In spiral vase mode only the
//!   bottom layers are classified, the last of them made top and the layers above internal.
//! - With `enable_extra_bridge_layer` external or all, the internal surfaces of the layer above
//!   each bottom bridge, where they overlap it by more than the walls and away from that layer's
//!   tops, become a second bridge layer (`stInternalAfterExternalBridge`), then, for now,
//!   `stBottomBridge` again in every region.
//! - Each layer's fill surfaces are its slices clipped by its fill area
//!   (`LayerRegion::slices_to_fill_surfaces_clipped`).
//!
//! The object is then marked as having typed slices.
//!
//! Signatures: the member is the free function [`detect_surfaces_type`]`(print, object_idx)`,
//! which takes the object's layers out for its length ([`crate::layers_out::LayersOut`]). A flow
//! the C++ throws on (`FlowError`) is returned.
//!
//! The parallel loops (`crate::layers_out::map_range`, applied in index order): the
//! classification of a layer reads its own region's slices and fill surfaces and the layers
//! above and below (their lslices, which no step here changes, or with `interface_shells` their
//! region's slices, which the C++ itself caches for that reason) and writes only its own slices;
//! the second bridge layer's first loop only reads, and its second writes only layer `i + 1`'s
//! slices from layer `i`'s cached bridges and layer `i + 1`'s own slices. The reclassification
//! back to bottom bridges and the clipping into fill surfaces change each layer in place
//! (`orca_tbb::parallel_for_each_mut`, the same `blocked_range`).
//!
//! The info message `"Detecting solid surfaces..."` leaves out `log_memory_info()`, the process's
//! memory sizes, which no two runs share (HAZARDS.md). The debug messages and the parallel loops
//! are kept; the SVG exports of `SLIC3R_DEBUG_SLICE_PROCESSING` and the `#if 0` branch are not
//! built in the engine and are left out.
//!
//! `NotPorted`: an object with supports (`has_support()`), which each leaf of the classifying
//! loop reads, stops with the supports or tree-supports family (`crate::layers_out`); domain A has
//! no supports.

use orca_config::print_config::{
    CounterboreHoleBridgingOption, EnableExtraBridgeLayer, SupportType,
};
use orca_geom::clipper_utils::{
    ApplySafetyOffset, DEFAULT_JOIN_TYPE, DEFAULT_MITER_LIMIT, diff_ex_expolygon,
    diff_ex_expolygon_expolygons, diff_ex_expolygons, diff_ex_expolygons_polygons,
    diff_ex_polygons_expolygons, intersection_ex, intersection_ex_expolygons,
    intersection_expolygons, offset_ex_expolygon, offset_ex_expolygons, opening_ex,
    shrink_ex_expolygons, union_ex_two,
};
use orca_geom::expolygon::ExPolygons;
use orca_geom::polygon::Polygons;
use orca_layer::flow::{FlowError, FlowRole};
use orca_layer::layer::Layer;
use orca_layer::print::Print;
use orca_layer::print_object::PrintObject;
use orca_layer::surface::{
    Surface, SurfaceType, Surfaces, surfaces_append_owned, surfaces_append_typed_owned,
    to_expolygons, to_polygons, to_polygons_surfaces,
};
use orca_mesh::log::{Severity, log};
use orca_tbb::parallel_for_each_mut;

use crate::layers_out::{
    LayersOut, is_auto, map_range, supports_stop, union_safety_offset_ex_expolygons,
};

/// The C++ name `NotPorted` gives for this step.
const FUNCTION: &str = "PrintObject::detect_surfaces_type";

/// `PrintObject::detect_surfaces_type()` of `print.objects[object_idx]`: the regions' slices
/// classified into top, bottom and internal surfaces, the fill surfaces made from them, and
/// `typed_slices` set.
///
/// # Errors
/// A flow the C++ throws on, from the first leaf of a loop to give one.
///
/// # Panics
/// With `NotPorted` for an object with supports; when the print has no object `object_idx`, or
/// in spiral vase mode when the object has no region (the C++ reads past its regions).
pub fn detect_surfaces_type(print: &mut Print, object_idx: usize) -> Result<(), FlowError> {
    log(Severity::Info, "Detecting solid surfaces...");

    {
        let mut out = LayersOut::new(print, object_idx);
        let (layers, object, print) = out.parts();
        detect_in_layers(layers, object, print)?;
    }

    // Mark the object to have the region slices classified (typed, which also means they are
    // split based on whether they are supported, bridging, top layers etc.)
    print.objects[object_idx].typed_slices = true;
    Ok(())
}

/// The body of `detect_surfaces_type` over the layers taken out of `object`.
#[allow(clippy::too_many_lines)]
fn detect_in_layers(
    layers: &mut [Layer],
    object: &PrintObject,
    print: &Print,
) -> Result<(), FlowError> {
    // Interface shells: the intersecting parts are treated as self standing objects supporting
    // each other. Each of the objects will have a full number of top / bottom layers, even if
    // these top / bottom layers are completely hidden inside a collective body of intersecting
    // parts. This is useful if one of the parts is to be dissolved, or if it is transparent and
    // the internal shells should be visible.
    let spiral_mode = print.config().spiral_mode.value;
    let interface_shells = !spiral_mode && object.config().interface_shells.value;
    let num_layers = if spiral_mode {
        #[allow(clippy::cast_sign_loss)]
        let bottom_shell_layers =
            object.printing_region(0).config().bottom_shell_layers.value as usize;
        orca_stl::min(bottom_shell_layers, layers.len())
    } else {
        layers.len()
    };

    for region_id in 0..object.num_printing_regions() {
        log(
            Severity::Debug,
            &format!("Detecting solid surfaces for region {region_id} in parallel - start"),
        );

        // If interface shells are allowed, the region->surfaces cannot be overwritten as they may
        // be used by other threads: the C++ caches the loop's results then; this translation
        // always does (crate::layers_out::map_range).
        let end = if spiral_mode {
            // In spiral vase mode, reserve the last layer for the top surface if more than 1
            // layer is planned for the vase bottom.
            if num_layers > 1 {
                num_layers - 1
            } else {
                num_layers
            }
        } else {
            // In non-spiral vase mode, go over all layers.
            layers.len()
        };
        let surfaces_new = {
            let layers: &[Layer] = layers;
            map_range(0, end, 1, |idx_layer| {
                classify_layer(
                    layers,
                    object,
                    print,
                    region_id,
                    idx_layer,
                    interface_shells,
                )
            })?
        };
        print.throw_if_canceled();
        // save surfaces to layer
        for (idx_layer, surfaces) in surfaces_new.into_iter().enumerate() {
            layers[idx_layer].regions[region_id].slices.surfaces = surfaces;
        }

        if spiral_mode {
            if num_layers > 1 {
                // Turn the last bottom layer infill to a top infill, so it will be extruded with
                // a proper pattern.
                layers[num_layers - 1].regions[region_id]
                    .slices
                    .set_type(SurfaceType::Top);
            }
            for layer in &mut layers[num_layers..] {
                layer.regions[region_id]
                    .slices
                    .set_type(SurfaceType::Internal);
            }
        }

        // ORCA: Create a SECOND bridge layer above the first bridge layer. Surface is flagged as
        // a new surface type called stInternalAfterExternalBridge. Algorithm only considers
        // stInternal surfaces for re-classification, leaving stTop unaffected. Only iterate to
        // the second-to-last layer, since we look at layer i+1.
        let extra_bridge_layer = object.config().enable_extra_bridge_layer.value;
        if extra_bridge_layer == EnableExtraBridgeLayer::eblApplyToAll
            || extra_bridge_layer == EnableExtraBridgeLayer::eblExternalBridgeOnly
        {
            second_bridge_layer(layers, object, print, region_id)?;
        }

        log(
            Severity::Debug,
            &format!(
                "Detecting solid surfaces for region {region_id} - clipping in parallel - start"
            ),
        );
        // Fill in layerm->fill_surfaces by trimming the layerm->slices by the cummulative
        // layerm->fill_surfaces.
        parallel_for_each_mut(layers, |layer: &mut Layer| {
            print.throw_if_canceled();
            layer.regions[region_id].slices_to_fill_surfaces_clipped();
        });
        print.throw_if_canceled();
        log(
            Severity::Debug,
            &format!(
                "Detecting solid surfaces for region {region_id} - clipping in parallel - end"
            ),
        );
    } // for each this->print->region_count
    Ok(())
}

/// One iteration of the classifying loop: the new slices of layer `idx_layer`'s region
/// `region_id` (`surfaces_new[idx_layer]` with `interface_shells`, else what replaces
/// `layerm->slices.surfaces`).
#[allow(clippy::too_many_lines)]
fn classify_layer(
    layers: &[Layer],
    object: &PrintObject,
    print: &Print,
    region_id: usize,
    idx_layer: usize,
    interface_shells: bool,
) -> Result<Surfaces, FlowError> {
    // If we have soluble support material, don't bridge. The overhang will be squished against a
    // soluble layer separating the support from the print.
    // BBS: the above logic only applys for normal(auto) support. Complete logic:
    // 1. has support, top z distance=0 (soluble material), auto support
    // 2. for normal(auto), bridge_no_support is off
    // 3. for tree(auto), interface top layers=0, max bridge length=0,
    //    support_critical_regions_only=false (only in this way the bridge is fully supported)
    // (The C++ works this out once for each leaf of the loop; it reads no layer.)
    if object.has_support() {
        supports_stop(object, FUNCTION);
    }
    let config = object.config();
    let mut bottom_is_fully_supported = object.has_support()
        && config.support_top_z_distance.value == 0.
        && is_auto(config.support_type.value);
    if config.support_type.value == SupportType::stNormalAuto {
        bottom_is_fully_supported &= !config.bridge_no_support.value;
    } else if config.support_type.value == SupportType::stTreeAuto {
        bottom_is_fully_supported &= config.support_interface_top_layers.value > 0
            && config.max_bridge_length.value == 0.
            && !config.support_critical_regions_only.value;
    }
    let surface_type_bottom_other = if bottom_is_fully_supported {
        SurfaceType::Bottom
    } else {
        SurfaceType::BottomBridge
    };

    print.throw_if_canceled();
    let layer = &layers[idx_layer];
    let layerm = &layer.regions[region_id];
    // comparison happens against the *full* slices (considering all regions)
    // unless internal shells are requested
    let upper_layer = if idx_layer + 1 < layers.len() {
        Some(&layers[idx_layer + 1])
    } else {
        None
    };
    let lower_layer = if idx_layer > 0 {
        Some(&layers[idx_layer - 1])
    } else {
        None
    };
    // collapse very narrow parts (using the safety offset in the diff is not enough)
    #[allow(clippy::cast_precision_loss)]
    let offset = layerm
        .flow(layer, object, print, FlowRole::ExternalPerimeter)?
        .scaled_width() as f32
        / 10.0f32;

    let mut layerm_slices_surfaces = to_expolygons(&layerm.slices.surfaces);
    // no_perimeter_full_bridge allow to put bridges where there are nothing, hence adding area to
    // slice, that's why we need to start from the result of PerimeterGenerator.
    if layerm
        .region(object)
        .config()
        .counterbore_hole_bridging
        .value
        == CounterboreHoleBridgingOption::chbFilled
    {
        layerm_slices_surfaces = union_ex_two(
            &layerm_slices_surfaces,
            &to_expolygons(&layerm.fill_surfaces.surfaces),
            false,
        );
    }

    // find top surfaces (difference between current surfaces
    // of current layer and upper one)
    let mut top: Surfaces = Surfaces::new();
    if let Some(upper_layer) = upper_layer {
        let upper_slices = if interface_shells {
            diff_ex_expolygons(
                &layerm_slices_surfaces,
                &to_expolygons(&upper_layer.regions[region_id].slices.surfaces),
                ApplySafetyOffset::Yes,
            )
        } else {
            diff_ex_expolygons(
                &layerm_slices_surfaces,
                &upper_layer.lslices,
                ApplySafetyOffset::Yes,
            )
        };
        surfaces_append_typed_owned(
            &mut top,
            opening_ex(
                &upper_slices,
                offset,
                DEFAULT_JOIN_TYPE,
                DEFAULT_MITER_LIMIT,
            ),
            SurfaceType::Top,
        );
    } else {
        // if no upper layer, all surfaces of this one are solid
        // we clone surfaces because we're going to clear the slices collection
        top.clone_from(&layerm.slices.surfaces);
        for surface in &mut top {
            surface.surface_type = SurfaceType::Top;
        }
    }

    // Find bottom surfaces (difference between current surfaces of current layer and lower one).
    let mut bottom: Surfaces = Surfaces::new();
    if let Some(lower_layer) = lower_layer {
        // Any surface lying on the void is a true bottom bridge (an overhang)
        surfaces_append_typed_owned(
            &mut bottom,
            opening_ex(
                &diff_ex_expolygons(
                    &layerm_slices_surfaces,
                    &lower_layer.lslices,
                    ApplySafetyOffset::Yes,
                ),
                offset,
                DEFAULT_JOIN_TYPE,
                DEFAULT_MITER_LIMIT,
            ),
            surface_type_bottom_other,
        );
        // if user requested internal shells, we need to identify surfaces
        // lying on other slices not belonging to this region
        if interface_shells {
            // non-bridging bottom surfaces: any part of this layer lying
            // on something else, excluding those lying on our own region
            surfaces_append_typed_owned(
                &mut bottom,
                opening_ex(
                    &diff_ex_polygons_expolygons(
                        // supported
                        &intersection_expolygons(
                            &layerm_slices_surfaces,
                            &lower_layer.lslices,
                            ApplySafetyOffset::No,
                        ),
                        &to_expolygons(&lower_layer.regions[region_id].slices.surfaces),
                        ApplySafetyOffset::Yes,
                    ),
                    offset,
                    DEFAULT_JOIN_TYPE,
                    DEFAULT_MITER_LIMIT,
                ),
                SurfaceType::Bottom,
            );
        }
    } else {
        // if no lower layer, all surfaces of this one are solid
        // we clone surfaces because we're going to clear the slices collection
        bottom.clone_from(&layerm.slices.surfaces);
        for surface in &mut bottom {
            surface.surface_type = SurfaceType::Bottom;
        }
    }

    // now, if the object contained a thin membrane, we could have overlapping bottom
    // and top surfaces; let's do an intersection to discover them and consider them
    // as bottom surfaces (to allow for bridge detection)
    if !top.is_empty() && !bottom.is_empty() {
        let cracks = intersection_ex_expolygons(
            &to_expolygons(&top),
            &to_expolygons(&bottom),
            ApplySafetyOffset::No,
        );
        if !cracks.is_empty() {
            if lower_layer.is_some() {
                // Only detect small cracks for non-first layer, because first layer should always
                // be bottom
                #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
                let small_crack_threshold = (-layerm
                    .flow(layer, object, print, FlowRole::ExternalPerimeter)?
                    .scaled_width() as f64
                    * 1.5) as f32;

                for crack in &cracks {
                    if offset_ex_expolygon(
                        crack,
                        small_crack_threshold,
                        DEFAULT_JOIN_TYPE,
                        DEFAULT_MITER_LIMIT,
                    )
                    .is_empty()
                    {
                        // For small cracks, if it's part of a large bottom surface, then it
                        // should be added to bottom as well
                        if bottom.iter().any(|s: &Surface| {
                            let se = &s.expolygon;
                            diff_ex_expolygon(crack, se, ApplySafetyOffset::Yes).is_empty()
                                && se.area() > crack.area() * 2.
                                && !offset_ex_expolygons(
                                    &diff_ex_expolygon(se, crack, ApplySafetyOffset::No),
                                    small_crack_threshold,
                                    DEFAULT_JOIN_TYPE,
                                    DEFAULT_MITER_LIMIT,
                                )
                                .is_empty()
                        }) {
                            continue;
                        }

                        // Crack too small, leave it as part of the top surface, remove it from
                        // bottom surfaces
                        let mut bot_tmp: Surfaces = Surfaces::new();
                        for b in &bottom {
                            surfaces_append_typed_owned(
                                &mut bot_tmp,
                                diff_ex_expolygon_expolygons(
                                    &b.expolygon,
                                    &offset_ex_expolygon(
                                        crack,
                                        -small_crack_threshold,
                                        DEFAULT_JOIN_TYPE,
                                        DEFAULT_MITER_LIMIT,
                                    ),
                                    ApplySafetyOffset::No,
                                ),
                                b.surface_type,
                            );
                        }
                        bottom = bot_tmp;
                    }
                }
            }

            let top_polygons = to_polygons_surfaces(&top);
            top.clear();
            surfaces_append_typed_owned(
                &mut top,
                diff_ex_polygons_expolygons(
                    &top_polygons,
                    &to_expolygons(&bottom),
                    ApplySafetyOffset::No,
                ),
                SurfaceType::Top,
            );
        }
    }

    // save surfaces to layer
    let mut surfaces_out: Surfaces = Surfaces::new();
    // The C++ reads the slices it is replacing (surfaces_backup) without interface shells.
    let surfaces_prev_expolys: ExPolygons = if interface_shells {
        layerm_slices_surfaces
    } else {
        to_expolygons(&layerm.slices.surfaces)
    };

    // find internal surfaces (difference between top/bottom surfaces and others)
    {
        let mut topbottom: Polygons = to_polygons_surfaces(&top);
        topbottom.extend(to_polygons_surfaces(&bottom));
        surfaces_append_typed_owned(
            &mut surfaces_out,
            diff_ex_expolygons_polygons(&surfaces_prev_expolys, &topbottom, ApplySafetyOffset::No),
            SurfaceType::Internal,
        );
    }

    surfaces_append_owned(&mut surfaces_out, top);
    surfaces_append_owned(&mut surfaces_out, bottom);
    Ok(surfaces_out)
}

/// ORCA's second bridge layer for region `region_id`: the collect pass, the write pass, and the
/// interim reclassification of `stInternalAfterExternalBridge` back to `stBottomBridge` in every
/// region (a loop over all the regions inside the loop over the regions, as the C++ has it).
fn second_bridge_layer(
    layers: &mut [Layer],
    object: &PrintObject,
    print: &Print,
    region_id: usize,
) -> Result<(), FlowError> {
    let last = if layers.is_empty() {
        0
    } else {
        layers.len() - 1
    };

    // ORCA: Two-phase split (collect-then-apply) to eliminate a data race in the original
    // single-phase parallel_for.
    //
    // Phase 1: read-only pass — collect each layer's stBottomBridge polygons into a per-layer
    // cache. No surfaces are mutated, so concurrent reads are safe.
    let new_top_surfaces = {
        let layers: &[Layer] = layers;
        let bridge_polys_per_layer: Vec<Polygons> =
            map_range(0, last, 1, |i| -> Result<Polygons, FlowError> {
                print.throw_if_canceled();
                let mut bridge_polys = Polygons::new();
                for sbot in &layers[i].regions[region_id].slices.surfaces {
                    if sbot.surface_type == SurfaceType::BottomBridge {
                        bridge_polys.extend(to_polygons(sbot));
                    }
                }
                Ok(bridge_polys)
            })?;

        // Phase 2: write pass — each iteration mutates only m_layers[i+1]->slices.surfaces and
        // reads its bridge polygons from the precomputed cache.
        map_range(0, last, 1, |i| {
            second_bridge_surfaces(
                layers,
                object,
                print,
                region_id,
                i,
                &bridge_polys_per_layer[i],
            )
        })?
    };
    for (i, new_surfaces) in new_top_surfaces.into_iter().enumerate() {
        if let Some(new_surfaces) = new_surfaces {
            layers[i + 1].regions[region_id].slices.surfaces = new_surfaces;
        }
    }

    // ORCA: Interim workaround - for now the new stInternalAfterExternalBridge surfaace is
    // re-classified back to a bottom bridge. As a starting point, this improves bridging
    // reliability as it extrudes two external bridge layers. However, TODO: Implement a new
    // surface type throughout the codebase
    for region_id in 0..object.num_printing_regions() {
        parallel_for_each_mut(layers, |layer: &mut Layer| {
            for s in &mut layer.regions[region_id].slices.surfaces {
                if s.surface_type == SurfaceType::InternalAfterExternalBridge {
                    s.surface_type = SurfaceType::BottomBridge;
                }
            }
        });
    }
    Ok(())
}

/// Phase 2's iteration `i`: what replaces layer `i + 1`'s slices of region `region_id`, or `None`
/// where layer `i` has no bottom bridge (`continue`).
fn second_bridge_surfaces(
    layers: &[Layer],
    object: &PrintObject,
    print: &Print,
    region_id: usize,
    i: usize,
    polygons_bridge: &Polygons,
) -> Result<Option<Surfaces>, FlowError> {
    print.throw_if_canceled();

    // Step 1 + 2: pull the precomputed bridge polygons for the current source layer.
    // Step 3: Early termination of loop if no meaningfull bridge found
    // No bridge polygons found, continue to the next layer
    if polygons_bridge.is_empty() {
        return Ok(None);
    }

    // Step 4: Bottom bridge polygons found - scan and create layer+1 bridge polygon
    let top_surfs = &layers[i + 1].regions[region_id].slices.surfaces;
    let mut new_surfaces: Surfaces = Surfaces::with_capacity(top_surfs.len());

    // filtering parameters here. Filter bridges that are less than 2x external walls and 2xN
    // internal perimeters wide.
    let layer = &layers[i];
    let layerm = &layer.regions[region_id];
    // number of internal walls, clamped to a minimum of 0 as a safety precaution
    let number_of_internal_walls = orca_stl::max(
        0,
        layerm
            .region(object)
            .config()
            .wall_loops
            .value
            .wrapping_sub(1),
    );
    // shrink down by external perimeter width (effectively filtering out 2x external perimeters
    // wide bridges), plus the number of internal walls times their width: coord_t arithmetic,
    // then float.
    let external_width = layerm
        .flow(layer, object, print, FlowRole::ExternalPerimeter)?
        .scaled_width();
    let perimeter_width = layerm
        .flow(layer, object, print, FlowRole::Perimeter)?
        .scaled_width();
    #[allow(clippy::cast_precision_loss)]
    let offset_distance = external_width
        .wrapping_add(perimeter_width.wrapping_mul(i64::from(number_of_internal_walls)))
        as f32;

    // ORCA: Same-layer-top guard: every stTop polygon at layer i+1 (this region), expanded by
    // offset_distance; any candidate second-bridge area under it is subtracted below. Kept as
    // ExPolygons so that holes inside an stTop surface are offset with the correct sign.
    let mut same_layer_top_expanded: ExPolygons = ExPolygons::new();
    {
        let mut same_layer_top: ExPolygons = ExPolygons::new();
        for s in top_surfs {
            if s.surface_type == SurfaceType::Top {
                same_layer_top.push(s.expolygon.clone());
            }
        }
        if !same_layer_top.is_empty() {
            same_layer_top_expanded = offset_ex_expolygons(
                &same_layer_top,
                offset_distance,
                DEFAULT_JOIN_TYPE,
                DEFAULT_MITER_LIMIT,
            );
        }
    }

    // For each surface in the layer above
    for s_up in top_surfs {
        // Only reclassify stInternal polygons (i.e. what will become later solid and sparse
        // infill). Leave the rest unaffected
        if s_up.surface_type != SurfaceType::Internal {
            new_surfaces.push(s_up.clone()); // do not modify them
            continue; // continue to the next surface
        }
        // Identify stInternal polygons that overlap with the bridging polygons on the layer
        // underneath.
        let p_up = to_polygons(s_up);
        let mut overlap = intersection_ex(&p_up, polygons_bridge, ApplySafetyOffset::Yes);
        // Filter out the resulting candidate bridges based on size. First perform a shrink
        // operation... followed by an expand operation to bring them back to the original size
        // (positive offset)
        overlap = offset_ex_expolygons(
            &shrink_ex_expolygons(
                &overlap,
                offset_distance,
                DEFAULT_JOIN_TYPE,
                DEFAULT_MITER_LIMIT,
            ),
            offset_distance,
            DEFAULT_JOIN_TYPE,
            DEFAULT_MITER_LIMIT,
        );

        // ORCA: subtract the expanded same-layer stTop mask. Drops stInternal islands fully
        // surrounded by stTop at i+1 without affecting bridges that lie away from the top.
        if !same_layer_top_expanded.is_empty() && !overlap.is_empty() {
            overlap =
                diff_ex_expolygons(&overlap, &same_layer_top_expanded, ApplySafetyOffset::Yes);
        }

        // Now subtract the filtered new bridge layer from the remaining internal surfaces to
        // create the new internal surface
        let remainder = diff_ex_polygons_expolygons(&p_up, &overlap, ApplySafetyOffset::Yes);

        // Remainder stays as stInternal
        let unified_remainder = union_safety_offset_ex_expolygons(&remainder);
        for ex_remainder in unified_remainder {
            new_surfaces.push(Surface::with_expolygon(SurfaceType::Internal, ex_remainder));
        }
        // Overlap portion becomes the new polygon type - stInternalAfterExternalBridge
        let unified_overlap = union_safety_offset_ex_expolygons(&overlap);
        for ex_overlap in unified_overlap {
            new_surfaces.push(Surface::with_expolygon(
                SurfaceType::InternalAfterExternalBridge,
                ex_overlap,
            ));
        }
    }
    Ok(Some(new_surfaces))
}
