// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Arachne/BeadingStrategy/WideningBeadingStrategy.hpp
// lines 1-50 and WideningBeadingStrategy.cpp lines 1-98 whole, of OrcaSlicer v2.4.2, translated
// on 2026-11-13 by arachne-1. Copyright Ultimaker B.V. (2022, CuraEngine), with the PrusaSlicer
// authors' (Prusa Research) and the OrcaSlicer authors' changes (Orca #14376), under the GNU Affero
// General Public License, version 3 (AGPL-3.0; LICENSE).

//! `WideningBeadingStrategy`: a meta-strategy that prints a part thinner than one line as one
//! line of at least the minimum output width, from the minimum input width up.
//!
//! `compute` makes that one bead only when at most one bead is asked for and the thickness is
//! below `getTransitionThickness(1)` (Orca #14376), its own method, which asks the parent; any
//! other count goes to the parent. The strategy sets no name of its own, so it keeps its
//! parent's (only `toString`, "Widening+...", shows it).

use orca_geom::point::Coord;

use super::{Beading, BeadingStrategy, BeadingStrategyBase, BeadingStrategyPtr};

/// `WideningBeadingStrategy`.
pub struct WideningBeadingStrategy {
    base: BeadingStrategyBase,
    parent: BeadingStrategyPtr,
    min_input_width: Coord,
    min_output_width: Coord,
}

impl WideningBeadingStrategy {
    /// `WideningBeadingStrategy(parent, min_input_width, min_output_width)`: the parent's base
    /// fields copied, its name included.
    #[must_use]
    pub fn new(
        parent: BeadingStrategyPtr,
        min_input_width: Coord,
        min_output_width: Coord,
    ) -> Self {
        let base = parent.base().clone();
        Self {
            base,
            parent,
            min_input_width,
            min_output_width,
        }
    }
}

impl BeadingStrategy for WideningBeadingStrategy {
    fn base(&self) -> &BeadingStrategyBase {
        &self.base
    }

    fn to_string(&self) -> String {
        format!("Widening+{}", self.parent.to_string())
    }

    fn compute(&self, thickness: Coord, bead_count: Coord) -> Beading {
        if bead_count <= 1 && thickness < self.get_transition_thickness(1) {
            let mut ret = Beading {
                total_thickness: thickness,
                ..Beading::default()
            };
            if thickness >= self.min_input_width {
                ret.bead_widths
                    .push(orca_stl::max(thickness, self.min_output_width));
                ret.toolpath_locations.push(thickness / 2);
                ret.left_over = 0;
            } else {
                ret.left_over = thickness;
            }
            return ret;
        }
        self.parent.compute(thickness, bead_count)
    }

    fn get_optimal_thickness(&self, bead_count: Coord) -> Coord {
        self.parent.get_optimal_thickness(bead_count)
    }

    fn get_transition_thickness(&self, lower_bead_count: Coord) -> Coord {
        if lower_bead_count == 0 {
            self.min_input_width
        } else {
            self.parent.get_transition_thickness(lower_bead_count)
        }
    }

    fn get_optimal_bead_count(&self, thickness: Coord) -> Coord {
        if thickness < self.min_input_width {
            return 0;
        }
        let ret = self.parent.get_optimal_bead_count(thickness);
        if thickness >= self.min_input_width && ret < 1 {
            return 1;
        }
        ret
    }

    fn get_transitioning_length(&self, lower_bead_count: Coord) -> Coord {
        self.parent.get_transitioning_length(lower_bead_count)
    }

    fn get_transition_anchor_pos(&self, lower_bead_count: Coord) -> f32 {
        self.parent.get_transition_anchor_pos(lower_bead_count)
    }

    /// The minimum output width, then the parent's.
    fn get_nonlinear_thicknesses(&self, lower_bead_count: Coord) -> Vec<Coord> {
        let mut ret = vec![self.min_output_width];
        ret.extend(self.parent.get_nonlinear_thicknesses(lower_bead_count));
        ret
    }
}
