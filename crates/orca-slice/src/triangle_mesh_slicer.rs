// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/TriangleMeshSlicer.hpp lines 1-136 (MeshSlicingParams and
// MeshSlicingParamsEx; the declarations of slice_mesh, slice_mesh_ex, slice_mesh_slabs,
// project_mesh and cut_mesh go with their bodies) and src/libslic3r/TriangleMeshSlicer.cpp lines
// 1-1057, with next_idx_modulo of src/libslic3r/libslic3r.h, of OrcaSlicer v2.4.2, translated on
// 2026-11-13 by orca-o3-slice-1. Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu
// Lab), the PrusaSlicer authors (Prusa Research) and the Slic3r authors (Alessandro Ranellucci),
// under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! The first half of libslic3r's mesh slicer (`TriangleMeshSlicer`): the slicing parameters, the
//! intersection lines a plane cuts out of a triangle, and the lines of a whole mesh at a list of
//! heights, at one height, and on the slabs between heights. The second half (orca-o3-slice-2)
//! chains these lines into loops by triangle connectivity and makes the polygons and expolygons
//! of `slice_mesh`, `slice_mesh_ex` and `slice_mesh_slabs`.
//!
//! - [`slice_facet`] cuts one triangle with a plane: the vertices are taken from the lowest one
//!   on, each edge either lies on the plane (a horizontal edge: [`FacetEdgeType::Top`] when the
//!   third vertex is below, reversed, else [`FacetEdgeType::Bottom`]; the bottom-most edge of a
//!   triangle is not its own), touches it at a vertex, or crosses it at
//!   `floor(b + (a - b) t + 0.5)` (the edge's ends ordered by vertex id, so both faces of an edge
//!   give the same point). A vertex `x` or `y` becomes a `coord_t` by truncation, as the C++'s
//!   assignment of a `float` to a `coord_t` does (`orca_crt::f32_to_i64`).
//! - [`slice_facet_at_zs`] finds the heights a face spans (`std::lower_bound` and
//!   `std::upper_bound` through orca-stl, its extent through the UCRT's `fminf` and `fmaxf`) and
//!   cuts it at each, skipping horizontal faces.
//! - [`slice_make_lines`] runs that over every face in a `tbb::parallel_for` (orca-tbb, the auto
//!   partitioner over `blocked_range<int>`), and [`slice_make_lines_plane`] over the faces a filter
//!   keeps, at one height, on the calling thread.
//! - [`slice_facet_with_slabs`] and [`slice_slabs_make_lines`] make the lines of
//!   `slice_mesh_slabs`: the lines on each plane and the open edges projected into the slab below
//!   (from the top) or above (from the bottom), the edges at the top plane numbered past
//!   `num_edges` so that the chaining can tell the two planes of a slab apart.
//!
//! The C++ pushes each line under one of 64 mutexes, so its order in a layer is the order the
//! threads took them; under the oracle's one thread (`parity/sop1/slice.md`) that is the order of
//! the faces, each face's lines in the order it makes them. Here each leaf of the loop keeps its
//! own lines, and the leaves are joined in the order of their first face once the loop is done, so
//! a layer's lines come in the order of the faces however the threads ran (the C++'s order of
//! results under the oracle, the same in a serial and a parallel arena). The C++'s
//! `throw_on_cancel` is a closure called at the same faces; its panic is the C++'s exception,
//! which orca-tbb carries out of the loop.
//!
//! Left out, since no piece carries the C++ that calls them (`cut_mesh` and `triangulate_slice`,
//! TriangleMeshSlicer.cpp 2161-2420, which only the cut tool reaches, `CutUtils.cpp`, and no job
//! does): `is_equal` of two floats and of two `Vec3f`, `is_equal_for_sort` (lines 44-56) and
//! `slice_facet_for_cut_mesh` (lines 322-473). `remove_tangent_edges` (lines 977-1041) is
//! compiled out (`#if 0`), and so are the debug-only `IntersectionLine::source` and the asserts.

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

use orca_crt::{f32_to_i64, f64_to_i64, floor, fmaxf, fminf};
use orca_eigen::{Transform3d, Vec3f, Vec3i32};
use orca_geom::line::Line;
use orca_geom::multi_point::length;
use orca_geom::point::{Point, Points};
use orca_mesh::admesh::{StlTriangleVertexIndices, StlVertex};
use orca_stl::{lower_bound, upper_bound};
use orca_tbb::{BlockedRange, Partitioner, parallel_for};

/// `MeshSlicingParams::SlicingMode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u32)]
pub enum SlicingMode {
    /// Regular slicing, maintain all contours and their orientation. `slice_mesh_ex()` applies
    /// ClipperLib's `pftNonZero` rule to the result of `slice_mesh()`.
    #[default]
    Regular = 0,
    /// For slicing 3DLabPrints plane models (aka to be compatible with S3D default strategy).
    /// `slice_mesh_ex()` applies ClipperLib's `pftEvenOdd` rule. `slice_mesh()` slices `EvenOdd`
    /// as `Regular`.
    EvenOdd = 1,
    /// Maintain all contours, orient all contours CCW. `slice_mesh_ex()` applies ClipperLib's
    /// `pftNonZero` rule, thus holes will be closed.
    Positive = 2,
    /// Orient all contours CCW and keep only the contour with the largest area. This mode is
    /// useful for slicing complex objects in vase mode.
    PositiveLargestContour = 3,
}

impl SlicingMode {
    /// The mode of its `uint32_t` value, or `None` for a value the enum does not declare.
    #[must_use]
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::Regular),
            1 => Some(Self::EvenOdd),
            2 => Some(Self::Positive),
            3 => Some(Self::PositiveLargestContour),
            _ => None,
        }
    }
}

/// `MeshSlicingParams`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshSlicingParams {
    /// `mode`.
    pub mode: SlicingMode,
    /// `slicing_mode_normal_below_layer`: for vase mode, below this layer a different slicing
    /// mode is used to produce a single contour; 0 ignores it.
    pub slicing_mode_normal_below_layer: usize,
    /// `mode_below`: the mode below `slicing_mode_normal_below_layer`, ignored when that is 0.
    pub mode_below: SlicingMode,
    /// `trafo`: the transformation of the faces during the slicing.
    pub trafo: Transform3d,
}

impl Default for MeshSlicingParams {
    /// `MeshSlicingParams{}`: regular slicing with no transformation.
    fn default() -> Self {
        Self {
            mode: SlicingMode::Regular,
            slicing_mode_normal_below_layer: 0,
            mode_below: SlicingMode::Regular,
            trafo: Transform3d::identity(),
        }
    }
}

/// `MeshSlicingParamsEx`: `MeshSlicingParams` (held as [`MeshSlicingParamsEx::params`]) and what
/// `slice_mesh_ex` does to the loops.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshSlicingParamsEx {
    /// The `MeshSlicingParams` part.
    pub params: MeshSlicingParams,
    /// `closing_radius`: the morphological closing when making the output expolygons, unscaled.
    pub closing_radius: f32,
    /// `extra_offset`: the positive offset applied when making the output expolygons, unscaled.
    pub extra_offset: f32,
    /// `resolution`: the resolution of the contour simplification, unscaled; 0 does not
    /// simplify.
    pub resolution: f64,
}

impl Default for MeshSlicingParamsEx {
    /// `MeshSlicingParamsEx{}`: no closing, no offset, no simplification.
    fn default() -> Self {
        Self {
            params: MeshSlicingParams::default(),
            closing_radius: 0.0,
            extra_offset: 0.0,
            resolution: 0.0,
        }
    }
}

/// `next_idx_modulo(idx, count)` of `libslic3r.h`: `idx + 1`, wrapping to 0 at `count`. It belongs
/// in orca-geom, beside the other helpers of `libslic3r.h`.
#[must_use]
pub fn next_idx_modulo(idx: usize, count: usize) -> usize {
    let idx = idx + 1;
    if idx == count { 0 } else { idx }
}

/// `IntersectionReference`: where an intersection point lies, on a mesh vertex or a mesh edge.
/// Only one of the two is set, the other stays -1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IntersectionReference {
    /// `point_id`: the index of the mesh vertex.
    pub point_id: i32,
    /// `edge_id`: the index of the mesh edge.
    pub edge_id: i32,
}

impl Default for IntersectionReference {
    fn default() -> Self {
        Self {
            point_id: -1,
            edge_id: -1,
        }
    }
}

impl IntersectionReference {
    /// `IntersectionReference(point_id, edge_id)`.
    #[must_use]
    pub const fn new(point_id: i32, edge_id: i32) -> Self {
        Self { point_id, edge_id }
    }
}

/// `IntersectionPoint`: a `Point` and its `IntersectionReference`. `Point()` is (0, 0).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct IntersectionPoint {
    /// The `Point` part.
    point: Point,
    /// The `IntersectionReference` part.
    reference: IntersectionReference,
}

/// `IntersectionLine::FacetEdgeType`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FacetEdgeType {
    /// A general case, the cutting plane intersects a face at two different edges.
    #[default]
    General,
    /// Two vertices are aligned with the cutting plane, the third vertex is below the cutting
    /// plane.
    Top,
    /// Two vertices are aligned with the cutting plane, the third vertex is above the cutting
    /// plane.
    Bottom,
    /// Two vertices are aligned with the cutting plane, the edge is shared by two triangles,
    /// where one triangle is below or at the cutting plane and the other is above or at the
    /// cutting plane (only one vertex may lie on the plane).
    TopBottom,
    /// All three vertices of a face are aligned with the cutting plane.
    Horizontal,
    /// An edge projected into a slab.
    Slab,
}

/// `IntersectionLine`: a `Line` (its `a` and `b`) cut out of a face, with where each end comes
/// from. For each end either its vertex index (`a_id`, `b_id`) or its mesh edge (`edge_a_id`,
/// `edge_b_id`) is set, the other left at -1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntersectionLine {
    /// `a`, of the `Line` part.
    pub a: Point,
    /// `b`, of the `Line` part.
    pub b: Point,
    /// `a_id`: the vertex index of `a`.
    pub a_id: i32,
    /// `b_id`: the vertex index of `b`.
    pub b_id: i32,
    /// `edge_a_id`: the mesh edge of `a`.
    pub edge_a_id: i32,
    /// `edge_b_id`: the mesh edge of `b`.
    pub edge_b_id: i32,
    /// `edge_type`.
    pub edge_type: FacetEdgeType,
    /// `flags`: the `EDGE*`, `NO_SEED` and `SKIP` bits.
    pub flags: u32,
}

impl Default for IntersectionLine {
    /// `IntersectionLine()`: both points (0, 0), every id -1, a general edge, no flags.
    fn default() -> Self {
        Self {
            a: Point::default(),
            b: Point::default(),
            a_id: -1,
            b_id: -1,
            edge_a_id: -1,
            edge_b_id: -1,
            edge_type: FacetEdgeType::General,
            flags: 0,
        }
    }
}

impl IntersectionLine {
    /// Triangle edge added, because it has no neighbor.
    pub const EDGE0_NO_NEIGHBOR: u32 = 0x001;
    /// Triangle edge added, because it has no neighbor.
    pub const EDGE1_NO_NEIGHBOR: u32 = 0x002;
    /// Triangle edge added, because it has no neighbor.
    pub const EDGE2_NO_NEIGHBOR: u32 = 0x004;
    /// Triangle edge added, because it makes a fold with another horizontal edge.
    pub const EDGE0_FOLD: u32 = 0x010;
    /// Triangle edge added, because it makes a fold with another horizontal edge.
    pub const EDGE1_FOLD: u32 = 0x020;
    /// Triangle edge added, because it makes a fold with another horizontal edge.
    pub const EDGE2_FOLD: u32 = 0x040;
    /// The edge cannot be a seed of a greedy loop extraction (folds are not safe to become
    /// seeds).
    pub const NO_SEED: u32 = 0x100;
    /// The line is skipped.
    pub const SKIP: u32 = 0x200;

    /// The C++'s brace initialisation `IntersectionLine { line, a_id, b_id, edge_a_id, edge_b_id,
    /// edge_type }` (an aggregate: `Line`, then the members in order; `flags` keeps its 0).
    #[must_use]
    pub const fn new(
        line: Line,
        a_id: i32,
        b_id: i32,
        edge_a_id: i32,
        edge_b_id: i32,
        edge_type: FacetEdgeType,
    ) -> Self {
        Self {
            a: line.a,
            b: line.b,
            a_id,
            b_id,
            edge_a_id,
            edge_b_id,
            edge_type,
            flags: 0,
        }
    }

    /// The `Line` part.
    #[must_use]
    pub const fn line(&self) -> Line {
        Line {
            a: self.a,
            b: self.b,
        }
    }

    /// `skip()`.
    #[must_use]
    pub const fn skip(&self) -> bool {
        (self.flags & Self::SKIP) != 0
    }

    /// `set_skip()`.
    pub fn set_skip(&mut self) {
        self.flags |= Self::SKIP;
    }

    /// `is_seed_candidate()`.
    #[must_use]
    pub const fn is_seed_candidate(&self) -> bool {
        (self.flags & Self::NO_SEED) == 0 && !self.skip()
    }

    /// `set_no_seed(set)`.
    pub fn set_no_seed(&mut self, set: bool) {
        if set {
            self.flags |= Self::NO_SEED;
        } else {
            self.flags &= !Self::NO_SEED;
        }
    }

    /// `reverse()`: swaps the ends, their vertex ids and their edge ids.
    pub fn reverse(&mut self) {
        core::mem::swap(&mut self.a, &mut self.b);
        core::mem::swap(&mut self.a_id, &mut self.b_id);
        core::mem::swap(&mut self.edge_a_id, &mut self.edge_b_id);
    }
}

/// `IntersectionLines`.
pub type IntersectionLines = Vec<IntersectionLine>;

/// `FacetSliceType`: what [`slice_facet`] made of a face.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum FacetSliceType {
    /// The face was not sliced.
    NoSlice = 0,
    /// The line is used for slicing.
    Slicing = 1,
    /// The line is used for object cutting only.
    Cutting = 2,
}

/// A vertex `x` and `y`, `float`s, assigned to a `Point`'s `coord_t`s: truncated.
fn point_of_xy(v: &StlVertex) -> Point {
    Point::new(f32_to_i64(v.x()), f32_to_i64(v.y()))
}

/// `slice_facet(slice_z, vertices, indices, edge_ids, idx_vertex_lowest, horizontal, line_out)`:
/// cuts the triangle with the plane at `slice_z` (scaled or unscaled, as the vertices' `z`; their
/// `x` and `y` are scaled). Returns whether the face was sliced, and fills `line_out` when it was:
/// [`FacetSliceType::Slicing`] for a line of the slice, [`FacetSliceType::Cutting`] for a line used
/// only to cut the object (a bottom edge, or a horizontal face), [`FacetSliceType::NoSlice`] when
/// the plane misses the face or touches it at one vertex (`line_out` is then left alone).
pub fn slice_facet(
    slice_z: f32,
    vertices: &[StlVertex; 3],
    indices: &StlTriangleVertexIndices,
    edge_ids: &Vec3i32,
    idx_vertex_lowest: i32,
    horizontal: bool,
    line_out: &mut IntersectionLine,
) -> FacetSliceType {
    let mut points = [IntersectionPoint::default(); 3];
    let mut num_points: usize = 0;
    let mut point_on_layer = usize::MAX;

    // Reorder vertices so that the first one is the one with lowest Z.
    // This is needed to get all intersection lines in a consistent order
    // (external on the right of the line)
    for j in 0..3 {
        // loop through facet edges
        let edge_id: i32;
        let mut a: usize;
        let mut b: usize;
        let mut a_id: i32;
        let mut b_id: i32;
        {
            let k = (idx_vertex_lowest + j) % 3;
            let l = (k + 1) % 3;
            edge_id = edge_ids[k as usize];
            a_id = indices[k as usize];
            a = k as usize;
            b_id = indices[l as usize];
            b = l as usize;
            // The C++ also points `c` at the third vertex, which it never reads here.
        }

        // Is edge or face aligned with the cutting plane?
        if vertices[a].z() == slice_z && vertices[b].z() == slice_z {
            // Edge is horizontal and belongs to the current layer.
            let v0 = &vertices[0];
            let v1 = &vertices[1];
            let v2 = &vertices[2];
            // We may ignore this edge for slicing purposes, but we may still use it for object
            // cutting.
            let result;
            if horizontal {
                // All three vertices are aligned with slice_z.
                line_out.edge_type = FacetEdgeType::Horizontal;
                result = FacetSliceType::Cutting;
                // Products and differences of floats, in float, then widened.
                let normal = f64::from(
                    (v1.x() - v0.x()) * (v2.y() - v1.y()) - (v1.y() - v0.y()) * (v2.x() - v1.x()),
                );
                if normal < 0.0 {
                    // If normal points downwards this is a bottom horizontal facet so we reverse
                    // its point order.
                    core::mem::swap(&mut a, &mut b);
                    core::mem::swap(&mut a_id, &mut b_id);
                }
            } else {
                // Two vertices are aligned with the cutting plane, the third vertex is below or
                // above the cutting plane. Is the third vertex below the cutting plane?
                let third_below = v0.z() < slice_z || v1.z() < slice_z || v2.z() < slice_z;
                // Two vertices on the cutting plane, the third vertex is below the plane.
                // Consider the edge to be part of the slice only if it is the upper edge.
                // (the bottom most edge resp. vertex of a triangle is not owned by the triangle,
                // but the top most edge resp. vertex is part of the triangle in respect to the
                // cutting plane).
                result = if third_below {
                    FacetSliceType::Slicing
                } else {
                    FacetSliceType::Cutting
                };
                if third_below {
                    line_out.edge_type = FacetEdgeType::Top;
                    core::mem::swap(&mut a, &mut b);
                    core::mem::swap(&mut a_id, &mut b_id);
                } else {
                    line_out.edge_type = FacetEdgeType::Bottom;
                }
            }
            line_out.a = point_of_xy(&vertices[a]);
            line_out.b = point_of_xy(&vertices[b]);
            line_out.a_id = a_id;
            line_out.b_id = b_id;
            return result;
        }

        if vertices[a].z() == slice_z {
            // Only point a aligns with the cutting plane.
            if point_on_layer == usize::MAX || points[point_on_layer].reference.point_id != a_id {
                point_on_layer = num_points;
                let point = &mut points[num_points];
                num_points += 1;
                point.point = point_of_xy(&vertices[a]);
                point.reference.point_id = a_id;
            }
        } else if vertices[b].z() == slice_z {
            // Only point b aligns with the cutting plane.
            if point_on_layer == usize::MAX || points[point_on_layer].reference.point_id != b_id {
                point_on_layer = num_points;
                let point = &mut points[num_points];
                num_points += 1;
                point.point = point_of_xy(&vertices[b]);
                point.reference.point_id = b_id;
            }
        } else if (vertices[a].z() < slice_z && vertices[b].z() > slice_z)
            || (vertices[b].z() < slice_z && vertices[a].z() > slice_z)
        {
            // A general case. The face edge intersects the cutting plane. Calculate the
            // intersection point. Sort the edge to give a consistent answer.
            if a_id > b_id {
                core::mem::swap(&mut a_id, &mut b_id);
                core::mem::swap(&mut a, &mut b);
            }
            let (va, vb) = (&vertices[a], &vertices[b]);
            let t =
                (f64::from(slice_z) - f64::from(vb.z())) / (f64::from(va.z()) - f64::from(vb.z()));
            if t <= 0.0 {
                if point_on_layer == usize::MAX || points[point_on_layer].reference.point_id != a_id
                {
                    let point = &mut points[num_points];
                    point.point = point_of_xy(va);
                    point_on_layer = num_points;
                    num_points += 1;
                    point.reference.point_id = a_id;
                }
            } else if t >= 1.0 {
                if point_on_layer == usize::MAX || points[point_on_layer].reference.point_id != b_id
                {
                    let point = &mut points[num_points];
                    point.point = point_of_xy(vb);
                    point_on_layer = num_points;
                    num_points += 1;
                    point.reference.point_id = b_id;
                }
            } else {
                let point = &mut points[num_points];
                point.point.x = f64_to_i64(floor(
                    f64::from(vb.x()) + (f64::from(va.x()) - f64::from(vb.x())) * t + 0.5,
                ));
                point.point.y = f64_to_i64(floor(
                    f64::from(vb.y()) + (f64::from(va.y()) - f64::from(vb.y())) * t + 0.5,
                ));
                point.reference.edge_id = edge_id;
                num_points += 1;
            }
        }
    }

    // Facets must intersect each plane 0 or 2 times, or it may touch the plane at a single vertex
    // only.
    if num_points == 2 {
        line_out.edge_type = FacetEdgeType::General;
        line_out.a = points[1].point;
        line_out.b = points[0].point;
        line_out.a_id = points[1].reference.point_id;
        line_out.b_id = points[0].reference.point_id;
        line_out.edge_a_id = points[1].reference.edge_id;
        line_out.edge_b_id = points[0].reference.edge_id;
        // Not a zero length edge.
        // FIXME slice_facet() may create zero length edges due to rounding of doubles into
        // coord_t. General slicing position, use the segment for both slicing and object
        // cutting.
        return FacetSliceType::Slicing;
    }
    FacetSliceType::NoSlice
}

/// The face's three vertices after `transform_vertex_fn`.
fn transformed_vertices<T: Fn(Vec3f) -> Vec3f>(
    mesh_vertices: &[Vec3f],
    transform_vertex_fn: &T,
    indices: &StlTriangleVertexIndices,
) -> [StlVertex; 3] {
    [
        transform_vertex_fn(mesh_vertices[indices[0] as usize]),
        transform_vertex_fn(mesh_vertices[indices[1] as usize]),
        transform_vertex_fn(mesh_vertices[indices[2] as usize]),
    ]
}

/// The face's lowest `z` and highest `z`: `fminf(v0, fminf(v1, v2))` and the same with `fmaxf`.
fn facet_extents(vertices: &[StlVertex; 3]) -> (f32, f32) {
    let min_z = fminf(vertices[0].z(), fminf(vertices[1].z(), vertices[2].z()));
    let max_z = fmaxf(vertices[0].z(), fmaxf(vertices[1].z(), vertices[2].z()));
    (min_z, max_z)
}

/// The vertex the slicing of a face starts from: the first of vertices 1, 2 and 0 at the face's
/// lowest `z`.
fn lowest_vertex(vertices: &[StlVertex; 3], min_z: f32) -> i32 {
    if vertices[1].z() == min_z {
        1
    } else if vertices[2].z() == min_z {
        2
    } else {
        0
    }
}

/// `slice_facet_at_zs(mesh_vertices, transform_vertex_fn, indices, edge_ids, zs, lines,
/// lines_mutex)`: cuts one face at each height of `zs` (sorted, scaled or unscaled as the vertices'
/// `z` once transformed) its extent spans, and gives each line of the slice to `emit` with the
/// index of its height, in the order of the heights. The C++ pushes it into `lines[slice_id]` under
/// a mutex; [`slice_make_lines`] keeps the order. A horizontal face is skipped: any valid
/// horizontal triangle has a vertical triangle connected, otherwise the part has zero volume.
pub fn slice_facet_at_zs<T: Fn(Vec3f) -> Vec3f>(
    mesh_vertices: &[Vec3f],
    transform_vertex_fn: &T,
    indices: &StlTriangleVertexIndices,
    edge_ids: &Vec3i32,
    zs: &[f32],
    emit: &mut impl FnMut(usize, IntersectionLine),
) {
    let vertices = transformed_vertices(mesh_vertices, transform_vertex_fn, indices);

    // find facet extents
    let (min_z, max_z) = facet_extents(&vertices);

    // find layer extents
    // first layer whose slice_z is >= min_z
    let min_layer = lower_bound(zs, &min_z);
    // first layer whose slice_z is > max_z
    let max_layer = min_layer + upper_bound(&zs[min_layer..], &max_z);
    let idx_vertex_lowest = lowest_vertex(&vertices, min_z);

    for (slice_id, &slice_z) in zs.iter().enumerate().take(max_layer).skip(min_layer) {
        let mut il = IntersectionLine::default();
        // Ignore horizontal triangles. Any valid horizontal triangle must have a vertical
        // triangle connected, otherwise the part has zero volume.
        if min_z != max_z
            && slice_facet(
                slice_z,
                &vertices,
                indices,
                edge_ids,
                idx_vertex_lowest,
                false,
                &mut il,
            ) == FacetSliceType::Slicing
        {
            emit(slice_id, il);
        }
    }
}

/// Runs `leaf` on each leaf of a `tbb::parallel_for` over `blocked_range<int>(0, count)` (the
/// auto partitioner, grain 1) and returns what each leaf gave, in the order of their first index:
/// the order one thread runs them in, whatever the threads did.
fn leaves_in_order<O: Send>(count: usize, leaf: impl Fn(i32, i32) -> O + Sync) -> Vec<O> {
    let done: Mutex<BTreeMap<i32, O>> = Mutex::new(BTreeMap::new());
    // `int(indices.size())`: the low 32 bits, as MSVC converts.
    let end = count as i32;
    parallel_for(
        BlockedRange::new(0_i32, end, 1),
        Partitioner::Auto,
        |range: BlockedRange<i32>| {
            let out = leaf(range.begin(), range.end());
            done.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(range.begin(), out);
        },
    );
    done.into_inner()
        .unwrap_or_else(PoisonError::into_inner)
        .into_values()
        .collect()
}

/// `slice_make_lines(vertices, transform_vertex_fn, indices, face_edge_ids, zs,
/// throw_on_cancel_fn)`: the lines of every face at every height of `zs`, one list per height,
/// each in the order of the faces. The faces are cut in a `tbb::parallel_for` (orca-tbb), and
/// `throw_on_cancel_fn` is called at every face whose index is a multiple of 65536.
pub fn slice_make_lines<T, C>(
    vertices: &[StlVertex],
    transform_vertex_fn: &T,
    indices: &[StlTriangleVertexIndices],
    face_edge_ids: &[Vec3i32],
    zs: &[f32],
    throw_on_cancel_fn: &C,
) -> Vec<IntersectionLines>
where
    T: Fn(Vec3f) -> Vec3f + Sync,
    C: Fn() + Sync,
{
    let mut lines: Vec<IntersectionLines> = vec![IntersectionLines::new(); zs.len()];
    let leaves = leaves_in_order(indices.len(), |begin, end| {
        let mut out: Vec<(usize, IntersectionLine)> = Vec::new();
        for face_idx in begin..end {
            if (face_idx & 0x0ffff) == 0 {
                throw_on_cancel_fn();
            }
            let face = face_idx as usize;
            slice_facet_at_zs(
                vertices,
                transform_vertex_fn,
                &indices[face],
                &face_edge_ids[face],
                zs,
                &mut |slice_id, il| out.push((slice_id, il)),
            );
        }
        out
    });
    for out in leaves {
        for (slice_id, il) in out {
            lines[slice_id].push(il);
        }
    }
    lines
}

/// `slice_make_lines(mesh_vertices, transform_vertex_fn, mesh_faces, face_edge_ids, plane_z,
/// face_filter)`, the overload for a single plane: the lines at `plane_z` of the faces
/// `face_filter` keeps (each of which spans the plane), in the order of the faces, on the calling
/// thread.
pub fn slice_make_lines_plane<T: Fn(Vec3f) -> Vec3f>(
    mesh_vertices: &[StlVertex],
    transform_vertex_fn: &T,
    mesh_faces: &[StlTriangleVertexIndices],
    face_edge_ids: &[Vec3i32],
    plane_z: f32,
    mut face_filter: impl FnMut(i32) -> bool,
) -> IntersectionLines {
    let mut lines = IntersectionLines::new();
    // `int(mesh_faces.size())`: the low 32 bits, as MSVC converts.
    let count = mesh_faces.len() as i32;
    for face_idx in 0..count {
        if face_filter(face_idx) {
            let indices = &mesh_faces[face_idx as usize];
            let vertices = transformed_vertices(mesh_vertices, transform_vertex_fn, indices);
            // find facet extents
            let (min_z, max_z) = facet_extents(&vertices);
            let idx_vertex_lowest = lowest_vertex(&vertices, min_z);
            let mut il = IntersectionLine::default();
            // Ignore horizontal triangles. Any valid horizontal triangle must have a vertical
            // triangle connected, otherwise the part has zero volume.
            if min_z != max_z
                && slice_facet(
                    plane_z,
                    &vertices,
                    indices,
                    &face_edge_ids[face_idx as usize],
                    idx_vertex_lowest,
                    false,
                    &mut il,
                ) == FacetSliceType::Slicing
            {
                lines.push(il);
            }
        }
    }
    lines
}

/// `SlabLines`: what [`slice_slabs_make_lines`] gives for projecting a triangle set onto slice
/// slabs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SlabLines {
    /// `at_slice`: the intersection lines of a slice with the triangle set, CCW oriented.
    pub at_slice: Vec<IntersectionLines>,
    /// `between_slices`: the projections of the triangle set's boundary lines into the layer
    /// below (for projection from the top) or into the layer above (for projection from the
    /// bottom). In both cases the intersection lines are CCW oriented.
    pub between_slices: Vec<IntersectionLines>,
}

/// Where [`slice_facet_with_slabs`] puts a line: `lines.at_slice[id]` or
/// `lines.between_slices[id]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SlabSlot {
    /// `at_slice[id]`.
    AtSlice(usize),
    /// `between_slices[id]`.
    BetweenSlices(usize),
}

/// `FaceOrientation`: the orientation of the face normal in regard to a XY plane pointing upwards.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i8)]
pub enum FaceOrientation {
    /// Z component of the normal is positive.
    Up,
    /// Z component of the normal is negative.
    Down,
    /// Z component of the normal is zero.
    Vertical,
    /// Triangle is degenerate, thus its normal is undefined. We may want to slice the degenerate
    /// triangles because of the connectivity information they carry.
    Degenerate,
}

/// A vertex's `to_2d(v).cast<coord_t>()`: `x` and `y` truncated.
fn cast_2d(v: &StlVertex) -> Point {
    Point::new(f32_to_i64(v.x()), f32_to_i64(v.y()))
}

/// `emit_slab_edge(il, slab_id, reverse)`, the lambda of `slice_facet_with_slabs`: the line,
/// reversed when asked, into `between_slices[slab_id]`.
fn emit_slab_edge(
    emit: &mut impl FnMut(SlabSlot, IntersectionLine),
    mut il: IntersectionLine,
    slab_id: usize,
    reverse: bool,
) {
    if reverse {
        il.reverse();
    }
    emit(SlabSlot::BetweenSlices(slab_id), il);
}

/// `slice_facet_with_slabs<ProjectionFromTop>(mesh_vertices, mesh_triangles, facet_idx,
/// facet_neighbors, facet_edge_ids, num_edges, zs, lines, lines_mutex)`: the lines face
/// `facet_idx` gives `slice_mesh_slabs`, each passed to `emit` with its slot in the order the C++
/// pushes them. `at_slice_len` is `lines.at_slice.size()` (the count of heights). The edges at the
/// top plane of a slab are numbered `edge_id + num_edges`, to allow chaining from the bottom plane
/// of the slab to the top plane and back.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub fn slice_facet_with_slabs<const PROJECTION_FROM_TOP: bool>(
    mesh_vertices: &[Vec3f],
    mesh_triangles: &[StlTriangleVertexIndices],
    facet_idx: usize,
    facet_neighbors: &Vec3i32,
    facet_edge_ids: &Vec3i32,
    num_edges: i32,
    zs: &[f32],
    at_slice_len: usize,
    emit: &mut impl FnMut(SlabSlot, IntersectionLine),
) {
    let indices = &mesh_triangles[facet_idx];
    let vertices: [StlVertex; 3] = [
        mesh_vertices[indices[0] as usize],
        mesh_vertices[indices[1] as usize],
        mesh_vertices[indices[2] as usize],
    ];

    // find facet extents
    let (min_z, max_z) = facet_extents(&vertices);
    let horizontal = min_z == max_z;

    // find layer extents
    // first layer whose slice_z is >= min_z
    let min_layer = lower_bound(zs, &min_z);
    // first layer whose slice_z is > max_z
    let max_layer = min_layer + upper_bound(&zs[min_layer..], &max_z);

    if min_layer == max_layer || horizontal {
        // Horizontal face or a nearly horizontal face that fits between two layers or below the
        // bottom most or above the top most layer.
        if horizontal && min_layer != zs.len() && zs[min_layer] == min_z {
            // Slicing a horizontal triangle with a slicing plane. The triangle has to be upwards
            // facing for ProjectionFromTop and downwards facing for ! ProjectionFromTop.
            // Slicing plane with which the triangle is coplanar.
            let slice_id = min_layer;
            // Project the coplanar bottom facing triangles to the plane above the slicing plane
            // to match the behavior of slice_mesh() / slice_mesh_ex(), where the slicing plane
            // slices the top facing surfaces, but misses the bottom facing surfaces.
            let line_id = if PROJECTION_FROM_TOP {
                slice_id
            } else {
                slice_id + 1
            };
            if PROJECTION_FROM_TOP || line_id < at_slice_len {
                for iedge in 0..3 {
                    if facet_neighbors[iedge] == -1 {
                        let i = iedge;
                        let j = next_idx_modulo(i, 3);
                        let il = IntersectionLine::new(
                            Line {
                                a: cast_2d(&vertices[i]),
                                b: cast_2d(&vertices[j]),
                            },
                            indices[i],
                            indices[j],
                            -1,
                            -1,
                            if PROJECTION_FROM_TOP {
                                FacetEdgeType::Bottom
                            } else {
                                FacetEdgeType::Top
                            },
                        );
                        // Don't flip the FacetEdgeType::Top edge, it will be flipped when
                        // chaining.
                        emit(SlabSlot::AtSlice(line_id), il);
                    }
                }
            }
        } else {
            // Triangle is completely between two slicing planes, the triangle may or may not be
            // horizontal, which does not matter for the processing of such a triangle.
            let mut slab_id;
            if PROJECTION_FROM_TOP {
                if max_layer == 0 {
                    // Not slicing the triangle and it is below the lowest layer.
                    return;
                }
                // Not slicing the triangle and it could be projected into a slab.
                slab_id = max_layer;
            } else {
                // projection from bottom
                if min_layer == zs.len() {
                    // Not slicing the triangle and it is above the highest layer.
                    return;
                }
                // Not slicing the triangle and it could be projected into a slab.
                slab_id = min_layer;
            }
            if PROJECTION_FROM_TOP {
                slab_id -= 1;
            }
            for iedge in 0..3 {
                if facet_neighbors[iedge] == -1 {
                    let i = iedge;
                    let j = next_idx_modulo(i, 3);
                    emit_slab_edge(
                        emit,
                        IntersectionLine::new(
                            Line {
                                a: cast_2d(&vertices[i]),
                                b: cast_2d(&vertices[j]),
                            },
                            indices[i],
                            indices[j],
                            -1,
                            -1,
                            FacetEdgeType::Slab,
                        ),
                        slab_id,
                        !PROJECTION_FROM_TOP,
                    );
                }
            }
        }
    } else {
        // The triangle is not horizontal and at least a single slicing plane intersects the
        // triangle.
        let idx_vertex_lowest = lowest_vertex(&vertices, min_z);
        let mut il_prev = IntersectionLine::default();
        for it in min_layer..max_layer {
            let mut il = IntersectionLine::default();
            let mut ty = slice_facet(
                zs[it],
                &vertices,
                indices,
                facet_edge_ids,
                idx_vertex_lowest,
                false,
                &mut il,
            );
            if ty == FacetSliceType::NoSlice {
                // One and exactly one vertex is touching the slicing plane.
            } else {
                if il.edge_type == FacetEdgeType::Top || il.edge_type == FacetEdgeType::Bottom {
                    // The non-horizontal triangle is being sliced at one of its edges.
                    // If the edge is open (it does not have a neighbor), add it.
                    // If the edge has a neighbor, then add it as TopBottom, and do it just once.
                    // Identify edge ID from the edge vertices.
                    let edge_id = if ty == FacetSliceType::Cutting {
                        // The edge is oriented CCW along the face perimeter.
                        if il.a_id == indices[0] {
                            0
                        } else if il.a_id == indices[1] {
                            1
                        } else {
                            2
                        }
                    } else {
                        // The edge is oriented CW along the face perimeter.
                        if il.b_id == indices[0] {
                            0
                        } else if il.b_id == indices[1] {
                            1
                        } else {
                            2
                        }
                    };
                    let neighbor_idx = facet_neighbors[edge_id];
                    if neighbor_idx == -1 {
                        // Save the open edge for sure.
                        ty = FacetSliceType::Slicing;
                    } else if il.edge_type == FacetEdgeType::Top {
                        // Project the coplanar bottom facing triangles to the plane above the
                        // slicing plane to match the behavior of slice_mesh() / slice_mesh_ex(),
                        // where the slicing plane slices the top facing surfaces, but misses the
                        // bottom facing surfaces.
                        // Indicate that the edge belongs to both the slab below and above the
                        // plane.
                        il.edge_type = FacetEdgeType::TopBottom;
                    } else {
                        // Don't add this edge, as the neighbor triangle will add the same edge
                        // as FacetEdgeType::TopBottom.
                    }
                }
                if ty == FacetSliceType::Slicing {
                    if !PROJECTION_FROM_TOP {
                        il.reverse();
                    }
                    let line_id = it;
                    emit(SlabSlot::AtSlice(line_id), il);
                }
            }
            if !PROJECTION_FROM_TOP || it != 0 {
                let mut slab_id = it;
                if PROJECTION_FROM_TOP {
                    slab_id -= 1;
                }
                // Try to project unbound edges.
                for iedge in 0..3 {
                    if facet_neighbors[iedge] == -1 {
                        // Unbound edge.
                        let edge_id = facet_edge_ids[iedge];
                        let intersects_this = il.edge_a_id == edge_id || il.edge_b_id == edge_id;
                        let intersects_prev =
                            il_prev.edge_a_id == edge_id || il_prev.edge_b_id == edge_id;
                        let i = iedge;
                        let j = next_idx_modulo(i, 3);
                        let edge_up = vertices[j].z() > vertices[i].z();
                        if intersects_this && intersects_prev {
                            // Intersects both, emit the segment between these intersections.
                            let l = Line {
                                a: if il_prev.edge_a_id == edge_id {
                                    il_prev.a
                                } else {
                                    il_prev.b
                                },
                                b: if il.edge_a_id == edge_id { il.a } else { il.b },
                            };
                            emit_slab_edge(
                                emit,
                                IntersectionLine::new(
                                    l,
                                    -1,
                                    -1,
                                    edge_id,
                                    edge_id.wrapping_add(num_edges),
                                    FacetEdgeType::Slab,
                                ),
                                slab_id,
                                PROJECTION_FROM_TOP != edge_up,
                            );
                        } else if intersects_this {
                            // Intersects just the top plane, may touch the bottom plane.
                            emit_slab_edge(
                                emit,
                                IntersectionLine::new(
                                    Line {
                                        a: cast_2d(if edge_up {
                                            &vertices[i]
                                        } else {
                                            &vertices[j]
                                        }),
                                        b: if il.edge_a_id == edge_id { il.a } else { il.b },
                                    },
                                    if edge_up { indices[i] } else { indices[j] },
                                    -1,
                                    -1,
                                    edge_id.wrapping_add(num_edges),
                                    FacetEdgeType::Slab,
                                ),
                                slab_id,
                                PROJECTION_FROM_TOP != edge_up,
                            );
                        } else if intersects_prev {
                            // Intersects just the bottom plane, may touch the top vertex.
                            emit_slab_edge(
                                emit,
                                IntersectionLine::new(
                                    Line {
                                        a: if il_prev.edge_a_id == edge_id {
                                            il_prev.a
                                        } else {
                                            il_prev.b
                                        },
                                        b: cast_2d(if edge_up {
                                            &vertices[j]
                                        } else {
                                            &vertices[i]
                                        }),
                                    },
                                    -1,
                                    if edge_up { indices[j] } else { indices[i] },
                                    edge_id,
                                    -1,
                                    FacetEdgeType::Slab,
                                ),
                                slab_id,
                                PROJECTION_FROM_TOP != edge_up,
                            );
                        } else {
                            let zi = vertices[i].z();
                            let zj = vertices[j].z();
                            if zi < zs[it] || zj < zs[it] {
                                // The edge does not intersect the current plane and it does not
                                // intersect the previous plane either. Both points have to be
                                // inside the slab. Is it inside the slab?
                                let mut inside_slab = true;
                                if it != min_layer {
                                    let it_prev = it - 1;
                                    // One point may touch the plane below, the other must not.
                                    inside_slab = zi > zs[it_prev] || zj > zs[it_prev];
                                }
                                if inside_slab {
                                    emit_slab_edge(
                                        emit,
                                        IntersectionLine::new(
                                            Line {
                                                a: cast_2d(&vertices[i]),
                                                b: cast_2d(&vertices[j]),
                                            },
                                            indices[i],
                                            indices[j],
                                            -1,
                                            -1,
                                            FacetEdgeType::Slab,
                                        ),
                                        slab_id,
                                        !PROJECTION_FROM_TOP,
                                    );
                                }
                            }
                        }
                    }
                }
            }
            il_prev = il;
        }
        if PROJECTION_FROM_TOP || max_layer != zs.len() {
            // Try to project unbound edges above the last slicing plane to the last slab.
            // Last layer slicing this triangle.
            let it = max_layer - 1;
            let mut slab_id = max_layer;
            if PROJECTION_FROM_TOP {
                slab_id -= 1;
            }
            for iedge in 0..3 {
                if facet_neighbors[iedge] == -1 {
                    // Unbound edge.
                    let edge_id = facet_edge_ids[iedge];
                    let i = iedge;
                    let j = next_idx_modulo(i, 3);
                    if il_prev.edge_a_id == edge_id || il_prev.edge_b_id == edge_id {
                        // Intersects just the bottom plane, may touch the top vertex.
                        let edge_up = vertices[j].z() > vertices[i].z();
                        emit_slab_edge(
                            emit,
                            IntersectionLine::new(
                                Line {
                                    a: if il_prev.edge_a_id == edge_id {
                                        il_prev.a
                                    } else {
                                        il_prev.b
                                    },
                                    b: cast_2d(if edge_up { &vertices[j] } else { &vertices[i] }),
                                },
                                -1,
                                if edge_up { indices[j] } else { indices[i] },
                                edge_id,
                                -1,
                                FacetEdgeType::Slab,
                            ),
                            slab_id,
                            PROJECTION_FROM_TOP != edge_up,
                        );
                    } else {
                        let zi = vertices[i].z();
                        let zj = vertices[j].z();
                        if zi > zs[it] || zj > zs[it] {
                            // The edge does not intersect the current plane and it does not
                            // intersect the previous plane either. Both points have to be inside
                            // the slab.
                            emit_slab_edge(
                                emit,
                                IntersectionLine::new(
                                    Line {
                                        a: cast_2d(&vertices[i]),
                                        b: cast_2d(&vertices[j]),
                                    },
                                    indices[i],
                                    indices[j],
                                    -1,
                                    -1,
                                    FacetEdgeType::Slab,
                                ),
                                slab_id,
                                !PROJECTION_FROM_TOP,
                            );
                        }
                    }
                }
            }
        }
    }
}

/// `neighbors` with each neighbour whose orientation is not `keep` (or degenerate) reset to -1:
/// the reset `slice_slabs_make_lines` makes in case the other triangle is oriented backwards from
/// this one.
fn neighbors_of_orientation(
    mut neighbors: Vec3i32,
    face_orientation: &[FaceOrientation],
    keep: FaceOrientation,
) -> Vec3i32 {
    for i in 0..3 {
        if neighbors[i] != -1 {
            let fo2 = face_orientation[neighbors[i] as usize];
            if fo2 != keep && fo2 != FaceOrientation::Degenerate {
                neighbors[i] = -1;
            }
        }
    }
    neighbors
}

/// `slice_slabs_make_lines(vertices, indices, face_neighbors, face_edge_ids, num_edges,
/// face_orientation, zs, top, bottom, throw_on_cancel_fn)`: the lines `slice_mesh_slabs` chains,
/// as the top and the bottom `SlabLines`. Each is sized to `zs` only when asked for (`top`,
/// `bottom`). The upward (or degenerate) faces make the top lines and the downward (or
/// degenerate) ones the bottom lines, each with its neighbours of another orientation dropped.
/// `num_edges` is the count of edges (every face edge id is below it). The faces run in a
/// `tbb::parallel_for` (orca-tbb), every list in the order of the faces; `throw_on_cancel_fn` is
/// called at every face whose index is a multiple of 65536.
#[allow(clippy::too_many_arguments)]
pub fn slice_slabs_make_lines<C: Fn() + Sync>(
    vertices: &[StlVertex],
    indices: &[StlTriangleVertexIndices],
    face_neighbors: &[Vec3i32],
    face_edge_ids: &[Vec3i32],
    num_edges: i32,
    face_orientation: &[FaceOrientation],
    zs: &[f32],
    top: bool,
    bottom: bool,
    throw_on_cancel_fn: &C,
) -> (SlabLines, SlabLines) {
    let mut lines_top = SlabLines::default();
    let mut lines_bottom = SlabLines::default();

    if top {
        lines_top.at_slice = vec![IntersectionLines::new(); zs.len()];
        lines_top.between_slices = vec![IntersectionLines::new(); zs.len()];
    }
    if bottom {
        lines_bottom.at_slice = vec![IntersectionLines::new(); zs.len()];
        lines_bottom.between_slices = vec![IntersectionLines::new(); zs.len()];
    }
    let (top_len, bottom_len) = (lines_top.at_slice.len(), lines_bottom.at_slice.len());

    let leaves = leaves_in_order(indices.len(), |begin, end| {
        // Each line with whether it is a top line and its slot, in the order they are made.
        let mut out: Vec<(bool, SlabSlot, IntersectionLine)> = Vec::new();
        for face_idx in begin..end {
            if (face_idx & 0x0ffff) == 0 {
                throw_on_cancel_fn();
            }
            let face = face_idx as usize;
            let fo = face_orientation[face];
            let edge_ids = face_edge_ids[face];
            if top && (fo == FaceOrientation::Up || fo == FaceOrientation::Degenerate) {
                let neighbors = neighbors_of_orientation(
                    face_neighbors[face],
                    face_orientation,
                    FaceOrientation::Up,
                );
                slice_facet_with_slabs::<true>(
                    vertices,
                    indices,
                    face,
                    &neighbors,
                    &edge_ids,
                    num_edges,
                    zs,
                    top_len,
                    &mut |slot, il| out.push((true, slot, il)),
                );
            }
            if bottom && (fo == FaceOrientation::Down || fo == FaceOrientation::Degenerate) {
                let neighbors = neighbors_of_orientation(
                    face_neighbors[face],
                    face_orientation,
                    FaceOrientation::Down,
                );
                slice_facet_with_slabs::<false>(
                    vertices,
                    indices,
                    face,
                    &neighbors,
                    &edge_ids,
                    num_edges,
                    zs,
                    bottom_len,
                    &mut |slot, il| out.push((false, slot, il)),
                );
            }
        }
        out
    });
    for out in leaves {
        for (is_top, slot, il) in out {
            let lines = if is_top {
                &mut lines_top
            } else {
                &mut lines_bottom
            };
            match slot {
                SlabSlot::AtSlice(id) => lines.at_slice[id].push(il),
                SlabSlot::BetweenSlices(id) => lines.between_slices[id].push(il),
            }
        }
    }
    (lines_top, lines_bottom)
}

/// `OpenPolyline`: a chain of intersection lines that did not close, with the references of its
/// two ends.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenPolyline {
    /// `start`.
    pub start: IntersectionReference,
    /// `end`.
    pub end: IntersectionReference,
    /// `points`.
    pub points: Points,
    /// `length`: `Slic3r::length(points)` when made.
    pub length: f64,
    /// `consumed`.
    pub consumed: bool,
}

impl OpenPolyline {
    /// `OpenPolyline(start, end, points)`: not consumed, its length that of `points`.
    #[must_use]
    pub fn new(start: IntersectionReference, end: IntersectionReference, points: Points) -> Self {
        let length = length(&points);
        Self {
            start,
            end,
            points,
            length,
            consumed: false,
        }
    }

    /// `reverse()`: swaps the ends and reverses the points (the length stays as it was).
    pub fn reverse(&mut self) {
        core::mem::swap(&mut self.start, &mut self.end);
        self.points.reverse();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orca_eigen::Vector;
    use orca_tbb::Arena;

    fn v(x: f32, y: f32, z: f32) -> Vec3f {
        Vector([x, y, z])
    }

    fn face(a: i32, b: i32, c: i32) -> Vec3i32 {
        Vector([a, b, c])
    }

    #[test]
    fn a_crossing_face_gives_the_points_on_its_edges() {
        // A vertical triangle: vertex 0 at the bottom, 1 and 2 above, XY scaled.
        let vertices = [v(0.0, 0.0, 0.0), v(1000.0, 0.0, 2.0), v(0.0, 1000.0, 2.0)];
        let indices = face(0, 1, 2);
        let edges = face(10, 11, 12);
        let mut il = IntersectionLine::default();
        let ty = slice_facet(1.0, &vertices, &indices, &edges, 0, false, &mut il);
        assert_eq!(ty, FacetSliceType::Slicing);
        assert_eq!(il.edge_type, FacetEdgeType::General);
        // Edge 0 (0-1) is crossed first, edge 2 (2-0, sorted to 0-2) second: a is the second.
        assert_eq!(il.a, Point::new(0, 500));
        assert_eq!(il.b, Point::new(500, 0));
        assert_eq!(
            (il.a_id, il.b_id, il.edge_a_id, il.edge_b_id),
            (-1, -1, 12, 10)
        );
    }

    #[test]
    fn an_edge_on_the_plane_is_top_or_bottom() {
        let vertices = [v(0.0, 0.0, 0.0), v(1000.0, 0.0, 2.0), v(0.0, 1000.0, 2.0)];
        let indices = face(4, 5, 6);
        let edges = face(0, 1, 2);
        let mut il = IntersectionLine::default();
        // The upper edge 1-2 lies on z 2 with the third vertex below: Top, reversed.
        let ty = slice_facet(2.0, &vertices, &indices, &edges, 0, false, &mut il);
        assert_eq!(ty, FacetSliceType::Slicing);
        assert_eq!(il.edge_type, FacetEdgeType::Top);
        assert_eq!((il.a_id, il.b_id), (6, 5));
        assert_eq!((il.a, il.b), (Point::new(0, 1000), Point::new(1000, 0)));
        // At z 0 only vertex 0 touches: no line.
        let mut il2 = IntersectionLine::default();
        let ty = slice_facet(0.0, &vertices, &indices, &edges, 0, false, &mut il2);
        assert_eq!(ty, FacetSliceType::NoSlice);
        assert_eq!(il2, IntersectionLine::default());
        // The bottom edge of an upside-down face is only for cutting.
        let flipped = [v(0.0, 0.0, 2.0), v(1000.0, 0.0, 0.0), v(0.0, 1000.0, 0.0)];
        let mut il3 = IntersectionLine::default();
        let ty = slice_facet(0.0, &flipped, &indices, &edges, 1, false, &mut il3);
        assert_eq!(ty, FacetSliceType::Cutting);
        assert_eq!(il3.edge_type, FacetEdgeType::Bottom);
        assert_eq!((il3.a_id, il3.b_id), (5, 6));
    }

    /// A closed cylinder of `n` sides and `rings` rings, XY scaled by 1e6, its edge ids made from
    /// the sorted vertex pairs.
    fn cylinder(n: usize, rings: usize) -> (Vec<Vec3f>, Vec<Vec3i32>, Vec<Vec3i32>) {
        let mut vertices = Vec::new();
        for r in 0..=rings {
            for k in 0..n {
                let a = k as f64 / n as f64 * std::f64::consts::TAU;
                vertices.push(v(
                    (orca_crt::cos(a) * 1e7) as f32,
                    (orca_crt::sin(a) * 1e7) as f32,
                    r as f32 * 0.5,
                ));
            }
        }
        let mut faces = Vec::new();
        for r in 0..rings {
            for k in 0..n {
                let (a, b) = ((r * n + k) as i32, (r * n + (k + 1) % n) as i32);
                let (c, d) = (a + n as i32, b + n as i32);
                faces.push(face(a, b, d));
                faces.push(face(a, d, c));
            }
        }
        let mut pairs: BTreeMap<(i32, i32), i32> = BTreeMap::new();
        let mut edge_ids = Vec::new();
        for f in &faces {
            let mut e = Vector([0, 0, 0]);
            for i in 0..3 {
                let (p, q) = (f[i], f[next_idx_modulo(i, 3)]);
                let key = if p < q { (p, q) } else { (q, p) };
                let next = pairs.len() as i32;
                e[i] = *pairs.entry(key).or_insert(next);
            }
            edge_ids.push(e);
        }
        (vertices, faces, edge_ids)
    }

    #[test]
    fn the_lines_come_in_the_order_of_the_faces_in_any_arena() {
        let (vertices, faces, edge_ids) = cylinder(97, 23);
        let zs: Vec<f32> = (0..40).map(|i| 0.13 + i as f32 * 0.29).collect();
        let identity = |p: Vec3f| p;
        // The order one thread gives: face by face.
        let mut want = vec![IntersectionLines::new(); zs.len()];
        for (f, e) in faces.iter().zip(&edge_ids) {
            slice_facet_at_zs(&vertices, &identity, f, e, &zs, &mut |id, il| {
                want[id].push(il)
            });
        }
        assert!(want.iter().all(|l| !l.is_empty()));
        let serial = Arena::serial(8)
            .execute(|| slice_make_lines(&vertices, &identity, &faces, &edge_ids, &zs, &|| ()));
        assert_eq!(serial, want);
        let parallel = Arena::new(4)
            .unwrap()
            .execute(|| slice_make_lines(&vertices, &identity, &faces, &edge_ids, &zs, &|| ()));
        assert_eq!(parallel, want);
        // One plane: the same lines as that height's list.
        let plane =
            slice_make_lines_plane(&vertices, &identity, &faces, &edge_ids, zs[7], |_| true);
        assert_eq!(plane, want[7]);
    }

    #[test]
    fn slab_lines_come_in_the_order_of_the_faces_in_any_arena() {
        let (vertices, faces, edge_ids) = cylinder(31, 9);
        // The side faces are vertical; call half of them up and half down, every fifth
        // degenerate, so that both projections run on open edges.
        let fo: Vec<FaceOrientation> = (0..faces.len())
            .map(|i| match i % 5 {
                0 => FaceOrientation::Degenerate,
                1 | 3 => FaceOrientation::Up,
                _ => FaceOrientation::Down,
            })
            .collect();
        let neighbors: Vec<Vec3i32> = (0..faces.len())
            .map(|i| {
                let i = i as i32;
                Vector([
                    if i % 3 == 0 {
                        -1
                    } else {
                        (i + 1) % faces.len() as i32
                    },
                    -1,
                    i / 2,
                ])
            })
            .collect();
        let zs: Vec<f32> = (0..12).map(|i| 0.25 + i as f32 * 0.375).collect();
        let num_edges = 1000;
        let run = || {
            slice_slabs_make_lines(
                &vertices,
                &faces,
                &neighbors,
                &edge_ids,
                num_edges,
                &fo,
                &zs,
                true,
                true,
                &|| (),
            )
        };
        let serial = Arena::serial(8).execute(run);
        let parallel = Arena::new(4).unwrap().execute(run);
        assert_eq!(serial, parallel);
        assert!(serial.0.between_slices.iter().any(|l| !l.is_empty()));
        assert!(serial.1.between_slices.iter().any(|l| !l.is_empty()));
        // Only what is asked for is sized.
        let (top, bottom) = slice_slabs_make_lines(
            &vertices,
            &faces,
            &neighbors,
            &edge_ids,
            num_edges,
            &fo,
            &zs,
            false,
            true,
            &|| (),
        );
        assert!(top.at_slice.is_empty() && top.between_slices.is_empty());
        assert_eq!(bottom, serial.1);
    }

    #[test]
    fn a_cancel_stops_the_loop() {
        let (vertices, faces, edge_ids) = cylinder(8, 2);
        let zs = [0.3_f32];
        let caught = std::panic::catch_unwind(|| {
            slice_make_lines(&vertices, &|p| p, &faces, &edge_ids, &zs, &|| {
                panic!("canceled")
            })
        });
        assert!(caught.is_err());
    }
}
