// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The records of `slice.sop1` of the piece front-2 (`parity/sop1/slice.md`), which
//! `parity/oracle/probe/slice/front.cpp` writes: SLFE (`its_face_edge_ids` in its three forms,
//! orca-mesh's), SLMC (the comparisons PrintApply makes, after Model's copy and a change made
//! through its API, with the painted queries and `raw_mesh`), SLTS (`FacetsAnnotation`'s
//! `has_facets`, `get_facets_strict` and `equals`), SLSH (the shrinkage compensation) and SLBX
//! (the bed's excluded area). The meshes come from the file's SLMH table. Written by
//! orca-o3-front-2, 2026-11-13.
//!
//! Each record is rebuilt from its inputs and what orca-model computes, and compared whole. Two
//! fields are taken from the record instead: SLTS's `get_facets_strict` of an annotation that
//! paints, where orca-model stops with `NotPorted` naming `painted-facets` (the test checks that
//! it does, and counts them). SLMC is checked twice: the results from the two models as the record
//! describes them, and the description of the second model made again through orca-model's own
//! copy and change (the change's object, part or second object drawn by the probe's generator are
//! not recorded, so every candidate is tried and one must give the recorded bytes).

use std::collections::BTreeMap;
use std::sync::Arc;

use orca_config::print_config::PrintConfig;
use orca_eigen::{Mat4, Transform3d, Vec3d, Vec3f, Vec3i32};
use orca_geom::polygon::Polygon;
use orca_mesh::admesh::IndexedTriangleSet;
use orca_mesh::face_edge_ids::{
    its_face_edge_ids, its_face_edge_ids_from_neighbors, its_face_edge_ids_masked,
};
use orca_mesh::its::its_face_neighbors;
use orca_mesh::triangle_mesh::TriangleMesh;
use orca_model::arrange::{get_bed_excluded_area, get_bed_shape_with_excluded_area};
use orca_model::compare::{
    model_brim_points_data_changed, model_custom_seam_data_changed,
    model_custom_supports_data_changed, model_fuzzy_skin_data_changed, model_has_advanced_features,
    model_has_multi_part_objects, model_mmu_segmentation_data_changed, model_object_list_equal,
    model_object_list_extended, model_volume_list_changed, model_volume_list_changed_of,
};
use orca_model::config::ModelConfig;
use orca_model::facets::FacetsAnnotation;
use orca_model::model::{BrimPoint, Model, ModelObject, ModelVolume, ModelVolumeType};
use orca_model::object_id::{ObjectID, Timestamp};
use orca_model::stop::PAINTED_FACETS;
use orca_model::transformation::Transformation;
use orca_probe::{Mismatches, ProbeFile, Reader, Record, load_probe};

use super::{Payload, oracle_arena, quiet};

fn slice_file() -> ProbeFile {
    load_probe("slice").unwrap_or_else(|e| panic!("{e}"))
}

/// The kinds of front-2.
const FRONT_KINDS: [&str; 5] = ["SLFE", "SLMC", "SLTS", "SLSH", "SLBX"];

// ---- the values ----------------------------------------------------------------------------

fn read_its(r: &mut Reader<'_>) -> IndexedTriangleSet {
    let nv = r.count().unwrap();
    let vertices = (0..nv)
        .map(|_| Vec3f::new(r.f32().unwrap(), r.f32().unwrap(), r.f32().unwrap()))
        .collect();
    let nf = r.count().unwrap();
    let indices = (0..nf)
        .map(|_| Vec3i32::new(r.i32().unwrap(), r.i32().unwrap(), r.i32().unwrap()))
        .collect();
    IndexedTriangleSet {
        indices,
        vertices,
        properties: Vec::new(),
    }
}

fn put_its(p: &mut Payload, its: &IndexedTriangleSet) {
    p.len(its.vertices.len());
    for v in &its.vertices {
        p.f32(v.x()).f32(v.y()).f32(v.z());
    }
    p.len(its.indices.len());
    for f in &its.indices {
        p.i32(f[0]).i32(f[1]).i32(f[2]);
    }
}

fn read_edges(r: &mut Reader<'_>) -> Vec<Vec3i32> {
    let n = r.count().unwrap();
    (0..n)
        .map(|_| Vec3i32::new(r.i32().unwrap(), r.i32().unwrap(), r.i32().unwrap()))
        .collect()
}

fn put_edges(p: &mut Payload, e: &[Vec3i32]) {
    p.len(e.len());
    for v in e {
        p.i32(v[0]).i32(v[1]).i32(v[2]);
    }
}

/// A `matrix` of `slice.sop1`: column by column.
fn read_matrix(r: &mut Reader<'_>) -> Transform3d {
    let mut rows = [[0.0; 4]; 4];
    for c in 0..4 {
        for row in &mut rows {
            row[c] = r.f64().unwrap();
        }
    }
    Transform3d::from_matrix(Mat4::from_rows(rows))
}

fn put_matrix(p: &mut Payload, m: &Transform3d) {
    for c in 0..4 {
        for row in 0..4 {
            p.f64(m.m.at(row, c));
        }
    }
}

/// A transformation (`Xform`): offset, rotation, scale, mirror, set in that order on a default
/// one (`Xform::make`).
struct Xform {
    v: [Vec3d; 4],
}

impl Xform {
    fn read(r: &mut Reader<'_>) -> Xform {
        let mut v = [Vec3d::zero(); 4];
        for x in &mut v {
            *x = Vec3d::new(r.f64().unwrap(), r.f64().unwrap(), r.f64().unwrap());
        }
        Xform { v }
    }

    fn identity() -> Xform {
        let one = Vec3d::new(1.0, 1.0, 1.0);
        Xform {
            v: [Vec3d::zero(), Vec3d::zero(), one, one],
        }
    }

    fn put(&self, p: &mut Payload) {
        for x in &self.v {
            p.vec3(x);
        }
    }

    fn make(&self) -> Transformation {
        let mut t = Transformation::default();
        t.set_offset(&self.v[0]);
        t.set_rotation(&self.v[1]);
        t.set_scaling_factor(&self.v[2]);
        t.set_mirror(&self.v[3]);
        t
    }
}

fn read_pairs(r: &mut Reader<'_>) -> Vec<(String, String)> {
    let n = r.count().unwrap();
    (0..n)
        .map(|_| (r.str().unwrap(), r.str().unwrap()))
        .collect()
}

fn set_config(c: &mut ModelConfig, pairs: &[(String, String)]) {
    for (k, v) in pairs {
        c.set_deserialize(k, v)
            .unwrap_or_else(|e| panic!("{k}={v}: {}", e.what()));
    }
}

fn put_config(p: &mut Payload, c: &ModelConfig) {
    let entries: Vec<(&String, String)> = c
        .iter()
        .map(|(k, v)| (k, v.serialize().unwrap_or_else(|e| panic!("{k}: {e:?}"))))
        .collect();
    p.len(entries.len());
    for (k, v) in entries {
        p.str(k.as_bytes()).str(v.as_bytes());
    }
}

fn volume_type(v: i32) -> ModelVolumeType {
    match v {
        0 => ModelVolumeType::ModelPart,
        1 => ModelVolumeType::NegativeVolume,
        2 => ModelVolumeType::ParameterModifier,
        3 => ModelVolumeType::SupportBlocker,
        4 => ModelVolumeType::SupportEnforcer,
        _ => ModelVolumeType::Invalid,
    }
}

// ---- the mesh table (SLMH) -----------------------------------------------------------------

/// The mesh table by id: the mesh and whether it is symmetric.
fn mesh_table(file: &ProbeFile) -> BTreeMap<u32, (IndexedTriangleSet, bool)> {
    let mut out = BTreeMap::new();
    for rec in file.of_kind("SLMH") {
        let mut r = rec.reader();
        let id = r.u32().unwrap();
        let _label = r.str().unwrap();
        let symmetric = r.bool().unwrap();
        let its = read_its(&mut r);
        r.finish("SLMH").unwrap();
        out.insert(id, (its, symmetric));
    }
    out
}

type Meshes = BTreeMap<u32, (IndexedTriangleSet, bool)>;

fn mesh_of(meshes: &Meshes, id: u32) -> &IndexedTriangleSet {
    &meshes
        .get(&id)
        .unwrap_or_else(|| panic!("mesh {id} is not in SLMH"))
        .0
}

/// Compares every record of `kind`, each rebuilt by `rebuild`.
fn check_slice_kind(file: &ProbeFile, kind: &str, rebuild: &(dyn Fn(&Record) -> Payload + Sync)) {
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

// ---- SLFE ----------------------------------------------------------------------------------

fn rebuild_fe(rec: &Record, meshes: &Meshes) -> Payload {
    let mut r = rec.reader();
    let mut p = Payload::default();
    let id = r.u32().unwrap();
    let its = mesh_of(meshes, id);
    p.field("mesh").u32(id);
    read_edges(&mut r);
    p.field("its_face_edge_ids(its)");
    put_edges(&mut p, &its_face_edge_ids(its));
    let n = r.count().unwrap();
    let mask: Vec<bool> = (0..n).map(|_| r.bool().unwrap()).collect();
    p.field("mask").len(mask.len());
    for &b in &mask {
        p.boolean(b);
    }
    read_edges(&mut r);
    p.field("its_face_edge_ids(its, mask)");
    put_edges(&mut p, &its_face_edge_ids_masked(its, &mask));
    let symmetric = r.bool().unwrap();
    p.field("symmetric").boolean(symmetric);
    if symmetric {
        read_edges(&mut r);
        let nb = its_face_neighbors(its);
        p.field("its_face_neighbors");
        put_edges(&mut p, &nb);
        for unbound in [false, true] {
            read_edges(&mut r);
            r.i32().unwrap();
            let mut num = -7;
            let e = its_face_edge_ids_from_neighbors(its, &nb, unbound, Some(&mut num));
            p.field(if unbound {
                "its_face_edge_ids(neighbours, true)"
            } else {
                "its_face_edge_ids(neighbours, false)"
            });
            put_edges(&mut p, &e);
            p.field("num_edges").i32(num);
        }
    }
    r.finish("SLFE").unwrap();
    p
}

#[test]
fn face_edge_ids_are_the_engines() {
    quiet();
    let file = slice_file();
    let meshes = mesh_table(&file);
    check_slice_kind(&file, "SLFE", &|rec| rebuild_fe(rec, &meshes));
}

// ---- SLMC ----------------------------------------------------------------------------------

/// One part as the record describes it.
struct VolDesc {
    id: u32,
    ty: i32,
    mesh: u32,
    matrix: Transform3d,
    stamps: [(u32, bool); 4],
    config: Vec<(String, String)>,
}

/// One object as the record describes it.
struct ObjDesc {
    id: u32,
    vols: Vec<VolDesc>,
    instances: usize,
    config: Vec<(String, String)>,
    brim: Vec<[f32; 4]>,
}

fn read_model(r: &mut Reader<'_>) -> Vec<ObjDesc> {
    let n = r.count().unwrap();
    (0..n)
        .map(|_| {
            let id = r.u32().unwrap();
            let nv = r.count().unwrap();
            let vols = (0..nv)
                .map(|_| {
                    let id = r.u32().unwrap();
                    let ty = r.i32().unwrap();
                    let mesh = r.u32().unwrap();
                    let matrix = read_matrix(r);
                    let mut stamps = [(0, false); 4];
                    for s in &mut stamps {
                        *s = (r.u32().unwrap(), r.bool().unwrap());
                    }
                    let config = read_pairs(r);
                    VolDesc {
                        id,
                        ty,
                        mesh,
                        matrix,
                        stamps,
                        config,
                    }
                })
                .collect();
            let instances = r.count().unwrap();
            let config = read_pairs(r);
            let nb = r.count().unwrap();
            let brim = (0..nb)
                .map(|_| {
                    [
                        r.f32().unwrap(),
                        r.f32().unwrap(),
                        r.f32().unwrap(),
                        r.f32().unwrap(),
                    ]
                })
                .collect();
            ObjDesc {
                id,
                vols,
                instances,
                config,
                brim,
            }
        })
        .collect()
}

/// The ids and timestamps the replay loads from tokens, far above what the process's counters
/// reach, so that an id or timestamp the replay's own copies and changes make never meets one.
const ID_BASE: usize = 1 << 40;
const STAMP_BASE: Timestamp = 1 << 50;

/// The timestamp a token stands for. Token 0 is the first timestamp model a writes, its first
/// part's support painting, which nothing has touched (the probe paints with
/// `set_triangle_from_string`, which does not touch): the timestamp 1 every annotation is made
/// with, which a part the change adds has too. The others stand far above the process's counter.
fn stamp_of(token: u32) -> Timestamp {
    if token == 0 {
        1
    } else {
        STAMP_BASE + Timestamp::from(token)
    }
}

/// The volumes' meshes by address, as the probe's `MeshIds`.
type MeshIds = BTreeMap<usize, u32>;

fn mesh_key(v: &ModelVolume) -> usize {
    Arc::as_ptr(&v.mesh_ptr()) as usize
}

fn annotation_mut(v: &mut ModelVolume, k: usize) -> &mut FacetsAnnotation {
    match k {
        0 => &mut v.supported_facets,
        1 => &mut v.seam_facets,
        2 => &mut v.mmu_segmentation_facets,
        _ => &mut v.fuzzy_skin_facets,
    }
}

fn annotation(v: &ModelVolume, k: usize) -> &FacetsAnnotation {
    match k {
        0 => &v.supported_facets,
        1 => &v.seam_facets,
        2 => &v.mmu_segmentation_facets,
        _ => &v.fuzzy_skin_facets,
    }
}

/// A model built from its description: ids and timestamps loaded from the tokens, a painted
/// annotation given `set_triangle_from_string(0, "4")` as the probe paints one.
fn build_model(objs: &[ObjDesc], meshes: &Meshes, ids: &mut MeshIds) -> Model {
    let mut model = Model::default();
    for od in objs {
        let oi = model.add_empty_object();
        let o = &mut model.objects[oi];
        o.load_id(ObjectID::new(ID_BASE + od.id as usize));
        for vd in &od.vols {
            let vi = o.add_volume(
                TriangleMesh::from_its(mesh_of(meshes, vd.mesh).clone()),
                volume_type(vd.ty),
                false,
            );
            let v = &mut o.volumes[vi];
            ids.insert(mesh_key(v), vd.mesh);
            v.load_id(ObjectID::new(ID_BASE + vd.id as usize));
            v.set_transformation(Transformation::new(&vd.matrix));
            for (k, &(stamp, empty)) in vd.stamps.iter().enumerate() {
                let a = annotation_mut(v, k);
                if !empty {
                    a.set_triangle_from_string(0, b"4");
                }
                a.load_timestamp(stamp_of(stamp));
            }
            set_config(&mut v.config, &vd.config);
        }
        for _ in 0..od.instances {
            o.add_instance();
        }
        set_config(&mut o.config, &od.config);
        o.brim_points = od
            .brim
            .iter()
            .map(|b| BrimPoint {
                pos: Vec3f::new(b[0], b[1], b[2]),
                head_front_radius: b[3],
            })
            .collect();
    }
    model
}

/// The probe's `Tokens`: ids and timestamps numbered in the order they are first written.
#[derive(Default)]
struct Tokens {
    ids: BTreeMap<ObjectID, u32>,
    stamps: BTreeMap<Timestamp, u32>,
}

impl Tokens {
    fn id(&mut self, i: ObjectID) -> u32 {
        let n = self.ids.len() as u32;
        *self.ids.entry(i).or_insert(n)
    }

    fn stamp(&mut self, t: Timestamp) -> u32 {
        let n = self.stamps.len() as u32;
        *self.stamps.entry(t).or_insert(n)
    }
}

/// The probe's `put_model`.
fn put_model(p: &mut Payload, model: &Model, t: &mut Tokens, ids: &MeshIds) {
    p.len(model.objects.len());
    for o in &model.objects {
        p.u32(t.id(o.id()));
        p.len(o.volumes.len());
        for v in &o.volumes {
            p.u32(t.id(v.id())).i32(v.volume_type().value());
            let mesh = ids
                .get(&mesh_key(v))
                .unwrap_or_else(|| panic!("a volume's mesh is not in the table"));
            p.u32(*mesh);
            put_matrix(p, v.get_matrix());
            for k in 0..4 {
                let a = annotation(v, k);
                p.u32(t.stamp(a.timestamp())).boolean(a.empty());
            }
            put_config(p, &v.config);
        }
        p.len(o.instances.len());
        put_config(p, &o.config);
        p.len(o.brim_points.len());
        for b in &o.brim_points {
            p.f32(b.pos.x())
                .f32(b.pos.y())
                .f32(b.pos.z())
                .f32(b.head_front_radius);
        }
    }
}

/// The results of an SLMC record from the two models.
fn put_results(p: &mut Payload, a: &Model, b: &Model) {
    p.field("model_object_list_equal")
        .boolean(model_object_list_equal(a, b));
    p.field("model_object_list_extended")
        .boolean(model_object_list_extended(a, b));
    let n = a.objects.len().min(b.objects.len());
    p.len(n);
    for k in 0..n {
        let (oa, ob) = (&a.objects[k], &b.objects[k]);
        p.field(&format!("object {k}'s volume lists"));
        let parts = model_volume_list_changed(oa, ob, ModelVolumeType::ModelPart);
        p.boolean(parts);
        p.boolean(model_volume_list_changed_of(
            oa,
            ob,
            &[
                ModelVolumeType::ModelPart,
                ModelVolumeType::NegativeVolume,
                ModelVolumeType::ParameterModifier,
            ],
        ));
        p.boolean(model_volume_list_changed(
            oa,
            ob,
            ModelVolumeType::SupportBlocker,
        ));
        p.boolean(model_volume_list_changed(
            oa,
            ob,
            ModelVolumeType::SupportEnforcer,
        ));
        if !parts {
            p.field(&format!("object {k}'s painting"));
            p.boolean(model_custom_supports_data_changed(oa, ob))
                .boolean(model_custom_seam_data_changed(oa, ob));
            p.boolean(model_mmu_segmentation_data_changed(oa, ob))
                .boolean(model_fuzzy_skin_data_changed(oa, ob));
        }
        p.field(&format!("object {k}'s brim points"))
            .boolean(model_brim_points_data_changed(oa, ob));
    }
    p.field("model_has_multi_part_objects")
        .boolean(model_has_multi_part_objects(a))
        .boolean(model_has_multi_part_objects(b));
    p.field("model_has_advanced_features")
        .boolean(model_has_advanced_features(a))
        .boolean(model_has_advanced_features(b));
    p.field("b's painted queries")
        .boolean(b.is_fdm_support_painted())
        .boolean(b.is_seam_painted())
        .boolean(b.is_mm_painted())
        .boolean(b.is_fuzzy_skin_painted());
    p.len(b.objects.len());
    for (k, ob) in b.objects.iter().enumerate() {
        p.field(&format!("b's object {k}'s painted queries"))
            .boolean(ob.is_fdm_support_painted())
            .boolean(ob.is_seam_painted())
            .boolean(ob.is_mm_painted())
            .boolean(ob.is_fuzzy_skin_painted());
        p.field(&format!("b's object {k}'s raw_mesh"));
        put_its(p, &ob.raw_mesh().its);
    }
}

/// An SLMC record read: the change, the two descriptions, where the descriptions end.
struct Comparison {
    change: u8,
    a: Vec<ObjDesc>,
    b: Vec<ObjDesc>,
    desc_end: usize,
}

fn read_comparison(rec: &Record) -> Comparison {
    let mut r = rec.reader();
    let change = r.u8().unwrap();
    let a = read_model(&mut r);
    let b = read_model(&mut r);
    let desc_end = rec.payload.len() - r.remaining();
    Comparison {
        change,
        a,
        b,
        desc_end,
    }
}

fn rebuild_mc(rec: &Record, meshes: &Meshes) -> Payload {
    let c = read_comparison(rec);
    let mut ids = MeshIds::new();
    let a = build_model(&c.a, meshes, &mut ids);
    let b = build_model(&c.b, meshes, &mut ids);
    let mut p = Payload::default();
    let mut t = Tokens::default();
    p.field("change").u8(c.change);
    p.field("model a");
    put_model(&mut p, &a, &mut t, &ids);
    p.field("model b");
    put_model(&mut p, &b, &mut t, &ids);
    put_results(&mut p, &a, &b);
    p
}

#[test]
fn model_comparisons_are_the_engines() {
    quiet();
    let file = slice_file();
    let meshes = mesh_table(&file);
    check_slice_kind(&file, "SLMC", &|rec| rebuild_mc(rec, &meshes));
}

/// The descriptions of model a and of the model the change `change` makes of its copy, on the
/// object `oi`, the part `vi` and the second object `oj`, through orca-model's API as the probe
/// makes it through the C++'s (`record_comparisons`). `None` when the candidate does not apply.
fn changed_copy(
    c: &Comparison,
    meshes: &Meshes,
    oi: usize,
    vi: usize,
    oj: usize,
) -> Option<Vec<u8>> {
    let mut ids = MeshIds::new();
    let a = build_model(&c.a, meshes, &mut ids);
    // Model b(a)
    let mut b = a.clone();
    if oi >= b.objects.len() {
        return None;
    }
    let nv = b.objects[oi].volumes.len();
    match c.change {
        0 => {}
        // AppendClone: b.add_object(*o)
        1 => {
            let o = b.objects[oi].clone();
            b.add_object_copy(&o);
        }
        // DeleteObject
        2 => {
            b.delete_object(oi);
        }
        // ReplaceWithClone
        3 => {
            let o = b.objects[oi].clone();
            b.add_object_copy(&o);
            let last = b.objects.len() - 1;
            b.objects.swap(oi, last);
            b.delete_object(b.objects.len() - 1);
        }
        // SwapObjects
        4 => {
            if oj >= b.objects.len() {
                return None;
            }
            b.objects.swap(oi, oj);
        }
        // AddVolume: the mesh and the type the record's model b shows last in this object
        5 => {
            let vd = c.b.get(oi)?.vols.last()?;
            let o = &mut b.objects[oi];
            let v = o.add_volume(
                TriangleMesh::from_its(mesh_of(meshes, vd.mesh).clone()),
                volume_type(vd.ty),
                false,
            );
            ids.insert(mesh_key(&o.volumes[v]), vd.mesh);
            o.volumes[v].set_transformation(Xform::identity().make());
        }
        // ClearVolumes
        6 => b.objects[oi].clear_volumes(),
        // ChangeType
        7 => {
            if vi >= nv {
                return None;
            }
            let v = &mut b.objects[oi].volumes[vi];
            let t = if v.volume_type() == ModelVolumeType::ModelPart {
                ModelVolumeType::NegativeVolume
            } else {
                ModelVolumeType::ModelPart
            };
            v.set_type(t);
        }
        // MoveVolume, NudgeVolume
        8 | 9 => {
            if vi >= nv {
                return None;
            }
            let d = if c.change == 8 { 1.0 } else { 1e-13 };
            let v = &mut b.objects[oi].volumes[vi];
            let off = v.get_offset() + Vec3d::new(d, 0.0, 0.0);
            v.set_offset(&off);
        }
        // PaintNoTouch
        10 => {
            if vi >= nv {
                return None;
            }
            b.objects[oi].volumes[vi]
                .supported_facets
                .set_triangle_from_string(1, b"8");
        }
        // ResetSupports, ResetSeam, ResetMmu, ResetFuzzy
        11..=14 => {
            if vi >= nv {
                return None;
            }
            annotation_mut(&mut b.objects[oi].volumes[vi], usize::from(c.change - 11)).reset();
        }
        // AddBrimPoint
        15 => b.objects[oi].brim_points.push(BrimPoint {
            pos: Vec3f::new(1.0, 2.0, 0.0),
            head_front_radius: 1.5,
        }),
        // AddInstance (its transformation is not described)
        16 => {
            b.objects[oi].add_instance();
        }
        // ClearInstances
        17 => b.objects[oi].clear_instances(),
        // AddConfig
        18 => b.objects[oi]
            .config
            .set_deserialize("sparse_infill_density", "35%")
            .unwrap(),
        // MovedCopy: Model c(a); b = Model(std::move(c));
        19 => {
            let mut copy = a.clone();
            let mut moved = Model::default();
            moved.assign_move(&mut copy);
            b.assign_move(&mut moved);
        }
        // ClearObjects
        20 => b.clear_objects(),
        // ClearMaterials
        21 => b.clear_materials(),
        _ => return None,
    }
    let mut p = Payload::default();
    let mut t = Tokens::default();
    p.u8(c.change);
    put_model(&mut p, &a, &mut t, &ids);
    put_model(&mut p, &b, &mut t, &ids);
    Some(p.bytes)
}

#[test]
fn model_copies_and_changes_are_the_engines() {
    quiet();
    let file = slice_file();
    let meshes = mesh_table(&file);
    let mut m = Mismatches::default();
    let mut by_change = [0u32; 22];
    oracle_arena().execute(|| {
        for (i, rec) in file.of_kind("SLMC").enumerate() {
            let c = read_comparison(rec);
            let want = &rec.payload[..c.desc_end];
            let n = c.a.len();
            let mut found = false;
            // Only ChangeType to ResetFuzzy read the part, and only SwapObjects a second object.
            let uses_part = (7..=14).contains(&c.change);
            let second = if c.change == 4 { n } else { 1 };
            'search: for oi in 0..n {
                let nv = if uses_part { c.a[oi].vols.len() } else { 1 };
                for vi in 0..nv {
                    for oj in 0..second {
                        if changed_copy(&c, &meshes, oi, vi, oj).as_deref() == Some(want) {
                            found = true;
                            break 'search;
                        }
                    }
                }
            }
            if found && usize::from(c.change) < by_change.len() {
                by_change[usize::from(c.change)] += 1;
            }
            m.check(found, || {
                format!(
                    "SLMC #{i}: no object, part or second object makes change {} give the \
                     recorded model b",
                    c.change
                )
            });
        }
    });
    m.assert_none("SLMC (copies and changes)");
    eprintln!("SLMC copies and changes reproduced, by change: {by_change:?}");
}

// ---- SLTS ----------------------------------------------------------------------------------

fn rebuild_ts(rec: &Record, meshes: &Meshes, stopped: &std::sync::atomic::AtomicU64) -> Payload {
    let mut r = rec.reader();
    let mut p = Payload::default();
    let id = r.u32().unwrap();
    let its = mesh_of(meshes, id).clone();
    p.field("mesh").u32(id);
    let n = r.count().unwrap();
    let tris: Vec<(i32, Vec<u8>)> = (0..n)
        .map(|_| (r.i32().unwrap(), r.bytes().unwrap().to_vec()))
        .collect();
    p.field("strings").len(tris.len());
    for (t, s) in &tris {
        p.i32(*t).str(s);
    }

    let mut object = ModelObject::default();
    let vi = object.add_volume(
        TriangleMesh::from_its(its.clone()),
        ModelVolumeType::ModelPart,
        false,
    );
    for (t, s) in &tris {
        object.volumes[vi]
            .supported_facets
            .set_triangle_from_string(*t, s);
    }
    let vol = &object.volumes[vi];
    let f = &vol.supported_facets;

    r.bool().unwrap();
    p.field("empty").boolean(f.empty());
    p.field("has_facets");
    // EnforcerBlockerType from NONE to ExtruderMax
    for state in 0..=16_i8 {
        r.bool().unwrap();
        p.boolean(f.has_facets(vol, state));
    }
    for state in 1..=3_i8 {
        let recorded = read_its(&mut r);
        p.field(&format!("get_facets_strict({state})"));
        match f.get_facets_strict(vol, state) {
            Ok(out) => put_its(&mut p, &out),
            Err(e) => {
                assert_eq!(
                    (e.family, f.empty()),
                    (PAINTED_FACETS, false),
                    "only painting stops"
                );
                stopped.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                put_its(&mut p, &recorded);
            }
        }
    }

    let mut other = ModelVolume::new(TriangleMesh::from_its(its), ModelVolumeType::ModelPart);
    for (t, s) in &tris {
        other.seam_facets.set_triangle_from_string(*t, s);
    }
    r.bool().unwrap();
    p.field("equals(seam)")
        .boolean(f.equals(&other.seam_facets));
    if let Some((last_t, last_s)) = tris.last() {
        let mut last = last_s.clone();
        let end = last.len() - 1;
        last[end] = if last[end] == b'4' { b'8' } else { b'4' };
        r.bytes().unwrap();
        p.field("last").str(&last);
        other.mmu_segmentation_facets.reset();
        for (t, s) in &tris[..tris.len() - 1] {
            other
                .mmu_segmentation_facets
                .set_triangle_from_string(*t, s);
        }
        other
            .mmu_segmentation_facets
            .set_triangle_from_string(*last_t, &last);
        r.bool().unwrap();
        p.field("equals(colour)")
            .boolean(f.equals(&other.mmu_segmentation_facets));
    }
    r.finish("SLTS").unwrap();
    p
}

#[test]
fn facets_annotations_answer_as_the_engines() {
    quiet();
    let file = slice_file();
    let meshes = mesh_table(&file);
    let stopped = std::sync::atomic::AtomicU64::new(0);
    check_slice_kind(&file, "SLTS", &|rec| rebuild_ts(rec, &meshes, &stopped));
    eprintln!(
        "SLTS: {} get_facets_strict fields of painted annotations stopped at painted-facets",
        stopped.load(std::sync::atomic::Ordering::Relaxed)
    );
}

// ---- SLSH ----------------------------------------------------------------------------------

fn rebuild_sh(rec: &Record) -> Payload {
    let mut r = rec.reader();
    let mut p = Payload::default();
    let x = Xform::read(&mut r);
    p.field("transformation");
    x.put(&mut p);
    let s = Vec3d::new(r.f64().unwrap(), r.f64().unwrap(), r.f64().unwrap());
    p.field("compensation").vec3(&s);
    read_matrix(&mut r);
    p.field("matrix");
    put_matrix(
        &mut p,
        &x.make().get_matrix_with_applied_shrinkage_compensation(&s),
    );
    r.finish("SLSH").unwrap();
    p
}

#[test]
fn shrinkage_compensation_is_the_engines() {
    quiet();
    let file = slice_file();
    check_slice_kind(&file, "SLSH", &rebuild_sh);
}

// ---- SLBX ----------------------------------------------------------------------------------

fn read_points(r: &mut Reader<'_>) -> Vec<[f64; 2]> {
    let n = r.count().unwrap();
    (0..n)
        .map(|_| [r.f64().unwrap(), r.f64().unwrap()])
        .collect()
}

fn put_points(p: &mut Payload, pts: &[[f64; 2]]) {
    p.len(pts.len());
    for v in pts {
        p.f64(v[0]).f64(v[1]);
    }
}

fn put_polygon(p: &mut Payload, poly: &Polygon) {
    p.len(poly.points.len());
    for pt in &poly.points {
        p.i64(pt.x).i64(pt.y);
    }
}

fn skip_polygon(r: &mut Reader<'_>) {
    let n = r.count().unwrap();
    for _ in 0..n {
        r.i64().unwrap();
        r.i64().unwrap();
    }
}

fn rebuild_bx(rec: &Record) -> Payload {
    let mut r = rec.reader();
    let mut p = Payload::default();
    let bed = read_points(&mut r);
    let excl = read_points(&mut r);
    let na = r.count().unwrap();
    let areas: Vec<Vec<[f64; 2]>> = (0..na).map(|_| read_points(&mut r)).collect();
    p.field("bed");
    put_points(&mut p, &bed);
    p.field("excluded");
    put_points(&mut p, &excl);
    p.field("extruder areas").len(areas.len());
    for a in &areas {
        put_points(&mut p, a);
    }
    let mut pc = PrintConfig::default();
    pc.printable_area.values = bed;
    pc.bed_exclude_area.values = excl;
    pc.extruder_printable_area.values = areas;

    let n = r.count().unwrap();
    for _ in 0..n {
        skip_polygon(&mut r);
    }
    let excluded = get_bed_excluded_area(&pc);
    p.field("get_bed_excluded_area").len(excluded.len());
    for poly in &excluded {
        put_polygon(&mut p, poly);
    }
    skip_polygon(&mut r);
    p.field("get_bed_shape_with_excluded_area(false)");
    put_polygon(&mut p, &get_bed_shape_with_excluded_area(&pc, false));
    skip_polygon(&mut r);
    p.field("get_bed_shape_with_excluded_area(true)");
    put_polygon(&mut p, &get_bed_shape_with_excluded_area(&pc, true));
    r.finish("SLBX").unwrap();
    p
}

#[test]
fn bed_excluded_areas_are_the_engines() {
    quiet();
    let file = slice_file();
    check_slice_kind(&file, "SLBX", &rebuild_bx);
}

/// Every kind of front-2 has records in the file.
#[test]
fn front_2_records_are_present() {
    let file = slice_file();
    for kind in FRONT_KINDS {
        let n = file.of_kind(kind).count();
        assert!(n > 0, "{}: no {kind} record", file.path.display());
        eprintln!("{kind}: {n} records ({})", file.path.display());
    }
    assert!(
        file.of_kind("SLMH").count() > 0,
        "{}: no SLMH record",
        file.path.display()
    );
}
