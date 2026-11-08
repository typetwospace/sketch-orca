// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Qhull's data structures (`libqhull_r.h`, `merge_r.h`) and the lists of facets and vertices
//! (`poly_r.c`, `poly2_r.c`, `merge_r.c`'s `qh_willdelete`).
//!
//! Carries Qhull code: the structures of Qhull 2015.2's `libqhull_r.h` and `merge_r.h` and the
//! list functions of `poly_r.c` and `poly2_r.c`, translated into Rust on 2026-11-07, Copyright (c)
//! 1993-2015 C.B. Barber and The Geometry Center, University of Minnesota, under Qhull's licence
//! (`licenses/Qhull.txt`); the copyright holders are named in `NOTICE`.
//!
//! Facets, vertices, ridges and merges live in arenas and point at each other by index, with
//! [`NIL`] for C's NULL; an object Qhull frees stays in its arena unused, so an index is never
//! reused (Qhull's memory pool reuses the memory, which only an address could tell, and the
//! translation reads no address: see `qh_gethash` in `poly.rs`).
//!
//! The options are those of `runQhull("", 3, n, points, "Qt")`, which `its_convex_hull` passes:
//! `qh_initflags` sets only `TRIangulate`, and `qh_initqhull_globals` adds `MERGING`, `PREmerge`
//! and `ZEROcentrum` (`C-0`, the default merging of a 3-d hull), `DOcheckmax` and
//! `KEEPnearinside`, with `CENTERtype` `qh_AScentrum`. Every other option keeps the value
//! `qh_initqhull_start2` gives it, and the code those options guard is left out where it stands,
//! with the option named: `ONLYgood`, `DELAUNAY`, `VORONOI`, `KEEPcoplanar`, `KEEPinside`,
//! `BESToutside`, `MERGEexact`, `POSTmerge` and `POSTmerging`, `TESTvneighbors`, `RANDOMdist`,
//! `RANDOMoutside`, `TRInormals`, `AVOIDold`, `SKIPconvex`, `FORCEoutput`, `NOwide`,
//! `CHECKfrequently`, `VERIFYoutput` and `GOODpoint`/`GOODvertex` are False or unset,
//! `JOGGLEmax`, `TRACEdist` and `ROTATErandom`'s seed are as the defaults give them, `IStracing`
//! and `TRACElevel` are 0 (so no trace prints), and `MERGEvertices`, `ANGLEmerge` and
//! `MERGEindependent` are True.

use crate::set::{self, NIL, Set};

/// `qh.hull_dim`: the hull is 3-d.
pub(crate) const DIM: usize = 3;

/// `qh_DUPLICATEridge`, the neighbour of a facet across a ridge more than two facets share.
pub(crate) const DUPLICATE_RIDGE: u32 = u32::MAX - 1;

/// `qh_MERGEridge`, the neighbour of a facet across a duplicate ridge to merge.
pub(crate) const MERGE_RIDGE: u32 = u32::MAX - 2;

/// `REALmax` (`DBL_MAX`: `realT` is `double`).
pub(crate) const REAL_MAX: f64 = f64::MAX;

/// `REALmin` (`DBL_MIN`).
pub(crate) const REAL_MIN: f64 = f64::MIN_POSITIVE;

/// `REALepsilon` (`DBL_EPSILON`).
pub(crate) const REAL_EPSILON: f64 = f64::EPSILON;

/// `qh_IDunknown`: what `qh_pointid` gives for a point it does not know.
pub(crate) const ID_UNKNOWN: i32 = -1;

/// The `mergeType` of a merge (`merge_r.h`), in the sort order of `qh_comparemerge`.
pub(crate) type MergeType = i32;
/// `MRGcoplanar`: centrum coplanar.
pub(crate) const MRG_COPLANAR: MergeType = 1;
/// `MRGanglecoplanar`: angle coplanar.
pub(crate) const MRG_ANGLECOPLANAR: MergeType = 2;
/// `MRGconcave`: concave ridge.
pub(crate) const MRG_CONCAVE: MergeType = 3;
/// `MRGflip`: flipped facet (`facet1 == facet2`).
pub(crate) const MRG_FLIP: MergeType = 4;
/// `MRGridge`: duplicate ridge (`qh_MERGEridge`).
pub(crate) const MRG_RIDGE: MergeType = 5;
/// `MRGdegen`: degenerate facet, too few neighbours (`facet1 == facet2`).
pub(crate) const MRG_DEGEN: MergeType = 6;
/// `MRGredundant`: redundant facet, its vertices a subset of a neighbour's.
pub(crate) const MRG_REDUNDANT: MergeType = 7;
/// `MRGmirror`: mirror facet from `qh_triangulate`.
pub(crate) const MRG_MIRROR: MergeType = 8;

/// An error that ends a run: `qh_errexit`, which the C++ turns into a `QhullError`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// `qh_errexit` after Qhull's error message with this code (`qhull_status`, the exception's
    /// error code), such as 6214 for too few points or 6154 for a flat initial simplex.
    Qhull(u32),
    /// A branch that the options of `its_convex_hull` never reach and the translation leaves
    /// out, named here; nothing returns it unless the analysis that rules it out is wrong.
    Unported(&'static str),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Qhull(code) => write!(f, "Qhull error QH{code}"),
            Error::Unported(what) => write!(f, "Qhull branch not translated: {what}"),
        }
    }
}

impl std::error::Error for Error {}

/// What a function of the translation returns: Qhull's `qh_errexit` is the `Err`.
pub(crate) type QResult<T> = Result<T, Error>;

/// `facetT` (`libqhull_r.h`).
#[derive(Clone, Debug, Default)]
pub(crate) struct Facet {
    /// `furthestdist`: the distance to the furthest point of `outsideset`.
    pub furthestdist: f64,
    /// `maxoutside`: the largest distance of a point above the facet.
    pub maxoutside: f64,
    /// `offset`: the hyperplane's offset.
    pub offset: f64,
    /// `normal`: the unit normal, NULL until `qh_setfacetplane`. A tricoplanar facet shares its
    /// original facet's (`qh_triangulate_facet`), here as a copy, which nothing changes after.
    pub normal: Option<[f64; 3]>,
    /// `center`: the centrum (`qh_AScentrum`), NULL until computed.
    pub center: Option<[f64; 3]>,
    /// `f`, the union of `replace`, `samecycle`, `newcycle`, `trivisible` and `triowner`.
    pub f: u32,
    /// `previous` in the facet list.
    pub previous: u32,
    /// `next` in the facet list.
    pub next: u32,
    /// `vertices`, inverse sorted by id (if simplicial, the first was the apex).
    pub vertices: Set,
    /// `ridges`: the explicit ridges of a non-simplicial facet.
    pub ridges: Set,
    /// `neighbors`: if simplicial, the k-th is opposite the k-th vertex.
    pub neighbors: Set,
    /// `outsideset`: the points outside, the furthest last.
    pub outsideset: Set,
    /// `coplanarset`: the coplanar points, the furthest last.
    pub coplanarset: Set,
    /// `visitid`.
    pub visitid: u32,
    /// `id`: Qhull numbers its facets from 0 as it makes them (the sentinel first), so a facet's
    /// id is its index here; only Qhull's messages read it, and the wrapper's `QhullFacet::id`.
    pub id: u32,
    /// `nummerge` (9 bits: at most `qh_MAXnummerge`).
    pub nummerge: u32,
    pub tricoplanar: bool,
    pub newfacet: bool,
    pub visible: bool,
    pub toporient: bool,
    pub simplicial: bool,
    pub seen: bool,
    pub flipped: bool,
    pub upperdelaunay: bool,
    pub notfurthest: bool,
    pub good: bool,
    pub dupridge: bool,
    pub mergeridge: bool,
    pub mergeridge2: bool,
    pub coplanar: bool,
    pub mergehorizon: bool,
    pub cycledone: bool,
    pub tested: bool,
    pub keepcentrum: bool,
    pub newmerge: bool,
    pub degenerate: bool,
    pub redundant: bool,
}

/// `vertexT` (`libqhull_r.h`).
#[derive(Clone, Debug, Default)]
pub(crate) struct Vertex {
    /// `next` in the vertex list.
    pub next: u32,
    /// `previous` in the vertex list.
    pub previous: u32,
    /// `point`: the index of the input point, [`NIL`] for the list's sentinel.
    pub point: u32,
    /// `neighbors`: the facets of the vertex, once `qh_vertexneighbors` has run.
    pub neighbors: Set,
    /// `id`: Qhull numbers its vertices from 0 as it makes them (the simplex's first, then the
    /// sentinel), so a vertex's id is its index here; the vertex sets are sorted by it.
    pub id: u32,
    /// `visitid`.
    pub visitid: u32,
    pub seen: bool,
    pub delridge: bool,
    pub deleted: bool,
    pub newlist: bool,
}

/// `ridgeT` (`libqhull_r.h`).
#[derive(Clone, Debug, Default)]
pub(crate) struct Ridge {
    /// `vertices`, inverse sorted by id.
    pub vertices: Set,
    /// `top`.
    pub top: u32,
    /// `bottom`.
    pub bottom: u32,
    // `id` and `seen` are left out: only messages and the output functions read them.
    pub tested: bool,
    pub nonconvex: bool,
}

/// `mergeT` (`merge_r.h`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Merge {
    /// `angle`: set by `qh_appendmergeset` only when it is given one. Qhull leaves it
    /// uninitialised otherwise; no such merge is ever sorted (`merge.rs`), so 0 stands for it.
    pub angle: f64,
    /// `facet1`, which merges into `facet2`.
    pub facet1: u32,
    /// `facet2`.
    pub facet2: u32,
    /// `type`.
    pub mtype: MergeType,
}

/// The state of a Qhull run, `qhT` (`libqhull_r.h`), with the fields the run reads.
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct Qh {
    /// `first_point`: the input points (`num_points` of them).
    pub points: Vec<[f64; 3]>,
    /// `num_points`.
    pub num_points: i32,
    /// `interior_point`: the centre of the initial simplex.
    pub interior_point: [f64; 3],
    pub facets: Vec<Facet>,
    pub vertices: Vec<Vertex>,
    pub ridges: Vec<Ridge>,
    pub merges: Vec<Merge>,
    pub facet_list: u32,
    pub facet_tail: u32,
    pub facet_next: u32,
    pub newfacet_list: u32,
    pub visible_list: u32,
    pub vertex_list: u32,
    pub vertex_tail: u32,
    pub newvertex_list: u32,
    pub num_facets: i32,
    pub num_vertices: i32,
    pub num_visible: i32,
    pub num_outside: i32,
    pub facet_id: u32,
    pub vertex_id: u32,
    pub visit_id: u32,
    pub vertex_visit: u32,
    pub furthest_id: i32,
    /// `other_points`: points not in `first_point`, which this run never has.
    pub other_points: Set,
    pub del_vertices: Set,
    pub coplanarfacetset: Set,
    pub facet_mergeset: Set,
    pub degen_mergeset: Set,
    pub hash_table: Set,
    pub dist_round: f64,
    pub min_denom: f64,
    pub min_denom_1: f64,
    pub min_denom_1_2: f64,
    pub min_denom_2: f64,
    pub angle_round: f64,
    pub premerge_centrum: f64,
    pub postmerge_centrum: f64,
    pub premerge_cos: f64,
    pub postmerge_cos: f64,
    pub one_merge: f64,
    pub near_inside: f64,
    pub min_visible: f64,
    pub max_coplanar: f64,
    pub min_outside: f64,
    pub wide_facet: f64,
    pub max_outside: f64,
    pub max_vertex: f64,
    pub min_vertex: f64,
    pub max_abs_coord: f64,
    pub max_sum_coord: f64,
    pub max_width: f64,
    pub near_zero: [f64; 3],
    pub cos_max: f64,
    pub centrum_radius: f64,
    /// `JOGGLEmax`: `REALmax` (no `QJ`).
    pub joggle_max: f64,
    pub merging: bool,
    pub zero_centrum: bool,
    pub zero_all_ok: bool,
    pub docheckmax: bool,
    pub keep_nearinside: bool,
    pub narrow_hull: bool,
    pub was_coplanar: bool,
    pub vertex_neighbors: bool,
    pub new_facets: bool,
    pub findbestnew: bool,
    pub findbest_notsharp: bool,
    pub maxoutdone: bool,
    pub has_triangulation: bool,
    pub triangulate: bool,
    /// The statistic `Ztotmerge`, which `qh_USEfindbestnew` reads.
    pub ztotmerge: i32,
    /// The statistic `Zsetplane`, which `qh_maxsimplex` reads.
    pub zsetplane: i32,
    /// `qhull_message` of the C++ `QhullQh`: what `qh_fprintf` appends (`QhullQh.cpp`).
    pub message: String,
}

impl Qh {
    /// `qh_newfacet`: a new facet with the next id and an empty neighbour set.
    pub fn newfacet(&mut self) -> u32 {
        let index = u32::try_from(self.facets.len()).expect("fewer than 2^32 facets");
        let id = self.facet_id;
        self.facet_id = self.facet_id.wrapping_add(1);
        self.facets.push(Facet {
            id,
            neighbors: Some(Vec::new()),
            furthestdist: 0.0,
            maxoutside: self.dist_round,
            simplicial: true,
            good: true,
            newfacet: true,
            previous: NIL,
            next: NIL,
            f: NIL,
            ..Facet::default()
        });
        index
    }

    /// `qh_newvertex`: a new vertex of the point with the next id.
    ///
    /// # Errors
    ///
    /// Qhull's error 6159 for the 2^32-th vertex.
    pub fn newvertex(&mut self, point: u32) -> QResult<u32> {
        if self.vertex_id == u32::MAX {
            return Err(Error::Qhull(6159));
        }
        let index = u32::try_from(self.vertices.len()).expect("fewer than 2^32 vertices");
        let id = self.vertex_id;
        self.vertex_id += 1;
        self.vertices.push(Vertex {
            id,
            point,
            previous: NIL,
            next: NIL,
            ..Vertex::default()
        });
        Ok(index)
    }

    /// `qh_newridge`: a new ridge (warning 7074 at 2^32 ridges, which no run here reaches: a
    /// ridge needs memory; the ridge's id is left out with the field).
    pub fn newridge(&mut self) -> u32 {
        let index = u32::try_from(self.ridges.len()).expect("fewer than 2^32 ridges");
        self.ridges.push(Ridge {
            top: NIL,
            bottom: NIL,
            ..Ridge::default()
        });
        index
    }

    /// `qh_appendfacet`: puts the facet at the end of the facet list.
    pub fn appendfacet(&mut self, facet: u32) {
        let tail = self.facet_tail;
        if tail == self.newfacet_list {
            self.newfacet_list = facet;
        }
        if tail == self.facet_next {
            self.facet_next = facet;
        }
        let prev = self.facets[tail as usize].previous;
        self.facets[facet as usize].previous = prev;
        self.facets[facet as usize].next = tail;
        if prev != NIL {
            self.facets[prev as usize].next = facet;
        } else {
            self.facet_list = facet;
        }
        self.facets[tail as usize].previous = facet;
        self.num_facets += 1;
    }

    /// `qh_appendvertex`: puts the vertex at the end of the vertex list and marks it new.
    pub fn appendvertex(&mut self, vertex: u32) {
        let tail = self.vertex_tail;
        if tail == self.newvertex_list {
            self.newvertex_list = vertex;
        }
        self.vertices[vertex as usize].newlist = true;
        let prev = self.vertices[tail as usize].previous;
        self.vertices[vertex as usize].previous = prev;
        self.vertices[vertex as usize].next = tail;
        if prev != NIL {
            self.vertices[prev as usize].next = vertex;
        } else {
            self.vertex_list = vertex;
        }
        self.vertices[tail as usize].previous = vertex;
        self.num_vertices += 1;
    }

    /// `qh_removefacet`: unlinks the facet from the facet list.
    pub fn removefacet(&mut self, facet: u32) {
        let next = self.facets[facet as usize].next;
        let previous = self.facets[facet as usize].previous;
        if facet == self.newfacet_list {
            self.newfacet_list = next;
        }
        if facet == self.facet_next {
            self.facet_next = next;
        }
        if facet == self.visible_list {
            self.visible_list = next;
        }
        if previous != NIL {
            self.facets[previous as usize].next = next;
            self.facets[next as usize].previous = previous;
        } else {
            self.facet_list = next;
            self.facets[next as usize].previous = NIL;
        }
        self.num_facets -= 1;
    }

    /// `qh_removevertex`: unlinks the vertex from the vertex list.
    pub fn removevertex(&mut self, vertex: u32) {
        let next = self.vertices[vertex as usize].next;
        let previous = self.vertices[vertex as usize].previous;
        if vertex == self.newvertex_list {
            self.newvertex_list = next;
        }
        if previous != NIL {
            self.vertices[previous as usize].next = next;
            self.vertices[next as usize].previous = previous;
        } else {
            self.vertex_list = next;
            self.vertices[next as usize].previous = NIL;
        }
        self.num_vertices -= 1;
    }

    /// `qh_prependfacet(facet, &qh.facet_next)`.
    pub fn prependfacet_next(&mut self, facet: u32) {
        let mut list = self.facet_next;
        self.prependfacet(facet, &mut list);
        self.facet_next = list;
    }

    /// `qh_prependfacet(facet, &qh.visible_list)`.
    pub fn prependfacet_visible(&mut self, facet: u32) {
        let mut list = self.visible_list;
        self.prependfacet(facet, &mut list);
        self.visible_list = list;
    }

    /// `qh_prependfacet`: puts the facet before the list's first facet (the tail for an empty
    /// list) and makes it the list's first. `facetlist` is a copy of the `qh` field the C passes
    /// by address, which the callers store back: the C's own updates of `qh.facet_list` and
    /// `qh.facet_next` come first, so a caller passing either keeps the C's order.
    fn prependfacet(&mut self, facet: u32, facetlist: &mut u32) {
        if *facetlist == NIL {
            *facetlist = self.facet_tail;
        }
        let list = *facetlist;
        let prevfacet = self.facets[list as usize].previous;
        self.facets[facet as usize].previous = prevfacet;
        if prevfacet != NIL {
            self.facets[prevfacet as usize].next = facet;
        }
        self.facets[list as usize].previous = facet;
        self.facets[facet as usize].next = list;
        if self.facet_list == list {
            self.facet_list = facet;
        }
        if self.facet_next == list {
            self.facet_next = facet;
        }
        *facetlist = facet;
        self.num_facets += 1;
    }

    /// `qh_delfacet`: unlinks the facet and frees it (its normal and centrum unless a
    /// tricoplanar facet shares them).
    pub fn delfacet(&mut self, facet: u32) {
        self.removefacet(facet);
        let f = &mut self.facets[facet as usize];
        if !f.tricoplanar || f.keepcentrum {
            f.normal = None;
            f.center = None;
        }
        f.neighbors = None;
        f.ridges = None;
        f.vertices = None;
        f.outsideset = None;
        f.coplanarset = None;
    }

    /// `qh_delvertex`: unlinks the vertex and frees it.
    pub fn delvertex(&mut self, vertex: u32) {
        self.removevertex(vertex);
        self.vertices[vertex as usize].neighbors = None;
    }

    /// `qh_delridge`: takes the ridge out of both facets' ridge sets and frees it.
    pub fn delridge(&mut self, ridge: u32) {
        let (top, bottom) = {
            let r = &self.ridges[ridge as usize];
            (r.top, r.bottom)
        };
        set::del(&mut self.facets[top as usize].ridges, ridge);
        set::del(&mut self.facets[bottom as usize].ridges, ridge);
        self.ridges[ridge as usize].vertices = None;
    }

    /// Frees a ridge without touching the facets' sets (`qh_setfree` of its vertices and
    /// `qh_memfree` of the ridge, as `qh_makenew_nonsimplicial` and the merges write it).
    pub fn freeridge(&mut self, ridge: u32) {
        self.ridges[ridge as usize].vertices = None;
    }

    /// `qh_deletevisible`: deletes the visible facets and the vertices of `del_vertices`.
    ///
    /// # Errors
    ///
    /// Qhull's error 6103 when the visible list does not hold `num_visible` facets.
    pub fn deletevisible(&mut self) -> QResult<()> {
        let mut numvisible = 0;
        let mut visible = self.visible_list;
        while visible != NIL && self.facets[visible as usize].visible {
            let nextfacet = self.facets[visible as usize].next;
            numvisible += 1;
            self.delfacet(visible);
            visible = nextfacet;
        }
        if numvisible != self.num_visible {
            return Err(Error::Qhull(6103));
        }
        self.num_visible = 0;
        let mut i = 0;
        loop {
            let vertex = set::elem(&self.del_vertices, i);
            i += 1;
            if vertex == NIL {
                break;
            }
            self.delvertex(vertex);
        }
        set::truncate(&mut self.del_vertices, 0);
        Ok(())
    }

    /// `qh_resetlists`: clears the new and visible lists (and, with `reset_visible`, the
    /// visible flags). The statistics of `stats` are not kept.
    pub fn resetlists(&mut self, reset_visible: bool) {
        let mut vertex = self.newvertex_list;
        while vertex != NIL && self.vertices[vertex as usize].next != NIL {
            self.vertices[vertex as usize].newlist = false;
            vertex = self.vertices[vertex as usize].next;
        }
        self.newvertex_list = NIL;
        let mut newfacet = self.newfacet_list;
        while newfacet != NIL && self.facets[newfacet as usize].next != NIL {
            self.facets[newfacet as usize].newfacet = false;
            newfacet = self.facets[newfacet as usize].next;
        }
        self.newfacet_list = NIL;
        if reset_visible {
            let mut visible = self.visible_list;
            while visible != NIL && self.facets[visible as usize].visible {
                self.facets[visible as usize].f = NIL;
                self.facets[visible as usize].visible = false;
                visible = self.facets[visible as usize].next;
            }
            self.num_visible = 0;
        }
        self.visible_list = NIL;
        self.new_facets = false;
    }

    /// `qh_willdelete`: moves the facet to the visible list, marked visible with its
    /// replacement.
    pub fn willdelete(&mut self, facet: u32, replace: u32) {
        self.removefacet(facet);
        self.prependfacet_visible(facet);
        self.num_visible += 1;
        let f = &mut self.facets[facet as usize];
        f.visible = true;
        f.f = replace;
    }

    /// `qh_pointid`: the index of an input point (the translation's points are indices).
    pub fn pointid(&self, point: u32) -> i32 {
        if point == NIL {
            return -3;
        }
        if (point as usize) < self.points.len() {
            i32::try_from(point).expect("fewer than 2^31 points")
        } else {
            ID_UNKNOWN
        }
    }

    /// `qh_point(id)`: the point of an id, or NULL.
    pub fn point(&self, id: i32) -> u32 {
        if id < 0 {
            return NIL;
        }
        if id < self.num_points {
            return u32::try_from(id).expect("a point id is not negative");
        }
        NIL
    }

    /// The coordinates of a point.
    #[inline]
    pub fn coords(&self, point: u32) -> [f64; 3] {
        self.points[point as usize]
    }

    /// `otherfacet_(ridge, facet)`: the ridge's other facet.
    #[inline]
    pub fn otherfacet(&self, ridge: u32, facet: u32) -> u32 {
        let r = &self.ridges[ridge as usize];
        if r.top == facet { r.bottom } else { r.top }
    }

    /// `qh_fprintf` of the C++ `QhullQh` for a message it keeps: appends what `vsnprintf` into a
    /// buffer of `MSG_MAXLEN` (3000) bytes gives, at most 2999 bytes.
    pub fn append_message(&mut self, text: &str) {
        let mut end = text.len().min(2999);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        self.message.push_str(&text[..end]);
    }
}

/// `fabs_(a)`: `((a) < 0 ? -(a) : (a))`, which keeps the sign of a negative zero and a NaN.
#[inline]
pub(crate) fn fabs_(a: f64) -> f64 {
    if a < 0.0 { -a } else { a }
}

/// `fmax_(a, b)`: `((a) < (b) ? (b) : (a))`.
#[inline]
pub(crate) fn fmax_(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

/// `fmin_(a, b)`: `((a) > (b) ? (b) : (a))`.
#[inline]
pub(crate) fn fmin_(a: f64, b: f64) -> f64 {
    if a > b { b } else { a }
}

/// `maximize_(maxval, val)`: `if ((maxval) < (val)) (maxval)= (val)`.
#[inline]
pub(crate) fn maximize_(maxval: &mut f64, val: f64) {
    if *maxval < val {
        *maxval = val;
    }
}

/// `minimize_(minval, val)`: `if ((minval) > (val)) (minval)= (val)`.
#[inline]
pub(crate) fn minimize_(minval: &mut f64, val: f64) {
    if *minval > val {
        *minval = val;
    }
}

/// `det2_(a1, a2, b1, b2)`: `((a1)*(b2) - (a2)*(b1))`.
#[inline]
pub(crate) fn det2_(a1: f64, a2: f64, b1: f64, b2: f64) -> f64 {
    a1 * b2 - a2 * b1
}

/// `det3_`: `((a1)*det2_(b2,b3,c2,c3) - (b1)*det2_(a2,a3,c2,c3) + (c1)*det2_(a2,a3,b2,b3))`.
#[allow(clippy::too_many_arguments, clippy::many_single_char_names)]
#[inline]
pub(crate) fn det3_(
    a1: f64,
    a2: f64,
    a3: f64,
    b1: f64,
    b2: f64,
    b3: f64,
    c1: f64,
    c2: f64,
    c3: f64,
) -> f64 {
    a1 * det2_(b2, b3, c2, c3) - b1 * det2_(a2, a3, c2, c3) + c1 * det2_(a2, a3, b2, b3)
}
