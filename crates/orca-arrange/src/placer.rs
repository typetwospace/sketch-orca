// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Libnest2D code: libnest2d as OrcaSlicer v2.4.2 carries it in deps_src/libnest2d,
// translated on 2026-11-07 from include/libnest2d/placers/nfpplacer.hpp (NfpPConfig, EdgeCache,
// correctNfpPosition, minimizeCircle, _NofitPolyPlacer), placers/placer_boilerplate.hpp
// (PlacerBoilerplate, PackResult) and parallel.hpp (__parallel::enumerate). Copyright (C) Tamas
// Meszaros, modified by Prusa Research, Bambu Lab and the OrcaSlicer contributors, under the GNU
// Lesser General Public License, version 3 (licenses/LGPL-3.0.txt).

//! libnest2d's no-fit-polygon placer, `_NofitPolyPlacer<ExPolygon, Bin>`, for the three bins
//! `Arrange.cpp` uses: a box, a circle and a shape.
//!
//! Items live in one arena (`&[Item]`, `&mut [Item]`); an item group (`ItemGroup`, a vector of
//! references) is a vector of arena indices. The item being placed is passed on its own, so the
//! packed ones can be read while it changes, as the C++ reads them through references.
//!
//! - `trypack` first tries the starting corner when every packed item is a virtual object (an
//!   excluded region), then slides the item around the no-fit polygons of the packed items inside
//!   the bin's box, optimising the position on each polygon's contour (and holes) from its
//!   corners with subplex, and keeps the best position that passes the bin check.
//! - The corner optimisations and the no-fit polygons run in parallel in the C++
//!   (`__parallel::enumerate` over TBB); each task writes its own slot, so the order of the tasks
//!   never reaches a result. They run through [`orca_tbb::parallel_for_each_mut`] here.
//! - The edge cache walks the open contour without its closing edge, starts the contour's
//!   corners with two zeros, and maps a relative position to a point with `round` of the
//!   distance along the edge's angle, as libnest2d does.
//! - `finalAlign` runs when the C++ placer is cleared or destroyed: it moves the packed items so
//!   their box is aligned in the bin (a circle bin moves every item, virtual and fixed ones too),
//!   keeping the pile's reference point inside the fixed items' no-fit region.

use crate::ArrangeError;
use crate::bg::{touches_point, within_point, within_shape_shape};
use crate::geometry::{
    Box, Circle, Segment, area_expolygon, bounding_box, bounding_box_circle, bounding_box2,
    convex_hull_shapes, distance, is_inside_shape_box, is_inside_shape_circle, merge_with,
    subtract, translate_shape,
};
use crate::item::Item;
use crate::nfp::{NfpResult, nfp_convex_only, nfp_inner_rect_bed, rightmost_up_vertex};
use crate::optimizer::{OptResult, StopCriteria, optimize_min, placer_stop_criteria};
use orca_geom::expolygon::{ExPolygon, ExPolygons, projection_onto};
use orca_geom::point::{Coord, Point};

/// `NfpPConfig::Alignment`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alignment {
    /// `CENTER`.
    Center,
    /// `BOTTOM_LEFT`.
    BottomLeft,
    /// `BOTTOM_RIGHT`.
    BottomRight,
    /// `TOP_LEFT`.
    TopLeft,
    /// `TOP_RIGHT`.
    TopRight,
    /// `DONT_ALIGN`.
    DontAlign,
    /// `USER_DEFINED`.
    UserDefined,
}

/// The bin a placer fills.
#[derive(Clone, Debug, PartialEq)]
pub enum Bin {
    /// `_Box<Point>`.
    Box(Box),
    /// `_Circle<Point>`.
    Circle(Circle),
    /// `ExPolygon`.
    Shape(ExPolygon),
}

impl Bin {
    /// `sl::boundingBox(bin)`.
    #[must_use]
    pub fn bounding_box(&self) -> Box {
        match self {
            Bin::Box(b) => *b,
            Bin::Circle(c) => bounding_box_circle(c),
            Bin::Shape(s) => bounding_box(s),
        }
    }

    /// `sl::area(bin)`.
    #[must_use]
    pub fn area(&self) -> f64 {
        match self {
            Bin::Box(b) => b.area_f64(),
            Bin::Circle(c) => c.area(),
            Bin::Shape(s) => area_expolygon(s),
        }
    }
}

/// `NfpPConfig`: the placer's configuration, its object function of the hooks' type.
#[derive(Clone, Debug)]
pub struct PlacerConfig<F> {
    /// `rotations`.
    pub rotations: Vec<f64>,
    /// `alignment`.
    pub alignment: Alignment,
    /// `starting_point`.
    pub starting_point: Alignment,
    /// `best_object_pos`.
    pub best_object_pos: Point,
    /// `object_function`.
    pub object_function: F,
    /// `accuracy`.
    pub accuracy: f32,
    /// `explore_holes`.
    pub explore_holes: bool,
    /// `parallel`.
    pub parallel: bool,
}

impl<F> PlacerConfig<F> {
    /// `NfpPConfig()` with an object function: rotations 0, Pi/2, Pi and 3Pi/2, centred, accuracy
    /// 0.65, parallel.
    pub fn new(object_function: F) -> PlacerConfig<F> {
        let pi = crate::geometry::PI;
        PlacerConfig {
            rotations: vec![0.0, pi / 2.0, pi, 3.0 * pi / 2.0],
            alignment: Alignment::Center,
            starting_point: Alignment::Center,
            best_object_pos: Point::new(0, 0),
            object_function,
            accuracy: 0.65,
            explore_holes: false,
            parallel: true,
        }
    }
}

/// What the placer's configuration calls back: the object function, `before_packing`,
/// `on_preload` and the selection's `sortfunc`. `Arrange.cpp`'s `AutoArranger` sets them all.
pub trait Hooks: Sync {
    /// The object function's type.
    type ObjFn: Clone + Send + Sync;

    /// `object_function(item, packed_items)`; the packed items are not passed, as
    /// `Arrange.cpp`'s functions ignore them.
    ///
    /// # Errors
    ///
    /// An exception of the function.
    fn object_function(
        &self,
        arena: &[Item],
        f: &Self::ObjFn,
        item: &Item,
    ) -> Result<f64, ArrangeError>;

    /// `before_packing(merged_pile, packed_items, remaining_items)`.
    fn before_packing(
        &mut self,
        arena: &[Item],
        merged_pile: &[ExPolygon],
        packed: &[usize],
        remaining: &[usize],
    );

    /// `on_preload(items, config)`.
    fn on_preload(&self, arena: &[Item], items: &[usize], cfg: &mut PlacerConfig<Self::ObjFn>);

    /// `sortfunc(i1, i2)`.
    fn sort_less(&self, a: &Item, b: &Item) -> bool;
}

/// One contour's edge cache.
#[derive(Clone, Debug, Default)]
struct ContourCache {
    emap: Vec<Segment>,
    distances: Vec<f64>,
    full_distance: f64,
}

impl ContourCache {
    /// `createCache` of one open path: its edges without the closing one.
    fn new(points: &[Point]) -> ContourCache {
        let mut c = ContourCache::default();
        for w in points.windows(2) {
            let e = Segment::new(w[0], w[1]);
            c.emap.push(e);
            c.full_distance += orca_crt::sqrt(e.sqlength_f64());
            c.distances.push(c.full_distance);
        }
        c
    }

    /// `coords(cache, distance)`.
    fn coords(&self, distance: f64) -> Point {
        if self.distances.is_empty() || self.emap.is_empty() {
            return Point::new(0, 0);
        }
        let distance = if distance > 1.0 {
            orca_crt::fmod(distance, 1.0)
        } else {
            distance
        };
        let d = distance * self.full_distance;
        let idx = orca_stl::lower_bound_by(&self.distances, &d, |e, x| e < x);
        let edge = self.emap[idx];
        let ed = d - if idx > 0 {
            self.distances[idx - 1]
        } else {
            0.0
        };
        let angle = edge.angle_to_xaxis();
        let dx = orca_crt::f64_to_i64(orca_crt::round(ed * orca_crt::cos(angle)));
        let dy = orca_crt::f64_to_i64(orca_crt::round(ed * orca_crt::sin(angle)));
        edge.first + Point::new(dx, dy)
    }
}

/// `EdgeCache<ExPolygon>`: points on a shape's contour and holes by relative position.
#[derive(Clone, Debug)]
pub struct EdgeCache {
    contour: ContourCache,
    holes: Vec<ContourCache>,
    accuracy: f64,
}

impl EdgeCache {
    /// `EdgeCache(sh)`, accuracy 1.
    #[must_use]
    pub fn new(sh: &ExPolygon) -> EdgeCache {
        EdgeCache {
            contour: ContourCache::new(&sh.contour.points),
            holes: sh
                .holes
                .iter()
                .map(|h| ContourCache::new(&h.points))
                .collect(),
            accuracy: 1.0,
        }
    }

    /// `accuracy(a)`.
    pub fn set_accuracy(&mut self, a: f64) {
        self.accuracy = a;
    }

    /// `stride(N)`: `round(N / pow(N, pow(accuracy, 1/3)))` as `coord_t`, then `size_t`.
    fn stride(&self, n: usize) -> usize {
        let nf = n as f64;
        let s = orca_crt::round(nf / orca_crt::pow(nf, orca_crt::pow(self.accuracy, 1.0 / 3.0)));
        orca_crt::f64_to_i64(s) as usize
    }

    /// The corners after the given leading zeros (two for the contour, one for a hole).
    fn corners_of(&self, c: &ContourCache, zeros: usize) -> Vec<f64> {
        let n = c.distances.len();
        let s = self.stride(n);
        assert!(s != 0, "EdgeCache stride 0: the C++ divides by it");
        let mut corners = vec![0.0; zeros];
        let n_1 = n.wrapping_sub(1);
        let mut i = 0usize;
        while i < n_1 {
            corners.push(c.distances[i] / c.full_distance);
            i = i.wrapping_add(s);
        }
        corners
    }

    /// `corners()`: 0, 0, then the relative distance at every stride-th edge end.
    #[must_use]
    pub fn corners(&self) -> Vec<f64> {
        self.corners_of(&self.contour, 2)
    }

    /// `corners(holeidx)`: 0, then the relative distance at every stride-th edge end.
    #[must_use]
    pub fn hole_corners(&self, hidx: usize) -> Vec<f64> {
        self.corners_of(&self.holes[hidx], 1)
    }

    /// `coords(distance)` on the contour.
    #[must_use]
    pub fn coords(&self, distance: f64) -> Point {
        self.contour.coords(distance)
    }

    /// `coords(hidx, distance)` on a hole.
    #[must_use]
    pub fn hole_coords(&self, hidx: usize, distance: f64) -> Point {
        self.holes[hidx].coords(distance)
    }

    /// `holeCount()`.
    #[must_use]
    pub fn hole_count(&self) -> usize {
        self.holes.len()
    }

    /// `circumference()`: the contour's length without its closing edge.
    #[must_use]
    pub fn circumference(&self) -> f64 {
        self.contour.full_distance
    }
}

/// `correctNfpPosition`: the no-fit polygon moved so that the orbiter's top vertex, touching the
/// stationary shape's top vertex with its bottom vertex, is its reference point.
fn correct_nfp_position(
    nfp: &mut NfpResult,
    touch_sh: Point,
    orbiter_lmb: Point,
    orbiter_rmt: Point,
) {
    let dtouch = touch_sh - orbiter_lmb;
    let top_other = orbiter_rmt + dtouch;
    let dnfp = top_other - nfp.1;
    translate_shape(&mut nfp.0, dnfp);
}

/// `minimizeCircle(sh)`: subplex (30 evaluations, relative difference 1e-3) over the centre's
/// offset in units of the box's diagonal from the box's centre, minimising the largest distance
/// to a contour point.
///
/// # Errors
///
/// NLopt's exceptions other than a forced stop.
pub fn minimize_circle(sh: &ExPolygon) -> Result<Circle, ArrangeError> {
    let ctr = &sh.contour.points;
    if ctr.is_empty() {
        return Ok(Circle::new(Point::new(0, 0), 0.0));
    }
    let bb = bounding_box(sh);
    let capprx = bb.center();
    let rapprx = distance(bb.min, bb.max);
    let stopcr = StopCriteria {
        max_iterations: 30,
        relative_score_difference: 1e-3,
        ..StopCriteria::default()
    };
    let at = |xf: f64, yf: f64| {
        Point::new(
            orca_crt::f64_to_i64(orca_crt::round(capprx.x as f64 + rapprx * xf)),
            orca_crt::f64_to_i64(orca_crt::round(capprx.y as f64 + rapprx * yf)),
        )
    };
    let mut dists = vec![0.0; ctr.len()];
    let result = optimize_min(
        &stopcr,
        &mut |x: &[f64]| {
            let centr = at(x[0], x[1]);
            for (i, &v) in ctr.iter().enumerate() {
                dists[i] = distance(v, centr);
            }
            Ok(dists[orca_stl::max_element_f64(&dists)])
        },
        [0.0, 0.0],
        [(-1.0, 1.0), (-1.0, 1.0)],
    )?;
    Ok(Circle::new(
        at(result.optimum[0], result.optimum[1]),
        result.score,
    ))
}

/// `PlacerBoilerplate::PackResult`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PackResult {
    /// Whether an item was placed (`item_ptr_ != nullptr`).
    pub packed: bool,
    /// `move_`.
    pub move_: Point,
    /// `rot_`.
    pub rot: f64,
    /// `overfit_`.
    pub overfit: f64,
    /// `score_`.
    pub score: f64,
}

impl Default for PackResult {
    /// `PackResult()`: nothing placed, overfit 1, score -1.11.
    fn default() -> PackResult {
        PackResult::unfit(1.0)
    }
}

impl PackResult {
    /// `PackResult(overfit)`.
    #[must_use]
    pub fn unfit(overfit: f64) -> PackResult {
        PackResult {
            packed: false,
            move_: Point::new(0, 0),
            rot: 0.0,
            overfit,
            score: -1.11,
        }
    }
}

/// `_NofitPolyPlacer<ExPolygon, Bin>`.
#[derive(Clone, Debug)]
pub struct NfpPlacer<F> {
    /// `bin_`.
    pub bin: Bin,
    /// `items_`: arena indices.
    pub items: Vec<usize>,
    /// `config_`.
    pub config: PlacerConfig<F>,
    norm: f64,
    /// `score_`: the sum of the accepted items' scores.
    pub score: f64,
    /// `plate_id`.
    pub plate_id: i32,
    merged_pile: ExPolygons,
}

/// One task of the parallel corner optimisation.
struct CornerTask {
    pos: f64,
    result: OptResult<1>,
}

/// One task of the parallel no-fit polygons.
struct NfpTask<'a> {
    fixed: &'a ExPolygon,
    touch: Point,
    out: Option<Result<ExPolygon, ArrangeError>>,
}

impl<F: Clone + Send + Sync> NfpPlacer<F> {
    /// `_NofitPolyPlacer(bin)` then `configure(config)`: `norm_ = sqrt(abs(area(bin)))`.
    #[must_use]
    pub fn new(bin: Bin, config: PlacerConfig<F>) -> NfpPlacer<F> {
        let norm = orca_crt::sqrt(orca_crt::fabs(bin.area()));
        NfpPlacer {
            bin,
            items: Vec::with_capacity(50),
            config,
            norm,
            score: 0.0,
            plate_id: 0,
            merged_pile: ExPolygons::new(),
        }
    }

    /// The norm of the bin (`norm_`).
    #[must_use]
    pub fn norm(&self) -> f64 {
        self.norm
    }

    /// `getPackedSize()`: the items that are not fixed.
    #[must_use]
    pub fn packed_size(&self, arena: &[Item]) -> usize {
        self.items.iter().filter(|&&i| !arena[i].is_fixed()).count()
    }

    /// `overfit(chull, bin)` of a shape: by the box, the bounding circle or the containment.
    fn overfit_shape(chull: &ExPolygon, bin: &Bin) -> Result<f64, ArrangeError> {
        match bin {
            Bin::Box(b) => Ok(overfit_box_box(&bounding_box(chull), b)),
            Bin::Circle(c) => overfit_shape_circle(chull, c),
            Bin::Shape(s) => Ok(overfit_shape_shape(chull, s)),
        }
    }

    /// `preload(packeditems)`: the items added, then `on_preload`.
    pub fn preload<H: Hooks<ObjFn = F>>(&mut self, arena: &[Item], packed: &[usize], hooks: &H) {
        self.items.extend_from_slice(packed);
        hooks.on_preload(arena, packed, &mut self.config);
    }

    /// `accept(r)`: the item takes the result's place and is added; the pile is merged again.
    pub fn accept(&mut self, arena: &mut [Item], item: usize, r: &PackResult) {
        if r.packed {
            arena[item].set_translation(r.move_);
            arena[item].set_rotation(r.rot);
            self.items.push(item);
            self.merged_pile = merge_with(&self.merged_pile, arena[item].transformed_shape());
            self.score += r.score;
        }
    }

    /// `clearItems()`: `finalAlign`, then no items.
    ///
    /// # Errors
    ///
    /// The exceptions of `finalAlign`.
    pub fn clear_items(&mut self, arena: &mut [Item]) -> Result<(), ArrangeError> {
        self.final_align(arena)?;
        self.items.clear();
        Ok(())
    }

    /// `clearItems(clearFunc)`: `finalAlign`, then only the items the function keeps.
    ///
    /// # Errors
    ///
    /// The exceptions of `finalAlign`.
    pub fn clear_items_if(
        &mut self,
        arena: &mut [Item],
        clear: impl Fn(&Item) -> bool,
    ) -> Result<(), ArrangeError> {
        self.final_align(arena)?;
        self.items.retain(|&i| !clear(&arena[i]));
        Ok(())
    }

    /// `setInitialPosition(item)`: the item's box aligned by the starting point in the bin's box
    /// shrunk by 10.
    fn set_initial_position(&self, item: &mut Item) {
        let bb = item.bounding_box();
        let mut bbin = self.bin.bounding_box();
        let shrink = Point::new(10, 10);
        bbin.max -= shrink;
        bbin.min += shrink;
        let (ci, cb) = corner_pair(self.config.starting_point, &bb, &bbin, Point::new(0, 0));
        item.translate(cb - ci);
    }

    /// `placeOutsideOfBin(item)`: the item's bottom-right corner on the bin box's top-right one.
    fn place_outside_of_bin(&self, item: &mut Item) {
        let bb = item.bounding_box();
        let binbb = self.bin.bounding_box();
        let v = Point::new(bb.max.x, bb.min.y);
        let dx: Coord = binbb.max.x.wrapping_sub(v.x);
        let dy: Coord = binbb.max.y.wrapping_sub(v.y);
        item.translate(Point::new(dx, dy));
    }

    /// `calcnfp(trsh, bed)`: the box's inner fit region less each packed item's no-fit polygon.
    fn calcnfp(&self, arena: &[Item], trsh: &Item, bed: &Box) -> Result<ExPolygons, ArrangeError> {
        let orb = trsh.transformed_shape();
        let (lmb, rmt) = (trsh.leftmost_bottom_vertex(), trsh.rightmost_top_vertex());
        let mut tasks: Vec<NfpTask<'_>> = self
            .items
            .iter()
            .map(|&i| NfpTask {
                fixed: arena[i].transformed_shape(),
                touch: arena[i].rightmost_top_vertex(),
                out: None,
            })
            .collect();
        run_nfp_tasks(&mut tasks, orb, lmb, rmt);
        let mut nfps = ExPolygons::with_capacity(tasks.len());
        for t in tasks {
            nfps.push(t.out.expect("task ran")?);
        }
        let inner = nfp_inner_rect_bed(bed, orb).0;
        Ok(subtract(&[inner], &nfps))
    }

    /// `trypack(item, remaining)`.
    ///
    /// # Errors
    ///
    /// The exceptions of the no-fit polygons, the object function and the circle fits; NLopt's
    /// own exceptions in a corner optimisation are caught, leaving that corner's result zero.
    pub fn trypack<H: Hooks<ObjFn = F>>(
        &mut self,
        arena: &[Item],
        item: &mut Item,
        remaining: &[usize],
        hooks: &mut H,
    ) -> Result<PackResult, ArrangeError> {
        let mut can_pack = false;
        let mut best_overfit = f64::MAX;
        let mut global_score = f64::MAX;
        let initial_tr = item.translation();
        let initial_rot = item.rotation();
        let mut final_tr = Point::new(0, 0);
        let mut final_rot = initial_rot;
        let binbb = self.bin.bounding_box();

        let first_object = self
            .items
            .iter()
            .all(|&i| arena[i].is_virt_object && !arena[i].is_wipe_tower);
        let overlap_with_virt_object = |placer: &Self, item: &Item| -> Result<f64, ArrangeError> {
            if placer.items.is_empty() {
                return Ok(0.0);
            }
            let nfps = placer.calcnfp(arena, item, &binbb)?;
            let v = item.reference_vertex();
            for nfp in &nfps {
                if within_point(v, nfp) || touches_point(v, nfp) {
                    return Ok(0.0);
                }
            }
            Ok(1.0)
        };

        if first_object {
            self.set_initial_position(item);
            let mut best_tr = item.translation();
            let mut best_rot = item.rotation();
            best_overfit = Self::overfit_shape(item.transformed_shape(), &self.bin)?
                + overlap_with_virt_object(self, item)?;
            for &rot in &self.config.rotations {
                item.set_translation(initial_tr);
                item.set_rotation(initial_rot + rot);
                self.set_initial_position(item);
                let of = Self::overfit_shape(item.transformed_shape(), &self.bin)?;
                if of + overlap_with_virt_object(self, item)? < best_overfit {
                    best_overfit = of;
                    best_tr = item.translation();
                    best_rot = item.rotation();
                }
            }
            can_pack = best_overfit <= 0.0;
            if can_pack {
                global_score = 0.2;
            }
            item.set_rotation(best_rot);
            item.set_translation(best_tr);
        }

        if !can_pack {
            let mut merged_pile = self.merged_pile.clone();
            let rotations = self.config.rotations.clone();
            for rot in rotations {
                item.set_translation(initial_tr);
                item.set_rotation(initial_rot + rot);
                let _ = item.bounding_box();
                self.place_outside_of_bin(item);
                let nfps = self.calcnfp(arena, item, &binbb)?;
                let iv = item.reference_vertex();
                let startpos = item.translation();
                let ecache: Vec<EdgeCache> = nfps
                    .iter()
                    .map(|nfp| {
                        let mut e = EdgeCache::new(nfp);
                        e.set_accuracy(f64::from(self.config.accuracy));
                        e
                    })
                    .collect();
                let mut optimum = Optimum {
                    relpos: 0.0,
                    nfpidx: 0,
                    hidx: -1,
                };
                let mut best_score = f64::MAX;
                hooks.before_packing(arena, &merged_pile, &self.items, remaining);
                let accuracy = self.config.accuracy;
                for ch in 0..ecache.len() {
                    let cache = &ecache[ch];
                    let corners = cache.corners();
                    let results = optimise_corners(
                        &corners,
                        &*hooks,
                        arena,
                        &self.config.object_function,
                        item,
                        &ecache,
                        Optimum {
                            relpos: 0.0,
                            nfpidx: ch,
                            hidx: -1,
                        },
                        iv,
                        startpos,
                        accuracy,
                    )?;
                    let mr =
                        results[orca_stl::min_element_by(&results, |r1, r2| r1.score < r2.score)];
                    if mr.score < best_score {
                        let o = Optimum {
                            relpos: mr.optimum[0],
                            nfpidx: ch,
                            hidx: -1,
                        };
                        let miss =
                            self.boundary_check(&ecache, &o, item, iv, startpos, &mut merged_pile)?;
                        if miss <= 0.0 {
                            best_score = mr.score;
                            optimum = o;
                        } else {
                            best_overfit = orca_stl::min(miss, best_overfit);
                        }
                    }
                    for hidx in 0..cache.hole_count() {
                        let corners = cache.hole_corners(hidx);
                        let results = optimise_corners(
                            &corners,
                            &*hooks,
                            arena,
                            &self.config.object_function,
                            item,
                            &ecache,
                            Optimum {
                                relpos: 0.0,
                                nfpidx: ch,
                                hidx: hidx as i64,
                            },
                            iv,
                            startpos,
                            accuracy,
                        )?;
                        let hmr = results
                            [orca_stl::min_element_by(&results, |r1, r2| r1.score < r2.score)];
                        if hmr.score < best_score {
                            let o = Optimum {
                                relpos: hmr.optimum[0],
                                nfpidx: ch,
                                hidx: hidx as i64,
                            };
                            let miss = self.boundary_check(
                                &ecache,
                                &o,
                                item,
                                iv,
                                startpos,
                                &mut merged_pile,
                            )?;
                            if miss <= 0.0 {
                                best_score = hmr.score;
                                optimum = o;
                            } else {
                                best_overfit = orca_stl::min(miss, best_overfit);
                            }
                        }
                    }
                }
                if best_score < global_score {
                    let d = (nfp_point(&ecache, &optimum) - iv) + startpos;
                    final_tr = d;
                    final_rot = initial_rot + rot;
                    can_pack = true;
                    global_score = best_score;
                }
            }
            item.set_translation(final_tr);
            item.set_rotation(final_rot);
        }

        if can_pack {
            Ok(PackResult {
                packed: true,
                move_: item.translation(),
                rot: item.rotation(),
                overfit: 1.0,
                score: global_score,
            })
        } else {
            Ok(PackResult::unfit(best_overfit))
        }
    }

    /// The `boundaryCheck` lambda: the item moved to the point, the hull of the pile with it,
    /// and that hull's overfit (or containment, for `DONT_ALIGN`).
    fn boundary_check(
        &self,
        ecache: &[EdgeCache],
        o: &Optimum,
        item: &mut Item,
        iv: Point,
        startpos: Point,
        merged_pile: &mut ExPolygons,
    ) -> Result<f64, ArrangeError> {
        let v = nfp_point(ecache, o);
        let d = (v - iv) + startpos;
        item.set_translation(d);
        merged_pile.push(item.transformed_shape().clone());
        let chull = convex_hull_shapes(merged_pile);
        merged_pile.pop();
        if self.config.alignment == Alignment::DontAlign {
            Ok(if is_inside_shape_bin(&chull, &self.bin) {
                -1.0
            } else {
                1.0
            })
        } else {
            Self::overfit_shape(&chull, &self.bin)
        }
    }

    /// `finalAlign(bin_)`.
    ///
    /// # Errors
    ///
    /// The exceptions of the no-fit polygons and the circle fit.
    pub fn final_align(&self, arena: &mut [Item]) -> Result<(), ArrangeError> {
        match &self.bin {
            Bin::Circle(c) => {
                let c = *c;
                self.final_align_circle(arena, &c)
            }
            other => {
                let bbin = other.bounding_box();
                self.final_align_box(arena, bbin)
            }
        }
    }

    /// `finalAlign(_Circle)`: every item moved so the pile's bounding circle is the bin's centre.
    fn final_align_circle(&self, arena: &mut [Item], cbin: &Circle) -> Result<(), ArrangeError> {
        if self.items.is_empty() || self.config.alignment == Alignment::DontAlign {
            return Ok(());
        }
        let m: ExPolygons = self
            .items
            .iter()
            .map(|&i| arena[i].transformed_shape().clone())
            .collect();
        let c = minimize_circle(&convex_hull_shapes(&m))?;
        let d = cbin.center - c.center;
        for &i in &self.items {
            arena[i].translate(d);
        }
        Ok(())
    }

    /// `finalAlign(Box)`: the real items' box aligned in the bin's box, the shift corrected so the
    /// pile's hull stays in the fixed items' no-fit region.
    fn final_align_box(&self, arena: &mut [Item], bbin: Box) -> Result<(), ArrangeError> {
        if self.items.is_empty() || self.config.alignment == Alignment::DontAlign {
            return Ok(());
        }
        let mut bb = Box::default();
        for &i in &self.items {
            if !arena[i].is_virt_object {
                bb = bounding_box2(&arena[i].bounding_box(), &bb);
            }
        }
        let (ci, cb) = corner_pair(
            self.config.alignment,
            &bb,
            &bbin,
            self.config.best_object_pos,
        );
        let mut d = cb - ci;
        let mut objs = ExPolygons::new();
        let mut excludes = ExPolygons::new();
        for &i in &self.items {
            if !arena[i].is_fixed() {
                objs.push(arena[i].transformed_shape().clone());
            }
        }
        if objs.is_empty() {
            return Ok(());
        }
        {
            let objs_convex_hull = convex_hull_shapes(&objs);
            for &i in &self.items {
                if arena[i].is_fixed() {
                    excludes.push(arena[i].transformed_shape().clone());
                }
            }
            let nfps = calcnfp_shapes(&objs_convex_hull, &excludes, &bbin)?;
            if nfps.is_empty() {
                return Ok(());
            }
            let hull_item = Item::new(objs_convex_hull.clone());
            let hull_ref = hull_item.reference_vertex();
            let diff = hull_ref - bounding_box(&objs_convex_hull).center();
            let ref_aligned = cb + diff;
            let ok = nfps.iter().any(|nfp| within_point(ref_aligned, nfp));
            if !ok {
                let ref_projected = projection_onto(&nfps, ref_aligned);
                d += ref_projected - ref_aligned;
            }
        }
        for &i in &self.items {
            if !arena[i].is_virt_object {
                arena[i].translate(d);
            }
        }
        Ok(())
    }
}

/// `isInside(chull, bin)` for each bin type.
fn is_inside_shape_bin(chull: &ExPolygon, bin: &Bin) -> bool {
    match bin {
        Bin::Box(b) => is_inside_shape_box(chull, b),
        Bin::Circle(c) => is_inside_shape_circle(chull, c),
        Bin::Shape(s) => within_shape_shape(chull, s),
    }
}

/// `overfit(Box, Box)`: how much wider and taller the box is than the bin, in `double`.
#[must_use]
pub fn overfit_box_box(bb: &Box, bin: &Box) -> f64 {
    let wdiff = bb.width().wrapping_sub(bin.width());
    let hdiff = bb.height().wrapping_sub(bin.height());
    let mut diff = 0.0;
    if wdiff > 0 {
        diff += wdiff as f64;
    }
    if hdiff > 0 {
        diff += hdiff as f64;
    }
    diff
}

/// `overfit(chull, bin)` of a shape and a circle: the radius of the shape's bounding circle
/// (`minimizeCircle`) less the bin's.
///
/// # Errors
///
/// NLopt's exceptions other than a forced stop.
pub fn overfit_shape_circle(chull: &ExPolygon, bin: &Circle) -> Result<f64, ArrangeError> {
    Ok(minimize_circle(chull)?.radius - bin.radius)
}

/// `overfit(chull, bin)` of two shapes: -1 when the hull, moved to the bin box's centre, is
/// within the bin, else 1.
#[must_use]
pub fn overfit_shape_shape(chull: &ExPolygon, bin: &ExPolygon) -> f64 {
    let bbch = bounding_box(chull);
    let bbin = bounding_box(bin);
    let d = bbin.center() - bbch.center();
    let mut cpy = chull.clone();
    translate_shape(&mut cpy, d);
    if within_shape_shape(&cpy, bin) {
        -1.0
    } else {
        1.0
    }
}

/// The corners an alignment pairs: the item box's and the bin box's (`USER_DEFINED`: the item
/// box's centre and the given position; `DONT_ALIGN`: the origin twice).
fn corner_pair(a: Alignment, bb: &Box, bbin: &Box, user: Point) -> (Point, Point) {
    match a {
        Alignment::Center => (bb.center(), bbin.center()),
        Alignment::BottomLeft => (bb.min, bbin.min),
        Alignment::BottomRight => (
            Point::new(bb.max.x, bb.min.y),
            Point::new(bbin.max.x, bbin.min.y),
        ),
        Alignment::TopLeft => (
            Point::new(bb.min.x, bb.max.y),
            Point::new(bbin.min.x, bbin.max.y),
        ),
        Alignment::TopRight => (bb.max, bbin.max),
        Alignment::UserDefined => (bb.center(), user),
        Alignment::DontAlign => (Point::new(0, 0), Point::new(0, 0)),
    }
}

/// The placer's `Optimum`: a relative position on a no-fit polygon's contour or hole.
#[derive(Clone, Copy, Debug)]
struct Optimum {
    relpos: f64,
    nfpidx: usize,
    hidx: i64,
}

/// The `getNfpPoint` lambda.
fn nfp_point(ecache: &[EdgeCache], o: &Optimum) -> Point {
    if o.hidx < 0 {
        ecache[o.nfpidx].coords(o.relpos)
    } else {
        ecache[o.nfpidx].hole_coords(o.hidx as usize, o.relpos)
    }
}

/// The corner optimisations of one contour or hole, in parallel: from each corner, subplex on
/// the position (bounds 0 and 1) of a copy of the item, scored by the object function. A task
/// whose NLopt run throws keeps the zero result.
#[allow(clippy::too_many_arguments)]
fn optimise_corners<H: Hooks>(
    corners: &[f64],
    hooks: &H,
    arena: &[Item],
    objfn: &H::ObjFn,
    item: &Item,
    ecache: &[EdgeCache],
    base: Optimum,
    iv: Point,
    startpos: Point,
    accuracy: f32,
) -> Result<Vec<OptResult<1>>, ArrangeError> {
    let mut tasks: Vec<CornerTask> = corners
        .iter()
        .map(|&pos| CornerTask {
            pos,
            result: OptResult::default(),
        })
        .collect();
    let stopcr = placer_stop_criteria(accuracy);
    orca_tbb::parallel_for_each_mut(&mut tasks, |t| {
        let mut itemcpy = item.clone();
        let mut ofn = |x: &[f64]| -> Result<f64, ArrangeError> {
            let o = Optimum {
                relpos: x[0],
                ..base
            };
            let v = nfp_point(ecache, &o);
            let d = (v - iv) + startpos;
            itemcpy.set_translation(d);
            hooks.object_function(arena, objfn, &itemcpy)
        };
        if let Ok(r) = optimize_min(&stopcr, &mut ofn, [t.pos], [(0.0, 1.0)]) {
            t.result = r;
        }
    });
    Ok(tasks.into_iter().map(|t| t.result).collect())
}

/// The parallel no-fit polygons of `calcnfp`: each stationary shape against the orbiter, moved
/// into place.
fn run_nfp_tasks(tasks: &mut [NfpTask<'_>], orb: &ExPolygon, lmb: Point, rmt: Point) {
    orca_tbb::parallel_for_each_mut(tasks, |t| {
        t.out = Some(
            nfp_convex_only(t.fixed, orb)
                .map_err(ArrangeError::Exact)
                .map(|mut r| {
                    correct_nfp_position(&mut r, t.touch, lmb, rmt);
                    r.0
                }),
        );
    });
}

/// `calcnfp(sliding, stationarys, bed)`: the box's inner fit region less the no-fit polygon of
/// each stationary shape.
fn calcnfp_shapes(
    sliding: &ExPolygon,
    stationarys: &[ExPolygon],
    bed: &Box,
) -> Result<ExPolygons, ArrangeError> {
    let sliding_item = Item::new(sliding.clone());
    // correctNfpPosition reads the orbiter's vertices for each stationary shape, so without one
    // they are never read: an empty sliding shape (the hull of a lone triangle) goes on to the
    // inner fit region and Clipper's range test, as in the C++.
    let (lmb, rmt) = if stationarys.is_empty() {
        (Point::new(0, 0), Point::new(0, 0))
    } else {
        (
            sliding_item.leftmost_bottom_vertex(),
            sliding_item.rightmost_top_vertex(),
        )
    };
    let mut tasks: Vec<NfpTask<'_>> = stationarys
        .iter()
        .map(|s| NfpTask {
            fixed: s,
            touch: rightmost_up_vertex(s),
            out: None,
        })
        .collect();
    run_nfp_tasks(&mut tasks, sliding, lmb, rmt);
    let mut nfps = ExPolygons::with_capacity(tasks.len());
    for t in tasks {
        nfps.push(t.out.expect("task ran")?);
    }
    let inner = nfp_inner_rect_bed(bed, sliding).0;
    Ok(subtract(&[inner], &nfps))
}
