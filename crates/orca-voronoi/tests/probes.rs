// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The probe gate for orca-voronoi: the records of `voronoi.sop1` (`parity/sop1/voronoi.md`),
//! replayed piece by piece and compared whole, bit for bit.
//!
//! voronoi-1 (orca-o3-voronoi-1, 2026-11-13) replays its thirteen kinds:
//!
//! - VRUL: `ulp_comparison<double>`;
//! - VREF: `extended_exponent_fpt<double>`'s constructors, arithmetic, `sqrt`, `d()` and signs;
//! - VRXC, VRXI: `extended_int<64>`'s constructors and operations, `p()`, `d()` and the two
//!   converters, unnormalised values and 64-chunk overflow included;
//! - VRRF, VRRD: `robust_fpt<double>` and `robust_dif` with every operator and compound operator;
//! - VRSQ: `robust_sqrt_expr`'s `eval1` to `eval4`, each from a fresh object;
//! - VRCP: `robust_cross_product` and the four-value and three-point orientation tests;
//! - VRPC, VREC: the point and event comparison predicates;
//! - VRDP, VRNC, VRCE: the distance, node comparison and circle existence predicates on the calls
//!   the builder made in the probe's runs.
//!
//! voronoi-2 (orca-o3-voronoi-2, 2026-11-13) replays six more:
//!
//! - VRMP, VRLZ: the exact and the lazy circle formation functors on the triples the builder's
//!   runs met, with every set of recompute flags for the exact one;
//! - VRCF: the circle formation predicate, whether its functor ran and the circle it kept;
//! - VRQU: the ordered circle event queue on scripts of pushes, deactivations and pops, with many
//!   ties;
//! - VRSS: the builder's site order (`std::sort`, `std::unique`, the sorted indexes);
//! - VRBT: every call the builder makes on its output for a site set, the whole diagram's
//!   construction, degenerate input included.
//!
//! Since voronoi-2 the predicates of voronoi-1 are replayed on the crate's own `site_event`,
//! `circle_event` and `beach_line_node_key` (module `structures`), built from a record's values as
//! the C++ builds them (an inverse site through `inverse()`).
//!
//! voronoi-3 (orca-o3-voronoi-3, 2026-11-14) replays six more:
//!
//! - VRVD: `boost::polygon::construct_voronoi` of points, segments or both into
//!   `voronoi_diagram<double>`, the whole diagram after `_build` (a NaN coordinate matches any
//!   NaN, as the page says);
//! - VRVG: `Slic3r::Geometry::VoronoiDiagram`'s point and mixed overloads, the state block and the
//!   diagram through the accessors;
//! - VRUT: `VoronoiUtils`' vertex helpers (`is_finite`, `is_in_range`, `to_point`,
//!   `make_rotated_vertex`);
//! - VRPI: `PolygonsPointIndex` and `PolygonsSegmentIndex` on every point of Arachne's polygons,
//!   with the locators, the hash and the comparisons;
//! - VRCR: the source lookups and the cell ranges on every cell of a raw diagram, and the calls
//!   that throw. The C++ builds sources 0 and 1 with `construct_voronoi(..., false)` of segments,
//!   whose body is voronoi-4's (`Geometry/Voronoi.cpp`); with the repair off it is Boost's
//!   `construct_voronoi` of the segments and the state `UNKNOWN`, which is what the mixed overload
//!   with no points does, so the replay builds them through that;
//! - VRDZ: `VoronoiUtils::discretize_parabola` with its log, each call from the record's inputs
//!   (the segment as a `PolygonsSegmentIndex` into a polygon of its two ends).
//!
//! voronoi-4 (orca-o3-voronoi-4, 2026-11-14) replays the last four:
//!
//! - VRGD: `VoronoiDiagram::construct_voronoi` of segments with the repair on or off, on lines or
//!   on `PolygonsSegmentIndex`es: the state block, the rotations the repair ran (each rerun on a
//!   fresh object with the public `try_to_repair_degenerated_voronoi_diagram_by_rotation`), the
//!   diagram a repair left, the log, and the second construction MedialAxis makes after a failed
//!   repair on the record's closing lines, which detects on the stale diagram and may throw;
//! - VRCG: `VoronoiUtilsCgal`'s two planarity checks on raw diagrams of lines, of
//!   `PolygonsSegmentIndex`es and of points, each answer or error;
//! - VRAN: `Voronoi::annotate_inside_outside` after a construction with the repair on: the state
//!   and the colour of every cell, vertex and edge, and the log;
//! - VRMA: `MedialAxis::build` of both outputs and `ExPolygon::medial_axis` of both outputs on
//!   one region: the thick polylines bit for bit, whether the plain polylines have the same points,
//!   or the error, and the log of the four calls.
//!
//! A kind that is in none of the lists fails the gate. The committed subset (`parity/probes/`)
//! runs by default; `SKETCH_ORCA_PROBES_DEEP=<folder>` replays the full set from that folder
//! (`<work>/parity/probes`).

use std::cell::{Cell, RefCell};
use std::fmt::{self, Debug, Formatter};
use std::rc::Rc;
use std::sync::{Once, OnceLock};

use orca_geom::expolygon::ExPolygon;
use orca_geom::line::{Line, Lines};
use orca_geom::point::{PI, Point, Points};
use orca_geom::polygon::{Polygon, Polygons};
use orca_geom::polyline::{Polylines, ThickPolylines};
use orca_mesh::log::Severity;
use orca_probe::{Mismatches, ProbeFile, Reader, load_probe};
use orca_voronoi::predicates::ULPS;
use orca_voronoi::structures::BeachLineNodeKey;
use orca_voronoi::{
    BigInt, BoostVoronoiDiagram, CircleEvent, CircleExistencePredicate, CircleFormationFunctor,
    CircleFormationPredicate, DistancePredicate, Efpt64, EventComparisonPredicate,
    ExPolygonMedialAxis, IssueType, LazyCircleFormationFunctor, MedialAxis,
    MpCircleFormationFunctor, NodeComparisonPredicate, OrderedQueue, OrientationTest,
    PathsPointIndexLocator, Point2d, PointComparisonPredicate, PointConcept, PolygonsPointIndex,
    PolygonsPointIndexSegmentLocator, PolygonsSegmentIndex, RobustDif, RobustFpt, RobustSqrtExpr,
    SegmentConcept, SiteEvent, SourceCategory, State, TypeConverterEfpt, TypeConverterFpt,
    UlpComparison, VoronoiBuilder, VoronoiCell, VoronoiDiagram, VoronoiEdge, VoronoiError,
    VoronoiOutput, VoronoiUtils, VoronoiUtilsCgal, VoronoiVertex, annotate_inside_outside,
    construct_voronoi_points, construct_voronoi_points_segments, construct_voronoi_segments,
    polygons_point_index_hash, robust_cross_product,
};

/// The probe file, read once for every test of this file (the full set is about 13 MB).
fn file() -> &'static ProbeFile {
    static FILE: OnceLock<ProbeFile> = OnceLock::new();
    FILE.get_or_init(|| load_probe("voronoi").unwrap_or_else(|e| panic!("{e}")))
}

/// The kinds voronoi-1 replays (`parity/sop1/voronoi.md`, "The pieces").
const VORONOI_1: [&str; 13] = [
    "VRUL", "VREF", "VRXC", "VRXI", "VRRF", "VRRD", "VRSQ", "VRCP", "VRPC", "VREC", "VRDP", "VRNC",
    "VRCE",
];

/// The kinds voronoi-2 replays.
const VORONOI_2: [&str; 6] = ["VRMP", "VRLZ", "VRCF", "VRQU", "VRSS", "VRBT"];

/// The kinds voronoi-3 replays.
const VORONOI_3: [&str; 6] = ["VRVD", "VRVG", "VRUT", "VRPI", "VRCR", "VRDZ"];

/// The kinds voronoi-4 replays.
const VORONOI_4: [&str; 4] = ["VRGD", "VRCG", "VRAN", "VRMA"];

// ---------------------------------------------------------------- compared values

/// An `f64` compared by its bits.
#[derive(Clone, Copy)]
struct F(f64);

impl PartialEq for F {
    fn eq(&self, o: &Self) -> bool {
        self.0.to_bits() == o.0.to_bits()
    }
}

impl Debug for F {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} ({:#018x})", self.0, self.0.to_bits())
    }
}

/// A big integer as a record holds it: the count and the chunks within it, lowest first.
#[derive(Clone, PartialEq, Debug)]
struct Big(i32, Vec<u32>);

fn big_of(v: &BigInt) -> Big {
    Big(v.count(), v.chunks()[..v.size()].to_vec())
}

/// An efpt as the object holds it: `val_` by its bits and `exp_`.
#[derive(Clone, Copy, PartialEq, Debug)]
struct E(F, i32);

fn efpt_of(v: &Efpt64) -> E {
    E(F(v.val()), v.exp())
}

/// A robust fpt: the value and the relative error, by their bits.
#[derive(Clone, Copy, PartialEq, Debug)]
struct R(F, F);

fn rfpt_of(v: &RobustFpt) -> R {
    R(F(v.fpv()), F(v.re()))
}

/// A robust dif: its positive and its negative sum.
#[derive(Clone, Copy, PartialEq, Debug)]
struct D(R, R);

fn rdif_of(v: &RobustDif) -> D {
    D(rfpt_of(&v.pos()), rfpt_of(&v.neg_sum()))
}

/// A circle as a record holds it: x, y and lower x by their bits, and whether it is active.
#[derive(Clone, Copy, PartialEq, Debug)]
struct C(F, F, F, bool);

fn circle_of(c: &CircleEvent) -> C {
    C(F(c.x()), F(c.y()), F(c.lower_x()), c.is_active())
}

/// A site as a record holds it: x0, y0, x1, y1 (as stored, swapped when inverse), the sorted
/// index, the initial index, the source category and the inverse flag.
#[derive(Clone, Copy, PartialEq, Debug)]
struct S(i32, i32, i32, i32, u64, u64, u8, bool);

fn site_of(s: &SiteEvent) -> S {
    S(
        s.x0(),
        s.y0(),
        s.x1(),
        s.y1(),
        s.sorted_index() as u64,
        s.initial_index() as u64,
        s.source_category().0 as u8,
        s.is_inverse(),
    )
}

/// A `site_event<int32>` with a record's values, built as the C++ builds one: the points as
/// sorted, the indexes and the category set, then `inverse()` for an inverse site (which swaps
/// the points back to the record's order and sets the flag).
fn site_from(v: S) -> SiteEvent {
    let S(x0, y0, x1, y1, sorted, initial, category, inverse) = v;
    let (a, b) = (Point2d::new(x0, y0), Point2d::new(x1, y1));
    let mut s = if inverse {
        SiteEvent::from_points(b, a)
    } else {
        SiteEvent::from_points(a, b)
    };
    s.set_sorted_index(sorted as usize)
        .set_initial_index(initial as usize)
        .set_source_category(SourceCategory(u32::from(category)));
    if inverse {
        s.inverse();
    }
    s
}

// ---------------------------------------------------------------- a record

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
    fn f64(&mut self) -> f64 {
        self.r.f64().unwrap()
    }
    fn bool(&mut self) -> bool {
        self.r.bool().unwrap()
    }

    /// A big integer: `i32` count, then |count| `u32` chunks, lowest first.
    fn read_big(&mut self) -> Big {
        let count = self.i32();
        let chunks = (0..count.unsigned_abs()).map(|_| self.u32()).collect();
        Big(count, chunks)
    }
    /// The same as an operand: `extended_int`'s vector constructor takes the chunks highest first
    /// and keeps the count as it is, leading zero chunks included.
    fn big(&mut self) -> BigInt {
        let Big(count, chunks) = self.read_big();
        let highest_first: Vec<u32> = chunks.iter().rev().copied().collect();
        BigInt::from_chunks(&highest_first, count >= 0)
    }
    fn read_efpt(&mut self) -> E {
        let val = self.f64();
        E(F(val), self.i32())
    }
    fn read_rfpt(&mut self) -> R {
        let fpv = self.f64();
        R(F(fpv), F(self.f64()))
    }
    fn rfpt(&mut self) -> RobustFpt {
        let fpv = self.f64();
        RobustFpt::with_error(fpv, self.f64())
    }
    fn read_rdif(&mut self) -> D {
        let pos = self.read_rfpt();
        D(pos, self.read_rfpt())
    }
    fn rdif(&mut self) -> RobustDif {
        let pos = self.rfpt();
        RobustDif::from_sums(pos, self.rfpt())
    }
    /// A site's values: `i32` x0, y0, x1, y1, `u64` sorted index, `u64` initial index, `u8`
    /// source category, `bool` inverse.
    fn read_site(&mut self) -> S {
        let (x0, y0, x1, y1) = (self.i32(), self.i32(), self.i32(), self.i32());
        let (sorted, initial) = (self.u64(), self.u64());
        let category = self.u8();
        S(x0, y0, x1, y1, sorted, initial, category, self.bool())
    }
    /// A site.
    fn site(&mut self) -> SiteEvent {
        let v = self.read_site();
        site_from(v)
    }
    /// A circle's values: `f64` x, y, lower x, `bool` active.
    fn read_circle(&mut self) -> C {
        let (x, y, lower_x) = (self.f64(), self.f64(), self.f64());
        C(F(x), F(y), F(lower_x), self.bool())
    }
    /// A circle.
    fn circle(&mut self) -> CircleEvent {
        let C(x, y, lower_x, active) = self.read_circle();
        let mut c = CircleEvent::new(x.0, y.0, lower_x.0);
        if !active {
            c.deactivate();
        }
        c
    }
    /// A site set: `u32` points, each `i32` x, y; `u32` segments, each `i32` x1, y1, x2, y2.
    fn site_set(&mut self) -> SiteSet {
        let np = self.u32();
        let points = (0..np).map(|_| (self.i32(), self.i32())).collect();
        let ns = self.u32();
        let segments = (0..ns)
            .map(|_| (self.i32(), self.i32(), self.i32(), self.i32()))
            .collect();
        SiteSet { points, segments }
    }

    fn is_big(&mut self, what: &str, got: &BigInt) {
        let want = self.read_big();
        self.same(what, &big_of(got), &want);
    }
    fn is_efpt(&mut self, what: &str, got: &Efpt64) {
        let want = self.read_efpt();
        self.same(what, &efpt_of(got), &want);
    }
    fn is_rfpt(&mut self, what: &str, got: &RobustFpt) {
        let want = self.read_rfpt();
        self.same(what, &rfpt_of(got), &want);
    }
    fn is_rdif(&mut self, what: &str, got: &RobustDif) {
        let want = self.read_rdif();
        self.same(what, &rdif_of(got), &want);
    }
    fn is_circle(&mut self, what: &str, got: &CircleEvent) {
        let want = self.read_circle();
        self.same(what, &circle_of(got), &want);
    }
    fn is_f64(&mut self, what: &str, got: f64) {
        let want = F(self.f64());
        self.same(what, &F(got), &want);
    }
    fn is_i32(&mut self, what: &str, got: i32) {
        let want = self.i32();
        self.same(what, &got, &want);
    }
    /// Compares a `bool`; true when it is the C++'s (a record's later fields follow it).
    fn is_bool(&mut self, what: &str, got: bool) -> bool {
        let want = self.bool();
        self.same(what, &got, &want)
    }

    /// Compares one result with the C++ engine's; true when they are the same.
    fn same<T: PartialEq + Debug>(&mut self, what: &str, got: &T, want: &T) -> bool {
        let label = &self.label;
        let ok = got == want;
        self.m.check(ok, || {
            format!("{label}: {what}: {got:?} instead of {want:?}")
        });
        ok
    }
}

/// Replays every record of `kind` with `f`; a record that matched must have been read to its end.
fn replay(kind: &str, f: impl Fn(&mut Case)) {
    let file = file();
    let mut m = Mismatches::default();
    let mut records = 0;
    for (i, rec) in file.of_kind(kind).enumerate() {
        records += 1;
        let before = m.count;
        let mut c = Case {
            r: rec.reader(),
            m: &mut m,
            label: format!("{kind} {i}"),
        };
        f(&mut c);
        if c.m.count == before {
            let label = c.label.clone();
            c.r.finish(&label).unwrap();
        }
    }
    // Shown with `-- --nocapture`: what the replay covered.
    eprintln!(
        "{kind}: {records} records of the {} set, {} comparisons",
        file.set.name(),
        m.checked
    );
    m.assert_none(kind);
}

// ---------------------------------------------------------------- VRUL

fn vrul(c: &mut Case) {
    let a = c.f64();
    let b = c.f64();
    let ulps = c.u32();
    c.is_i32(
        "ulp_comparison(a, b, maxUlps)",
        UlpComparison.call(a, b, ulps) as i32,
    );
}

#[test]
fn ulp_comparison_matches() {
    replay("VRUL", vrul);
}

// ---------------------------------------------------------------- VREF

fn vref(c: &mut Case) {
    let va = c.f64();
    let ea = c.i32();
    let vb = c.f64();
    let eb = c.i32();
    let a = Efpt64::with_exp(va, ea);
    let b = Efpt64::with_exp(vb, eb);
    c.is_efpt("A = efpt(va, ea)", &a);
    c.is_efpt("B = efpt(vb, eb)", &b);
    c.is_efpt("efpt(va)", &Efpt64::new(va));
    c.is_efpt("-A", &-a);
    c.is_efpt("A + B", &(a + b));
    c.is_efpt("A - B", &(a - b));
    c.is_efpt("A * B", &(a * b));
    if !c.is_bool("B nonzero", !b.is_zero()) {
        return;
    }
    if !b.is_zero() {
        c.is_efpt("A / B", &(a / b));
    }
    if !c.is_bool("A not negative", !a.is_neg()) {
        return;
    }
    if !a.is_neg() {
        c.is_efpt("A.sqrt()", &a.sqrt());
    }
    c.is_f64("A.d()", a.d());
    c.is_f64("B.d()", b.d());
    c.is_bool("A.is_pos()", a.is_pos());
    c.is_bool("A.is_neg()", a.is_neg());
    c.is_bool("A.is_zero()", a.is_zero());
}

#[test]
fn extended_exponent_fpt_matches() {
    replay("VREF", vref);
}

// ---------------------------------------------------------------- VRXC, VRXI

fn vrxc(c: &mut Case) {
    let form = c.u8();
    let got = match form {
        0 => BigInt::from_i32(c.i32()),
        1 => BigInt::from_i64(c.i64()),
        2 => {
            let plus = c.bool();
            let n = c.u32();
            let chunks: Vec<u32> = (0..n).map(|_| c.u32()).collect();
            BigInt::from_chunks(&chunks, plus)
        }
        _ => panic!("{}: form {form}", c.label),
    };
    c.is_big("extended_int(...)", &got);
}

#[test]
fn extended_int_constructors_match() {
    replay("VRXC", vrxc);
}

fn vrxi(c: &mut Case) {
    let x = c.big();
    let y = c.big();
    let bits = u8::from(x == y)
        | (u8::from(x != y) << 1)
        | (u8::from(x < y) << 2)
        | (u8::from(x > y) << 3)
        | (u8::from(x <= y) << 4)
        | (u8::from(x >= y) << 5);
    let want = c.u8();
    c.same("x == y, !=, <, >, <=, >=", &bits, &want);
    c.is_big("x + y", &(x + y));
    c.is_big("x - y", &(x - y));
    c.is_big("x * y", &(x * y));
    c.is_big("-x", &-x);
    let m = c.i32();
    c.is_big("x * m", &(x * m));
    let m64 = c.i64();
    c.is_big("x * m64", &(x * m64));
    let p = x.p();
    c.is_f64("x.p().first", p.0);
    c.is_i32("x.p().second", p.1);
    c.is_f64("x.d()", x.d());
    c.is_efpt("type_converter_efpt(x)", &TypeConverterEfpt.call(&x));
    c.is_f64("type_converter_fpt(x)", TypeConverterFpt.call(&x));
}

#[test]
fn extended_int_operations_match() {
    replay("VRXI", vrxi);
}

// ---------------------------------------------------------------- VRRF, VRRD

fn vrrf(c: &mut Case) {
    let x = c.rfpt();
    let y = c.rfpt();
    let div = y.fpv() != 0.0;
    // The probe's `!(x.fpv() < 0.0)`, which is `!has_neg_value()`.
    let root = !x.has_neg_value();
    if !c.is_bool("y.fpv nonzero", div) || !c.is_bool("x.fpv not negative", root) {
        return;
    }
    c.is_rfpt("x + y", &(x + y));
    c.is_rfpt("x - y", &(x - y));
    c.is_rfpt("x * y", &(x * y));
    if div {
        c.is_rfpt("x / y", &(x / y));
    }
    let mut v = x;
    v += y;
    c.is_rfpt("x += y", &v);
    v = x;
    v -= y;
    c.is_rfpt("x -= y", &v);
    v = x;
    v *= y;
    c.is_rfpt("x *= y", &v);
    if div {
        v = x;
        v /= y;
        c.is_rfpt("x /= y", &v);
    }
    c.is_rfpt("-x", &-x);
    if root {
        c.is_rfpt("x.sqrt()", &x.sqrt());
    }
    c.is_bool("x.has_pos_value()", x.has_pos_value());
    c.is_bool("x.has_neg_value()", x.has_neg_value());
    c.is_bool("x.has_zero_value()", x.has_zero_value());
}

#[test]
fn robust_fpt_matches() {
    replay("VRRF", vrrf);
}

fn vrrd(c: &mut Case) {
    let d1 = c.rdif();
    let d2 = c.rdif();
    let v = c.rfpt();
    let div = v.fpv() != 0.0;
    if !c.is_bool("v.fpv nonzero", div) {
        return;
    }
    c.is_rfpt("d1.dif()", &d1.dif());
    c.is_rdif("-d1", &-d1);
    c.is_rdif("d1 + d2", &(d1 + d2));
    c.is_rdif("d1 + v", &(d1 + v));
    c.is_rdif("v + d1", &(v + d1));
    c.is_rdif("d1 - d2", &(d1 - d2));
    c.is_rdif("d1 - v", &(d1 - v));
    c.is_rdif("v - d1", &(v - d1));
    c.is_rdif("d1 * d2", &(d1 * d2));
    c.is_rdif("d1 * v", &(d1 * v));
    c.is_rdif("v * d1", &(v * d1));
    if div {
        c.is_rdif("d1 / v", &(d1 / v));
    }
    let mut e = d1;
    e += v;
    c.is_rdif("d1 += v", &e);
    e = d1;
    e += d2;
    c.is_rdif("d1 += d2", &e);
    e = d1;
    e -= v;
    c.is_rdif("d1 -= v", &e);
    e = d1;
    e -= d2;
    c.is_rdif("d1 -= d2", &e);
    e = d1;
    e *= v;
    c.is_rdif("d1 *= v", &e);
    e = d1;
    e *= d2;
    c.is_rdif("d1 *= d2", &e);
    if div {
        e = d1;
        e /= v;
        c.is_rdif("d1 /= v", &e);
    }
}

#[test]
fn robust_dif_matches() {
    replay("VRRD", vrrd);
}

// ---------------------------------------------------------------- VRSQ

fn vrsq(c: &mut Case) {
    let n = c.u8();
    let a: Vec<BigInt> = (0..n).map(|_| c.big()).collect();
    let b: Vec<BigInt> = (0..n).map(|_| c.big()).collect();
    let mut e = RobustSqrtExpr::new();
    let got = match n {
        1 => e.eval1(&a, &b),
        2 => e.eval2(&a, &b),
        3 => e.eval3(&a, &b),
        4 => e.eval4(&a, &b),
        _ => panic!("{}: n {n}", c.label),
    };
    c.is_efpt("robust_sqrt_expr::eval", &got);
}

#[test]
fn robust_sqrt_expr_matches() {
    replay("VRSQ", vrsq);
}

// ---------------------------------------------------------------- VRCP, VRPC

fn vrcp(c: &mut Case) {
    match c.u8() {
        0 => {
            let (a1, b1, a2, b2) = (c.i64(), c.i64(), c.i64(), c.i64());
            c.is_f64("robust_cross_product", robust_cross_product(a1, b1, a2, b2));
            c.is_i32(
                "orientation_test::eval(a1, b1, a2, b2)",
                OrientationTest::eval_dif(a1, b1, a2, b2) as i32,
            );
        }
        1 => {
            let mut p = [Point2d::default(); 3];
            for q in &mut p {
                let (x, y) = (c.i32(), c.i32());
                *q = Point2d::new(x, y);
            }
            c.is_i32(
                "orientation_test::eval(p1, p2, p3)",
                OrientationTest::eval_points(&p[0], &p[1], &p[2]) as i32,
            );
        }
        form => panic!("{}: form {form}", c.label),
    }
}

#[test]
fn cross_product_and_orientation_match() {
    replay("VRCP", vrcp);
}

fn vrpc(c: &mut Case) {
    let a = Point2d::new(c.i32(), c.i32());
    let b = Point2d::new(c.i32(), c.i32());
    c.is_bool(
        "point_comparison_predicate",
        PointComparisonPredicate.call(&a, &b),
    );
}

#[test]
fn point_comparison_matches() {
    replay("VRPC", vrpc);
}

// ---------------------------------------------------------------- VREC

fn vrec(c: &mut Case) {
    let cmp = EventComparisonPredicate::default();
    let got = match c.u8() {
        0 => {
            let (x, y) = (c.site(), c.site());
            cmp.site_site(&x, &y)
        }
        1 => {
            let x = c.site();
            let k = c.circle();
            cmp.site_circle(&x, &k)
        }
        2 => {
            let k = c.circle();
            let x = c.site();
            cmp.circle_site(&k, &x)
        }
        3 => {
            let (k1, k2) = (c.circle(), c.circle());
            cmp.circle_circle(&k1, &k2)
        }
        form => panic!("{}: form {form}", c.label),
    };
    c.is_bool("event_comparison_predicate", got);
}

#[test]
fn event_comparison_matches() {
    assert_eq!(ULPS, 64);
    replay("VREC", vrec);
}

// ---------------------------------------------------------------- VRDP, VRNC, VRCE

fn vrdp(c: &mut Case) {
    let left = c.site();
    let right = c.site();
    let p = Point2d::new(c.i32(), c.i32());
    c.is_bool(
        "distance_predicate",
        DistancePredicate::default().call(&left, &right, &p),
    );
}

#[test]
fn distance_predicate_matches() {
    replay("VRDP", vrdp);
}

fn vrnc(c: &mut Case) {
    let (l1, r1) = (c.site(), c.site());
    let (l2, r2) = (c.site(), c.site());
    let n1 = BeachLineNodeKey::new(&l1, &r1);
    let n2 = BeachLineNodeKey::new(&l2, &r2);
    c.is_bool(
        "node_comparison_predicate",
        NodeComparisonPredicate::default().call(&n1, &n2),
    );
}

#[test]
fn node_comparison_matches() {
    replay("VRNC", vrnc);
}

fn vrce(c: &mut Case) {
    let form = c.u8();
    let index = c.i32();
    let (s1, s2, s3) = (c.site(), c.site(), c.site());
    let p = CircleExistencePredicate;
    let got = match form {
        0 => p.ppp(&s1, &s2, &s3),
        1 => p.pps(&s1, &s2, &s3, index),
        2 => p.pss(&s1, &s2, &s3, index),
        3 => p.sss(&s1, &s2, &s3),
        _ => panic!("{}: form {form}", c.label),
    };
    c.is_bool("circle_existence_predicate", got);
}

#[test]
fn circle_existence_matches() {
    replay("VRCE", vrce);
}

// ---------------------------------------------------------------- VRMP, VRLZ, VRCF

fn vrmp(c: &mut Case) {
    let form = c.u8();
    let index = c.i32();
    let (s1, s2, s3) = (c.site(), c.site(), c.site());
    let flags = c.u8();
    let (fx, fy, fl) = (flags & 1 != 0, flags & 2 != 0, flags & 4 != 0);
    let mut circle = c.circle();
    let mut mp = MpCircleFormationFunctor::default();
    match form {
        0 => mp.ppp(&s1, &s2, &s3, &mut circle, fx, fy, fl),
        1 => mp.pps(&s1, &s2, &s3, index, &mut circle, fx, fy, fl),
        2 => mp.pss(&s1, &s2, &s3, index, &mut circle, fx, fy, fl),
        3 => mp.sss(&s1, &s2, &s3, &mut circle, fx, fy, fl),
        _ => panic!("{}: form {form}", c.label),
    }
    c.is_circle("mp_circle_formation_functor", &circle);
}

#[test]
fn mp_circle_formation_matches() {
    replay("VRMP", vrmp);
}

fn vrlz(c: &mut Case) {
    let form = c.u8();
    let index = c.i32();
    let (s1, s2, s3) = (c.site(), c.site(), c.site());
    let mut circle = CircleEvent::default();
    let mut lazy = LazyCircleFormationFunctor::default();
    match form {
        0 => lazy.ppp(&s1, &s2, &s3, &mut circle),
        1 => lazy.pps(&s1, &s2, &s3, index, &mut circle),
        2 => lazy.pss(&s1, &s2, &s3, index, &mut circle),
        3 => lazy.sss(&s1, &s2, &s3, &mut circle),
        _ => panic!("{}: form {form}", c.label),
    }
    c.is_circle("lazy_circle_formation_functor", &circle);
}

#[test]
fn lazy_circle_formation_matches() {
    replay("VRLZ", vrlz);
}

/// The lazy functor, noting that it ran (the probe's `RecLazy`, which sets `g_cap.formed`).
struct Formed {
    lazy: LazyCircleFormationFunctor,
    ran: Rc<Cell<bool>>,
}

impl CircleFormationFunctor for Formed {
    fn ppp(&mut self, s1: &SiteEvent, s2: &SiteEvent, s3: &SiteEvent, e: &mut CircleEvent) {
        self.lazy.ppp(s1, s2, s3, e);
        self.ran.set(true);
    }
    fn pps(&mut self, s1: &SiteEvent, s2: &SiteEvent, s3: &SiteEvent, i: i32, e: &mut CircleEvent) {
        self.lazy.pps(s1, s2, s3, i, e);
        self.ran.set(true);
    }
    fn pss(&mut self, s1: &SiteEvent, s2: &SiteEvent, s3: &SiteEvent, i: i32, e: &mut CircleEvent) {
        self.lazy.pss(s1, s2, s3, i, e);
        self.ran.set(true);
    }
    fn sss(&mut self, s1: &SiteEvent, s2: &SiteEvent, s3: &SiteEvent, e: &mut CircleEvent) {
        self.lazy.sss(s1, s2, s3, e);
        self.ran.set(true);
    }
}

fn vrcf(c: &mut Case) {
    let (s1, s2, s3) = (c.site(), c.site(), c.site());
    let ran = Rc::new(Cell::new(false));
    let mut p = CircleFormationPredicate::new(
        CircleExistencePredicate,
        Formed {
            lazy: LazyCircleFormationFunctor::default(),
            ran: Rc::clone(&ran),
        },
    );
    let mut circle = CircleEvent::default();
    let got = p.call(&s1, &s2, &s3, &mut circle);
    if !c.is_bool("the formation functor ran", ran.get())
        || !c.is_bool("circle_formation_predicate", got)
    {
        return;
    }
    if ran.get() {
        c.is_circle("the circle", &circle);
    }
}

#[test]
fn circle_formation_predicate_matches() {
    replay("VRCF", vrcf);
}

// ---------------------------------------------------------------- VRQU

/// The probe's queue element: a circle and its id.
type Qe = (CircleEvent, u32);

/// The builder's circle event order (`voronoi_builder.hpp` 160-165), the probe's `QCmp`.
fn qcmp(l: &Qe, r: &Qe) -> bool {
    EventComparisonPredicate::default().circle_circle(&r.0, &l.0)
}

fn vrqu(c: &mut Case) {
    let ops = c.u32();
    let mut q = OrderedQueue::new(qcmp as fn(&Qe, &Qe) -> bool);
    let mut live: Vec<usize> = Vec::new();
    for op in 0..ops {
        match c.u8() {
            0 => {
                let circle = c.circle();
                let id = c.u32();
                assert_eq!(id as usize, live.len(), "{}: push {op}", c.label);
                live.push(q.push((circle, id)));
            }
            2 => {
                let id = c.u32();
                q.get_mut(live[id as usize]).0.deactivate();
            }
            1 => {
                let (id, active) = (q.top().1, q.top().0.is_active());
                let want_id = c.u32();
                let want_active = c.bool();
                let what = format!("pop {op}: the top's id and activity");
                if !c.same(&what, &(id, active), &(want_id, want_active)) {
                    return;
                }
                q.pop();
            }
            tag => panic!("{}: operation {tag}", c.label),
        }
    }
}

#[test]
fn ordered_queue_matches() {
    replay("VRQU", vrqu);
}

// ---------------------------------------------------------------- VRSS, VRBT

/// A builder input: points, then segments.
struct SiteSet {
    points: Vec<(i32, i32)>,
    segments: Vec<(i32, i32, i32, i32)>,
}

/// One call the builder made on its output, as VRBT records it: site references are the sorted
/// index and the inverse flag, edges their index in the diagram's edge vector.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Call {
    /// `_reserve(n)`.
    Reserve(u64),
    /// `_process_single_site(site)`.
    Single((u32, bool)),
    /// `_insert_new_edge(site1, site2)` and the two new half-edges.
    Edge((u32, bool), (u32, bool), u32, u32),
    /// `_insert_new_edge(site1, site3, circle, edge12, edge23)` and the two new half-edges.
    Circle((u32, bool), (u32, bool), [F; 3], [u32; 4]),
    /// `_build()`.
    Build,
}

/// A sorted site as the calls show it: x0, y0, x1, y1 (as sorted, not inverse), the initial index
/// and the category; `None` for a site no call names.
type Entry = Option<(i32, i32, i32, i32, u64, u8)>;

/// The output the builder runs into: every call kept, the edges numbered as `voronoi_diagram`
/// numbers them (each `_insert_new_edge` pushes two edges).
#[derive(Default)]
struct Trace {
    calls: Vec<Call>,
    table: Vec<Entry>,
    edges: u32,
}

impl Trace {
    fn reference(&mut self, s: &SiteEvent) -> (u32, bool) {
        let i = s.sorted_index();
        if i >= self.table.len() {
            self.table.resize(i + 1, None);
        }
        if self.table[i].is_none() {
            // The site as sorted: an inverse site holds its points swapped.
            let (a, b) = if s.is_inverse() {
                (s.point1(), s.point0())
            } else {
                (s.point0(), s.point1())
            };
            self.table[i] = Some((
                a.x(),
                a.y(),
                b.x(),
                b.y(),
                s.initial_index() as u64,
                s.source_category().0 as u8,
            ));
        }
        (i as u32, s.is_inverse())
    }
}

impl VoronoiOutput for Trace {
    type Edge = u32;
    fn reserve(&mut self, num_sites: usize) {
        self.calls.push(Call::Reserve(num_sites as u64));
    }
    fn process_single_site(&mut self, site: &SiteEvent) {
        let a = self.reference(site);
        self.calls.push(Call::Single(a));
    }
    fn insert_new_edge(&mut self, site1: &SiteEvent, site2: &SiteEvent) -> (u32, u32) {
        let a = self.reference(site1);
        let b = self.reference(site2);
        let (r1, r2) = (self.edges, self.edges + 1);
        self.edges += 2;
        self.calls.push(Call::Edge(a, b, r1, r2));
        (r1, r2)
    }
    fn insert_new_edge_at_circle(
        &mut self,
        site1: &SiteEvent,
        site3: &SiteEvent,
        circle: &CircleEvent,
        data12: Option<u32>,
        data23: Option<u32>,
    ) -> (u32, u32) {
        let a = self.reference(site1);
        let b = self.reference(site3);
        let (r1, r2) = (self.edges, self.edges + 1);
        self.edges += 2;
        let d12 = data12.expect("the builder passed a null edge12");
        let d23 = data23.expect("the builder passed a null edge23");
        self.calls.push(Call::Circle(
            a,
            b,
            [F(circle.x()), F(circle.y()), F(circle.lower_x())],
            [d12, d23, r1, r2],
        ));
        (r1, r2)
    }
    fn build(&mut self) {
        self.calls.push(Call::Build);
    }
}

/// Runs the builder on a site set, as the probe does (`insert_point` for each point, then
/// `insert_segment` for each segment, then `construct`).
fn build(ss: &SiteSet) -> (VoronoiBuilder<u32>, Trace) {
    let mut builder = VoronoiBuilder::<u32>::new();
    for &(x, y) in &ss.points {
        builder.insert_point(x, y);
    }
    for &(x1, y1, x2, y2) in &ss.segments {
        builder.insert_segment(x1, y1, x2, y2);
    }
    let mut out = Trace::default();
    builder.construct(&mut out);
    (builder, out)
}

fn vrss(c: &mut Case) {
    let ss = c.site_set();
    let (builder, _) = build(&ss);
    let n = c.u32();
    let want: Vec<S> = (0..n).map(|_| c.read_site()).collect();
    let got: Vec<S> = builder.site_events().iter().map(site_of).collect();
    c.same("the sorted sites", &got, &want);
}

#[test]
fn site_order_matches() {
    replay("VRSS", vrss);
}

/// A site reference: `u32` sorted index, `bool` inverse.
fn read_ref(c: &mut Case) -> (u32, bool) {
    let i = c.u32();
    (i, c.bool())
}

fn vrbt(c: &mut Case) {
    let ss = c.site_set();
    let (_, out) = build(&ss);
    let n = c.u32();
    let mut table: Vec<Entry> = Vec::with_capacity(n as usize);
    for _ in 0..n {
        table.push(if c.bool() {
            let (x0, y0, x1, y1) = (c.i32(), c.i32(), c.i32(), c.i32());
            let initial = c.u64();
            Some((x0, y0, x1, y1, initial, c.u8()))
        } else {
            None
        });
    }
    let n = c.u32();
    let mut calls = Vec::with_capacity(n as usize);
    for _ in 0..n {
        calls.push(match c.u8() {
            0 => Call::Reserve(c.u64()),
            1 => Call::Single(read_ref(c)),
            2 => {
                let (a, b) = (read_ref(c), read_ref(c));
                let (r1, r2) = (c.u32(), c.u32());
                Call::Edge(a, b, r1, r2)
            }
            3 => {
                let (a, b) = (read_ref(c), read_ref(c));
                let xyl = [F(c.f64()), F(c.f64()), F(c.f64())];
                let e = [c.u32(), c.u32(), c.u32(), c.u32()];
                Call::Circle(a, b, xyl, e)
            }
            4 => Call::Build,
            tag => panic!("{}: call tag {tag}", c.label),
        });
    }
    // The first call that differs, then the whole trace's length, so a mismatch shows where the
    // sweep went another way.
    if let Some(k) = (0..calls.len().min(out.calls.len())).find(|&k| calls[k] != out.calls[k]) {
        c.same(&format!("call {k}"), &out.calls[k], &calls[k]);
        return;
    }
    if !c.same("the number of calls", &out.calls.len(), &calls.len()) {
        return;
    }
    c.same("the sorted sites the calls name", &out.table, &table);
}

#[test]
fn builder_run_matches() {
    replay("VRBT", vrbt);
}

// ---------------------------------------------------------------- voronoi-3: values

/// A point of a site set as the probe gives `boost::polygon::point_data<int32>`.
struct P32(i32, i32);

impl PointConcept for P32 {
    fn x(&self) -> i64 {
        i64::from(self.0)
    }
    fn y(&self) -> i64 {
        i64::from(self.1)
    }
}

/// A segment of a site set as the probe gives `boost::polygon::segment_data<int32>`.
struct S32(P32, P32);

impl SegmentConcept for S32 {
    type Point = P32;
    fn low(&self) -> P32 {
        P32((self.0).0, (self.0).1)
    }
    fn high(&self) -> P32 {
        P32((self.1).0, (self.1).1)
    }
}

/// A vertex coordinate as the page compares it: a NaN matches any NaN, else the bits.
#[derive(Clone, Copy)]
struct Coord(f64);

impl PartialEq for Coord {
    fn eq(&self, o: &Self) -> bool {
        if self.0.is_nan() || o.0.is_nan() {
            self.0.is_nan() && o.0.is_nan()
        } else {
            self.0.to_bits() == o.0.to_bits()
        }
    }
}

impl Debug for Coord {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} ({:#018x})", self.0, self.0.to_bits())
    }
}

/// A cell as a diagram record holds it: source index, source category, incident edge, colour.
#[derive(Clone, Copy, PartialEq, Debug)]
struct DCell(u64, u8, i32, u32);

/// A vertex: x, y, incident edge, colour.
#[derive(Clone, Copy, PartialEq, Debug)]
struct DVertex(Coord, Coord, i32, u32);

/// An edge: cell, vertex0, twin, next, prev, flags (1 linear, 2 primary), colour.
#[derive(Clone, Copy, PartialEq, Debug)]
struct DEdge(i32, i32, i32, i32, i32, u8, u32);

/// A whole diagram as a record holds it.
#[derive(Default)]
struct Dia {
    cells: Vec<DCell>,
    vertices: Vec<DVertex>,
    edges: Vec<DEdge>,
}

/// A link as a record holds it: the index, -1 for none.
fn link(i: Option<usize>) -> i32 {
    i.map_or(-1, |i| i as i32)
}

/// The diagram of the three containers, as the probe's `put_diagram` writes it.
fn dia_of(cells: &[VoronoiCell], vertices: &[VoronoiVertex], edges: &[VoronoiEdge]) -> Dia {
    Dia {
        cells: cells
            .iter()
            .map(|c| {
                DCell(
                    c.source_index() as u64,
                    c.source_category().0 as u8,
                    link(c.incident_edge()),
                    c.color() as u32,
                )
            })
            .collect(),
        vertices: vertices
            .iter()
            .map(|v| {
                DVertex(
                    Coord(v.x()),
                    Coord(v.y()),
                    link(v.incident_edge()),
                    v.color() as u32,
                )
            })
            .collect(),
        edges: edges
            .iter()
            .map(|e| {
                DEdge(
                    link(e.cell()),
                    link(e.vertex0()),
                    link(e.twin()),
                    link(e.next()),
                    link(e.prev()),
                    u8::from(e.is_linear()) | (u8::from(e.is_primary()) << 1),
                    e.color() as u32,
                )
            })
            .collect(),
    }
}

thread_local! {
    /// What the crate logged on this thread while a capture runs, as the probe's sink writes it:
    /// `[<severity>] <message>` and a line feed for each record.
    static CAPTURE: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The lowest severity the probe's capture sees: libslic3r sets Boost.Log's core filter when it
/// is loaded (`set_logging_level(2)` in a release version: warning and above).
const PROBE_LEVEL: Severity = Severity::Warning;

/// Starts this thread's capture, with the sink set once for the test program.
fn log_start() {
    static SINK: Once = Once::new();
    SINK.call_once(|| {
        orca_mesh::log::set_sink(Some(Box::new(|severity, message| {
            if severity < PROBE_LEVEL {
                return;
            }
            CAPTURE.with(|c| {
                if let Some(log) = c.borrow_mut().as_mut() {
                    log.push_str(&format!("[{}] {message}\n", severity.name()));
                }
            });
        })));
    });
    CAPTURE.with(|c| *c.borrow_mut() = Some(String::new()));
}

/// What came since the last take.
fn log_take() -> String {
    CAPTURE.with(|c| {
        c.borrow_mut()
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    })
}

/// Ends this thread's capture.
fn log_stop() {
    CAPTURE.with(|c| *c.borrow_mut() = None);
}

/// An exception as a record holds it: `u8` 1 `InvalidArgument`, 2 `OutOfRange`, 3
/// `RuntimeError`, then `what()`.
fn error_of(e: &VoronoiError) -> (u8, String) {
    let code = match e {
        VoronoiError::InvalidArgument(_) => 1,
        VoronoiError::OutOfRange(_) => 2,
        VoronoiError::RuntimeError(_) => 3,
    };
    (code, e.what().to_owned())
}

impl Case<'_, '_> {
    fn f32(&mut self) -> f32 {
        self.r.f32().unwrap()
    }
    fn str(&mut self) -> String {
        self.r.str().unwrap()
    }
    /// A point: `i64` x, `i64` y.
    fn pt(&mut self) -> Point {
        let x = self.i64();
        Point::new(x, self.i64())
    }
    /// Points: a `u32` count, then each point.
    fn pts(&mut self) -> Points {
        let n = self.u32();
        (0..n).map(|_| self.pt()).collect()
    }
    /// Polygons: a `u32` count, then each polygon's points.
    fn polys(&mut self) -> Polygons {
        let n = self.u32();
        (0..n).map(|_| Polygon::new(self.pts())).collect()
    }
    /// Lines: a `u32` count, then each line's two points.
    fn lines(&mut self) -> Lines {
        let n = self.u32();
        (0..n)
            .map(|_| {
                let a = self.pt();
                Line::new(a, self.pt())
            })
            .collect()
    }
    /// A diagram record's diagram.
    fn read_dia(&mut self) -> Dia {
        let mut d = Dia::default();
        for _ in 0..self.u32() {
            let (index, category) = (self.u64(), self.u8());
            let (edge, color) = (self.i32(), self.u32());
            d.cells.push(DCell(index, category, edge, color));
        }
        for _ in 0..self.u32() {
            let (x, y) = (self.f64(), self.f64());
            let (edge, color) = (self.i32(), self.u32());
            d.vertices.push(DVertex(Coord(x), Coord(y), edge, color));
        }
        for _ in 0..self.u32() {
            let (cell, vertex0, twin) = (self.i32(), self.i32(), self.i32());
            let (next, prev) = (self.i32(), self.i32());
            let (flags, color) = (self.u8(), self.u32());
            d.edges
                .push(DEdge(cell, vertex0, twin, next, prev, flags, color));
        }
        d
    }
    /// Compares a diagram with the record's: the counts, then the first element that differs.
    fn is_dia(&mut self, got: &Dia) {
        let want = self.read_dia();
        fn first<T: PartialEq + Debug>(c: &mut Case, what: &str, got: &[T], want: &[T]) {
            if !c.same(&format!("the number of {what}"), &got.len(), &want.len()) {
                return;
            }
            if let Some(k) = (0..got.len()).find(|&k| got[k] != want[k]) {
                c.same(&format!("{what} {k}"), &got[k], &want[k]);
            }
        }
        first(self, "cells", &got.cells, &want.cells);
        first(self, "vertices", &got.vertices, &want.vertices);
        first(self, "edges", &got.edges, &want.edges);
    }
    fn is_pt(&mut self, what: &str, got: Point) {
        let want = self.pt();
        self.same(what, &got, &want);
    }
    fn is_u32(&mut self, what: &str, got: u32) {
        let want = self.u32();
        self.same(what, &got, &want);
    }
    /// A call's result: `u8` 0 when it returned, else the error; true when it returned.
    fn is_outcome<T>(&mut self, what: &str, got: &Result<T, VoronoiError>) -> bool {
        let want = match self.u8() {
            0 => None,
            code => Some((code, self.str())),
        };
        let got_error = got.as_ref().err().map(error_of);
        self.same(what, &got_error, &want) && want.is_none()
    }
}

// ---------------------------------------------------------------- VRVD

fn vrvd(c: &mut Case) {
    let entry = c.u8();
    let ss = c.site_set();
    let pts: Vec<P32> = ss.points.iter().map(|&(x, y)| P32(x, y)).collect();
    let segs: Vec<S32> = ss
        .segments
        .iter()
        .map(|&(x1, y1, x2, y2)| S32(P32(x1, y1), P32(x2, y2)))
        .collect();
    let mut vd = BoostVoronoiDiagram::new();
    let got_entry = if segs.is_empty() {
        construct_voronoi_points(&pts, &mut vd);
        0
    } else if pts.is_empty() {
        construct_voronoi_segments(&segs, &mut vd);
        1
    } else {
        construct_voronoi_points_segments(&pts, &segs, &mut vd);
        2
    };
    if !c.same("the entry", &got_entry, &entry) {
        return;
    }
    c.is_dia(&dia_of(vd.cells(), vd.vertices(), vd.edges()));
}

#[test]
fn boost_diagram_matches() {
    replay("VRVD", vrvd);
}

// ---------------------------------------------------------------- VRVG

fn vrvg(c: &mut Case) {
    let entry = c.u8();
    let ss = c.site_set();
    let pts: Points = ss
        .points
        .iter()
        .map(|&(x, y)| Point::new(i64::from(x), i64::from(y)))
        .collect();
    let lines: Lines = ss
        .segments
        .iter()
        .map(|&(x1, y1, x2, y2)| {
            Line::new(
                Point::new(i64::from(x1), i64::from(y1)),
                Point::new(i64::from(x2), i64::from(y2)),
            )
        })
        .collect();
    let mut vd = VoronoiDiagram::new();
    let got_entry = if lines.is_empty() {
        vd.construct_voronoi_points(&pts);
        0
    } else {
        vd.construct_voronoi_points_segments(&pts, &lines);
        2
    };
    if !c.same("the entry", &got_entry, &entry) {
        return;
    }
    let state = (
        vd.get_issue_type() as u8,
        vd.get_state() as u8,
        vd.is_valid(),
    );
    let want = (c.u8(), c.u8(), c.bool());
    c.same("the issue, state and validity", &state, &want);
    let counts = (
        vd.num_vertices() as u64,
        vd.num_edges() as u64,
        vd.num_cells() as u64,
    );
    let want = (c.u64(), c.u64(), c.u64());
    c.same("num_vertices, num_edges, num_cells", &counts, &want);
    c.is_dia(&dia_of(vd.cells(), vd.vertices(), vd.edges()));
}

#[test]
fn slic3r_point_diagram_matches() {
    replay("VRVG", vrvg);
}

// ---------------------------------------------------------------- VRUT

fn vrut(c: &mut Case) {
    let (x, y, angle) = (c.f64(), c.f64(), c.f64());
    let color = c.u32();
    let v = VoronoiVertex::new(x, y);
    v.set_color(color as usize);
    let finite = VoronoiUtils::is_finite(&v);
    if !c.is_bool("is_finite", finite) {
        return;
    }
    c.is_bool(
        "is_in_range<int64_t>(x)",
        VoronoiUtils::is_in_range::<i64>(x),
    );
    c.is_bool(
        "is_in_range<int64_t>(y)",
        VoronoiUtils::is_in_range::<i64>(y),
    );
    c.is_bool(
        "is_in_range<int32_t>(x)",
        VoronoiUtils::is_in_range::<i32>(x),
    );
    c.is_bool(
        "is_in_range<int32_t>(y)",
        VoronoiUtils::is_in_range::<i32>(y),
    );
    c.is_bool(
        "is_in_range<int64_t>(vertex)",
        VoronoiUtils::is_in_range_vertex::<i64>(&v),
    );
    c.is_bool(
        "is_in_range<int32_t>(vertex)",
        VoronoiUtils::is_in_range_vertex::<i32>(&v),
    );
    let safe = finite && x.abs() < 9.0e18 && y.abs() < 9.0e18;
    if !c.is_bool("safe", safe) {
        return;
    }
    if safe {
        let q = VoronoiUtils::to_point(&v);
        let r = VoronoiUtils::to_point_ptr(Some(&v));
        let got = (q.x, q.y, r.x, r.y);
        let want = (c.i64(), c.i64(), c.i64(), c.i64());
        c.same("to_point(vertex), to_point(&vertex)", &got, &want);
    }
    if finite {
        let w = VoronoiUtils::make_rotated_vertex(&v, angle);
        c.is_f64("make_rotated_vertex: x", w.x());
        c.is_f64("make_rotated_vertex: y", w.y());
        c.is_u32("make_rotated_vertex: colour", w.color() as u32);
    }
}

#[test]
fn vertex_utils_match() {
    replay("VRUT", vrut);
}

// ---------------------------------------------------------------- VRPI

fn vrpi(c: &mut Case) {
    match c.u8() {
        0 => {
            let empty = PolygonsPointIndex::default();
            c.is_pt("the empty index's p()", empty.p());
            c.is_bool("the empty index's initialized()", empty.initialized());
        }
        1 => {
            let polys = c.polys();
            let n = polys.iter().map(|p| p.points.len()).sum::<usize>();
            let want = c.u32();
            if !c.same("the number of points", &(n as u32), &want) {
                return;
            }
            for poly in 0..polys.len() as u32 {
                for point in 0..polys[poly as usize].points.len() as u32 {
                    let i = PolygonsPointIndex::new(&polys, poly, point);
                    let si = PolygonsSegmentIndex::new(&polys, poly, point);
                    let seg = PolygonsPointIndexSegmentLocator.call(&i);
                    let loc = PathsPointIndexLocator.call(&i);
                    let label = format!("point ({poly}, {point})");
                    let want = (c.u32(), c.u32());
                    if !c.same(&format!("{label}: the indexes"), &(poly, point), &want) {
                        return;
                    }
                    c.is_pt(&format!("{label}: p()"), i.p());
                    c.is_u32(&format!("{label}: next()"), i.next().point_idx);
                    c.is_u32(&format!("{label}: prev()"), i.prev().point_idx);
                    c.is_pt(&format!("{label}: from()"), si.from());
                    c.is_pt(&format!("{label}: to()"), si.to());
                    c.is_pt(&format!("{label}: the segment's start"), seg.0);
                    c.is_pt(&format!("{label}: the segment's end"), seg.1);
                    c.is_pt(&format!("{label}: the locator's point"), loc);
                    let hash = c.u64();
                    c.same(
                        &format!("{label}: std::hash"),
                        &polygons_point_index_hash(&i),
                        &hash,
                    );
                    let (opoly, opoint) = (c.u32(), c.u32());
                    let o = PolygonsPointIndex::new(&polys, opoly, opoint);
                    let got = (i.lt(&o), i == o, i != o);
                    let want = (c.bool(), c.bool(), c.bool());
                    c.same(
                        &format!("{label}: <, ==, != ({opoly}, {opoint})"),
                        &got,
                        &want,
                    );
                }
            }
        }
        form => panic!("{}: form {form}", c.label),
    }
}

#[test]
fn polygons_point_index_matches() {
    replay("VRPI", vrpi);
}

// ---------------------------------------------------------------- VRCR

/// Whether every edge around the cell is finite (the probe's `all_finite`).
fn all_finite(vd: &VoronoiDiagram, cell: &VoronoiCell) -> bool {
    let edges = vd.edges();
    let start = cell.incident_edge().unwrap();
    let mut e = start;
    loop {
        if edges[e].is_infinite(edges) {
            return false;
        }
        e = edges[e].next().unwrap();
        if e == start {
            return true;
        }
    }
}

/// The segment a cell lookup returned, as the probe writes it: a line's two points, or a
/// `PolygonsSegmentIndex`'s polygon and point index.
trait RecordedSegment: SegmentConcept<Point = Point> {
    fn check(&self, c: &mut Case, what: &str);
}

impl RecordedSegment for Line {
    fn check(&self, c: &mut Case, what: &str) {
        c.is_pt(&format!("{what}: a"), self.a);
        c.is_pt(&format!("{what}: b"), self.b);
    }
}

impl RecordedSegment for PolygonsSegmentIndex<'_> {
    fn check(&self, c: &mut Case, what: &str) {
        let got = (self.poly_idx, self.point_idx);
        let want = (c.u32(), c.u32());
        c.same(what, &got, &want);
    }
}

/// The non-degenerate cells of a raw diagram with their lookups (the probe's `put_cells`);
/// `psi` for the `PolygonsSegmentIndex` source, whose point cells also hold the point index and
/// the point cell range.
fn vrcr_cells<S: RecordedSegment>(
    c: &mut Case,
    vd: &VoronoiDiagram,
    segments: &[S],
    psi: Option<&[PolygonsSegmentIndex<'_>]>,
) -> bool {
    let n = vd
        .cells()
        .iter()
        .filter(|cell| !cell.is_degenerate())
        .count();
    let want = c.u32();
    if !c.same("the number of non-degenerate cells", &(n as u32), &want) {
        return false;
    }
    for (ci, cell) in vd.cells().iter().enumerate() {
        if cell.is_degenerate() {
            continue;
        }
        let got = (ci as u32, cell.contains_segment(), cell.contains_point());
        let want = (c.u32(), c.bool(), c.bool());
        if !c.same("a cell's index and kind", &got, &want) {
            return false;
        }
        let what = format!("cell {ci}");
        if cell.contains_segment() {
            match VoronoiUtils::get_source_segment(cell, segments) {
                Ok(s) => s.check(c, &format!("{what}: get_source_segment")),
                Err(e) => {
                    c.same(&format!("{what}: get_source_segment"), &Some(e), &None);
                    return false;
                }
            }
            let r = match VoronoiUtils::compute_segment_cell_range(vd, cell, segments) {
                Ok(r) => r,
                Err(e) => {
                    c.same(
                        &format!("{what}: compute_segment_cell_range"),
                        &Some(e),
                        &None,
                    );
                    return false;
                }
            };
            c.is_pt(
                &format!("{what}: range start"),
                r.source_segment_start_point,
            );
            c.is_pt(&format!("{what}: range end"), r.source_segment_end_point);
            let got = (link(r.edge_begin), link(r.edge_end), r.is_valid());
            let want = (c.i32(), c.i32(), c.bool());
            c.same(&format!("{what}: range edges, valid"), &got, &want);
        } else {
            match VoronoiUtils::get_source_point(cell, segments) {
                Ok(p) => c.is_pt(&format!("{what}: get_source_point"), p),
                Err(e) => {
                    c.same(&format!("{what}: get_source_point"), &Some(e), &None);
                    return false;
                }
            }
            if let Some(psi) = psi {
                match VoronoiUtils::get_source_point_index(cell, psi) {
                    Ok(i) => {
                        let got = (i.poly_idx, i.point_idx);
                        let want = (c.u32(), c.u32());
                        c.same(&format!("{what}: get_source_point_index"), &got, &want);
                    }
                    Err(e) => {
                        c.same(&format!("{what}: get_source_point_index"), &Some(e), &None);
                        return false;
                    }
                }
                let edges = vd.edges();
                let edge = &edges[cell.incident_edge().unwrap()];
                let called = edge.is_infinite(edges)
                    || !VoronoiUtils::is_in_range_edge::<i64, _>(vd, edge)
                    || all_finite(vd, cell);
                if !c.is_bool(&format!("{what}: compute_point_cell_range called"), called) {
                    return false;
                }
                if called {
                    let r = match VoronoiUtils::compute_point_cell_range(vd, cell, psi) {
                        Ok(r) => r,
                        Err(e) => {
                            c.same(
                                &format!("{what}: compute_point_cell_range"),
                                &Some(e),
                                &None,
                            );
                            return false;
                        }
                    };
                    c.is_pt(&format!("{what}: point range source"), r.source_point);
                    let got = (link(r.edge_begin), link(r.edge_end), r.is_valid());
                    let want = (c.i32(), c.i32(), c.bool());
                    c.same(&format!("{what}: point range edges, valid"), &got, &want);
                }
            }
        }
    }
    true
}

/// The lookups that throw (the probe's `put_throwing`): `u8` call, `u32` cell, `u64` range
/// length, then the outcome.
fn vrcr_throwing<S: SegmentConcept<Point = Point>>(
    c: &mut Case,
    vd: &VoronoiDiagram,
    segments: &[S],
    psi: Option<&[PolygonsSegmentIndex<'_>]>,
) {
    let mut calls: Vec<(u8, usize, usize)> = Vec::new();
    let all = segments.len();
    let (mut seg_done, mut pt_done, mut range_done) = (false, false, false);
    for (ci, cell) in vd.cells().iter().enumerate() {
        if cell.is_degenerate() {
            continue;
        }
        if cell.contains_segment() && !seg_done {
            seg_done = true;
            calls.push((1, ci, all));
            if psi.is_some() {
                calls.push((2, ci, all));
            }
        }
        if cell.contains_point() && !pt_done {
            pt_done = true;
            calls.push((0, ci, all));
        }
        if cell.contains_segment() && !range_done && cell.source_index() >= 1 {
            range_done = true;
            calls.push((0, ci, cell.source_index()));
        }
    }
    let want = c.u32();
    if !c.same("the number of throwing calls", &(calls.len() as u32), &want) {
        return;
    }
    for &(call, ci, length) in &calls {
        let got = (call, ci as u32, length as u64);
        let want = (c.u8(), c.u32(), c.u64());
        if !c.same("a throwing call", &got, &want) {
            return;
        }
        let cell = &vd.cells()[ci];
        let what = format!("call {call} on cell {ci} of {length}");
        match call {
            0 => {
                let r = VoronoiUtils::get_source_segment(cell, &segments[..length]);
                c.is_outcome(&what, &r);
            }
            1 => {
                let r = VoronoiUtils::get_source_point(cell, &segments[..length]);
                c.is_outcome(&what, &r);
            }
            _ => {
                let psi = psi.expect("call 2 is made on PolygonsSegmentIndex only");
                let r = VoronoiUtils::get_source_point_index(cell, &psi[..length]);
                c.is_outcome(&what, &r);
            }
        }
    }
}

fn vrcr(c: &mut Case) {
    let no_points: &[Point] = &[];
    match c.u8() {
        0 => {
            let lines = c.lines();
            let mut vd = VoronoiDiagram::new();
            vd.construct_voronoi_points_segments(no_points, &lines);
            if vrcr_cells(c, &vd, &lines, None) {
                vrcr_throwing(c, &vd, &lines, None);
            }
        }
        1 => {
            let polys = c.polys();
            let mut segs = Vec::new();
            for poly in 0..polys.len() as u32 {
                for point in 0..polys[poly as usize].points.len() as u32 {
                    segs.push(PolygonsSegmentIndex::new(&polys, poly, point));
                }
            }
            let mut vd = VoronoiDiagram::new();
            vd.construct_voronoi_points_segments(no_points, &segs);
            if vrcr_cells(c, &vd, &segs, Some(&segs)) {
                vrcr_throwing(c, &vd, &segs, Some(&segs));
            }
        }
        2 => {
            let pts = c.pts();
            let lines = c.lines();
            let mut vd = VoronoiDiagram::new();
            vd.construct_voronoi_points_segments(&pts, &lines);
            let n = vd
                .cells()
                .iter()
                .filter(|cell| !cell.is_degenerate())
                .count();
            let want = c.u32();
            if !c.same("the number of non-degenerate cells", &(n as u32), &want) {
                return;
            }
            for (ci, cell) in vd.cells().iter().enumerate() {
                if cell.is_degenerate() {
                    continue;
                }
                let got = (
                    u8::from(cell.contains_point()),
                    ci as u32,
                    lines.len() as u64,
                );
                let want = (c.u8(), c.u32(), c.u64());
                if !c.same("a cell's kind, index and line count", &got, &want) {
                    return;
                }
                let what = format!("cell {ci}");
                let end_point = cell.source_category() == SourceCategory::SEGMENT_START_POINT
                    || cell.source_category() == SourceCategory::SEGMENT_END_POINT;
                if cell.contains_point() && end_point && cell.source_index() >= lines.len() {
                    // The C++ reads past the lines there, which is undefined: the probe makes no
                    // call and writes `u8` 255.
                    let marker = c.u8();
                    c.same(&format!("{what}: the call not made"), &marker, &255);
                    continue;
                }
                if cell.contains_point() {
                    let r = VoronoiUtils::get_source_point(cell, &lines);
                    if c.is_outcome(&format!("{what}: get_source_point"), &r) {
                        c.is_pt(&format!("{what}: get_source_point"), r.unwrap());
                    }
                } else {
                    let r = VoronoiUtils::get_source_segment(cell, &lines);
                    if c.is_outcome(&format!("{what}: get_source_segment"), &r) {
                        r.unwrap().check(c, &format!("{what}: get_source_segment"));
                    }
                }
            }
        }
        source => panic!("{}: source {source}", c.label),
    }
}

#[test]
fn cell_lookups_match() {
    replay("VRCR", vrcr);
}

// ---------------------------------------------------------------- VRDZ

fn vrdz(c: &mut Case) {
    let source = c.pt();
    let from = c.pt();
    let to = c.pt();
    let start = c.pt();
    let end = c.pt();
    let step = c.i64();
    let angle = c.f32();
    let polys: Polygons = vec![Polygon::new(vec![from, to])];
    let seg = PolygonsSegmentIndex::new(&polys, 0, 0);
    log_start();
    let out = VoronoiUtils::discretize_parabola(source, &seg, start, end, step, angle);
    let log = log_take();
    log_stop();
    let want = c.pts();
    c.same("the points", &out, &want);
    let want = c.str();
    c.same("the log", &log, &want);
}

#[test]
fn discretize_parabola_matches() {
    replay("VRDZ", vrdz);
}

// ---------------------------------------------------------------- voronoi-4: values

impl Case<'_, '_> {
    /// The state block (the probe's `put_state`): `u8` issue, `u8` state, `bool` valid, `u64`
    /// vertices, edges and cells; true when it is the C++'s.
    fn is_state(&mut self, what: &str, vd: &VoronoiDiagram) -> bool {
        let got = (
            vd.get_issue_type() as u8,
            vd.get_state() as u8,
            vd.is_valid(),
            vd.num_vertices() as u64,
            vd.num_edges() as u64,
            vd.num_cells() as u64,
        );
        let want = (
            self.u8(),
            self.u8(),
            self.bool(),
            self.u64(),
            self.u64(),
            self.u64(),
        );
        self.same(
            &format!("{what}: issue, state, valid, vertices, edges, cells"),
            &got,
            &want,
        )
    }
    /// A diagram through the accessors.
    fn is_dia_of(&mut self, vd: &VoronoiDiagram) {
        self.is_dia(&dia_of(vd.cells(), vd.vertices(), vd.edges()));
    }
    /// An error the record may hold: `u8` 0 when the calls returned, else `u8` 1 and the error
    /// (`put_error`: its `u8` code and `what()`); true when it is the C++'s.
    fn is_error<T>(&mut self, what: &str, got: &Result<T, VoronoiError>) -> bool {
        let want = match self.u8() {
            0 => None,
            _ => {
                let code = self.u8();
                Some((code, self.str()))
            }
        };
        let got = got.as_ref().err().map(error_of);
        self.same(what, &got, &want)
    }
    /// Bytes: a `u32` count, then each `u8`; the count, then the first byte that differs.
    fn is_bytes(&mut self, what: &str, got: &[u8]) {
        let n = self.u32();
        let want: Vec<u8> = (0..n).map(|_| self.u8()).collect();
        if !self.same(&format!("the number of {what}"), &got.len(), &want.len()) {
            return;
        }
        if let Some(k) = (0..got.len()).find(|&k| got[k] != want[k]) {
            self.same(&format!("{what} {k}"), &got[k], &want[k]);
        }
    }
    /// A check's answer (the probe's `put_check`): `u8` 0 false, 1 true, 2 then the error.
    fn is_check(&mut self, what: &str, got: &Result<bool, VoronoiError>) {
        let want = match self.u8() {
            0 => Ok(false),
            1 => Ok(true),
            _ => {
                let code = self.u8();
                Err((code, self.str()))
            }
        };
        let got = got.as_ref().map(|b| *b).map_err(error_of);
        self.same(what, &got, &want);
    }
    /// Thick polylines (the probe's `put_thick`): a `u32` count, then each one's points, a `u32`
    /// count of widths and each `f64`, and the two end flags.
    fn read_thick(&mut self) -> Vec<Thick> {
        let n = self.u32();
        (0..n)
            .map(|_| {
                let points = self.pts();
                let nw = self.u32();
                let width = (0..nw).map(|_| F(self.f64())).collect();
                let first = self.bool();
                Thick(points, width, first, self.bool())
            })
            .collect()
    }
    /// Compares thick polylines: the count, then the first that differs.
    fn is_thick(&mut self, what: &str, got: &ThickPolylines) {
        let want = self.read_thick();
        let got: Vec<Thick> = got
            .iter()
            .map(|t| {
                Thick(
                    t.points.clone(),
                    t.width.iter().map(|&w| F(w)).collect(),
                    t.endpoints.0,
                    t.endpoints.1,
                )
            })
            .collect();
        if !self.same(&format!("{what}: the number"), &got.len(), &want.len()) {
            return;
        }
        if let Some(k) = (0..got.len()).find(|&k| got[k] != want[k]) {
            self.same(&format!("{what}: polyline {k}"), &got[k], &want[k]);
        }
    }
}

/// A thick polyline as a record holds it: points, widths by their bits, the two end flags.
#[derive(Clone, PartialEq, Debug)]
struct Thick(Points, Vec<F>, bool, bool);

/// The `PolygonsSegmentIndex` of every point of the polygons, in order (the probe's `psi_of`).
fn psi_of(polys: &Polygons) -> Vec<PolygonsSegmentIndex<'_>> {
    let mut segs = Vec::new();
    for poly in 0..polys.len() as u32 {
        for point in 0..polys[poly as usize].points.len() as u32 {
            segs.push(PolygonsSegmentIndex::new(polys, poly, point));
        }
    }
    segs
}

// ---------------------------------------------------------------- VRGD

/// The angles of `try_to_repair_degenerated_voronoi_diagram`, in its order.
const FIX_ANGLES: [f64; 4] = [PI / 6.0, PI / 5.0, PI / 7.0, PI / 11.0];

/// Whether the construction's repair ran (the probe's `repaired`).
fn repaired(vd: &VoronoiDiagram) -> bool {
    matches!(
        vd.get_state(),
        State::RepairSuccessful | State::RepairUnsuccessful
    )
}

/// The rotations the repair ran, each the issue its detection found (the probe's `rotations`):
/// none when the repair did not run, else the angles in turn on one fresh object until one finds
/// no issue.
fn rotations<S: SegmentConcept<Point = Point>>(
    segs: &[S],
    vd: &VoronoiDiagram,
) -> Result<Vec<u8>, VoronoiError> {
    let mut out = Vec::new();
    if !repaired(vd) {
        return Ok(out);
    }
    let mut scratch = VoronoiDiagram::new();
    for a in FIX_ANGLES {
        let t = scratch.try_to_repair_degenerated_voronoi_diagram_by_rotation(segs, a)?;
        out.push(t as u8);
        if t == IssueType::NoIssueDetected {
            break;
        }
    }
    Ok(out)
}

/// One VRGD record from its input on: the construction, the rotations, the diagram, the log and
/// the second construction. Returns at the first mismatch, after which the record's later fields
/// cannot be read in step.
fn vrgd_run<S: SegmentConcept<Point = Point>>(c: &mut Case, source: u8, repair: bool, segs: &[S]) {
    let mut vd = VoronoiDiagram::new();
    let first = vd.construct_voronoi_segments(segs, repair);
    let mut log = log_take();
    if let Err(e) = &first {
        // The C++'s first construction returned on every record (it has no error field).
        c.same("construct_voronoi", &Some(error_of(e)), &None);
        return;
    }
    if !c.is_state("construct_voronoi", &vd) {
        return;
    }
    if repair {
        let got = rotations(segs, &vd).map_err(|e| error_of(&e));
        let n = c.u8();
        let want: Vec<u8> = (0..n).map(|_| c.u8()).collect();
        if !c.same("the rotations", &got, &Ok(want)) {
            return;
        }
    }
    let has = repaired(&vd) || (source == 2 && !repair);
    if !c.is_bool("a diagram is recorded", has) {
        return;
    }
    if has {
        c.is_dia_of(&vd);
    }
    log.push_str(&log_take());
    let want = c.str();
    c.same("the log", &log, &want);
    // MedialAxis's second construction: the search's draws always have their ExPolygon, a shape
    // only when it is one ExPolygon, the polygons and the line sets never.
    let second = c.bool();
    let possible = repair && !vd.is_valid();
    let allowed = match source {
        3 => second == possible,
        0 => !second || possible,
        _ => !second,
    };
    if !c.same(
        "a second construction as the source allows",
        &allowed,
        &true,
    ) || !second
    {
        return;
    }
    let again = c.lines();
    let r = vd.construct_voronoi_segments(&again, true);
    if !c.is_error("the second construction", &r) {
        return;
    }
    if r.is_ok() {
        if !c.is_state("the second construction", &vd) {
            return;
        }
        c.is_dia_of(&vd);
    }
    let log = log_take();
    let want = c.str();
    c.same("the second construction's log", &log, &want);
}

fn vrgd(c: &mut Case) {
    let source = c.u8();
    if source == 3 {
        // The search's draw, its family and its turn: where the lines came from.
        let _ = (c.u32(), c.u8(), c.u8());
    }
    let repair = c.bool();
    log_start();
    if source == 1 {
        let polys = c.polys();
        let segs = psi_of(&polys);
        vrgd_run(c, source, repair, &segs);
    } else {
        let lines = c.lines();
        vrgd_run(c, source, repair, &lines);
    }
    log_stop();
}

#[test]
fn construction_and_repair_match() {
    replay("VRGD", vrgd);
}

// ---------------------------------------------------------------- VRCG

/// The two checks of one raw diagram.
fn vrcg_checks<S: SegmentConcept<Point = Point>>(c: &mut Case, vd: &VoronoiDiagram, segs: &[S]) {
    c.is_check(
        "is_voronoi_diagram_planar_angle",
        &VoronoiUtilsCgal::is_voronoi_diagram_planar_angle(vd, segs),
    );
    c.is_check(
        "is_voronoi_diagram_planar_intersection",
        &Ok(VoronoiUtilsCgal::is_voronoi_diagram_planar_intersection(vd)),
    );
}

fn vrcg(c: &mut Case) {
    const RAW: &str = "a construction with the repair off runs no detection";
    let mut vd = VoronoiDiagram::new();
    match c.u8() {
        0 => {
            let lines = c.lines();
            vd.construct_voronoi_segments(&lines, false).expect(RAW);
            vrcg_checks(c, &vd, &lines);
        }
        1 => {
            let polys = c.polys();
            let segs = psi_of(&polys);
            vd.construct_voronoi_segments(&segs, false).expect(RAW);
            vrcg_checks(c, &vd, &segs);
        }
        2 => {
            let pts = c.pts();
            vd.construct_voronoi_points(&pts);
            let none: Lines = Vec::new();
            vrcg_checks(c, &vd, &none);
        }
        source => panic!("{}: source {source}", c.label),
    }
}

#[test]
fn planarity_checks_match() {
    replay("VRCG", vrcg);
}

// ---------------------------------------------------------------- VRAN

fn vran(c: &mut Case) {
    let lines = c.lines();
    log_start();
    let mut vd = VoronoiDiagram::new();
    let r = vd.construct_voronoi_segments(&lines, true);
    if r.is_ok() {
        annotate_inside_outside(&vd, &lines);
    }
    let log = log_take();
    log_stop();
    if let Err(e) = &r {
        // The C++'s construction returned on every record (it has no error field).
        c.same("construct_voronoi", &Some(error_of(e)), &None);
        return;
    }
    if !c.is_state("construct_voronoi", &vd) {
        return;
    }
    let cells: Vec<u8> = vd.cells().iter().map(|x| x.color() as u8).collect();
    c.is_bytes("cell colours", &cells);
    let vertices: Vec<u8> = vd.vertices().iter().map(|x| x.color() as u8).collect();
    c.is_bytes("vertex colours", &vertices);
    let edges: Vec<u8> = vd.edges().iter().map(|x| x.color() as u8).collect();
    c.is_bytes("edge colours", &edges);
    let want = c.str();
    c.same("the log", &log, &want);
}

#[test]
fn inside_outside_annotations_match() {
    replay("VRAN", vran);
}

// ---------------------------------------------------------------- VRMA

/// Whether the polylines have the thick polylines' points, one for one (the probe's
/// `same_points`).
fn same_points(a: &Polylines, b: &ThickPolylines) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.points == y.points)
}

/// The probe's four calls, in its order: each a fresh object or call on the same region.
type MedialOut = (ThickPolylines, Polylines, ThickPolylines, Polylines);

fn vrma_calls(ex: &ExPolygon, min_width: f64, max_width: f64) -> Result<MedialOut, VoronoiError> {
    let mut build = ThickPolylines::new();
    MedialAxis::new(min_width, max_width, ex).build(&mut build)?;
    let mut build_lines = Polylines::new();
    MedialAxis::new(min_width, max_width, ex).build_polylines(&mut build_lines)?;
    let mut medial = ThickPolylines::new();
    ex.medial_axis(min_width, max_width, &mut medial)?;
    let mut medial_lines = Polylines::new();
    ex.medial_axis_polylines(min_width, max_width, &mut medial_lines)?;
    Ok((build, build_lines, medial, medial_lines))
}

fn vrma(c: &mut Case) {
    let contour = c.pts();
    let holes = c.polys();
    let mut ex = ExPolygon::new(Polygon::new(contour));
    ex.holes = holes;
    let (min_width, max_width) = (c.f64(), c.f64());
    log_start();
    let result = vrma_calls(&ex, min_width, max_width);
    let log = log_take();
    log_stop();
    if !c.is_error("the medial axis calls", &result) {
        return;
    }
    if let Ok((build, build_lines, medial, medial_lines)) = &result {
        c.is_thick("MedialAxis::build", build);
        c.is_thick("ExPolygon::medial_axis", medial);
        c.is_bool(
            "build(Polylines*) has build(ThickPolylines*)'s points",
            same_points(build_lines, build),
        );
        c.is_bool(
            "medial_axis(Polylines*) has medial_axis(ThickPolylines*)'s points",
            same_points(medial_lines, medial),
        );
    }
    let want = c.str();
    c.same("the log", &log, &want);
}

#[test]
fn medial_axis_matches() {
    replay("VRMA", vrma);
}

// ---------------------------------------------------------------- the kinds

/// Every kind voronoi-1 to voronoi-4 replay is recorded, and the file holds no other kind, so no
/// record goes unreplayed.
#[test]
fn every_kind_has_a_piece() {
    let file = file();
    let pieces = || {
        VORONOI_1
            .iter()
            .chain(VORONOI_2.iter())
            .chain(VORONOI_3.iter())
            .chain(VORONOI_4.iter())
    };
    for kind in pieces() {
        assert!(
            file.of_kind(kind).next().is_some(),
            "{}: no {kind} record",
            file.path.display()
        );
    }
    for rec in &file.records {
        let kind = rec.kind();
        assert!(
            pieces().any(|k| *k == kind),
            "{}: the kind {kind} is no piece's (parity/sop1/voronoi.md)",
            file.path.display()
        );
    }
}
