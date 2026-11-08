// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Qhull's build (`libqhull_r.c`, `poly2_r.c`'s `qh_initbuild`): the initial simplex and its
//! outside sets, then one point at a time, the furthest first, its visible facets, their horizon
//! and the cone of new facets, merged (`qh_premerge`) before the visible facets' points move to the
//! new facets.
//!
//! Carries Qhull code: the functions of Qhull 2015.2's `libqhull_r.c` and `qh_initbuild` of
//! `poly2_r.c` that a 3-d hull with the options of `its_convex_hull` reaches, translated into Rust
//! on 2026-11-07, Copyright (c) 1993-2015 C.B. Barber and The Geometry Center, University of
//! Minnesota, under Qhull's licence (`licenses/Qhull.txt`); the copyright holders are named in
//! `NOTICE`.

use crate::geom::getangle;
use crate::qh::{DIM, Error, ID_UNKNOWN, QResult, Qh, REAL_MAX, maximize_, minimize_};
use crate::set::{self, NIL};

impl Qh {
    /// `qh_initbuild`: the initial simplex from the extreme points, the outside sets of its
    /// facets and the furthest of them first (no good point or vertex, no `ONLYgood`).
    ///
    /// # Errors
    ///
    /// As [`Qh::detroundoff`], [`Qh::initialvertices`], [`Qh::initialhull`] and the partition.
    pub(crate) fn initbuild(&mut self) -> QResult<()> {
        self.furthest_id = ID_UNKNOWN;
        self.facet_id = 0;
        self.vertex_id = 0;
        // ridge_id = 0: ridges keep no id here.
        self.visit_id = 0;
        self.vertex_visit = 0;
        self.maxoutdone = false;
        let maxpoints = self.maxmin();
        self.detroundoff()?;
        let vertices = self.initialvertices(DIM, &maxpoints)?;
        self.initialhull(&vertices)?;
        self.partitionall(&vertices)?;
        self.resetlists(true);
        self.facet_next = self.facet_list;
        self.furthestnext();
        // PREmerge.
        self.cos_max = self.premerge_cos;
        self.centrum_radius = self.premerge_centrum;
        Ok(())
    }

    /// `qh_buildhull`: adds the furthest outside point until none is left (`ONLYmax` is False,
    /// so `qh_addpoint` does not test it again), then on a narrow hull moves the outside points
    /// left to the coplanar sets.
    ///
    /// # Errors
    ///
    /// Qhull's internal errors 6165 and 6166 (new or visible facets or vertices on the lists),
    /// 6167 (outside points left) and those of [`Qh::addpoint`].
    pub(crate) fn buildhull(&mut self) -> QResult<()> {
        let mut facet = self.facet_list;
        while facet != NIL && self.facets[facet as usize].next != NIL {
            let f = &self.facets[facet as usize];
            if f.visible || f.newfacet {
                return Err(Error::Qhull(6165));
            }
            facet = f.next;
        }
        let mut vertex = self.vertex_list;
        while vertex != NIL && self.vertices[vertex as usize].next != NIL {
            if self.vertices[vertex as usize].newlist {
                return Err(Error::Qhull(6166));
            }
            // No STOPpoint or STOPcone.
            vertex = self.vertices[vertex as usize].next;
        }
        self.facet_next = self.facet_list;
        let mut furthest;
        loop {
            let (point, facet) = self.nextfurthest();
            furthest = point;
            if furthest == NIL {
                break;
            }
            self.num_outside -= 1;
            self.addpoint(furthest, facet)?;
        }
        if self.narrow_hull {
            self.outcoplanar()?;
        }
        if self.num_outside != 0 && furthest == NIL {
            return Err(Error::Qhull(6167));
        }
        Ok(())
    }

    /// `qh_buildtracing(furthest, facet)` as it runs without tracing or reports: the visit
    /// counters start again from 0 past `INT_MAX`, and the point becomes `furthest_id`.
    fn buildtracing(&mut self, furthest: u32) {
        let int_max = i32::MAX.unsigned_abs();
        if self.visit_id > int_max {
            self.visit_id = 0;
            let mut facet = self.facet_list;
            while facet != NIL && self.facets[facet as usize].next != NIL {
                self.facets[facet as usize].visitid = 0;
                facet = self.facets[facet as usize].next;
            }
        }
        if self.vertex_visit > int_max {
            self.vertex_visit = 0;
            let mut vertex = self.vertex_list;
            while vertex != NIL && self.vertices[vertex as usize].next != NIL {
                self.vertices[vertex as usize].visitid = 0;
                vertex = self.vertices[vertex as usize].next;
            }
        }
        self.furthest_id = self.pointid(furthest);
    }

    /// `qh_addpoint(furthest, facet, False)`: adds the point to the hull, from a facet it is
    /// outside of. The return of the C is always True here (no `STOPpoint`, `STOPcone` or
    /// `ONLYgood`).
    ///
    /// # Errors
    ///
    /// The errors of the steps.
    pub(crate) fn addpoint(&mut self, furthest: u32, facet: u32) -> QResult<()> {
        self.maxoutdone = false;
        if facet == NIL {
            return Err(Error::Qhull(6213));
        }
        self.buildtracing(furthest);
        self.findhorizon(furthest, facet)?;
        let vertex = self.makenewfacets(furthest)?;
        self.makenewplanes()?;
        self.matchnewfacets()?;
        self.updatevertices();
        self.findbestnew = false;
        // PREmerge.
        self.premerge(vertex, self.premerge_centrum, self.premerge_cos)?;
        if self.use_findbestnew() {
            self.findbestnew = true;
        } else {
            let mut newfacet = self.newfacet_list;
            while newfacet != NIL && self.facets[newfacet as usize].next != NIL {
                if !self.facets[newfacet as usize].simplicial {
                    self.findbestnew = true;
                    break;
                }
                newfacet = self.facets[newfacet as usize].next;
            }
        }
        self.partitionvisible(false)?;
        self.findbestnew = false;
        self.findbest_notsharp = false;
        self.deletevisible()?;
        self.new_facets = false;
        self.resetlists(true);
        Ok(())
    }

    /// `qh_findhorizon(point, facet, &goodvisible, &goodhorizon)`: the facets the point is above
    /// (by more than `MINvisible`), moved to the end of the facet list as the visible list, and
    /// marks the horizon facets the point is nearly coplanar with.
    ///
    /// # Errors
    ///
    /// Qhull's precision error 6168 for an empty horizon and internal error 6230 for a
    /// tricoplanar facet.
    pub(crate) fn findhorizon(&mut self, point: u32, facet: u32) -> QResult<()> {
        let mut numhorizon = 0;
        let p = self.coords(point);
        self.removefacet(facet);
        self.appendfacet(facet);
        self.num_visible = 1;
        self.visible_list = facet;
        self.facets[facet as usize].visible = true;
        self.facets[facet as usize].f = NIL;
        self.visit_id = self.visit_id.wrapping_add(1);
        let mut visible = self.visible_list;
        while visible != NIL && self.facets[visible as usize].visible {
            let v = visible as usize;
            if self.facets[v].tricoplanar {
                return Err(Error::Qhull(6230));
            }
            self.facets[v].visitid = self.visit_id;
            let mut i = 0;
            loop {
                let neighbor = set::elem(&self.facets[v].neighbors, i);
                i += 1;
                if neighbor == NIL {
                    break;
                }
                let n = neighbor as usize;
                if self.facets[n].visitid == self.visit_id {
                    continue;
                }
                self.facets[n].visitid = self.visit_id;
                let dist = self.distplane(&p, neighbor);
                if dist > self.min_visible {
                    self.removefacet(neighbor);
                    self.appendfacet(neighbor);
                    self.facets[n].visible = true;
                    self.facets[n].f = NIL;
                    self.num_visible += 1;
                } else {
                    if dist > -self.max_coplanar {
                        self.facets[n].coplanar = true;
                        // qh_precision: no restart. MERGING.
                        if dist > 0.0 {
                            maximize_(&mut self.max_outside, dist);
                            maximize_(&mut self.max_vertex, dist);
                            maximize_(&mut self.facets[n].maxoutside, dist);
                        } else {
                            minimize_(&mut self.min_vertex, dist);
                        }
                    } else {
                        self.facets[n].coplanar = false;
                    }
                    numhorizon += 1;
                }
            }
            visible = self.facets[v].next;
        }
        if numhorizon == 0 {
            return Err(Error::Qhull(6168));
        }
        Ok(())
    }

    /// `qh_nextfurthest(&visible)`: the next point to add and its facet, from `facet_next` on
    /// (taken off the end of the facet's outside set), or NULL when no facet has outside points.
    /// On a narrow hull a facet whose furthest point is not clearly outside is passed over.
    fn nextfurthest(&mut self) -> (u32, u32) {
        loop {
            let facet = self.facet_next;
            if facet == self.facet_tail {
                return (NIL, NIL);
            }
            let f = facet as usize;
            if self.facets[f].outsideset.is_none() {
                self.facet_next = self.facets[f].next;
                continue;
            }
            if set::size(&self.facets[f].outsideset) == 0 {
                self.facets[f].outsideset = None;
                self.facet_next = self.facets[f].next;
                continue;
            }
            if self.narrow_hull {
                if self.facets[f].notfurthest {
                    self.furthestout(facet);
                }
                let dist = self.facets[f].furthestdist;
                if dist < self.min_outside {
                    self.facet_next = self.facets[f].next;
                    continue;
                }
            }
            // Not RANDOMoutside, VIRTUALmemory or PICKfurthest.
            let point = set::del_last(&mut self.facets[f].outsideset);
            return (point, facet);
        }
    }

    /// `qh_partitionall(vertices, points, numpoints)`: the outside set of each facet of the
    /// initial simplex, the furthest point last; then every other point to the best facet
    /// (`MERGING`: a point inside may be coplanar).
    ///
    /// # Errors
    ///
    /// As [`Qh::partitionpoint`].
    fn partitionall(&mut self, vertices: &set::Set) -> QResult<()> {
        let numpoints = self.points.len();
        let mut pointset: Vec<u32> =
            (0..u32::try_from(numpoints).expect("fewer than 2^32 points")).collect();
        self.num_outside = 0;
        let mut i = 0;
        loop {
            let vertex = set::elem(vertices, i);
            i += 1;
            if vertex == NIL {
                break;
            }
            let id = self.pointid(self.vertices[vertex as usize].point);
            if let Ok(id) = usize::try_from(id) {
                pointset[id] = NIL;
            }
        }
        // No GOODpointp or GOODvertexp; not BESToutside.
        let distoutside = self.distoutside();
        let mut remaining = self.num_facets;
        let mut point_end = numpoints;
        let mut facet = self.facet_list;
        while facet != NIL && self.facets[facet as usize].next != NIL {
            let f = facet as usize;
            let size = point_end / usize::try_from(remaining).expect("a facet count") + 100;
            remaining -= 1;
            self.facets[f].outsideset = Some(Vec::with_capacity(size));
            let mut bestpoint = NIL;
            let mut bestdist = -REAL_MAX;
            point_end = 0;
            let point_n = pointset.len();
            for point_i in 0..point_n {
                let point = pointset[point_i];
                if point == NIL {
                    continue;
                }
                let dist = self.distplane_point(point, facet);
                if dist < distoutside {
                    pointset[point_end] = point;
                    point_end += 1;
                } else {
                    self.num_outside += 1;
                    if bestpoint == NIL {
                        bestpoint = point;
                        bestdist = dist;
                    } else if dist > bestdist {
                        set::append(&mut self.facets[f].outsideset, bestpoint);
                        bestpoint = point;
                        bestdist = dist;
                    } else {
                        set::append(&mut self.facets[f].outsideset, point);
                    }
                }
            }
            if bestpoint == NIL {
                self.facets[f].outsideset = None;
            } else {
                set::append(&mut self.facets[f].outsideset, bestpoint);
                self.facets[f].furthestdist = bestdist;
            }
            pointset.truncate(point_end);
            facet = self.facets[f].next;
        }
        // MERGING.
        self.findbestnew = true;
        for &point in &pointset {
            if point != NIL {
                self.partitionpoint(point, self.facet_list)?;
            }
        }
        self.findbestnew = false;
        Ok(())
    }

    /// `qh_partitionpoint(point, facet)`: puts the point in the outside set of its best facet,
    /// or, if it is not outside, in the coplanar set (`KEEPnearinside`) when it is near.
    ///
    /// # Errors
    ///
    /// As [`Qh::findbest`] and [`Qh::findbestnew`].
    pub(crate) fn partitionpoint(&mut self, point: u32, facet: u32) -> QResult<()> {
        let p = self.coords(point);
        let (bestfacet, mut bestdist, mut isoutside) = if self.findbestnew {
            self.findbestnew(&p, facet, false)?
        } else {
            self.findbest(&p, facet, false, true, false)?
        };
        if self.narrow_hull && bestdist >= -self.near_inside {
            // Not DELAUNAY; KEEPnearinside.
            isoutside = true;
        }
        let b = bestfacet as usize;
        if isoutside {
            if self.facets[b].outsideset.is_none() || set::last(&self.facets[b].outsideset) == NIL {
                set::append(&mut self.facets[b].outsideset, point);
                if !self.facets[b].newfacet {
                    self.removefacet(bestfacet);
                    self.appendfacet(bestfacet);
                }
                self.facets[b].furthestdist = bestdist;
            } else if self.facets[b].furthestdist < bestdist {
                set::append(&mut self.facets[b].outsideset, point);
                self.facets[b].furthestdist = bestdist;
            } else {
                set::append_2nd_last(&mut self.facets[b].outsideset, point);
            }
            self.num_outside += 1;
        } else if bestdist >= -self.max_coplanar || bestdist > -self.near_inside {
            // (KEEPcoplanar + KEEPnearinside) is 1, and the third case also partitions it as
            // coplanar (KEEPnearinside).
            self.partitioncoplanar(point, bestfacet, Some(&mut bestdist))?;
        }
        Ok(())
    }

    /// `qh_partitioncoplanar(point, facet, dist)`: puts a point that is not outside in the
    /// coplanar set of its best facet, the furthest last (`KEEPnearinside`), raising
    /// `max_outside`; with no `dist` it looks for the best facet first and drops a point far
    /// inside.
    ///
    /// # Errors
    ///
    /// As [`Qh::findbest`] and [`Qh::findbestnew`].
    pub(crate) fn partitioncoplanar(
        &mut self,
        point: u32,
        facet: u32,
        dist: Option<&mut f64>,
    ) -> QResult<()> {
        self.was_coplanar = true;
        let p = self.coords(point);
        let have_dist = dist.is_some();
        let (bestfacet, bestdist) = if let Some(d) = dist {
            (facet, *d)
        } else {
            let (bestfacet, bestdist, _) = if self.findbestnew {
                self.findbestnew(&p, facet, true)?
            } else {
                self.findbest(&p, facet, true, false, false)?
            };
            // Not DELAUNAY or KEEPinside; KEEPnearinside.
            if bestdist < -self.near_inside {
                return Ok(());
            }
            (bestfacet, bestdist)
        };
        if bestdist > self.max_outside {
            if !have_dist && facet != bestfacet {
                let a = self.facets[facet as usize]
                    .normal
                    .expect("a facet's hyperplane");
                let b = self.facets[bestfacet as usize]
                    .normal
                    .expect("a facet's hyperplane");
                let angle = getangle(&a, &b);
                if angle < 0.0 {
                    let oldfindbest = self.findbestnew;
                    self.findbestnew = false;
                    self.partitionpoint(point, bestfacet)?;
                    self.findbestnew = oldfindbest;
                    return Ok(());
                }
            }
            self.max_outside = bestdist;
            // Message 8122 needs bestdist above TRACEdist (REALmax).
        }
        // KEEPnearinside.
        let b = bestfacet as usize;
        let oldfurthest = set::last(&self.facets[b].coplanarset);
        let mut dist2 = 0.0;
        if oldfurthest != NIL {
            dist2 = self.distplane_point(oldfurthest, bestfacet);
        }
        if oldfurthest == NIL || dist2 < bestdist {
            set::append(&mut self.facets[b].coplanarset, point);
        } else {
            set::append_2nd_last(&mut self.facets[b].coplanarset, point);
        }
        Ok(())
    }

    /// `qh_partitionvisible(allpoints, &numoutside)`: moves the outside and coplanar points of
    /// the visible facets to the new facets (the coplanar ones as coplanar points, since
    /// `allpoints` is False here), and the points of deleted vertices too.
    ///
    /// # Errors
    ///
    /// Qhull's errors 6149 (a cycle of replacements) and 6170 (every new facet merged away), and
    /// those of the partition.
    pub(crate) fn partitionvisible(&mut self, allpoints: bool) -> QResult<()> {
        // Not ONLYmax.
        let mut visible = self.visible_list;
        while visible != NIL && self.facets[visible as usize].visible {
            let v = visible as usize;
            if self.facets[v].outsideset.is_none() && self.facets[v].coplanarset.is_none() {
                visible = self.facets[v].next;
                continue;
            }
            let mut newfacet = self.facets[v].f;
            let mut count: u32 = 0;
            while newfacet != NIL && self.facets[newfacet as usize].visible {
                newfacet = self.facets[newfacet as usize].f;
                let c = count;
                count = count.wrapping_add(1);
                if c > self.facet_id {
                    return Err(Error::Qhull(6149));
                }
            }
            if newfacet == NIL {
                newfacet = self.newfacet_list;
            }
            if newfacet == self.facet_tail {
                return Err(Error::Qhull(6170));
            }
            if self.facets[v].outsideset.is_some() {
                let size = i32::try_from(set::size(&self.facets[v].outsideset))
                    .expect("a set fits an int");
                self.num_outside -= size;
                let mut i = 0;
                loop {
                    let point = set::elem(&self.facets[v].outsideset, i);
                    i += 1;
                    if point == NIL {
                        break;
                    }
                    self.partitionpoint(point, newfacet)?;
                }
            }
            if self.facets[v].coplanarset.is_some() {
                // KEEPnearinside.
                let mut i = 0;
                loop {
                    let point = set::elem(&self.facets[v].coplanarset, i);
                    i += 1;
                    if point == NIL {
                        break;
                    }
                    if allpoints {
                        self.partitionpoint(point, newfacet)?;
                    } else {
                        self.partitioncoplanar(point, newfacet, None)?;
                    }
                }
            }
            visible = self.facets[v].next;
        }
        let mut i = 0;
        loop {
            let vertex = set::elem(&self.del_vertices, i);
            i += 1;
            if vertex == NIL {
                break;
            }
            let point = self.vertices[vertex as usize].point;
            if point != NIL {
                if allpoints {
                    self.partitionpoint(point, self.newfacet_list)?;
                } else {
                    self.partitioncoplanar(point, self.newfacet_list, None)?;
                }
            }
        }
        Ok(())
    }
}
