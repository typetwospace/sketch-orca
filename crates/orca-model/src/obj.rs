// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Format/objparser.cpp (objparse, mtlparse and their line
// parsers) and src/libslic3r/Format/OBJ.cpp (load_obj), with their headers, of OrcaSlicer v2.4.2,
// translated on 2026-11-08. Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab)
// and the PrusaSlicer authors (Prusa Research), under the GNU Affero General Public License,
// version 3 (AGPL-3.0; LICENSE).

//! The OBJ reader: `ObjParser::objparse` and `mtlparse`, and `load_obj`, which turns what they
//! read into one object of one part.
//!
//! The file is read as the C reads it: `fopen(path, "rt")` (the UCRT's text mode, through
//! `orca-fmt`), 64 KiB at a time, each line ended by a CR or an LF and read as a C string (so up to
//! a NUL in it), with spaces and tabs before it dropped. A line after the last line end is never
//! read; an unfinished line of more than 64 KiB after a read ends the file with "failed to parse"
//! (so a line of up to about 128 KiB passes or fails by where it starts). The line parsers' answers
//! are ignored, as the C ignores them: a line they refuse leaves what it had pushed (a face cut
//! short keeps the corners read before the fault, with no end marker after them). Numbers go
//! through `strtod` and `strtol` (`orca-fmt`, the "C" locale), the values kept as `float`.
//!
//! What the C logs at error goes to `orca_mesh::log` in the same words and order: each line of a
//! command the parser does not know (`ObjParser: Unknown command: <its first character>`; the C
//! writes that byte as it is, and a byte past 0x7F, which is no text alone, is U+FFFD here), a line
//! past 64 KiB (`ObjParser: Excessive line length`, `MtlParser: Excessive line length`), each
//! failure of `load_obj` (`load_obj: failed to parse <path>`, with its reason for a face or a
//! corner, `load_obj:load_mtl: failed to parse <library>`, `load_obj: This OBJ file couldn't be
//! read because it's empty. <path>`) and a material library that is not there (`load_obj: failed
//! to load mtl_path:<library>`, after which the load goes on). The C's `Out of memory` records are
//! not translated: Rust ends the process at a failed allocation.
//!
//! A face of three corners is one triangle, of four two (0, 1, 2 and 0, 2, 3); more than four or
//! fewer than three is an error, as is a corner outside the vertices. A corner index of 0 becomes
//! -1, the end-of-face marker, as the C's `--` makes it. Vertex colours and the material library's
//! colours go into [`ObjInfo`], which only the GUI reads (`parity/HAZARDS.md`, orca-model).

use std::path::Path;

use orca_eigen::{Vec3f, Vec3i32};
use orca_fmt::parse::{strtod, strtol};
use orca_fmt::stdio::fopen;
use orca_mesh::admesh::IndexedTriangleSet;
use orca_mesh::log::{Severity, log};
use orca_mesh::triangle_mesh::TriangleMesh;
use orca_stl::minmax::clamp;

use crate::model::Model;
use crate::read::object_name_of;

/// `OBJ_VERTEX_LENGTH`: x, y, z and a colour's four components.
pub const OBJ_VERTEX_LENGTH: usize = 7;

/// `ObjVertex`: a face's corner, its indices counted from 0 (all -1 ends a face).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ObjVertex {
    /// `coordIdx`.
    pub coord_idx: i32,
    /// `textureCoordIdx`.
    pub texture_coord_idx: i32,
    /// `normalIdx`.
    pub normal_idx: i32,
}

/// `ObjUseMtl`: a `usemtl` line and the faces after it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObjUseMtl {
    /// `vertexIdxFirst`.
    pub vertex_idx_first: i32,
    /// `vertexIdxEnd`.
    pub vertex_idx_end: i32,
    /// `face_start`.
    pub face_start: i32,
    /// `face_end`.
    pub face_end: i32,
    /// `name`.
    pub name: Vec<u8>,
}

/// `ObjObject` and `ObjGroup`: an `o` or `g` line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObjNamed {
    /// `vertexIdxFirst`.
    pub vertex_idx_first: i32,
    /// `name`.
    pub name: Vec<u8>,
}

/// `ObjSmoothingGroup`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ObjSmoothingGroup {
    /// `vertexIdxFirst`.
    pub vertex_idx_first: i32,
    /// `smoothingGroupID`.
    pub smoothing_group_id: i32,
}

/// `ObjData`: what `objparse` reads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObjData {
    /// `coordinates`: x, y, z, then the colour's r, g, b, a (all 0 without a colour).
    pub coordinates: Vec<f32>,
    /// `has_vertex_color`.
    pub has_vertex_color: bool,
    /// `textureCoordinates`: u, v.
    pub texture_coordinates: Vec<f32>,
    /// `normals`.
    pub normals: Vec<f32>,
    /// `parameters`.
    pub parameters: Vec<f32>,
    /// `mtllibs`.
    pub mtllibs: Vec<Vec<u8>>,
    /// `usemtls`.
    pub usemtls: Vec<ObjUseMtl>,
    /// `objects`.
    pub objects: Vec<ObjNamed>,
    /// `groups`.
    pub groups: Vec<ObjNamed>,
    /// `smoothingGroups`.
    pub smoothing_groups: Vec<ObjSmoothingGroup>,
    /// `vertices`: the faces' corners, each face ended by a corner of all -1.
    pub vertices: Vec<ObjVertex>,
}

/// `ObjNewMtl`: a material of the library, value-initialised (`Tr` 1, the rest 0).
#[derive(Clone, Debug, PartialEq)]
pub struct ObjNewMtl {
    /// `Ns`.
    pub ns: f32,
    /// `Ni`.
    pub ni: f32,
    /// `d`.
    pub d: f32,
    /// `illum`.
    pub illum: f32,
    /// `Tr`.
    pub tr: f32,
    /// `Tf`.
    pub tf: [f32; 3],
    /// `Ka`.
    pub ka: [f32; 3],
    /// `Kd`.
    pub kd: [f32; 3],
    /// `Ks`.
    pub ks: [f32; 3],
    /// `Ke`.
    pub ke: [f32; 3],
    /// `map_Kd`.
    pub map_kd: Vec<u8>,
}

impl Default for ObjNewMtl {
    fn default() -> Self {
        ObjNewMtl {
            ns: 0.0,
            ni: 0.0,
            d: 0.0,
            illum: 0.0,
            tr: 1.0,
            tf: [0.0; 3],
            ka: [0.0; 3],
            kd: [0.0; 3],
            ks: [0.0; 3],
            ke: [0.0; 3],
            map_kd: Vec::new(),
        }
    }
}

/// `MtlData`: the materials by name (an `unordered_map`; nothing reads it in order).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MtlData {
    /// `new_mtl_unmap`.
    pub new_mtl_unmap: std::collections::BTreeMap<Vec<u8>, ObjNewMtl>,
}

/// `RGBA`.
pub type Rgba = [f32; 4];

/// `ObjInfo`: the colours `load_obj` finds, for the GUI's colour dialog.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObjInfo {
    /// `vertex_colors`.
    pub vertex_colors: Vec<Rgba>,
    /// `face_colors`.
    pub face_colors: Vec<Rgba>,
    /// `is_single_mtl`.
    pub is_single_mtl: bool,
    /// `lost_material_name`.
    pub lost_material_name: Vec<u8>,
    /// `uvs`: a triangle's three texture coordinates.
    pub uvs: Vec<[[f32; 2]; 3]>,
    /// `pngs`.
    pub pngs: std::collections::BTreeMap<Vec<u8>, bool>,
    /// `uv_map_pngs`.
    pub uv_map_pngs: std::collections::BTreeMap<i32, Vec<u8>>,
    /// `has_uv_png`.
    pub has_uv_png: bool,
}

/// A C string being parsed: the bytes up to the first NUL, read as if a NUL followed them.
struct Line<'a> {
    s: &'a [u8],
    p: usize,
}

impl<'a> Line<'a> {
    fn new(s: &'a [u8]) -> Self {
        let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
        Line { s: &s[..end], p: 0 }
    }

    /// `*line`.
    fn peek(&self) -> u8 {
        self.s.get(self.p).copied().unwrap_or(0)
    }

    /// `*line++`.
    fn next(&mut self) -> u8 {
        let c = self.peek();
        self.p += 1;
        c
    }

    /// `EATWS()`.
    fn eat_ws(&mut self) {
        while self.peek() == b' ' || self.peek() == b'\t' {
            self.p += 1;
        }
    }

    /// `line` as a C string from here.
    fn rest(&self) -> &'a [u8] {
        if self.p >= self.s.len() {
            &[]
        } else {
            &self.s[self.p..]
        }
    }

    /// `strtod(line, &endptr)`: the value and `*endptr`; `line` is not moved.
    fn strtod(&self) -> (f64, usize) {
        let r = strtod(self.rest());
        (r.value, self.p + r.end)
    }

    /// `strtol(line, &endptr, 10)`.
    fn strtol(&self) -> (i32, usize) {
        let r = strtol(self.rest(), 10);
        (r.value, self.p + r.end)
    }

    /// The byte at `at` (`*endptr`).
    fn at(&self, at: usize) -> u8 {
        self.s.get(at).copied().unwrap_or(0)
    }

    /// Whether the byte at `at` is a space, a tab, or (with `nul`) the end.
    fn ws_at(&self, at: usize, nul: bool) -> bool {
        let c = self.at(at);
        c == b' ' || c == b'\t' || (nul && c == 0)
    }

    /// The next bytes are `word` (each compared and stepped over until one differs, as the C's
    /// chain of `*(line++) != c` does).
    fn expect(&mut self, word: &[u8]) -> bool {
        for &w in word {
            if self.next() != w {
                return false;
            }
        }
        true
    }
}

/// `obj_parseline`: one line into `data`; false where the C returns false (which the reader
/// ignores).
fn obj_parseline(line: &[u8], data: &mut ObjData) -> bool {
    let mut l = Line::new(line);
    if l.peek() == 0 {
        return true;
    }
    l.eat_ws();
    let c1 = l.next();
    match c1 {
        b'#' => {}
        b'v' => {
            let c2 = l.next();
            match c2 {
                b't' => {
                    let c2 = l.next();
                    if c2 != b' ' && c2 != b'\t' {
                        return false;
                    }
                    l.eat_ws();
                    let (u, e) = l.strtod();
                    if !l.ws_at(e, false) {
                        return false;
                    }
                    l.p = e;
                    l.eat_ws();
                    let mut v = 0.0;
                    if l.peek() != 0 {
                        let (x, e) = l.strtod();
                        if !l.ws_at(e, true) {
                            return false;
                        }
                        v = x;
                        l.p = e;
                        l.eat_ws();
                    }
                    if l.peek() != 0 {
                        return false;
                    }
                    data.texture_coordinates.push(u as f32);
                    data.texture_coordinates.push(v as f32);
                }
                b'n' => {
                    let c2 = l.next();
                    if c2 != b' ' && c2 != b'\t' {
                        return false;
                    }
                    l.eat_ws();
                    let (x, e) = l.strtod();
                    if !l.ws_at(e, false) {
                        return false;
                    }
                    l.p = e;
                    l.eat_ws();
                    let (y, e) = l.strtod();
                    if !l.ws_at(e, false) {
                        return false;
                    }
                    l.p = e;
                    l.eat_ws();
                    let (z, e) = l.strtod();
                    if !l.ws_at(e, true) {
                        return false;
                    }
                    l.p = e;
                    l.eat_ws();
                    if l.peek() != 0 {
                        return false;
                    }
                    data.normals.push(x as f32);
                    data.normals.push(y as f32);
                    data.normals.push(z as f32);
                }
                b'p' => {
                    let c2 = l.next();
                    if c2 != b' ' && c2 != b'\t' {
                        return false;
                    }
                    l.eat_ws();
                    let (u, e) = l.strtod();
                    if !l.ws_at(e, true) {
                        return false;
                    }
                    l.p = e;
                    l.eat_ws();
                    let (v, e) = l.strtod();
                    if !l.ws_at(e, true) {
                        return false;
                    }
                    l.p = e;
                    l.eat_ws();
                    let mut w = 0.0;
                    if l.peek() != 0 {
                        let (x, e) = l.strtod();
                        if !l.ws_at(e, true) {
                            return false;
                        }
                        w = x;
                        l.p = e;
                        l.eat_ws();
                    }
                    if l.peek() != 0 {
                        return false;
                    }
                    data.parameters.push(u as f32);
                    data.parameters.push(v as f32);
                    data.parameters.push(w as f32);
                }
                _ => {
                    if c2 != b' ' && c2 != b'\t' {
                        return false;
                    }
                    l.eat_ws();
                    let (x, e) = l.strtod();
                    if !l.ws_at(e, false) {
                        return false;
                    }
                    l.p = e;
                    l.eat_ws();
                    let (y, e) = l.strtod();
                    if !l.ws_at(e, false) {
                        return false;
                    }
                    l.p = e;
                    l.eat_ws();
                    let (z, e) = l.strtod();
                    if !l.ws_at(e, true) {
                        return false;
                    }
                    l.p = e;
                    l.eat_ws();
                    let (mut cx, mut cy, mut cz, mut cw) = (0.0, 0.0, 0.0, 0.0);
                    if l.peek() != 0 {
                        data.has_vertex_color = true;
                        let (v, e) = l.strtod();
                        cx = v;
                        if !l.ws_at(e, true) {
                            return false;
                        }
                        l.p = e;
                        l.eat_ws();
                        let (v, e) = l.strtod();
                        cy = v;
                        if !l.ws_at(e, true) {
                            return false;
                        }
                        l.p = e;
                        l.eat_ws();
                        let (v, e) = l.strtod();
                        cz = v;
                        if !l.ws_at(e, true) {
                            return false;
                        }
                        l.p = e;
                        l.eat_ws();
                        cw = 1.0;
                        if l.peek() != 0 {
                            let (v, e) = l.strtod();
                            cw = v;
                            if !l.ws_at(e, true) {
                                return false;
                            }
                            l.p = e;
                            l.eat_ws();
                        }
                    }
                    // Extra data after the vertex is allowed (MeshLab writes some).
                    data.coordinates.extend([
                        x as f32, y as f32, z as f32, cx as f32, cy as f32, cz as f32, cw as f32,
                    ]);
                }
            }
        }
        b'f' => {
            l.eat_ws();
            if l.peek() == 0 {
                return false;
            }
            let mut vertex = ObjVertex::default();
            while l.peek() != 0 {
                vertex.coord_idx = 0;
                vertex.normal_idx = 0;
                vertex.texture_coord_idx = 0;
                let (c, e) = l.strtol();
                vertex.coord_idx = c;
                let ce = l.at(e);
                if ce != b' ' && ce != b'\t' && ce != b'/' && ce != 0 {
                    return false;
                }
                l.p = e;
                if l.peek() == b'/' {
                    l.p += 1;
                    if l.peek() != b'/' {
                        let (t, e) = l.strtol();
                        vertex.texture_coord_idx = t;
                        let ce = l.at(e);
                        if ce != b' ' && ce != b'\t' && ce != b'/' && ce != 0 {
                            return false;
                        }
                        l.p = e;
                    }
                    if l.peek() == b'/' {
                        l.p += 1;
                        let (n, e) = l.strtol();
                        vertex.normal_idx = n;
                        if !l.ws_at(e, true) {
                            return false;
                        }
                        l.p = e;
                    }
                }
                if vertex.coord_idx < 0 {
                    vertex.coord_idx = vertex
                        .coord_idx
                        .wrapping_add(data.coordinates.len() as i32 / OBJ_VERTEX_LENGTH as i32);
                } else {
                    vertex.coord_idx -= 1;
                }
                if vertex.normal_idx < 0 {
                    vertex.normal_idx = vertex
                        .normal_idx
                        .wrapping_add(data.normals.len() as i32 / 3);
                } else {
                    vertex.normal_idx -= 1;
                }
                if vertex.texture_coord_idx < 0 {
                    vertex.texture_coord_idx = vertex
                        .texture_coord_idx
                        .wrapping_add(data.texture_coordinates.len() as i32 / 3);
                } else {
                    vertex.texture_coord_idx -= 1;
                }
                data.vertices.push(vertex);
                l.eat_ws();
            }
            if let Some(last) = data.usemtls.last_mut() {
                last.vertex_idx_end = data.vertices.len() as i32;
            }
            if !data.usemtls.is_empty() {
                let mut face_index_count = 0;
                for v in data.vertices.iter().rev() {
                    if v.coord_idx == -1 {
                        break;
                    }
                    face_index_count += 1;
                }
                let last = data.usemtls.last_mut().expect("a usemtl");
                if face_index_count == 3 {
                    last.face_end += 1;
                } else if face_index_count == 4 {
                    last.face_end += 2;
                }
            }
            data.vertices.push(ObjVertex {
                coord_idx: -1,
                texture_coord_idx: -1,
                normal_idx: -1,
            });
        }
        b'm' => {
            if !l.expect(b"tllib") {
                return false;
            }
            l.eat_ws();
            data.mtllibs.push(l.rest().to_vec());
        }
        b'u' => {
            if !l.expect(b"semtl") {
                return false;
            }
            l.eat_ws();
            if let Some(last) = data.usemtls.last_mut() {
                last.vertex_idx_end = data.vertices.len() as i32;
            }
            data.usemtls.push(ObjUseMtl {
                vertex_idx_first: data.vertices.len() as i32,
                vertex_idx_end: -1,
                face_start: 0,
                face_end: -1,
                name: l.rest().to_vec(),
            });
            let n = data.usemtls.len();
            if n == 1 {
                data.usemtls[0].face_start = 0;
            } else {
                data.usemtls[n - 1].face_start = data.usemtls[n - 2].face_end + 1;
            }
            data.usemtls[n - 1].face_end = data.usemtls[n - 1].face_start - 1;
        }
        b'o' => {
            l.eat_ws();
            while l.peek() != b' ' && l.peek() != b'\t' && l.peek() != 0 {
                l.p += 1;
            }
            l.eat_ws();
            if l.peek() != 0 {
                return false;
            }
            data.objects.push(ObjNamed {
                vertex_idx_first: data.vertices.len() as i32,
                name: l.rest().to_vec(),
            });
        }
        b'g' => {
            data.groups.push(ObjNamed {
                vertex_idx_first: data.vertices.len() as i32,
                name: l.rest().to_vec(),
            });
        }
        b's' => {
            let c2 = l.next();
            if c2 != b' ' && c2 != b'\t' {
                return false;
            }
            l.eat_ws();
            let (g, e) = l.strtol();
            if !l.ws_at(e, true) {
                return false;
            }
            l.p = e;
            l.eat_ws();
            if l.peek() != 0 {
                return false;
            }
            data.smoothing_groups.push(ObjSmoothingGroup {
                vertex_idx_first: data.vertices.len() as i32,
                smoothing_group_id: g,
            });
        }
        _ => log(
            Severity::Error,
            &format!(
                "ObjParser: Unknown command: {}",
                String::from_utf8_lossy(&[c1])
            ),
        ),
    }
    true
}

/// Three numbers after a material's key (`Ka`, `Kd`, `Ks`, `Ke`, `Tf`): `None` where the C
/// returns false.
fn mtl_triple(l: &mut Line<'_>) -> Option<[f32; 3]> {
    l.eat_ws();
    let (x, e) = l.strtod();
    if !l.ws_at(e, false) {
        return None;
    }
    l.p = e;
    l.eat_ws();
    let (y, e) = l.strtod();
    if !l.ws_at(e, false) {
        return None;
    }
    l.p = e;
    l.eat_ws();
    let (z, e) = l.strtod();
    if !l.ws_at(e, true) {
        return None;
    }
    l.p = e;
    l.eat_ws();
    Some([x as f32, y as f32, z as f32])
}

/// `mtl_parseline`; `cur` is the static `cur_mtl_name`.
fn mtl_parseline(line: &[u8], data: &mut MtlData, cur: &mut Vec<u8>) -> bool {
    let mut l = Line::new(line);
    if l.peek() == 0 {
        return true;
    }
    l.eat_ws();
    let c1 = l.next();
    match c1 {
        b'#' => {}
        b'n' => {
            if !l.expect(b"ewmtl") {
                return false;
            }
            l.eat_ws();
            *cur = l.rest().to_vec();
            data.new_mtl_unmap.insert(cur.clone(), ObjNewMtl::default());
        }
        b'm' => {
            if !l.expect(b"ap_Kd") {
                return false;
            }
            l.eat_ws();
            if let Some(m) = data.new_mtl_unmap.get_mut(cur.as_slice()) {
                m.map_kd = l.rest().to_vec();
            }
        }
        b'N' => {
            let c = l.next();
            if c == b's' || c == b'i' {
                l.eat_ws();
                let v = l.strtod().0 as f32;
                if let Some(m) = data.new_mtl_unmap.get_mut(cur.as_slice()) {
                    if c == b's' {
                        m.ns = v;
                    } else {
                        m.ni = v;
                    }
                }
            }
        }
        b'K' => {
            let c = l.next();
            if matches!(c, b'a' | b'd' | b's' | b'e') {
                let Some(t) = mtl_triple(&mut l) else {
                    return false;
                };
                if let Some(m) = data.new_mtl_unmap.get_mut(cur.as_slice()) {
                    match c {
                        b'a' => m.ka = t,
                        b'd' => m.kd = t,
                        b's' => m.ks = t,
                        _ => m.ke = t,
                    }
                }
            }
        }
        b'i' => {
            if !l.expect(b"llum") {
                return false;
            }
            l.eat_ws();
            let v = l.strtod().0 as f32;
            if let Some(m) = data.new_mtl_unmap.get_mut(cur.as_slice()) {
                m.illum = v;
            }
        }
        b'd' => {
            l.eat_ws();
            let v = l.strtod().0 as f32;
            if let Some(m) = data.new_mtl_unmap.get_mut(cur.as_slice()) {
                m.d = v;
            }
        }
        b'T' => {
            let c = l.next();
            if c == b'r' {
                l.eat_ws();
                let v = l.strtod().0 as f32;
                if let Some(m) = data.new_mtl_unmap.get_mut(cur.as_slice()) {
                    m.tr = v;
                }
            } else if c == b'f' {
                let Some(t) = mtl_triple(&mut l) else {
                    return false;
                };
                if let Some(m) = data.new_mtl_unmap.get_mut(cur.as_slice()) {
                    m.tf = t;
                }
            }
        }
        _ => {}
    }
    true
}

/// The loop `objparse` and `mtlparse` share: the file in text mode, 64 KiB at a time, each line
/// handed to `parse_line`. False when the file cannot be opened or a line passes 64 KiB, which is
/// logged under `parser`'s name ("ObjParser" or "MtlParser").
fn read_lines(path: &str, parser: &str, mut parse_line: impl FnMut(&[u8])) -> bool {
    let Some(mut file) = fopen(Path::new(path), "rt") else {
        return false;
    };
    let mut buf = vec![0u8; 65536 * 2];
    let mut len_prev = 0usize;
    loop {
        let got = file.fread(&mut buf[len_prev..len_prev + 65536], 1, 65536);
        if got == 0 {
            break;
        }
        let len = got + len_prev;
        let mut last_line = 0;
        for i in 0..len {
            if buf[i] == b'\r' || buf[i] == b'\n' {
                buf[i] = 0;
                let mut c = last_line;
                while buf[c] == b' ' || buf[c] == b'\t' {
                    c += 1;
                }
                parse_line(&buf[c..i]);
                last_line = i + 1;
            }
        }
        len_prev = len - last_line;
        if len_prev > 65536 {
            log(Severity::Error, &format!("{parser}: Excessive line length"));
            return false;
        }
        buf.copy_within(last_line..len, 0);
    }
    true
}

/// `ObjParser::objparse(path, data)`.
pub fn objparse(path: &str, data: &mut ObjData) -> bool {
    read_lines(path, "ObjParser", |line| {
        obj_parseline(line, data);
    })
}

/// `ObjParser::mtlparse(path, data)`.
pub fn mtlparse(path: &str, data: &mut MtlData) -> bool {
    let mut cur = Vec::new();
    read_lines(path, "MtlParser", |line| {
        mtl_parseline(line, data, &mut cur);
    })
}

/// `boost::filesystem::path(p).parent_path().string()` for a Windows path: everything before the
/// last separator, the separators before it dropped (a root's own separator kept).
fn parent_path(p: &str) -> String {
    Path::new(p)
        .parent()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The colour `set_face_color` gives a face of material `name`, into `info`.
fn set_face_color(
    info: &mut ObjInfo,
    data: &ObjData,
    mtl_data: &MtlData,
    uvs: &[i32; 4],
    face_index: i32,
    name: &[u8],
) {
    let Some(m) = mtl_data.new_mtl_unmap.get(name) else {
        if info.lost_material_name.is_empty() {
            info.lost_material_name = name.to_vec();
        }
        return;
    };
    let mut merge = true;
    for n in 0..3 {
        if f64::from(m.ka[n] + m.kd[n]) > 1.0 {
            merge = false;
            break;
        }
    }
    let mut color: Rgba = [0.0; 4];
    for (n, c) in color.iter_mut().enumerate().take(3) {
        *c = if merge {
            clamp(m.ka[n] + m.kd[n], 0.0, 1.0)
        } else {
            clamp(m.kd[n], 0.0, 1.0)
        };
    }
    color[3] = m.tr;
    if !m.map_kd.is_empty() {
        info.has_uv_png = true;
        info.pngs.entry(m.map_kd.clone()).or_insert(false);
        info.uv_map_pngs.insert(face_index, m.map_kd.clone());
    }
    if !data.texture_coordinates.is_empty() {
        // The C reads these without a bound; a corner without a texture index reads before the
        // vector. Here such a coordinate reads as 0.
        let at = |k: usize| -> [f32; 2] {
            let i = i64::from(uvs[k]) * 2;
            let get = |j: i64| -> f32 {
                usize::try_from(j)
                    .ok()
                    .and_then(|j| data.texture_coordinates.get(j).copied())
                    .unwrap_or(0.0)
            };
            [get(i), get(i + 1)]
        };
        info.uvs.push([at(0), at(1), at(2)]);
    }
    info.face_colors.push(color);
}

fn set_face_color_by_mtl(
    info: &mut ObjInfo,
    data: &ObjData,
    mtl_data: &MtlData,
    uvs: &[i32; 4],
    face_index: i32,
) {
    if data.usemtls.len() == 1 {
        set_face_color(info, data, mtl_data, uvs, face_index, &data.usemtls[0].name);
    } else {
        for mtl in &data.usemtls {
            if face_index >= mtl.face_start && face_index <= mtl.face_end {
                set_face_color(info, data, mtl_data, uvs, face_index, &mtl.name);
                break;
            }
        }
    }
}

/// `load_obj`'s refusal of the file's faces: the record `load_obj: failed to parse <path>.
/// <message>` at error, and the message.
fn parse_failure(path: &str, message: &str) -> String {
    log(
        Severity::Error,
        &format!("load_obj: failed to parse {path}. {message}"),
    );
    message.to_owned()
}

/// `load_obj(path, meshptr, obj_info, message)`: the mesh, or the message.
///
/// # Errors
///
/// The C's message: the file failed to parse, its material library failed to parse, a face of
/// more than four or fewer than three corners, a corner outside the vertices, or no triangle.
pub fn load_obj_mesh(path: &str, obj_info: &mut ObjInfo) -> Result<TriangleMesh, String> {
    let mut data = ObjData::default();
    let mut mtl_data = MtlData::default();
    if !objparse(path, &mut data) {
        log(
            Severity::Error,
            &format!("load_obj: failed to parse {path}"),
        );
        return Err("load_obj: failed to parse".to_owned());
    }
    let mut exist_mtl = false;
    for mtl_name in &data.mtllibs {
        if mtl_name.is_empty() {
            continue;
        }
        exist_mtl = true;
        let name = String::from_utf8_lossy(mtl_name).into_owned();
        // A name that exists from the working folder is taken as a path.
        let mtl_path = if Path::new(&name).exists() {
            name
        } else {
            format!("{}/{}", parent_path(path), name)
        };
        if !Path::new(&mtl_path).exists() {
            log(
                Severity::Error,
                &format!("load_obj: failed to load mtl_path:{mtl_path}"),
            );
        } else if !mtlparse(&mtl_path, &mut mtl_data) {
            log(
                Severity::Error,
                &format!("load_obj:load_mtl: failed to parse {mtl_path}"),
            );
            return Err("load mtl in obj: failed to parse".to_owned());
        }
    }
    let mut num_faces = 0usize;
    let mut num_quads = 0usize;
    let mut i = 0;
    while i < data.vertices.len() {
        let mut j = i;
        while j < data.vertices.len() && data.vertices[j].coord_idx != -1 {
            j += 1;
        }
        let n = j - i;
        if n > 0 {
            if n > 4 {
                return Err(parse_failure(
                    path,
                    "The file contains polygons with more than 4 vertices.",
                ));
            } else if n < 3 {
                return Err(parse_failure(
                    path,
                    "The file contains polygons with less than 2 vertices.",
                ));
            }
            if n == 4 {
                num_quads += 1;
            }
            num_faces += 1;
            i = j;
        }
        i += 1;
    }
    let mut its = IndexedTriangleSet::default();
    let num_vertices = data.coordinates.len() / OBJ_VERTEX_LENGTH;
    its.vertices.reserve(num_vertices);
    its.indices.reserve(num_faces + num_quads);
    if exist_mtl {
        obj_info.is_single_mtl = data.usemtls.len() == 1 && mtl_data.new_mtl_unmap.len() == 1;
        obj_info.face_colors.reserve(num_faces + num_quads);
    }
    for i in 0..num_vertices {
        let c = &data.coordinates[i * OBJ_VERTEX_LENGTH..(i + 1) * OBJ_VERTEX_LENGTH];
        its.vertices.push(Vec3f::new(c[0], c[1], c[2]));
        if data.has_vertex_color {
            obj_info.vertex_colors.push([
                clamp(c[3], 0.0, 1.0),
                clamp(c[4], 0.0, 1.0),
                clamp(c[5], 0.0, 1.0),
                clamp(c[6], 0.0, 1.0),
            ]);
        }
    }
    let mut indices = [0i32; 4];
    let mut uvs = [0i32; 4];
    let mut i = 0;
    while i < data.vertices.len() {
        if data.vertices[i].coord_idx == -1 {
            i += 1;
            continue;
        }
        let mut cnt = 0;
        while i < data.vertices.len() {
            let vertex = data.vertices[i];
            i += 1;
            if vertex.coord_idx == -1 {
                break;
            }
            if vertex.coord_idx < 0 || vertex.coord_idx >= its.vertices.len() as i32 {
                return Err(parse_failure(
                    path,
                    "The file contains invalid vertex index.",
                ));
            }
            indices[cnt] = vertex.coord_idx;
            uvs[cnt] = vertex.texture_coord_idx;
            cnt += 1;
        }
        if cnt != 0 {
            its.indices
                .push(Vec3i32::new(indices[0], indices[1], indices[2]));
            if exist_mtl {
                let face_index = its.indices.len() as i32 - 1;
                set_face_color_by_mtl(obj_info, &data, &mtl_data, &uvs, face_index);
            }
            if cnt == 4 {
                its.indices
                    .push(Vec3i32::new(indices[0], indices[2], indices[3]));
                if exist_mtl {
                    let face_index = its.indices.len() as i32 - 1;
                    set_face_color_by_mtl(obj_info, &data, &mtl_data, &uvs, face_index);
                }
            }
        }
    }
    let mut mesh = TriangleMesh::from_its(its);
    if mesh.empty() {
        log(
            Severity::Error,
            &format!("load_obj: This OBJ file couldn't be read because it's empty. {path}"),
        );
        return Err("This OBJ file couldn't be read because it's empty.".to_owned());
    }
    if mesh.volume() < 0.0 {
        mesh.flip_triangles();
    }
    Ok(mesh)
}

/// `load_obj(path, model, obj_info, message, object_name)`: the mesh added to `model` as one
/// object, named `object_name` or after the text behind the path's last `\`.
///
/// # Errors
///
/// As [`load_obj_mesh`].
pub fn load_obj(
    path: &str,
    model: &mut Model,
    obj_info: &mut ObjInfo,
    object_name: Option<&str>,
) -> Result<(), String> {
    let mesh = load_obj_mesh(path, obj_info)?;
    let name = object_name
        .unwrap_or_else(|| object_name_of(path))
        .to_owned();
    model.add_object(&name, path, mesh);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> ObjData {
        let mut d = ObjData::default();
        for line in text.split('\n') {
            let t = line.trim_start_matches([' ', '\t']);
            obj_parseline(t.as_bytes(), &mut d);
        }
        d
    }

    #[test]
    fn vertices_faces_and_quads() {
        let d =
            parse("v 0 0 0\nv 1 0 0\nv 1 1 0 0.5 0.25 1\nv 0 1 0\nf 1 2 3 4\nf -4/1/2 -3//1 -2\n");
        assert_eq!(d.coordinates.len(), 4 * OBJ_VERTEX_LENGTH);
        assert!(d.has_vertex_color);
        assert_eq!(
            &d.coordinates[14..21],
            &[1.0, 1.0, 0.0, 0.5, 0.25, 1.0, 1.0]
        );
        let idx: Vec<i32> = d.vertices.iter().map(|v| v.coord_idx).collect();
        assert_eq!(idx, [0, 1, 2, 3, -1, 0, 1, 2, -1]);
        assert_eq!(d.vertices[5].texture_coord_idx, 0);
        assert_eq!(d.vertices[5].normal_idx, 1);
    }

    #[test]
    fn a_refused_face_keeps_its_corners() {
        let d = parse("v 0 0 0\nf 1 2 x 3\n");
        let idx: Vec<i32> = d.vertices.iter().map(|v| v.coord_idx).collect();
        assert_eq!(idx, [0, 1]);
        // A vertex index of 0 becomes the end marker.
        let d = parse("f 0 1 2\n");
        assert_eq!(d.vertices[0].coord_idx, -1);
    }

    #[test]
    fn names_and_materials() {
        let d = parse(
            "o  cube \nusemtl red\nf 1 2 3\nf 1 2 3 4\nusemtl blue\ns off\ns 1\nmtllib  a b.mtl",
        );
        assert_eq!(d.objects[0].name, b"");
        assert_eq!(d.usemtls[0].face_start, 0);
        assert_eq!(d.usemtls[0].face_end, 2);
        assert_eq!(d.usemtls[1].face_start, 3);
        assert_eq!(d.usemtls[1].face_end, 2);
        assert_eq!(d.smoothing_groups.len(), 1);
        assert_eq!(d.mtllibs[0], b"a b.mtl");
        let mut m = MtlData::default();
        let mut cur = Vec::new();
        for l in [
            "newmtl red",
            "Ka 0.1 0.2 0.3",
            "Kd 0.5 0.5 0.5",
            "Tr 0.5",
            "map_Kd t.png",
        ] {
            mtl_parseline(l.as_bytes(), &mut m, &mut cur);
        }
        let red = &m.new_mtl_unmap[b"red".as_slice()];
        assert_eq!(red.ka, [0.1, 0.2, 0.3]);
        assert_eq!(red.tr, 0.5);
        assert_eq!(red.map_kd, b"t.png");
    }

    thread_local! {
        static RECORDS: std::cell::RefCell<Option<Vec<String>>> =
            const { std::cell::RefCell::new(None) };
    }

    /// Runs `f` and returns what it returns with the records it logged on this thread, each as
    /// `[<severity>] <message>`.
    fn logged<T>(f: impl FnOnce() -> T) -> (T, Vec<String>) {
        orca_mesh::log::set_sink(Some(Box::new(|severity, message| {
            RECORDS.with(|r| {
                if let Some(v) = r.borrow_mut().as_mut() {
                    v.push(format!("[{}] {message}", severity.name()));
                }
            });
        })));
        RECORDS.with(|r| *r.borrow_mut() = Some(Vec::new()));
        let out = f();
        let records = RECORDS.with(|r| r.borrow_mut().take()).unwrap_or_default();
        (out, records)
    }

    #[test]
    fn each_failure_and_a_missing_library_are_logged_as_the_c_logs_them() {
        let dir = std::env::temp_dir().join(format!("orca-model-obj-log-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = |name: &str, text: &str| {
            let p = dir.join(name);
            std::fs::write(&p, text).unwrap();
            p.to_string_lossy().into_owned()
        };
        let tetra = "v 0 0 0\nv 1 0 0\nv 0 1 0\nv 0 0 1\nf 1 2 3\nf 1 2 4\nf 1 3 4\nf 2 3 4\n";
        // An unknown command, then a library that is not there: both logged, and the load goes on.
        let missing = file(
            "missing.obj",
            &format!("mtllib missing.mtl\n{tetra}l 1 2\n"),
        );
        let (mesh, records) = logged(|| load_obj_mesh(&missing, &mut ObjInfo::default()));
        assert!(mesh.is_ok());
        assert_eq!(
            records,
            [
                "[error] ObjParser: Unknown command: l".to_owned(),
                format!(
                    "[error] load_obj: failed to load mtl_path:{}/missing.mtl",
                    parent_path(&missing)
                ),
            ]
        );
        // A corner outside the vertices.
        let bad = file("bad.obj", "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 9\n");
        let (mesh, records) = logged(|| load_obj_mesh(&bad, &mut ObjInfo::default()));
        assert_eq!(mesh.unwrap_err(), "The file contains invalid vertex index.");
        assert_eq!(
            records,
            [format!(
                "[error] load_obj: failed to parse {bad}. The file contains invalid vertex index."
            )]
        );
        // No triangle.
        let empty = file("empty.obj", "# nothing\n");
        let (mesh, records) = logged(|| load_obj_mesh(&empty, &mut ObjInfo::default()));
        assert!(mesh.is_err());
        assert_eq!(
            records,
            [format!(
                "[error] load_obj: This OBJ file couldn't be read because it's empty. {empty}"
            )]
        );
        // A face of five corners, and one of two: the C's words, "less than 2" included.
        let five = file(
            "five.obj",
            "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 0 1\nf 1 2 3 4 5\n",
        );
        let (mesh, records) = logged(|| load_obj_mesh(&five, &mut ObjInfo::default()));
        assert_eq!(
            mesh.unwrap_err(),
            "The file contains polygons with more than 4 vertices."
        );
        assert_eq!(
            records,
            [format!(
                "[error] load_obj: failed to parse {five}. The file contains polygons with more than 4 vertices."
            )]
        );
        let two = file("two.obj", "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2\n");
        let (mesh, records) = logged(|| load_obj_mesh(&two, &mut ObjInfo::default()));
        assert_eq!(
            mesh.unwrap_err(),
            "The file contains polygons with less than 2 vertices."
        );
        assert_eq!(
            records,
            [format!(
                "[error] load_obj: failed to parse {two}. The file contains polygons with less than 2 vertices."
            )]
        );
        // A line still unfinished after a read, past 64 KiB: the parser's record, then load_obj's.
        let long_line = format!("# {}\n", "a".repeat(140_000));
        let long_obj = file("long.obj", &format!("{tetra}{long_line}"));
        let (mesh, records) = logged(|| load_obj_mesh(&long_obj, &mut ObjInfo::default()));
        assert_eq!(mesh.unwrap_err(), "load_obj: failed to parse");
        assert_eq!(
            records,
            [
                "[error] ObjParser: Excessive line length".to_owned(),
                format!("[error] load_obj: failed to parse {long_obj}"),
            ]
        );
        // The same in a material library that is there.
        file("long-lib.mtl", &format!("newmtl red\n{long_line}"));
        let long_mtl = file("long-mtl.obj", &format!("mtllib long-lib.mtl\n{tetra}"));
        let (mesh, records) = logged(|| load_obj_mesh(&long_mtl, &mut ObjInfo::default()));
        assert_eq!(mesh.unwrap_err(), "load mtl in obj: failed to parse");
        assert_eq!(
            records,
            [
                "[error] MtlParser: Excessive line length".to_owned(),
                format!(
                    "[error] load_obj:load_mtl: failed to parse {}/long-lib.mtl",
                    parent_path(&long_mtl)
                ),
            ]
        );
        // A library that is there but cannot be opened (a folder): mtlparse logs nothing itself.
        std::fs::create_dir_all(dir.join("folder-lib.mtl")).unwrap();
        let shut = file("shut.obj", &format!("mtllib folder-lib.mtl\n{tetra}"));
        let (mesh, records) = logged(|| load_obj_mesh(&shut, &mut ObjInfo::default()));
        assert_eq!(mesh.unwrap_err(), "load mtl in obj: failed to parse");
        assert_eq!(
            records,
            [format!(
                "[error] load_obj:load_mtl: failed to parse {}/folder-lib.mtl",
                parent_path(&shut)
            )]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
