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
// TriangleSelector.cpp, of OrcaSlicer v2.4.2, translated on 2026-11-07; and the reading of the
// bit stream in TriangleSelector::deserialize (TriangleSelector.cpp:1772-1893) with the states
// TriangleSelector::get_facets(facets_per_type) (1496-1520) lists, translated on 2026-11-11;
// FacetsAnnotation's timestamp (touch in set_data and reset, assign), equals, has_facets and
// get_facets_strict (Model.cpp:3511-3522, 3596-3600) with TriangleSelector::has_facets
// (TriangleSelector.cpp:1928-1977) and, for an annotation that splits nothing, what
// TriangleSelector's constructor, reset, push_triangle and get_facets_strict give
// (TriangleSelector.cpp:1326-1349, 1356-1361, 1522-1576), translated on 2026-11-13 by
// orca-o3-front-2. Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and the PrusaSlicer
// authors (Prusa Research), under the GNU Affero General Public License, version 3 (AGPL-3.0;
// LICENSE).

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
//!
//! [`TriangleSplittingData::leaf_states`] is what `ModelVolume::get_extruders` asks of a
//! `TriangleSelector`: the states left on the leaves once each triangle's tree is read. It reads
//! only the nibbles each tree takes, where `used_states` reads every nibble, so a string with a
//! nibble past its tree (`48`: a leaf of state 2, then a 1 nobody reads) gives fewer states. The
//! geometry of the splits is not built: which leaf gets which state does not change the list.
//!
//! PrintApply asks an annotation three things (orca-o3-front-2, 2026-11-13): whether it changed,
//! by its timestamp ([`FacetsAnnotation::timestamp_matches`]: 1 when made, copied with it, the
//! next of the process's counter at each `touch()`, which `set_data` and `reset` make and
//! `set_triangle_from_string` does not; [`crate::object_id`]); whether it equals another
//! ([`FacetsAnnotation::equals`]: the data, the used states included); and whether any leaf has a
//! state ([`TriangleSplittingData::has_facets`], `TriangleSelector::has_facets`, which reads the
//! trees without building them). [`FacetsAnnotation::get_facets_strict`] builds a
//! `TriangleSelector`, which phase O5 carries: on an annotation that splits nothing (or names a
//! triangle past the mesh, which the C++ logs and reads no further) the selector holds the mesh's
//! triangles unsplit, each of state `NONE`, so the result is the vertices some triangle uses, in
//! order, and the triangles when `NONE` is asked for, none otherwise; any other annotation is
//! painting, outside domain A, and stops with [`NotPorted`] naming `painted-facets`.

use orca_mesh::admesh::IndexedTriangleSet;
use orca_mesh::log::{Severity, log};

use crate::model::ModelVolume;
use crate::object_id::{Timestamp, next_timestamp};
use crate::stop::{NotPorted, PAINTED_FACETS};

/// `EnforcerBlockerType::ExtruderMax`.
pub const EXTRUDER_MAX: usize = 16;

/// `EnforcerBlockerType`, an `int8_t`: `NONE` 0, `ENFORCER` 1 (`FUZZY_SKIN`, `Extruder1`),
/// `BLOCKER` 2 (`Extruder2`), up to `ExtruderMax` 16.
pub type EnforcerBlockerType = i8;
/// `EnforcerBlockerType::NONE`.
pub const NONE: EnforcerBlockerType = 0;
/// `EnforcerBlockerType::ENFORCER`.
pub const ENFORCER: EnforcerBlockerType = 1;
/// `EnforcerBlockerType::BLOCKER`.
pub const BLOCKER: EnforcerBlockerType = 2;

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

    /// The states from 1 up that `TriangleSelector::deserialize(data, false)` leaves on a leaf of
    /// a mesh of `facets_count` triangles, in order: the `facets_per_type` that
    /// `get_facets(facets_per_type)` fills with triangles, `NONE` left out.
    ///
    /// One triangle id at or past `facets_count` and the C++ logs at info and reads nothing (lines
    /// 1781-1783), so every triangle stays `NONE`. A state past `ExtruderMax` is `NONE`
    /// (1831-1834). A negative id or a split code whose special side does not fit is undefined in
    /// the C++ (it indexes before its triangles or its vertices); the reader writes neither, and
    /// here the tree is read as any other.
    #[must_use]
    pub fn leaf_states(&self, facets_count: usize) -> Vec<i32> {
        let size = facets_count as i32;
        if self
            .triangles_to_split
            .iter()
            .any(|t| t.triangle_idx >= size)
        {
            // __FUNCTION__ with no separator after it, as the C++ writes it (line 1782)
            log(
                Severity::Info,
                "Slic3r::TriangleSelector::deserialize\
                 array bound:error:triangle_id >= int(m_triangles.size())",
            );
            return Vec::new();
        }
        let mut on_leaf = [false; EXTRUDER_MAX + 1];
        // (processed_children, total_children) of each parent, the root first.
        let mut parents: Vec<(u8, u8)> = Vec::new();
        for t in &self.triangles_to_split {
            let mut ibit = t.bitstream_start_idx as usize;
            parents.clear();
            loop {
                let code = self.nibble(&mut ibit);
                let num_of_split_sides = code & 0b11;
                let num_of_children = if num_of_split_sides == 0 {
                    0
                } else {
                    num_of_split_sides + 1
                };
                let is_split = num_of_children != 0;
                let mut state = if is_split {
                    0
                } else if (code & 0b1100) == 0b1100 {
                    self.nibble(&mut ibit) + 3
                } else {
                    code >> 2
                };
                if usize::from(state) > EXTRUDER_MAX {
                    state = 0;
                }
                if parents.is_empty() {
                    if is_split {
                        parents.push((0, num_of_children));
                        continue;
                    }
                    on_leaf[usize::from(state)] = true;
                    break;
                }
                if is_split {
                    parents.push((0, num_of_children));
                } else {
                    on_leaf[usize::from(state)] = true;
                    if let Some(last) = parents.last_mut() {
                        last.0 += 1;
                    }
                }
                while parents.last().is_some_and(|p| p.0 == p.1) {
                    parents.pop();
                    if let Some(last) = parents.last_mut() {
                        last.0 += 1;
                    }
                }
                if parents.is_empty() {
                    break;
                }
            }
        }
        (1..=EXTRUDER_MAX)
            .filter(|&s| on_leaf[s])
            .map(|s| s as i32)
            .collect()
    }

    /// `TriangleSelector::has_facets(data, test_state)`: whether a leaf of some triangle's tree has
    /// `test_state`, the trees read depth first without being built. A leaf's state is its
    /// nibble's upper two bits, or the next nibble plus 3 when both are set (so up to 18, never
    /// cut to `ExtruderMax` here, unlike `deserialize`); a bit past the end of the stream reads
    /// as 0 (the module documentation).
    #[must_use]
    pub fn has_facets(&self, test_state: EnforcerBlockerType) -> bool {
        // Depth-first queue of a number of unvisited children.
        let mut parents_children: Vec<i32> = Vec::with_capacity(64);
        let test_state = i32::from(test_state);

        for triangle_id_and_ibit in &self.triangles_to_split {
            let mut ibit = triangle_id_and_ibit.bitstream_start_idx as usize;
            // < 0 -> negative of a number of children
            // >= 0 -> state
            let num_children_or_state = |ibit: &mut usize| -> i32 {
                let code = i32::from(self.nibble(ibit));
                let num_of_split_sides = code & 0b11;
                if num_of_split_sides == 0 {
                    if (code & 0b1100) == 0b1100 {
                        i32::from(self.nibble(ibit)) + 3
                    } else {
                        code >> 2
                    }
                } else {
                    -num_of_split_sides - 1
                }
            };

            let state = num_children_or_state(&mut ibit);
            if state < 0 {
                // Root is split.
                parents_children.clear();
                parents_children.push(-state);
                loop {
                    let back = parents_children
                        .last_mut()
                        .expect("the loop runs while the queue is not empty");
                    *back -= 1;
                    if *back >= 0 {
                        let state = num_children_or_state(&mut ibit);
                        if state < 0 {
                            // Child is split.
                            parents_children.push(-state);
                        } else if state == test_state {
                            // Child is not split and a face of test_state was found.
                            return true;
                        }
                    } else {
                        parents_children.pop();
                    }
                    if parents_children.is_empty() {
                        break;
                    }
                }
            } else if state == test_state {
                // Root is not split and a face of test_state was found.
                return true;
            }
        }

        false
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

/// `FacetsAnnotation`: the data and its timestamp (its `ObjectBase` id is not kept:
/// [`crate::object_id`]). A copy (`Clone`) keeps the timestamp, as the C++'s copy constructor
/// does. `==` compares the data alone, as [`FacetsAnnotation::equals`] does (the C++ has no
/// `operator==`).
#[derive(Clone, Debug)]
pub struct FacetsAnnotation {
    m_data: TriangleSplittingData,
    m_timestamp: Timestamp,
}

impl Default for FacetsAnnotation {
    /// `FacetsAnnotation()`: no data, timestamp 1.
    fn default() -> Self {
        FacetsAnnotation {
            m_data: TriangleSplittingData::default(),
            m_timestamp: 1,
        }
    }
}

impl PartialEq for FacetsAnnotation {
    fn eq(&self, other: &Self) -> bool {
        self.equals(other)
    }
}

impl Eq for FacetsAnnotation {}

impl FacetsAnnotation {
    /// `timestamp()`.
    #[must_use]
    pub fn timestamp(&self) -> Timestamp {
        self.m_timestamp
    }

    /// `timestamp_matches(rhs)`.
    #[must_use]
    pub fn timestamp_matches(&self, rhs: &FacetsAnnotation) -> bool {
        self.m_timestamp == rhs.m_timestamp
    }

    /// `touch()`: the next timestamp of the process.
    pub fn touch(&mut self) {
        self.m_timestamp = next_timestamp();
    }

    /// The timestamp set as the C++'s serialization loads it (`serialize`: `ar(m_timestamp)`,
    /// which the undo stack reads back); the probe replays set a recorded one so.
    pub fn load_timestamp(&mut self, timestamp: Timestamp) {
        self.m_timestamp = timestamp;
    }

    /// `assign(rhs)`: the data and the timestamp taken when the timestamps differ.
    pub fn assign(&mut self, rhs: &FacetsAnnotation) {
        if !self.timestamp_matches(rhs) {
            self.m_data = rhs.m_data.clone();
            self.m_timestamp = rhs.m_timestamp;
        }
    }

    /// `get_data()`.
    #[must_use]
    pub fn get_data(&self) -> &TriangleSplittingData {
        &self.m_data
    }

    /// `set_data(data)`: the data, then `touch()`.
    pub fn set_data(&mut self, data: TriangleSplittingData) {
        self.m_data = data;
        self.touch();
    }

    /// `empty()`: no triangle to split.
    #[must_use]
    pub fn empty(&self) -> bool {
        self.m_data.triangles_to_split.is_empty()
    }

    /// `reset()`: the triangles and the bit stream cleared, then `touch()`; the used states stay
    /// as they were, as the C++ leaves them.
    pub fn reset(&mut self) {
        self.m_data.triangles_to_split.clear();
        self.m_data.bitstream.clear();
        self.touch();
    }

    /// `equals(other)`: the data equal (`TriangleSplittingData::operator==`: the triangles, the
    /// bit stream and the used states).
    #[must_use]
    pub fn equals(&self, other: &FacetsAnnotation) -> bool {
        let data = other.get_data();
        self.m_data == *data
    }

    /// `has_facets(mv, type)`: [`TriangleSplittingData::has_facets`] of the data (the part is not
    /// read).
    #[must_use]
    pub fn has_facets(&self, _mv: &ModelVolume, test_state: EnforcerBlockerType) -> bool {
        self.m_data.has_facets(test_state)
    }

    /// `get_facets_strict(mv, type)`: the triangles of state `state` of the part's mesh as a
    /// `TriangleSelector` deserializes this annotation onto it (see the module documentation).
    ///
    /// # Errors
    ///
    /// [`NotPorted`] naming `painted-facets` when the annotation splits a triangle of the mesh:
    /// the selector's splitting is phase O5's.
    pub fn get_facets_strict(
        &self,
        mv: &ModelVolume,
        state: EnforcerBlockerType,
    ) -> Result<IndexedTriangleSet, NotPorted> {
        let its = &mv.mesh().its;
        // TriangleSelector selector(mv.mesh()): reset() pushes each triangle, unsplit, of state
        // NONE, counting the references to each vertex.
        let mut ref_cnt: Vec<i32> = vec![0; its.vertices.len()];
        for ind in &its.indices {
            for k in 0..3 {
                ref_cnt[ind[k] as usize] += 1;
            }
        }
        let triangles_count = its.indices.len() as i32;
        // selector.deserialize(m_data, false)
        if self
            .m_data
            .triangles_to_split
            .iter()
            .any(|t| t.triangle_idx >= triangles_count)
        {
            // __FUNCTION__ with no separator after it, as the C++ writes it (line 1782); the
            // selector stays as reset() left it.
            log(
                Severity::Info,
                "Slic3r::TriangleSelector::deserialize\
                 array bound:error:triangle_id >= int(m_triangles.size())",
            );
        } else if !self.m_data.triangles_to_split.is_empty() {
            return Err(NotPorted {
                function: "FacetsAnnotation::get_facets_strict",
                family: PAINTED_FACETS,
            });
        }
        // selector.get_facets_strict(type)
        let mut out = IndexedTriangleSet::default();
        let num_vertices = ref_cnt.iter().filter(|&&c| c > 0).count();
        out.vertices.reserve(num_vertices);
        let mut vertex_map: Vec<i32> = vec![-1; its.vertices.len()];
        for (i, v) in its.vertices.iter().enumerate() {
            if ref_cnt[i] > 0 {
                vertex_map[i] = out.vertices.len() as i32;
                out.vertices.push(*v);
            }
        }
        // get_facets_strict_recursive of each unsplit triangle: of state NONE, emitted whole by
        // get_facets_split_by_tjoints (no neighbour is split, so no T-joint) when NONE is asked.
        if state == NONE {
            out.indices.extend(its.indices.iter().copied());
        }
        for triangle in &mut out.indices {
            for i in 0..3 {
                triangle[i] = vertex_map[triangle[i] as usize];
            }
        }
        Ok(out)
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
    fn leaves_take_only_the_nibbles_of_their_tree() {
        let mut f = FacetsAnnotation::default();
        // A leaf of state 2, then a 1 past its tree.
        f.set_triangle_from_string(1, b"48");
        assert!(f.get_data().used_states[1]);
        assert_eq!(f.get_data().leaf_states(4), vec![2]);
        // A split into two (code 1), its children of states 3 (C then 0) and 1.
        f.set_triangle_from_string(2, b"40C1");
        assert_eq!(f.get_data().leaf_states(4), vec![1, 2, 3]);
        // A triangle past the mesh and nothing is read.
        assert!(f.get_data().leaf_states(2).is_empty());
    }

    #[test]
    fn an_unknown_character_is_zero() {
        let mut f = FacetsAnnotation::default();
        f.set_triangle_from_string(0, b"a");
        assert_eq!(f.get_triangle_as_string(0), "0");
        assert!(f.get_data().used_states[0]);
    }
}
