// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The probe gate for orca-geom: `geom.sop1`'s records but the Eigen ones, which orca-eigen replays
//! (`parity/SOP1.md`, "geom.sop1"), replayed against the crate bit for bit (added 2026-11-07):
//!
//! - CCLP: `Clipper` runs, every clip and fill type, `PolyTree` results, open paths, the three
//!   `initOptions`, `GetBounds` and the exceptions `AddPath` and `Execute` throw;
//! - COFF: `ClipperOffset` runs, every join and end type, miter limits, arc tolerances and the
//!   shortest edge length;
//! - CARE, CCLN, CSIM and CMNK: `Area`, `Orientation` and `PointInPolygon`; `CleanPolygon`;
//!   `SimplifyPolygon(s)`; `MinkowskiSum` and `MinkowskiDiff`;
//! - CINT: `Int128`'s filtered predicates, and through them the exact ones.
//!
//! The committed subset (`parity/probes/`) runs by default; `SKETCH_ORCA_PROBES_DEEP=<folder>`
//! replays the full set from that folder (`<work>/parity/probes`).

use std::fmt::Debug;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::Once;

use orca_geom::clipper::{
    self, ClipType, Clipper, ClipperException, ClipperOffset, EndType, IntPoint, IntRect, JoinType,
    NodeId, Path, Paths, PolyFillType, PolyTree, PolyType, ROOT,
};
use orca_geom::int128;
use orca_probe::{Mismatches, ProbeFile, Reader, load_probe};

fn file() -> ProbeFile {
    load_probe("geom").unwrap_or_else(|e| panic!("{e}"))
}

/// The kinds of `geom.sop1` that orca-eigen's `tests/probes.rs` replays.
const EIGEN_KINDS: [&str; 5] = ["EVEC", "EMAT", "ETRF", "EROT", "ESVD"];
/// The kinds this file replays.
const GEOM_KINDS: [&str; 7] = ["CCLP", "COFF", "CARE", "CCLN", "CSIM", "CMNK", "CINT"];

/// Runs `f`, turning a thrown [`ClipperException`] into its `what()`; other panics go on.
fn catch<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    static QUIET: Once = Once::new();
    QUIET.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
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

/// A `PolyNode` below the root as the probe writes it.
#[derive(Debug, PartialEq, Eq)]
struct Node {
    open: bool,
    hole: bool,
    contour: Path,
    children: Vec<Node>,
}

/// A `PolyTree` as the probe writes it: the root's contour, `Total()` and the children.
#[derive(Debug, PartialEq, Eq)]
struct Tree {
    contour: Path,
    total: i32,
    children: Vec<Node>,
}

fn nodes_of(t: &PolyTree, id: NodeId) -> Vec<Node> {
    t.node(id)
        .childs
        .iter()
        .map(|&c| Node {
            open: t.is_open(c),
            hole: t.is_hole(c),
            contour: t.node(c).contour.clone(),
            children: nodes_of(t, c),
        })
        .collect()
}

fn tree_of(t: &PolyTree) -> Tree {
    Tree {
        contour: t.root().contour.clone(),
        total: t.total(),
        children: nodes_of(t, ROOT),
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
    fn str(&mut self) -> String {
        self.r.str().unwrap()
    }
    fn point(&mut self) -> IntPoint {
        let x = self.i64();
        IntPoint::new(x, self.i64())
    }
    fn path(&mut self) -> Path {
        let n = self.u32();
        (0..n).map(|_| self.point()).collect()
    }
    fn paths(&mut self) -> Paths {
        let n = self.u32();
        (0..n).map(|_| self.path()).collect()
    }
    fn nodes(&mut self) -> Vec<Node> {
        let n = self.u32();
        (0..n)
            .map(|_| {
                let open = self.bool();
                let hole = self.bool();
                let contour = self.path();
                Node {
                    open,
                    hole,
                    contour,
                    children: self.nodes(),
                }
            })
            .collect()
    }
    fn tree(&mut self) -> Tree {
        let contour = self.path();
        let total = self.i32();
        Tree {
            contour,
            total,
            children: self.nodes(),
        }
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

    /// Reads the outcome of a call that may throw (0 or 1, or 2 and `what()`) and compares
    /// `got` with it; true when the record goes on (the call returned on both sides).
    fn outcome(&mut self, what: &str, got: &Result<bool, String>) -> bool {
        let flag = self.u8();
        let want = if flag == 2 {
            Err(self.str())
        } else {
            Ok(flag == 1)
        };
        self.same(what, got, &want);
        want.is_ok() && got.is_ok()
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

fn clip_type(v: u8) -> ClipType {
    [
        ClipType::Intersection,
        ClipType::Union,
        ClipType::Difference,
        ClipType::Xor,
    ][usize::from(v)]
}

fn fill_type(v: u8) -> PolyFillType {
    [
        PolyFillType::EvenOdd,
        PolyFillType::NonZero,
        PolyFillType::Positive,
        PolyFillType::Negative,
    ][usize::from(v)]
}

fn join_type(v: u8) -> JoinType {
    [JoinType::Square, JoinType::Round, JoinType::Miter][usize::from(v)]
}

fn end_type(v: u8) -> EndType {
    [
        EndType::ClosedPolygon,
        EndType::ClosedLine,
        EndType::OpenButt,
        EndType::OpenSquare,
        EndType::OpenRound,
    ][usize::from(v)]
}

fn poly_type(v: u8) -> PolyType {
    if v == 0 {
        PolyType::Subject
    } else {
        PolyType::Clip
    }
}

fn slices(paths: &[Path]) -> impl Iterator<Item = &[IntPoint]> {
    paths.iter().map(Vec::as_slice)
}

/// A whole CCLP record, which stops early where the C++ engine threw.
fn cclp(c: &mut Case) {
    let opts = i32::from(c.u8());
    let (ct, sf, cf) = (clip_type(c.u8()), fill_type(c.u8()), fill_type(c.u8()));
    let (tree, each, twice, convert) = (c.bool(), c.bool(), c.bool(), c.bool());
    let groups: Vec<(PolyType, bool, Paths)> = (0..c.u32())
        .map(|_| {
            let pt = poly_type(c.u8());
            let closed = c.bool();
            (pt, closed, c.paths())
        })
        .collect();
    let any_open = groups.iter().any(|g| !g.1);
    let mut clipper = Clipper::new(opts);
    for (pt, closed, paths) in &groups {
        if each {
            for path in paths {
                let got = catch(|| clipper.add_path(path, *pt, *closed));
                if !c.outcome("AddPath", &got) {
                    return;
                }
            }
        } else {
            let got = catch(|| clipper.add_paths(slices(paths), *pt, *closed));
            if !c.outcome("AddPaths", &got) {
                return;
            }
        }
    }
    if !any_open {
        let want = IntRect {
            left: c.i64(),
            top: c.i64(),
            right: c.i64(),
            bottom: c.i64(),
        };
        c.same("GetBounds", &clipper.get_bounds(), &want);
    }
    for _ in 0..if twice { 2 } else { 1 } {
        if tree {
            let mut t = PolyTree::new();
            let ok = clipper.execute_tree(ct, &mut t, sf, cf);
            let want = c.bool();
            c.same("Execute", &ok, &want);
            let want = c.tree();
            c.same("PolyTree", &tree_of(&t), &want);
            if convert {
                let want = c.paths();
                c.same("PolyTreeToPaths", &clipper::poly_tree_to_paths(&t), &want);
                let want = c.paths();
                c.same(
                    "ClosedPathsFromPolyTree",
                    &clipper::closed_paths_from_poly_tree(&t),
                    &want,
                );
                let want = c.paths();
                c.same(
                    "OpenPathsFromPolyTree",
                    &clipper::open_paths_from_poly_tree(&t),
                    &want,
                );
            }
        } else {
            let mut out = Paths::new();
            let got = catch(|| clipper.execute(ct, &mut out, sf, cf));
            if !c.outcome("Execute", &got) {
                return;
            }
            let want = c.paths();
            c.same("solution", &out, &want);
        }
    }
}

#[test]
fn clipper_runs_match_clipper() {
    replay("CCLP", cclp);
}

/// A whole COFF record.
fn coff(c: &mut Case) {
    let (miter, arc, shortest) = (c.f64(), c.f64(), c.f64());
    let mut co = ClipperOffset::new(miter, arc, shortest);
    for _ in 0..c.u32() {
        let (join, end) = (join_type(c.u8()), end_type(c.u8()));
        let path = c.path();
        co.add_path(&path, join, end);
    }
    let runs: Vec<(f64, bool)> = (0..c.u32())
        .map(|_| {
            let delta = c.f64();
            (delta, c.bool())
        })
        .collect();
    for (delta, tree) in runs {
        if tree {
            let mut t = PolyTree::new();
            let got = catch(|| co.execute_tree(&mut t, delta)).map(|()| true);
            if !c.outcome("Execute(PolyTree)", &got) {
                return;
            }
            let want = c.tree();
            c.same("PolyTree", &tree_of(&t), &want);
        } else {
            let mut out = Paths::new();
            let got = catch(|| co.execute(&mut out, delta)).map(|()| true);
            if !c.outcome("Execute(Paths)", &got) {
                return;
            }
            let want = c.paths();
            c.same("solution", &out, &want);
        }
    }
}

#[test]
fn offsets_match_clipper() {
    replay("COFF", coff);
}

#[test]
fn area_orientation_and_point_in_polygon_match_clipper() {
    replay("CARE", |c| {
        let path = c.path();
        let want = c.f64();
        c.same("Area", &clipper::area(&path).to_bits(), &want.to_bits());
        let want = c.bool();
        c.same("Orientation", &clipper::orientation(&path), &want);
        for _ in 0..c.u32() {
            let pt = c.point();
            let want = c.i32();
            c.same(
                &format!("PointInPolygon({pt:?})"),
                &clipper::point_in_polygon(pt, &path),
                &want,
            );
        }
    });
}

#[test]
fn clean_polygon_matches_clipper() {
    replay("CCLN", |c| {
        let path = c.path();
        let distance = c.f64();
        let want = c.path();
        c.same(
            "CleanPolygon",
            &clipper::clean_polygon(&path, distance),
            &want,
        );
    });
}

#[test]
fn simplify_polygons_match_clipper() {
    replay("CSIM", |c| {
        let many = c.bool();
        let paths = c.paths();
        let (fill, strictly) = (fill_type(c.u8()), c.bool());
        let got = if many {
            clipper::simplify_polygons(slices(&paths), fill, strictly)
        } else {
            clipper::simplify_polygon(&paths[0], fill, strictly)
        };
        let want = c.paths();
        c.same("SimplifyPolygon(s)", &got, &want);
    });
}

#[test]
fn minkowski_matches_clipper() {
    replay("CMNK", |c| {
        let (kind, closed) = (c.u8(), c.bool());
        let pattern = c.path();
        let paths = c.paths();
        let got = match kind {
            0 => clipper::minkowski_sum(&pattern, &paths[0], closed),
            1 => clipper::minkowski_sum_paths(&pattern, &paths, closed),
            _ => clipper::minkowski_diff(&pattern, &paths[0]),
        };
        let want = c.paths();
        c.same("Minkowski", &got, &want);
    });
}

#[test]
fn int128_predicates_match() {
    replay("CINT", |c| {
        let (a, b, d, e) = (c.i64(), c.i64(), c.i64(), c.i64());
        let got = [
            int128::sign_determinant_2x2_filtered(a, b, d, e),
            int128::compare_rationals_filtered(a, b, d, e),
        ];
        let want = [c.i32(), c.i32()];
        c.same(&format!("Int128 ({a}, {b}, {d}, {e})"), &got, &want);
    });
}

/// Every kind of the file has a replay here or in orca-eigen, and every kind here is recorded.
#[test]
fn every_kind_is_replayed() {
    assert_eq!(orca_geom::NAME, "orca-geom");
    let file = file();
    let unknown: Vec<&str> = file
        .records
        .iter()
        .map(|r| r.kind())
        .filter(|k| !EIGEN_KINDS.contains(k) && !GEOM_KINDS.contains(k))
        .collect();
    assert!(
        unknown.is_empty(),
        "{}: {} records nothing replays ({:?}...)",
        file.path.display(),
        unknown.len(),
        &unknown[..unknown.len().min(4)]
    );
    for kind in GEOM_KINDS {
        assert!(
            file.of_kind(kind).next().is_some(),
            "{}: no {kind} record",
            file.path.display()
        );
    }
}
