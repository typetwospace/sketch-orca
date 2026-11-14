// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: the enums ExtrusionRole and ExtrusionLoopRole and the role predicates of
// src/libslic3r/ExtrusionEntity.hpp (19-103), and ExtrusionEntity::role_to_string and
// string_to_role of ExtrusionEntity.cpp (12, 583-653), of OrcaSlicer v2.4.2, translated on
// 2026-11-13 (orca-o3-layer-1). Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu
// Lab) and the PrusaSlicer authors (Prusa Research), under the GNU Affero General Public License,
// version 3 (AGPL-3.0; LICENSE).

//! The roles of extrusions: [`ExtrusionRole`] (which extruder and speed an extrusion takes),
//! [`ExtrusionLoopRole`] (the flags of a loop), the predicates on a role, and the role names.
//!
//! `role_to_string` names each role in English: the `L()` of `ExtrusionEntity.cpp` marks the text
//! for translation and returns it unchanged. `erCount` has no name: the C++ asserts, which its
//! release build does not, and returns the empty text. `string_to_role` reads the exact names
//! back (case and spaces count) and gives `erNone` for any other text, `"Undefined"` included.

/// `ExtrusionRole` (`uint8_t`): each value names a distinct set of extruder and speed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum ExtrusionRole {
    /// `erNone`.
    #[default]
    None = 0,
    /// `erPerimeter`.
    Perimeter = 1,
    /// `erExternalPerimeter`.
    ExternalPerimeter = 2,
    /// `erOverhangPerimeter`.
    OverhangPerimeter = 3,
    /// `erInternalInfill`.
    InternalInfill = 4,
    /// `erSolidInfill`.
    SolidInfill = 5,
    /// `erTopSolidInfill`.
    TopSolidInfill = 6,
    /// `erBottomSurface`.
    BottomSurface = 7,
    /// `erIroning`.
    Ironing = 8,
    /// `erBridgeInfill`.
    BridgeInfill = 9,
    /// `erInternalBridgeInfill`.
    InternalBridgeInfill = 10,
    /// `erGapFill`.
    GapFill = 11,
    /// `erSkirt`.
    Skirt = 12,
    /// `erBrim`.
    Brim = 13,
    /// `erSupportMaterial`.
    SupportMaterial = 14,
    /// `erSupportMaterialInterface`.
    SupportMaterialInterface = 15,
    /// `erSupportTransition`.
    SupportTransition = 16,
    /// `erWipeTower`.
    WipeTower = 17,
    /// `erCustom`.
    Custom = 18,
    /// `erMixed`: the role of a collection of several roles.
    Mixed = 19,
    /// `erCount`.
    Count = 20,
}

impl ExtrusionRole {
    /// Every role, `erNone` to `erCount`, by value.
    pub const ALL: [ExtrusionRole; 21] = [
        Self::None,
        Self::Perimeter,
        Self::ExternalPerimeter,
        Self::OverhangPerimeter,
        Self::InternalInfill,
        Self::SolidInfill,
        Self::TopSolidInfill,
        Self::BottomSurface,
        Self::Ironing,
        Self::BridgeInfill,
        Self::InternalBridgeInfill,
        Self::GapFill,
        Self::Skirt,
        Self::Brim,
        Self::SupportMaterial,
        Self::SupportMaterialInterface,
        Self::SupportTransition,
        Self::WipeTower,
        Self::Custom,
        Self::Mixed,
        Self::Count,
    ];

    /// The role of a value, `None` past `erCount`.
    #[must_use]
    pub fn from_u8(v: u8) -> Option<Self> {
        Self::ALL.get(usize::from(v)).copied()
    }
}

/// `is_perimeter(role)`: an inner, outer or overhang wall.
#[must_use]
pub fn is_perimeter(role: ExtrusionRole) -> bool {
    role == ExtrusionRole::Perimeter
        || role == ExtrusionRole::ExternalPerimeter
        || role == ExtrusionRole::OverhangPerimeter
}

/// `is_internal_perimeter(role)`.
#[must_use]
pub fn is_internal_perimeter(role: ExtrusionRole) -> bool {
    role == ExtrusionRole::Perimeter
}

/// `is_external_perimeter(role)`.
#[must_use]
pub fn is_external_perimeter(role: ExtrusionRole) -> bool {
    role == ExtrusionRole::ExternalPerimeter
}

/// `is_infill(role)`: the bridges, sparse and solid infill, the surfaces and ironing.
#[must_use]
pub fn is_infill(role: ExtrusionRole) -> bool {
    role == ExtrusionRole::BridgeInfill
        || role == ExtrusionRole::InternalBridgeInfill
        || role == ExtrusionRole::InternalInfill
        || role == ExtrusionRole::SolidInfill
        || role == ExtrusionRole::TopSolidInfill
        || role == ExtrusionRole::BottomSurface
        || role == ExtrusionRole::Ironing
}

/// `is_top_surface(role)`.
#[must_use]
pub fn is_top_surface(role: ExtrusionRole) -> bool {
    role == ExtrusionRole::TopSolidInfill
}

/// `is_solid_infill(role)`: [`is_infill`] but sparse infill.
#[must_use]
pub fn is_solid_infill(role: ExtrusionRole) -> bool {
    role == ExtrusionRole::BridgeInfill
        || role == ExtrusionRole::InternalBridgeInfill
        || role == ExtrusionRole::SolidInfill
        || role == ExtrusionRole::TopSolidInfill
        || role == ExtrusionRole::BottomSurface
        || role == ExtrusionRole::Ironing
}

/// `is_bridge(role)`: the bridges and overhang walls.
#[must_use]
pub fn is_bridge(role: ExtrusionRole) -> bool {
    role == ExtrusionRole::BridgeInfill
        || role == ExtrusionRole::InternalBridgeInfill
        || role == ExtrusionRole::OverhangPerimeter
}

/// `ExtrusionLoopRole` (`uint8_t`): flags, so a value may combine them (`elrHole | elrInternal`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ExtrusionLoopRole(pub u8);

impl ExtrusionLoopRole {
    /// `elrDefault`.
    pub const DEFAULT: Self = Self(0x0);
    /// `elrHole`: the loop of a hole, not of a contour.
    pub const HOLE: Self = Self(0x1);
    /// `elrInternal`: the loop nearest the infill.
    pub const INTERNAL: Self = Self(0x2);
    /// `elrSkirt`.
    pub const SKIRT: Self = Self(0x4);
}

/// `ExtrusionEntity::role_to_string(role)`: the role's English name; `erCount` gives "".
#[must_use]
pub fn role_to_string(role: ExtrusionRole) -> String {
    let name = match role {
        ExtrusionRole::None => "Undefined",
        ExtrusionRole::Perimeter => "Inner wall",
        ExtrusionRole::ExternalPerimeter => "Outer wall",
        ExtrusionRole::OverhangPerimeter => "Overhang wall",
        ExtrusionRole::InternalInfill => "Sparse infill",
        ExtrusionRole::SolidInfill => "Internal solid infill",
        ExtrusionRole::TopSolidInfill => "Top surface",
        ExtrusionRole::BottomSurface => "Bottom surface",
        ExtrusionRole::Ironing => "Ironing",
        ExtrusionRole::BridgeInfill => "Bridge",
        ExtrusionRole::InternalBridgeInfill => "Internal Bridge",
        ExtrusionRole::GapFill => "Gap infill",
        ExtrusionRole::Skirt => "Skirt",
        ExtrusionRole::Brim => "Brim",
        ExtrusionRole::SupportMaterial => "Support",
        ExtrusionRole::SupportMaterialInterface => "Support interface",
        ExtrusionRole::SupportTransition => "Support transition",
        ExtrusionRole::WipeTower => "Prime tower",
        ExtrusionRole::Custom => "Custom",
        ExtrusionRole::Mixed => "Multiple",
        // `default: assert(false);`, then `return "";`.
        ExtrusionRole::Count => "",
    };
    name.to_string()
}

/// `ExtrusionEntity::string_to_role(role)`: the role of an exact name, in the C++'s order of
/// tests; `erNone` for any other text ("Undefined" included).
#[must_use]
pub fn string_to_role(role: &str) -> ExtrusionRole {
    if role == "Inner wall" {
        ExtrusionRole::Perimeter
    } else if role == "Outer wall" {
        ExtrusionRole::ExternalPerimeter
    } else if role == "Overhang wall" {
        ExtrusionRole::OverhangPerimeter
    } else if role == "Sparse infill" {
        ExtrusionRole::InternalInfill
    } else if role == "Internal solid infill" {
        ExtrusionRole::SolidInfill
    } else if role == "Top surface" {
        ExtrusionRole::TopSolidInfill
    } else if role == "Bottom surface" {
        ExtrusionRole::BottomSurface
    } else if role == "Ironing" {
        ExtrusionRole::Ironing
    } else if role == "Bridge" {
        ExtrusionRole::BridgeInfill
    } else if role == "Internal Bridge" {
        ExtrusionRole::InternalBridgeInfill
    } else if role == "Gap infill" {
        ExtrusionRole::GapFill
    } else if role == "Skirt" {
        ExtrusionRole::Skirt
    } else if role == "Brim" {
        ExtrusionRole::Brim
    } else if role == "Support" {
        ExtrusionRole::SupportMaterial
    } else if role == "Support interface" {
        ExtrusionRole::SupportMaterialInterface
    } else if role == "Support transition" {
        ExtrusionRole::SupportTransition
    } else if role == "Prime tower" {
        ExtrusionRole::WipeTower
    } else if role == "Custom" {
        ExtrusionRole::Custom
    } else if role == "Multiple" {
        ExtrusionRole::Mixed
    } else {
        ExtrusionRole::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_read_back() {
        for role in ExtrusionRole::ALL {
            let name = role_to_string(role);
            let back = string_to_role(&name);
            match role {
                ExtrusionRole::None | ExtrusionRole::Count => {
                    assert_eq!(back, ExtrusionRole::None);
                }
                _ => assert_eq!(back, role),
            }
        }
    }

    #[test]
    fn from_u8_stops_past_count() {
        assert_eq!(ExtrusionRole::from_u8(20), Some(ExtrusionRole::Count));
        assert_eq!(ExtrusionRole::from_u8(21), None);
    }
}
