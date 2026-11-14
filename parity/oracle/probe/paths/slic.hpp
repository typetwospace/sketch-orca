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
// The paths probe's writers of libslic3r values and its shape generators (parity/sop1/paths.md,
// "Values"), shared by geom1.cpp, geom2.cpp and chain.cpp. A point is i64 x and y, a Point3 i64
// x, y and z, a list a u32 count and its items; an ArcSegment and a list of PathFittingData are
// written as the page's "Values" says. Added 2026-11-13 by orca-o3-probe-paths.
#pragma once

#include "common.hpp"

#include <algorithm>
#include <cmath>
#include <vector>

#include "libslic3r/ArcFitter.hpp"
#include "libslic3r/Circle.hpp"
#include "libslic3r/ExPolygon.hpp"
#include "libslic3r/Line.hpp"
#include "libslic3r/Point.hpp"
#include "libslic3r/Polygon.hpp"
#include "libslic3r/Polyline.hpp"

namespace probe {
namespace paths {

using namespace Slic3r;

inline void put(Payload &p, const Point &q) { p.i64(q.x()).i64(q.y()); }
inline void put(Payload &p, const Point3 &q) { p.i64(q.x()).i64(q.y()).i64(q.z()); }
inline void put(Payload &p, const Vec3crd &q) { p.i64(q.x()).i64(q.y()).i64(q.z()); }
inline void put(Payload &p, const Vec2d &v) { p.f64(v.x()).f64(v.y()); }
inline void put(Payload &p, const Vec3d &v) { p.f64(v.x()).f64(v.y()).f64(v.z()); }
inline void put(Payload &p, const Vec3f &v) { p.f32(v.x()).f32(v.y()).f32(v.z()); }
inline void put(Payload &p, const Points &pts)
{
    p.u32(uint32_t(pts.size()));
    for (const Point &q : pts)
        put(p, q);
}
inline void put(Payload &p, const Points3 &pts)
{
    p.u32(uint32_t(pts.size()));
    for (const Point3 &q : pts)
        put(p, q);
}
inline void put(Payload &p, const Polygons &polys)
{
    p.u32(uint32_t(polys.size()));
    for (const Polygon &q : polys)
        put(p, q.points);
}
inline void put(Payload &p, const Polylines &pls)
{
    p.u32(uint32_t(pls.size()));
    for (const Polyline &q : pls)
        put(p, q.points);
}
inline void put(Payload &p, const ExPolygons &es)
{
    p.u32(uint32_t(es.size()));
    for (const ExPolygon &e : es) {
        put(p, e.contour.points);
        put(p, e.holes);
    }
}
inline void put(Payload &p, const Circle &c)
{
    put(p, c.center);
    p.f64(c.radius);
}
// An ArcSegment: u8 is_arc, center, f64 radius, start_point, end_point, u8 direction, f64 length,
// angle_radians, polar_start_theta and polar_end_theta.
inline void put(Payload &p, const ArcSegment &a)
{
    p.boolean(a.is_arc);
    put(p, a.center);
    p.f64(a.radius);
    put(p, a.start_point);
    put(p, a.end_point);
    p.u8(uint8_t(a.direction));
    p.f64(a.length).f64(a.angle_radians).f64(a.polar_start_theta).f64(a.polar_end_theta);
}
// A fitting result: u32 count, each u64 start_point_index, u64 end_point_index, u8 path_type and
// the ArcSegment.
inline void put(Payload &p, const std::vector<PathFittingData> &r)
{
    p.u32(uint32_t(r.size()));
    for (const PathFittingData &d : r) {
        p.u64(d.start_point_index).u64(d.end_point_index).u8(uint8_t(d.path_type));
        put(p, d.arc_data);
    }
}
inline void put(Payload &p, const Polyline &pl)
{
    put(p, pl.points);
    put(p, pl.fitting_result);
}
inline void put(Payload &p, const Polyline3 &pl)
{
    put(p, pl.points);
    put(p, pl.fitting_result);
}

// A coordinate range of domain A's scaled coordinates (1e-6 mm): 50 nm to a 600 mm bed, with the
// odd one past it.
inline int64_t range(SplitMix &g)
{
    switch (g.below(8)) {
    case 0: return 1000;
    case 1: return 100000;
    case 2:
    case 3: return 1000000;
    case 4:
    case 5: return 30000000;
    case 6: return 300000000;
    default: return 3000000000LL;
    }
}

inline Point rpoint(SplitMix &g, int64_t r) { return Point(rnd(g, -r, r), rnd(g, -r, r)); }

// n points on an arc around c of radius r from angle a0 sweeping `sweep` radians, each moved by up
// to `noise` along x and y.
inline Points arc_points(SplitMix &g, const Point &c, double r, double a0, double sweep, size_t n, double noise)
{
    Points out;
    for (size_t i = 0; i < n; ++i) {
        const double a = a0 + sweep * double(i) / double(n > 1 ? n - 1 : 1);
        const double dx = noise > 0. ? real(g, -noise, noise) : 0.;
        const double dy = noise > 0. ? real(g, -noise, noise) : 0.;
        out.emplace_back(int64_t(std::llround(double(c.x()) + r * std::cos(a) + dx)), int64_t(std::llround(double(c.y()) + r * std::sin(a) + dy)));
    }
    return out;
}

// A star-shaped polygon of n points around c within r, counter-clockwise (clockwise when cw).
inline Points star(SplitMix &g, const Point &c, int64_t r, size_t n, bool cw)
{
    Points pts;
    for (size_t i = 0; i < n; ++i) {
        const double a  = 2. * PI * (double(i) + real(g, 0., 0.8)) / double(n);
        const double rr = real(g, 0.2, 1.0) * double(r);
        pts.emplace_back(c.x() + int64_t(std::llround(rr * std::cos(a))), c.y() + int64_t(std::llround(rr * std::sin(a))));
    }
    if (cw)
        std::reverse(pts.begin(), pts.end());
    return pts;
}

inline Points rect(int64_t x0, int64_t y0, int64_t x1, int64_t y1) { return Points{Point(x0, y0), Point(x1, y0), Point(x1, y1), Point(x0, y1)}; }

// A path for the arc fitter: straight runs, arcs (clean or noisy, either way round) and zigzags,
// joined end to end, as a perimeter or a wall's polyline may hold them.
inline Points fit_path(SplitMix &g)
{
    Points       out;
    const double scale = std::pow(10., real(g, 4., 7.5));
    Point        cur(rnd(g, -100000000, 100000000), rnd(g, -100000000, 100000000));
    double       heading = real(g, 0., 2. * PI);
    out.push_back(cur);
    const int pieces = int(rnd(g, 1, 4));
    for (int k = 0; k < pieces; ++k) {
        switch (g.below(4)) {
        case 0: { // a straight run of a few collinear points
            const int    n    = int(rnd(g, 1, 5));
            const double step = scale * real(g, 0.05, 0.5);
            for (int i = 0; i < n; ++i) {
                cur = Point(cur.x() + int64_t(std::llround(step * std::cos(heading))), cur.y() + int64_t(std::llround(step * std::sin(heading))));
                out.push_back(cur);
            }
            break;
        }
        case 1:
        case 2: { // an arc tangent to the heading
            const double r     = scale * real(g, 0.2, 3.);
            const double turn  = (g.below(2) ? 1. : -1.) * real(g, 0.2, 5.5);
            const int    n     = int(rnd(g, 3, 24));
            const double side  = turn > 0 ? PI / 2. : -PI / 2.;
            const double cx    = double(cur.x()) + r * std::cos(heading + side);
            const double cy    = double(cur.y()) + r * std::sin(heading + side);
            const double a0    = std::atan2(double(cur.y()) - cy, double(cur.x()) - cx);
            const double noise = one_in(g, 3) ? r * real(g, 0., 0.02) : 0.;
            for (int i = 1; i <= n; ++i) {
                const double a = a0 + turn * double(i) / double(n);
                cur            = Point(int64_t(std::llround(cx + r * std::cos(a) + (noise > 0. ? real(g, -noise, noise) : 0.))),
                                       int64_t(std::llround(cy + r * std::sin(a) + (noise > 0. ? real(g, -noise, noise) : 0.))));
                out.push_back(cur);
            }
            heading += turn;
            break;
        }
        default: { // a zigzag
            const int    n    = int(rnd(g, 2, 6));
            const double step = scale * real(g, 0.05, 0.5);
            for (int i = 0; i < n; ++i) {
                const double h = heading + ((i & 1) ? 0.6 : -0.6);
                cur            = Point(cur.x() + int64_t(std::llround(step * std::cos(h))), cur.y() + int64_t(std::llround(step * std::sin(h))));
                out.push_back(cur);
            }
            break;
        }
        }
    }
    return out;
}

inline Points3 lift(const Points &pts, SplitMix &g, bool flat)
{
    Points3 out;
    for (const Point &q : pts)
        out.emplace_back(q.x(), q.y(), flat ? int64_t(0) : rnd(g, -2000000, 2000000));
    return out;
}

} // namespace paths
} // namespace probe
