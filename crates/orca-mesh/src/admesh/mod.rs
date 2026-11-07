// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries ADMesh code: the types of deps_src/admesh/stl.h of OrcaSlicer v2.4.2 (stl_facet,
// stl_neighbors, stl_stats, stl_file, indexed_triangle_set, FaceProperty and the inline helpers),
// translated on 2026-11-07. Copyright (C) 1995, 1996 Anthony D. Martin and (C) 2013, 2014 the
// ADMesh contributors, with changes by the PrusaSlicer and OrcaSlicer authors, under the GNU
// General Public License, version 2 or later (GPL-2.0-or-later; licenses/GPL-2.0.txt), taken here
// under version 3 (licenses/GPL-3.0.txt) as section 13 of the AGPL-3.0 allows.

//! admesh (`deps_src/admesh`) as libslic3r runs it: the STL reader, the facet connectivity, the
//! repair steps and the shared vertices.
//!
//! - [`stlinit`]: `stl_open`, which tells binary from ASCII files and reads either, skipping
//!   facets with a NaN coordinate.
//! - [`connect`]: the exact and the nearby edge matching (`stl_check_facets_exact`,
//!   `stl_check_facets_nearby`), `stl_remove_unconnected_facets` and `stl_fill_holes`.
//! - [`normals`]: the facet orientation (`stl_fix_normal_directions`), the normals
//!   (`stl_fix_normal_values`) and `stl_reverse_all_facets`.
//! - [`shared`]: `stl_generate_shared_vertices`, which turns the facets into an
//!   [`IndexedTriangleSet`] by walking the fan of facets around each vertex.
//! - [`util`]: `stl_verify_neighbors`, the volume, the transforms and `stl_repair`.
//!
//! The types mirror `stl.h` field for field. `float` stays `f32`, so every sum and product is the
//! C++ engine's own; where the C++ widens to `double` (the area, the normal's length, the
//! rotations) the translation widens at the same place. `stl_file::facet_start` and
//! `neighbors_start` may hold more entries than `stats.number_of_facets` after
//! `stl_remove_unconnected_facets` stops early on broken connectivity, as the C++ does
//! (`parity/HAZARDS.md`, orca-mesh); every loop runs to `number_of_facets`, as there.

pub mod connect;
pub mod normals;
pub mod shared;
pub mod stlinit;
pub mod util;

use orca_eigen::{Vec3f, Vec3i32};

/// `LABEL_SIZE`: the size of a binary STL's free-form header.
pub const LABEL_SIZE: i32 = 80;
/// `NUM_FACET_SIZE`: the binary STL's facet count.
pub const NUM_FACET_SIZE: i32 = 4;
/// `HEADER_SIZE`.
pub const HEADER_SIZE: i32 = 84;
/// `STL_MIN_FILE_SIZE`.
pub const STL_MIN_FILE_SIZE: i32 = 284;
/// `ASCII_LINES_PER_FACET`.
pub const ASCII_LINES_PER_FACET: i32 = 7;
/// `SIZEOF_STL_FACET`: a facet of a binary STL file.
pub const SIZEOF_STL_FACET: usize = 50;

/// `stl_vertex`: `Eigen::Matrix<float, 3, 1, Eigen::DontAlign>`.
pub type StlVertex = Vec3f;
/// `stl_normal`.
pub type StlNormal = Vec3f;
/// `stl_triangle_vertex_indices`: `Eigen::Matrix<int, 3, 1, Eigen::DontAlign>`.
pub type StlTriangleVertexIndices = Vec3i32;

/// `stl_facet`: a normal, three vertices and the two attribute bytes of a binary facet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StlFacet {
    /// `normal`.
    pub normal: StlNormal,
    /// `vertex[3]`.
    pub vertex: [StlVertex; 3],
    /// `extra[2]`.
    pub extra: [u8; 2],
}

impl StlFacet {
    /// `stl_facet()` value-initialised: every byte zero, as `stl_allocate` fills the facets.
    pub const ZERO: StlFacet = StlFacet {
        normal: Vec3f::new(0.0, 0.0, 0.0),
        vertex: [Vec3f::new(0.0, 0.0, 0.0); 3],
        extra: [0, 0],
    };

    /// A binary STL facet: twelve little-endian `float`s (the normal, then the vertices) and two
    /// attribute bytes, as `fread` copies them into the struct.
    #[must_use]
    pub fn from_bytes(b: &[u8; SIZEOF_STL_FACET]) -> StlFacet {
        let f = |i: usize| f32::from_le_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]]);
        StlFacet {
            normal: Vec3f::new(f(0), f(1), f(2)),
            vertex: [
                Vec3f::new(f(3), f(4), f(5)),
                Vec3f::new(f(6), f(7), f(8)),
                Vec3f::new(f(9), f(10), f(11)),
            ],
            extra: [b[48], b[49]],
        }
    }
}

/// `stl_neighbors`: for each edge of a facet (the edge from vertex j to vertex j + 1), the
/// neighbouring facet and the index of its vertex opposite the edge, plus 3 when the two facets
/// run the edge the same way round (their orientations disagree).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StlNeighbors {
    /// `neighbor[3]`: -1 for none.
    pub neighbor: [i32; 3],
    /// `which_vertex_not[3]` (a `char`, signed in MSVC): -1 for none.
    pub which_vertex_not: [i8; 3],
}

impl Default for StlNeighbors {
    fn default() -> Self {
        StlNeighbors::RESET
    }
}

impl StlNeighbors {
    /// `stl_neighbors()` and `reset()`: no neighbours.
    pub const RESET: StlNeighbors = StlNeighbors {
        neighbor: [-1, -1, -1],
        which_vertex_not: [-1, -1, -1],
    };

    /// `reset()`.
    pub fn reset(&mut self) {
        *self = StlNeighbors::RESET;
    }

    /// `num_neighbors()`.
    #[must_use]
    pub fn num_neighbors(&self) -> i32 {
        3 - (i32::from(self.neighbor[0] == -1)
            + i32::from(self.neighbor[1] == -1)
            + i32::from(self.neighbor[2] == -1))
    }
}

/// `stl_type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StlType {
    /// `binary` (0, the default).
    #[default]
    Binary,
    /// `ascii`.
    Ascii,
    /// `inmemory`.
    InMemory,
}

/// `stl_stats`.
#[derive(Clone, Debug, PartialEq)]
pub struct StlStats {
    /// `header`: `reset_header(size)` makes it `size + 1` zero bytes.
    pub header: Vec<u8>,
    /// `type`.
    pub type_: StlType,
    /// `number_of_facets`.
    pub number_of_facets: u32,
    /// `max`.
    pub max: StlVertex,
    /// `min`.
    pub min: StlVertex,
    /// `size`.
    pub size: StlVertex,
    /// `bounding_diameter`.
    pub bounding_diameter: f32,
    /// `shortest_edge`.
    pub shortest_edge: f32,
    /// `volume`, -1 until computed.
    pub volume: f32,
    /// `connected_edges`.
    pub connected_edges: i32,
    /// `connected_facets_1_edge`.
    pub connected_facets_1_edge: i32,
    /// `connected_facets_2_edge`.
    pub connected_facets_2_edge: i32,
    /// `connected_facets_3_edge`.
    pub connected_facets_3_edge: i32,
    /// `facets_w_1_bad_edge`.
    pub facets_w_1_bad_edge: i32,
    /// `facets_w_2_bad_edge`.
    pub facets_w_2_bad_edge: i32,
    /// `facets_w_3_bad_edge`.
    pub facets_w_3_bad_edge: i32,
    /// `original_num_facets`.
    pub original_num_facets: i32,
    /// `edges_fixed`.
    pub edges_fixed: i32,
    /// `degenerate_facets`.
    pub degenerate_facets: i32,
    /// `facets_removed`.
    pub facets_removed: i32,
    /// `facets_added`.
    pub facets_added: i32,
    /// `facets_reversed`.
    pub facets_reversed: i32,
    /// `backwards_edges`.
    pub backwards_edges: i32,
    /// `normals_fixed`.
    pub normals_fixed: i32,
    /// `number_of_parts`.
    pub number_of_parts: i32,
}

impl Default for StlStats {
    fn default() -> Self {
        StlStats {
            header: Vec::new(),
            type_: StlType::Binary,
            number_of_facets: 0,
            max: Vec3f::zero(),
            min: Vec3f::zero(),
            size: Vec3f::zero(),
            bounding_diameter: 0.0,
            shortest_edge: 0.0,
            volume: -1.0,
            connected_edges: 0,
            connected_facets_1_edge: 0,
            connected_facets_2_edge: 0,
            connected_facets_3_edge: 0,
            facets_w_1_bad_edge: 0,
            facets_w_2_bad_edge: 0,
            facets_w_3_bad_edge: 0,
            original_num_facets: 0,
            edges_fixed: 0,
            degenerate_facets: 0,
            facets_removed: 0,
            facets_added: 0,
            facets_reversed: 0,
            backwards_edges: 0,
            normals_fixed: 0,
            number_of_parts: 0,
        }
    }
}

impl StlStats {
    /// `reset_header(size)`: `size + 1` zero bytes.
    pub fn reset_header(&mut self, size: i32) {
        self.header.clear();
        self.header
            .resize(usize::try_from(size).unwrap_or(0) + 1, 0);
    }

    /// `clear()`.
    pub fn clear(&mut self) {
        *self = StlStats::default();
    }
}

/// `stl_file`: the facets, their neighbours and the statistics.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StlFile {
    /// `facet_start`.
    pub facet_start: Vec<StlFacet>,
    /// `neighbors_start`.
    pub neighbors_start: Vec<StlNeighbors>,
    /// `stats`.
    pub stats: StlStats,
}

impl StlFile {
    /// `clear()`.
    pub fn clear(&mut self) {
        self.facet_start.clear();
        self.neighbors_start.clear();
        self.stats.clear();
    }

    /// The number of facets, `stats.number_of_facets`, as an index bound.
    #[must_use]
    pub fn facets(&self) -> usize {
        self.stats.number_of_facets as usize
    }
}

/// `EnumFaceTypes`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FaceType {
    /// `eNormal` (0).
    #[default]
    Normal,
    /// `eSmallOverhang`.
    SmallOverhang,
    /// `eSmallHole`.
    SmallHole,
    /// `eExteriorAppearance`.
    ExteriorAppearance,
}

/// `FaceProperty`: a face's type and area, value-initialised to `eNormal` and 0.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct FaceProperty {
    /// `type`.
    pub type_: FaceType,
    /// `area`.
    pub area: f64,
}

/// `indexed_triangle_set`: shared vertices and the faces that index them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IndexedTriangleSet {
    /// `indices`.
    pub indices: Vec<StlTriangleVertexIndices>,
    /// `vertices`.
    pub vertices: Vec<StlVertex>,
    /// `properties`: as many as the faces when made by the two-argument constructor, empty when
    /// default-constructed and filled in place (`stl_generate_shared_vertices`).
    pub properties: Vec<FaceProperty>,
}

impl IndexedTriangleSet {
    /// `indexed_triangle_set(indices, vertices)`: with a value-initialised property per face.
    #[must_use]
    pub fn new(indices: Vec<StlTriangleVertexIndices>, vertices: Vec<StlVertex>) -> Self {
        let properties = vec![FaceProperty::default(); indices.len()];
        IndexedTriangleSet {
            indices,
            vertices,
            properties,
        }
    }

    /// `clear()`.
    pub fn clear(&mut self) {
        self.indices.clear();
        self.vertices.clear();
        self.properties.clear();
    }

    /// `empty()`: no faces or no vertices.
    #[must_use]
    pub fn empty(&self) -> bool {
        self.indices.is_empty() || self.vertices.is_empty()
    }

    /// `get_vertex(facet_idx, vertex_idx)`.
    #[must_use]
    pub fn get_vertex(&self, facet_idx: usize, vertex_idx: usize) -> StlVertex {
        self.vertices[self.indices[facet_idx][vertex_idx] as usize]
    }
}

/// `stl_calculate_normal`: `(v1 - v0).cross(v2 - v0)`.
#[must_use]
pub fn stl_calculate_normal(facet: &StlFacet) -> StlNormal {
    (facet.vertex[1] - facet.vertex[0]).cross(&(facet.vertex[2] - facet.vertex[0]))
}

/// `stl_normalize_vector`: zero when the length (`normal.cast<double>().norm()`) is below 1e-12,
/// else each coordinate times `float(1.0 / length)`.
#[must_use]
pub fn stl_normalize_vector(normal: StlNormal) -> StlNormal {
    let length = normal.cast::<f64>().norm();
    if length < 0.000_000_000_001 {
        Vec3f::zero()
    } else {
        normal * ((1.0 / length) as f32)
    }
}

/// `std::max(d(0), std::max(d(1), d(2)))`.
pub(crate) fn max3(d: StlVertex) -> f32 {
    orca_stl::max(d[0], orca_stl::max(d[1], d[2]))
}

/// A `float` times -1, as `x *= -1.f` (`mulss`) or `x *= -1.0` (`mulsd` between conversions)
/// computes it on x64: any number negated exactly, a NaN returned quieted with its sign and
/// payload. A plain `-x` (or `x * -1.0`, which LLVM folds into it) would flip a NaN's sign.
#[must_use]
pub fn times_minus_one(x: f32) -> f32 {
    if x.is_nan() {
        f32::from_bits(x.to_bits() | 0x0040_0000)
    } else {
        -x
    }
}
