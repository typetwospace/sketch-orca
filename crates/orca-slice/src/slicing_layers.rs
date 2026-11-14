// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Slicing.cpp lines 307-901 (layer_height_profile_adaptive,
// smooth_height_profile, adjust_layer_height_profile, adjust_layer_series_to_align_object_height,
// generate_object_layers and check_object_layers_fixed), with lerp of src/libslic3r/libslic3r.h,
// of OrcaSlicer v2.4.2, translated on 2026-11-14 by orca-o3-slice-2. Copyright the OrcaSlicer
// authors, the BambuStudio authors (Bambu Lab), the PrusaSlicer authors (Prusa Research) and the
// Slic3r authors (Alessandro Ranellucci), under the GNU Affero General Public License, version 3
// (AGPL-3.0; LICENSE).

//! The layer height profile and the layer series of an object (the second half of `Slicing.cpp`):
//! a profile is pairs of `z` and layer height, from z = 0 to the object's top; the layer series is
//! pairs of each layer's low and high `z`.
//!
//! - [`layer_height_profile_adaptive`] steps up the object with [`SlicingAdaptive`]'s height at
//!   each `print_z` (as `float`, as the C++ keeps it), each step at most
//!   `LAYER_HEIGHT_CHANGE_STEP` (0.04) away from the last (BBS), and a height range's own
//!   `layer_height` where `print_z` is inside it (both ends counted); a last step closes the gap
//!   to the top, its height clamped between the limits (`std::clamp`, orca-stl).
//! - [`smooth_height_profile`] runs a gaussian blur six times (BBS; the steep-change test is
//!   commented out in the C++), each height the weighted mean of its neighbours within the radius,
//!   biased towards the minimum layer height, clamped, and kept at or under the old one when
//!   asked; the first layer stays when it is fixed, and a profile of fewer than six values past it
//!   is given back as it is.
//! - [`adjust_layer_height_profile`] is the layer height editor's brush: the profile resampled
//!   every 0.1 mm across the band about `z`, each height moved by a cosine weight (increase,
//!   decrease, reduce towards the layer height) or then smoothed six rounds (smooth). Its indices
//!   are `size_t` and wrap as the C++'s do (`idx -= 2` below 0 is `SIZE_MAX - 1`, and `idx + 2`
//!   is 0 again); a read past the profile, where the C++ reads out of the vector (a band that
//!   starts at the bottom of a profile whose first layer is not fixed, an odd count), stops with
//!   [`PROFILE_INDEX_OUT_OF_RANGE`].
//! - [`generate_object_layers`] walks the profile (its `z` scaled by the Z shrinkage compensation,
//!   Orca) to make the layer series, then, when the Z height is to be precise,
//!   [`adjust_layer_series_to_align_object_height`] moves the top of the last five layers to the
//!   object's height (BBS). As in the C++, a layer that can no longer be adjusted is still moved by
//!   the share of the gap in the loop's `else`; and the C++'s `abs` of a `double` is MSVC's
//!   `double` overload (`fabs`).
//! - [`check_object_layers_fixed`]: whether a profile of four or eight values is the fixed one
//!   `generate_object_layers` would make from the first layer and the layer height.
//!
//! `lerp` (`libslic3r.h`) belongs in orca-geom; no piece carries it there (orca-layer has its own
//! copy, outside this crate's dependencies), so it is translated here. A range without a float
//! `layer_height` stops with [`RANGE_WITHOUT_LAYER_HEIGHT`] where the C++'s `opt_float`
//! dereferences a null `dynamic_cast`.

use orca_config::OptionValue;
use orca_config::option::is_approx;
use orca_crt::{cos, exp, fabs, sqrt};
use orca_geom::point::{EPSILON, PI};
use orca_index::stop::not_ported;
use orca_model::config::ModelConfig;
use orca_model::model::ModelObject;
use orca_stl::{clamp, max, min};

use crate::slicing::{
    HeightProfileSmoothingParams, LAYER_HEIGHT_CHANGE_STEP, LayerHeightEditActionType,
    RANGE_WITHOUT_LAYER_HEIGHT, SlicingParameters,
};
use crate::slicing_adaptive::SlicingAdaptive;

/// The stop of [`adjust_layer_height_profile`], [`smooth_height_profile`] and
/// [`generate_object_layers`] where the C++ reads or writes a profile out of its range.
pub const PROFILE_INDEX_OUT_OF_RANGE: &str = "profile-index-out-of-range";

/// `lerp(a, b, t)` of `libslic3r.h`: `(1 - t) * a + t * b` (its assert does not run). Belongs in
/// orca-geom.
#[must_use]
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    (1. - t) * a + t * b
}

/// `profile[i]`, or the stop where the C++ reads past the vector (an index that wrapped included).
fn at(function: &'static str, profile: &[f64], i: usize) -> f64 {
    match profile.get(i) {
        Some(&v) => v,
        None => not_ported(function, PROFILE_INDEX_OUT_OF_RANGE),
    }
}

/// A height range's `options.opt_float("layer_height")`: the value of a `ConfigOptionFloat` (or
/// of a type derived from it, a percent or a float-or-percent), as the C++'s `dynamic_cast` reads
/// it.
fn range_opt_float(function: &'static str, config: &ModelConfig) -> f64 {
    match config.option("layer_height") {
        Some(
            OptionValue::Float(v)
            | OptionValue::Percent(v)
            | OptionValue::FloatOrPercent { value: v, .. },
        ) => *v,
        _ => not_ported(function, RANGE_WITHOUT_LAYER_HEIGHT),
    }
}

/// `layer_height_profile_adaptive(slicing_params, object, quality_factor)`, based on the work of
/// @platsch: the layer height profile of the object filled by heights ensuring a prescribed
/// maximum cusp height.
///
/// # Panics
///
/// With [`NotPorted`](orca_index::NotPorted) where [`SlicingAdaptive::prepare`] stops (an object
/// without an instance), or [`RANGE_WITHOUT_LAYER_HEIGHT`] for a height range the layers reach
/// whose config holds no float `layer_height`.
#[must_use]
pub fn layer_height_profile_adaptive(
    slicing_params: &SlicingParameters,
    object: &ModelObject,
    quality_factor: f32,
) -> Vec<f64> {
    // 1) Initialize the SlicingAdaptive class with the object meshes.
    let mut adaptive = SlicingAdaptive::default();
    adaptive.set_slicing_parameters(*slicing_params);
    adaptive.prepare(object);

    // 2) Generate layers using the algorithm of @platsch
    let mut layer_height_profile: Vec<f64> = vec![0.0, slicing_params.first_object_layer_height];
    if slicing_params.first_object_layer_height_fixed() {
        layer_height_profile.push(slicing_params.first_object_layer_height);
        layer_height_profile.push(slicing_params.first_object_layer_height);
    }
    let mut print_z = slicing_params.first_object_layer_height;
    // last facet visited by the as.next_layer_height() function, where the facets are sorted by
    // their increasing Z span.
    let mut current_facet: usize = 0;
    // loop until we have at least one layer and the max slice_z reaches the object height
    while print_z + EPSILON < slicing_params.object_print_z_uncompensated_height() {
        let mut height = slicing_params.max_layer_height as f32;
        // determine next layer height
        let cusp_height =
            adaptive.next_layer_height(print_z as f32, quality_factor, &mut current_facet);

        height = min(cusp_height, height);

        //BBS: avoid the layer height change to be too steep
        let back = layer_height_profile[layer_height_profile.len() - 1];
        if back < f64::from(height) && f64::from(height) - back > LAYER_HEIGHT_CHANGE_STEP {
            height = (back + LAYER_HEIGHT_CHANGE_STEP) as f32;
        } else if back > f64::from(height) && back - f64::from(height) > LAYER_HEIGHT_CHANGE_STEP {
            height = (back - LAYER_HEIGHT_CHANGE_STEP) as f32;
        }

        for (range, options) in &object.layer_config_ranges {
            if print_z >= range.0 && print_z <= range.1 {
                height = range_opt_float("layer_height_profile_adaptive", options) as f32;
                break;
            }
        }

        layer_height_profile.push(print_z);
        layer_height_profile.push(f64::from(height));
        print_z += f64::from(height);
    }

    let z_gap = slicing_params.object_print_z_uncompensated_height()
        - layer_height_profile[layer_height_profile.len() - 2];
    if z_gap > 0.0 {
        layer_height_profile.push(slicing_params.object_print_z_uncompensated_height());
        layer_height_profile.push(clamp(
            z_gap,
            slicing_params.min_layer_height,
            slicing_params.max_layer_height,
        ));
    }

    layer_height_profile
}

/// The lambda `gauss_kernel(radius)` of `smooth_height_profile`, reworked from
/// `getGaussianKernelSize(float sigma)` of OpenCV's `AKAZEFeatures.cpp`.
fn gauss_kernel(radius: u32) -> Vec<f64> {
    let size = radius.wrapping_mul(2).wrapping_add(1);
    let mut ret: Vec<f64> = Vec::with_capacity(size as usize);

    let sigma = 0.3 * f64::from(radius.wrapping_sub(1)) + 0.8;
    let two_sq_sigma = 2.0 * sigma * sigma;
    let inv_root_two_pi_sq_sigma = 1.0 / sqrt(PI * two_sq_sigma);

    for i in 0..size {
        let x = f64::from(i) - f64::from(radius);
        ret.push(inv_root_two_pi_sq_sigma * exp(-x * x / two_sq_sigma));
    }

    ret
}

/// The lambda `gauss_blur(profile, smoothing_params)` of `smooth_height_profile`: one round.
fn gauss_blur(
    profile: &[f64],
    slicing_params: &SlicingParameters,
    smoothing_params: &HeightProfileSmoothingParams,
) -> Vec<f64> {
    const F: &str = "smooth_height_profile";
    // skip first layer ?
    let skip_count: usize = if slicing_params.first_object_layer_height_fixed() {
        4
    } else {
        0
    };

    // not enough data to smmoth
    if (profile.len() as i32).wrapping_sub(skip_count as i32) < 6 {
        return profile.to_vec();
    }

    let radius: u32 = max(smoothing_params.radius, 1);
    let kernel = gauss_kernel(radius);
    let two_radius: i32 = 2_i32.wrapping_mul(radius as i32);

    let size = profile.len();
    let mut ret: Vec<f64> = Vec::with_capacity(size);

    // leave first layer untouched
    ret.extend_from_slice(&profile[..skip_count]);

    // smooth the rest of the profile by biasing a gaussian blur
    // the bias moves the smoothed profile closer to the min_layer_height
    let delta_h = slicing_params.max_layer_height - slicing_params.min_layer_height;
    let inv_delta_h = if delta_h != 0.0 { 1.0 / delta_h } else { 1.0 };

    let max_dz_band = f64::from(radius) * slicing_params.layer_height;
    let mut i = skip_count;
    while i < size {
        let zi = at(F, profile, i);
        let hi = at(F, profile, i + 1);
        ret.push(zi);
        ret.push(0.0);
        let mut height = 0.0;
        let begin = max((i as i32).wrapping_sub(two_radius), skip_count as i32);
        let end = min(
            (i as i32).wrapping_add(two_radius),
            (size as i32).wrapping_sub(2),
        );
        let mut weight_total = 0.0;
        let mut j = begin;
        while j <= end {
            let kernel_id = (radius as i32).wrapping_add(j.wrapping_sub(i as i32) / 2);
            let dz = fabs(zi - at(F, profile, j as usize));
            if dz * slicing_params.layer_height <= max_dz_band {
                let hj = at(F, profile, j as usize + 1);
                let dh = fabs(slicing_params.max_layer_height - hj);
                let weight = kernel[kernel_id as usize] * sqrt(dh * inv_delta_h);
                height += weight * hj;
                weight_total += weight;
            }
            j += 2;
        }

        height = clamp(
            if weight_total == 0.0 {
                hi
            } else {
                height / weight_total
            },
            slicing_params.min_layer_height,
            slicing_params.max_layer_height,
        );
        if smoothing_params.keep_min {
            height = min(height, hi);
        }
        let last = ret.len() - 1;
        ret[last] = height;
        i += 2;
    }

    ret
}

/// `smooth_height_profile(profile, slicing_params, smoothing_params)`: the profile blurred six
/// times (see the module documentation).
///
/// # Panics
///
/// With [`NotPorted`](orca_index::NotPorted) [`PROFILE_INDEX_OUT_OF_RANGE`] for a profile of an
/// odd count, where the C++ reads past it.
#[must_use]
pub fn smooth_height_profile(
    profile: &[f64],
    slicing_params: &SlicingParameters,
    smoothing_params: &HeightProfileSmoothingParams,
) -> Vec<f64> {
    //BBS: avoid the layer height change to be too steep
    let mut count = 0;
    let mut ret: Vec<f64> = profile.to_vec();
    while count < 6 {
        ret = gauss_blur(&ret, slicing_params, smoothing_params);
        count += 1;
    }
    ret
}

/// `adjust_layer_height_profile(model_object, slicing_params, layer_height_profile, z,
/// layer_thickness_delta, band_width, action)`: the layer height editor's change of the profile
/// about `z` (see the module documentation). Nothing changes when `z` is outside the variable
/// span (above a fixed first layer, below the top), inside a height range by one layer height,
/// or when the delta cannot be applied.
///
/// # Panics
///
/// With [`NotPorted`](orca_index::NotPorted) [`PROFILE_INDEX_OUT_OF_RANGE`] where the C++ reads or
/// writes the profile out of its range.
#[allow(clippy::too_many_lines)]
pub fn adjust_layer_height_profile(
    model_object: &ModelObject,
    slicing_params: &SlicingParameters,
    layer_height_profile: &mut Vec<f64>,
    z: f64,
    layer_thickness_delta: f64,
    band_width: f64,
    action: LayerHeightEditActionType,
) {
    const F: &str = "adjust_layer_height_profile";
    let mut layer_thickness_delta = layer_thickness_delta;
    // Constrain the profile variability by the 1st layer height.
    let z_span_variable: (f64, f64) = (
        if slicing_params.first_object_layer_height_fixed() {
            slicing_params.first_object_layer_height
        } else {
            0.
        },
        slicing_params.object_print_z_uncompensated_height(),
    );
    if z < z_span_variable.0 || z > z_span_variable.1 {
        return;
    }

    // 1) Get the current layer thickness at z.
    let mut current_layer_height = slicing_params.layer_height;
    let mut i = 0usize;
    while i < layer_height_profile.len() {
        if i.wrapping_add(2) == layer_height_profile.len() {
            current_layer_height = at(F, layer_height_profile, i + 1);
            break;
        } else if at(F, layer_height_profile, i.wrapping_add(2)) > z {
            let z1 = at(F, layer_height_profile, i);
            let h1 = at(F, layer_height_profile, i + 1);
            let z2 = at(F, layer_height_profile, i + 2);
            let h2 = at(F, layer_height_profile, i + 3);
            current_layer_height = lerp(h1, h2, (z - z1) / (z2 - z1));
            break;
        }
        i += 2;
    }

    for (range, _options) in &model_object.layer_config_ranges {
        if z >= range.0 - current_layer_height && z <= range.1 + current_layer_height {
            return;
        }
    }

    // 2) Is it possible to apply the delta?
    match action {
        LayerHeightEditActionType::Decrease | LayerHeightEditActionType::Increase => {
            if action == LayerHeightEditActionType::Decrease {
                layer_thickness_delta = -layer_thickness_delta;
            }
            // fallthrough
            if layer_thickness_delta > 0.0 {
                if current_layer_height >= slicing_params.max_layer_height - EPSILON {
                    return;
                }
                layer_thickness_delta = min(
                    layer_thickness_delta,
                    slicing_params.max_layer_height - current_layer_height,
                );
            } else {
                if current_layer_height <= slicing_params.min_layer_height + EPSILON {
                    return;
                }
                layer_thickness_delta = max(
                    layer_thickness_delta,
                    slicing_params.min_layer_height - current_layer_height,
                );
            }
        }
        LayerHeightEditActionType::Reduce | LayerHeightEditActionType::Smooth => {
            layer_thickness_delta = fabs(layer_thickness_delta);
            layer_thickness_delta = min(
                layer_thickness_delta,
                fabs(slicing_params.layer_height - current_layer_height),
            );
            if layer_thickness_delta < EPSILON {
                return;
            }
        }
    }

    // 3) Densify the profile inside z +- band_width/2, remove duplicate Zs from the height profile
    // inside the band.
    let lo = max(z_span_variable.0, z - 0.5 * band_width);
    // Do not limit the upper side of the band, so that the modifications to the top point of the
    // profile will be allowed.
    let hi = z + 0.5 * band_width;
    let z_step = 0.1;
    let mut idx: usize = 0;
    while idx < layer_height_profile.len() && layer_height_profile[idx] < lo {
        idx += 2;
    }
    idx = idx.wrapping_sub(2);

    let mut profile_new: Vec<f64> = Vec::with_capacity(layer_height_profile.len());
    match layer_height_profile.get(..idx.wrapping_add(2)) {
        Some(head) => profile_new.extend_from_slice(head),
        None => not_ported(F, PROFILE_INDEX_OUT_OF_RANGE),
    }
    let mut zz = lo;
    let mut i_resampled_start = profile_new.len();
    while zz < hi {
        let next = idx.wrapping_add(2);
        let z1 = at(F, layer_height_profile, idx);
        let h1 = at(F, layer_height_profile, idx.wrapping_add(1));
        let mut height = h1;
        if next < layer_height_profile.len() {
            let z2 = at(F, layer_height_profile, next);
            let h2 = at(F, layer_height_profile, next + 1);
            height = lerp(h1, h2, (zz - z1) / (z2 - z1));
        }
        // Adjust height by layer_thickness_delta.
        let weight = if fabs(zz - z) < 0.5 * band_width {
            0.5 + 0.5 * cos(2. * PI * (zz - z) / band_width)
        } else {
            0.
        };
        match action {
            LayerHeightEditActionType::Increase | LayerHeightEditActionType::Decrease => {
                height += weight * layer_thickness_delta;
            }
            LayerHeightEditActionType::Reduce => {
                let delta = height - slicing_params.layer_height;
                let mut step = weight * layer_thickness_delta;
                step = if fabs(delta) > step {
                    if delta > 0.0 { -step } else { step }
                } else {
                    -delta
                };
                height += step;
            }
            LayerHeightEditActionType::Smooth => {
                // Don't modify the profile during resampling process, do it at the next step.
            }
        }
        height = clamp(
            height,
            slicing_params.min_layer_height,
            slicing_params.max_layer_height,
        );
        if zz == z_span_variable.1 {
            // This is the last point of the profile.
            if at(F, &profile_new, profile_new.len().wrapping_sub(2)) + EPSILON > zz {
                profile_new.pop();
                profile_new.pop();
            }
            profile_new.push(zz);
            profile_new.push(height);
            idx = layer_height_profile.len();
            break;
        }
        // Avoid entering a too short segment.
        if at(F, &profile_new, profile_new.len().wrapping_sub(2)) + EPSILON < zz {
            profile_new.push(zz);
            profile_new.push(height);
        }
        // Limit zz to the object height, so the next iteration the last profile point will be
        // set.
        zz = min(zz + z_step, z_span_variable.1);
        idx = next;
        while idx < layer_height_profile.len() && layer_height_profile[idx] < zz {
            idx += 2;
        }
        idx = idx.wrapping_sub(2);
    }

    idx = idx.wrapping_add(2);
    let mut i_resampled_end = profile_new.len();
    if idx < layer_height_profile.len() {
        profile_new.extend_from_slice(&layer_height_profile[idx..]);
    } else if at(F, &profile_new, profile_new.len().wrapping_sub(2)) + 0.5 * EPSILON
        < z_span_variable.1
    {
        match layer_height_profile.len().checked_sub(2) {
            Some(start) => profile_new.extend_from_slice(&layer_height_profile[start..]),
            None => not_ported(F, PROFILE_INDEX_OUT_OF_RANGE),
        }
    }
    *layer_height_profile = profile_new;

    if action == LayerHeightEditActionType::Smooth {
        if i_resampled_start == 0 {
            i_resampled_start += 1;
        }
        if i_resampled_end == layer_height_profile.len() {
            i_resampled_end = i_resampled_end.wrapping_sub(2);
        }
        let n_rounds = 6;
        for _ in 0..n_rounds {
            let profile_new = layer_height_profile.clone();
            let mut i = i_resampled_start;
            while i < i_resampled_end {
                let zz = at(F, &profile_new, i);
                let t = if fabs(zz - z) < 0.5 * band_width {
                    0.25 + 0.25 * cos(2. * PI * (zz - z) / band_width)
                } else {
                    0.
                };
                let value = if i == 0 {
                    (1. - t) * at(F, &profile_new, i + 1) + t * at(F, &profile_new, i + 3)
                } else if i + 1 == profile_new.len() {
                    (1. - t) * at(F, &profile_new, i + 1) + t * at(F, &profile_new, i - 1)
                } else {
                    (1. - t) * at(F, &profile_new, i + 1)
                        + 0.5 * t * (at(F, &profile_new, i - 1) + at(F, &profile_new, i + 3))
                };
                match layer_height_profile.get_mut(i + 1) {
                    Some(slot) => *slot = value,
                    None => not_ported(F, PROFILE_INDEX_OUT_OF_RANGE),
                }
                i += 2;
            }
        }
    }
}

/// `adjust_layer_series_to_align_object_height(slicing_params, layer_series)` (BBS): the tops of
/// the last five layers moved so that the last one is at the object's height, each layer kept
/// between the limits; whether the series ends at that height. A series of fewer than twelve
/// values (five layers and the first) is not adjusted.
///
/// An empty series makes the C++ read `back()` of an empty vector: it returns true or false by
/// what it reads there and leaves the series as it is either way; this port returns false.
pub fn adjust_layer_series_to_align_object_height(
    slicing_params: &SlicingParameters,
    layer_series: &mut [f64],
) -> bool {
    let object_height = slicing_params.object_print_z_height();
    let Some(&back) = layer_series.last() else {
        return false;
    };
    if is_approx(back, object_height) {
        return true;
    }

    // need at least 5 + 1(first_layer) layers to adjust the height
    let layer_size = layer_series.len();
    if layer_size < 12 {
        return false;
    }

    let mut last_5_layers_heght: Vec<f64> = (0..5)
        .map(|i| layer_series[layer_size - 10 + 2 * i + 1] - layer_series[layer_size - 10 + 2 * i])
        .collect();

    let mut gap = fabs(back - object_height);
    // to record whether every layer can adjust layer height
    let mut can_adjust = [true; 5];
    let taller_than_object = back < object_height;

    let get_valid_size =
        |can_adjust: &[bool; 5]| -> i32 { can_adjust.iter().map(|&b| i32::from(b)).sum() };

    let mut adjust_layer_height = |gap: f64, can_adjust: &mut [bool; 5]| -> f64 {
        let delta_gap = gap / f64::from(get_valid_size(can_adjust));
        let mut remain_gap = 0.0;
        for (i, l_height) in last_5_layers_heght.iter_mut().enumerate() {
            if taller_than_object {
                if can_adjust[i] && is_approx(*l_height, slicing_params.max_layer_height) {
                    remain_gap += delta_gap;
                    can_adjust[i] = false;
                    continue;
                }

                if can_adjust[i] && *l_height + delta_gap > slicing_params.max_layer_height {
                    remain_gap += *l_height + delta_gap - slicing_params.max_layer_height;
                    *l_height = slicing_params.max_layer_height;
                    can_adjust[i] = false;
                } else {
                    *l_height += delta_gap;
                }
            } else {
                if can_adjust[i] && is_approx(*l_height, slicing_params.min_layer_height) {
                    remain_gap += delta_gap;
                    can_adjust[i] = false;
                    continue;
                }

                if can_adjust[i] && *l_height - delta_gap < slicing_params.min_layer_height {
                    remain_gap += slicing_params.min_layer_height + delta_gap - *l_height;
                    *l_height = slicing_params.min_layer_height;
                    can_adjust[i] = false;
                } else {
                    *l_height -= delta_gap;
                }
            }
        }
        remain_gap
    };

    while gap > 0.0 {
        let valid_size = get_valid_size(&can_adjust);
        if valid_size == 0 {
            // 5 layers can not adjust z within valid layer height
            return false;
        }

        gap = adjust_layer_height(gap, &mut can_adjust);
        if is_approx(gap, 0.0) {
            // adjust succeed
            break;
        }
    }

    for (i, &h) in last_5_layers_heght.iter().enumerate() {
        if i > 0 {
            layer_series[layer_size - 10 + 2 * i] = layer_series[layer_size - 10 + 2 * i - 1];
        }
        layer_series[layer_size - 10 + 2 * i + 1] = layer_series[layer_size - 10 + 2 * i] + h;
    }

    true
}

/// `generate_object_layers(slicing_params, layer_height_profile, is_precise_z_height)`: the object
/// layers as pairs of low / high layer boundaries, stored into a linear vector.
///
/// # Panics
///
/// With [`NotPorted`](orca_index::NotPorted) [`PROFILE_INDEX_OUT_OF_RANGE`] for a profile of an
/// odd count, where the C++ reads past it.
#[must_use]
pub fn generate_object_layers(
    slicing_params: &SlicingParameters,
    layer_height_profile: &[f64],
    is_precise_z_height: bool,
) -> Vec<f64> {
    const F: &str = "generate_object_layers";
    let mut print_z = 0.0;

    let mut out: Vec<f64> = Vec::new();

    if slicing_params.first_object_layer_height_fixed() {
        out.push(0.0);
        print_z = slicing_params.first_object_layer_height;
        out.push(print_z);
    }

    // Orca: XYZ shrinkage compensation
    let shrinkage_compensation_z = slicing_params.object_shrinkage_compensation_z;
    let mut idx_layer_height_profile: usize = 0;
    // loop until we have at least one layer and the max slice_z reaches the object height
    let mut slice_z = print_z + 0.5 * slicing_params.min_layer_height;
    while slice_z < slicing_params.object_print_z_height() {
        let mut height = slicing_params.min_layer_height;
        if idx_layer_height_profile < layer_height_profile.len() {
            let mut next = idx_layer_height_profile + 2;
            loop {
                // Orca: XYZ shrinkage compensation
                if next >= layer_height_profile.len()
                    || slice_z < layer_height_profile[next] * shrinkage_compensation_z
                {
                    break;
                }
                idx_layer_height_profile = next;
                next += 2;
            }
            // Orca: XYZ shrinkage compensation
            let z1 = layer_height_profile[idx_layer_height_profile] * shrinkage_compensation_z;
            let h1 = at(F, layer_height_profile, idx_layer_height_profile + 1);
            height = h1;
            if next < layer_height_profile.len() {
                // Orca: XYZ shrinkage compensation
                let z2 = layer_height_profile[next] * shrinkage_compensation_z;
                let h2 = at(F, layer_height_profile, next + 1);
                height = lerp(h1, h2, (slice_z - z1) / (z2 - z1));
            }
        }
        slice_z = print_z + 0.5 * height;
        if slice_z >= slicing_params.object_print_z_height() {
            break;
        }
        out.push(print_z);
        print_z += height;
        slice_z = print_z + 0.5 * slicing_params.min_layer_height;
        out.push(print_z);
    }

    if is_precise_z_height {
        adjust_layer_series_to_align_object_height(slicing_params, &mut out);
    }
    out
}

/// `check_object_layers_fixed(slicing_params, layer_height_profile)`: whether the layer height
/// profile describes a fixed layer height profile.
///
/// # Panics
///
/// Where the C++ reads past a profile of four or eight values that it asserts on (a profile of
/// fewer than four values never gets past the size test).
#[must_use]
pub fn check_object_layers_fixed(
    slicing_params: &SlicingParameters,
    layer_height_profile: &[f64],
) -> bool {
    let p = layer_height_profile;
    if p.len() != 4 && p.len() != 8 {
        return false;
    }

    let fixed_step1 = is_approx(p[1], p[3]);
    let fixed_step2 = p.len() == 4 || (p[2] == p[4] && is_approx(p[5], p[7]));

    if !fixed_step1 || !fixed_step2 {
        return false;
    }

    if p[2] < 0.5 * slicing_params.first_object_layer_height + EPSILON
        || !is_approx(p[3], slicing_params.first_object_layer_height)
    {
        return false;
    }

    let z_max = p[p.len() - 2];
    let z_2nd = slicing_params.first_object_layer_height + 0.5 * slicing_params.layer_height;
    if z_2nd > z_max {
        return true;
    }
    if z_2nd < p[p.len() - 4] + EPSILON || !is_approx(p[p.len() - 1], slicing_params.layer_height) {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> SlicingParameters {
        SlicingParameters {
            valid: true,
            layer_height: 0.2,
            min_layer_height: 0.07,
            max_layer_height: 0.3,
            first_print_layer_height: 0.25,
            first_object_layer_height: 0.25,
            object_print_z_max: 2.0,
            object_print_z_uncompensated_max: 2.0,
            object_shrinkage_compensation_z: 1.0,
            ..SlicingParameters::default()
        }
    }

    #[test]
    fn a_fixed_profile_gives_even_layers() {
        let sp = params();
        let profile = vec![0.0, 0.25, 0.25, 0.25, 0.25, 0.2, 2.0, 0.2];
        assert!(check_object_layers_fixed(&sp, &profile));
        let layers = generate_object_layers(&sp, &profile, false);
        assert_eq!(&layers[..2], &[0.0, 0.25]);
        assert_eq!(layers.len() % 2, 0);
        for pair in layers[2..].chunks(2) {
            assert!((pair[1] - pair[0] - 0.2).abs() < 1e-9);
        }
        // Each layer starts where the one below it ends.
        for k in (2..layers.len()).step_by(2) {
            assert_eq!(layers[k], layers[k - 1]);
        }
        // Not a fixed profile when the last height differs.
        let uneven = vec![0.0, 0.25, 0.25, 0.25, 0.25, 0.2, 2.0, 0.3];
        assert!(!check_object_layers_fixed(&sp, &uneven));
    }

    #[test]
    fn the_last_layers_are_moved_to_the_top() {
        let sp = SlicingParameters {
            object_print_z_max: 2.1,
            object_print_z_uncompensated_max: 2.1,
            ..params()
        };
        let profile = vec![0.0, 0.25, 0.25, 0.25, 0.25, 0.2, 2.1, 0.2];
        let plain = generate_object_layers(&sp, &profile, false);
        let precise = generate_object_layers(&sp, &profile, true);
        assert_eq!(plain.len(), precise.len());
        assert!(plain.len() >= 12);
        assert!(!is_approx(plain[plain.len() - 1], 2.1));
        assert!(is_approx(precise[precise.len() - 1], 2.1));
        assert_eq!(plain[..plain.len() - 10], precise[..precise.len() - 10]);
    }

    #[test]
    fn smoothing_keeps_the_limits() {
        let sp = params();
        let profile = vec![
            0.0, 0.25, 0.25, 0.25, 0.25, 0.07, 0.5, 0.3, 0.8, 0.07, 1.2, 0.3, 1.6, 0.1, 2.0, 0.1,
        ];
        let smooth =
            smooth_height_profile(&profile, &sp, &HeightProfileSmoothingParams::new(2, false));
        assert_eq!(smooth.len(), profile.len());
        // The fixed first layer stays, and so does every z.
        assert_eq!(&smooth[..4], &profile[..4]);
        for k in (0..profile.len()).step_by(2) {
            assert_eq!(smooth[k], profile[k]);
        }
        for k in (5..profile.len()).step_by(2) {
            assert!(smooth[k] >= sp.min_layer_height && smooth[k] <= sp.max_layer_height);
        }
        // keep_min keeps each height at or under the old one in each round.
        let kept =
            smooth_height_profile(&profile, &sp, &HeightProfileSmoothingParams::new(2, true));
        for k in (5..profile.len()).step_by(2) {
            assert!(kept[k] <= profile[k]);
        }
        // Too short a profile comes back as it is.
        let short = vec![0.0, 0.25, 0.25, 0.25, 0.25, 0.2, 2.0, 0.2];
        assert_eq!(
            smooth_height_profile(&short, &sp, &HeightProfileSmoothingParams::default()),
            short
        );
    }

    #[test]
    fn the_brush_raises_the_band_only() {
        let sp = params();
        let object = ModelObject::default();
        let base = vec![0.0, 0.25, 0.25, 0.25, 0.25, 0.2, 2.0, 0.2];
        let mut profile = base.clone();
        adjust_layer_height_profile(
            &object,
            &sp,
            &mut profile,
            1.0,
            0.05,
            0.6,
            LayerHeightEditActionType::Increase,
        );
        assert_ne!(profile, base);
        assert_eq!(&profile[..4], &base[..4]);
        assert_eq!(profile[profile.len() - 2], 2.0);
        for k in (0..profile.len()).step_by(2) {
            let (z, h) = (profile[k], profile[k + 1]);
            if (z - 1.0).abs() >= 0.3 && z > 0.25 {
                assert!((h - 0.2).abs() < 1e-9, "{z}: {h}");
            } else if (z - 1.0).abs() < 0.29 {
                assert!(h > 0.2, "{z}: {h}");
            }
        }
        // Outside the variable span nothing changes.
        let mut same = base.clone();
        adjust_layer_height_profile(
            &object,
            &sp,
            &mut same,
            2.5,
            0.05,
            0.6,
            LayerHeightEditActionType::Increase,
        );
        assert_eq!(same, base);
    }

    #[test]
    fn a_band_from_the_bottom_of_a_free_profile_stops() {
        // A raft under a first layer printed with a normal flow: the first layer is not fixed.
        let sp = SlicingParameters {
            base_raft_layers: 1,
            first_object_layer_bridging: false,
            ..params()
        };
        assert!(!sp.first_object_layer_height_fixed());
        let object = ModelObject::default();
        let mut profile = vec![0.0, 0.2, 2.0, 0.2];
        let caught = std::panic::catch_unwind(move || {
            adjust_layer_height_profile(
                &object,
                &sp,
                &mut profile,
                0.1,
                0.05,
                1.0,
                LayerHeightEditActionType::Increase,
            );
        });
        let stop = caught
            .unwrap_err()
            .downcast::<orca_index::NotPorted>()
            .unwrap();
        assert_eq!(stop.condition, PROFILE_INDEX_OUT_OF_RANGE);
    }

    #[test]
    fn lerp_is_the_cpp_form() {
        assert_eq!(lerp(1.0, 3.0, 0.5), 2.0);
        assert_eq!(lerp(0.1, 0.3, 0.0), 0.1);
        assert_eq!(lerp(0.1, 0.3, 1.0), 0.3);
    }
}
