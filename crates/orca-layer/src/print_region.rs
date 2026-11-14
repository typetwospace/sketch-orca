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
// Carries OrcaSlicer code: class PrintRegion and its operator== and operator!= of
// src/libslic3r/Print.hpp lines 102-153 of OrcaSlicer v2.4.2, translated on 2026-11-14
// (orca-o3-layer-2). Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab), the
// PrusaSlicer authors (Prusa Research) and the Slic3r authors (Alessandro Ranellucci), under the
// GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).
//
// Carries OrcaSlicer code: src/libslic3r/PrintRegion.cpp lines 1-112 (extruder, flow,
// nozzle_dmr_avg, bridging_height_avg and both collect_object_printing_extruders) of OrcaSlicer
// v2.4.2, translated on 2026-11-14 (orca-o3-layer-3). Copyright the OrcaSlicer authors, the
// BambuStudio authors (Bambu Lab), the PrusaSlicer authors (Prusa Research) and the Slic3r
// authors, under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! [`PrintRegion`]: a group of volumes printed with the same region config (and so the same
//! extruders), with the config's hash, its index in the print's regions and in its object's, and
//! the count of the print objects that use it.
//!
//! A region lives in its object's [`PrintObjectRegions`](crate::print_object::PrintObjectRegions),
//! which the print objects of one model object share (an `Arc`). `Print::apply` sets the print's
//! index (`m_print_region_id`) and counts the references (`print_region_ref_inc` and its kin,
//! PrintApply.cpp's friends) while the regions are shared, so those two are atomics, read and
//! written with relaxed order (the C++ ones are plain `int`s that only the main thread touches).
//! The config and its hash change together, through [`PrintRegion::set_config`] and
//! [`PrintRegion::config_apply_only`].
//!
//! `PrintRegion()` leaves the hash uninitialised; here it is 0. The constructors from a config
//! alone, which hash it, are Print.cpp's (lines 55-56, orca-o3-print-1's).
//!
//! PrintRegion.cpp (orca-o3-layer-3): [`PrintRegion::extruder`] is the role's 1-based filament
//! (the config's `int` through a `size_t` into an `unsigned int`), and "Unknown role" for the
//! support roles. [`PrintRegion::flow`] takes the first layer's width when it is set, else the
//! role's, and the object's `line_width` for a width whose value is 0 (a percent too); its
//! nozzle is `nozzle_diameter.get_at(extruder - 1)`, where filament 0 underflows the `unsigned`
//! to `UINT_MAX` and `get_at` falls back to the first nozzle. The object's print (`object.print()`)
//! is an argument. [`PrintRegion::nozzle_dmr_avg`] averages the nozzles of six filaments, each
//! `get_at(id - 1)` in `int` (filament 0 gives -1, a huge `size_t`, so the first nozzle).
//! [`PrintRegion::collect_object_printing_extruders_of`] lists the 0-based filaments the region
//! prints with, a filament past the printer's filaments counted as 0.

use core::sync::atomic::{AtomicI32, Ordering};

use orca_config::{ConfigBase, ConfigError, PrintRegionConfig, StaticPrintConfig};

use crate::flow::{Flow, FlowError, FlowRole, nozzle_diameter_at};
use crate::print::{Print, has_brim};
use crate::print_object::PrintObject;

/// The text of the `InvalidArgument` a support role throws.
const UNKNOWN_ROLE: &str = "Unknown role";

/// `class PrintRegion`.
#[derive(Debug)]
pub struct PrintRegion {
    /// `m_config`.
    config: PrintRegionConfig,
    /// `m_config_hash`.
    config_hash: u64,
    /// `m_print_region_id`: the index in `Print::m_print_regions`, -1 until the print sets it.
    pub print_region_id: AtomicI32,
    /// `m_print_object_region_id`: the index in the object's regions.
    pub print_object_region_id: i32,
    /// `m_ref_cnt`: the print objects using it, as `Print::apply` counts them.
    pub ref_cnt: AtomicI32,
}

impl Default for PrintRegion {
    /// `PrintRegion()`: the default config, the hash 0 (uninitialised in the C++).
    fn default() -> Self {
        Self::new(PrintRegionConfig::default(), 0, -1)
    }
}

impl Clone for PrintRegion {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            config_hash: self.config_hash,
            print_region_id: AtomicI32::new(self.print_region_id()),
            print_object_region_id: self.print_object_region_id,
            ref_cnt: AtomicI32::new(self.ref_cnt.load(Ordering::Relaxed)),
        }
    }
}

impl PartialEq for PrintRegion {
    /// `operator==(lhs, rhs)`: the same hash and the same config.
    fn eq(&self, rhs: &Self) -> bool {
        self.config_hash() == rhs.config_hash() && self.config() == rhs.config()
    }
}

impl PrintRegion {
    /// `PrintRegion(config, config_hash, print_object_region_id = -1)`.
    #[must_use]
    pub fn new(config: PrintRegionConfig, config_hash: u64, print_object_region_id: i32) -> Self {
        Self {
            config,
            config_hash,
            print_region_id: AtomicI32::new(-1),
            print_object_region_id,
            ref_cnt: AtomicI32::new(0),
        }
    }

    /// `config()`.
    #[must_use]
    pub fn config(&self) -> &PrintRegionConfig {
        &self.config
    }

    /// `config_hash()`.
    #[must_use]
    pub fn config_hash(&self) -> u64 {
        self.config_hash
    }

    /// `print_region_id()`.
    #[must_use]
    pub fn print_region_id(&self) -> i32 {
        self.print_region_id.load(Ordering::Relaxed)
    }

    /// `print_object_region_id()`.
    #[must_use]
    pub fn print_object_region_id(&self) -> i32 {
        self.print_object_region_id
    }

    /// `set_config(config)`: the config and its new hash.
    pub fn set_config(&mut self, config: PrintRegionConfig) {
        self.config = config;
        self.config_hash = self.config.hash();
    }

    /// `config_apply_only(other, keys, ignore_nonexistent)`: the keys applied, then the hash
    /// taken again (not when the apply throws, as in the C++).
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
        self.config.apply_only(other, keys, ignore_nonexistent)?;
        self.config_hash = self.config.hash();
        Ok(())
    }

    /// `extruder(role)`: the 1-based filament of a wall or infill role.
    ///
    /// # Errors
    /// `InvalidArgument("Unknown role")` for the support roles.
    pub fn extruder(&self, role: FlowRole) -> Result<u32, FlowError> {
        // The int is converted to size_t, then returned as unsigned int.
        let extruder: i32 = match role {
            FlowRole::Perimeter => self.config.inner_wall_filament_id.value,
            FlowRole::ExternalPerimeter => self.config.outer_wall_filament_id.value,
            FlowRole::Infill => self.config.sparse_infill_filament_id.value,
            FlowRole::SolidInfill => self.config.internal_solid_filament_id.value,
            FlowRole::TopSolidInfill => self.config.top_surface_filament_id.value,
            _ => return Err(FlowError::InvalidArgument(UNKNOWN_ROLE.to_string())),
        };
        #[allow(clippy::cast_sign_loss)]
        Ok(extruder as u32)
    }

    /// `flow(object, role, layer_height, first_layer)`: the role's flow at that height, from the
    /// width setting (the first layer's when set; the object's `line_width` for one of value 0)
    /// and the role's filament's nozzle. `print` is the object's print.
    ///
    /// # Errors
    /// "Unknown role" for the support roles, and what `Flow::new_from_config_width` gives.
    ///
    /// # Panics
    /// When the print's `nozzle_diameter` is empty.
    #[allow(clippy::cast_possible_truncation)]
    pub fn flow(
        &self,
        object: &PrintObject,
        print: &Print,
        role: FlowRole,
        layer_height: f64,
        first_layer: bool,
    ) -> Result<Flow, FlowError> {
        let print_config = print.config();
        // The extrusion width from the configuration (an absolute value, a percent, or zero for
        // auto).
        let mut config_width = if first_layer && print_config.initial_layer_line_width.value > 0. {
            print_config.initial_layer_line_width
        } else if role == FlowRole::ExternalPerimeter {
            self.config.outer_wall_line_width
        } else if role == FlowRole::Perimeter {
            self.config.inner_wall_line_width
        } else if role == FlowRole::Infill {
            self.config.sparse_infill_line_width
        } else if role == FlowRole::SolidInfill {
            self.config.internal_solid_infill_line_width
        } else if role == FlowRole::TopSolidInfill {
            self.config.top_surface_line_width
        } else {
            return Err(FlowError::InvalidArgument(UNKNOWN_ROLE.to_string()));
        };

        if config_width.value == 0. {
            config_width = object.config().line_width;
        }

        // The nozzle of the role's filament. extruder(role) - 1 may underflow to UINT_MAX; get_at
        // then falls back to the first nozzle.
        let e = self.extruder(role)?.wrapping_sub(1);
        let nozzle_diameter =
            nozzle_diameter_at(print_config, e as usize, "PrintRegion::flow") as f32;
        Flow::new_from_config_width(role, &config_width, nozzle_diameter, layer_height as f32)
    }

    /// `nozzle_dmr_avg(print_config)`: the mean nozzle of the six wall, infill and surface
    /// filaments.
    ///
    /// # Panics
    /// When `nozzle_diameter` is empty.
    #[must_use]
    pub fn nozzle_dmr_avg(&self, print_config: &orca_config::PrintConfig) -> f64 {
        // Each id - 1 in int, converted to size_t: filament 0 reads past the end, so the first.
        #[allow(clippy::cast_sign_loss)]
        let at = |id: i32| {
            nozzle_diameter_at(
                print_config,
                id.wrapping_sub(1) as isize as usize,
                "PrintRegion::nozzle_dmr_avg",
            )
        };
        (at(self.config.outer_wall_filament_id.value)
            + at(self.config.inner_wall_filament_id.value)
            + at(self.config.sparse_infill_filament_id.value)
            + at(self.config.internal_solid_filament_id.value)
            + at(self.config.top_surface_filament_id.value)
            + at(self.config.bottom_surface_filament_id.value))
            / 6.
    }

    /// `bridging_height_avg(print_config)`: the mean nozzle times the root of the bridge flow.
    ///
    /// # Panics
    /// When `nozzle_diameter` is empty.
    #[must_use]
    pub fn bridging_height_avg(&self, print_config: &orca_config::PrintConfig) -> f64 {
        self.nozzle_dmr_avg(print_config) * orca_crt::sqrt(self.config.bridge_flow.value)
    }

    /// The static `collect_object_printing_extruders(print_config, region_config, has_brim,
    /// object_extruders)`: the 0-based filaments of the walls (with a brim too), the sparse and
    /// solid infill and the top and bottom surfaces that the region prints, appended in that
    /// order; a filament past the printer's filaments counts as 0. The same logic enables the
    /// GUI's filament fields.
    pub fn collect_object_printing_extruders_of(
        print_config: &orca_config::PrintConfig,
        region_config: &PrintRegionConfig,
        has_brim: bool,
        object_extruders: &mut Vec<u32>,
    ) {
        // BBS
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let num_extruders = print_config.gcode.filament_diameter.values.len() as i32;
        let mut emplace_extruder = |extruder_id: i32| {
            let i = orca_stl::max(0, extruder_id.wrapping_sub(1));
            #[allow(clippy::cast_sign_loss)]
            object_extruders.push(if i >= num_extruders { 0 } else { i as u32 });
        };
        if region_config.wall_loops.value > 0 || has_brim {
            emplace_extruder(region_config.outer_wall_filament_id.value);
            if region_config.wall_loops.value > 1 {
                emplace_extruder(region_config.inner_wall_filament_id.value);
            }
        }
        if region_config.sparse_infill_density.value > 0. {
            emplace_extruder(region_config.sparse_infill_filament_id.value);
        }
        if region_config.sparse_infill_density.value > 0.
            || region_config.top_shell_layers.value > 0
            || region_config.bottom_shell_layers.value > 0
        {
            emplace_extruder(region_config.internal_solid_filament_id.value);
        }
        if region_config.top_shell_layers.value > 0 {
            emplace_extruder(region_config.top_surface_filament_id.value);
        }
        if region_config.bottom_shell_layers.value > 0 {
            emplace_extruder(region_config.bottom_surface_filament_id.value);
        }
    }

    /// `collect_object_printing_extruders(print, object_extruders)`: the static one with the
    /// print's config and [`has_brim`]. (The C++ asserts in debug builds that each filament is
    /// one of the printer's, which `Print::apply` ensures.)
    pub fn collect_object_printing_extruders(
        &self,
        print: &Print,
        object_extruders: &mut Vec<u32>,
    ) {
        Self::collect_object_printing_extruders_of(
            print.config(),
            self.config(),
            has_brim(print),
            object_extruders,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_config_takes_the_hash_and_equality_reads_both() {
        let mut r = PrintRegion::default();
        assert_eq!((r.print_region_id(), r.print_object_region_id()), (-1, -1));
        r.set_config(PrintRegionConfig::default());
        let h = PrintRegionConfig::default().hash();
        assert_eq!(r.config_hash(), h);
        let same = PrintRegion::new(PrintRegionConfig::default(), h, 3);
        assert!(r == same);
        assert!(r != PrintRegion::new(PrintRegionConfig::default(), h ^ 1, 3));
        r.print_region_id.store(7, Ordering::Relaxed);
        assert_eq!(r.clone().print_region_id(), 7);
    }
}
