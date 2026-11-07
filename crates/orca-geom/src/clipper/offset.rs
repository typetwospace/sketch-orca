// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Clipper code: class ClipperOffset and GetUnitNormal of deps_src/clipper/clipper.cpp and
// clipper.hpp of OrcaSlicer v2.4.2 (Clipper 6.4.2 with the PrusaSlicer authors' changes),
// translated on 2026-11-07. Copyright Angus Johnson 2010-2017 and the PrusaSlicer authors, under
// the Boost Software License, version 1.0 (BSL-1.0; licenses/BSL-1.0.txt).

//! `ClipperOffset`: offsets closed polygons and open or closed lines by a delta, with square,
//! round and miter joins and butt, square and round ends, then cleans the result with a union.
//!
//! The trigonometry is the static UCRT's (`acos`, `sin`, `cos`, `atan2`, `tan` through
//! `orca-crt`), and every point rounds through `Round<cInt>` as the C++ does.

use super::{
    ClipType, Clipper, DEF_ARC_TOLERANCE, EndType, IntPoint, JoinType, PI, Path, Paths,
    PolyFillType, PolyTree, PolyType, TWO_PI, orientation, round_cint,
};
use crate::point::Point;

/// `DoublePoint`: a unit normal.
#[derive(Clone, Copy, Debug, Default)]
struct DoublePoint {
    x: f64,
    y: f64,
}

/// A path added to the offsetter: the C++ keeps it as a child `PolyNode` of `m_polyNodes`.
#[derive(Clone, Debug)]
struct Node {
    contour: Path,
    join_type: JoinType,
    end_type: EndType,
}

/// `ClipperOffset`.
#[derive(Clone, Debug)]
pub struct ClipperOffset {
    /// `MiterLimit` (2 by default).
    pub miter_limit: f64,
    /// `ArcTolerance` (0.25 by default).
    pub arc_tolerance: f64,
    /// `ShortestEdgeLength`: closer points merge in `AddPath` (0 by default: only equal ones).
    pub shortest_edge_length: f64,
    dest_polys: Paths,
    src_poly: Path,
    dest_poly: Path,
    normals: Vec<DoublePoint>,
    delta: f64,
    sin_a: f64,
    sin: f64,
    cos: f64,
    miter_lim: f64,
    steps_per_rad: f64,
    /// `m_lowest`: x the index of the lowest contour, y the index of its lowest point.
    lowest: Point,
    /// `m_polyNodes`' children.
    nodes: Vec<Node>,
}

impl Default for ClipperOffset {
    fn default() -> Self {
        Self::new(2.0, DEF_ARC_TOLERANCE, 0.0)
    }
}

/// `NEAR_ZERO`.
fn near_zero(val: f64) -> bool {
    const TOLERANCE: f64 = 1.0e-20;
    val > -TOLERANCE && val < TOLERANCE
}

/// `GetUnitNormal`.
fn get_unit_normal(pt1: IntPoint, pt2: IntPoint) -> DoublePoint {
    if pt2.x == pt1.x && pt2.y == pt1.y {
        return DoublePoint { x: 0.0, y: 0.0 };
    }
    let mut dx = pt2.x.wrapping_sub(pt1.x) as f64;
    let mut dy = pt2.y.wrapping_sub(pt1.y) as f64;
    let f = 1.0 / orca_crt::sqrt(dx * dx + dy * dy);
    dx *= f;
    dy *= f;
    DoublePoint { x: dy, y: -dx }
}

impl ClipperOffset {
    /// `ClipperOffset(miterLimit, roundPrecision, shortestEdgeLength)`.
    #[must_use]
    pub fn new(miter_limit: f64, arc_tolerance: f64, shortest_edge_length: f64) -> Self {
        Self {
            miter_limit,
            arc_tolerance,
            shortest_edge_length,
            dest_polys: Paths::new(),
            src_poly: Path::new(),
            dest_poly: Path::new(),
            normals: Vec::new(),
            delta: 0.0,
            sin_a: 0.0,
            sin: 0.0,
            cos: 0.0,
            miter_lim: 0.0,
            steps_per_rad: 0.0,
            lowest: Point::new(-1, 0),
            nodes: Vec::new(),
        }
    }

    /// `Clear()`.
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.lowest.x = -1;
    }

    /// `AddPath`: strips duplicate points (or points closer than `ShortestEdgeLength`) and
    /// keeps the lowest point of the closed polygons.
    pub fn add_path(&mut self, path: &[IntPoint], join_type: JoinType, end_type: EndType) {
        let mut high_i = path.len() as i32 - 1;
        if high_i < 0 {
            return;
        }
        let has_shortest_edge_length = self.shortest_edge_length > 0.0;
        let shortest_edge_length2 = if has_shortest_edge_length {
            self.shortest_edge_length * self.shortest_edge_length
        } else {
            0.0
        };
        let same = |a: IntPoint, b: IntPoint| {
            if has_shortest_edge_length {
                let dx = a.x.wrapping_sub(b.x) as f64;
                let dy = a.y.wrapping_sub(b.y) as f64;
                dx * dx + dy * dy < shortest_edge_length2
            } else {
                a == b
            }
        };
        if end_type == EndType::ClosedLine || end_type == EndType::ClosedPolygon {
            while high_i > 0 {
                if !same(path[high_i as usize], path[0]) {
                    break;
                }
                high_i -= 1;
            }
        }
        let mut contour = Path::with_capacity(high_i as usize + 1);
        contour.push(path[0]);
        let (mut j, mut k) = (0_usize, 0_usize);
        for &p in &path[1..=high_i as usize] {
            if same(p, contour[j]) {
                continue;
            }
            j += 1;
            contour.push(p);
            if p.y > contour[k].y || (p.y == contour[k].y && p.x < contour[k].x) {
                k = j;
            }
        }
        if end_type == EndType::ClosedPolygon && j < 2 {
            return;
        }
        self.nodes.push(Node {
            contour,
            join_type,
            end_type,
        });

        // A lower lowest point than all the others updates m_lowest.
        if end_type != EndType::ClosedPolygon {
            return;
        }
        let last = self.nodes.len() - 1;
        if self.lowest.x < 0 {
            self.lowest = Point::new(last as i64, k as i64);
        } else {
            let ip = self.nodes[self.lowest.x as usize].contour[self.lowest.y as usize];
            let ck = self.nodes[last].contour[k];
            if ck.y > ip.y || (ck.y == ip.y && ck.x < ip.x) {
                self.lowest = Point::new(last as i64, k as i64);
            }
        }
    }

    /// `AddPaths`.
    pub fn add_paths<'a>(
        &mut self,
        paths: impl IntoIterator<Item = &'a [IntPoint]>,
        join_type: JoinType,
        end_type: EndType,
    ) {
        for path in paths {
            self.add_path(path, join_type, end_type);
        }
    }

    /// `FixOrientations()`: when the polygon with the lowest vertex is clockwise, every closed
    /// polygon is reversed; closed lines are made clockwise.
    fn fix_orientations(&mut self) {
        if self.lowest.x >= 0 && !orientation(&self.nodes[self.lowest.x as usize].contour) {
            for node in &mut self.nodes {
                if node.end_type == EndType::ClosedPolygon
                    || (node.end_type == EndType::ClosedLine && orientation(&node.contour))
                {
                    node.contour.reverse();
                }
            }
        } else {
            for node in &mut self.nodes {
                if node.end_type == EndType::ClosedLine && !orientation(&node.contour) {
                    node.contour.reverse();
                }
            }
        }
    }

    /// The corner-cleaning union of `Execute`.
    fn union_of_offsets(&self, delta: f64) -> Clipper {
        let mut clpr = Clipper::new(0);
        clpr.add_paths(
            self.dest_polys.iter().map(Vec::as_slice),
            PolyType::Subject,
            true,
        );
        if delta <= 0.0 {
            let r = clpr.get_bounds();
            let outer = [
                Point::new(r.left.wrapping_sub(10), r.bottom.wrapping_add(10)),
                Point::new(r.right.wrapping_add(10), r.bottom.wrapping_add(10)),
                Point::new(r.right.wrapping_add(10), r.top.wrapping_sub(10)),
                Point::new(r.left.wrapping_sub(10), r.top.wrapping_sub(10)),
            ];
            clpr.add_path(&outer, PolyType::Subject, true);
            clpr.set_reverse_solution(true);
        }
        clpr
    }

    /// `Execute(Paths&, delta)`.
    pub fn execute(&mut self, solution: &mut Paths, delta: f64) {
        solution.clear();
        self.fix_orientations();
        self.do_offset(delta);
        // Clean up the corners.
        let mut clpr = self.union_of_offsets(delta);
        if delta > 0.0 {
            clpr.execute(
                ClipType::Union,
                solution,
                PolyFillType::Positive,
                PolyFillType::Positive,
            );
        } else {
            clpr.execute(
                ClipType::Union,
                solution,
                PolyFillType::Negative,
                PolyFillType::Negative,
            );
            if !solution.is_empty() {
                solution.remove(0);
            }
        }
    }

    /// `Execute(PolyTree&, delta)`.
    pub fn execute_tree(&mut self, solution: &mut PolyTree, delta: f64) {
        solution.clear();
        self.fix_orientations();
        self.do_offset(delta);
        let mut clpr = self.union_of_offsets(delta);
        if delta > 0.0 {
            clpr.execute_tree(
                ClipType::Union,
                solution,
                PolyFillType::Positive,
                PolyFillType::Positive,
            );
        } else {
            clpr.execute_tree(
                ClipType::Union,
                solution,
                PolyFillType::Negative,
                PolyFillType::Negative,
            );
            // Remove the outer rectangle.
            solution.remove_outermost_polygon();
        }
    }

    /// The point `src_poly[j] + n * delta`, rounded.
    fn offset_by(&self, j: usize, n: DoublePoint, delta: f64) -> IntPoint {
        let s = self.src_poly[j];
        Point::new(
            round_cint(s.x as f64 + n.x * delta),
            round_cint(s.y as f64 + n.y * delta),
        )
    }

    /// `DoOffset(delta)`.
    fn do_offset(&mut self, delta: f64) {
        self.dest_polys.clear();
        self.delta = delta;

        // A zero offset copies the closed polygons.
        if near_zero(delta) {
            for node in &self.nodes {
                if node.end_type == EndType::ClosedPolygon {
                    self.dest_polys.push(node.contour.clone());
                }
            }
            return;
        }

        self.miter_lim = if self.miter_limit > 2.0 {
            2.0 / (self.miter_limit * self.miter_limit)
        } else {
            0.5
        };
        let abs_delta = orca_crt::fabs(delta);
        let y = if self.arc_tolerance <= 0.0 {
            DEF_ARC_TOLERANCE
        } else if self.arc_tolerance > abs_delta * DEF_ARC_TOLERANCE {
            abs_delta * DEF_ARC_TOLERANCE
        } else {
            self.arc_tolerance
        };
        let mut steps = PI / orca_crt::acos(1.0 - y / abs_delta);
        if steps > abs_delta * PI {
            steps = abs_delta * PI; // Excessive precision check.
        }
        self.sin = orca_crt::sin(TWO_PI / steps);
        self.cos = orca_crt::cos(TWO_PI / steps);
        self.steps_per_rad = steps / TWO_PI;
        if delta < 0.0 {
            self.sin = -self.sin;
        }

        self.dest_polys.reserve(self.nodes.len() * 2);
        for i in 0..self.nodes.len() {
            self.src_poly.clone_from(&self.nodes[i].contour);
            let (join_type, end_type) = (self.nodes[i].join_type, self.nodes[i].end_type);
            let len = self.src_poly.len();
            if len == 0 || (delta <= 0.0 && (len < 3 || end_type != EndType::ClosedPolygon)) {
                continue;
            }

            self.dest_poly.clear();
            if len == 1 {
                let s = self.src_poly[0];
                if join_type == JoinType::Round {
                    let (mut x, mut y) = (1.0, 0.0);
                    let mut j: i64 = 1;
                    while j as f64 <= steps {
                        self.dest_poly.push(Point::new(
                            round_cint(s.x as f64 + x * delta),
                            round_cint(s.y as f64 + y * delta),
                        ));
                        let x2 = x;
                        x = x * self.cos - self.sin * y;
                        y = x2 * self.sin + y * self.cos;
                        j += 1;
                    }
                } else {
                    let (mut x, mut y) = (-1.0, -1.0);
                    for _ in 0..4 {
                        self.dest_poly.push(Point::new(
                            round_cint(s.x as f64 + x * delta),
                            round_cint(s.y as f64 + y * delta),
                        ));
                        if x < 0.0 {
                            x = 1.0;
                        } else if y < 0.0 {
                            y = 1.0;
                        } else {
                            x = -1.0;
                        }
                    }
                }
                self.dest_polys.push(self.dest_poly.clone());
                continue;
            }

            // The normals.
            self.normals.clear();
            self.normals.reserve(len);
            for j in 0..len - 1 {
                self.normals
                    .push(get_unit_normal(self.src_poly[j], self.src_poly[j + 1]));
            }
            if end_type == EndType::ClosedLine || end_type == EndType::ClosedPolygon {
                self.normals
                    .push(get_unit_normal(self.src_poly[len - 1], self.src_poly[0]));
            } else {
                self.normals.push(self.normals[len - 2]);
            }

            if end_type == EndType::ClosedPolygon {
                let mut k = len - 1;
                for j in 0..len {
                    self.offset_point(j, &mut k, join_type);
                }
                self.dest_polys.push(self.dest_poly.clone());
            } else if end_type == EndType::ClosedLine {
                let mut k = len - 1;
                for j in 0..len {
                    self.offset_point(j, &mut k, join_type);
                }
                self.dest_polys.push(self.dest_poly.clone());
                self.dest_poly.clear();
                // Rebuild the normals, reversed.
                let n = self.normals[len - 1];
                for j in (1..len).rev() {
                    let p = self.normals[j - 1];
                    self.normals[j] = DoublePoint { x: -p.x, y: -p.y };
                }
                self.normals[0] = DoublePoint { x: -n.x, y: -n.y };
                k = 0;
                for j in (0..len).rev() {
                    self.offset_point(j, &mut k, join_type);
                }
                self.dest_polys.push(self.dest_poly.clone());
            } else {
                let mut k = 0;
                for j in 1..len - 1 {
                    self.offset_point(j, &mut k, join_type);
                }
                if end_type == EndType::OpenButt {
                    let j = len - 1;
                    let n = self.normals[j];
                    let pt1 = self.offset_by(j, n, delta);
                    self.dest_poly.push(pt1);
                    let pt1 = self.offset_by(j, n, -delta);
                    self.dest_poly.push(pt1);
                } else {
                    let j = len - 1;
                    k = len - 2;
                    self.sin_a = 0.0;
                    let n = self.normals[j];
                    self.normals[j] = DoublePoint { x: -n.x, y: -n.y };
                    if end_type == EndType::OpenSquare {
                        self.do_square(j, k);
                    } else {
                        self.do_round(j, k);
                    }
                }

                // Rebuild the normals, reversed.
                for j in (1..len).rev() {
                    let p = self.normals[j - 1];
                    self.normals[j] = DoublePoint { x: -p.x, y: -p.y };
                }
                let n1 = self.normals[1];
                self.normals[0] = DoublePoint { x: -n1.x, y: -n1.y };

                k = len - 1;
                for j in (1..k).rev() {
                    self.offset_point(j, &mut k, join_type);
                }

                if end_type == EndType::OpenButt {
                    let n = self.normals[0];
                    let pt1 = self.offset_by(0, n, -delta);
                    self.dest_poly.push(pt1);
                    let pt1 = self.offset_by(0, n, delta);
                    self.dest_poly.push(pt1);
                } else {
                    self.sin_a = 0.0;
                    if end_type == EndType::OpenSquare {
                        self.do_square(0, 1);
                    } else {
                        self.do_round(0, 1);
                    }
                }
                self.dest_polys.push(self.dest_poly.clone());
            }
        }
    }

    /// `OffsetPoint(j, k, jointype)`.
    #[allow(clippy::manual_clamp)] // The C++ if-chain, whose first test is another.
    fn offset_point(&mut self, j: usize, k: &mut usize, join_type: JoinType) {
        let (nk, nj) = (self.normals[*k], self.normals[j]);
        // Cross product.
        self.sin_a = nk.x * nj.y - nj.x * nk.y;
        if orca_crt::fabs(self.sin_a * self.delta) < 1.0 {
            // Dot product.
            let cos_a = nk.x * nj.x + nj.y * nk.y;
            if cos_a > 0.0 {
                // The angle is near 0 degrees.
                let p = self.offset_by(j, nk, self.delta);
                self.dest_poly.push(p);
                return;
            }
            // Else the angle is near 180 degrees.
        } else if self.sin_a > 1.0 {
            self.sin_a = 1.0;
        } else if self.sin_a < -1.0 {
            self.sin_a = -1.0;
        }

        if self.sin_a * self.delta < 0.0 {
            let p = self.offset_by(j, nk, self.delta);
            self.dest_poly.push(p);
            self.dest_poly.push(self.src_poly[j]);
            let p = self.offset_by(j, nj, self.delta);
            self.dest_poly.push(p);
        } else {
            match join_type {
                JoinType::Miter => {
                    let r = 1.0 + (nj.x * nk.x + nj.y * nk.y);
                    if r >= self.miter_lim {
                        self.do_miter(j, *k, r);
                    } else {
                        self.do_square(j, *k);
                    }
                }
                JoinType::Square => self.do_square(j, *k),
                JoinType::Round => self.do_round(j, *k),
            }
        }
        *k = j;
    }

    /// `DoSquare(j, k)`.
    fn do_square(&mut self, j: usize, k: usize) {
        let (nk, nj, s, d) = (
            self.normals[k],
            self.normals[j],
            self.src_poly[j],
            self.delta,
        );
        let dx = orca_crt::tan(orca_crt::atan2(self.sin_a, nk.x * nj.x + nk.y * nj.y) / 4.0);
        self.dest_poly.push(Point::new(
            round_cint(s.x as f64 + d * (nk.x - nk.y * dx)),
            round_cint(s.y as f64 + d * (nk.y + nk.x * dx)),
        ));
        self.dest_poly.push(Point::new(
            round_cint(s.x as f64 + d * (nj.x + nj.y * dx)),
            round_cint(s.y as f64 + d * (nj.y - nj.x * dx)),
        ));
    }

    /// `DoMiter(j, k, r)`.
    fn do_miter(&mut self, j: usize, k: usize, r: f64) {
        let (nk, nj, s) = (self.normals[k], self.normals[j], self.src_poly[j]);
        let q = self.delta / r;
        self.dest_poly.push(Point::new(
            round_cint(s.x as f64 + (nk.x + nj.x) * q),
            round_cint(s.y as f64 + (nk.y + nj.y) * q),
        ));
    }

    /// `DoRound(j, k)`: `std::max<int>` of the step count, the 64-bit count cut to `int` first.
    fn do_round(&mut self, j: usize, k: usize) {
        let (nk, nj, s, d) = (
            self.normals[k],
            self.normals[j],
            self.src_poly[j],
            self.delta,
        );
        let a = orca_crt::atan2(self.sin_a, nk.x * nj.x + nk.y * nj.y);
        let steps = orca_stl::max(round_cint(self.steps_per_rad * orca_crt::fabs(a)) as i32, 1);
        let (mut x, mut y) = (nk.x, nk.y);
        for _ in 0..steps {
            self.dest_poly.push(Point::new(
                round_cint(s.x as f64 + x * d),
                round_cint(s.y as f64 + y * d),
            ));
            let x2 = x;
            x = x * self.cos - self.sin * y;
            y = x2 * self.sin + y * self.cos;
        }
        self.dest_poly.push(Point::new(
            round_cint(s.x as f64 + nj.x * d),
            round_cint(s.y as f64 + nj.y * d),
        ));
    }
}
