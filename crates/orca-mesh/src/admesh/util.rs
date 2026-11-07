// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries ADMesh code: deps_src/admesh/util.cpp of OrcaSlicer v2.4.2 (stl_verify_neighbors, the
// translations, scaling, rotations and mirrors, stl_get_size, get_area, get_volume,
// stl_calculate_volume, stl_repair) and stl_write_facet and stl_write_vertex of stl_io.cpp,
// translated on 2026-11-07. Copyright (C) 1995, 1996 Anthony D. Martin and (C) 2013, 2014 the
// ADMesh contributors, with changes by the PrusaSlicer and OrcaSlicer authors, under the GNU
// General Public License, version 2 or later (GPL-2.0-or-later; licenses/GPL-2.0.txt), taken here
// under version 3 (licenses/GPL-3.0.txt) as section 13 of the AGPL-3.0 allows.

//! The checks, measures and transforms of `util.cpp`.
//!
//! `stl_verify_neighbors` counts the edges joined backwards and, for a joined edge whose
//! vertices do not match, logs it and prints both facets on C's stdout (`stl_write_facet`:
//! `"facet (%d)/ %s\n"` and three `"  vertex %d/%d % .8E % .8E % .8E\n"` lines), through
//! `orca-fmt`'s `printf` and `stdio::stdout_write`, as the engine prints them.
//!
//! The volume is the C++'s: per facet, the area from the `double` cross products of the vertex
//! pairs summed into a `float` vector and dotted with the facet's unit normal, times the height of
//! its first vertex over the first facet's first vertex, over 3, summed in `float`. With no facets
//! it is 0 (the C++ reads the first facet's vertex all the same, which matters only then).

use orca_eigen::Vec3f;
use orca_fmt::printf::{Arg, sprintf};
use orca_fmt::stdio::stdout_write;

use super::connect::{
    stl_check_facets_exact, stl_check_facets_nearby, stl_fill_holes, stl_remove_unconnected_facets,
};
use super::normals::{stl_fix_normal_directions, stl_fix_normal_values, stl_reverse_all_facets};
use super::{
    IndexedTriangleSet, StlFacet, StlFile, StlVertex, stl_calculate_normal, stl_normalize_vector,
};
use crate::log::{Severity, log};

/// `M_PI`.
const M_PI: f64 = core::f64::consts::PI;

/// `stl_write_vertex(stl, facet, vertex)`.
fn stl_write_vertex(stl: &StlFile, facet: i32, vertex: i32) {
    let v = stl.facet_start[facet as usize].vertex[vertex as usize];
    let line = sprintf(
        b"  vertex %d/%d % .8E % .8E % .8E\n",
        &[
            Arg::Int(i64::from(vertex)),
            Arg::Int(i64::from(facet)),
            Arg::Double(f64::from(v[0])),
            Arg::Double(f64::from(v[1])),
            Arg::Double(f64::from(v[2])),
        ],
    )
    .expect("a valid format");
    stdout_write(&line);
}

/// `stl_write_facet(stl, label, facet)`: the facet and its vertices on C's stdout.
pub fn stl_write_facet(stl: &StlFile, label: &str, facet: i32) {
    let line = sprintf(
        b"facet (%d)/ %s\n",
        &[Arg::Int(i64::from(facet)), Arg::Str(label.as_bytes())],
    )
    .expect("a valid format");
    stdout_write(&line);
    for vertex in 0..3 {
        stl_write_vertex(stl, facet, vertex);
    }
}

/// `stl_verify_neighbors`: counts the backwards edges and reports the joined edges whose vertices
/// differ.
pub fn stl_verify_neighbors(stl: &mut StlFile) {
    stl.stats.backwards_edges = 0;
    for i in 0..stl.facets() {
        for j in 0..3usize {
            let p1 = stl.facet_start[i].vertex[j];
            let p2 = stl.facet_start[i].vertex[(j + 1) % 3];
            let neighbor = stl.neighbors_start[i].neighbor[j];
            if neighbor == -1 {
                continue;
            }
            let vnot = i32::from(stl.neighbors_start[i].which_vertex_not[j]);
            let other = &stl.facet_start[neighbor as usize];
            let (q1, q2) = if vnot < 3 {
                (
                    other.vertex[((vnot + 2) % 3) as usize],
                    other.vertex[((vnot + 1) % 3) as usize],
                )
            } else {
                stl.stats.backwards_edges += 1;
                (
                    other.vertex[((vnot + 1) % 3) as usize],
                    other.vertex[((vnot + 2) % 3) as usize],
                )
            };
            if p1 != q1 || p2 != q2 {
                log(
                    Severity::Info,
                    &format!(
                        "edge {j} of facet {i} doesn't match edge {} of facet {neighbor}",
                        vnot + 1
                    ),
                );
                stl_write_facet(stl, "first facet", i as i32);
                stl_write_facet(stl, "second facet", neighbor);
            }
        }
    }
}

/// `stl_translate(stl, x, y, z)`: moves the minimum to `(x, y, z)`.
pub fn stl_translate(stl: &mut StlFile, x: f32, y: f32, z: f32) {
    let new_min = Vec3f::new(x, y, z);
    let shift = new_min - stl.stats.min;
    for i in 0..stl.facets() {
        for v in &mut stl.facet_start[i].vertex {
            *v += shift;
        }
    }
    stl.stats.min = new_min;
    stl.stats.max += shift;
}

/// `stl_translate_relative(stl, x, y, z)`.
pub fn stl_translate_relative(stl: &mut StlFile, x: f32, y: f32, z: f32) {
    let shift = Vec3f::new(x, y, z);
    for i in 0..stl.facets() {
        for v in &mut stl.facet_start[i].vertex {
            *v += shift;
        }
    }
    stl.stats.min += shift;
    stl.stats.max += shift;
}

/// `stl_scale_versor(stl, versor)`.
pub fn stl_scale_versor(stl: &mut StlFile, versor: &Vec3f) {
    stl.stats.min = stl.stats.min.cwise_product(versor);
    stl.stats.max = stl.stats.max.cwise_product(versor);
    stl.stats.size = stl.stats.size.cwise_product(versor);
    if f64::from(stl.stats.volume) > 0.0 {
        stl.stats.volume *= versor[0] * versor[1] * versor[2];
    }
    for i in 0..stl.facets() {
        for v in &mut stl.facet_start[i].vertex {
            *v = v.cwise_product(versor);
        }
    }
}

/// `stl_scale(stl, factor)`.
pub fn stl_scale(stl: &mut StlFile, factor: f32) {
    stl_scale_versor(stl, &Vec3f::new(factor, factor, factor));
}

/// `calculate_normals`.
fn calculate_normals(stl: &mut StlFile) {
    for i in 0..stl.facets() {
        let normal = stl_normalize_vector(stl_calculate_normal(&stl.facet_start[i]));
        stl.facet_start[i].normal = normal;
    }
}

/// `rotate_point_2d(x, y, c, s)`: in `double`, each result rounded to `float`.
fn rotate_point_2d(x: &mut f32, y: &mut f32, c: f64, s: f64) {
    let xold = f64::from(*x);
    let yold = f64::from(*y);
    *x = (c * xold - s * yold) as f32;
    *y = (s * xold + c * yold) as f32;
}

/// The cosine and sine of `angle` degrees: `(angle / 180.0) * M_PI` through the UCRT.
fn cos_sin_degrees(angle: f32) -> (f64, f64) {
    let radian_angle = (f64::from(angle) / 180.0) * M_PI;
    (orca_crt::cos(radian_angle), orca_crt::sin(radian_angle))
}

/// Rotates coordinates `a` and `b` of every vertex of the facets.
fn stl_rotate(stl: &mut StlFile, angle: f32, a: usize, b: usize) {
    let (c, s) = cos_sin_degrees(angle);
    for i in 0..stl.facets() {
        for v in &mut stl.facet_start[i].vertex {
            let (mut x, mut y) = (v[a], v[b]);
            rotate_point_2d(&mut x, &mut y, c, s);
            v[a] = x;
            v[b] = y;
        }
    }
    stl_get_size(stl);
    calculate_normals(stl);
}

/// `stl_rotate_x(stl, angle)`: degrees.
pub fn stl_rotate_x(stl: &mut StlFile, angle: f32) {
    stl_rotate(stl, angle, 1, 2);
}

/// `stl_rotate_y(stl, angle)`.
pub fn stl_rotate_y(stl: &mut StlFile, angle: f32) {
    stl_rotate(stl, angle, 2, 0);
}

/// `stl_rotate_z(stl, angle)`.
pub fn stl_rotate_z(stl: &mut StlFile, angle: f32) {
    stl_rotate(stl, angle, 0, 1);
}

/// Rotates coordinates `a` and `b` of every vertex of the set.
fn its_rotate(its: &mut IndexedTriangleSet, angle: f32, a: usize, b: usize) {
    let (c, s) = cos_sin_degrees(angle);
    for v in &mut its.vertices {
        let (mut x, mut y) = (v[a], v[b]);
        rotate_point_2d(&mut x, &mut y, c, s);
        v[a] = x;
        v[b] = y;
    }
}

/// `its_rotate_x(its, angle)`: degrees.
pub fn its_rotate_x(its: &mut IndexedTriangleSet, angle: f32) {
    its_rotate(its, angle, 1, 2);
}

/// `its_rotate_y(its, angle)`.
pub fn its_rotate_y(its: &mut IndexedTriangleSet, angle: f32) {
    its_rotate(its, angle, 2, 0);
}

/// `its_rotate_z(its, angle)`.
pub fn its_rotate_z(its: &mut IndexedTriangleSet, angle: f32) {
    its_rotate(its, angle, 0, 1);
}

/// `stl_get_size`: the bounding box of the facets (nothing with none).
pub fn stl_get_size(stl: &mut StlFile) {
    if stl.stats.number_of_facets == 0 {
        return;
    }
    stl.stats.min = stl.facet_start[0].vertex[0];
    stl.stats.max = stl.stats.min;
    for i in 0..stl.facets() {
        let face = stl.facet_start[i];
        for v in &face.vertex {
            stl.stats.min = stl.stats.min.cwise_min(v);
            stl.stats.max = stl.stats.max.cwise_max(v);
        }
    }
    stl.stats.size = stl.stats.max - stl.stats.min;
    stl.stats.bounding_diameter = stl.stats.size.norm();
}

/// `float *= -1.0`: the product in `double`, rounded back ([`super::times_minus_one`]).
fn negate(x: f32) -> f32 {
    super::times_minus_one(x)
}

/// The mirrors: coordinate `axis` negated, its extremes swapped, the facets reversed with the
/// count of reversals kept.
fn stl_mirror(stl: &mut StlFile, axis: usize) {
    for i in 0..stl.facets() {
        for v in &mut stl.facet_start[i].vertex {
            v[axis] = negate(v[axis]);
        }
    }
    std::mem::swap(&mut stl.stats.min[axis], &mut stl.stats.max[axis]);
    stl.stats.min[axis] = negate(stl.stats.min[axis]);
    stl.stats.max[axis] = negate(stl.stats.max[axis]);
    stl_reverse_all_facets(stl);
    // `int -= uint32_t`: an unsigned subtraction, wrapped back into the `int`.
    stl.stats.facets_reversed = stl
        .stats
        .facets_reversed
        .wrapping_sub(stl.stats.number_of_facets as i32);
}

/// `stl_mirror_xy`: z negated.
pub fn stl_mirror_xy(stl: &mut StlFile) {
    stl_mirror(stl, 2);
}

/// `stl_mirror_yz`: x negated.
pub fn stl_mirror_yz(stl: &mut StlFile) {
    stl_mirror(stl, 0);
}

/// `stl_mirror_xz`: y negated.
pub fn stl_mirror_xz(stl: &mut StlFile) {
    stl_mirror(stl, 1);
}

/// `get_area(facet)`: see the module documentation.
#[must_use]
pub fn get_area(facet: &StlFacet) -> f32 {
    let v = |i: usize, k: usize| f64::from(facet.vertex[i][k]);
    let mut cross = [[0f64; 3]; 3];
    for (i, row) in cross.iter_mut().enumerate() {
        let n = (i + 1) % 3;
        row[0] = v(i, 1) * v(n, 2) - v(i, 2) * v(n, 1);
        row[1] = v(i, 2) * v(n, 0) - v(i, 0) * v(n, 2);
        row[2] = v(i, 0) * v(n, 1) - v(i, 1) * v(n, 0);
    }
    let sum = Vec3f::new(
        (cross[0][0] + cross[1][0] + cross[2][0]) as f32,
        (cross[0][1] + cross[1][1] + cross[2][1]) as f32,
        (cross[0][2] + cross[1][2] + cross[2][2]) as f32,
    );
    let n = stl_normalize_vector(stl_calculate_normal(facet));
    0.5f32 * n.dot(&sum)
}

/// `get_volume(stl)`: see the module documentation.
fn get_volume(stl: &StlFile) -> f32 {
    if stl.stats.number_of_facets == 0 {
        return 0.0;
    }
    let p0: StlVertex = stl.facet_start[0].vertex[0];
    let mut volume = 0f32;
    for i in 0..stl.facets() {
        let facet = &stl.facet_start[i];
        let height = facet.normal.dot(&(facet.vertex[0] - p0));
        let area = get_area(facet);
        volume += (area * height) / 3.0f32;
    }
    volume
}

/// `stl_calculate_volume`: the volume, made positive by reversing every facet when it is not.
pub fn stl_calculate_volume(stl: &mut StlFile) {
    stl.stats.volume = get_volume(stl);
    if f64::from(stl.stats.volume) < 0.0 {
        stl_reverse_all_facets(stl);
        stl.stats.volume = -stl.stats.volume;
    }
}

/// The flags and parameters of `stl_repair`.
#[derive(Clone, Copy, Debug, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct RepairFlags {
    /// `fixall_flag`.
    pub fixall: bool,
    /// `exact_flag`.
    pub exact: bool,
    /// `tolerance_flag`.
    pub tolerance_flag: bool,
    /// `tolerance`.
    pub tolerance: f32,
    /// `increment_flag`.
    pub increment_flag: bool,
    /// `increment`.
    pub increment: f32,
    /// `nearby_flag`.
    pub nearby: bool,
    /// `iterations`.
    pub iterations: i32,
    /// `remove_unconnected_flag`.
    pub remove_unconnected: bool,
    /// `fill_holes_flag`.
    pub fill_holes: bool,
    /// `normal_directions_flag`.
    pub normal_directions: bool,
    /// `normal_values_flag`.
    pub normal_values: bool,
    /// `reverse_all_flag`.
    pub reverse_all: bool,
    /// `verbose_flag`.
    pub verbose: bool,
}

/// Prints a `printf` line of `stl_repair`'s verbose mode on C's stdout.
fn verbose_print(format: &[u8], args: &[Arg<'_>]) {
    stdout_write(&sprintf(format, args).expect("a valid format"));
}

/// `stl_repair(stl, ...)`: admesh's own repair sequence (libslic3r runs its own,
/// `crate::triangle_mesh::trianglemesh_repair_on_import`).
pub fn stl_repair(stl: &mut StlFile, flags: &RepairFlags) {
    let mut exact = flags.exact;
    let mut tolerance = flags.tolerance;
    let mut increment = flags.increment;
    if flags.exact
        || flags.fixall
        || flags.nearby
        || flags.remove_unconnected
        || flags.fill_holes
        || flags.normal_directions
    {
        if flags.verbose {
            verbose_print(b"Checking exact...\n", &[]);
        }
        exact = true;
        stl_check_facets_exact(stl);
        stl.stats.facets_w_1_bad_edge =
            stl.stats.connected_facets_2_edge - stl.stats.connected_facets_3_edge;
        stl.stats.facets_w_2_bad_edge =
            stl.stats.connected_facets_1_edge - stl.stats.connected_facets_2_edge;
        stl.stats.facets_w_3_bad_edge =
            (stl.stats.number_of_facets as i32).wrapping_sub(stl.stats.connected_facets_1_edge);
    }
    if flags.nearby || flags.fixall {
        if !flags.tolerance_flag {
            tolerance = stl.stats.shortest_edge;
        }
        if !flags.increment_flag {
            increment = (f64::from(stl.stats.bounding_diameter) / 10000.0) as f32;
        }
    }
    if stl.stats.connected_facets_3_edge < stl.stats.number_of_facets as i32 {
        let mut last_edges_fixed = 0;
        for i in 0..flags.iterations {
            if stl.stats.connected_facets_3_edge < stl.stats.number_of_facets as i32 {
                if flags.verbose {
                    verbose_print(
                        b"Checking nearby. Tolerance= %f Iteration=%d of %d...",
                        &[
                            Arg::Double(f64::from(tolerance)),
                            Arg::Int(i64::from(i + 1)),
                            Arg::Int(i64::from(flags.iterations)),
                        ],
                    );
                }
                stl_check_facets_nearby(stl, tolerance);
                if flags.verbose {
                    verbose_print(
                        b"  Fixed %d edges.\n",
                        &[Arg::Int(i64::from(
                            stl.stats.edges_fixed - last_edges_fixed,
                        ))],
                    );
                }
                last_edges_fixed = stl.stats.edges_fixed;
                tolerance += increment;
            } else {
                if flags.verbose {
                    verbose_print(
                        b"All facets connected.  No further nearby check necessary.\n",
                        &[],
                    );
                }
                break;
            }
        }
    } else if flags.verbose {
        verbose_print(b"All facets connected.  No nearby check necessary.\n", &[]);
    }
    if flags.remove_unconnected || flags.fixall || flags.fill_holes {
        if stl.stats.connected_facets_3_edge < stl.stats.number_of_facets as i32 {
            if flags.verbose {
                verbose_print(b"Removing unconnected facets...\n", &[]);
            }
            stl_remove_unconnected_facets(stl);
        } else if flags.verbose {
            verbose_print(b"No unconnected need to be removed.\n", &[]);
        }
    }
    if flags.fill_holes || flags.fixall {
        if stl.stats.connected_facets_3_edge < stl.stats.number_of_facets as i32 {
            if flags.verbose {
                verbose_print(b"Filling holes...\n", &[]);
            }
            stl_fill_holes(stl);
        } else if flags.verbose {
            verbose_print(b"No holes need to be filled.\n", &[]);
        }
    }
    if flags.reverse_all {
        if flags.verbose {
            verbose_print(b"Reversing all facets...\n", &[]);
        }
        stl_reverse_all_facets(stl);
    }
    if flags.normal_directions || flags.fixall {
        if flags.verbose {
            verbose_print(b"Checking normal directions...\n", &[]);
        }
        stl_fix_normal_directions(stl);
    }
    if flags.normal_values || flags.fixall {
        if flags.verbose {
            verbose_print(b"Checking normal values...\n", &[]);
        }
        stl_fix_normal_values(stl);
    }
    if flags.verbose {
        verbose_print(b"Calculating volume...\n", &[]);
    }
    stl_calculate_volume(stl);
    if exact {
        if flags.verbose {
            verbose_print(b"Verifying neighbors...\n", &[]);
        }
        stl_verify_neighbors(stl);
    }
}
