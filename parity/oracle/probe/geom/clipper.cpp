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
// geom.sop1's Clipper records (parity/SOP1.md, "CCLP" to "CINT", added 2026-11-07), which
// orca-geom's tests/probes.rs replays: Clipper 6.4.2 as libslic3r compiles it (Slic3r::ClipperLib,
// with Slic3r::Point as its IntPoint) and Int128's predicates. Each kind starts with hand-made
// edge cases (collinear, duplicate and spike points, self-intersections, holes, touching edges,
// open paths, coordinates at loRange and hiRange and past them, empty input), so that the
// committed set holds them, then cases from a fixed-seed std::mt19937. Every input is in its
// record, so the replay needs no generator.
#include "../probe.hpp"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <functional>
#include <limits>
#include <random>
#include <string>
#include <vector>

#include "libslic3r/clipper.hpp"
#include "libslic3r/Int128.hpp"

namespace probe {

namespace {

namespace CL = Slic3r::ClipperLib;
using Path  = CL::Path;
using Paths = CL::Paths;
using Rng   = std::mt19937;

constexpr int64_t LO = 0x3FFFFFFF;
constexpr int64_t HI = 0x3FFFFFFFFFFFFFFFLL;

CL::IntPoint pt(int64_t x, int64_t y) { return CL::IntPoint(x, y); }

int64_t rnd(Rng &g, int64_t lo, int64_t hi) { return std::uniform_int_distribution<int64_t>(lo, hi)(g); }
double  unit(Rng &g) { return std::uniform_real_distribution<double>(0.0, 1.0)(g); }

void put(Payload &p, const CL::IntPoint &q) { p.i64(q.x()).i64(q.y()); }
void put(Payload &p, const Path &path)
{
    p.u32(uint32_t(path.size()));
    for (const CL::IntPoint &q : path)
        put(p, q);
}
void put(Payload &p, const Paths &paths)
{
    p.u32(uint32_t(paths.size()));
    for (const Path &path : paths)
        put(p, path);
}
void put_children(Payload &p, const CL::PolyNode &n)
{
    p.u32(uint32_t(n.ChildCount()));
    for (int i = 0; i < n.ChildCount(); ++i) {
        const CL::PolyNode &c = *n.Childs[i];
        p.boolean(c.IsOpen()).boolean(c.IsHole());
        put(p, c.Contour);
        put_children(p, c);
    }
}
// A tree: the root's contour, Total(), then the children depth first, each with IsOpen(),
// IsHole(), its contour and its own children.
void put(Payload &p, const CL::PolyTree &t)
{
    put(p, t.Contour);
    p.i32(t.Total());
    put_children(p, t);
}

// A star-shaped polygon of n vertices around c, radii between r / 4 and r.
Path star(Rng &g, int n, CL::IntPoint c, int64_t r)
{
    std::vector<double> a(size_t(n), 0.);
    for (double &t : a)
        t = unit(g) * 6.283185307179586;
    std::sort(a.begin(), a.end());
    Path p;
    for (double t : a) {
        const double rr = double(rnd(g, r / 4, r));
        p.push_back(pt(c.x() + std::llround(rr * std::cos(t)), c.y() + std::llround(rr * std::sin(t))));
    }
    if (rnd(g, 0, 1))
        std::reverse(p.begin(), p.end());
    return p;
}

// n points anywhere in the square of half-side r around c: self-intersecting paths.
Path scatter(Rng &g, int n, CL::IntPoint c, int64_t r)
{
    Path p;
    for (int i = 0; i < n; ++i)
        p.push_back(pt(c.x() + rnd(g, -r, r), c.y() + rnd(g, -r, r)));
    return p;
}

// n points on a grid of `cells` steps each way: collinear runs, horizontals, duplicates, spikes.
Path grid(Rng &g, int n, int64_t step, int64_t cells)
{
    Path p;
    for (int i = 0; i < n; ++i)
        p.push_back(pt(step * rnd(g, -cells, cells), step * rnd(g, -cells, cells)));
    return p;
}

Path rect(int64_t x0, int64_t y0, int64_t x1, int64_t y1) { return Path{pt(x0, y0), pt(x1, y0), pt(x1, y1), pt(x0, y1)}; }

Path reversed(Path p)
{
    std::reverse(p.begin(), p.end());
    return p;
}

struct Group
{
    CL::PolyType type;
    bool         closed;
    Paths        paths;
};

struct ClipCase
{
    int                opts   = 0;
    CL::ClipType       ct     = CL::ctUnion;
    CL::PolyFillType   sf     = CL::pftNonZero;
    CL::PolyFillType   cf     = CL::pftNonZero;
    bool               tree   = false;
    bool               each   = false; // AddPath per path, else AddPaths per group
    bool               twice  = false; // Execute a second time on the same Clipper
    bool               convert = false; // with a tree: PolyTreeToPaths and its kin
    std::vector<Group> groups;
};

// CCLP: one Clipper run. The case, then each Add call's result (0, 1, or 2 and what() when it
// threw, which ends the record), GetBounds() when no group is open, then each Execute's result.
void cclp(Payload &p, const ClipCase &k)
{
    p.u8(uint8_t(k.opts)).u8(uint8_t(k.ct)).u8(uint8_t(k.sf)).u8(uint8_t(k.cf));
    p.boolean(k.tree).boolean(k.each).boolean(k.twice).boolean(k.convert);
    p.u32(uint32_t(k.groups.size()));
    bool any_open = false;
    for (const Group &gr : k.groups) {
        p.u8(uint8_t(gr.type)).boolean(gr.closed);
        put(p, gr.paths);
        any_open |= ! gr.closed;
    }
    CL::Clipper c(k.opts);
    try {
        for (const Group &gr : k.groups) {
            if (k.each)
                for (const Path &path : gr.paths)
                    p.u8(c.AddPath(path, gr.type, gr.closed) ? 1 : 0);
            else
                p.u8(c.AddPaths(gr.paths, gr.type, gr.closed) ? 1 : 0);
        }
    } catch (const CL::clipperException &e) {
        p.u8(2).str(e.what());
        return;
    }
    if (! any_open) {
        const CL::IntRect r = c.GetBounds();
        p.i64(r.left).i64(r.top).i64(r.right).i64(r.bottom);
    }
    for (int run = 0; run < (k.twice ? 2 : 1); ++run) {
        if (k.tree) {
            CL::PolyTree t;
            p.boolean(c.Execute(k.ct, t, k.sf, k.cf));
            put(p, t);
            if (k.convert) {
                Paths a, b, o;
                CL::PolyTreeToPaths(t, a);
                CL::ClosedPathsFromPolyTree(t, b);
                CL::OpenPathsFromPolyTree(t, o);
                put(p, a);
                put(p, b);
                put(p, o);
            }
        } else {
            Paths out;
            try {
                p.boolean(c.Execute(k.ct, out, k.sf, k.cf));
            } catch (const CL::clipperException &e) {
                p.u8(2).str(e.what());
                return;
            }
            put(p, out);
        }
    }
}

ClipCase closed_case(CL::ClipType ct, CL::PolyFillType f, Paths subj, Paths clip = {}, bool tree = false, int opts = 0)
{
    ClipCase k;
    k.opts = opts;
    k.ct   = ct;
    k.sf = k.cf = f;
    k.tree      = tree;
    k.groups.push_back({CL::ptSubject, true, std::move(subj)});
    if (! clip.empty())
        k.groups.push_back({CL::ptClip, true, std::move(clip)});
    return k;
}

std::vector<ClipCase> cclp_edges()
{
    std::vector<ClipCase> v;
    const Path tri{pt(0, 0), pt(100, 0), pt(50, 80)};
    const Path sq = rect(0, 0, 100, 100);
    const Path sq_mid{pt(0, 0), pt(50, 0), pt(100, 0), pt(100, 50), pt(100, 100), pt(50, 100), pt(0, 100), pt(0, 50)};
    const Path spike{pt(0, 0), pt(100, 0), pt(100, 100), pt(100, 150), pt(100, 100), pt(0, 100)};
    const Path dups{pt(0, 0), pt(0, 0), pt(100, 0), pt(100, 0), pt(100, 100), pt(0, 100), pt(0, 0)};
    const Path bowtie{pt(0, 0), pt(100, 100), pt(100, 0), pt(0, 100)};
    const Path figure8{pt(0, 0), pt(100, 0), pt(50, 50), pt(100, 100), pt(0, 100), pt(50, 50)};
    const Path comb{pt(0, 0), pt(70, 0), pt(70, 30), pt(60, 30), pt(60, 10), pt(50, 10), pt(50, 30), pt(40, 30),
                    pt(40, 10), pt(30, 10), pt(30, 30), pt(20, 30), pt(20, 10), pt(10, 10), pt(10, 30), pt(0, 30)};
    v.push_back(closed_case(CL::ctUnion, CL::pftNonZero, {}));
    v.push_back(closed_case(CL::ctUnion, CL::pftNonZero, {tri}));
    v.push_back(closed_case(CL::ctUnion, CL::pftNonZero, {sq_mid}));
    v.push_back(closed_case(CL::ctUnion, CL::pftNonZero, {sq_mid}, {}, false, CL::ioPreserveCollinear));
    v.push_back(closed_case(CL::ctUnion, CL::pftNonZero, {spike}));
    v.push_back(closed_case(CL::ctUnion, CL::pftNonZero, {spike}, {}, false, CL::ioPreserveCollinear));
    v.push_back(closed_case(CL::ctUnion, CL::pftNonZero, {dups}));
    for (CL::PolyFillType f : {CL::pftEvenOdd, CL::pftNonZero, CL::pftPositive, CL::pftNegative})
        v.push_back(closed_case(CL::ctUnion, f, {bowtie}, {}, true));
    v.push_back(closed_case(CL::ctUnion, CL::pftNonZero, {rect(0, 0, 100, 100), rect(100, 0, 200, 100)}));
    v.push_back(closed_case(CL::ctUnion, CL::pftNonZero, {rect(0, 0, 100, 100), rect(100, 50, 200, 150), rect(0, 100, 100, 200)}, {}, true));
    v.push_back(closed_case(CL::ctUnion, CL::pftNonZero, {rect(0, 0, 300, 300), reversed(rect(100, 100, 200, 200))}, {}, true));
    v.push_back(closed_case(CL::ctUnion, CL::pftEvenOdd, {rect(0, 0, 500, 500), rect(100, 100, 400, 400), rect(200, 200, 300, 300)}, {}, true));
    for (CL::ClipType ct : {CL::ctIntersection, CL::ctUnion, CL::ctDifference, CL::ctXor})
        v.push_back(closed_case(ct, CL::pftNonZero, {sq}, {sq}));
    v.push_back(closed_case(CL::ctUnion, CL::pftNonZero, {rect(0, 0, 100, 100), rect(100, 100, 200, 200)}, {}, true, CL::ioStrictlySimple));
    v.push_back(closed_case(CL::ctUnion, CL::pftNonZero, {figure8}, {}, true, CL::ioStrictlySimple));
    v.push_back(closed_case(CL::ctUnion, CL::pftNonZero, {figure8}, {}, true));
    v.push_back(closed_case(CL::ctDifference, CL::pftNonZero, {comb}, {rect(5, 5, 65, 20)}, true));
    v.push_back(closed_case(CL::ctUnion, CL::pftNonZero, {comb, rect(0, 30, 70, 40)}, {}, false, CL::ioStrictlySimple));
    // Open paths: a polyline across a square, each clip type.
    const Path line{pt(-50, 50), pt(150, 50), pt(150, 70), pt(-20, 90)};
    for (CL::ClipType ct : {CL::ctIntersection, CL::ctUnion, CL::ctDifference, CL::ctXor}) {
        ClipCase k;
        k.ct = ct;
        k.tree = k.convert = true;
        k.groups.push_back({CL::ptSubject, false, {line}});
        k.groups.push_back({CL::ptClip, true, {sq}});
        v.push_back(k);
    }
    {
        // An open path whose ends meet, a flat open path, a two-point one and a one-point one.
        ClipCase k;
        k.ct = CL::ctIntersection;
        k.tree = k.convert = k.each = true;
        k.groups.push_back({CL::ptSubject, false, {Path{pt(10, 10), pt(90, 10), pt(50, 90), pt(10, 10)}, Path{pt(-10, 40), pt(40, 40), pt(120, 40)}, Path{pt(5, 5), pt(95, 95)}, Path{pt(1, 1)}}});
        k.groups.push_back({CL::ptClip, true, {sq}});
        v.push_back(k);
    }
    {
        // Subject open paths meeting a subject polygon in a union.
        ClipCase k;
        k.ct = CL::ctUnion;
        k.tree = k.convert = true;
        k.groups.push_back({CL::ptSubject, false, {line, Path{pt(50, -20), pt(50, 120)}}});
        k.groups.push_back({CL::ptSubject, true, {sq}});
        v.push_back(k);
    }
    // Flat, short and degenerate closed paths: nothing to add.
    v.push_back(closed_case(CL::ctUnion, CL::pftNonZero, {Path{pt(0, 0), pt(100, 0), pt(50, 0)}, Path{pt(0, 0), pt(10, 10)}, Path{pt(3, 3), pt(3, 3), pt(3, 3)}, Path{}}));
    {
        ClipCase k = closed_case(CL::ctUnion, CL::pftNonZero, {Path{pt(0, 0), pt(100, 0), pt(50, 0)}, Path{pt(0, 0), pt(10, 10)}, Path{pt(3, 3), pt(3, 3), pt(3, 3)}, Path{}});
        k.each = true;
        v.push_back(k);
    }
    // loRange and past it (the full range and Int128's filtered slopes), hiRange, and past it.
    v.push_back(closed_case(CL::ctUnion, CL::pftNonZero, {Path{pt(-LO, -LO), pt(LO, -LO), pt(LO, LO), pt(0, LO / 2), pt(-LO, LO)}}, {rect(-LO / 2, -LO / 2, LO / 2, LO / 2)}));
    v.push_back(closed_case(CL::ctXor, CL::pftNonZero, {Path{pt(-LO - 1, 0), pt(LO + 1, 3), pt(LO + 1, LO + 1), pt(7, LO + 1)}}, {rect(0, 0, LO + 2, LO + 2)}, true));
    v.push_back(closed_case(CL::ctIntersection, CL::pftNonZero, {Path{pt(-HI, -HI), pt(HI, -HI), pt(HI, HI), pt(-HI, HI)}}, {Path{pt(-HI / 2, -HI / 3), pt(HI / 2, -HI / 5), pt(HI / 7, HI / 2)}}));
    v.push_back(closed_case(CL::ctUnion, CL::pftNonZero, {Path{pt(0, 0), pt(HI + 1, 0), pt(0, 10)}}));
    v.push_back(closed_case(CL::ctUnion, CL::pftNonZero, {Path{pt(0, 0), pt(10, 0), pt(0, -HI - 1)}}));
    v.push_back(closed_case(CL::ctUnion, CL::pftNonZero, {Path{pt(0, 0), pt(10, 0), pt(std::numeric_limits<int64_t>::min(), 10)}}));
    {
        // An open clip path throws; so does an open path with a Paths result.
        ClipCase k;
        k.groups.push_back({CL::ptSubject, true, {sq}});
        k.groups.push_back({CL::ptClip, false, {line}});
        v.push_back(k);
        ClipCase k2;
        k2.groups.push_back({CL::ptSubject, false, {line}});
        k2.groups.push_back({CL::ptClip, true, {sq}});
        v.push_back(k2);
    }
    {
        // A second Execute on the same Clipper, and ReverseSolution.
        ClipCase k = closed_case(CL::ctUnion, CL::pftNonZero, {sq, tri});
        k.twice = true;
        v.push_back(k);
        v.push_back(closed_case(CL::ctUnion, CL::pftNonZero, {rect(0, 0, 300, 300), reversed(rect(100, 100, 200, 200))}, {}, false, CL::ioReverseSolution));
    }
    // A closed path of collinear points, and of one point repeated.
    v.push_back(closed_case(CL::ctUnion, CL::pftNonZero, {Path{pt(0, 0), pt(10, 10), pt(20, 20), pt(30, 30), pt(5, 5)}}, {}, false, CL::ioPreserveCollinear));
    return v;
}

const CL::ClipType     CLIP_TYPES[] = {CL::ctIntersection, CL::ctUnion, CL::ctDifference, CL::ctXor};
const CL::PolyFillType FILL_TYPES[] = {CL::pftEvenOdd, CL::pftNonZero, CL::pftPositive, CL::pftNegative};

// A random case: the style decides the shapes; the options, types and result form are random.
ClipCase cclp_random(Rng &g)
{
    ClipCase k;
    k.opts  = int(rnd(g, 0, 7));
    k.ct    = CLIP_TYPES[rnd(g, 0, 3)];
    k.sf    = FILL_TYPES[rnd(g, 0, 3)];
    k.cf    = rnd(g, 0, 2) ? k.sf : FILL_TYPES[rnd(g, 0, 3)];
    k.tree  = rnd(g, 0, 1) != 0;
    k.each  = rnd(g, 0, 3) == 0;
    k.twice = rnd(g, 0, 15) == 0;
    const int style = int(rnd(g, 0, 7));
    auto shapes = [&](int count) {
        Paths ps;
        for (int i = 0; i < count; ++i) {
            switch (style) {
            case 0: ps.push_back(star(g, int(rnd(g, 3, 24)), pt(rnd(g, -2000000, 2000000), rnd(g, -2000000, 2000000)), rnd(g, 1000, 5000000))); break;
            case 1: ps.push_back(scatter(g, int(rnd(g, 3, 10)), pt(0, 0), rnd(g, 0, 1) ? 100 : 100000000)); break;
            case 2: ps.push_back(grid(g, int(rnd(g, 3, 14)), rnd(g, 1, 1000), rnd(g, 2, 6))); break;
            case 3: {
                const int64_t s = rnd(g, 1, 100);
                const int64_t x = s * rnd(g, -10, 10), y = s * rnd(g, -10, 10);
                Path r = rect(x, y, x + s * rnd(g, 1, 8), y + s * rnd(g, 1, 8));
                ps.push_back(rnd(g, 0, 3) ? r : reversed(r));
                break;
            }
            case 4: ps.push_back(star(g, int(rnd(g, 3, 16)), pt(rnd(g, -HI / 4, HI / 4), rnd(g, -HI / 4, HI / 4)), int64_t(1) << rnd(g, 32, 60))); break;
            case 5: ps.push_back(star(g, int(rnd(g, 3, 16)), pt(LO - rnd(g, 0, 1000), LO - rnd(g, 0, 1000)), rnd(g, 10, 4000))); break;
            default: ps.push_back(grid(g, int(rnd(g, 4, 30)), 1, rnd(g, 2, 4))); break;
            }
        }
        return ps;
    };
    if (rnd(g, 0, 4) == 0) {
        // Open subject paths (a tree result), with closed subjects or clips.
        k.tree = true;
        k.convert = rnd(g, 0, 3) == 0;
        Paths lines;
        for (int i = int(rnd(g, 1, 3)); i > 0; --i)
            lines.push_back(style == 2 || style == 6 ? grid(g, int(rnd(g, 2, 8)), rnd(g, 1, 100), 4) : scatter(g, int(rnd(g, 2, 8)), pt(0, 0), 3000000));
        k.groups.push_back({CL::ptSubject, false, lines});
        k.groups.push_back({rnd(g, 0, 1) ? CL::ptClip : CL::ptSubject, true, shapes(int(rnd(g, 1, 3)))});
        return k;
    }
    k.groups.push_back({CL::ptSubject, true, shapes(int(rnd(g, 1, 4)))});
    if (rnd(g, 0, 5) != 0)
        k.groups.push_back({CL::ptClip, true, shapes(int(rnd(g, 1, 3)))});
    return k;
}

// COFF: one ClipperOffset run. MiterLimit, ArcTolerance, ShortestEdgeLength, the paths with their
// join and end types, then each Execute's delta and result form, then each result (1 and the
// result, or 2 and what() when it threw, which ends the record).
struct OffsetCase
{
    double                                                miter = 2., arc = 0.25, shortest = 0.;
    std::vector<std::pair<std::pair<int, int>, Path>>     paths; // (join, end), path
    std::vector<std::pair<double, bool>>                  runs;  // delta, tree
};

void coff(Payload &p, const OffsetCase &k)
{
    p.f64(k.miter).f64(k.arc).f64(k.shortest);
    p.u32(uint32_t(k.paths.size()));
    for (const auto &e : k.paths) {
        p.u8(uint8_t(e.first.first)).u8(uint8_t(e.first.second));
        put(p, e.second);
    }
    p.u32(uint32_t(k.runs.size()));
    for (const auto &r : k.runs)
        p.f64(r.first).boolean(r.second);
    CL::ClipperOffset co(k.miter, k.arc, k.shortest);
    for (const auto &e : k.paths)
        co.AddPath(e.second, CL::JoinType(e.first.first), CL::EndType(e.first.second));
    for (const auto &r : k.runs) {
        try {
            if (r.second) {
                CL::PolyTree t;
                co.Execute(t, r.first);
                p.u8(1);
                put(p, t);
            } else {
                Paths out;
                co.Execute(out, r.first);
                p.u8(1);
                put(p, out);
            }
        } catch (const CL::clipperException &e) {
            p.u8(2).str(e.what());
            return;
        }
    }
}

OffsetCase offset_case(int join, int end, std::vector<Path> paths, double delta, bool tree, double miter = 2., double arc = 0.25, double shortest = 0.)
{
    OffsetCase k;
    k.miter = miter;
    k.arc = arc;
    k.shortest = shortest;
    for (Path &path : paths)
        k.paths.push_back({{join, end}, std::move(path)});
    k.runs.push_back({delta, tree});
    return k;
}

std::vector<OffsetCase> coff_edges()
{
    std::vector<OffsetCase> v;
    const Path sq = rect(0, 0, 1000, 1000);
    const Path spike{pt(0, 0), pt(1000, 0), pt(1000, 1000), pt(500, 100), pt(0, 1000)};
    const Path line{pt(0, 0), pt(1000, 0), pt(1000, 1000), pt(2000, 300)};
    for (int join = 0; join < 3; ++join) {
        v.push_back(offset_case(join, CL::etClosedPolygon, {Path{pt(5, 5)}}, 100., false));
        v.push_back(offset_case(join, CL::etOpenRound, {Path{pt(5, 5)}}, 100., false));
        for (int end = 0; end < 5; ++end)
            v.push_back(offset_case(join, end, {line}, 120., end == 1));
        v.push_back(offset_case(join, CL::etClosedPolygon, {spike}, 150., false, 10.));
        v.push_back(offset_case(join, CL::etClosedPolygon, {spike}, -150., true, 3.));
        v.push_back(offset_case(join, CL::etClosedPolygon, {Path{pt(0, 0), pt(1000, 0)}}, 50., false));
    }
    v.push_back(offset_case(CL::jtMiter, CL::etClosedPolygon, {sq, reversed(rect(200, 200, 800, 800))}, -50., true, 3.));
    v.push_back(offset_case(CL::jtMiter, CL::etClosedPolygon, {sq, reversed(rect(200, 200, 800, 800))}, 50., true, 3.));
    v.push_back(offset_case(CL::jtMiter, CL::etClosedPolygon, {sq, reversed(rect(200, 200, 800, 800))}, -400., true, 3.));
    v.push_back(offset_case(CL::jtSquare, CL::etClosedPolygon, {sq}, 0., false));
    v.push_back(offset_case(CL::jtSquare, CL::etClosedPolygon, {sq}, 1e-21, true));
    v.push_back(offset_case(CL::jtRound, CL::etClosedPolygon, {sq}, -2000., true));
    v.push_back(offset_case(CL::jtRound, CL::etClosedPolygon, {reversed(sq)}, 25., false, 2., 0.));
    // Below an eighth with ArcTolerance 0: acos of less than -1, so a NaN step count.
    v.push_back(offset_case(CL::jtRound, CL::etClosedPolygon, {sq}, 0.1, false, 2., 0.));
    v.push_back(offset_case(CL::jtRound, CL::etOpenRound, {line, Path{pt(7, 7)}}, -0.1, false, 2., 0.));
    v.push_back(offset_case(CL::jtRound, CL::etClosedPolygon, {sq}, 25., false, 2., 12500.));
    v.push_back(offset_case(CL::jtRound, CL::etClosedPolygon, {sq}, 250000., false, 2., 12500.));
    v.push_back(offset_case(CL::jtMiter, CL::etClosedPolygon, {Path{pt(0, 0), pt(3, 0), pt(1000, 2), pt(1001, 500), pt(1000, 1000), pt(4, 997), pt(0, 1000)}}, 10., false, 3., 0.25, 5.));
    v.push_back(offset_case(CL::jtMiter, CL::etOpenButt, {Path{pt(0, 0), pt(1, 0), pt(2, 1), pt(500, 1), pt(500, 3), pt(1000, 3)}}, 10., false, 3., 0.25, 0.05));
    v.push_back(offset_case(CL::jtMiter, CL::etClosedPolygon, {Path{pt(-LO, -LO), pt(LO, -LO), pt(LO, LO), pt(-LO, LO)}}, 10., false, 3.));
    v.push_back(offset_case(CL::jtSquare, CL::etClosedPolygon, {Path{pt(HI - 10, 0), pt(HI - 5, 0), pt(HI - 5, 10)}}, 100., false));
    {
        // Two Executes with different deltas, one tree and one not; and paths added twice.
        OffsetCase k = offset_case(CL::jtMiter, CL::etClosedPolygon, {reversed(sq), rect(2000, 0, 2600, 500)}, 30., false, 3.);
        k.runs.push_back({-60., true});
        k.paths.push_back({{CL::jtRound, CL::etClosedLine}, line});
        v.push_back(k);
    }
    return v;
}

OffsetCase coff_random(Rng &g)
{
    OffsetCase k;
    const double miters[] = {0., 1.2, 2., 3., 5., 10.};
    const double arcs[]   = {0., 0.25, 3., 5000., 10000., 12500., 100000.};
    k.miter = miters[rnd(g, 0, 5)];
    k.arc   = arcs[rnd(g, 0, 6)];
    const bool tiny = rnd(g, 0, 7) == 0;
    const double delta = (rnd(g, 0, 2) ? 1. : -1.) * (tiny ? double(rnd(g, 1, 20)) * 0.25 : std::pow(10., 1. + 5. * unit(g)));
    k.shortest = rnd(g, 0, 1) ? std::abs(delta * 0.005) : 0.;
    const int n = int(rnd(g, 1, 3));
    const int style = int(rnd(g, 0, 3));
    for (int i = 0; i < n; ++i) {
        Path path;
        switch (style) {
        case 0: path = star(g, int(rnd(g, 3, 20)), pt(rnd(g, -3000000, 3000000), rnd(g, -3000000, 3000000)), rnd(g, 100, 3000000)); break;
        case 1: path = grid(g, int(rnd(g, 2, 10)), rnd(g, 10, 100000), 5); break;
        case 2: path = scatter(g, int(rnd(g, 2, 9)), pt(0, 0), rnd(g, 10, 5000000)); break;
        default: path = rect(0, 0, rnd(g, 1, 1000000), rnd(g, 1, 1000000)); break;
        }
        const int end = rnd(g, 0, 2) ? CL::etClosedPolygon : int(rnd(g, 0, 4));
        k.paths.push_back({{int(rnd(g, 0, 2)), end}, path});
    }
    k.runs.push_back({delta, rnd(g, 0, 2) == 0});
    if (rnd(g, 0, 9) == 0)
        k.runs.push_back({-delta * 0.5, rnd(g, 0, 1) != 0});
    return k;
}

// CARE: Area, Orientation and PointInPolygon: the path, Area's bits, Orientation, then the points
// with PointInPolygon's answer for each.
void care(Payload &p, const Path &path, const Path &pts)
{
    put(p, path);
    p.f64(CL::Area(path)).boolean(CL::Orientation(path));
    p.u32(uint32_t(pts.size()));
    for (const CL::IntPoint &q : pts) {
        put(p, q);
        p.i32(CL::PointInPolygon(q, path));
    }
}

// Points to test against a path: its vertices, edge midpoints, and points near and far.
Path probe_points(Rng &g, const Path &path)
{
    Path pts;
    for (size_t i = 0; i < path.size() && i < 6; ++i) {
        pts.push_back(path[i]);
        const CL::IntPoint &b = path[(i + 1) % path.size()];
        pts.push_back(pt(path[i].x() + (b.x() - path[i].x()) / 2, path[i].y() + (b.y() - path[i].y()) / 2));
    }
    int64_t span = 1;
    for (const CL::IntPoint &q : path)
        span = std::max(span, std::max(std::abs(q.x()), std::abs(q.y())));
    for (int i = 0; i < 6; ++i)
        pts.push_back(pt(rnd(g, -span, span), rnd(g, -span, span)));
    return pts;
}

// CCLN: CleanPolygon: the path, the distance, the result.
void ccln(Payload &p, const Path &path, double distance)
{
    put(p, path);
    p.f64(distance);
    Path out;
    CL::CleanPolygon(path, out, distance);
    put(p, out);
}

// Points on a grid with a jitter below the cleaning distance, so that many are close.
Path jittered(Rng &g, int n, int64_t step, int64_t jitter)
{
    Path p = star(g, n, pt(0, 0), step * 20);
    for (CL::IntPoint &q : p)
        q = pt(q.x() / step * step + rnd(g, -jitter, jitter), q.y() / step * step + rnd(g, -jitter, jitter));
    return p;
}

// CSIM: SimplifyPolygon (one path) or SimplifyPolygons: u8 0 or 1, the paths, the fill type,
// StrictlySimple, the result.
void csim(Payload &p, const Paths &paths, bool many, CL::PolyFillType f, bool strictly)
{
    p.boolean(many);
    put(p, paths);
    p.u8(uint8_t(f)).boolean(strictly);
    put(p, many ? CL::SimplifyPolygons(paths, f, strictly) : CL::SimplifyPolygon(paths.front(), f, strictly));
}

// CMNK: u8 0 (MinkowskiSum of a path), 1 (MinkowskiSum of paths) or 2 (MinkowskiDiff), the
// closed flag, the pattern, the paths, the result.
void cmnk(Payload &p, int kind, bool closed, const Path &pattern, const Paths &paths)
{
    p.u8(uint8_t(kind)).boolean(closed);
    put(p, pattern);
    put(p, paths);
    Paths out;
    if (kind == 0)
        CL::MinkowskiSum(pattern, paths.front(), out, closed);
    else if (kind == 1)
        CL::MinkowskiSum(pattern, paths, out, closed);
    else
        CL::MinkowskiDiff(pattern, paths.front(), out);
    put(p, out);
}

// CINT: Int128 on four int64_t: sign_determinant_2x2_filtered(a, b, c, d) and
// compare_rationals_filtered(a, b, c, d). The exact predicates they fall back on are private in
// the MSVC build (no public: before them), so the filtered ones are what callers reach.
void cint(Payload &p, int64_t a, int64_t b, int64_t c, int64_t d)
{
    p.i64(a).i64(b).i64(c).i64(d);
    p.i32(::Int128::sign_determinant_2x2_filtered(a, b, c, d));
    p.i32(::Int128::compare_rationals_filtered(a, b, c, d));
}

int64_t wide(Rng &g)
{
    const int64_t m = int64_t(1) << rnd(g, 0, 62);
    return rnd(g, -m, m);
}

// Every edge case goes to both sets; of the random cases, the first `committed_random` that take at
// most `committed_bytes` go to the committed set too.
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

} // namespace

void record_clipper(Sets &sets)
{
    Rng g(0xC1199E2u);
    using Fn = std::function<void(Payload &)>;

    std::vector<Fn> e;
    for (const ClipCase &k : cclp_edges())
        e.push_back([k](Payload &p) { cclp(p, k); });
    record_cases(sets, "CCLP", e, 3000, 30, 4096, [&](Payload &p) { cclp(p, cclp_random(g)); });

    e.clear();
    for (const OffsetCase &k : coff_edges())
        e.push_back([k](Payload &p) { coff(p, k); });
    record_cases(sets, "COFF", e, 1500, 12, 4096, [&](Payload &p) { coff(p, coff_random(g)); });

    e.clear();
    for (const Path &path : {Path{}, Path{pt(1, 1)}, Path{pt(0, 0), pt(5, 5)}, Path{pt(0, 0), pt(10, 0), pt(20, 0)}, rect(0, 0, 10, 10), reversed(rect(0, 0, 10, 10)),
                             Path{pt(0, 0), pt(10, 10), pt(10, 0), pt(0, 10)}, Path{pt(-HI, -HI), pt(HI, -HI), pt(HI, HI)}})
        e.push_back([path](Payload &p) {
            Path pts = path;
            pts.push_back(pt(5, 5));
            pts.push_back(pt(10, 5));
            pts.push_back(pt(-1, 5));
            pts.push_back(pt(5, 10));
            care(p, path, pts);
        });
    record_cases(sets, "CARE", e, 1000, 16, 2048, [&](Payload &p) {
        const int style = int(rnd(g, 0, 2));
        const Path path = style == 0 ? star(g, int(rnd(g, 3, 30)), pt(0, 0), rnd(g, 10, 100000000)) : style == 1 ? grid(g, int(rnd(g, 3, 12)), rnd(g, 1, 50), 4) : scatter(g, int(rnd(g, 3, 12)), pt(0, 0), 1000);
        care(p, path, probe_points(g, path));
    });

    e.clear();
    for (const Path &path : {Path{}, Path{pt(1, 1)}, Path{pt(0, 0), pt(1, 1)}, Path{pt(0, 0), pt(1, 0), pt(1, 1)}, Path{pt(0, 0), pt(100, 0), pt(100, 1), pt(200, 0), pt(200, 100), pt(0, 100)},
                             Path{pt(0, 0), pt(100, 0), pt(50, 0), pt(100, 100), pt(0, 100)}, Path{pt(0, 0), pt(100, 0), pt(100, 100), pt(1, 1), pt(0, 100)}})
        e.push_back([path](Payload &p) { ccln(p, path, 1.415); });
    record_cases(sets, "CCLN", e, 800, 12, 2048, [&](Payload &p) {
        const double distances[] = {1.415, 0., 5., 100., 10000.};
        const double d = distances[rnd(g, 0, 4)];
        ccln(p, jittered(g, int(rnd(g, 3, 40)), rnd(g, 1, 1000), rnd(g, 0, 8)), d);
    });

    e.clear();
    e.push_back([](Payload &p) { csim(p, {Path{pt(0, 0), pt(100, 100), pt(100, 0), pt(0, 100)}}, false, CL::pftNonZero, true); });
    e.push_back([](Payload &p) { csim(p, {Path{pt(0, 0), pt(100, 0), pt(50, 50), pt(100, 100), pt(0, 100), pt(50, 50)}}, false, CL::pftNonZero, true); });
    e.push_back([](Payload &p) { csim(p, {rect(0, 0, 10, 10), rect(5, 5, 15, 15)}, true, CL::pftEvenOdd, false); });
    record_cases(sets, "CSIM", e, 600, 8, 2048, [&](Payload &p) {
        const bool many = rnd(g, 0, 1) != 0;
        Paths ps;
        for (int i = many ? int(rnd(g, 1, 4)) : 1; i > 0; --i)
            ps.push_back(rnd(g, 0, 1) ? scatter(g, int(rnd(g, 3, 12)), pt(0, 0), 1000000) : grid(g, int(rnd(g, 3, 12)), 10, 3));
        csim(p, ps, many, FILL_TYPES[rnd(g, 0, 3)], rnd(g, 0, 2) != 0);
    });

    e.clear();
    e.push_back([](Payload &p) { cmnk(p, 0, true, rect(-5, -5, 5, 5), {rect(0, 0, 100, 100)}); });
    e.push_back([](Payload &p) { cmnk(p, 0, false, rect(-5, -5, 5, 5), {Path{pt(0, 0), pt(100, 0), pt(100, 100)}}); });
    e.push_back([](Payload &p) { cmnk(p, 1, true, Path{pt(0, 0), pt(10, 0), pt(5, 8)}, {rect(0, 0, 100, 100), rect(200, 0, 300, 50)}); });
    e.push_back([](Payload &p) { cmnk(p, 2, true, rect(-5, -5, 5, 5), {rect(0, 0, 100, 100)}); });
    record_cases(sets, "CMNK", e, 400, 6, 4096, [&](Payload &p) {
        const int kind = int(rnd(g, 0, 2));
        const Path pattern = star(g, int(rnd(g, 3, 8)), pt(rnd(g, -10, 10), rnd(g, -10, 10)), rnd(g, 4, 2000));
        Paths ps;
        for (int i = kind == 1 ? int(rnd(g, 1, 3)) : 1; i > 0; --i)
            ps.push_back(star(g, int(rnd(g, 3, 12)), pt(rnd(g, -100000, 100000), rnd(g, -100000, 100000)), rnd(g, 100, 100000)));
        cmnk(p, kind, kind == 2 || rnd(g, 0, 1) != 0, pattern, ps);
    });

    e.clear();
    const int64_t mn = std::numeric_limits<int64_t>::min(), mx = std::numeric_limits<int64_t>::max();
    const int64_t specials[] = {0, 1, -1, int64_t(1) << 31, -(int64_t(1) << 31), (int64_t(1) << 31) - 1, int64_t(1) << 32, -(int64_t(1) << 32), (int64_t(1) << 32) + (int64_t(1) << 31),
                                LO, LO + 1, HI, -HI, int64_t(1) << 62, mx, mn};
    for (int64_t a : specials)
        for (int64_t b : {int64_t(0), int64_t(1) << 31, HI, mn})
            e.push_back([a, b](Payload &p) { cint(p, a, b, b, a); });
    record_cases(sets, "CINT", e, 4000, 32, 64, [&](Payload &p) {
        if (rnd(g, 0, 3) == 0) {
            // Nearly collinear: c, d a multiple of a, b, nudged.
            const int64_t a = wide(g) >> 2, b = wide(g) >> 2, m = rnd(g, -3, 3);
            cint(p, a, b, a * m + rnd(g, -2, 2), b * m + rnd(g, -2, 2));
        } else
            cint(p, wide(g), wide(g), wide(g), wide(g));
    });
}

} // namespace probe
