// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The stops of orca-fill (added by fill-1, 2026-11-14; fill-2's added the same day): where the
//! C++ engine has no answer, where domain A never goes, and where a later piece of the crate has
//! not written its code yet.
//!
//! Each stop panics with orca-index's [`NotPorted`] payload ([`not_ported`]), the one payload type
//! of every phase O3 crate, which `catch_unwind` and `downcast::<NotPorted>()` catch. Its
//! `function` is the C++ function and its `condition` one of the texts below:
//!
//! - a family of `scripts/lib/parity/corpus.ts` for code only a job outside domain A reaches (the
//!   program declines such a job by that name once its objects are placed, so no run gets here);
//! - an input the C++ engine's release build has no answer for (it divides by zero or follows a
//!   null pointer, with its asserts compiled out);
//! - the piece that writes a function whose signature the trait or a caller here needs now: that
//!   piece replaces the stop with the body.

pub use orca_index::stop::{NotPorted, not_ported};

/// The family of the support base pattern (`ipSupportBase`), the one user of
/// `Fill::connect_base_support`: supports mark a job outside domain A.
pub const INFILL_SUPPORTBASE: &str = "infill-supportbase";

/// The code of FillRectilinear.cpp that piece fill-3 (orca-o3-fill-3) ports: the rectilinear
/// fillers' `fill_surface` (from line 1354 on, with the graph walk it calls). fill-3 replaces these
/// stops.
pub const FILL_3: &str = "FillRectilinear.cpp's code that orca-o3-fill-3 ports";

/// `Fill::_infill_direction` on a surface whose `thickness_layers` is 0: the C++ divides the layer
/// id by it (an integer division by zero). Every surface the engine makes is one layer thick or
/// more.
pub const ZERO_THICKNESS_LAYERS: &str = "a surface whose thickness_layers is 0";

/// A contour intersection point whose neighbour on its contour was never set, where the C++
/// follows the null `prev_on_contour` or `next_on_contour`. `create_boundary_infill_graph` links
/// every point that lies on a contour into a ring, and only those are walked.
pub const NULL_NEIGHBOUR: &str = "a contour intersection point without its neighbour";

/// An infill line without points where the C++ reads its first or last point: `take_limited`, and
/// `create_boundary_infill_graph`, which projects both ends of every line onto the boundary. Every
/// line a pattern hands `connect_infill` has two points or more.
pub const EMPTY_POLYLINE: &str = "an infill line without points";

/// `create_boundary_infill_graph` given a boundary contour without points: the C++ reads the
/// contour's last point and the parameter before its first (an index of -1) when it parametrises
/// it. The boundary of an island has no empty contour.
pub const EMPTY_CONTOUR: &str = "a boundary contour without points";

/// `Fill::fill_surface_extrusion` for top solid infill on a filler without a print object config:
/// the C++ reads `calib_flowrate_topinfill_special_order` through the null pointer. `make_fills`
/// sets the config on every filler.
pub const NULL_PRINT_OBJECT_CONFIG: &str = "a filler without its print object config";

/// `Fill::_create_gap_fill` with fill parameters whose region config is null, where the C++ reads
/// `filter_out_gap_fill` through it. `make_fills` sets it for every surface.
pub const NULL_REGION_CONFIG: &str = "fill parameters without their region config";

/// A link of an intersection of the rectilinear infill's vertical lines followed while it is -1
/// (no intersection found), where the C++ indexes a vector with it. Every contour that crosses a
/// vertical line crosses it twice, so the links are found.
pub const NEGATIVE_LINK: &str = "an intersection link of -1";

/// Stops with orca-index's [`NotPorted`] payload naming the C++ `function` and the `condition`.
pub fn stop(function: &'static str, condition: &'static str) -> ! {
    not_ported(function, condition)
}
