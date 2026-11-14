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
// What the layer probe's sources share (parity/sop1/layer.md, "Values"): the writer of one record
// into both sets, the compound values (point, polygon, expolygon, surface, polyline, entity, box,
// flow) in parity/SOP1.md's encodings, the attempt() wrapper that writes a call's result or the
// text of the exception it threw, and the seeded draws. Recorded by orca-o3-probe-layer,
// 2026-11-13.
#pragma once

#include "../probe.hpp"

#include <cmath>
#include <cstdint>
#include <exception>
#include <map>
#include <string>
#include <typeinfo>
#include <vector>

#include "libslic3r/BoundingBox.hpp"
#include "libslic3r/ExPolygon.hpp"
#include "libslic3r/ExtrusionEntity.hpp"
#include "libslic3r/ExtrusionEntityCollection.hpp"
#include "libslic3r/Flow.hpp"
#include "libslic3r/Point.hpp"
#include "libslic3r/Polygon.hpp"
#include "libslic3r/Polyline.hpp"
#include "libslic3r/Surface.hpp"

namespace probe {
namespace layer {

using namespace Slic3r;

// One record into the full set, and into the committed set when `committed`.
inline void add(Sets &sets, const char (&kind)[5], const Payload &p, bool committed)
{
    sets.full.add(kind, p);
    if (committed)
        sets.committed.add(kind, p);
}

// One record into the full set, and into the committed set while the kind's committed records,
// headers included, stay within `budget` bytes: the committed set takes each record that still
// fits, in order, and skips one that does not. Keeps the committed file within its 512 KB.
inline void add_within(Sets &sets, const char (&kind)[5], const Payload &p, size_t budget)
{
    static std::map<std::string, size_t> used;
    size_t &u    = used[kind];
    const size_t size = p.bytes.size() + 12;
    const bool   fits = u + size <= budget;
    if (fits)
        u += size;
    add(sets, kind, p, fits);
}

// ---- Compound values (parity/SOP1.md, "Values") --------------------------------------------

inline void put(Payload &p, const Point &q) { p.i64(int64_t(q.x())).i64(int64_t(q.y())); }
inline void put(Payload &p, const Points &pts)
{
    p.u64(pts.size());
    for (const Point &q : pts)
        put(p, q);
}
inline void put(Payload &p, const Polygon &poly) { put(p, poly.points); }
inline void put(Payload &p, const Polygons &polys)
{
    p.u64(polys.size());
    for (const Polygon &q : polys)
        put(p, q.points);
}
inline void put(Payload &p, const ExPolygon &e)
{
    put(p, e.contour.points);
    put(p, e.holes);
}
inline void put(Payload &p, const ExPolygons &es)
{
    p.u64(es.size());
    for (const ExPolygon &e : es)
        put(p, e);
}
inline void put(Payload &p, const std::vector<PathFittingData> &fitting)
{
    p.u64(fitting.size());
    for (const PathFittingData &f : fitting) {
        p.u64(uint64_t(f.start_point_index));
        p.u64(uint64_t(f.end_point_index));
        p.u8(uint8_t(f.path_type));
        const ArcSegment &arc = f.arc_data;
        p.boolean(arc.is_arc);
        put(p, arc.center);
        p.f64(arc.radius);
        p.f64(arc.length);
        p.f64(arc.angle_radians);
        p.f64(arc.polar_start_theta);
        p.f64(arc.polar_end_theta);
        put(p, arc.start_point);
        put(p, arc.end_point);
        p.u8(uint8_t(arc.direction));
    }
}
inline void put(Payload &p, const Polyline &pl)
{
    put(p, pl.points);
    put(p, pl.fitting_result);
}
inline void put(Payload &p, const Polylines &pls)
{
    p.u64(pls.size());
    for (const Polyline &pl : pls)
        put(p, pl);
}
inline void put(Payload &p, const Surface &s)
{
    p.u32(uint32_t(s.surface_type));
    put(p, s.expolygon);
    p.f64(s.thickness);
    p.u16(uint16_t(s.thickness_layers));
    p.f64(s.bridge_angle);
    p.u16(uint16_t(s.extra_perimeters));
}
inline void put(Payload &p, const Surfaces &ss)
{
    p.u64(ss.size());
    for (const Surface &s : ss)
        put(p, s);
}
inline void put(Payload &p, const BoundingBox &b)
{
    put(p, b.min);
    put(p, b.max);
    p.boolean(b.defined);
}
inline void put(Payload &p, const Flow &f)
{
    p.f32(f.width()).f32(f.height()).f32(f.spacing()).f32(f.nozzle_diameter()).boolean(f.bridge());
}
inline void put_strs(Payload &p, const std::vector<std::string> &v)
{
    p.u64(v.size());
    for (const std::string &s : v)
        p.str(s);
}

// An extrusion entity, as parity/SOP1.md's `entity`: its kind by its exact type, then its fields.
inline void put_entity(Payload &p, const ExtrusionEntity &e);

inline void put_path_fields(Payload &p, const ExtrusionPath &path)
{
    p.u8(uint8_t(path.role()));
    p.i32(int32_t(path.inset_idx));
    p.u64(path.polyline.points.size());
    for (const Point3 &q : path.polyline.points)
        p.i64(int64_t(q.x())).i64(int64_t(q.y())).i64(int64_t(q.z()));
    put(p, path.polyline.fitting_result);
    p.f64(path.overhang_degree);
    p.i32(int32_t(path.curve_degree));
    p.f64(path.mm3_per_mm);
    p.f32(path.width);
    p.f32(path.height);
    p.f64(path.smooth_speed);
    p.boolean(path.z_contoured);
    p.boolean(path.is_force_no_extrusion());
    p.boolean(path.can_reverse());
}

inline void put_loop_fields(Payload &p, const ExtrusionLoop &loop)
{
    p.i32(int32_t(loop.inset_idx));
    p.u8(uint8_t(loop.loop_role()));
    p.u64(loop.paths.size());
    for (const ExtrusionPath &path : loop.paths)
        put_entity(p, path);
}

inline void put_entity(Payload &p, const ExtrusionEntity &e)
{
    const std::type_info &t = typeid(e);
    if (t == typeid(ExtrusionPath)) {
        p.u8(1);
        put_path_fields(p, static_cast<const ExtrusionPath &>(e));
    } else if (t == typeid(ExtrusionPathContoured)) {
        const auto &path = static_cast<const ExtrusionPathContoured &>(e);
        p.u8(2);
        put_path_fields(p, path);
        p.u64(path.z_diffs.size());
        for (double z : path.z_diffs)
            p.f64(z);
    } else if (t == typeid(ExtrusionPathSloped)) {
        const auto &path = static_cast<const ExtrusionPathSloped &>(e);
        p.u8(3);
        put_path_fields(p, path);
        p.f64(path.slope_begin.z_ratio).f64(path.slope_begin.e_ratio).f64(path.slope_end.z_ratio).f64(path.slope_end.e_ratio);
    } else if (t == typeid(ExtrusionPathOriented)) {
        p.u8(4);
        put_path_fields(p, static_cast<const ExtrusionPath &>(e));
    } else if (t == typeid(ExtrusionMultiPath)) {
        const auto &multi = static_cast<const ExtrusionMultiPath &>(e);
        p.u8(5);
        p.i32(int32_t(multi.inset_idx));
        p.boolean(multi.can_reverse());
        p.u64(multi.paths.size());
        for (const ExtrusionPath &path : multi.paths)
            put_entity(p, path);
    } else if (t == typeid(ExtrusionLoop)) {
        p.u8(6);
        put_loop_fields(p, static_cast<const ExtrusionLoop &>(e));
    } else if (t == typeid(ExtrusionLoopSloped)) {
        const auto &loop = static_cast<const ExtrusionLoopSloped &>(e);
        p.u8(7);
        put_loop_fields(p, loop);
        p.u64(loop.starts.size());
        for (const ExtrusionPathSloped &path : loop.starts)
            put_entity(p, path);
        p.u64(loop.ends.size());
        for (const ExtrusionPathSloped &path : loop.ends)
            put_entity(p, path);
    } else if (t == typeid(ExtrusionEntityCollection)) {
        const auto &c = static_cast<const ExtrusionEntityCollection &>(e);
        p.u8(8);
        p.i32(int32_t(c.inset_idx));
        p.boolean(c.no_sort);
        p.boolean(c.can_reverse());
        p.u64(c.entities.size());
        for (const ExtrusionEntity *child : c.entities)
            put_entity(p, *child);
    } else {
        p.u8(0);
        p.str(t.name());
    }
}

inline void put(Payload &p, const ExtrusionEntitiesPtr &v)
{
    p.u64(v.size());
    for (const ExtrusionEntity *e : v)
        put_entity(p, *e);
}
inline void put(Payload &p, const ExtrusionPaths &v)
{
    p.u64(v.size());
    for (const ExtrusionPath &e : v)
        put_entity(p, e);
}

// Writes `u8` 1 and what `f` writes, or `u8` 0 and the text of the exception `f` threw (what it
// wrote before the throw is dropped).
template<class F> void attempt(Payload &p, F f)
{
    Payload inner;
    try {
        f(inner);
    } catch (const std::exception &e) {
        p.u8(0);
        p.str(e.what());
        return;
    }
    p.u8(1);
    p.bytes += inner.bytes;
}

// ---- Seeded draws ---------------------------------------------------------------------------

inline double unit(SplitMix &g) { return double(g.next() >> 11) * (1.0 / 9007199254740992.0); }
inline double real(SplitMix &g, double lo, double hi) { return lo + (hi - lo) * unit(g); }
inline int64_t rnd(SplitMix &g, int64_t lo, int64_t hi) { return lo + int64_t(g.below(uint64_t(hi - lo + 1))); }

// A star-shaped polygon of n points around c, counter-clockwise, radii in [r/4, r].
inline Polygon star(SplitMix &g, const Point &c, int64_t r, size_t n)
{
    Polygon poly;
    for (size_t i = 0; i < n; ++i) {
        const double a  = 2. * PI * (double(i) + real(g, 0., 0.7)) / double(n);
        const double rr = real(g, 0.25, 1.0) * double(r);
        poly.points.emplace_back(c.x() + int64_t(std::llround(rr * std::cos(a))), c.y() + int64_t(std::llround(rr * std::sin(a))));
    }
    return poly;
}

// An axis-aligned square of side 2r around c, counter-clockwise.
inline Polygon square(const Point &c, int64_t r)
{
    return Polygon({Point(c.x() - r, c.y() - r), Point(c.x() + r, c.y() - r), Point(c.x() + r, c.y() + r), Point(c.x() - r, c.y() + r)});
}

// An ExPolygon: a star or a square, now and then with a square hole (clockwise) inside.
inline ExPolygon island(SplitMix &g, const Point &c, int64_t r)
{
    ExPolygon e;
    e.contour = g.below(3) == 0 ? square(c, r) : star(g, c, r, size_t(rnd(g, 3, 16)));
    if (g.below(3) == 0) {
        Polygon hole = square(c, r / 8);
        hole.reverse();
        e.holes.push_back(hole);
    }
    return e;
}

} // namespace layer
} // namespace probe
