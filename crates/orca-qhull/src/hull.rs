// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The run as `its_convex_hull` makes it: `orgQhull::Qhull`'s `runQhull("", 3, n, points, "Qt")`
//! (`Qhull.cpp`), which sets Qhull up (`qh_initqhull_start2`, `qh_init_B`), builds the hull
//! (`qh_qhull`), triangulates it (`qh_prepare_output`), and leaves the facets that
//! `QhullFacetList` walks and the messages `~Qhull` prints.
//!
//! Carries Qhull code: `qh_initqhull_start2`, `qh_init_B` and `qh_initqhull_globals` of Qhull
//! 2015.2's `global_r.c`, `qh_qhull` of `libqhull_r.c`, `qh_check_output` of `poly2_r.c`,
//! `qh_prepare_output` of `io_r.c`, `qh_rand` and `qh_srand` of `random_r.c` (in the tests) and
//! `Qhull::runQhull` of `libqhullcpp`'s `Qhull.cpp`, translated into Rust on 2026-11-07,
//! Copyright (c) 1993-2015 C.B. Barber and The Geometry Center, University of Minnesota, under
//! Qhull's licence (`licenses/Qhull.txt`); the copyright holders are named in `NOTICE`.
//!
//! `qh_initqhull_globals` draws 1000 numbers from `qh_rand` seeded with 1 (`ROTATErandom` is
//! unset) and warns (message 8037) when their average is far from half of `qh_RANDOMmax`; it never
//! is, which the tests check, and the seed is put back to 1, so the run starts from 1 here. No
//! option of the run draws a random number after that.

use crate::qh::{DIM, Error, ID_UNKNOWN, QResult, Qh, REAL_MAX, REAL_MIN, fmax_};
use crate::set::NIL;

/// A vertex of a facet, as `orgQhull::QhullVertex` gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Vertex {
    /// `vertex->id`: Qhull numbers its vertices from 0 as it makes them.
    pub id: u32,
    /// The index of the vertex's point in the input (`qh_pointid(vertex->point)`).
    pub point: usize,
}

/// A facet of the hull, as `orgQhull::QhullFacet` gives it.
#[derive(Clone, Debug, PartialEq)]
pub struct Facet {
    /// `facet->id`: Qhull numbers its facets from 0 as it makes them.
    pub id: u32,
    /// `facet->vertices`, in the set's order: by id, the highest first (after `Qt`, three).
    pub vertices: Vec<Vertex>,
    /// `facet->normal`: the facet's outward unit normal (a triangle of a triangulated facet has
    /// the facet's; see `poly.rs` for the one case where Qhull reads freed memory there).
    pub normal: [f64; 3],
}

/// What a run leaves: the facets and Qhull's messages.
#[derive(Clone, Debug, PartialEq)]
pub struct Hull {
    /// The facets from `qh.facet_list` to `qh.facet_tail`, the order `QhullFacetList` walks them.
    pub facets: Vec<Facet>,
    /// `qhull_message`: what Qhull printed during a run that succeeded, which `~Qhull` writes to
    /// `std::cerr` after `"\nQhull output at end\n"` when it is not empty. With the options of
    /// `its_convex_hull` the only such message is the narrow-hull warning (9375).
    pub message: String,
}

/// `runQhull("", 3, points.len(), points, "Qt")` on a new `orgQhull::Qhull`: the convex hull of
/// the points, triangulated, as `its_convex_hull` asks for it.
///
/// # Errors
///
/// The [`Error::Qhull`] code of the error that ends the run (`qh_errexit`), on which the C++
/// throws a `QhullError` and clears the messages: 6214 for fewer than 4 points, 6154 when the
/// initial simplex is flat (the points are coplanar), 6006 when the points are not finite, and
/// the precision and internal errors of the build. [`Error::Unported`] would name a branch the
/// options never reach.
pub fn convex_hull(points: &[[f64; 3]]) -> Result<Hull, Error> {
    let mut qh = Qh::new(points)?;
    qh.qhull()?;
    // qh_check_output: not VERIFYoutput, IStracing or CHECKfrequently, and MERGING.
    qh.prepare_output()?;
    // Not VERIFYoutput: no qh_check_points.
    Ok(qh.output())
}

impl Qh {
    /// The `qhT` of a new `orgQhull::QhullQh` (`qh_initqhull_start2`: every field 0, then its
    /// defaults) after `qh_init_B` with the points (`qh_initqhull_globals`,
    /// `qh_initqhull_buffers`; `qh_initthresholds` finds no `P` option, and no option projects,
    /// scales or rotates the points).
    ///
    /// # Errors
    ///
    /// Qhull's input error 6214 for fewer than `hull_dim + 1` points.
    fn new(points: &[[f64; 3]]) -> QResult<Qh> {
        // qh_initqhull_globals: not NOpremerge, MERGEexact or PREmerge, no joggle, so MERGING
        // and (hull_dim <= 4) PREmerge; then, as POSTmerge is unset, premerge_cos is REALmax
        // and premerge_centrum 0, ZEROcentrum ('C-0') with ZEROall_ok.
        let merging = true;
        let docheckmax = merging; // Not SKIPcheckmax.
        let keep_nearinside = docheckmax; // Not KEEPinside with KEEPcoplanar, not NOnearinside.
        if points.len() < DIM + 1 {
            // After the seeding of qh_rand (see the module notes); GOODpoint is unset.
            return Err(Error::Qhull(6214));
        }
        Ok(Qh {
            points: points.to_vec(),
            num_points: i32::try_from(points.len()).expect("the C++ passes the count as an int"),
            interior_point: [0.0; 3],
            facets: Vec::new(),
            vertices: Vec::new(),
            ridges: Vec::new(),
            merges: Vec::new(),
            facet_list: NIL,
            facet_tail: NIL,
            facet_next: NIL,
            newfacet_list: NIL,
            visible_list: NIL,
            vertex_list: NIL,
            vertex_tail: NIL,
            newvertex_list: NIL,
            num_facets: 0,
            num_vertices: 0,
            num_visible: 0,
            num_outside: 0,
            facet_id: 0,
            vertex_id: 0,
            visit_id: 0,
            vertex_visit: 0,
            furthest_id: ID_UNKNOWN,
            // qh_initqhull_buffers.
            other_points: Some(Vec::new()),
            del_vertices: Some(Vec::new()),
            coplanarfacetset: Some(Vec::new()),
            facet_mergeset: None,
            degen_mergeset: None,
            hash_table: None,
            dist_round: 0.0,
            min_denom: 0.0,
            min_denom_1: fmax_(1.0 / REAL_MAX, REAL_MIN),
            min_denom_1_2: 0.0,
            min_denom_2: 0.0,
            angle_round: 0.0,
            premerge_centrum: 0.0,
            postmerge_centrum: 0.0,
            premerge_cos: REAL_MAX,
            postmerge_cos: REAL_MAX,
            one_merge: 0.0,
            near_inside: 0.0,
            min_visible: REAL_MAX,
            max_coplanar: REAL_MAX,
            min_outside: 0.0,
            wide_facet: 0.0,
            max_outside: 0.0,
            max_vertex: 0.0,
            min_vertex: 0.0,
            max_abs_coord: 0.0,
            max_sum_coord: 0.0,
            max_width: -REAL_MAX,
            near_zero: [0.0; 3],
            cos_max: 0.0,
            centrum_radius: 0.0,
            joggle_max: REAL_MAX,
            merging,
            zero_centrum: true,
            zero_all_ok: true,
            docheckmax,
            keep_nearinside,
            narrow_hull: false,
            was_coplanar: false,
            vertex_neighbors: false,
            new_facets: false,
            findbestnew: false,
            findbest_notsharp: false,
            maxoutdone: false,
            has_triangulation: false,
            triangulate: true,
            ztotmerge: 0,
            zsetplane: 0,
            message: String::new(),
        })
    }

    /// `qh_qhull`: builds the hull (no joggle or rerun), then, unless every new facet was
    /// clearly convex and no point was coplanar, checks the outer planes against the points
    /// (`DOcheckmax`) and drops the coplanar points that are not near (`KEEPnearinside`).
    ///
    /// # Errors
    ///
    /// Those of the build.
    fn qhull(&mut self) -> QResult<()> {
        self.initbuild()?;
        self.buildhull()?;
        // No STOPpoint or STOPcone; not MERGEexact or TESTvneighbors.
        if self.zero_all_ok && !self.was_coplanar {
            self.docheckmax = false;
        } else if self.visible_list == self.facet_list {
            // No post-merge runs (hull_dim <= qh_DIMreduceBuild, no POSTmerge), and the lists
            // were reset after the last point, so the visible list is empty.
            return Err(Error::Unported("qh_qhull's partition after a post-merge"));
        }
        if self.docheckmax {
            // No REPORTfreq.
            self.check_maxout();
        }
        if self.keep_nearinside && !self.maxoutdone {
            self.nearcoplanar();
        }
        // The temporary sets are freed where they are made: no error 6164.
        Ok(())
    }

    /// `qh_prepare_output`: the triangulation (`TRIangulate`); not `VORONOI`, `GETarea` or
    /// `PRINTstatistics`, nothing kept by area or merges, and `qh_findgood_all` only counts
    /// the facets (no good point, vertex or threshold).
    ///
    /// # Errors
    ///
    /// As [`Qh::triangulate`].
    fn prepare_output(&mut self) -> QResult<()> {
        if self.triangulate && !self.has_triangulation {
            self.triangulate()?;
        }
        Ok(())
    }

    /// The facets as `QhullFacetList` walks them, with their vertices and normals.
    fn output(&self) -> Hull {
        let mut facets = Vec::new();
        let mut facet = self.facet_list;
        while facet != self.facet_tail {
            let f = &self.facets[facet as usize];
            let vertices = f
                .vertices
                .iter()
                .flatten()
                .take_while(|&&v| v != NIL)
                .map(|&v| {
                    let vertex = &self.vertices[v as usize];
                    Vertex {
                        id: vertex.id,
                        point: vertex.point as usize,
                    }
                })
                .collect();
            facets.push(Facet {
                id: f.id,
                vertices,
                normal: f.normal.expect("a facet of the hull has a hyperplane"),
            });
            facet = f.next;
        }
        Hull {
            facets,
            message: self.message.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Error, convex_hull};

    /// `qh_rand` (`random_r.c`): Park and Miller's minimal standard generator.
    fn qh_rand(last_random: &mut i32) -> i32 {
        const A: i32 = 16807;
        const M: i32 = 2_147_483_647;
        const Q: i32 = 127_773;
        const R: i32 = 2836;
        let seed = *last_random;
        let hi = seed / Q;
        let lo = seed % Q;
        let test = A * lo - R * hi;
        let seed = if test > 0 { test } else { test + M };
        *last_random = seed;
        seed
    }

    #[test]
    fn the_random_check_of_qh_initqhull_globals_passes() {
        // qh_srand(1); 1000 draws; neither 8036 (a draw above qh_RANDOMmax) nor 8037.
        const RANDOM_MAX: f64 = 2_147_483_646.0;
        let mut last_random = 1;
        let mut randr = 0.0;
        for _ in 0..1000 {
            let randi = qh_rand(&mut last_random);
            randr += f64::from(randi);
            assert!(f64::from(randi) <= RANDOM_MAX);
        }
        randr /= 1000.0;
        assert!(
            (RANDOM_MAX * 0.1..=RANDOM_MAX * 0.9).contains(&randr),
            "{randr}"
        );
    }

    #[test]
    fn too_few_points_end_the_run() {
        let points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        assert_eq!(convex_hull(&points), Err(Error::Qhull(6214)));
    }

    #[test]
    fn coplanar_points_end_the_run() {
        let points = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
        ];
        assert_eq!(convex_hull(&points), Err(Error::Qhull(6154)));
    }

    #[test]
    fn infinite_points_end_the_run() {
        let points = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, f64::INFINITY],
        ];
        assert_eq!(convex_hull(&points), Err(Error::Qhull(6006)));
    }

    #[test]
    fn a_cube_has_twelve_triangles() {
        let mut points = Vec::new();
        for i in 0..8 {
            points.push([
                f64::from(i & 1) * 20.0,
                f64::from((i >> 1) & 1) * 20.0,
                f64::from((i >> 2) & 1) * 20.0,
            ]);
        }
        let hull = convex_hull(&points).expect("a cube's hull");
        assert_eq!(hull.facets.len(), 12);
        assert!(hull.message.is_empty());
        for facet in &hull.facets {
            assert_eq!(facet.vertices.len(), 3);
            assert!(facet.vertices.windows(2).all(|w| w[0].id > w[1].id));
            let n = facet.normal;
            assert!((n[0] * n[0] + n[1] * n[1] + n[2] * n[2] - 1.0).abs() < 1e-12);
        }
    }

    #[test]
    fn a_tetrahedron_keeps_its_four_points() {
        let points = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        let hull = convex_hull(&points).expect("a tetrahedron's hull");
        assert_eq!(hull.facets.len(), 4);
        let ids: std::collections::BTreeSet<u32> = hull
            .facets
            .iter()
            .flat_map(|f| f.vertices.iter().map(|v| v.id))
            .collect();
        assert_eq!(ids.into_iter().collect::<Vec<_>>(), [0, 1, 2, 3]);
    }

    #[test]
    fn a_narrow_hull_leaves_the_warning() {
        // A flat tetrahedron: its base and a side meet at an angle whose cosine, about
        // -1 + 2e-16, is below qh_WARNnarrow.
        let points = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.5, 0.5, 1e-8],
        ];
        let hull = convex_hull(&points).expect("a thin tetrahedron's hull");
        assert!(
            hull.message.starts_with(
                "qhull precision warning: \nThe initial hull is narrow (cosine of min. angle is 0.99999"
            ),
            "{}",
            hull.message
        );
        assert!(
            hull.message
                .ends_with("See 'Limitations' in qh-impre.htm.\n")
        );
    }
}
