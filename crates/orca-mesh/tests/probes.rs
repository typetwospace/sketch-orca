// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The probe gate for orca-mesh: the records of `mesh.sop1` (`parity/SOP1.md`, MCAS and MSTD),
//! from the committed subset (`parity/probes/`), or from the full set when
//! `SKETCH_ORCA_PROBES_DEEP` names its folder (`<work>/parity/probes`).
//!
//! Each record is rebuilt here byte for byte as `parity/oracle/probe/mesh.cpp` writes it, from
//! what the Rust reads, repairs and computes with the inputs the record holds (the file, the
//! header length, each operation's parameters), and the two are compared whole; a difference is
//! named by the field it falls in. MCAS holds a hash of admesh's state after each repair step;
//! MSTD (the full set only, meshes of 24 facets or fewer) holds the states themselves, so there a
//! difference is named down to the facet or the neighbour entry. Filled by o2-mesh (2026-11-07).

use orca_eigen::{Mat3, Mat4, Matrix3d, Transform3d, Vec3d, Vec3f};
use orca_geom::bounding_box::BoundingBoxf3;
use orca_geom::point::Point;
use orca_geom::polygon::Polygon;
use orca_mesh::admesh::connect::{
    stl_check_facets_exact, stl_check_facets_nearby, stl_remove_unconnected_facets,
};
use orca_mesh::admesh::normals::{stl_fix_normal_directions, stl_fix_normal_values};
use orca_mesh::admesh::shared::stl_generate_shared_vertices;
use orca_mesh::admesh::stlinit::stl_open_bytes;
use orca_mesh::admesh::util::{stl_calculate_volume, stl_verify_neighbors};
use orca_mesh::admesh::{IndexedTriangleSet, StlFile, StlType};
use orca_mesh::its::{its_num_open_edges_of, its_number_of_patches};
use orca_mesh::triangle_mesh::{Axis, TriangleMesh};
use orca_probe::{Mismatches, ProbeFile, ProbeSet, Reader, Record, load_probe};

fn mesh_file() -> ProbeFile {
    load_probe("mesh").unwrap_or_else(|e| panic!("{e}"))
}

/// Discards what admesh prints (`stl_write_facet`) and what libslic3r logs: the probe records
/// neither.
fn quiet() {
    orca_fmt::stdio::set_stdout_sink(Some(Box::new(|_| {})));
    orca_mesh::log::set_sink(None);
}

/// Bytes as `probe.hpp`'s `Payload` writes them (little-endian; a `str` as its `u32` length and
/// its bytes), with where each field starts when the fields are named, to name a difference.
#[derive(Default)]
struct Payload {
    bytes: Vec<u8>,
    fields: Option<Vec<(usize, String)>>,
}

impl Payload {
    /// A payload that names its fields.
    fn named() -> Self {
        Payload {
            bytes: Vec::new(),
            fields: Some(Vec::new()),
        }
    }

    fn field(&mut self, name: &str) -> &mut Self {
        let at = self.bytes.len();
        if let Some(fields) = self.fields.as_mut() {
            fields.push((at, name.to_owned()));
        }
        self
    }

    fn field_at(&mut self, name: &str, i: usize) -> &mut Self {
        let at = self.bytes.len();
        if let Some(fields) = self.fields.as_mut() {
            fields.push((at, format!("{name} {i}")));
        }
        self
    }

    fn u8(&mut self, v: u8) -> &mut Self {
        self.bytes.push(v);
        self
    }

    fn u32(&mut self, v: u32) -> &mut Self {
        self.bytes.extend_from_slice(&v.to_le_bytes());
        self
    }

    fn u64(&mut self, v: u64) -> &mut Self {
        self.bytes.extend_from_slice(&v.to_le_bytes());
        self
    }

    fn i32(&mut self, v: i32) -> &mut Self {
        self.bytes.extend_from_slice(&v.to_le_bytes());
        self
    }

    fn i64(&mut self, v: i64) -> &mut Self {
        self.bytes.extend_from_slice(&v.to_le_bytes());
        self
    }

    fn boolean(&mut self, v: bool) -> &mut Self {
        self.u8(u8::from(v))
    }

    fn f32(&mut self, v: f32) -> &mut Self {
        self.u32(v.to_bits())
    }

    fn f64(&mut self, v: f64) -> &mut Self {
        self.u64(v.to_bits())
    }

    fn str(&mut self, s: &[u8]) -> &mut Self {
        self.u32(u32::try_from(s.len()).expect("a str under 4 GiB"));
        self.bytes.extend_from_slice(s);
        self
    }

    /// `inner`'s bytes as a `str`, its fields named under `prefix`.
    fn str_of(&mut self, prefix: &str, inner: &Payload) -> &mut Self {
        self.u32(u32::try_from(inner.bytes.len()).expect("a str under 4 GiB"));
        let start = self.bytes.len();
        if let (Some(fields), Some(inner_fields)) = (self.fields.as_mut(), inner.fields.as_ref()) {
            for (at, name) in inner_fields {
                fields.push((start + at, format!("{prefix}: {name}")));
            }
        }
        self.bytes.extend_from_slice(&inner.bytes);
        self
    }

    fn vec3f(&mut self, v: &Vec3f) -> &mut Self {
        for i in 0..3 {
            self.f32(v[i]);
        }
        self
    }

    fn vec3d(&mut self, v: &Vec3d) -> &mut Self {
        for i in 0..3 {
            self.f64(v[i]);
        }
        self
    }

    /// Where these bytes first differ from `want`, and the field there; `None` when they agree.
    fn first_difference(&self, want: &[u8]) -> Option<String> {
        if self.bytes == want {
            return None;
        }
        let at = self
            .bytes
            .iter()
            .zip(want)
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| Ord::min(self.bytes.len(), want.len()));
        let field = self
            .fields
            .as_ref()
            .and_then(|f| f.iter().rev().find(|(start, _)| *start <= at))
            .map_or("the start", |(_, name)| name.as_str());
        Some(format!(
            "byte {at} differs ({} bytes here, {} recorded), in {field}",
            self.bytes.len(),
            want.len()
        ))
    }
}

/// FNV-1a, 64 bits.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for &c in bytes {
        h ^= u64::from(c);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// The `stl_file` as the probe's `snapshot` serialises it: the statistics, then every facet and
/// neighbour entry the arrays hold (a facet's two extra bytes only for a binary file).
fn snapshot(stl: &StlFile, named: bool) -> Payload {
    let mut p = if named {
        Payload::named()
    } else {
        Payload::default()
    };
    let s = &stl.stats;
    p.field("header").str(&s.header);
    let type_ = match s.type_ {
        StlType::Binary => 0,
        StlType::Ascii => 1,
        StlType::InMemory => 2,
    };
    p.field("type").u8(type_);
    p.field("number_of_facets").u32(s.number_of_facets);
    p.field("max").vec3f(&s.max);
    p.field("min").vec3f(&s.min);
    p.field("size").vec3f(&s.size);
    p.field("bounding_diameter").f32(s.bounding_diameter);
    p.field("shortest_edge").f32(s.shortest_edge);
    p.field("volume").f32(s.volume);
    for (name, v) in [
        ("connected_edges", s.connected_edges),
        ("connected_facets_1_edge", s.connected_facets_1_edge),
        ("connected_facets_2_edge", s.connected_facets_2_edge),
        ("connected_facets_3_edge", s.connected_facets_3_edge),
        ("facets_w_1_bad_edge", s.facets_w_1_bad_edge),
        ("facets_w_2_bad_edge", s.facets_w_2_bad_edge),
        ("facets_w_3_bad_edge", s.facets_w_3_bad_edge),
        ("original_num_facets", s.original_num_facets),
        ("edges_fixed", s.edges_fixed),
        ("degenerate_facets", s.degenerate_facets),
        ("facets_removed", s.facets_removed),
        ("facets_added", s.facets_added),
        ("facets_reversed", s.facets_reversed),
        ("backwards_edges", s.backwards_edges),
        ("normals_fixed", s.normals_fixed),
        ("number_of_parts", s.number_of_parts),
    ] {
        p.field(name).i32(v);
    }
    p.field("facet_start's size")
        .u64(stl.facet_start.len() as u64);
    for (i, f) in stl.facet_start.iter().enumerate() {
        p.field_at("facet", i).vec3f(&f.normal);
        for v in &f.vertex {
            p.vec3f(v);
        }
        if s.type_ == StlType::Binary {
            p.u8(f.extra[0]).u8(f.extra[1]);
        }
    }
    p.field("neighbors_start's size")
        .u64(stl.neighbors_start.len() as u64);
    for (i, n) in stl.neighbors_start.iter().enumerate() {
        p.field_at("the neighbours of facet", i);
        for k in 0..3 {
            p.i32(n.neighbor[k]);
        }
        for k in 0..3 {
            p.i32(i32::from(n.which_vertex_not[k]));
        }
    }
    p
}

fn its_bytes(its: &IndexedTriangleSet) -> Vec<u8> {
    let mut p = Payload::default();
    p.u64(its.vertices.len() as u64);
    for v in &its.vertices {
        p.vec3f(v);
    }
    p.u64(its.indices.len() as u64);
    for f in &its.indices {
        for k in 0..3 {
            p.i32(f[k]);
        }
    }
    p.u64(its.properties.len() as u64);
    for f in &its.properties {
        p.u8(f.type_ as u8).f64(f.area);
    }
    p.bytes
}

/// A `TriangleMesh`: its statistics and a hash of its indexed triangle set.
fn put_mesh(p: &mut Payload, m: &TriangleMesh) {
    let s = m.stats();
    p.u32(s.number_of_facets)
        .vec3f(&s.max)
        .vec3f(&s.min)
        .vec3f(&s.size)
        .f32(s.volume)
        .i32(s.number_of_parts)
        .i32(s.open_edges);
    let e = &s.repaired_errors;
    p.i32(e.edges_fixed)
        .i32(e.degenerate_facets)
        .i32(e.facets_removed)
        .i32(e.facets_reversed)
        .i32(e.backwards_edges);
    p.u64(fnv1a(&its_bytes(&m.its)));
}

fn put_bbox(p: &mut Payload, b: &BoundingBoxf3) {
    p.boolean(b.defined).vec3d(&b.min).vec3d(&b.max);
}

fn put_polygon(p: &mut Payload, poly: &Polygon) {
    p.u64(poly.points.len() as u64);
    for pt in &poly.points {
        p.i64(pt.x).i64(pt.y);
    }
}

fn put_trafo(p: &mut Payload, t: &Transform3d) {
    for r in 0..4 {
        for c in 0..4 {
            p.f64(t.m.at(r, c));
        }
    }
}

// The steps of trianglemesh_repair_on_import, as MCAS numbers them.
const OPEN: u8 = 0;
const EXACT: u8 = 1;
const NEARBY_1: u8 = 2;
const NEARBY_2: u8 = 3;
const REMOVE_UNCONNECTED: u8 = 4;
const NORMAL_DIRECTIONS: u8 = 5;
const NORMAL_VALUES: u8 = 6;
const VOLUME: u8 = 7;
const VERIFY_NEIGHBORS: u8 = 8;
const EXACT_AGAIN: u8 = 9;

/// The probe's `repair_steps`: `trianglemesh_repair_on_import` a step at a time, with the state
/// after opening and after each step.
fn repair_steps(stl: &mut StlFile, named: bool) -> Vec<(u8, Payload)> {
    let mut steps = vec![(OPEN, snapshot(stl, named))];
    if stl.stats.number_of_facets == 0 {
        return steps;
    }
    stl_check_facets_exact(stl);
    stl.stats.facets_w_1_bad_edge =
        stl.stats.connected_facets_2_edge - stl.stats.connected_facets_3_edge;
    stl.stats.facets_w_2_bad_edge =
        stl.stats.connected_facets_1_edge - stl.stats.connected_facets_2_edge;
    stl.stats.facets_w_3_bad_edge =
        (stl.stats.number_of_facets as i32).wrapping_sub(stl.stats.connected_facets_1_edge);
    steps.push((EXACT, snapshot(stl, named)));
    let mut tolerance = stl.stats.shortest_edge;
    let increment = stl.stats.bounding_diameter / 10000.0;
    if stl.stats.connected_facets_3_edge < stl.stats.number_of_facets as i32 {
        for i in 0..2 {
            if stl.stats.connected_facets_3_edge < stl.stats.number_of_facets as i32 {
                stl_check_facets_nearby(stl, tolerance);
                tolerance += increment;
                steps.push((
                    if i == 0 { NEARBY_1 } else { NEARBY_2 },
                    snapshot(stl, named),
                ));
            } else {
                break;
            }
        }
    }
    if stl.stats.connected_facets_3_edge < stl.stats.number_of_facets as i32 {
        stl_remove_unconnected_facets(stl);
        steps.push((REMOVE_UNCONNECTED, snapshot(stl, named)));
    }
    stl_fix_normal_directions(stl);
    steps.push((NORMAL_DIRECTIONS, snapshot(stl, named)));
    stl_fix_normal_values(stl);
    steps.push((NORMAL_VALUES, snapshot(stl, named)));
    stl_calculate_volume(stl);
    steps.push((VOLUME, snapshot(stl, named)));
    stl_verify_neighbors(stl);
    steps.push((VERIFY_NEIGHBORS, snapshot(stl, named)));
    if stl.stats.number_of_facets > 0 && stl.stats.degenerate_facets > 0 {
        stl_check_facets_exact(stl);
        steps.push((EXACT_AGAIN, snapshot(stl, named)));
    }
    steps
}

/// One operation of the transform chain, with its parameters.
enum Op {
    Scale(f32),
    ScaleVersor(Vec3f),
    Translate(Vec3f),
    RotateAxis(f32, u8),
    RotateVec(f32, Vec3d),
    Mirror(u8),
    Transform(Transform3d, bool),
    TransformMatrix(Matrix3d, bool),
    Flip,
    Align,
    Merge(Vec3f),
    TransformedBox(Transform3d),
    TransformedBoxAbove(Transform3d, f64),
    RotatePoint(f64, i64, i64),
    Hull2d,
}

impl Op {
    fn name(&self) -> &'static str {
        match self {
            Op::Scale(_) => "scale",
            Op::ScaleVersor(_) => "scale(versor)",
            Op::Translate(_) => "translate",
            Op::RotateAxis(..) => "rotate(angle, axis)",
            Op::RotateVec(..) => "rotate(angle, Vec3d)",
            Op::Mirror(_) => "mirror",
            Op::Transform(..) => "transform(Transform3d)",
            Op::TransformMatrix(..) => "transform(Matrix3d)",
            Op::Flip => "flip_triangles",
            Op::Align => "align_to_origin",
            Op::Merge(_) => "merge",
            Op::TransformedBox(_) => "transformed_bounding_box",
            Op::TransformedBoxAbove(..) => "transformed_bounding_box(trafo, world_min_z)",
            Op::RotatePoint(..) => "rotate(angle, Point*)",
            Op::Hull2d => "convex_hull",
        }
    }
}

/// An MCAS record's inputs: the file, its header length and the operations.
struct Case<'a> {
    label: String,
    bytes: &'a [u8],
    chl: i32,
    ops: Vec<Op>,
}

fn read_vec3f(r: &mut Reader<'_>) -> Vec3f {
    Vec3f::new(r.f32().unwrap(), r.f32().unwrap(), r.f32().unwrap())
}

fn read_trafo(r: &mut Reader<'_>) -> Transform3d {
    let mut rows = [[0.0; 4]; 4];
    for row in &mut rows {
        for v in row.iter_mut() {
            *v = r.f64().unwrap();
        }
    }
    Transform3d::from_matrix(Mat4::from_rows(rows))
}

fn skip_mesh(r: &mut Reader<'_>) {
    // A u32, nine f32s, an f32, two i32s, five i32s and a u64: 80 bytes.
    for _ in 0..10 {
        r.u64().unwrap();
    }
}

fn skip_bbox(r: &mut Reader<'_>) {
    r.u8().unwrap();
    for _ in 0..6 {
        r.f64().unwrap();
    }
}

fn skip_polygon(r: &mut Reader<'_>) {
    let n = r.count().unwrap();
    for _ in 0..n {
        r.i64().unwrap();
        r.i64().unwrap();
    }
}

/// The inputs of an MCAS record; what the C++ computed from them is skipped.
fn parse_case(rec: &Record) -> Case<'_> {
    let mut r = rec.reader();
    let label = r.str().unwrap();
    let bytes = r.bytes().unwrap();
    let chl = r.i32().unwrap();
    let mut ops = Vec::new();
    if r.bool().unwrap() {
        let steps = r.u32().unwrap();
        for _ in 0..steps {
            r.u8().unwrap();
            r.u64().unwrap();
        }
        r.u64().unwrap();
    }
    if r.bool().unwrap() {
        skip_mesh(&mut r);
        r.u64().unwrap();
        skip_bbox(&mut r);
        for _ in 0..6 {
            r.f64().unwrap();
        }
        r.bool().unwrap();
        r.bool().unwrap();
        r.u64().unwrap();
        r.u64().unwrap();
        skip_polygon(&mut r);
        let parts = r.u32().unwrap();
        for _ in 0..parts {
            skip_mesh(&mut r);
        }
        let n = r.u32().unwrap();
        for _ in 0..n {
            let op = match r.u8().unwrap() {
                0 => Op::Scale(r.f32().unwrap()),
                1 => Op::ScaleVersor(read_vec3f(&mut r)),
                2 => Op::Translate(read_vec3f(&mut r)),
                3 => Op::RotateAxis(r.f32().unwrap(), r.u8().unwrap()),
                4 => Op::RotateVec(
                    r.f32().unwrap(),
                    Vec3d::new(r.f64().unwrap(), r.f64().unwrap(), r.f64().unwrap()),
                ),
                5 => Op::Mirror(r.u8().unwrap()),
                6 => Op::Transform(read_trafo(&mut r), r.bool().unwrap()),
                7 => {
                    let mut rows = [[0.0; 3]; 3];
                    for row in &mut rows {
                        for v in row.iter_mut() {
                            *v = r.f64().unwrap();
                        }
                    }
                    Op::TransformMatrix(Mat3::from_rows(rows), r.bool().unwrap())
                }
                8 => Op::Flip,
                9 => Op::Align,
                10 => Op::Merge(read_vec3f(&mut r)),
                11 => Op::TransformedBox(read_trafo(&mut r)),
                12 => Op::TransformedBoxAbove(read_trafo(&mut r), r.f64().unwrap()),
                13 => Op::RotatePoint(r.f64().unwrap(), r.i64().unwrap(), r.i64().unwrap()),
                14 => Op::Hull2d,
                other => panic!("{label}: an MCAS operation {other}"),
            };
            match op {
                Op::TransformedBox(_) | Op::TransformedBoxAbove(..) => skip_bbox(&mut r),
                Op::Hull2d => skip_polygon(&mut r),
                _ => skip_mesh(&mut r),
            }
            ops.push(op);
        }
    }
    r.finish("MCAS").unwrap();
    Case {
        label,
        bytes,
        chl,
        ops,
    }
}

fn axis(a: u8) -> Axis {
    match a {
        0 => Axis::X,
        1 => Axis::Y,
        2 => Axis::Z,
        other => panic!("an axis {other}"),
    }
}

/// The MCAS record the C++ probe would write for `case`, from the Rust.
fn rebuild_case(case: &Case<'_>) -> Payload {
    let mut p = Payload::named();
    p.field("the label").str(case.label.as_bytes());
    p.field("the file").str(case.bytes);
    p.field("the header length").i32(case.chl);
    let mut stl = StlFile::default();
    let opened = stl_open_bytes(&mut stl, Some(case.bytes), &case.label, case.chl).is_ok();
    p.field("stl_open's result").boolean(opened);
    if opened {
        let steps = repair_steps(&mut stl, false);
        p.field("the repair step count").u32(steps.len() as u32);
        for (i, (id, state)) in steps.iter().enumerate() {
            p.field_at("the state after repair step", i)
                .u8(*id)
                .u64(fnv1a(&state.bytes));
        }
        let mut its = IndexedTriangleSet::default();
        stl_generate_shared_vertices(&stl, &mut its);
        p.field("stl_generate_shared_vertices")
            .u64(fnv1a(&its_bytes(&its)));
    }
    let mut mesh = TriangleMesh::default();
    let read = mesh
        .read_stl_bytes(Some(case.bytes), &case.label, true, case.chl)
        .is_ok();
    p.field("ReadSTLFile's result").boolean(read);
    if !read {
        return p;
    }
    p.field("the mesh read");
    put_mesh(&mut p, &mesh);
    p.field("facets_count").u64(mesh.facets_count() as u64);
    p.field("bounding_box");
    put_bbox(&mut p, &mesh.bounding_box());
    p.field("size").vec3d(&mesh.size());
    p.field("center").vec3d(&mesh.center());
    p.field("empty").boolean(mesh.empty());
    p.field("is_splittable").boolean(mesh.is_splittable());
    p.field("its_num_open_edges")
        .u64(its_num_open_edges_of(&mesh.its) as u64);
    p.field("its_number_of_patches")
        .u64(its_number_of_patches(&mesh.its) as u64);
    p.field("convex_hull");
    put_polygon(&mut p, &mesh.convex_hull());
    let parts = mesh.split();
    p.field("split's part count").u32(parts.len() as u32);
    for (i, part) in parts.iter().enumerate() {
        p.field_at("split part", i);
        put_mesh(&mut p, part);
    }
    let first = mesh.clone();
    let mut work = mesh;
    p.field("the operation count").u32(case.ops.len() as u32);
    for (i, op) in case.ops.iter().enumerate() {
        p.field(&format!("operation {i}, {}", op.name()));
        match *op {
            Op::Scale(f) => {
                p.u8(0).f32(f);
                work.scale(f);
                put_mesh(&mut p, &work);
            }
            Op::ScaleVersor(v) => {
                p.u8(1).vec3f(&v);
                work.scale_versor(&v);
                put_mesh(&mut p, &work);
            }
            Op::Translate(v) => {
                p.u8(2).vec3f(&v);
                work.translate(&v);
                put_mesh(&mut p, &work);
            }
            Op::RotateAxis(a, ax) => {
                p.u8(3).f32(a).u8(ax);
                work.rotate_axis(a, axis(ax));
                put_mesh(&mut p, &work);
            }
            Op::RotateVec(a, v) => {
                p.u8(4).f32(a).vec3d(&v);
                work.rotate_about(a, &v);
                put_mesh(&mut p, &work);
            }
            Op::Mirror(ax) => {
                p.u8(5).u8(ax);
                work.mirror(axis(ax));
                put_mesh(&mut p, &work);
            }
            Op::Transform(t, fix) => {
                p.u8(6);
                put_trafo(&mut p, &t);
                p.boolean(fix);
                work.transform(&t, fix);
                put_mesh(&mut p, &work);
            }
            Op::TransformMatrix(m, fix) => {
                p.u8(7);
                for r in 0..3 {
                    for c in 0..3 {
                        p.f64(m.at(r, c));
                    }
                }
                p.boolean(fix);
                work.transform_matrix(&m, fix);
                put_mesh(&mut p, &work);
            }
            Op::Flip => {
                p.u8(8);
                work.flip_triangles();
                put_mesh(&mut p, &work);
            }
            Op::Align => {
                p.u8(9);
                work.align_to_origin();
                put_mesh(&mut p, &work);
            }
            Op::Merge(v) => {
                p.u8(10).vec3f(&v);
                let mut other = first.clone();
                other.translate(&v);
                work.merge(&other);
                put_mesh(&mut p, &work);
            }
            Op::TransformedBox(t) => {
                p.u8(11);
                put_trafo(&mut p, &t);
                put_bbox(&mut p, &work.transformed_bounding_box(&t));
            }
            Op::TransformedBoxAbove(t, z) => {
                p.u8(12);
                put_trafo(&mut p, &t);
                p.f64(z);
                put_bbox(&mut p, &work.transformed_bounding_box_above(&t, z));
            }
            Op::RotatePoint(a, x, y) => {
                p.u8(13).f64(a).i64(x).i64(y);
                work.rotate_about_point(a, &Point::new(x, y));
                put_mesh(&mut p, &work);
            }
            Op::Hull2d => {
                p.u8(14);
                put_polygon(&mut p, &work.convex_hull());
            }
        }
    }
    p
}

/// The probe's threading, the oracle's: one thread in an arena of concurrency 8
/// (`task_arena{8, 8u}` under `global_control(max_allowed_parallelism, 1)`). `its_split`,
/// `its_is_splittable` and `its_number_of_patches` take their face neighbours from
/// `create_face_neighbors_index(ex_tbb, ...)`, whose threads race on shared entries
/// (`parity/HAZARDS.md`, orca-mesh), so a result holds only for that one thread.
fn oracle_arena() -> orca_tbb::Arena {
    orca_tbb::Arena::serial(8)
}

#[test]
fn stl_files_read_repair_and_transform_as_the_engine_does() {
    quiet();
    let file = mesh_file();
    let mut m = Mismatches::default();
    oracle_arena().execute(|| {
        for rec in file.of_kind("MCAS") {
            let case = parse_case(rec);
            let diff = rebuild_case(&case).first_difference(&rec.payload);
            m.check(diff.is_none(), || {
                format!("{}: {}", case.label, diff.unwrap_or_default())
            });
        }
    });
    m.assert_none("MCAS");
}

#[test]
fn repair_steps_leave_admesh_as_the_engine_leaves_it() {
    quiet();
    let file = mesh_file();
    let mut m = Mismatches::default();
    let mut last_case: Option<&Record> = None;
    for rec in &file.records {
        match rec.kind() {
            "MCAS" => last_case = Some(rec),
            "MSTD" => {
                let case = parse_case(last_case.expect("an MSTD record follows its MCAS record"));
                let mut p = Payload::named();
                p.field("the label").str(case.label.as_bytes());
                let mut stl = StlFile::default();
                if stl_open_bytes(&mut stl, Some(case.bytes), &case.label, case.chl).is_ok() {
                    let steps = repair_steps(&mut stl, true);
                    p.field("the repair step count").u32(steps.len() as u32);
                    for (i, (id, state)) in steps.iter().enumerate() {
                        p.field_at("repair step", i).u8(*id);
                        p.str_of(&format!("the state after repair step {i} ({id})"), state);
                    }
                    let mut its = IndexedTriangleSet::default();
                    stl_generate_shared_vertices(&stl, &mut its);
                    p.field("stl_generate_shared_vertices")
                        .str(&its_bytes(&its));
                } else {
                    p.field("stl_open, which failed here");
                }
                let diff = p.first_difference(&rec.payload);
                m.check(diff.is_none(), || {
                    format!("{}: {}", case.label, diff.unwrap_or_default())
                });
            }
            _ => {}
        }
    }
    if file.set == ProbeSet::Committed && m.checked == 0 {
        // The committed set holds no MSTD record: they are the full set's alone.
        return;
    }
    m.assert_none("MSTD");
}
