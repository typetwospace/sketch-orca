// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Phase O3 (process() and the layers answer for domain A): libslic3r's Arachne wall generator: the
//! beading strategies, ExtrusionLine, the skeletal trapezoidation graph and SkeletalTrapezoidation,
//! the grids, the polyline stitcher and WallToolPaths.
//!
//! Set up empty by orca-o3-skeleton (2026-11-13). The pieces of the orca-o3 wave fill it: arachne-1
//! (the beading strategies and ExtrusionLine), arachne-2 (the skeletal trapezoidation graph, the
//! grids and the stitcher), arachne-3 (SkeletalTrapezoidation, first half), arachne-4
//! (SkeletalTrapezoidation, second half, and WallToolPaths). Each piece names here what it adds and
//! the C++ it carries. README.md lists the C++ files and how the crate is proven.
//!
//! arachne-1 (2026-11-13) adds, each module carrying its C++ file whole unless it says otherwise:
//!
//! - [`beading`]: `BeadingStrategy` and its five strategies (distributed, limited, outer wall
//!   inset, redistribute, widening) and `BeadingStrategyFactory::makeStrategy`, which stacks them
//!   (`Arachne/BeadingStrategy/*`).
//! - [`extrusion_junction`] and [`extrusion_line`]: `ExtrusionJunction`, `ExtrusionLine` (its
//!   length, `simplify`, the area error, `is_contour`, `area`) and the conversions
//!   `to_thick_polyline` (of these lines and of ClipperLib_Z's paths), `to_polygon` and
//!   `to_points` (`Arachne/utils/ExtrusionJunction.hpp`, `ExtrusionLine.hpp` and `.cpp` but
//!   `extrusion_paths_append`, which is the piece walls-1's in orca-perimeter).
//! - [`linear_alg_2d`]: `LinearAlg2D::pointIsLeftOfLine` and `getAngleLeft`
//!   (`Arachne/utils/linearAlg2D.hpp`).
//! - [`half_edge`]: the half-edge graph templates (`Arachne/utils/HalfEdge.hpp`,
//!   `HalfEdgeGraph.hpp`, `HalfEdgeNode.hpp`), the `std::list`s of edges and nodes as
//!   [`half_edge::StableList`]s whose handles stand for the C++ pointers.
//! - [`stop`]: where the C++ has no answer (`parity/sop1/arachne.md`, "The pieces and their
//!   edges"), the crate panics with orca-index's `NotPorted` payload naming the edge.
//!
//! arachne-2 (2026-11-14) adds, each module carrying its C++ file whole:
//!
//! - [`skeletal_trapezoidation_edge`] and [`skeletal_trapezoidation_joint`]: the data of the
//!   graph's edges and nodes, with their weak pointers to the lists and beadings
//!   SkeletalTrapezoidation owns (`Arachne/SkeletalTrapezoidationEdge.hpp`,
//!   `SkeletalTrapezoidationJoint.hpp`).
//! - [`skeletal_trapezoidation_graph`]: the graph and the methods of its edges and nodes
//!   (`canGoUp`, `isUpward`, `distToGoUp`, `isLocalMaximum` and the rest), `collapseSmallEdges`,
//!   `makeRib`, `insertRib` and `insertNode` (`Arachne/SkeletalTrapezoidationGraph.hpp` and
//!   `.cpp`).
//! - [`square_grid`], [`sparse_grid`], [`sparse_point_grid`] and [`sparse_line_grid`]: the grids
//!   (`Arachne/utils/SquareGrid.hpp` and `.cpp`, `SparseGrid.hpp`, `SparsePointGrid.hpp`,
//!   `SparseLineGrid.hpp`), their cells in [`unordered_multimap`], MSVC's
//!   `std::unordered_multimap` (which belongs in orca-stl).
//! - [`polyline_stitcher`]: `PolylineStitcher<VariableWidthLines, ExtrusionLine,
//!   ExtrusionJunction>::stitch` and its specialisations (`Arachne/utils/PolylineStitcher.hpp` and
//!   `.cpp`).
//!
//! arachne-3 (2026-11-14) adds:
//!
//! - [`skeletal_trapezoidation`]: `SkeletalTrapezoidation`, its construction from the Voronoi
//!   diagram of the outline and the steps of `generateToolpaths` from `updateIsCentral` to
//!   `generateAllTransitionEnds` (`Arachne/SkeletalTrapezoidation.hpp` and `.cpp` lines 1-1268,
//!   with `isEndOfCentral`).
//!
//! arachne-4 (2026-11-14) adds:
//!
//! - [`skeletal_trapezoidation`], second half: `applyTransitions`, `generateExtraRibs` and
//!   `generateSegments` with what it calls, the beading propagation, the junctions and the
//!   toolpaths (`Arachne/SkeletalTrapezoidation.cpp` lines 1269-2098).
//! - [`wall_tool_paths`]: `WallToolPaths`, `make_paths_params` and the outline helpers of
//!   `Arachne/WallToolPaths.cpp` (`Arachne/WallToolPaths.hpp` and `.cpp`), the wall settings read
//!   from orca-config and the bead widths through orca-layer's `Flow`.
//! - [`dense_set`]: the order of `ankerl::unordered_dense::set` (which belongs in orca-stl), for
//!   `connectJunctions` and `getRegionOrder`.
//!
//! `tests/probes.rs` replays the records of `arachne.sop1` that name arachne-1 (ANBS, ANLA, ANXL
//! and ANAE), arachne-2 (ANSQ, ANSP, ANSL, ANPS and ANGR), arachne-3 (ANSC and ANST stages 0 to
//! 7) and arachne-4 (ANST stages 8 to 10, ANWS, ANIP, ANWH, ANWT and ANMP).

pub mod beading;
pub mod dense_set;
pub mod extrusion_junction;
pub mod extrusion_line;
pub mod half_edge;
pub mod linear_alg_2d;
pub mod polyline_stitcher;
pub mod skeletal_trapezoidation;
pub mod skeletal_trapezoidation_edge;
pub mod skeletal_trapezoidation_graph;
pub mod skeletal_trapezoidation_joint;
pub mod sparse_grid;
pub mod sparse_line_grid;
pub mod sparse_point_grid;
pub mod square_grid;
pub mod stop;
pub mod unordered_multimap;
pub mod wall_tool_paths;
