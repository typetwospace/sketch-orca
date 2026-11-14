// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: enum EMovePathType, struct PathFittingData and class ArcFitter of
// src/libslic3r/ArcFitter.hpp (1-53) and ArcFitter.cpp (1-153) of OrcaSlicer v2.4.2, translated on
// 2026-11-13. Copyright the OrcaSlicer authors and the BambuStudio authors (Bambu Lab), under the
// GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! `ArcFitter`: which runs of a path's points become arcs (G2 and G3) and which stay lines.
//!
//! [`do_arc_fitting`] grows a run point by point while [`ArcSegment::try_create_arc`] still fits
//! it, and saves the last arc that fitted when the next point breaks it; what fits no arc is a
//! linear move, joined to the linear move before it. [`do_arc_fitting_and_simplify`] then keeps
//! each part's Douglas-Peucker points and shifts the indices to match.
//!
//! The indices are `size_t` in the C++ and wrap the same way here: fewer than three points give
//! one linear move ending at `size - 1`, which is `usize::MAX` for no points. A tolerance of at
//! most `SCALED_EPSILON` appends one linear move to the result it is given, without clearing it,
//! as the C++ does.

use crate::circle::{
    ArcDirection, ArcSegment, DEFAULT_ARC_LENGTH_PERCENT_TOLERANCE, default_scaled_max_radius,
};
use crate::multi_point;
use crate::point::{Point, Points, SCALED_EPSILON};
use crate::polyline::Polyline;

/// `EMovePathType`: a linear move (G0 and G1) or an arc move (G2 and G3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum EMovePathType {
    /// `Noop_move`.
    #[default]
    NoopMove = 0,
    /// `Linear_move`.
    LinearMove = 1,
    /// `Arc_move_cw`.
    ArcMoveCw = 2,
    /// `Arc_move_ccw`.
    ArcMoveCcw = 3,
}

/// `PathFittingData`: a run of a path's points, `start_point_index` to `end_point_index`, as one
/// move.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathFittingData {
    /// `start_point_index`.
    pub start_point_index: usize,
    /// `end_point_index`.
    pub end_point_index: usize,
    /// `path_type`.
    pub path_type: EMovePathType,
    /// `arc_data`: the arc, for an arc move.
    pub arc_data: ArcSegment,
}

impl PathFittingData {
    /// `PathFittingData{ start, end, type, arc }`.
    #[must_use]
    pub fn new(
        start_point_index: usize,
        end_point_index: usize,
        path_type: EMovePathType,
        arc_data: ArcSegment,
    ) -> Self {
        Self {
            start_point_index,
            end_point_index,
            path_type,
            arc_data,
        }
    }

    /// `PathFittingData{ start, end, EMovePathType::Linear_move, ArcSegment() }`.
    #[must_use]
    pub fn linear(start_point_index: usize, end_point_index: usize) -> Self {
        Self::new(
            start_point_index,
            end_point_index,
            EMovePathType::LinearMove,
            ArcSegment::default(),
        )
    }

    /// `is_linear_move()`.
    #[must_use]
    pub fn is_linear_move(&self) -> bool {
        self.path_type == EMovePathType::LinearMove
    }

    /// `is_arc_move()`.
    #[must_use]
    pub fn is_arc_move(&self) -> bool {
        self.path_type == EMovePathType::ArcMoveCcw || self.path_type == EMovePathType::ArcMoveCw
    }

    /// `reverse_arc_path()`: the arc reversed and the move's direction with it; false for a
    /// linear move or no arc.
    pub fn reverse_arc_path(&mut self) -> bool {
        if !self.is_arc_move() || !self.arc_data.reverse() {
            return false;
        }
        self.path_type = if self.arc_data.direction == ArcDirection::Ccw {
            EMovePathType::ArcMoveCcw
        } else {
            EMovePathType::ArcMoveCw
        };
        true
    }
}

/// The arc move of an arc's direction.
fn arc_move_of(arc: &ArcSegment) -> EMovePathType {
    if arc.direction == ArcDirection::Ccw {
        EMovePathType::ArcMoveCcw
    } else {
        EMovePathType::ArcMoveCw
    }
}

/// `ArcFitter::do_arc_fitting(points, result, tolerance)`: `result` cleared, then the runs of
/// `points` as arc and linear moves.
pub fn do_arc_fitting(points: &[Point], result: &mut Vec<PathFittingData>, tolerance: f64) {
    result.clear();
    if points.len() < 3 {
        result.push(PathFittingData::linear(0, points.len().wrapping_sub(1)));
        return;
    }

    let mut front_index: usize = 0;
    let mut back_index: usize = 0;
    let mut last_arc = ArcSegment::default();
    let mut current_segment = Points::with_capacity(points.len());
    for (i, &point) in points.iter().enumerate() {
        // Too few points on the stack: build it first.
        back_index = i;
        current_segment.push(point);
        if back_index.wrapping_sub(front_index) < 2 {
            continue;
        }

        let fitted = ArcSegment::try_create_arc(
            &current_segment,
            Polyline::new(current_segment.clone()).length(),
            default_scaled_max_radius(),
            tolerance,
            DEFAULT_ARC_LENGTH_PERCENT_TOLERANCE,
        );
        if let Some(target_arc) = fitted {
            // It fits an arc: keep the arc for now.
            last_arc = target_arc;
            if back_index == points.len() - 1 {
                result.push(PathFittingData::new(
                    front_index,
                    back_index,
                    arc_move_of(&last_arc),
                    last_arc,
                ));
                front_index = back_index;
            }
        } else {
            if back_index.wrapping_sub(front_index) > 2 {
                // The stack without its top fitted: save the last arc.
                result.push(PathFittingData::new(
                    front_index,
                    back_index - 1,
                    arc_move_of(&last_arc),
                    last_arc,
                ));
            } else if result.is_empty()
                || result[result.len() - 1].path_type != EMovePathType::LinearMove
            {
                // Three points that fit no arc: the first segment as a linear move.
                result.push(PathFittingData::linear(front_index, front_index + 1));
            } else {
                let n = result.len();
                result[n - 1].end_point_index = front_index + 1;
            }
            front_index = back_index - 1;
            current_segment.clear();
            current_segment.push(points[front_index]);
            current_segment.push(points[front_index + 1]);
        }
    }
    // The rest.
    if front_index != back_index {
        if result.is_empty() || result[result.len() - 1].path_type != EMovePathType::LinearMove {
            result.push(PathFittingData::linear(front_index, back_index));
        } else {
            let n = result.len();
            result[n - 1].end_point_index = back_index;
        }
    }
}

/// `ArcFitter::do_arc_fitting_and_simplify(points, result, tolerance)`: the fitting (or, for a
/// tolerance of at most `SCALED_EPSILON`, one linear move appended to `result`), then
/// Douglas-Peucker on each part, the arcs keeping their points for the wipe, `points` replaced by
/// what is kept and the indices shifted to match.
///
/// # Panics
/// When more than one move is left and `points` is empty or an index is past it, which the C++
/// reads outside the list for.
pub fn do_arc_fitting_and_simplify(
    points: &mut Points,
    result: &mut Vec<PathFittingData>,
    tolerance: f64,
) {
    // The arc fitting first; `abs(double)` is MSVC's `double` overload.
    if orca_crt::fabs(tolerance) > SCALED_EPSILON {
        do_arc_fitting(points, result, tolerance);
    } else {
        result.push(PathFittingData::linear(0, points.len().wrapping_sub(1)));
    }

    // The straight parts get Douglas-Peucker; an arc needs only its ends.
    if result.len() == 1 && result[0].path_type == EMovePathType::LinearMove {
        // All straight.
        *points = multi_point::douglas_peucker(points, tolerance);
        result[0].end_point_index = points.len().wrapping_sub(1);
        return;
    }
    // Arcs and straight parts: each simplified on its own.
    let mut simplified_points = Points::with_capacity(points.len());
    simplified_points.push(points[0]);
    let mut reduce_count = vec![0_usize; result.len()];
    for (i, data) in result.iter().enumerate() {
        let start_index = data.start_point_index;
        let end_index = data.end_point_index;
        // Douglas-Peucker on an arc keeps more than its ends, which the wipe needs.
        let mut straight_or_arc_part = Points::new();
        let mut j = start_index;
        while j <= end_index {
            straight_or_arc_part.push(points[j]);
            j += 1;
        }
        straight_or_arc_part = multi_point::douglas_peucker(&straight_or_arc_part, tolerance);
        // How many points were dropped.
        reduce_count[i] = end_index
            .wrapping_sub(start_index)
            .wrapping_add(1)
            .wrapping_sub(straight_or_arc_part.len());
        // The kept points.
        simplified_points.extend_from_slice(straight_or_arc_part.get(1..).unwrap_or(&[]));
    }
    *points = simplified_points;
    // The indices of the kept points.
    for j in 1..reduce_count.len() {
        reduce_count[j] = reduce_count[j].wrapping_add(reduce_count[j - 1]);
    }
    let n = result.len();
    for j in 0..n {
        result[j].end_point_index = result[j].end_point_index.wrapping_sub(reduce_count[j]);
        if j != n - 1 {
            result[j + 1].start_point_index = result[j].end_point_index;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_and_straight_paths_are_one_linear_move() {
        let mut r = Vec::new();
        do_arc_fitting(&[], &mut r, 1.0);
        assert_eq!(r, vec![PathFittingData::linear(0, usize::MAX)]);
        let pts: Points = (0..5).map(|i| Point::new(i * 1000, 0)).collect();
        do_arc_fitting(&pts, &mut r, 50_000.0);
        assert_eq!(r, vec![PathFittingData::linear(0, 4)]);
        let mut q = pts.clone();
        let mut r2 = Vec::new();
        do_arc_fitting_and_simplify(&mut q, &mut r2, 50_000.0);
        assert_eq!(q, vec![Point::new(0, 0), Point::new(4000, 0)]);
        assert_eq!(r2, vec![PathFittingData::linear(0, 1)]);
    }
}
