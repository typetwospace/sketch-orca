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
// arrange.sop1's libnest2d records (parity/SOP1.md, "arrange.sop1"), which orca-arrange's
// tests/probes.rs replays, with libnest2d instantiated as Arrange.cpp instantiates it (the
// libslic3r backend, LargeInt and the rational edge sort):
//
// - RTRE: Arrange.cpp's spatial index, bgi::rtree<std::pair<Box, unsigned>, bgi::rstar<16, 4>>:
//   inserts, intersects queries (their results sorted, as Arrange.cpp never reads their order),
//   bounds, size and clear;
// - NFPC: nfpConvexOnly<ExPolygon, boost::rational<LargeInt>> on convex and non-convex shapes,
//   and nfpInnerRectBed;
// - NGEO: _Item (transformed shape, area, bounding box, reference vertices), convexHull,
//   minimizeCircle, the isInside, within and touches predicates, the placer's overfit functions
//   and EdgeCache (corners and coords).
//
// Recorded by the o2-arrange task (2026-11-07). Each record holds its inputs and then the
// engine's results.
#include "../probe.hpp"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <exception>
#include <functional>
#include <limits>
#include <string>
#include <utility>
#include <vector>

#include <libnest2d/backends/libslic3r/geometries.hpp>
#include <libnest2d/optimizers/nlopt/subplex.hpp>
#include <libnest2d/placers/nfpplacer.hpp>
#include <libnest2d/selections/firstfit.hpp>

#include <boost/geometry/index/rtree.hpp>
#include <boost/multiprecision/integer.hpp>
#include <boost/rational.hpp>

#include "libslic3r/ExPolygon.hpp"
#include "libslic3r/Point.hpp"
#include "libslic3r/Polygon.hpp"

// As Arrange.cpp has them under MSVC: LargeInt is Boost's int128_t, a scalar to libnest2d, and
// boost::rational a rational.
namespace libnest2d {
#if !defined(_MSC_VER) && defined(__SIZEOF_INT128__) && !defined(__APPLE__)
using LargeInt = __int128;
#else
using LargeInt = boost::multiprecision::int128_t;
template<> struct _NumTag<LargeInt>
{
    using Type = ScalarTag;
};
#endif
template<class T> struct _NumTag<boost::rational<T>>
{
    using Type = RationalTag;
};
} // namespace libnest2d

namespace probe {

namespace {

using Slic3r::ExPolygon;
using Slic3r::Point;
using Slic3r::Points;
using Item        = libnest2d::_Item<ExPolygon>;
using Box         = libnest2d::_Box<Point>;
using Circle      = libnest2d::_Circle<Point>;
using SpatElement = std::pair<Box, unsigned>;
using SpatIndex   = boost::geometry::index::rtree<SpatElement, boost::geometry::index::rstar<16, 4>>;
using BoxPlacer   = libnest2d::placers::_NofitPolyPlacer<ExPolygon, Box>;
namespace sl      = libnest2d::shapelike;

void put(Payload &p, const Point &q) { p.i64(q.x()).i64(q.y()); }
void put(Payload &p, const Points &pts)
{
    p.u32(uint32_t(pts.size()));
    for (const Point &q : pts)
        put(p, q);
}
void put(Payload &p, const ExPolygon &e)
{
    put(p, e.contour.points);
    p.u32(uint32_t(e.holes.size()));
    for (const auto &h : e.holes)
        put(p, h.points);
}
void put(Payload &p, const Box &b)
{
    put(p, b.minCorner());
    put(p, b.maxCorner());
}

ExPolygon shape(const Points &pts)
{
    ExPolygon e;
    e.contour.points = pts;
    return e;
}

double  unit(SplitMix &g) { return double(g.next() >> 11) * (1.0 / 9007199254740992.0); }
double  span(SplitMix &g, double lo, double hi) { return lo + (hi - lo) * unit(g); }
int64_t ispan(SplitMix &g, int64_t lo, int64_t hi) { return lo + int64_t(g.below(uint64_t(hi - lo) + 1)); }

// A shape around `c` within `r`: a convex polygon, a star (not convex), a rectangle, an L, a
// thin sliver (all simple), or points in any order (`soup`, not simple unless `simple` is asked
// for); either way round, now and then with a repeated or a collinear point.
Points rshape(SplitMix &g, Point c, int64_t r, bool simple = false, bool *was_simple = nullptr)
{
    Points    pts;
    const int kind = int(g.below(simple ? 7 : 8));
    if (was_simple)
        *was_simple = kind != 7;
    if (kind <= 2) { // convex: points on an ellipse at increasing angles
        const size_t n  = size_t(3 + g.below(kind == 0 ? 3 : 30));
        const double rx = double(r) * span(g, 0.2, 1.), ry = double(r) * span(g, 0.2, 1.);
        double       a0 = span(g, 0., 6.28);
        for (size_t i = 0; i < n; ++i) {
            const double a = a0 + 6.283185307179586 * (double(i) + 0.5 * unit(g)) / double(n);
            pts.emplace_back(c.x() + int64_t(std::llround(rx * std::cos(a))), c.y() + int64_t(std::llround(ry * std::sin(a))));
        }
    } else if (kind == 3) { // star
        const size_t n = size_t(5 + g.below(10));
        for (size_t i = 0; i < 2 * n; ++i) {
            const double a  = 6.283185307179586 * double(i) / double(2 * n);
            const double rr = double(r) * (i % 2 == 0 ? 1. : span(g, 0.2, 0.7));
            pts.emplace_back(c.x() + int64_t(std::llround(rr * std::cos(a))), c.y() + int64_t(std::llround(rr * std::sin(a))));
        }
    } else if (kind == 4) { // rectangle
        const int64_t w = ispan(g, 1, r), h = ispan(g, 1, r);
        pts = {Point(c.x() - w, c.y() - h), Point(c.x() + w, c.y() - h), Point(c.x() + w, c.y() + h), Point(c.x() - w, c.y() + h)};
    } else if (kind == 5) { // L
        const int64_t w = ispan(g, 2, r), h = ispan(g, 2, r), t = ispan(g, 1, std::min(w, h) - 1);
        pts = {Point(c.x(), c.y()), Point(c.x() + w, c.y()), Point(c.x() + w, c.y() + t), Point(c.x() + t, c.y() + t), Point(c.x() + t, c.y() + h),
               Point(c.x(), c.y() + h)};
    } else if (kind == 6) { // sliver
        const int64_t w = ispan(g, 10, r);
        pts = {Point(c.x(), c.y()), Point(c.x() + w, c.y() + ispan(g, 0, 3)), Point(c.x() + w / 2, c.y() + ispan(g, 1, 5))};
    } else { // soup
        const size_t n = size_t(3 + g.below(10));
        for (size_t i = 0; i < n; ++i)
            pts.emplace_back(c.x() + ispan(g, -r, r), c.y() + ispan(g, -r, r));
    }
    if (g.below(2) == 0)
        std::reverse(pts.begin(), pts.end());
    if (g.below(10) == 0 && pts.size() > 1) {
        const size_t i = size_t(g.below(pts.size()));
        pts.insert(pts.begin() + i, pts[i]);
    }
    if (g.below(10) == 0 && pts.size() > 1) {
        const Point a = pts[0], b = pts[1];
        pts.insert(pts.begin() + 1, Point((a.x() + b.x()) / 2, (a.y() + b.y()) / 2));
    }
    return pts;
}

int64_t rrange(SplitMix &g)
{
    switch (g.below(5)) {
    case 0: return 1000;
    case 1: return 1000000;
    case 2: return 50000000;
    case 3: return 200000000;
    default: return 20000000;
    }
}

Box rbox(SplitMix &g, int64_t r)
{
    const int64_t x0 = ispan(g, -r, r), y0 = ispan(g, -r, r);
    const int64_t w = g.below(8) == 0 ? 0 : ispan(g, 0, r), h = g.below(8) == 0 ? 0 : ispan(g, 0, r);
    return Box(Point(x0, y0), Point(x0 + w, y0 + h));
}

// --- RTRE --------------------------------------------------------------------------------------

// One RTRE record: u32 the number of operations, then each: u8 0 insert (box, u32 index), 1
// query (box; the elements whose box meets it, sorted by index), 2 bounds (box, u64 size, bool
// empty), 3 clear.
void rtre(Payload &p, SplitMix &g, size_t ops)
{
    SpatIndex     tree;
    const int64_t r    = rrange(g);
    unsigned      next = 0;
    std::vector<Box> inserted;
    p.u32(uint32_t(ops));
    for (size_t i = 0; i < ops; ++i) {
        const uint64_t c = g.below(20);
        if (c < 12) {
            Box b = rbox(g, r);
            if (!inserted.empty() && g.below(6) == 0)
                b = inserted[size_t(g.below(inserted.size()))]; // the same box again
            inserted.push_back(b);
            p.u8(0);
            put(p, b);
            p.u32(next);
            tree.insert({b, next});
            ++next;
        } else if (c < 17) {
            Box q = rbox(g, r);
            if (!inserted.empty() && g.below(3) == 0) { // touching an inserted box at a corner
                const Box &o = inserted[size_t(g.below(inserted.size()))];
                q            = Box(o.maxCorner(), Point(o.maxCorner().x() + ispan(g, 0, r), o.maxCorner().y() + ispan(g, 0, r)));
            }
            std::vector<SpatElement> result;
            tree.query(boost::geometry::index::intersects(q), std::back_inserter(result));
            std::sort(result.begin(), result.end(), [](const SpatElement &a, const SpatElement &b) { return a.second < b.second; });
            p.u8(1);
            put(p, q);
            p.u32(uint32_t(result.size()));
            for (const SpatElement &e : result) {
                put(p, e.first);
                p.u32(e.second);
            }
        } else if (c < 19) {
            Box bb;
            boost::geometry::convert(tree.bounds(), bb);
            p.u8(2);
            put(p, bb);
            p.u64(uint64_t(tree.size())).boolean(tree.empty());
        } else {
            p.u8(3);
            tree.clear();
            inserted.clear();
        }
    }
}

// --- NFPC --------------------------------------------------------------------------------------

// One NFPC record: sh and other (contours), then nfpConvexOnly(sh, other) (u8 0, the shape and
// its reference point, or 1 and what()), then the bed box and nfpInnerRectBed(bed, other).
void nfpc(Payload &p, const Points &a, const Points &b, const Box &bed)
{
    put(p, a);
    put(p, b);
    const ExPolygon sh = shape(a), other = shape(b);
    try {
        auto r = libnest2d::nfp::nfpConvexOnly<ExPolygon, boost::rational<libnest2d::LargeInt>>(sh, other);
        p.boolean(false);
        put(p, r.first);
        put(p, r.second);
    } catch (const std::exception &e) {
        p.boolean(true).str(e.what());
    }
    put(p, bed);
    auto inner = libnest2d::nfp::nfpInnerRectBed(bed, other);
    put(p, inner.first);
    put(p, inner.second);
}

// Whether a contour has an edge of non-zero length, which nfpConvexOnly needs.
bool has_edge(const Points &pts)
{
    for (size_t i = 1; i < pts.size(); ++i)
        if (pts[i] != pts[i - 1])
            return true;
    return false;
}

// --- NGEO --------------------------------------------------------------------------------------

struct Ngeo
{
    Points  pts, other; // other is simple
    bool    simple       = true; // whether pts is
    bool    set_rotation = false, set_translation = false, set_inflation = false;
    double  rotation    = 0.;
    Point   translation = Point(0, 0);
    int64_t inflation   = 0;
    Box     box;
    Point   circle_center = Point(0, 0);
    double  radius        = 0.;
    Point   pt            = Point(0, 0);
    double  accuracy      = 1.;
    std::vector<double> distances;
};

// Boost.Geometry's within(a, b) of two polygons: u8 0 and the answer, or 1 and what().
void within_outcome(Payload &p, const ExPolygon &a, const ExPolygon &b)
{
    try {
        const bool w = sl::isInside(a, b);
        p.boolean(false).boolean(w);
    } catch (const std::exception &e) {
        p.boolean(true).str(e.what());
    }
}

// One NGEO record: the case, then the item's transformed shape, area, bounding box, reference
// and leftmost-bottom vertices; the convex hull of the shape and of [shape, other];
// minimizeCircle of the shape; isInside of (shape, box), (shape, circle), (bounding box, circle),
// (bounding box, box), (point, circle), (point, box); within and touches of the point; within
// (hull, other) and, for a simple shape, within (shape, other); the placer's overfit of
// (bounding box, box), (shape, circle) and (hull, other); and EdgeCache of the shape at the
// accuracy: its corners and coords at each distance.
void ngeo(Payload &p, const Ngeo &k)
{
    put(p, k.pts);
    put(p, k.other);
    p.boolean(k.simple);
    p.boolean(k.set_rotation).f64(k.rotation).boolean(k.set_translation);
    put(p, k.translation);
    p.boolean(k.set_inflation).i64(k.inflation);
    put(p, k.box);
    put(p, k.circle_center);
    p.f64(k.radius);
    put(p, k.pt);
    p.f64(k.accuracy);
    p.u32(uint32_t(k.distances.size()));
    for (double d : k.distances)
        p.f64(d);

    Item item(shape(k.pts));
    if (k.set_inflation)
        item.inflation(k.inflation);
    if (k.set_rotation)
        item.rotation(k.rotation);
    if (k.set_translation)
        item.translation(k.translation);
    const ExPolygon &ts = item.transformedShape();
    put(p, ts);
    p.f64(item.area());
    put(p, item.boundingBox());
    put(p, item.referenceVertex());
    put(p, item.leftmostBottomVertex());

    const ExPolygon other = shape(k.other);
    const ExPolygon hull  = sl::convexHull(ts);
    put(p, hull);
    put(p, sl::convexHull(Slic3r::ExPolygons{ts, other}));
    const Circle mc = libnest2d::placers::minimizeCircle(ts);
    put(p, mc.center());
    p.f64(mc.radius());

    const Circle circ(k.circle_center, k.radius);
    const Box    bb = item.boundingBox();
    p.boolean(sl::isInside(ts, k.box));
    p.boolean(sl::isInside(ts, circ));
    p.boolean(sl::isInside(bb, circ));
    p.boolean(sl::isInside(bb, k.box));
    p.boolean(sl::isInside(k.pt, circ));
    p.boolean(sl::isInside(k.pt, k.box));
    p.boolean(sl::isInside(k.pt, ts));
    p.boolean(sl::touches(k.pt, ts));
    within_outcome(p, hull, other);
    if (k.simple)
        within_outcome(p, ts, other);
    p.f64(BoxPlacer::overfit(bb, k.box));
    p.f64(BoxPlacer::overfit(ts, circ));
    try {
        const double of = BoxPlacer::overfit(hull, other);
        p.boolean(false).f64(of);
    } catch (const std::exception &e) {
        p.boolean(true).str(e.what());
    }

    libnest2d::placers::EdgeCache<ExPolygon> ec(ts);
    ec.accuracy(k.accuracy);
    const std::vector<double> &corners = ec.corners();
    p.u32(uint32_t(corners.size()));
    for (double c : corners)
        p.f64(c);
    p.f64(ec.circumference());
    for (double c : corners)
        put(p, ec.coords(c));
    for (double d : k.distances)
        put(p, ec.coords(d));
}

Ngeo random_ngeo(SplitMix &g)
{
    Ngeo          k;
    const int64_t r = rrange(g);
    const Point   c(ispan(g, -r, r), ispan(g, -r, r));
    do {
        k.pts = rshape(g, c, r, false, &k.simple);
    } while (k.pts.size() < 3 || !has_edge(k.pts));
    k.other           = rshape(g, Point(c.x() + ispan(g, -r / 2, r / 2), c.y() + ispan(g, -r / 2, r / 2)), r * 2, true);
    k.set_rotation    = g.below(4) == 0;
    k.rotation        = g.below(3) == 0 ? 0. : span(g, -7., 7.);
    k.set_translation = g.below(2) == 0;
    k.translation     = Point(ispan(g, -r, r), ispan(g, -r, r));
    k.set_inflation   = g.below(2) == 0;
    k.inflation       = g.below(3) == 0 ? 0 : ispan(g, -r / 20, r / 10);
    k.box             = rbox(g, 2 * r);
    k.circle_center   = Point(ispan(g, -r, r), ispan(g, -r, r));
    k.radius          = span(g, 0., 3. * double(r));
    k.pt              = g.below(3) == 0 ? k.pts[size_t(g.below(k.pts.size()))] : Point(ispan(g, -2 * r, 2 * r), ispan(g, -2 * r, 2 * r));
    static const double acc[] = {1., 0.65, 0.5, 0.3, 0.1, 0.};
    k.accuracy                = acc[g.below(6)];
    const size_t nd           = size_t(g.below(6));
    for (size_t i = 0; i < nd; ++i)
        k.distances.push_back(g.below(5) == 0 ? (g.below(2) == 0 ? 1. : 0.) : span(g, 0., g.below(6) == 0 ? 2.5 : 1.));
    return k;
}

void record_kind(Sets &sets, const char (&kind)[5], size_t edges_and_random, size_t committed, size_t committed_bytes,
                 const std::function<void(Payload &, size_t)> &make)
{
    size_t taken = 0;
    for (size_t i = 0; i < edges_and_random; ++i) {
        Payload p;
        make(p, i);
        sets.full.add(kind, p);
        if (taken < committed && p.bytes.size() <= committed_bytes) {
            sets.committed.add(kind, p);
            ++taken;
        }
    }
}

} // namespace

void record_nest(Sets &sets)
{
    SplitMix g(0x52545245u);
    record_kind(sets, "RTRE", 400, 12, 4096, [&g](Payload &p, size_t i) { rtre(p, g, i < 4 ? 8 + 40 * i : size_t(1 + g.below(i % 5 == 0 ? 400 : 60))); });

    SplitMix h(0x4E465043u);
    record_kind(sets, "NFPC", 3000, 80, 1536, [&h](Payload &p, size_t i) {
        Points a, b;
        if (i == 0) { // two clockwise squares, as Arrange.cpp makes its contours
            a = {Point(0, 0), Point(0, 10), Point(10, 10), Point(10, 0)};
            b = {Point(0, 0), Point(0, 4), Point(4, 4), Point(4, 0)};
        } else if (i == 1) { // counter-clockwise triangle and square
            a = {Point(0, 0), Point(20, 0), Point(10, 15)};
            b = {Point(0, 0), Point(6, 0), Point(6, 6), Point(0, 6)};
        } else if (i == 2) { // parallel edges of equal direction in both
            a = {Point(0, 0), Point(10, 0), Point(20, 0), Point(20, 10), Point(0, 10)};
            b = {Point(5, 5), Point(15, 5), Point(15, 15), Point(5, 15)};
        } else {
            const int64_t r = std::min(rrange(h), int64_t(200000000));
            do {
                a = rshape(h, Point(ispan(h, -r, r), ispan(h, -r, r)), r);
            } while (!has_edge(a));
            do {
                b = rshape(h, Point(ispan(h, -r, r), ispan(h, -r, r)), r);
            } while (!has_edge(b));
            if (h.below(2) == 0) { // the hulls, as the placer's shapes mostly are
                a = libnest2d::shapelike::convexHull(shape(a)).contour.points;
                b = libnest2d::shapelike::convexHull(shape(b)).contour.points;
                if (!has_edge(a) || !has_edge(b)) {
                    a = {Point(0, 0), Point(r, 0), Point(r, r)};
                    b = {Point(0, 0), Point(r / 2, r), Point(0, r)};
                }
            }
        }
        const int64_t rb = 400000000;
        Box           bed(Point(ispan(h, -rb, 0), ispan(h, -rb, 0)), Point(ispan(h, 0, rb), ispan(h, 0, rb)));
        if (h.below(8) == 0)
            bed = Box(Point(0, 0), Point(ispan(h, 0, 20), ispan(h, 0, 20))); // smaller than the shape
        nfpc(p, a, b, bed);
    });

    SplitMix k(0x4E47454Fu);
    record_kind(sets, "NGEO", 1500, 30, 3072, [&k](Payload &p, size_t i) {
        Ngeo c;
        if (i == 0) {
            c.pts       = {Point(0, 0), Point(0, 10000000), Point(20000000, 10000000), Point(20000000, 0)};
            c.other     = {Point(-1, -1), Point(30000000, -1), Point(30000000, 30000000), Point(-1, 30000000)};
            c.box       = Box(Point(-5000000, -5000000), Point(50000000, 50000000));
            c.radius    = 30000000.;
            c.pt        = Point(0, 5000000);
            c.distances = {0., 0.25, 0.5, 1.};
        } else if (i == 1) { // inflated and moved, as Arrange.cpp's items are
            c.pts             = {Point(0, 0), Point(0, 3000000), Point(3000000, 3000000), Point(3000000, 0)};
            c.other           = c.pts;
            c.set_inflation   = true;
            c.inflation       = 1000000;
            c.set_translation = true;
            c.translation     = Point(100000000, 50000000);
            c.box             = Box(Point(0, 0), Point(256000000, 256000000));
            c.circle_center   = Point(128000000, 128000000);
            c.radius          = 100000000.;
            c.pt              = Point(101500000, 51500000);
            c.accuracy        = 0.65;
        } else
            c = random_ngeo(k);
        ngeo(p, c);
    });
}

} // namespace probe
