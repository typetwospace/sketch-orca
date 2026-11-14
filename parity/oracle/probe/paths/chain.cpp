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
// paths.sop1's records of the piece paths-1 (parity/sop1/paths.md), which orca-paths replays:
// ShortestPath's chaining, reached through its public entry points (the chain_segments_*
// templates live only in ShortestPath.cpp, so every instance the engine compiles is reached
// through the function that instantiates it, as the O2 probes reach static code).
//
//   PSCP  chain_points (chain_segments_greedy, closest point and greedy, no reversal constraint).
//   PSCX  chain_expolygons.
//   PSXP  chain_extrusion_paths and chain_and_reorder_extrusion_paths.
//   PSXE  chain_extrusion_entities (chain_segments_greedy_constrained_reversals: reversal
//         constraints, the reversed retry and the closest-point fallback) and
//         chain_and_reorder_extrusion_entities (which drops entities without end points first).
//   PSPL  chain_polylines (chain_segments_greedy2 and, with no start point, the two-exchange
//         improvement with segment flipping).
//   PSUO  union_pt_chained_outside_in (union_pt, traverse_pt_outside_in, chain_clipper_polynodes).
//   PSLN  chain_lines.
//
// The inputs are what domain A hands these functions: island and surface centers, infill lines
// (parallel, with equal lengths and aligned ends, so with ties), perimeter loops, nested contours,
// clusters of nearby items far from each other, duplicates and single items; all made here.
// Added 2026-11-13 by orca-o3-probe-paths.
#include "slic.hpp"

#include <map>
#include <vector>

#include "libslic3r/ClipperUtils.hpp"
#include "libslic3r/ExtrusionEntity.hpp"
#include "libslic3r/ExtrusionEntityCollection.hpp"
#include "libslic3r/ShortestPath.hpp"

namespace probe {
namespace paths {

namespace {

// chain_lines keeps the square of the epsilon of its first call in a function-local static
// (ShortestPath.cpp, chain_lines), so every later call compares against this one.
constexpr double CHAIN_LINES_FIRST_EPS = 10000.;

void put_start(Payload &p, bool has, const Point &s)
{
    p.boolean(has);
    put(p, has ? s : Point(0, 0));
}

void put_chain(Payload &p, const std::vector<std::pair<size_t, bool>> &chain)
{
    p.u32(uint32_t(chain.size()));
    for (const auto &c : chain)
        p.u64(c.first).boolean(c.second);
}

void put_order(Payload &p, const std::vector<size_t> &order)
{
    std::vector<uint64_t> v(order.begin(), order.end());
    p.u64s(v);
}

void pscp(Payload &p, const Points &pts, bool has, const Point &s)
{
    put(p, pts);
    put_start(p, has, s);
    Point start = s;
    put_order(p, chain_points(pts, has ? &start : nullptr));
}

void pscx(Payload &p, const ExPolygons &es)
{
    put(p, es);
    put_order(p, chain_expolygons(es));
}

ExtrusionPath make_path(const Points3 &pts)
{
    ExtrusionPath path(erPerimeter, 0.1, 0.4f, 0.2f);
    path.polyline = Polyline3(pts);
    return path;
}

void psxp(Payload &p, const std::vector<Points3> &polys, bool has, const Point &s)
{
    put_start(p, has, s);
    p.u32(uint32_t(polys.size()));
    for (const Points3 &q : polys)
        put(p, q);
    std::vector<ExtrusionPath> paths;
    for (const Points3 &q : polys)
        paths.push_back(make_path(q));
    put_chain(p, chain_extrusion_paths(paths, has ? &s : nullptr));
    chain_and_reorder_extrusion_paths(paths, has ? &s : nullptr);
    p.u32(uint32_t(paths.size()));
    for (const ExtrusionPath &q : paths)
        put(p, q.polyline.points);
}

// An extrusion entity of PSXE: its type (parity/sop1/paths.md, PSXE) and its paths' points.
struct Ent
{
    uint8_t              type = 0;
    std::vector<Points3> paths;
};

bool has_end_points(const Ent &e)
{
    switch (e.type) {
    case 10: case 11: case 12: case 13: case 14: return false;
    default: return true;
    }
}

ExtrusionEntity *build(const Ent &e)
{
    switch (e.type) {
    case 0: return new ExtrusionPath(make_path(e.paths[0]));
    case 1: {
        auto *q = new ExtrusionPath(make_path(e.paths[0]));
        q->set_reverse();
        return q;
    }
    case 2: {
        auto *q     = new ExtrusionPathOriented(erPerimeter, 0.1, 0.4f, 0.2f);
        q->polyline = Polyline3(e.paths[0]);
        return q;
    }
    case 3: return new ExtrusionLoop(make_path(e.paths[0]));
    case 4: {
        ExtrusionPaths ps;
        for (const Points3 &q : e.paths)
            ps.push_back(make_path(q));
        return new ExtrusionLoop(std::move(ps));
    }
    case 5: {
        ExtrusionPaths ps;
        for (const Points3 &q : e.paths)
            ps.push_back(make_path(q));
        return new ExtrusionMultiPath(ps);
    }
    case 6: {
        ExtrusionPath q = make_path(e.paths[0]);
        q.set_reverse();
        return new ExtrusionMultiPath(q);
    }
    case 7:
    case 8:
    case 9:
    case 11:
    case 14: {
        auto *c = new ExtrusionEntityCollection();
        for (const Points3 &q : e.paths)
            c->append(make_path(q));
        if (e.type == 8)
            c->no_sort = true;
        if (e.type == 9)
            c->set_reverse();
        if (e.type == 14)
            c->append(make_path(Points3{}));
        return c;
    }
    case 10: return new ExtrusionPath(make_path(Points3{}));
    case 12: {
        ExtrusionPaths ps;
        for (const Points3 &q : e.paths)
            ps.push_back(make_path(q));
        ps.push_back(make_path(Points3{}));
        return new ExtrusionMultiPath(ps);
    }
    default: return new ExtrusionLoop();
    }
}

void psxe(Payload &p, const std::vector<Ent> &ents, bool has, const Point &s)
{
    put_start(p, has, s);
    p.u32(uint32_t(ents.size()));
    bool all = true;
    for (const Ent &e : ents) {
        p.u8(e.type).u32(uint32_t(e.paths.size()));
        for (const Points3 &q : e.paths)
            put(p, q);
        all = all && has_end_points(e);
    }
    p.boolean(all);
    if (all) {
        std::vector<ExtrusionEntity *> v;
        for (const Ent &e : ents)
            v.push_back(build(e));
        put_chain(p, chain_extrusion_entities(v, has ? &s : nullptr));
        for (ExtrusionEntity *q : v)
            delete q;
    }
    std::vector<ExtrusionEntity *>      v;
    std::map<const ExtrusionEntity *, uint32_t> index;
    for (const Ent &e : ents) {
        v.push_back(build(e));
        index[v.back()] = uint32_t(v.size() - 1);
    }
    const std::vector<ExtrusionEntity *> owned = v;
    chain_and_reorder_extrusion_entities(v, has ? &s : nullptr);
    p.u32(uint32_t(v.size()));
    for (const ExtrusionEntity *q : v) {
        p.u32(index[q]);
        put(p, q->first_point());
        put(p, q->last_point());
    }
    for (ExtrusionEntity *q : owned)
        delete q;
}

void pspl(Payload &p, const Polylines &pls, bool has, const Point &s)
{
    put_start(p, has, s);
    put(p, pls);
    put(p, chain_polylines(Polylines(pls), has ? &s : nullptr));
}

void psuo(Payload &p, const Polygons &polys)
{
    put(p, polys);
    put(p, union_pt_chained_outside_in(polys));
}

void psln(Payload &p, const Lines &lines, double eps)
{
    p.u32(uint32_t(lines.size()));
    for (const Line &l : lines) {
        put(p, l.a);
        put(p, l.b);
    }
    p.f64(eps).f64(CHAIN_LINES_FIRST_EPS);
    put(p, chain_lines(lines, eps));
}

// ------------------------------------------------------------ the shapes

// Points as chain_points gets them: scattered, on a grid (ties), in clusters, collinear, or
// with duplicates.
Points chain_input(SplitMix &g, size_t n)
{
    Points        pts;
    const int64_t r = std::min(range(g), int64_t(300000000));
    switch (g.below(5)) {
    case 0:
        for (size_t i = 0; i < n; ++i)
            pts.push_back(rpoint(g, r));
        break;
    case 1: {
        const size_t  cols = size_t(rnd(g, 1, 8));
        const int64_t s    = std::max<int64_t>(1, r / 8);
        const Point   o    = rpoint(g, r);
        for (size_t i = 0; i < n; ++i)
            pts.emplace_back(o.x() + int64_t(i % cols) * s, o.y() + int64_t(i / cols) * s);
        break;
    }
    case 2: {
        const size_t  k  = size_t(rnd(g, 1, 5));
        const int64_t cr = std::max<int64_t>(1, r / 100);
        std::vector<Point> centers;
        for (size_t i = 0; i < k; ++i)
            centers.push_back(rpoint(g, r));
        for (size_t i = 0; i < n; ++i) {
            const Point &c = centers[size_t(g.below(k))];
            pts.emplace_back(c.x() + rnd(g, -cr, cr), c.y() + rnd(g, -cr, cr));
        }
        break;
    }
    case 3: {
        const Point   o = rpoint(g, r);
        const int64_t dx = rnd(g, -r / 10, r / 10), dy = one_in(g, 2) ? 0 : rnd(g, -r / 10, r / 10);
        for (size_t i = 0; i < n; ++i)
            pts.emplace_back(o.x() + int64_t(i) * dx, o.y() + int64_t(i) * dy);
        if (one_in(g, 2))
            std::reverse(pts.begin(), pts.end());
        break;
    }
    default:
        for (size_t i = 0; i < n; ++i)
            pts.push_back(i > 0 && one_in(g, 2) ? pts[size_t(g.below(i))] : rpoint(g, r));
        break;
    }
    return pts;
}

// Polylines as the fills and walls hand them over: infill lines (parallel, aligned ends, either
// way round), short random segments, walks, closed loops, clustered dashes.
Polylines segments(SplitMix &g, size_t n)
{
    Polylines     out;
    const int64_t r = std::min(range(g), int64_t(300000000));
    const int     kind = int(g.below(5));
    const int64_t s    = std::max<int64_t>(2, r / 20);
    for (size_t i = 0; i < n; ++i) {
        Points pts;
        switch (kind) {
        case 0: { // rectilinear or monotonic infill lines
            const int64_t y  = int64_t(i) * s;
            const int64_t xa = one_in(g, 2) ? -r : rnd(g, -r, 0), xb = one_in(g, 2) ? r : rnd(g, 1, r);
            pts              = Points{Point(xa, y), Point(xb, y)};
            if (one_in(g, 2))
                std::reverse(pts.begin(), pts.end());
            break;
        }
        case 1: pts = Points{rpoint(g, r), rpoint(g, r)}; break;
        case 2: {
            Point c = rpoint(g, r);
            pts.push_back(c);
            for (int k = 0, m = int(rnd(g, 1, 6)); k < m; ++k) {
                c = Point(c.x() + rnd(g, -s, s), c.y() + rnd(g, -s, s));
                pts.push_back(c);
            }
            break;
        }
        case 3: { // a closed loop (first point repeated)
            pts = star(g, rpoint(g, r), std::max<int64_t>(3, r / 10), size_t(rnd(g, 3, 8)), one_in(g, 3));
            pts.push_back(pts.front());
            break;
        }
        default: { // a dash in one of a few clusters
            const Point c(int64_t(i % 3) * r / 2, int64_t(i % 2) * r / 3);
            const Point a(c.x() + rnd(g, -s, s), c.y() + rnd(g, -s, s));
            pts = Points{a, Point(a.x() + rnd(g, -s, s), a.y() + rnd(g, -s, s))};
            break;
        }
        }
        out.emplace_back(std::move(pts));
    }
    return out;
}

Points3 flat3(const Points &pts, int64_t z)
{
    Points3 out;
    for (const Point &q : pts)
        out.emplace_back(q.x(), q.y(), z);
    return out;
}

} // namespace

void record_chain(Sets &sets)
{
    // PSLN first: chain_lines' static is set by its first call, which is this record's.
    {
        SplitMix g(0x9C770007u);
        Emit     e(sets, "PSLN", 16, 2048);
        const double eps = CHAIN_LINES_FIRST_EPS;
        e.hand([&] { Payload p; psln(p, Lines{Line(Point(0, 0), Point(100000, 0)), Line(Point(100005, 3), Point(200000, 0)), Line(Point(300000, 0), Point(200004, 2))}, eps); return p; }());
        e.hand([&] { Payload p; psln(p, Lines{}, eps); return p; }());
        e.hand([&] { Payload p; psln(p, Lines{Line(Point(0, 0), Point(0, 0))}, eps); return p; }());
        e.hand([&] { Payload p; psln(p, Lines{Line(Point(0, 0), Point(1000000, 0)), Line(Point(1000000, 0), Point(1000000, 1000000)), Line(Point(1000000, 1000000), Point(0, 0))}, eps); return p; }());
        e.hand([&] { Payload p; psln(p, Lines{Line(Point(0, 0), Point(100000, 0)), Line(Point(150000, 0), Point(200000, 0))}, 100000.); return p; }());
        for (int i = 0; i < 400; ++i) {
            // Pieces of a few polylines, cut into lines, each end moved a little, shuffled and
            // some turned round.
            Lines         lines;
            const int64_t jitter = one_in(g, 3) ? 0 : rnd(g, 0, 15000);
            for (int k = 0, m = int(rnd(g, 1, 4)); k < m; ++k) {
                Point c = rpoint(g, 100000000);
                for (int j = 0, n = int(rnd(g, 1, 12)); j < n; ++j) {
                    const Point d(c.x() + rnd(g, -2000000, 2000000), c.y() + rnd(g, -2000000, 2000000));
                    Line        l(Point(c.x() + rnd(g, -jitter, jitter), c.y() + rnd(g, -jitter, jitter)), d);
                    if (one_in(g, 2))
                        std::swap(l.a, l.b);
                    lines.push_back(l);
                    c = d;
                }
            }
            for (size_t k = lines.size(); k > 1; --k)
                std::swap(lines[k - 1], lines[size_t(g.below(k))]);
            Payload p;
            psln(p, lines, one_in(g, 8) ? real(g, 1000., 50000.) : eps);
            e.random(p);
        }
    }

    // PSCP
    {
        SplitMix g(0x9C770001u);
        Emit     e(sets, "PSCP", 32, 2048);
        e.hand([] { Payload p; pscp(p, Points{}, false, Point(0, 0)); return p; }());
        e.hand([] { Payload p; pscp(p, Points{Point(5, 5)}, true, Point(0, 0)); return p; }());
        e.hand([] { Payload p; pscp(p, Points{Point(0, 0), Point(10, 0)}, false, Point(0, 0)); return p; }());
        e.hand([] { Payload p; pscp(p, Points{Point(0, 0), Point(10, 0)}, true, Point(11, 0)); return p; }());
        {
            Points grid;
            for (int y = 0; y < 3; ++y)
                for (int x = 0; x < 3; ++x)
                    grid.emplace_back(x * 1000, y * 1000);
            e.hand([&] { Payload p; pscp(p, grid, false, Point(0, 0)); return p; }());
            e.hand([&] { Payload p; pscp(p, grid, true, Point(1000, 1000)); return p; }());
        }
        e.hand([] { Payload p; pscp(p, Points{Point(7, 7), Point(7, 7), Point(7, 7), Point(7, 7)}, false, Point(0, 0)); return p; }());
        e.hand([] { Payload p; pscp(p, Points{Point(0, 0), Point(10, 0), Point(20, 0), Point(30, 0), Point(40, 0), Point(50, 0)}, true, Point(50, 0)); return p; }());
        for (int i = 0; i < 1200; ++i) {
            const size_t n = size_t(one_in(g, 10) ? rnd(g, 60, 250) : rnd(g, 2, 40));
            const Points pts = chain_input(g, n);
            Payload      p;
            pscp(p, pts, one_in(g, 2), rpoint(g, 300000000));
            e.random(p);
        }
    }

    // PSCX
    {
        SplitMix g(0x9C770002u);
        Emit     e(sets, "PSCX", 16, 2048);
        e.hand([] { Payload p; pscx(p, ExPolygons{}); return p; }());
        e.hand([] { Payload p; pscx(p, ExPolygons{ExPolygon(rect(0, 0, 100, 100))}); return p; }());
        {
            ExPolygons es;
            for (int k = 0; k < 3; ++k)
                es.emplace_back(rect(k * 2000, 0, k * 2000 + 1000, 1000));
            ExPolygon holed(rect(0, 3000, 5000, 8000));
            holed.holes.emplace_back(Points{Point(1000, 4000), Point(1000, 7000), Point(4000, 7000), Point(4000, 4000)});
            es.push_back(holed);
            e.hand([&] { Payload p; pscx(p, es); return p; }());
        }
        for (int i = 0; i < 600; ++i) {
            const size_t  n  = size_t(rnd(g, 1, 30));
            const Points  cs = chain_input(g, n);
            const int64_t r  = std::max<int64_t>(10, std::min(range(g), int64_t(30000000)));
            ExPolygons    es;
            for (const Point &c : cs) {
                ExPolygon ex(star(g, c, r, size_t(rnd(g, 3, 9)), false));
                if (one_in(g, 4))
                    ex.holes.emplace_back(star(g, c, std::max<int64_t>(3, r / 10), 4, true));
                es.push_back(std::move(ex));
            }
            Payload p;
            pscx(p, es);
            e.random(p);
        }
    }

    // PSXP
    {
        SplitMix g(0x9C770003u);
        Emit     e(sets, "PSXP", 12, 2048);
        e.hand([] { Payload p; psxp(p, {}, false, Point(0, 0)); return p; }());
        e.hand([] { Payload p; psxp(p, {Points3{Point3(0, 0, 0), Point3(100, 0, 0)}}, true, Point(101, 0)); return p; }());
        e.hand([] { Payload p; psxp(p, {Points3{Point3(0, 0, 0), Point3(100, 0, 0)}}, false, Point(0, 0)); return p; }());
        e.hand([] { Payload p; psxp(p, {Points3{Point3(0, 0, 1)}, Points3{Point3(10, 0, 2)}, Points3{Point3(5, 0, 3)}}, true, Point(0, 0)); return p; }());
        {
            std::vector<Points3> lines;
            for (int k = 0; k < 6; ++k)
                lines.push_back(k % 2 ? Points3{Point3(1000, k * 100, 0), Point3(0, k * 100, 0)} : Points3{Point3(0, k * 100, 0), Point3(1000, k * 100, 0)});
            e.hand([&] { Payload p; psxp(p, lines, false, Point(0, 0)); return p; }());
            e.hand([&] { Payload p; psxp(p, lines, true, Point(1000, 500)); return p; }());
        }
        for (int i = 0; i < 900; ++i) {
            const Polylines      pls = segments(g, size_t(one_in(g, 10) ? rnd(g, 40, 120) : rnd(g, 2, 30)));
            std::vector<Points3> polys;
            for (const Polyline &pl : pls)
                polys.push_back(lift(one_in(g, 10) ? Points{pl.points.front()} : pl.points, g, one_in(g, 2)));
            Payload p;
            psxp(p, polys, one_in(g, 2), rpoint(g, 300000000));
            e.random(p);
        }
    }

    // PSXE
    {
        SplitMix g(0x9C770004u);
        Emit     e(sets, "PSXE", 14, 3072);
        const int64_t z0 = 0;
        auto open = [z0](int64_t x0, int64_t y, int64_t x1) { return Points3{Point3(x0, y, z0), Point3(x1, y, z0)}; };
        auto loop = [z0](int64_t x, int64_t y, int64_t s) { return Points3{Point3(x, y, z0), Point3(x + s, y, z0), Point3(x + s, y + s, z0), Point3(x, y + s, z0), Point3(x, y, z0)}; };
        {
            // Every type once, then none reversible in a row that wants reversals, then the empty
            // ones that chain_and_reorder drops.
            std::vector<Ent> all{{0, {open(0, 0, 1000)}},         {1, {open(2000, 0, 1500)}},       {2, {open(3000, 0, 2500)}},
                                 {3, {loop(4000, 0, 500)}},       {4, {Points3{Point3(5000, 0, 0), Point3(5500, 0, 0)}, Points3{Point3(5500, 0, 0), Point3(5000, 0, 0)}}},
                                 {5, {open(6000, 0, 6500), open(6500, 0, 7000)}}, {6, {open(8000, 0, 7500)}},
                                 {7, {open(9000, 0, 9500), open(9500, 0, 10000)}}, {8, {open(11000, 0, 10500)}}, {9, {open(12000, 0, 11500)}}};
            e.hand([&] { Payload p; psxe(p, all, false, Point(0, 0)); return p; }());
            e.hand([&] { Payload p; psxe(p, all, true, Point(12000, 0)); return p; }());
            std::vector<Ent> stuck;
            for (int k = 0; k < 5; ++k)
                stuck.push_back(Ent{1, {k % 2 ? open(k * 1000 + 900, 0, k * 1000) : open(k * 1000, 0, k * 1000 + 900)}});
            e.hand([&] { Payload p; psxe(p, stuck, false, Point(0, 0)); return p; }());
            e.hand([&] { Payload p; psxe(p, stuck, true, Point(5000, 0)); return p; }());
            std::vector<Ent> empties{{0, {open(0, 0, 10)}}, {10, {}}, {11, {}}, {12, {open(20, 0, 30)}}, {13, {}}, {14, {open(40, 0, 50)}}, {3, {loop(60, 0, 5)}}};
            e.hand([&] { Payload p; psxe(p, empties, true, Point(0, 0)); return p; }());
            e.hand([&] { Payload p; psxe(p, std::vector<Ent>{}, false, Point(0, 0)); return p; }());
            e.hand([&] { Payload p; psxe(p, std::vector<Ent>{Ent{1, {open(0, 0, 10)}}}, true, Point(10, 0)); return p; }());
        }
        for (int i = 0; i < 900; ++i) {
            const size_t     n     = size_t(rnd(g, 1, 24));
            const Polylines  pls   = segments(g, n);
            const bool       empty = one_in(g, 5);
            const bool       stiff = one_in(g, 3); // mostly non-reversible
            std::vector<Ent> ents;
            for (const Polyline &pl : pls) {
                Ent     x;
                Points3 pts = flat3(pl.points, rnd(g, 0, 3) * 200000);
                const bool closed = pl.points.size() > 2 && pl.points.front() == pl.points.back();
                if (closed)
                    x.type = one_in(g, 2) ? 3 : 7;
                else if (stiff)
                    x.type = std::vector<uint8_t>{1, 2, 6, 8, 9}[g.below(5)];
                else
                    x.type = std::vector<uint8_t>{0, 0, 0, 1, 5, 7}[g.below(6)];
                if (x.type == 5 || x.type == 7) {
                    // Two pieces end to end.
                    const size_t mid = pts.size() / 2;
                    if (pts.size() >= 3) {
                        x.paths.push_back(Points3(pts.begin(), pts.begin() + mid + 1));
                        x.paths.push_back(Points3(pts.begin() + mid, pts.end()));
                    } else
                        x.paths.push_back(pts);
                } else
                    x.paths.push_back(pts);
                if (empty && one_in(g, 4))
                    x.type = std::vector<uint8_t>{10, 11, 12, 13, 14}[g.below(5)];
                if (x.type == 10 || x.type == 11 || x.type == 13)
                    x.paths.clear();
                ents.push_back(std::move(x));
            }
            Payload p;
            psxe(p, ents, one_in(g, 2), rpoint(g, 300000000));
            e.random(p);
        }
    }

    // PSPL
    {
        SplitMix g(0x9C770005u);
        Emit     e(sets, "PSPL", 14, 2048);
        e.hand([] { Payload p; pspl(p, Polylines{}, false, Point(0, 0)); return p; }());
        e.hand([] { Payload p; pspl(p, Polylines{Polyline(Point(0, 0), Point(100, 0))}, true, Point(100, 0)); return p; }());
        {
            Polylines lines;
            for (int k = 0; k < 8; ++k)
                lines.emplace_back(Point(0, k * 100), Point(1000, k * 100));
            e.hand([&] { Payload p; pspl(p, lines, false, Point(0, 0)); return p; }());
            e.hand([&] { Payload p; pspl(p, lines, true, Point(1000, 350)); return p; }());
            Polylines loops{Polyline(rect(0, 0, 10, 10)), Polyline(rect(20, 0, 30, 10))};
            for (Polyline &pl : loops)
                pl.points.push_back(pl.points.front());
            loops.emplace_back(Points{Point(40, 0)});
            e.hand([&] { Payload p; pspl(p, loops, false, Point(0, 0)); return p; }());
        }
        for (int i = 0; i < 900; ++i) {
            Polylines pls = segments(g, size_t(one_in(g, 10) ? rnd(g, 40, 150) : rnd(g, 2, 40)));
            Payload   p;
            pspl(p, pls, one_in(g, 3), rpoint(g, 300000000));
            e.random(p);
        }
    }

    // PSUO
    {
        SplitMix g(0x9C770006u);
        Emit     e(sets, "PSUO", 10, 3072);
        e.hand([] { Payload p; psuo(p, Polygons{}); return p; }());
        {
            // Concentric fill's loops: nested squares, alternately oriented, and two islands.
            Polygons nested;
            for (int k = 0; k < 5; ++k) {
                Polygon q(rect(k * 1000, k * 1000, 20000 - k * 1000, 20000 - k * 1000));
                if (k % 2)
                    q.reverse();
                nested.push_back(q);
            }
            e.hand([&] { Payload p; psuo(p, nested); return p; }());
            Polygons two = nested;
            for (int k = 0; k < 3; ++k)
                two.emplace_back(rect(30000 + k * 1000, k * 1000, 40000 - k * 1000, 10000 - k * 1000));
            e.hand([&] { Payload p; psuo(p, two); return p; }());
        }
        for (int i = 0; i < 500; ++i) {
            Polygons      polys;
            const int64_t r = std::max<int64_t>(1000, std::min(range(g), int64_t(100000000)));
            for (int isl = 0, m = int(rnd(g, 1, 4)); isl < m; ++isl) {
                const Point c = rpoint(g, r * 3);
                const int   depth = int(rnd(g, 1, 6));
                const bool  round = one_in(g, 2);
                for (int k = 0; k < depth; ++k) {
                    const int64_t rr = r - k * (r / int64_t(depth + 1));
                    Points        pts = round ? arc_points(g, c, double(rr), 0., 2. * PI * (1. - 1. / 24.), 24, 0.) : rect(c.x() - rr, c.y() - rr, c.x() + rr, c.y() + rr);
                    Polygon       q(pts);
                    if (k % 2 && one_in(g, 2))
                        q.reverse();
                    polys.push_back(q);
                }
            }
            if (one_in(g, 3))
                polys.emplace_back(star(g, rpoint(g, r), r, size_t(rnd(g, 3, 12)), one_in(g, 2)));
            Payload p;
            psuo(p, polys);
            e.random(p);
        }
    }
}

} // namespace paths
} // namespace probe
