// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/PerimeterGenerator.hpp (12-47, FuzzySkinConfig) and
// src/libslic3r/Feature/FuzzySkin/FuzzySkin.cpp (476-493, should_fuzzify; 561-586 and 685-693,
// the parts of the two apply_fuzzy_skin that leave a wall as it is) of OrcaSlicer v2.4.2,
// translated on 2026-11-14 (orca-o3-walls-1). Copyright the OrcaSlicer authors, the BambuStudio
// authors (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU Affero General
// Public License, version 3 (AGPL-3.0; LICENSE).

//! The fuzzy skin a perimeter generator asks about: [`FuzzySkinConfig`], [`should_fuzzify`] and
//! the two `apply_fuzzy_skin`s ([`apply_fuzzy_skin`] of a classic loop's polygon,
//! [`apply_fuzzy_skin_line`] of an Arachne line).
//!
//! Domain A has no fuzzy skin: every region's `fuzzy_skin` is `none` or `disabled_fuzzy`, so
//! `should_fuzzify` is false for every wall and both functions leave the wall as it is, which is
//! all this module does. Where a region of the generator's `regions_by_fuzzify` would fuzzify the
//! wall (the only way the C++ reaches `fuzzy_polyline`, `fuzzy_extrusion_line`,
//! `collect_merged_fuzzy_regions` and `Algorithm::split_line`), the port stops with
//! [`NotPorted`] naming the family [`FUZZY_SKIN`]. `group_region_by_fuzzify`, which fills the
//! list, is walls-2's.
//!
//! These functions belong in a fuzzy-skin crate that does not exist yet; they are translated
//! here (walls-1) because `traverse_loops` and `traverse_extrusions` call them. The header's
//! `std::hash<FuzzySkinConfig>` is left out: the engine never instantiates it.
//!
//! [`NotPorted`]: crate::stop::NotPorted

use orca_arachne::extrusion_line::ExtrusionLine;
use orca_config::print_config::{FuzzySkinMode, FuzzySkinType, NoiseType};
use orca_geom::point::Coord;
use orca_geom::polygon::Polygon;

use crate::perimeter_generator::PerimeterGenerator;
use crate::stop::{FUZZY_SKIN, not_ported};

/// `FuzzySkinConfig`: the fuzzy skin of a region on a layer.
#[derive(Clone, Copy, Debug, Default)]
pub struct FuzzySkinConfig {
    /// `type`.
    pub r#type: FuzzySkinType,
    /// `thickness`, scaled.
    pub thickness: Coord,
    /// `point_distance`, scaled.
    pub point_distance: Coord,
    /// `fuzzy_first_layer`.
    pub fuzzy_first_layer: bool,
    /// `noise_type`.
    pub noise_type: NoiseType,
    /// `noise_scale`.
    pub noise_scale: f64,
    /// `noise_octaves`.
    pub noise_octaves: i32,
    /// `noise_persistence`.
    pub noise_persistence: f64,
    /// `mode`.
    pub mode: FuzzySkinMode,
    /// `ripples_per_layer`.
    pub ripples_per_layer: i32,
    /// `ripple_offset`.
    pub ripple_offset: f64,
    /// `layers_between_ripple_offset`.
    pub layers_between_ripple_offset: i32,
    /// `layer_id`: not compared by `==`.
    pub layer_id: i32,
}

impl PartialEq for FuzzySkinConfig {
    /// `operator==`: every field but `layer_id`.
    #[allow(clippy::float_cmp)]
    fn eq(&self, r: &Self) -> bool {
        self.r#type == r.r#type
            && self.thickness == r.thickness
            && self.point_distance == r.point_distance
            && self.fuzzy_first_layer == r.fuzzy_first_layer
            && self.noise_type == r.noise_type
            && self.noise_scale == r.noise_scale
            && self.noise_octaves == r.noise_octaves
            && self.noise_persistence == r.noise_persistence
            && self.mode == r.mode
            && self.ripples_per_layer == r.ripples_per_layer
            && self.ripple_offset == r.ripple_offset
            && self.layers_between_ripple_offset == r.layers_between_ripple_offset
    }
}

/// `should_fuzzify(config, layer_id, loop_idx, is_contour)`: whether the wall of index `loop_idx`
/// (0 the outer one), a contour or a hole, gets the fuzzy skin on this layer.
#[must_use]
pub fn should_fuzzify(
    config: &FuzzySkinConfig,
    layer_id: i32,
    loop_idx: usize,
    is_contour: bool,
) -> bool {
    let fuzziy_type = config.r#type;

    if fuzziy_type == FuzzySkinType::None || fuzziy_type == FuzzySkinType::Disabled_fuzzy {
        return false;
    }
    if !config.fuzzy_first_layer && layer_id <= 0 {
        // Do not fuzzy first layer unless told to
        return false;
    }

    let fuzzify_contours = (loop_idx == 0 && fuzziy_type != FuzzySkinType::Hole)
        || fuzziy_type == FuzzySkinType::AllWalls;
    let fuzzify_holes = (fuzziy_type == FuzzySkinType::Hole
        || fuzziy_type == FuzzySkinType::All
        || fuzziy_type == FuzzySkinType::AllWalls)
        && (loop_idx == 0 || fuzziy_type == FuzzySkinType::AllWalls);

    if is_contour {
        fuzzify_contours
    } else {
        fuzzify_holes
    }
}

/// Stops when any region of the generator would fuzzify the wall: the only case in which the C++
/// changes it (with one region by `should_fuzzify` directly, with several through
/// `collect_merged_fuzzy_regions`, which keeps only the regions that fuzzify it).
fn stop_if_fuzzified(
    function: &'static str,
    perimeter_generator: &PerimeterGenerator<'_>,
    loop_idx: usize,
    is_contour: bool,
) {
    for (config, _) in &perimeter_generator.regions_by_fuzzify {
        if should_fuzzify(config, perimeter_generator.layer_id, loop_idx, is_contour) {
            not_ported(function, FUZZY_SKIN);
        }
    }
}

/// `apply_fuzzy_skin(polygon, perimeter_generator, loop_idx, is_contour)`: the polygon as it is
/// when no region fuzzifies the wall.
///
/// # Panics
/// With [`NotPorted`](crate::stop::NotPorted) ([`FUZZY_SKIN`]) where a region would fuzzify it.
#[must_use]
pub fn apply_fuzzy_skin(
    polygon: &Polygon,
    perimeter_generator: &PerimeterGenerator<'_>,
    loop_idx: usize,
    is_contour: bool,
) -> Polygon {
    stop_if_fuzzified(
        "apply_fuzzy_skin",
        perimeter_generator,
        loop_idx,
        is_contour,
    );
    polygon.clone()
}

/// `apply_fuzzy_skin(extrusion, perimeter_generator, is_contour)` of an Arachne line, whose
/// `inset_idx` is the wall's index: the line left as it is when no region fuzzifies it.
///
/// # Panics
/// With [`NotPorted`](crate::stop::NotPorted) ([`FUZZY_SKIN`]) where a region would fuzzify it.
pub fn apply_fuzzy_skin_line(
    extrusion: &mut ExtrusionLine,
    perimeter_generator: &PerimeterGenerator<'_>,
    is_contour: bool,
) {
    stop_if_fuzzified(
        "apply_fuzzy_skin",
        perimeter_generator,
        extrusion.inset_idx,
        is_contour,
    );
}
