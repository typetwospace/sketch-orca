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
// index.sop1's AABBTreeLines records (parity/sop1/index.md), which orca-index replays for the piece
// index-1:
//
//   IXLT  build_aabb_tree_over_indexed_lines over Lines (coord_t, as PerimeterGenerator.cpp,
//         PrintObject.cpp, Fill.cpp and LineSplit.cpp build LinesDistancer<Line>) or over Linefs
//         (doubles): the nodes; then for each point LinesDistancer's distance_from_lines_extra,
//         unsigned and signed, outside(), squared_distance_to_indexed_lines with a distance limit
//         and all_lines_in_radius; then for each line intersections_with_line, unsorted and sorted.
//
// The lines: closed contours (squares, a square with a hole, touching squares, stars, seeded
// polygons, and the sort case: a zigzag that a query line meets at 34 points in tied pairs, so the
// sorted intersections_with_line runs MSVC's introsort), duplicated, zero-length and collinear
// lines, open segments and no line at all, in
// scaled coordinates and as doubles. Points sit inside, outside, on lines and at vertices; query
// lines cross, touch at vertices and run parallel. Each record holds its inputs and then the
// engine's results. Recorded by orca-o3-probe-index, 2026-11-13.
#include "index.hpp"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <functional>
#include <initializer_list>
#include <limits>
#include <tuple>
#include <type_traits>
#include <vector>

#include "libslic3r/AABBTreeLines.hpp"
#include "libslic3r/Line.hpp"

namespace probe {

namespace {

using namespace Slic3r;
using namespace probe::ix;

template<class S> struct LtPoint
{
    Vec<2, S> pt;
    double    max_sqr;
    double    radius;
};

template<class LineT> void put_line(Payload &p, const LineT &l)
{
    put_scalar(p, l.a.x());
    put_scalar(p, l.a.y());
    put_scalar(p, l.b.x());
    put_scalar(p, l.b.y());
}

template<class V> void put_hits(Payload &p, const std::vector<std::pair<V, size_t>> &hits)
{
    p.u64(hits.size());
    for (const auto &h : hits) {
        put_scalar(p, h.first.x());
        put_scalar(p, h.first.y());
        p.u64(h.second);
    }
}

template<class LineT> void ixlt(Payload &p, const std::vector<LineT> &lines, const std::vector<LtPoint<typename LineT::Scalar>> &points, const std::vector<LineT> &queries)
{
    using S = typename LineT::Scalar;
    p.u8(std::is_same<S, double>::value ? 1 : 0);
    p.u64(lines.size());
    for (const LineT &l : lines)
        put_line(p, l);
    const auto tree = AABBTreeLines::build_aabb_tree_over_indexed_lines(lines);
    put_nodes(p, tree);
    AABBTreeLines::LinesDistancer<LineT> dist(lines);

    p.u64(points.size());
    for (const LtPoint<S> &q : points) {
        put_scalar(p, q.pt.x());
        put_scalar(p, q.pt.y());
        p.f64(q.max_sqr).f64(q.radius);
        {
            const auto r = dist.template distance_from_lines_extra<false>(q.pt);
            p.f64(std::get<0>(r)).u64(std::get<1>(r)).f64(std::get<2>(r).x()).f64(std::get<2>(r).y());
        }
        {
            const auto r = dist.template distance_from_lines_extra<true>(q.pt);
            p.f64(std::get<0>(r)).u64(std::get<1>(r)).f64(std::get<2>(r).x()).f64(std::get<2>(r).y());
        }
        p.i32(dist.outside(q.pt));
        {
            const Vec2d  pd  = q.pt.template cast<double>();
            size_t       idx = NPOS;
            Vec2d        hit = Vec2d::Zero();
            const double r   = AABBTreeLines::squared_distance_to_indexed_lines(lines, tree, pd, idx, hit, q.max_sqr);
            p.f64(r).u64(idx).f64(hit.x()).f64(hit.y());
        }
        put_idx(p, dist.all_lines_in_radius(q.pt, q.radius));
    }

    p.u64(queries.size());
    for (const LineT &l : queries) {
        put_line(p, l);
        put_hits(p, dist.template intersections_with_line<false>(l));
        put_hits(p, dist.template intersections_with_line<true>(l));
    }
}

// A closed contour's lines.
void contour(Lines &out, const Points &pts)
{
    for (size_t i = 0; i < pts.size(); ++i)
        out.emplace_back(pts[i], pts[(i + 1) % pts.size()]);
}

Points square(int64_t x0, int64_t y0, int64_t x1, int64_t y1, bool ccw = true)
{
    Points pts{Point(x0, y0), Point(x1, y0), Point(x1, y1), Point(x0, y1)};
    if (!ccw)
        std::reverse(pts.begin(), pts.end());
    return pts;
}

Points star(SplitMix &g, int64_t cx, int64_t cy, int64_t r, size_t n)
{
    Points pts;
    for (size_t i = 0; i < n; ++i) {
        const double a = 2. * 3.141592653589793 * double(i) / double(n);
        const double k = i % 2 == 0 ? 1. : real(g, 0.3, 0.9);
        pts.emplace_back(cx + int64_t(std::floor(double(r) * k * std::cos(a))), cy + int64_t(std::floor(double(r) * k * std::sin(a))));
    }
    return pts;
}

// Points and query lines for a set of lines: vertices, the point beside a vertex at its height,
// midpoints, points a little off them, the bounding box's centre and corners, seeded ones; query
// lines between those, along an edge, through vertices and across everything.
template<class S> struct LtQueries
{
    std::vector<LtPoint<S>> points;
    std::vector<Vec<2, S>>  ends;
};

LtQueries<coord_t> lt_queries(SplitMix &g, const Lines &lines, int64_t lo, int64_t hi, size_t npoints)
{
    LtQueries<coord_t> q;
    const double       span = double(hi - lo);
    auto               radius = [&]() { return rnd(g, 0, 5) == 0 ? 0. : real(g, 0, span / 3); };
    auto               limit  = [&]() { return rnd(g, 0, 2) == 0 ? std::numeric_limits<double>::infinity() : std::pow(real(g, 0, span / 2), 2); };
    for (size_t i = 0; i < npoints; ++i) {
        Point pt;
        const int64_t k = lines.empty() ? 5 : rnd(g, 0, 5);
        const Line   *l = lines.empty() ? nullptr : &lines[size_t(g.below(lines.size()))];
        switch (k) {
        case 0: pt = l->a; break;
        case 1: pt = Point(l->a.x() - rnd(g, 1, 50), l->a.y()); break;
        case 2: pt = Point((l->a.x() + l->b.x()) / 2, (l->a.y() + l->b.y()) / 2); break;
        case 3: pt = Point((l->a.x() + l->b.x()) / 2 + rnd(g, -30, 30), (l->a.y() + l->b.y()) / 2 + rnd(g, -30, 30)); break;
        case 4: pt = Point((lo + hi) / 2, (lo + hi) / 2); break;
        default: pt = Point(rnd(g, lo, hi), rnd(g, lo, hi)); break;
        }
        q.points.push_back(LtPoint<coord_t>{pt, limit(), radius()});
        q.ends.push_back(pt);
    }
    return q;
}

template<class LineT, class S> std::vector<LineT> lt_lines(SplitMix &g, const std::vector<Vec<2, S>> &ends, const std::vector<LineT> &lines, size_t n)
{
    std::vector<LineT> out;
    for (size_t i = 0; i < n && !ends.empty(); ++i) {
        if (i % 4 == 3 && !lines.empty()) {
            const LineT &l = lines[size_t(g.below(lines.size()))];
            out.push_back(i % 8 == 3 ? l : LineT(l.b, l.a));
        } else
            out.push_back(LineT(ends[size_t(g.below(ends.size()))], ends[size_t(g.below(ends.size()))]));
    }
    return out;
}

// The same lines, points and queries as doubles, scaled by `k`.
std::vector<Linef> as_linef(const Lines &lines, double k)
{
    std::vector<Linef> out;
    for (const Line &l : lines)
        out.emplace_back(l.a.cast<double>() * k, l.b.cast<double>() * k);
    return out;
}

void add_case(std::vector<std::function<void(Payload &)>> &e, const Lines &lines, uint64_t seed, int64_t lo, int64_t hi, bool also_double)
{
    e.push_back([lines, seed, lo, hi](Payload &p) {
        SplitMix                 g(seed);
        const LtQueries<coord_t> q = lt_queries(g, lines, lo, hi, 14);
        ixlt(p, lines, q.points, lt_lines(g, q.ends, lines, 8));
    });
    if (!also_double)
        return;
    e.push_back([lines, seed, lo, hi](Payload &p) {
        SplitMix                 g(seed);
        const LtQueries<coord_t> q = lt_queries(g, lines, lo, hi, 14);
        const double             k = 0.001;
        const std::vector<Linef> lf = as_linef(lines, k);
        std::vector<LtPoint<double>> pts;
        std::vector<Vec2d>           ends;
        for (const LtPoint<coord_t> &x : q.points) {
            const Vec2d d = x.pt.cast<double>() * k + Vec2d(0.25 * k, 0.);
            pts.push_back(LtPoint<double>{pts.size() % 3 == 0 ? Vec2d(x.pt.cast<double>() * k) : d, x.max_sqr * k * k, x.radius * k});
            ends.push_back(pts.back().pt);
        }
        ixlt(p, lf, pts, lt_lines(g, ends, lf, 8));
    });
}

// The sort case: a closed zigzag with 17 vertices on the line x = 1000 (between them it steps out to
// x = 1008; it closes through x = 1016). Each of those 17 vertices ends two lines, so a query line
// along x = 1000 meets 34 points in 17 tied pairs, and the sorted intersections_with_line's std::sort
// runs MSVC's introsort, not only its insertion sort. The query lines are 64 and 128 long and every
// coordinate a small integer, so each crossing's parameter is an exact binary fraction and the two
// points of a pair are equal to the bit.
Lines zigzag()
{
    const coord_t c = 1000, w = 8;
    Points        pts;
    for (coord_t k = 0; k < 17; ++k) {
        pts.emplace_back(c, 2 * k);
        if (k < 16)
            pts.emplace_back(c + w, 2 * k + 1);
    }
    pts.emplace_back(c + 2 * w, 32);
    pts.emplace_back(c + 2 * w, 0);
    Lines out;
    contour(out, pts);
    return out;
}

} // namespace

void record_index_lines(Sets &sets)
{
    using Fn = std::function<void(Payload &)>;
    std::vector<Fn> e;
    auto            lines_of = [](std::initializer_list<Points> contours) {
        Lines out;
        for (const Points &c : contours)
            contour(out, c);
        return out;
    };
    // The sort case (zigzag), with no points and two query lines: up along x = 1000 from its first
    // vertex, and down across all of it. The Line record is pinned: it goes into both sets whatever
    // its size, first. The same lines as doubles (times 0.25, still exact) are an ordinary case.
    std::vector<Fn> pinned;
    {
        const Lines z = zigzag();
        pinned.push_back([z](Payload &p) { ixlt(p, z, std::vector<LtPoint<coord_t>>{}, Lines{Line(Point(1000, 0), Point(1000, 64)), Line(Point(1000, 64), Point(1000, -64))}); });
        e.push_back([z](Payload &p) {
            ixlt(p, as_linef(z, 0.25), std::vector<LtPoint<double>>{},
                 std::vector<Linef>{Linef(Vec2d(250., 0.), Vec2d(250., 16.)), Linef(Vec2d(250., 16.), Vec2d(250., -16.))});
        });
    }
    SplitMix s(0x1B0C0000u);
    add_case(e, {}, 0x1B0C0001u, -500, 500, true);
    add_case(e, lines_of({square(0, 0, 1000, 1000)}), 0x1B0C0002u, -500, 1500, true);
    add_case(e, lines_of({square(0, 0, 1000, 1000), square(250, 250, 750, 750, false)}), 0x1B0C0003u, -200, 1200, true);
    add_case(e, lines_of({square(0, 0, 1000, 1000), square(1000, 0, 2000, 1000)}), 0x1B0C0004u, -200, 2200, true);
    add_case(e, lines_of({star(s, 0, 0, 100000, 10), star(s, 300000, 0, 50000, 7)}), 0x1B0C0005u, -150000, 400000, true);
    {
        Lines l = lines_of({square(0, 0, 100, 100)});
        l.push_back(l[1]);
        l.push_back(l[1]);
        l.emplace_back(Point(50, 50), Point(50, 50));
        l.emplace_back(Point(0, 200), Point(100, 200));
        l.emplace_back(Point(100, 200), Point(200, 200));
        l.emplace_back(Point(50, 200), Point(150, 200));
        add_case(e, l, 0x1B0C0006u, -50, 250, true);
    }
    add_case(e, Lines{Line(Point(0, 0), Point(10, 10))}, 0x1B0C0007u, -20, 30, true);
    add_case(e, Lines{Line(Point(-100000000, 0), Point(100000000, 0)), Line(Point(0, -100000000), Point(0, 100000000))}, 0x1B0C0008u, -120000000, 120000000, true);
    add_case(e, lines_of({square(0, 0, 400, 400), square(100, 100, 300, 300, false), square(150, 150, 250, 250)}), 0x1B0C0009u, -50, 450, true);

    SplitMix g(0x1B0C1000u);
    std::vector<Fn> random;
    for (int i = 0; i < 400; ++i) {
        const int64_t r = int64_t(1) << rnd(g, 4, 27);
        Lines         lines;
        const int64_t kind = rnd(g, 0, 3);
        if (kind == 0) {
            const size_t n = size_t(rnd(g, 0, 30));
            for (size_t j = 0; j < n; ++j)
                lines.emplace_back(Point(rnd(g, -r, r), rnd(g, -r, r)), Point(rnd(g, -r, r), rnd(g, -r, r)));
        } else {
            for (int64_t c = 0; c < kind; ++c)
                contour(lines, star(g, rnd(g, -r, r), rnd(g, -r, r), std::max<int64_t>(4, r / 2), size_t(rnd(g, 3, 24))));
        }
        add_case(random, lines, g.next(), -r - r / 2, r + r / 2, i % 2 == 0);
    }
    record_cases(sets, "IXLT", pinned, 0, 0, std::numeric_limits<size_t>::max(), [](Payload &) {});
    size_t next = 0;
    record_cases(sets, "IXLT", e, random.size(), 12, 4096, [&](Payload &p) { random[next++](p); });
}

} // namespace probe
