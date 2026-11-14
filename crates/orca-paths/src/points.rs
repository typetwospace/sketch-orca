// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/ShortestPath.hpp lines 1-20 and 29-47 and
// src/libslic3r/ShortestPath.cpp lines 1096-1115 and 1998-2012 of OrcaSlicer v2.4.2, translated
// on 2026-11-13 by paths-1. Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab)
// and the PrusaSlicer authors (Prusa Research), under the GNU Affero General Public License,
// version 3 (AGPL-3.0; LICENSE).

//! The chaining of points and of the items they stand for: [`chain_points`] (each point a segment
//! whose two ends are that point, chained by [`chain_segments_greedy`]), [`chain_expolygons`]
//! (each island by its box's center), [`chain_path_items`] and [`chain_clipper_polynodes`] (items
//! in the order of their points) and [`reorder_by_shortest_traverse`] (contours by their first
//! points).
//!
//! The C++'s `chain_points` takes a non-const `Point *start_near` and `chain_clipper_polynodes`
//! a vector of `PolyNode*`; here the start is an `Option<&Point>` and the nodes are orca-geom's
//! [`NodeId`]s.

use orca_geom::clipper::NodeId;
use orca_geom::expolygon::{self, ExPolygon};
use orca_geom::point::Point;
use orca_geom::polygon::Polygon;
use orca_geom::polyline::{Polyline, ThickPolyline};

use crate::segments::chain_segments_greedy;
use crate::stop::{EMPTY_POLYLINE, stop};

/// `chain_expolygons(input_exploy)`: the islands' order, each by the center of its box
/// (`get_extents`, its contour's box), chained by [`chain_points`] with no start.
#[must_use]
pub fn chain_expolygons(input_exploy: &[ExPolygon]) -> Vec<usize> {
    let mut points = Vec::new();
    for exploy in input_exploy {
        let bbox = expolygon::get_extents(exploy);
        points.push(bbox.center());
    }
    chain_points(&points, None)
}

/// `chain_points(points, start_near)`: the points' order, from the one nearest `start_near` when
/// given.
#[must_use]
pub fn chain_points(points: &[Point], start_near: Option<&Point>) -> Vec<usize> {
    let segment_end_point = |idx: usize, _first_point: bool| -> Point { points[idx] };
    let ordered = chain_segments_greedy(segment_end_point, points.len(), start_near);
    let mut out = Vec::with_capacity(ordered.len());
    for segment_and_reversal in &ordered {
        out.push(segment_and_reversal.0);
    }
    out
}

/// `chain_path_items<T>(points, items)`: the items in the order [`chain_segments_greedy`] gives
/// their points (no start), as copies.
#[must_use]
pub fn chain_path_items<T: Clone>(points: &[Point], items: &[T]) -> Vec<T> {
    let segment_end_point = |idx: usize, _first_point: bool| -> Point { points[idx] };
    let ordered = chain_segments_greedy(segment_end_point, points.len(), None);
    let mut out = Vec::with_capacity(items.len());
    for segment_and_reversal in &ordered {
        out.push(items[segment_and_reversal.0].clone());
    }
    out
}

/// `chain_clipper_polynodes(points, items)`: Clipper's nodes in the order of their points.
#[must_use]
pub fn chain_clipper_polynodes(points: &[Point], items: &[NodeId]) -> Vec<NodeId> {
    chain_path_items(points, items)
}

/// What `reorder_by_shortest_traverse` reads of a contour: `contour.points`.
pub trait ContourPoints {
    /// The contour's points.
    fn contour_points(&self) -> &[Point];
}

impl ContourPoints for Polyline {
    fn contour_points(&self) -> &[Point] {
        &self.points
    }
}

impl ContourPoints for ThickPolyline {
    fn contour_points(&self) -> &[Point] {
        &self.polyline.points
    }
}

impl ContourPoints for Polygon {
    fn contour_points(&self) -> &[Point] {
        &self.points
    }
}

/// `reorder_by_shortest_traverse<T>(polylines_out)`: the contours in the order [`chain_points`]
/// gives their first points (no start), each moved from a copy of the list (a moved-from contour
/// is `T::default()`). A contour with no points stops ([`EMPTY_POLYLINE`]): the C++ reads its
/// `points.front()`.
pub fn reorder_by_shortest_traverse<T: ContourPoints + Clone + Default>(
    polylines_out: &mut Vec<T>,
) {
    let mut start_point = Vec::with_capacity(polylines_out.len());
    for contour in polylines_out.iter() {
        match contour.contour_points().first() {
            Some(&p) => start_point.push(p),
            None => stop("reorder_by_shortest_traverse", EMPTY_POLYLINE),
        }
    }

    let order = chain_points(&start_point, None);

    let mut temp = polylines_out.clone();
    polylines_out.clear();

    for i in order {
        polylines_out.push(core::mem::take(&mut temp[i]));
    }
}
