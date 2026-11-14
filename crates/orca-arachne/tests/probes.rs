// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The probe gate for orca-arachne: the records of `arachne.sop1` (`parity/sop1/arachne.md`),
//! each rebuilt from the crate and compared whole, field by field, every `float` and `double` by
//! its bits, the logs record by record (added by arachne-1, 2026-11-13):
//!
//! - ANBS: a beading strategy, built by `BeadingStrategyFactory::makeStrategy` or as a chain of
//!   the five strategies: its construction's log, `toString`, thresholds and angle, for each lower
//!   bead count its optimal and transition thicknesses, transition length, anchor and nonlinear
//!   thicknesses, and for each thickness its optimal bead count and `compute` for the counts the
//!   record names, then the log of all that;
//! - ANLA: `LinearAlg2D::pointIsLeftOfLine` and `getAngleLeft`;
//! - ANXL: an `ExtrusionLine`'s length, `is_contour`, `area`, `to_thick_polyline`, `to_polygon`,
//!   `to_points` and `simplify` for each argument triple;
//! - ANAE: `ExtrusionLine::calculateExtrusionAreaDeviationError`.
//!
//! and the kinds of arachne-2 (added 2026-11-14):
//!
//! - ANSQ: `SquareGrid`'s coordinates and the cells its two walks hand over;
//! - ANSP: a `SparsePointGrid`'s iteration order (MSVC's `unordered_multimap`'s), `getNearby` and
//!   `processNearby`;
//! - ANSL: the `SparseLineGrid` of `WallToolPaths.cpp`'s `createLocToLineGrid` (the crate's
//!   [`create_loc_to_line_grid`] since arachne-4): its order and `getNearby`;
//! - ANPS: `PolylineStitcher::stitch`'s lines and polygons;
//! - ANGR: the graph's operations (`collapseSmallEdges`, `makeRib`, `insertNode` on copies,
//!   `isLocalMaximum`, `isUpward` and `distToGoUp`), each graph built from the record and written
//!   back as the probe writes it, node by node and edge by edge.
//!
//! and the kinds of arachne-3 (added 2026-11-14):
//!
//! - ANSC: a `SkeletalTrapezoidation` built from the record's outline, strategy and parameters,
//!   with the log of the strategy's construction and the constructor's;
//! - ANST stages 0 to 7: the graph after the constructor (stage 0) and after each step from
//!   `updateIsCentral` to `generateAllTransitionEnds`, each step run on the graph built from the
//!   stage before's record, and its log ([`skeletal_trapezoidations_match`]).
//!
//! and the kinds of arachne-4 (added 2026-11-14):
//!
//! - ANST stages 8 to 10: `applyTransitions` (stage 8) and `generateExtraRibs` (stage 9) on the
//!   graph built from the stage before's record, as above (stage 9's after the transition lists
//!   expired, as the probe's block ends before it), and `generateSegments` (stage 10): its
//!   toolpaths and log, and whether a fresh trapezoidation's `generateToolpaths` gives the same;
//! - ANWS: `WallToolPaths`' steps after the trapezoidation (`stitchToolPaths`, `removeSmallLines`,
//!   `simplifyToolPaths`, `removeEmptyToolPaths`) on stage 10's toolpaths, and whether
//!   `getToolPaths` on the outline gives them without the zero-width insets (the outline taken from
//!   the ANWT record of the same shape and setup; [`wall_steps_match`]);
//! - ANIP: the two `interpolate` overloads;
//! - ANWH: the outline preparation of `WallToolPaths::generate`, as a chain run on its own
//!   outputs and one step at a time;
//! - ANWT: `WallToolPaths` whole: `getToolPaths`, `getInnerContour`, their log, and
//!   `getRegionOrder` both ways;
//! - ANMP: `make_paths_params`.
//!
//! A record of no piece fails [`every_kind_has_a_piece`]. The committed subset (`parity/probes/`)
//! runs by default; `SKETCH_ORCA_PROBES_DEEP=<folder>` replays the full set from that folder
//! (`<work>/parity/probes`).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::rc::Rc;
use std::sync::Once;

use orca_arachne::beading::{
    Beading, BeadingStrategyPtr, DistributedBeadingStrategy, LimitedBeadingStrategy,
    OuterWallInsetBeadingStrategy, RedistributeBeadingStrategy, WideningBeadingStrategy,
    make_strategy,
};
use orca_arachne::extrusion_junction::{ExtrusionJunction, LineJunctions};
use orca_arachne::extrusion_line::{ExtrusionLine, to_points, to_polygon, to_thick_polyline};
use orca_arachne::half_edge::{EdgeId, NodeId};
use orca_arachne::linear_alg_2d::{get_angle_left, point_is_left_of_line};
use orca_arachne::polyline_stitcher::stitch;
use orca_arachne::skeletal_trapezoidation::SkeletalTrapezoidation;
use orca_arachne::skeletal_trapezoidation_edge::{
    EdgeType, Shared, SkeletalTrapezoidationEdge, TransitionEnd, TransitionEnds, TransitionMiddle,
    TransitionMiddles,
};
use orca_arachne::skeletal_trapezoidation_graph::{
    STHalfEdge, STHalfEdgeNode, SkeletalTrapezoidationGraph,
};
use orca_arachne::skeletal_trapezoidation_joint::{
    BeadingPropagation, SkeletalTrapezoidationJoint,
};
use orca_arachne::sparse_point_grid::SparsePointGrid;
use orca_arachne::square_grid::SquareGrid;
use orca_arachne::wall_tool_paths::{
    WallToolPaths, WallToolPathsParams, create_loc_to_line_grid, fix_self_intersections,
    make_paths_params, remove_colinear_edges, remove_degenerate_verts, remove_small_areas, simplify,
};
use orca_config::{PrintConfig, PrintObjectConfig};
use orca_geom::clipper_utils::{DEFAULT_JOIN_TYPE, DEFAULT_MITER_LIMIT, offset, union_};
use orca_geom::point::Point;
use orca_geom::polygon::{Polygon, Polygons};
use orca_mesh::log::Severity;
use orca_probe::{Mismatches, ProbeFile, Reader, Record, load_probe};

fn file() -> ProbeFile {
    load_probe("arachne").unwrap_or_else(|e| panic!("{e}"))
}

/// The kinds of the piece arachne-1, which this file replays.
const ARACHNE1_KINDS: [&str; 4] = ["ANBS", "ANLA", "ANXL", "ANAE"];
/// The kinds of the piece arachne-2 (the graph, the grids and the stitcher).
const ARACHNE2_KINDS: [&str; 5] = ["ANSQ", "ANSP", "ANSL", "ANPS", "ANGR"];
/// The kinds of the pieces arachne-3 and arachne-4 (ANST's stages 0 to 7 are arachne-3's, 8 to 10
/// arachne-4's).
const ARACHNE3_4_KINDS: [&str; 7] = ["ANSC", "ANST", "ANWS", "ANIP", "ANWH", "ANWT", "ANMP"];

// ---------------------------------------------------------------- the logs

/// What the crate logged on this thread while a capture runs: each record's severity (trace 0 to
/// fatal 5) and message, and the text written to `std::cerr`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct LogText {
    records: Vec<(u8, String)>,
    cerr: String,
}

thread_local! {
    static CAPTURE: RefCell<Option<LogText>> = const { RefCell::new(None) };
}

fn severity_code(s: Severity) -> u8 {
    match s {
        Severity::Trace => 0,
        Severity::Debug => 1,
        Severity::Info => 2,
        Severity::Warning => 3,
        Severity::Error => 4,
        Severity::Fatal => 5,
    }
}

/// The lowest severity the probe's capture sees. Its sink takes every record Boost.Log's core
/// passes, but libslic3r sets the core's filter when it is loaded (`utils.cpp`, `RunOnInit`:
/// `set_logging_level(2)`, warning and above in a release version) and the probe never changes it,
/// so the factory's trace records never reach the probe's records, while `LimitedBeadingStrategy`'s
/// warnings do.
const PROBE_LEVEL: Severity = Severity::Warning;

/// Sets, once for the test program, the sinks that hand each record the probe's capture would see
/// (and the `std::cerr` text) to the capture of the thread that wrote it.
fn install_sinks() {
    static SINKS: Once = Once::new();
    SINKS.call_once(|| {
        orca_mesh::log::set_sink(Some(Box::new(|severity, message| {
            if severity < PROBE_LEVEL {
                return;
            }
            CAPTURE.with(|c| {
                if let Some(log) = c.borrow_mut().as_mut() {
                    log.records
                        .push((severity_code(severity), message.to_owned()));
                }
            });
        })));
        orca_mesh::log::set_stderr_sink(Some(Box::new(|text| {
            CAPTURE.with(|c| {
                if let Some(log) = c.borrow_mut().as_mut() {
                    log.cerr.push_str(text);
                }
            });
        })));
    });
}

/// Starts this thread's capture (`log_start`).
fn log_start() {
    install_sinks();
    CAPTURE.with(|c| *c.borrow_mut() = Some(LogText::default()));
}

/// What came since the last take (`log_take`).
fn log_take() -> LogText {
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

// ---------------------------------------------------------------- reading and comparing

/// An `f64` compared by its bits.
#[derive(Clone, Copy, Debug)]
struct F64(f64);

impl PartialEq for F64 {
    fn eq(&self, o: &Self) -> bool {
        self.0.to_bits() == o.0.to_bits()
    }
}

/// An `f32` compared by its bits.
#[derive(Clone, Copy, Debug)]
struct F32(f32);

impl PartialEq for F32 {
    fn eq(&self, o: &Self) -> bool {
        self.0.to_bits() == o.0.to_bits()
    }
}

/// One record being replayed: its reader, the test's mismatches and the record's label.
struct Case<'r, 'm> {
    r: Reader<'r>,
    m: &'m mut Mismatches,
    label: String,
}

impl Case<'_, '_> {
    fn u8(&mut self) -> u8 {
        self.r.u8().unwrap()
    }
    fn i32(&mut self) -> i32 {
        self.r.i32().unwrap()
    }
    fn i64(&mut self) -> i64 {
        self.r.i64().unwrap()
    }
    fn u64(&mut self) -> u64 {
        self.r.u64().unwrap()
    }
    fn count(&mut self) -> usize {
        self.r.count().unwrap()
    }
    fn bool(&mut self) -> bool {
        self.r.bool().unwrap()
    }
    fn f32(&mut self) -> f32 {
        self.r.f32().unwrap()
    }
    fn f64(&mut self) -> f64 {
        self.r.f64().unwrap()
    }
    fn str(&mut self) -> String {
        self.r.str().unwrap()
    }
    fn point(&mut self) -> Point {
        let x = self.i64();
        Point::new(x, self.i64())
    }
    fn points(&mut self) -> Vec<Point> {
        let n = self.count();
        (0..n).map(|_| self.point()).collect()
    }
    fn i64s(&mut self) -> Vec<i64> {
        let n = self.count();
        (0..n).map(|_| self.i64()).collect()
    }
    /// A junction: `point` p, `i64` width, `u64` perimeter index.
    fn junction(&mut self) -> ExtrusionJunction {
        let p = self.point();
        let w = self.i64();
        let perimeter_index = usize::try_from(self.u64()).unwrap();
        ExtrusionJunction {
            p,
            w,
            perimeter_index,
        }
    }
    /// A line: `u64` inset, `bool` odd, `bool` closed, `vec<junction>`.
    fn line(&mut self) -> ExtrusionLine {
        let inset_idx = usize::try_from(self.u64()).unwrap();
        let is_odd = self.bool();
        let is_closed = self.bool();
        let n = self.count();
        let junctions = (0..n).map(|_| self.junction()).collect();
        ExtrusionLine {
            inset_idx,
            is_odd,
            is_closed,
            junctions,
        }
    }
    /// A beading: `i64` total, `vec<i64>` widths, `vec<i64>` locations, `i64` left over.
    fn beading(&mut self) -> Beading {
        let total_thickness = self.i64();
        let bead_widths = self.i64s();
        let toolpath_locations = self.i64s();
        let left_over = self.i64();
        Beading {
            total_thickness,
            bead_widths,
            toolpath_locations,
            left_over,
        }
    }
    /// A log: a `u64` count of records, each `u8` severity and `str` message, then `str` cerr.
    fn log(&mut self) -> LogText {
        let n = self.count();
        let records = (0..n)
            .map(|_| {
                let s = self.u8();
                (s, self.str())
            })
            .collect();
        LogText {
            records,
            cerr: self.str(),
        }
    }

    fn same<T: PartialEq + Debug>(&mut self, what: &str, got: &T, want: &T) {
        let label = &self.label;
        self.m.check(got == want, || {
            format!("{label}: {what}: got {got:?}, want {want:?}")
        });
    }
    fn is_i64(&mut self, what: &str, got: i64) {
        let want = self.i64();
        self.same(what, &got, &want);
    }
    fn is_bool(&mut self, what: &str, got: bool) {
        let want = self.bool();
        self.same(what, &got, &want);
    }
    fn is_f32(&mut self, what: &str, got: f32) {
        let want = F32(self.f32());
        self.same(what, &F32(got), &want);
    }
    fn is_f64(&mut self, what: &str, got: f64) {
        let want = F64(self.f64());
        self.same(what, &F64(got), &want);
    }
    fn is_str(&mut self, what: &str, got: &str) {
        let want = self.str();
        self.same(what, &got.to_owned(), &want);
    }
    fn is_i64s(&mut self, what: &str, got: &[i64]) {
        let want = self.i64s();
        self.same(what, &got.to_vec(), &want);
    }
    fn is_points(&mut self, what: &str, got: &[Point]) {
        let want = self.points();
        self.same(what, &got.to_vec(), &want);
    }
    fn is_line(&mut self, what: &str, got: &ExtrusionLine) {
        let want = self.line();
        self.same(what, got, &want);
    }
    fn is_beading(&mut self, what: &str, got: &Beading) {
        let want = self.beading();
        self.same(what, got, &want);
    }
    fn is_log(&mut self, what: &str, got: &LogText) {
        let want = self.log();
        self.same(what, got, &want);
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

// ---------------------------------------------------------------- ANBS

/// The builder of a strategy as the record gives it (`parity/sop1/arachne.md`, "a strategy"), which
/// builds it as the probe's `make_strategy` does; the arguments are read first, so that the build
/// runs inside the log capture alone.
fn strategy(c: &mut Case) -> Builder {
    let factory = c.u8();
    if factory == 1 {
        let outer = c.i64();
        let inner = c.i64();
        let transition_length = c.i64();
        let transitioning_angle = c.f32();
        let print_thin_walls = c.bool();
        let min_bead_width = c.i64();
        let min_feature_size = c.i64();
        let split = c.f64();
        let add = c.f64();
        let max_bead_count = c.i64();
        let outer_wall_offset = c.i64();
        let inward = c.i32();
        let ratio = c.f64();
        return Box::new(move || {
            make_strategy(
                outer,
                inner,
                transition_length,
                transitioning_angle,
                print_thin_walls,
                min_bead_width,
                min_feature_size,
                split,
                add,
                max_bead_count,
                outer_wall_offset,
                inward,
                ratio,
            )
        });
    }
    assert_eq!(factory, 0, "{}: a strategy of form {factory}", c.label);
    let n = c.count();
    let mut layers = Vec::with_capacity(n);
    for _ in 0..n {
        let kind = c.u8();
        let layer = match kind {
            0 => {
                let a = c.i64();
                let b = c.i64();
                let f = c.f64();
                let g = c.f64();
                let h = c.f64();
                Layer::Distributed(a, b, f, g, h, c.i32())
            }
            1 => {
                let a = c.i64();
                Layer::Redistribute(a, c.f64())
            }
            2 => {
                let a = c.i64();
                Layer::Widening(a, c.i64())
            }
            3 => Layer::OuterWallInset(c.i64()),
            4 => Layer::Limited(c.i64()),
            k => panic!("{}: a strategy of kind {k}", c.label),
        };
        layers.push(layer);
    }
    Box::new(move || {
        let mut ret: Option<BeadingStrategyPtr> = None;
        for layer in layers {
            ret = Some(match layer {
                Layer::Distributed(a, b, f, g, h, radius) => {
                    Box::new(DistributedBeadingStrategy::new(a, b, f, g, h, radius))
                }
                Layer::Redistribute(a, f) => Box::new(RedistributeBeadingStrategy::new(
                    a,
                    f,
                    ret.expect("a chain starts with a distributed strategy"),
                )),
                Layer::Widening(a, b) => Box::new(WideningBeadingStrategy::new(
                    ret.expect("a chain starts with a distributed strategy"),
                    a,
                    b,
                )),
                Layer::OuterWallInset(a) => Box::new(OuterWallInsetBeadingStrategy::new(
                    a,
                    ret.expect("a chain starts with a distributed strategy"),
                )),
                Layer::Limited(a) => Box::new(LimitedBeadingStrategy::new(
                    a,
                    ret.expect("a chain starts with a distributed strategy"),
                )),
            });
        }
        ret.expect("a chain of at least one strategy")
    })
}

/// One strategy of a chain the probe builds itself, with its constructor's arguments.
enum Layer {
    Distributed(i64, i64, f64, f64, f64, i32),
    Redistribute(i64, f64),
    Widening(i64, i64),
    OuterWallInset(i64),
    Limited(i64),
}

/// Builds a strategy.
type Builder = Box<dyn FnOnce() -> BeadingStrategyPtr>;

fn anbs(c: &mut Case) {
    let name = c.str();
    c.label = format!("{} ({name})", c.label);
    let make = strategy(c);
    log_start();
    let strat = make();
    let built = log_take();
    c.is_log("the construction's log", &built);
    c.is_str("toString", &strat.to_string());
    c.is_f64(
        "getSplitMiddleThreshold",
        strat.get_split_middle_threshold(),
    );
    c.is_f64("getTransitioningAngle", strat.get_transitioning_angle());
    let k_count = c.u64();
    for k in 0..k_count {
        let k = i64::try_from(k).unwrap();
        c.is_i64(
            &format!("getOptimalThickness({k})"),
            strat.get_optimal_thickness(k),
        );
        c.is_i64(
            &format!("getTransitionThickness({k})"),
            strat.get_transition_thickness(k),
        );
        c.is_i64(
            &format!("getTransitioningLength({k})"),
            strat.get_transitioning_length(k),
        );
        c.is_f32(
            &format!("getTransitionAnchorPos({k})"),
            strat.get_transition_anchor_pos(k),
        );
        c.is_i64s(
            &format!("getNonlinearThicknesses({k})"),
            &strat.get_nonlinear_thicknesses(k),
        );
    }
    let samples = c.count();
    for _ in 0..samples {
        let t = c.i64();
        c.is_i64(
            &format!("getOptimalBeadCount({t})"),
            strat.get_optimal_bead_count(t),
        );
        let n = c.count();
        for _ in 0..n {
            let count = c.i64();
            c.is_beading(&format!("compute({t}, {count})"), &strat.compute(t, count));
        }
    }
    let after = log_take();
    log_stop();
    c.is_log("the log", &after);
}

#[test]
fn beading_strategies_match() {
    replay("ANBS", anbs);
}

// ---------------------------------------------------------------- ANLA

fn anla(c: &mut Case) {
    let p = c.point();
    let a = c.point();
    let b = c.point();
    let q = c.point();
    c.is_i64("pointIsLeftOfLine", point_is_left_of_line(p, a, b));
    c.is_f32("getAngleLeft", get_angle_left(a, b, q));
}

#[test]
fn linear_alg_2d_matches() {
    replay("ANLA", anla);
}

// ---------------------------------------------------------------- ANXL

fn anxl(c: &mut Case) {
    let line = c.line();
    c.is_i64("getLength", line.get_length());
    c.is_bool("is_contour", line.is_contour());
    let closed = c.bool();
    c.same("closed", &line.is_closed, &closed);
    if closed {
        c.is_f64("area", line.area());
    }
    let thick = c.bool();
    c.same("thick", &(line.junctions.len() >= 2), &thick);
    if thick {
        let t = to_thick_polyline(&line.junctions);
        c.is_points("to_thick_polyline points", &t.points);
        let n = c.count();
        let want: Vec<F64> = (0..n).map(|_| F64(c.f64())).collect();
        let got: Vec<F64> = t.width.iter().copied().map(F64).collect();
        c.same("to_thick_polyline widths", &got, &want);
        c.is_bool("first endpoint", t.endpoints.0);
        c.is_bool("last endpoint", t.endpoints.1);
    }
    let poly = c.bool();
    let rule = line.is_closed
        && line.junctions.len() >= 3
        && line.junctions[0].p == line.junctions[line.junctions.len() - 1].p;
    c.same("polygon", &rule, &poly);
    if poly {
        c.is_points("to_polygon", &to_polygon(&line).points);
    }
    c.is_points("to_points", &to_points(&line));
    let n = c.count();
    for _ in 0..n {
        let seg2 = c.i64();
        let err2 = c.i64();
        let area = c.i64();
        let mut s = line.clone();
        s.simplify(seg2, err2, area);
        c.is_line(&format!("simplify({seg2}, {err2}, {area})"), &s);
    }
}

#[test]
fn extrusion_lines_match() {
    replay("ANXL", anxl);
}

// ---------------------------------------------------------------- ANAE

fn anae(c: &mut Case) {
    let a = c.junction();
    let b = c.junction();
    let q = c.junction();
    c.is_i64(
        "calculateExtrusionAreaDeviationError",
        ExtrusionLine::calculate_extrusion_area_deviation_error(a, b, q),
    );
}

#[test]
fn extrusion_area_errors_match() {
    replay("ANAE", anae);
}

// ---------------------------------------------------------------- ANSQ

fn ansq(c: &mut Case) {
    let cell = c.i64();
    let op = c.u8();
    let g = SquareGrid::new(cell);
    match op {
        0 => {
            let v = c.i64();
            c.is_i64(&format!("toGridCoord({v})"), g.to_grid_coord(v));
        }
        1 => {
            let v = c.i64();
            c.is_i64(&format!("toLowerCoord({v})"), g.to_lower_coord(v));
        }
        4 => {
            let q = c.point();
            c.is_point(&format!("toGridPoint({q:?})"), g.to_grid_point(q));
        }
        2 | 3 => {
            let a = c.point();
            let (b, radius) = if op == 2 {
                (c.point(), 0)
            } else {
                (a, c.i64())
            };
            let stop = c.i64();
            let mut cells: Vec<Point> = Vec::new();
            let take = |cell: Point, cells: &mut Vec<Point>| {
                cells.push(cell);
                stop < 0 || i64::try_from(cells.len()).unwrap() < stop
            };
            let r = if op == 2 {
                g.process_line_cells((a, b), |cell| take(cell, &mut cells))
            } else {
                g.process_nearby(a, radius, |cell| take(cell, &mut cells))
            };
            let what = if op == 2 {
                format!("processLineCells({a:?}, {b:?}) cells")
            } else {
                format!("processNearby({a:?}, {radius}) cells")
            };
            c.is_points(&what, &cells);
            c.is_bool("its result", r);
        }
        _ => panic!("{}: an ANSQ op {op}", c.label),
    }
}

#[test]
fn square_grids_match() {
    replay("ANSQ", ansq);
}

// ---------------------------------------------------------------- ANSP

/// The probe's element: a point and an id.
type Elem = (Point, u64);

fn elem_point(e: &Elem) -> Point {
    e.0
}

fn ansp(c: &mut Case) {
    let cell = c.i64();
    let reserve = usize::try_from(c.u64()).unwrap();
    let load = c.f32();
    let n = c.count();
    let elems: Vec<Elem> = (0..n)
        .map(|_| {
            let p = c.point();
            (p, c.u64())
        })
        .collect();
    let mut grid = SparsePointGrid::new(cell, reserve, load, elem_point);
    for e in &elems {
        grid.insert(*e);
    }
    let n = c.count();
    let want: Vec<(Point, u64)> = (0..n)
        .map(|_| {
            let p = c.point();
            (p, c.u64())
        })
        .collect();
    let got: Vec<(Point, u64)> = grid.iter().map(|(gp, e)| (*gp, e.1)).collect();
    c.same("the grid's order", &got, &want);
    let queries = c.count();
    for _ in 0..queries {
        let q = c.point();
        let r = c.i64();
        let near: Vec<u64> = grid.get_nearby(q, r).iter().map(|e| e.1).collect();
        c.is_u64s(&format!("getNearby({q:?}, {r})"), &near);
        let mut seen: Vec<u64> = Vec::new();
        let res = grid.process_nearby(q, r, |e| {
            seen.push(e.1);
            seen.len() < 2
        });
        c.is_u64s(&format!("processNearby({q:?}, {r})"), &seen);
        c.is_bool("its result", res);
    }
}

#[test]
fn sparse_point_grids_match() {
    replay("ANSP", ansp);
}

// ---------------------------------------------------------------- ANSL

fn ansl(c: &mut Case) {
    let name = c.str();
    c.label = format!("{} ({name})", c.label);
    let polys = c.polygons();
    let square = c.i32();
    let grid = create_loc_to_line_grid(&polys, square);
    let n = c.count();
    let want: Vec<(Point, u32, u32)> = (0..n)
        .map(|_| {
            let p = c.point();
            let a = c.u32();
            (p, a, c.u32())
        })
        .collect();
    let got: Vec<(Point, u32, u32)> = grid
        .iter()
        .map(|(gp, e)| (*gp, e.poly_idx, e.point_idx))
        .collect();
    c.same("the grid's order", &got, &want);
    let queries = c.count();
    for _ in 0..queries {
        let q = c.point();
        let r = c.i64();
        let n = c.count();
        let want: Vec<(u32, u32)> = (0..n)
            .map(|_| {
                let a = c.u32();
                (a, c.u32())
            })
            .collect();
        let got: Vec<(u32, u32)> = grid
            .get_nearby(q, r)
            .iter()
            .map(|e| (e.poly_idx, e.point_idx))
            .collect();
        c.same(&format!("getNearby({q:?}, {r})"), &got, &want);
    }
}

#[test]
fn sparse_line_grids_match() {
    replay("ANSL", ansl);
}

// ---------------------------------------------------------------- ANPS

fn anps(c: &mut Case) {
    let lines = c.lines();
    let max_dist = c.i64();
    let snap = c.i64();
    let mut res_lines = Vec::new();
    let mut res_polys = Vec::new();
    stitch(&lines, &mut res_lines, &mut res_polys, max_dist, snap);
    c.is_lines("the result lines", &res_lines);
    c.is_lines("the result polygons", &res_polys);
}

#[test]
fn polyline_stitchers_match() {
    replay("ANPS", anps);
}

// ---------------------------------------------------------------- ANGR

/// A node of a graph as the probe writes it (`parity/sop1/arachne.md`, "a graph").
#[derive(Clone, Debug, PartialEq)]
struct NodeRec {
    p: Point,
    distance_to_boundary: i64,
    bead_count: i64,
    transition_ratio: F32,
    incident_edge: i32,
    beading: Option<(Beading, i64, i64, bool)>,
}

/// An edge of a graph as the probe writes it.
#[derive(Clone, Debug, PartialEq)]
struct EdgeRec {
    edge_type: u8,
    central: u8,
    from: i32,
    to: i32,
    twin: i32,
    next: i32,
    prev: i32,
    transitions: Option<Vec<(i64, i32, i64)>>,
    transition_ends: Option<Vec<(i64, i32, bool)>>,
    junctions: Option<Vec<ExtrusionJunction>>,
}

/// A graph as the probe writes it: the nodes and the edges in their lists' order.
#[derive(Clone, Debug, PartialEq)]
struct GraphRec {
    nodes: Vec<NodeRec>,
    edges: Vec<EdgeRec>,
}

/// The storage a built graph's weak pointers point to, kept alive while the graph is used (the
/// C++'s SkeletalTrapezoidation owns it).
#[derive(Default)]
struct Keep {
    beadings: Vec<Shared<BeadingPropagation>>,
    transitions: Vec<Shared<TransitionMiddles>>,
    transition_ends: Vec<Shared<TransitionEnds>>,
    junctions: Vec<Shared<LineJunctions>>,
}

/// A graph built from a record: the handles of its nodes and edges in the record's order.
struct Built {
    g: SkeletalTrapezoidationGraph,
    nodes: Vec<NodeId>,
    edges: Vec<EdgeId>,
    _keep: Keep,
}

/// What a pointer written as -2 (an object no longer in its list) becomes in a built graph.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Gone {
    /// `nullptr`, as the probe's `copy_graph` makes it on the copies ANGR's ops 0 to 2 change.
    Null,
    /// An object no longer in the list, as the original graph ops 3 and 4 read holds it.
    Erased,
}

fn edge_type_code(t: EdgeType) -> u8 {
    t as u8
}

fn edge_type_of(code: u8) -> EdgeType {
    match code {
        0 => EdgeType::Normal,
        1 => EdgeType::ExtraVd,
        2 => EdgeType::TransitionEnd,
        _ => panic!("an edge type {code}"),
    }
}

impl Case<'_, '_> {
    fn u32(&mut self) -> u32 {
        self.r.u32().unwrap()
    }
    fn polygons(&mut self) -> Polygons {
        let n = self.count();
        (0..n).map(|_| Polygon::new(self.points())).collect()
    }
    fn lines(&mut self) -> Vec<ExtrusionLine> {
        let n = self.count();
        (0..n).map(|_| self.line()).collect()
    }
    fn is_i32(&mut self, what: &str, got: i32) {
        let want = self.i32();
        self.same(what, &got, &want);
    }
    fn is_point(&mut self, what: &str, got: Point) {
        let want = self.point();
        self.same(what, &got, &want);
    }
    fn is_u64s(&mut self, what: &str, got: &[u64]) {
        let n = self.count();
        let want: Vec<u64> = (0..n).map(|_| self.u64()).collect();
        self.same(what, &got.to_vec(), &want);
    }
    fn is_lines(&mut self, what: &str, got: &[ExtrusionLine]) {
        let want = self.lines();
        self.same(&format!("{what}: the count"), &got.len(), &want.len());
        for (i, (g, w)) in got.iter().zip(&want).enumerate() {
            self.same(&format!("{what} {i}"), g, w);
        }
    }
    fn opt_i64(&mut self) -> Option<i64> {
        if self.bool() { Some(self.i64()) } else { None }
    }
    fn graph(&mut self) -> GraphRec {
        let n_nodes = self.count();
        let n_edges = self.count();
        let nodes = (0..n_nodes)
            .map(|_| {
                let p = self.point();
                let distance_to_boundary = self.i64();
                let bead_count = self.i64();
                let transition_ratio = F32(self.f32());
                let incident_edge = self.i32();
                let beading = if self.bool() {
                    let b = self.beading();
                    let bottom = self.i64();
                    let top = self.i64();
                    Some((b, bottom, top, self.bool()))
                } else {
                    None
                };
                NodeRec {
                    p,
                    distance_to_boundary,
                    bead_count,
                    transition_ratio,
                    incident_edge,
                    beading,
                }
            })
            .collect();
        let edges = (0..n_edges)
            .map(|_| {
                let edge_type = self.u8();
                let central = self.u8();
                let from = self.i32();
                let to = self.i32();
                let twin = self.i32();
                let next = self.i32();
                let prev = self.i32();
                let transitions = if self.bool() {
                    let n = self.count();
                    Some(
                        (0..n)
                            .map(|_| {
                                let pos = self.i64();
                                let lower = self.i32();
                                (pos, lower, self.i64())
                            })
                            .collect(),
                    )
                } else {
                    None
                };
                let transition_ends = if self.bool() {
                    let n = self.count();
                    Some(
                        (0..n)
                            .map(|_| {
                                let pos = self.i64();
                                let lower = self.i32();
                                (pos, lower, self.bool())
                            })
                            .collect(),
                    )
                } else {
                    None
                };
                let junctions = if self.bool() {
                    let n = self.count();
                    Some((0..n).map(|_| self.junction()).collect())
                } else {
                    None
                };
                EdgeRec {
                    edge_type,
                    central,
                    from,
                    to,
                    twin,
                    next,
                    prev,
                    transitions,
                    transition_ends,
                    junctions,
                }
            })
            .collect();
        GraphRec { nodes, edges }
    }
    /// Compares a graph with the one the record holds next, node by node and edge by edge.
    fn is_graph(&mut self, what: &str, got: &GraphRec) {
        let want = self.graph();
        self.same(
            &format!("{what}: the node count"),
            &got.nodes.len(),
            &want.nodes.len(),
        );
        self.same(
            &format!("{what}: the edge count"),
            &got.edges.len(),
            &want.edges.len(),
        );
        for (i, (g, w)) in got.nodes.iter().zip(&want.nodes).enumerate() {
            self.same(&format!("{what}: node {i}"), g, w);
        }
        for (i, (g, w)) in got.edges.iter().zip(&want.edges).enumerate() {
            self.same(&format!("{what}: edge {i}"), g, w);
        }
    }
}

/// Builds a graph from a record, its lists in the record's order, every pointer an index into
/// them (-1 `nullptr`, -2 as `gone` says), each live list or beading in a storage of its own.
fn build_graph(rec: &GraphRec, gone: Gone) -> Built {
    let mut g = SkeletalTrapezoidationGraph::default();
    let mut keep = Keep::default();
    // An object of each list that is no longer in it, for the pointers written as -2.
    let gone_node = g.nodes.push_back(STHalfEdgeNode::new(
        SkeletalTrapezoidationJoint::new(),
        Point::new(0, 0),
    ));
    g.nodes.erase(gone_node);
    let gone_edge = g
        .edges
        .push_back(STHalfEdge::new(SkeletalTrapezoidationEdge::default()));
    g.edges.erase(gone_edge);
    let nodes: Vec<NodeId> = rec
        .nodes
        .iter()
        .map(|n| {
            let mut data = SkeletalTrapezoidationJoint::new();
            data.distance_to_boundary = n.distance_to_boundary;
            data.bead_count = n.bead_count;
            data.transition_ratio = n.transition_ratio.0;
            if let Some((b, bottom, top, up)) = &n.beading {
                let mut prop = BeadingPropagation::new(b.clone());
                prop.dist_to_bottom_source = *bottom;
                prop.dist_from_top_source = *top;
                prop.is_upward_propagated_only = *up;
                let storage = Rc::new(RefCell::new(prop));
                data.set_beading(&storage);
                keep.beadings.push(storage);
            }
            g.nodes.push_back(STHalfEdgeNode::new(data, n.p))
        })
        .collect();
    let edges: Vec<EdgeId> = rec
        .edges
        .iter()
        .map(|e| {
            let mut data = SkeletalTrapezoidationEdge::new(edge_type_of(e.edge_type));
            match e.central {
                0 => data.set_is_central(false),
                1 => data.set_is_central(true),
                _ => {}
            }
            if let Some(list) = &e.transitions {
                let mut l = TransitionMiddles::new();
                for &(pos, lower, radius) in list {
                    l.push_back(TransitionMiddle::new(pos, lower, radius));
                }
                let storage = Rc::new(RefCell::new(l));
                data.set_transitions(&storage);
                keep.transitions.push(storage);
            }
            if let Some(list) = &e.transition_ends {
                let mut l = TransitionEnds::new();
                for &(pos, lower, is_lower) in list {
                    l.push_back(TransitionEnd::new(pos, lower, is_lower));
                }
                let storage = Rc::new(RefCell::new(l));
                data.set_transition_ends(&storage);
                keep.transition_ends.push(storage);
            }
            if let Some(js) = &e.junctions {
                let storage = Rc::new(RefCell::new(js.clone()));
                data.set_extrusion_junctions(&storage);
                keep.junctions.push(storage);
            }
            g.edges.push_back(STHalfEdge::new(data))
        })
        .collect();
    let node_at = |i: i32| match i {
        -1 => None,
        -2 => (gone == Gone::Erased).then_some(gone_node),
        i => Some(nodes[usize::try_from(i).unwrap()]),
    };
    let edge_at = |i: i32| match i {
        -1 => None,
        -2 => (gone == Gone::Erased).then_some(gone_edge),
        i => Some(edges[usize::try_from(i).unwrap()]),
    };
    for (n, k) in rec.nodes.iter().zip(&nodes) {
        g.nodes[*k].incident_edge = edge_at(n.incident_edge);
    }
    for (e, k) in rec.edges.iter().zip(&edges) {
        let x = &mut g.edges[*k];
        x.from = node_at(e.from);
        x.to = node_at(e.to);
        x.twin = edge_at(e.twin);
        x.next = edge_at(e.next);
        x.prev = edge_at(e.prev);
    }
    Built {
        g,
        nodes,
        edges,
        _keep: keep,
    }
}

/// A graph as the probe's `put_graph` writes it.
fn graph_rec(g: &SkeletalTrapezoidationGraph) -> GraphRec {
    let np = g.nodes.positions();
    let ep = g.edges.positions();
    let nodes = g
        .nodes
        .values()
        .map(|n| NodeRec {
            p: n.p,
            distance_to_boundary: n.data.distance_to_boundary,
            bead_count: n.data.bead_count,
            transition_ratio: F32(n.data.transition_ratio),
            incident_edge: ep.index(n.incident_edge),
            beading: n.data.get_beading().map(|b| {
                let b = b.borrow();
                (
                    b.beading.clone(),
                    b.dist_to_bottom_source,
                    b.dist_from_top_source,
                    b.is_upward_propagated_only,
                )
            }),
        })
        .collect();
    let edges = g
        .edges
        .values()
        .map(|e| {
            let central = if e.data.central_is_set() {
                u8::from(e.data.is_central())
            } else {
                255
            };
            EdgeRec {
                edge_type: edge_type_code(e.data.edge_type),
                central,
                from: np.index(e.from),
                to: np.index(e.to),
                twin: ep.index(e.twin),
                next: ep.index(e.next),
                prev: ep.index(e.prev),
                transitions: e.data.get_transitions().map(|l| {
                    l.borrow()
                        .values()
                        .map(|m| (m.pos, m.lower_bead_count, m.feature_radius))
                        .collect()
                }),
                transition_ends: e.data.get_transition_ends().map(|l| {
                    l.borrow()
                        .values()
                        .map(|m| (m.pos, m.lower_bead_count, m.is_lower_end))
                        .collect()
                }),
                junctions: e.data.get_extrusion_junctions().map(|j| j.borrow().clone()),
            }
        })
        .collect();
    GraphRec { nodes, edges }
}

/// The index of an edge in the graph's list, as the probe's `edge_index` gives it.
fn edge_index(g: &SkeletalTrapezoidationGraph, e: EdgeId) -> i32 {
    g.edges.positions().index(Some(e))
}

fn angr(c: &mut Case) {
    let name = c.str();
    let op = c.u8();
    c.label = format!("{} ({name}, op {op})", c.label);
    let input = c.graph();
    match op {
        0 => {
            let n = c.count();
            for _ in 0..n {
                let snap = c.i64();
                let mut b = build_graph(&input, Gone::Null);
                b.g.collapse_small_edges(snap);
                c.is_graph(&format!("collapseSmallEdges({snap})"), &graph_rec(&b.g));
            }
        }
        1 => {
            // The probe picks on the graph itself, then works on copies.
            let b = build_graph(&input, Gone::Erased);
            let picks: Vec<i32> = b
                .edges
                .iter()
                .enumerate()
                .filter(|(_, e)| {
                    let x = &b.g.edges[**e];
                    x.prev.is_some() && x.next.is_some() && x.twin.is_some()
                })
                .take(3)
                .map(|(i, _)| i32::try_from(i).unwrap())
                .collect();
            let n = c.count();
            c.same("makeRib's count", &picks.len(), &n);
            for &k in picks.iter().take(n) {
                c.is_i32("makeRib's edge", k);
                let mut b = build_graph(&input, Gone::Null);
                let e = b.edges[usize::try_from(k).unwrap()];
                let mut start = e;
                while let Some(p) = b.g.edges[start].prev {
                    start = p;
                }
                let mut end = e;
                while let Some(nx) = b.g.edges[end].next {
                    end = nx;
                }
                let a = b.g.nodes[b.g.edges[start].from.unwrap()].p;
                let z = b.g.nodes[b.g.edges[end].to.unwrap()].p;
                c.is_point("the start source", a);
                c.is_point("the end source", z);
                let mut prev = e;
                b.g.make_rib(&mut prev, a, z);
                c.is_i32("the edge makeRib leaves", edge_index(&b.g, prev));
                c.is_graph(&format!("makeRib on edge {k}"), &graph_rec(&b.g));
            }
        }
        2 => {
            // The probe picks on the graph itself, then works on copies.
            let b = build_graph(&input, Gone::Erased);
            let picks: Vec<i32> = b
                .edges
                .iter()
                .enumerate()
                .filter(|(_, e)| {
                    // In the probe's order, which stops at the first test that fails.
                    let x = &b.g.edges[**e];
                    if !(x.twin.is_some()
                        && x.data.central_is_set()
                        && x.data.is_central()
                        && x.data.edge_type != EdgeType::ExtraVd)
                    {
                        return false;
                    }
                    let (from, to) = (&b.g.nodes[x.from.unwrap()], &b.g.nodes[x.to.unwrap()]);
                    from.data.distance_to_boundary < to.data.distance_to_boundary
                        && (to.p - from.p).squared_norm() > 4 * 20000 * 20000
                })
                .take(3)
                .map(|(i, _)| i32::try_from(i).unwrap())
                .collect();
            let n = c.count();
            c.same("insertNode's count", &picks.len(), &n);
            for &k in picks.iter().take(n) {
                c.is_i32("insertNode's edge", k);
                let mut b = build_graph(&input, Gone::Null);
                let e = b.edges[usize::try_from(k).unwrap()];
                let (from, to) = (b.g.edges[e].from.unwrap(), b.g.edges[e].to.unwrap());
                let mid = (b.g.nodes[from].p + b.g.nodes[to].p) / 2;
                let count = orca_stl::min(
                    b.g.nodes[from].data.bead_count,
                    b.g.nodes[to].data.bead_count,
                );
                c.is_point("the middle", mid);
                c.is_i64("the bead count", count);
                let ret = b.g.insert_node(e, mid, count);
                c.is_i32("the edge insertNode returns", edge_index(&b.g, ret));
                c.is_graph(&format!("insertNode on edge {k}"), &graph_rec(&b.g));
            }
        }
        3 => {
            let b = build_graph(&input, Gone::Erased);
            let n = c.count();
            c.same("the node count", &b.nodes.len(), &n);
            for (i, node) in b.nodes.iter().enumerate().take(n) {
                c.is_bool(
                    &format!("isLocalMaximum of node {i}"),
                    b.g.is_local_maximum(*node, false),
                );
            }
        }
        4 => {
            let b = build_graph(&input, Gone::Erased);
            let picks: Vec<(i32, EdgeId)> = b
                .edges
                .iter()
                .enumerate()
                .filter(|(_, e)| {
                    let x = &b.g.edges[**e];
                    x.prev.is_some() && x.next.is_some()
                })
                .map(|(i, e)| (i32::try_from(i).unwrap(), *e))
                .collect();
            let n = c.count();
            c.same("the edge count", &picks.len(), &n);
            for &(k, e) in picks.iter().take(n) {
                c.is_i32("the edge", k);
                c.is_bool(&format!("isUpward of edge {k}"), b.g.is_upward(e));
                let want = c.opt_i64();
                c.same(
                    &format!("distToGoUp of edge {k}"),
                    &b.g.dist_to_go_up(e),
                    &want,
                );
                let twin = b.g.edges[e].twin.unwrap();
                let want = c.opt_i64();
                c.same(
                    &format!("distToGoUp of edge {k}'s twin"),
                    &b.g.dist_to_go_up(twin),
                    &want,
                );
            }
        }
        _ => panic!("{}: an ANGR op {op}", c.label),
    }
}

#[test]
fn graph_operations_match() {
    replay("ANGR", angr);
}

// ---------------------------------------------------------------- ANSC and ANST

/// The last ANST stage, 10 (`generateSegments`, whose record holds the toolpaths, not a graph).
const LAST_STAGE: u8 = 10;

impl Case<'_, '_> {
    /// Compares a graph with one read before, node by node and edge by edge.
    fn graph_is(&mut self, what: &str, got: &GraphRec, want: &GraphRec) {
        self.same(
            &format!("{what}: the node count"),
            &got.nodes.len(),
            &want.nodes.len(),
        );
        self.same(
            &format!("{what}: the edge count"),
            &got.edges.len(),
            &want.edges.len(),
        );
        for (i, (g, w)) in got.nodes.iter().zip(&want.nodes).enumerate() {
            self.same(&format!("{what}: node {i}"), g, w);
        }
        for (i, (g, w)) in got.edges.iter().zip(&want.edges).enumerate() {
            self.same(&format!("{what}: edge {i}"), g, w);
        }
    }
}

/// Replays an ANSC record and the ANST records after it: the trapezoidation is built from the
/// ANSC's inputs (its log compared with the strategy's and the constructor's) and its graph
/// compared with stage 0's; each stage from 1 to 10 runs its one step on the graph built from the
/// record of the stage before, as the page says, and is compared with its own record. The two
/// lists of `generateTransitioningRibs` are the step's own for stages 5, 7 and 8, and the built
/// graph's storage for stages 6 to 8; stage 9 runs after the probe's block that holds them ends,
/// so the built graph's transition lists are dropped before it. Stage 10 compares the toolpaths
/// `generateSegments` makes, its log, and whether a fresh trapezoidation's `generateToolpaths`
/// gives the same toolpaths (the probe's check).
fn ansc(sc: &Record, stages: &[Record], n: usize, m: &mut Mismatches) {
    let mut c = Case {
        r: sc.reader(),
        m: &mut *m,
        label: format!("ANSC {n}"),
    };
    let before = c.m.count;
    let name = c.str();
    let setup = c.str();
    c.label = format!("{} ({name}, {setup})", c.label);
    // The rest of the wall setup, which only shaped the inputs that follow.
    let _nozzle = c.f64();
    let _layer_height = c.f64();
    let _bead_width_0 = c.i64();
    let _bead_width_x = c.i64();
    let _inset_count = c.u64();
    let _outer_wall_inset = c.i64();
    let _layer_id = c.i32();
    let _top_or_bottom = c.bool();
    let polygons = c.polygons();
    let make = strategy(&mut c);
    let transitioning_angle = c.f64();
    let discretization_step_size = c.i64();
    let transition_filter_dist = c.i64();
    let allowed_filter_deviation = c.i64();
    let beading_propagation_transition_dist = c.i64();
    log_start();
    let strat = make();
    let made = SkeletalTrapezoidation::new(
        &polygons,
        strat.as_ref(),
        transitioning_angle,
        discretization_step_size,
        transition_filter_dist,
        allowed_filter_deviation,
        beading_propagation_transition_dist,
    );
    let log = log_take();
    log_stop();
    let label = c.label.clone();
    let mut st = made.unwrap_or_else(|e| panic!("{label}: the constructor threw: {e}"));
    c.is_log("the log", &log);
    if c.m.count == before {
        c.r.finish(&c.label).unwrap();
    }

    let mut prev: Option<GraphRec> = None;
    for (k, rec) in stages.iter().enumerate() {
        let mut c = Case {
            r: rec.reader(),
            m: &mut *m,
            label: format!("{label}: ANST {k}"),
        };
        let stage = c.u8();
        c.same("the stage", &usize::from(stage), &k);
        assert!(stage <= LAST_STAGE, "{label}: an ANST stage {stage}");
        let before = c.m.count;
        c.label = format!("{label}: stage {stage}");
        if stage == LAST_STAGE {
            let input = prev.as_ref().expect("stage 9 comes before");
            let Built { g, _keep: keep, .. } = build_graph(input, Gone::Erased);
            st.graph = g;
            st.p_generated_toolpaths = Vec::new();
            log_start();
            st.generate_segments();
            let log = log_take();
            log_stop();
            let staged = core::mem::take(&mut st.p_generated_toolpaths);
            drop(keep);
            c.is_toolpaths("the toolpaths", &staged);
            c.is_log("the log", &log);
            // The probe's check: a fresh trapezoidation's generateToolpaths, its log dropped.
            let mut direct = Vec::new();
            {
                let mut d = SkeletalTrapezoidation::new(
                    &polygons,
                    strat.as_ref(),
                    transitioning_angle,
                    discretization_step_size,
                    transition_filter_dist,
                    allowed_filter_deviation,
                    beading_propagation_transition_dist,
                )
                .unwrap_or_else(|e| panic!("{label}: the direct constructor threw: {e}"));
                d.generate_toolpaths(&mut direct, false);
            }
            c.is_bool(
                "the staged run is generateToolpaths'",
                direct == staged,
            );
            if c.m.count == before {
                c.r.finish(&c.label).unwrap();
            }
            continue;
        }
        let want = c.graph();
        if stage == 0 {
            // The constructor's log is the ANSC's; stage 0's is empty.
            c.graph_is("the graph", &graph_rec(&st.graph), &want);
            c.is_log("the log", &LogText::default());
        } else {
            let input = prev.as_ref().expect("stage 0 comes first");
            let Built { g, _keep: mut keep, .. } = build_graph(input, Gone::Erased);
            st.graph = g;
            let mut mids = Vec::new();
            let mut ends = Vec::new();
            if stage == 9 {
                // generateTransitioningRibs' lists are gone when generateExtraRibs runs.
                keep.transitions.clear();
                keep.transition_ends.clear();
            }
            log_start();
            match stage {
                1 => st.update_is_central(),
                2 => st.filter_central(st.central_filter_dist()),
                3 => st.update_bead_count(),
                4 => st.filter_noncentral_regions(),
                5 => st.generate_transition_mids(&mut mids),
                6 => st.filter_transition_mids(),
                7 => st.generate_all_transition_ends(&mut ends),
                8 => st.apply_transitions(&mut ends),
                9 => st.generate_extra_ribs(),
                _ => unreachable!("the stages 1 to 9"),
            }
            let log = log_take();
            log_stop();
            c.graph_is("the graph", &graph_rec(&st.graph), &want);
            c.is_log("the log", &log);
            // The lists live until the graph has been written, as the probe's do.
            drop((mids, ends, keep));
        }
        if c.m.count == before {
            c.r.finish(&c.label).unwrap();
        }
        prev = Some(want);
    }
}

/// Every ANSC record of the file with the ANST records that follow it (the probe writes them
/// together, stages 0 to 10).
#[test]
fn skeletal_trapezoidations_match() {
    let file = file();
    let mut m = Mismatches::default();
    let records = &file.records;
    let mut cases = 0;
    let mut i = 0;
    while i < records.len() {
        if records[i].kind() != "ANSC" {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < records.len() && records[j].kind() == "ANST" {
            j += 1;
        }
        ansc(&records[i], &records[i + 1..j], cases, &mut m);
        cases += 1;
        i = j;
    }
    assert!(cases > 0, "{}: no ANSC record", file.path.display());
    m.assert_none("ANSC and ANST");
}

// ---------------------------------------------------------------- ANWS, ANIP, ANWH, ANWT, ANMP

/// A wall setup as the probe writes it (walls.cpp, `put(WallSetup)`).
#[derive(Clone, Debug)]
struct Setup {
    name: String,
    layer_height: f64,
    bead_width_0: i64,
    bead_width_x: i64,
    inset_count: usize,
    wall_0_inset: i64,
}

/// The parameters' fields, each `float` by its bits.
type ParamsRec = (F32, F32, F32, F32, F32, F32, i32, bool, i64, i64);

fn params_rec(w: &WallToolPathsParams) -> ParamsRec {
    (
        F32(w.min_bead_width),
        F32(w.min_feature_size),
        F32(w.min_length_factor),
        F32(w.wall_transition_length),
        F32(w.wall_transition_angle),
        F32(w.wall_transition_filter_deviation),
        w.wall_distribution_count,
        w.is_top_or_bottom_layer,
        w.wall_maximum_resolution,
        w.wall_maximum_deviation,
    )
}

/// One step of the outline preparation as an ANWH record names it: op, a, b, f, holes.
type StepRec = (u8, i64, i64, F64, bool);

impl Case<'_, '_> {
    /// A setup: `str` name, `f64` nozzle, `f64` layer height, `i64` widths, `u64` inset count,
    /// `i64` outer wall inset, `i32` layer, `bool` top or bottom.
    fn setup(&mut self) -> Setup {
        let name = self.str();
        let _nozzle = self.f64();
        let layer_height = self.f64();
        let bead_width_0 = self.i64();
        let bead_width_x = self.i64();
        let inset_count = usize::try_from(self.u64()).unwrap();
        let wall_0_inset = self.i64();
        let _layer_id = self.i32();
        let _top_or_bottom = self.bool();
        Setup {
            name,
            layer_height,
            bead_width_0,
            bead_width_x,
            inset_count,
            wall_0_inset,
        }
    }
    /// `WallToolPathsParams`: six `f32`, `i32` distribution count, `bool` top or bottom, `i64`
    /// resolution and deviation.
    fn params(&mut self) -> WallToolPathsParams {
        let min_bead_width = self.f32();
        let min_feature_size = self.f32();
        let min_length_factor = self.f32();
        let wall_transition_length = self.f32();
        let wall_transition_angle = self.f32();
        let wall_transition_filter_deviation = self.f32();
        let wall_distribution_count = self.i32();
        let is_top_or_bottom_layer = self.bool();
        let wall_maximum_resolution = self.i64();
        let wall_maximum_deviation = self.i64();
        WallToolPathsParams {
            min_bead_width,
            min_feature_size,
            min_length_factor,
            wall_transition_length,
            wall_transition_angle,
            wall_transition_filter_deviation,
            wall_distribution_count,
            is_top_or_bottom_layer,
            wall_maximum_resolution,
            wall_maximum_deviation,
        }
    }
    fn toolpaths(&mut self) -> Vec<Vec<ExtrusionLine>> {
        let n = self.count();
        (0..n).map(|_| self.lines()).collect()
    }
    /// Compares toolpaths with the ones the record holds next, inset by inset and line by line.
    fn is_toolpaths(&mut self, what: &str, got: &[Vec<ExtrusionLine>]) {
        let want = self.toolpaths();
        self.same(&format!("{what}: the inset count"), &got.len(), &want.len());
        for (i, (g, w)) in got.iter().zip(&want).enumerate() {
            self.same(&format!("{what}: inset {i}: the line count"), &g.len(), &w.len());
            for (k, (gl, wl)) in g.iter().zip(w).enumerate() {
                self.same(&format!("{what}: inset {i} line {k}"), gl, wl);
            }
        }
    }
    fn is_polygons(&mut self, what: &str, got: &Polygons) {
        let want = self.polygons();
        self.same(&format!("{what}: the count"), &got.len(), &want.len());
        for (i, (g, w)) in got.iter().zip(&want).enumerate() {
            self.same(&format!("{what} {i}"), g, w);
        }
    }
    fn is_params(&mut self, what: &str, got: &WallToolPathsParams) {
        let want = self.params();
        self.same(what, &params_rec(got), &params_rec(&want));
    }
    fn step(&mut self) -> StepRec {
        let op = self.u8();
        let a = self.i64();
        let b = self.i64();
        let f = F64(self.f64());
        (op, a, b, f, self.bool())
    }
}

/// The outline of each ANWT record by its shape and setup name, for ANWS's check.
fn anwt_outlines(file: &ProbeFile) -> BTreeMap<(String, String), Polygons> {
    let mut m = Mismatches::default();
    let mut out = BTreeMap::new();
    for rec in file.of_kind("ANWT") {
        let mut c = Case {
            r: rec.reader(),
            m: &mut m,
            label: String::from("ANWT"),
        };
        let name = c.str();
        let setup = c.setup();
        let _params = c.params();
        out.entry((name, setup.name)).or_insert_with(|| c.polygons());
    }
    out
}

fn anws(c: &mut Case, outlines: &BTreeMap<(String, String), Polygons>) -> bool {
    let name = c.str();
    let setup = c.setup();
    c.label = format!("{} ({name}, {})", c.label, setup.name);
    let params = c.params();
    let mut t = c.toolpaths();
    log_start();
    WallToolPaths::stitch_tool_paths(&mut t, setup.bead_width_x);
    let after_stitch = t.clone();
    // removeSmallLines reads only the parameters, not the outline.
    let no_outline = Polygons::new();
    let walls = WallToolPaths::new(
        &no_outline,
        setup.bead_width_0,
        setup.bead_width_x,
        setup.inset_count,
        setup.wall_0_inset,
        setup.layer_height,
        &params,
    );
    walls.remove_small_lines(&mut t);
    let after_small = t.clone();
    WallToolPaths::simplify_tool_paths(&mut t, &params);
    let after_simplify = t.clone();
    let empty = WallToolPaths::remove_empty_tool_paths(&mut t);
    let log = log_take();
    log_stop();
    c.is_toolpaths("after stitchToolPaths", &after_stitch);
    c.is_toolpaths("after removeSmallLines", &after_small);
    c.is_toolpaths("after simplifyToolPaths", &after_simplify);
    c.is_toolpaths("after removeEmptyToolPaths", &t);
    c.is_bool("removeEmptyToolPaths", empty);
    c.is_log("the log", &log);
    // The probe's check: getToolPaths on the outline is these toolpaths without the zero-width
    // insets. The ANWS record does not hold the outline; the ANWT record of the same shape and
    // setup does, when the file has it.
    let Some(outline) = outlines.get(&(name, setup.name.clone())) else {
        let _match = c.bool();
        return false;
    };
    let mut whole = WallToolPaths::new(
        outline,
        setup.bead_width_0,
        setup.bead_width_x,
        setup.inset_count,
        setup.wall_0_inset,
        setup.layer_height,
        &params,
    );
    let label = c.label.clone();
    let engine = whole
        .get_tool_paths()
        .unwrap_or_else(|e| panic!("{label}: getToolPaths threw: {e}"))
        .clone();
    let mine: Vec<Vec<ExtrusionLine>> = t
        .iter()
        .filter(|inset| {
            let zero = !inset.is_empty()
                && inset
                    .iter()
                    .all(|l| l.junctions.iter().all(|j| j.w == 0));
            !zero
        })
        .cloned()
        .collect();
    c.is_bool("getToolPaths is the steps' result", engine == mine);
    true
}

/// Every ANWS record; the probe's check of each against `getToolPaths` runs where the file holds
/// the ANWT record of its shape and setup (every one in the full set).
#[test]
fn wall_steps_match() {
    let file = file();
    let outlines = anwt_outlines(&file);
    let mut m = Mismatches::default();
    let mut checked = 0;
    for (i, rec) in file.of_kind("ANWS").enumerate() {
        let before = m.count;
        let mut c = Case {
            r: rec.reader(),
            m: &mut m,
            label: format!("ANWS {i}"),
        };
        if anws(&mut c, &outlines) {
            checked += 1;
        }
        if c.m.count == before {
            c.r.finish(&c.label).unwrap();
        }
    }
    assert!(checked > 0, "{}: no ANWS record has its ANWT outline", file.path.display());
    m.assert_none("ANWS");
}

fn anip(c: &mut Case) {
    let left = c.beading();
    let ratio = c.f64();
    let right = c.beading();
    let radius = c.opt_i64();
    let got = match radius {
        Some(r) => SkeletalTrapezoidation::interpolate_at(&left, ratio, &right, r),
        None => SkeletalTrapezoidation::interpolate(&left, ratio, &right),
    };
    c.is_beading(&format!("interpolate({ratio}, {radius:?})"), &got);
}

#[test]
fn interpolations_match() {
    replay("ANIP", anip);
}

/// Runs one step of the outline preparation, as the probe's `run_step` does.
fn run_step(step: &StepRec, polys: &mut Polygons) {
    let (op, a, b, f, holes) = *step;
    match op {
        0 => simplify(polys, a, b),
        1 => fix_self_intersections(a, polys),
        2 => remove_degenerate_verts(polys),
        3 => remove_colinear_edges(polys, f.0),
        _ => remove_small_areas(polys, f.0, holes),
    }
}

fn anwh(c: &mut Case) {
    let form = c.u8();
    match form {
        0 => {
            let name = c.str();
            c.label = format!("{} ({name})", c.label);
            let outline = c.polygons();
            let smallest = c.i64();
            let allowed = c.i64();
            let bead_width_0 = c.i64();
            // WallToolPaths::generate's own expressions (WallToolPaths.cpp 487-509).
            let epsilon_offset = (allowed / 2).wrapping_sub(1);
            let small_area_length = bead_width_0 as f64 / 2.;
            let off = |p: &Polygons, d: i64| offset(p, d as f32, DEFAULT_JOIN_TYPE, DEFAULT_MITER_LIMIT);
            let mut cur = off(
                &off(&off(&outline, epsilon_offset.wrapping_neg()), epsilon_offset.wrapping_mul(2)),
                epsilon_offset.wrapping_neg(),
            );
            c.is_polygons("the triple offset", &cur);
            let chain: [StepRec; 7] = [
                (0, smallest, allowed, F64(0.), false),
                (1, epsilon_offset, 0, F64(0.), false),
                (2, 0, 0, F64(0.), false),
                (3, 0, 0, F64(0.005), false),
                (1, epsilon_offset, 0, F64(0.), false),
                (2, 0, 0, F64(0.), false),
                (4, 0, 0, F64(small_area_length * small_area_length), false),
            ];
            let n = c.count();
            c.same("the step count", &n, &chain.len());
            for (k, want) in chain.iter().enumerate().take(n) {
                let step = c.step();
                c.same(&format!("step {k}"), &step, want);
                run_step(want, &mut cur);
                c.is_polygons(&format!("step {k}'s output"), &cur);
            }
            let united = union_(&cur);
            c.is_polygons("the union", &united);
        }
        1 => {
            let step = c.step();
            c.label = format!("{} (op {})", c.label, step.0);
            let mut polys = c.polygons();
            run_step(&step, &mut polys);
            c.is_polygons("the output", &polys);
        }
        _ => panic!("{}: an ANWH form {form}", c.label),
    }
}

#[test]
fn outline_preparations_match() {
    replay("ANWH", anwh);
}

fn anwt(c: &mut Case) {
    let name = c.str();
    let setup = c.setup();
    c.label = format!("{} ({name}, {})", c.label, setup.name);
    let params = c.params();
    let outline = c.polygons();
    let label = c.label.clone();
    log_start();
    let mut wt = WallToolPaths::new(
        &outline,
        setup.bead_width_0,
        setup.bead_width_x,
        setup.inset_count,
        setup.wall_0_inset,
        setup.layer_height,
        &params,
    );
    let tp = wt
        .get_tool_paths()
        .unwrap_or_else(|e| panic!("{label}: getToolPaths threw: {e}"))
        .clone();
    let inner = wt
        .get_inner_contour()
        .unwrap_or_else(|e| panic!("{label}: getInnerContour threw: {e}"))
        .clone();
    let log = log_take();
    c.is_log("the log", &log);
    c.is_toolpaths("getToolPaths", &tp);
    c.is_polygons("getInnerContour", &inner);
    // getRegionOrder on the lines in PerimeterGenerator's order: inner to outer unless the outer
    // wall goes first, the empty insets skipped.
    for outer_first in [false, true] {
        let insets: Vec<&Vec<ExtrusionLine>> = if outer_first {
            tp.iter().collect()
        } else {
            tp.iter().rev().collect()
        };
        let all: Vec<&ExtrusionLine> = insets
            .into_iter()
            .filter(|inset| !inset.is_empty())
            .flatten()
            .collect();
        let order = WallToolPaths::get_region_order(&all, outer_first);
        c.is_bool("outer_first", outer_first);
        let n_lines = c.u64();
        c.same("the line count", &(all.len() as u64), &n_lines);
        let n_pairs = c.count();
        c.same(
            &format!("getRegionOrder({outer_first}): the pair count"),
            &order.len(),
            &n_pairs,
        );
        let want: Vec<(i32, i32)> = (0..n_pairs)
            .map(|_| {
                let a = c.i32();
                (a, c.i32())
            })
            .collect();
        let got: Vec<(i32, i32)> = order
            .iter()
            .map(|&(a, b)| (i32::try_from(a).unwrap(), i32::try_from(b).unwrap()))
            .collect();
        c.same(&format!("getRegionOrder({outer_first})"), &got, &want);
    }
    let log = log_take();
    log_stop();
    c.is_log("getRegionOrder's log", &log);
}

#[test]
fn wall_tool_paths_match() {
    replay("ANWT", anwt);
}

fn anmp(c: &mut Case) {
    let layer_id = c.i32();
    let mut poc = PrintObjectConfig::default();
    let mut pc = PrintConfig::default();
    poc.min_bead_width.value = c.f64();
    poc.initial_layer_min_bead_width.value = c.f64();
    poc.min_feature_size.value = c.f64();
    poc.min_length_factor.value = c.f64();
    poc.wall_transition_filter_deviation.value = c.f64();
    poc.wall_transition_length.value = c.f64();
    poc.wall_transition_angle.value = c.f64();
    poc.wall_distribution_count.value = c.i32();
    poc.wall_maximum_resolution.value = c.f64();
    poc.wall_maximum_deviation.value = c.f64();
    let n = c.count();
    pc.nozzle_diameter.values = (0..n).map(|_| c.f64()).collect();
    c.label = format!("{} (layer {layer_id}, nozzles {:?})", c.label, pc.nozzle_diameter.values);
    let got = make_paths_params(layer_id, &poc, &pc);
    c.is_params("make_paths_params", &got);
}

#[test]
fn paths_params_match() {
    replay("ANMP", anmp);
}

// ---------------------------------------------------------------- every kind

/// Every kind of `arachne.sop1` belongs to a piece, and every kind of every piece is recorded.
#[test]
fn every_kind_has_a_piece() {
    let file = file();
    let unknown: Vec<&str> = file
        .records
        .iter()
        .map(|r| r.kind())
        .filter(|k| {
            !ARACHNE1_KINDS.contains(k)
                && !ARACHNE2_KINDS.contains(k)
                && !ARACHNE3_4_KINDS.contains(k)
        })
        .collect();
    assert!(
        unknown.is_empty(),
        "{}: {} records of no piece ({:?}...)",
        file.path.display(),
        unknown.len(),
        &unknown[..unknown.len().min(4)]
    );
    for kind in ARACHNE1_KINDS
        .into_iter()
        .chain(ARACHNE2_KINDS)
        .chain(ARACHNE3_4_KINDS)
    {
        assert!(
            file.of_kind(kind).next().is_some(),
            "{}: no {kind} record",
            file.path.display()
        );
    }
}
