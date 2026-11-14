// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The probe gate for orca-model: the records of `model.sop1` (`parity/SOP1.md`, "Probe files"),
//! from the committed subset (`parity/probes/`), or from the full set when
//! `SKETCH_ORCA_PROBES_DEEP` names its folder (`<work>/parity/probes`).
//!
//! Each record is rebuilt here byte for byte as `parity/oracle/probe/model.cpp` writes it, from
//! the inputs it holds and what this crate computes with them, and the two are compared whole; a
//! difference is named by the field it falls in. The kinds: MXFM (Transformation), MFAC (a part's
//! painting), MCFG (ModelConfig), MCIR (the circle fits), MBVL (BuildVolume), MLOD (a model file
//! read as the command line reads it, then placed, checked against beds and arranged), M3MF (a 3MF
//! file read so), MARP (the bed and the items' inflation) and MTMP (the filaments' temperature
//! classes). Filled by o2-model (2026-11-07); M3MF by orca-3mf-probe (2026-11-10), one test for
//! each of the 3MF reader's three pieces, and all three replay: piece 1's (orca-3mf-port-1,
//! 2026-11-10), piece 2's (orca-3mf-port-2, 2026-11-11) and piece 3's (orca-3mf-port-3,
//! 2026-11-11).
//!
//! The records of `slice.sop1` of the piece front-2 (SLFE, SLMC, SLTS, SLSH and SLBX,
//! `parity/sop1/slice.md`) are replayed by the module `probes/slice_front.rs` (orca-o3-front-2,
//! 2026-11-13), with the helpers here.

#[path = "probes/slice_front.rs"]
mod slice_front;

use std::cell::RefCell;
use std::path::PathBuf;

use orca_arrange::arrange::{ArrangeParams, ArrangePolygon};
use orca_eigen::{Mat4, Transform3d, Vec2d, Vec3d, Vec3f, Vec3i32};
use orca_geom::bounding_box::{BoundingBox, BoundingBoxf3};
use orca_geom::point::{Point, Points};
use orca_geom::polygon::Polygon;
use orca_mesh::admesh::IndexedTriangleSet;
use orca_mesh::log::Severity;
use orca_mesh::triangle_mesh::TriangleMesh;
use orca_model::arrange::{
    apply_arrange_polygon, get_bed_shape, get_compatible_filament_type, get_filament_temp_type,
    get_instance_arrange_poly, get_shrink_bedpts, update_arrange_params,
    update_selected_items_inflation, update_unselected_items_inflation,
};
use orca_model::build_volume::{BuildVolume, BuildVolumeType};
use orca_model::circle::{Circled, circle_ransac, smallest_enclosing_circle_welzl};
use orca_model::config::ModelConfig;
use orca_model::facets::FacetsAnnotation;
use orca_model::model::Model;
use orca_model::read::{load_strategy, read_from_file};
use orca_model::transformation::{Transformation, transform3d_from_string};
use orca_probe::{Mismatches, ProbeFile, Reader, Record, load_probe};

fn model_file() -> ProbeFile {
    load_probe("model").unwrap_or_else(|e| panic!("{e}"))
}

thread_local! {
    /// The log of the read this thread is replaying (`logged`), or none.
    static READ_LOG: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Discards what libslic3r logs and what Qhull writes to `std::cerr`, which the probe does not
/// record, but for the log of a read that `logged` holds on the calling thread (M3MF records it).
/// The sink is the same for every test: the tests run on threads of their own, each sets it, and a
/// record goes to the log of its own thread only (the oracle's arena runs a read on the thread
/// that calls it).
fn quiet() {
    orca_mesh::log::set_sink(Some(Box::new(|severity, message| {
        READ_LOG.with(|log| {
            if let Some(text) = log.borrow_mut().as_mut() {
                put_log_record(text, severity, message);
            }
        });
    })));
    orca_mesh::log::set_stderr_sink(Some(Box::new(|_| {})));
    orca_fmt::stdio::set_stdout_sink(Some(Box::new(|_| {})));
}

/// One record as the engine prints it on stderr (`cli/main.cpp`, `log_to_stderr`, at its default
/// level 1), the probe's `ReadLog`: error and fatal only, `[<severity>] <message>` and a line feed
/// unless the message ends with one.
fn put_log_record(text: &mut String, severity: Severity, message: &str) {
    if severity < Severity::Error {
        return;
    }
    text.push('[');
    text.push_str(severity.name());
    text.push_str("] ");
    text.push_str(message);
    if !message.ends_with('\n') {
        text.push('\n');
    }
}

/// Runs `f` and returns what it returns with the log of it (`put_log_record`), as the probe's
/// `ReadLog` holds it around a read. `quiet` must have set the sink.
fn logged<T>(f: impl FnOnce() -> T) -> (T, String) {
    READ_LOG.with(|log| *log.borrow_mut() = Some(String::new()));
    let out = f();
    let text = READ_LOG
        .with(|log| log.borrow_mut().take())
        .unwrap_or_default();
    (out, text)
}

/// The probe's threading, the oracle's: one thread in an arena of concurrency 8.
fn oracle_arena() -> orca_tbb::Arena {
    orca_tbb::Arena::serial(8)
}

/// Bytes as `probe.hpp`'s `Payload` writes them (little-endian; a `str` as its `u32` length and
/// its bytes), with where each field starts, to name a difference.
#[derive(Default)]
struct Payload {
    bytes: Vec<u8>,
    fields: Vec<(usize, String)>,
}

impl Payload {
    fn field(&mut self, name: &str) -> &mut Self {
        self.fields.push((self.bytes.len(), name.to_owned()));
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

    fn len(&mut self, n: usize) -> &mut Self {
        self.u64(n as u64)
    }

    fn vec3(&mut self, v: &Vec3d) -> &mut Self {
        self.f64(v.x()).f64(v.y()).f64(v.z())
    }

    fn mat(&mut self, t: &Transform3d) -> &mut Self {
        for r in 0..4 {
            for c in 0..4 {
                self.f64(t.m.at(r, c));
            }
        }
        self
    }

    fn bb3(&mut self, b: &BoundingBoxf3) -> &mut Self {
        self.boolean(b.defined).vec3(&b.min).vec3(&b.max)
    }

    fn bb(&mut self, b: &BoundingBox) -> &mut Self {
        self.boolean(b.defined)
            .i64(b.min.x)
            .i64(b.min.y)
            .i64(b.max.x)
            .i64(b.max.y)
    }

    fn points(&mut self, pts: &[Point]) -> &mut Self {
        self.len(pts.len());
        for p in pts {
            self.i64(p.x).i64(p.y);
        }
        self
    }

    fn vec2s(&mut self, pts: &[Vec2d]) -> &mut Self {
        self.len(pts.len());
        for v in pts {
            self.f64(v.x()).f64(v.y());
        }
        self
    }

    fn circle(&mut self, c: &Circled) -> &mut Self {
        self.f64(c.center.x()).f64(c.center.y()).f64(c.radius)
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
            .iter()
            .rev()
            .find(|(start, _)| *start <= at)
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

/// The probe's `its_hash`: the vertex count and the vertices, the face count and the faces.
fn its_hash(its: &IndexedTriangleSet) -> u64 {
    let mut p = Payload::default();
    p.len(its.vertices.len());
    for v in &its.vertices {
        p.f32(v.x()).f32(v.y()).f32(v.z());
    }
    p.len(its.indices.len());
    for f in &its.indices {
        for k in 0..3 {
            p.i32(f[k]);
        }
    }
    fnv1a(&p.bytes)
}

/// The probe's `config_dump`: "key=value\n" for each option, in key order.
fn config_dump(c: &ModelConfig) -> Vec<u8> {
    let mut out = String::new();
    for (k, v) in c.iter() {
        out.push_str(k);
        out.push('=');
        out.push_str(&v.serialize().unwrap_or_else(|e| panic!("{k}: {e:?}")));
        out.push('\n');
    }
    out.into_bytes()
}

fn vec3(r: &mut Reader<'_>) -> Vec3d {
    Vec3d::new(r.f64().unwrap(), r.f64().unwrap(), r.f64().unwrap())
}

fn mat(r: &mut Reader<'_>) -> Transform3d {
    let mut rows = [[0.0; 4]; 4];
    for row in &mut rows {
        for v in row.iter_mut() {
            *v = r.f64().unwrap();
        }
    }
    Transform3d::from_matrix(Mat4::from_rows(rows))
}

fn vec2s(r: &mut Reader<'_>) -> Vec<Vec2d> {
    let n = r.count().unwrap();
    (0..n)
        .map(|_| Vec2d::new(r.f64().unwrap(), r.f64().unwrap()))
        .collect()
}

fn points(r: &mut Reader<'_>) -> Points {
    let n = r.count().unwrap();
    (0..n)
        .map(|_| Point::new(r.i64().unwrap(), r.i64().unwrap()))
        .collect()
}

fn bytes_str(r: &mut Reader<'_>) -> Vec<u8> {
    r.bytes().unwrap().to_vec()
}

/// A bed of the probe's `put_bed`.
struct Bed {
    area: Vec<Vec2d>,
    height: f64,
    extruders: Vec<Vec<Vec2d>>,
    extruder_heights: Vec<f64>,
}

impl Bed {
    fn read(r: &mut Reader<'_>) -> Bed {
        let area = vec2s(r);
        let height = r.f64().unwrap();
        let n = r.count().unwrap();
        let mut extruders = Vec::new();
        let mut extruder_heights = Vec::new();
        for _ in 0..n {
            extruders.push(vec2s(r));
            extruder_heights.push(r.f64().unwrap());
        }
        Bed {
            area,
            height,
            extruders,
            extruder_heights,
        }
    }

    fn put(&self, p: &mut Payload) {
        p.vec2s(&self.area)
            .f64(self.height)
            .len(self.extruders.len());
        for (e, h) in self.extruders.iter().zip(&self.extruder_heights) {
            p.vec2s(e).f64(*h);
        }
    }

    fn build(&self) -> BuildVolume {
        BuildVolume::new(
            &self.area,
            self.height,
            &self.extruders,
            &self.extruder_heights,
        )
    }
}

/// Compares every record of `kind`, each rebuilt by `rebuild`.
fn check_kind(kind: &str, rebuild: impl Fn(&Record) -> Payload + Sync) {
    quiet();
    let file = model_file();
    let mut m = Mismatches::default();
    oracle_arena().execute(|| {
        for (i, rec) in file.of_kind(kind).enumerate() {
            let diff = rebuild(rec).first_difference(&rec.payload);
            m.check(diff.is_none(), || {
                format!("{kind} #{i}: {}", diff.unwrap_or_default())
            });
        }
    });
    m.assert_none(kind);
}

// ---- MXFM ----------------------------------------------------------------------------------

fn rebuild_xfm(rec: &Record) -> Payload {
    let mut r = rec.reader();
    let (off, rot, scl, mir) = (vec3(&mut r), vec3(&mut r), vec3(&mut r), vec3(&mut r));
    let raw = mat(&mut r);
    let text = bytes_str(&mut r);
    let mut p = Payload::default();
    p.field("the inputs")
        .vec3(&off)
        .vec3(&rot)
        .vec3(&scl)
        .vec3(&mir)
        .mat(&raw)
        .str(&text);
    let mut t = Transformation::default();
    t.set_offset(&off);
    t.set_rotation(&rot);
    t.set_scaling_factor(&scl);
    t.set_mirror(&mir);
    p.field("get_matrix").mat(t.get_matrix());
    p.field("get_matrix_no_offset")
        .mat(&t.get_matrix_no_offset());
    p.field("get_offset").vec3(&t.get_offset());
    p.field("get_rotation").vec3(&t.get_rotation());
    p.field("get_scaling_factor").vec3(&t.get_scaling_factor());
    p.field("get_mirror").vec3(&t.get_mirror());
    p.field("is_left_handed").boolean(t.is_left_handed());
    let u = Transformation::new(&raw);
    p.field("the matrix's get_offset").vec3(&u.get_offset());
    p.field("the matrix's get_rotation").vec3(&u.get_rotation());
    p.field("the matrix's get_scaling_factor")
        .vec3(&u.get_scaling_factor());
    p.field("the matrix's get_mirror").vec3(&u.get_mirror());
    p.field("the matrix's is_left_handed")
        .boolean(u.is_left_handed());
    p.field("the matrix's get_matrix_no_offset")
        .mat(&u.get_matrix_no_offset());
    let text = String::from_utf8(text).expect("an ASCII text");
    p.field("transform3d_from_string")
        .mat(&transform3d_from_string(&text));
    p
}

#[test]
fn transformations_decompose_as_the_engines() {
    check_kind("MXFM", rebuild_xfm);
}

// ---- MFAC ----------------------------------------------------------------------------------

fn put_facets(p: &mut Payload, f: &FacetsAnnotation) {
    let d = f.get_data();
    p.len(d.triangles_to_split.len());
    for m in &d.triangles_to_split {
        p.i32(m.triangle_idx).i32(m.bitstream_start_idx);
    }
    p.len(d.bitstream.len());
    for &b in &d.bitstream {
        p.boolean(b);
    }
    p.len(d.used_states.len());
    for &b in &d.used_states {
        p.boolean(b);
    }
}

fn rebuild_fac(rec: &Record) -> Payload {
    let mut r = rec.reader();
    let n = r.count().unwrap();
    let tris: Vec<(i32, Vec<u8>)> = (0..n)
        .map(|_| (r.i32().unwrap(), bytes_str(&mut r)))
        .collect();
    let absent = r.i32().unwrap();
    let mut p = Payload::default();
    p.field("the triangles").len(tris.len());
    for (id, s) in &tris {
        p.i32(*id).str(s);
    }
    p.i32(absent);
    let mut f = FacetsAnnotation::default();
    for (id, s) in &tris {
        f.set_triangle_from_string(*id, s);
    }
    p.field("the data");
    put_facets(&mut p, &f);
    for (i, (id, _)) in tris.iter().enumerate() {
        p.field(&format!("get_triangle_as_string {i}"))
            .str(f.get_triangle_as_string(*id).as_bytes());
    }
    p.field("get_triangle_as_string of an absent triangle")
        .str(f.get_triangle_as_string(absent).as_bytes());
    f.reset();
    let (id, s) = tris.last().expect("a triangle");
    f.set_triangle_from_string(*id, s);
    p.field("the data after reset() and the last triangle");
    put_facets(&mut p, &f);
    p
}

#[test]
fn painting_strings_load_as_the_engines() {
    check_kind("MFAC", rebuild_fac);
}

// ---- MCFG ----------------------------------------------------------------------------------

fn rebuild_cfg(rec: &Record) -> Payload {
    let mut r = rec.reader();
    let key = r.str().unwrap();
    let value = r.str().unwrap();
    let mut p = Payload::default();
    p.field("the key and the value")
        .str(key.as_bytes())
        .str(value.as_bytes());
    let mut c = ModelConfig::new();
    match c.set_deserialize(&key, &value) {
        Ok(()) => p.field("the options").u8(0).str(&config_dump(&c)),
        Err(e) => p.field("the exception").u8(1).str(e.what().as_bytes()),
    };
    p
}

#[test]
fn model_configs_read_values_as_the_engine() {
    check_kind("MCFG", rebuild_cfg);
}

// ---- MCIR ----------------------------------------------------------------------------------

fn rebuild_cir(rec: &Record) -> Payload {
    let mut r = rec.reader();
    let pts = vec2s(&mut r);
    let mut p = Payload::default();
    p.field("the points").vec2s(&pts);
    let (c, err) = circle_ransac(&pts, 20);
    // Fewer than three points: the C++ leaves the error as the caller set it (-1).
    let err = if pts.len() < 3 { -1.0 } else { err };
    p.field("circle_ransac").circle(&c).f64(err);
    let sp = Polygon::new_scale(&pts).points;
    if !sp.is_empty() {
        p.field("smallest_enclosing_circle_welzl")
            .circle(&smallest_enclosing_circle_welzl(&sp));
    }
    p
}

#[test]
fn circles_fit_as_the_engines() {
    check_kind("MCIR", rebuild_cir);
}

// ---- MBVL ----------------------------------------------------------------------------------

fn unit_cube() -> IndexedTriangleSet {
    let v = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 1.0],
    ];
    let f = [
        [0, 2, 1],
        [0, 3, 2],
        [4, 5, 6],
        [4, 6, 7],
        [0, 1, 5],
        [0, 5, 4],
        [1, 2, 6],
        [1, 6, 5],
        [2, 3, 7],
        [2, 7, 6],
        [3, 0, 4],
        [3, 4, 7],
    ];
    IndexedTriangleSet {
        vertices: v.iter().map(|p| Vec3f::new(p[0], p[1], p[2])).collect(),
        indices: f.iter().map(|t| Vec3i32::new(t[0], t[1], t[2])).collect(),
        properties: Vec::new(),
    }
}

fn rebuild_bvl(rec: &Record) -> Payload {
    let mut r = rec.reader();
    let bed = Bed::read(&mut r);
    let mut p = Payload::default();
    p.field("the bed");
    bed.put(&mut p);
    let bv = bed.build();
    p.field("type").i32(i32::from(bv.volume_type().value()));
    p.field("type_name").str(bv.volume_type().name().as_bytes());
    let (hull, bbox, _) = bv.convex_hull();
    p.field("bounding_box").bb(&bbox);
    p.field("bounding_volume").bb3(&bv.bounding_volume());
    p.field("convex_hull").points(&hull.points);
    p.field("polygon").points(&bv.polygon().points);
    p.field("circle").circle(&bv.circle());
    let (scene, bed_d) = bv.decompositions();
    p.field("the scene's decomposition")
        .vec2s(&scene.0)
        .vec2s(&scene.1);
    p.field("the bed's decomposition")
        .vec2s(&bed_d.0)
        .vec2s(&bed_d.1);
    p.field("the extruder volumes")
        .len(bv.extruder_volumes().len());
    for ev in bv.extruder_volumes() {
        p.boolean(ev.same_with_bed)
            .i32(i32::from(ev.volume_type.value()))
            .bb(&ev.bbox)
            .bb3(&ev.bboxf)
            .circle(&ev.circle);
    }
    // The cases' matrices and flags follow what the C++ computed of the bed: walked past here.
    let _type = r.i32().unwrap();
    let _name = r.bytes().unwrap();
    for _ in 0..(33 + 49) {
        let _ = r.u8().unwrap();
    }
    let _ = points(&mut r);
    let _ = points(&mut r);
    for _ in 0..3 {
        let _ = r.f64().unwrap();
    }
    for _ in 0..4 {
        let _ = vec2s(&mut r);
    }
    let n = r.count().unwrap();
    for _ in 0..n * (1 + 4 + 33 + 49 + 24) {
        let _ = r.u8().unwrap();
    }
    let cube = unit_cube();
    let mesh = TriangleMesh::from_its(cube.clone());
    for k in 0..24 {
        let t = mat(&mut r);
        let below = r.bool().unwrap();
        let bottom = r.bool().unwrap();
        let _ = r.u8().unwrap();
        if bv.volume_type() == BuildVolumeType::Rectangle {
            let _ = r.u8().unwrap();
        }
        p.field(&format!("case {k}"))
            .mat(&t)
            .boolean(below)
            .boolean(bottom);
        p.field(&format!("case {k}: object_state")).u8(bv
            .object_state(&cube, &t.cast::<f32>(), below, bottom)
            .value());
        if bv.volume_type() == BuildVolumeType::Rectangle {
            p.field(&format!("case {k}: volume_state_bbox")).u8(bv
                .volume_state_bbox(&mesh.transformed_bounding_box(&t), bottom)
                .value());
        }
    }
    p
}

#[test]
fn build_volumes_classify_and_check_as_the_engines() {
    check_kind("MBVL", rebuild_bvl);
}

// ---- MLOD ----------------------------------------------------------------------------------

/// A folder for the model files, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!(
            "sketch-orca-model-probe-{tag}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("a temporary folder");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The settings a record lists, as `DynamicPrintConfig::set_deserialize_strict` sets them.
fn settings(pairs: &[(String, String)]) -> ModelConfig {
    let mut c = ModelConfig::new();
    for (k, v) in pairs {
        c.set_deserialize(k, v)
            .unwrap_or_else(|e| panic!("{k}={v}: {}", e.what()));
    }
    c
}

fn read_pairs(r: &mut Reader<'_>) -> Vec<(String, String)> {
    let n = r.count().unwrap();
    (0..n)
        .map(|_| (r.str().unwrap(), r.str().unwrap()))
        .collect()
}

fn put_pairs(p: &mut Payload, pairs: &[(String, String)]) {
    p.len(pairs.len());
    for (k, v) in pairs {
        p.str(k.as_bytes()).str(v.as_bytes());
    }
}

fn put_ap(p: &mut Payload, ap: &ArrangePolygon) {
    p.points(&ap.poly.contour.points)
        .i64(ap.translation.x)
        .i64(ap.translation.y)
        .f64(ap.rotation)
        .i32(ap.bed_idx)
        .len(ap.extrude_ids.len());
    for &e in &ap.extrude_ids {
        p.i32(e);
    }
    p.i32(ap.bed_temp)
        .i32(ap.first_bed_temp)
        .i32(ap.print_temp)
        .i32(ap.first_print_temp)
        .i32(ap.vitrify_temp)
        .i32(ap.filament_temp_type)
        .f64(ap.brim_width)
        .boolean(ap.has_tree_support)
        .f64(ap.height)
        .str(ap.name.as_bytes())
        .i64(ap.inflation);
}

fn put_model(p: &mut Payload, model: &Model) {
    p.field("the model").len(model.objects.len());
    for (oi, o) in model.objects.iter().enumerate() {
        p.field(&format!("object {oi}"))
            .str(o.name.as_bytes())
            .boolean(o.printable)
            .vec3(&o.origin_translation)
            .str(&config_dump(&o.config))
            .len(o.instances.len());
        for (ii, i) in o.instances.iter().enumerate() {
            p.field(&format!("object {oi} instance {ii}"))
                .mat(i.get_matrix())
                .u8(i.print_volume_state.value())
                .boolean(i.printable)
                .boolean(i.auto_drop)
                .i32(i.arrange_order);
        }
        p.len(o.volumes.len());
        for (vi, v) in o.volumes.iter().enumerate() {
            let its = &v.mesh().its;
            p.field(&format!("object {oi} volume {vi}"))
                .str(v.name.as_bytes())
                .i32(v.volume_type().value())
                .mat(v.get_matrix());
            p.field(&format!("object {oi} volume {vi}: the mesh"))
                .len(its.vertices.len())
                .len(its.indices.len())
                .u64(its_hash(its));
            let hull = v.try_get_convex_hull();
            p.field(&format!("object {oi} volume {vi}: the hull"))
                .boolean(hull.is_some());
            if let Some(h) = hull {
                p.len(h.its.vertices.len())
                    .len(h.its.indices.len())
                    .u64(its_hash(&h.its));
            }
            p.field(&format!("object {oi} volume {vi}: the source"))
                .i32(v.source.object_idx)
                .i32(v.source.volume_idx)
                .vec3(&v.source.mesh_offset)
                .str(&config_dump(&v.config))
                .i32(v.get_repaired_errors_count());
            p.field(&format!("object {oi} volume {vi}: the painting"));
            put_facets(p, &v.supported_facets);
            put_facets(p, &v.seam_facets);
            put_facets(p, &v.mmu_segmentation_facets);
            put_facets(p, &v.fuzzy_skin_facets);
        }
    }
}

/// An arrange result a record lists: the bed, the translation, the rotation and the item's id.
type Placed = (i32, i64, i64, f64, i32);

/// The beds of MBVL's records, in their order: an MLOD record names its beds by index among them.
fn mbvl_beds(file: &ProbeFile) -> Vec<Bed> {
    file.of_kind("MBVL")
        .map(|rec| Bed::read(&mut rec.reader()))
        .collect()
}

/// The inputs of the steps after loading (the probe's `LodInputs`): two instances (offset,
/// scale, rotation, mirror), allow_negative_z, the beds, the print settings and three arrange
/// results.
struct LodInputs {
    inst: [[f64; 12]; 2],
    allow_negative_z: bool,
    bed_ids: Vec<u64>,
    pairs: Vec<(String, String)>,
    placed: Vec<Placed>,
}

impl LodInputs {
    fn read(r: &mut Reader<'_>) -> LodInputs {
        let mut inst = [[0.0f64; 12]; 2];
        for row in &mut inst {
            for v in row.iter_mut() {
                *v = r.f64().unwrap();
            }
        }
        let allow_negative_z = r.bool().unwrap();
        let nbeds = r.count().unwrap();
        let bed_ids: Vec<u64> = (0..nbeds).map(|_| r.u64().unwrap()).collect();
        let pairs = read_pairs(r);
        let placed: Vec<Placed> = (0..3)
            .map(|_| {
                (
                    r.i32().unwrap(),
                    r.i64().unwrap(),
                    r.i64().unwrap(),
                    r.f64().unwrap(),
                    r.i32().unwrap(),
                )
            })
            .collect();
        LodInputs {
            inst,
            allow_negative_z,
            bed_ids,
            pairs,
            placed,
        }
    }

    fn put(&self, p: &mut Payload) {
        p.field("the instances' inputs");
        for row in &self.inst {
            for &v in row {
                p.f64(v);
            }
        }
        p.boolean(self.allow_negative_z);
        p.field("the beds").len(self.bed_ids.len());
        for &b in &self.bed_ids {
            p.u64(b);
        }
        p.field("the settings");
        put_pairs(p, &self.pairs);
        p.field("the arrange results");
        for &(bed, x, y, rot, item) in &self.placed {
            p.i32(bed).i64(x).i64(y).f64(rot).i32(item);
        }
    }
}

/// cli/run.cpp's load_model after the read: each configuration cut to "extruder", the height
/// ranges and the layer height profile dropped, the first plate, ensure_on_bed.
fn cli_cuts(model: &mut Model) {
    for mo in &mut model.objects {
        for key in mo.config.keys() {
            if key != "extruder" {
                mo.config.erase(&key);
            }
        }
        mo.layer_config_ranges.clear();
        mo.layer_height_profile.clear();
        for v in &mut mo.volumes {
            for key in v.config.keys() {
                if key != "extruder" {
                    v.config.erase(&key);
                }
            }
        }
    }
    model.curr_plate_index = 0;
    for mo in &mut model.objects {
        mo.ensure_on_bed(false);
    }
}

fn rebuild_lod(rec: &Record, dir: &TempDir, all_beds: &[Bed]) -> (String, Payload) {
    let mut r = rec.reader();
    let label = r.str().unwrap();
    let name = r.str().unwrap();
    let bytes = bytes_str(&mut r);
    let nextra = r.count().unwrap();
    let extra: Vec<(String, Vec<u8>)> = (0..nextra)
        .map(|_| (r.str().unwrap(), bytes_str(&mut r)))
        .collect();
    let inputs = LodInputs::read(&mut r);

    let mut p = Payload::default();
    p.field("the label, the name and the bytes")
        .str(label.as_bytes())
        .str(name.as_bytes())
        .str(&bytes);
    p.field("the files beside it").len(extra.len());
    for (n, b) in &extra {
        p.str(n.as_bytes()).str(b);
    }
    inputs.put(&mut p);

    let file = dir.0.join(&name);
    std::fs::write(&file, &bytes).expect("the model file written");
    for (n, b) in &extra {
        std::fs::write(dir.0.join(n), b).expect("a file beside it written");
    }
    let path = file.to_str().expect("a UTF-8 path").to_owned();
    let read = read_from_file(
        &path,
        load_strategy::LOAD_MODEL | load_strategy::ADD_DEFAULT_INSTANCES,
    );
    let _ = std::fs::remove_file(&file);
    for (n, _) in &extra {
        let _ = std::fs::remove_file(dir.0.join(n));
    }
    let mut model = match read {
        Ok(m) => m,
        Err(e) => {
            p.field("the error").u8(1).str(e.what().as_bytes());
            return (label, p);
        }
    };
    p.field("read").u8(0);
    cli_cuts(&mut model);
    put_model(&mut p, &model);
    lod_tail(&mut p, &mut model, &inputs, all_beds, &label, false);
    (label, p)
}

/// The steps after loading, on the first object (the probe's `lod_tail`): its boxes, hulls and z,
/// two instances added, ensure_on_bed, the print volume states, the arrange polygons of its first
/// three instances with a result applied to each, and center_around_origin. They run only when
/// every part has a hull (and, when `require_parts`, the object has a part).
fn lod_tail(
    p: &mut Payload,
    model: &mut Model,
    inputs: &LodInputs,
    all_beds: &[Bed],
    label: &str,
    require_parts: bool,
) {
    let LodInputs {
        inst,
        allow_negative_z,
        bed_ids,
        pairs,
        placed,
    } = inputs;
    let allow_negative_z = *allow_negative_z;
    let first = &model.objects[0];
    let hull_ok = (!require_parts || !first.volumes.is_empty())
        && first
            .volumes
            .iter()
            .all(|v| v.try_get_convex_hull().is_some());
    p.field("every part has a hull").boolean(hull_ok);
    if !hull_ok {
        return;
    }
    let beds: Vec<&Bed> = bed_ids
        .iter()
        .map(|&b| &all_beds[usize::try_from(b).expect("a bed index")])
        .collect();
    {
        let o = &mut model.objects[0];
        p.field("bounding_box_approx").bb3(&o.bounding_box_approx());
        p.field("bounding_box_exact").bb3(&o.bounding_box_exact());
        p.field("min_z, max_z").f64(o.min_z()).f64(o.max_z());
        p.field("instance_bounding_box")
            .bb3(&o.instance_bounding_box(0, false))
            .bb3(&o.instance_bounding_box(0, true));
        p.field("instance_convex_hull_bounding_box")
            .bb3(&o.instance_convex_hull_bounding_box(0, false))
            .bb3(&o.instance_convex_hull_bounding_box(0, true));
        let raw = o
            .raw_bounding_box()
            .unwrap_or_else(|e| panic!("{label}: {}", e.0));
        p.field("raw_bounding_box").bb3(&raw);
        p.field("raw_mesh_bounding_box")
            .bb3(&o.raw_mesh_bounding_box());
        p.field("full_raw_mesh_bounding_box")
            .bb3(&o.full_raw_mesh_bounding_box());
        p.field("facets_count, parts_count")
            .len(o.facets_count())
            .len(o.parts_count());
        p.field("get_instance_min_z, get_instance_max_z")
            .f64(o.get_instance_min_z(0))
            .f64(o.get_instance_max_z(0));
        p.field("the instance's convex_hull_2d")
            .points(&o.instance_convex_hull_2d(0).points);
        for row in inst {
            o.add_instance_with(
                &Vec3d::new(row[0], row[1], row[2]),
                &Vec3d::new(row[3], row[4], row[5]),
                &Vec3d::new(row[6], row[7], row[8]),
                &Vec3d::new(row[9], row[10], row[11]),
            );
        }
        for i in 0..o.instances.len() {
            p.field(&format!("instance {i}: the matrix"))
                .mat(o.instances[i].get_matrix());
            p.field(&format!("instance {i}: instance_bounding_box"))
                .bb3(&o.instance_bounding_box(i, false));
            p.field(&format!("instance {i}: min and max z"))
                .f64(o.get_instance_min_z(i))
                .f64(o.get_instance_max_z(i));
            p.field(&format!("instance {i}: convex_hull_2d"))
                .points(&o.instance_convex_hull_2d(i).points);
        }
        p.field("bounding_box_exact of three instances")
            .bb3(&o.bounding_box_exact());
        o.ensure_on_bed(allow_negative_z);
        p.field("the offsets after ensure_on_bed");
        for i in &o.instances {
            p.vec3(&i.get_offset());
        }
        p.field("min_z after ensure_on_bed").f64(o.min_z());
    }
    for (b, bed) in beds.iter().enumerate() {
        let bv = bed.build();
        let n = model.update_print_volume_state(&bv);
        p.field(&format!("bed {b}: update_print_volume_state"))
            .u32(n);
        for i in &model.objects[0].instances {
            p.u8(i.print_volume_state.value());
        }
    }
    let config = settings(pairs);
    let o = &mut model.objects[0];
    let n = o.instances.len();
    for (i, &(bed, x, y, rot, item)) in placed.iter().enumerate().take(n) {
        let mut ap = get_instance_arrange_poly(o, i, &config);
        p.field(&format!("instance {i}: get_instance_arrange_poly"));
        put_ap(p, &ap);
        ap.bed_idx = bed;
        ap.translation = Point::new(x, y);
        ap.rotation = rot;
        ap.itemid = item;
        apply_arrange_polygon(o, i, &mut ap);
        apply_arrange_polygon(o, i, &mut ap);
        p.field(&format!("instance {i}: the matrix after apply()"))
            .mat(o.instances[i].get_matrix())
            .i32(o.instances[i].arrange_order)
            .i32(ap.is_applied);
    }
    p.field("bounding_box_exact after arranging")
        .bb3(&o.bounding_box_exact());
    o.center_around_origin(false);
    p.field("center_around_origin").vec3(&o.origin_translation);
    for v in &o.volumes {
        p.vec3(&v.get_offset());
    }
    p.field("bounding_box_exact after center_around_origin")
        .bb3(&o.bounding_box_exact());
}

#[test]
fn model_files_load_and_place_as_the_engine_does() {
    quiet();
    let file = model_file();
    let dir = TempDir::new("mlod");
    let beds = mbvl_beds(&file);
    let mut m = Mismatches::default();
    oracle_arena().execute(|| {
        for rec in file.of_kind("MLOD") {
            let (label, p) = rebuild_lod(rec, &dir, &beds);
            let diff = p.first_difference(&rec.payload);
            m.check(diff.is_none(), || {
                format!("MLOD {label}: {}", diff.unwrap_or_default())
            });
        }
    });
    m.assert_none("MLOD");
}

// ---- M3MF ----------------------------------------------------------------------------------

/// `s` with the temporary folder written `<dir>`, in either separator form (the probe's
/// `portable`).
fn portable(s: &str, dir: &TempDir) -> String {
    let native = dir.0.to_str().expect("a UTF-8 path");
    let generic = native.replace('\\', "/");
    s.replace(native, "<dir>").replace(&generic, "<dir>")
}

/// The model's information (the probe's `put_info`): the design information (when a Designer is
/// given), the profile information and the model information with its metadata items by name, as
/// the root model's metadata leave them (`Model`'s `design_info`, `profile_info` and `model_info`,
/// from piece 1 of the 3MF port).
fn put_info(p: &mut Payload, model: &Model) {
    p.field("the model's information: design")
        .boolean(model.design_info.is_some());
    if let Some(d) = &model.design_info {
        p.str(d.design_id.as_bytes())
            .str(d.designer.as_bytes())
            .str(d.designer_user_id.as_bytes());
    }
    p.field("the model's information: profile")
        .boolean(model.profile_info.is_some());
    if let Some(i) = &model.profile_info {
        p.str(i.profile_tile.as_bytes())
            .str(i.profile_cover.as_bytes())
            .str(i.profile_description.as_bytes())
            .str(i.profile_user_id.as_bytes())
            .str(i.profile_user_name.as_bytes());
    }
    p.field("the model's information: model")
        .boolean(model.model_info.is_some());
    if let Some(i) = &model.model_info {
        p.str(i.cover_file.as_bytes())
            .str(i.license.as_bytes())
            .str(i.description.as_bytes())
            .str(i.copyright.as_bytes())
            .str(i.model_name.as_bytes())
            .str(i.origin.as_bytes());
        p.field("the model's information: metadata items")
            .len(i.metadata_items.len());
        for (k, v) in &i.metadata_items {
            p.str(k.as_bytes()).str(v.as_bytes());
        }
    }
}

/// What the read leaves that the cuts drop or `put_model` leaves out (the probe's
/// `put_as_read`): the model's information; each object's module, whole configuration, layer
/// height profile, height ranges and brim ears; each instance's loaded id and assembly
/// transformation (when it has one); each part's whole configuration.
fn put_as_read(p: &mut Payload, model: &Model) {
    put_info(p, model);
    p.field("as read").len(model.objects.len());
    for (oi, o) in model.objects.iter().enumerate() {
        p.field(&format!("as read: object {oi}: module, settings"))
            .str(o.module_name.as_bytes())
            .str(&config_dump(&o.config));
        p.field(&format!("as read: object {oi}: the layer height profile"))
            .len(o.layer_height_profile.len());
        for &z in &o.layer_height_profile {
            p.f64(z);
        }
        p.field(&format!("as read: object {oi}: the height ranges"))
            .len(o.layer_config_ranges.len());
        for ((lo, hi), c) in &o.layer_config_ranges {
            p.f64(*lo).f64(*hi).str(&config_dump(c));
        }
        p.field(&format!("as read: object {oi}: the brim ears"))
            .len(o.brim_points.len());
        for b in &o.brim_points {
            p.f32(b.pos.x())
                .f32(b.pos.y())
                .f32(b.pos.z())
                .f32(b.head_front_radius);
        }
        p.len(o.instances.len());
        for (ii, i) in o.instances.iter().enumerate() {
            p.field(&format!("as read: object {oi} instance {ii}"))
                .u64(u64::try_from(i.loaded_id).expect("a loaded id in 64 bits"))
                .boolean(i.is_assemble_initialized());
            if i.is_assemble_initialized() {
                p.mat(i.get_assemble_transformation().get_matrix())
                    .vec3(&i.get_offset_to_assembly());
            }
        }
        p.len(o.volumes.len());
        for (vi, v) in o.volumes.iter().enumerate() {
            p.field(&format!("as read: object {oi} volume {vi}: the settings"))
                .str(&config_dump(&v.config));
        }
    }
}

/// An M3MF record's label and piece: the piece of the 3MF reader's port it needs last (1: the
/// container, its relationships and the root model; 2: the config entries; 3: the sub-model
/// files and the assembly).
fn m3mf_piece(rec: &Record) -> (String, u8) {
    let mut r = rec.reader();
    let label = r.str().unwrap();
    (label, r.u8().unwrap())
}

fn rebuild_3mf(rec: &Record, dir: &TempDir, all_beds: &[Bed]) -> (String, Payload) {
    let mut r = rec.reader();
    let label = r.str().unwrap();
    let piece = r.u8().unwrap();
    let name = r.str().unwrap();
    let bytes = bytes_str(&mut r);
    let steps = r.bool().unwrap();
    let inputs = steps.then(|| LodInputs::read(&mut r));

    let file = dir.0.join(&name);
    std::fs::write(&file, &bytes).expect("the 3MF file written");
    let path = file.to_str().expect("a UTF-8 path").to_owned();
    let (read, log) = logged(|| {
        read_from_file(
            &path,
            load_strategy::LOAD_MODEL | load_strategy::ADD_DEFAULT_INSTANCES,
        )
    });
    let _ = std::fs::remove_file(&file);

    let mut p = Payload::default();
    // A file that loads needs the whole reader: piece 3.
    p.field("the label and the piece")
        .str(label.as_bytes())
        .u8(if read.is_ok() { 3 } else { piece });
    p.field("the name and the bytes")
        .str(name.as_bytes())
        .str(&bytes);
    p.field("steps").boolean(steps);
    if let Some(i) = &inputs {
        i.put(&mut p);
    }
    p.field("the log").str(portable(&log, dir).as_bytes());
    let mut model = match read {
        Ok(m) => m,
        Err(e) => {
            p.field("the error")
                .u8(1)
                .str(portable(&e.what(), dir).as_bytes());
            return (label, p);
        }
    };
    p.field("read").u8(0);
    put_as_read(&mut p, &model);
    cli_cuts(&mut model);
    put_model(&mut p, &model);
    if let Some(i) = &inputs {
        lod_tail(&mut p, &mut model, i, all_beds, &label, true);
    }
    (label, p)
}

/// The M3MF tests one at a time: the 3MF reader keeps its backup folders under the process's one
/// temporary folder (`orca_model::utils::temporary_dir`), which each test points at its own.
static M3MF_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Replays the M3MF records of one piece; the probe's temporary folder for the backup folders is
/// the test's own (`model.cpp`: `set_temporary_dir(dir / "backup")`).
fn check_3mf_piece(piece: u8) {
    let _one_at_a_time = M3MF_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    quiet();
    let file = model_file();
    let dir = TempDir::new(&format!("m3mf-{piece}"));
    let backup = dir.0.join("backup");
    orca_model::utils::set_temporary_dir(backup.to_str().expect("a UTF-8 path"));
    let beds = mbvl_beds(&file);
    let mut m = Mismatches::default();
    let mut replayed = 0usize;
    oracle_arena().execute(|| {
        for rec in file
            .of_kind("M3MF")
            .filter(|rec| m3mf_piece(rec).1 == piece)
        {
            let (label, p) = rebuild_3mf(rec, &dir, &beds);
            let diff = p.first_difference(&rec.payload);
            m.check(diff.is_none(), || {
                format!("M3MF {label}: {}", diff.unwrap_or_default())
            });
            replayed += 1;
        }
    });
    eprintln!(
        "{}: {replayed} M3MF records of piece {piece} replayed",
        file.path.display()
    );
    assert!(replayed > 0, "no M3MF record of piece {piece}");
    m.assert_none("M3MF");
}

/// The file holds M3MF records of each piece, and only of the three.
#[test]
fn three_mf_records_are_present() {
    let file = model_file();
    let mut seen = [0usize; 3];
    for rec in file.of_kind("M3MF") {
        let (label, piece) = m3mf_piece(rec);
        assert!(
            (1..=3).contains(&piece),
            "{}: M3MF {label} names piece {piece}",
            file.path.display()
        );
        seen[usize::from(piece - 1)] += 1;
    }
    assert!(
        seen.iter().all(|&n| n > 0),
        "{}: M3MF records of pieces 1, 2 and 3: {seen:?}",
        file.path.display()
    );
}

#[test]
fn three_mf_files_the_container_or_the_root_model_refuses() {
    check_3mf_piece(1);
}

#[test]
fn three_mf_files_the_config_entries_refuse() {
    check_3mf_piece(2);
}

#[test]
fn three_mf_files_load_and_place_as_the_engine_does() {
    check_3mf_piece(3);
}

// ---- MARP ----------------------------------------------------------------------------------

fn rebuild_arp(rec: &Record) -> Payload {
    let mut r = rec.reader();
    let pairs = read_pairs(&mut r);
    let mut params = ArrangeParams {
        is_seq_print: r.bool().unwrap(),
        bed_shrink_x: r.f32().unwrap(),
        bed_shrink_y: r.f32().unwrap(),
        clearance_radius: r.f32().unwrap(),
        min_obj_distance: r.i64().unwrap(),
        nozzle_height: r.f32().unwrap(),
        object_skirt_offset: r.f32().unwrap(),
        ..ArrangeParams::default()
    };
    let mut p = Payload::default();
    p.field("the settings");
    put_pairs(&mut p, &pairs);
    p.field("the parameters")
        .boolean(params.is_seq_print)
        .f32(params.bed_shrink_x)
        .f32(params.bed_shrink_y)
        .f32(params.clearance_radius)
        .i64(params.min_obj_distance)
        .f32(params.nozzle_height)
        .f32(params.object_skirt_offset);
    let ns = r.count().unwrap();
    let mut selected = Vec::new();
    p.field("the selected items").len(ns);
    for _ in 0..ns {
        let mut ap = ArrangePolygon::default();
        ap.poly.contour = Polygon::new(points(&mut r));
        ap.height = r.f64().unwrap();
        ap.brim_width = r.f64().unwrap();
        ap.has_tree_support = r.bool().unwrap();
        p.points(&ap.poly.contour.points)
            .f64(ap.height)
            .f64(ap.brim_width)
            .boolean(ap.has_tree_support);
        selected.push(ap);
    }
    let nu = r.count().unwrap();
    let mut unselected = Vec::new();
    p.field("the other items").len(nu);
    for _ in 0..nu {
        let mut ap = ArrangePolygon::default();
        ap.poly.contour = Polygon::new(points(&mut r));
        ap.brim_width = r.f64().unwrap();
        ap.is_virt_object = r.bool().unwrap();
        ap.is_extrusion_cali_object = r.bool().unwrap();
        p.points(&ap.poly.contour.points)
            .f64(ap.brim_width)
            .boolean(ap.is_virt_object)
            .boolean(ap.is_extrusion_cali_object);
        unselected.push(ap);
    }
    let config = settings(&pairs);
    p.field("get_bed_shape")
        .points(&get_bed_shape(&config, true));
    p.field("get_bed_shape, not shared")
        .points(&get_bed_shape(&config, false));
    update_arrange_params(&mut params, &config);
    p.field("update_arrange_params")
        .f32(params.brim_skirt_distance)
        .f32(params.bed_shrink_x)
        .f32(params.bed_shrink_y);
    p.field("get_shrink_bedpts")
        .points(&get_shrink_bedpts(&config, &params));
    update_selected_items_inflation(&mut selected, &config, &mut params);
    p.field("update_selected_items_inflation")
        .i64(params.min_obj_distance);
    for ap in &selected {
        p.i64(ap.inflation);
    }
    update_unselected_items_inflation(&mut unselected, &params);
    p.field("update_unselected_items_inflation");
    for ap in &unselected {
        p.i64(ap.inflation).points(&ap.poly.contour.points);
    }
    p
}

#[test]
fn beds_and_inflation_are_the_engines() {
    check_kind("MARP", rebuild_arp);
}

// ---- MTMP ----------------------------------------------------------------------------------

fn rebuild_tmp(rec: &Record) -> Payload {
    let mut r = rec.reader();
    let mut p = Payload::default();
    let n = r.count().unwrap();
    p.field("get_filament_temp_type").len(n);
    for _ in 0..n {
        let name = r.str().unwrap();
        let _ = r.i32().unwrap();
        p.str(name.as_bytes()).i32(get_filament_temp_type(&name));
    }
    let n = r.count().unwrap();
    p.field("get_compatible_filament_type").len(n);
    for _ in 0..n {
        let k = r.count().unwrap();
        let set: Vec<i32> = (0..k).map(|_| r.i32().unwrap()).collect();
        let _ = r.i32().unwrap();
        p.len(set.len());
        for &v in &set {
            p.i32(v);
        }
        p.i32(get_compatible_filament_type(&set));
    }
    p
}

#[test]
fn filament_temperature_classes_are_the_engines() {
    check_kind("MTMP", rebuild_tmp);
}

/// Every record of the file is of a kind replayed here.
#[test]
fn every_record_is_replayed() {
    let file = model_file();
    for rec in &file.records {
        assert!(
            matches!(
                rec.kind(),
                "MXFM" | "MFAC" | "MCFG" | "MCIR" | "MBVL" | "MLOD" | "M3MF" | "MARP" | "MTMP"
            ),
            "{}: a {} record, which nothing replays",
            file.path.display(),
            rec.kind()
        );
    }
}
