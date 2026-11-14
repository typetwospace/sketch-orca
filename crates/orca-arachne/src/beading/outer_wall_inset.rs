// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Arachne/BeadingStrategy/OuterWallInsetBeadingStrategy.hpp
// lines 1-38 and OuterWallInsetBeadingStrategy.cpp lines 1-62 whole, of OrcaSlicer v2.4.2,
// translated on 2026-11-13 by arachne-1. Copyright Ultimaker B.V. (2022, CuraEngine), with the
// PrusaSlicer authors' (Prusa Research) and the OrcaSlicer authors' changes, under the GNU Affero
// General Public License, version 3 (AGPL-3.0; LICENSE).

//! `OuterWallInsetBeadingStrategy`: a meta-strategy that moves the outer wall's toolpath inward
//! by an offset (OrcaSlicer allows a negative one), never past the middle line, when there are at
//! least two beads of positive width.
//!
//! It overrides neither `getTransitionAnchorPos` nor `getNonlinearThicknesses`, so those run the
//! base class's bodies: the anchor through this strategy's own optimal and transition thicknesses
//! (its parent's), not through the parent's own anchor, and no nonlinear thickness. The name is
//! "OuterWallOfsetBeadingStrategy", spelled as the C++ spells it.

use orca_geom::point::Coord;

use super::{Beading, BeadingStrategy, BeadingStrategyBase, BeadingStrategyPtr};

/// `OuterWallInsetBeadingStrategy`.
pub struct OuterWallInsetBeadingStrategy {
    base: BeadingStrategyBase,
    parent: BeadingStrategyPtr,
    outer_wall_offset: Coord,
}

impl OuterWallInsetBeadingStrategy {
    /// `OuterWallInsetBeadingStrategy(outer_wall_offset, parent)`: the parent's base fields
    /// copied, then the name "OuterWallOfsetBeadingStrategy".
    #[must_use]
    pub fn new(outer_wall_offset: Coord, parent: BeadingStrategyPtr) -> Self {
        let mut base = parent.base().clone();
        base.name = "OuterWallOfsetBeadingStrategy".to_owned();
        Self {
            base,
            parent,
            outer_wall_offset,
        }
    }
}

impl BeadingStrategy for OuterWallInsetBeadingStrategy {
    fn base(&self) -> &BeadingStrategyBase {
        &self.base
    }

    fn get_optimal_thickness(&self, bead_count: Coord) -> Coord {
        self.parent.get_optimal_thickness(bead_count)
    }

    fn get_transition_thickness(&self, lower_bead_count: Coord) -> Coord {
        self.parent.get_transition_thickness(lower_bead_count)
    }

    fn get_optimal_bead_count(&self, thickness: Coord) -> Coord {
        self.parent.get_optimal_bead_count(thickness)
    }

    fn get_transitioning_length(&self, lower_bead_count: Coord) -> Coord {
        self.parent.get_transitioning_length(lower_bead_count)
    }

    fn to_string(&self) -> String {
        format!("OuterWallOfsetBeadingStrategy+{}", self.parent.to_string())
    }

    fn compute(&self, thickness: Coord, bead_count: Coord) -> Beading {
        let mut ret = self.parent.compute(thickness, bead_count);

        // The walls there are, not the zero-width marker walls.
        let walls = ret.bead_widths.iter().filter(|w| **w > 0).count();

        // A single wall is not moved.
        if walls < 2 {
            return ret;
        }

        // The outer wall goes inside, never past the middle line.
        ret.toolpath_locations[0] = orca_stl::min(
            ret.toolpath_locations[0].wrapping_add(self.outer_wall_offset),
            thickness / 2,
        );
        ret
    }
}
