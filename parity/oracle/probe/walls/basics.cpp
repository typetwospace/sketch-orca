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
// walls.sop1's records of the functions walls-1 carries that are called on their own
// (parity/sop1/walls.md), recorded by orca-o3-probe-walls (2026-11-13):
//
//   WLMT  compute_moments_of_area_of_triangle (PrincipalComponents2D.cpp)
//   WLPC  compute_principal_components (PrincipalComponents2D.cpp)
//   WLDB  detect_bridging_direction, both overloads (BridgeDetector.hpp)
//   WLBD  BridgeDetector: detect_angle, coverage (precise and not) and unsupported_edges, which
//         reach its private bridge_direction_candidates and the static get_trapezoids2 and
//         get_trapezoids3_half (BridgeDetector.cpp)
//   WLEA  extrusion_paths_append of ClipperLib_Z paths and of an Arachne line
//         (Arachne/utils/ExtrusionLine.cpp)
//
// Each record holds its inputs, then what the engine computed from them.
#include "values.hpp"

#include <array>
#include <cmath>
#include <exception>
#include <string>
#include <tuple>
#include <vector>

#include "libslic3r/BridgeDetector.hpp"
#include "libslic3r/ClipperUtils.hpp"
#include "libslic3r/PrincipalComponents2D.hpp"

namespace probe::walls {

using namespace Slic3r;

namespace {

constexpr double PI_ = 3.14159265358979323846;

coord_t sc(double mm) { return coord_t(std::llround(mm * 1000000.)); }

// ---- WLMT ----------------------------------------------------------------------------------

void moments_case(Writer &w, const Vec2f &a, const Vec2f &b, const Vec2f &c, bool committed)
{
    Payload p;
    p.f32(a.x()).f32(a.y()).f32(b.x()).f32(b.y()).f32(c.x()).f32(c.y());
    auto [area, first, second, cov] = compute_moments_of_area_of_triangle(a, b, c);
    p.f32(area).f32(first.x()).f32(first.y()).f32(second.x()).f32(second.y()).f32(cov);
    w.add("WLMT", p, committed);
}

// ---- WLPC ----------------------------------------------------------------------------------

void principal_case(Writer &w, const Polygons &polys, bool committed)
{
    Payload p;
    put_polygons(p, polys);
    auto [pc1, pc2] = compute_principal_components(polys);
    p.f32(pc1.x()).f32(pc1.y()).f32(pc2.x()).f32(pc2.y());
    w.add("WLPC", p, committed);
}

Polygon star(SplitMix &g, double cx, double cy, double rmin, double rmax, int n)
{
    Polygon out;
    for (int i = 0; i < n; ++i) {
        const double a = 2. * PI_ * double(i) / double(n);
        const double r = real(g, rmin, rmax);
        out.points.emplace_back(sc(cx + r * std::cos(a)), sc(cy + r * std::sin(a)));
    }
    return out;
}

// ---- WLDB ----------------------------------------------------------------------------------

void direction_polygons_case(Writer &w, const Polygons &to_cover, const Polygons &anchors, bool committed)
{
    Payload p;
    p.u8(1);
    put_polygons(p, to_cover);
    put_polygons(p, anchors);
    auto [dir, cost] = detect_bridging_direction(to_cover, anchors);
    p.f64(dir.x()).f64(dir.y()).f64(cost);
    w.add("WLDB", p, committed);
}

void direction_lines_case(Writer &w, const Lines &edges, const Polygons &area, bool committed)
{
    Payload p;
    p.u8(0);
    put_lines(p, edges);
    put_polygons(p, area);
    auto [dir, cost] = detect_bridging_direction(edges, area);
    p.f64(dir.x()).f64(dir.y()).f64(cost);
    w.add("WLDB", p, committed);
}

// ---- WLBD ----------------------------------------------------------------------------------

struct BridgeCase
{
    bool       one;     // the ExPolygon constructor (else the ExPolygons one)
    ExPolygons expolygons;
    ExPolygons lower;
    coord_t    spacing;
    double     override_angle;
    double     angle2;
};

void bridge_case(Writer &w, const BridgeCase &c, bool committed)
{
    Payload p;
    p.u8(c.one ? 0 : 1);
    put_expolygons(p, c.expolygons);
    put_expolygons(p, c.lower);
    p.i64(int64_t(c.spacing)).f64(c.override_angle).f64(c.angle2);
    // The detector holds a reference to its expolygons (to its own copy for the one-ExPolygon
    // constructor), so it is never copied: each constructor's detector is run where it is made.
    // The result is built apart and joins the record, after u8 1, only once every call has
    // returned: a call that throws leaves u8 2 and the message where the result would be.
    auto run = [&](BridgeDetector &d) {
        Payload r;
        r.f64(d.resolution);
        const bool found = d.detect_angle(c.override_angle);
        r.boolean(found).f64(d.angle);
        put_polygons(r, d.coverage(-1, true));
        put_polygons(r, d.coverage(-1, false));
        put_polygons(r, d.coverage(c.angle2, true));
        put_polylines(r, d.unsupported_edges(-1));
        put_polylines(r, d.unsupported_edges(c.angle2));
        p.u8(1);
        p.bytes += r.bytes;
    };
    try {
        if (c.one) {
            BridgeDetector d(c.expolygons.front(), c.lower, c.spacing);
            run(d);
        } else {
            BridgeDetector d(c.expolygons, c.lower, c.spacing);
            run(d);
        }
    } catch (const std::exception &e) {
        p.u8(2).str(e.what());
    }
    w.add("WLBD", p, committed);
}

// ---- WLEA ----------------------------------------------------------------------------------

Flow role_flow(int which)
{
    switch (which) {
    case 0: return Flow(0.45f, 0.2f, 0.4f);
    case 1: return Flow(0.42f, 0.2f, 0.4f);
    case 2: return Flow::bridging_flow(0.4f, 0.4f);
    case 3: return Flow(0.45f, 0.2f, 0.4f).with_flow_ratio(0.9);
    default: return Flow(0.62f, 0.3f, 0.6f);
    }
}

void append_zpaths_case(Writer &w, const ClipperLib_Z::Paths &paths, ExtrusionRole role, const Flow &flow, bool committed)
{
    Payload p;
    p.u8(0).u8(uint8_t(role));
    put_flow(p, flow);
    put_zpaths(p, paths);
    try {
        ExtrusionPaths dst;
        extrusion_paths_append(dst, paths, role, flow);
        p.u8(1);
        put_paths(p, dst);
    } catch (const std::exception &e) {
        p.u8(2).str(e.what());
    }
    w.add("WLEA", p, committed);
}

void append_line_case(Writer &w, const Arachne::ExtrusionLine &line, ExtrusionRole role, const Flow &flow, bool committed)
{
    Payload p;
    p.u8(1).u8(uint8_t(role));
    put_flow(p, flow);
    put_arachne_line(p, line);
    try {
        ExtrusionPaths dst;
        extrusion_paths_append(dst, line, role, flow);
        p.u8(1);
        put_paths(p, dst);
    } catch (const std::exception &e) {
        p.u8(2).str(e.what());
    }
    w.add("WLEA", p, committed);
}

// A path of n points with widths: constant (mode 0), slowly varying within the 0.05 mm tolerance
// (1), varying past it (2), or jumping (3); now and then a repeated point.
ClipperLib_Z::Path zpath(SplitMix &g, int n, int mode, bool closed)
{
    ClipperLib_Z::Path out;
    double x = real(g, -10, 10), y = real(g, -10, 10), wmm = real(g, 0.25, 0.6);
    for (int i = 0; i < n; ++i) {
        if (i > 0 && g.below(9) == 0) {
            out.push_back(out.back());
            continue;
        }
        if (i > 0) {
            x += real(g, -3, 3);
            y += real(g, -3, 3);
            if (mode == 1)
                wmm += real(g, -0.02, 0.02);
            else if (mode == 2)
                wmm += real(g, -0.2, 0.2);
            else if (mode == 3)
                wmm = real(g, 0.15, 0.9);
            if (wmm < 0.12)
                wmm = 0.12;
        }
        out.emplace_back(sc(x), sc(y), sc(wmm));
    }
    if (closed && out.size() >= 2)
        out.push_back(out.front());
    return out;
}

} // namespace

void record_moments(Writer &w)
{
    std::vector<std::array<float, 6>> hand = {
        {0, 0, 0, 0, 0, 0},        {0, 0, 1, 0, 0, 1},       {0, 0, 0, 1, 1, 0},          {1, 1, 2, 2, 3, 3},
        {-5, -5, 5, -5, 0, 5},     {100, 100, 101, 100, 100, 101}, {-150, 20, 150, 20.5f, 0, -120},
        {0.001f, 0, 0, 0.001f, 0, 0}, {1e-6f, 2e-6f, 3e-6f, -1e-6f, 0, 0}, {20, 0, 0, 20, 20, 20},
    };
    for (const auto &t : hand)
        moments_case(w, Vec2f(t[0], t[1]), Vec2f(t[2], t[3]), Vec2f(t[4], t[5]), true);
    SplitMix g(0x3A11E001ull);
    for (int i = 0; i < 300; ++i) {
        const double s = i % 3 == 0 ? 150. : i % 3 == 1 ? 10. : 0.5;
        Vec2f a(float(real(g, -s, s)), float(real(g, -s, s))), b(float(real(g, -s, s)), float(real(g, -s, s))), c(float(real(g, -s, s)), float(real(g, -s, s)));
        moments_case(w, a, b, c, i < 30);
    }
}

void record_principal(Writer &w)
{
    std::vector<Polygons> hand;
    hand.push_back({});
    hand.push_back({rect(0, 0, 10, 10)});
    hand.push_back({rect(-10, -2.5, 10, 2.5)});
    hand.push_back({rect(-2.5, -10, 2.5, 10)});
    for (double deg : {10., 45., 100., 135., 170.})
        hand.push_back({rotated(rect(-10, -2, 10, 2), deg * PI_ / 180.)});
    {
        Polygon cw = rect(0, 0, 10, 10);
        cw.reverse();
        hand.push_back({cw});
    }
    {
        Polygon h = circle(5, 5, 2, 24);
        h.make_clockwise();
        hand.push_back({rect(0, 0, 10, 10), h});
    }
    hand.push_back({poly({{0, 0}, {8, 0}, {0, 3}})});
    hand.push_back({poly({{0, 0}, {10, 0}, {10, 2}, {2, 2}, {2, 12}, {0, 12}})});
    hand.push_back({poly({{0, 0}, {0.000001, 0}, {0.000001, 0.000001}, {0, 0.000001}})});
    hand.push_back({rect(0, 0, 5, 1), rect(10, 0, 11, 7)});
    hand.push_back({circle(0, 0, 4, 48)});
    hand.push_back({poly({{0, 0}, {5, 5}})});
    for (const Polygons &p : hand)
        principal_case(w, p, true);
    SplitMix g(0x3A11E002ull);
    for (int i = 0; i < 120; ++i) {
        Polygons ps;
        const int n = 1 + int(g.below(3));
        for (int k = 0; k < n; ++k)
            ps.push_back(star(g, real(g, -30, 30), real(g, -30, 30), real(g, 0.5, 3), real(g, 3, 12), 3 + int(g.below(14))));
        if (g.below(5) == 0)
            ps.back().reverse();
        principal_case(w, ps, i < 16);
    }
}

void record_bridging_direction(Writer &w)
{
    // The polygons overload: an overhang and the anchors around it.
    struct Hand { Polygons cover, anchors; };
    std::vector<Hand> hand = {
        {{rect(0, 0, 10, 4)}, {rect(-3, -1, 0.2, 5)}},
        {{rect(0, 0, 10, 4)}, {rect(-3, -1, 0.2, 5), rect(9.8, -1, 13, 5)}},
        {{rect(0, 0, 10, 4)}, {rect(-3, -1, 0.2, 5), rect(9.8, -1, 13, 5), rect(-3, -3, 13, 0.2)}},
        {{rect(0, 0, 10, 4)}, {rect(-3, -3, 13, 0.2), rect(-3, 3.8, 13, 7), rect(-3, -3, 0.2, 7), rect(9.8, -3, 13, 7)}},
        {{rect(0, 0, 10, 4)}, {}},
        {{rect(0, 0, 0.0001, 0.0001)}, {}},
        {{circle(0, 0, 3, 32)}, {rect(-5, -5, 5, -2.5)}},
        {{poly({{0, 0}, {10, 0}, {10, 10}, {8, 10}, {8, 2}, {0, 2}})}, {rect(-2, -1, 0.3, 3), rect(7.7, 9.7, 10.5, 12)}},
        {{}, {rect(0, 0, 1, 1)}},
    };
    for (const Hand &h : hand)
        direction_polygons_case(w, h.cover, h.anchors, true);
    for (double deg : {15., 60., 90., 123., 181., 270.}) {
        const double a = deg * PI_ / 180.;
        direction_polygons_case(w, {rotated(rect(0, 0, 10, 4), a)}, {rotated(rect(-3, -1, 0.2, 5), a), rotated(rect(9.8, -1, 13, 5), a)}, true);
        direction_polygons_case(w, {rotated(rect(0, 0, 10, 4), a)}, {rotated(rect(-3, -1, 0.2, 5), a)}, deg < 100.);
    }
    // The lines overload, on its own: floating edges and the overhang's area.
    direction_lines_case(w, {}, {rect(0, 0, 10, 4)}, true);
    direction_lines_case(w, {}, {rect(0, 0, 4, 10)}, true);
    direction_lines_case(w, {}, {}, true);
    direction_lines_case(w, {Line(Point(0, 0), Point(sc(10), 0))}, {rect(0, 0, 10, 4)}, true);
    direction_lines_case(w, {Line(Point(0, 0), Point(sc(10), 0)), Line(Point(sc(10), sc(4)), Point(0, sc(4)))}, {rect(0, 0, 10, 4)}, true);
    // Two edges whose normals quantise to the same key: the first one's normal is kept.
    direction_lines_case(w, {Line(Point(0, 0), Point(sc(10), 0)), Line(Point(0, sc(1)), Point(sc(10), sc(1) + 1))}, {rect(0, 0, 10, 4)}, true);
    SplitMix g(0x3A11E003ull);
    for (int i = 0; i < 80; ++i) {
        Lines edges;
        const int n = int(g.below(8));
        for (int k = 0; k < n; ++k) {
            const Point a(sc(real(g, -10, 10)), sc(real(g, -10, 10)));
            Point b(a.x() + sc(real(g, -6, 6)), a.y() + sc(real(g, -6, 6)));
            if (g.below(6) == 0)
                b = Point(a.x() + sc(real(g, 1, 5)), a.y());
            edges.emplace_back(a, b);
        }
        direction_lines_case(w, edges, {star(g, 0, 0, 1, 6, 5 + int(g.below(8)))}, i < 12);
    }
    for (int i = 0; i < 60; ++i) {
        const double a = real(g, 0, 2 * PI_);
        Polygons cover = {rotated(star(g, 0, 0, 2, 6, 6 + int(g.below(10))), a)};
        Polygons anchors;
        const int n = int(g.below(4));
        for (int k = 0; k < n; ++k) {
            const double ang = real(g, 0, 2 * PI_), r = real(g, 3, 7);
            anchors.push_back(rotated(rect(-2, -2, 2, 2), ang, 0, 0));
            for (Point &pt : anchors.back().points)
                pt += Point(sc(r * std::cos(ang)), sc(r * std::sin(ang)));
        }
        direction_polygons_case(w, cover, anchors, i < 8);
    }
}

void record_bridge_detector(Writer &w)
{
    const coord_t s45 = sc(0.45), s4 = sc(0.45) / 4;
    std::vector<BridgeCase> cases;
    auto add = [&](bool one, ExPolygons e, ExPolygons lower, coord_t spacing, double over, double a2) {
        cases.push_back({one, std::move(e), std::move(lower), spacing, over, a2});
    };
    const ExPolygons pads = {ExPolygon(rect(-3, -1, 0.5, 5)), ExPolygon(rect(9.5, -1, 13, 5))};
    // A straight bridge between two pads, with the finer spacing process_no_bridge uses.
    add(true, {ExPolygon(rect(0, 0, 10, 4))}, pads, s45, 0., 0.3);
    add(true, {ExPolygon(rect(0, 0, 10, 4))}, pads, s4, 0., PI_ / 2);
    add(false, {ExPolygon(rect(0, 0, 10, 4))}, pads, s45, 0., 1.0);
    // Overrides: one direction only, one past PI (folded back), one along the bridge.
    add(true, {ExPolygon(rect(0, 0, 10, 4))}, pads, s45, PI_ / 2, 0.);
    add(true, {ExPolygon(rect(0, 0, 10, 4))}, pads, s45, 3.5, 2.);
    add(true, {ExPolygon(rect(0, 0, 10, 4))}, pads, s45, 0.01, 2.);
    add(true, {ExPolygon(rect(0, 0, 10, 4))}, pads, s45, PI_, 0.5);
    // Rotated bridges.
    for (double deg : {30., 75., 140.}) {
        const double a = deg * PI_ / 180.;
        add(true, {rotated(ExPolygon(rect(0, 0, 10, 4)), a, 5, 2)}, {rotated(pads[0], a, 5, 2), rotated(pads[1], a, 5, 2)}, s45, 0., a);
    }
    // U-shaped support, support all around (a counterbore's ring), none at all, far away.
    add(true, {ExPolygon(rect(0, 0, 10, 4))}, {ExPolygon(poly({{-3, -3}, {13, -3}, {13, 7}, {9.5, 7}, {9.5, 0.5}, {0.5, 0.5}, {0.5, 7}, {-3, 7}}))}, s45, 0., 0.);
    add(true, {ExPolygon(circle(0, 0, 3, 40))}, {with_holes(rect(-8, -8, 8, 8), {circle(0, 0, 2.6, 40)})}, s4, 0., 0.7);
    add(true, {ExPolygon(rect(0, 0, 10, 4))}, {}, s45, 0., 0.);
    add(true, {ExPolygon(rect(0, 0, 10, 4))}, {ExPolygon(rect(30, 30, 40, 40))}, s45, 0., 0.);
    // An L-shaped bridge, two bridges at once, a bridge with a hole.
    add(true, {ExPolygon(poly({{0, 0}, {10, 0}, {10, 10}, {7, 10}, {7, 3}, {0, 3}}))}, {ExPolygon(rect(-3, -1, 0.5, 4)), ExPolygon(rect(6, 9.5, 11, 13))}, s45, 0., 0.);
    add(false, {ExPolygon(rect(0, 0, 10, 4)), ExPolygon(rect(0, 8, 10, 12))}, {ExPolygon(rect(-3, -1, 0.5, 13)), ExPolygon(rect(9.5, -1, 13, 13))}, s45, 0., 0.2);
    add(true, {with_holes(rect(0, 0, 10, 6), {rect(4, 2, 6, 4)})}, pads, s45, 0., 0.);
    add(true, {ExPolygon(rect(0, 0, 10, 4))}, {ExPolygon(rect(-3, -1, 0.5, 5))}, s45, 0., 0.);
    SplitMix g(0x3A11E004ull);
    for (int i = 0; i < 40; ++i) {
        const double a = real(g, 0, PI_), len = real(g, 3, 15), wid = real(g, 1, 6);
        ExPolygon bridge(rect(0, 0, len, wid));
        ExPolygons lower = {ExPolygon(rect(-3, -1, 0.4, wid + 1))};
        if (g.below(3) != 0)
            lower.push_back(ExPolygon(rect(len - 0.4, -1, len + 3, wid + 1)));
        if (g.below(4) == 0)
            lower.push_back(ExPolygon(rect(-3, -3, len + 3, 0.3)));
        for (ExPolygon &e : lower)
            e = rotated(e, a);
        cases.push_back({true, {rotated(bridge, a)}, lower, g.below(2) == 0 ? s45 : s4, g.below(5) == 0 ? real(g, 0.01, 3.5) : 0., real(g, 0, PI_)});
    }
    for (size_t i = 0; i < cases.size(); ++i)
        bridge_case(w, cases[i], i < 26);
}

void record_paths_append(Writer &w)
{
    const ExtrusionRole roles[] = {erExternalPerimeter, erPerimeter, erOverhangPerimeter};
    // By hand: a two-point path, a path with a tiny segment at each place it can be, one whose
    // width steps past the tolerance, and an overhang with a bridging flow (its width kept).
    {
        ClipperLib_Z::Paths ps = {{{0, 0, sc(0.45)}, {sc(10), 0, sc(0.45)}}};
        append_zpaths_case(w, ps, erPerimeter, role_flow(0), true);
        append_zpaths_case(w, ps, erOverhangPerimeter, role_flow(2), true);
        append_zpaths_case(w, ps, erOverhangPerimeter, role_flow(3), true);
        append_zpaths_case(w, {}, erPerimeter, role_flow(0), true);
    }
    {
        ClipperLib_Z::Paths ps = {{{0, 0, sc(0.4)}, {5, 0, sc(0.4)}, {sc(10), 0, sc(0.42)}, {sc(10), sc(10), sc(0.6)}, {sc(10), sc(10) + 3, sc(0.6)}}};
        append_zpaths_case(w, ps, erExternalPerimeter, role_flow(1), true);
        ClipperLib_Z::Paths one_tiny = {{{0, 0, sc(0.4)}, {5, 0, sc(0.4)}}};
        append_zpaths_case(w, one_tiny, erPerimeter, role_flow(0), true);
        ClipperLib_Z::Paths steps = {{{0, 0, sc(0.2)}, {sc(4), 0, sc(0.8)}, {sc(4), sc(4), sc(0.3)}}, {{sc(20), 0, sc(0.4)}, {sc(25), 0, sc(0.45)}}};
        append_zpaths_case(w, steps, erPerimeter, role_flow(0), true);
    }
    SplitMix g(0x3A11E005ull);
    for (int i = 0; i < 160; ++i) {
        ClipperLib_Z::Paths ps;
        const int n = 1 + int(g.below(3));
        for (int k = 0; k < n; ++k)
            ps.push_back(zpath(g, 2 + int(g.below(10)), int(g.below(4)), g.below(3) == 0));
        append_zpaths_case(w, ps, roles[g.below(3)], role_flow(int(g.below(5))), i < 8);
    }
    // Arachne lines: closed and open, odd and even.
    for (int i = 0; i < 120; ++i) {
        Arachne::ExtrusionLine line(size_t(g.below(4)), g.below(3) == 0);
        const bool closed = g.below(2) == 0;
        ClipperLib_Z::Path zp = zpath(g, 2 + int(g.below(12)), int(g.below(4)), closed);
        for (const ClipperLib_Z::IntPoint &pt : zp)
            line.junctions.emplace_back(Point(pt.x(), pt.y()), coord_t(pt.z()), coord_t(line.inset_idx));
        line.is_closed = closed;
        append_line_case(w, line, roles[g.below(3)], role_flow(int(g.below(5))), i < 12);
    }
}

} // namespace probe::walls
