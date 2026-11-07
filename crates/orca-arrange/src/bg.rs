// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Boost code: Boost.Geometry of Boost 1.84.0, translated on 2026-11-07 from
// boost/geometry/strategy/cartesian/side_by_triangle.hpp, strategies/cartesian/
// point_in_poly_winding.hpp, algorithms/detail/within/point_in_geometry.hpp,
// views/detail/closed_clockwise_view.hpp, arithmetic/determinant.hpp and util/math.hpp
// (equals_factor_policy). Copyright (c) 2007-2015 Barend Gehrels, Copyright (c) 2008-2015 Bruno
// Lalande, Copyright (c) 2009-2015 Mateusz Loskot, Copyright (c) 2013-2014 Adam Wulkiewicz and
// modifications copyright (c) 2013-2023 Oracle and/or its affiliates, under the Boost Software
// License, version 1.0 (licenses/BSL-1.0.txt).

//! The Boost.Geometry predicates libnest2d's libslic3r backend calls (`utils/boost_alg.hpp`),
//! for its rings: open, declared counter-clockwise, with 64-bit coordinates.
//!
//! - [`point_in_shape`] is `detail::within::point_in_geometry` with the cartesian winding
//!   strategy: -1 outside, 0 on the boundary, 1 inside. `within(point, polygon)` is 1 and
//!   `touches(point, polygon)` is 0 of it. Each ring is walked as `closed_clockwise_view` gives
//!   it, the closing point appended and the order reversed, because the side test is not
//!   symmetric in floating point.
//! - The side test is `side_by_triangle`: the coordinate differences in 64-bit integers, then
//!   `double`, the determinant in `double`, and zero within `DBL_EPSILON` times the largest
//!   difference (at least 1).
//! - [`within_shape_shape`] stands for `within(polygon, polygon)`, which Boost computes with its
//!   relate (DE-9IM) machinery. It is not translated: this is the exact predicate (every point of
//!   the first polygon in the closed second one, their interiors meeting), in 128-bit integers.
//!   It agrees with Boost wherever Boost's double side tests are exact; only an irregular bed (a
//!   bed that is neither a box nor a circle) reaches it (`parity/HAZARDS.md`).

use orca_geom::expolygon::ExPolygon;
use orca_geom::point::Point;

/// `side_by_triangle<>::apply(p1, p2, p)`: 1 when `p` is left of `p1 -> p2`, -1 right, 0 on it.
#[must_use]
pub fn side_by_triangle(p1: Point, p2: Point, p: Point) -> i32 {
    let dx = p2.x.wrapping_sub(p1.x) as f64;
    let dy = p2.y.wrapping_sub(p1.y) as f64;
    let dpx = p.x.wrapping_sub(p1.x) as f64;
    let dpy = p.y.wrapping_sub(p1.y) as f64;
    let greatest = orca_stl::max(
        orca_stl::max(orca_stl::max(dx.abs(), dy.abs()), dpx.abs()),
        dpy.abs(),
    );
    let factor = if greatest < 1.0 { 1.0 } else { greatest };
    let s = dx * dpy - dy * dpx;
    let equals_zero = s == 0.0 || (s.is_finite() && s.abs() <= f64::EPSILON * factor);
    if equals_zero {
        0
    } else if s > 0.0 {
        1
    } else {
        -1
    }
}

/// The winding strategy's counter.
struct Counter {
    count: i32,
    touches: bool,
}

/// `cartesian_winding_base::apply` for one segment; false stops the walk.
fn winding_apply(point: Point, s1: Point, s2: Point, state: &mut Counter) -> bool {
    let (px, s1x, s2x) = (point.x, s1.x, s2.x);
    let eq1 = s1x == px;
    let eq2 = s2x == px;
    let count = if eq1 && eq2 {
        let (py, s1y, s2y) = (point.y, s1.y, s2.y);
        if (s1y <= py && s2y >= py) || (s2y <= py && s1y >= py) {
            state.touches = true;
        }
        0
    } else if eq1 {
        if s2x > px { 1 } else { -1 }
    } else if eq2 {
        if s1x > px { -1 } else { 1 }
    } else if s1x < px && s2x > px {
        2
    } else if s2x < px && s1x > px {
        -2
    } else {
        0
    };
    if count != 0 {
        let side = if count == 1 || count == -1 {
            let se = if eq1 { s1 } else { s2 };
            if point.y == se.y {
                0
            } else if point.y < se.y {
                -count
            } else {
                count
            }
        } else {
            side_by_triangle(s1, s2, point)
        };
        if side == 0 {
            state.touches = true;
            state.count = 0;
            return false;
        }
        if side * count > 0 {
            state.count += count;
        }
    }
    !state.touches
}

/// `point_in_geometry` of a ring: -1 for fewer than three points; else the winding walk over
/// the ring closed and reversed (`p0, p[n-1], ..., p1, p0`).
#[must_use]
pub fn point_in_ring(point: Point, ring: &[Point]) -> i32 {
    let n = ring.len();
    if n < 3 {
        return -1;
    }
    let at = |k: usize| {
        if k == 0 || k == n {
            ring[0]
        } else {
            ring[n - k]
        }
    };
    let mut state = Counter {
        count: 0,
        touches: false,
    };
    for k in 0..n {
        if !winding_apply(point, at(k), at(k + 1), &mut state) {
            break;
        }
    }
    if state.touches {
        0
    } else if state.count == 0 {
        -1
    } else {
        1
    }
}

/// `point_in_geometry` of a polygon: the exterior ring's code; inside it, the first hole the
/// point is not outside of decides (inside a hole is outside, on a hole's ring is on the
/// boundary).
#[must_use]
pub fn point_in_shape(point: Point, sh: &ExPolygon) -> i32 {
    let code = point_in_ring(point, &sh.contour.points);
    if code == 1 {
        for h in &sh.holes {
            let interior = point_in_ring(point, &h.points);
            if interior != -1 {
                return -interior;
            }
        }
    }
    code
}

/// `boost::geometry::within(point, polygon)`.
#[must_use]
pub fn within_point(point: Point, sh: &ExPolygon) -> bool {
    point_in_shape(point, sh) == 1
}

/// `boost::geometry::touches(point, polygon)`.
#[must_use]
pub fn touches_point(point: Point, sh: &ExPolygon) -> bool {
    point_in_shape(point, sh) == 0
}

type P2 = (i128, i128);

fn p2(p: Point) -> P2 {
    (i128::from(p.x), i128::from(p.y))
}

fn orient(a: P2, b: P2, c: P2) -> i128 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

/// Exact point-in-ring over the implicitly closed ring: -1 outside, 0 on it, 1 inside.
fn exact_in_ring(p: P2, ring: &[P2]) -> i32 {
    let n = ring.len();
    if n < 3 {
        return -1;
    }
    let mut inside = false;
    for i in 0..n {
        let a = ring[i];
        let b = ring[(i + 1) % n];
        if orient(a, b, p) == 0
            && orca_stl::min(a.0, b.0) <= p.0
            && p.0 <= orca_stl::max(a.0, b.0)
            && orca_stl::min(a.1, b.1) <= p.1
            && p.1 <= orca_stl::max(a.1, b.1)
        {
            return 0;
        }
        if (a.1 > p.1) != (b.1 > p.1) {
            let num = (a.0 - p.0) * (b.1 - a.1) + (p.1 - a.1) * (b.0 - a.0);
            let den = b.1 - a.1;
            if num != 0 && ((num > 0) == (den > 0)) {
                inside = !inside;
            }
        }
    }
    if inside { 1 } else { -1 }
}

/// Exact point-in-polygon, holes as Boost treats them.
fn exact_in_shape(p: P2, rings: &[Vec<P2>]) -> i32 {
    let code = exact_in_ring(p, &rings[0]);
    if code == 1 {
        for h in &rings[1..] {
            let interior = exact_in_ring(p, h);
            if interior != -1 {
                return -interior;
            }
        }
    }
    code
}

/// `boost::geometry::within(polygon, polygon)` as the exact predicate: the first shape's
/// contour (libnest2d's shapes here have no holes) has non-zero area, every vertex of it is in
/// the closed second shape, no edge of it properly crosses a ring of the second, every piece of
/// its edges between the second shape's vertices on them has its midpoint in the closed second
/// shape, and no vertex of the second shape is inside the first.
#[must_use]
pub fn within_shape_shape(a: &ExPolygon, b: &ExPolygon) -> bool {
    let ac: Vec<P2> = a.contour.points.iter().map(|&p| p2(p)).collect();
    let n = ac.len();
    if n < 3 {
        return false;
    }
    let mut twice_area = 0i128;
    for i in 0..n {
        let (p, q) = (ac[i], ac[(i + 1) % n]);
        twice_area += p.0 * q.1 - q.0 * p.1;
    }
    if twice_area == 0 {
        return false;
    }
    let mut b_rings: Vec<Vec<P2>> = vec![b.contour.points.iter().map(|&p| p2(p)).collect()];
    b_rings.extend(
        b.holes
            .iter()
            .map(|h| h.points.iter().map(|&p| p2(p)).collect()),
    );
    for &v in &ac {
        if exact_in_shape(v, &b_rings) < 0 {
            return false;
        }
    }
    let doubled: Vec<Vec<P2>> = b_rings
        .iter()
        .map(|r| r.iter().map(|&(x, y)| (2 * x, 2 * y)).collect())
        .collect();
    for i in 0..n {
        let (a1, a2) = (ac[i], ac[(i + 1) % n]);
        let d = (a2.0 - a1.0, a2.1 - a1.1);
        let len2 = d.0 * d.0 + d.1 * d.1;
        let mut cuts: Vec<(i128, P2)> = Vec::new();
        for ring in &b_rings {
            let m = ring.len();
            for j in 0..m {
                let (b1, b2) = (ring[j], ring[(j + 1) % m]);
                let o1 = orient(a1, a2, b1).signum();
                let o2 = orient(a1, a2, b2).signum();
                let o3 = orient(b1, b2, a1).signum();
                let o4 = orient(b1, b2, a2).signum();
                if o1 * o2 < 0 && o3 * o4 < 0 {
                    return false;
                }
                if o1 == 0 {
                    let t = (b1.0 - a1.0) * d.0 + (b1.1 - a1.1) * d.1;
                    if t > 0 && t < len2 {
                        cuts.push((t, b1));
                    }
                }
            }
        }
        orca_stl::sort_by_key(&mut cuts, |c| c.0);
        let mut prev = a1;
        for &(_, c) in cuts.iter().chain(std::iter::once(&(len2, a2))) {
            let mid = (prev.0 + c.0, prev.1 + c.1);
            if exact_in_shape(mid, &doubled) < 0 {
                return false;
            }
            prev = c;
        }
    }
    let a_rings = vec![ac];
    for ring in &b_rings {
        for &v in ring {
            if exact_in_shape(v, &a_rings) == 1 {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use orca_geom::polygon::Polygon;

    fn square(x0: i64, y0: i64, x1: i64, y1: i64) -> ExPolygon {
        ExPolygon::new(Polygon::new(vec![
            Point::new(x0, y0),
            Point::new(x1, y0),
            Point::new(x1, y1),
            Point::new(x0, y1),
        ]))
    }

    #[test]
    fn point_codes() {
        let s = square(0, 0, 10, 10);
        assert_eq!(point_in_shape(Point::new(5, 5), &s), 1);
        assert_eq!(point_in_shape(Point::new(10, 5), &s), 0);
        assert_eq!(point_in_shape(Point::new(0, 0), &s), 0);
        assert_eq!(point_in_shape(Point::new(11, 5), &s), -1);
        let mut h = s.clone();
        h.holes.push(Polygon::new(vec![
            Point::new(2, 2),
            Point::new(2, 8),
            Point::new(8, 8),
            Point::new(8, 2),
        ]));
        assert_eq!(point_in_shape(Point::new(5, 5), &h), -1);
        assert_eq!(point_in_shape(Point::new(2, 5), &h), 0);
        assert_eq!(point_in_shape(Point::new(1, 5), &h), 1);
    }

    #[test]
    fn polygon_within_polygon() {
        let big = square(0, 0, 10, 10);
        assert!(within_shape_shape(&square(1, 1, 9, 9), &big));
        assert!(within_shape_shape(&square(0, 0, 10, 10), &big));
        assert!(!within_shape_shape(&square(5, 5, 11, 9), &big));
        let notch = ExPolygon::new(Polygon::new(vec![
            Point::new(0, 0),
            Point::new(10, 0),
            Point::new(10, 10),
            Point::new(6, 10),
            Point::new(5, 5),
            Point::new(4, 10),
            Point::new(0, 10),
        ]));
        assert!(!within_shape_shape(&square(1, 1, 9, 9), &notch));
        assert!(within_shape_shape(&square(1, 1, 9, 4), &notch));
    }
}
