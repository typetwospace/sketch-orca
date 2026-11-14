// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Phase O3 (process() and the layers answer for domain A): libslic3r's MutablePolygon, region
//! expansion, the surface types, the vertical and horizontal shells, prepare_infill, the external
//! surfaces and bridges over infill.
//!
//! Set up empty by orca-o3-skeleton (2026-11-13). The pieces of the orca-o3 wave fill it:
//! surfaces-1 (MutablePolygon and region expansion), surfaces-2 (surface types and the vertical and
//! horizontal shells), surfaces-3 (prepare_infill, the external surfaces and bridges over infill).
//! Each piece names here what it adds and the C++ it carries. README.md lists the C++ files and how
//! the crate is proven.
//!
//! surfaces-1 (2026-11-13) adds:
//!
//! - [`mutable_polygon`]: `MutablePolygon.hpp` and `MutablePolygon.cpp` whole: the polygon as a
//!   loop of linked points, its range of unprocessed points, `remove_duplicates` and
//!   `smooth_outward` with `clip_narrow_corner`;
//! - [`region_expansion`]: `Algorithm/RegionExpansion.hpp` and `.cpp` whole: the expansion
//!   parameters, the wave seeds, the wave propagation, `expand_expolygons` and the merge of the
//!   expansions into their sources;
//! - [`clipper_z_utils`]: `ClipperZUtils.hpp` and the `ClipperLib_Z::Path` form of
//!   `polylines_merge`, which region expansion calls and no piece carries in orca-geom, where they
//!   belong.
//!
//! surfaces-2 (2026-11-14) adds the steps of `PrintObject::prepare_infill` before the external
//! surfaces, each a free function `(print, object_idx)`:
//!
//! - [`detect_surfaces`]: `PrintObject::detect_surfaces_type` (top, bottom, bottom bridge and
//!   internal slices, ORCA's second bridge layer, the fill surfaces made from the slices);
//! - [`vertical_shells`]: the vertical shells of `ensure_vertical_shell_thickness` = all,
//!   `PrintObject::discover_vertical_shells`;
//! - [`horizontal_shells`]: `PrintObject::clip_fill_surfaces`, `discover_horizontal_shells` and
//!   `combine_infill`;
//! - [`layer_region`]: `LayerRegion::prepare_fill_surfaces`, `infill_area_threshold` and
//!   `trim_surfaces`, the trait `LayerRegionFill` on orca-layer's `LayerRegion`;
//! - [`utils`]: `check_layer_id_pattern` of `utils.cpp` (the `extra_solid_infills` pattern),
//!   which belongs in a utils crate no piece carries;
//! - [`layers_out`]: how a step takes an object's layers out to change them beside the object and
//!   the print it reads, and runs its parallel loops.

pub mod clipper_z_utils;
pub mod detect_surfaces;
pub mod horizontal_shells;
pub mod layer_region;
pub mod layers_out;
pub mod mutable_polygon;
pub mod region_expansion;
pub mod utils;
pub mod vertical_shells;
