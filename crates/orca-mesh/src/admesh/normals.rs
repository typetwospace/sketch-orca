// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries ADMesh code: deps_src/admesh/normals.cpp of OrcaSlicer v2.4.2 (reverse_facet,
// check_normal_vector, stl_fix_normal_directions, stl_fix_normal_values, stl_reverse_all_facets),
// translated on 2026-11-07. Copyright (C) 1995, 1996 Anthony D. Martin and (C) 2013, 2014 the
// ADMesh contributors, with changes by the PrusaSlicer and OrcaSlicer authors, under the GNU
// General Public License, version 2 or later (GPL-2.0-or-later; licenses/GPL-2.0.txt), taken here
// under version 3 (licenses/GPL-3.0.txt) as section 13 of the AGPL-3.0 allows.

//! The facets' orientation and normals.
//!
//! `stl_fix_normal_directions` starts at facet 0, reversed if its stored normal points against
//! the one its vertices give, and walks each connected part depth first (a stack, the neighbours
//! pushed in edge order), reversing each neighbour joined backwards. Reaching a facet already
//! fixed through a backwards edge undoes every reversal made so far, in reverse order, and stops
//! (the C++ comment lists the admesh issues this fixes). Each part found counts in
//! `number_of_parts`.
//!
//! The walk marks facets in an array of `number_of_facets` bytes; a neighbour index past it (left
//! by `stl_remove_unconnected_facets` stopping early) is read and written beyond the C++ array's
//! end. Here the array covers the facet arrays' whole length and those entries read as unfixed
//! (`parity/HAZARDS.md`, orca-mesh).

use super::{StlFile, stl_calculate_normal, stl_normalize_vector};

/// `reverse_facet(stl, facet_num)`: swaps vertices 0 and 1 and fixes the neighbour entries on
/// both sides (`which_vertex_not` worked in `int` and stored back in a `char`).
pub(crate) fn reverse_facet(stl: &mut StlFile, facet_num: usize) {
    stl.stats.facets_reversed += 1;
    let n = stl.neighbors_start[facet_num];
    let neighbor = n.neighbor;
    let vnot = [
        i32::from(n.which_vertex_not[0]),
        i32::from(n.which_vertex_not[1]),
        i32::from(n.which_vertex_not[2]),
    ];

    stl.facet_start[facet_num].vertex.swap(0, 1);

    for (k, add) in [(0usize, 3i32), (1, 4), (2, 2)] {
        if neighbor[k] != -1 {
            let slot = ((vnot[k] + 1) % 3) as usize;
            let other = &mut stl.neighbors_start[neighbor[k] as usize];
            other.which_vertex_not[slot] =
                ((i32::from(other.which_vertex_not[slot]) + add) % 6) as i8;
        }
    }

    let own = &mut stl.neighbors_start[facet_num];
    own.neighbor[1] = neighbor[2];
    own.neighbor[2] = neighbor[1];
    own.which_vertex_not[1] = vnot[2] as i8;
    own.which_vertex_not[2] = vnot[1] as i8;
    for k in 0..3 {
        own.which_vertex_not[k] = ((i32::from(own.which_vertex_not[k]) + 3) % 6) as i8;
    }
}

/// `check_normal_vector(stl, facet_num, normal_fix_flag)`: whether the stored normal points
/// against the computed one; with the flag, the computed one replaces a stored one off by 0.001
/// or more in a coordinate.
pub(crate) fn check_normal_vector(
    stl: &mut StlFile,
    facet_num: usize,
    normal_fix_flag: bool,
) -> bool {
    let facet = stl.facet_start[facet_num];
    let normal = stl_normalize_vector(stl_calculate_normal(&facet));
    let eps = 0.001f32;
    let within = |d: orca_eigen::Vec3f| d[0] < eps && d[1] < eps && d[2] < eps;

    let normal_dif = (normal - facet.normal).cwise_abs();
    if within(normal_dif) {
        stl.facet_start[facet_num].normal = normal;
        return false;
    }

    let mut test_norm = stl_normalize_vector(facet.normal);
    let normal_dif = (normal - test_norm).cwise_abs();
    if within(normal_dif) {
        if normal_fix_flag {
            stl.facet_start[facet_num].normal = normal;
            stl.stats.normals_fixed += 1;
        }
        return false;
    }

    test_norm *= -1.0f32;
    let normal_dif = (normal - test_norm).cwise_abs();
    if within(normal_dif) {
        if normal_fix_flag {
            stl.facet_start[facet_num].normal = normal;
            stl.stats.normals_fixed += 1;
        }
        return true;
    }

    if normal_fix_flag {
        stl.facet_start[facet_num].normal = normal;
        stl.stats.normals_fixed += 1;
    }
    false
}

/// `stl_fix_normal_directions`: orients the facets of each connected part alike (see the module
/// documentation).
pub fn stl_fix_normal_directions(stl: &mut StlFile) {
    if stl.stats.number_of_facets == 0 {
        return;
    }
    let n = stl.facets();
    // The pool's list: pushed and popped at its head, so a stack.
    let mut list: Vec<i32> = Vec::new();
    let mut norm_sw = vec![0u8; orca_stl::max(stl.facet_start.len(), n)];
    let mut reversed_ids: Vec<usize> = Vec::with_capacity(n);

    let mut facet_num = 0usize;
    if check_normal_vector(stl, 0, false) {
        reverse_facet(stl, 0);
        reversed_ids.push(0);
    }
    norm_sw[facet_num] = 1;
    let mut checked: i32 = 1;

    loop {
        let mut force_exit = false;
        for j in 0..3 {
            // The neighbour is read afresh at each use, as the C++ reads it.
            if stl.neighbors_start[facet_num].which_vertex_not[j] > 2
                && stl.neighbors_start[facet_num].neighbor[j] != -1
            {
                if norm_sw[stl.neighbors_start[facet_num].neighbor[j] as usize] == 1 {
                    for &id in reversed_ids.iter().rev() {
                        reverse_facet(stl, id);
                    }
                    force_exit = true;
                    break;
                }
                reverse_facet(stl, stl.neighbors_start[facet_num].neighbor[j] as usize);
                reversed_ids.push(stl.neighbors_start[facet_num].neighbor[j] as usize);
            }
            let neighbor = stl.neighbors_start[facet_num].neighbor[j];
            if neighbor != -1 && norm_sw[neighbor as usize] != 1 {
                list.push(neighbor);
            }
        }
        if force_exit {
            break;
        }

        if let Some(next) = list.pop() {
            facet_num = next as usize;
            if norm_sw[facet_num] != 1 {
                norm_sw[facet_num] = 1;
                checked += 1;
            }
        } else {
            stl.stats.number_of_parts += 1;
            if checked >= stl.stats.number_of_facets as i32 {
                break;
            }
            for i in 0..n {
                if norm_sw[i] == 0 {
                    facet_num = i;
                    if check_normal_vector(stl, i, false) {
                        reverse_facet(stl, i);
                        reversed_ids.push(i);
                    }
                    norm_sw[facet_num] = 1;
                    checked += 1;
                    break;
                }
            }
        }
    }
}

/// `stl_fix_normal_values`: `check_normal_vector(stl, i, 1)` for every facet.
pub fn stl_fix_normal_values(stl: &mut StlFile) {
    for i in 0..stl.facets() {
        check_normal_vector(stl, i, true);
    }
}

/// `stl_reverse_all_facets`: reverses every facet and recomputes its normal.
pub fn stl_reverse_all_facets(stl: &mut StlFile) {
    for i in 0..stl.facets() {
        reverse_facet(stl, i);
        let normal = stl_normalize_vector(stl_calculate_normal(&stl.facet_start[i]));
        stl.facet_start[i].normal = normal;
    }
}
