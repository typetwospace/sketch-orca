// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.
//
// Carries OrcaSlicer code: MaterialTypeInfo and MaterialType (all, find and the five getters, with
// their defaults and the table of 75 materials) are translated from src/libslic3r/MaterialType.hpp
// (lines 1-32) and MaterialType.cpp (lines 1-170) of OrcaSlicer v2.4.2, commit 8500fcd, on
// 2026-11-13 (orca-o3-front-1); their copyright is the OrcaSlicer contributors', under the
// AGPL-3.0.

//! `MaterialType`: what Orca knows of each filament type ([`MaterialTypeInfo`]: its nozzle and
//! chamber temperature ranges, adhesion coefficient, yield strength and thermal length), and the
//! lookups by exact name.
//!
//! - [`all`] is the table, in Orca's order; [`find`] is a linear search for the exact name (case
//!   and spaces count: "pla" and "PLA " find nothing).
//! - Each getter first writes its defaults (190 to 300 degrees, a chamber of 0 to 100, adhesion
//!   1, yield strength 0.02, thermal length 200), then the material's values when the name is
//!   found, and says whether it was. The C++ writes through references; these return the values.

/// `MaterialTypeInfo`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaterialTypeInfo {
    pub name: &'static str,
    pub min_temp: i32,
    pub max_temp: i32,
    pub chamber_min_temp: i32,
    pub chamber_max_temp: i32,
    pub adhesion_coefficient: f64,
    pub yield_strength: f64,
    pub thermal_length: f64,
}

const DEFAULT_MIN_TEMP: i32 = 190;
const DEFAULT_MAX_TEMP: i32 = 300;
const DEFAULT_CHAMBER_MIN_TEMP: i32 = 0;
const DEFAULT_CHAMBER_MAX_TEMP: i32 = 100;
const DEFAULT_ADHESION_COEFFICIENT: f64 = 1.0;
const DEFAULT_YIELD_STRENGTH: f64 = 0.02;
const DEFAULT_THERMAL_LENGTH: f64 = 200.0;

/// `material_types`, the table `MaterialType::all()` returns.
#[rustfmt::skip]
static MATERIAL_TYPES: &[MaterialTypeInfo] = &[
    MaterialTypeInfo { name: "ABS", min_temp: 190, max_temp: 300, chamber_min_temp: 50, chamber_max_temp: 65, adhesion_coefficient: 1.0, yield_strength: 0.1, thermal_length: 100.0 },
    MaterialTypeInfo { name: "ABS-CF", min_temp: 220, max_temp: 300, chamber_min_temp: 50, chamber_max_temp: 65, adhesion_coefficient: 1.0, yield_strength: 0.1, thermal_length: 100.0 },
    MaterialTypeInfo { name: "ABS-GF", min_temp: 240, max_temp: 280, chamber_min_temp: 50, chamber_max_temp: 65, adhesion_coefficient: 1.0, yield_strength: 0.1, thermal_length: 100.0 },
    MaterialTypeInfo { name: "ASA", min_temp: 220, max_temp: 300, chamber_min_temp: 50, chamber_max_temp: 65, adhesion_coefficient: 1.0, yield_strength: 0.1, thermal_length: 100.0 },
    MaterialTypeInfo { name: "ASA-CF", min_temp: 230, max_temp: 300, chamber_min_temp: 50, chamber_max_temp: 65, adhesion_coefficient: 1.0, yield_strength: 0.1, thermal_length: 100.0 },
    MaterialTypeInfo { name: "ASA-GF", min_temp: 240, max_temp: 300, chamber_min_temp: 50, chamber_max_temp: 65, adhesion_coefficient: 1.0, yield_strength: 0.1, thermal_length: 100.0 },
    MaterialTypeInfo { name: "ASA-AERO", min_temp: 240, max_temp: 280, chamber_min_temp: 50, chamber_max_temp: 65, adhesion_coefficient: 1.0, yield_strength: 0.1, thermal_length: 100.0 },
    MaterialTypeInfo { name: "BVOH", min_temp: 190, max_temp: 240, chamber_min_temp: 0, chamber_max_temp: 70, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "CoPE", min_temp: 190, max_temp: 240, chamber_min_temp: 0, chamber_max_temp: 45, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "EVA", min_temp: 175, max_temp: 220, chamber_min_temp: 0, chamber_max_temp: 50, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "FLEX", min_temp: 210, max_temp: 230, chamber_min_temp: 0, chamber_max_temp: 50, adhesion_coefficient: 0.5, yield_strength: 0.02, thermal_length: 1000.0 },
    MaterialTypeInfo { name: "HIPS", min_temp: 220, max_temp: 270, chamber_min_temp: 50, chamber_max_temp: 60, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PA", min_temp: 235, max_temp: 280, chamber_min_temp: 50, chamber_max_temp: 60, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 100.0 },
    MaterialTypeInfo { name: "PA-CF", min_temp: 240, max_temp: 315, chamber_min_temp: 50, chamber_max_temp: 60, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 100.0 },
    MaterialTypeInfo { name: "PA-GF", min_temp: 240, max_temp: 290, chamber_min_temp: 50, chamber_max_temp: 60, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 100.0 },
    MaterialTypeInfo { name: "PA6", min_temp: 260, max_temp: 300, chamber_min_temp: 50, chamber_max_temp: 60, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 100.0 },
    MaterialTypeInfo { name: "PA6-CF", min_temp: 230, max_temp: 300, chamber_min_temp: 50, chamber_max_temp: 60, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 100.0 },
    MaterialTypeInfo { name: "PA6-GF", min_temp: 260, max_temp: 300, chamber_min_temp: 50, chamber_max_temp: 60, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 100.0 },
    MaterialTypeInfo { name: "PA11", min_temp: 275, max_temp: 295, chamber_min_temp: 50, chamber_max_temp: 60, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 100.0 },
    MaterialTypeInfo { name: "PA11-CF", min_temp: 275, max_temp: 295, chamber_min_temp: 50, chamber_max_temp: 60, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 100.0 },
    MaterialTypeInfo { name: "PA11-GF", min_temp: 275, max_temp: 295, chamber_min_temp: 50, chamber_max_temp: 60, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 100.0 },
    MaterialTypeInfo { name: "PA12", min_temp: 250, max_temp: 270, chamber_min_temp: 50, chamber_max_temp: 60, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 100.0 },
    MaterialTypeInfo { name: "PA12-CF", min_temp: 250, max_temp: 300, chamber_min_temp: 50, chamber_max_temp: 60, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 100.0 },
    MaterialTypeInfo { name: "PA12-GF", min_temp: 255, max_temp: 270, chamber_min_temp: 50, chamber_max_temp: 60, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 100.0 },
    MaterialTypeInfo { name: "PAHT", min_temp: 260, max_temp: 310, chamber_min_temp: 55, chamber_max_temp: 65, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PAHT-CF", min_temp: 270, max_temp: 310, chamber_min_temp: 55, chamber_max_temp: 65, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PAHT-GF", min_temp: 270, max_temp: 310, chamber_min_temp: 55, chamber_max_temp: 65, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PC", min_temp: 240, max_temp: 300, chamber_min_temp: 60, chamber_max_temp: 70, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 40.0 },
    MaterialTypeInfo { name: "PC-ABS", min_temp: 230, max_temp: 270, chamber_min_temp: 60, chamber_max_temp: 70, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 80.0 },
    MaterialTypeInfo { name: "PC-CF", min_temp: 270, max_temp: 295, chamber_min_temp: 60, chamber_max_temp: 70, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 80.0 },
    MaterialTypeInfo { name: "PC-PBT", min_temp: 260, max_temp: 300, chamber_min_temp: 60, chamber_max_temp: 70, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 40.0 },
    MaterialTypeInfo { name: "PCL", min_temp: 130, max_temp: 170, chamber_min_temp: 0, chamber_max_temp: 45, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PCTG", min_temp: 220, max_temp: 300, chamber_min_temp: 0, chamber_max_temp: 55, adhesion_coefficient: 2.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PE", min_temp: 175, max_temp: 260, chamber_min_temp: 45, chamber_max_temp: 60, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PE-CF", min_temp: 175, max_temp: 260, chamber_min_temp: 45, chamber_max_temp: 60, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PE-GF", min_temp: 230, max_temp: 270, chamber_min_temp: 45, chamber_max_temp: 60, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PEI-1010", min_temp: 370, max_temp: 430, chamber_min_temp: 80, chamber_max_temp: 100, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PEI-1010-CF", min_temp: 380, max_temp: 430, chamber_min_temp: 80, chamber_max_temp: 100, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PEI-1010-GF", min_temp: 380, max_temp: 430, chamber_min_temp: 80, chamber_max_temp: 100, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PEI-9085", min_temp: 350, max_temp: 390, chamber_min_temp: 80, chamber_max_temp: 100, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PEI-9085-CF", min_temp: 365, max_temp: 390, chamber_min_temp: 80, chamber_max_temp: 100, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PEI-9085-GF", min_temp: 370, max_temp: 390, chamber_min_temp: 80, chamber_max_temp: 100, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PEEK", min_temp: 350, max_temp: 460, chamber_min_temp: 80, chamber_max_temp: 100, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PEEK-CF", min_temp: 380, max_temp: 410, chamber_min_temp: 80, chamber_max_temp: 100, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PEEK-GF", min_temp: 375, max_temp: 410, chamber_min_temp: 80, chamber_max_temp: 100, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PEKK", min_temp: 325, max_temp: 400, chamber_min_temp: 80, chamber_max_temp: 100, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PEKK-CF", min_temp: 360, max_temp: 400, chamber_min_temp: 80, chamber_max_temp: 100, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PES", min_temp: 340, max_temp: 390, chamber_min_temp: 80, chamber_max_temp: 100, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PET", min_temp: 200, max_temp: 290, chamber_min_temp: 0, chamber_max_temp: 55, adhesion_coefficient: 2.0, yield_strength: 0.3, thermal_length: 100.0 },
    MaterialTypeInfo { name: "PET-CF", min_temp: 240, max_temp: 320, chamber_min_temp: 0, chamber_max_temp: 55, adhesion_coefficient: 2.0, yield_strength: 0.3, thermal_length: 100.0 },
    MaterialTypeInfo { name: "PET-GF", min_temp: 280, max_temp: 320, chamber_min_temp: 0, chamber_max_temp: 55, adhesion_coefficient: 2.0, yield_strength: 0.3, thermal_length: 100.0 },
    MaterialTypeInfo { name: "PETG", min_temp: 190, max_temp: 260, chamber_min_temp: 0, chamber_max_temp: 55, adhesion_coefficient: 2.0, yield_strength: 0.3, thermal_length: 100.0 },
    MaterialTypeInfo { name: "PETG-CF", min_temp: 230, max_temp: 290, chamber_min_temp: 0, chamber_max_temp: 55, adhesion_coefficient: 1.0, yield_strength: 0.3, thermal_length: 100.0 },
    MaterialTypeInfo { name: "PETG-GF", min_temp: 210, max_temp: 270, chamber_min_temp: 0, chamber_max_temp: 55, adhesion_coefficient: 1.0, yield_strength: 0.3, thermal_length: 100.0 },
    MaterialTypeInfo { name: "PHA", min_temp: 190, max_temp: 250, chamber_min_temp: 0, chamber_max_temp: 55, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PI", min_temp: 390, max_temp: 410, chamber_min_temp: 90, chamber_max_temp: 100, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PLA", min_temp: 180, max_temp: 240, chamber_min_temp: 0, chamber_max_temp: 45, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PLA-AERO", min_temp: 220, max_temp: 270, chamber_min_temp: 0, chamber_max_temp: 55, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PLA-CF", min_temp: 190, max_temp: 250, chamber_min_temp: 0, chamber_max_temp: 50, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "POM", min_temp: 210, max_temp: 250, chamber_min_temp: 50, chamber_max_temp: 65, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PP", min_temp: 200, max_temp: 240, chamber_min_temp: 45, chamber_max_temp: 60, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PP-CF", min_temp: 210, max_temp: 250, chamber_min_temp: 45, chamber_max_temp: 60, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PP-GF", min_temp: 220, max_temp: 260, chamber_min_temp: 45, chamber_max_temp: 60, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PPA-CF", min_temp: 260, max_temp: 300, chamber_min_temp: 55, chamber_max_temp: 70, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PPA-GF", min_temp: 260, max_temp: 290, chamber_min_temp: 55, chamber_max_temp: 70, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PPS", min_temp: 300, max_temp: 345, chamber_min_temp: 90, chamber_max_temp: 100, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PPS-CF", min_temp: 295, max_temp: 350, chamber_min_temp: 90, chamber_max_temp: 100, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PPSU", min_temp: 360, max_temp: 420, chamber_min_temp: 90, chamber_max_temp: 100, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PSU", min_temp: 350, max_temp: 380, chamber_min_temp: 90, chamber_max_temp: 100, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PVA", min_temp: 185, max_temp: 250, chamber_min_temp: 0, chamber_max_temp: 60, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PVB", min_temp: 190, max_temp: 250, chamber_min_temp: 0, chamber_max_temp: 55, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "PVDF", min_temp: 245, max_temp: 265, chamber_min_temp: 40, chamber_max_temp: 60, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "SBS", min_temp: 195, max_temp: 250, chamber_min_temp: 0, chamber_max_temp: 55, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "TPI", min_temp: 420, max_temp: 445, chamber_min_temp: 90, chamber_max_temp: 100, adhesion_coefficient: 1.0, yield_strength: 0.02, thermal_length: 200.0 },
    MaterialTypeInfo { name: "TPU", min_temp: 175, max_temp: 260, chamber_min_temp: 0, chamber_max_temp: 50, adhesion_coefficient: 0.5, yield_strength: 0.02, thermal_length: 1000.0 },
];

/// `MaterialType::all()`.
#[must_use]
pub fn all() -> &'static [MaterialTypeInfo] {
    MATERIAL_TYPES
}

/// `MaterialType::find(name)`: the first material named exactly `name`.
#[must_use]
pub fn find(name: &str) -> Option<&'static MaterialTypeInfo> {
    MATERIAL_TYPES.iter().find(|info| info.name == name)
}

/// `MaterialType::get_temperature_range(type, min_temp, max_temp)`: whether the type was found,
/// and the range (the defaults when it was not).
#[must_use]
pub fn get_temperature_range(ty: &str) -> (bool, i32, i32) {
    match find(ty) {
        Some(info) => (true, info.min_temp, info.max_temp),
        None => (false, DEFAULT_MIN_TEMP, DEFAULT_MAX_TEMP),
    }
}

/// `MaterialType::get_chamber_temperature_range(type, chamber_min_temp, chamber_max_temp)`.
#[must_use]
pub fn get_chamber_temperature_range(ty: &str) -> (bool, i32, i32) {
    match find(ty) {
        Some(info) => (true, info.chamber_min_temp, info.chamber_max_temp),
        None => (false, DEFAULT_CHAMBER_MIN_TEMP, DEFAULT_CHAMBER_MAX_TEMP),
    }
}

/// `MaterialType::get_adhesion_coefficient(type, adhesion_coefficient)`.
#[must_use]
pub fn get_adhesion_coefficient(ty: &str) -> (bool, f64) {
    match find(ty) {
        Some(info) => (true, info.adhesion_coefficient),
        None => (false, DEFAULT_ADHESION_COEFFICIENT),
    }
}

/// `MaterialType::get_yield_strength(type, yield_strength)`.
#[must_use]
pub fn get_yield_strength(ty: &str) -> (bool, f64) {
    match find(ty) {
        Some(info) => (true, info.yield_strength),
        None => (false, DEFAULT_YIELD_STRENGTH),
    }
}

/// `MaterialType::get_thermal_length(type, thermal_length)`.
#[must_use]
pub fn get_thermal_length(ty: &str) -> (bool, f64) {
    match find(ty) {
        Some(info) => (true, info.thermal_length),
        None => (false, DEFAULT_THERMAL_LENGTH),
    }
}
