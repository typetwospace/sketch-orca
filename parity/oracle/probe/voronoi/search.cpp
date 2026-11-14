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
// The voronoi probe's repair search (common.hpp, search_shape; parity/sop1/voronoi.md, "Inputs"
// and VRGD): seeded shapes of the kinds that make Boost.Polygon's diagram degenerate, so that
// VoronoiDiagram::construct_voronoi's detection, rotations and failed repair are reached. Each draw
// has its own generator, seeded with kSearchSeed + its index, and makes one ExPolygon of one family:
// squares and stars with vertices a few nanometres off their edges (the missing-vertex polygons of
// test_voronoi.cpp hold such vertices on the edge), polygons with two vertices 1 to 3 nm apart,
// slivers, slits and notches a few nanometres wide, and holes a few nanometres from the contour.
// A draw whose edges intersect (EdgeGrid's intersecting_edges, as test_voronoi.cpp checks its own
// inputs) gives no shape. Written by orca-o3-probe-voronoi, 2026-11-13.
#include "common.hpp"

#include <algorithm>
#include <cmath>

#include "libslic3r/EdgeGrid.hpp"

namespace probe::vr {

namespace {

using namespace Slic3r;

using Ring = std::vector<Vec2d>;

// A side length or radius: 10^[4, 7] times [1, 5) nanometres, so from 10 µm to 50 mm.
double scale(Gen &g) { return std::pow(10., double(g.range(4, 7))) * g.real(1., 5.); }

// An angle: none half the time (edges along the axes), else any.
double turn(Gen &g) { return g.chance(2) ? 0. : g.real(0., 2. * PI); }

// The ring turned by `a` about the origin, then moved by (cx, cy).
Ring placed(const Ring &r, double a, double cx, double cy)
{
    const double c = std::cos(a), s = std::sin(a);
    Ring         out;
    for (const Vec2d &v : r)
        out.emplace_back(cx + v.x() * c - v.y() * s, cy + v.x() * s + v.y() * c);
    return out;
}

// A polygon of the ring's points rounded to the nanometre, consecutive equal points dropped;
// clockwise when `cw`.
Polygon rounded(const Ring &r, bool cw)
{
    Points pts;
    for (const Vec2d &v : r) {
        const Point q(coord_t(std::llround(v.x())), coord_t(std::llround(v.y())));
        if (pts.empty() || q != pts.back())
            pts.push_back(q);
    }
    while (pts.size() > 1 && pts.front() == pts.back())
        pts.pop_back();
    if (cw)
        std::reverse(pts.begin(), pts.end());
    return Polygon(pts);
}

// The square of side l centred on the origin, counter-clockwise.
Ring square(double l)
{
    const double h = l / 2.;
    return {{-h, -h}, {h, -h}, {h, h}, {-h, h}};
}

// A star of n points around the origin at angles 2 pi (i + [0, 0.8)) / n and radii [0.5, 1) r,
// counter-clockwise.
Ring star(Gen &g, double r, size_t n)
{
    Ring out;
    for (size_t i = 0; i < n; ++i) {
        const double a  = 2. * PI * (double(i) + g.real(0., 0.8)) / double(n);
        const double rr = g.real(0.5, 1.) * r;
        out.emplace_back(rr * std::cos(a), rr * std::sin(a));
    }
    return out;
}

// The ring with k vertices added off its edges: each on a random edge at a fraction [lo, hi] of it,
// moved by an integer [-5, 5] nm along the edge's normal (0 leaves it on the edge).
Ring with_offsets(Gen &g, const Ring &r, int k, double lo, double hi)
{
    struct Extra
    {
        size_t edge;
        double t;
        double d;
    };
    std::vector<Extra> extra;
    for (int i = 0; i < k; ++i)
        extra.push_back({size_t(g.below(r.size())), g.real(lo, hi), double(g.range(-5, 5))});
    std::sort(extra.begin(), extra.end(), [](const Extra &a, const Extra &b) { return a.edge != b.edge ? a.edge < b.edge : a.t < b.t; });
    Ring   out;
    size_t next = 0;
    for (size_t e = 0; e < r.size(); ++e) {
        out.push_back(r[e]);
        const Vec2d a = r[e], b = r[(e + 1) % r.size()];
        const Vec2d v = b - a;
        const Vec2d n = Vec2d(v.y(), -v.x()) / std::max(v.norm(), 1.);
        for (; next < extra.size() && extra[next].edge == e; ++next)
            out.push_back(a + v * extra[next].t + n * extra[next].d);
    }
    return out;
}

// The ring with its vertex i replaced by two 1 to 3 nm apart: cut back along both edges by [0.5, 3]
// nm, or kept with a second vertex [-3, 3] nm from it in x and y.
Ring with_close_pair(Gen &g, const Ring &r)
{
    const size_t i = size_t(g.below(r.size()));
    const Vec2d  v = r[i];
    const Vec2d  p = r[(i + r.size() - 1) % r.size()], q = r[(i + 1) % r.size()];
    Ring         out;
    for (size_t j = 0; j < r.size(); ++j) {
        if (j != i) {
            out.push_back(r[j]);
            continue;
        }
        if (g.chance(2)) {
            out.push_back(v + (p - v).normalized() * g.real(0.5, 3.));
            out.push_back(v + (q - v).normalized() * g.real(0.5, 3.));
        } else {
            int64_t dx = g.range(-3, 3), dy = g.range(-3, 3);
            if (dx == 0 && dy == 0)
                dx = 1;
            out.push_back(v);
            out.push_back(v + Vec2d(double(dx), double(dy)));
        }
    }
    return out;
}

} // namespace

SearchShape search_shape(uint32_t draw)
{
    Gen          g(kSearchSeed + draw);
    SearchShape  out{uint8_t(g.below(6)), {}};
    const double a  = turn(g);
    const double cx = double(g.range(-10000000, 10000000)), cy = double(g.range(-10000000, 10000000));
    const double l  = scale(g);
    ExPolygon    e;
    switch (out.family) {
    case 0: {
        // A square with one to three vertices off its edges, and a quarter of the time a square hole
        // with up to two.
        e.contour = rounded(placed(with_offsets(g, square(l), int(g.range(1, 3)), 0.02, 0.98), a, cx, cy), false);
        if (g.chance(4))
            e.holes.push_back(rounded(placed(with_offsets(g, square(l * g.real(0.3, 0.8)), int(g.range(0, 2)), 0.02, 0.98), a, cx, cy), true));
        break;
    }
    case 1: {
        // A star of 5 to 24 points with one or two vertices off its edges.
        const Ring s = star(g, l, size_t(g.range(5, 24)));
        e.contour    = rounded(placed(with_offsets(g, s, int(g.range(1, 2)), 0.05, 0.95), a, cx, cy), false);
        break;
    }
    case 2: {
        // A square or a star with two vertices 1 to 3 nm apart.
        const Ring base = g.chance(2) ? square(l) : star(g, l, size_t(g.range(5, 16)));
        e.contour       = rounded(placed(with_close_pair(g, base), a, cx, cy), false);
        break;
    }
    case 3: {
        // A sliver 1 to 20 nm wide: a triangle with its apex over the base, or a quadrilateral.
        const double w1 = double(g.range(1, 20)), w2 = double(g.range(1, 20));
        const Ring   r  = g.chance(2) ? Ring{{0., 0.}, {l, 0.}, {l * g.real(0.05, 0.95), w1}} : Ring{{0., 0.}, {l, 0.}, {l, w1}, {0., w2}};
        e.contour       = rounded(placed(r, a, cx, cy), false);
        break;
    }
    case 4: {
        // A square with a slit 1 to 10 nm wide, or a notch of that opening, cut from its bottom edge.
        const double w  = double(g.range(1, 10));
        const double x0 = l * g.real(0.2, 0.8) - l / 2., h = l * g.real(0.2, 0.8) - l / 2., b = -l / 2., t = l / 2.;
        Ring         r  = g.chance(2) ? Ring{{b, b}, {x0, b}, {x0, h}, {x0 + w, h}, {x0 + w, b}, {t, b}, {t, t}, {b, t}}
                                      : Ring{{b, b}, {x0, b}, {x0 + w / 2., h}, {x0 + w, b}, {t, b}, {t, t}, {b, t}};
        e.contour = rounded(placed(r, a, cx, cy), false);
        break;
    }
    default: {
        // A square with a triangular hole whose apex lies 1 to 5 nm inside the bottom edge.
        const double d = double(g.range(1, 5)), b = -l / 2.;
        const double x = l * g.real(-0.2, 0.2), hw = l * g.real(0.05, 0.25), hh = l * g.real(0.1, 0.4);
        e.contour = rounded(placed(square(l), a, cx, cy), false);
        e.holes.push_back(rounded(placed(Ring{{x, b + d}, {x + hw, b + d + hh}, {x - hw, b + d + hh}}, a, cx, cy), true));
        break;
    }
    }
    if (e.contour.size() < 3)
        return out;
    for (const Polygon &h : e.holes)
        if (h.size() < 3)
            return out;
    if (!intersecting_edges(to_polygons(e)).empty())
        return out;
    out.ex.push_back(std::move(e));
    return out;
}

} // namespace probe::vr
