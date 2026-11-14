// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Phase O3 (process() and the layers answer for domain A): Boost.Polygon 1.84's Voronoi diagram of
//! points and segments (its robust arithmetic, predicates, structures and builder) and libslic3r's
//! Voronoi utilities, polygon indexes and medial axis.
//!
//! Set up empty by orca-o3-skeleton (2026-11-13). The pieces of the orca-o3 wave fill it: voronoi-1
//! (Boost.Polygon's Voronoi types, robust arithmetic and first predicates), voronoi-2 (the
//! remaining predicates, structures and the builder), voronoi-3 (the Voronoi diagram, VoronoiUtils
//! and the polygon indexes), voronoi-4 (the construction and repair, the exact checks and the
//! medial axis). Each piece names here what it adds and the C++ it carries. README.md lists the C++
//! files and how the crate is proven.
//!
//! voronoi-1 (orca-o3-voronoi-1, 2026-11-13) adds:
//!
//! - [`ctypes`]: `boost/polygon/detail/voronoi_ctypes.hpp` 1-643, the ulp comparison, the
//!   extended-exponent double ([`Efpt64`]), the big integer ([`BigInt`], `extended_int<64>`), the
//!   converters and `voronoi_ctype_traits<int32>`;
//! - [`robust_fpt`]: `voronoi_robust_fpt.hpp` 1-501, the relative-error double ([`RobustFpt`]),
//!   its difference form ([`RobustDif`]) and the sums of square roots ([`RobustSqrtExpr`]);
//! - [`predicates`]: `voronoi_predicates.hpp` 1-524, the robust cross product, the orientation
//!   test, the point, event and node comparisons, the distance predicate and the circle existence
//!   predicate, generic over the traits that voronoi-2's site, circle and beach-line types
//!   implement.
//!
//! voronoi-2 (orca-o3-voronoi-2, 2026-11-13) adds:
//!
//! - [`geometry_type`]: `boost/polygon/voronoi_geometry_type.hpp` 1-48, the geometry and source
//!   categories of a site ([`GeometryCategory`], [`SourceCategory`]) and `belongs`;
//! - [`structures`]: `voronoi_structures.hpp` 1-450, the point, the site and circle events, the
//!   ordered circle event queue and the beach line's key and data ([`SiteEvent`],
//!   [`CircleEvent`], [`OrderedQueue`]);
//! - [`formation`]: `voronoi_predicates.hpp` 525-1533, the exact and lazy circle formation
//!   functors and the circle formation predicate;
//! - [`tree`]: MSVC's `std::map` (`<xtree>`'s red-black tree), which the builder's beach line is,
//!   translated here for want of a home in `orca-stl`;
//! - [`builder`]: `boost/polygon/voronoi_builder.hpp` 1-521, the sweep ([`VoronoiBuilder`]) and
//!   the output it fills ([`VoronoiOutput`]).
//!
//! voronoi-3 (orca-o3-voronoi-3, 2026-11-14) adds:
//!
//! - [`diagram`]: `boost/polygon/voronoi_diagram.hpp` 1-620, the cells, vertices and half-edges
//!   ([`VoronoiCell`], [`VoronoiVertex`], [`VoronoiEdge`]) and `voronoi_diagram<double>`
//!   ([`BoostVoronoiDiagram`]), the builder's output, with `_build`;
//! - [`voronoi`]: `boost/polygon/voronoi.hpp` 1-157, `insert` and `construct_voronoi` of points,
//!   segments, or both, over the point and segment concepts ([`PointConcept`],
//!   [`SegmentConcept`]);
//! - [`geometry_voronoi`]: libslic3r's `Geometry/Voronoi.hpp` 1-212,
//!   `Slic3r::Geometry::VoronoiDiagram` ([`VoronoiDiagram`]) with its issue and state, its
//!   accessors and its point and mixed constructions, and its [`Segment`];
//! - [`polygons_point_index`]: libslic3r's `Arachne/utils/PolygonsPointIndex.hpp` 1-178 and
//!   `PolygonsSegmentIndex.hpp` 1-50, the indexes into polygons ([`PolygonsPointIndex`],
//!   [`PolygonsSegmentIndex`]) and their locators and hash;
//! - [`voronoi_utils`]: libslic3r's `Geometry/VoronoiUtils.hpp` 1-167 and `VoronoiUtils.cpp`
//!   1-348 ([`VoronoiUtils`]): the vertex helpers, the source lookups, `discretize_parabola` and
//!   the segment and point cell ranges.
//!
//! voronoi-4 (orca-o3-voronoi-4, 2026-11-14) adds:
//!
//! - [`repair`]: libslic3r's `Geometry/Voronoi.cpp` 1-364, [`VoronoiDiagram`]'s construction from
//!   segments with the detection of known issues and the repair by rotation, `clear` and
//!   `copy_to_local`;
//! - [`voronoi_utils_cgal`]: `Geometry/VoronoiUtilsCgal.hpp` 1-29 and `.cpp` 1-334
//!   ([`VoronoiUtilsCgal`]), the two planarity checks, with CGAL's answers computed by
//!   [`exact`];
//! - [`exact`]: the exact sign of a polynomial in `double`s (an interval filter and exact dyadic
//!   numbers), written for this crate (it belongs in `orca-exact`);
//! - [`voronoi_offset`]: the parts of `Geometry/VoronoiOffset.hpp` and `.cpp` that MedialAxis
//!   calls, [`annotate_inside_outside`] and the colour categories (no piece carries the file);
//! - [`medial_axis`]: `Geometry/MedialAxis.hpp` 1-53 and `.cpp` 1-707 ([`MedialAxis`]) and
//!   `ExPolygon.cpp` 261-378 ([`ExPolygonMedialAxis`], `ExPolygon::medial_axis`).

pub mod builder;
pub mod ctypes;
pub mod diagram;
pub mod exact;
pub mod formation;
pub mod geometry_type;
pub mod geometry_voronoi;
pub mod medial_axis;
pub mod polygons_point_index;
pub mod predicates;
pub mod repair;
pub mod robust_fpt;
pub mod structures;
pub mod tree;
pub mod voronoi;
pub mod voronoi_offset;
pub mod voronoi_utils;
pub mod voronoi_utils_cgal;

pub use builder::{DefaultVoronoiBuilder, VoronoiBuilder, VoronoiOutput};
pub use diagram::{
    BoostVoronoiDiagram, VertexEqualityPredicate, VoronoiCell, VoronoiEdge, VoronoiVertex,
};
pub use formation::{
    CircleExistence, CircleFormationFunctor, CircleFormationPredicate, LazyCircleFormationFunctor,
    MpCircleFormationFunctor,
};
pub use geometry_type::{GeometryCategory, SourceCategory, belongs};
pub use geometry_voronoi::{IssueType, Segment, State, VoronoiDiagram};
pub use polygons_point_index::{
    PathPoints, PathsPointIndex, PathsPointIndexLocator, PolygonsPointIndex,
    PolygonsPointIndexSegmentLocator, PolygonsSegmentIndex, point_hash, polygons_point_index_hash,
};
pub use structures::{BeachLineNodeData, CircleEvent, OrderedQueue, Point2d, SiteEvent};
pub use voronoi::{
    PointConcept, SegmentConcept, construct_voronoi_points, construct_voronoi_points_segments,
    construct_voronoi_segments,
};
pub use voronoi_utils::{
    InRange, PointCellRange, SegmentCellRange, VoronoiContainers, VoronoiError, VoronoiUtils,
};

pub use medial_axis::{ExPolygonMedialAxis, MedialAxis};
pub use voronoi_offset::{
    CellCategory, EdgeCategory, VertexCategory, annotate_inside_outside,
    reset_inside_outside_annotations,
};
pub use voronoi_utils_cgal::{CgalOrientation, VoronoiUtilsCgal};

pub use ctypes::{
    BigInt, Efpt64, ExtendedExponentFpt, ExtendedInt, GetSqrt, Sign, ToFpt, TypeConverterEfpt,
    TypeConverterFpt, UlpComparison, UlpResult,
};
pub use predicates::{
    BeachLineNodeKey, CircleExistencePredicate, DistancePredicate, EventComparisonPredicate,
    NodeComparisonPredicate, Orientation, OrientationTest, PointComparisonPredicate, VoronoiCircle,
    VoronoiPoint, VoronoiSite, robust_cross_product,
};
pub use robust_fpt::{RobustDif, RobustFpt, RobustSqrtExpr};
