// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Phase O3 (process() and the layers answer for domain A): libslic3r's mesh slicer
//! (TriangleMeshSlicer), SlicingParameters, adaptive slicing and the layer height series.
//!
//! Set up empty by orca-o3-skeleton (2026-11-13). The pieces of the orca-o3 wave fill it: slice-1
//! (TriangleMeshSlicer's first half and SlicingParameters), slice-2 (TriangleMeshSlicer's second
//! half, adaptive slicing and the layer series). Each piece names here what it adds and the C++ it
//! carries. README.md lists the C++ files and how the crate is proven.
//!
//! slice-1 (2026-11-13) adds:
//!
//! - [`triangle_mesh_slicer`]: `TriangleMeshSlicer.hpp` (`MeshSlicingParams`,
//!   `MeshSlicingParamsEx`) and `TriangleMeshSlicer.cpp` lines 1-1057: the intersection lines of a
//!   facet with a plane (`slice_facet`), the lines of every face at every `z` (`slice_make_lines`,
//!   joined in the order of the faces in any thread arena), the slab lines of the faces up or down
//!   (`slice_facet_with_slabs`, `slice_slabs_make_lines`) and `OpenPolyline`; with
//!   `next_idx_modulo` of `libslic3r.h` (it belongs in orca-geom). The cut-mesh helpers are left
//!   out: only the cut tool reaches them.
//! - [`slicing`]: `Slicing.hpp` and `Slicing.cpp` lines 1-306: `SlicingParameters` and
//!   `create_from_config`, `equal_layering`, the nozzle limits of namespace `Slicing` and
//!   `layer_height_profile_from_ranges`.
//!
//! slice-2 (2026-11-14) adds:
//!
//! - [`slice_mesh`]: `TriangleMeshSlicer.cpp` lines 1058-2159: the lines chained into loops by
//!   triangle connectivity, then the open polylines joined exactly and across gaps (`make_loops`),
//!   the slab loops, `make_expolygons`, and the public `slice_mesh` (at heights and at one plane),
//!   `slice_mesh_ex` with its two inline overloads, and `slice_mesh_slabs`. `make_expolygons_simple`
//!   is left out: only the cut tool reaches it.
//! - [`slicing_adaptive`]: `SlicingAdaptive.hpp` and `SlicingAdaptive.cpp`: the faces of an object
//!   by height and the layer height their slope allows (`next_layer_height`), and the distance to
//!   the next horizontal face.
//! - [`slicing_layers`]: `Slicing.cpp` lines 307-901: `layer_height_profile_adaptive`,
//!   `smooth_height_profile`, `adjust_layer_height_profile`,
//!   `adjust_layer_series_to_align_object_height`, `generate_object_layers` and
//!   `check_object_layers_fixed`.
//!
//! The stops are orca-index's [`NotPorted`](orca_index::NotPorted).

pub mod slice_mesh;
pub mod slicing;
pub mod slicing_adaptive;
pub mod slicing_layers;
pub mod triangle_mesh_slicer;
