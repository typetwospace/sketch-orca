// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! slice-2's records of `slice.sop1` (`parity/sop1/slice.md`), replayed against the crate bit for
//! bit (added 2026-11-14 by orca-o3-slice-2), in the oracle's thread arena (`Arena::serial(8)`):
//!
//! - SLMS, SLM1, SLMX: `slice_mesh` at a list of heights and at one plane, and `slice_mesh_ex`, on
//!   a mesh of SLMH (flipped by `its_flip_triangles` when the record says), every point compared;
//! - SLSB: `slice_mesh_slabs` on the symmetric meshes, the tops, bottoms and vertical points the
//!   record's flags ask for;
//! - SLAD: `SlicingAdaptive` on the record's object: the faces `prepare` keeps, each recorded call
//!   of `next_layer_height` (from the facet index it was given) and `horizontal_facet_distance`;
//! - SLAP, SLSM, SLAJ, SLOL: `layer_height_profile_adaptive`, `smooth_height_profile`,
//!   `adjust_layer_height_profile` (on an object holding the record's ranges),
//!   `generate_object_layers` and `check_object_layers_fixed`.
//!
//! An object is built as the probe builds it: a `ModelObject` whose volumes are the table's meshes,
//! added uncentred, each given its recorded matrix (`Transformation::new`), and one instance with
//! its matrix; whether the instance is left-handed is checked against the record. Every `f32` and
//! `f64` is compared by its bits.

use std::collections::BTreeMap;

use orca_eigen::{Mat4, Transform3d, Vec3f, Vec3i32};
use orca_geom::expolygon::{ExPolygon, ExPolygons};
use orca_geom::point::Point;
use orca_geom::polygon::{Polygon, Polygons};
use orca_mesh::admesh::IndexedTriangleSet;
use orca_mesh::its::its_flip_triangles;
use orca_mesh::triangle_mesh::TriangleMesh;
use orca_model::model::{ModelObject, ModelVolumeType};
use orca_model::transformation::Transformation;
use orca_probe::{ProbeFile, ProbeSet};
use orca_slice::slice_mesh::{slice_mesh, slice_mesh_ex, slice_mesh_plane, slice_mesh_slabs};
use orca_slice::slicing::{HeightProfileSmoothingParams, LayerHeightEditActionType};
use orca_slice::slicing_adaptive::SlicingAdaptive;
use orca_slice::slicing_layers::{
    adjust_layer_height_profile, check_object_layers_fixed, generate_object_layers,
    layer_height_profile_adaptive, smooth_height_profile,
};
use orca_slice::triangle_mesh_slicer::{MeshSlicingParams, MeshSlicingParamsEx, SlicingMode};

use super::{Case, file, replay_in};

/// The kinds this file replays.
pub const SLICE_2_KINDS: [&str; 9] = [
    "SLMS", "SLM1", "SLMX", "SLSB", "SLAD", "SLAP", "SLSM", "SLAJ", "SLOL",
];

/// The mesh table (SLMH) by id: the mesh and whether it is symmetric.
type Meshes = BTreeMap<u32, (IndexedTriangleSet, bool)>;

fn mesh_table(file: &ProbeFile) -> Meshes {
    let mut out = BTreeMap::new();
    for rec in file.of_kind("SLMH") {
        let mut r = rec.reader();
        let id = r.u32().unwrap();
        let _label = r.str().unwrap();
        let symmetric = r.bool().unwrap();
        let nv = r.count().unwrap();
        let vertices = (0..nv)
            .map(|_| Vec3f::new(r.f32().unwrap(), r.f32().unwrap(), r.f32().unwrap()))
            .collect();
        let nf = r.count().unwrap();
        let indices = (0..nf)
            .map(|_| Vec3i32::new(r.i32().unwrap(), r.i32().unwrap(), r.i32().unwrap()))
            .collect();
        r.finish("SLMH").unwrap();
        out.insert(id, (IndexedTriangleSet::new(indices, vertices), symmetric));
    }
    out
}

fn mesh_of(meshes: &Meshes, id: u32) -> &IndexedTriangleSet {
    &meshes
        .get(&id)
        .unwrap_or_else(|| panic!("mesh {id} is not in SLMH"))
        .0
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

fn mode_of(v: u32) -> SlicingMode {
    SlicingMode::from_u32(v).unwrap_or_else(|| panic!("slicing mode {v}"))
}

/// The records of `kind` replayed in the oracle's arena, with the file's mesh table.
fn replay_meshes(
    kind: &str,
    full_count: impl Fn(&Meshes) -> usize,
    f: impl FnMut(&mut Case<'_, '_>, &Meshes) + Send,
) {
    let file = file();
    let meshes = mesh_table(&file);
    let full = if file.set == ProbeSet::Full {
        full_count(&meshes)
    } else {
        0
    };
    let mut f = f;
    orca_tbb::Arena::serial(8).execute(|| {
        replay_in(&file, kind, full, |c| f(c, &meshes));
    });
}

/// The polygons of a record, read whole.
fn read_polygon(c: &mut Case<'_, '_>) -> Polygon {
    let n = c.index();
    Polygon::new((0..n).map(|_| Point::new(c.i64(), c.i64())).collect())
}

fn read_polygons(c: &mut Case<'_, '_>) -> Polygons {
    let n = c.index();
    (0..n).map(|_| read_polygon(c)).collect()
}

fn read_expolygons(c: &mut Case<'_, '_>) -> ExPolygons {
    let n = c.index();
    (0..n)
        .map(|_| {
            let contour = read_polygon(c);
            let holes = read_polygons(c);
            ExPolygon { contour, holes }
        })
        .collect()
}

/// A count of layers, each read by `read`.
fn read_layers<T>(c: &mut Case<'_, '_>, read: fn(&mut Case<'_, '_>) -> T) -> Vec<T> {
    let n = c.index();
    (0..n).map(|_| read(c)).collect()
}

/// Where two lists first differ: the count, else the first index whose items differ, by
/// `describe`.
fn first_difference<T: PartialEq>(
    got: &[T],
    want: &[T],
    describe: impl Fn(&T, &T) -> String,
) -> Option<String> {
    if got.len() != want.len() {
        return Some(format!("{} instead of {}", got.len(), want.len()));
    }
    (0..got.len())
        .find(|&k| got[k] != want[k])
        .map(|k| format!("[{k}]: {}", describe(&got[k], &want[k])))
}

fn describe_polygon(got: &Polygon, want: &Polygon) -> String {
    match first_difference(&got.points, &want.points, |a, b| {
        format!("{a:?} instead of {b:?}")
    }) {
        Some(d) => format!("points {d}"),
        None => String::from("same"),
    }
}

fn describe_polygons(got: &Polygons, want: &Polygons) -> String {
    first_difference(got, want, describe_polygon).unwrap_or_default()
}

fn describe_expolygons(got: &ExPolygons, want: &ExPolygons) -> String {
    first_difference(got, want, |a, b| {
        if a.contour == b.contour {
            format!("holes {}", describe_polygons(&a.holes, &b.holes))
        } else {
            format!("contour {}", describe_polygon(&a.contour, &b.contour))
        }
    })
    .unwrap_or_default()
}

impl Case<'_, '_> {
    fn f32(&mut self) -> f32 {
        self.r.f32().unwrap()
    }
    fn i32(&mut self) -> i32 {
        self.r.i32().unwrap()
    }
    fn i64(&mut self) -> i64 {
        self.r.i64().unwrap()
    }
    fn u8(&mut self) -> u8 {
        self.r.u8().unwrap()
    }
    /// A `matrix`: 16 `f64`, column by column.
    fn matrix(&mut self) -> Transform3d {
        let mut rows = [[0.0; 4]; 4];
        for c in 0..4 {
            for row in &mut rows {
                row[c] = self.f64();
            }
        }
        Transform3d::from_matrix(Mat4::from_rows(rows))
    }
    /// The floats: a count of `f32`.
    fn floats(&mut self) -> Vec<f32> {
        let n = self.index();
        (0..n).map(|_| self.f32()).collect()
    }
    /// A slicing mode: mode, `slicing_mode_normal_below_layer`, `mode_below` and `trafo`.
    fn mode(&mut self) -> MeshSlicingParams {
        let mode = mode_of(self.u32());
        let slicing_mode_normal_below_layer = self.index();
        let mode_below = mode_of(self.u32());
        let trafo = self.matrix();
        MeshSlicingParams {
            mode,
            slicing_mode_normal_below_layer,
            mode_below,
            trafo,
        }
    }
    /// The object, built as the probe builds it; whether its instance is left-handed checked.
    fn object(&mut self, meshes: &Meshes) -> ModelObject {
        let mut o = ModelObject::default();
        let n = self.index();
        for _ in 0..n {
            let id = self.u32();
            let ty = volume_type(self.i32());
            let matrix = self.matrix();
            let vi = o.add_volume(
                TriangleMesh::from_its(mesh_of(meshes, id).clone()),
                ty,
                false,
            );
            o.volumes[vi].set_transformation(Transformation::new(&matrix));
        }
        let matrix = self.matrix();
        let ii = o.add_instance();
        o.instances[ii].set_transformation(Transformation::new(&matrix));
        let left_handed = o.instances[ii].is_left_handed();
        self.is_bool("instance left-handed", left_handed);
        o.layer_config_ranges = self.ranges();
        o
    }
    /// A mesh of the table by the record's id, flipped when the record's `bool` says.
    fn flipped_mesh(&mut self, meshes: &Meshes) -> IndexedTriangleSet {
        let id = self.u32();
        let flipped = self.bool();
        let mut its = mesh_of(meshes, id).clone();
        if flipped {
            its_flip_triangles(&mut its);
        }
        its
    }
    fn is_f32(&mut self, what: &str, got: f32) {
        let want = self.f32();
        self.same(
            what,
            &format!("{got:?} ({:#x})", got.to_bits()),
            &format!("{want:?} ({:#x})", want.to_bits()),
        );
    }
    fn is_index(&mut self, what: &str, got: usize) {
        let want = self.index();
        self.same(what, &got, &want);
    }
    fn is_listed<T: PartialEq>(
        &mut self,
        what: &str,
        got: &[T],
        want: &[T],
        describe: impl Fn(&T, &T) -> String,
    ) {
        let diff = first_difference(got, want, describe);
        let label = &self.label;
        self.m.check(diff.is_none(), || {
            format!("{label}: {what} {}", diff.unwrap_or_default())
        });
    }
}

#[test]
fn slms_slice_mesh_at_heights() {
    replay_meshes(
        "SLMS",
        |m| 7 * m.len(),
        |c, meshes| {
            let its = c.flipped_mesh(meshes);
            let params = c.mode();
            let zs = c.floats();
            let got = slice_mesh(&its, &zs, &params, &|| ());
            let want = read_layers(c, read_polygons);
            c.is_listed("layers", &got, &want, describe_polygons);
        },
    );
}

#[test]
fn slm1_slice_mesh_at_one_plane() {
    replay_meshes(
        "SLM1",
        |m| 3 * m.len(),
        |c, meshes| {
            let its = c.flipped_mesh(meshes);
            let params = c.mode();
            let z = c.f32();
            let got = slice_mesh_plane(&its, z, &params);
            let want = read_polygons(c);
            c.is_listed("polygons", &got, &want, describe_polygon);
        },
    );
}

#[test]
fn slmx_slice_mesh_ex() {
    replay_meshes(
        "SLMX",
        |m| 7 * m.len(),
        |c, meshes| {
            let its = c.flipped_mesh(meshes);
            let params = c.mode();
            let closing_radius = c.f32();
            let extra_offset = c.f32();
            let resolution = c.f64();
            let zs = c.floats();
            let params = MeshSlicingParamsEx {
                params,
                closing_radius,
                extra_offset,
                resolution,
            };
            let got = slice_mesh_ex(&its, &zs, &params, &|| ());
            let want = read_layers(c, read_expolygons);
            c.is_listed("layers", &got, &want, describe_expolygons);
        },
    );
}

#[test]
fn slsb_slice_mesh_slabs() {
    replay_meshes(
        "SLSB",
        |m| 2 * m.values().filter(|(_, symmetric)| *symmetric).count(),
        |c, meshes| {
            let id = c.u32();
            let trafo = c.matrix();
            let flags = c.u8();
            let zs = c.floats();
            let its = mesh_of(meshes, id);
            let (mut top, mut bottom, mut vertical) = (Vec::new(), Vec::new(), Vec::new());
            slice_mesh_slabs(
                its,
                &zs,
                &trafo,
                (flags & 1 != 0).then_some(&mut top),
                (flags & 2 != 0).then_some(&mut bottom),
                (flags & 4 != 0).then_some(&mut vertical),
                &|| (),
            );
            if flags & 1 != 0 {
                let want = read_layers(c, read_polygons);
                c.is_listed("tops", &top, &want, describe_polygons);
            }
            if flags & 2 != 0 {
                let want = read_layers(c, read_polygons);
                c.is_listed("bottoms", &bottom, &want, describe_polygons);
            }
            if flags & 4 != 0 {
                let n = c.index();
                let want: Vec<[u32; 6]> = (0..n)
                    .map(|_| std::array::from_fn(|_| c.f32().to_bits()))
                    .collect();
                let got: Vec<[u32; 6]> = vertical
                    .iter()
                    .map(|(p, v)| [p.x(), p.y(), p.z(), v.x(), v.y(), v.z()].map(f32::to_bits))
                    .collect();
                c.is_listed("vertical points", &got, &want, |a, b| {
                    format!(
                        "{:?} instead of {:?}",
                        a.map(f32::from_bits),
                        b.map(f32::from_bits)
                    )
                });
            }
        },
    );
}

#[test]
fn slad_slicing_adaptive() {
    replay_meshes(
        "SLAD",
        |_| 48,
        |c, meshes| {
            let object = c.object(meshes);
            let params = c.params();
            let mut adaptive = SlicingAdaptive::default();
            adaptive.set_slicing_parameters(params);
            adaptive.prepare(&object);
            let n = c.index();
            let want: Vec<[u32; 4]> = (0..n)
                .map(|_| std::array::from_fn(|_| c.f32().to_bits()))
                .collect();
            let got: Vec<[u32; 4]> = adaptive
                .faces()
                .iter()
                .map(|f| [f.z_span.0, f.z_span.1, f.n_cos, f.n_sin].map(f32::to_bits))
                .collect();
            c.is_listed("faces", &got, &want, |a, b| {
                format!(
                    "{:?} instead of {:?}",
                    a.map(f32::from_bits),
                    b.map(f32::from_bits)
                )
            });
            let quality = c.f32();
            let calls = c.index();
            for k in 0..calls {
                let print_z = c.f32();
                let mut facet = c.index();
                let height = adaptive.next_layer_height(print_z, quality, &mut facet);
                c.is_f32(&format!("next_layer_height #{k} ({print_z})"), height);
                c.is_index(&format!("next_layer_height #{k} facet"), facet);
            }
            let six = c.index();
            for _ in 0..six {
                let z = c.f32();
                c.is_f32(
                    &format!("horizontal_facet_distance({z})"),
                    adaptive.horizontal_facet_distance(z),
                );
            }
        },
    );
}

#[test]
fn slap_profile_adaptive() {
    replay_meshes(
        "SLAP",
        |_| 72,
        |c, meshes| {
            let object = c.object(meshes);
            let params = c.params();
            let quality = c.f32();
            let profile = layer_height_profile_adaptive(&params, &object, quality);
            c.is_profile("profile", &profile);
        },
    );
}

#[test]
fn slsm_smooth_profile() {
    replay_meshes(
        "SLSM",
        |_| 72,
        |c, _| {
            let params = c.params();
            let profile = c.profile();
            let radius = c.u32();
            let keep_min = c.bool();
            let smooth = smooth_height_profile(
                &profile,
                &params,
                &HeightProfileSmoothingParams::new(radius, keep_min),
            );
            c.is_profile("smoothed", &smooth);
        },
    );
}

#[test]
fn slaj_adjust_profile() {
    replay_meshes(
        "SLAJ",
        |_| 80,
        |c, _| {
            let params = c.params();
            let mut object = ModelObject::default();
            object.layer_config_ranges = c.ranges();
            let mut profile = c.profile();
            let z = c.f64();
            let delta = c.f64();
            let band = c.f64();
            let action = c.u32();
            let action = LayerHeightEditActionType::from_u32(action)
                .unwrap_or_else(|| panic!("action {action}"));
            adjust_layer_height_profile(&object, &params, &mut profile, z, delta, band, action);
            c.is_profile("adjusted", &profile);
        },
    );
}

#[test]
fn slol_object_layers() {
    replay_meshes(
        "SLOL",
        |_| 100,
        |c, _| {
            let params = c.params();
            let profile = c.profile();
            let precise = c.bool();
            let layers = generate_object_layers(&params, &profile, precise);
            c.is_profile("layers", &layers);
            c.is_bool(
                "check_object_layers_fixed",
                check_object_layers_fixed(&params, &profile),
            );
        },
    );
}
