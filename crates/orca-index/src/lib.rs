// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Phase O3 (process() and the layers answer for domain A): libslic3r's spatial indexes: the
//! indirect AABB trees over triangles and lines, the indirect KD tree, the mutable priority queue
//! and EdgeGrid.
//!
//! Set up empty by orca-o3-skeleton (2026-11-13). The pieces of the orca-o3 wave fill it: index-1
//! (the AABB trees, the KD tree and the mutable priority queue), index-2 (EdgeGrid). Each piece
//! names here what it adds and the C++ it carries. README.md lists the C++ files and how the crate
//! is proven.
//!
//! index-1 (2026-11-13) adds:
//!
//! - [`aligned_box`]: Eigen's `AlignedBox`, the box of each tree node (Eigen 5.0.1's
//!   `AlignedBox.h`; it belongs in orca-eigen).
//! - [`aabb_tree`]: `AABBTreeIndirect.hpp` whole: the tree, its build, the ray, distance and
//!   radius queries over an indexed triangle set, `get_candidate_idxs`, `traverse` with
//!   `intersecting` and `within`, and `BoundingBoxWrapper`.
//! - [`aabb_lines`]: `AABBTreeLines.hpp` whole: the tree over lines, the distance, radius, inside
//!   and intersection queries and `LinesDistancer`, over `Line` and `Linef` (`Linef` belongs in
//!   orca-geom).
//! - [`kd_tree`]: `KDTreeIndirect.hpp` whole.
//! - [`priority_queue`]: `MutablePriorityQueue.hpp` whole.
//! - [`stop`]: [`NotPorted`], the stop on the inputs the engine's release build has no answer for
//!   (`parity/sop1/index.md`, IXNP).
//!
//! index-2 (2026-11-13) adds:
//!
//! - [`edge_grid`]: `EdgeGrid.hpp` and `EdgeGrid.cpp` (all but their debug and SVG output and the
//!   code inside `#if 0`): `Contour`, `Grid` with its `create` overloads, cell walks, signed
//!   distances, signed distance field, `contours_simplified` and `intersecting_edges`, and the free
//!   `intersecting_edges`; its stops are index-2's IXNP rows. It also holds
//!   `sort_remove_duplicates` (`libslic3r.h`; it belongs in orca-geom).

pub mod aabb_lines;
pub mod aabb_tree;
pub mod aligned_box;
pub mod edge_grid;
pub mod kd_tree;
pub mod priority_queue;
pub mod stop;

pub use stop::NotPorted;
