// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The probe gate for orca-index: the records of `index.sop1` (`parity/sop1/index.md`) that the
//! piece index-1 owns, replayed against the crate bit for bit (added 2026-11-13 by index-1):
//!
//! - IXBX: a `Tree<2, coord_t>` over `BoundingBoxWrapper` boxes (`build_modify_input`), its nodes,
//!   then `traverse` with RegionExpansion's predicate, `intersecting` and `within`, and
//!   `get_candidate_idxs`;
//! - IXTM: `build_aabb_tree_over_indexed_triangle_set` over float and double meshes, its nodes,
//!   the ray queries, the distance and radius queries, `get_candidate_idxs` and the box walks;
//! - IXLT: `build_aabb_tree_over_indexed_lines` and `LinesDistancer` over `Line` and `Linef`;
//! - IXKD: `KDTreeIndirect<2>` and `<3>` of doubles, `visit` and the four searches;
//! - IXPQ: the six priority queues (`MutablePriorityQueue` with and without the index reset, the
//!   skip heaps with blocks of 32 and 4) after every step;
//! - IXNP: the index-1 rows, each a call that stops with [`NotPorted`] and the same texts.
//!
//! and those that index-2 owns (added 2026-11-13 by index-2):
//!
//! - IXEG: an `EdgeGrid::Grid` made by each `create` overload, with or without a box set first:
//!   its box, size, contours (where each points into the input), cells and cell data, the cell
//!   walks along lines and boxes, the three distance queries at points, `contours_simplified`,
//!   `intersecting_edges` and `has_intersecting_edges`; then `calculate_sdf`'s field, the
//!   distances that read it and `contours_simplified` again;
//! - IXEI: the free `intersecting_edges(Polygons)`;
//! - IXNP: the index-2 rows, each a call that stops with [`NotPorted`] and the same texts.
//!
//! Every value is compared by its bits (a sign of zero or a NaN counts), and no other kind may be
//! in the file.
//!
//! The committed subset (`parity/probes/`) runs by default; `SKETCH_ORCA_PROBES_DEEP=<folder>`
//! replays the full set from that folder (`<work>/parity/probes`).

use std::cell::RefCell;
use std::fmt::{self, Debug, Formatter};
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::Rc;
use std::sync::Once;

use orca_eigen::{Cast, Real, Vec2d, Vec3d, Vec3i32, Vector};
use orca_geom::bounding_box::BoundingBox;
use orca_geom::expolygon::ExPolygon;
use orca_geom::line::Line;
use orca_geom::point::{Point, SCALING_FACTOR};
use orca_geom::polygon::Polygon;
use orca_geom::polyline::Polyline;
use orca_index::NotPorted;
use orca_index::aabb_lines::{
    IndexedLine, IntersectingLine, Linef, LinesDistancer, build_aabb_tree_over_indexed_lines,
    squared_distance_to_indexed_lines,
};
use orca_index::aabb_tree::{self as at, BoundingBoxWrapper, Hit, NPOS, Node, SourceNode, Tree};
use orca_index::aligned_box::{AlignedBox, BoxScalar, ExteriorDistance};
use orca_index::edge_grid::{self as eg, Grid};
use orca_index::kd_tree::{self as kd, KdTreeIndirect, VisitorReturnMask};
use orca_index::priority_queue::{
    MutablePriorityQueue, MutableSkipHeapPriorityQueue, make_miniheap_mutable_priority_queue,
    make_mutable_priority_queue,
};
use orca_probe::{Mismatches, ProbeFile, Reader, load_probe};

fn file() -> ProbeFile {
    load_probe("index").unwrap_or_else(|e| panic!("{e}"))
}

/// The kinds this file replays (IXNP only in its index-1 rows).
const INDEX_1_KINDS: [&str; 6] = ["IXBX", "IXTM", "IXLT", "IXKD", "IXPQ", "IXNP"];
/// The kinds index-2 replays.
const INDEX_2_KINDS: [&str; 2] = ["IXEG", "IXEI"];

/// A scalar of the records: `coord_t`, `float` or `double`, read as the record writes it and
/// compared by its bits.
trait Sc: Copy + Debug {
    fn read(c: &mut Case<'_, '_>) -> Self;
    fn bits(self) -> u64;
}

impl Sc for i64 {
    fn read(c: &mut Case<'_, '_>) -> Self {
        c.i64()
    }
    fn bits(self) -> u64 {
        u64::from_ne_bytes(self.to_ne_bytes())
    }
}

impl Sc for i32 {
    fn read(c: &mut Case<'_, '_>) -> Self {
        c.i32()
    }
    fn bits(self) -> u64 {
        u64::from(u32::from_ne_bytes(self.to_ne_bytes()))
    }
}

impl Sc for f32 {
    fn read(c: &mut Case<'_, '_>) -> Self {
        c.f32()
    }
    fn bits(self) -> u64 {
        u64::from(self.to_bits())
    }
}

impl Sc for f64 {
    fn read(c: &mut Case<'_, '_>) -> Self {
        c.f64()
    }
    fn bits(self) -> u64 {
        self.to_bits()
    }
}

/// A scalar compared by its bits.
#[derive(Clone, Copy)]
struct B<T>(T);

impl<T: Sc> PartialEq for B<T> {
    fn eq(&self, o: &Self) -> bool {
        self.0.bits() == o.0.bits()
    }
}

impl<T: Sc> Debug for B<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

/// A node as the record writes it: idx, the box's minimum and maximum.
type NodeOf<S, const N: usize> = (usize, [B<S>; N], [B<S>; N]);
/// A hit as the record writes it: id, gid, u, v, t.
type HitOf = (i32, i32, B<f32>, B<f32>, B<f32>);

fn hit_of(h: &Hit) -> HitOf {
    (h.id, h.gid, B(h.u), B(h.v), B(h.t))
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
    /// A `u64` count or index (npos is `usize::MAX`).
    fn index(&mut self) -> usize {
        usize::try_from(self.u64()).unwrap()
    }
    fn point(&mut self) -> Point {
        let x = self.i64();
        Point::new(x, self.i64())
    }
    fn vector<S: Sc, const N: usize>(&mut self) -> Vector<S, N> {
        Vector(core::array::from_fn(|_| S::read(self)))
    }
    fn indices(&mut self) -> Vec<usize> {
        let n = self.index();
        (0..n).map(|_| self.index()).collect()
    }
    fn hit(&mut self) -> HitOf {
        let (id, gid) = (self.i32(), self.i32());
        let (u, v) = (B(self.f32()), B(self.f32()));
        (id, gid, u, v, B(self.f32()))
    }
    fn line<L: MakeLine>(&mut self) -> L
    where
        L::Scalar: Sc,
    {
        let a = self.vector();
        L::make(a, self.vector())
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
            self.same(what, &got.to_vec_debug(), &want.to_vec_debug());
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
    fn is_i32(&mut self, what: &str, got: i32) {
        let want = self.i32();
        self.same(what, &got, &want);
    }
    fn is_index(&mut self, what: &str, got: usize) {
        let want = self.index();
        self.same(what, &got, &want);
    }
    fn is_f64(&mut self, what: &str, got: f64) {
        let want = B(self.f64());
        self.same(what, &B(got), &want);
    }
    fn is_vec<const N: usize>(&mut self, what: &str, got: &Vector<f64, N>) {
        let want = self.vector::<f64, N>().0.map(B);
        self.same(what, &got.0.map(B), &want);
    }
    fn is_indices(&mut self, what: &str, got: &[usize]) {
        let want = self.indices();
        self.list(what, got, &want);
    }
    fn is_nodes<S: Sc + BoxScalar, const N: usize>(&mut self, tree: &Tree<S, N>) {
        let n = self.index();
        let want: Vec<NodeOf<S, N>> = (0..n)
            .map(|_| {
                let idx = self.index();
                let min = self.vector::<S, N>().0.map(B);
                (idx, min, self.vector::<S, N>().0.map(B))
            })
            .collect();
        let got: Vec<NodeOf<S, N>> = tree
            .nodes()
            .iter()
            .map(|node| (node.idx, node.bbox.min.0.map(B), node.bbox.max.0.map(B)))
            .collect();
        self.list("nodes", &got, &want);
    }
}

/// A list shown whole when the counts differ.
trait ToVecDebug<T> {
    fn to_vec_debug(&self) -> ListOf<'_, T>;
}

/// A list compared by its items and shown with its count.
struct ListOf<'a, T>(&'a [T]);

impl<T: PartialEq> PartialEq for ListOf<'_, T> {
    fn eq(&self, o: &Self) -> bool {
        self.0 == o.0
    }
}

impl<T: Debug> Debug for ListOf<'_, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{} items {:?}", self.0.len(), self.0)
    }
}

impl<T> ToVecDebug<T> for [T] {
    fn to_vec_debug(&self) -> ListOf<'_, T> {
        ListOf(self)
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

/// The leaves `traverse` reaches with `pred`, stopping once it holds `stop` of them (0: never).
fn walk<S: BoxScalar, const N: usize>(
    tree: &Tree<S, N>,
    pred: impl FnMut(&Node<S, N>) -> bool,
    stop: u64,
) -> Vec<usize> {
    let mut out = Vec::new();
    at::traverse(tree, pred, |node: &Node<S, N>| {
        out.push(node.idx);
        stop == 0 || (out.len() as u64) < stop
    });
    out
}

#[test]
fn bounding_box_trees_match() {
    replay("IXBX", |c| {
        c.is_f64("SCALING_FACTOR", SCALING_FACTOR);
        let origin = BoundingBox::from_corners(Point::new(0, 0), Point::new(0, 0));
        let grown = BoundingBoxWrapper::new(0, &origin).bbox().max[0];
        let want = c.i64();
        c.same("BoundingBoxWrapper's epsilon", &grown, &want);
        let n = c.index();
        let boxes: Vec<BoundingBox> = (0..n)
            .map(|_| {
                let min = c.point();
                BoundingBox::from_corners(min, c.point())
            })
            .collect();
        let mut input: Vec<BoundingBoxWrapper> = boxes
            .iter()
            .enumerate()
            .map(|(i, b)| BoundingBoxWrapper::new(i, b))
            .collect();
        let mut tree = Tree::<i64, 2>::new();
        tree.build_modify_input(&mut input);
        c.is_nodes(&tree);
        for q in 0..c.index() {
            let kind = c.u8();
            let (a, b) = (c.point(), c.point());
            let stop = c.u64();
            let (a, b) = (Vector([a.x, a.y]), Vector([b.x, b.y]));
            let got = match kind {
                0 => walk(&tree, |node| node.bbox.contains_point(&a), stop),
                1 => walk(&tree, at::intersecting(AlignedBox::new(a, b)), stop),
                2 => walk(&tree, at::within(AlignedBox::new(a, b)), stop),
                _ => {
                    let mut out = Vec::new();
                    at::get_candidate_idxs(&tree, &a, &mut out, 0);
                    out
                }
            };
            c.is_indices(&format!("query {q} (kind {kind})"), &got);
        }
    });
}

/// One IXTM record after its `u8`, over vertices of the scalar `S`.
fn ixtm<S>(c: &mut Case<'_, '_>)
where
    S: Sc + Real + BoxScalar + Cast<f64> + ExteriorDistance<f64>,
    f64: Cast<S>,
{
    let label = c.str();
    c.label = format!("{} ({label})", c.label);
    let nv = c.index();
    let vertices: Vec<Vector<S, 3>> = (0..nv).map(|_| c.vector()).collect();
    let nf = c.index();
    let faces: Vec<Vec3i32> = (0..nf).map(|_| c.vector()).collect();
    let eps: S = Sc::read(c);
    let tree = at::build_aabb_tree_over_indexed_triangle_set(&vertices, &faces, eps);
    c.is_nodes(&tree);

    for k in 0..c.index() {
        let origin: Vec3d = c.vector();
        let dir: Vec3d = c.vector();
        let eps = c.f64();
        let first = at::intersect_ray_first_hit(&vertices, &faces, &tree, &origin, &dir, eps);
        let want_first = c.bool();
        c.same(
            &format!("ray {k}: intersect_ray_first_hit"),
            &first.is_some(),
            &want_first,
        );
        if want_first {
            let want = c.hit();
            if let Some(h) = first {
                c.same(&format!("ray {k}: its hit"), &hit_of(&h), &want);
            }
        }
        let mut hits = Vec::new();
        let any =
            at::intersect_ray_all_hits(&vertices, &faces, &tree, &origin, &dir, &mut hits, eps);
        c.is_bool(&format!("ray {k}: intersect_ray_all_hits"), any);
        let n = c.index();
        let want: Vec<HitOf> = (0..n).map(|_| c.hit()).collect();
        let got: Vec<HitOf> = hits.iter().map(hit_of).collect();
        c.list(&format!("ray {k}: its hits"), &got, &want);
    }

    for k in 0..c.index() {
        let pt: Vec3d = c.vector();
        let r2 = c.f64();
        let mut idx = NPOS;
        let mut closest = Vec3d::zero();
        let d = at::squared_distance_to_indexed_triangle_set(
            &vertices,
            &faces,
            &tree,
            &pt,
            &mut idx,
            &mut closest,
        );
        c.is_f64(
            &format!("point {k}: squared_distance_to_indexed_triangle_set"),
            d,
        );
        c.is_index(&format!("point {k}: its face"), idx);
        c.is_vec(&format!("point {k}: its closest point"), &closest);
        let any = at::is_any_triangle_in_radius(&vertices, &faces, &tree, &pt, r2);
        c.is_bool(&format!("point {k}: is_any_triangle_in_radius"), any);
        let all = at::all_triangles_in_radius(&vertices, &faces, &tree, &pt, r2);
        c.is_indices(&format!("point {k}: all_triangles_in_radius"), &all);
        let mut candidates = Vec::new();
        at::get_candidate_idxs(&tree, &pt.cast::<S>(), &mut candidates, 0);
        c.is_indices(&format!("point {k}: get_candidate_idxs"), &candidates);
    }

    for k in 0..c.index() {
        let lo: Vector<S, 3> = c.vector();
        let hi: Vector<S, 3> = c.vector();
        let stop = c.u64();
        let bbox = AlignedBox::new(lo, hi);
        let got = walk(&tree, at::intersecting(bbox), stop);
        c.is_indices(&format!("box {k}: intersecting"), &got);
        let got = walk(&tree, at::within(bbox), stop);
        c.is_indices(&format!("box {k}: within"), &got);
    }
}

#[test]
fn triangle_trees_match() {
    replay("IXTM", |c| match c.u8() {
        0 => ixtm::<f32>(c),
        1 => ixtm::<f64>(c),
        k => panic!("{}: scalar {k}", c.label),
    });
}

/// A line made of its two ends, as the records give them.
trait MakeLine: IntersectingLine + Clone {
    fn make(a: Vector<Self::Scalar, 2>, b: Vector<Self::Scalar, 2>) -> Self;
}

impl MakeLine for Line {
    fn make(a: Vector<i64, 2>, b: Vector<i64, 2>) -> Self {
        Line::new(Point::new(a[0], a[1]), Point::new(b[0], b[1]))
    }
}

impl MakeLine for Linef {
    fn make(a: Vec2d, b: Vec2d) -> Self {
        Linef::new(a, b)
    }
}

/// One IXLT record after its `u8`, over lines of the type `L`.
fn ixlt<L>(c: &mut Case<'_, '_>)
where
    L: MakeLine + IndexedLine<2, Floating = f64>,
    <L as IndexedLine<2>>::Scalar: Sc + Cast<f64> + ExteriorDistance<f64>,
    f64: Cast<<L as IndexedLine<2>>::Scalar>,
{
    let n = c.index();
    let lines: Vec<L> = (0..n).map(|_| c.line::<L>()).collect();
    let tree = build_aabb_tree_over_indexed_lines::<L, 2>(&lines);
    c.is_nodes(&tree);
    let distancer = LinesDistancer::<L, 2>::new(lines.clone());

    for k in 0..c.index() {
        let pt: Vector<<L as IndexedLine<2>>::Scalar, 2> = c.vector();
        let max_sqr = c.f64();
        let radius = c.f64();
        let (d, line, nearest) = distancer.distance_from_lines_extra::<false>(&pt);
        c.is_f64(&format!("point {k}: distance_from_lines_extra<false>"), d);
        c.is_index(&format!("point {k}: its line"), line);
        c.is_vec(&format!("point {k}: its nearest point"), &nearest);
        let (d, line, nearest) = distancer.distance_from_lines_extra::<true>(&pt);
        c.is_f64(&format!("point {k}: distance_from_lines_extra<true>"), d);
        c.is_index(&format!("point {k}: its line (signed)"), line);
        c.is_vec(&format!("point {k}: its nearest point (signed)"), &nearest);
        c.is_i32(&format!("point {k}: outside()"), distancer.outside(&pt));
        let mut idx = NPOS;
        let mut hit = Vec2d::zero();
        let r = squared_distance_to_indexed_lines(
            &lines,
            &tree,
            &pt.cast::<f64>(),
            &mut idx,
            &mut hit,
            max_sqr,
        );
        c.is_f64(&format!("point {k}: squared_distance_to_indexed_lines"), r);
        c.is_index(&format!("point {k}: its line (limited)"), idx);
        c.is_vec(&format!("point {k}: its hit"), &hit);
        let all = distancer.all_lines_in_radius(&pt, radius);
        c.is_indices(&format!("point {k}: all_lines_in_radius"), &all);
    }

    for k in 0..c.index() {
        let query: L = c.line();
        for sorted in [false, true] {
            let got = if sorted {
                distancer.intersections_with_line::<true>(&query)
            } else {
                distancer.intersections_with_line::<false>(&query)
            };
            let got: Vec<_> = got.iter().map(|(p, i)| (p.0.map(B), *i)).collect();
            let n = c.index();
            let want: Vec<_> = (0..n)
                .map(|_| {
                    let p = c.vector::<<L as IndexedLine<2>>::Scalar, 2>().0.map(B);
                    (p, c.index())
                })
                .collect();
            c.list(
                &format!("query line {k}: intersections_with_line<{sorted}>"),
                &got,
                &want,
            );
        }
    }
}

#[test]
fn line_trees_match() {
    replay("IXLT", |c| match c.u8() {
        0 => ixlt::<Line>(c),
        1 => ixlt::<Linef>(c),
        k => panic!("{}: line type {k}", c.label),
    });
}

/// One IXKD record after its `u8`, in `D` dimensions.
fn ixkd<const D: usize>(c: &mut Case<'_, '_>) {
    let n = c.index();
    let pts: Vec<[f64; D]> = (0..n).map(|_| core::array::from_fn(|_| c.f64())).collect();
    let from_indices = c.bool();
    let coord = |idx: usize, dim: usize| pts[idx][dim];
    let tree = if from_indices {
        KdTreeIndirect::<D, f64, _>::with_indices(coord, c.indices())
    } else {
        KdTreeIndirect::<D, f64, _>::with_count(coord, n)
    };

    let mut seen = Vec::new();
    tree.visit(&mut |idx: usize, dim: usize| {
        seen.push((idx, dim));
        VisitorReturnMask::ContinueLeft as u32 | VisitorReturnMask::ContinueRight as u32
    });
    let m = c.index();
    let want: Vec<(usize, usize)> = (0..m)
        .map(|_| {
            let idx = c.index();
            (idx, c.index())
        })
        .collect();
    c.list("visit", &seen, &want);

    for k in 0..c.index() {
        let pt: [f64; D] = core::array::from_fn(|_| c.f64());
        let (modulo, rem) = (c.u64(), c.u64());
        let radius = c.f64();
        let lo: [f64; D] = core::array::from_fn(|_| c.f64());
        let hi: [f64; D] = core::array::from_fn(|_| c.f64());
        let filter = |idx: usize| modulo == 0 || idx as u64 % modulo != rem;
        let (closest, three, nearby) = if modulo == 0 {
            (
                kd::find_closest_point_all(&tree, &pt),
                kd::find_closest_points_all::<3, D, f64, _, _>(&tree, &pt),
                kd::find_nearby_points_all(&tree, &pt, radius),
            )
        } else {
            (
                kd::find_closest_point(&tree, &pt, filter),
                kd::find_closest_points::<3, D, f64, _, _, _>(&tree, &pt, filter),
                kd::find_nearby_points(&tree, &pt, radius, filter),
            )
        };
        c.is_index(&format!("query {k}: find_closest_point"), closest);
        let want = [c.index(), c.index(), c.index()];
        c.same(&format!("query {k}: find_closest_points<3>"), &three, &want);
        c.is_indices(&format!("query {k}: find_nearby_points"), &nearby);
        let in_box = kd::find_nearby_points_in_box(&tree, &lo, &hi, filter);
        c.is_indices(&format!("query {k}: find_nearby_points in a box"), &in_box);
    }
}

#[test]
fn kd_trees_match() {
    replay("IXKD", |c| match c.u8() {
        2 => ixkd::<2>(c),
        3 => ixkd::<3>(c),
        d => panic!("{}: {d} dimensions", c.label),
    });
}

/// An IXPQ item: its key and the index the queue's setter writes.
#[derive(Clone, Copy)]
struct Item {
    key: f64,
    qidx: usize,
}

type Items = Rc<RefCell<Vec<Item>>>;

/// The slot of a skip heap that holds no item.
const PADDING: u32 = 0xFFFF_FFFF;

/// The six IXPQ queues behind one interface; each holds item numbers.
trait Queue {
    fn push_item(&mut self, item: u32);
    fn pop_item(&mut self);
    fn top_item(&mut self) -> u32;
    fn remove_at(&mut self, idx: usize);
    fn update_at(&mut self, idx: usize);
    fn item_at(&self, idx: usize) -> u32;
    fn clear_all(&mut self);
    fn item_count(&self) -> usize;
    /// Each slot's item, [`PADDING`] for a padding slot.
    fn slot_items(&self) -> Vec<u32>;
}

impl<S, L, const R: bool> Queue for MutablePriorityQueue<u32, S, L, R>
where
    S: FnMut(&mut u32, usize),
    L: FnMut(&u32, &u32) -> bool,
{
    fn push_item(&mut self, item: u32) {
        self.push(item);
    }
    fn pop_item(&mut self) {
        self.pop();
    }
    fn top_item(&mut self) -> u32 {
        *self.top()
    }
    fn remove_at(&mut self, idx: usize) {
        self.remove(idx);
    }
    fn update_at(&mut self, idx: usize) {
        self.update(idx);
    }
    fn item_at(&self, idx: usize) -> u32 {
        *self.get(idx)
    }
    fn clear_all(&mut self) {
        self.clear();
    }
    fn item_count(&self) -> usize {
        self.size()
    }
    fn slot_items(&self) -> Vec<u32> {
        self.as_slice().to_vec()
    }
}

impl<S, L, const BLOCK: usize, const R: bool> Queue
    for MutableSkipHeapPriorityQueue<u32, S, L, BLOCK, R>
where
    S: FnMut(&mut u32, usize),
    L: FnMut(&u32, &u32) -> bool,
{
    fn push_item(&mut self, item: u32) {
        self.push(item);
    }
    fn pop_item(&mut self) {
        self.pop();
    }
    fn top_item(&mut self) -> u32 {
        *self.top()
    }
    fn remove_at(&mut self, idx: usize) {
        self.remove(idx);
    }
    fn update_at(&mut self, idx: usize) {
        self.update(idx);
    }
    fn item_at(&self, idx: usize) -> u32 {
        *self.get(idx)
    }
    fn clear_all(&mut self) {
        self.clear();
    }
    fn item_count(&self) -> usize {
        self.size()
    }
    fn slot_items(&self) -> Vec<u32> {
        self.slots()
            .iter()
            .enumerate()
            .map(|(addr, &item)| if addr % BLOCK == 0 { PADDING } else { item })
            .collect()
    }
}

/// The probe's setter: the item's index.
fn setter(items: &Items) -> impl FnMut(&mut u32, usize) + 'static {
    let items = Rc::clone(items);
    move |item: &mut u32, idx: usize| items.borrow_mut()[*item as usize].qidx = idx
}

/// The probe's less: the keys compared with `<`.
fn less(items: &Items) -> impl FnMut(&u32, &u32) -> bool + 'static {
    let items = Rc::clone(items);
    move |a: &u32, b: &u32| {
        let items = items.borrow();
        items[*a as usize].key < items[*b as usize].key
    }
}

/// The IXPQ queue `instance` over `items`.
fn queue_of(instance: u8, items: &Items) -> Box<dyn Queue> {
    let (s, l) = (setter(items), less(items));
    match instance {
        0 => Box::new(make_mutable_priority_queue::<u32, false, _, _>(s, l)),
        1 => Box::new(make_mutable_priority_queue::<u32, true, _, _>(s, l)),
        2 => Box::new(make_miniheap_mutable_priority_queue::<u32, 32, false, _, _>(s, l)),
        3 => Box::new(make_miniheap_mutable_priority_queue::<u32, 32, true, _, _>(
            s, l,
        )),
        4 => Box::new(make_miniheap_mutable_priority_queue::<u32, 4, false, _, _>(
            s, l,
        )),
        _ => Box::new(make_miniheap_mutable_priority_queue::<u32, 4, true, _, _>(
            s, l,
        )),
    }
}

fn item_no(i: usize) -> u32 {
    u32::try_from(i).unwrap()
}

fn ixpq(c: &mut Case<'_, '_>) {
    let instance = c.u8();
    let n = c.index();
    let items: Items = Rc::new(RefCell::new(
        (0..n)
            .map(|_| Item {
                key: c.f64(),
                qidx: NPOS,
            })
            .collect(),
    ));
    let mut q = queue_of(instance, &items);
    for step in 0..c.index() {
        let code = c.u8();
        let a = c.index();
        let key = c.f64();
        match code {
            0 => {
                items.borrow_mut()[a].key = key;
                q.push_item(item_no(a));
            }
            1 => q.pop_item(),
            2 => q.remove_at(a),
            3 => {
                let item = q.item_at(a) as usize;
                items.borrow_mut()[item].key = key;
                q.update_at(a);
            }
            _ => q.clear_all(),
        }
        c.is_index(
            &format!("step {step} (code {code}): size()"),
            q.item_count(),
        );
        let slots = c.index();
        let want: Vec<u32> = (0..slots).map(|_| c.u32()).collect();
        c.list(&format!("step {step}: the slots"), &q.slot_items(), &want);
        let got: Vec<usize> = items.borrow().iter().map(|item| item.qidx).collect();
        let want: Vec<usize> = (0..got.len()).map(|_| c.index()).collect();
        c.list(&format!("step {step}: the indices"), &got, &want);
    }
}

#[test]
fn priority_queues_match() {
    replay("IXPQ", ixpq);
}

/// Runs `f`, turning a [`NotPorted`] stop into its texts; other panics go on.
fn stop_of(f: impl FnOnce()) -> Option<(String, String)> {
    static QUIET: Once = Once::new();
    QUIET.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if info.payload().downcast_ref::<NotPorted>().is_none() {
                default(info);
            }
        }));
    });
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(()) => None,
        Err(payload) => match payload.downcast::<NotPorted>() {
            Ok(stop) => Some((stop.function.to_owned(), stop.condition.to_owned())),
            Err(other) => resume_unwind(other),
        },
    }
}

#[test]
fn not_ported_rows_stop() {
    let mut rows = 0;
    replay("IXNP", |c| {
        let piece = c.str();
        let function = c.str();
        let condition = c.str();
        let n = c.index();
        let example: Vec<i64> = (0..n).map(|_| c.i64()).collect();
        if piece != "index-1" {
            c.same("the piece", &piece.as_str(), &"index-2");
            return;
        }
        rows += 1;
        // The priority queues: the IXPQ queue number, the items pushed, then the index or address.
        let instance = u8::try_from(example[0]).unwrap();
        let pushed = usize::try_from(example[1]).unwrap();
        let (calls, at): (&[&str], usize) = match example.len() {
            2 => (&["top", "pop"], 0),
            3 => (
                &["remove", "update", "operator[]"],
                usize::try_from(example[2]).unwrap(),
            ),
            k => panic!("{}: an example of {k} numbers", c.label),
        };
        let want = Some((function.clone(), condition.clone()));
        for &call in calls {
            let got = stop_of(|| {
                let items: Items = Rc::new(RefCell::new(vec![
                    Item {
                        key: 0.0,
                        qidx: NPOS
                    };
                    pushed
                ]));
                let mut q = queue_of(instance, &items);
                for i in 0..pushed {
                    q.push_item(item_no(i));
                }
                match call {
                    "top" => {
                        q.top_item();
                    }
                    "pop" => q.pop_item(),
                    "remove" => q.remove_at(at),
                    "update" => q.update_at(at),
                    _ => {
                        q.item_at(at);
                    }
                }
            });
            c.same(&format!("queue {instance}, {call}"), &got, &want);
        }
    });
    assert!(rows > 0, "index.sop1 holds no IXNP row of index-1");
}

/// Every kind of the file has a replay here or in index-2's, and every kind here is recorded.
#[test]
fn every_kind_is_known() {
    let file = file();
    let unknown: Vec<&str> = file
        .records
        .iter()
        .map(|r| r.kind())
        .filter(|k| !INDEX_1_KINDS.contains(k) && !INDEX_2_KINDS.contains(k))
        .collect();
    assert!(
        unknown.is_empty(),
        "{}: {} records nothing replays ({:?}...)",
        file.path.display(),
        unknown.len(),
        &unknown[..unknown.len().min(4)]
    );
    for kind in INDEX_1_KINDS {
        assert!(
            file.of_kind(kind).next().is_some(),
            "{}: no {kind} record",
            file.path.display()
        );
    }
}

// index-2: EdgeGrid (IXEG, IXEI and index-2's IXNP rows), added 2026-11-13.

impl Case<'_, '_> {
    /// Points: a `u64` count, then each point.
    fn points(&mut self) -> Vec<Point> {
        let n = self.index();
        (0..n).map(|_| self.point()).collect()
    }
    /// Polygons: a `u64` count, then each one's points.
    fn polygons(&mut self) -> Vec<Vec<Point>> {
        let n = self.index();
        (0..n).map(|_| self.points()).collect()
    }
    /// Cells: a `u64` count, then each `i64` row and column.
    fn cells(&mut self) -> Vec<(i64, i64)> {
        let n = self.index();
        (0..n)
            .map(|_| {
                let r = self.i64();
                (r, self.i64())
            })
            .collect()
    }
    /// Pairs of `u64`: a count, then each pair.
    fn pairs(&mut self) -> Vec<(usize, usize)> {
        let n = self.index();
        (0..n)
            .map(|_| {
                let a = self.index();
                (a, self.index())
            })
            .collect()
    }
}

/// A contour as IXEG writes it: the input path it points into (npos if none) and its offset
/// there, its point count and points, `open()`, `num_segments()`, `get_segments()`, and each
/// segment's `segment_idx_prev`, `segment_idx_next` and `segment_prev`.
#[derive(Debug, PartialEq)]
struct ContourOf {
    from: usize,
    offset: usize,
    count: usize,
    points: Vec<Point>,
    open: bool,
    num_segments: usize,
    segments: Vec<(Point, Point)>,
    neighbours: Vec<(usize, usize, Point)>,
}

/// One point's queries before the field: `closest_point_signed_distance` (contour, start point,
/// distance, t), `signed_distance_edges` (its result, d, on) and `signed_distance` (its result, d).
type AtOf = (
    usize,
    usize,
    B<f64>,
    B<f64>,
    bool,
    B<f64>,
    bool,
    bool,
    B<f64>,
);

fn polygons_of(polys: &[Polygon]) -> Vec<Vec<Point>> {
    polys.iter().map(|p| p.points.clone()).collect()
}

/// The cells a walk gives its visitor.
fn visit(walk: impl FnOnce(&mut Vec<(i64, i64)>)) -> Vec<(i64, i64)> {
    let mut cells = Vec::new();
    walk(&mut cells);
    cells
}

fn ixeg(c: &mut Case<'_, '_>) {
    let label = c.str();
    c.label = format!("{} ({label})", c.label);
    let overload = c.u8();
    let open = c.bool();
    let n = c.index();
    let paths: Vec<(u8, u32, Vec<Point>)> = (0..n)
        .map(|_| {
            let role = c.u8();
            let group = c.u32();
            (role, group, c.points())
        })
        .collect();
    let set_box = if c.bool() {
        let min = c.point();
        let mut b = BoundingBox::from_corners(min, c.point());
        b.defined = c.bool();
        Some(b)
    } else {
        None
    };
    let resolution = c.i64();

    // The containers create() is given, made from the paths in order; the grid's contours point
    // into them. `sources` holds each path's points as the grid sees them (address and count).
    let mut polygons: Vec<Polygon> = Vec::new();
    let mut polylines: Vec<Polyline> = Vec::new();
    let mut lists: Vec<Vec<Point>> = Vec::new();
    let mut expolys: Vec<ExPolygon> = Vec::new();
    for (role, group, pts) in &paths {
        match role {
            0 => polygons.push(Polygon::new(pts.clone())),
            1 => polylines.push(Polyline::new(pts.clone())),
            4 => lists.push(pts.clone()),
            _ => {
                let g = *group as usize;
                if expolys.len() <= g {
                    expolys.resize(g + 1, ExPolygon::default());
                }
                if *role == 2 {
                    expolys[g].contour = Polygon::new(pts.clone());
                } else {
                    expolys[g].holes.push(Polygon::new(pts.clone()));
                }
            }
        }
    }
    let mut sources: Vec<(usize, usize)> = Vec::with_capacity(paths.len());
    {
        let (mut ipoly, mut iline, mut ilist) = (0, 0, 0);
        let mut holes = vec![0_usize; expolys.len()];
        for (role, group, _) in &paths {
            let g = *group as usize;
            let pts: &[Point] = match role {
                0 => {
                    ipoly += 1;
                    &polygons[ipoly - 1].points
                }
                1 => {
                    iline += 1;
                    &polylines[iline - 1].points
                }
                4 => {
                    ilist += 1;
                    &lists[ilist - 1]
                }
                2 => &expolys[g].contour.points,
                _ => {
                    holes[g] += 1;
                    &expolys[g].holes[holes[g] - 1].points
                }
            };
            sources.push((pts.as_ptr() as usize, pts.len()));
        }
    }
    let pointers: Vec<&Polygon> = polygons.iter().collect();
    // Overload 4 takes the first ExPolygon, or an empty one when there is none.
    let none = ExPolygon::default();
    let mut grid = Grid::new();
    if let Some(b) = set_box {
        grid.set_bbox(b);
    }
    match overload {
        0 => grid.create_polygons(&polygons, resolution),
        1 => grid.create_polygon_refs(&pointers, resolution),
        2 => grid.create_points(&lists, resolution, open),
        3 => grid.create_polygons_polylines(&polygons, &polylines, resolution),
        4 => grid.create_expolygon(expolys.first().unwrap_or(&none), resolution),
        _ => grid.create_expolygons(&expolys, resolution),
    }

    let min = c.point();
    let max = c.point();
    let defined = c.bool();
    let b = grid.bbox();
    c.same("bbox()", &(b.min, b.max, b.defined), &(min, max, defined));
    let want = c.i64();
    c.same("resolution()", &grid.resolution(), &want);
    c.is_index("rows()", grid.rows());
    c.is_index("cols()", grid.cols());

    let got: Vec<ContourOf> = grid
        .contours()
        .iter()
        .map(|ct| {
            let begin = ct.points().as_ptr() as usize;
            let (mut from, mut offset) = (NPOS, 0);
            for (i, &(addr, len)) in sources.iter().enumerate() {
                if len != 0 && begin >= addr && begin < addr + len * size_of::<Point>() {
                    from = i;
                    offset = (begin - addr) / size_of::<Point>();
                }
            }
            ContourOf {
                from,
                offset,
                count: ct.size(),
                points: ct.points().to_vec(),
                open: ct.open(),
                num_segments: ct.num_segments(),
                segments: ct.get_segments().iter().map(|l| (l.a, l.b)).collect(),
                neighbours: (0..ct.num_segments())
                    .map(|i| {
                        (
                            ct.segment_idx_prev(i),
                            ct.segment_idx_next(i),
                            ct.segment_prev(i),
                        )
                    })
                    .collect(),
            }
        })
        .collect();
    let n = c.index();
    let want: Vec<ContourOf> = (0..n)
        .map(|_| {
            let from = c.index();
            let offset = c.index();
            let count = c.index();
            let points = c.points();
            let open = c.bool();
            let num_segments = c.index();
            let ns = c.index();
            let segments = (0..ns)
                .map(|_| {
                    let a = c.point();
                    (a, c.point())
                })
                .collect();
            let neighbours = (0..num_segments)
                .map(|_| {
                    let prev = c.index();
                    let next = c.index();
                    (prev, next, c.point())
                })
                .collect();
            ContourOf {
                from,
                offset,
                count,
                points,
                open,
                num_segments,
                segments,
                neighbours,
            }
        })
        .collect();
    c.list("contours", &got, &want);

    let got: Vec<(usize, usize)> = grid
        .cells()
        .iter()
        .map(|cell| (cell.begin, cell.end))
        .collect();
    let want = c.pairs();
    c.list("cells", &got, &want);
    let want = c.pairs();
    c.list("cell data", grid.cell_data(), &want);

    for k in 0..c.index() {
        let (a, b) = (c.point(), c.point());
        let stop = c.u64();
        let got = visit(|cells| {
            grid.visit_cells_intersecting_line(a, b, &mut |iy, ix| {
                cells.push((iy, ix));
                stop == 0 || (cells.len() as u64) < stop
            });
        });
        let want = c.cells();
        c.list(
            &format!("line {k}: visit_cells_intersecting_line"),
            &got,
            &want,
        );
    }
    for k in 0..c.index() {
        let min = c.point();
        let bbox = BoundingBox::from_corners(min, c.point());
        let stop = c.u64();
        let got = visit(|cells| {
            grid.visit_cells_intersecting_box(bbox, &mut |iy, ix| {
                cells.push((iy, ix));
                stop == 0 || (cells.len() as u64) < stop
            });
        });
        let want = c.cells();
        c.list(
            &format!("box {k}: visit_cells_intersecting_box"),
            &got,
            &want,
        );
    }

    let n = c.index();
    let mut ats = Vec::with_capacity(n);
    for k in 0..n {
        let pt = c.point();
        let radius = c.i64();
        ats.push((pt, radius));
        let r = grid.closest_point_signed_distance(pt, radius);
        let (mut d, mut on) = (12345.5, false);
        let sde = grid.signed_distance_edges(pt, radius, &mut d, Some(&mut on));
        let mut d2 = 12345.5;
        let sd = grid.signed_distance(pt, radius, &mut d2);
        let got: AtOf = (
            r.contour_idx,
            r.start_point_idx,
            B(r.distance),
            B(r.t),
            sde,
            B(d),
            on,
            sd,
            B(d2),
        );
        let want: AtOf = (
            c.index(),
            c.index(),
            B(c.f64()),
            B(c.f64()),
            c.bool(),
            B(c.f64()),
            c.bool(),
            c.bool(),
            B(c.f64()),
        );
        c.same(&format!("point {k} ({pt:?}, radius {radius})"), &got, &want);
    }

    let n = c.index();
    let mut simplify = Vec::with_capacity(n);
    for _ in 0..n {
        let offset = c.i64();
        let fill_holes = c.bool();
        simplify.push((offset, fill_holes));
        let got = polygons_of(&grid.contours_simplified(offset, fill_holes));
        let want = c.polygons();
        c.list(
            &format!("contours_simplified({offset}, {fill_holes})"),
            &got,
            &want,
        );
    }

    let got: Vec<[usize; 4]> = grid
        .intersecting_edges()
        .iter()
        .map(|&((ic, ipt), (jc, jpt))| [ic, ipt, jc, jpt])
        .collect();
    let n = c.index();
    let want: Vec<[usize; 4]> = (0..n)
        .map(|_| [c.index(), c.index(), c.index(), c.index()])
        .collect();
    c.list("intersecting_edges()", &got, &want);
    c.is_bool("has_intersecting_edges()", grid.has_intersecting_edges());

    if !c.bool() {
        return;
    }
    grid.calculate_sdf();
    let n = c.index();
    let want: Vec<B<f32>> = (0..n).map(|_| B(c.f32())).collect();
    let got: Vec<B<f32>> = grid.signed_distance_field().iter().map(|&f| B(f)).collect();
    c.list("calculate_sdf()'s field", &got, &want);
    for (k, &(pt, radius)) in ats.iter().enumerate() {
        let mut d = 12345.5;
        let sd = grid.signed_distance(pt, radius, &mut d);
        let bilinear = grid.signed_distance_bilinear(pt);
        let want = (c.bool(), B(c.f64()), B(c.f32()));
        c.same(
            &format!("point {k} ({pt:?}, radius {radius}) with the field"),
            &(sd, B(d), B(bilinear)),
            &want,
        );
    }
    for &(offset, fill_holes) in &simplify {
        let got = polygons_of(&grid.contours_simplified(offset, fill_holes));
        let want = c.polygons();
        c.list(
            &format!("contours_simplified({offset}, {fill_holes}) with the field"),
            &got,
            &want,
        );
    }
}

#[test]
fn edge_grids_match() {
    replay("IXEG", ixeg);
}

#[test]
fn intersecting_edges_match() {
    replay("IXEI", |c| {
        let polys: Vec<Polygon> = c.polygons().into_iter().map(Polygon::new).collect();
        let out = eg::intersecting_edges(&polys);
        c.is_bool("empty()", out.is_empty());
        let got: Vec<(usize, usize)> = out.iter().map(|&((_, a), (_, b))| (a, b)).collect();
        let want = c.pairs();
        c.list("the pairs' segments", &got, &want);
    });
}

/// Points from an example's numbers, x then y.
fn points_of(v: &[i64]) -> Vec<Point> {
    v.chunks(2).map(|p| Point::new(p[0], p[1])).collect()
}

#[test]
fn edge_grid_not_ported_rows_stop() {
    let mut rows = 0;
    replay("IXNP", |c| {
        let piece = c.str();
        let function = c.str();
        let condition = c.str();
        let n = c.index();
        let example: Vec<i64> = (0..n).map(|_| c.i64()).collect();
        if piece != "index-2" {
            // index-1's rows replay in not_ported_rows_stop.
            return;
        }
        rows += 1;
        let want = Some((function.clone(), condition.clone()));
        match function.as_str() {
            "EdgeGrid::Grid queries" => {
                // A grid never created: each query that reads the resolution stops.
                let calls = [
                    "resolution",
                    "visit_cells_intersecting_line",
                    "visit_cells_intersecting_box",
                    "closest_point_signed_distance",
                    "signed_distance_edges",
                    "signed_distance",
                    "signed_distance_bilinear",
                    "calculate_sdf",
                ];
                for call in calls {
                    let got = stop_of(|| {
                        let mut grid = Grid::new();
                        let origin = Point::new(0, 0);
                        let mut d = 0.0;
                        match call {
                            "resolution" => {
                                let _ = grid.resolution();
                            }
                            "visit_cells_intersecting_line" => {
                                grid.visit_cells_intersecting_line(origin, origin, &mut |_, _| true)
                            }
                            "visit_cells_intersecting_box" => grid
                                .visit_cells_intersecting_box(BoundingBox::new(), &mut |_, _| true),
                            "closest_point_signed_distance" => {
                                let _ = grid.closest_point_signed_distance(origin, 0);
                            }
                            "signed_distance_edges" => {
                                let _ = grid.signed_distance_edges(origin, 0, &mut d, None);
                            }
                            "signed_distance" => {
                                let _ = grid.signed_distance(origin, 0, &mut d);
                            }
                            "signed_distance_bilinear" => {
                                let _ = grid.signed_distance_bilinear(origin);
                            }
                            _ => grid.calculate_sdf(),
                        }
                    });
                    c.same(&format!("a grid never created, {call}"), &got, &want);
                }
            }
            "Slic3r::intersecting_edges(const Polygons &)" => {
                // Each polygon's point count, then its points.
                let mut polys = Vec::new();
                let mut k = 0;
                while k < example.len() {
                    let m = usize::try_from(example[k]).unwrap();
                    polys.push(Polygon::new(points_of(&example[k + 1..k + 1 + 2 * m])));
                    k += 1 + 2 * m;
                }
                let got = stop_of(|| {
                    let _ = eg::intersecting_edges(&polys);
                });
                c.same(&function, &got, &want);
            }
            _ => {
                // The resolution, the polygon's points, then the query's points.
                let resolution = example[0];
                let rest = &example[1..];
                let query_len = match function.as_str() {
                    "EdgeGrid::Grid::visit_cells_intersecting_line" => 4,
                    "EdgeGrid::Grid::signed_distance_bilinear" => 2,
                    _ => 0,
                };
                let polygons = vec![Polygon::new(points_of(&rest[..rest.len() - query_len]))];
                let query = points_of(&rest[rest.len() - query_len..]);
                let got = stop_of(|| {
                    let mut grid = Grid::new();
                    grid.create_polygons(&polygons, resolution);
                    match query.len() {
                        2 => {
                            grid.visit_cells_intersecting_line(query[0], query[1], &mut |_, _| true)
                        }
                        1 => {
                            let _ = grid.signed_distance_bilinear(query[0]);
                        }
                        _ => {}
                    }
                });
                c.same(&format!("{function}: {condition}"), &got, &want);
            }
        }
    });
    assert!(rows > 0, "index.sop1 holds no IXNP row of index-2");
}

/// index-2's kinds are in the file.
#[test]
fn edge_grid_kinds_are_recorded() {
    let file = file();
    for kind in INDEX_2_KINDS {
        assert!(
            file.of_kind(kind).next().is_some(),
            "{}: no {kind} record",
            file.path.display()
        );
    }
}
