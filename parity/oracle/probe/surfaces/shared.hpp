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
// What the surfaces probe's sources share (parity/sop1/surfaces.md, added 2026-11-13 by
// orca-o3-probe-surfaces): the writers of SOP1.md's compound values (`point`, `polygon`,
// `expolygon`, `surface`) and of the probe's own (a wave seed, a region expansion, a set of
// expansion parameters), the choice of the committed cases, the seeded shapes every record kind
// draws from, and the layers of OrcaSlicer's test meshes (<exe>/../src/tests/data), sliced and
// typed into fill surfaces the way a print's layers look before process_external_surfaces.
#pragma once

#include "../probe.hpp"

#include <cmath>
#include <cstdint>
#include <string>
#include <vector>

#include "libslic3r/Algorithm/RegionExpansion.hpp"
#include "libslic3r/ExPolygon.hpp"
#include "libslic3r/Point.hpp"
#include "libslic3r/Polygon.hpp"
#include "libslic3r/Surface.hpp"

namespace probe {
namespace surfaces {

using Slic3r::ExPolygon;
using Slic3r::ExPolygons;
using Slic3r::Point;
using Slic3r::Points;
using Slic3r::Polygon;
using Slic3r::Polygons;
using Slic3r::Surface;
using Slic3r::Surfaces;

// ---------------------------------------------------------------- values (SOP1.md, "Values")

inline void put(Payload &p, const Point &q) { p.i64(q.x()).i64(q.y()); }
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
    for (const Polygon &poly : polys)
        put(p, poly);
}
inline void put(Payload &p, const ExPolygon &ex)
{
    put(p, ex.contour);
    put(p, ex.holes);
}
inline void put(Payload &p, const ExPolygons &exs)
{
    p.u64(exs.size());
    for (const ExPolygon &ex : exs)
        put(p, ex);
}
// SOP1.md's `surface`: u32 type, expolygon, f64 thickness, u16 thickness layers, f64 bridge angle,
// u16 extra perimeters.
inline void put(Payload &p, const Surface &s)
{
    p.u32(uint32_t(s.surface_type));
    put(p, s.expolygon);
    p.f64(s.thickness).u16(s.thickness_layers).f64(s.bridge_angle).u16(s.extra_perimeters);
}
inline void put(Payload &p, const Surfaces &ss)
{
    p.u64(ss.size());
    for (const Surface &s : ss)
        put(p, s);
}
// A set of expansion parameters, each field in its own width, in the struct's order.
inline void put(Payload &p, const Slic3r::Algorithm::RegionExpansionParameters &r)
{
    p.f32(r.tiny_expansion).f32(r.initial_step).f32(r.other_step).u64(r.num_other_steps).f32(r.max_inflation).f64(r.arc_tolerance).f64(r.shortest_edge_length);
}
// A wave seed: u32 src, u32 boundary, the path's points.
inline void put(Payload &p, const Slic3r::Algorithm::WaveSeed &s)
{
    p.u32(s.src).u32(s.boundary);
    put(p, s.path);
}
inline void put(Payload &p, const Slic3r::Algorithm::WaveSeeds &ss)
{
    p.u64(ss.size());
    for (const auto &s : ss)
        put(p, s);
}
// A region expansion: the polygon, u32 src id, u32 boundary id.
inline void put(Payload &p, const std::vector<Slic3r::Algorithm::RegionExpansion> &v)
{
    p.u64(v.size());
    for (const auto &r : v) {
        put(p, r.polygon);
        p.u32(r.src_id).u32(r.boundary_id);
    }
}
// The same with an expolygon.
inline void put(Payload &p, const std::vector<Slic3r::Algorithm::RegionExpansionEx> &v)
{
    p.u64(v.size());
    for (const auto &r : v) {
        put(p, r.expolygon);
        p.u32(r.src_id).u32(r.boundary_id);
    }
}

inline bool same(const Polygon &a, const Polygon &b) { return a.points == b.points; }
inline bool same(const Polygons &a, const Polygons &b)
{
    if (a.size() != b.size())
        return false;
    for (size_t i = 0; i < a.size(); ++i)
        if (!same(a[i], b[i]))
            return false;
    return true;
}
inline bool same(const ExPolygon &a, const ExPolygon &b) { return same(a.contour, b.contour) && same(a.holes, b.holes); }
inline bool same(const ExPolygons &a, const ExPolygons &b)
{
    if (a.size() != b.size())
        return false;
    for (size_t i = 0; i < a.size(); ++i)
        if (!same(a[i], b[i]))
            return false;
    return true;
}
inline bool same(const Surface &a, const Surface &b)
{
    return a.surface_type == b.surface_type && same(a.expolygon, b.expolygon) && bits(a.thickness) == bits(b.thickness) && a.thickness_layers == b.thickness_layers &&
           bits(a.bridge_angle) == bits(b.bridge_angle) && a.extra_perimeters == b.extra_perimeters;
}
inline bool same(const Surfaces &a, const Surfaces &b)
{
    if (a.size() != b.size())
        return false;
    for (size_t i = 0; i < a.size(); ++i)
        if (!same(a[i], b[i]))
            return false;
    return true;
}

// ---------------------------------------------------------------- the committed set

// Which cases of one record kind the committed set takes: the first `count` whose record is at
// most `bytes` long, in the order they are recorded (the hand-made cases first).
struct Picker
{
    size_t count;
    size_t bytes;
    size_t taken = 0;
    bool   take(size_t size)
    {
        if (taken < count && size <= bytes) {
            ++taken;
            return true;
        }
        return false;
    }
};

// Adds one case to the full set, and to the committed set when the picker takes it.
inline void add(Sets &sets, const char (&kind)[5], const Payload &p, Picker &pick)
{
    sets.full.add(kind, p);
    if (pick.take(p.bytes.size()))
        sets.committed.add(kind, p);
}

// ---------------------------------------------------------------- shapes

// Millimetres to scaled coordinates (SCALING_FACTOR is 1e-6 in the command line engine).
inline int64_t mm(double v) { return int64_t(std::llround(v * 1e6)); }

Polygon rect(double x0, double y0, double x1, double y1);
// A star-shaped polygon of n vertices around (cx, cy), radii between r / 4 and r (mm), CCW.
Polygon star(SplitMix &g, int n, double cx, double cy, double r);
// n points anywhere in the square of half-side r around (cx, cy), in scaled units.
Points scatter(SplitMix &g, int n, int64_t cx, int64_t cy, int64_t r);
Polygon reversed(Polygon p);
// A uniform double in [lo, hi).
double uniform(SplitMix &g, double lo, double hi);
// A random region: the union of 1 to `n` rectangles and stars in the square [0, size] mm.
ExPolygons region(SplitMix &g, int n, double size);

// One layer of a test mesh as process_external_surfaces meets it: its fill surfaces, typed
// (stTop, stBottomBridge or stBottom, stInternalSolid, stInternal), and the flows it expands by.
struct Layer
{
    std::string name;     // "<mesh>@<z>"
    Surfaces    surfaces; // in the order tops, bottoms, internal solid, internal
    ExPolygons  slices;   // the layer's slice
    ExPolygons  lower;    // the slice below
};

// The layers of OrcaSlicer's test meshes (<exe>/../src/tests/data/<name>.obj) that have a bottom
// (ground not over the layer below) or a top (ground not under the layer above), at most
// `per_mesh` of each, spread over its height, and of the large meshes only when `large`. False,
// with `error`, when a mesh cannot be read.
bool mesh_layers(std::vector<Layer> &out, bool large, size_t per_mesh, std::string &error);

} // namespace surfaces
} // namespace probe
