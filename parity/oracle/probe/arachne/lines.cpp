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
// ANLA, ANXL and ANAE (parity/sop1/arachne.md), piece arachne-1: linearAlg2D's two functions,
// ExtrusionLine's operations (getLength, is_contour, area, to_thick_polyline, to_polygon,
// to_points and simplify with the parameters WallToolPaths gives it and others) and
// ExtrusionLine::calculateExtrusionAreaDeviationError. Inputs come from splitmix64 generators
// (seeds 0xA7A1A000, 0xA7A1A100, 0xA7A1A200). Recorded by orca-o3-probe-arachne, 2026-11-13.
#include "common.hpp"

#include <cmath>

#include "libslic3r/Arachne/utils/linearAlg2D.hpp"
#include "libslic3r/Polyline.hpp"

namespace probe::arachne {

namespace {

int64_t span(SplitMix &g, int64_t r) { return int64_t(g.below(uint64_t(2 * r + 1))) - r; }

Point rpt(SplitMix &g, int64_t r) { return Point(span(g, r), span(g, r)); }

// ANLA: p, a, b, c; pointIsLeftOfLine(p, a, b), getAngleLeft(a, b, c).
Payload anla(const Point &p, const Point &a, const Point &b, const Point &c)
{
    Payload out;
    put(out, p);
    put(out, a);
    put(out, b);
    put(out, c);
    out.i64(Slic3r::Arachne::LinearAlg2D::pointIsLeftOfLine(p, a, b));
    out.f32(Slic3r::Arachne::LinearAlg2D::getAngleLeft(a, b, c));
    return out;
}

void record_linalg(Out &out)
{
    std::vector<Payload> edges;
    const Point          o(0, 0);
    // Collinear: pointy (same direction), straight (opposite), zero vectors, each axis.
    edges.push_back(anla(Point(5, 5), Point(10, 0), o, Point(20, 0)));
    edges.push_back(anla(Point(-5, 5), Point(10, 0), o, Point(-20, 0)));
    edges.push_back(anla(Point(0, 7), Point(0, 10), o, Point(0, 3)));
    edges.push_back(anla(Point(0, -7), Point(0, 10), o, Point(0, -3)));
    edges.push_back(anla(Point(1, 1), o, o, Point(4, 0)));
    edges.push_back(anla(Point(1, 1), Point(4, 0), o, o));
    edges.push_back(anla(o, o, o, o));
    edges.push_back(anla(Point(3, 3), Point(-4, 0), o, Point(-1, 0)));
    edges.push_back(anla(Point(3, -3), Point(0, -4), o, Point(0, 9)));
    // Right angles both ways and near-straight angles.
    edges.push_back(anla(Point(1, 2), Point(10, 0), o, Point(0, 10)));
    edges.push_back(anla(Point(2, 1), Point(0, 10), o, Point(10, 0)));
    edges.push_back(anla(Point(0, 0), Point(-1000000, 1), o, Point(1000000, 0)));
    edges.push_back(anla(Point(0, 0), Point(-1000000, -1), o, Point(1000000, 0)));
    edges.push_back(anla(Point(1000000000, -1000000000), Point(-1000000000, 1000000000), Point(999999999, 1), Point(-999999998, -1000000000)));
    Case c;
    for (const Payload &p : edges)
        c.add("ANLA", p);
    out.put("linalg", c, true);
    SplitMix g(0xA7A1A000ull);
    for (int block = 0; block < 8; ++block) {
        Case          r;
        const int64_t range = block % 4 == 0 ? 1000 : block % 4 == 1 ? 1000000 : block % 4 == 2 ? 100000000 : 1000000000;
        for (int i = 0; i < 64; ++i)
            r.add("ANLA", anla(rpt(g, range), rpt(g, range), rpt(g, range), rpt(g, range)));
        out.put("linalg", r, block < 4);
    }
}

// ---------------------------------------------------------------- ExtrusionLine

ExtrusionLine make_line(size_t inset, bool odd, bool closed, const std::vector<std::pair<Point, coord_t>> &js)
{
    ExtrusionLine l(inset, odd);
    l.is_closed = closed;
    for (const auto &[pt, w] : js)
        l.junctions.emplace_back(pt, w, coord_t(inset));
    return l;
}

// A line along a polyline of `n` points with widths that vary, open or closed (closed: the first
// junction repeated at the end, as WallToolPaths leaves its closed lines).
ExtrusionLine random_line(SplitMix &g, int kind)
{
    const size_t                              inset  = size_t(g.below(4));
    const bool                                odd    = g.below(2) == 0;
    const bool                                closed = kind % 2 == 1;
    std::vector<std::pair<Point, coord_t>>    js;
    const int                                 n      = 2 + int(g.below(40));
    const coord_t                             w0     = 200000 + coord_t(g.below(400000));
    if (kind < 2) {
        // A wobbly circle: segments from microns to millimetres.
        const double r = 1. + double(g.below(20000)) / 1000.;
        for (int i = 0; i < n; ++i) {
            const double a  = 2. * M_PI * double(i) / double(n) + double(g.below(100)) / 1000.;
            const double rr = r * (1. + double(g.below(50)) / 1000.);
            js.emplace_back(Point(coord_t(std::llround(rr * 1e6 * std::cos(a))), coord_t(std::llround(rr * 1e6 * std::sin(a)))), w0 + coord_t(g.below(20000)));
        }
    } else if (kind < 4) {
        // A walk of short steps (below the resolution) with turns, spikes and repeats.
        Point p(0, 0);
        for (int i = 0; i < n; ++i) {
            const int     step  = int(g.below(6));
            const int64_t len   = step == 0 ? int64_t(g.below(6000)) : step == 1 ? int64_t(g.below(60000)) : step == 2 ? int64_t(g.below(700000)) : int64_t(g.below(3000000));
            const double  a     = double(g.below(6283)) / 1000.;
            p                   = Point(p.x() + coord_t(std::llround(double(len) * std::cos(a))), p.y() + coord_t(std::llround(double(len) * std::sin(a))));
            js.emplace_back(p, w0 + coord_t(g.below(3)) * coord_t(g.below(80000)));
            if (g.below(8) == 0)
                js.emplace_back(p, w0);
        }
    } else {
        // Nearly collinear runs with a long segment after short ones (the corner the simplification
        // moves to the two lines' intersection).
        Point p(0, 0);
        for (int i = 0; i < n; ++i) {
            const bool    long_seg = g.below(4) == 0;
            const int64_t dx       = long_seg ? 2000000 + int64_t(g.below(3000000)) : 1000 + int64_t(g.below(300000));
            const int64_t dy       = span(g, long_seg ? 30000 : 3000) + (i % 7 == 3 ? 200000 : 0);
            p                      = Point(p.x() + dx, p.y() + dy);
            js.emplace_back(p, w0 + coord_t(g.below(5)));
        }
    }
    if (closed && !js.empty())
        js.push_back(js.front());
    return make_line(inset, odd, closed, js);
}

struct SimplifyArgs
{
    int64_t seg2, err2, area;
};

// ANXL: the line; getLength; is_contour; area (closed lines); to_thick_polyline (two junctions or
// more); to_polygon (closed lines of three junctions or more whose ends meet); to_points; then
// simplify for each argument triple.
Payload anxl(const ExtrusionLine &l, const std::vector<SimplifyArgs> &args)
{
    Payload p;
    put(p, l);
    p.i64(l.getLength()).boolean(l.is_contour());
    p.boolean(l.is_closed);
    if (l.is_closed)
        p.f64(l.area());
    const bool thick = l.junctions.size() >= 2;
    p.boolean(thick);
    if (thick) {
        const Slic3r::ThickPolyline t = Slic3r::Arachne::to_thick_polyline(l);
        put(p, t.points);
        p.u64(t.width.size());
        for (double w : t.width)
            p.f64(w);
        p.boolean(t.endpoints.first).boolean(t.endpoints.second);
    }
    const bool poly = l.is_closed && l.junctions.size() >= 3 && l.junctions.front().p == l.junctions.back().p;
    p.boolean(poly);
    if (poly)
        put(p, Slic3r::Arachne::to_polygon(l).points);
    put(p, Slic3r::Arachne::to_points(l));
    p.u64(args.size());
    for (const SimplifyArgs &a : args) {
        ExtrusionLine s = l;
        s.simplify(a.seg2, a.err2, a.area);
        p.i64(a.seg2).i64(a.err2).i64(a.area);
        put(p, s);
    }
    return p;
}

void record_lines(Out &out)
{
    // simplifyToolPaths' arguments for the default wall resolution and deviation, then finer and coarser ones.
    const std::vector<SimplifyArgs> args = {{500000LL * 500000LL, 25000LL * 25000LL, 2000000},
                                            {100000LL * 100000LL, 5000LL * 5000LL, 2000000},
                                            {1000000LL * 1000000LL, 50000LL * 50000LL, 20000000},
                                            {500000LL * 500000LL, 25000LL * 25000LL, 0}};
    // Hand-made lines: the short ones simplify returns unchanged, a closed square, a spike.
    {
        Case c;
        const coord_t w = 400000;
        c.add("ANXL", anxl(make_line(0, false, false, {}), args));
        c.add("ANXL", anxl(make_line(0, true, false, {{Point(0, 0), w}}), args));
        c.add("ANXL", anxl(make_line(1, true, false, {{Point(0, 0), w}, {Point(1000000, 0), w + 50000}}), args));
        c.add("ANXL", anxl(make_line(0, false, true, {{Point(0, 0), w}, {Point(0, 1000000), w}, {Point(1000000, 1000000), w}, {Point(0, 0), w}}), args));
        c.add("ANXL", anxl(make_line(0, false, true, {{Point(0, 0), w}, {Point(0, 5000000), w}, {Point(5000000, 5000000), w}, {Point(5000000, 0), w}, {Point(0, 0), w}}), args));
        c.add("ANXL", anxl(make_line(0, false, true, {{Point(0, 0), w}, {Point(5000000, 0), w}, {Point(5000000, 5000000), w}, {Point(0, 5000000), w}, {Point(0, 0), w}}), args));
        c.add("ANXL", anxl(make_line(2, true, false, {{Point(0, 0), w}, {Point(3000, 0), w}, {Point(6000, 10), w}, {Point(2000000, 0), w}, {Point(2000000, 1000), w}, {Point(0, 1000), w}}), args));
        c.add("ANXL", anxl(make_line(1, true, false, {{Point(0, 0), w}, {Point(1000000, 0), w}, {Point(1300000, 0), 2 * w}, {Point(2600000, 0), w}, {Point(2600000, 0), w}}), args));
        c.add("ANXL", anxl(make_line(0, false, false, {{Point(0, 0), w}, {Point(5000000, 0), w}, {Point(5300000, 400000), w}, {Point(5300000, 6000000), w}}), args));
        out.put("line", c, true);
    }
    SplitMix g(0xA7A1A100ull);
    for (int block = 0; block < 24; ++block) {
        Case c;
        for (int i = 0; i < 6; ++i)
            c.add("ANXL", anxl(random_line(g, int(g.below(6))), args));
        out.put("line", c, true);
    }
}

// ANAE: three junctions; calculateExtrusionAreaDeviationError.
Payload anae(const ExtrusionJunction &a, const ExtrusionJunction &b, const ExtrusionJunction &c)
{
    Payload p;
    put(p, a);
    put(p, b);
    put(p, c);
    p.i64(ExtrusionLine::calculateExtrusionAreaDeviationError(a, b, c));
    return p;
}

void record_area_error(Out &out)
{
    Case       e;
    const auto J = [](coord_t x, coord_t y, coord_t w) { return ExtrusionJunction(Point(x, y), w, 0); };
    e.add("ANAE", anae(J(0, 0, 400000), J(1000000, 0, 400000), J(2000000, 0, 400000)));
    e.add("ANAE", anae(J(0, 0, 400000), J(1000000, 0, 400001), J(3000000, 0, 400000)));
    e.add("ANAE", anae(J(0, 0, 400000), J(1000000, 0, 400002), J(3000000, 0, 400000)));
    e.add("ANAE", anae(J(0, 0, 400000), J(1000000, 0, 500000), J(1500000, 0, 300000)));
    e.add("ANAE", anae(J(0, 0, 400000), J(1000000, 1000000, 200000), J(0, 2000000, 600000)));
    e.add("ANAE", anae(J(0, 0, 400000), J(0, 0, 400000), J(10, 0, 400001)));
    e.add("ANAE", anae(J(0, 0, 400000), J(5, 0, 400000), J(5, 0, 400009)));
    out.put("areaerr", e, true);
    SplitMix g(0xA7A1A200ull);
    for (int block = 0; block < 4; ++block) {
        Case c;
        for (int i = 0; i < 48; ++i) {
            const int64_t r  = block % 2 == 0 ? 100000 : 5000000;
            const coord_t w0 = 100000 + coord_t(g.below(500000));
            ExtrusionJunction a(rpt(g, r), w0, 0), b(rpt(g, r), w0 + coord_t(span(g, block < 2 ? 1 : 200000)), 0), cc(rpt(g, r), w0 + coord_t(span(g, 200000)), 0);
            c.add("ANAE", anae(a, b, cc));
        }
        out.put("areaerr", c, true);
    }
}

} // namespace

void record_lines_all(Out &out)
{
    record_linalg(out);
    record_lines(out);
    record_area_error(out);
}

} // namespace probe::arachne
