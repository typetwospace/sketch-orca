// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/BoundingBox.hpp and BoundingBox.cpp, and the get_extents
// of Point.cpp and Line.cpp, of OrcaSlicer v2.4.2, translated on 2026-11-07. Copyright the
// OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and the PrusaSlicer authors (Prusa
// Research), under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! `BoundingBox` (of `Point`s), `BoundingBoxf` (of `Vec2d`s) and `BoundingBoxf3` (of `Vec3d`s).
//!
//! A box is `defined` once a point was merged, or when built from two corners or from points with
//! a positive area. The `double` boxes take `cwiseMin` and `cwiseMax` as Eigen evaluates them
//! (`orca-eigen`), the `Point` box integer minima and maxima. `BoundingBox::offset` builds its
//! offset as `Point(delta, delta)`, which rounds, and `scale` multiplies with `Point::operator*=`,
//! which truncates.

use orca_eigen::{Matrix, Vec2d, Vec3d, Vector};

use crate::line::Line;
use crate::point::{Coord, Coordf, Point, align_to_grid};
use crate::polygon::Polygon;

/// `BoundingBox`: `BoundingBoxBase<Point, Points>`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoundingBox {
    /// `min`.
    pub min: Point,
    /// `max`.
    pub max: Point,
    /// `defined`.
    pub defined: bool,
}

/// `BoundingBoxes`.
pub type BoundingBoxes = Vec<BoundingBox>;

impl BoundingBox {
    /// `BoundingBox()`: undefined, both corners at the origin.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `BoundingBox(pmin, pmax)`: defined when `pmin` is below and left of `pmax`.
    #[must_use]
    pub fn from_corners(pmin: Point, pmax: Point) -> Self {
        Self {
            min: pmin,
            max: pmax,
            defined: pmin.x < pmax.x && pmin.y < pmax.y,
        }
    }

    /// `BoundingBox(points)`: defined when the points span a positive area.
    #[must_use]
    pub fn from_points(points: &[Point]) -> Self {
        let mut out = Self::new();
        Self::construct(&mut out, points, false);
        out
    }

    /// `construct<IncludeBoundary>(out, from, to)`: with `include_boundary` a single point
    /// defines the box. `cwiseMin` and `cwiseMax` go through SSE2's emulated 64-bit compare
    /// ([`Point::cwise_min`]), so past 2^31 the box can miss a point, as the engine's does.
    fn construct(out: &mut Self, points: &[Point], include_boundary: bool) {
        if let Some((&first, rest)) = points.split_first() {
            out.min = first;
            out.max = first;
            for &p in rest {
                out.min = out.min.cwise_min(p);
                out.max = out.max.cwise_max(p);
            }
            out.defined = include_boundary || (out.min.x < out.max.x && out.min.y < out.max.y);
        }
    }

    /// `reset()`.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// `merge(point)`: by `cwiseMin` and `cwiseMax` ([`Point::cwise_min`]).
    pub fn merge(&mut self, point: Point) {
        if self.defined {
            self.min = self.min.cwise_min(point);
            self.max = self.max.cwise_max(point);
        } else {
            self.min = point;
            self.max = point;
            self.defined = true;
        }
    }

    /// `merge(points)`: `merge(BoundingBox(points))`.
    pub fn merge_points(&mut self, points: &[Point]) {
        self.merge_box(&Self::from_points(points));
    }

    /// `merge(bb)`: nothing when `bb` is undefined; by `cwiseMin` and `cwiseMax`.
    pub fn merge_box(&mut self, bb: &BoundingBox) {
        if bb.defined {
            if self.defined {
                self.min = self.min.cwise_min(bb.min);
                self.max = self.max.cwise_max(bb.max);
            } else {
                self.min = bb.min;
                self.max = bb.max;
                self.defined = true;
            }
        }
    }

    /// `scale(factor)`: both corners `*= factor`, truncated.
    pub fn scale(&mut self, factor: f64) {
        self.min.scale_assign(factor);
        self.max.scale_assign(factor);
    }

    /// `scaled(factor)`.
    #[must_use]
    pub fn scaled(&self, factor: f64) -> Self {
        let mut out = *self;
        out.scale(factor);
        out
    }

    /// `size()`: `max - min`.
    #[must_use]
    pub fn size(&self) -> Point {
        self.max - self.min
    }

    /// `radius()`: half the diagonal.
    #[must_use]
    pub fn radius(&self) -> f64 {
        0.5 * (self.max - self.min).cast_f64().norm()
    }

    /// `area()`: `double(max.x - min.x) * (max.y - min.y)`.
    #[must_use]
    pub fn area(&self) -> f64 {
        self.max.x.wrapping_sub(self.min.x) as f64 * self.max.y.wrapping_sub(self.min.y) as f64
    }

    /// `translate(x, y)`: by `Point(x, y)`, rounded.
    pub fn translate_f64(&mut self, x: Coordf, y: Coordf) {
        self.translate(Point::from_f64(x, y));
    }

    /// `translate(v)`.
    pub fn translate(&mut self, v: Point) {
        self.min += v;
        self.max += v;
    }

    /// `offset(delta)`: out by `Point(delta, delta)`, rounded.
    pub fn offset(&mut self, delta: Coordf) {
        let v = Point::from_f64(delta, delta);
        self.min -= v;
        self.max += v;
    }

    /// `inflated(delta)`.
    #[must_use]
    pub fn inflated(&self, delta: Coordf) -> Self {
        let mut out = *self;
        out.offset(delta);
        out
    }

    /// `center()`: `(min + max) / 2` in integers.
    #[must_use]
    pub fn center(&self) -> Point {
        (self.min + self.max) / 2
    }

    /// `contains(point)`, the boundary included.
    #[must_use]
    pub fn contains(&self, point: Point) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
    }

    /// `contains(other)`.
    #[must_use]
    pub fn contains_box(&self, other: &BoundingBox) -> bool {
        self.contains(other.min) && self.contains(other.max)
    }

    /// `overlap(other)`, the boundary included.
    #[must_use]
    pub fn overlap(&self, other: &BoundingBox) -> bool {
        !(self.max.x < other.min.x
            || self.min.x > other.max.x
            || self.max.y < other.min.y
            || self.min.y > other.max.y)
    }

    /// `operator[](idx)`: the corners counter-clockwise from `min`, the origin past 3.
    #[must_use]
    pub fn corner(&self, idx: usize) -> Point {
        match idx {
            0 => self.min,
            1 => Point::new(self.max.x, self.min.y),
            2 => self.max,
            3 => Point::new(self.min.x, self.max.y),
            _ => Point::default(),
        }
    }

    /// `polygon()`: the four corners counter-clockwise from `min`.
    #[must_use]
    pub fn polygon(&self) -> Polygon {
        Polygon::new(vec![
            self.min,
            Point::new(self.max.x, self.min.y),
            self.max,
            Point::new(self.min.x, self.max.y),
        ])
    }

    /// `rotated(angle)`: the box of the four corners rotated about the origin.
    #[must_use]
    pub fn rotated(&self, angle: f64) -> Self {
        let mut out = Self::new();
        out.merge(self.min.rotated(angle));
        out.merge(self.max.rotated(angle));
        out.merge(Point::new(self.min.x, self.max.y).rotated(angle));
        out.merge(Point::new(self.max.x, self.min.y).rotated(angle));
        out
    }

    /// `rotated(angle, center)`.
    #[must_use]
    pub fn rotated_around(&self, angle: f64, center: Point) -> Self {
        let mut out = Self::new();
        out.merge(self.min.rotated_around(angle, center));
        out.merge(self.max.rotated_around(angle, center));
        out.merge(Point::new(self.min.x, self.max.y).rotated_around(angle, center));
        out.merge(Point::new(self.max.x, self.min.y).rotated_around(angle, center));
        out
    }

    /// `align_to_grid(cell_size)`: the min corner down to the grid.
    pub fn align_to_grid(&mut self, cell_size: Coord) {
        if self.defined {
            self.min.x = align_to_grid(self.min.x, cell_size);
            self.min.y = align_to_grid(self.min.y, cell_size);
        }
    }

    /// `empty(bb)`: undefined or without area.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.defined || self.min.x >= self.max.x || self.min.y >= self.max.y
    }
}

/// `get_extents<IncludeBoundary>(points)`.
#[must_use]
pub fn get_extents(points: &[Point], include_boundary: bool) -> BoundingBox {
    let mut out = BoundingBox::new();
    BoundingBox::construct(&mut out, points, include_boundary);
    out
}

/// `get_extents<IncludeBoundary>(VecOfPoints)`.
#[must_use]
pub fn get_extents_vec(pts: &[Vec<Point>], include_boundary: bool) -> BoundingBox {
    let mut bbox = BoundingBox::new();
    for p in pts {
        bbox.merge_box(&get_extents(p, include_boundary));
    }
    bbox
}

/// `get_extents(lines)`: every end merged.
#[must_use]
pub fn get_extents_lines(lines: &[Line]) -> BoundingBox {
    let mut bbox = BoundingBox::new();
    for line in lines {
        bbox.merge(line.a);
        bbox.merge(line.b);
    }
    bbox
}

/// `BoundingBoxf`: `BoundingBoxBase<Vec2d>`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoundingBoxf {
    /// `min`.
    pub min: Vec2d,
    /// `max`.
    pub max: Vec2d,
    /// `defined`.
    pub defined: bool,
}

impl Default for BoundingBoxf {
    fn default() -> Self {
        Self {
            min: Vec2d::zero(),
            max: Vec2d::zero(),
            defined: false,
        }
    }
}

impl BoundingBoxf {
    /// `BoundingBoxf(pmin, pmax)`.
    #[must_use]
    pub fn from_corners(pmin: Vec2d, pmax: Vec2d) -> Self {
        Self {
            min: pmin,
            max: pmax,
            defined: pmin.x() < pmax.x() && pmin.y() < pmax.y(),
        }
    }

    /// `BoundingBoxf(points)`.
    #[must_use]
    pub fn from_points(points: &[Vec2d]) -> Self {
        let mut out = Self::default();
        if let Some((&first, rest)) = points.split_first() {
            out.min = first;
            out.max = first;
            for p in rest {
                out.min = out.min.cwise_min(p);
                out.max = out.max.cwise_max(p);
            }
            out.defined = out.min.x() < out.max.x() && out.min.y() < out.max.y();
        }
        out
    }

    /// `merge(point)`.
    pub fn merge(&mut self, point: Vec2d) {
        if self.defined {
            self.min = self.min.cwise_min(&point);
            self.max = self.max.cwise_max(&point);
        } else {
            self.min = point;
            self.max = point;
            self.defined = true;
        }
    }

    /// `merge(bb)`.
    pub fn merge_box(&mut self, bb: &BoundingBoxf) {
        if bb.defined {
            if self.defined {
                self.min = self.min.cwise_min(&bb.min);
                self.max = self.max.cwise_max(&bb.max);
            } else {
                self.min = bb.min;
                self.max = bb.max;
                self.defined = true;
            }
        }
    }

    /// `size()`.
    #[must_use]
    pub fn size(&self) -> Vec2d {
        self.max - self.min
    }

    /// `radius()`.
    #[must_use]
    pub fn radius(&self) -> f64 {
        0.5 * (self.max - self.min).norm()
    }

    /// `center()`: `(min + max) / 2`.
    #[must_use]
    pub fn center(&self) -> Vec2d {
        (self.min + self.max) / 2.0
    }

    /// `offset(delta)`.
    pub fn offset(&mut self, delta: Coordf) {
        let v = Vector([delta, delta]);
        self.min -= v;
        self.max += v;
    }

    /// `scale(factor)`.
    pub fn scale(&mut self, factor: f64) {
        self.min *= factor;
        self.max *= factor;
    }

    /// `contains(point)`.
    #[must_use]
    pub fn contains(&self, point: Vec2d) -> bool {
        point.x() >= self.min.x()
            && point.x() <= self.max.x()
            && point.y() >= self.min.y()
            && point.y() <= self.max.y()
    }
}

/// `get_extents(const std::vector<Vec2d>&)`: every point merged.
#[must_use]
pub fn get_extents_f(pts: &[Vec2d]) -> BoundingBoxf {
    let mut bbox = BoundingBoxf::default();
    for &p in pts {
        bbox.merge(p);
    }
    bbox
}

/// `BoundingBoxf3`: `BoundingBox3Base<Vec3d>`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoundingBoxf3 {
    /// `min`.
    pub min: Vec3d,
    /// `max`.
    pub max: Vec3d,
    /// `defined`.
    pub defined: bool,
}

impl Default for BoundingBoxf3 {
    fn default() -> Self {
        Self {
            min: Vec3d::zero(),
            max: Vec3d::zero(),
            defined: false,
        }
    }
}

impl BoundingBoxf3 {
    /// `BoundingBoxf3(pmin, pmax)`: defined when `pmin` is below `pmax` on every axis.
    #[must_use]
    pub fn from_corners(pmin: Vec3d, pmax: Vec3d) -> Self {
        let mut defined = pmin.x() < pmax.x() && pmin.y() < pmax.y();
        if pmin.z() >= pmax.z() {
            defined = false;
        }
        Self {
            min: pmin,
            max: pmax,
            defined,
        }
    }

    /// `BoundingBoxf3(points)`.
    ///
    /// # Panics
    /// With no points, as the C++ throws `InvalidArgument("Empty point set supplied to
    /// BoundingBox3Base constructor")`.
    #[must_use]
    pub fn from_points(points: &[Vec3d]) -> Self {
        let Some((&first, rest)) = points.split_first() else {
            panic!("Empty point set supplied to BoundingBox3Base constructor");
        };
        let mut out = Self {
            min: first,
            max: first,
            defined: false,
        };
        for p in rest {
            out.min = out.min.cwise_min(p);
            out.max = out.max.cwise_max(p);
        }
        out.defined =
            out.min.x() < out.max.x() && out.min.y() < out.max.y() && out.min.z() < out.max.z();
        out
    }

    /// `merge(point)`.
    pub fn merge(&mut self, point: Vec3d) {
        if self.defined {
            self.min = self.min.cwise_min(&point);
            self.max = self.max.cwise_max(&point);
        } else {
            self.min = point;
            self.max = point;
            self.defined = true;
        }
    }

    /// `merge(bb)`.
    pub fn merge_box(&mut self, bb: &BoundingBoxf3) {
        if bb.defined {
            if self.defined {
                self.min = self.min.cwise_min(&bb.min);
                self.max = self.max.cwise_max(&bb.max);
            } else {
                self.min = bb.min;
                self.max = bb.max;
                self.defined = true;
            }
        }
    }

    /// `size()`.
    #[must_use]
    pub fn size(&self) -> Vec3d {
        self.max - self.min
    }

    /// `radius()`.
    #[must_use]
    pub fn radius(&self) -> f64 {
        0.5 * (self.max - self.min).norm()
    }

    /// `center()`: `(min + max) / 2`.
    #[must_use]
    pub fn center(&self) -> Vec3d {
        (self.min + self.max) / 2.0
    }

    /// `max_size()`: `std::max(s.x, std::max(s.y, s.z))`.
    #[must_use]
    pub fn max_size(&self) -> f64 {
        let s = self.size();
        orca_stl::max(s.x(), orca_stl::max(s.y(), s.z()))
    }

    /// `offset(delta)`.
    pub fn offset(&mut self, delta: Coordf) {
        let v = Vector([delta, delta, delta]);
        self.min -= v;
        self.max += v;
    }

    /// `translate(v)`.
    pub fn translate(&mut self, v: Vec3d) {
        self.min += v;
        self.max += v;
    }

    /// `contains(point)`.
    #[must_use]
    pub fn contains(&self, point: Vec3d) -> bool {
        point.x() >= self.min.x()
            && point.x() <= self.max.x()
            && point.y() >= self.min.y()
            && point.y() <= self.max.y()
            && point.z() >= self.min.z()
            && point.z() <= self.max.z()
    }

    /// `intersects(other)`, without the boundary.
    #[must_use]
    pub fn intersects(&self, other: &BoundingBoxf3) -> bool {
        self.min.x() < other.max.x()
            && self.max.x() > other.min.x()
            && self.min.y() < other.max.y()
            && self.max.y() > other.min.y()
            && self.min.z() < other.max.z()
            && self.max.z() > other.min.z()
    }

    /// `empty(bb)`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.defined
            || self.min.x() >= self.max.x()
            || self.min.y() >= self.max.y()
            || self.min.z() >= self.max.z()
    }

    /// `polygon(is_scaled)`: the 2D footprint, each corner `* (1 / (is_scaled ? SCALING_FACTOR :
    /// 1))` assigned to a `coord_t` (truncated).
    #[must_use]
    pub fn polygon(&self, is_scaled: bool) -> Polygon {
        let scale_factor = 1.0
            / if is_scaled {
                crate::point::SCALING_FACTOR
            } else {
                1.0
            };
        let c = |v: f64| orca_crt::f64_to_i64(v * scale_factor);
        Polygon::new(vec![
            Point::new(c(self.min.x()), c(self.min.y())),
            Point::new(c(self.max.x()), c(self.min.y())),
            Point::new(c(self.max.x()), c(self.max.y())),
            Point::new(c(self.min.x()), c(self.max.y())),
        ])
    }

    /// `transformed(matrix)`: the box of the eight corners transformed, the corners as
    /// `matrix * corners.colwise().homogeneous()` evaluates them (`linear() * corners`, then the
    /// translation added to each column).
    #[must_use]
    pub fn transformed(&self, matrix: &orca_eigen::Transform3d) -> Self {
        let (mn, mx) = (self.min, self.max);
        let corners = [
            [mn.x(), mn.y(), mn.z()],
            [mx.x(), mn.y(), mn.z()],
            [mx.x(), mx.y(), mn.z()],
            [mn.x(), mx.y(), mn.z()],
            [mn.x(), mn.y(), mx.z()],
            [mx.x(), mn.y(), mx.z()],
            [mx.x(), mx.y(), mx.z()],
            [mn.x(), mx.y(), mx.z()],
        ];
        let mut rows = [[0.0_f64; 8]; 3];
        for (j, c) in corners.iter().enumerate() {
            for i in 0..3 {
                rows[i][j] = c[i];
            }
        }
        let src = Matrix::<f64, 3, 8>::from_rows(rows);
        let mut dst = matrix.linear().mul(&src);
        let t = matrix.translation();
        for j in 0..8 {
            for i in 0..3 {
                let v = dst.at(i, j) + t[i];
                dst.set(i, j, v);
            }
        }
        let mut v_min = Vector([dst.at(0, 0), dst.at(1, 0), dst.at(2, 0)]);
        let mut v_max = v_min;
        for i in 1..8 {
            for j in 0..3 {
                v_min[j] = orca_stl::min(v_min[j], dst.at(j, i));
                v_max[j] = orca_stl::max(v_max[j], dst.at(j, i));
            }
        }
        Self::from_corners(v_min, v_max)
    }
}
