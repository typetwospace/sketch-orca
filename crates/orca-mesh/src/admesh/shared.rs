// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries ADMesh code: stl_generate_shared_vertices of deps_src/admesh/shared.cpp of OrcaSlicer
// v2.4.2, translated on 2026-11-07. Copyright (C) 1995, 1996 Anthony D. Martin and (C) 2013, 2014
// the ADMesh contributors, with changes by the PrusaSlicer and OrcaSlicer authors, under the GNU
// General Public License, version 2 or later (GPL-2.0-or-later; licenses/GPL-2.0.txt), taken here
// under version 3 (licenses/GPL-3.0.txt) as section 13 of the AGPL-3.0 allows.

//! `stl_generate_shared_vertices`: facets to an indexed triangle set.
//!
//! The facets are visited in order, and each vertex not yet given an index becomes a new shared
//! vertex, which every facet of its fan takes: the walk follows the neighbours around the vertex
//! one way, then from the start the other way, and stops at an open edge, back at the start, at a
//! neighbour index past the facet count or at a facet already visited in this walk (a stamp per
//! walk), as the C++ does on broken meshes.

use super::{IndexedTriangleSet, StlFile, StlTriangleVertexIndices};

/// `stl_generate_shared_vertices(stl, its)`: `its.indices` and `its.vertices` rebuilt from the
/// facets; `its.properties` is left as it was.
pub fn stl_generate_shared_vertices(stl: &StlFile, its: &mut IndexedTriangleSet) {
    let n = stl.facets();
    its.indices.clear();
    its.indices
        .resize(n, StlTriangleVertexIndices::new(-1, -1, -1));
    its.vertices.clear();

    let mut fan_traversal_stamp: u32 = 0;
    let mut fan_traversal_facet_visited = vec![0u32; n];

    for facet_idx in 0..n {
        for j in 0..3i32 {
            if its.indices[facet_idx][j as usize] != -1 {
                continue;
            }
            its.vertices
                .push(stl.facet_start[facet_idx].vertex[j as usize]);
            let mut facet_in_fan_idx = facet_idx as i32;
            let mut edge_direction = false;
            let mut traversal_reversed = false;
            let mut vnot = (j + 2) % 3;
            fan_traversal_stamp = fan_traversal_stamp.wrapping_add(1);
            loop {
                let pivot_vertex;
                let next_edge;
                if vnot > 2 {
                    if edge_direction {
                        pivot_vertex = (vnot + 1) % 3;
                        next_edge = vnot % 3;
                    } else {
                        pivot_vertex = (vnot + 2) % 3;
                        next_edge = pivot_vertex;
                    }
                    edge_direction = !edge_direction;
                } else if edge_direction {
                    pivot_vertex = (vnot + 2) % 3;
                    next_edge = pivot_vertex;
                } else {
                    pivot_vertex = (vnot + 1) % 3;
                    next_edge = vnot;
                }
                let fan = facet_in_fan_idx as usize;
                its.indices[fan][pivot_vertex as usize] = (its.vertices.len() - 1) as i32;
                fan_traversal_facet_visited[fan] = fan_traversal_stamp;
                let next_facet = stl.neighbors_start[fan].neighbor[next_edge as usize];
                if next_facet == -1 {
                    if traversal_reversed {
                        break;
                    }
                    edge_direction = true;
                    vnot = (j + 1) % 3;
                    traversal_reversed = true;
                    facet_in_fan_idx = facet_idx as i32;
                } else if next_facet as u32 == facet_idx as u32
                    || next_facet >= n as i32
                    || fan_traversal_facet_visited[next_facet as usize] == fan_traversal_stamp
                {
                    // Back at the start, past the facets, or a facet seen in this walk.
                    break;
                } else {
                    vnot = i32::from(stl.neighbors_start[fan].which_vertex_not[next_edge as usize]);
                    facet_in_fan_idx = next_facet;
                }
            }
        }
    }
}
