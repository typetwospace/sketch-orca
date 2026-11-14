// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The probe gate of orca-paths: the records of `paths.sop1` whose sections in
//! `parity/sop1/paths.md` name paths-1, replayed against the crate (added 2026-11-13 by paths-1):
//!
//! - PSLN: `chain_lines`, against the first call's epsilon the record names;
//! - PSCP: `chain_points` (the first version's greedy chaining, no constraint);
//! - PSCX: `chain_expolygons`;
//! - PSXP: `chain_extrusion_paths` and `chain_and_reorder_extrusion_paths`, here
//!   `chain_segments_greedy` over the paths' end points and the reorder written out as layer-3's
//!   code does it (the entity functions are layer-3's, in orca-layer);
//! - PSXE: `chain_extrusion_entities` and `chain_and_reorder_extrusion_entities`, here
//!   `chain_segments_greedy_constrained_reversals` over a model of the record's entity types (their
//!   end points, `is_loop`, `can_reverse`, `reverse` and whether they have end points, as
//!   ExtrusionEntity.hpp and ExtrusionEntityCollection.hpp define them);
//! - PSPL: `chain_polylines`;
//! - PSUO: `union_pt_chained_outside_in`.
//!
//! Every point, index, flag and count is compared exactly. The kinds of geom-1 and geom-2 in the
//! same file are orca-geom's.
//!
//! The committed subset (`parity/probes/`) runs by default; `SKETCH_ORCA_PROBES_DEEP=<folder>`
//! replays the full set from that folder (`<work>/parity/probes`).

use std::fmt::Debug;
use std::sync::OnceLock;

use orca_geom::expolygon::ExPolygon;
use orca_geom::line::Line;
use orca_geom::point::Point;
use orca_geom::polygon::Polygon;
use orca_geom::polyline::Polyline;
use orca_paths::lines::{chain_lines, chain_lines_against};
use orca_paths::outside_in::union_pt_chained_outside_in;
use orca_paths::points::{chain_expolygons, chain_points};
use orca_paths::polylines::chain_polylines;
use orca_paths::segments::{
    Chain, chain_segments_greedy, chain_segments_greedy_constrained_reversals,
};
use orca_probe::{Mismatches, ProbeFile, Reader, load_probe};

/// The file, read once for every test of this binary.
fn file() -> &'static ProbeFile {
    static FILE: OnceLock<ProbeFile> = OnceLock::new();
    FILE.get_or_init(|| load_probe("paths").unwrap_or_else(|e| panic!("{e}")))
}

/// The kinds this file replays: those of the piece paths-1.
const PATHS1_KINDS: [&str; 7] = ["PSLN", "PSCP", "PSCX", "PSXP", "PSXE", "PSPL", "PSUO"];
/// The kinds of the pieces geom-1 and geom-2, which orca-geom replays.
const GEOM_KINDS: [&str; 16] = [
    "PSZC", "PSRZ", "PSIC", "PSCV", "PSRC", "PSCI", "PSAS", "PSAC", "PSAF", "PSPF", "PSTP", "PSP3",
    "PSD3", "PSL3", "PSC3", "PSO3",
];

/// A `Point3`: x, y and z.
type P3 = (i64, i64, i64);

/// `Point3::to_point()`.
fn to_point(p: P3) -> Point {
    Point::new(p.0, p.1)
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
    fn i64(&mut self) -> i64 {
        self.r.i64().unwrap()
    }
    fn f64(&mut self) -> f64 {
        self.r.f64().unwrap()
    }
    fn bool(&mut self) -> bool {
        self.r.bool().unwrap()
    }
    /// A `u32` count.
    fn count(&mut self) -> usize {
        self.u32() as usize
    }
    fn point(&mut self) -> Point {
        let x = self.i64();
        Point::new(x, self.i64())
    }
    fn point3(&mut self) -> P3 {
        let (x, y) = (self.i64(), self.i64());
        (x, y, self.i64())
    }
    fn points(&mut self) -> Vec<Point> {
        let n = self.count();
        (0..n).map(|_| self.point()).collect()
    }
    fn points3(&mut self) -> Vec<P3> {
        let n = self.count();
        (0..n).map(|_| self.point3()).collect()
    }
    fn polygons(&mut self) -> Vec<Polygon> {
        let n = self.count();
        (0..n).map(|_| Polygon::new(self.points())).collect()
    }
    fn polylines(&mut self) -> Vec<Polyline> {
        let n = self.count();
        (0..n).map(|_| Polyline::new(self.points())).collect()
    }
    fn expolygons(&mut self) -> Vec<ExPolygon> {
        let n = self.count();
        (0..n)
            .map(|_| {
                let contour = Polygon::new(self.points());
                let holes = self.polygons();
                ExPolygon { contour, holes }
            })
            .collect()
    }
    /// `u8` a start point given and the start point.
    fn start(&mut self) -> Option<Point> {
        let has = self.bool();
        let s = self.point();
        has.then_some(s)
    }
    /// An order: a `u64` count and `u64` indices.
    fn order(&mut self) -> Vec<usize> {
        let n = self.u64();
        (0..n).map(|_| self.u64() as usize).collect()
    }
    /// A chain: a `u32` count, each `u64` index and `u8` reversed.
    fn chain(&mut self) -> Chain {
        let n = self.count();
        (0..n)
            .map(|_| {
                let idx = self.u64() as usize;
                (idx, self.bool())
            })
            .collect()
    }

    fn same<T: PartialEq + Debug>(&mut self, what: &str, got: &T, want: &T) {
        let label = &self.label;
        self.m.check(got == want, || {
            format!("{label}: {what}: {got:?} instead of {want:?}")
        });
    }
    /// Two lists: their counts, then the first item that differs.
    fn list<T: PartialEq + Debug>(&mut self, what: &str, got: &[T], want: &[T]) {
        if got.len() != want.len() {
            let (g, w) = (got.len(), want.len());
            self.same(&format!("{what} (count)"), &g, &w);
        } else if let Some(k) = (0..got.len()).find(|&k| got[k] != want[k]) {
            self.same(&format!("{what} [{k}]"), &got[k], &want[k]);
        } else {
            self.m.check(true, String::new);
        }
    }
    fn points_lists(&mut self, what: &str, got: &[Vec<Point>], want: &[Vec<Point>]) {
        self.list(what, got, want);
    }
}

fn replay(kind: &str, mut f: impl FnMut(&mut Case<'_, '_>)) {
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

fn points_of_polylines(pls: &[Polyline]) -> Vec<Vec<Point>> {
    pls.iter().map(|p| p.points.clone()).collect()
}

fn points_of_polygons(pls: &[Polygon]) -> Vec<Vec<Point>> {
    pls.iter().map(|p| p.points.clone()).collect()
}

#[test]
fn chain_lines_matches() {
    // chain_lines' function-local static: set by this process's first call, which is the first
    // PSLN record's, with the first epsilon (10000) every record names.
    let mut first_call = true;
    replay("PSLN", |c| {
        let n = c.count();
        let lines: Vec<Line> = (0..n)
            .map(|_| {
                let a = c.point();
                Line { a, b: c.point() }
            })
            .collect();
        let eps = c.f64();
        let first_eps = c.f64();
        let got = chain_lines_against(&lines, eps, first_eps * first_eps);
        let want = c.polylines();
        c.points_lists(
            "chain_lines",
            &points_of_polylines(&got),
            &points_of_polylines(&want),
        );
        if first_call {
            first_call = false;
            // The program's first call is the first record's, with the first epsilon; a set that
            // starts elsewhere seeds the static with a call of its own. Only this test calls
            // chain_lines in this binary.
            if eps.to_bits() != first_eps.to_bits() {
                let _ = chain_lines(&[], first_eps);
            }
        }
        // The library's own static holds the first call's square from here on.
        let with_static = chain_lines(&lines, eps);
        c.points_lists(
            "chain_lines with its static",
            &points_of_polylines(&with_static),
            &points_of_polylines(&want),
        );
    });
}

#[test]
fn chain_points_matches() {
    replay("PSCP", |c| {
        let pts = c.points();
        let start = c.start();
        let got = chain_points(&pts, start.as_ref());
        let want = c.order();
        c.list("chain_points", &got, &want);
    });
}

#[test]
fn chain_expolygons_matches() {
    replay("PSCX", |c| {
        let es = c.expolygons();
        let got = chain_expolygons(&es);
        let want = c.order();
        c.list("chain_expolygons", &got, &want);
    });
}

#[test]
fn chain_extrusion_paths_match() {
    replay("PSXP", |c| {
        let start = c.start();
        let n = c.count();
        let polys: Vec<Vec<P3>> = (0..n).map(|_| c.points3()).collect();
        // chain_extrusion_paths: ExtrusionPath::first_point() and last_point() are the polyline's
        // first and last Point3 as points.
        let chain = chain_segments_greedy(
            |idx, first_point| {
                let p = &polys[idx];
                to_point(if first_point { p[0] } else { p[p.len() - 1] })
            },
            polys.len(),
            start.as_ref(),
        );
        let want = c.chain();
        c.list("chain_extrusion_paths", &chain, &want);
        // chain_and_reorder_extrusion_paths: reorder_extrusion_paths of a new chain.
        let chain = chain_segments_greedy(
            |idx, first_point| {
                let p = &polys[idx];
                to_point(if first_point { p[0] } else { p[p.len() - 1] })
            },
            polys.len(),
            start.as_ref(),
        );
        let mut paths = polys.clone();
        if !paths.is_empty() {
            let mut out = Vec::with_capacity(paths.len());
            for &(idx, reversed) in &chain {
                out.push(core::mem::take(&mut paths[idx]));
                if reversed {
                    out.last_mut().unwrap().reverse();
                }
            }
            paths = out;
        }
        let n = c.count();
        let want: Vec<Vec<P3>> = (0..n).map(|_| c.points3()).collect();
        c.list("chain_and_reorder_extrusion_paths", &paths, &want);
    });
}

/// An extrusion entity of PSXE, as the probe builds it from its type
/// (`parity/oracle/probe/paths/chain.cpp`, `build`).
#[derive(Clone, Debug)]
enum Entity {
    /// An ExtrusionPath (types 0, 1 after `set_reverse()`, 2 an ExtrusionPathOriented, 10 with no
    /// points).
    Path { pts: Vec<P3>, can_reverse: bool },
    /// An ExtrusionLoop (types 3, 4, 13 with no paths).
    Loop { paths: Vec<Vec<P3>> },
    /// An ExtrusionMultiPath (types 5, 6 of a path that cannot reverse, 12 with an empty path
    /// last).
    Multi {
        paths: Vec<Vec<P3>>,
        can_reverse: bool,
    },
    /// An ExtrusionEntityCollection of paths (types 7, 8 with `no_sort`, 9 after
    /// `set_reverse()`, 11 empty, 14 with an empty path last).
    Collection {
        entities: Vec<Entity>,
        no_sort: bool,
        is_reverse: bool,
    },
}

impl Entity {
    fn build(ty: u8, paths: &[Vec<P3>]) -> Self {
        let path = |pts: &Vec<P3>| Entity::Path {
            pts: pts.clone(),
            can_reverse: true,
        };
        match ty {
            0 => path(&paths[0]),
            1 | 2 => Entity::Path {
                pts: paths[0].clone(),
                can_reverse: false,
            },
            3 => Entity::Loop {
                paths: vec![paths[0].clone()],
            },
            4 => Entity::Loop {
                paths: paths.to_vec(),
            },
            5 => Entity::Multi {
                paths: paths.to_vec(),
                can_reverse: true,
            },
            6 => Entity::Multi {
                paths: vec![paths[0].clone()],
                can_reverse: false,
            },
            7 | 8 | 9 | 11 | 14 => {
                let mut entities: Vec<Entity> = paths.iter().map(path).collect();
                if ty == 14 {
                    entities.push(path(&Vec::new()));
                }
                Entity::Collection {
                    entities,
                    no_sort: ty == 8,
                    is_reverse: ty != 9,
                }
            }
            10 => path(&Vec::new()),
            12 => {
                let mut ps = paths.to_vec();
                ps.push(Vec::new());
                Entity::Multi {
                    paths: ps,
                    can_reverse: true,
                }
            }
            _ => Entity::Loop { paths: Vec::new() },
        }
    }

    fn first_point(&self) -> Point {
        match self {
            Entity::Path { pts, .. } => to_point(pts[0]),
            Entity::Loop { paths } | Entity::Multi { paths, .. } => to_point(paths[0][0]),
            Entity::Collection { entities, .. } => entities[0].first_point(),
        }
    }

    fn last_point(&self) -> Point {
        match self {
            Entity::Path { pts, .. } => to_point(pts[pts.len() - 1]),
            Entity::Loop { .. } => self.first_point(),
            Entity::Multi { paths, .. } => {
                let p = &paths[paths.len() - 1];
                to_point(p[p.len() - 1])
            }
            Entity::Collection { entities, .. } => entities[entities.len() - 1].last_point(),
        }
    }

    fn is_loop(&self) -> bool {
        matches!(self, Entity::Loop { .. })
    }

    fn can_reverse(&self) -> bool {
        match self {
            Entity::Path { can_reverse, .. } | Entity::Multi { can_reverse, .. } => *can_reverse,
            Entity::Loop { .. } => false,
            Entity::Collection {
                no_sort,
                is_reverse,
                ..
            } => !*no_sort && *is_reverse,
        }
    }

    fn reverse(&mut self) {
        match self {
            Entity::Path { pts, .. } => pts.reverse(),
            Entity::Loop { paths } | Entity::Multi { paths, .. } => {
                for p in paths.iter_mut() {
                    p.reverse();
                }
                paths.reverse();
            }
            Entity::Collection { entities, .. } => {
                for e in entities.iter_mut() {
                    if !e.is_loop() {
                        e.reverse();
                    }
                }
                entities.reverse();
            }
        }
    }

    /// `extrusion_entity_has_endpoints`.
    fn has_endpoints(&self) -> bool {
        let paths_have_endpoints = |paths: &Vec<Vec<P3>>| {
            !paths.is_empty() && !paths[0].is_empty() && !paths[paths.len() - 1].is_empty()
        };
        match self {
            Entity::Collection { entities, .. } => {
                !entities.is_empty()
                    && entities[0].has_endpoints()
                    && entities[entities.len() - 1].has_endpoints()
            }
            Entity::Path { pts, .. } => !pts.is_empty(),
            Entity::Multi { paths, .. } | Entity::Loop { paths } => paths_have_endpoints(paths),
        }
    }
}

/// `chain_extrusion_entities(entities, start_near)`, as layer-3 writes it: the constrained
/// greedy chaining, a loop's reversal dropped.
fn chain_entities(entities: &[Entity], start_near: Option<&Point>) -> Chain {
    let mut out = chain_segments_greedy_constrained_reversals(
        |idx, first_point| {
            if first_point {
                entities[idx].first_point()
            } else {
                entities[idx].last_point()
            }
        },
        |idx| entities[idx].is_loop() || entities[idx].can_reverse(),
        entities.len(),
        start_near,
    );
    for segment in &mut out {
        if entities[segment.0].is_loop() {
            // Ignore reversals for loops, as the start point equals the end point.
            segment.1 = false;
        }
    }
    out
}

#[test]
fn chain_extrusion_entities_match() {
    replay("PSXE", |c| {
        let start = c.start();
        let n = c.count();
        let ents: Vec<(u8, Vec<Vec<P3>>)> = (0..n)
            .map(|_| {
                let ty = c.u8();
                let k = c.count();
                (ty, (0..k).map(|_| c.points3()).collect())
            })
            .collect();
        let built: Vec<Entity> = ents.iter().map(|(t, p)| Entity::build(*t, p)).collect();
        let all = built.iter().all(Entity::has_endpoints);
        let want_all = c.bool();
        c.same("every entity has end points", &all, &want_all);
        if want_all {
            let got = chain_entities(&built, start.as_ref());
            let want = c.chain();
            c.list("chain_extrusion_entities", &got, &want);
        }
        // chain_and_reorder_extrusion_entities: those without end points dropped, then the
        // chain, then reorder_extrusion_entities.
        let mut v: Vec<(u32, Entity)> = built
            .iter()
            .enumerate()
            .map(|(i, e)| (i as u32, e.clone()))
            .filter(|(_, e)| e.has_endpoints())
            .collect();
        let entities: Vec<Entity> = v.iter().map(|(_, e)| e.clone()).collect();
        let chain = chain_entities(&entities, start.as_ref());
        let mut out = Vec::with_capacity(v.len());
        for &(idx, reversed) in &chain {
            let mut e = v[idx].clone();
            if reversed {
                e.1.reverse();
            }
            out.push(e);
        }
        v = out;
        let got: Vec<(u32, Point, Point)> = v
            .iter()
            .map(|(i, e)| (*i, e.first_point(), e.last_point()))
            .collect();
        let n = c.count();
        let want: Vec<(u32, Point, Point)> = (0..n)
            .map(|_| {
                let i = c.u32();
                let f = c.point();
                (i, f, c.point())
            })
            .collect();
        c.list("chain_and_reorder_extrusion_entities", &got, &want);
    });
}

#[test]
fn chain_polylines_matches() {
    replay("PSPL", |c| {
        let start = c.start();
        let pls = c.polylines();
        let got = chain_polylines(pls, start.as_ref());
        let want = c.polylines();
        c.points_lists(
            "chain_polylines",
            &points_of_polylines(&got),
            &points_of_polylines(&want),
        );
    });
}

#[test]
fn union_pt_chained_outside_in_matches() {
    replay("PSUO", |c| {
        let polys = c.polygons();
        let got = union_pt_chained_outside_in(&polys);
        let want = c.polygons();
        c.points_lists(
            "union_pt_chained_outside_in",
            &points_of_polygons(&got),
            &points_of_polygons(&want),
        );
    });
}

/// Every kind of the file has a replay here or in orca-geom's, and every kind here is recorded.
#[test]
fn every_kind_is_known() {
    let file = file();
    let unknown: Vec<&str> = file
        .records
        .iter()
        .map(|r| r.kind())
        .filter(|k| !PATHS1_KINDS.contains(k) && !GEOM_KINDS.contains(k))
        .collect();
    assert!(
        unknown.is_empty(),
        "{}: {} records nothing replays ({:?}...)",
        file.path.display(),
        unknown.len(),
        &unknown[..unknown.len().min(4)]
    );
    for kind in PATHS1_KINDS {
        assert!(
            file.of_kind(kind).next().is_some(),
            "{}: no {kind} record",
            file.path.display()
        );
    }
}
