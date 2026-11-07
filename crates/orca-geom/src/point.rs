// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! libslic3r's `Point`: a 2D point of `coord_t` (`i64`) coordinates, which Clipper takes as its
//! `IntPoint` (`CLIPPERLIB_INTPOINT_TYPE`).

/// `coord_t`: the integer coordinate of libslic3r's geometry, 64 bits.
pub type Coord = i64;

/// libslic3r's `Point` (`Point.hpp`): `Vec2crd`, an Eigen vector of two `coord_t`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Point {
    /// `x()`.
    pub x: Coord,
    /// `y()`.
    pub y: Coord,
}

/// `Points`: `std::vector<Point>`.
pub type Points = Vec<Point>;

impl Point {
    /// `Point(x, y)`.
    #[must_use]
    pub const fn new(x: Coord, y: Coord) -> Self {
        Self { x, y }
    }
}
