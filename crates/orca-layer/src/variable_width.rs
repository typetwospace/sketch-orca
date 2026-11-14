// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/VariableWidth.hpp (1-13) and VariableWidth.cpp (1-236) of
// OrcaSlicer v2.4.2, translated on 2026-11-13 (orca-o3-layer-1). Copyright the OrcaSlicer authors,
// the BambuStudio authors (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU
// Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! Variable width: extrusions along a thick polyline (a medial axis with a width at each end of
//! each segment), the thin walls and gap fill of the perimeter generator.
//!
//! G-code cannot change the width within one move, so a segment whose widths differ by more than
//! the tolerance is cut into `ceil(difference / tolerance)` segments first, each width
//! interpolated. [`thick_polyline_to_multi_path`] then joins consecutive segments into one path
//! while the width stays within `merge_tolerance` of the path's, and joins a segment shorter than
//! `SCALED_EPSILON` to its neighbour. [`variable_width`] cuts a polyline into paths where the width
//! leaves the band of `scale_(0.05)` around the path's widths so far, each path's width the
//! length-weighted mean of its segments' (the last path weighs each segment's first width only, as
//! the C++ does); a polyline whose paths close on themselves becomes a loop.
//!
//! The width of a path's flow is the segment's width as a spacing turned into an extrusion width:
//! `unscale<float>(w) + height * float(1 - π/4)`, in `float`. A flow too narrow throws in the C++
//! (`Flow::with_width`, `mm3_per_mm`); these return its [`FlowError`].

use orca_geom::point::{PI, Point, SCALED_EPSILON, SCALING_FACTOR, scale_};
use orca_geom::point3::Point3;
use orca_geom::polyline::{ThickLine, ThickLines, ThickPolyline};

use crate::cpp_vec;
use crate::entity::{ExtrusionEntitiesPtr, ExtrusionEntity};
use crate::extrusion_loop::ExtrusionLoop;
use crate::flow::{Flow, FlowError};
use crate::multi_path::ExtrusionMultiPath;
use crate::path::{ExtrusionPath, ExtrusionPaths};
use crate::role::{ExtrusionLoopRole, ExtrusionRole};

/// `ThickLine::length()` (`Line::length()`): `(b - a).cast<double>().norm()`.
fn line_length(line: &ThickLine) -> f64 {
    (line.b - line.a).cast_f64().norm()
}

/// `unscale<float>(w)`: `float(w) * float(SCALING_FACTOR)`.
#[allow(clippy::cast_possible_truncation)]
fn unscale_f32(w: f64) -> f32 {
    (w as f32) * (SCALING_FACTOR as f32)
}

/// The extrusion width of a spacing `w` (scaled): `unscale<float>(w) + height * float(1 - π/4)`.
#[allow(clippy::cast_possible_truncation)]
fn width_from_spacing(flow: &Flow, w: f64) -> f32 {
    unscale_f32(w) + flow.height() * (1. - 0.25 * PI) as f32
}

/// The segment `line` (at `i`) cut into `ceil(thickness_delta / tolerance)` segments of equal
/// length, the widths interpolated along it.
#[allow(clippy::cast_precision_loss)]
fn split_line(lines: &mut ThickLines, i: usize, thickness_delta: f64, tolerance: f32) {
    let line = lines[i];
    let line_len = line_length(&line);
    let segments = orca_crt::f64_to_u32(orca_crt::ceil(thickness_delta / f64::from(tolerance)));
    let seg_len = line_len / f64::from(segments);
    let mut pp: Vec<Point> = Vec::new();
    let mut width: Vec<f64> = Vec::new();
    {
        pp.push(line.a);
        width.push(line.a_width);
        for j in 1..segments as usize {
            pp.push(Point::cast_from_vec2d(
                line.a.cast_f64()
                    + (line.b - line.a).cast_f64().normalized() * (j as f64 * seg_len),
            ));

            let w = line.a_width + (j as f64 * seg_len) * (line.b_width - line.a_width) / line_len;
            width.push(w);
            width.push(w);
        }
        pp.push(line.b);
        width.push(line.b_width);
    }

    // Delete this line and insert the new ones.
    lines.remove(i);
    for j in 0..segments as usize {
        let new_line = ThickLine::new(pp[j], pp[j + 1], width[2 * j], width[2 * j + 1]);
        lines.insert(i + j, new_line);
    }
}

/// `thick_polyline_to_multi_path(thick_polyline, role, flow, tolerance, merge_tolerance)`: one
/// multi path along the thick polyline, a new path wherever the flow's width leaves the current
/// path's by more than `merge_tolerance` (scaled).
///
/// # Errors
/// The [`FlowError`] of a flow too narrow (`with_width`, `mm3_per_mm`).
pub fn thick_polyline_to_multi_path(
    thick_polyline: &ThickPolyline,
    role: ExtrusionRole,
    flow: &Flow,
    tolerance: f32,
    merge_tolerance: f32,
) -> Result<ExtrusionMultiPath, FlowError> {
    let mut multi_path = ExtrusionMultiPath::default();
    let mut path = ExtrusionPath::with_role(role);
    let mut lines = thick_polyline.thicklines();

    // `int i`: where the C++ writes `-- i; continue;` (the same segment again), this loop
    // continues without the step.
    let mut i: usize = 0;
    while i < lines.len() {
        let iu = i;
        let line = lines[iu];

        let line_len = line_length(&line);
        if line_len < SCALED_EPSILON {
            // So tiny that its width does not matter when it joins another line.
            if !path.is_empty() {
                // The variable path is not empty: this tiny line joins it.
                if let Some(back) = path.polyline.points.last_mut() {
                    *back = Point3::from_point(line.b, 0);
                }
            } else if iu + 1 < lines.len() {
                // A line follows: this tiny line joins it.
                lines[iu + 1].a = line.a;
            } else if let Some(last) = multi_path.paths.last_mut() {
                // This tiny line joins the last finished path.
                if let Some(back) = last.polyline.points.last_mut() {
                    *back = Point3::from_point(line.b, 0);
                }
            }
            // Otherwise the tiny line is dropped.
            i += 1;
            continue;
        }

        let thickness_delta = orca_crt::fabs(line.a_width - line.b_width);
        if thickness_delta > f64::from(tolerance) {
            split_line(&mut lines, iu, thickness_delta, tolerance);
            // `-- i; continue;`: the first new segment next.
            continue;
        }

        let w = orca_crt::fmax(line.a_width, line.b_width);
        let new_flow = if role == ExtrusionRole::OverhangPerimeter && flow.bridge() {
            *flow
        } else {
            flow.with_width(width_from_spacing(flow, w))?
        };
        if path.polyline.points.is_empty() {
            path.polyline.append(Point3::from_point(line.a, 0));
            path.polyline.append(Point3::from_point(line.b, 0));
            // The spacing made an extrusion width by the model of a rectangle with half circles.
            path.mm3_per_mm = new_flow.mm3_per_mm()?;
            path.width = new_flow.width();
            path.height = new_flow.height();
        } else {
            // `scaled<double>(fabs(float))`: the division in `float`.
            let thickness_delta =
                f64::from(orca_crt::fabsf(path.width - new_flow.width()) / (SCALING_FACTOR as f32));
            if thickness_delta <= f64::from(merge_tolerance) {
                // Within the tolerance of the current path's width.
                path.polyline.append(Point3::from_point(line.b, 0));
            } else {
                // A new path from this line.
                cpp_vec::push(
                    &mut multi_path.paths,
                    core::mem::replace(&mut path, ExtrusionPath::with_role(role)),
                );
                // `-- i`: this line again.
                continue;
            }
        }
        i += 1;
    }
    if path.polyline.is_valid() {
        cpp_vec::push(&mut multi_path.paths, path);
    }
    Ok(multi_path)
}

/// A path along `lines[start_index]` to `lines[end]` (each line's first point, then `last`), its
/// width the mean `sum / length`; `None` when `length` is not above `SCALED_EPSILON`.
fn mean_width_path(
    role: ExtrusionRole,
    flow: &Flow,
    points: impl Iterator<Item = Point>,
    length: f64,
    sum: f64,
) -> Result<Option<ExtrusionPath>, FlowError> {
    let mut path = ExtrusionPath::with_role(role);
    for point in points {
        path.polyline.append(Point3::from_point(point, 0));
    }
    if length > SCALED_EPSILON {
        let w = sum / length;
        let new_flow = flow.with_width(width_from_spacing(flow, w))?;
        path.mm3_per_mm = new_flow.mm3_per_mm()?;
        path.width = new_flow.width();
        path.height = new_flow.height();
        Ok(Some(path))
    } else {
        Ok(None)
    }
}

/// `thick_polyline_to_extrusion_paths_2(thick_polyline, role, flow, tolerance)`: paths along the
/// thick polyline, a new one wherever a segment's end width leaves the band of the current
/// path's widths by more than `tolerance` (a segment too uneven in itself cut first).
fn thick_polyline_to_extrusion_paths_2(
    thick_polyline: &ThickPolyline,
    role: ExtrusionRole,
    flow: &Flow,
    tolerance: f32,
) -> Result<ExtrusionPaths, FlowError> {
    let mut paths = ExtrusionPaths::new();
    let mut lines = thick_polyline.thicklines();

    let mut start_index: usize = 0;
    let mut max_width = 0.;
    let mut min_width = 0.;

    let mut i: usize = 0;
    while i < lines.len() {
        let line = lines[i];

        if i == 0 {
            max_width = line.a_width;
            min_width = line.a_width;
        }

        let line_len = line_length(&line);
        if line_len < SCALED_EPSILON {
            i += 1;
            continue;
        }

        let thickness_delta = orca_stl::minmax::max(
            orca_crt::fabs(max_width - line.b_width),
            orca_crt::fabs(min_width - line.b_width),
        );
        if thickness_delta > f64::from(tolerance) {
            // A large difference in width: first the path from start_index to i (not included).
            if start_index != i {
                let mut length = line_length(&lines[start_index]);
                let mut sum = line_length(&lines[start_index])
                    * 0.5
                    * (lines[start_index].a_width + lines[start_index].b_width);
                for l in &lines[start_index + 1..i] {
                    length += line_length(l);
                    sum += line_length(l) * 0.5 * (l.a_width + l.b_width);
                }
                let points = lines[start_index..i]
                    .iter()
                    .map(|l| l.a)
                    .chain(core::iter::once(lines[i].a));
                if let Some(path) = mean_width_path(role, flow, points, length, sum)? {
                    cpp_vec::push(&mut paths, path);
                }
            }

            start_index = i;
            max_width = line.a_width;
            min_width = line.a_width;

            // Then the i-th segment.
            let thickness_delta = orca_crt::fabs(line.a_width - line.b_width);
            if thickness_delta > f64::from(tolerance) {
                split_line(&mut lines, i, thickness_delta, tolerance);
                // `--i; continue;`: the first new segment next.
                continue;
            }
        } else {
            // Just the band of widths updated.
            max_width =
                orca_stl::minmax::max(max_width, orca_stl::minmax::max(line.a_width, line.b_width));
            min_width =
                orca_stl::minmax::min(min_width, orca_stl::minmax::min(line.a_width, line.b_width));
        }
        i += 1;
    }
    // The remaining segments.
    let final_size = lines.len();
    if start_index < final_size {
        let mut length = line_length(&lines[start_index]);
        let mut sum = line_length(&lines[start_index]) * lines[start_index].a_width;
        for l in &lines[start_index + 1..final_size] {
            length += line_length(l);
            sum += line_length(l) * l.a_width;
        }
        let points = lines[start_index..final_size]
            .iter()
            .map(|l| l.a)
            .chain(core::iter::once(lines[final_size - 1].b));
        if let Some(path) = mean_width_path(role, flow, points, length, sum)? {
            cpp_vec::push(&mut paths, path);
        }
    }

    Ok(paths)
}

/// `variable_width(polylines, role, flow, out)`: each thick polyline's paths appended to `out`, as
/// one loop when the first path starts where the last ends, else path by path. The tolerance is
/// `float(scale_(0.05))`.
///
/// # Errors
/// The first [`FlowError`]; `out` keeps the polylines before it.
pub fn variable_width(
    polylines: &[ThickPolyline],
    role: ExtrusionRole,
    flow: &Flow,
    out: &mut ExtrusionEntitiesPtr,
) -> Result<(), FlowError> {
    // The granularity of the adaptive width (G-code has no variable extrusion within one move):
    // it only sets the number of segments; any pruning comes before.
    #[allow(clippy::cast_possible_truncation)]
    let tolerance = scale_(0.05) as f32;
    for p in polylines {
        let paths = thick_polyline_to_extrusion_paths_2(p, role, flow, tolerance)?;
        // The paths appended.
        if let (Some(front), Some(back)) = (paths.first(), paths.last()) {
            if front.first_point() == back.last_point() {
                out.push(ExtrusionEntity::Loop(ExtrusionLoop::from_paths(
                    paths,
                    ExtrusionLoopRole::DEFAULT,
                )));
            } else {
                for mut path in paths {
                    path.inset_idx = -1;
                    out.push(ExtrusionEntity::Path(path));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use orca_geom::polyline::Polyline;

    fn thick(points: &[(i64, i64)], widths: &[f64]) -> ThickPolyline {
        ThickPolyline {
            polyline: Polyline::new(points.iter().map(|&(x, y)| Point::new(x, y)).collect()),
            width: widths.to_vec(),
            endpoints: (false, false),
        }
    }

    #[test]
    fn an_even_line_is_one_path() {
        let tp = thick(
            &[(0, 0), (1_000_000, 0), (2_000_000, 0)],
            &[400_000., 400_000., 400_000., 400_000.],
        );
        let flow = Flow::new(0.45, 0.2, 0.4).unwrap();
        let m = thick_polyline_to_multi_path(&tp, ExtrusionRole::GapFill, &flow, 50_000., 20_000.)
            .unwrap();
        assert_eq!(m.paths.len(), 1);
        assert_eq!(m.paths[0].polyline.points.len(), 3);
        let mut out = ExtrusionEntitiesPtr::new();
        variable_width(&[tp], ExtrusionRole::GapFill, &flow, &mut out).unwrap();
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn an_uneven_line_is_cut() {
        let tp = thick(&[(0, 0), (1_000_000, 0)], &[100_000., 400_000.]);
        let flow = Flow::new(0.45, 0.2, 0.4).unwrap();
        let m =
            thick_polyline_to_multi_path(&tp, ExtrusionRole::GapFill, &flow, 50_000., 1.).unwrap();
        // Six segments of different widths, each its own path.
        assert_eq!(m.paths.len(), 6);
    }
}
