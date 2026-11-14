// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Arachne/BeadingStrategy/RedistributeBeadingStrategy.hpp
// lines 1-59 and RedistributeBeadingStrategy.cpp lines 1-100 whole, of OrcaSlicer v2.4.2,
// translated on 2026-11-13 by arachne-1. Copyright Ultimaker B.V. (2022, CuraEngine), with the
// PrusaSlicer authors' (Prusa Research) and the OrcaSlicer authors' changes, under the GNU Affero
// General Public License, version 3 (AGPL-3.0; LICENSE).

//! `RedistributeBeadingStrategy`: a meta-strategy that keeps the two outer walls at their own
//! width and hands the rest of the thickness to its parent for the inner walls.
//!
//! Below `minimum_variable_line_ratio` times the outer width no bead is made; up to twice the
//! outer width there are one or two symmetric beads (two above `1 + split threshold` times the
//! outer width); above it the parent's count for the inner thickness plus two. The `coord_t`
//! thickness is compared with those `double` products as a `double`, and the transition
//! thicknesses of no bead and one bead are the products truncated, as the C++'s returns are.

use orca_geom::point::Coord;

use super::{Beading, BeadingStrategy, BeadingStrategyBase, BeadingStrategyPtr};

/// `RedistributeBeadingStrategy`.
pub struct RedistributeBeadingStrategy {
    base: BeadingStrategyBase,
    parent: BeadingStrategyPtr,
    optimal_width_outer: Coord,
    minimum_variable_line_ratio: f64,
}

impl RedistributeBeadingStrategy {
    /// `RedistributeBeadingStrategy(optimal_width_outer, minimum_variable_line_ratio, parent)`:
    /// the parent's base fields copied, then the name "RedistributeBeadingStrategy".
    #[must_use]
    pub fn new(
        optimal_width_outer: Coord,
        minimum_variable_line_ratio: f64,
        parent: BeadingStrategyPtr,
    ) -> Self {
        let mut base = parent.base().clone();
        base.name = "RedistributeBeadingStrategy".to_owned();
        Self {
            base,
            parent,
            optimal_width_outer,
            minimum_variable_line_ratio,
        }
    }

    /// `minimum_variable_line_ratio * optimal_width_outer`, in `double`.
    #[allow(clippy::cast_precision_loss)]
    fn minimum_variable_line(&self) -> f64 {
        self.minimum_variable_line_ratio * self.optimal_width_outer as f64
    }

    /// `(1.0 + parent->getSplitMiddleThreshold()) * optimal_width_outer`, in `double`.
    #[allow(clippy::cast_precision_loss)]
    fn split_width(&self) -> f64 {
        (1.0 + self.parent.get_split_middle_threshold()) * self.optimal_width_outer as f64
    }
}

impl BeadingStrategy for RedistributeBeadingStrategy {
    fn base(&self) -> &BeadingStrategyBase {
        &self.base
    }

    fn get_optimal_thickness(&self, bead_count: Coord) -> Coord {
        let inner_bead_count = orca_stl::max(0, bead_count.wrapping_sub(2));
        let outer_bead_count = bead_count.wrapping_sub(inner_bead_count);
        self.parent
            .get_optimal_thickness(inner_bead_count)
            .wrapping_add(self.optimal_width_outer.wrapping_mul(outer_bead_count))
    }

    fn get_transition_thickness(&self, lower_bead_count: Coord) -> Coord {
        match lower_bead_count {
            0 => orca_crt::f64_to_i64(self.minimum_variable_line()),
            1 => orca_crt::f64_to_i64(self.split_width()),
            _ => self
                .parent
                .get_transition_thickness(lower_bead_count.wrapping_sub(2))
                .wrapping_add(self.optimal_width_outer.wrapping_mul(2)),
        }
    }

    #[allow(clippy::cast_precision_loss)]
    fn get_optimal_bead_count(&self, thickness: Coord) -> Coord {
        if (thickness as f64) < self.minimum_variable_line() {
            return 0;
        }
        if thickness <= self.optimal_width_outer.wrapping_mul(2) {
            return if thickness as f64 > self.split_width() {
                2
            } else {
                1
            };
        }
        self.parent
            .get_optimal_bead_count(
                thickness.wrapping_sub(self.optimal_width_outer.wrapping_mul(2)),
            )
            .wrapping_add(2)
    }

    fn get_transitioning_length(&self, lower_bead_count: Coord) -> Coord {
        self.parent.get_transitioning_length(lower_bead_count)
    }

    fn get_transition_anchor_pos(&self, lower_bead_count: Coord) -> f32 {
        self.parent.get_transition_anchor_pos(lower_bead_count)
    }

    fn to_string(&self) -> String {
        format!("RedistributeBeadingStrategy+{}", self.parent.to_string())
    }

    #[allow(clippy::cast_precision_loss)]
    fn compute(&self, thickness: Coord, bead_count: Coord) -> Beading {
        let mut ret = Beading::default();

        // The situations in which no line is produced.
        if bead_count == 0 || (thickness as f64) < self.minimum_variable_line() {
            ret.left_over = thickness;
            ret.total_thickness = thickness;
            return ret;
        }

        // The beadings of the inner walls, if any.
        let inner_bead_count = bead_count.wrapping_sub(2);
        let inner_thickness = thickness.wrapping_sub(self.optimal_width_outer.wrapping_mul(2));
        if inner_bead_count > 0 && inner_thickness > 0 {
            ret = self.parent.compute(inner_thickness, inner_bead_count);
            for toolpath_location in &mut ret.toolpath_locations {
                *toolpath_location = toolpath_location.wrapping_add(self.optimal_width_outer);
            }
        }

        // The outer wall(s) around the inner wall(s), which may be none.
        let actual_outer_thickness = if bead_count > 2 {
            orca_stl::min(thickness / 2, self.optimal_width_outer)
        } else {
            thickness.wrapping_div(bead_count)
        };
        ret.bead_widths.insert(0, actual_outer_thickness);
        ret.toolpath_locations.insert(0, actual_outer_thickness / 2);
        if bead_count > 1 {
            ret.bead_widths.push(actual_outer_thickness);
            ret.toolpath_locations
                .push(thickness.wrapping_sub(actual_outer_thickness / 2));
        }

        // The total and the left over thickness.
        ret.total_thickness = thickness;
        let mut sum: Coord = 0;
        for w in &ret.bead_widths {
            sum = sum.wrapping_add(*w);
        }
        ret.left_over = thickness.wrapping_sub(sum);
        ret
    }
}
