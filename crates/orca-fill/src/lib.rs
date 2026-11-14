// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Phase O3 (process() and the layers answer for domain A): libslic3r's infill: FillBase and its
//! helpers, gap fill and infill connection, the rectilinear patterns, cross hatch, the concentric
//! patterns, Fill::new_from_type, make_fills and PrintObject::infill.
//!
//! Set up empty by orca-o3-skeleton (2026-11-13). The pieces of the orca-o3 wave fill it: fill-1
//! (FillBase and its helpers), fill-2 (gap fill, infill connection and the rectilinear set-up),
//! fill-3 (the rectilinear patterns' graph and cross hatch), fill-4 (make_fills, the concentric
//! patterns, Fill::new_from_type and PrintObject::infill). Each piece names here what it adds and
//! the C++ it carries. README.md lists the C++ files and how the crate is proven.
//!
//! fill-1 (2026-11-14) adds:
//!
//! - [`base`]: `class Fill` as the trait [`base::Fill`] over [`base::FillBase`], `FillParams`,
//!   `LockRegionParam`, `fill_surface`, `fill_surface_arachne`, `_infill_direction`,
//!   `_adjust_solid_spacing` and the two `connect_infill` forms that hand on to the third
//!   (FillBase.hpp 1-235; FillBase.cpp 1-39, 105-132, 255-321 and 1243-1262).
//! - [`contour`]: the contour walks and clipping that `connect_infill` builds on (FillBase.cpp
//!   322-1242 and 1263-1264).
//! - [`stop`]: the crate's stops, each with orca-index's `NotPorted` payload.
//!
//! fill-2 (2026-11-14) adds:
//!
//! - [`extrusion`]: `Fill::fill_surface_extrusion`'s base body and `_create_gap_fill`, the gap
//!   fill by the medial axis (FillBase.cpp 133-254).
//! - [`connect`]: `BoundaryInfillGraph`, `create_boundary_infill_graph`, the third
//!   `connect_infill`, `chain_or_connect_infill` and `extended_object_bounding_box` (FillBase.cpp
//!   1265-1833).
//! - [`rectilinear`]: the rectilinear infill's set-up: the vertical lines' slicing, the
//!   intersections' links along the contours, the region with its offsets, and the rectilinear
//!   fillers of domain A, whose `fill_surface` is fill-3's (FillRectilinear.hpp 1-232;
//!   FillRectilinear.cpp 1-1353).
//!
//! What the C++ prints with `printf` goes through orca-fmt's `stdio::stdout_write`, C's `stdout`
//! for the whole port.

pub mod base;
pub mod connect;
pub mod contour;
pub mod extrusion;
pub mod rectilinear;
pub mod stop;
