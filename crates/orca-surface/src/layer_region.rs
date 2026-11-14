// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/LayerRegion.cpp lines 935-988 of OrcaSlicer v2.4.2
// (LayerRegion::prepare_fill_surfaces, infill_area_threshold and trim_surfaces), translated on
// 2026-11-14 by orca-o3-surfaces-2. Copyright the OrcaSlicer authors, the BambuStudio authors
// (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU Affero General Public
// License, version 3 (AGPL-3.0; LICENSE).

//! Three `LayerRegion` members that orca-layer leaves to this crate, as the extension trait
//! [`LayerRegionFill`] on orca-layer's `LayerRegion`:
//!
//! - `prepare_fill_surfaces()`: with no top shell layers (and not in spiral vase mode) the top
//!   fill surfaces become internal (internal void with `infill_only_where_needed`, which the
//!   engine fixes off); with no bottom shell layers the bottoms become internal; at 100 % sparse
//!   infill density (and not in spiral vase mode) the internal surfaces become internal solid.
//!   The boundaries never change, so `psPrepareInfill` stays idempotent.
//! - `infill_area_threshold()`: the solid infill spacing squared, the smallest area
//!   `combine_infill` combines.
//! - `trim_surfaces(trimming_polygons)`: the slices (all internal) clipped by the polygons. No
//!   C++ code calls it.
//!
//! The C++ reaches the layer, the object and the print through back-pointers, which are the
//! arguments here: `layer` holds the region, `object` the layer, `print` the object. The debug
//! build's check that every slice is internal (`trim_surfaces`, `#ifndef NDEBUG`) and the SVG
//! exports of `SLIC3R_DEBUG_SLICE_PROCESSING` are not built in the engine and are left out.
//! Nothing here reads a setting domain A leaves out, so nothing stops with `NotPorted`.

use orca_geom::clipper_utils::{ApplySafetyOffset, intersection_ex_expolygons_polygons};
use orca_geom::point::EPSILON;
use orca_geom::polygon::Polygon;
use orca_layer::flow::{FlowError, FlowRole};
use orca_layer::layer::{Layer, LayerRegion};
use orca_layer::print::Print;
use orca_layer::print_object::PrintObject;
use orca_layer::surface::{SurfaceType, to_expolygons};

/// `LayerRegion::prepare_fill_surfaces`, `infill_area_threshold` and `trim_surfaces`.
pub trait LayerRegionFill {
    /// `prepare_fill_surfaces()`: the fill surfaces retyped for zero top or bottom shell layers
    /// and for 100 % sparse infill density. `object` holds the region's layer, `print` the
    /// object.
    fn prepare_fill_surfaces(&mut self, object: &PrintObject, print: &Print);

    /// `infill_area_threshold()`: the solid infill flow's scaled spacing squared, in `double`.
    /// `layer` holds the region, `object` the layer, `print` the object.
    ///
    /// # Errors
    /// What the solid infill flow gives.
    fn infill_area_threshold(
        &self,
        layer: &Layer,
        object: &PrintObject,
        print: &Print,
    ) -> Result<f64, FlowError>;

    /// `trim_surfaces(trimming_polygons)`: the slices replaced by their intersection with
    /// `trimming_polygons`, all internal.
    fn trim_surfaces(&mut self, trimming_polygons: &[Polygon]);
}

impl LayerRegionFill for LayerRegion {
    fn prepare_fill_surfaces(&mut self, object: &PrintObject, print: &Print) {
        // Note: in order to make the psPrepareInfill step idempotent, we should never
        // alter fill_surfaces boundaries on which our idempotency relies since that's
        // the only meaningful information returned by psPerimeters.

        let spiral_mode = print.config().spiral_mode.value;
        let region_config = self.region(object).config();

        // if no solid layers are requested, turn top/bottom surfaces to internal
        if !spiral_mode && region_config.top_shell_layers.value == 0 {
            for surface in &mut self.fill_surfaces.surfaces {
                if surface.is_top() {
                    //BBS
                    surface.surface_type = if PrintObject::INFILL_ONLY_WHERE_NEEDED {
                        SurfaceType::InternalVoid
                    } else {
                        SurfaceType::Internal
                    };
                }
            }
        }
        if region_config.bottom_shell_layers.value == 0 {
            for surface in &mut self.fill_surfaces.surfaces {
                if surface.is_bottom() {
                    // (surface.surface_type == stBottom)
                    surface.surface_type = SurfaceType::Internal;
                }
            }
        }

        if !spiral_mode
            && orca_crt::fabs(region_config.sparse_infill_density.value - 100.) < EPSILON
        {
            // Turn all internal sparse infill into solid infill, if sparse_infill_density is
            // 100%
            for surface in &mut self.fill_surfaces.surfaces {
                if surface.surface_type == SurfaceType::Internal {
                    surface.surface_type = SurfaceType::InternalSolid;
                }
            }
        }
    }

    #[allow(clippy::cast_precision_loss)]
    fn infill_area_threshold(
        &self,
        layer: &Layer,
        object: &PrintObject,
        print: &Print,
    ) -> Result<f64, FlowError> {
        let ss = self
            .flow(layer, object, print, FlowRole::SolidInfill)?
            .scaled_spacing() as f64;
        Ok(ss * ss)
    }

    fn trim_surfaces(&mut self, trimming_polygons: &[Polygon]) {
        let trimmed = intersection_ex_expolygons_polygons(
            &to_expolygons(&self.slices.surfaces),
            trimming_polygons,
            ApplySafetyOffset::No,
        );
        self.slices.set_typed_owned(trimmed, SurfaceType::Internal);
    }
}
