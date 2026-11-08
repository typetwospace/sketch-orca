// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Qhull's polyhedron (`poly_r.c`, `poly2_r.c`): the initial simplex, the cone of new facets for
//! a point and their neighbours, the vertex lists, the final check of the outer planes and the
//! triangulation of non-simplicial facets (`Qt`).
//!
//! Carries Qhull code: the functions of Qhull 2015.2's `poly_r.c` and `poly2_r.c` that a 3-d hull
//! with the options of `its_convex_hull` reaches, translated into Rust on 2026-11-07, Copyright
//! (c) 1993-2015 C.B. Barber and The Geometry Center, University of Minnesota, under Qhull's
//! licence (`licenses/Qhull.txt`); the copyright holders are named in `NOTICE`.
//!
//! Two things here are not Qhull's to the bit, as Qhull itself is not repeatable there:
//!
//! - `qh_gethash` adds the addresses of vertices, which ASLR moves from run to run. The vertex
//!   ids stand in for them. Matching a ridge to its two facets compares vertices, so the hash only
//!   decides where a facet sits in the table; only a ridge of more than two new facets (a
//!   duplicate ridge, `qh_matchduplicates`) is resolved in table order, and there the result can
//!   differ from the C, as it can between two runs of the C.
//! - When `qh_triangulate` deletes a non-simplicial facet none of whose triangles takes its
//!   hyperplane over (all of them degenerate), those triangles keep a pointer to the freed normal,
//!   whose first coordinate Qhull's memory pool has overwritten with the next free block's address
//!   (a positive subnormal number, or 0). The translation writes +0.0 there; `its_convex_hull`
//!   reads the normal only for the sign of a dot product, which the term changes only when the
//!   other two terms sum to exactly 0.

use crate::geom::{getangle, pointdist};
use crate::qh::{
    DIM, DUPLICATE_RIDGE, Error, MERGE_RIDGE, MRG_MIRROR, QResult, Qh, REAL_MAX, minimize_,
};
use crate::set::{self, NIL, Set};
use orca_crt::sqrt;

/// `qh_MAXnarrow`: below it the initial hull is narrow (`NARROWhull`).
const MAX_NARROW: f64 = -0.999_999_99;
/// `qh_WARNnarrow`: below it Qhull warns about the narrow hull (message 9375).
const WARN_NARROW: f64 = -0.999_999_999_999_999;
/// `qh_WIDEduplicate`: the widest merge a duplicate ridge may cause (error 6271).
const WIDE_DUPLICATE: f64 = 100.0;
/// The format of message 9375 (`qh_printhelp_narrowhull`, `user_r.c`), which `qh_fprintf`
/// prints with the C runtime's `vsnprintf`.
const NARROW_HULL_FORMAT: &[u8] = b"qhull precision warning: \nThe initial hull is narrow (cosine \
    of min. angle is %.16f).\nIs the input lower dimensional (e.g., on a plane in 3-d)?  Qhull \
    may\nproduce a wide facet.  Options 'QbB' (scale to unit box) or 'Qbb' (scale\nlast \
    coordinate) may remove this warning.  Use 'Pp' to skip this warning.\nSee 'Limitations' in \
    qh-impre.htm.\n";

/// `qh_setreplace` with Qhull's error.
fn replace(set: &mut Set, old: u32, new: u32) -> QResult<()> {
    set::replace(set, old, new).map_err(Error::Qhull)
}

impl Qh {
    /// `qh_checkflipped(facet, distp, allerror)`: whether the facet is not flipped (the interior
    /// point below it, by more than `DISTround` with `allerror`); a flipped facet is marked.
    /// `distp` receives the distance when given (and an already flipped facet is measured
    /// again only then).
    pub(crate) fn checkflipped(
        &mut self,
        facet: u32,
        distp: Option<&mut f64>,
        allerror: bool,
    ) -> bool {
        let f = facet as usize;
        if self.facets[f].flipped && distp.is_none() {
            return false;
        }
        let interior = self.interior_point;
        let dist = self.distplane(&interior, facet);
        if let Some(d) = distp {
            *d = dist;
        }
        if (allerror && dist > -self.dist_round) || (!allerror && dist >= 0.0) {
            self.facets[f].flipped = true;
            // qh_precision: no restart, as PREmerge is set.
            return false;
        }
        true
    }

    /// `qh_createsimplex(vertices)`: the facets of the initial simplex, with the list sentinels.
    ///
    /// # Errors
    ///
    /// As [`Qh::newvertex`].
    fn createsimplex(&mut self, vertices: &Set) -> QResult<()> {
        let mut toporient = true;
        let sentinel = self.newfacet();
        self.facet_list = sentinel;
        self.newfacet_list = sentinel;
        self.facet_tail = sentinel;
        self.num_facets = 0;
        self.num_vertices = 0;
        self.num_visible = 0;
        let vsentinel = self.newvertex(NIL)?;
        self.vertex_list = vsentinel;
        self.newvertex_list = vsentinel;
        self.vertex_tail = vsentinel;
        let vertex_n = set::size(vertices);
        for vertex_i in 0..vertex_n {
            let vertex = set::elem(vertices, vertex_i);
            let newfacet = self.newfacet();
            let nf = newfacet as usize;
            self.facets[nf].vertices = set::new_del_nth_sorted(vertices, vertex_n, vertex_i, 0);
            self.facets[nf].toporient = toporient;
            self.appendfacet(newfacet);
            self.facets[nf].newfacet = true;
            self.appendvertex(vertex);
            toporient ^= true;
        }
        let mut newfacet = self.newfacet_list;
        while newfacet != NIL && self.facets[newfacet as usize].next != NIL {
            // SETelem_(newfacet->neighbors, nth++)= facet, then qh_settruncate to hull_dim.
            let mut neighbors = Vec::with_capacity(DIM);
            let mut facet = self.newfacet_list;
            while facet != NIL && self.facets[facet as usize].next != NIL {
                if facet != newfacet {
                    neighbors.push(facet);
                }
                facet = self.facets[facet as usize].next;
            }
            neighbors.truncate(DIM);
            self.facets[newfacet as usize].neighbors = Some(neighbors);
            newfacet = self.facets[newfacet as usize].next;
        }
        Ok(())
    }

    /// `qh_initialvertices(dim, maxpoints, points, numpoints)`: the vertices of the initial
    /// simplex (`qh_maxsimplex`: a 3-d hull is below `qh_INITIALmax`), inverse sorted by id.
    ///
    /// # Errors
    ///
    /// As [`Qh::maxsimplex`] and [`Qh::newvertex`].
    pub(crate) fn initialvertices(&mut self, dim: usize, maxpoints: &Set) -> QResult<Set> {
        let mut simplex: Set = Some(Vec::with_capacity(dim + 1));
        self.maxsimplex(dim, maxpoints, &mut simplex)?;
        let mut vertices: Set = Some(Vec::with_capacity(dim + 1));
        let mut i = 0;
        loop {
            let point = set::elem(&simplex, i);
            i += 1;
            if point == NIL {
                break;
            }
            let vertex = self.newvertex(point)?;
            set::add_nth(&mut vertices, 0, vertex);
        }
        Ok(vertices)
    }

    /// `qh_initialhull(vertices)`: the initial simplex, oriented with the interior point inside.
    ///
    /// # Errors
    ///
    /// Qhull's error 6154 for a flat simplex and 6114 for one that is not clearly convex.
    pub(crate) fn initialhull(&mut self, vertices: &Set) -> QResult<()> {
        let mut minangle = REAL_MAX;
        self.createsimplex(vertices)?;
        self.resetlists(true);
        self.facet_next = self.facet_list;
        self.interior_point = self.getcenter(vertices)?;
        let firstfacet = self.facet_list;
        self.setfacetplane(firstfacet)?;
        let interior = self.interior_point;
        let dist = self.distplane(&interior, firstfacet);
        if dist > 0.0 {
            let mut facet = self.facet_list;
            while facet != NIL && self.facets[facet as usize].next != NIL {
                self.facets[facet as usize].toporient ^= true;
                facet = self.facets[facet as usize].next;
            }
        }
        let mut facet = self.facet_list;
        while facet != NIL && self.facets[facet as usize].next != NIL {
            self.setfacetplane(facet)?;
            facet = self.facets[facet as usize].next;
        }
        let mut facet = self.facet_list;
        while facet != NIL && self.facets[facet as usize].next != NIL {
            if !self.checkflipped(facet, None, true) {
                self.facets[facet as usize].flipped = false;
                let mut other = self.facet_list;
                while other != NIL && self.facets[other as usize].next != NIL {
                    self.facets[other as usize].toporient ^= true;
                    self.orientoutside(other);
                    other = self.facets[other as usize].next;
                }
                break;
            }
            facet = self.facets[facet as usize].next;
        }
        let mut facet = self.facet_list;
        while facet != NIL && self.facets[facet as usize].next != NIL {
            if !self.checkflipped(facet, None, false) {
                // Not DELAUNAY.
                return Err(Error::Qhull(6154));
            }
            let normal = self.facets[facet as usize]
                .normal
                .expect("a facet of the simplex");
            let mut i = 0;
            loop {
                let neighbor = set::elem(&self.facets[facet as usize].neighbors, i);
                i += 1;
                if neighbor == NIL {
                    break;
                }
                let other = self.facets[neighbor as usize]
                    .normal
                    .expect("a facet of the simplex");
                let angle = getangle(&normal, &other);
                minimize_(&mut minangle, angle);
            }
            facet = self.facets[facet as usize].next;
        }
        if minangle < MAX_NARROW {
            // NOnarrow is False; qh_option only adds to qhull_options.
            self.narrow_hull = true;
            if minangle < WARN_NARROW {
                // !RERUN && PRINTprecision: qh_printhelp_narrowhull, the cosine as the angle
                // between the facets (-minangle).
                let text = orca_fmt::printf::sprintf_string(
                    std::str::from_utf8(NARROW_HULL_FORMAT).expect("an ASCII format"),
                    &[orca_fmt::printf::Arg::Double(-minangle)],
                )
                .expect("message 9375's format takes one double");
                self.append_message(&text);
            }
        }
        self.checkpolygon_initial();
        self.checkconvex_initial()
    }

    /// What `qh_checkpolygon(qh.facet_list)` changes on the initial simplex: its checks hold by
    /// construction there (four simplicial facets of three vertices in descending id order, each
    /// the neighbour of the others), so only its marks remain. `qh_checkfacet` leaves each
    /// neighbour's `seen` False; every vertex gets `seen` and a `visitid` that counts its facets,
    /// and `vertex_visit` moves past them all.
    fn checkpolygon_initial(&mut self) {
        let mut numfacets: u32 = 0;
        let mut facet = self.facet_list;
        while facet != NIL && self.facets[facet as usize].next != NIL {
            if !self.facets[facet as usize].visible {
                numfacets += 1;
                let mut i = 0;
                loop {
                    let neighbor = set::elem(&self.facets[facet as usize].neighbors, i);
                    i += 1;
                    if neighbor == NIL {
                        break;
                    }
                    self.facets[neighbor as usize].seen = false;
                }
            }
            facet = self.facets[facet as usize].next;
        }
        let mut vertex = self.vertex_list;
        while vertex != NIL && self.vertices[vertex as usize].next != NIL {
            self.vertices[vertex as usize].seen = false;
            self.vertices[vertex as usize].visitid = 0;
            vertex = self.vertices[vertex as usize].next;
        }
        let mut facet = self.facet_list;
        while facet != NIL && self.facets[facet as usize].next != NIL {
            if !self.facets[facet as usize].visible {
                let mut i = 0;
                loop {
                    let vertex = set::elem(&self.facets[facet as usize].vertices, i);
                    i += 1;
                    if vertex == NIL {
                        break;
                    }
                    let v = &mut self.vertices[vertex as usize];
                    v.visitid = v.visitid.wrapping_add(1);
                    v.seen = true;
                }
            }
            facet = self.facets[facet as usize].next;
        }
        self.vertex_visit = self.vertex_visit.wrapping_add(numfacets);
        self.vertex_visit = self.vertex_visit.wrapping_add(1);
    }

    /// `qh_checkconvex(qh.facet_list, qh_DATAfault)` on the initial simplex: no facet is flipped
    /// (`qh_initialhull` has stopped at one), and its simplicial facets are tested by the vertex
    /// opposite each neighbour (`ZEROcentrum`), so the centrum test does not run.
    ///
    /// # Errors
    ///
    /// Qhull's error 6114 when a vertex is not clearly below a neighbouring facet.
    fn checkconvex_initial(&mut self) -> QResult<()> {
        let mut facet = self.facet_list;
        while facet != NIL && self.facets[facet as usize].next != NIL {
            let f = facet as usize;
            let mut neighbor_i = 0;
            let mut i = 0;
            loop {
                let neighbor = set::elem(&self.facets[f].neighbors, i);
                i += 1;
                if neighbor == NIL {
                    break;
                }
                let vertex = set::elem(&self.facets[f].vertices, neighbor_i);
                neighbor_i += 1;
                let n = &self.facets[neighbor as usize];
                if !n.simplicial || n.tricoplanar {
                    continue;
                }
                let dist = self.distplane_point(self.vertices[vertex as usize].point, neighbor);
                if dist > -self.dist_round {
                    return Err(Error::Qhull(6114));
                }
            }
            facet = self.facets[f].next;
        }
        Ok(())
    }

    /// `qh_makenewfacets(point)`: the cone of new facets from the point's new vertex (the apex)
    /// to the horizon of the visible facets, which are attached to them (not `ONLYgood`).
    ///
    /// # Errors
    ///
    /// As [`Qh::newvertex`] and the internal errors of the facet makers.
    pub(crate) fn makenewfacets(&mut self, point: u32) -> QResult<u32> {
        let mut newfacet = NIL;
        let mut newfacet2 = NIL;
        let mut numnew = 0;
        self.newfacet_list = self.facet_tail;
        self.newvertex_list = self.vertex_tail;
        let apex = self.newvertex(point)?;
        self.appendvertex(apex);
        self.visit_id = self.visit_id.wrapping_add(1);
        self.new_facets = true;
        let mut visible = self.visible_list;
        while visible != NIL && self.facets[visible as usize].visible {
            let v = visible as usize;
            let mut i = 0;
            loop {
                let neighbor = set::elem(&self.facets[v].neighbors, i);
                i += 1;
                if neighbor == NIL {
                    break;
                }
                self.facets[neighbor as usize].seen = false;
            }
            if self.facets[v].ridges.is_some() {
                self.facets[v].visitid = self.visit_id;
                newfacet2 = self.makenew_nonsimplicial(visible, apex, &mut numnew)?;
            }
            if self.facets[v].simplicial {
                newfacet = self.makenew_simplicial(visible, apex, &mut numnew)?;
            }
            // Not ONLYgood. The two results carry over from one visible facet to the next.
            if newfacet2 != NIL {
                newfacet = newfacet2;
            }
            if newfacet != NIL {
                self.facets[v].f = newfacet;
            }
            if let Some(n) = self.facets[v].neighbors.as_mut()
                && !n.is_empty()
            {
                n[0] = NIL;
            }
            visible = self.facets[v].next;
        }
        Ok(apex)
    }

    /// `qh_makenew_nonsimplicial(visible, apex, &numnew)`: a new facet for each ridge of the
    /// visible facet to a horizon facet; returns the last new facet.
    ///
    /// # Errors
    ///
    /// Qhull's internal errors 6105 and 6177.
    fn makenew_nonsimplicial(&mut self, visible: u32, apex: u32, numnew: &mut i32) -> QResult<u32> {
        let mut newfacet = NIL;
        let v = visible as usize;
        let mut i = 0;
        loop {
            let ridge = set::elem(&self.facets[v].ridges, i);
            i += 1;
            if ridge == NIL {
                break;
            }
            let neighbor = self.otherfacet(ridge, visible);
            let n = neighbor as usize;
            if self.facets[n].visible {
                // Not ONLYgood.
                if self.facets[n].visitid == self.visit_id {
                    self.freeridge(ridge);
                }
            } else {
                let toporient = self.ridges[ridge as usize].top == visible;
                let mut vertices: Set = Some(Vec::with_capacity(DIM));
                set::append(&mut vertices, apex);
                set::append_set(&mut vertices, &self.ridges[ridge as usize].vertices);
                newfacet = self.makenewfacet(vertices, toporient, neighbor);
                *numnew += 1;
                if self.facets[n].coplanar {
                    self.facets[newfacet as usize].mergehorizon = true;
                    if self.facets[n].seen {
                        let samecycle = self.facets[n].f;
                        self.facets[newfacet as usize].f = self.facets[samecycle as usize].f;
                        self.facets[samecycle as usize].f = newfacet;
                    } else {
                        self.facets[newfacet as usize].f = newfacet;
                        self.facets[n].f = newfacet;
                    }
                }
                if self.facets[n].seen {
                    if self.facets[n].simplicial {
                        return Err(Error::Qhull(6105));
                    }
                    set::append(&mut self.facets[n].neighbors, newfacet);
                } else {
                    replace(&mut self.facets[n].neighbors, visible, newfacet)?;
                }
                if self.facets[n].simplicial {
                    set::del(&mut self.facets[n].ridges, ridge);
                    self.freeridge(ridge);
                } else {
                    set::append(&mut self.facets[newfacet as usize].ridges, ridge);
                    if toporient {
                        self.ridges[ridge as usize].top = newfacet;
                    } else {
                        self.ridges[ridge as usize].bottom = newfacet;
                    }
                }
            }
            self.facets[n].seen = true;
        }
        if let Some(r) = self.facets[v].ridges.as_mut()
            && !r.is_empty()
        {
            r[0] = NIL;
        }
        Ok(newfacet)
    }

    /// `qh_makenew_simplicial(visible, apex, &numnew)`: a new facet for each horizon neighbour of
    /// the simplicial visible facet; returns the last new facet.
    ///
    /// # Errors
    ///
    /// Qhull's internal error 6104.
    fn makenew_simplicial(&mut self, visible: u32, apex: u32, numnew: &mut i32) -> QResult<u32> {
        let mut newfacet = NIL;
        let v = visible as usize;
        let mut i = 0;
        loop {
            let neighbor = set::elem(&self.facets[v].neighbors, i);
            i += 1;
            if neighbor == NIL {
                break;
            }
            let n = neighbor as usize;
            if !self.facets[n].seen && !self.facets[n].visible {
                let (mut vertices, horizonskip, _visibleskip) =
                    self.facetintersect(neighbor, visible, 1)?;
                set::set_elem(&mut vertices, 0, apex);
                let toporient = if self.facets[n].toporient {
                    horizonskip & 1 != 0
                } else {
                    (horizonskip & 1) ^ 1 != 0
                };
                newfacet = self.makenewfacet(vertices, toporient, neighbor);
                *numnew += 1;
                if self.facets[n].coplanar {
                    // PREmerge.
                    self.facets[newfacet as usize].f = newfacet;
                    self.facets[newfacet as usize].mergehorizon = true;
                }
                // Not ONLYgood.
                set::set_elem(&mut self.facets[n].neighbors, horizonskip, newfacet);
            }
        }
        Ok(newfacet)
    }

    /// `qh_makenewfacet(vertices, toporient, horizon)`: a new facet of the vertices, whose
    /// vertices move to the new vertex list.
    pub(crate) fn makenewfacet(&mut self, vertices: Set, toporient: bool, horizon: u32) -> u32 {
        let mut i = 0;
        loop {
            let vertex = set::elem(&vertices, i);
            i += 1;
            if vertex == NIL {
                break;
            }
            if !self.vertices[vertex as usize].newlist {
                self.removevertex(vertex);
                self.appendvertex(vertex);
            }
        }
        let newfacet = self.newfacet();
        let nf = newfacet as usize;
        self.facets[nf].vertices = vertices;
        self.facets[nf].toporient = toporient;
        if horizon != NIL {
            set::append(&mut self.facets[nf].neighbors, horizon);
        }
        self.appendfacet(newfacet);
        newfacet
    }

    /// `qh_makenewplanes`: the hyperplanes of the new facets that do not merge into the horizon
    /// (no joggle).
    ///
    /// # Errors
    ///
    /// As [`Qh::setfacetplane`].
    pub(crate) fn makenewplanes(&mut self) -> QResult<()> {
        let mut newfacet = self.newfacet_list;
        while newfacet != NIL && self.facets[newfacet as usize].next != NIL {
            if !self.facets[newfacet as usize].mergehorizon {
                self.setfacetplane(newfacet)?;
            }
            newfacet = self.facets[newfacet as usize].next;
        }
        Ok(())
    }

    /// `qh_facetintersect(facetA, facetB, &skipA, &skipB, prepend)`: the vertices the simplicial
    /// facets share, after `prepend` NULL slots, with each one's index of the other.
    ///
    /// # Errors
    ///
    /// Qhull's internal error 6104 when they are not neighbours.
    pub(crate) fn facetintersect(
        &self,
        facet_a: u32,
        facet_b: u32,
        prepend: usize,
    ) -> QResult<(Set, usize, usize)> {
        let neighbors_a = &self.facets[facet_a as usize].neighbors;
        let neighbors_b = &self.facets[facet_b as usize].neighbors;
        let skip_a = (0..DIM).find(|&k| set::elem(neighbors_a, k) == facet_b);
        let skip_b = (0..DIM).find(|&k| set::elem(neighbors_b, k) == facet_a);
        let (Some(skip_a), Some(skip_b)) = (skip_a, skip_b) else {
            return Err(Error::Qhull(6104));
        };
        let intersect = set::new_del_nth_sorted(
            &self.facets[facet_a as usize].vertices,
            DIM,
            skip_a,
            prepend,
        );
        Ok((intersect, skip_a, skip_b))
    }

    /// `qh_matchnewfacets`: matches the new facets' ridges to each other, through a hash table,
    /// marking and resolving duplicate ridges; checks the new facets for flipping (`PREmerge`).
    ///
    /// # Errors
    ///
    /// Qhull's errors of the matching (6106, 6108, 6155, 6157, 6236, 6260) and of
    /// [`Qh::setfacetplane`].
    pub(crate) fn matchnewfacets(&mut self) -> QResult<()> {
        let mut numnew: i32 = 0;
        let mut hashcount: i32 = 0;
        let mut newfacet = self.newfacet_list;
        while newfacet != NIL && self.facets[newfacet as usize].next != NIL {
            numnew += 1;
            // The inline qh_setzero(neighbors, 1, hull_dim): the size becomes hull_dim, with
            // every slot after the horizon NULL.
            let neighbors = self.facets[newfacet as usize]
                .neighbors
                .as_mut()
                .expect("a new facet's neighbours");
            let horizon = neighbors.first().copied().unwrap_or(NIL);
            neighbors.clear();
            neighbors.push(horizon);
            neighbors.resize(DIM, NIL);
            newfacet = self.facets[newfacet as usize].next;
        }
        let hashsize = self.newhashtable(numnew * (3 - 1))?;
        let mut newfacet = self.newfacet_list;
        while newfacet != NIL && self.facets[newfacet as usize].next != NIL {
            for newskip in 1..DIM {
                self.matchneighbor(newfacet, newskip, hashsize, &mut hashcount)?;
            }
            newfacet = self.facets[newfacet as usize].next;
        }
        if hashcount != 0 {
            let mut newfacet = self.newfacet_list;
            while newfacet != NIL && self.facets[newfacet as usize].next != NIL {
                if self.facets[newfacet as usize].dupridge {
                    let neighbor_n = set::size(&self.facets[newfacet as usize].neighbors);
                    for neighbor_i in 0..neighbor_n {
                        let neighbor =
                            set::elem(&self.facets[newfacet as usize].neighbors, neighbor_i);
                        if neighbor == DUPLICATE_RIDGE {
                            self.matchduplicates(newfacet, neighbor_i, hashsize, &mut hashcount)?;
                        }
                    }
                }
                newfacet = self.facets[newfacet as usize].next;
            }
        }
        if hashcount != 0 {
            return Err(Error::Qhull(6108));
        }
        self.hash_table = None;
        // PREmerge.
        let mut newfacet = self.newfacet_list;
        while newfacet != NIL && self.facets[newfacet as usize].next != NIL {
            if self.facets[newfacet as usize].normal.is_some() {
                self.checkflipped(newfacet, None, true);
            }
            newfacet = self.facets[newfacet as usize].next;
        }
        Ok(())
    }

    /// `qh_matchneighbor(newfacet, newskip, hashsize, &hashcount)`: matches the new facet's ridge
    /// opposite its vertex `newskip` to a facet in the hash table, or adds the facet to it.
    fn matchneighbor(
        &mut self,
        newfacet: u32,
        newskip: usize,
        hashsize: usize,
        hashcount: &mut i32,
    ) -> QResult<()> {
        let nf = newfacet as usize;
        let mut newfound = false;
        let skipelem = set::elem(&self.facets[nf].vertices, newskip);
        let hash = self.gethash(hashsize, &self.facets[nf].vertices, DIM, 1, skipelem)?;
        let mut scan = hash;
        loop {
            let facet = set::elem(&self.hash_table, scan);
            if facet == NIL {
                break;
            }
            if facet == newfacet {
                newfound = true;
            } else if let Some((skip, same)) = self.matchvertices(
                1,
                &self.facets[nf].vertices,
                newskip,
                &self.facets[facet as usize].vertices,
            ) {
                let f = facet as usize;
                if set::elem(&self.facets[nf].vertices, newskip)
                    == set::elem(&self.facets[f].vertices, skip)
                {
                    return Err(Error::Qhull(6106));
                }
                let ismatch = same == (self.facets[nf].toporient ^ self.facets[f].toporient);
                let matchfacet = set::elem(&self.facets[f].neighbors, skip);
                if ismatch && matchfacet == NIL {
                    set::set_elem(&mut self.facets[f].neighbors, skip, newfacet);
                    set::set_elem(&mut self.facets[nf].neighbors, newskip, facet);
                    *hashcount -= 1;
                    return Ok(());
                }
                // PREmerge: a ridge of more than two facets is merged later.
                set::set_elem(&mut self.facets[nf].neighbors, newskip, DUPLICATE_RIDGE);
                self.facets[nf].dupridge = true;
                if self.facets[nf].normal.is_none() {
                    self.setfacetplane(newfacet)?;
                }
                self.addhash(newfacet, hashsize, hash);
                *hashcount += 1;
                if self.facets[f].normal.is_none() {
                    self.setfacetplane(facet)?;
                }
                if matchfacet != DUPLICATE_RIDGE {
                    set::set_elem(&mut self.facets[f].neighbors, skip, DUPLICATE_RIDGE);
                    self.facets[f].dupridge = true;
                    if self.facets[f].normal.is_none() {
                        self.setfacetplane(facet)?;
                    }
                    if matchfacet != NIL {
                        let m = matchfacet as usize;
                        let matchskip = set::index(&self.facets[m].neighbors, facet);
                        let Ok(matchskip) = usize::try_from(matchskip) else {
                            return Err(Error::Qhull(6260));
                        };
                        set::set_elem(&mut self.facets[m].neighbors, matchskip, DUPLICATE_RIDGE);
                        self.facets[m].dupridge = true;
                        if self.facets[m].normal.is_none() {
                            self.setfacetplane(matchfacet)?;
                        }
                        self.addhash(matchfacet, hashsize, hash);
                        *hashcount += 2;
                    }
                }
                return Ok(());
            }
            scan += 1;
            if scan >= hashsize {
                scan = 0;
            }
        }
        if !newfound {
            set::set_elem(&mut self.hash_table, scan, newfacet);
        }
        *hashcount += 1;
        Ok(())
    }

    /// `qh_matchduplicates(atfacet, atskip, hashsize, &hashcount)`: pairs the facets of a
    /// duplicate ridge, the pair furthest from each other (`qh_getdistance`) first, linking the
    /// others with `qh_MERGEridge` (or directly between tricoplanar facets).
    fn matchduplicates(
        &mut self,
        atfacet: u32,
        atskip: usize,
        hashsize: usize,
        hashcount: &mut i32,
    ) -> QResult<()> {
        let mut maxmatch = NIL;
        let mut maxmatch2 = NIL;
        let mut maxskip = 0;
        let mut maxskip2 = 0;
        let mut nextskip = 0;
        let mut maxdist = -REAL_MAX;
        let skipelem = set::elem(&self.facets[atfacet as usize].vertices, atskip);
        let hash = self.gethash(
            hashsize,
            &self.facets[atfacet as usize].vertices,
            DIM,
            1,
            skipelem,
        )?;
        for makematch in 0..2 {
            self.visit_id = self.visit_id.wrapping_add(1);
            let mut newfacet = atfacet;
            let mut newskip = atskip;
            while newfacet != NIL {
                let mut nextfacet = NIL;
                let nf = newfacet as usize;
                self.facets[nf].visitid = self.visit_id;
                let mut scan = hash;
                loop {
                    let facet = set::elem(&self.hash_table, scan);
                    if facet == NIL {
                        break;
                    }
                    let f = facet as usize;
                    if self.facets[f].dupridge
                        && self.facets[f].visitid != self.visit_id
                        && let Some((skip, same)) = self.matchvertices(
                            1,
                            &self.facets[nf].vertices,
                            newskip,
                            &self.facets[f].vertices,
                        )
                    {
                        let ismatch =
                            same == (self.facets[nf].toporient ^ self.facets[f].toporient);
                        if set::elem(&self.facets[f].neighbors, skip) != DUPLICATE_RIDGE {
                            if makematch == 0 {
                                return Err(Error::Qhull(6155));
                            }
                        } else if ismatch && makematch != 0 {
                            if set::elem(&self.facets[nf].neighbors, newskip) == DUPLICATE_RIDGE {
                                set::set_elem(&mut self.facets[f].neighbors, skip, newfacet);
                                let link = if self.facets[nf].tricoplanar {
                                    facet
                                } else {
                                    MERGE_RIDGE
                                };
                                set::set_elem(&mut self.facets[nf].neighbors, newskip, link);
                                *hashcount -= 2;
                            }
                        } else if ismatch {
                            let (mut mindist, _, _) = self.getdistance(facet, newfacet);
                            let (dist2, _, _) = self.getdistance(newfacet, facet);
                            minimize_(&mut mindist, dist2);
                            if mindist > maxdist {
                                maxdist = mindist;
                                maxmatch = facet;
                                maxskip = skip;
                                maxmatch2 = newfacet;
                                maxskip2 = newskip;
                            }
                        } else {
                            nextfacet = facet;
                            nextskip = skip;
                        }
                    }
                    scan += 1;
                    if scan >= hashsize {
                        scan = 0;
                    }
                }
                newfacet = nextfacet;
                newskip = nextskip;
            }
            if makematch == 0 {
                if maxmatch == NIL {
                    return Err(Error::Qhull(6157));
                }
                set::set_elem(
                    &mut self.facets[maxmatch as usize].neighbors,
                    maxskip,
                    maxmatch2,
                );
                set::set_elem(
                    &mut self.facets[maxmatch2 as usize].neighbors,
                    maxskip2,
                    maxmatch,
                );
                *hashcount -= 2;
                // qh_precision: no restart under PREmerge.
            }
        }
        Ok(())
    }

    /// `qh_gethash(hashsize, set, size, firstindex, skipelem)` for `size - firstindex` 2: the sum
    /// of the two elements less the skipped one, as Qhull adds pointers, here vertex ids (see the
    /// module notes), modulo the table size after Qhull's cast to `unsigned`.
    ///
    /// # Errors
    ///
    /// [`Error::Unported`] for another count of elements, which a 3-d hull never hashes.
    pub(crate) fn gethash(
        &self,
        hashsize: usize,
        set: &Set,
        size: usize,
        firstindex: usize,
        skipelem: u32,
    ) -> QResult<usize> {
        if size - firstindex != 2 {
            return Err(Error::Unported("qh_gethash of other than two elements"));
        }
        let id = |vertex: u32| -> i64 {
            if vertex == NIL {
                0
            } else {
                i64::from(self.vertices[vertex as usize].id)
            }
        };
        let hash =
            id(set::elem(set, firstindex)) + id(set::elem(set, firstindex + 1)) - id(skipelem);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let result = hash as u32;
        let hashsize = u32::try_from(hashsize).expect("a hash table fits an int");
        Ok((result % hashsize) as usize)
    }

    /// `qh_matchvertices(firstindex, verticesA, skipA, verticesB, &skipB, &same)`: whether the
    /// vertices of A from `firstindex` but its `skipA`-th are those of B but one, with that
    /// one's index and whether the two skipped indices have the same parity.
    pub(crate) fn matchvertices(
        &self,
        firstindex: usize,
        vertices_a: &Set,
        skip_a: usize,
        vertices_b: &Set,
    ) -> Option<(usize, bool)> {
        let mut ia = firstindex;
        let mut ib = firstindex;
        let mut skip_bp: Option<usize> = None;
        loop {
            if ia != skip_a {
                loop {
                    let ea = set::elem(vertices_a, ia);
                    let eb = set::elem(vertices_b, ib);
                    ib += 1;
                    if ea == eb {
                        break;
                    }
                    if skip_bp.is_some() {
                        return None;
                    }
                    skip_bp = Some(ib);
                }
            }
            ia += 1;
            if set::elem(vertices_a, ia) == NIL {
                break;
            }
        }
        let skip_bp = skip_bp.unwrap_or(ib + 1);
        let skip_b = skip_bp - 1;
        let same = ((skip_a & 1) ^ (skip_b & 1)) == 0;
        Some((skip_b, same))
    }

    /// `qh_addhash(newelem, hashtable, hashsize, hash)`: adds the element at the first free slot
    /// from `hash` unless it is in the run already.
    fn addhash(&mut self, newelem: u32, hashsize: usize, hash: usize) {
        let mut scan = hash;
        loop {
            let elem = set::elem(&self.hash_table, scan);
            if elem == NIL {
                set::set_elem(&mut self.hash_table, scan, newelem);
                return;
            }
            if elem == newelem {
                return;
            }
            scan += 1;
            if scan >= hashsize {
                scan = 0;
            }
        }
    }

    /// `qh_newhashtable(newsize)`: an empty hash table of at least twice the size, odd and not a
    /// multiple of 3 or 5; returns its size.
    ///
    /// # Errors
    ///
    /// Qhull's error 6236 for a negative size.
    pub(crate) fn newhashtable(&mut self, newsize: i32) -> QResult<usize> {
        let mut size = newsize.wrapping_add(1).wrapping_mul(2) | 1;
        loop {
            if newsize < 0 || size < 0 {
                return Err(Error::Qhull(6236));
            }
            if size % 3 != 0 && size % 5 != 0 {
                break;
            }
            size += 2;
        }
        let size = usize::try_from(size).expect("the size is not negative");
        self.hash_table = set::zeroed(size);
        Ok(size)
    }

    /// `qh_updatevertices`: updates the vertex neighbours for the new facets and marks the
    /// vertices of only visible facets deleted.
    pub(crate) fn updatevertices(&mut self) {
        if self.vertex_neighbors {
            let mut vertex = self.newvertex_list;
            while vertex != NIL && self.vertices[vertex as usize].next != NIL {
                let v = vertex as usize;
                let mut i = 0;
                loop {
                    let neighbor = set::elem(&self.vertices[v].neighbors, i);
                    i += 1;
                    if neighbor == NIL {
                        break;
                    }
                    if self.facets[neighbor as usize].visible {
                        set::set_elem(&mut self.vertices[v].neighbors, i - 1, NIL);
                    }
                }
                set::compact(&mut self.vertices[v].neighbors);
                vertex = self.vertices[v].next;
            }
            let mut newfacet = self.newfacet_list;
            while newfacet != NIL && self.facets[newfacet as usize].next != NIL {
                let mut i = 0;
                loop {
                    let vertex = set::elem(&self.facets[newfacet as usize].vertices, i);
                    i += 1;
                    if vertex == NIL {
                        break;
                    }
                    set::append(&mut self.vertices[vertex as usize].neighbors, newfacet);
                }
                newfacet = self.facets[newfacet as usize].next;
            }
            let mut visible = self.visible_list;
            while visible != NIL && self.facets[visible as usize].visible {
                let mut i = 0;
                loop {
                    let vertex = set::elem(&self.facets[visible as usize].vertices, i);
                    i += 1;
                    if vertex == NIL {
                        break;
                    }
                    let v = vertex as usize;
                    if !self.vertices[v].newlist && !self.vertices[v].deleted {
                        let mut j = 0;
                        let mut neighbor;
                        loop {
                            neighbor = set::elem(&self.vertices[v].neighbors, j);
                            j += 1;
                            if neighbor == NIL || !self.facets[neighbor as usize].visible {
                                break;
                            }
                        }
                        if neighbor != NIL {
                            set::del(&mut self.vertices[v].neighbors, visible);
                        } else {
                            self.vertices[v].deleted = true;
                            set::append(&mut self.del_vertices, vertex);
                        }
                    }
                }
                visible = self.facets[visible as usize].next;
            }
        } else {
            let mut visible = self.visible_list;
            while visible != NIL && self.facets[visible as usize].visible {
                let mut i = 0;
                loop {
                    let vertex = set::elem(&self.facets[visible as usize].vertices, i);
                    i += 1;
                    if vertex == NIL {
                        break;
                    }
                    let v = vertex as usize;
                    if !self.vertices[v].newlist && !self.vertices[v].deleted {
                        self.vertices[v].deleted = true;
                        set::append(&mut self.del_vertices, vertex);
                    }
                }
                visible = self.facets[visible as usize].next;
            }
        }
    }

    /// `qh_vertexneighbors`: the facets of each vertex, once.
    pub(crate) fn vertexneighbors(&mut self) {
        if self.vertex_neighbors {
            return;
        }
        self.vertex_visit = self.vertex_visit.wrapping_add(1);
        let mut facet = self.facet_list;
        while facet != NIL && self.facets[facet as usize].next != NIL {
            if !self.facets[facet as usize].visible {
                let mut i = 0;
                loop {
                    let vertex = set::elem(&self.facets[facet as usize].vertices, i);
                    i += 1;
                    if vertex == NIL {
                        break;
                    }
                    let v = &mut self.vertices[vertex as usize];
                    if v.visitid != self.vertex_visit {
                        v.visitid = self.vertex_visit;
                        v.neighbors = Some(Vec::with_capacity(DIM));
                    }
                    set::append(&mut v.neighbors, facet);
                }
            }
            facet = self.facets[facet as usize].next;
        }
        self.vertex_neighbors = true;
    }

    /// `qh_furthestnext`: moves the facet with the furthest outside point to `facet_next`
    /// (`qh_COMPUTEfurthest` is 0: by `furthestdist`).
    pub(crate) fn furthestnext(&mut self) {
        let mut bestfacet = NIL;
        let mut bestdist = -REAL_MAX;
        let mut facet = self.facet_list;
        while facet != NIL && self.facets[facet as usize].next != NIL {
            let f = &self.facets[facet as usize];
            if f.outsideset.is_some() {
                let dist = f.furthestdist;
                if dist > bestdist {
                    bestfacet = facet;
                    bestdist = dist;
                }
            }
            facet = f.next;
        }
        if bestfacet != NIL {
            self.removefacet(bestfacet);
            self.prependfacet_next(bestfacet);
        }
    }

    /// `qh_furthestout(facet)`: the distance of the furthest outside point. Qhull means to move
    /// that point to the end of the set, but its `qh_setdel` and `qh_setappend` get the loop's
    /// final NULL and do nothing, so only `furthestdist` changes.
    pub(crate) fn furthestout(&mut self, facet: u32) {
        let mut bestdist = -REAL_MAX;
        let mut found = false;
        let mut i = 0;
        loop {
            let point = set::elem(&self.facets[facet as usize].outsideset, i);
            i += 1;
            if point == NIL {
                break;
            }
            let dist = self.distplane_point(point, facet);
            if dist > bestdist {
                found = true;
                bestdist = dist;
            }
        }
        if found {
            self.facets[facet as usize].furthestdist = bestdist;
        }
        self.facets[facet as usize].notfurthest = false;
    }

    /// `qh_outcoplanar`: moves the outside points left on a narrow hull to the coplanar sets
    /// (`KEEPnearinside`).
    ///
    /// # Errors
    ///
    /// As [`Qh::partitioncoplanar`].
    pub(crate) fn outcoplanar(&mut self) -> QResult<()> {
        let mut facet = self.facet_list;
        while facet != NIL && self.facets[facet as usize].next != NIL {
            let mut i = 0;
            loop {
                let point = set::elem(&self.facets[facet as usize].outsideset, i);
                i += 1;
                if point == NIL {
                    break;
                }
                self.num_outside -= 1;
                let mut dist = self.distplane_point(point, facet);
                self.partitioncoplanar(point, facet, Some(&mut dist))?;
            }
            self.facets[facet as usize].outsideset = None;
            facet = self.facets[facet as usize].next;
        }
        Ok(())
    }

    /// `qh_nearcoplanar`: neither `KEEPcoplanar` nor `KEEPinside`, so every coplanar set goes.
    pub(crate) fn nearcoplanar(&mut self) {
        let mut facet = self.facet_list;
        while facet != NIL && self.facets[facet as usize].next != NIL {
            self.facets[facet as usize].coplanarset = None;
            facet = self.facets[facet as usize].next;
        }
    }

    /// `qh_check_maxout`: the furthest any point is above its facets (`max_outside`) and the
    /// lowest vertex below a facet of it (`min_vertex`), then `qh_nearcoplanar`.
    pub(crate) fn check_maxout(&mut self) {
        let mut maxoutside = 0.0;
        let mut minvertex = 0.0;
        // PRINTout[0] is qh_PRINTnone, so the vertices are measured once there are vertex
        // neighbours (the C's qh_pointvertex set is only built and freed). TRACEdist is REALmax
        // and there is no trace facet or vertex: message 8093 is not printed.
        if self.vertex_neighbors {
            let mut vertex = self.vertex_list;
            while vertex != NIL && self.vertices[vertex as usize].next != NIL {
                let point = self.vertices[vertex as usize].point;
                let mut i = 0;
                loop {
                    let neighbor = set::elem(&self.vertices[vertex as usize].neighbors, i);
                    i += 1;
                    if neighbor == NIL {
                        break;
                    }
                    let dist = self.distplane_point(point, neighbor);
                    minimize_(&mut minvertex, dist);
                }
                vertex = self.vertices[vertex as usize].next;
            }
            self.min_vertex = minvertex;
        }
        let facets = self.pointfacet();
        loop {
            let old_maxoutside = crate::qh::fmax_(self.max_outside, maxoutside);
            for (facet_i, &facet) in facets.iter().enumerate() {
                if facet == NIL {
                    continue;
                }
                let point = self.point(i32::try_from(facet_i).expect("a point id fits an int"));
                // GOODpointp is NULL, which no point is.
                let p = self.coords(point);
                let mut dist = self.distplane(&p, facet);
                let bestfacet = self.findbesthorizon(true, &p, facet, false, &mut dist);
                if bestfacet != NIL && dist > maxoutside {
                    // Not ONLYgood.
                    maxoutside = dist;
                }
                // Message 8094 needs dist above TRACEdist (REALmax) or a trace facet.
            }
            // The C's `do { ... } while (maxoutside > 2*old_maxoutside)`: a NaN ends the loop.
            if maxoutside > 2.0 * old_maxoutside {
                continue;
            }
            break;
        }
        self.max_outside = maxoutside;
        self.nearcoplanar();
        self.maxoutdone = true;
    }

    /// `qh_pointfacet`: for each point, the last facet of which it is a vertex, a coplanar point
    /// or an outside point (NULL if none).
    fn pointfacet(&mut self) -> Vec<u32> {
        let numpoints = usize::try_from(self.num_points).expect("a point count is not negative")
            + set::size(&self.other_points);
        let mut facets = vec![NIL; numpoints];
        self.vertex_visit = self.vertex_visit.wrapping_add(1);
        let mut facet = self.facet_list;
        while facet != NIL && self.facets[facet as usize].next != NIL {
            let f = facet as usize;
            let mut i = 0;
            loop {
                let vertex = set::elem(&self.facets[f].vertices, i);
                i += 1;
                if vertex == NIL {
                    break;
                }
                let v = &mut self.vertices[vertex as usize];
                if v.visitid != self.vertex_visit {
                    v.visitid = self.vertex_visit;
                    facets[v.point as usize] = facet;
                }
            }
            for list in [&self.facets[f].coplanarset, &self.facets[f].outsideset] {
                let mut i = 0;
                loop {
                    let point = set::elem(list, i);
                    i += 1;
                    if point == NIL {
                        break;
                    }
                    facets[point as usize] = facet;
                }
            }
            facet = self.facets[f].next;
        }
        facets
    }

    /// `qh_findbestlower(upperfacet, point, &bestdist, &numpart)`: the best facet among the
    /// neighbours of `upperfacet`, else of its nearest vertex, else of all.
    ///
    /// # Errors
    ///
    /// As [`Qh::nearvertex`].
    pub(crate) fn findbestlower(
        &mut self,
        upperfacet: u32,
        point: &[f64; 3],
    ) -> QResult<(u32, f64)> {
        let mut bestfacet = NIL;
        let mut bestdist = -REAL_MAX / 2.0;
        let mut i = 0;
        loop {
            let neighbor = set::elem(&self.facets[upperfacet as usize].neighbors, i);
            i += 1;
            if neighbor == NIL {
                break;
            }
            let n = &self.facets[neighbor as usize];
            if n.upperdelaunay || n.flipped {
                continue;
            }
            let dist = self.distplane(point, neighbor);
            if dist > bestdist {
                bestfacet = neighbor;
                bestdist = dist;
            }
        }
        if bestfacet == NIL {
            let (vertex, _) = self.nearvertex(upperfacet, point)?;
            self.vertexneighbors();
            let mut i = 0;
            loop {
                let neighbor = set::elem(&self.vertices[vertex as usize].neighbors, i);
                i += 1;
                if neighbor == NIL {
                    break;
                }
                let n = &self.facets[neighbor as usize];
                if n.upperdelaunay || n.flipped {
                    continue;
                }
                let dist = self.distplane(point, neighbor);
                if dist > bestdist {
                    bestfacet = neighbor;
                    bestdist = dist;
                }
            }
        }
        if bestfacet == NIL {
            let (facet, dist, _) = self.findfacet_all(point);
            bestfacet = facet;
            bestdist = dist;
        }
        Ok((bestfacet, bestdist))
    }

    /// `qh_findfacet_all(point, &bestdist, &isoutside, &numpart)`: the best facet of all, stopping
    /// at the first that the point is clearly outside of.
    pub(crate) fn findfacet_all(&self, point: &[f64; 3]) -> (u32, f64, bool) {
        let mut bestfacet = NIL;
        let mut bestdist = -REAL_MAX;
        let mut isoutside = false;
        let mut facet = self.facet_list;
        while facet != NIL && self.facets[facet as usize].next != NIL {
            let f = &self.facets[facet as usize];
            if !f.flipped && f.normal.is_some() {
                let dist = self.distplane(point, facet);
                if dist > bestdist {
                    bestdist = dist;
                    bestfacet = facet;
                    if dist > self.min_outside {
                        isoutside = true;
                        break;
                    }
                }
            }
            facet = f.next;
        }
        (bestfacet, bestdist, isoutside)
    }

    /// `qh_nearvertex(facet, point, &bestdist)`: the facet's vertex nearest the point.
    ///
    /// # Errors
    ///
    /// Qhull's internal error 6261 when the facet has no vertex nearer than `REALmax`, and
    /// [`Error::Unported`] for a tricoplanar facet, which exists only after the build.
    pub(crate) fn nearvertex(&self, facet: u32, point: &[f64; 3]) -> QResult<(u32, f64)> {
        let f = &self.facets[facet as usize];
        if f.tricoplanar {
            return Err(Error::Unported("qh_nearvertex of a tricoplanar facet"));
        }
        let mut bestdist = REAL_MAX;
        let mut bestvertex = NIL;
        let mut i = 0;
        loop {
            let vertex = set::elem(&f.vertices, i);
            i += 1;
            if vertex == NIL {
                break;
            }
            let p = self.coords(self.vertices[vertex as usize].point);
            let dist = pointdist(&p, point, -3);
            if dist < bestdist {
                bestdist = dist;
                bestvertex = vertex;
            }
        }
        let bestdist = sqrt(bestdist);
        if bestvertex == NIL {
            return Err(Error::Qhull(6261));
        }
        Ok((bestvertex, bestdist))
    }

    /// `qh_check_dupridge(facet1, dist1, facet2, dist2)`: refuses a merge for a duplicate ridge
    /// that is far wider than the hull's precision.
    ///
    /// # Errors
    ///
    /// Qhull's error 6271 when it is more than `qh_WIDEduplicate` times wider (`NOwide` is
    /// False). The distance between the facet's nearest vertices, which only the message reads,
    /// is not computed.
    pub(crate) fn check_dupridge(&self, dist1: f64, dist2: f64) -> QResult<()> {
        let mergedist = crate::qh::fmin_(dist1, dist2);
        let (outerplane, innerplane) = self.outerinner_hull();
        let mut prevdist = crate::qh::fmax_(outerplane, innerplane);
        crate::qh::maximize_(&mut prevdist, self.one_merge + self.dist_round);
        crate::qh::maximize_(&mut prevdist, self.min_outside + self.dist_round);
        let ratio = mergedist / prevdist;
        if ratio > WIDE_DUPLICATE {
            return Err(Error::Qhull(6271));
        }
        Ok(())
    }

    /// `qh_vertexintersect_new(vertexsetA, vertexsetB)`: the vertices of both, which are inverse
    /// sorted by id.
    pub(crate) fn vertexintersect_new(&self, set_a: &Set, set_b: &Set) -> Set {
        let mut intersection: Set = Some(Vec::with_capacity(DIM - 1));
        let (mut ia, mut ib) = (0usize, 0usize);
        loop {
            let a = set::elem(set_a, ia);
            let b = set::elem(set_b, ib);
            if a == NIL || b == NIL {
                break;
            }
            if a == b {
                set::append(&mut intersection, a);
                ia += 1;
                ib += 1;
            } else if self.vertices[a as usize].id > self.vertices[b as usize].id {
                ia += 1;
            } else {
                ib += 1;
            }
        }
        intersection
    }

    /// `qh_triangulate`: replaces each non-simplicial facet by triangles that share its
    /// hyperplane (tricoplanar facets), the first of them that is not degenerate taking the
    /// hyperplane over (`keepcentrum`); drops the degenerate triangles of a vertex repeated
    /// (`qh_triangulate_null`) and pairs of mirror triangles (`qh_triangulate_mirror`).
    ///
    /// # Errors
    ///
    /// Qhull's internal errors of the matching and of the triangulation (6161, 6162, 6163).
    #[allow(clippy::too_many_lines)]
    pub(crate) fn triangulate(&mut self) -> QResult<()> {
        if self.has_triangulation {
            return Ok(());
        }
        // Not VORONOI; ONLYgood is False already.
        self.visit_id = self.visit_id.wrapping_add(1);
        self.new_facets = true;
        self.degen_mergeset = Some(Vec::new());
        self.newvertex_list = self.vertex_tail;
        let mut new_facet_list = NIL;
        let mut new_vertex_list = NIL;
        let mut facet = self.facet_list;
        while facet != NIL && self.facets[facet as usize].next != NIL {
            let nextfacet = self.facets[facet as usize].next;
            let f = &self.facets[facet as usize];
            if !f.visible && !f.simplicial {
                if new_facet_list == NIL {
                    new_facet_list = facet;
                }
                self.triangulate_facet(facet, &mut new_vertex_list)?;
            }
            facet = nextfacet;
        }
        let mut facet = new_facet_list;
        while facet != NIL && self.facets[facet as usize].next != NIL {
            let nextfacet = self.facets[facet as usize].next;
            let f = facet as usize;
            if !self.facets[f].visible {
                if self.facets[f].ridges.is_some() {
                    if set::size(&self.facets[f].ridges) > 0 {
                        return Err(Error::Qhull(6161));
                    }
                    self.facets[f].ridges = None;
                }
                let v = &self.facets[f].vertices;
                if set::first(v) == set::second(v) {
                    self.triangulate_null(facet)?;
                }
            }
            facet = nextfacet;
        }
        self.visible_list = self.facet_tail;
        loop {
            let merge = set::del_last(&mut self.degen_mergeset);
            if merge == NIL {
                break;
            }
            let m = self.merges[merge as usize];
            if m.mtype == MRG_MIRROR {
                self.triangulate_mirror(m.facet1, m.facet2)?;
            }
        }
        self.degen_mergeset = None;
        self.newvertex_list = new_vertex_list;
        self.visible_list = NIL;
        self.updatevertices();
        self.resetlists(false);
        let mut orig_neighbor = NIL;
        let mut facet = new_facet_list;
        while facet != NIL && self.facets[facet as usize].next != NIL {
            let f = facet as usize;
            if self.facets[f].tricoplanar && !self.facets[f].visible {
                let neighbor_n = set::size(&self.facets[f].neighbors);
                for neighbor_i in 0..neighbor_n {
                    let neighbor = set::elem(&self.facets[f].neighbors, neighbor_i);
                    let n = &self.facets[neighbor as usize];
                    let owner = if n.tricoplanar { n.f } else { neighbor };
                    if neighbor_i == 0 {
                        orig_neighbor = owner;
                    } else if orig_neighbor == owner {
                        self.facets[f].degenerate = true;
                        break;
                    }
                }
            }
            facet = self.facets[f].next;
        }
        let mut owner = NIL;
        let mut visible = NIL;
        let mut facet = new_facet_list;
        while facet != NIL && self.facets[facet as usize].next != NIL {
            let f = facet as usize;
            let mut nextfacet = self.facets[f].next;
            if self.facets[f].visible {
                if self.facets[f].tricoplanar {
                    self.delfacet(facet);
                    self.num_visible -= 1;
                } else {
                    if visible != NIL && owner == NIL {
                        self.delete_unowned(visible, new_facet_list);
                    }
                    visible = facet;
                    owner = NIL;
                }
            } else if self.facets[f].tricoplanar {
                if self.facets[f].f != visible || visible == NIL {
                    return Err(Error::Qhull(6162));
                }
                if owner != NIL {
                    self.facets[f].f = owner;
                } else if !self.facets[f].degenerate {
                    owner = facet;
                    let vi = visible as usize;
                    nextfacet = self.facets[vi].next;
                    self.facets[f].keepcentrum = true;
                    self.facets[f].coplanarset = self.facets[vi].coplanarset.take();
                    self.facets[f].outsideset = self.facets[vi].outsideset.take();
                    // Not TRInormals: the owner takes the hyperplane over.
                    self.facets[vi].center = None;
                    self.facets[vi].normal = None;
                    self.delfacet(visible);
                    self.num_visible -= 1;
                }
            }
            facet = nextfacet;
        }
        if visible != NIL && owner == NIL {
            self.delete_unowned(visible, new_facet_list);
        }
        self.new_facets = false;
        self.has_triangulation = true;
        Ok(())
    }

    /// `qh_delfacet` of a triangulated facet none of whose triangles took its hyperplane over,
    /// in `qh_triangulate`: the triangles that remain keep a pointer to the freed normal, whose
    /// first coordinate becomes +0.0 here (see the module notes).
    fn delete_unowned(&mut self, visible: u32, new_facet_list: u32) {
        let mut facet = new_facet_list;
        while facet != NIL && self.facets[facet as usize].next != NIL {
            let f = &mut self.facets[facet as usize];
            if f.tricoplanar
                && !f.visible
                && f.f == visible
                && let Some(normal) = f.normal.as_mut()
            {
                normal[0] = 0.0;
            }
            facet = f.next;
        }
        self.delfacet(visible);
        self.num_visible -= 1;
    }

    /// `qh_triangulate_facet(facetA, &first_vertex)`: the triangles of a non-simplicial facet
    /// from its first vertex, sharing its hyperplane (`TRInormals` is False).
    fn triangulate_facet(&mut self, facet_a: u32, first_vertex: &mut u32) -> QResult<()> {
        let mut numnew = 0;
        let a = facet_a as usize;
        let mut i = 0;
        loop {
            let neighbor = set::elem(&self.facets[a].neighbors, i);
            i += 1;
            if neighbor == NIL {
                break;
            }
            self.facets[neighbor as usize].seen = false;
            self.facets[neighbor as usize].coplanar = false;
        }
        // CENTERtype is qh_AScentrum.
        self.willdelete(facet_a, NIL);
        self.newfacet_list = self.facet_tail;
        self.facets[a].visitid = self.visit_id;
        let apex = set::first(&self.facets[a].vertices);
        self.makenew_nonsimplicial(facet_a, apex, &mut numnew)?;
        if let Some(n) = self.facets[a].neighbors.as_mut()
            && !n.is_empty()
        {
            n[0] = NIL;
        }
        let mut newfacet = self.newfacet_list;
        while newfacet != NIL && self.facets[newfacet as usize].next != NIL {
            let (upperdelaunay, good, normal, center, offset, maxoutside) = {
                let fa = &self.facets[a];
                (
                    fa.upperdelaunay,
                    fa.good,
                    fa.normal,
                    fa.center,
                    fa.offset,
                    fa.maxoutside,
                )
            };
            let nf = &mut self.facets[newfacet as usize];
            nf.tricoplanar = true;
            nf.f = facet_a;
            nf.degenerate = false;
            nf.upperdelaunay = upperdelaunay;
            nf.good = good;
            nf.keepcentrum = false;
            nf.normal = normal;
            nf.center = center;
            nf.offset = offset;
            nf.maxoutside = maxoutside;
            newfacet = nf.next;
        }
        self.matchnewfacets()?;
        self.visible_list = NIL;
        if *first_vertex == NIL {
            *first_vertex = self.newvertex_list;
        }
        self.newvertex_list = NIL;
        self.updatevertices();
        self.resetlists(false);
        Ok(())
    }

    /// `qh_triangulate_link(oldfacetA, facetA, oldfacetB, facetB)`: links two facets across a
    /// deleted triangle, as a mirror pair when they are neighbours already.
    fn triangulate_link(
        &mut self,
        oldfacet_a: u32,
        facet_a: u32,
        oldfacet_b: u32,
        facet_b: u32,
    ) -> QResult<()> {
        let mut errmirror = false;
        if set::contains(&self.facets[facet_a as usize].neighbors, facet_b) {
            if set::contains(&self.facets[facet_b as usize].neighbors, facet_a) {
                self.appendmergeset(facet_a, facet_b, MRG_MIRROR, None)?;
            } else {
                errmirror = true;
            }
        } else if set::contains(&self.facets[facet_b as usize].neighbors, facet_a) {
            errmirror = true;
        }
        if errmirror {
            return Err(Error::Qhull(6163));
        }
        replace(
            &mut self.facets[facet_b as usize].neighbors,
            oldfacet_b,
            facet_a,
        )?;
        replace(
            &mut self.facets[facet_a as usize].neighbors,
            oldfacet_a,
            facet_b,
        )
    }

    /// `qh_triangulate_mirror(facetA, facetB)`: deletes two triangles with the same vertices,
    /// linking their neighbours.
    fn triangulate_mirror(&mut self, facet_a: u32, facet_b: u32) -> QResult<()> {
        let neighbor_n = set::size(&self.facets[facet_a as usize].neighbors);
        for neighbor_i in 0..neighbor_n {
            let neighbor = set::elem(&self.facets[facet_a as usize].neighbors, neighbor_i);
            let neighbor_b = set::elem(&self.facets[facet_b as usize].neighbors, neighbor_i);
            if neighbor == neighbor_b {
                continue;
            }
            self.triangulate_link(facet_a, neighbor, facet_b, neighbor_b)?;
        }
        self.willdelete(facet_a, NIL);
        self.willdelete(facet_b, NIL);
        Ok(())
    }

    /// `qh_triangulate_null(facetA)`: deletes a triangle with a vertex twice, linking its first
    /// two neighbours.
    fn triangulate_null(&mut self, facet_a: u32) -> QResult<()> {
        let neighbor = set::first(&self.facets[facet_a as usize].neighbors);
        let otherfacet = set::second(&self.facets[facet_a as usize].neighbors);
        self.triangulate_link(facet_a, neighbor, facet_a, otherfacet)?;
        self.willdelete(facet_a, NIL);
        Ok(())
    }
}
