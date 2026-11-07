// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: class TriangleMesh, TriangleMeshStats and RepairedMeshErrors of
// src/libslic3r/TriangleMesh.cpp and TriangleMesh.hpp of OrcaSlicer v2.4.2 (the constructors,
// trianglemesh_repair_on_import, from_stl, ReadSTLFile, the statistics, the transforms, the
// bounding boxes, split, merge and the convex hulls), translated on 2026-11-07. Copyright the
// OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and the PrusaSlicer authors (Prusa
// Research), under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! `TriangleMesh`: an indexed triangle set and its statistics.
//!
//! An STL file becomes a mesh by [`TriangleMesh::read_stl_file`]: admesh reads the facets
//! ([`crate::admesh::stlinit::stl_open`]), libslic3r repairs them on import
//! ([`trianglemesh_repair_on_import`]: the exact check, two nearby checks with a tolerance growing
//! from the shortest edge by a ten-thousandth of the bounding diameter, the removal of unconnected
//! facets, the normal directions and values, the volume, the neighbour check, and the exact check
//! again if any facet was degenerate), and the facets become shared vertices
//! ([`crate::admesh::shared::stl_generate_shared_vertices`]). The statistics are then taken from
//! the indexed set alone (`fill_initial_stats`): the facet count, [`its_volume`], the bounding
//! box, and the patches and open edges of the sequential face neighbours. admesh's own counters
//! (edges fixed, facets removed or reversed) do not reach them: `repaired_errors` stays zero, as
//! the C++ has that code under `#if 0`.
//!
//! `TriangleMeshStats::merge` keeps only the second mesh's repaired errors, as the C++ does: it
//! merges into a fresh `out`, not into the sum.

use std::path::Path;

use orca_eigen::rotation::AngleAxis;
use orca_eigen::{Matrix3d, Transform3d, Vec3d, Vec3f, Vec3i32};
use orca_geom::bounding_box::BoundingBoxf3;
use orca_geom::point::Point;
use orca_geom::polygon::Polygon;

use crate::admesh::connect::{
    stl_check_facets_exact, stl_check_facets_nearby, stl_remove_unconnected_facets,
};
use crate::admesh::normals::{stl_fix_normal_directions, stl_fix_normal_values};
use crate::admesh::shared::stl_generate_shared_vertices;
use crate::admesh::stlinit::{StlError, stl_open};
use crate::admesh::util::{
    its_rotate_x, its_rotate_y, its_rotate_z, stl_calculate_volume, stl_verify_neighbors,
};
use crate::admesh::{IndexedTriangleSet, StlFile, StlVertex, times_minus_one};
use crate::its::{
    bounding_box, its_face_neighbors, its_flip_triangles, its_is_splittable, its_merge,
    its_num_open_edges, its_number_of_patches_with, its_split, its_transform, its_transform_matrix,
    its_volume,
};
use crate::log::{Severity, log};

/// `RepairedMeshErrors`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RepairedMeshErrors {
    /// `edges_fixed`.
    pub edges_fixed: i32,
    /// `degenerate_facets`.
    pub degenerate_facets: i32,
    /// `facets_removed`.
    pub facets_removed: i32,
    /// `facets_reversed`.
    pub facets_reversed: i32,
    /// `backwards_edges`.
    pub backwards_edges: i32,
}

impl RepairedMeshErrors {
    /// `merge(rhs)`: the sums.
    pub fn merge(&mut self, rhs: &RepairedMeshErrors) {
        self.edges_fixed += rhs.edges_fixed;
        self.degenerate_facets += rhs.degenerate_facets;
        self.facets_removed += rhs.facets_removed;
        self.facets_reversed += rhs.facets_reversed;
        self.backwards_edges += rhs.backwards_edges;
    }

    /// `repaired()`.
    #[must_use]
    pub fn repaired(&self) -> bool {
        self.degenerate_facets > 0
            || self.edges_fixed > 0
            || self.facets_removed > 0
            || self.facets_reversed > 0
            || self.backwards_edges > 0
    }
}

/// `TriangleMeshStats`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TriangleMeshStats {
    /// `number_of_facets`.
    pub number_of_facets: u32,
    /// `max`.
    pub max: StlVertex,
    /// `min`.
    pub min: StlVertex,
    /// `size`.
    pub size: StlVertex,
    /// `volume`, -1 when not computed.
    pub volume: f32,
    /// `number_of_parts`.
    pub number_of_parts: i32,
    /// `open_edges`.
    pub open_edges: i32,
    /// `repaired_errors`.
    pub repaired_errors: RepairedMeshErrors,
}

impl Default for TriangleMeshStats {
    fn default() -> Self {
        TriangleMeshStats {
            number_of_facets: 0,
            max: Vec3f::zero(),
            min: Vec3f::zero(),
            size: Vec3f::zero(),
            volume: -1.0,
            number_of_parts: 0,
            open_edges: 0,
            repaired_errors: RepairedMeshErrors::default(),
        }
    }
}

impl TriangleMeshStats {
    /// `merge(rhs)` (see the module documentation).
    #[must_use]
    pub fn merge(&self, rhs: &TriangleMeshStats) -> TriangleMeshStats {
        if self.number_of_facets == 0 {
            *rhs
        } else if rhs.number_of_facets == 0 {
            *self
        } else {
            let mut out = TriangleMeshStats {
                number_of_facets: self.number_of_facets.wrapping_add(rhs.number_of_facets),
                min: self.min.cwise_min(&rhs.min),
                max: self.max.cwise_max(&rhs.max),
                ..TriangleMeshStats::default()
            };
            out.size = out.max - out.min;
            out.number_of_parts = self.number_of_parts + rhs.number_of_parts;
            out.open_edges = self.open_edges + rhs.open_edges;
            out.volume = self.volume + rhs.volume;
            out.repaired_errors.merge(&rhs.repaired_errors);
            out
        }
    }

    /// `manifold()`.
    #[must_use]
    pub fn manifold(&self) -> bool {
        self.open_edges == 0
    }

    /// `repaired()`.
    #[must_use]
    pub fn repaired(&self) -> bool {
        self.repaired_errors.repaired()
    }
}

/// `update_bounding_box(its, out)`: min, max and size from `bounding_box(its)`, cast to `float`.
fn update_bounding_box(its: &IndexedTriangleSet, out: &mut TriangleMeshStats) {
    let bbox = bounding_box(its);
    out.min = bbox.min.cast::<f32>();
    out.max = bbox.max.cast::<f32>();
    out.size = out.max - out.min;
}

/// `fill_initial_stats(its, out)`.
fn fill_initial_stats(its: &IndexedTriangleSet, out: &mut TriangleMeshStats) {
    out.number_of_facets = its.indices.len() as u32;
    out.volume = its_volume(its);
    update_bounding_box(its, out);
    let face_neighbors = its_face_neighbors(its);
    out.number_of_parts = its_number_of_patches_with(its, &face_neighbors) as i32;
    out.open_edges = its_num_open_edges(&face_neighbors) as i32;
}

/// `trianglemesh_repair_on_import(stl)`: libslic3r's repair of a freshly read STL (see the module
/// documentation).
pub fn trianglemesh_repair_on_import(stl: &mut StlFile) {
    if stl.stats.number_of_facets == 0 {
        return;
    }
    log(Severity::Debug, "TriangleMesh::repair() started");

    stl_check_facets_exact(stl);
    stl.stats.facets_w_1_bad_edge =
        stl.stats.connected_facets_2_edge - stl.stats.connected_facets_3_edge;
    stl.stats.facets_w_2_bad_edge =
        stl.stats.connected_facets_1_edge - stl.stats.connected_facets_2_edge;
    stl.stats.facets_w_3_bad_edge =
        (stl.stats.number_of_facets as i32).wrapping_sub(stl.stats.connected_facets_1_edge);

    let mut tolerance = stl.stats.shortest_edge;
    let increment = stl.stats.bounding_diameter / 10000.0f32;
    let iterations = 2;
    if stl.stats.connected_facets_3_edge < stl.stats.number_of_facets as i32 {
        for _ in 0..iterations {
            if stl.stats.connected_facets_3_edge < stl.stats.number_of_facets as i32 {
                stl_check_facets_nearby(stl, tolerance);
                tolerance += increment;
            } else {
                break;
            }
        }
    }

    if stl.stats.connected_facets_3_edge < stl.stats.number_of_facets as i32 {
        stl_remove_unconnected_facets(stl);
    }

    stl_fix_normal_directions(stl);
    stl_fix_normal_values(stl);
    stl_calculate_volume(stl);
    stl_verify_neighbors(stl);

    if stl.stats.number_of_facets > 0 && stl.stats.degenerate_facets > 0 {
        stl_check_facets_exact(stl);
    }
    log(Severity::Debug, "TriangleMesh::repair() finished");
}

/// The coordinate axes of `TriangleMesh::rotate` and `mirror` (`Axis`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// `X`.
    X,
    /// `Y`.
    Y,
    /// `Z`.
    Z,
}

/// `TriangleMesh`.
#[derive(Clone, Debug, PartialEq)]
pub struct TriangleMesh {
    /// `its`.
    pub its: IndexedTriangleSet,
    stats: TriangleMeshStats,
    init_shift: Vec3d,
}

impl Default for TriangleMesh {
    /// `TriangleMesh()`: empty, the volume -1 and the shift zero.
    fn default() -> Self {
        TriangleMesh {
            its: IndexedTriangleSet::default(),
            stats: TriangleMeshStats::default(),
            init_shift: Vec3d::zero(),
        }
    }
}

impl TriangleMesh {
    /// `TriangleMesh(vertices, faces)`: the set made by the two-argument constructor (a property
    /// per face), with its statistics.
    #[must_use]
    pub fn from_vertices_faces(vertices: Vec<Vec3f>, faces: Vec<Vec3i32>) -> Self {
        Self::from_its(IndexedTriangleSet::new(faces, vertices))
    }

    /// `TriangleMesh(its)` (copied or moved): the set as given, with its statistics.
    #[must_use]
    pub fn from_its(its: IndexedTriangleSet) -> Self {
        Self::from_its_with_errors(its, RepairedMeshErrors::default())
    }

    /// `TriangleMesh(its, errors)`.
    #[must_use]
    pub fn from_its_with_errors(its: IndexedTriangleSet, errors: RepairedMeshErrors) -> Self {
        let mut mesh = TriangleMesh {
            its,
            stats: TriangleMeshStats::default(),
            init_shift: Vec3d::zero(),
        };
        mesh.stats.repaired_errors = errors;
        fill_initial_stats(&mesh.its, &mut mesh.stats);
        mesh
    }

    /// `clear()`.
    pub fn clear(&mut self) {
        self.its.clear();
        self.stats = TriangleMeshStats::default();
    }

    /// `from_stl(stl, repair)`: repairs (if asked), shares the vertices and takes the statistics.
    /// Always true, as in the C++.
    pub fn from_stl(&mut self, stl: &mut StlFile, repair: bool) -> bool {
        if repair {
            trianglemesh_repair_on_import(stl);
        }
        stl_generate_shared_vertices(stl, &mut self.its);
        fill_initial_stats(&self.its, &mut self.stats);
        true
    }

    /// `ReadSTLFile(input_file, repair, nullptr, custom_header_length)`: reads the STL file at
    /// `path` (named `file` in the log) into this mesh.
    ///
    /// # Errors
    ///
    /// Those of [`stl_open`]; the mesh is left as it was.
    pub fn read_stl_file(
        &mut self,
        path: &Path,
        file: &str,
        repair: bool,
        custom_header_length: i32,
    ) -> Result<(), StlError> {
        let mut stl = StlFile::default();
        stl_open(&mut stl, path, file, custom_header_length)?;
        self.from_stl(&mut stl, repair);
        Ok(())
    }

    /// [`TriangleMesh::read_stl_file`] of a file holding `raw` (`None`: one that cannot be opened).
    ///
    /// # Errors
    ///
    /// As [`TriangleMesh::read_stl_file`].
    pub fn read_stl_bytes(
        &mut self,
        raw: Option<&[u8]>,
        file: &str,
        repair: bool,
        custom_header_length: i32,
    ) -> Result<(), StlError> {
        let mut stl = StlFile::default();
        crate::admesh::stlinit::stl_open_bytes(&mut stl, raw, file, custom_header_length)?;
        self.from_stl(&mut stl, repair);
        Ok(())
    }

    /// `stats()`.
    #[must_use]
    pub fn stats(&self) -> &TriangleMeshStats {
        &self.stats
    }

    /// `volume()`: the stored volume, computed first if it is -1.
    pub fn volume(&mut self) -> f32 {
        if self.stats.volume == -1.0 {
            self.stats.volume = its_volume(&self.its);
        }
        self.stats.volume
    }

    /// `scale(factor)`.
    pub fn scale(&mut self, factor: f32) {
        self.scale_versor(&Vec3f::new(factor, factor, factor));
    }

    /// `scale(versor)`.
    pub fn scale_versor(&mut self, versor: &Vec3f) {
        self.stats.min = self.stats.min.cwise_product(versor);
        self.stats.max = self.stats.max.cwise_product(versor);
        self.stats.size = self.stats.size.cwise_product(versor);
        if f64::from(self.stats.volume) > 0.0 {
            self.stats.volume *= versor[0] * versor[1] * versor[2];
        }
        if versor.x() == versor.y() && versor.x() == versor.z() {
            let s = versor.x();
            for v in &mut self.its.vertices {
                *v *= s;
            }
        } else {
            for v in &mut self.its.vertices {
                v[0] *= versor.x();
                v[1] *= versor.y();
                v[2] *= versor.z();
            }
        }
    }

    /// `translate(displacement)`: nothing for a zero displacement.
    pub fn translate(&mut self, displacement: &Vec3f) {
        if displacement.x() != 0.0 || displacement.y() != 0.0 || displacement.z() != 0.0 {
            for v in &mut self.its.vertices {
                *v += *displacement;
            }
            self.stats.min += *displacement;
            self.stats.max += *displacement;
        }
    }

    /// `translate(x, y, z)`.
    pub fn translate_xyz(&mut self, x: f32, y: f32, z: f32) {
        self.translate(&Vec3f::new(x, y, z));
    }

    /// `rotate(angle, axis)`: `angle` in radians, turned into degrees in `float`
    /// (`rad2deg`: `180.f * angle / float(PI)`); nothing for 0.
    pub fn rotate_axis(&mut self, angle: f32, axis: Axis) {
        if angle != 0.0 {
            let degrees = 180.0f32 * angle / core::f64::consts::PI as f32;
            match axis {
                Axis::X => its_rotate_x(&mut self.its, degrees),
                Axis::Y => its_rotate_y(&mut self.its, degrees),
                Axis::Z => its_rotate_z(&mut self.its, degrees),
            }
            update_bounding_box(&self.its, &mut self.stats);
        }
    }

    /// `rotate_x(angle)`.
    pub fn rotate_x(&mut self, angle: f32) {
        self.rotate_axis(angle, Axis::X);
    }

    /// `rotate_y(angle)`.
    pub fn rotate_y(&mut self, angle: f32) {
        self.rotate_axis(angle, Axis::Y);
    }

    /// `rotate_z(angle)`.
    pub fn rotate_z(&mut self, angle: f32) {
        self.rotate_axis(angle, Axis::Z);
    }

    /// `rotate(angle, axis)` about a `Vec3d` axis: `Transform3d::Identity().rotate(AngleAxisd(angle,
    /// axis.normalized()))` applied to the vertices; nothing for 0.
    pub fn rotate_about(&mut self, angle: f32, axis: &Vec3d) {
        if angle != 0.0 {
            let axis_norm = axis.normalized();
            let mut m = Transform3d::identity();
            m.rotate_angle_axis(&AngleAxis::new(f64::from(angle), axis_norm));
            its_transform(&mut self.its, &m, false);
            update_bounding_box(&self.its, &mut self.stats);
        }
    }

    /// `mirror(axis)`: the coordinate times -1 (`*= -1.f` for x, `*= -1.0` for y and z, alike:
    /// [`times_minus_one`]), the faces flipped, the box's extremes on that axis swapped and
    /// negated.
    pub fn mirror(&mut self, axis: Axis) {
        let iaxis = match axis {
            Axis::X => 0,
            Axis::Y => 1,
            Axis::Z => 2,
        };
        for v in &mut self.its.vertices {
            v[iaxis] = times_minus_one(v[iaxis]);
        }
        its_flip_triangles(&mut self.its);
        std::mem::swap(&mut self.stats.min[iaxis], &mut self.stats.max[iaxis]);
        self.stats.min[iaxis] = times_minus_one(self.stats.min[iaxis]);
        self.stats.max[iaxis] = times_minus_one(self.stats.max[iaxis]);
    }

    /// `mirror_x()`.
    pub fn mirror_x(&mut self) {
        self.mirror(Axis::X);
    }

    /// `mirror_y()`.
    pub fn mirror_y(&mut self) {
        self.mirror(Axis::Y);
    }

    /// `mirror_z()`.
    pub fn mirror_z(&mut self) {
        self.mirror(Axis::Z);
    }

    /// `transform(t, fix_left_handed)`: the vertices transformed, the faces flipped when asked and
    /// the determinant is negative, the volume times the determinant (made positive by the flip),
    /// the box taken again.
    pub fn transform(&mut self, t: &Transform3d, fix_left_handed: bool) {
        its_transform(&mut self.its, t, false);
        let mut det = crate::eigen_lu::dynamic_block_determinant(&t.linear());
        if fix_left_handed && det < 0.0 {
            its_flip_triangles(&mut self.its);
            det = -det;
        }
        self.stats.volume = (f64::from(self.stats.volume) * det) as f32;
        update_bounding_box(&self.its, &mut self.stats);
    }

    /// `transform(m, fix_left_handed)` for a `Matrix3d`.
    pub fn transform_matrix(&mut self, m: &Matrix3d, fix_left_handed: bool) {
        its_transform_matrix(&mut self.its, m, false);
        let mut det = crate::eigen_lu::dynamic_block_determinant(m);
        if fix_left_handed && det < 0.0 {
            its_flip_triangles(&mut self.its);
            det = -det;
        }
        self.stats.volume = (f64::from(self.stats.volume) * det) as f32;
        update_bounding_box(&self.its, &mut self.stats);
    }

    /// `flip_triangles()`: the faces flipped, the volume negated.
    pub fn flip_triangles(&mut self) {
        its_flip_triangles(&mut self.its);
        self.stats.volume = -self.stats.volume;
    }

    /// `align_to_origin()`: translated by minus the box's minimum.
    pub fn align_to_origin(&mut self) {
        let m = self.stats.min;
        self.translate_xyz(-m[0], -m[1], -m[2]);
    }

    /// `rotate(angle, center)`: about z through `center` (scaled coordinates cast to `float`),
    /// `angle` in degrees as `its_rotate_z` takes it; nothing for 0.
    pub fn rotate_about_point(&mut self, angle: f64, center: &Point) {
        if angle != 0.0 {
            let c = [center.x as f32, center.y as f32];
            self.translate_xyz(-c[0], -c[1], 0.0);
            its_rotate_z(&mut self.its, angle as f32);
            self.translate_xyz(c[0], c[1], 0.0);
        }
    }

    /// `is_splittable()`.
    #[must_use]
    pub fn is_splittable(&self) -> bool {
        its_is_splittable(&self.its)
    }

    /// `split()`: the connected parts, each made a mesh and flipped if its volume is negative.
    #[must_use]
    pub fn split(&self) -> Vec<TriangleMesh> {
        let itss = its_split(&self.its);
        let mut out = Vec::with_capacity(itss.len());
        for m in itss {
            let mut triangle_mesh = TriangleMesh::from_its(m);
            if triangle_mesh.volume() < 0.0 {
                triangle_mesh.flip_triangles();
            }
            out.push(triangle_mesh);
        }
        out
    }

    /// `merge(mesh)`: the sets merged, the statistics merged (see the module documentation).
    pub fn merge(&mut self, mesh: &TriangleMesh) {
        its_merge(&mut self.its, &mesh.its);
        self.stats = self.stats.merge(&mesh.stats);
    }

    /// `convex_hull()`: the 2D convex hull of the vertices projected on z = 0, scaled
    /// (`Point::new_scale` of each vertex's x and y).
    #[must_use]
    pub fn convex_hull(&self) -> Polygon {
        let pp: Vec<Point> = self
            .its
            .vertices
            .iter()
            .map(|v| Point::new_scale(f64::from(v[0]), f64::from(v[1])))
            .collect();
        orca_geom::convex_hull::convex_hull(pp)
    }

    /// `bounding_box()`: the stored box, always defined.
    #[must_use]
    pub fn bounding_box(&self) -> BoundingBoxf3 {
        BoundingBoxf3 {
            min: self.stats.min.cast::<f64>(),
            max: self.stats.max.cast::<f64>(),
            defined: true,
        }
    }

    /// `transformed_bounding_box(trafo)`: the box of the transformed vertices.
    #[must_use]
    pub fn transformed_bounding_box(&self, trafo: &Transform3d) -> BoundingBoxf3 {
        let mut bbox = BoundingBoxf3::default();
        for v in &self.its.vertices {
            bbox.merge(trafo.mul_point(&v.cast::<f64>()));
        }
        bbox
    }

    /// `transformed_bounding_box(trafod, world_min_z)`: the box of the part above `world_min_z` of
    /// the mesh transformed in `float` (`trafod.cast<float>()`), with the points where edges cross
    /// the plane; undefined when nothing is above.
    #[must_use]
    pub fn transformed_bounding_box_above(
        &self,
        trafod: &Transform3d,
        world_min_z: f64,
    ) -> BoundingBoxf3 {
        let trafo = trafod.cast::<f32>();
        let mut sides: Vec<i32> = Vec::with_capacity(self.its.vertices.len());
        let mut num_above = 0usize;
        // Eigen::AlignedBox<float, 3>, empty: min at the largest float, max at the lowest.
        let mut bmin = Vec3f::new(f32::MAX, f32::MAX, f32::MAX);
        let mut bmax = Vec3f::new(f32::MIN, f32::MIN, f32::MIN);
        let mut extend = |p: &Vec3f| {
            bmin = bmin.cwise_min(p);
            bmax = bmax.cwise_max(p);
        };
        for v in &self.its.vertices {
            let pt = trafo.mul_point(v);
            let z = f64::from(pt.z());
            let sign = if z > world_min_z {
                1
            } else if z < world_min_z {
                -1
            } else {
                0
            };
            sides.push(sign);
            if sign >= 0 {
                num_above += 1;
                extend(&pt);
            }
        }
        if num_above < self.its.vertices.len() {
            for tri in &self.its.indices {
                let s = [
                    sides[tri[0] as usize],
                    sides[tri[1] as usize],
                    sides[tri[2] as usize],
                ];
                if orca_stl::min(s[0], orca_stl::min(s[1], s[2])) < 0
                    && orca_stl::max(s[0], orca_stl::max(s[1], s[2])) > 0
                {
                    let mut iprev = 2;
                    for iedge in 0..3 {
                        if s[iprev] * s[iedge] == -1 {
                            let p1 = trafo.mul_point(&self.its.vertices[tri[iprev] as usize]);
                            let p2 = trafo.mul_point(&self.its.vertices[tri[iedge] as usize]);
                            let t = ((world_min_z - f64::from(p1.z())) / f64::from(p2.z() - p1.z()))
                                as f32;
                            extend(&Vec3f::new(
                                p1.x() + (p2.x() - p1.x()) * t,
                                p1.y() + (p2.y() - p1.y()) * t,
                                world_min_z as f32,
                            ));
                        }
                        iprev = iedge;
                    }
                }
            }
        }
        let mut out = BoundingBoxf3::default();
        // isEmpty(): some minimum above its maximum.
        let empty = (0..3).any(|k| bmin[k] > bmax[k]);
        if !empty {
            out.min = bmin.cast::<f64>();
            out.max = bmax.cast::<f64>();
            out.defined = true;
        }
        out
    }

    /// `size()`.
    #[must_use]
    pub fn size(&self) -> Vec3d {
        self.stats.size.cast::<f64>()
    }

    /// `center()`.
    #[must_use]
    pub fn center(&self) -> Vec3d {
        self.bounding_box().center()
    }

    /// `facets_count()`.
    #[must_use]
    pub fn facets_count(&self) -> usize {
        self.stats.number_of_facets as usize
    }

    /// `empty()`.
    #[must_use]
    pub fn empty(&self) -> bool {
        self.facets_count() == 0
    }

    /// `set_init_shift(offset)`.
    pub fn set_init_shift(&mut self, offset: Vec3d) {
        self.init_shift = offset;
    }

    /// `get_init_shift()`.
    #[must_use]
    pub fn get_init_shift(&self) -> Vec3d {
        self.init_shift
    }
}
