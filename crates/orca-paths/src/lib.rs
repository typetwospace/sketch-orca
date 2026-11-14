// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Phase O3 (process() and the layers answer for domain A): libslic3r's ShortestPath: the chaining
//! of extrusions, polylines and islands into short travel orders.
//!
//! Set up empty by orca-o3-skeleton (2026-11-13). The pieces of the orca-o3 wave fill it: paths-1
//! (ShortestPath's chaining). Each piece names here what it adds and the C++ it carries. README.md
//! lists the C++ files and how the crate is proven.
//!
//! paths-1 (2026-11-13) adds ShortestPath's generic chaining and Clipper's tree walked outside in:
//!
//! - [`segments`]: the segment chaining, a KD tree of end points and a mutable priority queue of
//!   their nearest free neighbours: `chain_segments_greedy` and its constrained variants, the first
//!   and the second versions (ShortestPath.cpp 42-1025);
//! - [`points`]: points, expolygons, path items and Clipper nodes chained, and
//!   `reorder_by_shortest_traverse` (ShortestPath.cpp 1096-1115, 1998-2012);
//! - [`polylines`]: `chain_polylines` with the two-exchange improvement and the flip edges
//!   (ShortestPath.cpp 1116-1996);
//! - [`lines`]: `chain_lines` and Point.hpp's `ClosestPointInRadiusLookup`, which belongs in
//!   orca-geom (ShortestPath.cpp 2043-2099);
//! - [`outside_in`]: `union_pt_chained_outside_in` (ClipperUtils.cpp 992-1017);
//! - [`stop`]: the edges no input of domain A reaches, which stop with `NotPorted`.
//!
//! The chaining of extrusion entities and print instances (`chain_extrusion_*`,
//! `chain_print_object_instances`) is layer-3's, in orca-layer, over [`segments`].

pub mod lines;
pub mod outside_in;
pub mod points;
pub mod polylines;
pub mod segments;
pub mod stop;

pub use lines::{ClosestPointInRadiusLookup, chain_lines};
pub use outside_in::union_pt_chained_outside_in;
pub use points::{
    chain_clipper_polynodes, chain_expolygons, chain_path_items, chain_points,
    reorder_by_shortest_traverse,
};
pub use polylines::{chain_polylines, chain_polylines_copy};
pub use segments::{
    Chain, chain_segments_greedy, chain_segments_greedy_constrained_reversals,
    chain_segments_greedy_constrained_reversals2, chain_segments_greedy2,
};
pub use stop::NotPorted;
