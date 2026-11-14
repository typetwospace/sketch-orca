// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The stops of this crate and its error (added by walls-1, 2026-11-14).
//!
//! Where the C++ reaches a family outside domain A (`parity/sop1/walls.md`, "The pieces and their
//! edges"), the port panics with orca-index's [`NotPorted`] payload, its `function` the C++
//! function and its `condition` the family's name: [`FUZZY_SKIN`] for a wall a region's fuzzy skin
//! would roughen, [`RAFT`] for a raft. Where the C++ has undefined behaviour on an input
//! (an empty list it reads the front of, a division by a zero spacing), the condition names the
//! input instead. The engine never runs on these inputs, so no caller reaches the stops.
//!
//! [`PerimeterError`] is what the C++ throws on the way: a flow too narrow (`Flow`'s
//! `FlowNegativeSpacing` or `FlowNegativeFlow`) and the Voronoi errors of the medial axis.

use core::fmt;

pub use orca_index::stop::{NotPorted, not_ported};
use orca_layer::flow::FlowError;
use orca_voronoi::voronoi_utils::VoronoiError;

/// The family of a region's fuzzy skin (`fuzzy_skin` other than `none` and `disabled_fuzzy`).
pub const FUZZY_SKIN: &str = "fuzzy-skin";
/// The family of a raft (`raft_layers` above 0).
pub const RAFT: &str = "raft";
/// A spacing the C++ divides by, or steps by, that is not positive.
pub const SPACING_NOT_POSITIVE: &str = "a spacing not above 0";
/// A list the C++ reads the front or back of while it is empty.
pub const EMPTY_LIST: &str = "an empty list read at its front";
/// An index of the first unanchored path past the end of the paths.
pub const UNANCHORED_PAST_END: &str = "index_of_first_unanchored past the end of the paths";

/// What the perimeter code throws: `Slic3r::FlowError`s and the Voronoi errors.
#[derive(Clone, Debug, PartialEq)]
pub enum PerimeterError {
    /// A flow error (`Flow::with_width`, `mm3_per_mm`).
    Flow(FlowError),
    /// An error of the Voronoi diagram the medial axis builds.
    Voronoi(VoronoiError),
}

impl PerimeterError {
    /// `what()`: the exception's message.
    #[must_use]
    pub fn what(&self) -> String {
        match self {
            Self::Flow(e) => e.what(),
            Self::Voronoi(e) => e.what().to_string(),
        }
    }
}

impl fmt::Display for PerimeterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.what())
    }
}

impl std::error::Error for PerimeterError {}

impl From<FlowError> for PerimeterError {
    fn from(e: FlowError) -> Self {
        Self::Flow(e)
    }
}

impl From<VoronoiError> for PerimeterError {
    fn from(e: VoronoiError) -> Self {
        Self::Voronoi(e)
    }
}
