// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Boost code: Boost.Polygon of Boost 1.84.0, translated on 2026-11-13 by orca-o3-voronoi-2
// from boost/polygon/voronoi_geometry_type.hpp, lines 1-48 (GeometryCategory, SourceCategory and
// belongs). Copyright Andrii Sydorchuk 2010-2012, under the Boost Software License, version 1.0
// (BSL-1.0; licenses/BSL-1.0.txt).

//! Boost.Polygon's site categories (`voronoi_geometry_type.hpp`): what kind of input a site event,
//! and later a Voronoi cell, comes from.
//!
//! - [`GeometryCategory`] is the site's topology: a point or a segment.
//! - [`SourceCategory`] is the input it comes from: a single point, a segment's start or end
//!   point, or a segment as given or reversed. The C++ enum also holds two constants that are no
//!   category (`SOURCE_CATEGORY_GEOMETRY_SHIFT` and `SOURCE_CATEGORY_BITMASK`), and a site keeps
//!   its category as bits it ORs into its flags (`site_event::source_category`), so the type here
//!   is a value of those bits, not a closed Rust enum: a value read back from the flags is any
//!   value of the mask, as the C++'s `static_cast<SourceCategory>` gives it.
//! - [`belongs`] tells a source category's geometry: its value shifted down by 3.
//!
//! Nothing of these lines is left out.

// ---------------------------------------------------------------- GeometryCategory (17-21)

/// `GeometryCategory`: the topology of a Voronoi site.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum GeometryCategory {
    /// `GEOMETRY_CATEGORY_POINT = 0x0`.
    Point = 0x0,
    /// `GEOMETRY_CATEGORY_SEGMENT = 0x1`.
    Segment = 0x1,
}

// ---------------------------------------------------------------- SourceCategory (23-36)

/// `SourceCategory`: the input source a site (and the cell it forms) comes from, as the bits the
/// C++ enum value holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SourceCategory(pub u32);

impl SourceCategory {
    /// `SOURCE_CATEGORY_SINGLE_POINT = 0x0`.
    pub const SINGLE_POINT: Self = Self(0x0);
    /// `SOURCE_CATEGORY_SEGMENT_START_POINT = 0x1`.
    pub const SEGMENT_START_POINT: Self = Self(0x1);
    /// `SOURCE_CATEGORY_SEGMENT_END_POINT = 0x2`.
    pub const SEGMENT_END_POINT: Self = Self(0x2);
    /// `SOURCE_CATEGORY_INITIAL_SEGMENT = 0x8`.
    pub const INITIAL_SEGMENT: Self = Self(0x8);
    /// `SOURCE_CATEGORY_REVERSE_SEGMENT = 0x9`.
    pub const REVERSE_SEGMENT: Self = Self(0x9);
    /// `SOURCE_CATEGORY_GEOMETRY_SHIFT = 0x3`: the shift from a source category to its geometry.
    pub const GEOMETRY_SHIFT: Self = Self(0x3);
    /// `SOURCE_CATEGORY_BITMASK = 0x1F`: the bits of a site's flags that hold its category.
    pub const BITMASK: Self = Self(0x1F);

    /// The value as `std::size_t`, as the C++ converts the enum.
    #[must_use]
    pub fn bits(self) -> usize {
        self.0 as usize
    }
}

// ---------------------------------------------------------------- belongs (38-44)

/// `belongs(source_category, geometry_category)`: the source category shifted down by
/// `SOURCE_CATEGORY_GEOMETRY_SHIFT` is the geometry category.
#[must_use]
pub fn belongs(source_category: SourceCategory, geometry_category: GeometryCategory) -> bool {
    (source_category.bits() >> SourceCategory::GEOMETRY_SHIFT.bits())
        == geometry_category as u32 as usize
}

#[cfg(test)]
mod tests {
    use super::{GeometryCategory, SourceCategory, belongs};

    #[test]
    fn categories_belong_to_their_geometry() {
        for c in [
            SourceCategory::SINGLE_POINT,
            SourceCategory::SEGMENT_START_POINT,
            SourceCategory::SEGMENT_END_POINT,
        ] {
            assert!(belongs(c, GeometryCategory::Point));
            assert!(!belongs(c, GeometryCategory::Segment));
        }
        for c in [
            SourceCategory::INITIAL_SEGMENT,
            SourceCategory::REVERSE_SEGMENT,
        ] {
            assert!(belongs(c, GeometryCategory::Segment));
            assert!(!belongs(c, GeometryCategory::Point));
        }
    }
}
