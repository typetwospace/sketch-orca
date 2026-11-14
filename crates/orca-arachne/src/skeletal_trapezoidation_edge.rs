// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Arachne/SkeletalTrapezoidationEdge.hpp lines 1-122 whole,
// of OrcaSlicer v2.4.2, translated on 2026-11-14 by arachne-2. Copyright Ultimaker B.V. (2021,
// CuraEngine), with the PrusaSlicer authors' (Prusa Research) and the OrcaSlicer authors' changes,
// under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! `SkeletalTrapezoidationEdge`: the data of an edge of the skeletal trapezoidation's graph: its
//! type, whether it is central, and three weak pointers to lists that SkeletalTrapezoidation owns
//! while it runs (the transition middles, the transition ends and the extrusion junctions along
//! the edge).
//!
//! A C++ `std::weak_ptr<T>` is a [`Weak`] of an `Rc<RefCell<T>>` ([`Shared`]): a default one has a
//! use count of 0, `setX(storage)` downgrades the storage, `getX()` is `lock()` (`None` once the
//! storage is gone, the C++'s empty `shared_ptr`), and `hasX(ignore_empty)` is `use_count() > 0`
//! and, unless `ignore_empty`, a list that is not empty. The graph is built and dropped on one
//! thread, as SkeletalTrapezoidation is, so the counts need not be atomic. A `std::list` of
//! transitions is a [`StableList`], whose handles ([`ListIt`]) stay valid as the list changes, as
//! the list's iterators do; the junctions are a `std::vector` ([`LineJunctions`]).
//!
//! `isCentral()` asserts that centrality was set; a release build reads an unknown one as not
//! central, and so does [`SkeletalTrapezoidationEdge::is_central`].

use core::cell::RefCell;
use std::rc::{Rc, Weak};

use orca_geom::point::Coord;

use crate::extrusion_junction::LineJunctions;
use crate::half_edge::{ListKey, StableList};

/// A `std::shared_ptr<T>` to storage the graph's data points to weakly.
pub type Shared<T> = Rc<RefCell<T>>;

/// An iterator into one of an edge's `std::list`s: the handle of an element of a [`StableList`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ListIt(u32);

impl ListKey for ListIt {
    fn from_slot(slot: u32) -> Self {
        Self(slot)
    }
    fn slot(self) -> u32 {
        self.0
    }
}

/// `SkeletalTrapezoidationEdge::TransitionMiddle`: where the anchor of a transition sits along
/// an edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransitionMiddle {
    /// `pos`: the position along the edge, measured from `edge.from.p`.
    pub pos: Coord,
    /// `lower_bead_count`.
    pub lower_bead_count: i32,
    /// `feature_radius`: the feature radius at which this transition is placed.
    pub feature_radius: Coord,
}

impl TransitionMiddle {
    /// `TransitionMiddle(pos, lower_bead_count, feature_radius)`.
    #[must_use]
    pub const fn new(pos: Coord, lower_bead_count: i32, feature_radius: Coord) -> Self {
        Self {
            pos,
            lower_bead_count,
            feature_radius,
        }
    }
}

/// `SkeletalTrapezoidationEdge::TransitionEnd`: where the lower or the upper end of a transition
/// sits along an edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransitionEnd {
    /// `pos`: the position along the edge, measured from `edge.from.p`, the half edge always
    /// oriented from the lower to the higher radius.
    pub pos: Coord,
    /// `lower_bead_count`.
    pub lower_bead_count: i32,
    /// `is_lower_end`: whether this is the end of the transition with the lower bead count.
    pub is_lower_end: bool,
}

impl TransitionEnd {
    /// `TransitionEnd(pos, lower_bead_count, is_lower_end)`.
    #[must_use]
    pub const fn new(pos: Coord, lower_bead_count: i32, is_lower_end: bool) -> Self {
        Self {
            pos,
            lower_bead_count,
            is_lower_end,
        }
    }
}

/// `std::list<TransitionMiddle>`.
pub type TransitionMiddles = StableList<ListIt, TransitionMiddle>;
/// `std::list<TransitionEnd>`.
pub type TransitionEnds = StableList<ListIt, TransitionEnd>;

/// `SkeletalTrapezoidationEdge::EdgeType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum EdgeType {
    /// `NORMAL` (0): from the Voronoi diagram.
    Normal = 0,
    /// `EXTRA_VD` (1): introduced to the Voronoi diagram in order to make the gMAT.
    ExtraVd = 1,
    /// `TRANSITION_END` (2): introduced to the Voronoi diagram in order to make the gMAT.
    TransitionEnd = 2,
}

/// `SkeletalTrapezoidationEdge::Central`: -1 unknown, 0 no, 1 yes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Central {
    Unknown,
    No,
    Yes,
}

/// `SkeletalTrapezoidationEdge`.
#[derive(Clone, Debug)]
pub struct SkeletalTrapezoidationEdge {
    /// `type`.
    pub edge_type: EdgeType,
    /// `is_central`: whether the edge is significant (whether the source segments have a sharp
    /// angle); unknown at first.
    is_central: Central,
    /// `transitions`.
    transitions: Weak<RefCell<TransitionMiddles>>,
    /// `transition_ends`.
    transition_ends: Weak<RefCell<TransitionEnds>>,
    /// `extrusion_junctions`.
    extrusion_junctions: Weak<RefCell<LineJunctions>>,
}

/// `SkeletalTrapezoidationEdge()`: a normal edge.
impl Default for SkeletalTrapezoidationEdge {
    fn default() -> Self {
        Self::new(EdgeType::Normal)
    }
}

impl SkeletalTrapezoidationEdge {
    /// `SkeletalTrapezoidationEdge(type)`: centrality unknown, no lists.
    #[must_use]
    pub const fn new(edge_type: EdgeType) -> Self {
        Self {
            edge_type,
            is_central: Central::Unknown,
            transitions: Weak::new(),
            transition_ends: Weak::new(),
            extrusion_junctions: Weak::new(),
        }
    }

    /// `isCentral()`: centrality set to yes (an unknown one reads as no, as in a release build).
    #[must_use]
    pub fn is_central(&self) -> bool {
        self.is_central == Central::Yes
    }

    /// `setIsCentral(b)`.
    pub fn set_is_central(&mut self, b: bool) {
        self.is_central = if b { Central::Yes } else { Central::No };
    }

    /// `centralIsSet()`.
    #[must_use]
    pub fn central_is_set(&self) -> bool {
        self.is_central != Central::Unknown
    }

    /// `hasTransitions(ignore_empty = false)`: the list is alive and, unless `ignore_empty`, not
    /// empty.
    #[must_use]
    pub fn has_transitions(&self, ignore_empty: bool) -> bool {
        self.transitions.strong_count() > 0
            && (ignore_empty
                || self
                    .get_transitions()
                    .is_some_and(|t| !t.borrow().is_empty()))
    }

    /// `setTransitions(storage)`.
    pub fn set_transitions(&mut self, storage: &Shared<TransitionMiddles>) {
        self.transitions = Rc::downgrade(storage);
    }

    /// `getTransitions()`: `None` once the list is gone.
    #[must_use]
    pub fn get_transitions(&self) -> Option<Shared<TransitionMiddles>> {
        self.transitions.upgrade()
    }

    /// `hasTransitionEnds(ignore_empty = false)`.
    #[must_use]
    pub fn has_transition_ends(&self, ignore_empty: bool) -> bool {
        self.transition_ends.strong_count() > 0
            && (ignore_empty
                || self
                    .get_transition_ends()
                    .is_some_and(|t| !t.borrow().is_empty()))
    }

    /// `setTransitionEnds(storage)`.
    pub fn set_transition_ends(&mut self, storage: &Shared<TransitionEnds>) {
        self.transition_ends = Rc::downgrade(storage);
    }

    /// `getTransitionEnds()`: `None` once the list is gone.
    #[must_use]
    pub fn get_transition_ends(&self) -> Option<Shared<TransitionEnds>> {
        self.transition_ends.upgrade()
    }

    /// `hasExtrusionJunctions(ignore_empty = false)`.
    #[must_use]
    pub fn has_extrusion_junctions(&self, ignore_empty: bool) -> bool {
        self.extrusion_junctions.strong_count() > 0
            && (ignore_empty
                || self
                    .get_extrusion_junctions()
                    .is_some_and(|j| !j.borrow().is_empty()))
    }

    /// `setExtrusionJunctions(storage)`.
    pub fn set_extrusion_junctions(&mut self, storage: &Shared<LineJunctions>) {
        self.extrusion_junctions = Rc::downgrade(storage);
    }

    /// `getExtrusionJunctions()`: `None` once the junctions are gone.
    #[must_use]
    pub fn get_extrusion_junctions(&self) -> Option<Shared<LineJunctions>> {
        self.extrusion_junctions.upgrade()
    }
}

#[cfg(test)]
mod tests {
    use core::cell::RefCell;
    use std::rc::Rc;

    use super::{EdgeType, SkeletalTrapezoidationEdge, TransitionMiddle, TransitionMiddles};

    #[test]
    fn a_weak_list_dies_with_its_storage() {
        let mut e = SkeletalTrapezoidationEdge::new(EdgeType::ExtraVd);
        assert!(!e.central_is_set() && !e.is_central());
        e.set_is_central(true);
        assert!(e.central_is_set() && e.is_central());
        assert!(!e.has_transitions(true));
        let storage = Rc::new(RefCell::new(TransitionMiddles::new()));
        e.set_transitions(&storage);
        assert!(e.has_transitions(true) && !e.has_transitions(false));
        storage
            .borrow_mut()
            .push_back(TransitionMiddle::new(5, 1, 100));
        assert!(e.has_transitions(false));
        let copy = e.clone();
        drop(storage);
        assert!(!e.has_transitions(true) && e.get_transitions().is_none());
        assert!(!copy.has_transitions(true));
    }
}
