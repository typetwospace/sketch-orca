// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The probe gate for orca-surface: the records of `surfaces.sop1` (`parity/sop1/surfaces.md`)
//! that the piece surfaces-1 owns, rebuilt from the crate and compared whole (added 2026-11-13 by
//! orca-o3-surfaces-1):
//!
//! - SFMP: a script of `MutablePolygon` steps (iterators, insert and remove, the range of
//!   unprocessed points, assign, clear, `polygon()`, `==`), the state after each step;
//! - SFRD: the three `remove_duplicates` of a `MutablePolygon` and the ExPolygons one;
//! - SFSO: `smooth_outward` of a `Polygon`, of `Polygons` and of `ExPolygons`;
//! - SFRP: `RegionExpansionParameters::build`;
//! - SFRX: one source and boundary through every public function of `RegionExpansion.hpp`;
//! - SFME: `merge_expansions_into_expolygons` of expansions made by the probe.
//!
//! The piece surfaces-2 adds (2026-11-14, orca-o3-surfaces-2):
//!
//! - SFLP: `check_layer_id_pattern` of each pattern at the layer indices -2 to 60 and the ends of
//!   `int`, the pattern read as bytes (it may hold a UTF-8 no-break space, `\v` and `\f`).
//!
//! Points are compared exactly and every `float` and `double` by its bits. surfaces-3 replays SFBR
//! and SFBF; until it does, this file only checks that no other kind is in the file. The committed
//! subset (`parity/probes/`) runs by default;
//! `SKETCH_ORCA_PROBES_DEEP=<folder>` replays the full set from that folder
//! (`<work>/parity/probes`).

use std::fmt::{self, Debug, Formatter};
use std::sync::OnceLock;

use orca_geom::expolygon::{ExPolygon, ExPolygons};
use orca_geom::point::{Point, Points};
use orca_geom::polygon::{Polygon, Polygons};
use orca_probe::{Mismatches, ProbeFile, Reader, load_probe};
use orca_surface::mutable_polygon::{
    Iter, MutablePolygon, Range, remove_duplicates, remove_duplicates_angle, remove_duplicates_eps,
    remove_duplicates_expolygons, smooth_outward_expolygons, smooth_outward_polygon,
    smooth_outward_polygons,
};
use orca_surface::region_expansion::{
    RegionExpansion, RegionExpansionEx, RegionExpansionParameters, WaveSeed, expand_expolygons,
    expand_merge_expolygons, merge_expansions_into_expolygons, propagate_waves, propagate_waves_ex,
    propagate_waves_ex_steps, propagate_waves_src, propagate_waves_steps, wave_seeds,
};
use orca_surface::utils::check_layer_id_pattern;

/// The probe file, read once.
fn file() -> &'static ProbeFile {
    static FILE: OnceLock<ProbeFile> = OnceLock::new();
    FILE.get_or_init(|| load_probe("surfaces").unwrap_or_else(|e| panic!("{e}")))
}

/// The kinds this file replays for surfaces-1.
const SURFACES_1_KINDS: [&str; 6] = ["SFMP", "SFRD", "SFSO", "SFRP", "SFRX", "SFME"];
/// The kind this file replays for surfaces-2.
const SURFACES_2_KINDS: [&str; 1] = ["SFLP"];
/// The kinds surfaces-3 replays.
const OTHER_PIECES_KINDS: [&str; 2] = ["SFBR", "SFBF"];

/// A `float` compared by its bits.
#[derive(Clone, Copy)]
struct F32(f32);

impl PartialEq for F32 {
    fn eq(&self, o: &Self) -> bool {
        self.0.to_bits() == o.0.to_bits()
    }
}

impl Debug for F32 {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

/// A `double` compared by its bits.
#[derive(Clone, Copy)]
struct F64(f64);

impl PartialEq for F64 {
    fn eq(&self, o: &Self) -> bool {
        self.0.to_bits() == o.0.to_bits()
    }
}

impl Debug for F64 {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

/// The fields of a `RegionExpansionParameters`, each compared by its bits.
type ParamsOf = (F32, F32, F32, u64, F32, F64, F64);

fn params_of(p: &RegionExpansionParameters) -> ParamsOf {
    (
        F32(p.tiny_expansion),
        F32(p.initial_step),
        F32(p.other_step),
        p.num_other_steps as u64,
        F32(p.max_inflation),
        F64(p.arc_tolerance),
        F64(p.shortest_edge_length),
    )
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
    fn i64(&mut self) -> i64 {
        self.r.i64().unwrap()
    }
    fn u64(&mut self) -> u64 {
        self.r.u64().unwrap()
    }
    fn f32(&mut self) -> f32 {
        self.r.f32().unwrap()
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
    /// A `u64` count.
    fn index(&mut self) -> usize {
        usize::try_from(self.u64()).unwrap()
    }
    fn point(&mut self) -> Point {
        let x = self.i64();
        Point::new(x, self.i64())
    }
    fn points(&mut self) -> Points {
        let n = self.index();
        (0..n).map(|_| self.point()).collect()
    }
    fn polygon(&mut self) -> Polygon {
        Polygon::new(self.points())
    }
    fn polygons(&mut self) -> Polygons {
        let n = self.index();
        (0..n).map(|_| self.polygon()).collect()
    }
    fn expolygon(&mut self) -> ExPolygon {
        let contour = self.polygon();
        ExPolygon {
            contour,
            holes: self.polygons(),
        }
    }
    fn expolygons(&mut self) -> ExPolygons {
        let n = self.index();
        (0..n).map(|_| self.expolygon()).collect()
    }
    fn params(&mut self) -> ParamsOf {
        let tiny = F32(self.f32());
        let initial = F32(self.f32());
        let other = F32(self.f32());
        let steps = self.u64();
        let inflation = F32(self.f32());
        let arc = F64(self.f64());
        (tiny, initial, other, steps, inflation, arc, F64(self.f64()))
    }
    fn seeds(&mut self) -> Vec<WaveSeed> {
        let n = self.index();
        (0..n)
            .map(|_| {
                let src = self.u32();
                let boundary = self.u32();
                WaveSeed {
                    src,
                    boundary,
                    path: self.points(),
                }
            })
            .collect()
    }
    fn expansions(&mut self) -> Vec<RegionExpansion> {
        let n = self.index();
        (0..n)
            .map(|_| {
                let polygon = self.polygon();
                let src_id = self.u32();
                RegionExpansion {
                    polygon,
                    src_id,
                    boundary_id: self.u32(),
                }
            })
            .collect()
    }
    fn expansions_ex(&mut self) -> Vec<RegionExpansionEx> {
        let n = self.index();
        (0..n)
            .map(|_| {
                let expolygon = self.expolygon();
                let src_id = self.u32();
                RegionExpansionEx {
                    expolygon,
                    src_id,
                    boundary_id: self.u32(),
                }
            })
            .collect()
    }
    fn u32s(&mut self) -> Vec<u32> {
        let n = self.index();
        (0..n).map(|_| self.u32()).collect()
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
            let label = &self.label;
            self.m.check(false, || {
                format!(
                    "{label}: {what}: {} items instead of {}: {got:?} instead of {want:?}",
                    got.len(),
                    want.len()
                )
            });
        } else if let Some(k) = (0..got.len()).find(|&k| got[k] != want[k]) {
            self.same(&format!("{what} [{k}]"), &got[k], &want[k]);
        } else {
            self.m.check(true, String::new);
        }
    }
    fn is_bool(&mut self, what: &str, got: bool) {
        let want = self.bool();
        self.same(what, &got, &want);
    }
    fn is_point(&mut self, what: &str, got: Point) {
        let want = self.point();
        self.same(what, &got, &want);
    }
    fn is_points(&mut self, what: &str, got: &[Point]) {
        let want = self.points();
        self.list(what, got, &want);
    }
    fn is_polygon(&mut self, what: &str, got: &Polygon) {
        self.is_points(what, &got.points);
    }
    fn is_polygons(&mut self, what: &str, got: &[Polygon]) {
        let want = self.polygons();
        self.list(what, got, &want);
    }
    fn is_expolygons(&mut self, what: &str, got: &[ExPolygon]) {
        let want = self.expolygons();
        self.list(what, got, &want);
    }
    fn is_params(&mut self, what: &str, got: &RegionExpansionParameters) {
        let want = self.params();
        self.same(what, &params_of(got), &want);
    }
    fn is_seeds(&mut self, what: &str, got: &[WaveSeed]) {
        let want = self.seeds();
        self.list(what, got, &want);
    }
    fn is_expansions(&mut self, what: &str, got: &[RegionExpansion]) {
        let want = self.expansions();
        self.list(what, got, &want);
    }
    fn is_expansions_ex(&mut self, what: &str, got: &[RegionExpansionEx]) {
        let want = self.expansions_ex();
        self.list(what, got, &want);
    }
}

fn replay(kind: &str, mut f: impl FnMut(&mut Case<'_, '_>)) {
    let file = file();
    let mut m = Mismatches::default();
    let mut n = 0usize;
    for (i, rec) in file.of_kind(kind).enumerate() {
        n += 1;
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
    eprintln!("{}: {n} {kind} records replayed", file.path.display());
    assert!(n > 0, "{}: no {kind} record", file.path.display());
    m.assert_none(kind);
}

/// SFMP's state after a step: size, empty, valid, the cursor (its point, whether it is `begin()`
/// and `end()`), and the range (empty, its ends' points, whether each is the cursor).
fn is_state(
    c: &mut Case<'_, '_>,
    step: &str,
    mp: &MutablePolygon,
    it: Iter,
    range: Option<&Range>,
) {
    let size = c.u64();
    c.same(&format!("{step}: size"), &(mp.size() as u64), &size);
    c.is_bool(&format!("{step}: empty"), mp.is_empty());
    c.is_bool(&format!("{step}: valid"), mp.valid());
    c.is_bool(&format!("{step}: the cursor is valid"), it.valid());
    if it.valid() {
        c.is_point(&format!("{step}: *it"), mp.point(it));
        c.is_bool(&format!("{step}: it == begin()"), it == mp.begin());
        c.is_bool(&format!("{step}: it == end()"), it == mp.end());
    }
    c.is_bool(&format!("{step}: a range is held"), range.is_some());
    if let Some(range) = range {
        c.is_bool(&format!("{step}: the range is empty"), range.is_empty());
        if !range.is_empty() {
            c.is_point(&format!("{step}: *range.begin()"), mp.point(range.begin()));
            c.is_point(&format!("{step}: *range.end()"), mp.point(range.end()));
            let v = it.valid();
            c.is_bool(
                &format!("{step}: range.begin() == it"),
                v && range.begin() == it,
            );
            c.is_bool(
                &format!("{step}: range.end() == it"),
                v && range.end() == it,
            );
        }
    }
}

/// The range a step of the script needs (the probe runs it only with one held).
fn held<'r>(range: &'r mut Option<Range>, label: &str) -> &'r mut Range {
    range
        .as_mut()
        .unwrap_or_else(|| panic!("{label}: a range step with no range held"))
}

#[test]
fn mutable_polygon_scripts_match() {
    replay("SFMP", |c| {
        let how = c.u8();
        let mut mp = MutablePolygon::new();
        let before = if how == 1 { Some(c.points()) } else { None };
        let pts = c.points();
        let reserve = c.index();
        match how {
            0 => mp.assign(&pts, reserve),
            1 => {
                mp.assign_polygon(&Polygon::new(before.unwrap_or_default()), 0);
                mp.assign_polygon(&Polygon::new(pts.clone()), reserve);
            }
            _ => mp = MutablePolygon::from_polygon(&Polygon::new(pts.clone()), reserve),
        }
        let mut it = mp.begin();
        let mut range: Option<Range> = None;
        is_state(c, "made", &mp, it, range.as_ref());
        let steps = c.u32();
        for k in 0..steps {
            let op = c.u8();
            let label = format!("{} step {k} (op {op})", c.label);
            match op {
                0 => it = mp.next(it),
                1 => it = mp.prev(it),
                2 => {
                    let q = c.point();
                    it = mp.insert(it, q);
                }
                3 => it = mp.remove(it),
                4 => it = mp.begin(),
                5 => it = mp.end(),
                6 => {
                    let got = mp.polygon();
                    c.is_polygon(&format!("step {k}: polygon()"), &got);
                }
                7 => {
                    mp.clear();
                    it = mp.begin();
                    range = None;
                }
                8 => {
                    let q = c.points();
                    let rv = c.index();
                    mp.assign(&q, rv);
                    it = mp.begin();
                    range = None;
                }
                9 => {
                    let q = c.points();
                    let other = MutablePolygon::from_polygon(&Polygon::new(q), 0);
                    c.is_bool(&format!("step {k}: =="), mp == other);
                    c.is_bool(&format!("step {k}: !="), mp != other);
                }
                10 => range = Some(Range::new(&mp)),
                11 => it = held(&mut range, &label).process_next(&mp),
                12 => held(&mut range, &label).advance_front(&mp),
                13 => held(&mut range, &label).retract_back(&mp),
                14 => it = held(&mut range, &label).remove_front(&mut mp, it),
                15 => it = held(&mut range, &label).remove_back(&mut mp, it),
                16 => {
                    let q = c.point();
                    *mp.point_mut(it) = q;
                }
                _ => panic!("{label}: an unknown operation"),
            }
            is_state(c, &format!("step {k}"), &mp, it, range.as_ref());
        }
    });
}

#[test]
fn remove_duplicates_match() {
    replay("SFRD", |c| {
        let variant = c.u8();
        if variant <= 2 {
            let input = c.polygon();
            let mut mp = MutablePolygon::from_polygon(&input, 0);
            match variant {
                0 => remove_duplicates(&mut mp),
                1 => {
                    let eps = c.f64();
                    remove_duplicates_eps(&mut mp, eps);
                }
                _ => {
                    let scaled_eps = c.i64();
                    let max_angle = c.f64();
                    remove_duplicates_angle(&mut mp, scaled_eps, max_angle);
                }
            }
            c.is_polygon("remove_duplicates", &mp.polygon());
        } else {
            let input = c.expolygons();
            let scaled_eps = c.i64();
            let max_angle = c.f64();
            let got = remove_duplicates_expolygons(input, scaled_eps, max_angle);
            c.is_expolygons("remove_duplicates(ExPolygons)", &got);
        }
    });
}

#[test]
fn smooth_outward_matches() {
    replay("SFSO", |c| {
        let variant = c.u8();
        let clip = c.i64();
        match variant {
            0 => {
                let input = c.polygon();
                let got = smooth_outward_polygon(input, clip);
                c.is_polygon("smooth_outward(Polygon)", &got);
            }
            1 => {
                let input = c.polygons();
                let got = smooth_outward_polygons(input, clip);
                c.is_polygons("smooth_outward(Polygons)", &got);
            }
            _ => {
                let input = c.expolygons();
                let got = smooth_outward_expolygons(input, clip);
                c.is_expolygons("smooth_outward(ExPolygons)", &got);
            }
        }
    });
}

#[test]
fn expansion_parameters_match() {
    replay("SFRP", |c| {
        let full = c.f32();
        let step = c.f32();
        let max_steps = c.index();
        let got = RegionExpansionParameters::build(full, step, max_steps);
        c.is_params("build", &got);
    });
}

#[test]
fn region_expansions_match() {
    replay("SFRX", |c| {
        let label = c.str();
        c.label = format!("{} ({label})", c.label);
        let src = c.expolygons();
        let boundary = c.expolygons();
        let expansion = c.f32();
        let step = c.f32();
        let max_steps = c.index();
        let params = RegionExpansionParameters::build(expansion, step, max_steps);
        c.is_params("build", &params);

        // 1 and 2: the seeds, unsorted and sorted.
        let unsorted = wave_seeds(&src, &boundary, params.tiny_expansion, false);
        let seeds = wave_seeds(&src, &boundary, params.tiny_expansion, true);
        c.is_seeds("wave_seeds(..., false)", &unsorted);
        if c.u8() == 0 {
            let perm = c.u32s();
            let permuted: Vec<WaveSeed> = perm
                .iter()
                .filter_map(|&i| unsorted.get(i as usize).cloned())
                .collect();
            c.list(
                "wave_seeds(..., true) (as the permutation)",
                &seeds,
                &permuted,
            );
        } else {
            c.is_seeds("wave_seeds(..., true)", &seeds);
        }

        // 3 and 4: the waves of the sorted seeds.
        let pw = propagate_waves(&seeds, &boundary, &params);
        let pwx = propagate_waves_ex(&seeds, &boundary, &params);
        c.is_expansions("propagate_waves(seeds)", &pw);
        c.is_expansions_ex("propagate_waves_ex(seeds)", &pwx);

        // 5: expand_expolygons, which equals 3 grouped by source.
        let ee = expand_expolygons(&src, &boundary, expansion, step, max_steps);
        let mut grouped = vec![Polygons::new(); src.len()];
        for r in &pw {
            if let Some(g) = grouped.get_mut(r.src_id as usize) {
                g.push(r.polygon.clone());
            }
        }
        let want_same = c.bool();
        c.same(
            "expand_expolygons equals 3 grouped",
            &(ee == grouped),
            &want_same,
        );
        if !want_same {
            let n = c.index();
            let want: Vec<Polygons> = (0..n).map(|_| c.polygons()).collect();
            c.list("expand_expolygons", &ee, &want);
        }

        // 6: the merge of 3 into the sources.
        let merged = merge_expansions_into_expolygons(src.clone(), pw.clone());
        c.is_expolygons("merge_expansions_into_expolygons", &merged);

        // 7: the overloads that start from the source.
        let pw2 = propagate_waves_src(&src, &boundary, &params);
        let want_same = c.bool();
        c.same(
            "propagate_waves(src, params) equals 3",
            &(pw2 == pw),
            &want_same,
        );
        if !want_same {
            c.is_expansions("propagate_waves(src, params)", &pw2);
        }
        let pw3 = propagate_waves_steps(&src, &boundary, expansion, step, max_steps);
        let want_same = c.bool();
        c.same(
            "propagate_waves(src, steps) equals 3",
            &(pw3 == pw),
            &want_same,
        );
        if !want_same {
            c.is_expansions("propagate_waves(src, steps)", &pw3);
        }
        let pwx2 = propagate_waves_ex_steps(&src, &boundary, expansion, step, max_steps);
        let want_same = c.bool();
        c.same(
            "propagate_waves_ex(src, steps) equals 4",
            &(pwx2 == pwx),
            &want_same,
        );
        if !want_same {
            c.is_expansions_ex("propagate_waves_ex(src, steps)", &pwx2);
        }
        let merged2 = expand_merge_expolygons(src.clone(), &boundary, &params);
        let want_same = c.bool();
        c.same(
            "expand_merge_expolygons equals 6",
            &(merged2 == merged),
            &want_same,
        );
        if !want_same {
            c.is_expolygons("expand_merge_expolygons", &merged2);
        }
    });
}

#[test]
fn merged_expansions_match() {
    replay("SFME", |c| {
        let src = c.expolygons();
        let expanded = c.expansions();
        let got = merge_expansions_into_expolygons(src, expanded);
        c.is_expolygons("merge_expansions_into_expolygons", &got);
    });
}

#[test]
fn layer_id_patterns_match() {
    replay("SFLP", |c| {
        let pattern = c.r.bytes().unwrap().to_vec();
        c.label = format!("{} \"{}\"", c.label, pattern.escape_ascii());
        let first = c.r.i32().unwrap();
        c.same("the first index", &first, &-2);
        let n = c.u32();
        for _ in 0..n {
            let layer_id = c.r.i32().unwrap();
            let got = check_layer_id_pattern(&pattern, layer_id);
            c.is_bool(&format!("layer {layer_id}"), got);
        }
    });
}

#[test]
fn every_kind_is_known() {
    let file = file();
    let unknown: Vec<&str> = file
        .records
        .iter()
        .map(|r| r.kind())
        .filter(|k| {
            !SURFACES_1_KINDS.contains(k)
                && !SURFACES_2_KINDS.contains(k)
                && !OTHER_PIECES_KINDS.contains(k)
        })
        .collect();
    assert!(
        unknown.is_empty(),
        "{}: {} records nothing replays ({:?}...)",
        file.path.display(),
        unknown.len(),
        &unknown[..unknown.len().min(5)]
    );
    for kind in SURFACES_1_KINDS.into_iter().chain(SURFACES_2_KINDS) {
        assert!(
            file.of_kind(kind).next().is_some(),
            "{}: no {kind} record",
            file.path.display()
        );
    }
}
