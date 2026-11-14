// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The stops of orca-paths: where ShortestPath's C++ has no answer, this crate stops instead of
//! guessing (added by paths-1, 2026-11-13).
//!
//! OrcaSlicer's release build defines `NDEBUG`, so ShortestPath's asserts do not run, and on some
//! inputs it reads the first point of an empty polyline or follows a pointer one past the end
//! points: undefined behaviour, whose result nothing can copy. `parity/sop1/paths.md` ("Edges")
//! names the first, and no probe record holds one; domain A never reaches any of them. On such an
//! input the crate panics with orca-index's [`NotPorted`] payload ([`stop`]), the one payload type
//! of every phase O3 crate, which `catch_unwind` and `downcast::<NotPorted>()` catch. Its
//! `function` is the C++ function and its `condition` one of the names below.

pub use orca_index::stop::{NotPorted, not_ported};

/// The first or last point of an empty polyline, path or contour, which the C++ reads outside
/// its points for (`parity/sop1/paths.md`, "Edges": "the `first_point()` of an empty polyline or
/// path anywhere in ShortestPath").
pub const EMPTY_POLYLINE: &str = "empty-polyline";

/// A KD tree search of the chaining that finds no end point the filter keeps, so the C++ takes
/// `&end_points[npos]`. The chaining always leaves one (this crate's name; the page does not
/// list it).
pub const NO_END_POINT: &str = "no-end-point";

/// The greedy chaining's walk from a null first or last end point: the second version's loop
/// ends by its iteration limit before it has picked them, and the walk subtracts the end points'
/// address from a null pointer (this crate's name; the page does not list it).
pub const NO_FIRST_POINT: &str = "no-first-point";

/// Stops with orca-index's [`NotPorted`] payload naming the C++ `function` and the `edge`.
pub fn stop(function: &'static str, edge: &'static str) -> ! {
    not_ported(function, edge)
}
