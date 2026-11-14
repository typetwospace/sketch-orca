// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Arachne/BeadingStrategy/BeadingStrategyFactory.hpp lines
// 1-39 and BeadingStrategyFactory.cpp lines 1-58 whole, of OrcaSlicer v2.4.2, translated on
// 2026-11-13 by arachne-1. Copyright Ultimaker B.V. (2022, CuraEngine), with the PrusaSlicer
// authors' (Prusa Research) and the OrcaSlicer authors' changes, under the GNU Affero General
// Public License, version 3 (AGPL-3.0; LICENSE).

//! `BeadingStrategyFactory::makeStrategy`: the stack of strategies WallToolPaths uses.
//!
//! A `DistributedBeadingStrategy` of the outer width when the bead limit is 2 or less (one outer
//! perimeter each side), else of the inner width; a `RedistributeBeadingStrategy` on it; a
//! `WideningBeadingStrategy` when thin walls are printed; an `OuterWallInsetBeadingStrategy`
//! when the outer wall's offset is not 0 (OrcaSlicer allows a negative one); and a
//! `LimitedBeadingStrategy` last, as its zero-width marker beads must not be touched by the
//! others. Each step logs its trace record before it builds its strategy, as the C++ does.
//!
//! [`make_strategy`] takes the thirteen arguments in the C++'s order; [`StrategyArgs`] holds them
//! with the header's defaults (`scaled<coord_t>(0.0005)` for both widths,
//! `scaled<coord_t>(0.0004)`, `float(M_PI / 4.0)`, no thin walls, thresholds of 0.5, a centre wall
//! count of 2 and a minimum variable line ratio of 0.5).

use orca_geom::point::{Coord, PI, scaled};
use orca_mesh::log::{Severity, log};

use super::{
    BeadingStrategyPtr, DistributedBeadingStrategy, LimitedBeadingStrategy,
    OuterWallInsetBeadingStrategy, RedistributeBeadingStrategy, WideningBeadingStrategy,
};

/// `BeadingStrategyFactory::makeStrategy(...)`, its arguments in the C++'s order.
#[must_use]
#[allow(clippy::too_many_arguments)] // The C++ function's thirteen parameters, in its order.
pub fn make_strategy(
    preferred_bead_width_outer: Coord,
    preferred_bead_width_inner: Coord,
    preferred_transition_length: Coord,
    transitioning_angle: f32,
    print_thin_walls: bool,
    min_bead_width: Coord,
    min_feature_size: Coord,
    wall_split_middle_threshold: f64,
    wall_add_middle_threshold: f64,
    max_bead_count: Coord,
    outer_wall_offset: Coord,
    inward_distributed_center_wall_count: i32,
    minimum_variable_line_ratio: f64,
) -> BeadingStrategyPtr {
    // One external perimeter each side: big differences between the inner and the other widths
    // trouble the strategies.
    let optimal_width = if max_bead_count <= 2 {
        preferred_bead_width_outer
    } else {
        preferred_bead_width_inner
    };
    let mut ret: BeadingStrategyPtr = Box::new(DistributedBeadingStrategy::new(
        optimal_width,
        preferred_transition_length,
        f64::from(transitioning_angle),
        wall_split_middle_threshold,
        wall_add_middle_threshold,
        inward_distributed_center_wall_count,
    ));

    log(
        Severity::Trace,
        &format!(
            "Applying the Redistribute meta-strategy with outer-wall width = \
             {preferred_bead_width_outer}, inner-wall width = {preferred_bead_width_inner}."
        ),
    );
    ret = Box::new(RedistributeBeadingStrategy::new(
        preferred_bead_width_outer,
        minimum_variable_line_ratio,
        ret,
    ));

    if print_thin_walls {
        log(
            Severity::Trace,
            &format!(
                "Applying the Widening Beading meta-strategy with minimum input width \
                 {min_feature_size} and minimum output width {min_bead_width}."
            ),
        );
        ret = Box::new(WideningBeadingStrategy::new(
            ret,
            min_feature_size,
            min_bead_width,
        ));
    }
    // OrcaSlicer allows a negative offset here.
    if outer_wall_offset != 0 {
        log(
            Severity::Trace,
            &format!(
                "Applying the OuterWallOffset meta-strategy with offset = {outer_wall_offset}."
            ),
        );
        ret = Box::new(OuterWallInsetBeadingStrategy::new(outer_wall_offset, ret));
    }

    // The limit last: its zero-width marker wall must not be touched by the other strategies.
    log(
        Severity::Trace,
        &format!(
            "Applying the Limited Beading meta-strategy with maximum bead count = \
             {max_bead_count}."
        ),
    );
    Box::new(LimitedBeadingStrategy::new(max_bead_count, ret))
}

/// The arguments of `BeadingStrategyFactory::makeStrategy`, with its header's defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrategyArgs {
    /// `preferred_bead_width_outer`.
    pub preferred_bead_width_outer: Coord,
    /// `preferred_bead_width_inner`.
    pub preferred_bead_width_inner: Coord,
    /// `preferred_transition_length`.
    pub preferred_transition_length: Coord,
    /// `transitioning_angle`.
    pub transitioning_angle: f32,
    /// `print_thin_walls`.
    pub print_thin_walls: bool,
    /// `min_bead_width`.
    pub min_bead_width: Coord,
    /// `min_feature_size`.
    pub min_feature_size: Coord,
    /// `wall_split_middle_threshold`.
    pub wall_split_middle_threshold: f64,
    /// `wall_add_middle_threshold`.
    pub wall_add_middle_threshold: f64,
    /// `max_bead_count`.
    pub max_bead_count: Coord,
    /// `outer_wall_offset`.
    pub outer_wall_offset: Coord,
    /// `inward_distributed_center_wall_count`.
    pub inward_distributed_center_wall_count: i32,
    /// `minimum_variable_line_width` in the header, `minimum_variable_line_ratio` in the
    /// definition.
    pub minimum_variable_line_ratio: f64,
}

impl Default for StrategyArgs {
    #[allow(clippy::cast_possible_truncation)]
    fn default() -> Self {
        Self {
            preferred_bead_width_outer: scaled(0.0005),
            preferred_bead_width_inner: scaled(0.0005),
            preferred_transition_length: scaled(0.0004),
            transitioning_angle: (PI / 4.0) as f32,
            print_thin_walls: false,
            min_bead_width: 0,
            min_feature_size: 0,
            wall_split_middle_threshold: 0.5,
            wall_add_middle_threshold: 0.5,
            max_bead_count: 0,
            outer_wall_offset: 0,
            inward_distributed_center_wall_count: 2,
            minimum_variable_line_ratio: 0.5,
        }
    }
}

impl StrategyArgs {
    /// [`make_strategy`] of these arguments.
    #[must_use]
    pub fn make(&self) -> BeadingStrategyPtr {
        make_strategy(
            self.preferred_bead_width_outer,
            self.preferred_bead_width_inner,
            self.preferred_transition_length,
            self.transitioning_angle,
            self.print_thin_walls,
            self.min_bead_width,
            self.min_feature_size,
            self.wall_split_middle_threshold,
            self.wall_add_middle_threshold,
            self.max_bead_count,
            self.outer_wall_offset,
            self.inward_distributed_center_wall_count,
            self.minimum_variable_line_ratio,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stack_names_each_strategy() {
        let s = StrategyArgs {
            preferred_bead_width_outer: 407_000,
            preferred_bead_width_inner: 407_000,
            print_thin_walls: true,
            outer_wall_offset: -21_500,
            max_bead_count: 4,
            ..StrategyArgs::default()
        }
        .make();
        assert_eq!(
            s.to_string(),
            "LimitedBeadingStrategy+OuterWallOfsetBeadingStrategy+Widening+\
             RedistributeBeadingStrategy+DistributedBeadingStrategy"
        );
    }
}
