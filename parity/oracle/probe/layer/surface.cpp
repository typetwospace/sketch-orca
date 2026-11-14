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
// layer.sop1's surface records of the piece layer-2 (orca-layer; parity/sop1/layer.md): LYSU a
// list of surfaces with what Surface.hpp and Surface.cpp compute of them, LYSC SurfaceCollection's
// operations. Recorded by orca-o3-probe-layer, 2026-11-13.
#include "common.hpp"

#include "libslic3r/SurfaceCollection.hpp"

namespace probe {
namespace layer {

namespace {

// A list of surfaces: islands of random types, thicknesses, bridge angles and extra perimeters;
// some share their properties, so that group() and surfaces_could_merge meet equal ones.
Surfaces some_surfaces(SplitMix &g, size_t n)
{
    Surfaces out;
    for (size_t i = 0; i < n; ++i) {
        Surface s(SurfaceType(g.below(size_t(stCount))), island(g, Point(rnd(g, -20000000, 20000000), rnd(g, -20000000, 20000000)), rnd(g, 200000, 6000000)));
        if (g.below(2) == 0) {
            s.thickness        = g.below(2) == 0 ? 0.2 : real(g, 0.05, 2.0);
            s.thickness_layers = (unsigned short) rnd(g, 1, 4);
        }
        if (g.below(3) == 0)
            s.bridge_angle = g.below(2) == 0 ? 0. : real(g, 0, 3.2);
        if (g.below(5) == 0)
            s.extra_perimeters = (unsigned short) rnd(g, 0, 3);
        if (!out.empty() && g.below(4) == 0) {
            const Surface &o = out[size_t(g.below(out.size()))];
            s.surface_type     = o.surface_type;
            s.thickness        = o.thickness;
            s.thickness_layers = o.thickness_layers;
            s.bridge_angle     = o.bridge_angle;
        }
        out.push_back(s);
    }
    return out;
}

uint16_t predicates(const Surface &s)
{
    uint16_t b = 0;
    b |= s.empty() ? 1 : 0;
    b |= s.is_top() ? 2 : 0;
    b |= s.is_bottom() ? 4 : 0;
    b |= s.is_bridge() ? 8 : 0;
    b |= s.is_internal_bridge() ? 16 : 0;
    b |= s.is_external() ? 32 : 0;
    b |= s.is_internal() ? 64 : 0;
    b |= s.is_solid() ? 128 : 0;
    b |= s.is_solid_infill() ? 256 : 0;
    return b;
}

void put_ptrs(Payload &p, const Surfaces &all, const SurfacesPtr &v)
{
    p.u64(v.size());
    for (const Surface *s : v)
        p.u64(uint64_t(s - all.data()));
}

// LYSU: each surface's predicates, area, extents, polygons and colour name; then of the list its
// extents, polygons, ExPolygons, the number of polygons and which pairs could merge.
void record_surfaces(Sets &sets)
{
    SplitMix g(0x1A7E5F00ull);
    for (int i = 0; i < 300; ++i) {
        Surfaces ss = some_surfaces(g, size_t(rnd(g, i == 0 ? 0 : 1, 6)));
        if (i == 1)
            ss.push_back(Surface(stInternalVoid));
        Payload p;
        put(p, ss);
        for (const Surface &s : ss) {
            p.u16(predicates(s)).f64(s.area());
            put(p, get_extents(s));
            put(p, to_polygons(s));
            p.str(surface_type_to_color_name(s.surface_type));
        }
        put(p, get_extents(ss));
        put(p, to_polygons(ss));
        put(p, to_expolygons(ss));
        p.u64(number_polygons(ss));
        for (size_t a = 0; a < ss.size(); ++a)
            for (size_t b = 0; b < ss.size(); ++b)
                p.boolean(surfaces_could_merge(ss[a], ss[b]));
        add_within(sets, "LYSU", p, 30000);
    }
    // The colour names of the types past the list (SurfaceType(-1) and stCount).
    Payload p;
    p.str(surface_type_to_color_name(SurfaceType(-1))).str(surface_type_to_color_name(stCount)).str(surface_type_to_color_name(stInternalAfterExternalBridge));
    add(sets, "LYSU", p, true);
}

// LYSC: SurfaceCollection's operations on a list of surfaces, each from the list as given.
void record_collection(Sets &sets)
{
    SplitMix g(0x1A7E5F01ull);
    for (int i = 0; i < 300; ++i) {
        const Surfaces    ss  = some_surfaces(g, size_t(rnd(g, 0, 8)));
        const SurfaceType t   = SurfaceType(g.below(size_t(stCount)));
        const SurfaceType t2  = SurfaceType(g.below(size_t(stCount)));
        const double      tol = real(g, 1000, 500000);
        const Point       q(rnd(g, -20000000, 20000000), rnd(g, -20000000, 20000000));
        const Point       q2  = ss.empty() ? q : ss[size_t(g.below(ss.size()))].expolygon.contour.points.front();
        Payload           p;
        put(p, ss);
        p.u32(uint32_t(t)).u32(uint32_t(t2)).f64(tol);
        put(p, q);
        put(p, q2);
        {
            SurfaceCollection c(ss);
            c.simplify(tol);
            put(p, c.surfaces);
        }
        {
            SurfaceCollection        c(ss);
            std::vector<SurfacesPtr> groups;
            c.group(&groups);
            p.u64(groups.size());
            for (const SurfacesPtr &gr : groups)
                put_ptrs(p, c.surfaces, gr);
        }
        {
            const SurfaceCollection c(ss);
            put_ptrs(p, c.surfaces, c.filter_by_type(t));
            put_ptrs(p, c.surfaces, c.filter_by_types({t, t2}));
            Polygons polys;
            c.filter_by_type(t, &polys);
            put(p, polys);
            uint16_t has = 0;
            for (size_t k = 0; k < size_t(stCount); ++k)
                has |= c.has(SurfaceType(k)) ? uint16_t(1u << k) : 0;
            p.u16(has);
            p.boolean(c.any_internal_contains(q)).boolean(c.any_internal_contains(q2));
            p.boolean(c.any_bottom_contains(q)).boolean(c.any_bottom_contains(q2));
        }
        {
            SurfaceCollection c(ss);
            c.keep_type(t);
            put(p, c.surfaces);
        }
        {
            SurfaceCollection c(ss);
            c.keep_types({t, t2});
            put(p, c.surfaces);
        }
        {
            SurfaceCollection c(ss);
            c.remove_type(t);
            put(p, c.surfaces);
        }
        {
            SurfaceCollection c(ss);
            ExPolygons        removed;
            c.remove_type(t, &removed);
            put(p, c.surfaces);
            put(p, removed);
        }
        {
            SurfaceCollection c(ss);
            c.remove_types({t, t2});
            put(p, c.surfaces);
        }
        {
            SurfaceCollection c(ss);
            c.change_to_new_type(t, t2);
            put(p, c.surfaces);
            c.set_type(t);
            put(p, c.surfaces);
        }
        add_within(sets, "LYSC", p, 30000);
    }
}

} // namespace

void record_surface(Sets &sets)
{
    record_surfaces(sets);
    record_collection(sets);
}

} // namespace layer
} // namespace probe
