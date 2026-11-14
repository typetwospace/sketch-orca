// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: namespace Slic3r::Geometry of src/libslic3r/Geometry.hpp and
// Geometry.cpp of OrcaSlicer v2.4.2 (the 2D predicates, angles and transform builders), translated
// on 2026-11-07; and Geometry.cpp 754-776 and 863-907 (rotation_xyz_diff, rotation_diff_z and
// is_point_inside_polygon_corner) with src/libslic3r/Geometry/Circle.hpp 153-183
// (ray_circle_intersections_r2_lv2_c), translated on 2026-11-13. Copyright the OrcaSlicer authors,
// the BambuStudio authors (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU
// Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! `Slic3r::Geometry`: orientation and intersection predicates, angle helpers, the transform
//! builders (`assemble_transform`, `rotation_transform`, `extract_euler_angles`), the rotation
//! difference of two sets of Euler angles (`rotation_xyz_diff`, `rotation_diff_z`), the corner test
//! `is_point_inside_polygon_corner` and the line and circle crossing
//! `ray_circle_intersections_r2_lv2_c`.
//!
//! The class `Transformation`, `arrange` (the grid arrangement of `Geometry.cpp`) and the string
//! helpers are not here yet, nor are the two templates of Geometry.cpp 287-306, which nothing
//! instantiates; the convex hulls are in [`crate::convex_hull`].

use orca_eigen::rotation::{AngleAxis, Quaternion, euler_angles};
use orca_eigen::{Matrix3d, Transform3d, Vec2d, Vec3d, Vector};

use crate::bounding_box::BoundingBoxf;
use crate::expolygon::ExPolygon;
use crate::line::Line;
use crate::multi_point;
use crate::point::{EPSILON, PI, Point, perp};
use crate::polygon::{Polygon, Polygons};

/// `Orientation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    /// `ORIENTATION_CCW`.
    Ccw = 1,
    /// `ORIENTATION_CW`.
    Cw = -1,
    /// `ORIENTATION_COLINEAR`.
    Colinear = 0,
}

/// The orientation of a determinant.
fn orientation_of_i64(d: i64) -> Orientation {
    if d > 0 {
        Orientation::Ccw
    } else if d == 0 {
        Orientation::Colinear
    } else {
        Orientation::Cw
    }
}

/// `orient(a, b, c)`: in 64-bit integers when every coordinate is at most `0xffffffff` (the C++
/// tests no lower bound, and its products wrap for large negative coordinates), in doubles
/// otherwise.
#[must_use]
pub fn orient(a: Point, b: Point, c: Point) -> Orientation {
    const LIM: i64 = 0xffff_ffff;
    if a.x <= LIM && b.x <= LIM && c.x <= LIM && a.y <= LIM && b.y <= LIM && c.y <= LIM {
        let u = b.x.wrapping_mul(c.y).wrapping_sub(b.y.wrapping_mul(c.x));
        let v = a.x.wrapping_mul(c.y).wrapping_sub(a.y.wrapping_mul(c.x));
        let w = a.x.wrapping_mul(b.y).wrapping_sub(a.y.wrapping_mul(b.x));
        orientation_of_i64(u.wrapping_sub(v).wrapping_add(w))
    } else {
        let u = b.x as f64 * c.y as f64 - b.y as f64 * c.x as f64;
        let v = a.x as f64 * c.y as f64 - a.y as f64 * c.x as f64;
        let w = a.x as f64 * b.y as f64 - a.y as f64 * b.x as f64;
        let d = u - v + w;
        if d > 0.0 {
            Orientation::Ccw
        } else if d == 0.0 {
            Orientation::Colinear
        } else {
            Orientation::Cw
        }
    }
}

/// `is_ccw(poly)`: the orientation at the lowest-leftmost vertex; true below three points.
#[must_use]
pub fn is_ccw(poly: &Polygon) -> bool {
    let pts = &poly.points;
    if pts.len() < 3 {
        return true;
    }
    let mut imin = 0;
    for i in 1..pts.len() {
        let pmin = pts[imin];
        let p = pts[i];
        if p.x < pmin.x || (p.x == pmin.x && p.y < pmin.y) {
            imin = i;
        }
    }
    let i_prev = if imin == 0 { pts.len() } else { imin } - 1;
    let i_next = if imin + 1 == pts.len() { 0 } else { imin + 1 };
    orient(pts[i_prev], pts[imin], pts[i_next]) == Orientation::Ccw
}

/// `ray_ray_intersection(p1, v1, p2, v2, res)`.
#[must_use]
pub fn ray_ray_intersection(p1: Vec2d, v1: Vec2d, p2: Vec2d, v2: Vec2d) -> Option<Vec2d> {
    let denom = v1.x() * v2.y() - v2.x() * v1.y();
    if orca_crt::fabs(denom) < EPSILON {
        return None;
    }
    let t = (v2.x() * (p1.y() - p2.y()) - v2.y() * (p1.x() - p2.x())) / denom;
    Some(Vector([p1.x() + t * v1.x(), p1.y() + t * v1.y()]))
}

/// `segment_segment_intersection(p1, v1, p2, v2, res)`.
#[must_use]
pub fn segment_segment_intersection(p1: Vec2d, v1: Vec2d, p2: Vec2d, v2: Vec2d) -> Option<Vec2d> {
    let mut denom = v1.x() * v2.y() - v2.x() * v1.y();
    if orca_crt::fabs(denom) < EPSILON {
        return None;
    }
    let s12_x = p1.x() - p2.x();
    let s12_y = p1.y() - p2.y();
    let mut s_numer = v1.x() * s12_y - v1.y() * s12_x;
    let mut denom_is_positive = false;
    if denom < 0.0 {
        denom_is_positive = true;
        denom = -denom;
        s_numer = -s_numer;
    }
    if s_numer < 0.0 {
        return None;
    }
    let mut t_numer = v2.x() * s12_y - v2.y() * s12_x;
    if !denom_is_positive {
        t_numer = -t_numer;
    }
    if t_numer < 0.0 || s_numer > denom || t_numer > denom {
        return None;
    }
    let t = t_numer / denom;
    Some(Vector([p1.x() + t * v1.x(), p1.y() + t * v1.y()]))
}

/// The signs of `jp1` and `jp2` against the segment `ip1 ip2`, in 64-bit cross products.
fn segments_could_intersect(ip1: Point, ip2: Point, jp1: Point, jp2: Point) -> (i32, i32) {
    let iv = ip2 - ip1;
    let vij1 = jp1 - ip1;
    let vij2 = jp2 - ip1;
    let tij1 = crate::point::cross2_coord(iv, vij1);
    let tij2 = crate::point::cross2_coord(iv, vij2);
    (tij1.signum() as i32, tij2.signum() as i32)
}

/// `segments_intersect(ip1, ip2, jp1, jp2)`: true when the closed segments share a point.
#[must_use]
pub fn segments_intersect(ip1: Point, ip2: Point, jp1: Point, jp2: Point) -> bool {
    let sign1 = segments_could_intersect(ip1, ip2, jp1, jp2);
    let sign2 = segments_could_intersect(jp1, jp2, ip1, ip2);
    let test1 = sign1.0 * sign1.1;
    let test2 = sign2.0 * sign2.1;
    if test1 <= 0 && test2 <= 0 {
        if test1 != 0 || test2 != 0 {
            return true;
        }
        if sign1.0 == 0 && sign1.1 == 0 {
            // Collinear: the intervals along the longer axis overlap.
            let vi = ip2 - ip1;
            let axis_x = vi.x.wrapping_abs() > vi.y.wrapping_abs();
            let pick = |p: Point| if axis_x { p.x } else { p.y };
            let (mut i, mut j, mut k, mut l) = (pick(ip1), pick(ip2), pick(jp1), pick(jp2));
            if i > j {
                core::mem::swap(&mut i, &mut j);
            }
            if k > l {
                core::mem::swap(&mut k, &mut l);
            }
            return (k >= i && k <= j) || (i >= k && i <= l);
        }
    }
    false
}

/// `foot_pt(line_pt, line_dir, pt)` of `Vec2d`.
#[must_use]
pub fn foot_pt(line_pt: Vec2d, line_dir: Vec2d, pt: Vec2d) -> Vec2d {
    let v = pt - line_pt;
    let l2 = line_dir.squared_norm();
    let t = if l2 == 0.0 {
        0.0
    } else {
        v.dot(&line_dir) / l2
    };
    line_pt + line_dir * t
}

/// `foot_pt(line, pt)`.
#[must_use]
pub fn foot_pt_line(iline: &Line, ipt: Point) -> Vec2d {
    foot_pt(
        iline.a.cast_f64(),
        (iline.b - iline.a).cast_f64(),
        ipt.cast_f64(),
    )
}

/// `ray_point_distance_squared(line, pt)`.
#[must_use]
pub fn ray_point_distance_squared(iline: &Line, ipt: Point) -> f64 {
    (foot_pt_line(iline, ipt) - ipt.cast_f64()).squared_norm()
}

/// `ray_point_distance(line, pt)`.
#[must_use]
pub fn ray_point_distance(iline: &Line, ipt: Point) -> f64 {
    (foot_pt_line(iline, ipt) - ipt.cast_f64()).norm()
}

/// `liang_barsky_line_clipping_interval(x0, v, bbox, out_interval)` of `Vec2d`.
#[must_use]
pub fn liang_barsky_line_clipping_interval(
    x0: Vec2d,
    v: Vec2d,
    bbox: &BoundingBoxf,
) -> Option<(f64, f64)> {
    let mut t0 = 0.0;
    let mut t1 = 1.0;
    let mut clip_side = |p: f64, q: f64| -> bool {
        if p == 0.0 {
            if q < 0.0 {
                return false;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                if r > t1 {
                    return false;
                }
                if r > t0 {
                    t0 = r;
                }
            } else {
                if r < t0 {
                    return false;
                }
                if r < t1 {
                    t1 = r;
                }
            }
        }
        true
    };
    if clip_side(-v.x(), -bbox.min.x() + x0.x())
        && clip_side(v.x(), bbox.max.x() - x0.x())
        && clip_side(-v.y(), -bbox.min.y() + x0.y())
        && clip_side(v.y(), bbox.max.y() - x0.y())
    {
        return Some((t0, t1));
    }
    None
}

/// `liang_barsky_line_clipping(x0, x1, bbox)`: the segment clipped to the box, `None` when it
/// misses it.
#[must_use]
pub fn liang_barsky_line_clipping(
    x0: Vec2d,
    x1: Vec2d,
    bbox: &BoundingBoxf,
) -> Option<(Vec2d, Vec2d)> {
    let v = x1 - x0;
    let (first, second) = liang_barsky_line_clipping_interval(x0, v, bbox)?;
    Some((x0 + v * first, x0 + v * second))
}

/// `directions_parallel(angle1, angle2, max_diff)`.
#[must_use]
pub fn directions_parallel(angle1: f64, angle2: f64, mut max_diff: f64) -> bool {
    let diff = orca_crt::fabs(angle1 - angle2);
    max_diff += EPSILON;
    diff < max_diff || orca_crt::fabs(diff - PI) < max_diff
}

/// `directions_perpendicular(angle1, angle2, max_diff)`.
#[must_use]
pub fn directions_perpendicular(angle1: f64, angle2: f64, mut max_diff: f64) -> bool {
    let diff = orca_crt::fabs(angle1 - angle2);
    max_diff += EPSILON;
    orca_crt::fabs(diff - 0.5 * PI) < max_diff || orca_crt::fabs(diff - 1.5 * PI) < max_diff
}

/// `contains(expolygons, point)`: inside any of them (border included).
#[must_use]
pub fn contains(vector: &[ExPolygon], point: Point) -> bool {
    vector.iter().any(|e| e.contains(point, true))
}

/// `rad2deg(angle)`.
#[must_use]
pub fn rad2deg(angle: f64) -> f64 {
    180.0 * angle / PI
}

/// `rad2deg_dir(angle)`.
#[must_use]
pub fn rad2deg_dir(mut angle: f64) -> f64 {
    angle = if angle < PI {
        -angle + PI / 2.0
    } else {
        angle + PI / 2.0
    };
    if angle < 0.0 {
        angle += PI;
    }
    rad2deg(angle)
}

/// `deg2rad(angle)`.
#[must_use]
pub fn deg2rad(angle: f64) -> f64 {
    PI * angle / 180.0
}

/// `deg2rad(angle)` of a `float`.
#[must_use]
pub fn deg2rad_f32(angle: f32) -> f32 {
    PI as f32 * angle / 180.0
}

/// `angle_to_0_2PI(angle)`: steps of 2π until the angle is in [0, 2π].
#[must_use]
pub fn angle_to_0_2pi(mut angle: f64) -> f64 {
    let two_pi = 2.0 * PI;
    while angle < 0.0 {
        angle += two_pi;
    }
    while two_pi < angle {
        angle -= two_pi;
    }
    angle
}

/// `to_range_pi_pi(angle)`: outside (-π, π], less `int(round(angle / 2π)) * 2` times π.
pub fn to_range_pi_pi(angle: &mut f64) {
    if *angle > PI || *angle <= -PI {
        let count = orca_crt::f64_to_i32(orca_crt::round(*angle / (2.0 * PI)));
        *angle -= f64::from(count.wrapping_mul(2)) * PI;
    }
}

/// `simplify_polygons(polygons, tolerance, retval)`: each polygon Douglas-Peucker simplified as a
/// closed path, then `Slic3r::simplify_polygons`.
#[must_use]
pub fn simplify_polygons(polygons: &[Polygon], tolerance: f64) -> Polygons {
    let mut pp = Polygons::with_capacity(polygons.len());
    for it in polygons {
        let mut points = it.points.clone();
        points.push(points[0]);
        let mut points = multi_point::douglas_peucker(&points, tolerance);
        points.pop();
        pp.push(Polygon::new(points));
    }
    crate::clipper_utils::simplify_polygons(&pp)
}

/// `linint(value, oldmin, oldmax, newmin, newmax)`.
#[must_use]
pub fn linint(value: f64, oldmin: f64, oldmax: f64, newmin: f64, newmax: f64) -> f64 {
    (value - oldmin) * (newmax - newmin) / (oldmax - oldmin) + newmin
}

/// The unit vectors of Eigen.
fn unit(axis: usize) -> Vec3d {
    let mut v = Vector([0.0; 3]);
    v[axis] = 1.0;
    v
}

/// `AngleAxisd(rz, UnitZ) * AngleAxisd(ry, UnitY) * AngleAxisd(rx, UnitX)`, as a rotation
/// matrix: the quaternion product of the three, then `toRotationMatrix`.
fn zyx_rotation(rotation: &Vec3d) -> Matrix3d {
    let q: Quaternion<f64> = AngleAxis::new(rotation[2], unit(2))
        .mul(&AngleAxis::new(rotation[1], unit(1)))
        .mul_angle_axis(&AngleAxis::new(rotation[0], unit(0)));
    q.to_rotation_matrix()
}

/// `assemble_transform(translation, rotation, scale, mirror)`: identity, translated, rotated Z
/// Y X, scaled by `scale ∘ mirror`.
#[must_use]
pub fn assemble_transform(
    translation: &Vec3d,
    rotation: &Vec3d,
    scale: &Vec3d,
    mirror: &Vec3d,
) -> Transform3d {
    let mut transform = Transform3d::identity();
    transform.translate(translation);
    transform.rotate(&zyx_rotation(rotation));
    let s = Vector([
        scale[0] * mirror[0],
        scale[1] * mirror[1],
        scale[2] * mirror[2],
    ]);
    transform.scale(&s);
    transform
}

/// `assemble_transform(translation, rotation, scale, mirror)` of transforms: their product.
#[must_use]
pub fn assemble_transform_of(
    translation: &Transform3d,
    rotation: &Transform3d,
    scale: &Transform3d,
    mirror: &Transform3d,
) -> Transform3d {
    translation.mul(rotation).mul(scale).mul(mirror)
}

/// `translation_transform(translation)`.
#[must_use]
pub fn translation_transform(translation: &Vec3d) -> Transform3d {
    let mut transform = Transform3d::identity();
    transform.translate(translation);
    transform
}

/// `rotation_transform(rotation)`: rotated Z Y X.
#[must_use]
pub fn rotation_transform(rotation: &Vec3d) -> Transform3d {
    let mut transform = Transform3d::identity();
    transform.rotate(&zyx_rotation(rotation));
    transform
}

/// `scale_transform(scale)` of a factor: `scale * Vec3d::Ones()`.
#[must_use]
pub fn scale_transform_uniform(scale: f64) -> Transform3d {
    scale_transform(&Vector([scale; 3]))
}

/// `scale_transform(scale)`.
#[must_use]
pub fn scale_transform(scale: &Vec3d) -> Transform3d {
    let mut transform = Transform3d::identity();
    transform.scale(scale);
    transform
}

/// `extract_euler_angles(rotation_matrix)`: `eulerAngles(2, 1, 0)` with the first and last
/// swapped, so that `rotation_transform` gives the matrix back.
#[must_use]
pub fn extract_euler_angles(rotation_matrix: &Matrix3d) -> Vec3d {
    let mut angles = euler_angles(rotation_matrix, 2, 1, 0);
    angles.0.swap(0, 2);
    angles
}

/// `extract_euler_angles(transform)`: of the linear part with each column normalized.
#[must_use]
pub fn extract_euler_angles_of(transform: &Transform3d) -> Vec3d {
    let mut m = transform.linear();
    for c in 0..3 {
        m.set_col(c, m.col(c).normalized());
    }
    extract_euler_angles(&m)
}

/// `rotation_xyz_diff(rot_xyz_from, rot_xyz_to)`: the rotation from the first set of X Y Z angles
/// to the second, `Z(to) Y(to) X(to)` then `X(-from) Y(-from) Z(-from)`, multiplied left to right
/// as Eigen does (the first two angle-axes into a quaternion, each later one onto it).
#[must_use]
pub fn rotation_xyz_diff(rot_xyz_from: &Vec3d, rot_xyz_to: &Vec3d) -> Quaternion<f64> {
    // From the current coordinate system to the world.
    AngleAxis::new(rot_xyz_to[2], unit(2))
        .mul(&AngleAxis::new(rot_xyz_to[1], unit(1)))
        .mul_angle_axis(&AngleAxis::new(rot_xyz_to[0], unit(0)))
        // From the world to the initial coordinate system.
        .mul_angle_axis(&AngleAxis::new(-rot_xyz_from[0], unit(0)))
        .mul_angle_axis(&AngleAxis::new(-rot_xyz_from[1], unit(1)))
        .mul_angle_axis(&AngleAxis::new(-rot_xyz_from[2], unit(2)))
}

/// `rotation_diff_z(rot_xyz_from, rot_xyz_to)`: the angle of [`rotation_xyz_diff`] as an
/// angle-axis, negated when its axis points down. Meant for rotations that differ only about Z
/// (the C++ asserts it in its debug build only).
#[must_use]
pub fn rotation_diff_z(rot_xyz_from: &Vec3d, rot_xyz_to: &Vec3d) -> f64 {
    let angle_axis = AngleAxis::from_quaternion(&rotation_xyz_diff(rot_xyz_from, rot_xyz_to));
    let axis = angle_axis.axis;
    let angle = angle_axis.angle;
    if axis[2] < 0.0 { -angle } else { angle }
}

/// `is_point_inside_polygon_corner(a, b, c, query_point)`: whether `query_point` is inside the
/// corner `a b c` of a counter-clockwise contour, from the unit vectors from `b` (the differences
/// in 64-bit integers) and their projections on `b q` and its left normal. `a` and `c` must
/// differ (the C++ asserts it in its debug build only).
#[must_use]
pub fn is_point_inside_polygon_corner(a: Point, b: Point, c: Point, query_point: Point) -> bool {
    // Based at B, and normalized so that the vectors end on the unit circle.
    let ba = (a - b).cast_f64().normalized();
    let bc = (c - b).cast_f64().normalized();
    let bq = (query_point - b).cast_f64().normalized();

    // The normal of BQ on its left side.
    let bq_left_normal = perp(bq);

    let proj_a_on_bq_normal = ba.dot(&bq_left_normal);
    let proj_c_on_bq_normal = bc.dot(&bq_left_normal);
    if (proj_a_on_bq_normal > 0. && proj_c_on_bq_normal <= 0.)
        || (proj_a_on_bq_normal <= 0. && proj_c_on_bq_normal > 0.)
    {
        // Q between A and C, or on BA or BC: inside when A is on the normal's side.
        proj_a_on_bq_normal > 0.
    } else {
        // Q not between A and C, but maybe still inside the corner: right of BA and right of the
        // bisector of ABC, or left of both.
        let proj_a_on_bq = ba.dot(&bq);
        let proj_c_on_bq = bc.dot(&bq);
        (proj_a_on_bq_normal > 0. && proj_c_on_bq < proj_a_on_bq)
            || (proj_a_on_bq_normal <= 0. && proj_c_on_bq >= proj_a_on_bq)
    }
}

/// `ray_circle_intersections_r2_lv2_c<double>(r2, a, b, lv2, c, out)` (Geometry/Circle.hpp):
/// where the line `a x + b y + c = 0` (`lv2 = a² + b²`) meets the circle of squared radius `r2`
/// about the origin. Returns 0 (and leaves `out`), 1 for a tangent or 2.
pub fn ray_circle_intersections_r2_lv2_c(
    r2: f64,
    a: f64,
    b: f64,
    lv2: f64,
    c: f64,
    out: &mut (Vec2d, Vec2d),
) -> i32 {
    let x0 = -a * c;
    let y0 = -b * c;
    let d2 = r2 * lv2 - c * c;
    if d2 < 0.0 {
        return 0;
    }
    let d = orca_crt::sqrt(d2);
    out.0[0] = (x0 + b * d) / lv2;
    out.0[1] = (y0 - a * d) / lv2;
    out.1[0] = (x0 - b * d) / lv2;
    out.1[1] = (y0 + a * d) / lv2;
    if d == 0.0 { 1 } else { 2 }
}
