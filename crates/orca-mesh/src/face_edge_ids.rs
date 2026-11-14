// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: its_face_edge_ids_impl and the four its_face_edge_ids overloads of
// src/libslic3r/TriangleMesh.cpp (lines 566-671), with the struct EdgeToFace and create_edge_map
// they call (lines 520-561) and its_triangle_edge_index of TriangleMesh.hpp (lines 261-266), of
// OrcaSlicer v2.4.2, translated on 2026-11-13 by orca-o3-front-2. Copyright the OrcaSlicer
// authors, the BambuStudio authors (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under
// the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! `its_face_edge_ids`: a unique id for each edge of a mesh, shared by the two faces that touch
//! along it, which the slicer chains its intersection lines by.
//!
//! The first three forms build an edge map (`create_edge_map`): each kept face's three edges with
//! their vertices sorted and the edge's index in its face made negative when the sort flipped it,
//! then `std::sort`ed on the vertex pair alone, through orca-stl's introsort, so where three or
//! more faces share an edge, which two of them get paired follows the order MSVC's sort leaves
//! the equal entries in (`parity/HAZARDS.md`, orca-mesh). Each edge not yet paired takes the next
//! id and is paired with the first later equal edge of opposite orientation not yet paired, else
//! with the first later equal edge not yet paired; an edge with no neighbour keeps its own id,
//! and a face the mask leaves out keeps -1 on all three edges.
//!
//! The fourth form ([`its_face_edge_ids_from_neighbors`]) walks the face neighbours instead: an
//! edge whose neighbour has a higher index takes the next id and gives it to the neighbour's
//! matching edge (the reversed edge, else the same edge). Its output is not initialised in the
//! C++ (`std::vector<Vec3i32>(n)` runs Eigen's default constructor, which leaves the values), so a
//! slot that no step writes holds what the heap held: here -1. When the neighbour holds neither
//! edge the C++ writes index -1 of the neighbour's triple, which is the last value of the face
//! before it in the vector's storage; the translation writes there too. Both happen only when the
//! neighbours are not symmetric, which `its_face_neighbors` of a closed, consistent mesh never
//! gives (`parity/HAZARDS.md`, orca-mesh).
//!
//! The C++'s `throw_on_cancel` callback is taken as a closure; the overloads that pass none pass
//! one that does nothing. Nothing here is parallel.

use orca_eigen::{Vec2i32, Vec3i32};

use crate::admesh::{IndexedTriangleSet, StlTriangleVertexIndices};
use crate::its::its_triangle_edge;

/// `its_triangle_edge_index(triangle_indices, triangle_edge)`: the index (0, 1 or 2) of the edge
/// in the triangle, in that orientation, or -1.
#[must_use]
pub fn its_triangle_edge_index(
    triangle_indices: &StlTriangleVertexIndices,
    triangle_edge: &Vec2i32,
) -> i32 {
    if triangle_edge[0] == triangle_indices[0] && triangle_edge[1] == triangle_indices[1] {
        0
    } else if triangle_edge[0] == triangle_indices[1] && triangle_edge[1] == triangle_indices[2] {
        1
    } else if triangle_edge[0] == triangle_indices[2] && triangle_edge[1] == triangle_indices[0] {
        2
    } else {
        -1
    }
}

/// `EdgeToFace`: one face's edge, its vertices sorted.
#[derive(Clone, Copy, Debug)]
struct EdgeToFace {
    /// `vertex_low`: the edge's first vertex, `vertex_low <= vertex_high`.
    vertex_low: i32,
    /// `vertex_high`.
    vertex_high: i32,
    /// `face`: the face, or -1 once the edge is paired.
    face: i32,
    /// `face_edge`: the edge's index in its face from 1, negative when the edge was stored
    /// reversed.
    face_edge: i32,
}

impl EdgeToFace {
    /// `operator==`: the same two vertices.
    fn same_edge(&self, other: &EdgeToFace) -> bool {
        self.vertex_low == other.vertex_low && self.vertex_high == other.vertex_high
    }

    /// `operator<`: by `vertex_low`, then `vertex_high`.
    fn less(&self, other: &EdgeToFace) -> bool {
        self.vertex_low < other.vertex_low
            || (self.vertex_low == other.vertex_low && self.vertex_high < other.vertex_high)
    }
}

/// `create_edge_map(its, face_filter, throw_on_cancel)`: the kept faces' edges, sorted.
fn create_edge_map(
    its: &IndexedTriangleSet,
    face_filter: &dyn Fn(u32) -> bool,
    throw_on_cancel: &mut dyn FnMut(),
) -> Vec<EdgeToFace> {
    let mut edges_map: Vec<EdgeToFace> = Vec::with_capacity(its.indices.len() * 3);
    let mut facet_idx: u32 = 0;
    while (facet_idx as usize) < its.indices.len() {
        if face_filter(facet_idx) {
            for i in 0..3_usize {
                let face = &its.indices[facet_idx as usize];
                let mut e2f = EdgeToFace {
                    vertex_low: face[i],
                    vertex_high: face[(i + 1) % 3],
                    // An uint32_t stored in an int.
                    face: facet_idx as i32,
                    // 1 based indexing, to be always strictly positive.
                    face_edge: i as i32 + 1,
                };
                if e2f.vertex_low > e2f.vertex_high {
                    // Sort the vertices
                    std::mem::swap(&mut e2f.vertex_low, &mut e2f.vertex_high);
                    // and make the face_edge negative to indicate a flipped edge.
                    e2f.face_edge = -e2f.face_edge;
                }
                edges_map.push(e2f);
            }
        }
        facet_idx += 1;
    }
    throw_on_cancel();
    orca_stl::sort_by(&mut edges_map, EdgeToFace::less);
    edges_map
}

/// `its_face_edge_ids_impl(its, face_filter, throw_on_cancel)`: see the module documentation.
fn its_face_edge_ids_impl(
    its: &IndexedTriangleSet,
    face_filter: &dyn Fn(u32) -> bool,
    throw_on_cancel: &mut dyn FnMut(),
) -> Vec<Vec3i32> {
    let mut out: Vec<Vec3i32> = vec![Vec3i32::new(-1, -1, -1); its.indices.len()];

    let mut edges_map = create_edge_map(its, face_filter, throw_on_cancel);

    // Assign a unique common edge id to touching triangle edges.
    let mut num_edges: i32 = 0;
    for i in 0..edges_map.len() {
        let edge_i = edges_map[i];
        if edge_i.face == -1 {
            // This edge has been connected to some neighbor already.
            continue;
        }
        // Unconnected edge. Find its neighbor with the correct orientation.
        let mut j = i + 1;
        let mut found = false;
        while j < edges_map.len() && edge_i.same_edge(&edges_map[j]) {
            if edge_i.face_edge * edges_map[j].face_edge < 0 && edges_map[j].face != -1 {
                // Faces touching with opposite oriented edges and none of the edges is connected
                // yet.
                found = true;
                break;
            }
            j += 1;
        }
        if !found {
            // Trying to find an edge with equal orientation (the C++'s FIXME: admesh can assign
            // the same edge ID to more than two facets, so a duplicate of this edge may already
            // have been seen in this orientation).
            j = i + 1;
            while j < edges_map.len() && edge_i.same_edge(&edges_map[j]) {
                if edges_map[j].face != -1 {
                    // Faces touching with equally oriented edges and none of the edges is
                    // connected yet.
                    found = true;
                    break;
                }
                j += 1;
            }
        }
        // Assign an edge index to the 1st face.
        out[edge_i.face as usize][(edge_i.face_edge.abs() - 1) as usize] = num_edges;
        if found {
            let edge_j = &mut edges_map[j];
            out[edge_j.face as usize][(edge_j.face_edge.abs() - 1) as usize] = num_edges;
            // Mark the edge as connected.
            edge_j.face = -1;
        }
        num_edges += 1;
        if (i & 0x0ffff) == 0 {
            throw_on_cancel();
        }
    }

    out
}

/// `its_face_edge_ids(its)`: every face's edges.
#[must_use]
pub fn its_face_edge_ids(its: &IndexedTriangleSet) -> Vec<Vec3i32> {
    its_face_edge_ids_impl(its, &|_| true, &mut || {})
}

/// `its_face_edge_ids(its, throw_on_cancel_callback)`: every face's edges, the callback called
/// where the C++ calls it (once after the edge map is made, then at every 65536th edge).
#[must_use]
pub fn its_face_edge_ids_cancellable(
    its: &IndexedTriangleSet,
    throw_on_cancel_callback: &mut dyn FnMut(),
) -> Vec<Vec3i32> {
    its_face_edge_ids_impl(its, &|_| true, throw_on_cancel_callback)
}

/// `its_face_edge_ids(its, face_mask)`: the edges of the faces the mask keeps; the others keep -1.
///
/// # Panics
///
/// When the mask is shorter than the faces, where the C++ reads past the `vector<bool>`'s end.
#[must_use]
pub fn its_face_edge_ids_masked(its: &IndexedTriangleSet, face_mask: &[bool]) -> Vec<Vec3i32> {
    its_face_edge_ids_impl(its, &|idx| face_mask[idx as usize], &mut || {})
}

/// `its_face_edge_ids(its, face_neighbors, assign_unbound_edges, num_edges)`: the edge ids from
/// the face neighbours (see the module documentation), an edge with no neighbour given an id of
/// its own when `assign_unbound_edges` is set and -1 otherwise; `num_edges`, when given, receives
/// the number of ids handed out.
///
/// # Panics
///
/// When a neighbour index is past the faces, where the C++ reads past the vector's end.
#[must_use]
pub fn its_face_edge_ids_from_neighbors(
    its: &IndexedTriangleSet,
    face_neighbors: &[Vec3i32],
    assign_unbound_edges: bool,
    num_edges: Option<&mut i32>,
) -> Vec<Vec3i32> {
    // out elements are not initialized in the C++: -1 here (the module documentation).
    let mut out: Vec<i32> = vec![-1; face_neighbors.len() * 3];
    let mut last_edge_id: i32 = 0;
    let count = face_neighbors.len() as i32;
    let mut i: i32 = 0;
    while i < count {
        let triangle = &its.indices[i as usize];
        let neighbors = &face_neighbors[i as usize];
        for j in 0..3_usize {
            let n = neighbors[j];
            if n > i {
                let triangle2 = &its.indices[n as usize];
                let edge_id = last_edge_id;
                last_edge_id += 1;
                let mut edge = its_triangle_edge(triangle, j);
                // First find an edge with opposite orientation.
                edge = Vec2i32::new(edge[1], edge[0]);
                let mut k = its_triangle_edge_index(triangle2, &edge);
                if k == -1 {
                    // Second find an edge with the same orientation (the neighbor triangle may be
                    // flipped).
                    edge = Vec2i32::new(edge[1], edge[0]);
                    k = its_triangle_edge_index(triangle2, &edge);
                }
                out[i as usize * 3 + j] = edge_id;
                // k is -1 when the neighbour holds neither edge: the C++ writes the int before
                // the neighbour's triple (n > i >= 0, so it lies inside the vector).
                out[(n as isize * 3 + k as isize) as usize] = edge_id;
            } else if n == -1 {
                out[i as usize * 3 + j] = if assign_unbound_edges {
                    let id = last_edge_id;
                    last_edge_id += 1;
                    id
                } else {
                    -1
                };
            } else {
                // Triangle shall never be neighbor of itself. Don't do anything, the neighbor
                // will assign us an edge ID in later iterations.
            }
        }
        i += 1;
    }
    if let Some(num_edges) = num_edges {
        *num_edges = last_edge_id;
    }
    out.chunks_exact(3)
        .map(|c| Vec3i32::new(c[0], c[1], c[2]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::its::its_face_neighbors;
    use orca_eigen::Vec3f;

    /// Two triangles sharing the edge 1-2, wound consistently.
    fn two_faces() -> IndexedTriangleSet {
        IndexedTriangleSet {
            indices: vec![Vec3i32::new(0, 1, 2), Vec3i32::new(2, 1, 3)],
            vertices: vec![
                Vec3f::new(0.0, 0.0, 0.0),
                Vec3f::new(1.0, 0.0, 0.0),
                Vec3f::new(0.0, 1.0, 0.0),
                Vec3f::new(1.0, 1.0, 0.0),
            ],
            properties: Vec::new(),
        }
    }

    #[test]
    fn the_shared_edge_gets_one_id() {
        let its = two_faces();
        let e = its_face_edge_ids(&its);
        // Sorted edges: (0,1) f0, (0,2) f0, (1,2) f0 and f1, (1,3) f1, (2,3) f1.
        assert_eq!(e[0], Vec3i32::new(0, 2, 1));
        assert_eq!(e[1], Vec3i32::new(2, 3, 4));
    }

    #[test]
    fn a_masked_face_keeps_minus_one() {
        let its = two_faces();
        let e = its_face_edge_ids_masked(&its, &[true, false]);
        assert_eq!(e[1], Vec3i32::new(-1, -1, -1));
        assert_eq!(e[0], Vec3i32::new(0, 2, 1));
    }

    #[test]
    fn neighbours_give_ids_in_face_order() {
        let its = two_faces();
        let nb = its_face_neighbors(&its);
        let mut n = -7;
        let e = its_face_edge_ids_from_neighbors(&its, &nb, false, Some(&mut n));
        assert_eq!(n, 1);
        assert_eq!(e[0], Vec3i32::new(-1, 0, -1));
        assert_eq!(e[1], Vec3i32::new(0, -1, -1));
        let e = its_face_edge_ids_from_neighbors(&its, &nb, true, Some(&mut n));
        assert_eq!(n, 5);
        assert_eq!(e[0], Vec3i32::new(0, 1, 2));
        assert_eq!(e[1], Vec3i32::new(1, 3, 4));
    }
}
