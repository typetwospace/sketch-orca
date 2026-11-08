// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: the indexed_triangle_set helpers of src/libslic3r/TriangleMesh.cpp and
// TriangleMesh.hpp (its_volume, its_flip_triangles, its_reverse_all_facets, its_merge,
// bounding_box, VertexFaceIndex, its_face_neighbors, its_num_open_edges, the face normals,
// its_collect_mesh_projection_points_above, its_convex_hull), src/libslic3r/MeshSplitImpl.hpp
// (create_face_neighbors_index, NeighborVisitor, its_split, its_is_splittable,
// its_number_of_patches) and the its_transform templates of deps_src/admesh/stl.h, of OrcaSlicer
// v2.4.2, translated on 2026-11-07. Copyright the OrcaSlicer authors, the BambuStudio authors
// (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU Affero General Public
// License, version 3 (AGPL-3.0; LICENSE).

//! The `its_*` functions over an [`IndexedTriangleSet`].
//!
//! The face neighbours (`create_face_neighbors_index`): for each face and each edge without a
//! neighbour yet, the first later face (in the order a vertex's faces are listed, which is face
//! order) that has the edge's two vertices the other way round and no neighbour on that edge.
//! `its_face_neighbors` runs the faces in order. `its_face_neighbors_par`, which `its_split`,
//! `its_is_splittable` and `its_number_of_patches` use when given no index, runs them as a oneTBB
//! `parallel_for` over `blocked_range(0, faces, max_concurrency)` (through `orca-tbb`): in a
//! serial arena, as the parity oracle runs the engine, the leaves run left to right and give the
//! same index; in a parallel arena faces race to claim a neighbour's edge as they race in the C++
//! engine, here through atomics (`parity/HAZARDS.md`, orca-mesh).
//!
//! The 3D convex hull ([`its_convex_hull`]) is Qhull's (`orca-qhull`, `runQhull` with `Qt`): the
//! triangles in the order Qhull lists its facets, each turned to face the way Qhull's normal does,
//! and the vertices numbered as the triangles first use them.

use std::sync::atomic::{AtomicI32, Ordering};

use orca_eigen::{Mat3, Matrix3d, Transform3, Transform3d, Vec2i32, Vec3d, Vec3f, Vec3i32};
use orca_geom::bounding_box::BoundingBoxf3;
use orca_geom::point::{Point, Points, SCALING_FACTOR};
use orca_geom::polygon::Polygon;
use orca_tbb::{BlockedRange, Partitioner};

use crate::admesh::{IndexedTriangleSet, StlTriangleVertexIndices, StlVertex};
use crate::log::{Severity, log, write_stderr};

/// `its_triangle`: a face's three vertices.
pub type ItsTriangle = [StlVertex; 3];

/// `its_triangle_vertices(its, face_id)`.
#[must_use]
pub fn its_triangle_vertices(its: &IndexedTriangleSet, face_id: usize) -> ItsTriangle {
    let f = its.indices[face_id];
    [
        its.vertices[f[0] as usize],
        its.vertices[f[1] as usize],
        its.vertices[f[2] as usize],
    ]
}

/// `its_unnormalized_normal(its, face_id)`: `(v1 - v0).cross(v2 - v0)`.
#[must_use]
pub fn its_unnormalized_normal(its: &IndexedTriangleSet, face_id: usize) -> Vec3f {
    let t = its_triangle_vertices(its, face_id);
    (t[1] - t[0]).cross(&(t[2] - t[0]))
}

/// `face_normal(vertex)`: `(v1 - v0).cross(v2 - v1).normalized()`.
#[must_use]
pub fn face_normal(vertex: &ItsTriangle) -> Vec3f {
    (vertex[1] - vertex[0])
        .cross(&(vertex[2] - vertex[1]))
        .normalized()
}

/// `face_normal_normalized(vertex)`: `face_normal(vertex).normalized()`.
#[must_use]
pub fn face_normal_normalized(vertex: &ItsTriangle) -> Vec3f {
    face_normal(vertex).normalized()
}

/// `its_face_normal(its, face)`.
#[must_use]
pub fn its_face_normal(its: &IndexedTriangleSet, face: StlTriangleVertexIndices) -> Vec3f {
    let vertices = [
        its.vertices[face[0] as usize],
        its.vertices[face[1] as usize],
        its.vertices[face[2] as usize],
    ];
    face_normal_normalized(&vertices)
}

/// `its_face_normals(its)`.
#[must_use]
pub fn its_face_normals(its: &IndexedTriangleSet) -> Vec<Vec3f> {
    its.indices
        .iter()
        .map(|&face| its_face_normal(its, face))
        .collect()
}

/// `its_flip_triangles(its)`: swaps each face's second and third index.
pub fn its_flip_triangles(its: &mut IndexedTriangleSet) {
    for face in &mut its.indices {
        face.0.swap(1, 2);
    }
}

/// `its_reverse_all_facets(its)`: swaps each face's first and second index.
pub fn its_reverse_all_facets(its: &mut IndexedTriangleSet) {
    for face in &mut its.indices {
        face.0.swap(0, 1);
    }
}

/// `its_merge(A, B)`: B's vertices and faces appended to A's, B's indices shifted.
pub fn its_merge(a: &mut IndexedTriangleSet, b: &IndexedTriangleSet) {
    let n = a.vertices.len() as i32;
    let n_f = a.indices.len();
    a.vertices.extend_from_slice(&b.vertices);
    a.indices.extend_from_slice(&b.indices);
    for face in &mut a.indices[n_f..] {
        *face += Vec3i32::new(n, n, n);
    }
}

/// `its_merge(A, triangles)`: each three vertices a new face.
pub fn its_merge_triangles(a: &mut IndexedTriangleSet, triangles: &[Vec3f]) {
    let offs = a.vertices.len();
    a.vertices.extend_from_slice(triangles);
    let mut i = offs as i32;
    while i < a.vertices.len() as i32 {
        a.indices.push(Vec3i32::new(i, i + 1, i + 2));
        i += 3;
    }
}

/// `its_merge(A, Pointf3s)`: the points cast to `float` first.
pub fn its_merge_pointf3s(a: &mut IndexedTriangleSet, triangles: &[Vec3d]) {
    let trianglesf: Vec<Vec3f> = triangles.iter().map(|t| t.cast::<f32>()).collect();
    its_merge_triangles(a, &trianglesf);
}

/// `its_volume(its)`: the signed volume, as admesh's `get_volume` but with each face's area and
/// unit normal from `float` cross products; 0 for an empty set.
#[must_use]
pub fn its_volume(its: &IndexedTriangleSet) -> f32 {
    if its.empty() {
        return 0.0;
    }
    let p0 = its.vertices[0];
    let mut volume = 0f32;
    for i in 0..its.indices.len() {
        let triangle = its_triangle_vertices(its, i);
        let u = triangle[1] - triangle[0];
        let v = triangle[2] - triangle[0];
        let c = u.cross(&v);
        let normal = c.normalized();
        let area = (0.5 * f64::from(c.norm())) as f32;
        let height = normal.dot(&(triangle[0] - p0));
        volume += (area * height) / 3.0f32;
    }
    volume
}

/// `its_average_edge_length(its)`.
#[must_use]
pub fn its_average_edge_length(its: &IndexedTriangleSet) -> f32 {
    if its.indices.is_empty() {
        return 0.0;
    }
    let mut edge_length = 0f64;
    for i in 0..its.indices.len() {
        let v = its_triangle_vertices(its, i);
        edge_length += (v[1] - v[0]).cast::<f64>().norm()
            + (v[2] - v[0]).cast::<f64>().norm()
            + (v[1] - v[2]).cast::<f64>().norm();
    }
    (edge_length / (3 * its.indices.len()) as f64) as f32
}

/// `bounding_box(its)`: `p.cwiseMin(bmin)` over the vertices, as a `BoundingBoxf3(min, max)`
/// (undefined when flat); an undefined zero box with no vertices.
#[must_use]
pub fn bounding_box(its: &IndexedTriangleSet) -> BoundingBoxf3 {
    let Some(&first) = its.vertices.first() else {
        return BoundingBoxf3::default();
    };
    let (mut bmin, mut bmax) = (first, first);
    for p in &its.vertices {
        bmin = p.cwise_min(&bmin);
        bmax = p.cwise_max(&bmax);
    }
    BoundingBoxf3::from_corners(bmin.cast::<f64>(), bmax.cast::<f64>())
}

/// `its_transform(its, t, fix_left_handed)` for a `Transform3d`: each vertex
/// `(t * v.cast<double>()).cast<float>()`, then the faces flipped (first two indices swapped) if
/// asked and the linear part's determinant is negative (`t.matrix().block(0, 0, 3, 3)`, a
/// runtime-sized block, so by LU: [`crate::eigen_lu`]).
pub fn its_transform(its: &mut IndexedTriangleSet, t: &Transform3d, fix_left_handed: bool) {
    for v in &mut its.vertices {
        *v = t.mul_point(&v.cast::<f64>()).cast::<f32>();
    }
    if fix_left_handed && crate::eigen_lu::dynamic_block_determinant(&t.linear()) < 0.0 {
        for face in &mut its.indices {
            face.0.swap(0, 1);
        }
    }
}

/// `its_transform(its, m, fix_left_handed)` for a `Matrix3d`: each vertex
/// `(m * v.cast<double>()).cast<float>()`, the faces flipped if asked and the (fixed-size,
/// cofactor) determinant is negative.
pub fn its_transform_matrix(its: &mut IndexedTriangleSet, m: &Matrix3d, fix_left_handed: bool) {
    for v in &mut its.vertices {
        *v = m.mul_vec(&v.cast::<f64>()).cast::<f32>();
    }
    if fix_left_handed && m.determinant() < 0.0 {
        for face in &mut its.indices {
            face.0.swap(0, 1);
        }
    }
}

/// `VertexFaceIndex`: the faces incident with each vertex, in face order.
#[derive(Clone, Debug, Default)]
pub struct VertexFaceIndex {
    vertex_to_face_start: Vec<usize>,
    vertex_faces_all: Vec<usize>,
}

impl VertexFaceIndex {
    /// `VertexFaceIndex(its)`.
    #[must_use]
    pub fn new(its: &IndexedTriangleSet) -> Self {
        let mut index = VertexFaceIndex::default();
        index.create(its);
        index
    }

    /// `create(its)`: counts, prefix sums, then the faces scattered in order.
    pub fn create(&mut self, its: &IndexedTriangleSet) {
        let start = &mut self.vertex_to_face_start;
        start.clear();
        start.resize(its.vertices.len() + 1, 0);
        for face in &its.indices {
            for k in 0..3 {
                start[face[k] as usize + 1] += 1;
            }
        }
        for i in 2..start.len() {
            start[i] += start[i - 1];
        }
        self.vertex_faces_all.clear();
        self.vertex_faces_all.resize(*start.last().unwrap_or(&0), 0);
        for (face_idx, face) in its.indices.iter().enumerate() {
            for k in 0..3 {
                let slot = &mut start[face[k] as usize];
                self.vertex_faces_all[*slot] = face_idx;
                *slot += 1;
            }
        }
        for i in (1..start.len()).rev() {
            start[i] = start[i - 1];
        }
        if let Some(first) = start.first_mut() {
            *first = 0;
        }
    }

    /// `operator[](vertex_id)`: the faces incident with the vertex.
    #[must_use]
    pub fn faces(&self, vertex_id: usize) -> &[usize] {
        &self.vertex_faces_all
            [self.vertex_to_face_start[vertex_id]..self.vertex_to_face_start[vertex_id + 1]]
    }

    /// `count(vertex_id)`.
    #[must_use]
    pub fn count(&self, vertex_id: usize) -> usize {
        self.vertex_to_face_start[vertex_id + 1] - self.vertex_to_face_start[vertex_id]
    }
}

/// `its_triangle_vertex_index(triangle_indices, vertex_idx)`: 0, 1, 2 or -1.
#[must_use]
pub fn its_triangle_vertex_index(
    triangle_indices: &StlTriangleVertexIndices,
    vertex_idx: i32,
) -> i32 {
    if vertex_idx == triangle_indices[0] {
        0
    } else if vertex_idx == triangle_indices[1] {
        1
    } else if vertex_idx == triangle_indices[2] {
        2
    } else {
        -1
    }
}

/// `its_triangle_edge(triangle_indices, edge_idx)`.
#[must_use]
pub fn its_triangle_edge(triangle_indices: &StlTriangleVertexIndices, edge_idx: usize) -> Vec2i32 {
    let next = if edge_idx == 2 { 0 } else { edge_idx + 1 };
    Vec2i32::new(triangle_indices[edge_idx], triangle_indices[next])
}

/// `no_value` of `create_face_neighbors_index`.
const NO_VALUE: i32 = -1;

/// The body of `create_face_neighbors_index` for one face, over neighbour slots read and written
/// through `get` and `set`.
fn face_neighbors_of(
    indices: &[StlTriangleVertexIndices],
    vertex_triangles: &VertexFaceIndex,
    face_idx: usize,
    get: &dyn Fn(usize, usize) -> i32,
    set: &dyn Fn(usize, usize, i32),
) {
    let triangle_indices = &indices[face_idx];
    for edge_index in 0..3 {
        if get(face_idx, edge_index) != NO_VALUE {
            continue;
        }
        let edge_indices = its_triangle_edge(triangle_indices, edge_index);
        for &other_face in vertex_triangles.faces(edge_indices[0] as usize) {
            if other_face <= face_idx {
                continue;
            }
            let face_indices = &indices[other_face];
            let vertex_index = its_triangle_vertex_index(face_indices, edge_indices[1]);
            if vertex_index < 0 {
                continue;
            }
            if edge_indices[0] != face_indices[((vertex_index + 1) % 3) as usize] {
                continue;
            }
            if get(other_face, vertex_index as usize) != NO_VALUE {
                continue;
            }
            set(face_idx, edge_index, other_face as i32);
            set(other_face, vertex_index as usize, face_idx as i32);
            break;
        }
    }
}

/// `create_face_neighbors_index(ex_seq, its)`: see the module documentation.
#[must_use]
pub fn its_face_neighbors(its: &IndexedTriangleSet) -> Vec<Vec3i32> {
    let indices = &its.indices;
    if indices.is_empty() {
        return Vec::new();
    }
    let vertex_triangles = VertexFaceIndex::new(its);
    let neighbors: Vec<[AtomicI32; 3]> = (0..indices.len())
        .map(|_| {
            [
                AtomicI32::new(NO_VALUE),
                AtomicI32::new(NO_VALUE),
                AtomicI32::new(NO_VALUE),
            ]
        })
        .collect();
    let get = |f: usize, e: usize| neighbors[f][e].load(Ordering::Relaxed);
    let set = |f: usize, e: usize, v: i32| neighbors[f][e].store(v, Ordering::Relaxed);
    for face_idx in 0..indices.len() {
        face_neighbors_of(indices, &vertex_triangles, face_idx, &get, &set);
    }
    collect_neighbors(&neighbors)
}

/// `create_face_neighbors_index(ex_tbb, its)`: see the module documentation.
#[must_use]
pub fn its_face_neighbors_par(its: &IndexedTriangleSet) -> Vec<Vec3i32> {
    let indices = &its.indices;
    if indices.is_empty() {
        return Vec::new();
    }
    let vertex_triangles = VertexFaceIndex::new(its);
    let neighbors: Vec<[AtomicI32; 3]> = (0..indices.len())
        .map(|_| {
            [
                AtomicI32::new(NO_VALUE),
                AtomicI32::new(NO_VALUE),
                AtomicI32::new(NO_VALUE),
            ]
        })
        .collect();
    let get = |f: usize, e: usize| neighbors[f][e].load(Ordering::Relaxed);
    let set = |f: usize, e: usize, v: i32| neighbors[f][e].store(v, Ordering::Relaxed);
    let grain = orca_tbb::max_concurrency();
    orca_tbb::parallel_for(
        BlockedRange::new(0usize, indices.len(), grain),
        Partitioner::Auto,
        |range: BlockedRange<usize>| {
            for face_idx in range.begin()..range.end() {
                face_neighbors_of(indices, &vertex_triangles, face_idx, &get, &set);
            }
        },
    );
    collect_neighbors(&neighbors)
}

fn collect_neighbors(neighbors: &[[AtomicI32; 3]]) -> Vec<Vec3i32> {
    neighbors
        .iter()
        .map(|n| {
            Vec3i32::new(
                n[0].load(Ordering::Relaxed),
                n[1].load(Ordering::Relaxed),
                n[2].load(Ordering::Relaxed),
            )
        })
        .collect()
}

/// `its_num_open_edges(face_neighbors)`: the negative entries.
#[must_use]
pub fn its_num_open_edges(face_neighbors: &[Vec3i32]) -> usize {
    face_neighbors
        .iter()
        .map(|n| n.0.iter().filter(|&&x| x < 0).count())
        .sum()
}

/// `its_num_open_edges(its)`.
#[must_use]
pub fn its_num_open_edges_of(its: &IndexedTriangleSet) -> usize {
    its_num_open_edges(&its_face_neighbors(its))
}

/// `meshsplit_detail::NeighborVisitor`: the connected patches of faces, one per `visit`, each
/// grown from its lowest unvisited face by a stack of faces.
struct NeighborVisitor<'a> {
    neighbor_index: &'a [Vec3i32],
    visited: Vec<bool>,
    facestack: Vec<usize>,
    seed: usize,
}

impl<'a> NeighborVisitor<'a> {
    fn new(its: &IndexedTriangleSet, neighbor_index: &'a [Vec3i32]) -> Self {
        NeighborVisitor {
            neighbor_index,
            visited: vec![false; its.indices.len()],
            facestack: Vec::with_capacity(its.indices.len()),
            seed: 0,
        }
    }

    /// `visit(visitor)`: the visitor sees each face of the next patch; false stops the walk.
    fn visit(&mut self, mut visitor: impl FnMut(usize) -> bool) {
        let found = self.visited[self.seed..].iter().position(|&v| !v);
        let Some(offset) = found else {
            self.seed = self.visited.len();
            return;
        };
        self.seed += offset;
        let idx = self.seed;
        self.seed += 1;
        if !visitor(idx) {
            return;
        }
        self.facestack.push(idx);
        self.visited[idx] = true;
        while let Some(facet_idx) = self.facestack.pop() {
            for &neighbor_idx in &self.neighbor_index[facet_idx].0 {
                if neighbor_idx >= 0 && !self.visited[neighbor_idx as usize] {
                    if !visitor(neighbor_idx as usize) {
                        return;
                    }
                    self.visited[neighbor_idx as usize] = true;
                    self.facestack.push(neighbor_idx as usize);
                }
            }
        }
    }
}

/// `its_split(its, face_neighbors)`: the connected patches as sets of their own, each with the
/// vertices of its faces in first-use order.
#[must_use]
pub fn its_split_with(
    its: &IndexedTriangleSet,
    face_neighbors: &[Vec3i32],
) -> Vec<IndexedTriangleSet> {
    #[derive(Clone, Copy)]
    struct VertexConv {
        part_id: usize,
        vertex_image: usize,
    }
    let mut vidx_conv = vec![
        VertexConv {
            part_id: usize::MAX,
            vertex_image: 0,
        };
        its.vertices.len()
    ];
    let mut visitor = NeighborVisitor::new(its, face_neighbors);
    let mut out = Vec::with_capacity(3);
    let mut facets: Vec<usize> = Vec::new();
    let mut part_id = 0usize;
    loop {
        facets.clear();
        visitor.visit(|idx| {
            facets.push(idx);
            true
        });
        if facets.is_empty() {
            break;
        }
        let mut mesh = IndexedTriangleSet::default();
        for &face_id in &facets {
            let face = its.indices[face_id];
            let mut new_face = Vec3i32::new(0, 0, 0);
            for v in 0..3 {
                let vi = face[v] as usize;
                if vidx_conv[vi].part_id != part_id {
                    vidx_conv[vi] = VertexConv {
                        part_id,
                        vertex_image: mesh.vertices.len(),
                    };
                    mesh.vertices.push(its.vertices[vi]);
                }
                new_face[v] = vidx_conv[vi].vertex_image as i32;
            }
            mesh.indices.push(new_face);
        }
        out.push(mesh);
        part_id += 1;
    }
    out
}

/// `its_split(its)`: with the neighbours of [`its_face_neighbors_par`].
#[must_use]
pub fn its_split(its: &IndexedTriangleSet) -> Vec<IndexedTriangleSet> {
    let neighbors = its_face_neighbors_par(its);
    its_split_with(its, &neighbors)
}

/// `its_is_splittable(its, face_neighbors)`: a second patch exists.
#[must_use]
pub fn its_is_splittable_with(its: &IndexedTriangleSet, face_neighbors: &[Vec3i32]) -> bool {
    let mut visitor = NeighborVisitor::new(its, face_neighbors);
    let mut has_some = false;
    let mut has_some2 = false;
    visitor.visit(|_| {
        has_some = true;
        true
    });
    if has_some {
        visitor.visit(|_| {
            has_some2 = true;
            false
        });
    }
    has_some && has_some2
}

/// `its_is_splittable(its)`: with the neighbours of [`its_face_neighbors_par`].
#[must_use]
pub fn its_is_splittable(its: &IndexedTriangleSet) -> bool {
    let neighbors = its_face_neighbors_par(its);
    its_is_splittable_with(its, &neighbors)
}

/// `its_number_of_patches(its, face_neighbors)`.
#[must_use]
pub fn its_number_of_patches_with(its: &IndexedTriangleSet, face_neighbors: &[Vec3i32]) -> usize {
    let mut visitor = NeighborVisitor::new(its, face_neighbors);
    let mut num_patches = 0;
    loop {
        let mut has_some = false;
        visitor.visit(|_| {
            has_some = true;
            true
        });
        if !has_some {
            break;
        }
        num_patches += 1;
    }
    num_patches
}

/// `its_number_of_patches(its)`: with the neighbours of [`its_face_neighbors_par`].
#[must_use]
pub fn its_number_of_patches(its: &IndexedTriangleSet) -> usize {
    let neighbors = its_face_neighbors_par(its);
    its_number_of_patches_with(its, &neighbors)
}

/// `scaled<coord_t>(float v)`: `coord_t(v / float(SCALING_FACTOR))`, the division in `float`.
fn scaled_from_f32(v: f32) -> i64 {
    orca_crt::f32_to_i64(v / SCALING_FACTOR as f32)
}

/// `its_collect_mesh_projection_points_above(its, transform_fn, z, all_pts)`: for each face, the
/// points where its edges cross the plane at `z`, and its vertices at or above it, scaled.
pub fn its_collect_mesh_projection_points_above(
    its: &IndexedTriangleSet,
    transform_fn: impl Fn(&Vec3f) -> Vec3f,
    z: f32,
    all_pts: &mut Points,
) {
    all_pts.reserve(its.indices.len() * 3);
    for tri in &its.indices {
        let pts = [
            transform_fn(&its.vertices[tri[0] as usize]),
            transform_fn(&its.vertices[tri[1] as usize]),
            transform_fn(&its.vertices[tri[2] as usize]),
        ];
        let mut iprev = 2;
        for iedge in 0..3 {
            let p1 = pts[iprev];
            let p2 = pts[iedge];
            if (p1.z() < z && p2.z() > z) || (p2.z() < z && p1.z() > z) {
                let t = (z - p1.z()) / (p2.z() - p1.z());
                all_pts.push(Point::new(
                    scaled_from_f32(p1.x() + (p2.x() - p1.x()) * t),
                    scaled_from_f32(p1.y() + (p2.y() - p1.y()) * t),
                ));
            }
            if p2.z() >= z {
                all_pts.push(Point::new(scaled_from_f32(p2.x()), scaled_from_f32(p2.y())));
            }
            iprev = iedge;
        }
    }
}

/// `its_convex_hull_2d_above(its, t, z)` for a `Transform3f`: the 2D hull of the points above.
#[must_use]
pub fn its_convex_hull_2d_above(its: &IndexedTriangleSet, t: &Transform3<f32>, z: f32) -> Polygon {
    let mut all_pts = Points::new();
    its_collect_mesh_projection_points_above(its, |p| t.mul_point(p), z, &mut all_pts);
    orca_geom::convex_hull::convex_hull(all_pts)
}

/// `its_convex_hull_2d_above(its, m, z)` for a `Matrix3f`.
#[must_use]
pub fn its_convex_hull_2d_above_matrix(its: &IndexedTriangleSet, m: &Mat3<f32>, z: f32) -> Polygon {
    let mut all_pts = Points::new();
    its_collect_mesh_projection_points_above(its, |p| m.mul_vec(p), z, &mut all_pts);
    orca_geom::convex_hull::convex_hull(all_pts)
}

/// `its_convex_hull(pts)`: the 3D convex hull of the points, by Qhull
/// (`runQhull("", 3, n, points, "Qt")`, the points as `double`s). For each of Qhull's facets in
/// its list order, the vertices take indices in the order they first appear (each the point as a
/// `float` again), and a facet of three becomes a triangle, whose vertex order is swapped when
/// its normal, `(v1 - v0) x (v2 - v1)` in `float`, points against Qhull's (the dot product in
/// `double`). When Qhull throws (fewer than 4 points, coplanar points, points that are not
/// finite), the error is logged and the set is empty; when it succeeds with a message (the
/// narrow-hull warning), `~Qhull` writes it to `std::cerr` ([`write_stderr`]) after the walk.
#[must_use]
pub fn its_convex_hull(pts: &[Vec3f]) -> IndexedTriangleSet {
    let mut dst_vertices: Vec<Vec3f> = Vec::new();
    let mut dst_facets: Vec<Vec3i32> = Vec::new();
    if !pts.is_empty() {
        // REALfloat is 0: the points are copied into doubles.
        let src_vertices: Vec<[f64; 3]> = pts
            .iter()
            .map(|v| [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])])
            .collect();
        let Ok(hull) = orca_qhull::convex_hull(&src_vertices) else {
            log(
                Severity::Error,
                "its_convex_hull: Unable to create convex hull",
            );
            // The thrown QhullError took Qhull's messages: ~Qhull prints nothing.
            return IndexedTriangleSet::default();
        };
        // Map of Qhull's vertex id to the vertex's index in dst_vertices.
        let mut map_dst_vertices: Vec<i32> = Vec::new();
        for facet in &hull.facets {
            let mut indices = [0i32; 3];
            let mut cnt = 0usize;
            for vertex in &facet.vertices {
                let id = vertex.id as usize;
                if id >= map_dst_vertices.len() {
                    map_dst_vertices.resize((id + 1).next_power_of_two(), -1);
                }
                let mut i = map_dst_vertices[id];
                if i == -1 {
                    i = i32::try_from(dst_vertices.len()).expect("fewer than 2^31 vertices");
                    map_dst_vertices[id] = i;
                    let pt = src_vertices[vertex.point];
                    #[allow(clippy::cast_possible_truncation)]
                    dst_vertices.push(Vec3f::new(pt[0] as f32, pt[1] as f32, pt[2] as f32));
                }
                // A fourth vertex would be written past the end of the C++'s Vec3i32; after
                // Qt every facet has three, so it never is.
                if cnt < 3 {
                    indices[cnt] = i;
                }
                let at = cnt;
                cnt += 1;
                if at == 3 {
                    break;
                }
            }
            if cnt == 3 {
                // Qhull sorts a facet's vertices by id, not by the facet's normal.
                let v = |k: usize| dst_vertices[indices[k] as usize];
                let n = (v(1) - v(0)).cross(&(v(2) - v(1)));
                let n2 = facet.normal;
                let d =
                    f64::from(n.x()) * n2[0] + f64::from(n.y()) * n2[1] + f64::from(n.z()) * n2[2];
                if d < 0.0 {
                    indices.swap(1, 2);
                }
                dst_facets.push(Vec3i32::new(indices[0], indices[1], indices[2]));
            }
        }
        // ~Qhull.
        if !hull.message.is_empty() {
            write_stderr("\nQhull output at end\n");
            write_stderr(&hull.message);
        }
    }
    IndexedTriangleSet::new(dst_facets, dst_vertices)
}

/// `its_convex_hull(its)`: the 3D convex hull of the set's vertices.
#[must_use]
pub fn its_convex_hull_its(its: &IndexedTriangleSet) -> IndexedTriangleSet {
    its_convex_hull(&its.vertices)
}
