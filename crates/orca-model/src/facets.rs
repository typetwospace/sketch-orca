// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: class FacetsAnnotation of src/libslic3r/Model.hpp and Model.cpp
// (set_triangle_from_string, get_triangle_as_string, reserve, shrink_to_fit, empty) and
// TriangleSelector::TriangleSplittingData with update_used_states of TriangleSelector.hpp and
// TriangleSelector.cpp, of OrcaSlicer v2.4.2, translated on 2026-11-07. Copyright the OrcaSlicer
// authors, the BambuStudio authors (Bambu Lab) and the PrusaSlicer authors (Prusa Research),
// under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! A part's painting (supports, seams, colours, fuzzy skin) as the 3MF reader loads it: the
//! triangles to split, the bit stream of their splitting and the states used.
//!
//! [`FacetsAnnotation::set_triangle_from_string`] reads a triangle's hexadecimal string from its
//! last character to its first, four bits per character, the lowest first; a character that is
//! not `0`-`9` or `A`-`F` counts as 0 (the C++ asserts, which a release build leaves out).
//! `update_used_states` reads past the end of the bit stream when the last nibble asks for
//! another (a `C`, `D`, `E` or `F` low nibble with nothing after it); MSVC's `vector<bool>` keeps
//! the bits past its size zero in its last word, so the extra nibble reads as 0 there, which the
//! translation takes everywhere (past the last word the C++ reads its storage's spare capacity:
//! `parity/HAZARDS.md`, orca-model).

/// `EnforcerBlockerType::ExtruderMax`.
pub const EXTRUDER_MAX: usize = 16;

/// `TriangleSelector::TriangleBitStreamMapping`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TriangleBitStreamMapping {
    /// `triangle_idx`.
    pub triangle_idx: i32,
    /// `bitstream_start_idx`.
    pub bitstream_start_idx: i32,
}

/// `TriangleSelector::TriangleSplittingData`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TriangleSplittingData {
    /// `triangles_to_split`.
    pub triangles_to_split: Vec<TriangleBitStreamMapping>,
    /// `bitstream`.
    pub bitstream: Vec<bool>,
    /// `used_states`, `ExtruderMax + 1` of them.
    pub used_states: Vec<bool>,
}

impl Default for TriangleSplittingData {
    fn default() -> Self {
        TriangleSplittingData {
            triangles_to_split: Vec::new(),
            bitstream: Vec::new(),
            used_states: vec![false; EXTRUDER_MAX + 1],
        }
    }
}

impl TriangleSplittingData {
    /// `update_used_states(bitstream_start_idx)`: marks each state the bit stream from
    /// `bitstream_start_idx` to its end sets.
    pub fn update_used_states(&mut self, bitstream_start_idx: usize) {
        if self.bitstream.is_empty() || self.bitstream.len() == bitstream_start_idx {
            return;
        }
        let len = self.bitstream.len();
        let mut nibble_idx = bitstream_start_idx;
        while nibble_idx < len {
            let code = self.nibble(&mut nibble_idx);
            if (code & 0b11) != 0 {
                continue;
            }
            let facet_state = if (code & 0b1100) == 0b1100 {
                self.nibble(&mut nibble_idx) + 3
            } else {
                code >> 2
            };
            if usize::from(facet_state) >= self.used_states.len() {
                continue;
            }
            self.used_states[usize::from(facet_state)] = true;
        }
    }

    /// `read_next_nibble()`: four bits, the lowest first, a bit past the end as 0.
    fn nibble(&self, nibble_idx: &mut usize) -> u8 {
        let mut code = 0u8;
        for bit_idx in 0..4 {
            let bit = self.bitstream.get(*nibble_idx).copied().unwrap_or(false);
            code |= u8::from(bit) << bit_idx;
            *nibble_idx += 1;
        }
        code
    }
}

/// `FacetsAnnotation`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FacetsAnnotation {
    m_data: TriangleSplittingData,
}

impl FacetsAnnotation {
    /// `get_data()`.
    #[must_use]
    pub fn get_data(&self) -> &TriangleSplittingData {
        &self.m_data
    }

    /// `set_data(data)`.
    pub fn set_data(&mut self, data: TriangleSplittingData) {
        self.m_data = data;
    }

    /// `empty()`: no triangle to split.
    #[must_use]
    pub fn empty(&self) -> bool {
        self.m_data.triangles_to_split.is_empty()
    }

    /// `reset()`: the triangles and the bit stream cleared; the used states stay as they were, as
    /// the C++ leaves them.
    pub fn reset(&mut self) {
        self.m_data.triangles_to_split.clear();
        self.m_data.bitstream.clear();
    }

    /// `reserve(n_triangles)`.
    pub fn reserve(&mut self, n_triangles: usize) {
        self.m_data.triangles_to_split.reserve(n_triangles);
    }

    /// `shrink_to_fit()`.
    pub fn shrink_to_fit(&mut self) {
        self.m_data.triangles_to_split.shrink_to_fit();
        self.m_data.bitstream.shrink_to_fit();
    }

    /// `set_triangle_from_string(triangle_id, str)`.
    pub fn set_triangle_from_string(&mut self, triangle_id: i32, s: &[u8]) {
        let start = self.m_data.bitstream.len();
        self.m_data
            .triangles_to_split
            .push(TriangleBitStreamMapping {
                triangle_idx: triangle_id,
                bitstream_start_idx: start as i32,
            });
        for &ch in s.iter().rev() {
            let dec = if ch.is_ascii_digit() {
                i32::from(ch - b'0')
            } else if (b'A'..=b'F').contains(&ch) {
                10 + i32::from(ch - b'A')
            } else {
                0
            };
            for i in 0..4 {
                self.m_data.bitstream.push(dec & (1 << i) != 0);
            }
        }
        self.m_data.update_used_states(start);
    }

    /// `get_triangle_as_string(triangle_idx)`.
    #[must_use]
    pub fn get_triangle_as_string(&self, triangle_idx: i32) -> String {
        let t = &self.m_data.triangles_to_split;
        let at =
            orca_stl::lower_bound_by(t, &triangle_idx, |m: &TriangleBitStreamMapping, r: &i32| {
                m.triangle_idx < *r
            });
        let mut out = Vec::new();
        if at < t.len() && t[at].triangle_idx == triangle_idx {
            let mut offset = t[at].bitstream_start_idx as usize;
            let end = if at + 1 == t.len() {
                self.m_data.bitstream.len()
            } else {
                t[at + 1].bitstream_start_idx as usize
            };
            while offset < end {
                let mut next_code = 0u8;
                for i in (0..4).rev() {
                    next_code = (next_code << 1) | u8::from(self.m_data.bitstream[offset + i]);
                }
                offset += 4;
                let digit = if next_code < 10 {
                    next_code + b'0'
                } else {
                    next_code - 10 + b'A'
                };
                out.insert(0, digit);
            }
        }
        String::from_utf8(out).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_string_round_trips() {
        let mut f = FacetsAnnotation::default();
        f.set_triangle_from_string(3, b"4");
        f.set_triangle_from_string(7, b"1C08");
        assert_eq!(f.get_triangle_as_string(3), "4");
        assert_eq!(f.get_triangle_as_string(7), "1C08");
        assert!(f.get_data().used_states[1]);
    }

    #[test]
    fn an_unknown_character_is_zero() {
        let mut f = FacetsAnnotation::default();
        f.set_triangle_from_string(0, b"a");
        assert_eq!(f.get_triangle_as_string(0), "0");
        assert!(f.get_data().used_states[0]);
    }
}
