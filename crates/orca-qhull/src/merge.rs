// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Qhull's merging (`merge_r.c`) as `qh_premerge` runs it on each cone of new facets: the facets
//! coplanar with the horizon merge into it, duplicate ridges and flipped facets are merged away,
//! then each non-convex ridge (by the centrums, `C-0`), the most concave first, until the new
//! facets are clearly convex; degenerate and redundant facets go as they appear, and vertices that
//! lost their ridges are dropped or renamed.
//!
//! Carries Qhull code: the functions of Qhull 2015.2's `merge_r.c` that a 3-d hull with the
//! options of `its_convex_hull` reaches, translated into Rust on 2026-11-07, Copyright (c)
//! 1993-2015 C.B. Barber and The Geometry Center, University of Minnesota, under Qhull's licence
//! (`licenses/Qhull.txt`); the copyright holders are named in `NOTICE`.
//!
//! The merge sets are sorted with `orca-crt`'s `qsort`, the UCRT's own: `qh_compareangle` never
//! answers 0, so the order of equal angles is the sort's. A merge Qhull makes without an angle
//! leaves it uninitialised; no such merge is in `facet_mergeset` when it is sorted (`qh_premerge`
//! frees the duplicate-ridge and flipped merges first), so 0 stands for it. Both hash tables here
//! (`qh_find_newvertex`) only tell whether a ridge exists, which the table order does not change.

use crate::geom::getangle;
use crate::qh::{
    DIM, Error, MERGE_RIDGE, MRG_ANGLECOPLANAR, MRG_CONCAVE, MRG_COPLANAR, MRG_DEGEN, MRG_FLIP,
    MRG_REDUNDANT, MRG_RIDGE, Merge, MergeType, QResult, Qh, REAL_MAX, maximize_, minimize_,
};
use crate::set::{self, NIL, Set};

/// `qh_MAXnummerge`: `nummerge` is a 9-bit field.
const MAX_NUMMERGE: u32 = 511;
/// `qh_ANGLEconcave`: added (with 0.5) to the angle of a concave merge.
const ANGLE_CONCAVE: f64 = 1.5;
/// `qh_BESTcentrum`, `qh_BESTcentrum2`: from `qh_BESTcentrum2 * hull_dim + qh_BESTcentrum`
/// vertices `qh_findbestneighbor` tests centrums.
const BEST_CENTRUM: usize = 20;
const BEST_CENTRUM2: usize = 2;
/// `qh_BESTnonconvex`: from `hull_dim + qh_BESTnonconvex` vertices the non-convex ridges go first.
const BEST_NONCONVEX: usize = 15;
/// `qh_MAXnewcentrum`: up to `hull_dim + qh_MAXnewcentrum` vertices a centrum is recomputed.
const MAX_NEWCENTRUM: usize = 5;

/// `qh_setreplace` with Qhull's error.
fn replace(set: &mut Set, old: u32, new: u32) -> QResult<()> {
    set::replace(set, old, new).map_err(Error::Qhull)
}

impl Qh {
    /// `qh_premerge(apex, maxcentrum, maxangle)`: merges the new facets until they are clearly
    /// convex (or, when `qh_checkzero` finds them so already, nothing).
    ///
    /// # Errors
    ///
    /// The errors of the merges.
    pub(crate) fn premerge(&mut self, _apex: u32, maxcentrum: f64, maxangle: f64) -> QResult<()> {
        let mut othermerge = false;
        if self.zero_centrum && self.checkzero() {
            return Ok(());
        }
        self.centrum_radius = maxcentrum;
        self.cos_max = maxangle;
        self.degen_mergeset = Some(Vec::new());
        self.facet_mergeset = Some(Vec::new());
        // hull_dim >= 3.
        self.mark_dupridges(self.newfacet_list)?;
        self.mergecycle_all(self.newfacet_list, &mut othermerge)?;
        self.forcedmerges(&mut othermerge)?;
        let mut newfacet = self.newfacet_list;
        while newfacet != NIL && self.facets[newfacet as usize].next != NIL {
            let f = &self.facets[newfacet as usize];
            if !f.simplicial && !f.mergeridge {
                self.degen_redundant_neighbors(newfacet, NIL)?;
            }
            newfacet = self.facets[newfacet as usize].next;
        }
        if self.merge_degenredundant()? != 0 {
            othermerge = true;
        }
        self.flippedmerges(self.newfacet_list, &mut othermerge)?;
        // Not MERGEexact; POSTmerging is False.
        self.getmergeset_initial(self.newfacet_list)?;
        self.all_merges(othermerge)?;
        self.facet_mergeset = None;
        self.degen_mergeset = None;
        Ok(())
    }

    /// `qh_checkzero(!qh_ALL)`: whether the new facets are clearly convex already (each vertex
    /// below its facet's neighbours, and the horizon's far vertex below the facet), so no merge
    /// is needed; `ZEROall_ok` is cleared otherwise (not `MERGEexact`).
    fn checkzero(&mut self) -> bool {
        let facetlist = self.newfacet_list;
        let mut facet = facetlist;
        while facet != NIL && self.facets[facet as usize].next != NIL {
            let f = &self.facets[facet as usize];
            let horizon = set::first(&f.neighbors);
            if !self.facets[horizon as usize].simplicial
                || f.flipped
                || f.dupridge
                || f.normal.is_none()
            {
                self.zero_all_ok = false;
                return false;
            }
            facet = f.next;
        }
        let mut facet = facetlist;
        while facet != NIL && self.facets[facet as usize].next != NIL {
            let fi = facet as usize;
            self.vertex_visit = self.vertex_visit.wrapping_add(1);
            let mut neighbor_i = 0;
            let mut horizon = NIL;
            let mut i = 0;
            loop {
                let neighbor = set::elem(&self.facets[fi].neighbors, i);
                i += 1;
                if neighbor == NIL {
                    break;
                }
                if neighbor_i == 0 {
                    horizon = neighbor;
                    neighbor_i += 1;
                    continue;
                }
                let vertex = set::elem(&self.facets[fi].vertices, neighbor_i);
                neighbor_i += 1;
                self.vertices[vertex as usize].visitid = self.vertex_visit;
                let dist = self.distplane_point(self.vertices[vertex as usize].point, neighbor);
                if dist >= -self.dist_round {
                    self.zero_all_ok = false;
                    return false;
                }
            }
            if horizon != NIL {
                let mut j = 0;
                loop {
                    let vertex = set::elem(&self.facets[horizon as usize].vertices, j);
                    j += 1;
                    if vertex == NIL {
                        break;
                    }
                    if self.vertices[vertex as usize].visitid != self.vertex_visit {
                        let dist =
                            self.distplane_point(self.vertices[vertex as usize].point, facet);
                        if dist >= -self.dist_round {
                            self.zero_all_ok = false;
                            return false;
                        }
                        break;
                    }
                }
            }
            facet = self.facets[fi].next;
        }
        true
    }

    /// `qh_appendmergeset(facet, neighbor, mergetype, angle)`: queues a merge, the convexity
    /// merges in `facet_mergeset`, the others in `degen_mergeset` (degenerate merges first).
    ///
    /// # Errors
    ///
    /// Qhull's internal errors 6092 and 6093 for a bad mirror pair.
    pub(crate) fn appendmergeset(
        &mut self,
        facet: u32,
        neighbor: u32,
        mergetype: MergeType,
        angle: Option<f64>,
    ) -> QResult<()> {
        let f = facet as usize;
        if self.facets[f].redundant {
            return Ok(());
        }
        if self.facets[f].degenerate && mergetype == MRG_DEGEN {
            return Ok(());
        }
        let merge = u32::try_from(self.merges.len()).expect("fewer than 2^32 merges");
        // ANGLEmerge is True; a merge without an angle keeps 0 (see the module notes).
        self.merges.push(Merge {
            angle: angle.unwrap_or(0.0),
            facet1: facet,
            facet2: neighbor,
            mtype: mergetype,
        });
        if mergetype < MRG_DEGEN {
            set::append(&mut self.facet_mergeset, merge);
        } else if mergetype == MRG_DEGEN {
            self.facets[f].degenerate = true;
            let lastmerge = set::last(&self.degen_mergeset);
            if lastmerge == NIL || self.merges[lastmerge as usize].mtype == MRG_DEGEN {
                set::append(&mut self.degen_mergeset, merge);
            } else {
                set::add_nth(&mut self.degen_mergeset, 0, merge);
            }
        } else if mergetype == MRG_REDUNDANT {
            self.facets[f].redundant = true;
            set::append(&mut self.degen_mergeset, merge);
        } else {
            // MRGmirror.
            if self.facets[neighbor as usize].redundant {
                return Err(Error::Qhull(6092));
            }
            if !set::equal(
                &self.facets[f].vertices,
                &self.facets[neighbor as usize].vertices,
            ) {
                return Err(Error::Qhull(6093));
            }
            self.facets[f].redundant = true;
            self.facets[neighbor as usize].redundant = true;
            set::append(&mut self.degen_mergeset, merge);
        }
        Ok(())
    }

    /// `qh_mark_dupridges(facetlist)`: queues a merge for each duplicate ridge, and makes the
    /// ridges of the facets it touches.
    fn mark_dupridges(&mut self, facetlist: u32) -> QResult<()> {
        let mut nummerge = 0;
        let mut facet = facetlist;
        while facet != NIL && self.facets[facet as usize].next != NIL {
            let fi = facet as usize;
            if self.facets[fi].dupridge {
                let mut i = 0;
                loop {
                    let neighbor = set::elem(&self.facets[fi].neighbors, i);
                    i += 1;
                    if neighbor == NIL {
                        break;
                    }
                    if neighbor == MERGE_RIDGE {
                        self.facets[fi].mergeridge = true;
                        continue;
                    }
                    let n = neighbor as usize;
                    if self.facets[n].dupridge && !set::contains(&self.facets[n].neighbors, facet) {
                        self.appendmergeset(facet, neighbor, MRG_RIDGE, None)?;
                        self.facets[fi].mergeridge2 = true;
                        self.facets[fi].mergeridge = true;
                        nummerge += 1;
                    }
                }
            }
            facet = self.facets[fi].next;
        }
        if nummerge == 0 {
            return Ok(());
        }
        let mut facet = facetlist;
        while facet != NIL && self.facets[facet as usize].next != NIL {
            let f = &self.facets[facet as usize];
            if f.mergeridge && !f.mergeridge2 {
                self.makeridges(facet);
            }
            facet = self.facets[facet as usize].next;
        }
        let mut i = 0;
        loop {
            let merge = set::elem(&self.facet_mergeset, i);
            i += 1;
            if merge == NIL {
                break;
            }
            let m = self.merges[merge as usize];
            if m.mtype == MRG_RIDGE {
                set::append(&mut self.facets[m.facet2 as usize].neighbors, m.facet1);
                self.makeridges(m.facet1);
            }
        }
        Ok(())
    }

    /// `qh_makeridges(facet)`: the explicit ridges of a simplicial facet to the neighbours it has
    /// none with, which makes it non-simplicial; drops its `qh_MERGEridge` neighbours.
    pub(crate) fn makeridges(&mut self, facet: u32) {
        let f = facet as usize;
        if !self.facets[f].simplicial {
            return;
        }
        self.facets[f].simplicial = false;
        let mut mergeridge = false;
        let mut i = 0;
        loop {
            let neighbor = set::elem(&self.facets[f].neighbors, i);
            i += 1;
            if neighbor == NIL {
                break;
            }
            if neighbor == MERGE_RIDGE {
                mergeridge = true;
            } else {
                self.facets[neighbor as usize].seen = false;
            }
        }
        let mut i = 0;
        loop {
            let ridge = set::elem(&self.facets[f].ridges, i);
            i += 1;
            if ridge == NIL {
                break;
            }
            let other = self.otherfacet(ridge, facet);
            self.facets[other as usize].seen = true;
        }
        let neighbor_n = set::size(&self.facets[f].neighbors);
        for neighbor_i in 0..neighbor_n {
            let neighbor = set::elem(&self.facets[f].neighbors, neighbor_i);
            if neighbor == MERGE_RIDGE {
                continue;
            }
            if !self.facets[neighbor as usize].seen {
                let ridge = self.newridge();
                let r = ridge as usize;
                self.ridges[r].vertices =
                    set::new_del_nth_sorted(&self.facets[f].vertices, DIM, neighbor_i, 0);
                let toporient = self.facets[f].toporient ^ (neighbor_i & 1 != 0);
                if toporient {
                    self.ridges[r].top = facet;
                    self.ridges[r].bottom = neighbor;
                } else {
                    self.ridges[r].top = neighbor;
                    self.ridges[r].bottom = facet;
                }
                set::append(&mut self.facets[f].ridges, ridge);
                set::append(&mut self.facets[neighbor as usize].ridges, ridge);
            }
        }
        if mergeridge {
            while set::del(&mut self.facets[f].neighbors, MERGE_RIDGE) {}
        }
    }

    /// `qh_mergecycle_all(facetlist, &wasmerge)`: merges each cycle of new facets coplanar with
    /// a horizon facet (they have no hyperplane) into it.
    fn mergecycle_all(&mut self, facetlist: u32, wasmerge: &mut bool) -> QResult<()> {
        let mut cycles = 0;
        let mut facet = facetlist;
        while facet != NIL {
            let mut nextfacet = self.facets[facet as usize].next;
            if nextfacet == NIL {
                break;
            }
            let fi = facet as usize;
            if self.facets[fi].normal.is_some() {
                facet = nextfacet;
                continue;
            }
            if !self.facets[fi].mergehorizon {
                return Err(Error::Qhull(6225));
            }
            let horizon = set::first(&self.facets[fi].neighbors);
            if self.facets[fi].f == facet {
                let apex = set::first(&self.facets[fi].vertices);
                let mut i = 0;
                loop {
                    let vertex = set::elem(&self.facets[fi].vertices, i);
                    i += 1;
                    if vertex == NIL {
                        break;
                    }
                    if vertex != apex {
                        self.vertices[vertex as usize].delridge = true;
                    }
                }
                self.facets[horizon as usize].f = NIL;
                self.mergefacet(facet, horizon, None, true)?;
            } else {
                let samecycle = facet;
                let mut facets: u32 = 0;
                let mut prev = facet;
                let mut same = self.facets[fi].f;
                while same != NIL {
                    let s = same as usize;
                    let nextsame = self.facets[s].f;
                    if self.facets[s].cycledone || self.facets[s].visible {
                        return Err(Error::Qhull(6149));
                    }
                    self.facets[s].cycledone = true;
                    if self.facets[s].normal.is_some() {
                        self.facets[prev as usize].f = self.facets[s].f;
                        self.facets[s].f = NIL;
                    } else {
                        prev = same;
                        facets += 1;
                    }
                    same = if same == facet { NIL } else { nextsame };
                }
                while nextfacet != NIL && self.facets[nextfacet as usize].cycledone {
                    nextfacet = self.facets[nextfacet as usize].next;
                }
                self.facets[horizon as usize].f = NIL;
                self.mergecycle(samecycle, horizon)?;
                let h = horizon as usize;
                let nummerge = self.facets[h].nummerge + facets;
                self.facets[h].nummerge = if nummerge > MAX_NUMMERGE {
                    MAX_NUMMERGE
                } else {
                    nummerge
                };
            }
            cycles += 1;
            facet = nextfacet;
        }
        if cycles != 0 {
            *wasmerge = true;
        }
        Ok(())
    }

    /// `qh_mergecycle(samecycle, newfacet)`: merges a cycle of new facets into the horizon facet
    /// `newfacet`.
    fn mergecycle(&mut self, samecycle: u32, newfacet: u32) -> QResult<()> {
        if self.facets[newfacet as usize].tricoplanar {
            return Err(Error::Qhull(6224));
        }
        if !self.vertex_neighbors {
            self.vertexneighbors();
        }
        self.ztotmerge += 1;
        let apex = set::first(&self.facets[samecycle as usize].vertices);
        self.makeridges(newfacet);
        self.mergecycle_neighbors(samecycle, newfacet)?;
        self.mergecycle_ridges(samecycle, newfacet)?;
        self.mergecycle_vneighbors(samecycle, newfacet);
        if set::first(&self.facets[newfacet as usize].vertices) != apex {
            set::add_nth(&mut self.facets[newfacet as usize].vertices, 0, apex);
        }
        if !self.facets[newfacet as usize].newfacet {
            self.newvertices(newfacet);
        }
        self.mergecycle_facets(samecycle, newfacet);
        Ok(())
    }

    /// The next facet of `FORALLsame_cycle_(samecycle)` after `same`.
    fn next_same(&self, same: u32, samecycle: u32) -> u32 {
        if same == samecycle {
            NIL
        } else {
            self.facets[same as usize].f
        }
    }

    /// `qh_mergecycle_neighbors(samecycle, newfacet)`: the cycle's neighbours become the
    /// horizon facet's.
    fn mergecycle_neighbors(&mut self, samecycle: u32, newfacet: u32) -> QResult<()> {
        self.visit_id = self.visit_id.wrapping_add(1);
        let samevisitid = self.visit_id;
        let mut same = self.facets[samecycle as usize].f;
        while same != NIL {
            let s = same as usize;
            if self.facets[s].visitid == samevisitid || self.facets[s].visible {
                return Err(Error::Qhull(6149));
            }
            self.facets[s].visitid = samevisitid;
            same = self.next_same(same, samecycle);
        }
        self.visit_id = self.visit_id.wrapping_add(1);
        let visit_id = self.visit_id;
        let nf = newfacet as usize;
        self.facets[nf].visitid = visit_id;
        let mut i = 0;
        loop {
            let neighbor = set::elem(&self.facets[nf].neighbors, i);
            i += 1;
            if neighbor == NIL {
                break;
            }
            if self.facets[neighbor as usize].visitid == samevisitid {
                set::set_elem(&mut self.facets[nf].neighbors, i - 1, NIL);
            } else {
                self.facets[neighbor as usize].visitid = visit_id;
            }
        }
        set::compact(&mut self.facets[nf].neighbors);
        let mut same = self.facets[samecycle as usize].f;
        while same != NIL {
            let s = same as usize;
            let mut i = 0;
            loop {
                let neighbor = set::elem(&self.facets[s].neighbors, i);
                i += 1;
                if neighbor == NIL {
                    break;
                }
                let n = neighbor as usize;
                if self.facets[n].visitid == samevisitid {
                    continue;
                }
                if self.facets[n].simplicial {
                    if self.facets[n].visitid == visit_id {
                        self.makeridges(neighbor);
                        set::del(&mut self.facets[n].neighbors, same);
                    } else {
                        set::append(&mut self.facets[nf].neighbors, neighbor);
                        replace(&mut self.facets[n].neighbors, same, newfacet)?;
                        self.facets[n].visitid = visit_id;
                        let mut j = 0;
                        loop {
                            let ridge = set::elem(&self.facets[n].ridges, j);
                            j += 1;
                            if ridge == NIL {
                                break;
                            }
                            let r = &mut self.ridges[ridge as usize];
                            if r.top == same {
                                r.top = newfacet;
                                break;
                            } else if r.bottom == same {
                                r.bottom = newfacet;
                                break;
                            }
                        }
                    }
                } else {
                    set::del(&mut self.facets[n].neighbors, same);
                    if self.facets[n].visitid != visit_id {
                        set::append(&mut self.facets[n].neighbors, newfacet);
                        set::append(&mut self.facets[nf].neighbors, neighbor);
                        self.facets[n].visitid = visit_id;
                    }
                }
            }
            same = self.next_same(same, samecycle);
        }
        Ok(())
    }

    /// `qh_mergecycle_ridges(samecycle, newfacet)`: the cycle's ridges become the horizon
    /// facet's, those inside the cycle are freed, and those to simplicial neighbours are made.
    fn mergecycle_ridges(&mut self, samecycle: u32, newfacet: u32) -> QResult<()> {
        let samevisitid = self.visit_id.wrapping_sub(1);
        let nf = newfacet as usize;
        let mut i = 0;
        loop {
            let ridge = set::elem(&self.facets[nf].ridges, i);
            i += 1;
            if ridge == NIL {
                break;
            }
            let neighbor = self.otherfacet(ridge, newfacet);
            if self.facets[neighbor as usize].visitid == samevisitid {
                set::set_elem(&mut self.facets[nf].ridges, i - 1, NIL);
            }
        }
        set::compact(&mut self.facets[nf].ridges);
        let mut same = self.facets[samecycle as usize].f;
        while same != NIL {
            let s = same as usize;
            let mut i = 0;
            loop {
                let ridge = set::elem(&self.facets[s].ridges, i);
                i += 1;
                if ridge == NIL {
                    break;
                }
                let r = ridge as usize;
                let neighbor;
                if self.ridges[r].top == same {
                    self.ridges[r].top = newfacet;
                    neighbor = self.ridges[r].bottom;
                } else if self.ridges[r].bottom == same {
                    self.ridges[r].bottom = newfacet;
                    neighbor = self.ridges[r].top;
                } else if self.ridges[r].top == newfacet || self.ridges[r].bottom == newfacet {
                    set::append(&mut self.facets[nf].ridges, ridge);
                    continue;
                } else {
                    return Err(Error::Qhull(6098));
                }
                if neighbor == newfacet {
                    self.freeridge(ridge);
                } else if self.facets[neighbor as usize].visitid == samevisitid {
                    set::del(&mut self.facets[neighbor as usize].ridges, ridge);
                    self.freeridge(ridge);
                } else {
                    set::append(&mut self.facets[nf].ridges, ridge);
                }
            }
            if self.facets[s].ridges.is_some() {
                set::truncate(&mut self.facets[s].ridges, 0);
            }
            if self.facets[s].simplicial {
                let neighbor_n = set::size(&self.facets[s].neighbors);
                for neighbor_i in 0..neighbor_n {
                    let neighbor = set::elem(&self.facets[s].neighbors, neighbor_i);
                    let n = neighbor as usize;
                    if self.facets[n].visitid != samevisitid && self.facets[n].simplicial {
                        let ridge = self.newridge();
                        let r = ridge as usize;
                        self.ridges[r].vertices =
                            set::new_del_nth_sorted(&self.facets[s].vertices, DIM, neighbor_i, 0);
                        let toporient = self.facets[s].toporient ^ (neighbor_i & 1 != 0);
                        if toporient {
                            self.ridges[r].top = newfacet;
                            self.ridges[r].bottom = neighbor;
                        } else {
                            self.ridges[r].top = neighbor;
                            self.ridges[r].bottom = newfacet;
                        }
                        set::append(&mut self.facets[nf].ridges, ridge);
                        set::append(&mut self.facets[n].ridges, ridge);
                    }
                }
            }
            same = self.next_same(same, samecycle);
        }
        Ok(())
    }

    /// `qh_mergecycle_vneighbors(samecycle, newfacet)`: the cycle's vertices take the horizon
    /// facet as their neighbour instead; a vertex left with one neighbour is deleted.
    fn mergecycle_vneighbors(&mut self, samecycle: u32, newfacet: u32) {
        let mergeid = self.visit_id.wrapping_sub(1);
        self.facets[newfacet as usize].visitid = mergeid;
        let mut vertices = self.basevertices(samecycle);
        let apex = set::first(&self.facets[samecycle as usize].vertices);
        set::append(&mut vertices, apex);
        let mut i = 0;
        loop {
            let vertex = set::elem(&vertices, i);
            i += 1;
            if vertex == NIL {
                break;
            }
            let v = vertex as usize;
            self.vertices[v].delridge = true;
            let mut j = 0;
            loop {
                let neighbor = set::elem(&self.vertices[v].neighbors, j);
                j += 1;
                if neighbor == NIL {
                    break;
                }
                if self.facets[neighbor as usize].visitid == mergeid {
                    set::set_elem(&mut self.vertices[v].neighbors, j - 1, NIL);
                }
            }
            set::compact(&mut self.vertices[v].neighbors);
            set::append(&mut self.vertices[v].neighbors, newfacet);
            if set::second(&self.vertices[v].neighbors) == NIL {
                set::del_sorted(&mut self.facets[newfacet as usize].vertices, vertex);
                self.vertices[v].deleted = true;
                set::append(&mut self.del_vertices, vertex);
            }
        }
    }

    /// `qh_basevertices(samecycle)`: the vertices of the cycle's facets but its apex (and those
    /// of facets with a `qh_MERGEridge`), each once.
    fn basevertices(&mut self, samecycle: u32) -> Set {
        let mut vertices: Set = Some(Vec::new());
        let apex = set::first(&self.facets[samecycle as usize].vertices);
        self.vertex_visit = self.vertex_visit.wrapping_add(1);
        self.vertices[apex as usize].visitid = self.vertex_visit;
        let mut same = self.facets[samecycle as usize].f;
        while same != NIL {
            let s = same as usize;
            if !self.facets[s].mergeridge {
                let mut i = 0;
                loop {
                    let vertex = set::elem(&self.facets[s].vertices, i);
                    i += 1;
                    if vertex == NIL {
                        break;
                    }
                    let v = &mut self.vertices[vertex as usize];
                    if v.visitid != self.vertex_visit {
                        set::append(&mut vertices, vertex);
                        v.visitid = self.vertex_visit;
                        v.seen = false;
                    }
                }
            }
            same = self.next_same(same, samecycle);
        }
        vertices
    }

    /// `qh_mergecycle_facets(samecycle, newfacet)`: deletes the cycle's facets, replaced by the
    /// horizon facet, which moves to the new facets.
    fn mergecycle_facets(&mut self, samecycle: u32, newfacet: u32) {
        self.removefacet(newfacet);
        self.appendfacet(newfacet);
        let nf = newfacet as usize;
        self.facets[nf].newfacet = true;
        self.facets[nf].simplicial = false;
        self.facets[nf].newmerge = true;
        let mut same = self.facets[samecycle as usize].f;
        while same != NIL {
            let next = self.facets[same as usize].f;
            self.willdelete(same, newfacet);
            same = if same == samecycle { NIL } else { next };
        }
        if self.facets[nf].center.is_some()
            && set::size(&self.facets[nf].vertices) <= DIM + MAX_NEWCENTRUM
        {
            self.facets[nf].center = None;
        }
    }

    /// `qh_forcedmerges(&wasmerge)`: merges the facets of each duplicate ridge, the nearer into
    /// the other.
    fn forcedmerges(&mut self, wasmerge: &mut bool) -> QResult<()> {
        let mut nummerge = 0;
        let othermerges = self.facet_mergeset.take();
        self.facet_mergeset = Some(Vec::new());
        let mut i = 0;
        loop {
            let merge = set::elem(&othermerges, i);
            i += 1;
            if merge == NIL {
                break;
            }
            let m = self.merges[merge as usize];
            if m.mtype != MRG_RIDGE {
                continue;
            }
            let mut facet1 = m.facet1;
            let mut facet2 = m.facet2;
            while self.facets[facet1 as usize].visible {
                facet1 = self.facets[facet1 as usize].f;
            }
            while self.facets[facet2 as usize].visible {
                facet2 = self.facets[facet2 as usize].f;
            }
            if facet1 == facet2 {
                continue;
            }
            if !set::contains(&self.facets[facet2 as usize].neighbors, facet1) {
                return Err(Error::Qhull(6096));
            }
            let (dist1, mindist1, maxdist1) = self.getdistance(facet1, facet2);
            let (dist2, mindist2, maxdist2) = self.getdistance(facet2, facet1);
            self.check_dupridge(dist1, dist2)?;
            if dist1 < dist2 {
                self.mergefacet(facet1, facet2, Some((mindist1, maxdist1)), false)?;
            } else {
                self.mergefacet(facet2, facet1, Some((mindist2, maxdist2)), false)?;
                facet1 = facet2;
            }
            if !self.facets[facet1 as usize].flipped {
                nummerge += 1;
            }
        }
        let mut i = 0;
        loop {
            let merge = set::elem(&othermerges, i);
            i += 1;
            if merge == NIL {
                break;
            }
            if self.merges[merge as usize].mtype != MRG_RIDGE {
                set::append(&mut self.facet_mergeset, merge);
            }
        }
        if nummerge != 0 {
            *wasmerge = true;
        }
        Ok(())
    }

    /// `qh_degen_redundant_neighbors(facet, delfacet)`: queues the facet if it is degenerate,
    /// and the neighbours of `delfacet` (the facet itself for NULL) that are redundant to it or
    /// degenerate.
    fn degen_redundant_neighbors(&mut self, facet: u32, delfacet: u32) -> QResult<()> {
        let f = facet as usize;
        if set::size(&self.facets[f].neighbors) < DIM {
            self.appendmergeset(facet, facet, MRG_DEGEN, None)?;
        }
        let delfacet = if delfacet == NIL { facet } else { delfacet };
        self.vertex_visit = self.vertex_visit.wrapping_add(1);
        let mut i = 0;
        loop {
            let vertex = set::elem(&self.facets[f].vertices, i);
            i += 1;
            if vertex == NIL {
                break;
            }
            self.vertices[vertex as usize].visitid = self.vertex_visit;
        }
        let mut i = 0;
        loop {
            let neighbor = set::elem(&self.facets[delfacet as usize].neighbors, i);
            i += 1;
            if neighbor == NIL {
                break;
            }
            if neighbor == facet {
                continue;
            }
            let mut j = 0;
            let mut vertex;
            loop {
                vertex = set::elem(&self.facets[neighbor as usize].vertices, j);
                j += 1;
                if vertex == NIL || self.vertices[vertex as usize].visitid != self.vertex_visit {
                    break;
                }
            }
            if vertex == NIL {
                self.appendmergeset(neighbor, facet, MRG_REDUNDANT, None)?;
            }
        }
        let mut i = 0;
        loop {
            let neighbor = set::elem(&self.facets[delfacet as usize].neighbors, i);
            i += 1;
            if neighbor == NIL {
                break;
            }
            if neighbor == facet {
                continue;
            }
            if set::size(&self.facets[neighbor as usize].neighbors) < DIM {
                self.appendmergeset(neighbor, neighbor, MRG_DEGEN, None)?;
            }
        }
        Ok(())
    }

    /// `qh_degen_redundant_facet(facet)`: queues the facet if it is redundant to a neighbour
    /// (the first) or degenerate.
    fn degen_redundant_facet(&mut self, facet: u32) -> QResult<()> {
        let f = facet as usize;
        let mut i = 0;
        loop {
            let neighbor = set::elem(&self.facets[f].neighbors, i);
            i += 1;
            if neighbor == NIL {
                break;
            }
            self.vertex_visit = self.vertex_visit.wrapping_add(1);
            let mut j = 0;
            loop {
                let vertex = set::elem(&self.facets[neighbor as usize].vertices, j);
                j += 1;
                if vertex == NIL {
                    break;
                }
                self.vertices[vertex as usize].visitid = self.vertex_visit;
            }
            let mut j = 0;
            let mut vertex;
            loop {
                vertex = set::elem(&self.facets[f].vertices, j);
                j += 1;
                if vertex == NIL || self.vertices[vertex as usize].visitid != self.vertex_visit {
                    break;
                }
            }
            if vertex == NIL {
                return self.appendmergeset(facet, neighbor, MRG_REDUNDANT, None);
            }
        }
        if set::size(&self.facets[f].neighbors) < DIM {
            self.appendmergeset(facet, facet, MRG_DEGEN, None)?;
        }
        Ok(())
    }

    /// `qh_merge_degenredundant`: merges the queued redundant facets into their neighbours and
    /// the degenerate ones into their best neighbour (or deletes one with no neighbour); returns
    /// how many.
    fn merge_degenredundant(&mut self) -> QResult<i32> {
        let mut nummerges = 0;
        loop {
            let merge = set::del_last(&mut self.degen_mergeset);
            if merge == NIL {
                break;
            }
            let m = self.merges[merge as usize];
            let facet1 = m.facet1;
            let mut facet2 = m.facet2;
            let f1 = facet1 as usize;
            if self.facets[f1].visible {
                continue;
            }
            self.facets[f1].degenerate = false;
            self.facets[f1].redundant = false;
            if m.mtype == MRG_REDUNDANT {
                while self.facets[facet2 as usize].visible {
                    let replace = self.facets[facet2 as usize].f;
                    if replace == NIL {
                        return Err(Error::Qhull(6097));
                    }
                    facet2 = replace;
                }
                if facet1 == facet2 {
                    self.degen_redundant_facet(facet1)?;
                    continue;
                }
                self.mergefacet(facet1, facet2, None, false)?;
                nummerges += 1;
            } else {
                let size = set::size(&self.facets[f1].neighbors);
                if size == 0 {
                    self.willdelete(facet1, NIL);
                    let mut i = 0;
                    loop {
                        let vertex = set::elem(&self.facets[f1].vertices, i);
                        i += 1;
                        if vertex == NIL {
                            break;
                        }
                        let v = vertex as usize;
                        set::del(&mut self.vertices[v].neighbors, facet1);
                        if set::first(&self.vertices[v].neighbors) == NIL {
                            self.vertices[v].deleted = true;
                            set::append(&mut self.del_vertices, vertex);
                        }
                    }
                    nummerges += 1;
                } else if size < DIM {
                    let (bestneighbor, _dist, mindist, maxdist) = self.findbestneighbor(facet1)?;
                    self.mergefacet(facet1, bestneighbor, Some((mindist, maxdist)), false)?;
                    nummerges += 1;
                }
            }
        }
        Ok(nummerges)
    }

    /// `qh_flippedmerges(facetlist, &wasmerge)`: merges each flipped new facet into its best
    /// neighbour.
    fn flippedmerges(&mut self, facetlist: u32, wasmerge: &mut bool) -> QResult<()> {
        let mut nummerge = 0;
        let mut facet = facetlist;
        while facet != NIL && self.facets[facet as usize].next != NIL {
            let f = &self.facets[facet as usize];
            if f.flipped && !f.visible {
                self.appendmergeset(facet, facet, MRG_FLIP, None)?;
            }
            facet = self.facets[facet as usize].next;
        }
        let othermerges = self.facet_mergeset.take();
        self.facet_mergeset = Some(Vec::new());
        let mut i = 0;
        loop {
            let merge = set::elem(&othermerges, i);
            i += 1;
            if merge == NIL {
                break;
            }
            let m = self.merges[merge as usize];
            let facet1 = m.facet1;
            if m.mtype != MRG_FLIP || self.facets[facet1 as usize].visible {
                continue;
            }
            let (neighbor, _dist, mindist, maxdist) = self.findbestneighbor(facet1)?;
            self.mergefacet(facet1, neighbor, Some((mindist, maxdist)), false)?;
            nummerge += 1;
            self.merge_degenredundant()?;
        }
        let mut i = 0;
        loop {
            let merge = set::elem(&othermerges, i);
            i += 1;
            if merge == NIL {
                break;
            }
            let m = self.merges[merge as usize];
            if !self.facets[m.facet1 as usize].visible && !self.facets[m.facet2 as usize].visible {
                set::append(&mut self.facet_mergeset, merge);
            }
        }
        if nummerge != 0 {
            *wasmerge = true;
        }
        Ok(())
    }

    /// The `qsort(facet_mergeset, ..., qh_compareangle)` of `qh_getmergeset` and
    /// `qh_getmergeset_initial` (`ANGLEmerge`).
    fn sort_mergeset(&mut self) {
        let merges = &self.merges;
        if let Some(v) = self.facet_mergeset.as_mut() {
            orca_crt::qsort(v.as_mut_slice(), |a, b| {
                if merges[*a as usize].angle > merges[*b as usize].angle {
                    1
                } else {
                    -1
                }
            });
        }
    }

    /// `qh_getmergeset_initial(facetlist)`: queues the non-convex ridges of the new facets,
    /// sorted by angle.
    fn getmergeset_initial(&mut self, facetlist: u32) -> QResult<()> {
        self.visit_id = self.visit_id.wrapping_add(1);
        let visit_id = self.visit_id;
        let mut facet = facetlist;
        while facet != NIL && self.facets[facet as usize].next != NIL {
            let f = facet as usize;
            self.facets[f].visitid = visit_id;
            self.facets[f].tested = true;
            let mut i = 0;
            loop {
                let neighbor = set::elem(&self.facets[f].neighbors, i);
                i += 1;
                if neighbor == NIL {
                    break;
                }
                if self.facets[neighbor as usize].visitid != visit_id
                    && self.test_appendmerge(facet, neighbor)?
                {
                    let mut j = 0;
                    loop {
                        let ridge = set::elem(&self.facets[neighbor as usize].ridges, j);
                        j += 1;
                        if ridge == NIL {
                            break;
                        }
                        if facet == self.otherfacet(ridge, neighbor) {
                            self.ridges[ridge as usize].nonconvex = true;
                            break;
                        }
                    }
                }
            }
            let mut i = 0;
            loop {
                let ridge = set::elem(&self.facets[f].ridges, i);
                i += 1;
                if ridge == NIL {
                    break;
                }
                self.ridges[ridge as usize].tested = true;
            }
            facet = self.facets[f].next;
        }
        self.sort_mergeset();
        Ok(())
    }

    /// `qh_getmergeset(facetlist)`: queues the untested non-convex ridges of the facets not yet
    /// tested, sorted by angle.
    fn getmergeset(&mut self, facetlist: u32) -> QResult<()> {
        self.visit_id = self.visit_id.wrapping_add(1);
        let visit_id = self.visit_id;
        let mut facet = facetlist;
        while facet != NIL && self.facets[facet as usize].next != NIL {
            let f = facet as usize;
            let next = self.facets[f].next;
            if self.facets[f].tested {
                facet = next;
                continue;
            }
            self.facets[f].visitid = visit_id;
            self.facets[f].tested = true;
            let mut i = 0;
            loop {
                let neighbor = set::elem(&self.facets[f].neighbors, i);
                i += 1;
                if neighbor == NIL {
                    break;
                }
                self.facets[neighbor as usize].seen = false;
            }
            let mut i = 0;
            loop {
                let ridge = set::elem(&self.facets[f].ridges, i);
                i += 1;
                if ridge == NIL {
                    break;
                }
                let r = ridge as usize;
                if self.ridges[r].tested && !self.ridges[r].nonconvex {
                    continue;
                }
                let neighbor = self.otherfacet(ridge, facet);
                let n = neighbor as usize;
                if self.facets[n].seen {
                    self.ridges[r].tested = true;
                    self.ridges[r].nonconvex = false;
                } else if self.facets[n].visitid != visit_id {
                    self.ridges[r].tested = true;
                    self.ridges[r].nonconvex = false;
                    self.facets[n].seen = true;
                    if self.test_appendmerge(facet, neighbor)? {
                        self.ridges[r].nonconvex = true;
                    }
                }
            }
            facet = self.facets[f].next;
        }
        self.sort_mergeset();
        Ok(())
    }

    /// `qh_test_appendmerge(facet, neighbor)`: queues a merge if the ridge between the facets is
    /// concave or coplanar by their centrums (`C-0`; there is no angle test, `cos_max` being
    /// `REALmax`), with the angle between their normals (2 more if concave).
    ///
    /// # Errors
    ///
    /// As [`Qh::getcentrum`].
    fn test_appendmerge(&mut self, facet: u32, neighbor: u32) -> QResult<bool> {
        let mut angle = -REAL_MAX;
        let mut isconcave = false;
        let mut iscoplanar = false;
        let mut okangle = false;
        let (f, n) = (facet as usize, neighbor as usize);
        // Not SKIPconvex or MERGEexact.
        if self.cos_max < REAL_MAX / 2.0 {
            let a = self.facets[f].normal.expect("a facet's hyperplane");
            let b = self.facets[n].normal.expect("a facet's hyperplane");
            angle = getangle(&a, &b);
            if angle > self.cos_max {
                self.appendmergeset(facet, neighbor, MRG_ANGLECOPLANAR, Some(angle))?;
                return Ok(true);
            }
            okangle = true;
        }
        if self.facets[f].center.is_none() {
            self.facets[f].center = Some(self.getcentrum(facet)?);
        }
        let center = self.facets[f].center.expect("the centrum just set");
        let dist = self.distplane(&center, neighbor);
        if dist > self.centrum_radius {
            isconcave = true;
        } else {
            if dist > -self.centrum_radius {
                iscoplanar = true;
            }
            if self.facets[n].center.is_none() {
                self.facets[n].center = Some(self.getcentrum(neighbor)?);
            }
            let center = self.facets[n].center.expect("the centrum just set");
            let dist2 = self.distplane(&center, facet);
            if dist2 > self.centrum_radius {
                isconcave = true;
            } else if !iscoplanar && dist2 > -self.centrum_radius {
                iscoplanar = true;
            }
        }
        if !isconcave && !iscoplanar {
            return Ok(false);
        }
        if !okangle {
            // ANGLEmerge.
            let a = self.facets[f].normal.expect("a facet's hyperplane");
            let b = self.facets[n].normal.expect("a facet's hyperplane");
            angle = getangle(&a, &b);
        }
        if isconcave {
            angle += ANGLE_CONCAVE + 0.5;
            self.appendmergeset(facet, neighbor, MRG_CONCAVE, Some(angle))?;
        } else {
            self.appendmergeset(facet, neighbor, MRG_COPLANAR, Some(angle))?;
        }
        Ok(true)
    }

    /// `qh_all_merges(othermerge, False)`: merges the queued non-convex ridges, the largest angle
    /// first, queueing the ridges the merges change, then renames or drops the vertices the
    /// merges left, until nothing changes.
    fn all_merges(&mut self, mut othermerge: bool) -> QResult<()> {
        loop {
            let mut wasmerge = false;
            while set::size(&self.facet_mergeset) != 0 {
                loop {
                    let merge = set::del_last(&mut self.facet_mergeset);
                    if merge == NIL {
                        break;
                    }
                    let m = self.merges[merge as usize];
                    let (facet1, facet2, mergetype) = (m.facet1, m.facet2, m.mtype);
                    let (f1, f2) = (&self.facets[facet1 as usize], &self.facets[facet2 as usize]);
                    if f1.visible || f2.visible {
                        continue;
                    }
                    if ((f1.newfacet && !f1.tested) || (f2.newfacet && !f2.tested))
                        && mergetype <= MRG_ANGLECOPLANAR
                    {
                        // MERGEindependent.
                        continue;
                    }
                    self.merge_nonconvex(facet1, facet2)?;
                    self.merge_degenredundant()?;
                    wasmerge = true;
                }
                // Not POSTmerging.
                self.getmergeset(self.newfacet_list)?;
            }
            if self.vertex_neighbors && (wasmerge || othermerge) {
                // hull_dim <= qh_DIMreduceBuild, not MERGEexact.
                othermerge = false;
                if self.reducevertices()? {
                    self.getmergeset(self.newfacet_list)?;
                    continue;
                }
            }
            // Not vneighbors.
            break;
        }
        Ok(())
    }

    /// `qh_merge_nonconvex(facet1, facet2, mergetype)`: merges the new facet of the pair (or the
    /// other, if that one is nearer its best neighbour) into its best neighbour (not
    /// `AVOIDold`).
    fn merge_nonconvex(&mut self, facet1: u32, facet2: u32) -> QResult<()> {
        let (bestfacet, facet2) = if self.facets[facet1 as usize].newfacet {
            (facet1, facet2)
        } else {
            (facet2, facet1)
        };
        let (bestneighbor, dist, mindist, maxdist) = self.findbestneighbor(bestfacet)?;
        let (neighbor, dist2, mindist2, maxdist2) = self.findbestneighbor(facet2)?;
        if dist < dist2 {
            self.mergefacet(bestfacet, bestneighbor, Some((mindist, maxdist)), false)
        } else {
            self.mergefacet(facet2, neighbor, Some((mindist2, maxdist2)), false)
        }
    }

    /// `qh_findbestneighbor(facet, &dist, &mindist, &maxdist)`: the neighbour the facet is
    /// nearest (by its vertices, or by its centrum for a facet of many vertices), the
    /// non-convex ridges first for a facet of many vertices; with the distance and the lowest
    /// and highest vertex.
    ///
    /// # Errors
    ///
    /// Qhull's internal error 6095 for a facet with no neighbour, and those of
    /// [`Qh::getcentrum`].
    fn findbestneighbor(&mut self, facet: u32) -> QResult<(u32, f64, f64, f64)> {
        let f = facet as usize;
        let mut bestfacet = NIL;
        let mut testcentrum = false;
        let size = set::size(&self.facets[f].vertices);
        let mut dist = REAL_MAX;
        let mut mindist = 0.0;
        let mut maxdist = 0.0;
        if size > BEST_CENTRUM2 * DIM + BEST_CENTRUM {
            testcentrum = true;
            if self.facets[f].center.is_none() {
                self.facets[f].center = Some(self.getcentrum(facet)?);
            }
        }
        if size > DIM + BEST_NONCONVEX {
            let mut i = 0;
            loop {
                let ridge = set::elem(&self.facets[f].ridges, i);
                i += 1;
                if ridge == NIL {
                    break;
                }
                if self.ridges[ridge as usize].nonconvex {
                    let neighbor = self.otherfacet(ridge, facet);
                    self.findbest_test(
                        testcentrum,
                        facet,
                        neighbor,
                        &mut bestfacet,
                        &mut dist,
                        &mut mindist,
                        &mut maxdist,
                    );
                }
            }
        }
        if bestfacet == NIL {
            let mut i = 0;
            loop {
                let neighbor = set::elem(&self.facets[f].neighbors, i);
                i += 1;
                if neighbor == NIL {
                    break;
                }
                self.findbest_test(
                    testcentrum,
                    facet,
                    neighbor,
                    &mut bestfacet,
                    &mut dist,
                    &mut mindist,
                    &mut maxdist,
                );
            }
        }
        if bestfacet == NIL {
            return Err(Error::Qhull(6095));
        }
        if testcentrum {
            let (_, low, high) = self.getdistance(facet, bestfacet);
            mindist = low;
            maxdist = high;
        }
        Ok((bestfacet, dist, mindist, maxdist))
    }

    /// `qh_findbest_test(testcentrum, facet, neighbor, &bestfacet, &dist, &mindist, &maxdist)`:
    /// keeps the neighbour if the facet is nearer it.
    #[allow(clippy::too_many_arguments)]
    fn findbest_test(
        &mut self,
        testcentrum: bool,
        facet: u32,
        neighbor: u32,
        bestfacet: &mut u32,
        distp: &mut f64,
        mindistp: &mut f64,
        maxdistp: &mut f64,
    ) {
        let (dist, mindist, maxdist);
        if testcentrum {
            let center = self.facets[facet as usize]
                .center
                .expect("the facet's centrum");
            let mut d = self.distplane(&center, neighbor);
            #[allow(clippy::cast_precision_loss)]
            {
                d *= DIM as f64;
            }
            if d < 0.0 {
                maxdist = 0.0;
                mindist = d;
                dist = -d;
            } else {
                mindist = 0.0;
                maxdist = d;
                dist = d;
            }
        } else {
            (dist, mindist, maxdist) = self.getdistance(facet, neighbor);
        }
        if dist < *distp {
            *bestfacet = neighbor;
            *mindistp = mindist;
            *maxdistp = maxdist;
            *distp = dist;
        }
    }

    /// `qh_mergefacet(facet1, facet2, mindist, maxdist, mergeapex)`: merges `facet1` into
    /// `facet2`, which moves to the new facets; `dists` are the lowest and highest vertex of
    /// `facet1` above `facet2`, when known. With `mergeapex`, `facet1` is a new facet whose
    /// apex is its only vertex not in `facet2`.
    ///
    /// # Errors
    ///
    /// Qhull's precision error 6227 when no more than `hull_dim + 1` facets would remain, its
    /// internal errors 6099 and 6226, and those of the steps.
    pub(crate) fn mergefacet(
        &mut self,
        facet1: u32,
        facet2: u32,
        dists: Option<(f64, f64)>,
        mergeapex: bool,
    ) -> QResult<()> {
        let (f1, f2) = (facet1 as usize, facet2 as usize);
        if self.facets[f1].tricoplanar || self.facets[f2].tricoplanar {
            // TRInormals is False.
            return Err(Error::Qhull(6226));
        }
        self.ztotmerge += 1;
        // No REPORTfreq2; the trace of a wide merge needs a distance beyond TRACEdist (REALmax)
        // and there is no trace facet.
        if facet1 == facet2 || self.facets[f1].visible || self.facets[f2].visible {
            return Err(Error::Qhull(6099));
        }
        if self.num_facets - self.num_visible <= 3 + 1 {
            return Err(Error::Qhull(6227));
        }
        if !self.vertex_neighbors {
            self.vertexneighbors();
        }
        self.makeridges(facet1);
        self.makeridges(facet2);
        if let Some((mindist, maxdist)) = dists {
            maximize_(&mut self.max_outside, maxdist);
            maximize_(&mut self.max_vertex, maxdist);
            maximize_(&mut self.facets[f2].maxoutside, maxdist);
            minimize_(&mut self.min_vertex, mindist);
            if !self.facets[f2].keepcentrum
                && (maxdist > self.wide_facet || mindist < -self.wide_facet)
            {
                self.facets[f2].keepcentrum = true;
            }
        }
        let nummerge = self.facets[f1].nummerge + self.facets[f2].nummerge + 1;
        self.facets[f2].nummerge = if nummerge >= MAX_NUMMERGE {
            MAX_NUMMERGE
        } else {
            nummerge
        };
        self.facets[f2].newmerge = true;
        self.facets[f2].dupridge = false;
        self.updatetested(facet1, facet2);
        if set::size(&self.facets[f1].vertices) == DIM {
            self.mergesimplex(facet1, facet2, mergeapex)?;
        } else {
            self.vertex_visit = self.vertex_visit.wrapping_add(1);
            let mut i = 0;
            loop {
                let vertex = set::elem(&self.facets[f2].vertices, i);
                i += 1;
                if vertex == NIL {
                    break;
                }
                self.vertices[vertex as usize].visitid = self.vertex_visit;
            }
            self.mergeneighbors(facet1, facet2)?;
            self.mergevertices(facet1, facet2)?;
            self.mergeridges(facet1, facet2);
            self.mergevertex_neighbors(facet1, facet2)?;
            if !self.facets[f2].newfacet {
                self.newvertices(facet2);
            }
        }
        if !mergeapex {
            self.degen_redundant_neighbors(facet2, facet1)?;
        }
        self.willdelete(facet1, facet2);
        self.removefacet(facet2);
        self.appendfacet(facet2);
        self.facets[f2].newfacet = true;
        self.facets[f2].tested = false;
        // qh_tracemerge: no trace facet or vertex, not CHECKfrequently.
        Ok(())
    }

    /// `qh_updatetested(facet1, facet2)`: the ridges of `facet1` and `facet2` are to be tested
    /// again; `facet2`'s centrum is kept or dropped by its size.
    fn updatetested(&mut self, facet1: u32, facet2: u32) {
        let f2 = facet2 as usize;
        self.facets[f2].tested = false;
        let mut i = 0;
        loop {
            let ridge = set::elem(&self.facets[facet1 as usize].ridges, i);
            i += 1;
            if ridge == NIL {
                break;
            }
            self.ridges[ridge as usize].tested = false;
        }
        if self.facets[f2].center.is_none() {
            return;
        }
        let size = set::size(&self.facets[f2].vertices);
        if !self.facets[f2].keepcentrum {
            if size > DIM + MAX_NEWCENTRUM {
                self.facets[f2].keepcentrum = true;
            }
        } else if size <= DIM + MAX_NEWCENTRUM && size == DIM {
            // Not POSTmerging.
            self.facets[f2].keepcentrum = false;
        }
        if !self.facets[f2].keepcentrum {
            self.facets[f2].center = None;
            let mut i = 0;
            loop {
                let ridge = set::elem(&self.facets[f2].ridges, i);
                i += 1;
                if ridge == NIL {
                    break;
                }
                self.ridges[ridge as usize].tested = false;
            }
        }
    }

    /// `qh_mergesimplex(facet1, facet2, mergeapex)`: merges the simplicial `facet1` into
    /// `facet2`: its one vertex not in `facet2` (the apex) joins `facet2`'s vertices, and its
    /// ridges and neighbours become `facet2`'s.
    #[allow(clippy::too_many_lines)]
    fn mergesimplex(&mut self, facet1: u32, facet2: u32, mergeapex: bool) -> QResult<()> {
        let (f1, f2) = (facet1 as usize, facet2 as usize);
        let mut issubset = false;
        let apex;
        if mergeapex {
            if !self.facets[f2].newfacet {
                self.newvertices(facet2);
            }
            apex = set::first(&self.facets[f1].vertices);
            if set::first(&self.facets[f2].vertices) == apex {
                issubset = true;
            } else {
                set::add_nth(&mut self.facets[f2].vertices, 0, apex);
            }
        } else {
            let mut i = 0;
            loop {
                let vertex = set::elem(&self.facets[f1].vertices, i);
                i += 1;
                if vertex == NIL {
                    break;
                }
                self.vertices[vertex as usize].seen = false;
            }
            let mut i = 0;
            loop {
                let ridge = set::elem(&self.facets[f1].ridges, i);
                i += 1;
                if ridge == NIL {
                    break;
                }
                if self.otherfacet(ridge, facet1) == facet2 {
                    let mut j = 0;
                    loop {
                        let vertex = set::elem(&self.ridges[ridge as usize].vertices, j);
                        j += 1;
                        if vertex == NIL {
                            break;
                        }
                        self.vertices[vertex as usize].seen = true;
                        self.vertices[vertex as usize].delridge = true;
                    }
                    break;
                }
            }
            let mut i = 0;
            let mut vertex;
            loop {
                vertex = set::elem(&self.facets[f1].vertices, i);
                i += 1;
                if vertex == NIL || !self.vertices[vertex as usize].seen {
                    break;
                }
            }
            apex = vertex;
            let apex_id = self.vertices[apex as usize].id;
            let vertex_n = set::size(&self.facets[f2].vertices);
            let mut vertex_i = 0;
            while vertex_i < vertex_n {
                let vertex = set::elem(&self.facets[f2].vertices, vertex_i);
                let id = self.vertices[vertex as usize].id;
                if id < apex_id {
                    break;
                } else if id == apex_id {
                    issubset = true;
                    break;
                }
                vertex_i += 1;
            }
            if !issubset {
                set::add_nth(&mut self.facets[f2].vertices, vertex_i, apex);
            }
            if !self.facets[f2].newfacet {
                self.newvertices(facet2);
            } else if !self.vertices[apex as usize].newlist {
                self.removevertex(apex);
                self.appendvertex(apex);
            }
        }
        let mut i = 0;
        loop {
            let vertex = set::elem(&self.facets[f1].vertices, i);
            i += 1;
            if vertex == NIL {
                break;
            }
            let v = vertex as usize;
            if vertex == apex && !issubset {
                replace(&mut self.vertices[v].neighbors, facet1, facet2)?;
            } else {
                set::del(&mut self.vertices[v].neighbors, facet1);
                if set::second(&self.vertices[v].neighbors) == NIL {
                    self.mergevertex_del(vertex, facet2);
                }
            }
        }
        self.visit_id = self.visit_id.wrapping_add(1);
        let visit_id = self.visit_id;
        let mut i = 0;
        loop {
            let neighbor = set::elem(&self.facets[f2].neighbors, i);
            i += 1;
            if neighbor == NIL {
                break;
            }
            self.facets[neighbor as usize].visitid = visit_id;
        }
        let mut i = 0;
        loop {
            let ridge = set::elem(&self.facets[f1].ridges, i);
            i += 1;
            if ridge == NIL {
                break;
            }
            let otherfacet = self.otherfacet(ridge, facet1);
            let o = otherfacet as usize;
            if otherfacet == facet2 {
                set::del(&mut self.facets[f2].ridges, ridge);
                self.freeridge(ridge);
                set::del(&mut self.facets[f2].neighbors, facet1);
            } else {
                set::append(&mut self.facets[f2].ridges, ridge);
                if self.facets[o].visitid == visit_id {
                    if self.facets[o].simplicial {
                        self.makeridges(otherfacet);
                    }
                    if set::first(&self.facets[o].neighbors) == facet1 {
                        set::del(&mut self.facets[o].neighbors, facet2);
                        replace(&mut self.facets[o].neighbors, facet1, facet2)?;
                    } else {
                        set::del(&mut self.facets[o].neighbors, facet1);
                    }
                } else {
                    set::append(&mut self.facets[f2].neighbors, otherfacet);
                    replace(&mut self.facets[o].neighbors, facet1, facet2)?;
                    self.facets[o].visitid = visit_id;
                }
                let r = &mut self.ridges[ridge as usize];
                if r.top == facet1 {
                    r.top = facet2;
                } else {
                    r.bottom = facet2;
                }
            }
        }
        if let Some(r) = self.facets[f1].ridges.as_mut()
            && !r.is_empty()
        {
            r[0] = NIL;
        }
        Ok(())
    }

    /// `qh_mergeneighbors(facet1, facet2)`: `facet1`'s neighbours become `facet2`'s.
    fn mergeneighbors(&mut self, facet1: u32, facet2: u32) -> QResult<()> {
        let (f1, f2) = (facet1 as usize, facet2 as usize);
        self.visit_id = self.visit_id.wrapping_add(1);
        let visit_id = self.visit_id;
        let mut i = 0;
        loop {
            let neighbor = set::elem(&self.facets[f2].neighbors, i);
            i += 1;
            if neighbor == NIL {
                break;
            }
            self.facets[neighbor as usize].visitid = visit_id;
        }
        let mut i = 0;
        loop {
            let neighbor = set::elem(&self.facets[f1].neighbors, i);
            i += 1;
            if neighbor == NIL {
                break;
            }
            let n = neighbor as usize;
            if self.facets[n].visitid == visit_id {
                if self.facets[n].simplicial {
                    self.makeridges(neighbor);
                }
                if set::first(&self.facets[n].neighbors) == facet1 {
                    set::del(&mut self.facets[n].neighbors, facet2);
                    replace(&mut self.facets[n].neighbors, facet1, facet2)?;
                } else {
                    set::del(&mut self.facets[n].neighbors, facet1);
                }
            } else if neighbor != facet2 {
                set::append(&mut self.facets[f2].neighbors, neighbor);
                replace(&mut self.facets[n].neighbors, facet1, facet2)?;
            }
        }
        set::del(&mut self.facets[f1].neighbors, facet2);
        set::del(&mut self.facets[f2].neighbors, facet1);
        Ok(())
    }

    /// `qh_mergevertices(facet1->vertices, &facet2->vertices)`: the union of the two vertex sets,
    /// inverse sorted by id, becomes `facet2`'s.
    ///
    /// # Errors
    ///
    /// Qhull's internal error 6100 when the facets do not share a ridge.
    fn mergevertices(&mut self, facet1: u32, facet2: u32) -> QResult<()> {
        let vertices1 = self.facets[facet1 as usize].vertices.clone();
        let vertices2 = self.facets[facet2 as usize].vertices.clone();
        let newsize = set::size(&vertices1) + set::size(&vertices2) + 1 - DIM;
        let mut merged: Vec<u32> = Vec::with_capacity(newsize);
        let id = |vertex: u32| self.vertices[vertex as usize].id;
        let mut k = 0;
        let mut i = 0;
        loop {
            let vertex = set::elem(&vertices1, i);
            i += 1;
            if vertex == NIL {
                break;
            }
            let vertex2 = set::elem(&vertices2, k);
            if vertex2 == NIL || id(vertex) > id(vertex2) {
                merged.push(vertex);
            } else {
                while set::elem(&vertices2, k) != NIL && id(set::elem(&vertices2, k)) > id(vertex) {
                    merged.push(set::elem(&vertices2, k));
                    k += 1;
                }
                let vertex2 = set::elem(&vertices2, k);
                if vertex2 == NIL || id(vertex2) < id(vertex) {
                    merged.push(vertex);
                } else {
                    merged.push(vertex2);
                    k += 1;
                }
            }
        }
        while set::elem(&vertices2, k) != NIL {
            merged.push(set::elem(&vertices2, k));
            k += 1;
        }
        if newsize < merged.len() {
            return Err(Error::Qhull(6100));
        }
        self.facets[facet2 as usize].vertices = Some(merged);
        Ok(())
    }

    /// `qh_mergeridges(facet1, facet2)`: deletes the ridges between the facets (their vertices
    /// marked `delridge`) and moves `facet1`'s other ridges to `facet2`.
    fn mergeridges(&mut self, facet1: u32, facet2: u32) {
        let (f1, f2) = (facet1 as usize, facet2 as usize);
        let mut i = 0;
        loop {
            let ridge = set::elem(&self.facets[f2].ridges, i);
            i += 1;
            if ridge == NIL {
                break;
            }
            let r = ridge as usize;
            if self.ridges[r].top == facet1 || self.ridges[r].bottom == facet1 {
                let mut j = 0;
                loop {
                    let vertex = set::elem(&self.ridges[r].vertices, j);
                    j += 1;
                    if vertex == NIL {
                        break;
                    }
                    self.vertices[vertex as usize].delridge = true;
                }
                self.delridge(ridge);
                i -= 1;
            }
        }
        let mut i = 0;
        loop {
            let ridge = set::elem(&self.facets[f1].ridges, i);
            i += 1;
            if ridge == NIL {
                break;
            }
            let r = &mut self.ridges[ridge as usize];
            if r.top == facet1 {
                r.top = facet2;
            } else {
                r.bottom = facet2;
            }
            set::append(&mut self.facets[f2].ridges, ridge);
        }
    }

    /// `qh_mergevertex_neighbors(facet1, facet2)`: `facet1`'s vertices take `facet2` as their
    /// neighbour instead (those already `facet2`'s just drop `facet1`, and are deleted when
    /// left with one neighbour).
    fn mergevertex_neighbors(&mut self, facet1: u32, facet2: u32) -> QResult<()> {
        let mut i = 0;
        loop {
            let vertex = set::elem(&self.facets[facet1 as usize].vertices, i);
            i += 1;
            if vertex == NIL {
                break;
            }
            let v = vertex as usize;
            if self.vertices[v].visitid == self.vertex_visit {
                set::del(&mut self.vertices[v].neighbors, facet1);
                if set::second(&self.vertices[v].neighbors) == NIL {
                    self.mergevertex_del(vertex, facet2);
                }
            } else {
                replace(&mut self.vertices[v].neighbors, facet1, facet2)?;
            }
        }
        Ok(())
    }

    /// `qh_newvertices(facet->vertices)`: moves the facet's vertices to the new vertex list.
    fn newvertices(&mut self, facet: u32) {
        let mut i = 0;
        loop {
            let vertex = set::elem(&self.facets[facet as usize].vertices, i);
            i += 1;
            if vertex == NIL {
                break;
            }
            if !self.vertices[vertex as usize].newlist {
                self.removevertex(vertex);
                self.appendvertex(vertex);
            }
        }
    }

    /// `qh_mergevertex_del(vertex, facet1, facet2)`: deletes a vertex left with one neighbour.
    fn mergevertex_del(&mut self, vertex: u32, facet2: u32) {
        set::del_sorted(&mut self.facets[facet2 as usize].vertices, vertex);
        self.vertices[vertex as usize].deleted = true;
        set::append(&mut self.del_vertices, vertex);
    }

    /// `qh_reducevertices`: after the merges, drops the vertices of merged facets that are in no
    /// ridge and renames the vertices that lost a ridge and are shared by just two facets
    /// (`MERGEvertices`; a 3-d hull renames no redundant vertex); returns whether a degenerate or
    /// redundant facet was merged.
    fn reducevertices(&mut self) -> QResult<bool> {
        let mut degenredun = false;
        if self.merge_degenredundant()? != 0 {
            degenredun = true;
        }
        let mut newfacet = self.newfacet_list;
        while newfacet != NIL && self.facets[newfacet as usize].next != NIL {
            if self.facets[newfacet as usize].newmerge {
                self.remove_extravertices(newfacet);
            }
            newfacet = self.facets[newfacet as usize].next;
        }
        let mut newfacet = self.newfacet_list;
        while newfacet != NIL && self.facets[newfacet as usize].next != NIL {
            let nf = newfacet as usize;
            if self.facets[nf].newmerge {
                self.facets[nf].newmerge = false;
                let mut i = 0;
                loop {
                    let vertex = set::elem(&self.facets[nf].vertices, i);
                    i += 1;
                    if vertex == NIL {
                        break;
                    }
                    if self.vertices[vertex as usize].delridge
                        && self.rename_sharedvertex(vertex, newfacet)? != NIL
                    {
                        i -= 1;
                    }
                }
            }
            newfacet = self.facets[nf].next;
        }
        let mut vertex = self.newvertex_list;
        while vertex != NIL && self.vertices[vertex as usize].next != NIL {
            let v = &mut self.vertices[vertex as usize];
            if v.delridge && !v.deleted {
                v.delridge = false;
                // qh_redundant_vertex needs hull_dim >= 4.
            }
            vertex = v.next;
        }
        Ok(degenredun)
    }

    /// `qh_remove_extravertices(facet)`: drops the facet's vertices that are in none of its
    /// ridges; returns whether there were any.
    fn remove_extravertices(&mut self, facet: u32) -> bool {
        let f = facet as usize;
        let mut foundrem = false;
        let mut i = 0;
        loop {
            let vertex = set::elem(&self.facets[f].vertices, i);
            i += 1;
            if vertex == NIL {
                break;
            }
            self.vertices[vertex as usize].seen = false;
        }
        let mut i = 0;
        loop {
            let ridge = set::elem(&self.facets[f].ridges, i);
            i += 1;
            if ridge == NIL {
                break;
            }
            let mut j = 0;
            loop {
                let vertex = set::elem(&self.ridges[ridge as usize].vertices, j);
                j += 1;
                if vertex == NIL {
                    break;
                }
                self.vertices[vertex as usize].seen = true;
            }
        }
        let mut i = 0;
        loop {
            let vertex = set::elem(&self.facets[f].vertices, i);
            i += 1;
            if vertex == NIL {
                break;
            }
            let v = vertex as usize;
            if !self.vertices[v].seen {
                foundrem = true;
                set::del_sorted(&mut self.facets[f].vertices, vertex);
                set::del(&mut self.vertices[v].neighbors, facet);
                if set::size(&self.vertices[v].neighbors) == 0 {
                    self.vertices[v].deleted = true;
                    set::append(&mut self.del_vertices, vertex);
                }
                // The C's `vertexp--` follows its `else trace3(...)`, so it always runs.
                i -= 1;
            }
        }
        foundrem
    }

    /// `qh_rename_sharedvertex(vertex, facet)`: renames a vertex shared by just the facet and one
    /// neighbour to another of their shared vertices, if no ridge would then repeat; returns the
    /// new vertex or NULL (a 3-d hull renames no vertex of more facets).
    fn rename_sharedvertex(&mut self, vertex: u32, facet: u32) -> QResult<u32> {
        let v = vertex as usize;
        if set::size(&self.vertices[v].neighbors) != 2 {
            return Ok(NIL);
        }
        let mut neighbor_a = set::first(&self.vertices[v].neighbors);
        if neighbor_a == facet {
            neighbor_a = set::second(&self.vertices[v].neighbors);
        }
        let mut ridges: Set = Some(Vec::new());
        self.visit_id = self.visit_id.wrapping_add(1);
        self.facets[neighbor_a as usize].visitid = self.visit_id;
        self.vertexridges_facet(vertex, facet, &mut ridges);
        let mut vertices = self.vertexintersect_new(
            &self.facets[facet as usize].vertices,
            &self.facets[neighbor_a as usize].vertices,
        );
        set::del(&mut vertices, vertex);
        let newvertex = self.find_newvertex(vertex, &mut vertices, &ridges)?;
        if newvertex != NIL {
            self.renamevertex(vertex, newvertex, &ridges, facet, neighbor_a)?;
        }
        Ok(newvertex)
    }

    /// `qh_vertexridges_facet(vertex, facet, &ridges)`: adds the facet's ridges with the vertex
    /// to a neighbour marked with the current `visit_id`, and marks the facet as done.
    fn vertexridges_facet(&mut self, vertex: u32, facet: u32, ridges: &mut Set) {
        let f = facet as usize;
        let mut i = 0;
        loop {
            let ridge = set::elem(&self.facets[f].ridges, i);
            i += 1;
            if ridge == NIL {
                break;
            }
            let neighbor = self.otherfacet(ridge, facet);
            if self.facets[neighbor as usize].visitid == self.visit_id
                && set::contains(&self.ridges[ridge as usize].vertices, vertex)
            {
                set::append(ridges, ridge);
            }
        }
        self.facets[f].visitid = self.visit_id.wrapping_sub(1);
    }

    /// `qh_vertexridges(vertex)`: the ridges of the vertex (each neighbour's but the last, whose
    /// ridges the others hold).
    fn vertexridges(&mut self, vertex: u32) -> Set {
        let mut ridges: Set = Some(Vec::new());
        let v = vertex as usize;
        self.visit_id = self.visit_id.wrapping_add(1);
        let mut i = 0;
        loop {
            let neighbor = set::elem(&self.vertices[v].neighbors, i);
            i += 1;
            if neighbor == NIL {
                break;
            }
            self.facets[neighbor as usize].visitid = self.visit_id;
        }
        let mut i = 0;
        loop {
            let neighbor = set::elem(&self.vertices[v].neighbors, i);
            i += 1;
            if neighbor == NIL {
                break;
            }
            if set::elem(&self.vertices[v].neighbors, i) != NIL {
                self.vertexridges_facet(vertex, neighbor, &mut ridges);
            }
        }
        ridges
    }

    /// `qh_find_newvertex(oldvertex, vertices, ridges)`: of the vertices in the old vertex's
    /// ridges, the first by number of ridges (`qsort` with `qh_comparevisit`) whose ridges would
    /// not repeat one of the old vertex's after renaming; NULL if none.
    fn find_newvertex(&mut self, oldvertex: u32, vertices: &mut Set, ridges: &Set) -> QResult<u32> {
        let mut i = 0;
        loop {
            let vertex = set::elem(vertices, i);
            i += 1;
            if vertex == NIL {
                break;
            }
            self.vertices[vertex as usize].visitid = 0;
        }
        let mut i = 0;
        loop {
            let ridge = set::elem(ridges, i);
            i += 1;
            if ridge == NIL {
                break;
            }
            let mut j = 0;
            loop {
                let vertex = set::elem(&self.ridges[ridge as usize].vertices, j);
                j += 1;
                if vertex == NIL {
                    break;
                }
                let v = &mut self.vertices[vertex as usize];
                v.visitid = v.visitid.wrapping_add(1);
            }
        }
        let mut i = 0;
        loop {
            let vertex = set::elem(vertices, i);
            i += 1;
            if vertex == NIL {
                break;
            }
            if self.vertices[vertex as usize].visitid == 0 {
                set::del_nth(vertices, i - 1);
                i -= 1;
            }
        }
        let numridges = set::size(ridges);
        self.vertex_visit = self
            .vertex_visit
            .wrapping_add(u32::try_from(numridges).expect("a set fits an int"));
        if set::size(vertices) == 0 {
            return Ok(NIL);
        }
        {
            let verts = &self.vertices;
            let v = vertices.as_mut().expect("a non-empty set");
            #[allow(clippy::cast_possible_wrap)]
            orca_crt::qsort(v.as_mut_slice(), |a, b| {
                verts[*a as usize]
                    .visitid
                    .wrapping_sub(verts[*b as usize].visitid) as i32
            });
        }
        let hashsize = self.newhashtable(i32::try_from(numridges).expect("a set fits an int"))?;
        let mut i = 0;
        loop {
            let ridge = set::elem(ridges, i);
            i += 1;
            if ridge == NIL {
                break;
            }
            self.hashridge(hashsize, ridge, oldvertex)?;
        }
        let mut found = NIL;
        let mut i = 0;
        loop {
            let vertex = set::elem(vertices, i);
            i += 1;
            if vertex == NIL {
                break;
            }
            let newridges = self.vertexridges(vertex);
            let mut duplicate = false;
            let mut j = 0;
            loop {
                let ridge = set::elem(&newridges, j);
                j += 1;
                if ridge == NIL {
                    break;
                }
                if self.hashridge_find(hashsize, ridge, vertex, oldvertex)? != NIL {
                    duplicate = true;
                    break;
                }
            }
            if !duplicate {
                found = vertex;
                break;
            }
        }
        self.hash_table = None;
        Ok(found)
    }

    /// `qh_hashridge(hashtable, hashsize, ridge, oldvertex)`: adds the ridge to the hash table
    /// by its vertices but `oldvertex`.
    fn hashridge(&mut self, hashsize: usize, ridge: u32, oldvertex: u32) -> QResult<()> {
        let mut hash = self.gethash(
            hashsize,
            &self.ridges[ridge as usize].vertices,
            DIM - 1,
            0,
            oldvertex,
        )?;
        loop {
            let ridge_a = set::elem(&self.hash_table, hash);
            if ridge_a == NIL {
                set::set_elem(&mut self.hash_table, hash, ridge);
                break;
            } else if ridge_a == ridge {
                break;
            }
            hash += 1;
            if hash == hashsize {
                hash = 0;
            }
        }
        Ok(())
    }

    /// `qh_hashridge_find(hashtable, hashsize, ridge, vertex, oldvertex, &hashslot)`: a ridge of
    /// the table that equals `ridge` once `vertex` is renamed to `oldvertex`, or NULL.
    fn hashridge_find(
        &self,
        hashsize: usize,
        ridge: u32,
        vertex: u32,
        oldvertex: u32,
    ) -> QResult<u32> {
        let vertices = &self.ridges[ridge as usize].vertices;
        let mut hash = self.gethash(hashsize, vertices, DIM - 1, 0, vertex)?;
        loop {
            let ridge_a = set::elem(&self.hash_table, hash);
            if ridge_a == NIL {
                break;
            }
            if ridge_a != ridge
                && set::equal_except(
                    vertices,
                    vertex,
                    &self.ridges[ridge_a as usize].vertices,
                    oldvertex,
                )
            {
                return Ok(ridge_a);
            }
            hash += 1;
            if hash == hashsize {
                hash = 0;
            }
        }
        Ok(NIL)
    }

    /// `qh_renamevertex(oldvertex, newvertex, ridges, oldfacet, neighborA)`: renames the vertex
    /// in its ridges and deletes it from its two facets.
    ///
    /// # Errors
    ///
    /// The errors of [`Qh::renameridgevertex`], and [`Error::Unported`] without a facet (a
    /// redundant vertex, which only a hull of 4 or more dimensions renames).
    fn renamevertex(
        &mut self,
        oldvertex: u32,
        newvertex: u32,
        ridges: &Set,
        oldfacet: u32,
        neighbor_a: u32,
    ) -> QResult<()> {
        // No trace vertex (tracevertex_id is UINT_MAX) and no tracing: messages 8082-8084 are
        // not printed.
        let mut i = 0;
        loop {
            let ridge = set::elem(ridges, i);
            i += 1;
            if ridge == NIL {
                break;
            }
            self.renameridgevertex(ridge, oldvertex, newvertex)?;
        }
        let ov = oldvertex as usize;
        if oldfacet == NIL {
            return Err(Error::Unported("qh_renamevertex of a redundant vertex"));
        } else if set::size(&self.vertices[ov].neighbors) == 2 {
            let mut i = 0;
            loop {
                let neighbor = set::elem(&self.vertices[ov].neighbors, i);
                i += 1;
                if neighbor == NIL {
                    break;
                }
                set::del_sorted(&mut self.facets[neighbor as usize].vertices, oldvertex);
            }
            self.vertices[ov].deleted = true;
            set::append(&mut self.del_vertices, oldvertex);
        } else {
            set::del_sorted(&mut self.facets[oldfacet as usize].vertices, oldvertex);
            set::del(&mut self.vertices[ov].neighbors, oldfacet);
            self.remove_extravertices(neighbor_a);
        }
        Ok(())
    }

    /// `qh_renameridgevertex(ridge, oldvertex, newvertex)`: puts `newvertex` in the ridge in
    /// `oldvertex`'s place, in id order, turning the ridge if the position's parity changes;
    /// deletes the ridge if it has `newvertex` already.
    ///
    /// # Errors
    ///
    /// Qhull's internal error 6175 when the ridge does not have `oldvertex`.
    fn renameridgevertex(&mut self, ridge: u32, oldvertex: u32, newvertex: u32) -> QResult<()> {
        let r = ridge as usize;
        let oldnth = set::index(&self.ridges[r].vertices, oldvertex);
        let Ok(oldnth_at) = usize::try_from(oldnth) else {
            return Err(Error::Qhull(6175));
        };
        set::del_nth_sorted(&mut self.ridges[r].vertices, oldnth_at);
        let newid = self.vertices[newvertex as usize].id;
        let mut nth: usize = 0;
        let mut i = 0;
        loop {
            let vertex = set::elem(&self.ridges[r].vertices, i);
            i += 1;
            if vertex == NIL {
                break;
            }
            if vertex == newvertex {
                if self.ridges[r].nonconvex {
                    self.copynonconvex(ridge);
                }
                self.delridge(ridge);
                return Ok(());
            }
            if self.vertices[vertex as usize].id < newid {
                break;
            }
            nth += 1;
        }
        set::add_nth(&mut self.ridges[r].vertices, nth, newvertex);
        let nth = i32::try_from(nth).expect("a set fits an int");
        if (oldnth - nth).abs() % 2 != 0 {
            let rr = &mut self.ridges[r];
            core::mem::swap(&mut rr.top, &mut rr.bottom);
        }
        Ok(())
    }

    /// `qh_copynonconvex(atridge)`: marks the other ridge between the same two facets non-convex.
    fn copynonconvex(&mut self, atridge: u32) {
        let facet = self.ridges[atridge as usize].top;
        let otherfacet = self.ridges[atridge as usize].bottom;
        let mut i = 0;
        loop {
            let ridge = set::elem(&self.facets[facet as usize].ridges, i);
            i += 1;
            if ridge == NIL {
                break;
            }
            if otherfacet == self.otherfacet(ridge, facet) && ridge != atridge {
                self.ridges[ridge as usize].nonconvex = true;
                break;
            }
        }
    }
}
