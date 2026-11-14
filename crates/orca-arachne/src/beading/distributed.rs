// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Arachne/BeadingStrategy/DistributedBeadingStrategy.hpp
// lines 1-41 and DistributedBeadingStrategy.cpp lines 1-100 whole, of OrcaSlicer v2.4.2,
// translated on 2026-11-13 by arachne-1. Copyright Ultimaker B.V. (2022, CuraEngine), with the
// PrusaSlicer authors' (Prusa Research) and the OrcaSlicer authors' changes, under the GNU Affero
// General Public License, version 3 (AGPL-3.0; LICENSE).

//! `DistributedBeadingStrategy`: the bead count whose widths deviate least from the optimal width,
//! the difference spread over the beads by weights.
//!
//! Above two beads the weight of bead `i` is `max(0, 1 - r * (i - middle)^2)` in `float`, with `r`
//! = `(1 / (radius - 1))^2` (`1 / 1 * 1 / 1` below a radius of 2) and `middle` = `(n - 1) / 2`;
//! each bead but the last takes the optimal width plus its share of the difference, `float` times
//! the `coord_t` difference truncated to `coord_t`, and the last takes what is left. Two beads
//! split the thickness in halves, one bead takes it all, none leaves it over. The `float`
//! operations keep the C++'s order: `1.0f / r * 1.0f / r` is `((1 / r) * 1) / r`, the weights'
//! sum is `std::accumulate`'s left fold from `0.f`, and `std::max(0.0f, w)` is `orca_stl::max`.

use orca_geom::point::Coord;

use super::{Beading, BeadingStrategy, BeadingStrategyBase};

/// `DistributedBeadingStrategy`.
#[derive(Clone, Debug)]
pub struct DistributedBeadingStrategy {
    base: BeadingStrategyBase,
    /// `one_over_distribution_radius_squared`: `(1 / distribution_radius)^2`, a `float`.
    one_over_distribution_radius_squared: f32,
}

impl DistributedBeadingStrategy {
    /// `DistributedBeadingStrategy(optimal_width, default_transition_length, transitioning_angle,
    /// wall_split_middle_threshold, wall_add_middle_threshold, distribution_radius)`: the base
    /// constructor narrows the `double` angle to its `float` parameter.
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    pub fn new(
        optimal_width: Coord,
        default_transition_length: Coord,
        transitioning_angle: f64,
        wall_split_middle_threshold: f64,
        wall_add_middle_threshold: f64,
        distribution_radius: i32,
    ) -> Self {
        let mut base = BeadingStrategyBase::new(
            optimal_width,
            wall_split_middle_threshold,
            wall_add_middle_threshold,
            default_transition_length,
            transitioning_angle as f32,
        );
        let one_over_distribution_radius_squared = if distribution_radius >= 2 {
            let r = (distribution_radius - 1) as f32;
            1.0f32 / r * 1.0f32 / r
        } else {
            // `1.0f / 1 * 1.0f / 1`, exactly 1.
            1.0f32
        };
        base.name = "DistributedBeadingStrategy".to_owned();
        Self {
            base,
            one_over_distribution_radius_squared,
        }
    }

    /// The `getWeight` lambda of `compute`: `std::max(0.0f, 1.0f - r * dev * dev)`, `dev` the
    /// bead's index (a `coord_t` made `float`) less the middle.
    #[allow(clippy::cast_precision_loss)]
    fn weight(&self, middle: f32, bead_idx: Coord) -> f32 {
        let dev_from_middle = bead_idx as f32 - middle;
        orca_stl::max(
            0.0f32,
            1.0f32 - self.one_over_distribution_radius_squared * dev_from_middle * dev_from_middle,
        )
    }
}

impl BeadingStrategy for DistributedBeadingStrategy {
    fn base(&self) -> &BeadingStrategyBase {
        &self.base
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss
    )]
    fn compute(&self, thickness: Coord, bead_count: Coord) -> Beading {
        let optimal_width = self.base.optimal_width;
        let mut ret = Beading {
            total_thickness: thickness,
            ..Beading::default()
        };
        if bead_count > 2 {
            let to_be_divided = thickness.wrapping_sub(bead_count.wrapping_mul(optimal_width));
            let middle = (bead_count - 1) as f32 / 2.0;

            let mut weights = vec![0.0f32; bead_count as usize];
            for bead_idx in 0..bead_count {
                weights[bead_idx as usize] = self.weight(middle, bead_idx);
            }

            // std::accumulate(weights.cbegin(), weights.cend(), 0.f): a left fold.
            let mut total_weight = 0.0f32;
            for w in &weights {
                total_weight += *w;
            }
            let mut accumulated_width: Coord = 0;
            for bead_idx in 0..bead_count {
                let weight_fraction = weights[bead_idx as usize] / total_weight;
                let splitup_left_over_weight =
                    orca_crt::f32_to_i64(to_be_divided as f32 * weight_fraction);
                let width = if bead_idx == bead_count - 1 {
                    thickness.wrapping_sub(accumulated_width)
                } else {
                    optimal_width.wrapping_add(splitup_left_over_weight)
                };

                // The toolpath location is the width halved, so it can be off by 1.
                if bead_idx == 0 {
                    ret.toolpath_locations.push(width / 2);
                } else {
                    let back = ret.toolpath_locations[ret.toolpath_locations.len() - 1];
                    let back_width = ret.bead_widths[ret.bead_widths.len() - 1];
                    ret.toolpath_locations
                        .push(back.wrapping_add(back_width.wrapping_add(width) / 2));
                }
                ret.bead_widths.push(width);
                accumulated_width = accumulated_width.wrapping_add(width);
            }
            ret.left_over = 0;
        } else if bead_count == 2 {
            let outer_width = thickness / 2;
            ret.bead_widths.push(outer_width);
            ret.bead_widths.push(outer_width);
            ret.toolpath_locations.push(outer_width / 2);
            ret.toolpath_locations
                .push(thickness.wrapping_sub(outer_width / 2));
            ret.left_over = 0;
        } else if bead_count == 1 {
            let outer_width = thickness;
            ret.bead_widths.push(outer_width);
            ret.toolpath_locations.push(outer_width / 2);
            ret.left_over = 0;
        } else {
            ret.left_over = thickness;
        }
        ret
    }

    /// `getOptimalBeadCount(thickness)`: the lines that fit for sure, and one more when the
    /// remainder reaches the optimal width times the split threshold (an odd count) or the add
    /// threshold (an even one), truncated.
    #[allow(clippy::cast_precision_loss)]
    fn get_optimal_bead_count(&self, thickness: Coord) -> Coord {
        let optimal_width = self.base.optimal_width;
        let naive_count = thickness.wrapping_div(optimal_width);
        let remainder = thickness.wrapping_sub(naive_count.wrapping_mul(optimal_width));
        let threshold = if naive_count % 2 == 1 {
            self.base.wall_split_middle_threshold
        } else {
            self.base.wall_add_middle_threshold
        };
        let minimum_line_width = orca_crt::f64_to_i64(optimal_width as f64 * threshold);
        naive_count.wrapping_add(Coord::from(remainder >= minimum_line_width))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radius_below_two_weighs_by_one() {
        let s = DistributedBeadingStrategy::new(400_000, 400_000, 0.1, 0.5, 0.5, 1);
        assert_eq!(s.one_over_distribution_radius_squared, 1.0);
        assert_eq!(s.to_string(), "DistributedBeadingStrategy");
        let b = s.compute(1_000_000, 0);
        assert_eq!(b.left_over, 1_000_000);
        assert!(b.bead_widths.is_empty());
    }

    #[test]
    fn three_beads_cover_the_thickness() {
        let s = DistributedBeadingStrategy::new(400_000, 400_000, 0.1, 0.5, 0.5, 2);
        let b = s.compute(1_300_000, 3);
        assert_eq!(b.bead_widths.iter().sum::<Coord>(), 1_300_000);
        assert_eq!(b.left_over, 0);
        assert_eq!(s.get_optimal_bead_count(1_300_000), 3);
    }
}
