// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Slicing.hpp lines 1-227 (SlicingParameters,
// equal_layering, the layer height range types, HeightProfileSmoothingParams,
// LayerHeightEditActionType and the nozzle limits of namespace Slicing; the other declarations go
// with their bodies) and src/libslic3r/Slicing.cpp lines 1-306, of OrcaSlicer v2.4.2, translated
// on 2026-11-13 by orca-o3-slice-1. Based on an implementation by @platsch. Copyright the
// OrcaSlicer authors, the BambuStudio authors (Bambu Lab), the PrusaSlicer authors (Prusa
// Research) and the Slic3r authors (Alessandro Ranellucci), under the GNU Affero General Public
// License, version 3 (AGPL-3.0; LICENSE).

//! `SlicingParameters`: the layer heights, the raft and the gaps an object is sliced with, made
//! from the print's and the object's configs; and the layer height profile of an object's height
//! ranges.
//!
//! - [`SlicingParameters::create_from_config`]: the first layer (`initial_layer_print_height`, or
//!   the layer height when it is 0 or less), the limits of the layer height over the support
//!   filaments (when there is any support, raft or enforced support layer) and the object's
//!   extruders (nozzle 0 when it has none: `get_at` of `size_t(-1)`, which is the first value), the
//!   zero-gap interfaces, the gaps (rounded to whole layers with `std::round` unless the support
//!   has a layer height of its own), and the raft's layers and heights, with the object lifted
//!   onto it. Every setting a branch reads is in the print or object config, so the C++ answers
//!   for every config and so does this; domain A has no raft and no support, but the probe's SLPR
//!   records reach every branch.
//! - [`equal_layering`]: whether two parameters lead to the same layering (the gaps and the
//!   support's maximum layer height are left out, as the C++ leaves them out).
//! - [`layer_height_profile_from_ranges`]: the height ranges trimmed one by the other (a range
//!   narrower than `EPSILON` dropped), the fixed first layer put first, then the profile of `z` and
//!   layer height pairs, the gaps filled with the object's layer height up to its uncompensated
//!   top; a pair whose height repeats the last one twice moves the last `z` instead.
//! - [`min_layer_height_from_nozzle`] and [`max_layer_height_from_nozzle`] (namespace `Slicing`):
//!   the limits of nozzle `idx` (1 based) of a `DynamicPrintConfig`. A minimum of 0 is 0.07 and
//!   any other is at least 0.01; a maximum of 0 is three quarters of the nozzle, and never below
//!   the minimum.
//!
//! The C++ asserts that `equal_layering`'s parameters are valid; its release build does not, and
//! neither does this. The declarations of `layer_height_profile_adaptive`,
//! `smooth_height_profile`, `adjust_layer_height_profile`, `generate_object_layers`,
//! `check_object_layers_fixed` and `generate_layer_height_texture` go with their bodies
//! (orca-o3-slice-2; the texture is the GUI's and not reached). The cereal `serialize` of a
//! height range is the GUI's undo stack and is not translated.

use orca_config::config::Config;
use orca_config::option::is_approx;
use orca_config::{ConfigOption, OptionValue, PrintConfig, PrintObjectConfig};
use orca_eigen::Vec3d;
use orca_geom::point::EPSILON;
use orca_index::stop::not_ported;
use orca_model::model::LayerConfigRanges;
use orca_stl::{max, min};

/// `MIN_LAYER_HEIGHT`: the smallest layer height the variable layer height algorithm takes.
pub const MIN_LAYER_HEIGHT: f64 = 0.01;
/// `MIN_LAYER_HEIGHT_DEFAULT`: the minimum layer height of a nozzle whose minimum is 0.
pub const MIN_LAYER_HEIGHT_DEFAULT: f64 = 0.07;
/// `LAYER_HEIGHT_CHANGE_STEP`: the step of the interactive layer height editor
/// (`adjust_layer_height_profile`, orca-o3-slice-2).
pub const LAYER_HEIGHT_CHANGE_STEP: f64 = 0.04;

/// The stop of [`layer_height_profile_from_ranges`] on a height range whose config holds no
/// float `layer_height`: the C++ dereferences the null option (undefined) or, for another type,
/// throws `BadOptionTypeException`. The command line's height ranges always hold a
/// `layer_height` above 0 (orca-cli's `place.rs`), and it drops the ranges a 3MF file holds, so no
/// job reaches it.
pub const RANGE_WITHOUT_LAYER_HEIGHT: &str = "range-without-layer-height";

/// The stop of [`min_layer_height_from_nozzle`] and [`max_layer_height_from_nozzle`] on a config
/// whose option is missing, not a non-nullable list of floats, or empty: the C++'s
/// `dynamic_cast` gives a null option it dereferences, or `get_at` reads past an empty vector
/// (undefined). Only Orca's GUI calls these (`PresetHints.cpp`), on a full printer config.
pub const NOZZLE_OPTION_MISSING: &str = "nozzle-option-missing";

/// `t_layer_height_range`: a height range, its low and high `z`.
pub type LayerHeightRange = (f64, f64);

/// `SlicingParameters`: the parameters that guide object slicing and support generation. They
/// account for a raft and for whether the first object layer is printed with a normal or a
/// bridging flow (a normal flow over a soluble support, a bridging flow over a non-soluble one).
/// `SlicingParameters()` zeroes everything and is not valid.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SlicingParameters {
    /// `valid`.
    pub valid: bool,
    /// `base_raft_layers`: the number of raft layers.
    pub base_raft_layers: usize,
    /// `interface_raft_layers`: the number of interface layers, the contact layer included.
    pub interface_raft_layers: usize,
    /// `base_raft_layer_height`.
    pub base_raft_layer_height: f64,
    /// `interface_raft_layer_height`.
    pub interface_raft_layer_height: f64,
    /// `contact_raft_layer_height`.
    pub contact_raft_layer_height: f64,
    /// `layer_height`: the regular layer height, of every layer but the first, unless the layer
    /// ranges or the variable layer height table override it.
    pub layer_height: f64,
    /// `min_layer_height`: the smallest layer height of the adaptive layer height algorithm and
    /// the interactive editor.
    pub min_layer_height: f64,
    /// `max_layer_height`: the largest one.
    pub max_layer_height: f64,
    /// `max_suport_layer_height` (sic).
    pub max_suport_layer_height: f64,
    /// `first_print_layer_height`: the first layer of the print, the raft's or the object's.
    pub first_print_layer_height: f64,
    /// `first_object_layer_height`: the first print layer without a raft, a bridging flow over a
    /// non-soluble raft, or a normal layer over a soluble one.
    pub first_object_layer_height: f64,
    /// `first_object_layer_bridging`: whether the first layer over a non-soluble raft is printed
    /// with a bridging flow.
    pub first_object_layer_bridging: bool,
    /// `zero_gap_interface_top`.
    pub zero_gap_interface_top: bool,
    /// `zero_gap_interface_bottom`.
    pub zero_gap_interface_bottom: bool,
    /// `zero_gap_interface_raft`.
    pub zero_gap_interface_raft: bool,
    /// `gap_raft_object`: the gap when placing the object over the raft.
    pub gap_raft_object: f64,
    /// `gap_object_support`: the gap when placing support over the object.
    pub gap_object_support: f64,
    /// `gap_support_object`: the gap when placing the object over support.
    pub gap_support_object: f64,
    /// `raft_base_top_z`.
    pub raft_base_top_z: f64,
    /// `raft_interface_top_z`.
    pub raft_interface_top_z: f64,
    /// `raft_contact_top_z`.
    pub raft_contact_top_z: f64,
    /// `object_print_z_min`: the bottom of the printed object, 0 without a raft, else the raft's
    /// height (and the gap, unless the raft interface has none).
    pub object_print_z_min: f64,
    /// `object_print_z_max`: the top of the printed object, scaled by the shrinkage compensation
    /// in Z.
    pub object_print_z_max: f64,
    /// `object_print_z_uncompensated_max`: the top, not scaled by the shrinkage compensation.
    pub object_print_z_uncompensated_max: f64,
    /// `object_shrinkage_compensation_z`: the scaling factor that compensates the shrinkage in Z.
    pub object_shrinkage_compensation_z: f64,
}

/// `size_t(i)` of an `int`: sign-extended, so -1 is `SIZE_MAX`.
fn size_t(i: i32) -> usize {
    i as usize
}

/// `ConfigOptionFloats::get_at(i)`: value `i`, or the first one past the end.
fn get_at(values: &orca_config::ConfigOptionFloats, i: usize) -> f64 {
    *values.get_at(i)
}

/// The file-local `min_layer_height_from_nozzle(print_config, idx_nozzle)` of Slicing.cpp, on a
/// `PrintConfig`: the minimum of nozzle `idx_nozzle` (1 based).
fn print_min_layer_height_from_nozzle(print_config: &PrintConfig, idx_nozzle: i32) -> f64 {
    let min_layer_height = get_at(
        &print_config.min_layer_height,
        size_t(idx_nozzle.wrapping_sub(1)),
    );
    if min_layer_height == 0.0 {
        MIN_LAYER_HEIGHT_DEFAULT
    } else {
        max(MIN_LAYER_HEIGHT, min_layer_height)
    }
}

/// The file-local `max_layer_height_from_nozzle(print_config, idx_nozzle)` of Slicing.cpp, on a
/// `PrintConfig`: the maximum of nozzle `idx_nozzle` (1 based), 3/4 of the nozzle by default and
/// never below the minimum.
fn print_max_layer_height_from_nozzle(print_config: &PrintConfig, idx_nozzle: i32) -> f64 {
    let min_layer_height = print_min_layer_height_from_nozzle(print_config, idx_nozzle);
    let max_layer_height = get_at(
        &print_config.max_layer_height,
        size_t(idx_nozzle.wrapping_sub(1)),
    );
    let nozzle_dmr = get_at(
        &print_config.nozzle_diameter,
        size_t(idx_nozzle.wrapping_sub(1)),
    );
    max(
        min_layer_height,
        if max_layer_height == 0.0 {
            0.75 * nozzle_dmr
        } else {
            max_layer_height
        },
    )
}

/// `DynamicPrintConfig::opt_float(key, idx)`, the const one:
/// `dynamic_cast<const ConfigOptionFloats*>(option(key))->get_at(idx)`.
fn opt_float(print_config: &Config, function: &'static str, key: &str, idx: u32) -> f64 {
    match print_config.get(key) {
        Some(OptionValue::Floats {
            values,
            nullable: false,
        }) if !values.is_empty() => {
            let i = idx as usize;
            if i < values.len() {
                values[i]
            } else {
                values[0]
            }
        }
        _ => not_ported(function, NOZZLE_OPTION_MISSING),
    }
}

/// `Slicing::min_layer_height_from_nozzle(print_config, idx_nozzle)`: the minimum layer height of
/// nozzle `idx_nozzle` (1 based) for the variable layer height algorithm. The index is
/// `opt_float`'s `unsigned int`, so nozzle 0 reads the first value.
///
/// # Panics
///
/// With [`NotPorted`](orca_index::NotPorted) [`NOZZLE_OPTION_MISSING`] where the C++ dereferences
/// a null option or reads an empty vector.
#[must_use]
pub fn min_layer_height_from_nozzle(print_config: &Config, idx_nozzle: i32) -> f64 {
    let min_layer_height = opt_float(
        print_config,
        "Slicing::min_layer_height_from_nozzle",
        "min_layer_height",
        idx_nozzle.wrapping_sub(1) as u32,
    );
    if min_layer_height == 0.0 {
        MIN_LAYER_HEIGHT_DEFAULT
    } else {
        max(MIN_LAYER_HEIGHT, min_layer_height)
    }
}

/// `Slicing::max_layer_height_from_nozzle(print_config, idx_nozzle)`: the maximum layer height of
/// nozzle `idx_nozzle` (1 based) for the variable layer height algorithm, 3/4 of the nozzle by
/// default, never below the minimum.
///
/// # Panics
///
/// As [`min_layer_height_from_nozzle`].
#[must_use]
pub fn max_layer_height_from_nozzle(print_config: &Config, idx_nozzle: i32) -> f64 {
    let min_layer_height = min_layer_height_from_nozzle(print_config, idx_nozzle);
    let function = "Slicing::max_layer_height_from_nozzle";
    let idx = idx_nozzle.wrapping_sub(1) as u32;
    let max_layer_height = opt_float(print_config, function, "max_layer_height", idx);
    let nozzle_dmr = opt_float(print_config, function, "nozzle_diameter", idx);
    max(
        min_layer_height,
        if max_layer_height == 0.0 {
            0.75 * nozzle_dmr
        } else {
            max_layer_height
        },
    )
}

impl SlicingParameters {
    /// `SlicingParameters::create_from_config(print_config, object_config, object_height,
    /// object_extruders, object_shrinkage_compensation)`: the parameters of an object of
    /// `object_height` (unscaled, before the shrinkage compensation) printed with
    /// `object_extruders` (0 based, as `PrintObject::object_extruders` gives them; each is read as
    /// a 1-based nozzle, as the C++ reads it).
    #[must_use]
    pub fn create_from_config(
        print_config: &PrintConfig,
        object_config: &PrintObjectConfig,
        object_height: f64,
        object_extruders: &[u32],
        object_shrinkage_compensation: &Vec3d,
    ) -> Self {
        let initial_layer_print_height = if print_config.initial_layer_print_height.value <= 0.0 {
            object_config.layer_height.value
        } else {
            print_config.initial_layer_print_height.value
        };

        // If object_config.support_filament == 0 resp. object_config.support_interface_filament
        // == 0, print_config.nozzle_diameter.get_at(size_t(-1)) returns the 0th nozzle diameter,
        // which is consistent with the requirement that if support_filament == 0 resp.
        // support_interface_filament == 0, support will not trigger tool change, but it will use
        // the current nozzle instead. In that case all the nozzles have to be of the same
        // diameter.
        let support_material_extruder_dmr = get_at(
            &print_config.nozzle_diameter,
            size_t(object_config.support_filament.value.wrapping_sub(1)),
        );
        let support_material_interface_extruder_dmr = get_at(
            &print_config.nozzle_diameter,
            size_t(
                object_config
                    .support_interface_filament
                    .value
                    .wrapping_sub(1),
            ),
        );

        // ORCA: store Z distance
        let support_top_z_gap = object_config.support_top_z_distance.value;
        let support_bottom_z_gap = object_config.support_bottom_z_distance.value;
        let raft_z_gap = object_config.raft_contact_distance.value;

        // ORCA: Zero-gap interface detection (asymmetric)
        let zero_top_z_contact = support_top_z_gap == 0.0;

        let zero_gap_interface_top =
            // Has some top interface layers
            object_config.support_interface_top_layers.value > 0 && zero_top_z_contact;

        let zero_gap_interface_bottom =
            // Negative value means "use same as top"
            (if object_config.support_interface_bottom_layers.value < 0 {
                object_config.support_interface_top_layers.value
            } else {
                object_config.support_interface_bottom_layers.value
            }) > 0 // Has some bottom interface layers
                && (support_bottom_z_gap == 0.0 || zero_top_z_contact);

        let zero_gap_interface_raft = raft_z_gap == 0.0 || zero_top_z_contact;

        let mut params = SlicingParameters {
            layer_height: object_config.layer_height.value,
            first_print_layer_height: initial_layer_print_height,
            first_object_layer_height: initial_layer_print_height,
            object_print_z_min: 0.0,
            // Orca: XYZ filament compensation
            object_print_z_max: object_height * object_shrinkage_compensation.z(),
            object_print_z_uncompensated_max: object_height,
            object_shrinkage_compensation_z: object_shrinkage_compensation.z(),
            base_raft_layers: size_t(object_config.raft_layers.value),
            zero_gap_interface_top,
            zero_gap_interface_bottom,
            zero_gap_interface_raft,
            ..SlicingParameters::default()
        };

        // Miniumum/maximum of the minimum layer height over all extruders.
        params.min_layer_height = MIN_LAYER_HEIGHT;
        params.max_layer_height = f64::MAX;
        if object_config.enable_support.value
            || params.base_raft_layers > 0
            || object_config.enforce_support_layers.value > 0
        {
            // Has some form of support. Add the support layers to the minimum / maximum layer
            // height limits.
            params.min_layer_height = max(
                print_min_layer_height_from_nozzle(
                    print_config,
                    object_config.support_filament.value,
                ),
                print_min_layer_height_from_nozzle(
                    print_config,
                    object_config.support_interface_filament.value,
                ),
            );
            params.max_layer_height = min(
                print_max_layer_height_from_nozzle(
                    print_config,
                    object_config.support_filament.value,
                ),
                print_max_layer_height_from_nozzle(
                    print_config,
                    object_config.support_interface_filament.value,
                ),
            );
            params.max_suport_layer_height = params.max_layer_height;
        }

        if object_extruders.is_empty() {
            params.min_layer_height = max(
                params.min_layer_height,
                print_min_layer_height_from_nozzle(print_config, 0),
            );
            params.max_layer_height = min(
                params.max_layer_height,
                print_max_layer_height_from_nozzle(print_config, 0),
            );
        } else {
            for &extruder_id in object_extruders {
                // The `unsigned int` passed as the `int` nozzle index.
                let idx = extruder_id as i32;
                params.min_layer_height = max(
                    params.min_layer_height,
                    print_min_layer_height_from_nozzle(print_config, idx),
                );
                params.max_layer_height = min(
                    params.max_layer_height,
                    print_max_layer_height_from_nozzle(print_config, idx),
                );
            }
        }

        params.min_layer_height = min(params.min_layer_height, params.layer_height);
        params.max_layer_height = max(params.max_layer_height, params.layer_height);

        // ORCA: Gap assignment
        let layer_height = object_config.layer_height.value;
        let independent = print_config.independent_support_layer_height.value;

        // ORCA: Raft contact (raft -> object)
        if zero_gap_interface_raft {
            params.gap_raft_object = 0.0;
        } else {
            params.gap_raft_object = raft_z_gap;
            if !independent {
                params.gap_raft_object =
                    orca_crt::round(params.gap_raft_object / layer_height + EPSILON) * layer_height;
            }
        }

        // ORCA: BOTTOM contact (object -> support)
        if zero_gap_interface_bottom {
            params.gap_object_support = 0.0;
        } else {
            params.gap_object_support = support_bottom_z_gap;
            if !independent {
                params.gap_object_support =
                    orca_crt::round(params.gap_object_support / layer_height + EPSILON)
                        * layer_height;
            }
        }

        // ORCA: TOP contact (support -> object)
        if zero_gap_interface_top {
            params.gap_support_object = 0.0;
        } else {
            params.gap_support_object = support_top_z_gap;
            if !independent {
                params.gap_support_object =
                    orca_crt::round(params.gap_support_object / layer_height + EPSILON)
                        * layer_height;
            }
        }

        // Raft logic
        if params.base_raft_layers > 0 {
            params.interface_raft_layers = params.base_raft_layers.wrapping_add(1) / 2;
            params.base_raft_layers = params
                .base_raft_layers
                .wrapping_sub(params.interface_raft_layers);
            // Use as large as possible layer height for the intermediate raft layers.
            params.base_raft_layer_height =
                max(params.layer_height, 0.75 * support_material_extruder_dmr);
            params.interface_raft_layer_height = max(
                params.layer_height,
                0.75 * support_material_interface_extruder_dmr,
            );
            params.first_object_layer_bridging = false;
            params.contact_raft_layer_height = max(
                params.layer_height,
                0.75 * support_material_interface_extruder_dmr,
            );
            params.first_object_layer_height = params.layer_height;
        }

        if params.has_raft() {
            // Raise first object layer Z by the thickness of the raft itself plus the extra
            // distance required by the support material logic.
            // FIXME The last raft layer is the contact layer, which shall be printed with a
            // bridging flow for ease of separation. Currently it is not the case.
            if params.raft_layers() == 1 {
                // There is only the contact layer.
                params.contact_raft_layer_height = initial_layer_print_height;
                params.raft_contact_top_z = initial_layer_print_height;
            } else {
                // Number of the base raft layers is decreased by the first layer.
                params.raft_base_top_z = initial_layer_print_height
                    + params.base_raft_layers.wrapping_sub(1) as f64
                        * params.base_raft_layer_height;
                // Number of the interface raft layers is decreased by the contact layer.
                params.raft_interface_top_z = params.raft_base_top_z
                    + params.interface_raft_layers.wrapping_sub(1) as f64
                        * params.interface_raft_layer_height;
                params.raft_contact_top_z =
                    params.raft_interface_top_z + params.contact_raft_layer_height;
            }

            let print_z = params.raft_contact_top_z + params.gap_raft_object;
            params.object_print_z_min = print_z;
            params.object_print_z_max += print_z;
            params.object_print_z_uncompensated_max += print_z;
        }

        params.valid = true;
        params
    }

    /// `has_raft()`: whether there is any raft layer.
    #[must_use]
    pub const fn has_raft(&self) -> bool {
        self.raft_layers() > 0
    }

    /// `raft_layers()`: the base and interface raft layers.
    #[must_use]
    pub const fn raft_layers(&self) -> usize {
        self.base_raft_layers
            .wrapping_add(self.interface_raft_layers)
    }

    /// `first_object_layer_height_fixed()`: whether the first object layer's height is fixed,
    /// not free to vary.
    #[must_use]
    pub const fn first_object_layer_height_fixed(&self) -> bool {
        !self.has_raft() || self.first_object_layer_bridging
    }

    /// `object_print_z_height()`: the height of the printed object, without the raft.
    #[must_use]
    pub fn object_print_z_height(&self) -> f64 {
        self.object_print_z_max - self.object_print_z_min
    }

    /// `object_print_z_uncompensated_height()`: the height of the printed object, without the
    /// raft and not scaled by the shrinkage compensation in Z.
    #[must_use]
    pub fn object_print_z_uncompensated_height(&self) -> f64 {
        self.object_print_z_uncompensated_max - self.object_print_z_min
    }
}

/// `equal_layering(sp1, sp2)`: whether the two parameters lead to the same layering as long as
/// the variable layer thickness is not in action. The support's maximum layer height and (BBS) the
/// zero-gap flags and the gaps are not compared, since a z gap may differ by multiples of the
/// layer height.
#[must_use]
pub fn equal_layering(sp1: &SlicingParameters, sp2: &SlicingParameters) -> bool {
    sp1.base_raft_layers == sp2.base_raft_layers
        && sp1.interface_raft_layers == sp2.interface_raft_layers
        && sp1.base_raft_layer_height == sp2.base_raft_layer_height
        && sp1.interface_raft_layer_height == sp2.interface_raft_layer_height
        && sp1.contact_raft_layer_height == sp2.contact_raft_layer_height
        && sp1.layer_height == sp2.layer_height
        && sp1.min_layer_height == sp2.min_layer_height
        && sp1.max_layer_height == sp2.max_layer_height
        && sp1.first_print_layer_height == sp2.first_print_layer_height
        && sp1.first_object_layer_height == sp2.first_object_layer_height
        && sp1.first_object_layer_bridging == sp2.first_object_layer_bridging
        && sp1.raft_base_top_z == sp2.raft_base_top_z
        && sp1.raft_interface_top_z == sp2.raft_interface_top_z
        && sp1.raft_contact_top_z == sp2.raft_contact_top_z
        && sp1.object_print_z_min == sp2.object_print_z_min
}

/// A height range's `layer_height`: `range.option("layer_height")->getFloat()`.
fn range_layer_height(config: &orca_model::config::ModelConfig) -> f64 {
    match config.option("layer_height").map(ConfigOption::get_float) {
        Some(Ok(height)) => height,
        _ => not_ported(
            "layer_height_profile_from_ranges",
            RANGE_WITHOUT_LAYER_HEIGHT,
        ),
    }
}

/// `layer_height_profile_from_ranges(slicing_params, layer_config_ranges)`: the layer height
/// profile, pairs of `z` and layer height, of the height ranges (sorted by their low and high
/// `z`, as the C++'s map holds them). Both are referenced to z = 0: the raft layers are not
/// counted, and the object may be lifted by the raft when the G-code is made.
///
/// # Panics
///
/// With [`NotPorted`](orca_index::NotPorted) [`RANGE_WITHOUT_LAYER_HEIGHT`] for a range whose
/// config holds no float `layer_height`.
#[must_use]
pub fn layer_height_profile_from_ranges(
    slicing_params: &SlicingParameters,
    layer_config_ranges: &LayerConfigRanges,
) -> Vec<f64> {
    // 1) If there are any height ranges, trim one by the other to make them non-overlapping.
    // Insert the 1st layer if fixed.
    let mut ranges_non_overlapping: Vec<(LayerHeightRange, f64)> =
        Vec::with_capacity(layer_config_ranges.len() * 4);
    if slicing_params.first_object_layer_height_fixed() {
        ranges_non_overlapping.push((
            (0.0, slicing_params.first_object_layer_height),
            slicing_params.first_object_layer_height,
        ));
    }
    // The height ranges are sorted lexicographically by low / high layer boundaries.
    for (range, config) in layer_config_ranges {
        let mut lo = range.0;
        let hi = min(range.1, slicing_params.object_print_z_height());
        let height = range_layer_height(config);
        if let Some(last) = ranges_non_overlapping.last() {
            // Trim current low with the last high.
            lo = max(lo, last.0.1);
        }
        if lo + EPSILON < hi {
            // Ignore too narrow ranges.
            ranges_non_overlapping.push(((lo, hi), height));
        }
    }

    // 2) Convert the trimmed ranges to a height profile, fill in the undefined intervals between
    // z=0 and z=slicing_params.object_print_z_max() with slicing_params.layer_height
    let mut layer_height_profile: Vec<f64> = Vec::new();
    let last_z = |profile: &Vec<f64>| -> f64 {
        if profile.is_empty() {
            0.0
        } else {
            profile[profile.len() - 2]
        }
    };
    let lh_append = |profile: &mut Vec<f64>, z: f64, layer_height: f64| {
        if !profile.is_empty() {
            let n = profile.len();
            let last_z_matches = is_approx(profile[n - 2], z);
            let last_h_matches = is_approx(profile[n - 1], layer_height);
            if last_h_matches {
                if last_z_matches {
                    // Drop a duplicate.
                    return;
                }
                if n >= 4 && is_approx(profile[n - 3], layer_height) {
                    // Third repetition of the same layer_height. Update z of the last entry.
                    profile[n - 2] = z;
                    return;
                }
            }
        }
        profile.push(z);
        profile.push(layer_height);
    };

    for &((lo, hi), height) in &ranges_non_overlapping {
        let z = last_z(&layer_height_profile);
        if lo > z + EPSILON {
            // Insert a step of normal layer height.
            lh_append(&mut layer_height_profile, z, slicing_params.layer_height);
            lh_append(&mut layer_height_profile, lo, slicing_params.layer_height);
        }
        // Insert a step of the overriden layer height.
        lh_append(&mut layer_height_profile, lo, height);
        lh_append(&mut layer_height_profile, hi, height);
    }

    let z = last_z(&layer_height_profile);
    if z < slicing_params.object_print_z_uncompensated_height() {
        // Insert a step of normal layer height up to the object top.
        lh_append(&mut layer_height_profile, z, slicing_params.layer_height);
        lh_append(
            &mut layer_height_profile,
            slicing_params.object_print_z_uncompensated_height(),
            slicing_params.layer_height,
        );
    }

    layer_height_profile
}

/// `HeightProfileSmoothingParams`: the radius and the minimum keeping of
/// `smooth_height_profile` (orca-o3-slice-2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeightProfileSmoothingParams {
    /// `radius`.
    pub radius: u32,
    /// `keep_min`.
    pub keep_min: bool,
}

impl Default for HeightProfileSmoothingParams {
    /// `HeightProfileSmoothingParams()`: a radius of 5, the minimum not kept.
    fn default() -> Self {
        Self {
            radius: 5,
            keep_min: false,
        }
    }
}

impl HeightProfileSmoothingParams {
    /// `HeightProfileSmoothingParams(radius, keep_min)`.
    #[must_use]
    pub const fn new(radius: u32, keep_min: bool) -> Self {
        Self { radius, keep_min }
    }
}

/// `LayerHeightEditActionType`: what `adjust_layer_height_profile` (orca-o3-slice-2) does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum LayerHeightEditActionType {
    /// `LAYER_HEIGHT_EDIT_ACTION_INCREASE`.
    Increase = 0,
    /// `LAYER_HEIGHT_EDIT_ACTION_DECREASE`.
    Decrease = 1,
    /// `LAYER_HEIGHT_EDIT_ACTION_REDUCE`.
    Reduce = 2,
    /// `LAYER_HEIGHT_EDIT_ACTION_SMOOTH`.
    Smooth = 3,
}

impl LayerHeightEditActionType {
    /// The action of its `unsigned int` value, or `None` for a value the enum does not declare.
    #[must_use]
    pub const fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::Increase),
            1 => Some(Self::Decrease),
            2 => Some(Self::Reduce),
            3 => Some(Self::Smooth),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orca_config::ConfigBase;
    use orca_eigen::Vector;
    use orca_model::config::ModelConfig;

    fn config(settings: &[(&str, &str)]) -> (PrintConfig, PrintObjectConfig) {
        let (mut pc, mut oc) = (PrintConfig::default(), PrintObjectConfig::default());
        for (k, v) in settings {
            if orca_config::StaticPrintConfig::optptr(&pc, k).is_some() {
                pc.set_deserialize_strict(k, v).unwrap();
            }
            if orca_config::StaticPrintConfig::optptr(&oc, k).is_some() {
                oc.set_deserialize_strict(k, v).unwrap();
            }
        }
        (pc, oc)
    }

    #[test]
    fn a_raft_lifts_the_object() {
        let (pc, oc) = config(&[
            ("layer_height", "0.2"),
            ("initial_layer_print_height", "0.3"),
            ("raft_layers", "3"),
            ("raft_contact_distance", "0.1"),
        ]);
        let p =
            SlicingParameters::create_from_config(&pc, &oc, 10.0, &[1], &Vector([1.0, 1.0, 1.0]));
        assert!(p.valid && p.has_raft());
        assert_eq!((p.base_raft_layers, p.interface_raft_layers), (1, 2));
        assert!(!p.first_object_layer_height_fixed());
        assert_eq!(p.first_object_layer_height, 0.2);
        assert!(p.object_print_z_min > 0.3);
        assert_eq!(p.object_print_z_max, 10.0 + p.object_print_z_min);
    }

    #[test]
    fn ranges_fill_the_gaps_with_the_layer_height() {
        let (pc, oc) = config(&[
            ("layer_height", "0.2"),
            ("initial_layer_print_height", "0.25"),
        ]);
        let p =
            SlicingParameters::create_from_config(&pc, &oc, 5.0, &[1], &Vector([1.0, 1.0, 1.0]));
        let mut c = ModelConfig::new();
        c.set_key_value("layer_height", OptionValue::Float(0.1));
        let ranges: LayerConfigRanges = vec![((1.0, 2.0), c)];
        let profile = layer_height_profile_from_ranges(&p, &ranges);
        assert_eq!(
            profile,
            vec![
                0.0, 0.25, 0.25, 0.25, 0.25, 0.2, 1.0, 0.2, 1.0, 0.1, 2.0, 0.1, 2.0, 0.2, 5.0, 0.2
            ]
        );
    }

    #[test]
    fn a_range_without_a_layer_height_stops() {
        let p = SlicingParameters {
            object_print_z_max: 3.0,
            object_print_z_uncompensated_max: 3.0,
            ..SlicingParameters::default()
        };
        let ranges: LayerConfigRanges = vec![((1.0, 2.0), ModelConfig::new())];
        let caught = std::panic::catch_unwind(|| layer_height_profile_from_ranges(&p, &ranges));
        let stop = caught
            .unwrap_err()
            .downcast::<orca_index::NotPorted>()
            .unwrap();
        assert_eq!(stop.condition, RANGE_WITHOUT_LAYER_HEIGHT);
    }
}
