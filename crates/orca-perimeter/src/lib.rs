// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Phase O3 (process() and the layers answer for domain A): libslic3r's PerimeterGenerator, classic
//! and Arachne, with make_perimeters, BridgeDetector and the 2D principal components.
//!
//! Set up empty by orca-o3-skeleton (2026-11-13). The pieces of the orca-o3 wave fill it: walls-1
//! (PerimeterGenerator's helpers, BridgeDetector and principal components), walls-2 (the classic
//! perimeters), walls-3 (the Arachne perimeters and make_perimeters). Each piece names here what it
//! adds and the C++ it carries. README.md lists the C++ files and how the crate is proven.
//!
//! walls-1 (2026-11-14) adds [`principal_components`] (PrincipalComponents2D), [`bridge_detector`]
//! (BridgeDetector), [`extrusion_paths`] (the variable-width `extrusion_paths_append`),
//! [`fuzzy_skin`] (the fuzzy skin's config and the walls it leaves alone), [`extra_perimeters`]
//! (the extra perimeters on overhangs and their helpers), [`perimeter_generator`] (the generator's
//! state, the loops' and the Arachne lines' walks, the top surface split) and [`stop`] (the
//! crate's stops and its error).

pub mod bridge_detector;
pub mod extra_perimeters;
pub mod extrusion_paths;
pub mod fuzzy_skin;
pub mod perimeter_generator;
pub mod principal_components;
pub mod stop;
