// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/PerimeterGenerator.cpp (673-1085) of OrcaSlicer v2.4.2,
// with Polygon.hpp's to_lines of polygons (208-219), translated on 2026-11-14 (orca-o3-walls-1).
// Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab), the PrusaSlicer authors
// (Prusa Research) and the Slic3r authors, under the GNU Affero General Public License, version 3
// (AGPL-3.0; LICENSE).

//! Extra perimeters on overhangs, PrusaSlicer's port (its author Pavel Mikuš): the overhang part
//! of the infill area filled with overhang perimeters that start from the anchors and run out
//! over the air, so that a later layer has something to stand on.
//!
//! [`generate_extra_perimeters_over_overhangs`] takes the infill area's overhang (the part off the
//! lower slices), shrinks the anchors away from it by the anchor size, and for each island of
//! what is left: skips it when a bridge would do (little of it outside its anchors' convex hull
//! and short unsupported bridge ends), else offsets inwards from the anchored outline one spacing
//! at a time while the loops still reach the overhang, keeping each loop's part inside the island
//! as overhang perimeters, and ends with the medial axis of what is too thin for a loop. Each
//! region's paths are then ordered by [`sort_extra_perimeters`]: the anchored ones first, each
//! later one after the paths it touches ([`paths_touch`]), nearest first, joined where they meet.
//! [`reconnect_polylines`] joins the polylines whose ends are within a limit.
//!
//! The C++'s quirks are kept: `reconnect_polylines` walks an `std::unordered_map<size_t,
//! Polyline>` in MSVC's order ([`orca_stl::UnorderedMap`]); `sort_extra_perimeters` keeps its
//! dependencies in `std::unordered_set<size_t>`s read only by membership and emptiness (a
//! `BTreeSet` here), compares a squared distance with `scaled(5.0)`, and starts its second search
//! at `next_idx + 1` with `next_idx` just set to `size_t(-1)`, so at 0; and
//! `generate_extra_perimeters_over_overhangs` keeps `if (double d = dist < min_dist)`, which stores
//! the comparison in `d`, so the closed first loop is rotated to the last point nearer than 1 to
//! the lower slices (after the first).

use std::collections::BTreeSet;

use orca_config::print_config::{PrintConfig, PrintObjectConfig};
use orca_geom::clipper::JoinType;
use orca_geom::clipper_utils::{
    ApplySafetyOffset, DEFAULT_JOIN_TYPE, DEFAULT_MITER_LIMIT,
    clip_clipper_polygons_with_subject_bbox, diff, diff_expolygons_polygons, expand, intersection,
    intersection_expolygons_polygons, intersection_pl, intersection_pl_polyline, offset, offset_ex,
    shrink, union_, union_ex_expolygons, union_two,
};
use orca_geom::convex_hull::convex_hull_polygons;
use orca_geom::expolygon::{self, ExPolygons, get_extents_ex, to_expolygons};
use orca_geom::line::{Line, Lines};
use orca_geom::point::{Coord, SCALED_EPSILON, scale_, scaled};
use orca_geom::polygon::{self, Polygon, Polygons};
use orca_geom::polyline::{Polyline, Polylines};
use orca_index::aabb_lines::LinesDistancer;
use orca_layer::cpp_vec;
use orca_layer::entity::extrusion_paths_append;
use orca_layer::flow::Flow;
use orca_layer::path::{ExtrusionPath, ExtrusionPaths};
use orca_layer::role::ExtrusionRole;
use orca_stl::UnorderedMap;
use orca_voronoi::medial_axis::ExPolygonMedialAxis;

use crate::bridge_detector::detect_bridging_direction;
use crate::stop::{EMPTY_LIST, PerimeterError, UNANCHORED_PAST_END, not_ported};

/// `EXTERNAL_INFILL_MARGIN` (libslic3r.h): 3 mm.
pub const EXTERNAL_INFILL_MARGIN: f64 = 3.;
/// `BRIDGE_INFILL_MARGIN` (libslic3r.h): 1 mm.
pub const BRIDGE_INFILL_MARGIN: f64 = 1.;

/// `to_lines(const Polygons &)` (Polygon.hpp 208-219): each polygon's edges, the closing one
/// last, with no size check (a polygon of one point gives one line, of two points two). It belongs
/// in `orca-geom`, whose `to_lines` is of one polygon and gives nothing below three points;
/// translated here (walls-1).
///
/// # Panics
/// With [`NotPorted`](crate::stop::NotPorted) for an empty polygon, whose `end() - 1` the C++
/// reads before its start.
#[must_use]
pub fn to_lines_polygons(polys: &[Polygon]) -> Lines {
    let mut lines = Lines::with_capacity(polygon::count_points(polys));
    for poly in polys {
        let pts = &poly.points;
        if pts.is_empty() {
            not_ported("to_lines", EMPTY_LIST);
        }
        for w in pts.windows(2) {
            lines.push(Line { a: w[0], b: w[1] });
        }
        lines.push(Line {
            a: pts[pts.len() - 1],
            b: pts[0],
        });
    }
    lines
}

/// `(a - b).cast<double>().squaredNorm()` of two points.
fn dist_sqr(a: orca_geom::point::Point, b: orca_geom::point::Point) -> f64 {
    (a - b).cast_f64().squared_norm()
}

/// `paths_touch(path_one, path_two, limit_distance)`: whether a point of either path is nearer
/// than `limit_distance` to the other's lines.
#[must_use]
pub fn paths_touch(
    path_one: &ExtrusionPath,
    path_two: &ExtrusionPath,
    limit_distance: f64,
) -> bool {
    let lines_two: LinesDistancer<Line, 2> = LinesDistancer::new(path_two.as_polyline().lines());
    for pt_idx in 0..path_one.polyline.size() {
        let pt = path_one.polyline.points[pt_idx].to_point();
        if lines_two.distance_from_lines::<false>(&pt.to_vec2crd()) < limit_distance {
            return true;
        }
    }
    let lines_one: LinesDistancer<Line, 2> = LinesDistancer::new(path_one.as_polyline().lines());
    for pt_idx in 0..path_two.polyline.size() {
        let pt = path_two.polyline.points[pt_idx].to_point();
        if lines_one.distance_from_lines::<false>(&pt.to_vec2crd()) < limit_distance {
            return true;
        }
    }
    false
}

/// `reconnect_polylines(polylines, limit_distance)`: each polyline in turn takes every later one
/// whose end is within the limit of one of its ends (last to first, last to last, first to last,
/// first to first), joined; the result in the order of the map that held them.
#[must_use]
pub fn reconnect_polylines(polylines: &[Polyline], limit_distance: f64) -> Polylines {
    if polylines.is_empty() {
        return polylines.to_vec();
    }

    let mut connected: UnorderedMap<usize, Polyline> = UnorderedMap::new();
    connected.reserve(polylines.len());
    for (i, pl) in polylines.iter().enumerate() {
        if !pl.is_empty() {
            connected.insert(i, pl.clone());
        }
    }

    let limit_sqr = limit_distance * limit_distance;
    for a in 0..polylines.len() {
        if !connected.contains_key(&a) {
            continue;
        }
        for b in (a + 1)..polylines.len() {
            let Some(next) = connected.get(&b) else {
                continue;
            };
            let (next_first, next_last) = (next.first_point(), next.last_point());
            let Some(base) = connected.get(&a) else {
                continue;
            };
            let (base_first, base_last) = (base.first_point(), base.last_point());
            if dist_sqr(base_last, next_first) < limit_sqr {
                let mut next = connected.remove(&b).expect("b is in the map");
                let base = connected.get_mut(&a).expect("a is in the map");
                base.append_polyline_moved(&mut next);
            } else if dist_sqr(base_last, next_last) < limit_sqr {
                let next = connected.remove(&b).expect("b is in the map");
                let base = connected.get_mut(&a).expect("a is in the map");
                base.points.extend(next.points.iter().rev());
            } else if dist_sqr(base_first, next_last) < limit_sqr {
                let mut next = connected.remove(&b).expect("b is in the map");
                let base = connected.get_mut(&a).expect("a is in the map");
                next.append_polyline_moved(base);
                *base = next;
                base.reverse();
            } else if dist_sqr(base_first, next_first) < limit_sqr {
                let mut next = connected.remove(&b).expect("b is in the map");
                let base = connected.get_mut(&a).expect("a is in the map");
                base.reverse();
                base.append_polyline_moved(&mut next);
                base.reverse();
            }
        }
    }

    let mut result = Polylines::new();
    for (_, ext) in connected.iter() {
        result.push(ext.clone());
    }

    result
}

/// `sort_extra_perimeters(extra_perims, index_of_first_unanchored, extrusion_spacing)`: the paths
/// in printing order (each after the paths it touches, from the anchored ones, nearest first,
/// reversed where that is nearer), those that meet joined, and those not longer than three
/// spacings dropped.
///
/// # Panics
/// With [`NotPorted`](crate::stop::NotPorted) for an index past the end, which the C++ writes past
/// its list for.
#[must_use]
#[allow(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss
)]
pub fn sort_extra_perimeters(
    extra_perims: &[ExtrusionPath],
    index_of_first_unanchored: i32,
    extrusion_spacing: f64,
) -> ExtrusionPaths {
    if extra_perims.is_empty() {
        return ExtrusionPaths::new();
    }
    let n = extra_perims.len();
    if i64::from(index_of_first_unanchored) > n as i64 {
        not_ported("sort_extra_perimeters", UNANCHORED_PAST_END);
    }

    let mut dependencies: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); n];
    for path_idx in 0..n {
        for prev_path_idx in 0..path_idx {
            // `extrusion_spacing * 1.5f`: the float promoted to double.
            if paths_touch(
                &extra_perims[path_idx],
                &extra_perims[prev_path_idx],
                extrusion_spacing * f64::from(1.5f32),
            ) {
                dependencies[path_idx].insert(prev_path_idx);
            }
        }
    }

    let mut processed = vec![false; n];
    // `for (int path_idx = 0; path_idx < index_of_first_unanchored; ...)`: none when negative.
    for p in processed
        .iter_mut()
        .take(usize::try_from(index_of_first_unanchored).unwrap_or(0))
    {
        *p = true;
    }

    // `size_t i = index_of_first_unanchored`: a negative index converts to a size past the end.
    let start = index_of_first_unanchored as usize;
    let mut i = start;
    while i < n {
        let mut change = false;
        for path_idx in start..n {
            if processed[path_idx] {
                continue;
            }
            let has_processed_dep = dependencies[path_idx].iter().any(|&dep| processed[dep]);
            if has_processed_dep {
                let deps: Vec<usize> = dependencies[path_idx].iter().copied().collect();
                for dep in deps {
                    if !processed[dep] {
                        dependencies[dep].insert(path_idx);
                        dependencies[path_idx].remove(&dep);
                    }
                }
                processed[path_idx] = true;
                change = true;
            }
        }
        if !change {
            break;
        }
        i += 1;
    }

    let mut current_point = extra_perims[0].first_point();

    let mut sorted_paths = ExtrusionPaths::new();
    let null_idx = usize::MAX;
    let mut next_idx = null_idx;
    let mut reverse = false;
    loop {
        if next_idx == null_idx {
            // find next pidx to print
            let mut dist = f64::MAX;
            for (path_idx, path) in extra_perims.iter().enumerate() {
                if !dependencies[path_idx].is_empty() {
                    continue;
                }
                let dist_a = dist_sqr(path.first_point(), current_point);
                if dist_a < dist {
                    dist = dist_a;
                    next_idx = path_idx;
                    reverse = false;
                }
                let dist_b = dist_sqr(path.last_point(), current_point);
                if dist_b < dist {
                    dist = dist_b;
                    next_idx = path_idx;
                    reverse = true;
                }
            }
            if next_idx == null_idx {
                break;
            }
        } else {
            // we have valid next_idx, add it to the sorted paths, update dependencies, update
            // current point and potentialy set new next_idx
            let mut path = extra_perims[next_idx].clone();
            if reverse {
                path.reverse();
            }
            cpp_vec::push(&mut sorted_paths, path);
            dependencies[next_idx].insert(null_idx);
            current_point = sorted_paths[sorted_paths.len() - 1].last_point();
            for deps in &mut dependencies {
                deps.remove(&next_idx);
            }
            let mut dist = f64::MAX;
            next_idx = null_idx;

            // `path_idx = next_idx + 1` with `next_idx` just set to `size_t(-1)`: from 0.
            for (path_idx, next_path) in extra_perims.iter().enumerate() {
                if !dependencies[path_idx].is_empty() {
                    continue;
                }
                let dist_a = dist_sqr(next_path.first_point(), current_point);
                if dist_a < dist {
                    dist = dist_a;
                    next_idx = path_idx;
                    reverse = false;
                }
                let dist_b = dist_sqr(next_path.last_point(), current_point);
                if dist_b < dist {
                    dist = dist_b;
                    next_idx = path_idx;
                    reverse = true;
                }
            }
            // A squared distance compared with a distance, as the C++ does.
            if dist > scaled(5.0) as f64 {
                next_idx = null_idx;
            }
        }
    }

    let mut reconnected = ExtrusionPaths::new();
    cpp_vec::reserve(&mut reconnected, sorted_paths.len());
    for path in &sorted_paths {
        if let Some(back) = reconnected.last_mut()
            && dist_sqr(back.last_point(), path.first_point())
                < extrusion_spacing * extrusion_spacing * 4.0
        {
            back.polyline
                .points
                .extend_from_slice(&path.polyline.points);
            continue;
        }
        cpp_vec::push(&mut reconnected, path.clone());
    }

    let mut filtered = ExtrusionPaths::new();
    cpp_vec::reserve(&mut filtered, reconnected.len());
    for p in &reconnected {
        if p.length() > 3. * extrusion_spacing {
            cpp_vec::push(&mut filtered, p.clone());
        }
    }

    filtered
}

/// `EXTRA_PERIMETER_OFFSET_PARAMETERS`: `ClipperLib::jtSquare, 0.`.
const EXTRA_PERIMETER_JOIN_TYPE: JoinType = JoinType::Square;
/// The miter limit of `EXTRA_PERIMETER_OFFSET_PARAMETERS`.
const EXTRA_PERIMETER_MITER_LIMIT: f64 = 0.;

/// `generate_extra_perimeters_over_overhangs(infill_area, lower_slices_polygons, perimeter_count,
/// overhang_flow, scaled_resolution, object_config, print_config)`: the extra perimeters of each
/// overhang region, and the area they fill. The resolution and the two configurations are not
/// read, as in the C++.
///
/// # Errors
/// The overhang flow's `mm3_per_mm` error, and the medial axis's Voronoi errors.
#[allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap
)]
pub fn generate_extra_perimeters_over_overhangs(
    infill_area: ExPolygons,
    lower_slices_polygons: &[Polygon],
    perimeter_count: i32,
    overhang_flow: &Flow,
    _scaled_resolution: f64,
    _object_config: &PrintObjectConfig,
    _print_config: &PrintConfig,
) -> Result<(Vec<ExtrusionPaths>, Polygons), PerimeterError> {
    let sp = overhang_flow.scaled_spacing();
    let spf = sp as f64;
    let anchors_size = orca_stl::min(
        orca_crt::f64_to_i64(scale_(EXTERNAL_INFILL_MARGIN)),
        sp.wrapping_mul(Coord::from(perimeter_count.wrapping_add(1))),
    );

    let infill_area_bb = get_extents_ex(&infill_area).inflated(SCALED_EPSILON);
    let optimized_lower_slices =
        clip_clipper_polygons_with_subject_bbox(lower_slices_polygons, &infill_area_bb);
    let overhangs =
        diff_expolygons_polygons(&infill_area, &optimized_lower_slices, ApplySafetyOffset::No);

    if overhangs.is_empty() {
        return Ok((Vec::new(), Polygons::new()));
    }

    let lower_layer_aabb_tree: LinesDistancer<Line, 2> =
        LinesDistancer::new(to_lines_polygons(&optimized_lower_slices));
    let anchors = intersection_expolygons_polygons(
        &infill_area,
        &optimized_lower_slices,
        ApplySafetyOffset::No,
    );
    let inset_anchors = diff(
        &anchors,
        &expand(
            &overhangs,
            (anchors_size as f64 + 0.1 * overhang_flow.scaled_width() as f64) as f32,
            EXTRA_PERIMETER_JOIN_TYPE,
            EXTRA_PERIMETER_MITER_LIMIT,
        ),
        ApplySafetyOffset::No,
    );
    let inset_overhang_area =
        diff_expolygons_polygons(&infill_area, &inset_anchors, ApplySafetyOffset::No);

    let mut inset_overhang_area_left_unfilled = Polygons::new();

    // overhang region -> extrusion paths
    let mut extra_perims: Vec<ExtrusionPaths> = Vec::new();
    for overhang in &union_ex_expolygons(&to_expolygons(inset_overhang_area.clone())) {
        let overhang_to_cover = expolygon::to_polygons(overhang);
        let expanded_overhang_to_cover = expand(
            &overhang_to_cover,
            (1.1 * spf) as f32,
            DEFAULT_JOIN_TYPE,
            DEFAULT_MITER_LIMIT,
        );
        let shrinked_overhang_to_cover = shrink(
            &overhang_to_cover,
            (0.1 * spf) as f32,
            DEFAULT_JOIN_TYPE,
            DEFAULT_MITER_LIMIT,
        );

        let real_overhang = intersection(&overhang_to_cover, &overhangs, ApplySafetyOffset::No);
        if real_overhang.is_empty() {
            inset_overhang_area_left_unfilled.extend_from_slice(&overhang_to_cover);
            continue;
        }
        extra_perims.push(ExtrusionPaths::new());
        let region_idx = extra_perims.len() - 1;

        let anchoring = intersection(
            &expanded_overhang_to_cover,
            &inset_anchors,
            ApplySafetyOffset::No,
        );
        let mut perimeter_polygon = offset(
            &union_two(
                &expand(
                    &overhang_to_cover,
                    (0.1 * spf) as f32,
                    DEFAULT_JOIN_TYPE,
                    DEFAULT_MITER_LIMIT,
                ),
                &anchoring,
            ),
            (-sp as f64 * 0.6) as f32,
            DEFAULT_JOIN_TYPE,
            DEFAULT_MITER_LIMIT,
        );

        let anchoring_convex_hull = convex_hull_polygons(&anchoring);
        let unbridgeable_area = polygon::area(&diff(
            &real_overhang,
            &[anchoring_convex_hull],
            ApplySafetyOffset::No,
        ));

        let (_dir, unsupp_dist) = detect_bridging_direction(&real_overhang, &anchors);

        if unbridgeable_area < 0.2 * polygon::area(&real_overhang)
            && unsupp_dist < polygon::total_length(&real_overhang) * 0.2
        {
            inset_overhang_area_left_unfilled.extend_from_slice(&overhang_to_cover);
            perimeter_polygon.clear();
        } else {
            //  fill the overhang with perimeters
            let mut continuation_loops = 2;
            while continuation_loops >= 0 {
                let prev = perimeter_polygon.clone();
                // prepare next perimeter lines
                let perimeter = intersection_pl(
                    &polygon::to_polylines(&perimeter_polygon),
                    &shrinked_overhang_to_cover,
                );

                // do not add the perimeter to result yet, first check if perimeter_polygon is not
                // empty after shrinking - this would mean that the polygon was possibly too small
                // for full perimeter loop and in that case try gap fill first
                perimeter_polygon = union_two(&perimeter_polygon, &anchoring);
                perimeter_polygon = intersection(
                    &offset(
                        &perimeter_polygon,
                        (-sp) as f32,
                        DEFAULT_JOIN_TYPE,
                        DEFAULT_MITER_LIMIT,
                    ),
                    &expanded_overhang_to_cover,
                    ApplySafetyOffset::No,
                );

                if perimeter_polygon.is_empty() {
                    // fill possible gaps of single extrusion width
                    let shrinked = intersection(
                        &offset(
                            &prev,
                            (-0.3 * spf) as f32,
                            DEFAULT_JOIN_TYPE,
                            DEFAULT_MITER_LIMIT,
                        ),
                        &expanded_overhang_to_cover,
                        ApplySafetyOffset::No,
                    );
                    if !shrinked.is_empty() {
                        let mm3_per_mm = overhang_flow.mm3_per_mm()?;
                        extrusion_paths_append(
                            &mut extra_perims[region_idx],
                            &reconnect_polylines(&perimeter, spf),
                            ExtrusionRole::OverhangPerimeter,
                            mm3_per_mm,
                            overhang_flow.width(),
                            overhang_flow.height(),
                        );
                    }

                    let mut fills = Polylines::new();
                    let gap = if shrinked.is_empty() {
                        offset_ex(
                            &prev,
                            (spf * 0.5) as f32,
                            DEFAULT_JOIN_TYPE,
                            DEFAULT_MITER_LIMIT,
                        )
                    } else {
                        to_expolygons(shrinked)
                    };

                    for ep in &gap {
                        ep.medial_axis_polylines(
                            0.75 * overhang_flow.scaled_width() as f64,
                            3.0 * spf,
                            &mut fills,
                        )?;
                    }
                    if !fills.is_empty() {
                        fills = intersection_pl(&fills, &shrinked_overhang_to_cover);
                        let mm3_per_mm = overhang_flow.mm3_per_mm()?;
                        extrusion_paths_append(
                            &mut extra_perims[region_idx],
                            &reconnect_polylines(&fills, spf),
                            ExtrusionRole::OverhangPerimeter,
                            mm3_per_mm,
                            overhang_flow.width(),
                            overhang_flow.height(),
                        );
                    }
                    break;
                }
                let mm3_per_mm = overhang_flow.mm3_per_mm()?;
                extrusion_paths_append(
                    &mut extra_perims[region_idx],
                    &reconnect_polylines(&perimeter, spf),
                    ExtrusionRole::OverhangPerimeter,
                    mm3_per_mm,
                    overhang_flow.width(),
                    overhang_flow.height(),
                );

                if intersection(&perimeter_polygon, &real_overhang, ApplySafetyOffset::No)
                    .is_empty()
                {
                    continuation_loops -= 1;
                }

                if prev == perimeter_polygon {
                    break;
                }
            }

            perimeter_polygon = expand(
                &perimeter_polygon,
                (0.5 * spf) as f32,
                DEFAULT_JOIN_TYPE,
                DEFAULT_MITER_LIMIT,
            );
            perimeter_polygon = union_two(&perimeter_polygon, &anchoring);
            inset_overhang_area_left_unfilled.extend_from_slice(&perimeter_polygon);

            let overhang_region = &mut extra_perims[region_idx];
            overhang_region.retain(|p| !p.is_empty());

            if !overhang_region.is_empty() {
                // there is a special case, where the first (or last) generated overhang perimeter
                // eats all anchor space. When this happens, the first overhang perimeter is also
                // a closed loop, and needs special check instead of the following simple
                // is_anchored lambda, which checks only the first and last point (not very useful
                // on closed polyline)
                let first_overhang_is_closed_and_anchored = overhang_region[0].first_point()
                    == overhang_region[0].last_point()
                    && !intersection_pl_polyline(
                        &overhang_region[0].polyline.to_polyline(),
                        &optimized_lower_slices,
                    )
                    .is_empty();

                let is_anchored = |path: &ExtrusionPath| -> bool {
                    lower_layer_aabb_tree
                        .distance_from_lines::<true>(&path.first_point().to_vec2crd())
                        <= 0.
                        || lower_layer_aabb_tree
                            .distance_from_lines::<true>(&path.last_point().to_vec2crd())
                            <= 0.
                };
                if first_overhang_is_closed_and_anchored {
                    let mut min_dist_idx = 0usize;
                    let mut min_dist = f64::MAX;
                    for i in 0..overhang_region[0].polyline.size() {
                        let p = overhang_region[0].polyline.points[i].to_point();
                        // `if (double d = distance < min_dist)`: d is the comparison, 1 or 0.
                        let d = f64::from(u8::from(
                            lower_layer_aabb_tree.distance_from_lines::<true>(&p.to_vec2crd())
                                < min_dist,
                        ));
                        if d != 0. {
                            min_dist = d;
                            min_dist_idx = i;
                        }
                    }
                    overhang_region[0].polyline.points.rotate_left(min_dist_idx);
                } else {
                    cpp_vec::reverse(overhang_region);
                }
                // `std::stable_partition(..., is_anchored)`: the anchored paths first, each part
                // in its order.
                let (anchored, unanchored): (ExtrusionPaths, ExtrusionPaths) =
                    overhang_region.drain(..).partition(|p| is_anchored(p));
                let index_of_first_unanchored = anchored.len() as i32;
                overhang_region.extend(anchored);
                overhang_region.extend(unanchored);
                *overhang_region =
                    sort_extra_perimeters(overhang_region, index_of_first_unanchored, spf);
            }
        }
    }

    inset_overhang_area_left_unfilled = union_(&inset_overhang_area_left_unfilled);

    Ok((
        extra_perims,
        diff(
            &inset_overhang_area,
            &inset_overhang_area_left_unfilled,
            ApplySafetyOffset::No,
        ),
    ))
}
