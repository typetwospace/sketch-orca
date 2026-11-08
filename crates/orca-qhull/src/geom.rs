// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Qhull's geometry (`geom_r.c`, `geom2_r.c`): distances, hyperplanes, the searches for the best
//! facet of a point and the roundoff bounds.
//!
//! Carries Qhull code: the functions of Qhull 2015.2's `geom_r.c` and `geom2_r.c` that a 3-d hull
//! with the options of `its_convex_hull` reaches, translated into Rust on 2026-11-07, Copyright
//! (c) 1993-2015 C.B. Barber and The Geometry Center, University of Minnesota, under Qhull's
//! licence (`licenses/Qhull.txt`); the copyright holders are named in `NOTICE`.
//!
//! The arithmetic is C's, operation by operation and in its order (the engine is built with
//! `/fp:precise` and no `/arch`, so no contraction), with Qhull's macros `fabs_`, `fmax_`,
//! `fmin_`, `maximize_` and `minimize_` rather than the library's, as they treat NaN and -0
//! their own way. The trace prints (`IStracing`, `TRACEpoint`) are left out: the options turn
//! tracing off.

use crate::qh::{
    DIM, Error, QResult, Qh, REAL_EPSILON, REAL_MAX, det2_, det3_, fabs_, fmax_, maximize_,
    minimize_,
};
use crate::set::{self, NIL, Set};
use orca_crt::sqrt;

impl Qh {
    /// `qh_USEfindbestnew`: `zzval_(Ztotmerge) > 50`.
    #[inline]
    pub(crate) fn use_findbestnew(&self) -> bool {
        self.ztotmerge > 50
    }

    /// `qh_DISToutside`: `(qh_USEfindbestnew ? 2 : 1) * fmax_((qh->MERGING ? 2 : 1) *
    /// qh->MINoutside, qh->max_outside)`.
    pub(crate) fn distoutside(&self) -> f64 {
        let scale = if self.use_findbestnew() { 2.0 } else { 1.0 };
        let merging = if self.merging { 2.0 } else { 1.0 };
        scale * fmax_(merging * self.min_outside, self.max_outside)
    }

    /// `qh_SEARCHdist`: `(qh_USEfindbestnew ? 2 : 1) * (qh->max_outside + 2 * qh->DISTround +
    /// fmax_(qh->MINvisible, qh->MAXcoplanar))`.
    pub(crate) fn searchdist(&self) -> f64 {
        let scale = if self.use_findbestnew() { 2.0 } else { 1.0 };
        scale
            * (self.max_outside
                + 2.0 * self.dist_round
                + fmax_(self.min_visible, self.max_coplanar))
    }

    /// `qh_distplane`: the signed distance from the point to the facet's hyperplane.
    ///
    /// # Panics
    ///
    /// For a facet without a normal, where the C dereferences NULL.
    pub(crate) fn distplane(&self, point: &[f64; 3], facet: u32) -> f64 {
        let f = &self.facets[facet as usize];
        let normal = f
            .normal
            .as_ref()
            .expect("qh_distplane of a facet without a normal");
        f.offset + point[0] * normal[0] + point[1] * normal[1] + point[2] * normal[2]
    }

    /// `qh_distplane` of an input point.
    #[inline]
    pub(crate) fn distplane_point(&self, point: u32, facet: u32) -> f64 {
        let p = self.coords(point);
        self.distplane(&p, facet)
    }

    /// `qh_findbest(point, startfacet, bestoutside, isnewfacets, noupper, &dist, &isoutside,
    /// &numpart)`: the best facet for the point, its distance and whether the point is outside
    /// (`numpart` only counts, for the statistics).
    #[allow(clippy::too_many_lines)]
    pub(crate) fn findbest(
        &mut self,
        point: &[f64; 3],
        startfacet: u32,
        bestoutside: bool,
        isnewfacets: bool,
        noupper: bool,
    ) -> QResult<(u32, f64, bool)> {
        let mut bestdist = -REAL_MAX / 2.0;
        let mut bestfacet = NIL;
        let mut lastfacet = NIL;
        self.visit_id = self.visit_id.wrapping_add(1);
        let visitid = self.visit_id;
        let mut testhorizon = true;
        let mut isoutside = true;
        let s = startfacet as usize;
        if !self.facets[s].flipped {
            let dist = self.distplane(point, startfacet);
            if !bestoutside
                && dist >= self.min_outside
                && (!self.facets[s].upperdelaunay || !noupper)
            {
                return Ok((startfacet, dist, isoutside));
            }
            bestdist = dist;
            if !self.facets[s].upperdelaunay {
                bestfacet = startfacet;
            }
        }
        self.facets[s].visitid = visitid;
        let mut facet = startfacet;
        while facet != NIL {
            lastfacet = facet;
            let mut i = 0;
            let mut neighbor;
            loop {
                neighbor = set::elem(&self.facets[facet as usize].neighbors, i);
                i += 1;
                if neighbor == NIL {
                    break;
                }
                let n = neighbor as usize;
                if !self.facets[n].newfacet && isnewfacets {
                    continue;
                }
                if self.facets[n].visitid == visitid {
                    continue;
                }
                self.facets[n].visitid = visitid;
                if !self.facets[n].flipped {
                    let dist = self.distplane(point, neighbor);
                    if dist > bestdist {
                        if !bestoutside
                            && dist >= self.min_outside
                            && (!self.facets[n].upperdelaunay || !noupper)
                        {
                            return Ok((neighbor, dist, isoutside));
                        }
                        if !self.facets[n].upperdelaunay {
                            bestfacet = neighbor;
                            bestdist = dist;
                            break;
                        } else if bestfacet == NIL {
                            bestdist = dist;
                            break;
                        }
                    }
                }
            }
            facet = neighbor;
        }
        if isnewfacets {
            if bestfacet == NIL {
                let next = self.facets[s].next;
                let (f, d, o) = self.findbestnew(point, next, bestoutside)?;
                bestfacet = f;
                bestdist = d;
                isoutside = o;
                testhorizon = false;
            } else if !self.findbest_notsharp && bestdist < -self.dist_round {
                if self.sharpnewfacets() {
                    let (f, d, o) = self.findbestnew(point, bestfacet, bestoutside)?;
                    bestfacet = f;
                    bestdist = d;
                    isoutside = o;
                    testhorizon = false;
                    self.findbestnew = true;
                } else {
                    self.findbest_notsharp = true;
                }
            }
        }
        if bestfacet == NIL {
            let (f, d) = self.findbestlower(lastfacet, point)?;
            bestfacet = f;
            bestdist = d;
        }
        if testhorizon {
            bestfacet = self.findbesthorizon(false, point, bestfacet, noupper, &mut bestdist);
        }
        if bestdist < self.min_outside {
            isoutside = false;
        }
        Ok((bestfacet, bestdist, isoutside))
    }

    /// `qh_findbesthorizon(ischeckmax, point, startfacet, noupper, &bestdist, &numpart)`: the best
    /// facet near `startfacet` through its coplanar neighbours; `bestdist` is updated. With
    /// `ischeckmax` it also raises the facets' `maxoutside`.
    pub(crate) fn findbesthorizon(
        &mut self,
        ischeckmax: bool,
        point: &[f64; 3],
        startfacet: u32,
        noupper: bool,
        bestdist: &mut f64,
    ) -> u32 {
        let mut bestfacet = startfacet;
        let mut nextfacet = NIL;
        self.visit_id = self.visit_id.wrapping_add(1);
        let visitid = self.visit_id;
        if ischeckmax && *bestdist > self.facets[startfacet as usize].maxoutside {
            self.facets[startfacet as usize].maxoutside = *bestdist;
        }
        let searchdist = self.searchdist();
        let mut minsearch = *bestdist - searchdist;
        if ischeckmax {
            minimize_(&mut minsearch, -searchdist);
        }
        let mut coplanarfacetset_size = 0usize;
        let mut facet = startfacet;
        loop {
            let mut i = 0;
            loop {
                let neighbor = set::elem(&self.facets[facet as usize].neighbors, i);
                i += 1;
                if neighbor == NIL {
                    break;
                }
                let n = neighbor as usize;
                if self.facets[n].visitid == visitid {
                    continue;
                }
                self.facets[n].visitid = visitid;
                if !self.facets[n].flipped {
                    let dist = self.distplane(point, neighbor);
                    if dist > *bestdist {
                        if !self.facets[n].upperdelaunay
                            || ischeckmax
                            || (!noupper && dist >= self.min_outside)
                        {
                            bestfacet = neighbor;
                            *bestdist = dist;
                            if !ischeckmax {
                                minsearch = dist - searchdist;
                                if dist > *bestdist + searchdist {
                                    coplanarfacetset_size = 0;
                                }
                            }
                        }
                    } else if dist < minsearch {
                        continue;
                    }
                    if ischeckmax && dist > self.facets[n].maxoutside {
                        self.facets[n].maxoutside = dist;
                    }
                }
                if nextfacet != NIL {
                    if coplanarfacetset_size == 0 {
                        coplanarfacetset_size = 1;
                        let v = self.coplanarfacetset.get_or_insert_with(Vec::new);
                        v.clear();
                        v.push(nextfacet);
                    } else {
                        coplanarfacetset_size += 1;
                        set::append(&mut self.coplanarfacetset, nextfacet);
                    }
                }
                nextfacet = neighbor;
            }
            facet = nextfacet;
            if facet != NIL {
                nextfacet = NIL;
            } else if coplanarfacetset_size == 0 {
                break;
            } else {
                coplanarfacetset_size -= 1;
                if coplanarfacetset_size == 0 {
                    facet = set::first(&self.coplanarfacetset);
                    set::truncate(&mut self.coplanarfacetset, 0);
                } else {
                    facet = set::del_last(&mut self.coplanarfacetset);
                }
            }
        }
        bestfacet
    }

    /// `qh_findbestnew(point, startfacet, &dist, bestoutside, &isoutside, &numpart)`: the best new
    /// facet for the point, searching every new facet from `startfacet` round to it.
    ///
    /// # Errors
    ///
    /// Qhull's error 6001 when there is no start facet (merging deleted the cone).
    pub(crate) fn findbestnew(
        &mut self,
        point: &[f64; 3],
        startfacet: u32,
        bestoutside: bool,
    ) -> QResult<(u32, f64, bool)> {
        let mut bestdist = -REAL_MAX / 2.0;
        let mut bestfacet = NIL;
        self.visit_id = self.visit_id.wrapping_add(1);
        let visitid = self.visit_id;
        if startfacet == NIL {
            return Err(Error::Qhull(if self.merging { 6001 } else { 6002 }));
        }
        let (isdistoutside, distoutside) = if bestoutside {
            (false, 0.0)
        } else {
            (true, self.distoutside())
        };
        let mut isoutside = true;
        for i in 0..2 {
            let mut facet = if i == 0 {
                startfacet
            } else {
                self.newfacet_list
            };
            while facet != NIL && self.facets[facet as usize].next != NIL {
                if facet == startfacet && i != 0 {
                    break;
                }
                let f = facet as usize;
                self.facets[f].visitid = visitid;
                if !self.facets[f].flipped {
                    let dist = self.distplane(point, facet);
                    if dist > bestdist
                        && (!self.facets[f].upperdelaunay || dist >= self.min_outside)
                    {
                        bestfacet = facet;
                        if isdistoutside && dist >= distoutside {
                            return Ok((bestfacet, dist, isoutside));
                        }
                        bestdist = dist;
                    }
                }
                facet = self.facets[f].next;
            }
        }
        // testhorizon is True, and qh_NOupper is True, so noupper is False.
        let start = if bestfacet != NIL {
            bestfacet
        } else {
            startfacet
        };
        bestfacet = self.findbesthorizon(false, point, start, false, &mut bestdist);
        // As the C writes it, so a NaN distance leaves the point outside.
        if bestdist < self.min_outside {
            isoutside = false;
        }
        Ok((bestfacet, bestdist, isoutside))
    }

    /// `qh_backnormal(rows, numrow, numcol, sign, normal, &nearzero)`: the normal by back
    /// substitution of the upper triangular rows; returns `nearzero`.
    fn backnormal(&self, rows: &[[f64; 3]; 3], sign: bool, normal: &mut [f64; 3]) -> bool {
        const NUMROW: usize = DIM - 1;
        const NUMCOL: usize = DIM;
        let mut zerocol: i32 = -1;
        normal[NUMCOL - 1] = if sign { -1.0 } else { 1.0 };
        for i in (0..NUMROW).rev() {
            normal[i] = 0.0;
            for j in i + 1..NUMCOL {
                normal[i] -= rows[i][j] * normal[j];
            }
            let diagonal = rows[i][i];
            if fabs_(diagonal) > self.min_denom_2 {
                normal[i] /= diagonal;
            } else {
                let (value, waszero) = divzero(normal[i], diagonal, self.min_denom_1_2);
                normal[i] = value;
                if waszero {
                    zerocol = i32::try_from(i).expect("a row index fits an int");
                    normal[i] = if sign { -1.0 } else { 1.0 };
                    for tail in normal.iter_mut().take(NUMCOL).skip(i + 1) {
                        *tail = 0.0;
                    }
                }
            }
        }
        zerocol != -1
    }

    /// `qh_gausselim(rows, numrow, numcol, &sign, &nearzero)`: Gaussian elimination with partial
    /// pivoting, swapping rows; returns `nearzero`.
    fn gausselim(
        &self,
        rows: &mut [[f64; 3]; 3],
        numrow: usize,
        numcol: usize,
        sign: &mut bool,
    ) -> bool {
        let mut nearzero = false;
        for k in 0..numrow {
            let mut pivot_abs = fabs_(rows[k][k]);
            let mut pivoti = k;
            for (i, row) in rows.iter().enumerate().take(numrow).skip(k + 1) {
                let temp = fabs_(row[k]);
                if temp > pivot_abs {
                    pivot_abs = temp;
                    pivoti = i;
                }
            }
            if pivoti != k {
                rows.swap(pivoti, k);
                *sign ^= true;
            }
            if pivot_abs <= self.near_zero[k] {
                nearzero = true;
                if pivot_abs == 0.0 {
                    continue;
                }
            }
            let pivotrow = rows[k];
            let pivot = pivotrow[k];
            for row in rows.iter_mut().take(numrow).skip(k + 1) {
                let n = row[k] / pivot;
                for j in k + 1..numcol {
                    row[j] -= n * pivotrow[j];
                }
            }
        }
        nearzero
    }

    /// `qh_getcenter(vertices)`: the mean of the vertices' points.
    ///
    /// # Errors
    ///
    /// Qhull's error 6003 for fewer than two vertices.
    pub(crate) fn getcenter(&self, vertices: &Set) -> QResult<[f64; 3]> {
        let count = set::size(vertices);
        if count < 2 {
            return Err(Error::Qhull(6003));
        }
        let mut center = [0.0f64; 3];
        for (k, coord) in center.iter_mut().enumerate() {
            *coord = 0.0;
            let mut i = 0;
            loop {
                let vertex = set::elem(vertices, i);
                i += 1;
                if vertex == NIL {
                    break;
                }
                *coord += self.points[self.vertices[vertex as usize].point as usize][k];
            }
            #[allow(clippy::cast_precision_loss)]
            {
                *coord /= count as f64;
            }
        }
        Ok(center)
    }

    /// `qh_getcentrum(facet)`: the centre of the facet's vertices projected onto its hyperplane.
    ///
    /// # Errors
    ///
    /// Qhull's error 6003 for a facet of fewer than two vertices.
    pub(crate) fn getcentrum(&self, facet: u32) -> QResult<[f64; 3]> {
        let point = self.getcenter(&self.facets[facet as usize].vertices)?;
        let dist = self.distplane(&point, facet);
        Ok(self.projectpoint(&point, facet, dist))
    }

    /// `qh_getdistance(facet, neighbor, &mindist, &maxdist)`: the furthest a vertex of `facet`
    /// not in `neighbor` is from `neighbor`'s hyperplane, with the most negative and most
    /// positive distances (each at least 0 in size).
    pub(crate) fn getdistance(&mut self, facet: u32, neighbor: u32) -> (f64, f64, f64) {
        let mut i = 0;
        loop {
            let vertex = set::elem(&self.facets[facet as usize].vertices, i);
            i += 1;
            if vertex == NIL {
                break;
            }
            self.vertices[vertex as usize].seen = false;
        }
        let mut i = 0;
        loop {
            let vertex = set::elem(&self.facets[neighbor as usize].vertices, i);
            i += 1;
            if vertex == NIL {
                break;
            }
            self.vertices[vertex as usize].seen = true;
        }
        let mut mind = 0.0;
        let mut maxd = 0.0;
        let mut i = 0;
        loop {
            let vertex = set::elem(&self.facets[facet as usize].vertices, i);
            i += 1;
            if vertex == NIL {
                break;
            }
            if !self.vertices[vertex as usize].seen {
                let dist = self.distplane_point(self.vertices[vertex as usize].point, neighbor);
                if dist < mind {
                    mind = dist;
                } else if dist > maxd {
                    maxd = dist;
                }
            }
        }
        let (mindist, maxdist) = (mind, maxd);
        let mind = -mind;
        (if maxd > mind { maxd } else { mind }, mindist, maxdist)
    }

    /// `qh_normalize2(normal, dim, toporient, NULL, NULL)`: makes the normal a unit vector,
    /// pointing the other way unless `toporient`.
    ///
    /// # Errors
    ///
    /// [`Error::Unported`] for a NaN normal, where `qh_maxabsval` finds no coordinate and the C
    /// dereferences NULL.
    pub(crate) fn normalize2(&self, normal: &mut [f64; 3], toporient: bool) -> QResult<()> {
        let mut norm = sqrt(normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]);
        if norm > self.min_denom {
            if !toporient {
                norm = -norm;
            }
            normal[0] /= norm;
            normal[1] /= norm;
            normal[2] /= norm;
        } else if norm == 0.0 {
            let temp = sqrt(1.0 / 3.0);
            *normal = [temp; 3];
        } else {
            if !toporient {
                norm = -norm;
            }
            for k in 0..DIM {
                let (temp, zerodiv) = divzero(normal[k], norm, self.min_denom_1);
                if zerodiv {
                    let Some(maxp) = maxabsval(normal) else {
                        return Err(Error::Unported(
                            "qh_normalize2 of a NaN normal (the C dereferences NULL)",
                        ));
                    };
                    let temp = if normal[maxp] * norm >= 0.0 {
                        1.0
                    } else {
                        -1.0
                    };
                    *normal = [0.0; 3];
                    normal[maxp] = temp;
                    return Ok(());
                }
                normal[k] = temp;
            }
        }
        Ok(())
    }

    /// `qh_projectpoint(point, facet, dist)`: the point moved `dist` against the facet's normal.
    pub(crate) fn projectpoint(&self, point: &[f64; 3], facet: u32, dist: f64) -> [f64; 3] {
        let normal = self.facets[facet as usize]
            .normal
            .expect("qh_projectpoint onto a facet without a normal");
        [
            point[0] - dist * normal[0],
            point[1] - dist * normal[1],
            point[2] - dist * normal[2],
        ]
    }

    /// `qh_setfacetplane(facet)`: the facet's hyperplane through its vertices, by determinants
    /// and, when they are nearly zero, by Gaussian elimination.
    ///
    /// # Errors
    ///
    /// As [`Qh::normalize2`].
    pub(crate) fn setfacetplane(&mut self, facet: u32) -> QResult<()> {
        let f = facet as usize;
        let point0 = self.vertices[set::first(&self.facets[f].vertices) as usize].point;
        self.zsetplane += 1;
        let mut nearzero = false;
        // qh_sethyperplane_det, dim 3: gm_row holds the vertices' points in set order.
        let mut rowpoints = [NIL; DIM];
        {
            let mut i = 0;
            loop {
                let vertex = set::elem(&self.facets[f].vertices, i);
                if vertex == NIL {
                    break;
                }
                rowpoints[i] = self.vertices[vertex as usize].point;
                i += 1;
            }
        }
        let rows = rowpoints.map(|p| self.coords(p));
        let toporient = self.facets[f].toporient;
        let dx = |i: usize, j: usize| rows[i][0] - rows[j][0];
        let dy = |i: usize, j: usize| rows[i][1] - rows[j][1];
        let dz = |i: usize, j: usize| rows[i][2] - rows[j][2];
        let mut normal = [
            det2_(dy(2, 0), dz(2, 0), dy(1, 0), dz(1, 0)),
            det2_(dx(1, 0), dz(1, 0), dx(2, 0), dz(2, 0)),
            det2_(dx(2, 0), dy(2, 0), dx(1, 0), dy(1, 0)),
        ];
        self.normalize2(&mut normal, toporient)?;
        let p0 = self.coords(point0);
        let mut offset = -(p0[0] * normal[0] + p0[1] * normal[1] + p0[2] * normal[2]);
        let maxround = self.dist_round;
        for i in (0..DIM).rev() {
            if rowpoints[i] != point0 {
                let point = rows[i];
                let dist =
                    offset + (point[0] * normal[0] + point[1] * normal[1] + point[2] * normal[2]);
                if dist > maxround || dist < -maxround {
                    nearzero = true;
                    break;
                }
            }
        }
        if nearzero {
            // qh_sethyperplane_gauss: the rows are the other vertices' points less point0.
            let mut grows = [[0.0f64; 3]; 3];
            let mut i = 0;
            let mut j = 0;
            loop {
                let vertex = set::elem(&self.facets[f].vertices, j);
                j += 1;
                if vertex == NIL {
                    break;
                }
                let p = self.vertices[vertex as usize].point;
                if p != point0 {
                    let c = self.coords(p);
                    for k in 0..DIM {
                        grows[i][k] = c[k] - p0[k];
                    }
                    i += 1;
                }
            }
            let mut sign = toporient;
            nearzero = self.gausselim(&mut grows, DIM - 1, DIM, &mut sign);
            for k in (0..DIM - 1).rev() {
                if grows[k][k] < 0.0 {
                    sign ^= true;
                }
            }
            let nearzero2 = self.backnormal(&grows, sign, &mut normal);
            if nearzero2 {
                nearzero = true;
            }
            self.normalize2(&mut normal, true)?;
            offset = -(p0[0] * normal[0]);
            for k in 1..DIM {
                offset -= p0[k] * normal[k];
            }
        }
        self.facets[f].normal = Some(normal);
        self.facets[f].offset = offset;
        if nearzero {
            self.orientoutside(facet);
        }
        self.facets[f].upperdelaunay = false;
        Ok(())
    }

    /// `qh_orientoutside(facet)`: turns the facet's hyperplane so the interior point is below it;
    /// returns whether it turned it.
    pub(crate) fn orientoutside(&mut self, facet: u32) -> bool {
        let interior = self.interior_point;
        let dist = self.distplane(&interior, facet);
        if dist > 0.0 {
            let f = &mut self.facets[facet as usize];
            let normal = f.normal.as_mut().expect("a facet with a hyperplane");
            for k in (0..DIM).rev() {
                normal[k] = -normal[k];
            }
            f.offset = -f.offset;
            return true;
        }
        false
    }

    /// `qh_detsimplex(apex, points, dim, &nearzero)`: the determinant of the first `dim` points
    /// less the apex, in their first `dim` coordinates.
    ///
    /// # Errors
    ///
    /// Qhull's error 6007 for fewer than `dim` points.
    fn detsimplex(&self, apex: u32, points: &Set, dim: usize) -> QResult<(f64, bool)> {
        let a = self.coords(apex);
        let mut rows = [[0.0f64; 3]; 3];
        let mut i = 0;
        let mut j = 0;
        loop {
            let point = set::elem(points, j);
            j += 1;
            if point == NIL || i == dim {
                break;
            }
            let c = self.coords(point);
            for k in 0..dim {
                rows[i][k] = c[k] - a[k];
            }
            i += 1;
        }
        if i < dim {
            return Err(Error::Qhull(6007));
        }
        Ok(self.determinant(&rows, dim))
    }

    /// `qh_determinant(rows, dim, &nearzero)` for dimension 2 or 3.
    fn determinant(&self, rows: &[[f64; 3]; 3], dim: usize) -> (f64, bool) {
        if dim == 2 {
            let det = det2_(rows[0][0], rows[0][1], rows[1][0], rows[1][1]);
            (det, fabs_(det) < 10.0 * self.near_zero[1])
        } else {
            let det = det3_(
                rows[0][0], rows[0][1], rows[0][2], rows[1][0], rows[1][1], rows[1][2], rows[2][0],
                rows[2][1], rows[2][2],
            );
            (det, fabs_(det) < 10.0 * self.near_zero[2])
        }
    }

    /// `qh_detroundoff`: the roundoff bounds from the input's extent.
    ///
    /// # Errors
    ///
    /// Qhull's error 6006 when the roundoff passes `JOGGLEmax` (`REALmax`), which an infinite
    /// coordinate makes it do.
    pub(crate) fn detroundoff(&mut self) -> QResult<()> {
        self.dist_round = distround(DIM, self.max_abs_coord, self.max_sum_coord);
        self.min_denom = self.min_denom_1 * self.max_abs_coord;
        self.min_denom_1_2 = sqrt(self.min_denom_1 * 3.0);
        self.min_denom_2 = self.min_denom_1_2 * self.max_abs_coord;
        self.angle_round = 1.01 * 3.0 * REAL_EPSILON;
        if self.premerge_cos < REAL_MAX / 2.0 {
            self.premerge_cos -= self.angle_round;
        }
        if self.postmerge_cos < REAL_MAX / 2.0 {
            self.postmerge_cos -= self.angle_round;
        }
        self.premerge_centrum += 2.0 * self.dist_round;
        self.postmerge_centrum += 2.0 * self.dist_round;
        {
            let mut maxangle = 1.0;
            crate::qh::minimize_(&mut maxangle, self.premerge_cos);
            crate::qh::minimize_(&mut maxangle, self.postmerge_cos);
            self.one_merge =
                sqrt(3.0) * self.max_width * sqrt(1.0 - maxangle * maxangle) + self.dist_round;
            let maxrho = 3.0 * self.premerge_centrum + self.dist_round;
            maximize_(&mut self.one_merge, maxrho);
            let maxrho = 3.0 * self.postmerge_centrum + self.dist_round;
            maximize_(&mut self.one_merge, maxrho);
        }
        self.near_inside = self.one_merge * 5.0;
        if self.joggle_max < self.dist_round {
            return Err(Error::Qhull(6006));
        }
        if self.min_visible > REAL_MAX / 2.0 {
            // MERGING and a 3-d hull.
            self.min_visible = self.premerge_centrum;
        }
        if self.max_coplanar > REAL_MAX / 2.0 {
            self.max_coplanar = self.min_visible;
        }
        // Not APPROXhull.
        self.min_outside = 2.0 * self.min_visible;
        if self.premerge_cos < REAL_MAX / 2.0 {
            maximize_(
                &mut self.min_outside,
                (1.0 - self.premerge_cos) * self.max_abs_coord,
            );
        }
        self.wide_facet = self.min_outside;
        maximize_(&mut self.wide_facet, 6.0 * self.max_coplanar);
        maximize_(&mut self.wide_facet, 6.0 * self.min_visible);
        // Warning 7001 needs MINvisible above MINoutside, which is twice it.
        self.max_vertex = self.dist_round;
        self.min_vertex = -self.dist_round;
        Ok(())
    }

    /// `qh_maxmin(points, numpoints, dimension)`: the extent of the input, and the points with
    /// the largest and smallest coordinate in each dimension (max then min, by dimension).
    pub(crate) fn maxmin(&mut self) -> Set {
        self.max_outside = 0.0;
        self.max_abs_coord = 0.0;
        self.max_width = -REAL_MAX;
        self.max_sum_coord = 0.0;
        self.min_vertex = 0.0;
        self.was_coplanar = false;
        if self.zero_centrum {
            self.zero_all_ok = true;
        }
        let mut set: Set = Some(Vec::with_capacity(2 * DIM));
        for k in 0..DIM {
            let mut minimum = 0u32;
            let mut maximum = 0u32;
            for point in 0..self.points.len() {
                let p = self.points[point][k];
                let point = u32::try_from(point).expect("fewer than 2^32 points");
                if self.points[maximum as usize][k] < p {
                    maximum = point;
                } else if self.points[minimum as usize][k] > p {
                    minimum = point;
                }
            }
            let maxk = self.points[maximum as usize][k];
            let mink = self.points[minimum as usize][k];
            let maxcoord = fmax_(maxk, -mink);
            let temp = maxk - mink;
            maximize_(&mut self.max_width, temp);
            maximize_(&mut self.max_abs_coord, maxcoord);
            self.max_sum_coord += maxcoord;
            set::append(&mut set, maximum);
            set::append(&mut set, minimum);
            self.near_zero[k] = 80.0 * self.max_sum_coord * REAL_EPSILON;
        }
        set
    }

    /// `qh_maxsimplex(dim, maxpoints, points, numpoints, &simplex)`: extends the simplex to
    /// `dim + 1` points of the largest determinants.
    ///
    /// # Errors
    ///
    /// Qhull's error 6013 when every point has the same x coordinate, and 6014 when no point is
    /// left to add.
    pub(crate) fn maxsimplex(
        &mut self,
        dim: usize,
        maxpoints: &Set,
        simplex: &mut Set,
    ) -> QResult<()> {
        let mut maxx = NIL;
        let mut minx = NIL;
        let mut maxnearzero = false;
        let mut mincoord = REAL_MAX;
        let mut maxcoord = -REAL_MAX;
        let mut sizinit = set::size(simplex);
        let numpoints = u32::try_from(self.points.len()).expect("fewer than 2^32 points");
        if sizinit < 2 {
            let mut scan = |point: u32| {
                let p0 = self.points[point as usize][0];
                if maxcoord < p0 {
                    maxcoord = p0;
                    maxx = point;
                }
                if mincoord > p0 {
                    mincoord = p0;
                    minx = point;
                }
            };
            if set::size(maxpoints) >= 2 {
                let mut i = 0;
                loop {
                    let point = set::elem(maxpoints, i);
                    i += 1;
                    if point == NIL {
                        break;
                    }
                    scan(point);
                }
            } else {
                for point in 0..numpoints {
                    scan(point);
                }
            }
            set::unique(simplex, minx);
            if set::size(simplex) < 2 {
                set::unique(simplex, maxx);
            }
            sizinit = set::size(simplex);
            if sizinit < 2 {
                let code = if self.zsetplane > 3 + 1 { 6012 } else { 6013 };
                return Err(Error::Qhull(code));
            }
        }
        for k in sizinit..=dim {
            let mut maxpoint = NIL;
            let mut maxdet = -REAL_MAX;
            let mut i = 0;
            loop {
                let point = set::elem(maxpoints, i);
                i += 1;
                if point == NIL {
                    break;
                }
                if !set::contains(simplex, point) {
                    let (det, nearzero) = self.detsimplex(point, simplex, k)?;
                    let det = fabs_(det);
                    if det > maxdet {
                        maxdet = det;
                        maxpoint = point;
                        maxnearzero = nearzero;
                    }
                }
            }
            if maxpoint == NIL || maxnearzero {
                for point in 0..numpoints {
                    if !set::contains(simplex, point) {
                        let (det, nearzero) = self.detsimplex(point, simplex, k)?;
                        let det = fabs_(det);
                        if det > maxdet {
                            maxdet = det;
                            maxpoint = point;
                            maxnearzero = nearzero;
                        }
                    }
                }
            }
            if maxpoint == NIL {
                return Err(Error::Qhull(6014));
            }
            set::append(simplex, maxpoint);
        }
        Ok(())
    }

    /// `qh_maxouter`: `fmax_(qh->max_outside, qh->DISTround) + qh->DISTround`.
    pub(crate) fn maxouter(&self) -> f64 {
        let mut dist = fmax_(self.max_outside, self.dist_round);
        dist += self.dist_round;
        dist
    }

    /// `qh_outerinner(NULL, &outerplane, &innerplane)`: the outer and inner planes of the hull
    /// (no joggle).
    pub(crate) fn outerinner_hull(&self) -> (f64, f64) {
        (self.maxouter(), self.min_vertex - self.dist_round)
    }

    /// `qh_sharpnewfacets`: whether the new facets' normals point into different orthants.
    pub(crate) fn sharpnewfacets(&self) -> bool {
        let mut quadrant = [false; DIM];
        let mut issharp = false;
        let mut facet = self.newfacet_list;
        while facet != NIL && self.facets[facet as usize].next != NIL {
            let normal = self.facets[facet as usize]
                .normal
                .expect("a new facet's hyperplane");
            if facet == self.newfacet_list {
                for k in (0..DIM).rev() {
                    quadrant[k] = normal[k] > 0.0;
                }
            } else {
                for k in (0..DIM).rev() {
                    if quadrant[k] != (normal[k] > 0.0) {
                        issharp = true;
                        break;
                    }
                }
            }
            if issharp {
                break;
            }
            facet = self.facets[facet as usize].next;
        }
        issharp
    }
}

/// `qh_getangle(vect1, vect2)`: the dot product, summed from 0.
pub(crate) fn getangle(vect1: &[f64; 3], vect2: &[f64; 3]) -> f64 {
    let mut angle = 0.0;
    for k in 0..DIM {
        angle += vect1[k] * vect2[k];
    }
    angle
}

/// `qh_distround(dimension, maxabs, maxsumabs)`: the roundoff of a distance computation.
fn distround(dimension: usize, maxabs: f64, maxsumabs: f64) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let dim = dimension as f64;
    let mut maxdistsum = sqrt(dim) * maxabs;
    minimize_(&mut maxdistsum, maxsumabs);
    REAL_EPSILON * (dim * maxdistsum * 1.01 + maxabs)
}

/// `qh_divzero(numer, denom, mindenom1, &zerodiv)`: `numer / denom`, or 0 and `zerodiv` when the
/// quotient would be too large.
pub(crate) fn divzero(numer: f64, denom: f64, mindenom1: f64) -> (f64, bool) {
    if numer < mindenom1 && numer > -mindenom1 {
        let numerx = fabs_(numer);
        let denomx = fabs_(denom);
        if numerx < denomx {
            return (numer / denom, false);
        }
        return (0.0, true);
    }
    let temp = denom / numer;
    if temp > mindenom1 || temp < -mindenom1 {
        (numer / denom, false)
    } else {
        (0.0, true)
    }
}

/// `qh_maxabsval(normal, dim)`: the index of the first coordinate of the largest size, or none
/// when every coordinate is NaN.
fn maxabsval(normal: &[f64; 3]) -> Option<usize> {
    let mut maxval = -REAL_MAX;
    let mut maxp = None;
    for (k, &c) in normal.iter().enumerate() {
        let absval = fabs_(c);
        if absval > maxval {
            maxval = absval;
            maxp = Some(k);
        }
    }
    maxp
}

/// `qh_pointdist(point1, point2, dim)`: the distance (`dim > 0`) or its square (`dim < 0`).
pub(crate) fn pointdist(point1: &[f64; 3], point2: &[f64; 3], dim: i32) -> f64 {
    let mut dist = 0.0;
    for k in 0..DIM {
        let diff = point1[k] - point2[k];
        dist += diff * diff;
    }
    if dim > 0 { sqrt(dist) } else { dist }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divzero_refuses_a_quotient_past_the_bound() {
        assert_eq!(divzero(1.0, 2.0, 1e-10), (0.5, false));
        assert_eq!(divzero(1.0, 1e-300, 1e-10), (0.0, true));
        assert_eq!(divzero(1e-20, 1e-30, 1e-10), (0.0, true));
    }

    #[test]
    fn fabs_keeps_negative_zero() {
        assert!(fabs_(-0.0).is_sign_negative());
        assert_eq!(maxabsval(&[f64::NAN; 3]), None);
        assert_eq!(maxabsval(&[1.0, -3.0, 3.0]), Some(1));
    }
}
