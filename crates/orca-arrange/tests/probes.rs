// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The probe gate for orca-arrange: `arrange.sop1`'s records but the exact-arithmetic ones, which
//! orca-exact replays (`parity/SOP1.md`, "arrange.sop1"), replayed against the crate bit for bit
//! (added 2026-11-07):
//!
//! - NLOP: NLopt 2.5.0's `LN_NELDERMEAD` and `LN_SBPLX` through `nlopt::opt`: bounds, fixed
//!   dimensions, every stopping criterion, initial steps, maximising, forced stops, NaN and
//!   infinite objectives; each evaluation, the result or exception, the final point and value;
//! - NOPT: libnest2d's subplex optimizer over one to three variables, with objectives that throw
//!   each kind of exception;
//! - RTRE: `Arrange.cpp`'s R*-tree: inserts, intersects queries (sorted by index), bounds, size,
//!   clear;
//! - NFPC: `nfpConvexOnly` with the rational edge sort, and `nfpInnerRectBed`;
//! - NGEO: `_Item`, the convex hulls, `minimizeCircle`, the `isInside`, `within` and `touches`
//!   predicates, the placer's `overfit` functions and `EdgeCache`;
//! - ARNG: whole `arrange` runs on every kind of bed, with fixed items, excluded regions,
//!   inflation, print by object, the CLI's parameters, a stop condition and the callbacks.
//!
//! The committed subset (`parity/probes/`) runs by default; `SKETCH_ORCA_PROBES_DEEP=<folder>`
//! replays the full set from that folder (`<work>/parity/probes`).

use std::fmt::{self, Debug, Formatter};
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::Once;

use orca_arrange::ArrangeError;
use orca_arrange::arrange::{
    ArrangeCallbacks, ArrangeParams, ArrangePolygon, Bed, CircleBed, InfiniteBed, arrange,
    arrange_on_points,
};
use orca_arrange::bg;
use orca_arrange::geometry::{self as g, Box, Circle};
use orca_arrange::item::Item;
use orca_arrange::nfp::{nfp_convex_only, nfp_inner_rect_bed};
use orca_arrange::nlopt::{Algorithm, NloptException, NloptResult, Opt, cxx_optimize};
use orca_arrange::optimizer::{StopCriteria, optimize_min};
use orca_arrange::placer::{
    EdgeCache, minimize_circle, overfit_box_box, overfit_shape_circle, overfit_shape_shape,
};
use orca_arrange::rtree::SpatIndex;
use orca_geom::bounding_box::BoundingBox;
use orca_geom::clipper::ClipperException;
use orca_geom::expolygon::ExPolygon;
use orca_geom::point::{Point, Points};
use orca_geom::polygon::Polygon;
use orca_probe::{Mismatches, ProbeFile, Reader, load_probe};

fn file() -> ProbeFile {
    load_probe("arrange").unwrap_or_else(|e| panic!("{e}"))
}

/// The kinds of `arrange.sop1` that orca-exact's `tests/probes.rs` replays.
const EXACT_KINDS: [&str; 2] = ["XI28", "XRAT"];
/// The kinds this file replays.
const ARRANGE_KINDS: [&str; 6] = ["NLOP", "NOPT", "RTRE", "NFPC", "NGEO", "ARNG"];

/// An `f64` compared by its bits, so that a sign of zero or a NaN counts.
#[derive(Clone, Copy)]
struct F(f64);

impl PartialEq for F {
    fn eq(&self, o: &Self) -> bool {
        self.0.to_bits() == o.0.to_bits()
    }
}

impl Debug for F {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

fn fs(v: &[f64]) -> Vec<F> {
    v.iter().copied().map(F).collect()
}

/// Runs `f`, turning a thrown [`ClipperException`] into its `what()`; other panics go on.
fn catch<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    static QUIET: Once = Once::new();
    QUIET.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(std::boxed::Box::new(move |info| {
            if info.payload().downcast_ref::<ClipperException>().is_none() {
                default(info);
            }
        }));
    });
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(v) => Ok(v),
        Err(payload) => match payload.downcast::<ClipperException>() {
            Ok(e) => Err(e.what().to_owned()),
            Err(other) => resume_unwind(other),
        },
    }
}

/// An exception's kind and `what()` as the records hold them (nlopt.cpp's `run_catching`): 1
/// `std::runtime_error`, 2 `std::invalid_argument`, 3 `std::bad_alloc` (MSVC's "bad allocation"),
/// 4 `nlopt::roundoff_limited`, 5 `nlopt::forced_stop`, 9 any other.
fn kind_of(e: &NloptException) -> (u8, String) {
    match e {
        NloptException::RuntimeError(m) => (1, m.clone()),
        NloptException::InvalidArgument(m) => (2, m.clone()),
        NloptException::BadAlloc => (3, "bad allocation".to_owned()),
        NloptException::RoundoffLimited => (4, "nlopt roundoff-limited".to_owned()),
        NloptException::ForcedStop => (5, "nlopt forced stop".to_owned()),
    }
}

/// The kind and `what()` of an arrangement's exception.
fn arrange_kind(e: &ArrangeError) -> (u8, String) {
    match e {
        ArrangeError::Nlopt(n) => kind_of(n),
        other => (9, what(other)),
    }
}

/// The C++ `what()` of an arrangement's exception.
fn what(e: &ArrangeError) -> String {
    match e {
        ArrangeError::Exact(x) => x.what().to_owned(),
        ArrangeError::Nlopt(n) => kind_of(n).1,
        other => format!("(no C++ exception: {other})"),
    }
}

/// One record's reader, the mismatch list and the record's label.
struct Case<'a, 'm> {
    r: Reader<'a>,
    m: &'m mut Mismatches,
    label: String,
}

impl Case<'_, '_> {
    fn u8(&mut self) -> u8 {
        self.r.u8().unwrap()
    }
    fn u32(&mut self) -> u32 {
        self.r.u32().unwrap()
    }
    fn u64(&mut self) -> u64 {
        self.r.u64().unwrap()
    }
    fn i32(&mut self) -> i32 {
        self.r.i32().unwrap()
    }
    fn i64(&mut self) -> i64 {
        self.r.i64().unwrap()
    }
    fn f32(&mut self) -> f32 {
        self.r.f32().unwrap()
    }
    fn f64(&mut self) -> f64 {
        self.r.f64().unwrap()
    }
    fn f64s(&mut self, n: usize) -> Vec<f64> {
        (0..n).map(|_| self.f64()).collect()
    }
    fn bool(&mut self) -> bool {
        self.r.bool().unwrap()
    }
    fn str(&mut self) -> String {
        self.r.str().unwrap()
    }
    fn point(&mut self) -> Point {
        let x = self.i64();
        Point::new(x, self.i64())
    }
    fn points(&mut self) -> Points {
        let n = self.u32();
        (0..n).map(|_| self.point()).collect()
    }
    fn expolygon(&mut self) -> ExPolygon {
        let contour = Polygon::new(self.points());
        let n = self.u32();
        ExPolygon {
            contour,
            holes: (0..n).map(|_| Polygon::new(self.points())).collect(),
        }
    }
    /// A box as the probe writes it: its corners (a box of two points is defined).
    fn nbox(&mut self) -> Box {
        let min = self.point();
        Box::new(min, self.point())
    }

    /// Reads the engine's value of each kind and compares `got` with it.
    fn is_bool(&mut self, what: &str, got: bool) {
        let want = self.bool();
        self.same(what, &got, &want);
    }
    fn is_f64(&mut self, what: &str, got: f64) {
        let want = F(self.f64());
        self.same(what, &F(got), &want);
    }
    fn is_point(&mut self, what: &str, got: Point) {
        let want = self.point();
        self.same(what, &got, &want);
    }
    fn is_expolygon(&mut self, what: &str, got: &ExPolygon) {
        let want = self.expolygon();
        self.same(what, got, &want);
    }
    fn is_box(&mut self, what: &str, got: &Box) {
        let want = self.nbox();
        self.same(what, &(got.min, got.max), &(want.min, want.max));
    }

    /// Compares one result with the C++ engine's.
    fn same<T: PartialEq + Debug>(&mut self, what: &str, got: &T, want: &T) {
        let label = &self.label;
        self.m.check(got == want, || {
            let cut = |s: String| s.chars().take(900).collect::<String>();
            format!(
                "{label}: {what}: {} instead of {}",
                cut(format!("{got:?}")),
                cut(format!("{want:?}"))
            )
        });
    }
}

/// Replays every record of `kind` with `f`; a record that matched must have been read to its end.
fn replay(kind: &str, f: impl Fn(&mut Case)) {
    let file = file();
    let mut m = Mismatches::default();
    for (i, rec) in file.of_kind(kind).enumerate() {
        let before = m.count;
        let mut c = Case {
            r: rec.reader(),
            m: &mut m,
            label: format!("{kind} {i}"),
        };
        f(&mut c);
        if c.m.count == before {
            c.r.finish(&c.label).unwrap();
        }
    }
    m.assert_none(kind);
}

// --- NLOP and NOPT -------------------------------------------------------------------------------

/// nlopt.cpp's objectives, by id, over `x` with the parameters `c`.
fn objective(id: u8, x: &[f64], c: &[f64; 4]) -> f64 {
    assert!(x.len() <= c.len(), "the C++ reads c[i] for each x[i]");
    let sphere = || {
        let mut s = 0.0;
        for (xi, ci) in x.iter().zip(c) {
            let d = xi - ci;
            s += d * d;
        }
        s
    };
    match id {
        0 => sphere(),
        1 => {
            let mut s = 0.0;
            for w in x.windows(2) {
                let a = w[1] - w[0] * w[0];
                let b = 1.0 - w[0];
                s += 100.0 * a * a + b * b;
            }
            s + c[0]
        }
        2 => {
            let mut s = 0.0;
            for (i, (xi, ci)) in x.iter().zip(c).enumerate() {
                s += (xi - ci).abs() * (i + 1) as f64;
            }
            s
        }
        3 => {
            if x[0] > c[1] {
                f64::NAN
            } else {
                sphere()
            }
        }
        4 => {
            if x[0] < c[1] {
                f64::INFINITY
            } else {
                sphere()
            }
        }
        _ => (if x[0] > c[0] { 1.0 } else { 0.0 }) + sphere() * 1e-3,
    }
}

/// nlopt-in.hpp's `mythrow` of a result code.
fn mythrow(opt: &Opt, r: NloptResult) -> Option<NloptException> {
    match r {
        NloptResult::Failure => Some(NloptException::RuntimeError(
            opt.errmsg().unwrap_or("nlopt failure").to_owned(),
        )),
        NloptResult::OutOfMemory => Some(NloptException::BadAlloc),
        NloptResult::InvalidArgs => Some(NloptException::InvalidArgument(
            opt.errmsg().unwrap_or("nlopt invalid argument").to_owned(),
        )),
        NloptResult::RoundoffLimited => Some(NloptException::RoundoffLimited),
        NloptResult::ForcedStop => Some(NloptException::ForcedStop),
        _ => None,
    }
}

fn nlop(c: &mut Case) {
    let alg = c.u8();
    let n = c.u32() as usize;
    let x0 = c.f64s(n);
    let lb = c.f64s(n);
    let ub = c.f64s(n);
    let maximize = c.bool();
    let set = c.u8();
    let (stopval, ftol_rel, ftol_abs, xtol_rel, xtol_abs) =
        (c.f64(), c.f64(), c.f64(), c.f64(), c.f64());
    let maxeval = c.i32();
    // The case's steps are written when they are set, and only then.
    let dx = if set & 64 != 0 { c.f64s(n) } else { Vec::new() };
    let obj = c.u8();
    let cc = [c.f64(), c.f64(), c.f64(), c.f64()];
    let force_at = c.i32();

    let mut opt = Opt::create(
        if alg == 0 {
            Algorithm::LnNelderMead
        } else {
            Algorithm::LnSbplx
        },
        n,
    );
    opt.set_lower_bounds(&lb);
    opt.set_upper_bounds(&ub);
    if set & 1 != 0 {
        opt.set_stopval(stopval);
    }
    if set & 2 != 0 {
        opt.set_ftol_rel(ftol_rel);
    }
    if set & 4 != 0 {
        opt.set_ftol_abs(ftol_abs);
    }
    if set & 8 != 0 {
        opt.set_xtol_rel(xtol_rel);
    }
    if set & 16 != 0 {
        opt.set_xtol_abs1(xtol_abs);
    }
    if set & 32 != 0 {
        opt.set_maxeval(maxeval);
    }
    if set & 64 != 0 {
        let r = opt.set_initial_step(Some(&dx));
        let got = mythrow(&opt, r).map(|e| kind_of(&e));
        let want = if c.bool() {
            Some((c.u8(), c.str()))
        } else {
            None
        };
        let stop = got.is_some() || want.is_some();
        c.same("set_initial_step", &got, &want);
        if stop {
            return;
        }
    } else {
        c.is_bool("set_initial_step threw", false);
    }

    if maximize {
        opt.set_max_objective();
    } else {
        opt.set_min_objective();
    }
    let flag = opt.force_stop_flag();
    let mut evals: Vec<(Vec<F>, F)> = Vec::new();
    let mut f = |x: &[f64]| -> f64 {
        let v = objective(obj, x, &cc);
        evals.push((fs(x), F(v)));
        if evals.len() as i32 - 1 == force_at {
            flag.set(1);
        }
        v
    };
    let mut x = x0;
    let mut opt_f = 0.0;
    let ret = cxx_optimize(&mut opt, &mut f, &mut x, &mut opt_f);

    let ne = c.u32() as usize;
    let want: Vec<(Vec<F>, F)> = (0..ne)
        .map(|_| {
            let xs = fs(&c.f64s(n));
            (xs, F(c.f64()))
        })
        .collect();
    c.same("evaluations", &evals, &want);
    let got = ret.map(|r| r.code()).map_err(|e| kind_of(&e));
    let want = if c.bool() {
        Err((c.u8(), c.str()))
    } else {
        Ok(c.i32())
    };
    c.same("outcome", &got, &want);
    let want_x = fs(&c.f64s(n));
    c.same("final x", &fs(&x), &want_x);
    c.is_f64("opt_f", opt_f);
}

#[test]
fn nlopt_runs_match() {
    replay("NLOP", nlop);
}

fn nopt(c: &mut Case) {
    let n = c.u32() as usize;
    let x0 = c.f64s(n);
    let lb = c.f64s(n);
    let ub = c.f64s(n);
    let stopcr = StopCriteria {
        absolute_score_difference: c.f64(),
        relative_score_difference: c.f64(),
        stop_score: c.f64(),
        max_iterations: c.u32(),
    };
    let obj = c.u8();
    let cc = [c.f64(), c.f64(), c.f64(), c.f64()];
    let throw_at = c.i32();
    let throw_kind = c.u8();

    let mut evals: Vec<(Vec<F>, u8, F)> = Vec::new();
    let mut func = |x: &[f64]| -> Result<f64, ArrangeError> {
        if evals.len() as i32 == throw_at {
            evals.push((fs(x), 1, F(0.0)));
            return Err(ArrangeError::Nlopt(match throw_kind {
                2 => NloptException::InvalidArgument("probe invalid argument".to_owned()),
                3 => NloptException::BadAlloc,
                4 => NloptException::RoundoffLimited,
                5 => NloptException::ForcedStop,
                _ => NloptException::RuntimeError("probe failure".to_owned()),
            }));
        }
        let v = objective(obj, x, &cc);
        evals.push((fs(x), 0, F(v)));
        Ok(v)
    };
    let b = |i: usize| (lb[i], ub[i]);
    let got = match n {
        1 => optimize_min(&stopcr, &mut func, [x0[0]], [b(0)])
            .map(|r| (r.resultcode, fs(&r.optimum), F(r.score))),
        2 => optimize_min(&stopcr, &mut func, [x0[0], x0[1]], [b(0), b(1)])
            .map(|r| (r.resultcode, fs(&r.optimum), F(r.score))),
        _ => optimize_min(
            &stopcr,
            &mut func,
            [x0[0], x0[1], x0[2]],
            [b(0), b(1), b(2)],
        )
        .map(|r| (r.resultcode, fs(&r.optimum), F(r.score))),
    }
    .map_err(|e| arrange_kind(&e));

    let ne = c.u32() as usize;
    let want: Vec<(Vec<F>, u8, F)> = (0..ne)
        .map(|_| {
            let xs = fs(&c.f64s(n));
            let threw = c.u8();
            (xs, threw, F(c.f64()))
        })
        .collect();
    c.same("evaluations", &evals, &want);
    let want = if c.bool() {
        Err((c.u8(), c.str()))
    } else {
        let code = c.i32();
        let optimum = fs(&c.f64s(n));
        Ok((code, optimum, F(c.f64())))
    };
    c.same("outcome", &got, &want);
}

#[test]
fn libnest2d_optimizer_runs_match() {
    replay("NOPT", nopt);
}

// --- RTRE ----------------------------------------------------------------------------------------

fn rtre(c: &mut Case) {
    let ops = c.u32();
    let mut tree = SpatIndex::new();
    for op in 0..ops {
        match c.u8() {
            0 => {
                let b = c.nbox();
                let idx = c.u32();
                tree.insert((b, idx));
            }
            1 => {
                let q = c.nbox();
                let mut got: Vec<((Point, Point), u32)> = tree
                    .query_intersects(&q)
                    .into_iter()
                    .map(|(b, i)| ((b.min, b.max), i))
                    .collect();
                // The probe sorts by index, which is unique.
                orca_stl::sort_by(&mut got, |a, b| a.1 < b.1);
                let n = c.u32();
                let want: Vec<((Point, Point), u32)> = (0..n)
                    .map(|_| {
                        let b = c.nbox();
                        ((b.min, b.max), c.u32())
                    })
                    .collect();
                c.same(&format!("query {op}"), &got, &want);
            }
            2 => {
                c.is_box(&format!("bounds {op}"), &tree.bounds());
                let size = c.u64();
                c.same(&format!("size {op}"), &(tree.len() as u64), &size);
                c.is_bool(&format!("empty {op}"), tree.is_empty());
            }
            _ => tree.clear(),
        }
    }
}

#[test]
fn spatial_index_matches() {
    replay("RTRE", rtre);
}

// --- NFPC ----------------------------------------------------------------------------------------

fn nfpc(c: &mut Case) {
    let sh = ExPolygon::from_points(c.points());
    let other = ExPolygon::from_points(c.points());
    let got = nfp_convex_only(&sh, &other).map_err(|e| e.what().to_owned());
    let want = if c.bool() {
        Err(c.str())
    } else {
        let e = c.expolygon();
        Ok((e, c.point()))
    };
    c.same("nfpConvexOnly", &got, &want);
    let bed = c.nbox();
    let (inner, rp) = nfp_inner_rect_bed(&bed, &other);
    c.is_expolygon("nfpInnerRectBed", &inner);
    c.is_point("nfpInnerRectBed reference", rp);
}

#[test]
fn no_fit_polygons_match() {
    replay("NFPC", nfpc);
}

// --- NGEO ----------------------------------------------------------------------------------------

/// `within_outcome`: `false` and the answer (the predicate throws nothing here).
fn is_within(c: &mut Case, what: &str, got: bool) {
    let want = if c.bool() { Err(c.str()) } else { Ok(c.bool()) };
    c.same(what, &Ok::<bool, String>(got), &want);
}

fn ngeo(c: &mut Case) {
    let pts = c.points();
    let other = ExPolygon::from_points(c.points());
    let simple = c.bool();
    let set_rotation = c.bool();
    let rotation = c.f64();
    let set_translation = c.bool();
    let translation = c.point();
    let set_inflation = c.bool();
    let inflation = c.i64();
    let bx = c.nbox();
    let circle_center = c.point();
    let radius = c.f64();
    let pt = c.point();
    let accuracy = c.f64();
    let nd = c.u32() as usize;
    let distances = c.f64s(nd);

    let mut item = Item::new(ExPolygon::from_points(pts));
    if set_inflation {
        item.set_inflation(inflation);
    }
    if set_rotation {
        item.set_rotation(rotation);
    }
    if set_translation {
        item.set_translation(translation);
    }
    let ts = item.transformed_shape().clone();
    c.is_expolygon("transformed shape", &ts);
    c.is_f64("area", item.area());
    let bb = item.bounding_box();
    c.is_box("bounding box", &bb);
    c.is_point("reference vertex", item.reference_vertex());
    c.is_point("leftmost bottom vertex", item.leftmost_bottom_vertex());

    let hull = ExPolygon::new(Polygon::new(g::convex_hull_points(&ts.contour.points)));
    c.is_expolygon("hull", &hull);
    c.is_expolygon(
        "hull of two",
        &g::convex_hull_shapes(&[ts.clone(), other.clone()]),
    );
    let mc = minimize_circle(&ts).unwrap();
    c.is_point("minimizeCircle centre", mc.center);
    c.is_f64("minimizeCircle radius", mc.radius);

    let circ = Circle::new(circle_center, radius);
    c.is_bool("isInside(shape, box)", g::is_inside_shape_box(&ts, &bx));
    c.is_bool(
        "isInside(shape, circle)",
        g::is_inside_shape_circle(&ts, &circ),
    );
    c.is_bool("isInside(bb, circle)", g::is_inside_box_circle(&bb, &circ));
    c.is_bool("isInside(bb, box)", g::is_inside_box_box(&bb, &bx));
    c.is_bool(
        "isInside(point, circle)",
        g::is_inside_point_circle(pt, &circ),
    );
    c.is_bool("isInside(point, box)", g::is_inside_point_box(pt, &bx));
    c.is_bool("isInside(point, shape)", bg::within_point(pt, &ts));
    c.is_bool("touches(point, shape)", bg::touches_point(pt, &ts));
    is_within(
        c,
        "within(hull, other)",
        bg::within_shape_shape(&hull, &other),
    );
    if simple {
        is_within(
            c,
            "within(shape, other)",
            bg::within_shape_shape(&ts, &other),
        );
    }
    c.is_f64("overfit(bb, box)", overfit_box_box(&bb, &bx));
    c.is_f64(
        "overfit(shape, circle)",
        overfit_shape_circle(&ts, &circ).unwrap(),
    );
    let want = if c.bool() {
        Err(c.str())
    } else {
        Ok(F(c.f64()))
    };
    c.same(
        "overfit(hull, other)",
        &Ok::<F, String>(F(overfit_shape_shape(&hull, &other))),
        &want,
    );

    let mut ec = EdgeCache::new(&ts);
    ec.set_accuracy(accuracy);
    let corners = ec.corners();
    let n = c.u32() as usize;
    let want = fs(&c.f64s(n));
    c.same("corners", &fs(&corners), &want);
    if corners.len() != n {
        return;
    }
    c.is_f64("circumference", ec.circumference());
    for (i, &k) in corners.iter().enumerate() {
        c.is_point(&format!("coords(corner {i})"), ec.coords(k));
    }
    for (i, &d) in distances.iter().enumerate() {
        c.is_point(&format!("coords(distance {i})"), ec.coords(d));
    }
}

#[test]
fn items_hulls_predicates_and_edge_caches_match() {
    replay("NGEO", ngeo);
}

// --- ARNG ----------------------------------------------------------------------------------------

/// One ARNG run's input, as runs.cpp writes it.
struct Run {
    bed: Bed,
    points: Option<Points>,
    params: ArrangeParams,
    items: Vec<ArrangePolygon>,
    excludes: Vec<ArrangePolygon>,
    progress: bool,
    stop_after: i32,
}

/// What arrange gave: whether it threw (and what), each item's translation, rotation (its bits),
/// bed and packing order, and every callback.
#[derive(Debug, PartialEq)]
struct Outcome {
    threw: Option<String>,
    items: Vec<(Point, u64, i32, i32)>,
    progress: Vec<(u32, String)>,
    packed: Vec<(i32, i32)>,
    stops: u32,
}

fn polygon_of(c: &mut Case) -> ArrangePolygon {
    let poly = ExPolygon::from_points(c.points());
    let translation = c.point();
    let rotation = c.f64();
    let inflation = c.i64();
    let bed_idx = c.i32();
    let priority = c.i32();
    let is_virt_object = c.bool();
    let is_wipe_tower = c.bool();
    let ne = c.u32();
    let extrude_ids = (0..ne).map(|_| c.i32()).collect();
    ArrangePolygon {
        poly,
        translation,
        rotation,
        inflation,
        bed_idx,
        priority,
        is_virt_object,
        is_wipe_tower,
        extrude_ids,
        filament_temp_type: c.i32(),
        first_bed_temp: c.i32(),
        print_temp: c.i32(),
        vitrify_temp: c.i32(),
        itemid: c.i32(),
        height: c.f64(),
        name: c.str(),
        ..ArrangePolygon::default()
    }
}

fn polygons_of(c: &mut Case) -> Vec<ArrangePolygon> {
    let n = c.u32();
    (0..n).map(|_| polygon_of(c)).collect()
}

fn run_of(c: &mut Case) -> Run {
    let kind = c.u8();
    let (bed, points) = match kind {
        0 => {
            let pts = c.points();
            (Bed::Infinite(InfiniteBed::default()), Some(pts))
        }
        1 => {
            let a = c.point();
            (Bed::Box(BoundingBox::from_corners(a, c.point())), None)
        }
        2 => {
            let center = c.point();
            (
                Bed::Circle(CircleBed {
                    center,
                    radius: c.f64(),
                }),
                None,
            )
        }
        3 => (Bed::Polygon(Polygon::new(c.points())), None),
        _ => (Bed::Infinite(InfiniteBed { center: c.point() }), None),
    };
    let mut params = ArrangeParams {
        min_obj_distance: c.i64(),
        accuracy: c.f32(),
        parallel: c.bool(),
        allow_rotations: c.bool(),
        do_final_align: c.bool(),
        allow_multi_materials_on_same_plate: c.bool(),
        avoid_extrusion_cali_region: c.bool(),
        is_seq_print: c.bool(),
        align_to_y_axis: c.bool(),
        bed_shrink_x: c.f32(),
        bed_shrink_y: c.f32(),
        brim_skirt_distance: c.f32(),
        clearance_height_to_rod: c.f32(),
        clearance_height_to_lid: c.f32(),
        clearance_radius: c.f32(),
        object_skirt_offset: c.f32(),
        nozzle_height: c.f32(),
        printable_height: c.f32(),
        ..ArrangeParams::default()
    };
    let x = c.f64();
    params.align_center = (x, c.f64());
    params.excluded_regions = polygons_of(c);
    params.nonprefered_regions = polygons_of(c);
    let items = polygons_of(c);
    let excludes = polygons_of(c);
    let progress = c.bool();
    let stop_after = c.i32();
    Run {
        bed,
        points,
        params,
        items,
        excludes,
        progress,
        stop_after,
    }
}

/// Runs arrange on the run as runs.cpp does, with its callbacks.
fn arrange_run(run: &mut Run) -> Outcome {
    let mut progress: Vec<(u32, String)> = Vec::new();
    let mut packed: Vec<(i32, i32)> = Vec::new();
    let mut stops = 0u32;
    let limit = run.stop_after;
    let threw = {
        let mut on_progress = |st: u32, name: &str| progress.push((st, name.to_owned()));
        let mut on_packed = |bed: i32, priority: i32| packed.push((bed, priority));
        let mut stop = || {
            stops += 1;
            stops as i32 > limit
        };
        let mut cb = ArrangeCallbacks::default();
        if run.progress {
            cb.progressind = Some(&mut on_progress);
            cb.on_packed = Some(&mut on_packed);
        }
        if limit >= 0 {
            cb.stopcondition = Some(&mut stop);
        }
        let (items, excludes, params) = (&mut run.items, &run.excludes, &run.params);
        let r = catch(|| match &run.points {
            Some(pts) => arrange_on_points(items, excludes, pts, params, &mut cb),
            None => arrange(items, excludes, &run.bed, params, &mut cb),
        });
        match r {
            Ok(Ok(())) => None,
            Ok(Err(e)) => Some(what(&e)),
            Err(clipper) => Some(clipper),
        }
    };
    Outcome {
        threw,
        items: run
            .items
            .iter()
            .map(|a| (a.translation, a.rotation.to_bits(), a.bed_idx, a.itemid))
            .collect(),
        progress,
        packed,
        stops,
    }
}

fn arng(c: &mut Case) {
    let mut run = run_of(c);
    let got = arrange_run(&mut run);
    let threw = if c.bool() { Some(c.str()) } else { None };
    let items = (0..run.items.len())
        .map(|_| {
            let tr = c.point();
            let rot = c.f64().to_bits();
            let bed = c.i32();
            (tr, rot, bed, c.i32())
        })
        .collect();
    let np = c.u32();
    let progress = (0..np)
        .map(|_| {
            let st = c.u32();
            (st, c.str())
        })
        .collect();
    let nk = c.u32();
    let packed = (0..nk)
        .map(|_| {
            let bed = c.i32();
            (bed, c.i32())
        })
        .collect();
    let want = Outcome {
        threw,
        items,
        progress,
        packed,
        stops: c.u32(),
    };
    c.same("arrange", &got, &want);
}

#[test]
fn arrange_runs_match() {
    replay("ARNG", arng);
}

/// Every kind of the file has a replay here or in orca-exact, and every kind here is recorded.
#[test]
fn every_kind_is_replayed() {
    assert_eq!(orca_arrange::NAME, "orca-arrange");
    let file = file();
    let unknown: Vec<&str> = file
        .records
        .iter()
        .map(|r| r.kind())
        .filter(|k| !EXACT_KINDS.contains(k) && !ARRANGE_KINDS.contains(k))
        .collect();
    assert!(
        unknown.is_empty(),
        "{}: {} records nothing replays ({:?}...)",
        file.path.display(),
        unknown.len(),
        &unknown[..unknown.len().min(4)]
    );
    for kind in ARRANGE_KINDS {
        assert!(
            file.of_kind(kind).next().is_some(),
            "{}: no {kind} record",
            file.path.display()
        );
    }
}
