// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Arachne/BeadingStrategy/LimitedBeadingStrategy.hpp lines
// 1-52 and LimitedBeadingStrategy.cpp lines 1-129 whole, of OrcaSlicer v2.4.2, translated on
// 2026-11-13 by arachne-1. Copyright Ultimaker B.V. (2022, CuraEngine), with the PrusaSlicer
// authors' (Prusa Research) and the OrcaSlicer authors' changes, under the GNU Affero General
// Public License, version 3 (AGPL-3.0; LICENSE).

//! `LimitedBeadingStrategy`: a meta-strategy that limits the walls to `max_bead_count` beads and
//! marks the edge of the walled area with a bead of zero width.
//!
//! Up to the limit, `compute` is the parent's, with a zero-width bead inserted in the middle when
//! the parent gave exactly the limit and it is even. Over the limit (the C++ asserts one over and
//! logs a warning for more), it is the parent's beading of the limit's optimal thickness,
//! stretched to the thickness: an odd middle bead takes the extra width, the locations are made
//! symmetric (the loop reads each location after the ones before it were written, so an odd
//! middle location becomes `thickness - thickness / 2`), and two zero-width beads mark the inner
//! edges. Above the limit, `getOptimalThickness` is 1 metre and `getTransitionThickness` 0.9 metre
//! (`scaled<coord_t>(1000.)` and `scaled<coord_t>(900.)`, where the C++ asserts false).
//!
//! The strategy sets no name of its own (its `toString` is "LimitedBeadingStrategy+..."), and it
//! overrides neither `getNonlinearThicknesses` (none) nor the angle and thresholds. Its
//! constructor warns of an odd limit; `%` of a negative limit is negative, so only a positive odd
//! one warns.
//!
//! Stops ([`crate::stop`]): a limit below 2 makes `compute` index -1 (`limited-below-2`: up to the
//! limit only with a limit of 0, over it always), and over the limit a parent that gave fewer than
//! half the limit's beads makes it read and insert past the end (`limited-short-parent`).

use orca_geom::point::{Coord, scaled};
use orca_mesh::log::{Severity, log};

use super::{Beading, BeadingStrategy, BeadingStrategyBase, BeadingStrategyPtr};
use crate::stop::{LIMITED_BELOW_2, LIMITED_SHORT_PARENT, stop};

/// The C++ function the stops name.
const COMPUTE: &str = "LimitedBeadingStrategy::compute";

/// `LimitedBeadingStrategy`.
pub struct LimitedBeadingStrategy {
    base: BeadingStrategyBase,
    max_bead_count: Coord,
    parent: BeadingStrategyPtr,
}

impl LimitedBeadingStrategy {
    /// `LimitedBeadingStrategy(max_bead_count, parent)`: the parent's base fields copied, and a
    /// warning when the limit is odd.
    #[must_use]
    pub fn new(max_bead_count: Coord, parent: BeadingStrategyPtr) -> Self {
        let base = parent.base().clone();
        if max_bead_count % 2 == 1 {
            log(
                Severity::Warning,
                "LimitedBeadingStrategy with odd bead count is odd indeed!",
            );
        }
        Self {
            base,
            max_bead_count,
            parent,
        }
    }

    /// `ret.toolpath_locations.insert(begin + at, location)` and `ret.bead_widths.insert(begin +
    /// at, 0)`: a zero-width marker bead.
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    fn insert_marker(ret: &mut Beading, at: Coord, location: Coord) {
        ret.toolpath_locations.insert(at as usize, location);
        ret.bead_widths.insert(at as usize, 0);
    }
}

impl BeadingStrategy for LimitedBeadingStrategy {
    fn base(&self) -> &BeadingStrategyBase {
        &self.base
    }

    fn to_string(&self) -> String {
        format!("LimitedBeadingStrategy+{}", self.parent.to_string())
    }

    fn get_transitioning_length(&self, lower_bead_count: Coord) -> Coord {
        self.parent.get_transitioning_length(lower_bead_count)
    }

    fn get_transition_anchor_pos(&self, lower_bead_count: Coord) -> f32 {
        self.parent.get_transition_anchor_pos(lower_bead_count)
    }

    #[allow(
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation
    )]
    fn compute(&self, thickness: Coord, bead_count: Coord) -> Beading {
        let max_bead_count = self.max_bead_count;
        if bead_count <= max_bead_count {
            let mut ret = self.parent.compute(thickness, bead_count);
            let bead_count = ret.toolpath_locations.len() as Coord;

            if bead_count % 2 == 0 && bead_count == max_bead_count {
                // A limit of 0 indexes -1 here.
                let at = max_bead_count / 2 - 1;
                if at < 0 {
                    stop(COMPUTE, LIMITED_BELOW_2);
                }
                let innermost_toolpath_location = ret.toolpath_locations[at as usize];
                let innermost_toolpath_width = ret.bead_widths[at as usize];
                Self::insert_marker(
                    &mut ret,
                    max_bead_count / 2,
                    innermost_toolpath_location.wrapping_add(innermost_toolpath_width / 2),
                );
            }
            return ret;
        }
        if bead_count != max_bead_count.wrapping_add(1) {
            log(
                Severity::Warning,
                &format!(
                    "Too many beads! {bead_count} != {}",
                    max_bead_count.wrapping_add(1)
                ),
            );
        }

        let optimal_thickness = self.parent.get_optimal_thickness(max_bead_count);
        let mut ret = self.parent.compute(optimal_thickness, max_bead_count);
        let bead_count = ret.toolpath_locations.len() as Coord;
        ret.left_over = ret
            .left_over
            .wrapping_add(thickness.wrapping_sub(ret.total_thickness));
        ret.total_thickness = thickness;

        // Enforce symmetry.
        if bead_count % 2 == 1 {
            let mid = (bead_count / 2) as usize;
            ret.toolpath_locations[mid] = thickness / 2;
            ret.bead_widths[mid] = thickness.wrapping_sub(optimal_thickness);
        }
        for bead_idx in 0..(bead_count + 1) / 2 {
            ret.toolpath_locations[(bead_count - 1 - bead_idx) as usize] =
                thickness.wrapping_sub(ret.toolpath_locations[bead_idx as usize]);
        }

        // A "fake" inner wall of zero width marks the edge of the walled area, for the structures
        // next to the variable-width walls.
        let at = max_bead_count / 2 - 1;
        if at < 0 {
            stop(COMPUTE, LIMITED_BELOW_2);
        }
        if bead_count < max_bead_count / 2 {
            stop(COMPUTE, LIMITED_SHORT_PARENT);
        }
        let innermost_toolpath_location = ret.toolpath_locations[at as usize];
        let innermost_toolpath_width = ret.bead_widths[at as usize];
        Self::insert_marker(
            &mut ret,
            max_bead_count / 2,
            innermost_toolpath_location.wrapping_add(innermost_toolpath_width / 2),
        );

        // The same on the other side: symmetric, as this point is reached only over the limit.
        let opposite_bead = bead_count - at;
        let innermost_toolpath_location = ret.toolpath_locations[opposite_bead as usize];
        let innermost_toolpath_width = ret.bead_widths[opposite_bead as usize];
        Self::insert_marker(
            &mut ret,
            opposite_bead,
            innermost_toolpath_location.wrapping_sub(innermost_toolpath_width / 2),
        );
        ret
    }

    fn get_optimal_thickness(&self, bead_count: Coord) -> Coord {
        if bead_count <= self.max_bead_count {
            return self.parent.get_optimal_thickness(bead_count);
        }
        // 1 metre (Cura was returning 10), where the C++ asserts false.
        scaled(1000.0)
    }

    fn get_transition_thickness(&self, lower_bead_count: Coord) -> Coord {
        if lower_bead_count < self.max_bead_count {
            return self.parent.get_transition_thickness(lower_bead_count);
        }
        if lower_bead_count == self.max_bead_count {
            return self
                .parent
                .get_optimal_thickness(lower_bead_count.wrapping_add(1))
                .wrapping_sub(scaled(0.01));
        }
        // 0.9 metre, where the C++ asserts false.
        scaled(900.0)
    }

    fn get_optimal_bead_count(&self, thickness: Coord) -> Coord {
        let max_bead_count = self.max_bead_count;
        let parent_bead_count = self.parent.get_optimal_bead_count(thickness);
        if parent_bead_count <= max_bead_count {
            // The C++ asks the parent again.
            self.parent.get_optimal_bead_count(thickness)
        } else if parent_bead_count == max_bead_count.wrapping_add(1) {
            if thickness
                < self
                    .parent
                    .get_optimal_thickness(max_bead_count.wrapping_add(1))
                    .wrapping_sub(scaled(0.01))
            {
                max_bead_count
            } else {
                max_bead_count.wrapping_add(1)
            }
        } else {
            max_bead_count.wrapping_add(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::*;
    use crate::beading::DistributedBeadingStrategy;
    use crate::stop::NotPorted;

    fn limited(max: Coord) -> LimitedBeadingStrategy {
        LimitedBeadingStrategy::new(
            max,
            Box::new(DistributedBeadingStrategy::new(
                400_000, 400_000, 0.1, 0.5, 0.5, 1,
            )),
        )
    }

    #[test]
    fn over_the_limit_marks_both_edges() {
        let s = limited(2);
        let b = s.compute(2_000_000, 3);
        assert_eq!(b.bead_widths, vec![400_000, 0, 0, 400_000]);
        assert_eq!(
            b.toolpath_locations,
            vec![200_000, 400_000, 1_600_000, 1_800_000]
        );
        assert_eq!(b.total_thickness, 2_000_000);
        assert_eq!(b.left_over, 1_200_000);
    }

    #[test]
    fn a_limit_below_two_stops() {
        let s = limited(1);
        let r = catch_unwind(AssertUnwindSafe(|| s.compute(2_000_000, 2)));
        let payload = r.expect_err("a limit of 1 over the limit indexes -1");
        let np = payload
            .downcast_ref::<NotPorted>()
            .expect("a NotPorted payload");
        assert_eq!(np.condition, LIMITED_BELOW_2);
        assert_eq!(np.function, COMPUTE);
        let s = limited(0);
        let r = catch_unwind(AssertUnwindSafe(|| s.compute(0, 0)));
        assert!(r.is_err());
    }
}
