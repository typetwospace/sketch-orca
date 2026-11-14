// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Arachne/BeadingStrategy/BeadingStrategy.hpp lines 1-121
// and BeadingStrategy.cpp lines 1-77 whole, of OrcaSlicer v2.4.2, translated on 2026-11-13 by
// arachne-1. Arachne comes from CuraEngine: copyright Ultimaker B.V. (2022), with the PrusaSlicer
// authors' (Prusa Research) and the OrcaSlicer authors' changes, under the GNU Affero General
// Public License, version 3 (AGPL-3.0; LICENSE; CuraEngine's "AGPLv3 or higher" taken as version
// 3).

//! The beading strategies: how many beads (walls) cover a given horizontal thickness of the
//! model, and how wide each is (`BeadingStrategy` and its five kinds).
//!
//! - [`BeadingStrategy`] is the C++ abstract class: `compute` and `getOptimalBeadCount` are pure
//!   virtual; the others have the base class's bodies as provided methods, which call the other
//!   methods through `self`, so a strategy that overrides `getOptimalThickness` changes what the
//!   base `getTransitionThickness` and `getTransitionAnchorPos` compute, as C++'s virtual calls
//!   do. `getSplitMiddleThreshold` and `getTransitioningAngle` are not virtual in C++ and no
//!   strategy overrides them here.
//! - [`BeadingStrategyBase`] holds the base class's fields. A meta-strategy's constructor copies
//!   its parent's (`BeadingStrategy(*parent)`), its name included, before it sets its own.
//! - The five strategies are [`distributed`], [`limited`], [`outer_wall_inset`], [`redistribute`]
//!   and [`widening`]; [`factory`] stacks them as `BeadingStrategyFactory::makeStrategy` does.
//!
//! Every `float` is `f32` and every `double` `f64`, converted where the C++ converts: the base
//! constructor takes the transitioning angle as a `float` (a `DistributedBeadingStrategy` given a
//! `double` angle narrows it there) and keeps it as a `double`; `coord_t` times a `double`
//! truncates to `coord_t` through `orca_crt::f64_to_i64`. `coord_t` arithmetic wraps, as MSVC's
//! does. `pi_div(3)`, the default angle of the base constructor, deduces `T` as `int`, so it is
//! `int(M_PI) / 3`, which is 1 ([`pi_div_int`]); every strategy passes its own angle.
//!
//! The logs (the factory's trace records, `LimitedBeadingStrategy`'s warnings) go through
//! `orca_mesh::log` at the C++'s severity, the integers in them as `std::ostream` writes a
//! `long long` (decimal, a minus sign for a negative), which Rust's `Display` of an `i64` gives.

pub mod distributed;
pub mod factory;
pub mod limited;
pub mod outer_wall_inset;
pub mod redistribute;
pub mod widening;

use orca_geom::point::{Coord, PI, scaled};

pub use distributed::DistributedBeadingStrategy;
pub use factory::{StrategyArgs, make_strategy};
pub use limited::LimitedBeadingStrategy;
pub use outer_wall_inset::OuterWallInsetBeadingStrategy;
pub use redistribute::RedistributeBeadingStrategy;
pub use widening::WideningBeadingStrategy;

/// `pi_div<int>(div)`: `static_cast<int>(M_PI) / div`, so `pi_div(3)` is 1.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub const fn pi_div_int(div: i32) -> i32 {
    // static_cast<int>(M_PI) truncates 3.14159... to 3.
    (PI as i32) / div
}

/// `pi_div<float>(div)`: `static_cast<float>(M_PI) / div`.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn pi_div_f32(div: f32) -> f32 {
    (PI as f32) / div
}

/// `pi_div<double>(div)`: `M_PI / div`.
#[must_use]
pub fn pi_div_f64(div: f64) -> f64 {
    PI / div
}

/// The base constructor's default `transitioning_angle`, `pi_div(3)`: the `int` 1 as a `float`.
#[allow(clippy::cast_precision_loss)]
pub const DEFAULT_TRANSITIONING_ANGLE: f32 = pi_div_int(3) as f32;

/// `BeadingStrategy::Beading`: the beading for a given horizontal model thickness.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Beading {
    /// `total_thickness`.
    pub total_thickness: Coord,
    /// `bead_widths`: the line width of each bead from the outer inset inward.
    pub bead_widths: Vec<Coord>,
    /// `toolpath_locations`: the distance of each bead's toolpath from the outline.
    pub toolpath_locations: Vec<Coord>,
    /// `left_over`: the distance no bead covers.
    pub left_over: Coord,
}

/// The fields of the C++ base class `BeadingStrategy`, which every strategy holds and a
/// meta-strategy copies from its parent.
#[derive(Clone, Debug, PartialEq)]
pub struct BeadingStrategyBase {
    /// `name`: "Unknown" until a strategy sets its own; a copy keeps the parent's.
    pub name: String,
    /// `optimal_width`: the nominal bead width.
    pub optimal_width: Coord,
    /// `wall_split_middle_threshold`: when a middle wall splits in two, as a ratio of the optimal
    /// width.
    pub wall_split_middle_threshold: f64,
    /// `wall_add_middle_threshold`: when a middle wall is added between an even number of walls,
    /// as a ratio of the optimal width.
    pub wall_add_middle_threshold: f64,
    /// `default_transition_length`: the length of the region between two bead counts.
    pub default_transition_length: Coord,
    /// `transitioning_angle`: a `double` set from the constructor's `float`.
    pub transitioning_angle: f64,
}

impl BeadingStrategyBase {
    /// `BeadingStrategy(optimal_width, wall_split_middle_threshold, wall_add_middle_threshold,
    /// default_transition_length, transitioning_angle)`: the name "Unknown".
    #[must_use]
    pub fn new(
        optimal_width: Coord,
        wall_split_middle_threshold: f64,
        wall_add_middle_threshold: f64,
        default_transition_length: Coord,
        transitioning_angle: f32,
    ) -> Self {
        Self {
            name: "Unknown".to_owned(),
            optimal_width,
            wall_split_middle_threshold,
            wall_add_middle_threshold,
            default_transition_length,
            transitioning_angle: f64::from(transitioning_angle),
        }
    }
}

/// `BeadingStrategy`: a strategy for covering a given (constant) horizontal model thickness with
/// a number of beads, which may have different widths. The provided methods are the base class's
/// virtual methods; a strategy overrides those its C++ class overrides.
pub trait BeadingStrategy: Send + Sync {
    /// The base class's fields.
    fn base(&self) -> &BeadingStrategyBase;

    /// `compute(thickness, bead_count)`: the bead widths with which to cover `thickness`; the
    /// bead count may differ from the optimal one.
    fn compute(&self, thickness: Coord, bead_count: Coord) -> Beading;

    /// `getOptimalBeadCount(thickness)`: the number of beads to use for a model thickness.
    fn get_optimal_bead_count(&self, thickness: Coord) -> Coord;

    /// `getOptimalThickness(bead_count)`: `optimal_width * bead_count`.
    fn get_optimal_thickness(&self, bead_count: Coord) -> Coord {
        self.base().optimal_width.wrapping_mul(bead_count)
    }

    /// `getTransitionThickness(lower_bead_count)`: the thickness at which the optimal bead count
    /// goes from `lower_bead_count` to one more: the lower optimum plus the split threshold (an
    /// odd lower count) or the add threshold (an even one) times the gap to the next optimum,
    /// in `double`, truncated.
    fn get_transition_thickness(&self, lower_bead_count: Coord) -> Coord {
        let lower_ideal_width = self.get_optimal_thickness(lower_bead_count);
        let higher_ideal_width = self.get_optimal_thickness(lower_bead_count.wrapping_add(1));
        let base = self.base();
        // `%` of a negative count is negative, as in C++, so -1 takes the add threshold.
        let threshold = if lower_bead_count % 2 == 1 {
            base.wall_split_middle_threshold
        } else {
            base.wall_add_middle_threshold
        };
        orca_crt::f64_to_i64(
            lower_ideal_width as f64
                + threshold * higher_ideal_width.wrapping_sub(lower_ideal_width) as f64,
        )
    }

    /// `getTransitioningLength(lower_bead_count)`: `scaled<coord_t>(0.01)` from no bead, else the
    /// default transition length.
    fn get_transitioning_length(&self, lower_bead_count: Coord) -> Coord {
        if lower_bead_count == 0 {
            return scaled(0.01);
        }
        self.base().default_transition_length
    }

    /// `getTransitionAnchorPos(lower_bead_count)`: `1.0 - float(transition - lower optimum) /
    /// float(upper optimum - lower optimum)`, the quotient in `float`, the difference in `double`,
    /// returned as a `float`.
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    fn get_transition_anchor_pos(&self, lower_bead_count: Coord) -> f32 {
        let lower_optimum = self.get_optimal_thickness(lower_bead_count);
        let transition_point = self.get_transition_thickness(lower_bead_count);
        let upper_optimum = self.get_optimal_thickness(lower_bead_count.wrapping_add(1));
        let q = transition_point.wrapping_sub(lower_optimum) as f32
            / upper_optimum.wrapping_sub(lower_optimum) as f32;
        (1.0 - f64::from(q)) as f32
    }

    /// `getNonlinearThicknesses(lower_bead_count)`: the thicknesses in a bead count's region
    /// where `compute` bends; none.
    fn get_nonlinear_thicknesses(&self, _lower_bead_count: Coord) -> Vec<Coord> {
        Vec::new()
    }

    /// `toString()`: the name.
    fn to_string(&self) -> String {
        self.base().name.clone()
    }

    /// `getSplitMiddleThreshold()` (not virtual in C++).
    fn get_split_middle_threshold(&self) -> f64 {
        self.base().wall_split_middle_threshold
    }

    /// `getTransitioningAngle()` (not virtual in C++).
    fn get_transitioning_angle(&self) -> f64 {
        self.base().transitioning_angle
    }
}

/// `BeadingStrategyPtr`: `std::unique_ptr<BeadingStrategy>`.
pub type BeadingStrategyPtr = Box<dyn BeadingStrategy>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pi_div_of_an_int_is_one() {
        assert_eq!(pi_div_int(3), 1);
        assert_eq!(DEFAULT_TRANSITIONING_ANGLE.to_bits(), 1.0f32.to_bits());
        assert_eq!(pi_div_f64(4.0).to_bits(), (PI / 4.0).to_bits());
    }

    #[test]
    fn base_constructor_keeps_the_float_angle() {
        let b = BeadingStrategyBase::new(400_000, 0.5, 0.5, 400_000, 0.1);
        assert_eq!(b.name, "Unknown");
        assert_eq!(b.transitioning_angle.to_bits(), f64::from(0.1f32).to_bits());
    }
}
