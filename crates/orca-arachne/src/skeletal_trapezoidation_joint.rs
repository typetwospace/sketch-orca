// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Arachne/SkeletalTrapezoidationJoint.hpp lines 1-60
// whole, of OrcaSlicer v2.4.2, translated on 2026-11-14 by arachne-2. Copyright Ultimaker B.V.
// (2020, CuraEngine), with the PrusaSlicer authors' (Prusa Research) and the OrcaSlicer authors'
// changes, under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! `SkeletalTrapezoidationJoint`: the data of a node of the skeletal trapezoidation's graph: its
//! distance to the outline, its bead count, its transition ratio, and a weak pointer to the
//! beading SkeletalTrapezoidation propagates to it ([`BeadingPropagation`], owned by
//! SkeletalTrapezoidation while it runs).
//!
//! The weak pointer is a [`Weak`] of a [`Shared`] storage, as the edge's lists are
//! ([`crate::skeletal_trapezoidation_edge`]): `hasBeading()` is `use_count() > 0`, `setBeading`
//! downgrades the storage and `getBeading()` locks it (`None` once it is gone).

use core::cell::RefCell;
use std::rc::{Rc, Weak};

use orca_geom::point::Coord;

use crate::beading::Beading;
use crate::skeletal_trapezoidation_edge::Shared;

/// `SkeletalTrapezoidationJoint::BeadingPropagation`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BeadingPropagation {
    /// `beading`.
    pub beading: Beading,
    /// `dist_to_bottom_source`.
    pub dist_to_bottom_source: Coord,
    /// `dist_from_top_source`.
    pub dist_from_top_source: Coord,
    /// `is_upward_propagated_only`.
    pub is_upward_propagated_only: bool,
}

impl BeadingPropagation {
    /// `BeadingPropagation(beading)`: both distances 0, not upward only.
    #[must_use]
    pub const fn new(beading: Beading) -> Self {
        Self {
            beading,
            dist_to_bottom_source: 0,
            dist_from_top_source: 0,
            is_upward_propagated_only: false,
        }
    }
}

/// `SkeletalTrapezoidationJoint`.
#[derive(Clone, Debug)]
pub struct SkeletalTrapezoidationJoint {
    /// `distance_to_boundary`.
    pub distance_to_boundary: Coord,
    /// `bead_count`.
    pub bead_count: Coord,
    /// `transition_ratio`: the distance near the skeleton to leave free because this joint is in
    /// the middle of a transition, as a fraction of the inner bead width of the bead at the
    /// higher transition.
    pub transition_ratio: f32,
    /// `beading`.
    beading: Weak<RefCell<BeadingPropagation>>,
}

/// `SkeletalTrapezoidationJoint()`: distance -1, bead count -1, ratio 0, no beading.
impl Default for SkeletalTrapezoidationJoint {
    fn default() -> Self {
        Self::new()
    }
}

impl SkeletalTrapezoidationJoint {
    /// `SkeletalTrapezoidationJoint()`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            distance_to_boundary: -1,
            bead_count: -1,
            transition_ratio: 0.0,
            beading: Weak::new(),
        }
    }

    /// `hasBeading()`: the beading is alive.
    #[must_use]
    pub fn has_beading(&self) -> bool {
        self.beading.strong_count() > 0
    }

    /// `setBeading(storage)`.
    pub fn set_beading(&mut self, storage: &Shared<BeadingPropagation>) {
        self.beading = Rc::downgrade(storage);
    }

    /// `getBeading()`: `None` once the beading is gone.
    #[must_use]
    pub fn get_beading(&self) -> Option<Shared<BeadingPropagation>> {
        self.beading.upgrade()
    }
}
