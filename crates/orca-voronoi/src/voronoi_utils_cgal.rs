// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Geometry/VoronoiUtilsCgal.hpp lines 1-29 and
// VoronoiUtilsCgal.cpp lines 1-334 of OrcaSlicer v2.4.2 (the class VoronoiUtilsCgal: the planarity
// checks by angle and by intersection, the parabolic tangent predicates, get_parabolic_segment,
// orientation_of_two_edges and check_if_three_edges_are_ccw), translated on 2026-11-14 by
// orca-o3-voronoi-4. Copyright the OrcaSlicer authors and the PrusaSlicer authors (Prusa Research),
// under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE). The file calls CGAL
// (its kernels, its filtered predicates and its sweep); no CGAL code is copied here: the answers
// CGAL gives are computed by this crate's own exact arithmetic (module `exact`).

//! libslic3r's VoronoiUtilsCgal (`Geometry/VoronoiUtilsCgal.hpp` and `.cpp`): two checks that a
//! Voronoi diagram is planar, which the repair's detection and Arachne's debug build use.
//!
//! - [`VoronoiUtilsCgal::is_voronoi_diagram_planar_angle`]: for every vertex with more than two
//!   finite edges (finite vertices only), each three neighbouring edges around it, in the order
//!   `rot_next` gives, must turn counter-clockwise. Two linear edges are compared with CGAL's
//!   `orientation` of `Simple_cartesian<double>`, which is not exact: the sign of
//!   `(q.x - p.x) * (r.y - p.y)` against `(r.x - p.x) * (q.y - p.y)`, each product rounded to
//!   `double` ([`orientation`]). A parabolic edge is compared by its tangent at the common vertex,
//!   with CGAL's filtered predicates, whose answer is the exact sign of a polynomial in the
//!   coordinates; that sign comes from [`crate::exact`]. The side of the focus that orients the
//!   tangent is CGAL's `opposite` of the double `orientation`, and a collinear side gives the
//!   clockwise perpendicular, as CGAL's `perpendicular` does with its precondition off.
//! - [`VoronoiUtilsCgal::is_voronoi_diagram_planar_intersection`]: whether no two finite linear
//!   edges (each pair of twins taken once, by colouring them 1 and back to 0) meet where CGAL's
//!   `compute_intersection_points` reports a point: a crossing of their interiors, or an end of
//!   one inside the other (its weak intersections, overlaps included). Edges that only share an
//!   end are not reported, as the sweep reports no such point. The test here is pairwise, with
//!   exact orientations, not CGAL's sweep; the answer is the same.
//!
//! Errors: the parabolic edges look up their sources with `VoronoiUtils::get_source_point` and
//! `get_source_segment`, whose exceptions pass through ([`VoronoiError`]). A null pointer the C++
//! follows (a vertex's incident edge, an edge's `prev`) panics here.
//!
//! Left out: the `ColoredLines` instantiation (`MultiMaterialSegmentation.cpp`, painted facets,
//! outside domain A: `ColoredLine` is not translated, so nothing can call it), the unused
//! `impl::Epick` class and `is_equal` (used only in asserts).

use orca_eigen::Vec2d;
use orca_geom::line::Line;
use orca_geom::point::{Point, vec2d};

use crate::diagram::{VoronoiEdge, VoronoiVertex};
use crate::exact::{Dyadic, Interval, Ring, exact_sign};
use crate::geometry_voronoi::VoronoiDiagram;
use crate::voronoi::SegmentConcept;
use crate::voronoi_utils::{VoronoiError, VoronoiUtils};

/// `CGAL::Orientation`: `RIGHT_TURN` (`CLOCKWISE`, -1), `COLLINEAR` (0), `LEFT_TURN`
/// (`COUNTERCLOCKWISE`, 1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CgalOrientation {
    /// `RIGHT_TURN`, `CLOCKWISE`, `NEGATIVE`.
    RightTurn,
    /// `COLLINEAR`, `ZERO`.
    Collinear,
    /// `LEFT_TURN`, `COUNTERCLOCKWISE`, `POSITIVE`.
    LeftTurn,
}

impl CgalOrientation {
    /// The orientation of a sign (`CGAL::sign`'s result as an `Orientation`).
    #[must_use]
    pub fn from_sign(sign: i32) -> Self {
        match sign {
            s if s < 0 => Self::RightTurn,
            0 => Self::Collinear,
            _ => Self::LeftTurn,
        }
    }

    /// `CGAL::opposite(o)`: `-o`.
    #[must_use]
    pub fn opposite(self) -> Self {
        match self {
            Self::RightTurn => Self::LeftTurn,
            Self::Collinear => Self::Collinear,
            Self::LeftTurn => Self::RightTurn,
        }
    }
}

/// `CGAL_Point` (`Simple_cartesian<double>::Point_2`).
#[derive(Clone, Copy, PartialEq, Debug)]
struct CgalPoint {
    x: f64,
    y: f64,
}

/// `to_cgal_point(const VD::vertex_type *)`.
fn to_cgal_point_vertex(v: &VoronoiVertex) -> CgalPoint {
    CgalPoint { x: v.x(), y: v.y() }
}

/// `to_cgal_point(const Point &)`: each `coord_t` converted to `double`.
#[allow(clippy::cast_precision_loss)]
fn to_cgal_point(p: Point) -> CgalPoint {
    CgalPoint {
        x: p.x as f64,
        y: p.y as f64,
    }
}

/// `to_cgal_point(const Vec2d &)`.
fn to_cgal_point_vec(v: Vec2d) -> CgalPoint {
    CgalPoint { x: v.x(), y: v.y() }
}

/// `CGAL::orientation(p, q, r)` of `Simple_cartesian<double>`: `orientationC2`, the sign of the
/// determinant as `compare((q.x - p.x) * (r.y - p.y), (r.x - p.x) * (q.y - p.y))` in `double`s
/// (`SMALLER` a right turn, `LARGER` a left turn, else collinear, a NaN included).
fn orientation(p: CgalPoint, q: CgalPoint, r: CgalPoint) -> CgalOrientation {
    let a00 = q.x - p.x;
    let a01 = q.y - p.y;
    let a10 = r.x - p.x;
    let a11 = r.y - p.y;
    let lhs = a00 * a11;
    let rhs = a10 * a01;
    if lhs < rhs {
        CgalOrientation::RightTurn
    } else if lhs > rhs {
        CgalOrientation::LeftTurn
    } else {
        CgalOrientation::Collinear
    }
}

// ---------------------------------------------------------------- the exact predicates

/// A point's two coordinates in a [`Ring`].
fn ring_point<N: Ring>(p: CgalPoint) -> [N; 2] {
    [N::from_f64(p.x), N::from_f64(p.y)]
}

/// `a - b` of two vectors.
fn diff<N: Ring>(a: &[N; 2], b: &[N; 2]) -> [N; 2] {
    [a[0].sub(&b[0]), a[1].sub(&b[1])]
}

/// `CGAL::scalar_product(a, b)`.
fn scalar_product<N: Ring>(a: &[N; 2], b: &[N; 2]) -> N {
    a[0].mul(&b[0]).add(&a[1].mul(&b[1]))
}

/// `a.x * b.y - a.y * b.x`.
fn cross<N: Ring>(a: &[N; 2], b: &[N; 2]) -> N {
    a[0].mul(&b[1]).sub(&a[1].mul(&b[0]))
}

/// `impl::calculate_parabolic_tangent_vector`: the tangent at `p` of the parabola of focus `f`
/// and directrix `u`-`v`, turned by `tangent_orientation`: the focus vector `(f - u) * |v - u|^2 -
/// (v - u) * ((v - u) . (p - u))` and its perpendicular, counter-clockwise for a left turn and
/// clockwise otherwise (CGAL's `perpendicular`, whose precondition `o != COLLINEAR` is off).
fn parabolic_tangent_vector<N: Ring>(
    p: CgalPoint,
    f: CgalPoint,
    u: CgalPoint,
    v: CgalPoint,
    tangent_orientation: CgalOrientation,
) -> [N; 2] {
    let (p, f, u, v) = (
        ring_point::<N>(p),
        ring_point::<N>(f),
        ring_point::<N>(u),
        ring_point::<N>(v),
    );
    let directrix_vec = diff(&v, &u);
    let directrix_vec_sqr_length = scalar_product(&directrix_vec, &directrix_vec);
    let along = scalar_product(&directrix_vec, &diff(&p, &u));
    let fu = diff(&f, &u);
    let focus_vec = [
        fu[0]
            .mul(&directrix_vec_sqr_length)
            .sub(&directrix_vec[0].mul(&along)),
        fu[1]
            .mul(&directrix_vec_sqr_length)
            .sub(&directrix_vec[1].mul(&along)),
    ];
    if tangent_orientation == CgalOrientation::LeftTurn {
        [focus_vec[1].neg(), focus_vec[0].clone()]
    } else {
        [focus_vec[1].clone(), focus_vec[0].neg()]
    }
}

/// `ParabolicTangentToSegmentOrientationPredicate`: the sign of the tangent at `p` against the
/// linear segment `p`-`q`.
fn tangent_to_segment<N: Ring>(
    p: CgalPoint,
    q: CgalPoint,
    f: CgalPoint,
    u: CgalPoint,
    v: CgalPoint,
    tangent_orientation: CgalOrientation,
) -> N {
    let tangent_vec = parabolic_tangent_vector::<N>(p, f, u, v, tangent_orientation);
    let linear_vec = diff(&ring_point::<N>(q), &ring_point::<N>(p));
    cross(&tangent_vec, &linear_vec)
}

/// `ParabolicTangentToParabolicTangentOrientationPredicate`: the sign of the first tangent at `p`
/// against the second.
#[allow(clippy::too_many_arguments)]
fn tangent_to_tangent<N: Ring>(
    p: CgalPoint,
    f_0: CgalPoint,
    u_0: CgalPoint,
    v_0: CgalPoint,
    tangent_orientation_0: CgalOrientation,
    f_1: CgalPoint,
    u_1: CgalPoint,
    v_1: CgalPoint,
    tangent_orientation_1: CgalOrientation,
) -> N {
    let tangent_vec_0 = parabolic_tangent_vector::<N>(p, f_0, u_0, v_0, tangent_orientation_0);
    let tangent_vec_1 = parabolic_tangent_vector::<N>(p, f_1, u_1, v_1, tangent_orientation_1);
    cross(&tangent_vec_0, &tangent_vec_1)
}

/// `ParabolicTangentToSegmentOrientation{}(p, q, f, u, v, o)`: the filtered predicate, whose
/// answer is the exact sign.
fn parabolic_tangent_to_segment_orientation(
    p: CgalPoint,
    q: CgalPoint,
    f: CgalPoint,
    u: CgalPoint,
    v: CgalPoint,
    o: CgalOrientation,
) -> CgalOrientation {
    CgalOrientation::from_sign(exact_sign(
        || tangent_to_segment::<Interval>(p, q, f, u, v, o),
        || tangent_to_segment::<Dyadic>(p, q, f, u, v, o),
    ))
}

/// `ParabolicTangentToParabolicTangentOrientation{}(...)`: the filtered predicate, whose answer
/// is the exact sign.
#[allow(clippy::too_many_arguments)]
fn parabolic_tangent_to_parabolic_tangent_orientation(
    p: CgalPoint,
    f_0: CgalPoint,
    u_0: CgalPoint,
    v_0: CgalPoint,
    o_0: CgalOrientation,
    f_1: CgalPoint,
    u_1: CgalPoint,
    v_1: CgalPoint,
    o_1: CgalOrientation,
) -> CgalOrientation {
    CgalOrientation::from_sign(exact_sign(
        || tangent_to_tangent::<Interval>(p, f_0, u_0, v_0, o_0, f_1, u_1, v_1, o_1),
        || tangent_to_tangent::<Dyadic>(p, f_0, u_0, v_0, o_0, f_1, u_1, v_1, o_1),
    ))
}

/// The exact orientation of three points (the exact kernel's `orientation`, which CGAL's sweep
/// decides intersections with).
fn exact_orientation(p: CgalPoint, q: CgalPoint, r: CgalPoint) -> i32 {
    fn det<N: Ring>(p: CgalPoint, q: CgalPoint, r: CgalPoint) -> N {
        let (p, q, r) = (ring_point::<N>(p), ring_point::<N>(q), ring_point::<N>(r));
        cross(&diff(&q, &p), &diff(&r, &p))
    }
    exact_sign(|| det::<Interval>(p, q, r), || det::<Dyadic>(p, q, r))
}

// ---------------------------------------------------------------- the diagram's edges

/// `struct ParabolicSegment`: a parabolic edge's focus, directrix, two points on it (its
/// vertices) and the side of the focus.
#[derive(Clone, Copy, Debug)]
struct ParabolicSegment {
    focus: Point,
    directrix: Line,
    /// `segment` (a `Linef`): `a` and `b`.
    segment: (Vec2d, Vec2d),
    is_focus_on_left: CgalOrientation,
}

/// An edge's `vertex0()`, which the C++ follows.
fn vertex0<'v>(vd: &'v VoronoiDiagram, edge: &VoronoiEdge) -> &'v VoronoiVertex {
    &vd.vertices()[edge
        .vertex0()
        .expect("VoronoiUtilsCgal: the C++ follows a null vertex0")]
}

/// An edge's `vertex1()`, which the C++ follows.
fn vertex1<'v>(vd: &'v VoronoiDiagram, edge: &VoronoiEdge) -> &'v VoronoiVertex {
    &vd.vertices()[edge
        .vertex1(vd.edges())
        .expect("VoronoiUtilsCgal: the C++ follows a null vertex1")]
}

/// `make_linef(edge)`: its two vertices as `Vec2d`s.
fn make_linef(vd: &VoronoiDiagram, edge: &VoronoiEdge) -> (Vec2d, Vec2d) {
    let v0 = vertex0(vd, edge);
    let v1 = vertex1(vd, edge);
    (vec2d(v0.x(), v0.y()), vec2d(v1.x(), v1.y()))
}

/// `get_parabolic_segment(edge, segment_begin, segment_end)`: the focus is the source point of
/// the edge's cell that holds a point (its own cell first), the directrix the source segment of
/// the other.
fn get_parabolic_segment<S: SegmentConcept<Point = Point>>(
    vd: &VoronoiDiagram,
    edge: &VoronoiEdge,
    segments: &[S],
) -> Result<ParabolicSegment, VoronoiError> {
    let edges = vd.edges();
    let cells = vd.cells();
    let left_cell = &cells[edge
        .cell()
        .expect("get_parabolic_segment: the C++ follows a null cell")];
    let twin = &edges[edge
        .twin()
        .expect("get_parabolic_segment: the C++ follows a null twin")];
    let right_cell = &cells[twin
        .cell()
        .expect("get_parabolic_segment: the C++ follows a null cell")];

    let focus_pt = VoronoiUtils::get_source_point(
        if left_cell.contains_point() {
            left_cell
        } else {
            right_cell
        },
        segments,
    )?;
    let directrix = VoronoiUtils::get_source_segment(
        if left_cell.contains_point() {
            right_cell
        } else {
            left_cell
        },
        segments,
    )?;
    let focus_side = orientation(
        to_cgal_point_vertex(vertex0(vd, edge)),
        to_cgal_point_vertex(vertex1(vd, edge)),
        to_cgal_point(focus_pt),
    )
    .opposite();

    let directrix_from = directrix.low();
    let directrix_to = directrix.high();
    Ok(ParabolicSegment {
        focus: focus_pt,
        directrix: Line::new(directrix_from, directrix_to),
        segment: make_linef(vd, edge),
        is_focus_on_left: focus_side,
    })
}

/// `orientation_of_two_edges(edge_a, edge_b, segment_begin, segment_end)`: how `edge_b` turns from
/// `edge_a` at their common first vertex.
fn orientation_of_two_edges<S: SegmentConcept<Point = Point>>(
    vd: &VoronoiDiagram,
    edge_a: &VoronoiEdge,
    edge_b: &VoronoiEdge,
    segments: &[S],
) -> Result<CgalOrientation, VoronoiError> {
    let mut orientation_ab;
    if edge_a.is_linear() && edge_b.is_linear() {
        orientation_ab = orientation(
            to_cgal_point_vertex(vertex0(vd, edge_a)),
            to_cgal_point_vertex(vertex1(vd, edge_a)),
            to_cgal_point_vertex(vertex1(vd, edge_b)),
        );
    } else if edge_a.is_curved() && edge_b.is_curved() {
        let parabolic_a = get_parabolic_segment(vd, edge_a, segments)?;
        let parabolic_b = get_parabolic_segment(vd, edge_b, segments)?;
        orientation_ab = parabolic_tangent_to_parabolic_tangent_orientation(
            to_cgal_point_vec(parabolic_a.segment.0),
            to_cgal_point(parabolic_a.focus),
            to_cgal_point(parabolic_a.directrix.a),
            to_cgal_point(parabolic_a.directrix.b),
            parabolic_a.is_focus_on_left,
            to_cgal_point(parabolic_b.focus),
            to_cgal_point(parabolic_b.directrix.a),
            to_cgal_point(parabolic_b.directrix.b),
            parabolic_b.is_focus_on_left,
        );
        return Ok(orientation_ab);
    } else {
        let linear_edge = if edge_a.is_curved() { edge_b } else { edge_a };
        let parabolic_edge = if edge_a.is_curved() { edge_a } else { edge_b };
        let parabolic = get_parabolic_segment(vd, parabolic_edge, segments)?;
        orientation_ab = parabolic_tangent_to_segment_orientation(
            to_cgal_point_vec(parabolic.segment.0),
            to_cgal_point_vertex(vertex1(vd, linear_edge)),
            to_cgal_point(parabolic.focus),
            to_cgal_point(parabolic.directrix.a),
            to_cgal_point(parabolic.directrix.b),
            parabolic.is_focus_on_left,
        );

        if edge_b.is_curved() {
            orientation_ab = orientation_ab.opposite();
        }
    }

    Ok(orientation_ab)
}

/// `check_if_three_edges_are_ccw(edge_first, edge_second, edge_third, ...)`: whether the three
/// edges from one vertex are in counter-clockwise order.
fn check_if_three_edges_are_ccw<S: SegmentConcept<Point = Point>>(
    vd: &VoronoiDiagram,
    edge_first: &VoronoiEdge,
    edge_second: &VoronoiEdge,
    edge_third: &VoronoiEdge,
    segments: &[S],
) -> Result<bool, VoronoiError> {
    let orientation_12 = orientation_of_two_edges(vd, edge_first, edge_second, segments)?;
    if orientation_12 == CgalOrientation::Collinear {
        // The first two edges are collinear, so the third edge must be on the right side on the
        // first of them.
        Ok(
            orientation_of_two_edges(vd, edge_first, edge_third, segments)?
                == CgalOrientation::RightTurn,
        )
    } else if orientation_12 == CgalOrientation::LeftTurn {
        // CCW oriented angle between vectors (common_pt, pt1) and (common_pt, pt2) is below PI.
        // So we need to check if test_pt isn't between them.
        let orientation1 = orientation_of_two_edges(vd, edge_first, edge_third, segments)?;
        let orientation2 = orientation_of_two_edges(vd, edge_second, edge_third, segments)?;
        Ok(orientation1 != CgalOrientation::LeftTurn || orientation2 != CgalOrientation::RightTurn)
    } else {
        // CCW oriented angle between vectors (common_pt, pt1) and (common_pt, pt2) is above PI.
        // So we need to check if test_pt is between them.
        let orientation1 = orientation_of_two_edges(vd, edge_first, edge_third, segments)?;
        let orientation2 = orientation_of_two_edges(vd, edge_second, edge_third, segments)?;
        Ok(orientation1 == CgalOrientation::RightTurn || orientation2 == CgalOrientation::LeftTurn)
    }
}

/// Whether an edge is finite with two finite vertices (the condition both checks filter by).
fn finite_with_finite_vertices(vd: &VoronoiDiagram, edge: &VoronoiEdge) -> bool {
    let edges = vd.edges();
    edge.is_finite(edges)
        && edge.vertex0().is_some()
        && edge.vertex1(edges).is_some()
        && VoronoiUtils::is_finite(vertex0(vd, edge))
        && VoronoiUtils::is_finite(vertex1(vd, edge))
}

/// A closed segment of the intersection check, in `double`s.
type CgalSegment = (CgalPoint, CgalPoint);

/// Whether `v` lies between `a` and `b`, both included.
fn between(v: f64, a: f64, b: f64) -> bool {
    (a <= v && v <= b) || (b <= v && v <= a)
}

/// Whether `p`, collinear with `s`, lies on `s` and is neither of its ends.
fn strictly_inside(p: CgalPoint, s: &CgalSegment) -> bool {
    p != s.0 && p != s.1 && between(p.x, s.0.x, s.1.x) && between(p.y, s.0.y, s.1.y)
}

/// Whether CGAL's sweep reports a point where `s` and `t` meet: a crossing of their interiors, or
/// an end of one inside the other (collinear overlaps included); not ends that are shared.
fn segments_report(s: &CgalSegment, t: &CgalSegment) -> bool {
    let (s_lo_x, s_hi_x) = if s.0.x <= s.1.x {
        (s.0.x, s.1.x)
    } else {
        (s.1.x, s.0.x)
    };
    let (s_lo_y, s_hi_y) = if s.0.y <= s.1.y {
        (s.0.y, s.1.y)
    } else {
        (s.1.y, s.0.y)
    };
    let (t_lo_x, t_hi_x) = if t.0.x <= t.1.x {
        (t.0.x, t.1.x)
    } else {
        (t.1.x, t.0.x)
    };
    let (t_lo_y, t_hi_y) = if t.0.y <= t.1.y {
        (t.0.y, t.1.y)
    } else {
        (t.1.y, t.0.y)
    };
    if s_hi_x < t_lo_x || t_hi_x < s_lo_x || s_hi_y < t_lo_y || t_hi_y < s_lo_y {
        return false;
    }
    let o1 = exact_orientation(s.0, s.1, t.0);
    let o2 = exact_orientation(s.0, s.1, t.1);
    let o3 = exact_orientation(t.0, t.1, s.0);
    let o4 = exact_orientation(t.0, t.1, s.1);
    if o1 * o2 < 0 && o3 * o4 < 0 {
        return true;
    }
    (o1 == 0 && strictly_inside(t.0, s))
        || (o2 == 0 && strictly_inside(t.1, s))
        || (o3 == 0 && strictly_inside(s.0, t))
        || (o4 == 0 && strictly_inside(s.1, t))
}

/// `class VoronoiUtilsCgal`: its static methods.
#[derive(Clone, Copy, Default, Debug)]
pub struct VoronoiUtilsCgal;

impl VoronoiUtilsCgal {
    /// `is_voronoi_diagram_planar_intersection(voronoi_diagram)`: whether no two finite linear
    /// edges with finite vertices intersect, as `CGAL::compute_intersection_points` of them
    /// reports intersections (the module's header says which). The edges' colours end at 0.
    #[must_use]
    pub fn is_voronoi_diagram_planar_intersection(voronoi_diagram: &VoronoiDiagram) -> bool {
        let edges = voronoi_diagram.edges();
        let mut segments: Vec<CgalSegment> = Vec::with_capacity(voronoi_diagram.num_edges());

        for edge in edges {
            if edge.color() != 0 {
                continue;
            }

            if edge.is_finite(edges)
                && edge.is_linear()
                && finite_with_finite_vertices(voronoi_diagram, edge)
            {
                segments.push((
                    to_cgal_point_vertex(vertex0(voronoi_diagram, edge)),
                    to_cgal_point_vertex(vertex1(voronoi_diagram, edge)),
                ));
                edge.set_color(1);
                let twin = edge
                    .twin()
                    .expect("is_voronoi_diagram_planar_intersection: a null twin");
                edges[twin].set_color(1);
            }
        }

        for edge in edges {
            edge.set_color(0);
        }

        for (i, s) in segments.iter().enumerate() {
            for t in &segments[i + 1..] {
                if segments_report(s, t) {
                    return false;
                }
            }
        }
        true
    }

    /// `is_voronoi_diagram_planar_angle(voronoi_diagram, segment_begin, segment_end)`: whether,
    /// around every vertex with more than two finite edges, each edge and its two neighbours turn
    /// counter-clockwise.
    ///
    /// # Errors
    ///
    /// The errors of `get_source_point` and `get_source_segment` on a parabolic edge's cells.
    pub fn is_voronoi_diagram_planar_angle<S: SegmentConcept<Point = Point>>(
        voronoi_diagram: &VoronoiDiagram,
        segments: &[S],
    ) -> Result<bool, VoronoiError> {
        let edges_all = voronoi_diagram.edges();
        for vertex in voronoi_diagram.vertices() {
            let mut edges: Vec<usize> = Vec::new();
            let start = vertex
                .incident_edge()
                .expect("is_voronoi_diagram_planar_angle: the C++ follows a null incident edge");
            let mut edge = start;

            loop {
                if finite_with_finite_vertices(voronoi_diagram, &edges_all[edge]) {
                    edges.push(edge);
                }

                edge = edges_all[edge]
                    .rot_next(edges_all)
                    .expect("is_voronoi_diagram_planar_angle: the C++ follows a null twin");
                if edge == start {
                    break;
                }
            }

            // Checking for CCW make sense for three and more edges.
            if edges.len() > 2 {
                let n = edges.len();
                for i in 0..n {
                    let prev_edge = if i == 0 { edges[n - 1] } else { edges[i - 1] };
                    let curr_edge = edges[i];
                    let next_edge = if i + 1 == n { edges[0] } else { edges[i + 1] };

                    if !check_if_three_edges_are_ccw(
                        voronoi_diagram,
                        &edges_all[prev_edge],
                        &edges_all[curr_edge],
                        &edges_all[next_edge],
                        segments,
                    )? {
                        return Ok(false);
                    }
                }
            }
        }

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::{CgalOrientation, CgalPoint, exact_orientation, orientation};
    use super::{parabolic_tangent_to_segment_orientation, segments_report, strictly_inside};

    fn p(x: f64, y: f64) -> CgalPoint {
        CgalPoint { x, y }
    }

    #[test]
    fn the_linear_orientation_is_in_doubles() {
        assert_eq!(
            orientation(p(0.0, 0.0), p(1.0, 0.0), p(0.0, 1.0)),
            CgalOrientation::LeftTurn
        );
        assert_eq!(
            orientation(p(0.0, 0.0), p(1.0, 0.0), p(0.0, -1.0)),
            CgalOrientation::RightTurn
        );
        // Not exact: (1 + 2^-30)^2 - (1 + 2^-29) is 2^-60, which the rounded products lose.
        let a = 1.0 + f64::from_bits(0x3E10_0000_0000_0000); // 1 + 2^-30
        let b = 1.0 + f64::from_bits(0x3E20_0000_0000_0000); // 1 + 2^-29
        let (o, q, r) = (p(0.0, 0.0), p(a, 1.0), p(b, a));
        assert_eq!(orientation(o, q, r), CgalOrientation::Collinear);
        assert_eq!(exact_orientation(o, q, r), 1);
    }

    #[test]
    fn a_parabola_tangent_turns_with_the_focus_side() {
        // The parabola of focus (0, 2) and directrix y = 0 has its apex at (0, 1), where the
        // tangent is horizontal: counter-clockwise of the focus vector it points to -x.
        let apex = p(0.0, 1.0);
        let f = p(0.0, 2.0);
        let (u, v) = (p(-10.0, 0.0), p(10.0, 0.0));
        let up = p(0.0, 5.0);
        assert_eq!(
            parabolic_tangent_to_segment_orientation(apex, up, f, u, v, CgalOrientation::LeftTurn),
            CgalOrientation::RightTurn
        );
        assert_eq!(
            parabolic_tangent_to_segment_orientation(apex, up, f, u, v, CgalOrientation::RightTurn),
            CgalOrientation::LeftTurn
        );
        // Collinear with the tangent.
        assert_eq!(
            parabolic_tangent_to_segment_orientation(
                apex,
                p(3.0, 1.0),
                f,
                u,
                v,
                CgalOrientation::LeftTurn
            ),
            CgalOrientation::Collinear
        );
    }

    #[test]
    fn shared_ends_are_not_intersections() {
        let s = (p(0.0, 0.0), p(2.0, 0.0));
        assert!(!segments_report(&s, &(p(2.0, 0.0), p(3.0, 1.0))));
        assert!(!segments_report(&s, &(p(2.0, 0.0), p(4.0, 0.0))));
        assert!(!segments_report(&s, &s));
        assert!(segments_report(&s, &(p(1.0, 0.0), p(1.0, 1.0))));
        assert!(segments_report(&s, &(p(1.0, -1.0), p(1.0, 1.0))));
        assert!(segments_report(&s, &(p(1.0, 0.0), p(3.0, 0.0))));
        assert!(!segments_report(&s, &(p(3.0, 0.0), p(4.0, 0.0))));
        assert!(strictly_inside(p(1.0, 0.0), &s) && !strictly_inside(p(0.0, 0.0), &s));
    }
}
