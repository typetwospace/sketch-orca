// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Phase O3 (process() and the layers answer for domain A): libslic3r's extrusion entities and
//! their collections, Flow and variable width, Surface and its collections, Layer and LayerRegion,
//! the declarations of Print, PrintObject and PrintRegion, and the reader and replay of the
//! oracle's step-boundary dumps.
//!
//! Set up empty by orca-o3-skeleton (2026-11-13). The pieces of the orca-o3 wave fill it: layer-1
//! (extrusion entities, Flow and variable width), layer-2 (Surface and the layer and print
//! declarations), layer-3 (Print's declaration, entity chaining, Layer and its regions, and the
//! support flows), dump-1 (the oracle's object and front-config dump writer, the record reader and
//! the replay module). Each piece names here what it adds and the C++ it carries. README.md lists
//! the C++ files and how the crate is proven.
//!
//! layer-1 (orca-o3-layer-1, 2026-11-13) adds the extrusion entities, Flow and variable width:
//! [`role`] (ExtrusionRole and its names, of ExtrusionEntity.hpp and .cpp), [`path`]
//! (ExtrusionPath and its contoured, sloped and oriented kinds), [`multi_path`]
//! (ExtrusionMultiPath), [`extrusion_loop`] (ExtrusionLoop and ExtrusionLoopSloped), [`entity`]
//! (ExtrusionEntity as an enum, ExtrusionEntitiesPtr and the append helpers), [`collection`]
//! (ExtrusionEntityCollection.hpp and .cpp, but the chaining, layer-3's), [`flow`] (Flow.hpp and
//! Flow.cpp, but the support flows, layer-3's) and [`variable_width`] (VariableWidth.hpp and .cpp),
//! with [`cpp_vec`], what MSVC's `std::vector` does to a path's `inset_idx` as it moves the path.
//!
//! layer-2 (orca-o3-layer-2, 2026-11-14) adds the data model's declarations: [`surface`]
//! (Surface.hpp and .cpp: `Surface`, `SurfaceType` and the helpers over lists of surfaces),
//! [`surface_collection`] (SurfaceCollection.hpp and .cpp), [`layer`] (Layer.hpp: `Layer`,
//! `LayerRegion`, `SupportLayer`, with Line.hpp's `CurledLine`), [`print_base`] (PrintBase.hpp:
//! the step states, `PrintBase` and the two bases with state, with the parser config
//! `PrintBase` holds), [`print_region`] (Print.hpp's `PrintRegion`) and [`print_object`]
//! (Print.hpp up to `PrintObject`: the steps, `PrintInstance`, `PrintObjectRegions` and
//! `PrintObject`). Back-pointers are indices or arguments; README.md says which.
//!
//! layer-3 (orca-o3-layer-3, 2026-11-14) finishes the data model: [`print`] (Print.hpp's
//! `PrintStatistics` and `Print`, with the extruder helpers of Print.cpp), [`chain`] (the entity
//! and instance chaining of ShortestPath.hpp and .cpp, which [`collection`]'s `chained_path_from`
//! now runs), [`fs_path`] (what PrintBase.cpp takes from Boost.Filesystem's path), PrintBase.cpp
//! in [`print_base`], PrintRegion.cpp in [`print_region`], the base parts of Layer.cpp and
//! LayerRegion.cpp in [`layer`], and the support flows of Flow.cpp in [`flow`].
//!
//! dump-1 (orca-o3-dump-1, 2026-11-14) adds the oracle's dumps: [`dump`] (the parity oracle's
//! OracleDump.hpp and the object and front-config parts of OracleDump.cpp, patches 0005 to 0007:
//! the SOP1 payload and container, the knob, the run record, the entity and config records, and
//! the object and front-config dumps) and [`replay`] (the goldens finder, the record reader of
//! every dump kind, the readers of the front-objects, front-config and object dumps into the data
//! model, and the byte comparison that names the first record that differs).

pub mod chain;
pub mod collection;
pub mod cpp_vec;
pub mod dump;
pub mod entity;
pub mod extrusion_loop;
pub mod flow;
pub mod fs_path;
pub mod layer;
pub mod multi_path;
pub mod path;
pub mod print;
pub mod print_base;
pub mod print_object;
pub mod print_region;
pub mod replay;
pub mod role;
pub mod surface;
pub mod surface_collection;
pub mod variable_width;
