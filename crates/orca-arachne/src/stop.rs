// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The stops of orca-arachne: where Arachne's C++ has no answer, this crate stops instead of
//! guessing (added by arachne-1, 2026-11-13; each piece to arachne-4 adds its own).
//!
//! OrcaSlicer's release build defines `NDEBUG`, so Arachne's asserts do not run, and on some
//! inputs it indexes -1, reads past a vector's end or divides by zero: undefined behaviour, whose
//! result nothing can copy. `parity/sop1/arachne.md` ("The pieces and their edges") names each such
//! edge, and no probe record holds one; domain A (one filament, printed by layer, classic or
//! Arachne walls) never reaches them. On such an input the crate panics with orca-index's
//! [`NotPorted`] payload ([`stop`]): one payload type for the stops of every phase O3 crate, which
//! `catch_unwind` and `downcast::<NotPorted>()` catch. Its `function` is the C++ function and its
//! `condition` the edge's name as the page writes it (`limited-below-2`), or, for an edge the page
//! does not list, a name of this crate's own (the constants below say which).

pub use orca_index::stop::{NotPorted, not_ported};

/// `LimitedBeadingStrategy::compute` with a maximum bead count below 2: it indexes the toolpath
/// locations and bead widths at `max_bead_count / 2 - 1`, which is -1 (the page's edge).
pub const LIMITED_BELOW_2: &str = "limited-below-2";

/// `LimitedBeadingStrategy::compute` over the limit when its parent gave fewer than
/// `max_bead_count / 2` beads at the optimal thickness of `max_bead_count`: it reads and inserts
/// past the end. No strategy of the five does so; the page does not list it (this crate's name).
pub const LIMITED_SHORT_PARENT: &str = "limited-short-parent";

/// `ExtrusionLine::area` of an open line, `to_thick_polyline` of fewer than two points and
/// `to_polygon` of fewer than three junctions, which assert (the page's edge).
pub const LINE_SHAPE: &str = "line-shape";

/// `ExtrusionLine::calculateExtrusionAreaDeviationError` of three junctions at one point whose
/// widths differ by more than 1: it divides by `ab_length + bc_length`, which is 0. `simplify`
/// calls it only after a segment of 5 microns or more; the page does not list it (this crate's
/// name).
pub const AREA_ERROR_NO_LENGTH: &str = "area-error-no-length";

/// `SkeletalTrapezoidationGraph::collapseSmallEdges` meeting a quad middle edge to collapse that
/// has no twin: it logs a warning and `continue`s without moving on, for ever (the page's edge;
/// added by arachne-2, 2026-11-14).
pub const COLLAPSE_NO_TWIN: &str = "collapse-no-twin";

/// `STHalfEdgeNode::isCentral`, `isLocalMaximum` and `isMultiIntersection` following a null edge,
/// `STHalfEdge::canGoUp`, `isUpward` and `distToGoUp` a null next edge, twin or node, and the
/// graph's other operations a null pointer of the half-edge graph (the page's edge, widened to
/// every null pointer the graph's code follows; added by arachne-2, 2026-11-14).
pub const GRAPH_NULL_WALK: &str = "graph-null-walk";

/// `PolylineStitcher<Polygons, Polygon, Point>`, which domain A never uses (the page's edge; added
/// by arachne-2, 2026-11-14).
pub const STITCH_POLYGONS: &str = "stitch-polygons";

/// `PolylineStitcher::stitch` on a line with no junction: it reads the first and the last
/// junction of an empty vector (the page's edge; added by arachne-2, 2026-11-14).
pub const STITCH_EMPTY_LINE: &str = "stitch-empty-line";

/// `SkeletalTrapezoidation::transferEdge` walking a twin's pieces back to a null twin: it logs a
/// warning and `continue`s, and the loop's step follows the null pointer (the page's edge; added
/// by arachne-3, 2026-11-14).
pub const TRANSFER_NO_TWIN: &str = "transfer-no-twin";

/// `SkeletalTrapezoidation::filterOuterCentral`, which only `generateToolpaths` with
/// `filter_outermost_central_edges` calls, and WallToolPaths never passes it (the page's edge;
/// added by arachne-3, 2026-11-14).
pub const FILTER_OUTER_CENTRAL: &str = "filter-outer-central";

/// `SkeletalTrapezoidation::filterTransitionMids` and `isGoingDown` taking the `front()` or
/// `back()` of an empty transition list, `pop_front()` or `pop_back()` of one, or erasing an
/// iterator a previous erase already took out: undefined. No probe record does; the page does not
/// list it (this crate's name; added by arachne-3, 2026-11-14).
pub const TRANSITIONS_EMPTY: &str = "transitions-empty";

/// The four-argument `SkeletalTrapezoidation::interpolate` reading the mix or the right beading
/// past its end, which it does unless the right beading has at most one toolpath location fewer
/// than the left one (the page's edge, widened to every read past the end of a beading's
/// locations in either `interpolate`; added by arachne-4, 2026-11-14).
pub const INTERPOLATE_SHORT_RIGHT: &str = "interpolate-short-right";

/// `SkeletalTrapezoidation::connectJunctions` dereferencing the junctions of the edge before the
/// peak, or of the twin of the edge after it, when that edge has none (an empty `shared_ptr`):
/// `generateJunctions` gives every upward edge of a quad whose ends differ in bead count a list.
/// No probe record does; the page does not list it (this crate's name; added by arachne-4,
/// 2026-11-14).
pub const JUNCTIONS_MISSING: &str = "junctions-missing";

/// `SkeletalTrapezoidation::addToolpathSegment` reading the last junction of an inset's last line
/// when that line has none: every line it and `generateLocalMaximaSingleBeads` make has two or
/// more. The page does not list it (this crate's name; added by arachne-4, 2026-11-14).
pub const TOOLPATH_EMPTY_LINE: &str = "toolpath-empty-line";

/// `make_paths_params` with a minimum feature size, minimum bead width (of the layer), transition
/// filter deviation or transition length option of 0: the option's value is its truth test, so
/// the field is left unset and read uninitialized later (the page's edge; added by arachne-4,
/// 2026-11-14).
pub const PATHS_PARAMS_ZERO: &str = "paths-params-zero";

/// `make_paths_params` with no nozzle diameter: it dereferences `std::min_element` of an empty
/// vector, its end. The page does not list it (this crate's name; added by arachne-4,
/// 2026-11-14).
pub const PATHS_PARAMS_NO_NOZZLE: &str = "paths-params-no-nozzle";

/// `fixSelfIntersections` moving a point away from a segment of zero length: it divides by the
/// segment's length, 0. A prepared outline has no repeated point; the page does not list it (this
/// crate's name; added by arachne-4, 2026-11-14).
pub const SELF_INTERSECTION_ZERO_SEGMENT: &str = "self-intersection-zero-segment";

/// `WallToolPaths::removeSmallLines` on an odd open line with no junction: `shorterThan` starts
/// from the last junction of an empty vector. The page does not list it (this crate's name; added
/// by arachne-4, 2026-11-14).
pub const SMALL_LINE_EMPTY: &str = "small-line-empty";

/// `WallToolPaths::separateOutInnerContour` with an inset count of 0: it divides by it (the
/// reserve of a vector it never uses). `generate` returns before it with no inset; the page does
/// not list it (this crate's name; added by arachne-4, 2026-11-14).
pub const INNER_CONTOUR_NO_INSETS: &str = "inner-contour-no-insets";

/// Stops with orca-index's [`NotPorted`] payload naming the C++ `function` and the `edge`.
pub fn stop(function: &'static str, edge: &'static str) -> ! {
    not_ported(function, edge)
}
