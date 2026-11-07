// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.
//
// geom.sop1's Geometry records (parity/SOP1.md, "geom.sop1"), which orca-geom's tests/probes.rs replays:
// GPNT (Point, Line and the Geometry predicates), GPLY (Polygon, Polyline, MultiPoint, BoundingBox and
// the convex hull), GCLU (ClipperUtils and ExPolygon) and GXFM (the transform builders,
// BoundingBoxf3::transformed, the 3D and convex polygon helpers and make_circle). Each record holds
// its inputs and then the engine's results, so the replay needs no generator.
#include "../probe.hpp"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <functional>
#include <random>
#include <vector>

#include "libslic3r/BoundingBox.hpp"
#include "libslic3r/ClipperUtils.hpp"
#include "libslic3r/ExPolygon.hpp"
#include "libslic3r/Geometry.hpp"
#include "libslic3r/Geometry/ConvexHull.hpp"
#include "libslic3r/Line.hpp"
#include "libslic3r/MultiPoint.hpp"
#include "libslic3r/Point.hpp"
#include "libslic3r/Polygon.hpp"
#include "libslic3r/Polyline.hpp"

namespace probe {

namespace {

using namespace Slic3r;
namespace CL = Slic3r::ClipperLib;
using Rng = std::mt19937;

int64_t rnd(Rng &g, int64_t lo, int64_t hi) { return std::uniform_int_distribution<int64_t>(lo, hi)(g); }
double  real(Rng &g, double lo, double hi) { return std::uniform_real_distribution<double>(lo, hi)(g); }

void put(Payload &p, const Point &q) { p.i64(q.x()).i64(q.y()); }
void put(Payload &p, const Vec2d &v) { p.f64(v.x()).f64(v.y()); }
void put(Payload &p, const Vec3d &v) { p.f64(v.x()).f64(v.y()).f64(v.z()); }
void put(Payload &p, const Points &pts)
{
    p.u32(uint32_t(pts.size()));
    for (const Point &q : pts)
        put(p, q);
}
void put(Payload &p, const Polygons &polys)
{
    p.u32(uint32_t(polys.size()));
    for (const Polygon &q : polys)
        put(p, q.points);
}
void put(Payload &p, const Polylines &pls)
{
    p.u32(uint32_t(pls.size()));
    for (const Polyline &q : pls)
        put(p, q.points);
}
void put(Payload &p, const ExPolygons &es)
{
    p.u32(uint32_t(es.size()));
    for (const ExPolygon &e : es) {
        put(p, e.contour.points);
        put(p, e.holes);
    }
}
void put(Payload &p, const BoundingBox &b)
{
    put(p, b.min);
    put(p, b.max);
    p.boolean(b.defined);
}
void put(Payload &p, const Transform3d &t)
{
    for (int r = 0; r < 4; ++r)
        for (int c = 0; c < 4; ++c)
            p.f64(t.matrix()(r, c));
}
void put(Payload &p, const std::vector<Vec2d> &v)
{
    p.u32(uint32_t(v.size()));
    for (const Vec2d &q : v)
        put(p, q);
}
void put(Payload &p, const Pointf3s &v)
{
    p.u32(uint32_t(v.size()));
    for (const Vec3d &q : v)
        put(p, q);
}
void put_opt(Payload &p, bool ok, const Point &q)
{
    p.boolean(ok);
    put(p, ok ? q : Point(0, 0));
}

// A coordinate range: tiny, small, medium, large or past orient's 32-bit branch.
int64_t range(Rng &g)
{
    switch (rnd(g, 0, 7)) {
    case 0: return 50;
    case 1:
    case 2: return 1000;
    case 3:
    case 4: return 1000000;
    case 5: return 100000000;
    case 6: return 10000000000LL;
    default: return 30000;
    }
}

Point rpoint(Rng &g, int64_t r) { return Point(rnd(g, -r, r), rnd(g, -r, r)); }

// A star-shaped polygon (simple, counter-clockwise) around a random center, a clockwise star, or a
// point soup that may cross itself; now and then with a repeated or a collinear point.
Points rpoly(Rng &g, int64_t r, size_t min_points = 3)
{
    const size_t n    = size_t(rnd(g, int64_t(min_points), 24));
    const int    kind = int(rnd(g, 0, 5));
    Points       pts;
    if (kind <= 3) {
        const Point c = rpoint(g, r / 2);
        for (size_t i = 0; i < n; ++i) {
            const double a  = 2. * PI * (double(i) + real(g, 0., 0.8)) / double(n);
            const double rr = real(g, 0.1, 1.0) * double(r);
            pts.emplace_back(c.x() + int64_t(std::llround(rr * std::cos(a))), c.y() + int64_t(std::llround(rr * std::sin(a))));
        }
        if (kind == 3)
            std::reverse(pts.begin(), pts.end());
    } else {
        for (size_t i = 0; i < n; ++i)
            pts.push_back(rpoint(g, r));
    }
    if (rnd(g, 0, 4) == 0 && pts.size() > 2)
        pts.insert(pts.begin() + 1, pts[1]);
    if (rnd(g, 0, 4) == 0 && pts.size() > 2)
        pts.insert(pts.begin() + 2, Point((pts[1].x() + pts[2].x()) / 2, (pts[1].y() + pts[2].y()) / 2));
    return pts;
}

Vec2d unscaled_point(const Point &q) { return Vec2d(double(q.x()) * SCALING_FACTOR, double(q.y()) * SCALING_FACTOR); }

// GPNT: inputs p, a, b, c, d, center, angle, dmax; then the results.
void gpnt(Payload &p, const Point &pt, const Point &a, const Point &b, const Point &c, const Point &d, const Point &center, double angle, double dmax)
{
    put(p, pt);
    put(p, a);
    put(p, b);
    put(p, c);
    put(p, d);
    put(p, center);
    p.f64(angle).f64(dmax);

    Point q = pt;
    q.rotate(angle);
    put(p, q);
    q = pt;
    q.rotate(angle, center);
    put(p, q);
    p.f64(pt.ccw(a, b)).f64(pt.ccw_angle(a, b));
    const Line l(a, b), m(c, d);
    put(p, pt.projection_onto(l));
    p.f64(l.distance_to(pt)).f64(Line::distance_to_infinite(pt, a, b)).f64(l.perp_distance_to(pt));
    Point ip;
    bool  ok = l.intersection(m, &ip);
    put_opt(p, ok, ip);
    ok = l.intersection_infinite(m, &ip);
    put_opt(p, ok, ip);
    p.f64(l.direction()).f64(l.orientation()).f64(l.atan2_());
    p.boolean(l.parallel_to(m)).boolean(l.perpendicular_to(m)).boolean(l.parallel_to(angle)).boolean(l.perpendicular_to(angle));
    put(p, l.midpoint());
    put(p, Point(l.normal()));
    Line e = l;
    e.extend(dmax);
    put(p, e.a);
    put(p, e.b);
    double len = 0.;
    ok         = l.overlap(m, len);
    p.boolean(ok).f64(ok ? len : 0.);
    p.i32(int32_t(Geometry::orient(a, b, pt)));
    p.boolean(Geometry::segments_intersect(a, b, c, d));
    const double angle2 = m.direction();
    p.boolean(Geometry::directions_parallel(angle, angle2, dmax * 1e-9)).boolean(Geometry::directions_perpendicular(angle, angle2, dmax * 1e-9));
    double r = angle * 7.;
    Geometry::to_range_pi_pi(r);
    p.f64(r).f64(Geometry::angle_to_0_2PI(angle));
    Point closest;
    p.f64(l.distance_to_squared(pt, &closest));
    put(p, closest);
    p.f64(l.distance_to_infinite_squared(pt, &closest));
    put(p, closest);
    p.i32(pt.nearest_point_index(Points{a, b, c, d}));
    Vec2d res(0., 0.);
    ok = Geometry::ray_ray_intersection(a.cast<double>(), (b - a).cast<double>(), c.cast<double>(), (d - c).cast<double>(), res);
    p.boolean(ok);
    put(p, ok ? res : Vec2d(0., 0.));
    res = Vec2d(0., 0.);
    ok  = Geometry::segment_segment_intersection(a.cast<double>(), (b - a).cast<double>(), c.cast<double>(), (d - c).cast<double>(), res);
    p.boolean(ok);
    put(p, ok ? res : Vec2d(0., 0.));
    put(p, Geometry::foot_pt(l, pt));
    p.f64(Geometry::ray_point_distance(l, pt));
}

// GPLY: inputs points, q, line a b, tol, frac, angle, factor; then the results.
void gply(Payload &p, const Points &pts, const Point &q, const Point &a, const Point &b, double tol, double frac, double angle, double factor)
{
    put(p, pts);
    put(p, q);
    put(p, a);
    put(p, b);
    p.f64(tol).f64(frac).f64(angle).f64(factor);

    const Polygon poly(pts);
    p.f64(poly.area()).f64(poly.length());
    put(p, poly.centroid());
    p.boolean(poly.is_counter_clockwise()).boolean(Geometry::is_ccw(poly));
    p.boolean(poly.contains(q)).boolean(Slic3r::contains(poly, q, false));
    put(p, poly.point_projection(q));
    Polygon dp = poly;
    dp.douglas_peucker(tol);
    put(p, dp.points);
    put(p, poly.simplify(tol));
    Polygons tri;
    poly.triangulate_convex(&tri);
    put(p, tri);
    p.boolean(polygon_is_convex(poly));
    Polygon rc = poly;
    remove_collinear(rc);
    put(p, rc.points);
    Polygon rs = poly;
    p.boolean(remove_sticks(rs));
    put(p, rs.points);
    Polygon rn = poly;
    p.boolean(remove_same_neighbor(rn));
    put(p, rn.points);
    const BoundingBox bb = poly.bounding_box();
    put(p, bb);
    const double dist = frac * std::max(1., poly.length()) * 0.25;
    BoundingBox  bo   = bb;
    bo.offset(dist);
    put(p, bo);
    BoundingBox bs = bb;
    bs.scale(factor);
    put(p, bs);
    put(p, bb.center());
    put(p, bb.size());
    p.f64(bb.radius()).boolean(bb.contains(q));
    put(p, bb.rotated(angle));
    put(p, bb.polygon().points);
    const Line l(a, b);
    Point      ip;
    bool       ok = poly.first_intersection(l, &ip);
    put_opt(p, ok, ip);
    Points ips;
    p.boolean(poly.intersections(l, &ips));
    put(p, ips);
    ok = poly.intersection(l, &ip);
    put_opt(p, ok, ip);
    p.boolean(poly.on_boundary(q, dist));
    put(p, q.projection_onto(poly));

    const Polyline pl(pts);
    const double   pl_len = pl.length();
    p.f64(pl_len);
    Polyline ce = pl;
    ce.clip_end(dist);
    put(p, ce.points);
    Polyline cs = pl;
    cs.clip_start(dist);
    put(p, cs.points);
    Polyline ee = pl;
    ee.extend_end(dist);
    put(p, ee.points);
    const double spacing = std::max(pl_len / (2. + 18. * frac), 1.);
    put(p, pl.equally_spaced_points(spacing));
    put(p, pl.equally_spaced_lines(spacing));
    Polyline si = pl;
    si.simplify(tol);
    put(p, si.points);
    Polyline p1, p2;
    ok = pl.split_at_length(pl_len * frac, &p1, &p2);
    p.boolean(ok);
    put(p, ok ? p1.points : Points());
    put(p, ok ? p2.points : Points());
    put(p, pl.leftmost_point());
    p.boolean(pl.is_straight());
    const std::pair<int, Point> fp = foot_pt(pts, q);
    p.i32(fp.first);
    put(p, fp.second);
    p.boolean(has_duplicate_points(pts));

    put(p, Geometry::convex_hull(pts).points);
    put(p, polygons_simplify(Polygons{poly}, tol, true));
    put(p, get_extents_rotated(pts, angle));
    Polygon ro = poly;
    ro.rotate(angle, q);
    put(p, ro.points);
    Polygon rr = poly;
    rr.rotate(angle);
    put(p, rr.points);
    Polygon sc = poly;
    sc.scale(factor);
    put(p, sc.points);
    put(p, MultiPoint::_douglas_peucker(pts, tol));
    put(p, poly.equally_spaced_points(spacing));
}

// GCLU: inputs A, B, delta, join, miter limit, safety, polyline, q; the results; for a non-empty
// union_ex(A) the variable offset deltas of its first expolygon (inputs drawn once it is known) and
// their results.
void gclu(Payload &p, const Polygons &A, const Polygons &B, float delta, int join, double ml, bool safety, const Polyline &pl, const Point &q, Rng &g)
{
    put(p, A);
    put(p, B);
    const CL::JoinType jt = join == 0 ? CL::jtSquare : join == 1 ? CL::jtRound : CL::jtMiter;
    p.f32(delta).u8(uint8_t(join)).f64(ml).boolean(safety);
    put(p, pl.points);
    put(p, q);
    const ApplySafetyOffset so = safety ? ApplySafetyOffset::Yes : ApplySafetyOffset::No;

    put(p, offset(A, delta, jt, ml));
    put(p, offset_ex(A, delta, jt, ml));
    put(p, union_(A));
    const ExPolygons EA = union_ex(A);
    const ExPolygons EB = union_ex(B);
    put(p, EA);
    put(p, diff(A, B, so));
    put(p, diff_ex(A, B, so));
    put(p, intersection(A, B, so));
    put(p, intersection_ex(A, B, so));
    put(p, xor_ex(EA, EB, so));
    put(p, diff_ex(EA, EB, so));
    put(p, offset_ex(EA, delta, jt, ml));
    put(p, offset(EA, delta, jt, ml));
    put(p, offset2_ex(EA, delta, -0.5f * delta, jt, ml));
    const float pos = std::fabs(delta) + 1.f;
    put(p, closing(A, pos, jt, ml));
    put(p, opening(A, pos, jt, ml));
    put(p, diff_pl(Polylines{pl}, A));
    put(p, intersection_pl(Polylines{pl}, A));
    put(p, diff_pl(A, B));
    put(p, offset(pl, pos, DefaultLineJoinType, DefaultLineMiterLimit, DefaultEndType));
    put(p, simplify_polygons(A));
    put(p, union_safety_offset(A));
    put(p, contour_to_polygons(A, 2.f * pos, jt, ml));
    put(p, diff_clipped(A, B, so));
    put(p, intersection_clipped(A, B, so));
    Polygons AB = A;
    AB.insert(AB.end(), B.begin(), B.end());
    put(p, top_level_islands(AB));
    put(p, ClipperUtils::clip_clipper_polygons_with_subject_bbox(B, get_extents(A)));
    if (!EA.empty()) {
        const ExPolygon &e = EA.front();
        p.f64(e.area()).boolean(e.contains(q)).boolean(e.is_valid());
        put(p, e.point_projection(q));
        p.boolean(!EB.empty() && e.overlaps(EB.front()));
        std::vector<std::vector<float>> outer, inner;
        for (size_t i = 0; i < e.num_contours(); ++i) {
            std::vector<float> o, n;
            for (size_t k = 0; k < e.contour_or_hole(i).size(); ++k) {
                o.push_back(float(real(g, 0., double(pos))));
                n.push_back(-float(real(g, 0., double(pos))));
            }
            outer.push_back(o);
            inner.push_back(n);
        }
        for (const std::vector<std::vector<float>> *ds : {&outer, &inner})
            for (const std::vector<float> &v : *ds) {
                p.u32(uint32_t(v.size()));
                for (float f : v)
                    p.f32(f);
            }
        put(p, variable_offset_outer(e, outer, ml));
        put(p, variable_offset_inner(e, inner, ml));
        put(p, variable_offset_outer_ex(e, outer, ml));
        put(p, variable_offset_inner_ex(e, inner, ml));
    }
}

// GXFM: inputs t, r, s, m, 3D points, two 2D soups, radius, error, angle; then the results.
void gxfm(Payload &p, const Vec3d &t, const Vec3d &r, const Vec3d &s, const Vec3d &m, const Pointf3s &pts, const Points &sa, const Points &sb, double radius, double error,
          double angle)
{
    put(p, t);
    put(p, r);
    put(p, s);
    put(p, m);
    put(p, pts);
    put(p, sa);
    put(p, sb);
    p.f64(radius).f64(error).f64(angle);

    const Transform3d tr = Geometry::assemble_transform(t, r, s, m);
    put(p, tr);
    const Transform3d rot = Geometry::rotation_transform(r);
    put(p, rot);
    put(p, Geometry::extract_euler_angles(tr));
    const Eigen::Matrix<double, 3, 3, Eigen::DontAlign> lin = rot.matrix().block<3, 3>(0, 0);
    put(p, Geometry::extract_euler_angles(lin));
    const BoundingBoxf3 bb(pts);
    const BoundingBoxf3 bt = bb.transformed(tr);
    put(p, bt.min);
    put(p, bt.max);
    put(p, Geometry::convex_hull(pts));
    put(p, make_circle(radius, error).points);
    p.f64(Geometry::rad2deg_dir(angle));
    const Polygon ha = Geometry::convex_hull(sa), hb = Geometry::convex_hull(sb);
    put(p, ha.points);
    put(p, hb.points);
    const bool both = ha.size() >= 3 && hb.size() >= 3;
    p.boolean(both && Geometry::convex_polygons_intersect(ha, hb));
    std::vector<Vec2d> un;
    for (const Point &q : ha.points)
        un.push_back(unscaled_point(q));
    const auto tb = Geometry::decompose_convex_polygon_top_bottom(un);
    put(p, tb.first);
    put(p, tb.second);
    for (const Point &q : sb)
        p.boolean(!tb.first.empty() && Geometry::inside_convex_polygon(tb, unscaled_point(q)));
}

template<class Edge, class Random>
void record_cases(Sets &sets, const char (&kind)[5], const std::vector<Edge> &edges, size_t random, size_t committed_random, size_t committed_bytes, Random next)
{
    for (const Edge &e : edges) {
        Payload p;
        e(p);
        sets.full.add(kind, p);
        sets.committed.add(kind, p);
    }
    size_t taken = 0;
    for (size_t i = 0; i < random; ++i) {
        Payload p;
        next(p);
        sets.full.add(kind, p);
        if (taken < committed_random && p.bytes.size() <= committed_bytes) {
            sets.committed.add(kind, p);
            ++taken;
        }
    }
}

Points rect(int64_t x0, int64_t y0, int64_t x1, int64_t y1) { return Points{Point(x0, y0), Point(x1, y0), Point(x1, y1), Point(x0, y1)}; }

} // namespace

void record_geometry(Sets &sets)
{
    Rng g(0x6E0E7A1u);
    using Fn = std::function<void(Payload &)>;

    std::vector<Fn> e;
    e.push_back([](Payload &p) { gpnt(p, Point(5, 5), Point(0, 0), Point(0, 0), Point(0, 0), Point(0, 0), Point(0, 0), 0., 0.); });
    e.push_back([](Payload &p) { gpnt(p, Point(3, 4), Point(0, 0), Point(10, 0), Point(5, -5), Point(5, 5), Point(1, 1), PI / 2., 2.); });
    e.push_back([](Payload &p) { gpnt(p, Point(-7, 2), Point(0, 0), Point(10, 10), Point(20, 20), Point(30, 30), Point(-3, 8), -PI, 1e6); });
    e.push_back([](Payload &p) {
        gpnt(p, Point(5000000000LL, -5000000000LL), Point(-6000000000LL, 1), Point(6000000000LL, 3), Point(1, 6000000000LL), Point(2, -6000000000LL), Point(0, 0), 12.5, 3.);
    });
    e.push_back([](Payload &p) { gpnt(p, Point(1, 0), Point(0, 0), Point(0, 10), Point(0, 5), Point(0, 20), Point(0, 0), 0.49999999999999994, 0.); });
    record_cases(sets, "GPNT", e, 3000, 40, 1024, [&](Payload &p) {
        const int64_t r = range(g);
        Point         a = rpoint(g, r), b = rpoint(g, r), c = rpoint(g, r), d = rpoint(g, r);
        if (rnd(g, 0, 9) == 0)
            b = a;
        if (rnd(g, 0, 9) == 0)
            d = Point(c.x() + (b.x() - a.x()), c.y() + (b.y() - a.y()));
        if (rnd(g, 0, 9) == 0)
            c = Point(a.x() + (b.x() - a.x()) / 2, a.y() + (b.y() - a.y()) / 2);
        const Point  pt     = rpoint(g, r);
        const Point  center = rpoint(g, r);
        const double angle  = real(g, -20., 20.);
        gpnt(p, pt, a, b, c, d, center, angle, real(g, 0., double(r)));
    });

    e.clear();
    for (const Points &pts : {rect(0, 0, 10, 10), rect(0, 0, 1000000, 500000), Points{Point(0, 0), Point(10, 0), Point(20, 0)},
                              Points{Point(0, 0), Point(10, 0), Point(10, 0), Point(10, 10), Point(0, 0)}, Points{Point(0, 0), Point(10, 10), Point(10, 0), Point(0, 10)},
                              Points{Point(0, 0), Point(100, 0), Point(50, 0), Point(50, 80)}})
        e.push_back([pts](Payload &p) { gply(p, pts, Point(5, 5), Point(-5, 5), Point(15, 5), 1., 0.5, 0.3, 1.5); });
    record_cases(sets, "GPLY", e, 1500, 12, 4096, [&](Payload &p) {
        const int64_t r   = range(g);
        const Points  pts = rpoly(g, r);
        const Point   q = rpoint(g, r), a = rpoint(g, r), b = rpoint(g, r);
        const double  tol    = real(g, 0., double(r) * 0.2);
        const double  frac   = real(g, 0., 1.);
        const double  angle  = real(g, -7., 7.);
        const double  factor = real(g, -3., 3.);
        gply(p, pts, q, a, b, tol, frac, angle, factor);
    });

    e.clear();
    e.push_back([&g](Payload &p) {
        gclu(p, Polygons{Polygon(rect(0, 0, 1000000, 1000000))}, Polygons{Polygon(rect(500000, 500000, 1500000, 1500000))}, 100000.f, 2, 3., false,
             Polyline(Points{Point(-100000, 200000), Point(2000000, 200000)}), Point(250000, 250000), g);
    });
    e.push_back([&g](Payload &p) {
        Polygons a{Polygon(rect(0, 0, 1000000, 1000000)), Polygon(Points{Point(200000, 200000), Point(200000, 800000), Point(800000, 800000), Point(800000, 200000)})};
        gclu(p, a, Polygons{Polygon(rect(400000, -100000, 600000, 1100000))}, -50000.f, 1, 10000., true, Polyline(Points{Point(0, 500000), Point(1000000, 500000)}),
             Point(100000, 100000), g);
    });
    record_cases(sets, "GCLU", e, 600, 4, 12288, [&](Payload &p) {
        const int64_t r = range(g) >= 1000000 ? 1000000 : 100000;
        Polygons      A, B;
        const int     na = int(rnd(g, 1, 3));
        const int     nb = int(rnd(g, 1, 3));
        for (int i = 0; i < na; ++i)
            A.emplace_back(rpoly(g, r));
        for (int i = 0; i < nb; ++i)
            B.emplace_back(rpoly(g, r));
        const float  delta  = float(real(g, -0.2, 0.2) * double(r));
        const int    join   = int(rnd(g, 0, 2));
        const double ml     = join == 1 ? real(g, double(r) * 1e-4, double(r) * 1e-2) : real(g, 1., 5.);
        const bool   safety = rnd(g, 0, 1) == 1;
        const Points pl     = rpoly(g, r, 2);
        const Point  q      = rpoint(g, r);
        gclu(p, A, B, delta, join, ml, safety, Polyline(pl), q, g);
    });

    e.clear();
    e.push_back([](Payload &p) {
        gxfm(p, Vec3d(0., 0., 0.), Vec3d(0., 0., 0.), Vec3d(1., 1., 1.), Vec3d(1., 1., 1.), Pointf3s{Vec3d(0., 0., 0.), Vec3d(1., 1., 1.)}, rect(0, 0, 10, 10),
             rect(5, 5, 15, 15), 1000000., 1000., 0.);
    });
    e.push_back([](Payload &p) {
        gxfm(p, Vec3d(10., -5., 2.), Vec3d(PI / 2., 0., PI), Vec3d(2., 0.5, 1.), Vec3d(-1., 1., 1.), Pointf3s{Vec3d(-1., -2., -3.), Vec3d(4., 5., 6.), Vec3d(0., 1., 0.)},
             rect(0, 0, 10, 10), rect(20, 20, 30, 30), 5000., 4999., PI);
    });
    record_cases(sets, "GXFM", e, 1500, 16, 3072, [&](Payload &p) {
        Vec3d t, r, s, m;
        for (int i = 0; i < 3; ++i)
            t(i) = real(g, -500., 500.);
        for (int i = 0; i < 3; ++i)
            r(i) = real(g, -7., 7.);
        for (int i = 0; i < 3; ++i)
            s(i) = real(g, 0.01, 10.);
        for (int i = 0; i < 3; ++i)
            m(i) = rnd(g, 0, 1) ? 1. : -1.;
        Pointf3s  pts;
        const int n = int(rnd(g, 3, 20));
        for (int i = 0; i < n; ++i) {
            Vec3d v;
            for (int k = 0; k < 3; ++k)
                v(k) = real(g, -100., 100.);
            pts.push_back(v);
        }
        if (rnd(g, 0, 3) == 0)
            pts.push_back(Vec3d(pts[0].x(), pts[0].y(), pts[0].z() + 1.));
        // At most 1e8: rotcalip's int64_t dot products wrap for coordinates past about 1e9, and
        // convex_polygons_intersect's caliper walk can then loop forever (parity/HAZARDS.md).
        const int64_t rr = std::min(range(g), int64_t(100000000));
        Points        sa, sb;
        const int     ka = int(rnd(g, 3, 16));
        const int     kb = int(rnd(g, 3, 16));
        const Point   ca = rpoint(g, rr);
        const Point   cb = rpoint(g, rr);
        for (int i = 0; i < ka; ++i) {
            const int64_t dx = rnd(g, -rr, rr), dy = rnd(g, -rr, rr);
            sa.emplace_back(ca.x() + dx, ca.y() + dy);
        }
        for (int i = 0; i < kb; ++i) {
            const int64_t dx = rnd(g, -rr, rr), dy = rnd(g, -rr, rr);
            sb.emplace_back(cb.x() + dx, cb.y() + dy);
        }
        const double radius = real(g, 1000., 10000000.);
        const double error  = real(g, radius * 1e-4, radius * 0.5);
        gxfm(p, t, r, s, m, pts, sa, sb, radius, error, real(g, -7., 7.));
    });
}

} // namespace probe
