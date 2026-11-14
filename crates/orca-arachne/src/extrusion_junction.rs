// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Arachne/utils/ExtrusionJunction.hpp lines 1-66 whole, of
// OrcaSlicer v2.4.2, translated on 2026-11-13 by arachne-1. Copyright Ultimaker B.V. (2020,
// CuraEngine), with the PrusaSlicer authors' (Prusa Research) and the OrcaSlicer authors' changes,
// under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! `ExtrusionJunction`: one vertex of a variable-width path: where the path's centre line passes,
//! how wide the path is there, and which perimeter it belongs to (0 the outer wall).
//!
//! The perimeter index is a `size_t` that the constructor takes as a `coord_t`, so a negative
//! index wraps to a large one, as the C++ conversion does. `x()`, `y()` and `z()` (the width) let
//! the templated helpers take a junction as a point with a third coordinate
//! ([`crate::extrusion_line::to_thick_polyline`] takes ClipperLib_Z's points the same way).

use core::ops::Sub;

use orca_geom::point::{Coord, Point};

/// `ExtrusionJunction`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtrusionJunction {
    /// `p`: the position of the path's centre line at the junction.
    pub p: Point,
    /// `w`: the path's width at the junction.
    pub w: Coord,
    /// `perimeter_index`: the perimeter, counted from the outside inward (0 the outer wall).
    pub perimeter_index: usize,
}

impl ExtrusionJunction {
    /// `ExtrusionJunction(p, w, perimeter_index)`: the `coord_t` index converted to `size_t`.
    #[must_use]
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    pub const fn new(p: Point, w: Coord, perimeter_index: Coord) -> Self {
        Self {
            p,
            w,
            perimeter_index: perimeter_index as usize,
        }
    }

    /// `x()`.
    #[must_use]
    pub const fn x(&self) -> Coord {
        self.p.x
    }

    /// `y()`.
    #[must_use]
    pub const fn y(&self) -> Coord {
        self.p.y
    }

    /// `z()`: the width.
    #[must_use]
    pub const fn z(&self) -> Coord {
        self.w
    }
}

/// `operator-(a, b)`: `a.p - b.p`.
impl Sub for ExtrusionJunction {
    type Output = Point;

    fn sub(self, b: Self) -> Point {
        self.p - b.p
    }
}

/// `make_point(ej)`: the junction's point, for the algorithms that take point-like elements.
#[must_use]
pub const fn make_point(ej: &ExtrusionJunction) -> Point {
    ej.p
}

/// `LineJunctions`: the junctions along a line without more information.
pub type LineJunctions = Vec<ExtrusionJunction>;
/// `ExtrusionJunctions`.
pub type ExtrusionJunctions = Vec<ExtrusionJunction>;
