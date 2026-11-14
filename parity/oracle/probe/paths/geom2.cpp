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
// paths.sop1's records of the piece geom-2 (parity/sop1/paths.md), which orca-geom replays:
//
//   PSP3  Point3, Line3, lerp, the Points/Points3 conversions, BoundingBox3 and MultiPoint3 (the
//         length, box, find_point, translate, rotate, remove_duplicate_points, lines) and foot_pt
//         of a Points3; Point3::projection_onto of a MultiPoint3, which throws.
//   PSD3  MultiPoint3::_douglas_peucker (the 3D Douglas-Peucker) and Polyline3::simplify.
//   PSL3  Polyline3: simplify_by_fitting_arc, reverse, clip_end, split_at_index, split_at (2D and
//         3D), split_at_length, the appends, to_polyline and lines, with and without a fitting
//         result.
//   PSC3  The Points3 forms of Circle::try_create_circle, ArcSegment::try_create_arc and
//         ArcSegment::are_points_within_slice.
//   PSO3  offset of a Polyline3 (ClipperUtils.cpp).
//
// Each record holds its inputs, then the engine's results. Added 2026-11-13 by orca-o3-probe-paths.
#include "slic.hpp"

#include <cmath>
#include <exception>
#include <string>
#include <vector>

#include "libslic3r/BoundingBox.hpp"
#include "libslic3r/ClipperUtils.hpp"
#include "libslic3r/MultiPoint.hpp"

namespace probe {
namespace paths {

namespace {

struct P3Case
{
    Point3  p, a, b, c;
    double  angle = 0., factor = 1., t = 0.5, eps = 0.;
    Points  pts2;
    Points3 pts3;
    Vec3d   v{0., 0., 0.};
    int64_t k = 0;
};

void psp3(Payload &p, const P3Case &x)
{
    put(p, x.p);
    put(p, x.a);
    put(p, x.b);
    put(p, x.c);
    p.f64(x.angle).f64(x.factor).f64(x.t).f64(x.eps);
    put(p, x.pts2);
    put(p, x.pts3);
    put(p, x.v);
    p.i64(x.k);

    Point3 q = x.p;
    put(p, q.rotated(x.angle));
    put(p, q.rotated(x.angle, x.c));
    put(p, q.rotate_90_degree_ccw());
    put(p, q.operator*(x.factor));
    put(p, Slic3r::operator*(q, x.factor));
    {
        Point3 r = q;
        r *= x.factor;
        put(p, r);
    }
    p.f64(q.ccw(x.a, x.b)).f64(q.ccw(Line3(x.a, x.b))).f64(q.ccw_angle(x.a, x.b));
    put(p, q.projection_onto(Line3(x.a, x.b)));
    p.i32(q.nearest_point_index(x.pts2));
    {
        Point3     out;
        const bool ok = q.nearest_point(x.pts2, &out);
        p.boolean(ok);
        put(p, out);
    }
    p.f64(q.distance_to(x.a));
    put(p, q.to_point());
    p.boolean(q.both_comp(x.a, ">")).boolean(q.both_comp(x.a, "<")).boolean(q.any_comp(x.a, ">")).boolean(q.any_comp(x.a, "<"));
    p.boolean(q.any_comp(x.c.x(), ">")).boolean(q.any_comp(x.c.x(), "<"));

    const Line3 l(x.a, x.b);
    p.f64(l.length());
    put(p, l.vector());
    put(p, l.midpoint());
    put(p, l.to_line().a);
    put(p, l.to_line().b);
    p.f64(Line3::distance_to_squared(x.p, x.a, x.b));
    put(p, lerp(x.a, x.b, x.t));
    put(p, Point3::new_scale(x.v));
    put(p, Point3::new_scale(Vec3f(x.v.cast<float>())));
    put(p, Point3::new_scale(x.v.x(), x.v.y(), x.v.z()));
    put(p, to_points(x.pts3));
    put(p, to_points3(x.pts2));

    const BoundingBox3 bb(x.pts3);
    put(p, bb.min);
    put(p, bb.max);
    p.boolean(bb.defined);

    const Polyline3 pl(x.pts3);
    p.f64(pl.length());
    const BoundingBox3 pb = pl.bounding_box();
    put(p, pb.min);
    put(p, pb.max);
    p.boolean(pb.defined);
    const Point3 vertex = x.pts3.empty() ? x.a : x.pts3[size_t(x.k) % x.pts3.size()];
    for (const Point3 &f : {x.a, vertex}) {
        p.i32(pl.find_point(f.to_point()));
        p.i32(pl.find_point(f.to_point(), x.eps));
        p.i32(pl.find_point(f));
        p.i32(pl.find_point(f, x.eps));
    }
    {
        Polyline3 m = pl;
        m.translate(x.c);
        put(p, m.points);
    }
    {
        Polyline3 m = pl;
        m.rotate(x.angle);
        put(p, m.points);
    }
    {
        Polyline3 m = pl;
        m.rotate(x.angle, x.c);
        put(p, m.points);
    }
    {
        Polyline3  m  = pl;
        const bool ok = m.remove_duplicate_points();
        p.boolean(ok);
        put(p, m.points);
    }
    {
        const Lines3 ls = pl.lines();
        p.u32(uint32_t(ls.size()));
        for (const Line3 &s : ls) {
            put(p, s.a);
            put(p, s.b);
        }
    }
    {
        const std::pair<int, Point3> f = foot_pt(x.pts3, x.p);
        p.i32(f.first);
        put(p, f.second);
    }
    {
        bool        threw = false;
        std::string what;
        try {
            (void) q.projection_onto(static_cast<const MultiPoint3 &>(pl));
        } catch (const std::exception &e) {
            threw = true;
            what  = e.what();
        }
        p.boolean(threw).str(what);
    }
}

void psd3(Payload &p, const Points3 &pts, double tol)
{
    put(p, pts);
    p.f64(tol);
    put(p, MultiPoint3::_douglas_peucker(pts, tol));
    Polyline3 pl(pts);
    pl.simplify(tol);
    put(p, pl.points);
}

struct L3Case
{
    Points3 pts, pts2;
    double  tol = 0., tol2 = 0.;
    Point   q2;
    Point3  q3;
    int64_t k    = 0;
    double  dist = 0., frac = 0.;
};

// The operations of PSL3 on `base` (fitted or not), `other` being the second polyline.
void l3_ops(Payload &p, const Polyline3 &base, const Polyline3 &other, const L3Case &x)
{
    put(p, base);
    {
        Polyline3 r = base;
        r.reverse();
        put(p, r);
    }
    {
        Polyline3 r = base;
        r.clip_end(x.dist);
        put(p, r);
    }
    {
        Polyline3  a, b;
        const bool ok = base.split_at_index(size_t(x.k), &a, &b);
        p.boolean(ok);
        put(p, a);
        put(p, b);
    }
    {
        Point     q = x.q2;
        Polyline3 a, b;
        base.split_at(q, &a, &b);
        put(p, q);
        put(p, a);
        put(p, b);
    }
    {
        Point3    q = x.q3;
        Polyline3 a, b;
        base.split_at(q, &a, &b);
        put(p, q);
        put(p, a);
        put(p, b);
    }
    {
        Polyline3  a, b;
        const bool ok = base.split_at_length(base.length() * x.frac, &a, &b);
        p.boolean(ok);
        put(p, a);
        put(p, b);
    }
    put(p, other);
    {
        Polyline3 a = base;
        a.append(other);
        put(p, a);
    }
    {
        Polyline3 a = base;
        a.append(x.q3);
        put(p, a);
    }
    {
        Polyline3 a = base;
        a.append_before(x.q3);
        put(p, a);
    }
    put(p, base.to_polyline().points);
}

void psl3(Payload &p, const L3Case &x)
{
    put(p, x.pts);
    put(p, x.pts2);
    p.f64(x.tol).f64(x.tol2);
    put(p, x.q2);
    put(p, x.q3);
    p.i64(x.k).f64(x.dist).f64(x.frac);
    const Polyline3 raw(x.pts), raw2(x.pts2);
    l3_ops(p, raw, raw2, x);
    Polyline3 fit = raw, fit2 = raw2;
    fit.simplify_by_fitting_arc(x.tol);
    fit2.simplify_by_fitting_arc(x.tol2);
    l3_ops(p, fit, fit2, x);
    {
        // A polyline without a fitting result takes one with it: the fake linear move.
        Polyline3 a = raw;
        a.append(fit2);
        put(p, a);
    }
}

void psc3(Payload &p, const Points3 &pts, double max_r, double tol, double approx, double pct)
{
    put(p, pts);
    p.f64(max_r).f64(tol).f64(approx).f64(pct);
    Circle     c;
    const bool okc = Circle::try_create_circle(pts, max_r, tol, c);
    p.boolean(okc);
    put(p, c);
    ArcSegment arc;
    const bool oka = pts.size() >= 2 && ArcSegment::try_create_arc(pts, arc, approx, max_r, tol, pct);
    p.boolean(oka);
    put(p, arc);
    p.boolean(oka && ArcSegment::are_points_within_slice(arc, pts));
    // Arcs of the fitted circle from the first to the last point, both ways, for the slice test.
    const Points pts2 = to_points(pts);
    for (ArcDirection dir : {ArcDirection::Arc_Dir_CCW, ArcDirection::Arc_Dir_CW}) {
        const ArcSegment a(c.center, okc ? c.radius : 0., pts.front().to_point(), pts.back().to_point(), dir);
        p.boolean(a.is_valid());
        p.boolean(a.is_valid() && pts.size() >= 3 && ArcSegment::are_points_within_slice(a, pts));
        p.boolean(a.is_valid() && pts.size() >= 3 && ArcSegment::are_points_within_slice(a, pts2));
    }
}

void pso3(Payload &p, const Points3 &pts, float delta, int join, double miter, int end)
{
    put(p, pts);
    p.f32(delta).u8(uint8_t(join)).f64(miter).u8(uint8_t(end));
    put(p, offset(Polyline3(pts), delta, ClipperLib::JoinType(join), miter, ClipperLib::EndType(end)));
}

Point3 rpoint3(SplitMix &g, int64_t r, int64_t rz) { return Point3(rnd(g, -r, r), rnd(g, -r, r), rnd(g, -rz, rz)); }

Points3 rpath3(SplitMix &g, size_t lo, size_t hi)
{
    Points3 pts = lift(fit_path(g), g, one_in(g, 3));
    while (pts.size() < lo)
        pts.emplace_back(pts.back().x() + 7000, pts.back().y() - 3000, pts.back().z() + 100);
    if (pts.size() > hi)
        pts.resize(hi);
    if (one_in(g, 6) && pts.size() > 2)
        pts.insert(pts.begin() + 1, pts[1]);
    return pts;
}

} // namespace

void record_geom2(Sets &sets)
{
    // PSP3
    {
        SplitMix g(0x9B770001u);
        Emit     e(sets, "PSP3", 16, 2048);
        {
            P3Case x;
            x.p    = Point3(10, 20, 30);
            x.a    = Point3(0, 0, 0);
            x.b    = Point3(100, 0, 50);
            x.c    = Point3(5, 5, 5);
            x.pts2 = Points{Point(1, 1), Point(10, 19), Point(10, 21)};
            x.pts3 = Points3{Point3(0, 0, 0), Point3(100, 0, 50), Point3(100, 0, 50), Point3(100, 100, 0)};
            x.v    = Vec3d(1.5, -2.25, 0.1);
            x.k    = 1;
            Payload p;
            psp3(p, x);
            e.hand(p);
            x.a     = x.b; // a line of one point
            x.angle = PI / 2.;
            x.factor = -1.5;
            x.t      = 1.;
            x.eps    = 50.;
            x.pts2.clear();
            Payload q;
            psp3(q, x);
            e.hand(q);
        }
        for (int i = 0; i < 600; ++i) {
            const int64_t r = std::min(range(g), int64_t(300000000));
            P3Case        x;
            x.p = rpoint3(g, r, 2000000);
            x.a = rpoint3(g, r, 2000000);
            x.b = one_in(g, 10) ? x.a : rpoint3(g, r, 2000000);
            x.c = rpoint3(g, r, 2000000);
            if (one_in(g, 8))
                x.p = x.a;
            x.angle  = real(g, -7., 7.);
            x.factor = real(g, -3., 3.);
            x.t      = real(g, 0., 1.);
            x.eps    = one_in(g, 4) ? 0. : real(g, 0., double(r) * 0.2);
            for (size_t k = 0, n = size_t(rnd(g, 0, 8)); k < n; ++k)
                x.pts2.push_back(rpoint(g, r));
            for (size_t k = 0, n = size_t(rnd(g, 0, 10)); k < n; ++k)
                x.pts3.push_back(rpoint3(g, r, 2000000));
            if (x.pts3.size() > 2 && one_in(g, 3))
                x.pts3.insert(x.pts3.begin() + 1, x.pts3[1]);
            x.v = Vec3d(real(g, -300., 300.), real(g, -300., 300.), real(g, -5., 5.));
            x.k = rnd(g, 0, 20);
            Payload p;
            psp3(p, x);
            e.random(p);
        }
    }

    // PSD3
    {
        SplitMix g(0x9B770002u);
        Emit     e(sets, "PSD3", 16, 2048);
        e.hand([] { Payload p; psd3(p, Points3{}, 10.); return p; }());
        e.hand([] { Payload p; psd3(p, Points3{Point3(1, 2, 3)}, 10.); return p; }());
        e.hand([] { Payload p; psd3(p, Points3{Point3(0, 0, 0), Point3(50, 1, 100), Point3(100, 0, 0)}, 10.); return p; }());
        e.hand([] { Payload p; psd3(p, Points3{Point3(0, 0, 0), Point3(50, 1, 100), Point3(100, 0, 0), Point3(0, 0, 0)}, 0.); return p; }());
        for (int i = 0; i < 800; ++i) {
            const Points3 pts = rpath3(g, 0, 60);
            Payload       p;
            psd3(p, pts, one_in(g, 10) ? 0. : std::pow(10., real(g, 1., 6.5)));
            e.random(p);
        }
    }

    // PSL3
    {
        SplitMix     g(0x9B770003u);
        Emit         e(sets, "PSL3", 3, 8192);
        const double tol = double(DEFAULT_SCALED_RESOLUTION);
        {
            L3Case x;
            x.pts  = lift(arc_points(g, Point(0, 0), 10000000., 0., PI, 10, 0.), g, false);
            x.pts.emplace_back(-12000000, -3000000, 500000);
            x.pts2 = lift(arc_points(g, Point(-12000000, -13000000), 10000000., PI / 2., -PI / 2., 8, 0.), g, true);
            x.tol = x.tol2 = tol;
            x.q2           = x.pts[3].to_point();
            x.q3           = Point3(x.pts[6].x() + 100, x.pts[6].y(), int64_t(7));
            x.k            = 4;
            x.dist         = 4000000.;
            x.frac         = 0.4;
            Payload p;
            psl3(p, x);
            e.hand(p);
        }
        for (int i = 0; i < 600; ++i) {
            L3Case x;
            x.pts  = rpath3(g, 3, 40);
            x.pts2 = rpath3(g, 2, 24);
            if (one_in(g, 2))
                x.pts2.front() = x.pts.back();
            x.tol      = one_in(g, 4) ? std::pow(10., real(g, 2., 6.)) : tol;
            x.tol2     = tol;
            x.q2       = one_in(g, 3) ? x.pts[size_t(g.below(x.pts.size()))].to_point() : Point(x.pts[0].x() + rnd(g, -10000000, 10000000), x.pts[0].y() + rnd(g, -10000000, 10000000));
            x.q3       = one_in(g, 3) ? x.pts[size_t(g.below(x.pts.size()))] : rpoint3(g, 100000000, 2000000);
            x.k        = rnd(g, 0, int64_t(x.pts.size()));
            x.dist     = Polyline3(x.pts).length() * real(g, 0., 1.1);
            x.frac     = real(g, 0., 1.);
            Payload p;
            psl3(p, x);
            e.random(p);
        }
    }

    // PSC3
    {
        SplitMix     g(0x9B770004u);
        Emit         e(sets, "PSC3", 24, 2048);
        const double max_r = double(DEFAULT_SCALED_MAX_RADIUS), tol = double(DEFAULT_SCALED_RESOLUTION), pct = DEFAULT_ARC_LENGTH_PERCENT_TOLERANCE;
        {
            const Points3 a = lift(arc_points(g, Point(0, 0), 10000000., 0., PI / 2., 7, 0.), g, false);
            e.hand([&] { Payload p; psc3(p, a, max_r, tol, Polyline3(a).length(), pct); return p; }());
            e.hand([&] { Payload p; psc3(p, Points3{Point3(0, 0, 1), Point3(1000, 0, 2)}, max_r, tol, 1000., pct); return p; }());
        }
        for (int i = 0; i < 800; ++i) {
            const double r   = std::pow(10., real(g, 4., 9.));
            Points3      pts = one_in(g, 4) ? rpath3(g, 2, 30)
                                            : lift(arc_points(g, rpoint(g, 100000000), r, real(g, 0., 2. * PI), (g.below(2) ? 1. : -1.) * real(g, 0.05, 6.2), size_t(rnd(g, 2, 16)),
                                                              one_in(g, 3) ? r * real(g, 0., 0.01) : 0.),
                                                   g, one_in(g, 2));
            double approx = Polyline3(pts).length();
            if (one_in(g, 5))
                approx *= real(g, 0.5, 1.5);
            if (approx == 0.)
                approx = 1.;
            Payload p;
            psc3(p, pts, max_r, one_in(g, 3) ? std::pow(10., real(g, 2., 6.)) : tol, approx, pct);
            e.random(p);
        }
    }

    // PSO3
    {
        SplitMix g(0x9B770005u);
        Emit     e(sets, "PSO3", 16, 3072);
        e.hand([] { Payload p; pso3(p, Points3{Point3(0, 0, 5), Point3(1000000, 0, 9)}, 100000.f, 0, 3., 2); return p; }());
        e.hand([] { Payload p; pso3(p, Points3{Point3(0, 0, 5), Point3(1000000, 0, 9), Point3(1000000, 1000000, 0)}, 50000.f, 2, 3., 3); return p; }());
        e.hand([] { Payload p; pso3(p, Points3{Point3(0, 0, 5), Point3(1000000, 0, 9), Point3(1000000, 1000000, 0)}, 50000.f, 1, 5000., 4); return p; }());
        for (int i = 0; i < 400; ++i) {
            Points3 pts = rpath3(g, 2, 20);
            const int join = int(g.below(3));
            Payload p;
            pso3(p, pts, float(std::pow(10., real(g, 3., 6.))), join, join == 1 ? std::pow(10., real(g, 2., 4.)) : real(g, 1., 5.), int(g.below(5)));
            e.random(p);
        }
    }
}

} // namespace paths
} // namespace probe
