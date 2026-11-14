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
// ANSQ, ANSP, ANSL and ANPS (parity/sop1/arachne.md), piece arachne-2: SquareGrid's coordinate
// mapping and cell walks, SparsePointGrid's insertion, iteration order and nearby queries,
// SparseLineGrid as WallToolPaths.cpp's createLocToLineGrid builds it for fixSelfIntersections,
// and PolylineStitcher<VariableWidthLines, ExtrusionLine, ExtrusionJunction>::stitch on fragments
// of closed and open lines. Inputs come from splitmix64 generators (seeds 0xA7A1B000 to
// 0xA7A1B300) and the probe's shapes. Recorded by orca-o3-probe-arachne, 2026-11-13.
#include "common.hpp"

#include <algorithm>
#include <cmath>

#include "libslic3r/Arachne/utils/PolygonsPointIndex.hpp"
#include "libslic3r/Arachne/utils/PolylineStitcher.hpp"
#include "libslic3r/Arachne/utils/SparseLineGrid.hpp"
#include "libslic3r/Arachne/utils/SparsePointGrid.hpp"
#include "libslic3r/Arachne/utils/SquareGrid.hpp"

namespace Slic3r::Arachne {
// WallToolPaths.cpp's grid of polygon segments (not static: the probe calls the engine's own).
typedef SparseLineGrid<PolygonsPointIndex, PolygonsPointIndexSegmentLocator> LocToLineGrid;
std::unique_ptr<LocToLineGrid> createLocToLineGrid(const Polygons &polygons, int square_size);
} // namespace Slic3r::Arachne

namespace probe::arachne {

namespace {

using Slic3r::Arachne::SquareGrid;

int64_t span(SplitMix &g, int64_t r) { return int64_t(g.below(uint64_t(2 * r + 1))) - r; }

// ---------------------------------------------------------------- ANSQ

// op 0 toGridCoord, 1 toLowerCoord, 2 processLineCells, 3 processNearby, 4 toGridPoint.
Payload sq_coord(coord_t cell, uint8_t op, int64_t v)
{
    SquareGrid g(cell);
    Payload    p;
    p.i64(cell).u8(op).i64(v);
    p.i64(op == 0 ? g.toGridCoord(v) : g.toLowerCoord(v));
    return p;
}

Payload sq_point(coord_t cell, const Point &q)
{
    SquareGrid g(cell);
    Payload    p;
    p.i64(cell).u8(4);
    put(p, q);
    put(p, g.toGridPoint(q.cast<int64_t>()));
    return p;
}

// The cells the walk hands over until the callback stops it after `stop` cells (-1: never).
Payload sq_walk(coord_t cell, uint8_t op, const Point &a, const Point &b, int64_t radius, int64_t stop)
{
    const SquareGrid g(cell);
    Points           cells;
    auto             f = [&](const Point &c) {
        cells.push_back(c);
        return stop < 0 || int64_t(cells.size()) < stop;
    };
    const bool r = op == 2 ? g.processLineCells(std::make_pair(a, b), std::function<bool(SquareGrid::GridPoint)>(f))
                           : g.processNearby(a, radius, std::function<bool(const SquareGrid::GridPoint &)>(f));
    Payload p;
    p.i64(cell).u8(op);
    put(p, a);
    if (op == 2)
        put(p, b);
    else
        p.i64(radius);
    p.i64(stop);
    put(p, cells);
    p.boolean(r);
    return p;
}

void record_square_grid(Out &out)
{
    {
        Case c;
        for (coord_t cell : {coord_t(1), coord_t(7), coord_t(1000), coord_t(2000000)}) {
            for (int64_t v : {int64_t(0), int64_t(1), int64_t(-1), int64_t(6), int64_t(7), int64_t(-7), int64_t(-8), int64_t(999), int64_t(1000), int64_t(-1000),
                              int64_t(-1001), int64_t(2000000), int64_t(-3999999), int64_t(1) << 40, -(int64_t(1) << 40)})
                c.add("ANSQ", sq_coord(cell, 0, v));
            for (int64_t v : {int64_t(0), int64_t(1), int64_t(-1), int64_t(5), int64_t(-5), int64_t(100000)})
                c.add("ANSQ", sq_coord(cell, 1, v));
            c.add("ANSQ", sq_point(cell, Point(-1, 1)));
            c.add("ANSQ", sq_point(cell, Point(3999999, -2000001)));
        }
        out.put("squaregrid", c, true);
    }
    {
        Case c;
        // Lines through the doubled cells at 0, along the axes, diagonal and reversed, exactly on corners.
        const coord_t s = 1000;
        const std::vector<std::pair<Point, Point>> lines = {
            {Point(0, 0), Point(0, 0)},           {Point(500, 500), Point(4500, 500)},    {Point(4500, 500), Point(500, 500)},
            {Point(500, 500), Point(500, 4500)},  {Point(500, 4500), Point(500, 500)},    {Point(0, 0), Point(5000, 5000)},
            {Point(5000, 5000), Point(0, 0)},     {Point(-2500, -2500), Point(2500, 2500)}, {Point(-2500, 2500), Point(2500, -2500)},
            {Point(-3000, -1000), Point(3000, -1000)}, {Point(-1, -1), Point(1, 1)},     {Point(100, -4900), Point(-4900, 100)},
            {Point(-4500, 300), Point(4500, -2700)}, {Point(2000, 1000), Point(3000, 5000)}, {Point(-2000, -1000), Point(-3000, -5000)},
            {Point(-999, 4000), Point(999, -4000)}};
        for (const auto &[a, b] : lines)
            c.add("ANSQ", sq_walk(s, 2, a, b, 0, -1));
        c.add("ANSQ", sq_walk(s, 2, Point(0, 0), Point(5000, 5000), 0, 3));
        c.add("ANSQ", sq_walk(s, 2, Point(-2500, 2500), Point(2500, -2500), 0, 1));
        out.put("squaregrid", c, true);
    }
    {
        Case          c;
        const coord_t s = 1000;
        for (const Point &q : {Point(0, 0), Point(1500, -1500), Point(-999, 999), Point(-1000, -1000)})
            for (int64_t r : {int64_t(0), int64_t(1), int64_t(999), int64_t(1000), int64_t(2500)})
                c.add("ANSQ", sq_walk(s, 3, q, q, r, -1));
        c.add("ANSQ", sq_walk(s, 3, Point(0, 0), Point(0, 0), 3000, 5));
        out.put("squaregrid", c, true);
    }
    SplitMix g(0xA7A1B000ull);
    for (int block = 0; block < 6; ++block) {
        Case          c;
        const coord_t cell = block % 3 == 0 ? 1000 : block % 3 == 1 ? 37 : 2000000;
        for (int i = 0; i < 40; ++i) {
            const Point a(span(g, 20 * cell), span(g, 20 * cell));
            const Point b(a.x() + span(g, 12 * cell), a.y() + span(g, 12 * cell));
            c.add("ANSQ", sq_walk(cell, 2, a, b, 0, g.below(5) == 0 ? int64_t(1 + g.below(6)) : -1));
        }
        for (int i = 0; i < 16; ++i) {
            const Point a(span(g, 20 * cell), span(g, 20 * cell));
            c.add("ANSQ", sq_walk(cell, 3, a, a, int64_t(g.below(uint64_t(3 * cell))), -1));
        }
        out.put("squaregrid", c, block < 3);
    }
}

// ---------------------------------------------------------------- ANSP

struct Elem
{
    Point    p;
    uint64_t id;
};
struct ElemLocator
{
    Point operator()(const Elem &e) const { return e.p; }
};
using PointGrid = Slic3r::Arachne::SparsePointGrid<Elem, ElemLocator>;

Payload ansp(coord_t cell, size_t reserve, float load, const std::vector<Elem> &elems, const std::vector<std::pair<Point, coord_t>> &queries)
{
    PointGrid grid(cell, reserve, load);
    for (const Elem &e : elems)
        grid.insert(e);
    Payload p;
    p.i64(cell).u64(reserve).f32(load);
    p.u64(elems.size());
    for (const Elem &e : elems) {
        put(p, e.p);
        p.u64(e.id);
    }
    // The grid's iteration order (std::unordered_multimap's, as getRegionOrder walks it).
    uint64_t n = 0;
    for (auto it = grid.begin(); it != grid.end(); ++it)
        ++n;
    p.u64(n);
    for (const auto &[gp, e] : grid) {
        put(p, gp);
        p.u64(e.id);
    }
    p.u64(queries.size());
    for (const auto &[q, r] : queries) {
        put(p, q);
        p.i64(r);
        const std::vector<Elem> near = grid.getNearby(q, r);
        p.u64(near.size());
        for (const Elem &e : near)
            p.u64(e.id);
        // processNearby stopped after the second element.
        std::vector<uint64_t> seen;
        const bool            res = grid.processNearby(q, r, std::function<bool(const Elem &)>([&seen](const Elem &e) {
            seen.push_back(e.id);
            return seen.size() < 2;
        }));
        p.u64s(seen).boolean(res);
    }
    return p;
}

void record_point_grid(Out &out)
{
    {
        Case c;
        // Points sharing cells and coordinates, around the doubled cells at 0.
        std::vector<Elem> e = {{Point(0, 0), 0},        {Point(0, 0), 1},       {Point(-1, -1), 2},    {Point(999, 0), 3},  {Point(1000, 0), 4},
                               {Point(-1000, 0), 5},    {Point(1500, 1500), 6}, {Point(1500, 1500), 7}, {Point(-2500, 300), 8}, {Point(0, 0), 9}};
        std::vector<std::pair<Point, coord_t>> q = {{Point(0, 0), 0}, {Point(0, 0), 1000}, {Point(1200, 1200), 500}, {Point(-2000, 0), 1500}, {Point(9000, 9000), 10}};
        for (size_t reserve : {size_t(0), e.size(), 2 * e.size()})
            c.add("ANSP", ansp(1000, reserve, 1.f, e, q));
        out.put("pointgrid", c, true);
    }
    SplitMix g(0xA7A1B100ull);
    for (int block = 0; block < 8; ++block) {
        Case              c;
        const coord_t     cell = block % 2 == 0 ? 1000 : 399999;
        const size_t      n    = block < 2 ? 24 : size_t(40 + g.below(200));
        std::vector<Elem> e;
        for (size_t i = 0; i < n; ++i) {
            Point p(span(g, 8 * cell), span(g, 8 * cell));
            if (i > 0 && g.below(6) == 0)
                p = e[size_t(g.below(i))].p;
            e.push_back({p, i});
        }
        std::vector<std::pair<Point, coord_t>> q;
        for (int i = 0; i < 12; ++i)
            q.emplace_back(i % 3 == 0 && !e.empty() ? e[size_t(g.below(e.size()))].p : Point(span(g, 9 * cell), span(g, 9 * cell)), coord_t(g.below(uint64_t(2 * cell))));
        const size_t reserve = block % 4 == 0 ? 0 : block % 4 == 1 ? n : block % 4 == 2 ? 2 * n : 3;
        c.add("ANSP", ansp(cell, reserve, 1.f, e, q));
        out.put("pointgrid", c, block < 2);
    }
}

// ---------------------------------------------------------------- ANSL

Payload ansl(const std::string &name, const Polygons &polys, int square, const std::vector<std::pair<Point, coord_t>> &queries)
{
    auto    grid = Slic3r::Arachne::createLocToLineGrid(polys, square);
    Payload p;
    p.str(name);
    put(p, polys);
    p.i32(square);
    uint64_t n = 0;
    for (auto it = grid->begin(); it != grid->end(); ++it)
        ++n;
    p.u64(n);
    for (const auto &[gp, e] : *grid) {
        put(p, gp);
        p.u32(e.poly_idx).u32(e.point_idx);
    }
    p.u64(queries.size());
    for (const auto &[q, r] : queries) {
        put(p, q);
        p.i64(r);
        const auto near = grid->getNearby(q, r);
        p.u64(near.size());
        for (const auto &e : near)
            p.u32(e.poly_idx).u32(e.point_idx);
    }
    return p;
}

void record_line_grid(Out &out)
{
    const std::vector<Shape> all = shapes();
    size_t                   i   = 0;
    for (const Shape &s : all) {
        // fixSelfIntersections' grid (2 mm) and query (every vertex, epsilon 12499), and a finer grid.
        std::vector<std::pair<Point, coord_t>> q;
        for (const Polygon &poly : s.polygons)
            for (const Point &pt : poly.points)
                q.emplace_back(pt, 12499);
        Case c;
        c.add("ANSL", ansl(s.name, s.polygons, 2000000, q));
        if (i < 8) {
            std::vector<std::pair<Point, coord_t>> q2;
            for (size_t k = 0; k < q.size(); k += 3)
                q2.emplace_back(q[k].first, 400000);
            c.add("ANSL", ansl(s.name, s.polygons, 250000, q2));
        }
        out.put("linegrid", c, i < 3);
        ++i;
    }
}

// ---------------------------------------------------------------- ANPS

using Stitcher = Slic3r::Arachne::PolylineStitcher<VariableWidthLines, ExtrusionLine, ExtrusionJunction>;

// Cuts `whole` into pieces at random junctions (neighbouring pieces share the junction at the cut,
// as the skeletal trapezoidation's segments do), moves some ends by a little, reverses odd pieces
// (and, for `reverse_even`, even ones) and shuffles them.
void fragment(SplitMix &g, const ExtrusionLine &whole, int pieces, bool jitter, bool reverse_even, VariableWidthLines &out)
{
    const size_t n = whole.junctions.size();
    if (n < 2)
        return;
    std::vector<size_t> cuts = {0};
    for (int i = 1; i < pieces; ++i)
        cuts.push_back(1 + size_t(g.below(uint64_t(n - 1))));
    cuts.push_back(n - 1);
    std::sort(cuts.begin(), cuts.end());
    cuts.erase(std::unique(cuts.begin(), cuts.end()), cuts.end());
    for (size_t k = 0; k + 1 < cuts.size(); ++k) {
        ExtrusionLine piece(whole.inset_idx, whole.is_odd);
        for (size_t j = cuts[k]; j <= cuts[k + 1]; ++j)
            piece.junctions.push_back(whole.junctions[j]);
        if (jitter && g.below(3) == 0) {
            const int64_t d = g.below(2) == 0 ? int64_t(g.below(9000)) : int64_t(10000 + g.below(300000));
            piece.junctions.front().p += Point(coord_t(d), coord_t(span(g, 3000)));
        }
        if ((whole.is_odd || reverse_even) && g.below(2) == 0)
            piece.reverse();
        out.push_back(std::move(piece));
    }
}

ExtrusionLine loop_line(SplitMix &g, size_t inset, bool odd, double r, int n, bool close)
{
    ExtrusionLine l(inset, odd);
    const coord_t w = 300000 + coord_t(g.below(200000));
    for (int i = 0; i < n; ++i) {
        const double a = 2. * M_PI * double(i) / double(n);
        l.junctions.emplace_back(Point(coord_t(std::llround(r * 1e6 * std::cos(a))), coord_t(std::llround(r * 1e6 * std::sin(a)))), w + coord_t(g.below(50000)), coord_t(inset));
    }
    if (close)
        l.junctions.push_back(l.junctions.front());
    return l;
}

ExtrusionLine open_line(SplitMix &g, size_t inset, bool odd, int n, Point start)
{
    ExtrusionLine l(inset, odd);
    Point         p = start;
    for (int i = 0; i < n; ++i) {
        l.junctions.emplace_back(p, 200000 + coord_t(g.below(300000)), coord_t(inset));
        p += Point(coord_t(100000 + g.below(900000)), coord_t(span(g, 600000)));
    }
    return l;
}

Payload anps(const VariableWidthLines &lines, coord_t max_dist, coord_t snap)
{
    VariableWidthLines res_lines, res_polys;
    Stitcher::stitch(lines, res_lines, res_polys, max_dist, snap);
    Payload p;
    put(p, lines);
    p.i64(max_dist).i64(snap);
    put(p, res_lines);
    put(p, res_polys);
    return p;
}

void record_stitch(Out &out)
{
    {
        Case               c;
        const coord_t      w = 400000;
        VariableWidthLines none;
        c.add("ANPS", anps(none, 406999, 10000));
        // Three even pieces of a square that close; the same with a gap; odd pieces that continue.
        VariableWidthLines sq;
        auto J = [&](coord_t x, coord_t y) { return ExtrusionJunction(Point(x, y), w, 0); };
        ExtrusionLine a(0, false), b(0, false), d(0, false);
        a.junctions = {J(0, 0), J(5000000, 0), J(5000000, 5000000)};
        b.junctions = {J(5000000, 5000000), J(0, 5000000)};
        d.junctions = {J(0, 5000000), J(0, 0)};
        sq = {b, d, a};
        c.add("ANPS", anps(sq, 406999, 10000));
        VariableWidthLines gap = sq;
        gap[1].junctions.front().p += Point(50000, 0);
        c.add("ANPS", anps(gap, 406999, 10000));
        VariableWidthLines odd = sq;
        for (ExtrusionLine &l : odd)
            l.is_odd = true;
        odd[0].reverse();
        c.add("ANPS", anps(odd, 406999, 10000));
        // A tiny loop: closing is refused below three stitch distances.
        VariableWidthLines tiny;
        ExtrusionLine      t1(1, false), t2(1, false);
        t1.junctions = {ExtrusionJunction(Point(0, 0), w, 1), ExtrusionJunction(Point(300000, 0), w, 1)};
        t2.junctions = {ExtrusionJunction(Point(300000, 0), w, 1), ExtrusionJunction(Point(0, 200000), w, 1), ExtrusionJunction(Point(0, 0), w, 1)};
        tiny = {t1, t2};
        c.add("ANPS", anps(tiny, 406999, 10000));
        c.add("ANPS", anps(tiny, 100000, 10000));
        // One junction lines and a mix of odd and even at one point.
        VariableWidthLines mix;
        ExtrusionLine      m1(0, true), m2(0, false), m3(0, true);
        m1.junctions = {J(0, 0)};
        m2.junctions = {J(0, 0), J(1000000, 0)};
        m3.junctions = {J(1000000, 0), J(1000000, 1000000), J(0, 3000)};
        mix = {m1, m2, m3};
        c.add("ANPS", anps(mix, 406999, 10000));
        out.put("stitch", c, true);
    }
    SplitMix g(0xA7A1B200ull);
    for (int block = 0; block < 14; ++block) {
        Case c;
        for (int k = 0; k < 4; ++k) {
            VariableWidthLines lines;
            const size_t       inset = size_t(g.below(3));
            const int          loops = 1 + int(g.below(3));
            for (int l = 0; l < loops; ++l) {
                const bool odd = g.below(3) == 0;
                fragment(g, loop_line(g, inset, odd, 0.5 + 0.8 * double(l) + double(g.below(4000)) / 1000., 6 + int(g.below(60)), true), 1 + int(g.below(7)), block >= 4, block % 5 == 4, lines);
            }
            if (g.below(2) == 0)
                fragment(g, open_line(g, inset, true, 2 + int(g.below(12)), Point(coord_t(span(g, 3000000)), coord_t(span(g, 3000000)))), 1 + int(g.below(4)), block >= 4, false, lines);
            for (size_t i = lines.size(); i > 1; --i)
                std::swap(lines[i - 1], lines[size_t(g.below(i))]);
            const coord_t max_dist = g.below(3) == 0 ? 100000 : 406999;
            c.add("ANPS", anps(lines, max_dist, g.below(4) == 0 ? 1000 : 10000));
        }
        out.put("stitch", c, block < 3);
    }
}

} // namespace

void record_grids(Out &out)
{
    record_square_grid(out);
    record_point_grid(out);
    record_line_grid(out);
    record_stitch(out);
}

} // namespace probe::arachne
