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
// layer.sop1's extrusion records (parity/sop1/layer.md): LYRO the roles, LYEN a path, LYEM a multi
// path, LYEL a loop, LYES a sloped loop, LYEC a collection, LYEA the append helpers and LYVW the
// variable width (layer-1), and LYCH the chaining of entities and paths (layer-3). Each record
// holds its inputs, then the engine's results. Recorded by orca-o3-probe-layer, 2026-11-13.
#include "common.hpp"

#include <algorithm>
#include <iterator>
#include <limits>
#include <memory>

#include "libslic3r/ShortestPath.hpp"
#include "libslic3r/VariableWidth.hpp"

namespace probe {
namespace layer {

namespace {

// ---- Inputs ---------------------------------------------------------------------------------

// A random walk of n points from a point within 5 mm of the origin, now and then repeating a point.
Points walk(SplitMix &g, size_t n, int64_t step)
{
    Points pts;
    Point  c(rnd(g, -5000000, 5000000), rnd(g, -5000000, 5000000));
    pts.push_back(c);
    for (size_t i = 1; i < n; ++i) {
        if (g.below(15) != 0)
            c = Point(c.x() + rnd(g, -step, step), c.y() + rnd(g, -step, step));
        pts.push_back(c);
    }
    return pts;
}

ExtrusionRole any_role(SplitMix &g) { return ExtrusionRole(rnd(g, 1, int64_t(erMixed) - 1)); }
ExtrusionRole wall_role(SplitMix &g)
{
    static const ExtrusionRole r[] = {erPerimeter, erExternalPerimeter, erOverhangPerimeter};
    return r[g.below(3)];
}

// A path along pts with random fields; z is 0 unless `z`.
ExtrusionPath make_path(SplitMix &g, const Points &pts, ExtrusionRole role, bool z = false)
{
    ExtrusionPath path(role, real(g, 0.01, 0.3), float(real(g, 0.1, 0.8)), float(real(g, 0.05, 0.4)), g.below(10) == 0);
    for (const Point &q : pts)
        path.polyline.points.emplace_back(int64_t(q.x()), int64_t(q.y()), z ? rnd(g, -2000, 2000) : int64_t(0));
    path.overhang_degree = g.below(4) == 0 ? real(g, 0, 5) : 0.;
    path.curve_degree    = int(g.below(3));
    path.smooth_speed    = g.below(4) == 0 ? real(g, 10, 200) : 0.;
    path.z_contoured     = g.below(8) == 0;
    path.inset_idx       = int(rnd(g, -1, 3));
    if (g.below(6) == 0)
        path.set_reverse();
    return path;
}

// Paths along a walk that share their end points.
ExtrusionPaths joined_paths(SplitMix &g, size_t count, ExtrusionRole role0, bool vary_roles)
{
    const Points  pts = walk(g, count * 3 + 2, 2000000);
    ExtrusionPaths out;
    size_t         at = 0;
    for (size_t i = 0; i < count; ++i) {
        const size_t next = i + 1 == count ? pts.size() - 1 : std::min(pts.size() - 1, at + 1 + size_t(g.below(4)));
        Points       seg(pts.begin() + at, pts.begin() + next + 1);
        if (seg.size() < 2)
            seg.push_back(seg.back());
        out.push_back(make_path(g, seg, vary_roles ? any_role(g) : role0));
        at = next;
    }
    return out;
}

// A loop: a star polygon cut into consecutive paths, the last closing back on the first point.
ExtrusionLoop make_loop(SplitMix &g)
{
    const Point   c(rnd(g, -5000000, 5000000), rnd(g, -5000000, 5000000));
    const Polygon poly = star(g, c, rnd(g, 300000, 8000000), size_t(rnd(g, 3, 24)));
    Points        pts  = poly.points;
    if (g.below(2) == 0)
        std::reverse(pts.begin(), pts.end());
    const size_t n     = pts.size();
    const size_t parts = size_t(rnd(g, 1, int64_t(std::min<size_t>(n, 4))));
    std::vector<size_t> cuts = {0};
    for (size_t i = 1; i < parts; ++i)
        cuts.push_back(std::min(n - 1, cuts.back() + 1 + size_t(g.below(std::max<uint64_t>(1, (n - cuts.back()) / 2)))));
    cuts.erase(std::unique(cuts.begin(), cuts.end()), cuts.end());
    ExtrusionPaths paths;
    for (size_t i = 0; i < cuts.size(); ++i) {
        const size_t a = cuts[i], b = i + 1 < cuts.size() ? cuts[i + 1] : n;
        Points       seg;
        for (size_t k = a; k <= b; ++k)
            seg.push_back(pts[k % n]);
        paths.push_back(make_path(g, seg, wall_role(g)));
    }
    static const ExtrusionLoopRole lr[] = {elrDefault, elrHole, elrInternal, elrSkirt, ExtrusionLoopRole(elrHole | elrInternal)};
    ExtrusionLoop loop(std::move(paths), lr[g.below(5)]);
    loop.inset_idx = int(rnd(g, -1, 3));
    return loop;
}

ExtrusionMultiPath make_multi(SplitMix &g)
{
    ExtrusionMultiPath m(joined_paths(g, size_t(rnd(g, 1, 4)), any_role(g), g.below(3) == 0));
    if (g.below(5) == 0)
        m.set_reverse();
    m.inset_idx = int(rnd(g, -1, 3));
    return m;
}

// Any entity, a collection holding one or more entities while depth allows.
ExtrusionEntity *make_any(SplitMix &g, int depth)
{
    switch (g.below(depth > 0 ? 7 : 6)) {
    case 0:
    case 1: return new ExtrusionPath(make_path(g, walk(g, size_t(rnd(g, 2, 8)), 1500000), any_role(g), g.below(4) == 0));
    case 2: {
        ExtrusionPath base = make_path(g, walk(g, size_t(rnd(g, 2, 6)), 1500000), any_role(g));
        auto *o = new ExtrusionPathOriented(base.role(), base.mm3_per_mm, base.width, base.height);
        o->polyline = base.polyline;
        return o;
    }
    case 3: {
        ExtrusionPath       base = make_path(g, walk(g, size_t(rnd(g, 2, 6)), 1500000), any_role(g), true);
        std::vector<double> z;
        for (size_t i = 0; i < base.polyline.points.size(); ++i)
            z.push_back(real(g, -0.1, 0.1));
        Polyline3 pl = base.polyline;
        return new ExtrusionPathContoured(std::move(pl), base, std::move(z));
    }
    case 4: return new ExtrusionMultiPath(make_multi(g));
    case 5: return new ExtrusionLoop(make_loop(g));
    default: {
        auto        *c = new ExtrusionEntityCollection();
        const size_t n = size_t(rnd(g, 1, 4));
        for (size_t i = 0; i < n; ++i)
            c->entities.push_back(make_any(g, depth - 1));
        c->no_sort = g.below(3) == 0;
        if (g.below(4) == 0)
            c->set_reverse();
        return c;
    }
    }
}

// One or two islands near a point, to clip paths with.
ExPolygons clip_islands(SplitMix &g, const Point &near)
{
    ExPolygons out;
    const size_t n = size_t(rnd(g, 1, 2));
    for (size_t i = 0; i < n; ++i)
        out.push_back(island(g, Point(near.x() + rnd(g, -2000000, 2000000), near.y() + rnd(g, -2000000, 2000000)), rnd(g, 300000, 3000000)));
    return out;
}

void put_covered(Payload &p, const ExtrusionEntity &e, float eps)
{
    attempt(p, [&](Payload &q) { put(q, e.polygons_covered_by_width(0.f)); });
    attempt(p, [&](Payload &q) { put(q, e.polygons_covered_by_width(eps)); });
    attempt(p, [&](Payload &q) { put(q, e.polygons_covered_by_spacing(0.f)); });
    attempt(p, [&](Payload &q) { put(q, e.polygons_covered_by_spacing(eps)); });
}

// ---- LYRO -----------------------------------------------------------------------------------

void record_roles(Sets &sets)
{
    Payload p;
    p.u64(uint64_t(erCount) + 1);
    for (int r = 0; r <= int(erCount); ++r) {
        const ExtrusionRole role = ExtrusionRole(r);
        const std::string   name = ExtrusionEntity::role_to_string(role);
        p.u8(uint8_t(r)).str(name).u8(uint8_t(ExtrusionEntity::string_to_role(name)));
        uint8_t bits = 0;
        bits |= is_perimeter(role) ? 1 : 0;
        bits |= is_internal_perimeter(role) ? 2 : 0;
        bits |= is_external_perimeter(role) ? 4 : 0;
        bits |= is_infill(role) ? 8 : 0;
        bits |= is_top_surface(role) ? 16 : 0;
        bits |= is_solid_infill(role) ? 32 : 0;
        bits |= is_bridge(role) ? 64 : 0;
        p.u8(bits);
    }
    const char *const odd[] = {"", "inner wall", "Inner wall ", "Skirt", "Brim", "Undefined", "Mixed", "Multiple", "Gap infill"};
    p.u64(std::size(odd));
    for (const char *s : odd)
        p.str(s).u8(uint8_t(ExtrusionEntity::string_to_role(s)));
    add(sets, "LYRO", p, true);
}

// ---- LYEN -----------------------------------------------------------------------------------

void record_paths(Sets &sets)
{
    SplitMix g(0x1A7EE00ull);
    for (int i = 0; i < 600; ++i) {
        std::unique_ptr<ExtrusionPath> path;
        const Points pts = i == 0 ? Points{Point(0, 0), Point(1000000, 0)} : i == 1 ? Points{Point(5, 5), Point(5, 5)} : walk(g, size_t(rnd(g, 2, 14)), 1500000);
        ExtrusionPath base = make_path(g, pts, i == 0 ? erPerimeter : any_role(g), g.below(4) == 0);
        if (i > 1 && g.below(8) == 0) {
            std::vector<double> z;
            for (size_t k = 0; k < base.polyline.points.size(); ++k)
                z.push_back(real(g, -0.1, 0.1));
            Polyline3 pl = base.polyline;
            path.reset(new ExtrusionPathContoured(std::move(pl), base, std::move(z)));
        } else
            path.reset(new ExtrusionPath(base));
        const float      eps  = float(rnd(g, 0, 50));
        const double     d    = real(g, 0, 1.2) * std::max(1., path->length());
        const double     tol  = real(g, 1000, 300000);
        const ExPolygons clip = clip_islands(g, pts.front());
        Payload          p;
        put_entity(p, *path);
        p.f32(eps).f64(d).f64(tol);
        put(p, clip);
        p.f64(path->length());
        put(p, path->first_point());
        put(p, path->last_point());
        p.boolean(path->is_closed());
        p.f64(path->min_mm3_per_mm()).f64(path->total_volume());
        put(p, path->as_polyline());
        Polylines pls;
        path->collect_polylines(pls);
        put(p, pls);
        Points cps;
        path->collect_points(cps);
        put(p, cps);
        put_covered(p, *path, eps);
        {
            std::unique_ptr<ExtrusionEntity> r(path->clone());
            r->reverse();
            put_entity(p, *r);
        }
        {
            std::unique_ptr<ExtrusionPath> c(static_cast<ExtrusionPath *>(path->clone()));
            c->clip_end(d);
            put_entity(p, *c);
        }
        {
            std::unique_ptr<ExtrusionPath> c(static_cast<ExtrusionPath *>(path->clone()));
            c->simplify(tol);
            put_entity(p, *c);
        }
        {
            std::unique_ptr<ExtrusionPath> c(static_cast<ExtrusionPath *>(path->clone()));
            c->ExtrusionPath::simplify_by_fitting_arc(tol);
            put_entity(p, *c);
        }
        {
            ExtrusionEntityCollection in, out;
            path->intersect_expolygons(clip, &in);
            path->subtract_expolygons(clip, &out);
            put_entity(p, in);
            put_entity(p, out);
        }
        add_within(sets, "LYEN", p, 25000);
    }
}

// ---- LYEM -----------------------------------------------------------------------------------

void record_multis(Sets &sets)
{
    SplitMix g(0x1A7EE01ull);
    for (int i = 0; i < 300; ++i) {
        ExtrusionMultiPath m   = make_multi(g);
        const float        eps = float(rnd(g, 0, 50));
        Payload            p;
        put_entity(p, m);
        p.f32(eps);
        p.f64(m.length()).u8(uint8_t(m.role()));
        put(p, m.first_point());
        put(p, m.last_point());
        p.f64(m.min_mm3_per_mm()).f64(m.total_volume());
        put(p, m.as_polyline());
        Polylines pls;
        m.collect_polylines(pls);
        put(p, pls);
        Points cps;
        m.collect_points(cps);
        put(p, cps);
        put_covered(p, m, eps);
        ExtrusionMultiPath r = m;
        r.reverse();
        put_entity(p, r);
        r.set_reverse();
        p.boolean(r.can_reverse());
        add_within(sets, "LYEM", p, 12000);
    }
}

// ---- LYEL -----------------------------------------------------------------------------------

void record_loops(Sets &sets)
{
    SplitMix g(0x1A7EE02ull);
    for (int i = 0; i < 400; ++i) {
        const ExtrusionLoop loop    = make_loop(g);
        const Polygon       poly    = loop.polygon();
        const Point         vertex  = poly.points[size_t(g.below(poly.points.size()))];
        Point               near_v  = vertex;
        switch (g.below(3)) {
        case 0: break;
        case 1: near_v = Point(vertex.x() + rnd(g, -600, 600), vertex.y() + rnd(g, -600, 600)); break;
        default: near_v = Point(vertex.x() + rnd(g, -3000000, 3000000), vertex.y() + rnd(g, -3000000, 3000000)); break;
        }
        const Point  q      = Point(vertex.x() + rnd(g, -2000000, 2000000), vertex.y() + rnd(g, -2000000, 2000000));
        const bool   prefer = g.below(2) == 0;
        const double eps    = g.below(2) == 0 ? scaled<double>(0.001) : real(g, 0, 200000);
        const double d      = real(g, 0, 1.2) * std::max(1., loop.length());
        const float  ceps   = float(rnd(g, 0, 50));
        Payload      p;
        put_entity(p, loop);
        put(p, near_v);
        put(p, q);
        p.boolean(prefer).f64(eps).f64(d).f32(ceps);
        put(p, poly);
        p.f64(loop.length()).u8(uint8_t(loop.role()));
        ExtrusionLoop a = loop;
        p.boolean(a.is_clockwise()).boolean(a.is_counter_clockwise());
        p.boolean(a.make_clockwise());
        put_entity(p, a);
        ExtrusionLoop b = loop;
        p.boolean(b.make_counter_clockwise());
        put_entity(p, b);
        ExtrusionLoop c = loop;
        p.boolean(c.split_at_vertex(near_v));
        put_entity(p, c);
        const ExtrusionLoop::ClosestPathPoint cp = loop.get_closest_path_and_point(q, prefer);
        p.u64(cp.path_idx).u64(cp.segment_idx);
        put(p, cp.foot_pt);
        ExtrusionLoop s = loop;
        s.split_at(q, prefer, eps);
        put_entity(p, s);
        ExtrusionPaths clipped;
        loop.clip_end(d, &clipped);
        put(p, clipped);
        p.boolean(loop.has_overhang_point(vertex));
        p.boolean(loop.is_smooth()).boolean(loop.is_smooth(0.5, 0.1));
        put(p, loop.as_polyline());
        p.f64(loop.min_mm3_per_mm()).f64(loop.total_volume());
        put_covered(p, loop, ceps);
        add_within(sets, "LYEL", p, 25000);
    }
}

// ---- LYES -----------------------------------------------------------------------------------

void record_sloped(Sets &sets)
{
    SplitMix g(0x1A7EE03ull);
    for (int i = 0; i < 200; ++i) {
        const ExtrusionLoop loop      = make_loop(g);
        ExtrusionPaths      original  = loop.paths;
        const double        seam_gap  = g.below(3) == 0 ? 0. : real(g, 0, 400000);
        const double        min_len   = real(g, 0.5, 25);
        const double        max_seg   = real(g, 200000, 3000000);
        const double        ratio     = real(g, 0, 1);
        const double        d         = real(g, 0, 3000000);
        const ExtrusionLoopRole role  = g.below(2) == 0 ? elrDefault : elrInternal;
        Payload p;
        put(p, original);
        p.f64(seam_gap).f64(min_len).f64(max_seg).f64(ratio).u8(uint8_t(role)).f64(d);
        attempt(p, [&](Payload &q) {
            ExtrusionPaths      in = original;
            ExtrusionLoopSloped s(in, seam_gap, min_len, max_seg, ratio, role);
            put_entity(q, s);
            q.u64(s.get_all_paths().size());
            q.f64(s.slope_path_length());
            q.u64(s.starts.size());
            for (const ExtrusionPathSloped &st : s.starts) {
                const ExtrusionPathSloped::Slope m = st.interpolate(0.5);
                q.f64(m.z_ratio).f64(m.e_ratio).boolean(st.is_flat());
            }
            ExtrusionLoopSloped a = s;
            a.clip_slope(d);
            put_entity(q, a);
            ExtrusionLoopSloped b = s;
            b.clip_end(d);
            put_entity(q, b);
            ExtrusionLoopSloped c = s;
            c.clip_front(d);
            put_entity(q, c);
        });
        add_within(sets, "LYES", p, 10000);
    }
}

// ---- LYEC -----------------------------------------------------------------------------------

void record_collections(Sets &sets)
{
    SplitMix g(0x1A7EE04ull);
    for (int i = 0; i < 300; ++i) {
        ExtrusionEntityCollection c;
        const size_t              n = size_t(rnd(g, 1, 6));
        for (size_t k = 0; k < n; ++k)
            c.entities.push_back(make_any(g, 2));
        c.no_sort = g.below(4) == 0;
        if (g.below(4) == 0)
            c.set_reverse();
        const float         eps   = float(rnd(g, 0, 50));
        const Point         start(rnd(g, -8000000, 8000000), rnd(g, -8000000, 8000000));
        const ExtrusionRole frole = g.below(2) == 0 ? c.entities.front()->role() : any_role(g);
        const size_t        ri    = size_t(g.below(c.entities.size()));
        Payload             p;
        put_entity(p, c);
        p.f32(eps);
        put(p, start);
        p.u8(uint8_t(frole)).u64(ri);
        p.u8(uint8_t(c.role())).u64(c.items_count());
        p.boolean(c.has_perimeters()).boolean(c.has_infill()).boolean(c.has_solid_infill()).boolean(c.can_sort()).boolean(c.can_reverse());
        p.f64(c.min_mm3_per_mm()).f64(c.total_volume());
        put(p, c.first_point());
        put(p, c.last_point());
        Polylines pls;
        c.collect_polylines(pls);
        put(p, pls);
        Points cps;
        c.collect_points(cps);
        put(p, cps);
        put_covered(p, c, eps);
        put_entity(p, c.flatten(false));
        put_entity(p, c.flatten(true));
        {
            ExtrusionEntityCollection r(c);
            r.reverse();
            put_entity(p, r);
        }
        put(p, ExtrusionPaths(c));
        put_entity(p, c.chained_path_from(start, erMixed));
        put_entity(p, c.chained_path_from(start, frole));
        {
            const ExtrusionEntitiesPtr kept = filter_by_extrusion_role(c.entities, frole);
            p.u64(kept.size());
            for (const ExtrusionEntity *e : kept)
                p.u64(uint64_t(std::find(c.entities.begin(), c.entities.end(), e) - c.entities.begin()));
        }
        {
            ExtrusionEntityCollection r(c);
            r.replace(ri, ExtrusionPath(make_path(g, walk(g, 3, 1000000), erGapFill)));
            put_entity(p, r);
            ExtrusionEntityCollection m(c);
            m.remove(ri);
            put_entity(p, m);
            ExtrusionEntityCollection a;
            a = c;
            p.boolean(a.can_reverse()).boolean(a.no_sort);
            ExtrusionEntityCollection s(c);
            s.set_reverse();
            p.boolean(s.can_reverse());
        }
        add_within(sets, "LYEC", p, 30000);
    }
}

// ---- LYEA -----------------------------------------------------------------------------------

void record_appends(Sets &sets)
{
    SplitMix g(0x1A7EE05ull);
    for (int i = 0; i < 200; ++i) {
        Polylines    pls;
        const size_t n = size_t(rnd(g, 0, 6));
        for (size_t k = 0; k < n; ++k) {
            Polyline pl(walk(g, size_t(rnd(g, 1, 6)), 1500000));
            if (pl.points.size() > 2 && g.below(4) == 0)
                pl.points.push_back(pl.points.front());
            if (k > 0 && g.below(3) == 0) {
                // Start near the last polyline's end, so the wipe joins them.
                const Point e = pls.back().points.back();
                for (Point &q : pl.points)
                    q = Point(q.x() - pl.points.front().x() + e.x() + rnd(g, -500000, 500000), q.y() - pl.points.front().y() + e.y());
            }
            pls.push_back(pl);
        }
        Polygons polys;
        for (const Polyline &pl : pls)
            polys.emplace_back(pl.points);
        const ExtrusionRole role = any_role(g);
        const double        mm3  = real(g, 0.01, 0.3);
        const float         w    = float(real(g, 0.1, 0.8));
        const float         h    = float(real(g, 0.05, 0.4));
        const bool          rev  = g.below(2) == 0;
        Payload             p;
        put(p, pls);
        p.u8(uint8_t(role)).f64(mm3).f32(w).f32(h).boolean(rev);
        {
            ExtrusionPaths dst;
            Polylines      copy = pls;
            extrusion_paths_append(dst, copy, role, mm3, w, h);
            put(p, dst);
        }
        {
            ExtrusionEntitiesPtr dst;
            Polylines            copy = pls;
            extrusion_entities_append_paths(dst, copy, role, mm3, w, h, rev);
            put(p, dst);
            for (ExtrusionEntity *e : dst)
                delete e;
        }
        {
            ExtrusionEntitiesPtr dst;
            extrusion_entities_append_paths_with_wipe(dst, Polylines(pls), role, mm3, w, h);
            put(p, dst);
            for (ExtrusionEntity *e : dst)
                delete e;
        }
        {
            ExtrusionEntitiesPtr dst;
            extrusion_entities_append_loops(dst, Polygons(polys), role, mm3, w, h);
            put(p, dst);
            for (ExtrusionEntity *e : dst)
                delete e;
        }
        {
            ExtrusionEntitiesPtr dst;
            extrusion_entities_append_loops_and_paths(dst, Polylines(pls), role, mm3, w, h);
            put(p, dst);
            for (ExtrusionEntity *e : dst)
                delete e;
        }
        add_within(sets, "LYEA", p, 8000);
    }
}

// ---- LYVW -----------------------------------------------------------------------------------

void record_variable_width(Sets &sets)
{
    SplitMix g(0x1A7EE06ull);
    for (int i = 0; i < 400; ++i) {
        ThickPolylines tps;
        const size_t   n = size_t(rnd(g, 1, 3));
        for (size_t k = 0; k < n; ++k) {
            ThickPolyline tp;
            const size_t  m = size_t(rnd(g, 2, 12));
            Point         c(rnd(g, -5000000, 5000000), rnd(g, -5000000, 5000000));
            tp.points.push_back(c);
            for (size_t j = 1; j < m; ++j) {
                const int64_t step = g.below(8) == 0 ? 60 : 1500000;
                c = Point(c.x() + rnd(g, -step, step), c.y() + rnd(g, -step, step));
                tp.points.push_back(c);
            }
            if (m > 3 && g.below(4) == 0)
                tp.points.push_back(tp.points.front());
            double w = real(g, 50000, 900000);
            for (size_t j = 0; j + 1 < tp.points.size(); ++j) {
                tp.width.push_back(w);
                w = g.below(3) == 0 ? w : std::max(150., w + real(g, -300000, 300000));
                tp.width.push_back(w);
            }
            tp.endpoints = std::make_pair(g.below(2) == 0, g.below(2) == 0);
            tps.push_back(tp);
        }
        static const ExtrusionRole roles[] = {erPerimeter, erExternalPerimeter, erOverhangPerimeter, erGapFill};
        const ExtrusionRole        role    = roles[g.below(4)];
        const int                  fk      = int(g.below(3));
        const Flow flow  = fk == 0 ? Flow(0.45f, 0.2f, 0.4f) : fk == 1 ? Flow::bridging_flow(0.4f, 0.4f) : Flow(float(real(g, 0.3, 0.8)), float(real(g, 0.08, 0.28)), 0.6f);
        const float tol  = float(real(g, 5000, 80000));
        const float mtol = tol * float(real(g, 0.2, 1.0));
        Payload     p;
        p.u64(tps.size());
        for (const ThickPolyline &tp : tps) {
            put(p, tp.points);
            p.u64(tp.width.size());
            for (double w : tp.width)
                p.f64(w);
            p.boolean(tp.endpoints.first).boolean(tp.endpoints.second);
        }
        p.u8(uint8_t(role));
        put(p, flow);
        p.f32(tol).f32(mtol);
        attempt(p, [&](Payload &q) {
            ExtrusionEntitiesPtr out;
            variable_width(tps, role, flow, out);
            put(q, out);
            for (ExtrusionEntity *e : out)
                delete e;
        });
        p.u64(tps.size());
        for (const ThickPolyline &tp : tps)
            attempt(p, [&](Payload &q) { put_entity(q, thick_polyline_to_multi_path(tp, role, flow, tol, mtol)); });
        add_within(sets, "LYVW", p, 15000);
    }
}

// ---- LYCH -----------------------------------------------------------------------------------

void record_chains(Sets &sets)
{
    SplitMix g(0x1A7EE07ull);
    for (int i = 0; i < 200; ++i) {
        const int  kind  = int(g.below(3));
        const bool has_s = g.below(4) != 0;
        const Point start(rnd(g, -8000000, 8000000), rnd(g, -8000000, 8000000));
        Payload    p;
        p.u8(uint8_t(kind)).boolean(has_s);
        put(p, start);
        if (kind == 0) {
            // chain_extrusion_entities of entities, then reorder_extrusion_entities by it.
            ExtrusionEntitiesPtr v;
            const size_t         n = size_t(rnd(g, 0, 8));
            for (size_t k = 0; k < n; ++k)
                v.push_back(make_any(g, 1));
            put(p, v);
            const std::vector<std::pair<size_t, bool>> chain = chain_extrusion_entities(v, has_s ? &start : nullptr);
            p.u64(chain.size());
            for (const auto &c : chain)
                p.u64(c.first).boolean(c.second);
            reorder_extrusion_entities(v, chain);
            put(p, v);
            for (ExtrusionEntity *e : v)
                delete e;
        } else if (kind == 1) {
            // chain_and_reorder_extrusion_paths of paths.
            ExtrusionPaths v;
            const size_t   n = size_t(rnd(g, 0, 8));
            for (size_t k = 0; k < n; ++k)
                v.push_back(make_path(g, walk(g, size_t(rnd(g, 2, 5)), 1500000), any_role(g)));
            put(p, v);
            chain_and_reorder_extrusion_paths(v, has_s ? &start : nullptr);
            put(p, v);
        } else {
            // chained_path_from of a collection that holds entities without end points, which
            // chain_and_reorder_extrusion_entities drops.
            ExtrusionEntityCollection c;
            const size_t              n = size_t(rnd(g, 1, 7));
            for (size_t k = 0; k < n; ++k) {
                switch (g.below(6)) {
                case 0: c.entities.push_back(new ExtrusionPath(any_role(g), 0.1, 0.4f, 0.2f)); break;
                case 1: c.entities.push_back(new ExtrusionMultiPath()); break;
                case 2: c.entities.push_back(new ExtrusionLoop()); break;
                case 3: c.entities.push_back(new ExtrusionEntityCollection()); break;
                default: c.entities.push_back(make_any(g, 1)); break;
                }
            }
            put_entity(p, c);
            put_entity(p, c.chained_path_from(start, erMixed));
        }
        add_within(sets, "LYCH", p, 20000);
    }
}

} // namespace

void record_extrusion(Sets &sets)
{
    record_roles(sets);
    record_paths(sets);
    record_multis(sets);
    record_loops(sets);
    record_sloped(sets);
    record_collections(sets);
    record_appends(sets);
    record_variable_width(sets);
    record_chains(sets);
}

} // namespace layer
} // namespace probe
