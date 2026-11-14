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
// paths.sop1's records of the piece geom-1 (parity/sop1/paths.md), which orca-geom replays:
//
//   PSRZ  Geometry::rotation_xyz_diff and rotation_diff_z of two rotations.
//   PSIC  Geometry::is_point_inside_polygon_corner.
//   PSCV  Polygon::convex_points and concave_points at an angle threshold.
//   PSRC  Geometry::ray_circle_intersections_r2_lv2_c<double> (Geometry/Circle.hpp).
//   PSCI  Circle: try_create_circle of three points and of a point list, get_polar_radians,
//         get_closest_point, is_over_deviation, get_deviation_sum_squared,
//         get_closest_perpendicular_point and calc_tangential_vector.
//   PSAS  ArcSegment::try_create_arc of a point list, are_points_within_slice of the arc it made,
//         then reverse, is_point_inside, clip_start, clip_end and split_at of query points.
//   PSAC  An ArcSegment made by its constructor, are_points_within_slice of points on and off it,
//         the same queries as PSAS, calc_arc_radian, calc_arc_radius and calc_arc_length (floats)
//         and ray_intersects_segment.
//   PSAF  ArcFitter::do_arc_fitting and do_arc_fitting_and_simplify.
//   PSPF  Polyline's fitting results: simplify_by_fitting_arc, then reverse, clip_end, clip_start,
//         split_at_index, split_at, split_at_length and the appends, each with the fitting result
//         it leaves.
//   PSTP  ThickPolyline::thicklines and start_at_index.
//
// Each record holds its inputs, then the engine's results, so the replay needs no generator. The
// inputs are shapes made here (arcs, clean and noisy, straight runs, zigzags, stars and grids in
// domain A's scaled coordinates); none come from the corpus. Added 2026-11-13 by
// orca-o3-probe-paths.
#include "slic.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <utility>
#include <vector>

#include "libslic3r/Geometry.hpp"
#include "libslic3r/Geometry/Circle.hpp"

namespace probe {
namespace paths {

namespace {

using V2 = Eigen::Matrix<double, 2, 1, Eigen::DontAlign>;

void psrz(Payload &p, const Vec3d &from, const Vec3d &to)
{
    put(p, from);
    put(p, to);
    const Eigen::Quaterniond q = Geometry::rotation_xyz_diff(from, to);
    p.f64(q.w()).f64(q.x()).f64(q.y()).f64(q.z());
    p.f64(Geometry::rotation_diff_z(from, to));
}

void psic(Payload &p, const Point &a, const Point &b, const Point &c, const Point &q)
{
    put(p, a);
    put(p, b);
    put(p, c);
    put(p, q);
    p.boolean(Geometry::is_point_inside_polygon_corner(a, b, c, q));
}

void pscv(Payload &p, const Points &pts, double th)
{
    put(p, pts);
    p.f64(th);
    const Polygon poly(pts);
    put(p, poly.convex_points(th));
    put(p, poly.concave_points(th));
}

void psrc(Payload &p, double r2, double a, double b, double lv2, double c)
{
    p.f64(r2).f64(a).f64(b).f64(lv2).f64(c);
    std::pair<V2, V2> out(V2(0., 0.), V2(0., 0.));
    const int         n = Geometry::ray_circle_intersections_r2_lv2_c(r2, a, b, lv2, c, out);
    p.i32(n);
    put(p, Vec2d(out.first.x(), out.first.y()));
    put(p, Vec2d(out.second.x(), out.second.y()));
}

void psci(Payload &p, const Points &pts, double max_r, double tol, const Vec3f &pos, const Vec3f &cpos, bool ccw)
{
    put(p, pts);
    p.f64(max_r).f64(tol);
    put(p, pos);
    put(p, cpos);
    p.boolean(ccw);
    const size_t n = pts.size();
    Circle       c3;
    const bool   ok3 = Circle::try_create_circle(pts[0], pts[n / 2], pts[n - 1], max_r, c3);
    p.boolean(ok3);
    put(p, c3);
    Circle     cn;
    const bool okn = Circle::try_create_circle(pts, max_r, tol, cn);
    p.boolean(okn);
    put(p, cn);
    Circle c;
    if (okn)
        c = cn;
    else if (ok3)
        c = c3;
    else {
        Point p0 = pts[0];
        c        = Circle(p0, std::max(1., (pts[n - 1] - pts[0]).cast<double>().norm()));
    }
    put(p, c);
    for (const Point &q : pts)
        p.f64(c.get_polar_radians(q));
    for (const Point &q : pts)
        put(p, c.get_closest_point(q));
    p.boolean(c.is_over_deviation(pts, tol));
    double     sum = 0.;
    const bool okd = c.get_deviation_sum_squared(pts, tol, sum);
    p.boolean(okd).f64(sum);
    for (size_t i = 0; i + 1 < n; ++i) {
        Point      out(0, 0);
        const bool ok = Circle::get_closest_perpendicular_point(pts[i], pts[i + 1], c.center, out);
        p.boolean(ok);
        put(p, out);
    }
    put(p, Circle::calc_tangential_vector(pos, cpos, ccw));
}

// The queries of an arc (PSAS and PSAC): reverse() of a copy, then for each query point
// is_point_inside, clip_start and clip_end of a copy, and split_at.
void arc_follow(Payload &p, const ArcSegment &arc, const Points &queries)
{
    ArcSegment r = arc;
    p.boolean(r.reverse());
    put(p, r);
    for (const Point &q : queries) {
        p.boolean(arc.is_point_inside(q));
        ArcSegment s = arc;
        p.boolean(s.clip_start(q));
        put(p, s);
        ArcSegment t = arc;
        p.boolean(t.clip_end(q));
        put(p, t);
        ArcSegment u = arc, a1, a2;
        p.boolean(u.split_at(q, a1, a2));
        put(p, a1);
        put(p, a2);
    }
}

void psas(Payload &p, const Points &pts, double approx, double max_r, double tol, double pct, const Points &queries)
{
    put(p, pts);
    p.f64(approx).f64(max_r).f64(tol).f64(pct);
    put(p, queries);
    ArcSegment arc;
    const bool ok = ArcSegment::try_create_arc(pts, arc, approx, max_r, tol, pct);
    p.boolean(ok);
    put(p, arc);
    p.boolean(ok && ArcSegment::are_points_within_slice(arc, pts));
    arc_follow(p, arc, queries);
}

struct ArcCase
{
    Point  center, start, end;
    double radius = 0.;
    int    dir    = 1;
    Points slice, queries;
    Vec3f  s3{0.f, 0.f, 0.f}, e3{0.f, 0.f, 0.f}, c3{0.f, 0.f, 0.f};
    bool   ccw = true;
    Point  origin;
    Vec2d  ray{1., 0.};
    Point  sa, sb;
};

void psac(Payload &p, const ArcCase &k)
{
    put(p, k.center);
    p.f64(k.radius);
    put(p, k.start);
    put(p, k.end);
    p.u8(uint8_t(k.dir));
    put(p, k.slice);
    put(p, k.queries);
    put(p, k.s3);
    put(p, k.e3);
    put(p, k.c3);
    p.boolean(k.ccw);
    put(p, k.origin);
    put(p, k.ray);
    put(p, k.sa);
    put(p, k.sb);
    const ArcSegment arc(k.center, k.radius, k.start, k.end, ArcDirection(k.dir));
    put(p, arc);
    p.boolean(arc.is_valid() && ArcSegment::are_points_within_slice(arc, k.slice));
    arc_follow(p, arc, k.queries);
    p.f32(ArcSegment::calc_arc_radian(k.s3, k.e3, k.c3, k.ccw));
    p.f32(ArcSegment::calc_arc_radius(k.s3, k.c3));
    p.f32(ArcSegment::calc_arc_length(k.s3, k.e3, k.c3, k.ccw));
    p.boolean(ArcSegment::ray_intersects_segment(k.origin, k.ray, Line(k.sa, k.sb)));
}

void psaf(Payload &p, const Points &pts, double tol)
{
    put(p, pts);
    p.f64(tol);
    std::vector<PathFittingData> r;
    ArcFitter::do_arc_fitting(pts, r, tol);
    put(p, r);
    Points                       q = pts;
    std::vector<PathFittingData> r2;
    ArcFitter::do_arc_fitting_and_simplify(q, r2, tol);
    put(p, q);
    put(p, r2);
}

void pspf(Payload &p, const Points &pts, double tol, const Points &pts2, double tol2, const Point &q, uint64_t k, double dist, double frac)
{
    put(p, pts);
    p.f64(tol);
    put(p, pts2);
    p.f64(tol2);
    put(p, q);
    p.u64(k).f64(dist).f64(frac);
    Polyline pl(pts);
    pl.simplify_by_fitting_arc(tol);
    put(p, pl);
    {
        Polyline r = pl;
        r.reverse();
        put(p, r);
    }
    {
        Polyline r = pl;
        r.clip_end(dist);
        put(p, r);
    }
    {
        Polyline r = pl;
        r.clip_start(dist);
        put(p, r);
    }
    {
        Polyline   a, b;
        const bool ok = pl.split_at_index(size_t(k), &a, &b);
        p.boolean(ok);
        put(p, a);
        put(p, b);
    }
    {
        Point    qq = q;
        Polyline a, b;
        pl.split_at(qq, &a, &b);
        put(p, qq);
        put(p, a);
        put(p, b);
    }
    {
        Polyline   a, b;
        const bool ok = pl.split_at_length(pl.length() * frac, &a, &b);
        p.boolean(ok);
        put(p, a);
        put(p, b);
    }
    Polyline pl2(pts2);
    pl2.simplify_by_fitting_arc(tol2);
    put(p, pl2);
    {
        Polyline a = pl;
        a.append(pl2);
        put(p, a);
    }
    {
        Polyline a = pl, b = pl2;
        a.append(std::move(b));
        put(p, a);
        put(p, b);
    }
    {
        Polyline a = pl;
        a.append(q);
        put(p, a);
    }
    {
        Polyline a = pl;
        a.append_before(q);
        put(p, a);
    }
    {
        Polyline a(pts2);
        a.append(pl);
        put(p, a);
    }
    {
        Polyline a = pl;
        a.append(pts2);
        put(p, a);
    }
}

void pstp(Payload &p, const Points &pts, const std::vector<double> &widths, bool ep1, bool ep2, int index)
{
    put(p, pts);
    p.u32(uint32_t(widths.size()));
    for (double w : widths)
        p.f64(w);
    p.boolean(ep1).boolean(ep2).i32(index);
    ThickPolyline tp;
    tp.points    = pts;
    tp.width     = widths;
    tp.endpoints = std::make_pair(ep1, ep2);
    const ThickLines lines = tp.thicklines();
    p.u32(uint32_t(lines.size()));
    for (const ThickLine &l : lines) {
        put(p, l.a);
        put(p, l.b);
        p.f64(l.a_width).f64(l.b_width);
    }
    tp.start_at_index(index);
    put(p, tp.points);
    p.u32(uint32_t(tp.width.size()));
    for (double w : tp.width)
        p.f64(w);
}

// Query points for an arc: its ends, its center, points of `pts`, and points on the circle in and
// out of the arc's angle.
Points arc_queries(SplitMix &g, const Point &center, double r, const Points &pts, size_t n)
{
    Points q;
    for (size_t i = 0; i < n; ++i) {
        switch (g.below(5)) {
        case 0: q.push_back(center); break;
        case 1:
            if (!pts.empty()) {
                q.push_back(pts[size_t(g.below(pts.size()))]);
                break;
            }
            [[fallthrough]];
        default: {
            const double a  = real(g, 0., 2. * PI);
            const double rr = r * real(g, 0.3, 1.7);
            q.emplace_back(int64_t(std::llround(double(center.x()) + rr * std::cos(a))), int64_t(std::llround(double(center.y()) + rr * std::sin(a))));
        }
        }
    }
    return q;
}

Vec3f unscaled3(const Point &q, float z) { return Vec3f(float(double(q.x()) * SCALING_FACTOR), float(double(q.y()) * SCALING_FACTOR), z); }

} // namespace

void record_geom1(Sets &sets)
{
    // PSRZ
    {
        SplitMix g(0x9A770001u);
        Emit     e(sets, "PSRZ", 16, 256);
        e.hand([] { Payload p; psrz(p, Vec3d(0., 0., 0.), Vec3d(0., 0., 0.)); return p; }());
        e.hand([] { Payload p; psrz(p, Vec3d(0., 0., 0.), Vec3d(0., 0., PI / 2.)); return p; }());
        e.hand([] { Payload p; psrz(p, Vec3d(0., 0., PI / 2.), Vec3d(0., 0., -PI / 2.)); return p; }());
        e.hand([] { Payload p; psrz(p, Vec3d(0., 0., 0.1), Vec3d(0., 0., 0.1 + 1e-9)); return p; }());
        e.hand([] { Payload p; psrz(p, Vec3d(PI / 2., 0., 0.3), Vec3d(PI / 2., 0., 1.3)); return p; }());
        e.hand([] { Payload p; psrz(p, Vec3d(0., 0., PI), Vec3d(0., 0., -PI)); return p; }());
        for (int i = 0; i < 300; ++i) {
            Payload   p;
            const int kind = int(g.below(10));
            Vec3d     from(0., 0., real(g, -PI, PI));
            if (kind >= 6) {
                from.x() = real(g, -PI, PI);
                from.y() = real(g, -PI / 2., PI / 2.);
            }
            Vec3d to = from;
            to.z() += real(g, -2. * PI, 2. * PI);
            if (kind == 9)
                to = Vec3d(real(g, -PI, PI), real(g, -PI, PI), real(g, -PI, PI));
            psrz(p, from, to);
            e.random(p);
        }
    }

    // PSIC
    {
        SplitMix g(0x9A770002u);
        Emit     e(sets, "PSIC", 64, 128);
        const Point o(0, 0);
        for (const auto &t : std::vector<std::array<Point, 4>>{{Point(10, 0), o, Point(0, 10), Point(5, 5)},
                                                                 {Point(10, 0), o, Point(0, 10), Point(-5, -5)},
                                                                 {Point(0, 10), o, Point(10, 0), Point(5, 5)},
                                                                 {Point(0, 10), o, Point(10, 0), Point(-5, -5)},
                                                                 {Point(10, 0), o, Point(0, 10), Point(7, 0)},
                                                                 {Point(10, 0), o, Point(0, 10), Point(0, 7)},
                                                                 {Point(10, 0), o, Point(0, 10), o},
                                                                 {Point(10, 0), o, Point(-10, 0), Point(0, 5)},
                                                                 {Point(10, 0), o, Point(-10, 0), Point(0, -5)},
                                                                 {Point(10, 0), o, Point(10, 0), Point(3, 3)},
                                                                 {Point(1000000000, 1), Point(-1000000000, -1000000000), Point(1, 1000000000), Point(0, 0)}}) {
            Payload p;
            psic(p, t[0], t[1], t[2], t[3]);
            e.hand(p);
        }
        for (int i = 0; i < 2000; ++i) {
            const int64_t r = range(g);
            const Point   b = rpoint(g, r);
            Point         a = rpoint(g, r), c = rpoint(g, r), q = rpoint(g, r);
            if (one_in(g, 12))
                q = b;
            if (one_in(g, 12))
                q = Point(b.x() + (a.x() - b.x()) * 2, b.y() + (a.y() - b.y()) * 2);
            if (one_in(g, 20))
                c = Point(b.x() - (a.x() - b.x()), b.y() - (a.y() - b.y()));
            Payload p;
            psic(p, a, b, c, q);
            e.random(p);
        }
    }

    // PSCV
    {
        SplitMix                  g(0x9A770003u);
        Emit                      e(sets, "PSCV", 16, 1024);
        const std::vector<double> ths{0., 1e-12, PI / 4., PI / 2., 3. * PI / 4., PI};
        const std::vector<Points> shapes{rect(0, 0, 1000, 1000),
                                         Points{Point(0, 0), Point(1000, 0), Point(1000, 1000), Point(500, 300), Point(0, 1000)},
                                         Points{Point(0, 1000), Point(500, 300), Point(1000, 1000), Point(1000, 0), Point(0, 0)},
                                         Points{Point(0, 0), Point(1000, 0), Point(2000, 0), Point(2000, 1000)},
                                         Points{Point(0, 0), Point(1000, 0), Point(1000, 0), Point(0, 1000)}};
        for (const Points &s : shapes)
            for (double th : ths) {
                Payload p;
                pscv(p, s, th);
                e.hand(p);
            }
        for (int i = 0; i < 600; ++i) {
            const int64_t r = range(g);
            Points        pts;
            if (one_in(g, 4)) {
                // A rounded or notched rectangle, as a hole or contour of a printed part.
                pts = rect(-r, -r / 2, r, r / 2);
                if (one_in(g, 2))
                    pts.insert(pts.begin() + 2, Point(r / 3, 0));
            } else
                pts = star(g, rpoint(g, r / 2), r, size_t(rnd(g, 3, 24)), one_in(g, 3));
            double th = ths[size_t(g.below(ths.size()))];
            if (one_in(g, 2))
                th = real(g, 0., PI);
            Payload p;
            pscv(p, pts, th);
            e.random(p);
        }
    }

    // PSRC
    {
        SplitMix g(0x9A770004u);
        Emit     e(sets, "PSRC", 32, 128);
        e.hand([] { Payload p; psrc(p, 1., 1., 0., 1., 1.); return p; }());
        e.hand([] { Payload p; psrc(p, 1., 1., 0., 1., 2.); return p; }());
        e.hand([] { Payload p; psrc(p, 4., 0., 1., 1., 0.); return p; }());
        e.hand([] { Payload p; psrc(p, 0., 1., 1., 2., 0.); return p; }());
        for (int i = 0; i < 500; ++i) {
            // FillBase's use: a circle of radius r around the origin and the line through p0 along v.
            const double r  = std::pow(10., real(g, 4., 7.5));
            const double vx = real(g, -1., 1.) * std::pow(10., real(g, 3., 7.)), vy = real(g, -1., 1.) * std::pow(10., real(g, 3., 7.));
            const double px = real(g, -1.5, 1.5) * r, py = real(g, -1.5, 1.5) * r;
            const double lv2 = vx * vx + vy * vy;
            Payload      p;
            psrc(p, r * r, vy, -vx, lv2, -vy * px + vx * py);
            e.random(p);
        }
    }

    // PSCI
    {
        SplitMix g(0x9A770005u);
        Emit     e(sets, "PSCI", 24, 2048);
        const double max_r = double(DEFAULT_SCALED_MAX_RADIUS);
        e.hand([&] { Payload p; psci(p, Points{Point(0, 0), Point(1000000, 1000000), Point(2000000, 0)}, max_r, 50000., Vec3f(1.f, 0.f, 0.f), Vec3f(0.f, 0.f, 0.f), true); return p; }());
        e.hand([&] { Payload p; psci(p, Points{Point(0, 0), Point(1000000, 0), Point(2000000, 0)}, max_r, 50000., Vec3f(0.f, 1.f, 0.f), Vec3f(0.f, 0.f, 3.f), false); return p; }());
        e.hand([&] { Payload p; psci(p, Points{Point(0, 0), Point(10, 0)}, max_r, 50000., Vec3f(0.f, 0.f, 0.f), Vec3f(0.f, 0.f, 0.f), true); return p; }());
        e.hand([&] { Payload p; psci(p, arc_points(g, Point(0, 0), 5000000., 0., PI / 2., 8, 0.), max_r, 50000., Vec3f(5.f, 0.f, 1.f), Vec3f(0.f, 0.f, 0.f), true); return p; }());
        e.hand([&] { Payload p; psci(p, arc_points(g, Point(0, 0), 5000000., 0., PI / 2., 9, 200000.), max_r, 50000., Vec3f(5.f, 0.f, 1.f), Vec3f(0.f, 0.f, 0.f), false); return p; }());
        e.hand([&] { Payload p; psci(p, arc_points(g, Point(0, 0), 3000000000., 0., 0.01, 5, 0.), max_r, 50000., Vec3f(5.f, 0.f, 1.f), Vec3f(0.f, 0.f, 0.f), false); return p; }());
        for (int i = 0; i < 800; ++i) {
            Points pts;
            if (one_in(g, 4))
                pts = fit_path(g);
            else {
                const double r = std::pow(10., real(g, 3., 9.5));
                pts            = arc_points(g, rpoint(g, 100000000), r, real(g, 0., 2. * PI), (g.below(2) ? 1. : -1.) * real(g, 0.05, 6.2), size_t(rnd(g, 2, 16)),
                                            one_in(g, 2) ? r * real(g, 0., 0.05) : 0.);
            }
            if (pts.size() > 40)
                pts.resize(40);
            const double tol = std::pow(10., real(g, 2., 6.));
            const Point  a = rpoint(g, 100000000), b = rpoint(g, 100000000);
            Payload      p;
            psci(p, pts, one_in(g, 5) ? std::pow(10., real(g, 5., 9.)) : max_r, tol, unscaled3(a, float(real(g, 0., 5.))), unscaled3(b, float(real(g, 0., 5.))), one_in(g, 2));
            e.random(p);
        }
    }

    // PSAS
    {
        SplitMix     g(0x9A770006u);
        Emit         e(sets, "PSAS", 16, 3072);
        const double max_r = double(DEFAULT_SCALED_MAX_RADIUS), tol = double(DEFAULT_SCALED_RESOLUTION), pct = DEFAULT_ARC_LENGTH_PERCENT_TOLERANCE;
        auto         run   = [&](const Points &pts, double approx, double mr, double tl, double pc, const Points &q) {
            Payload p;
            psas(p, pts, approx, mr, tl, pc, q);
            return p;
        };
        {
            const Points a = arc_points(g, Point(0, 0), 10000000., 0., PI / 2., 7, 0.);
            e.hand(run(a, Polyline(a).length(), max_r, tol, pct, Points{Point(0, 0), Point(7071068, 7071068), Point(-10000000, 0), a[2]}));
            Points rev(a.rbegin(), a.rend());
            e.hand(run(rev, Polyline(rev).length(), max_r, tol, pct, Points{Point(7071068, 7071068), Point(0, -10000000)}));
            e.hand(run(a, Polyline(a).length() * 3., max_r, tol, pct, Points{}));
            const Points w = arc_points(g, Point(0, 0), 10000000., 3. * PI / 2., PI, 9, 0.);
            e.hand(run(w, Polyline(w).length(), max_r, tol, pct, Points{Point(10000000, 0), Point(-10000000, 0)}));
            e.hand(run(Points{Point(0, 0), Point(1000, 0)}, 1000., max_r, tol, pct, Points{}));
        }
        for (int i = 0; i < 1200; ++i) {
            Points       pts;
            const double r = std::pow(10., real(g, 4., 9.));
            const Point  c = rpoint(g, 100000000);
            if (one_in(g, 4))
                pts = fit_path(g);
            else
                pts = arc_points(g, c, r, real(g, 0., 2. * PI), (g.below(2) ? 1. : -1.) * real(g, 0.05, 6.2), size_t(rnd(g, 3, 20)), one_in(g, 3) ? r * real(g, 0., 0.01) : 0.);
            if (pts.size() > 40)
                pts.resize(40);
            double approx = Polyline(pts).length();
            if (one_in(g, 5))
                approx *= real(g, 0.5, 1.5);
            if (approx == 0.)
                approx = 1.;
            const Points q = arc_queries(g, c, r, pts, size_t(rnd(g, 0, 4)));
            const double tl = one_in(g, 3) ? std::pow(10., real(g, 2., 6.)) : tol;
            e.random(run(pts, approx, one_in(g, 6) ? std::pow(10., real(g, 5., 9.)) : max_r, tl, one_in(g, 6) ? real(g, 0.001, 0.5) : pct, q));
        }
    }

    // PSAC
    {
        SplitMix g(0x9A770007u);
        Emit     e(sets, "PSAC", 16, 3072);
        auto     make = [&](const Point &c, double r, double a0, double a1, int dir, size_t ns, double noise, size_t nq) {
            ArcCase k;
            k.center = c;
            k.radius = r;
            k.start  = Point(int64_t(std::llround(double(c.x()) + r * std::cos(a0))), int64_t(std::llround(double(c.y()) + r * std::sin(a0))));
            k.end    = Point(int64_t(std::llround(double(c.x()) + r * std::cos(a1))), int64_t(std::llround(double(c.y()) + r * std::sin(a1))));
            k.dir    = dir;
            double sweep = a1 - a0;
            if (dir == int(ArcDirection::Arc_Dir_CCW) && sweep < 0.)
                sweep += 2. * PI;
            if (dir == int(ArcDirection::Arc_Dir_CW) && sweep > 0.)
                sweep -= 2. * PI;
            k.slice   = arc_points(g, c, r, a0, sweep, ns, noise);
            k.queries = arc_queries(g, c, r, k.slice, nq);
            k.s3      = unscaled3(k.start, float(real(g, 0., 3.)));
            k.e3      = unscaled3(k.end, float(real(g, 0., 3.)));
            k.c3      = unscaled3(c, float(real(g, 0., 3.)));
            k.ccw     = dir != int(ArcDirection::Arc_Dir_CW);
            k.origin  = c;
            k.ray     = Vec2d((double(k.start.x()) - double(c.x())) / r, (double(k.start.y()) - double(c.y())) / r);
            k.sa      = k.slice.empty() ? k.start : k.slice.front();
            k.sb      = k.slice.size() > 1 ? k.slice[1] : k.end;
            return k;
        };
        const int ccw = int(ArcDirection::Arc_Dir_CCW), cw = int(ArcDirection::Arc_Dir_CW);
        for (const ArcCase &k : {make(Point(0, 0), 1000000., 0., PI / 2., ccw, 5, 0., 3), make(Point(0, 0), 1000000., 0., PI / 2., cw, 5, 0., 3),
                                 make(Point(0, 0), 1000000., 3., -3., ccw, 6, 0., 3), make(Point(0, 0), 1000000., -3., 3., cw, 6, 0., 3),
                                 make(Point(0, 0), 0., 0., 1., ccw, 3, 0., 1), make(Point(0, 0), 1000000., 1., 1., ccw, 3, 0., 1),
                                 make(Point(5, 5), 1000000., 0., PI, 0, 4, 0., 2)}) {
            Payload p;
            psac(p, k);
            e.hand(p);
        }
        {
            // The full circle of calc_arc_radian: start and end the same.
            ArcCase k = make(Point(0, 0), 2000000., 0., 2. * PI, ccw, 4, 0., 1);
            k.e3      = k.s3;
            Payload p;
            psac(p, k);
            e.hand(p);
        }
        for (int i = 0; i < 800; ++i) {
            const double r   = std::pow(10., real(g, 3., 9.));
            const int    dir = one_in(g, 10) ? 0 : (g.below(2) ? ccw : cw);
            ArcCase      k   = make(rpoint(g, 100000000), r, real(g, -PI, PI), real(g, -PI, PI), dir, size_t(rnd(g, 3, 10)), one_in(g, 3) ? r * real(g, 0., 0.05) : 0., size_t(rnd(g, 0, 4)));
            if (one_in(g, 5))
                std::reverse(k.slice.begin(), k.slice.end());
            if (one_in(g, 4)) {
                k.ray = Vec2d(real(g, -1., 1.), real(g, -1., 1.));
                k.sa  = rpoint(g, 100000000);
                k.sb  = rpoint(g, 100000000);
            }
            Payload p;
            psac(p, k);
            e.random(p);
        }
    }

    // PSAF
    {
        SplitMix g(0x9A770008u);
        Emit     e(sets, "PSAF", 16, 3072);
        const double tol = double(DEFAULT_SCALED_RESOLUTION);
        e.hand([&] { Payload p; psaf(p, Points{}, tol); return p; }());
        e.hand([&] { Payload p; psaf(p, Points{Point(0, 0)}, tol); return p; }());
        e.hand([&] { Payload p; psaf(p, Points{Point(0, 0), Point(1000, 0)}, tol); return p; }());
        e.hand([&] { Payload p; psaf(p, Points{Point(0, 0), Point(1000, 0), Point(2000, 0), Point(3000, 10)}, tol); return p; }());
        e.hand([&] { Payload p; psaf(p, arc_points(g, Point(0, 0), 10000000., 0., PI, 12, 0.), tol); return p; }());
        e.hand([&] { Payload p; psaf(p, arc_points(g, Point(0, 0), 10000000., 0., PI, 12, 0.), 0.); return p; }());
        for (int i = 0; i < 1200; ++i) {
            Points pts = fit_path(g);
            if (pts.size() > 60)
                pts.resize(60);
            const double t = one_in(g, 3) ? std::pow(10., real(g, 1., 6.)) : (one_in(g, 20) ? 0. : tol);
            Payload      p;
            psaf(p, pts, t);
            e.random(p);
        }
    }

    // PSPF
    {
        SplitMix g(0x9A770009u);
        Emit     e(sets, "PSPF", 4, 6144);
        const double tol = double(DEFAULT_SCALED_RESOLUTION);
        {
            const Points a = arc_points(g, Point(0, 0), 10000000., 0., PI, 10, 0.);
            Points       b{a.back(), Point(-10000000, -5000000), Point(-12000000, -9000000)};
            Points       c = a;
            c.insert(c.end(), b.begin() + 1, b.end());
            Payload p;
            pspf(p, c, tol, arc_points(g, Point(-30000000, -9000000), 18000000., 0., -PI / 2., 8, 0.), tol, a[4], 4, 3000000., 0.5);
            e.hand(p);
            Payload q;
            pspf(q, c, tol, Points{Point(5, 5), Point(10, 10)}, tol, Point(1234567, 7654321), 0, 1e9, 0.);
            e.hand(q);
            Payload s;
            pspf(s, Points{Point(0, 0), Point(1000000, 0), Point(2000000, 0)}, tol, Points{Point(2000000, 0), Point(3000000, 0)}, tol, Point(500000, 10), 2, 10., 1.);
            e.hand(s);
        }
        for (int i = 0; i < 800; ++i) {
            Points pts = fit_path(g);
            while (pts.size() < 3)
                pts.push_back(Point(pts.back().x() + 1000, pts.back().y() + 7));
            if (pts.size() > 50)
                pts.resize(50);
            Points pts2 = fit_path(g);
            if (pts2.size() < 2)
                pts2.push_back(Point(pts2.back().x() + 5000, pts2.back().y()));
            if (pts2.size() > 30)
                pts2.resize(30);
            if (one_in(g, 2))
                pts2.front() = pts.back();
            const double len  = Polyline(pts).length();
            Point        q    = one_in(g, 3) ? pts[size_t(g.below(pts.size()))] : Point(pts[0].x() + rnd(g, -10000000, 10000000), pts[0].y() + rnd(g, -10000000, 10000000));
            const auto   k    = uint64_t(rnd(g, 0, int64_t(pts.size())));
            const double dist = len * real(g, 0., 1.1);
            Payload      p;
            pspf(p, pts, one_in(g, 4) ? std::pow(10., real(g, 2., 6.)) : tol, pts2, tol, q, k, dist, real(g, 0., 1.));
            e.random(p);
        }
    }

    // PSTP
    {
        SplitMix g(0x9A77000Au);
        Emit     e(sets, "PSTP", 16, 1024);
        e.hand([] { Payload p; pstp(p, Points{Point(0, 0), Point(10, 0)}, {1., 2.}, true, false, 0); return p; }());
        e.hand([] { Payload p; pstp(p, Points{Point(0, 0), Point(10, 0), Point(10, 10), Point(0, 0)}, {1., 2., 2., 3., 3., 1.}, false, false, 2); return p; }());
        e.hand([] { Payload p; pstp(p, Points{Point(0, 0), Point(10, 0), Point(10, 10), Point(0, 0)}, {1., 2., 2., 3., 3., 4.}, false, false, 1); return p; }());
        e.hand([] { Payload p; pstp(p, Points{Point(0, 0), Point(10, 0), Point(10, 10), Point(0, 10)}, {1., 2., 2., 3., 3., 1.}, true, true, 1); return p; }());
        for (int i = 0; i < 300; ++i) {
            const int64_t r      = range(g);
            const bool    closed = one_in(g, 2);
            Points        pts    = star(g, rpoint(g, r / 2), r, size_t(rnd(g, 2, 16)), one_in(g, 2));
            if (closed)
                pts.push_back(pts.front());
            std::vector<double> w;
            for (size_t k = 0; k + 1 < pts.size(); ++k) {
                const double a = real(g, 100000., 800000.);
                w.push_back(w.empty() ? a : w.back());
                w.push_back(real(g, 100000., 800000.));
            }
            if (closed && !w.empty() && one_in(g, 4) == false)
                w.back() = w.front();
            Payload p;
            pstp(p, pts, w, one_in(g, 2), one_in(g, 2), int(rnd(g, 0, int64_t(pts.size()) - 1)));
            e.random(p);
        }
    }
}

} // namespace paths
} // namespace probe
