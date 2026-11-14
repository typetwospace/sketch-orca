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
// surfaces.sop1's region expansion records (parity/sop1/surfaces.md, "SFRP", "SFRX" and "SFME",
// added 2026-11-13 by orca-o3-probe-surfaces), which orca-surface replays for the piece
// surfaces-1: Algorithm/RegionExpansion.cpp as the engine compiles it.
//
//   SFRP  RegionExpansionParameters::build;
//   SFRX  one source and boundary through every public function: wave_seeds (unsorted and
//         sorted), propagate_waves and propagate_waves_ex of the seeds, expand_expolygons,
//         merge_expansions_into_expolygons, and the overloads that start from the source, each
//         compared with the first result it must equal;
//   SFME  merge_expansions_into_expolygons of expansions made here (unsorted source ids, ties,
//         expansions apart from their source, sources that vanish in the union).
//
// The static helpers (the expanded and opened seed contours, merge_splits, the AABB tree over the
// boundaries, the wavefront steps) are reached through these functions, as the engine reaches
// them. The inputs: hand-made layouts for each branch of wave_seeds (a seed that crosses the
// boundary, a closed seed found by the AABB tree, both ends on boundary vertices, a closed seed
// that starts on a boundary vertex), the bridges of the test meshes' layers expanded into their
// shells, sparse infill and tops, and seeded layouts whose source and boundary share edges.
#include "shared.hpp"

#include <algorithm>
#include <numeric>

#include "libslic3r/ClipperUtils.hpp"
#include "libslic3r/clipper.hpp"

namespace probe {

void record_expansion(Sets &sets, const std::vector<surfaces::Layer> &layers);

namespace {

using namespace probe::surfaces;
namespace A = Slic3r::Algorithm;

// ---------------------------------------------------------------- SFRP

void put_build(Sets &sets, Picker &pick, float full, float step, uint64_t max_steps)
{
    Payload p;
    p.f32(full).f32(step).u64(max_steps);
    put(p, A::RegionExpansionParameters::build(full, step, size_t(max_steps)));
    add(sets, "SFRP", p, pick);
}

// The expansions process_external_surfaces asks for, with 0.4 mm nozzle flows: the perimeter
// spacing (into sparse infill), the shell width times sqrt(2) for one to five walls (into solid
// infill), SCALED_EPSILON with no walls, each by waves of scaled 0.1 mm, at most 5 of them.
std::vector<float> domain_expansions()
{
    std::vector<float> out{407080.f, 100.f};
    for (int walls = 1; walls <= 5; ++walls) {
        float shell = 0.5f * 420000.f + 377080.f;
        shell += 407080.f * float(walls - 1);
        out.push_back(float(shell * std::sqrt(2.)));
    }
    return out;
}

void record_sfrp(Sets &sets)
{
    SplitMix g(0x5F3E0004u);
    Picker   pick{260, 64};
    for (float e : domain_expansions())
        put_build(sets, pick, e, Slic3r::scaled<float>(0.1), 5);
    // Each branch: tiny expansion a quarter of the full one or 0.05 mm, one step, too many steps
    // for the tiny expansion, the step count capped, a single step after the cap.
    for (float e : {1.f, 3.f, 100.f, 199999.f, 200000.f, 200001.f, 250000.f, 260000.f, 400000.f, 1e6f, 5e6f, 3e7f})
        for (float s : {1.f, 1e4f, 1e5f, 2.5e5f, 1e6f})
            for (uint64_t m : {uint64_t(1), uint64_t(2), uint64_t(5), uint64_t(1000)})
                put_build(sets, pick, e, s, m);
    for (int i = 0; i < 2000; ++i) {
        const float    e = float(std::exp(uniform(g, std::log(1.), std::log(5e7))));
        const float    s = float(std::exp(uniform(g, std::log(1.), std::log(5e6))));
        const uint64_t m = 1 + g.below(g.below(4) == 0 ? 100 : 10);
        put_build(sets, pick, e, s, m);
    }
}

// ---------------------------------------------------------------- SFRX

struct RxCase
{
    std::string label;
    ExPolygons  src;
    ExPolygons  boundary;
    float       expansion;
    float       step;
    uint64_t    max_steps;
};

ExPolygon with_hole(const Polygon &contour, const Polygon &hole)
{
    ExPolygon e(contour);
    e.holes.push_back(reversed(hole));
    return e;
}

// The contour of `src` expanded as wave_seeds expands it before clipping (square joins, the
// shortest edge length of ClipperUtils), for building boundaries on its vertices.
Points expanded_contour(const Polygon &contour, float tiny)
{
    Slic3r::ClipperLib::ClipperOffset off;
    off.ShortestEdgeLength = tiny * Slic3r::ClipperOffsetShortestEdgeFactor;
    off.AddPath(Slic3r::ClipperLib::Path(contour.points.begin(), contour.points.end()), Slic3r::ClipperLib::jtSquare, Slic3r::ClipperLib::etClosedPolygon);
    Slic3r::ClipperLib::Paths out;
    off.Execute(out, tiny);
    return out.empty() ? Points{} : Points(out.front().begin(), out.front().end());
}

std::vector<RxCase> hand_cases()
{
    const float          e = 1103637.f; // two walls' shell width times sqrt(2)
    const float          s = 100000.f;
    std::vector<RxCase> out;
    auto push = [&](const char *label, ExPolygons src, ExPolygons boundary) { out.push_back({label, std::move(src), std::move(boundary), e, s, 5}); };
    push("touching side by side", {ExPolygon(rect(0, 0, 10, 10))}, {ExPolygon(rect(10, 0, 20, 10))});
    push("the source fills the boundary's hole (a closed seed)", {ExPolygon(rect(0, 0, 10, 10))}, {with_hole(rect(-10, -10, 20, 20), rect(0, 0, 10, 10))});
    push("two boundaries", {ExPolygon(rect(0, 0, 10, 10))}, {ExPolygon(rect(-10, 0, 0, 10)), ExPolygon(rect(10, 0, 20, 10))});
    push("a boundary in the source's hole", {with_hole(rect(0, 0, 30, 30), rect(10, 10, 20, 20))}, {ExPolygon(rect(10, 10, 20, 20)), with_hole(rect(-5, -5, 35, 35), rect(0, 0, 30, 30))});
    push("two sources, two boundaries", {ExPolygon(rect(0, 0, 10, 10)), ExPolygon(rect(20, 0, 30, 10))}, {ExPolygon(rect(10, 0, 20, 10)), ExPolygon(rect(0, 10, 30, 20))});
    push("a gap", {ExPolygon(rect(0, 0, 10, 10))}, {ExPolygon(rect(15, 0, 25, 10))});
    push("no source", {}, {ExPolygon(rect(15, 0, 25, 10))});
    push("no boundary", {ExPolygon(rect(0, 0, 10, 10))}, {});
    push("an island in the boundary's hole", {with_hole(rect(-10, -10, 20, 20), rect(2, 2, 8, 8))}, {with_hole(rect(-20, -20, 30, 30), rect(-10, -10, 20, 20)), ExPolygon(rect(2, 2, 8, 8))});
    push("a corridor narrower than a step", {ExPolygon(rect(0, 0, 10, 10))}, {ExPolygon(rect(10, 0, 10.05, 10)), ExPolygon(Polygon(Points{Point(mm(10.05), 0), Point(mm(30), 0), Point(mm(30), mm(0.3)), Point(mm(10.05), mm(0.3))}))});
    push("touching at a corner", {ExPolygon(rect(0, 0, 10, 10))}, {ExPolygon(rect(10, 10, 20, 20))});
    push("overlapping", {ExPolygon(rect(0, 0, 10, 10))}, {ExPolygon(rect(5, 5, 15, 15))});
    push("a long thin source", {ExPolygon(rect(0, 0, 40, 0.3))}, {ExPolygon(rect(0, 0.3, 40, 6)), ExPolygon(rect(0, -6, 40, 0))});
    push("a star into a ring", {ExPolygon(Polygon(Points{Point(0, mm(6)), Point(mm(1.5), mm(1.5)), Point(mm(6), 0), Point(mm(1.5), mm(-1.5)), Point(0, mm(-6)), Point(mm(-1.5), mm(-1.5)), Point(mm(-6), 0), Point(mm(-1.5), mm(1.5))}))}, {with_hole(rect(-10, -10, 10, 10), rect(-5, -5, 5, 5))});
    {
        // Boundaries on the vertices of the expanded seed contour.
        const Polygon sq   = rect(0, 0, 10, 10);
        const float   tiny = A::RegionExpansionParameters::build(e, s, 5).tiny_expansion;
        const Points  ex   = expanded_contour(sq, tiny);
        const int64_t t    = int64_t(tiny);
        // A diamond whose two vertices lie on the right and top edges, around the corner (10, 10).
        push("a boundary with vertices on the expanded seed, around its corner", {ExPolygon(sq)},
             {ExPolygon(Polygon(Points{Point(mm(5), mm(5)), Point(mm(10) + t, mm(5)), Point(mm(20), mm(20)), Point(mm(5), mm(10) + t)}))});
        if (ex.size() >= 4) {
            // A triangle on two vertices of the expanded contour and a point outside.
            const Point a = ex[0], b = ex[2];
            const Point c((a.x() + b.x()) / 2 + mm(8), (a.y() + b.y()) / 2 + mm(8));
            Polygon     tri(Points{a, c, b});
            if (tri.area() < 0)
                tri.reverse();
            push("a boundary on two vertices of the expanded seed", {ExPolygon(sq)}, {ExPolygon(tri)});
            // A closed seed inside a boundary whose notch touches one of its vertices.
            Point top = ex[0];
            for (const Point &q : ex)
                if (q.x() > top.x() || (q.x() == top.x() && q.y() > top.y()))
                    top = q;
            push("a closed seed touching a notch of its boundary at a vertex", {ExPolygon(sq)},
                 {with_hole(Polygon(Points{Point(mm(-10), mm(-10)), Point(mm(20), mm(-10)), Point(mm(20), top.y() - mm(3)), top, Point(mm(20), top.y() + mm(3)), Point(mm(20), mm(20)), Point(mm(-10), mm(20))}), sq)});
            push("a seed on the boundary's hole, its vertices shared", {ExPolygon(Polygon(ex))}, {with_hole(rect(-10, -10, 20, 20), Polygon(ex))});
        }
    }
    // The same layouts with the other expansions process_external_surfaces uses.
    const size_t n = out.size();
    for (size_t i = 0; i < n; ++i) {
        RxCase c = out[i];
        c.label += ", into sparse infill";
        c.expansion = 407080.f;
        out.push_back(c);
    }
    {
        RxCase c = out[0];
        c.label  = "touching side by side, no walls (SCALED_EPSILON)";
        c.expansion = 100.f;
        out.push_back(c);
        c.label     = "touching side by side, one step";
        c.expansion = 1e6f;
        c.max_steps = 1;
        out.push_back(c);
        c.label     = "touching side by side, a 3 mm expansion by 0.5 mm";
        c.expansion = 3e6f;
        c.step      = 5e5f;
        c.max_steps = 10;
        out.push_back(c);
    }
    return out;
}

void layer_cases(std::vector<RxCase> &out, const std::vector<Layer> &layers)
{
    const std::vector<float> ex = domain_expansions();
    for (const Layer &l : layers) {
        ExPolygons bridges, tops;
        Polygons   shells, sparse, top_polys;
        for (const Surface &s : l.surfaces) {
            if (s.surface_type == Slic3r::stBottomBridge || s.surface_type == Slic3r::stBottom)
                bridges.push_back(s.expolygon);
            else if (s.surface_type == Slic3r::stTop)
                tops.push_back(s.expolygon);
            if (s.surface_type == Slic3r::stInternalSolid)
                Slic3r::polygons_append(shells, Slic3r::to_polygons(s.expolygon));
            else if (s.surface_type == Slic3r::stInternal)
                Slic3r::polygons_append(sparse, Slic3r::to_polygons(s.expolygon));
            else if (s.surface_type == Slic3r::stTop)
                Slic3r::polygons_append(top_polys, Slic3r::to_polygons(s.expolygon));
        }
        const ExPolygons zshells = Slic3r::union_ex(shells), zsparse = Slic3r::union_ex(sparse), ztop = Slic3r::union_ex(top_polys);
        out.push_back({l.name + ": bridges into shells", bridges, zshells, ex[3], 100000.f, 5});
        out.push_back({l.name + ": bridges into sparse infill", bridges, zsparse, ex[0], 100000.f, 5});
        if (!ztop.empty())
            out.push_back({l.name + ": bridges into tops", bridges, ztop, ex[3], 100000.f, 5});
        if (!tops.empty())
            out.push_back({l.name + ": tops into shells", tops, zshells, ex[4], 100000.f, 5});
    }
}

void random_cases(std::vector<RxCase> &out, int count)
{
    SplitMix g(0x5F3E0005u);
    for (int i = 0; i < count; ++i) {
        const ExPolygons r = region(g, 4, 30.), q = region(g, 3, 30.);
        RxCase           c;
        c.label = "seeded";
        switch (g.below(4)) {
        case 0:
            c.src      = Slic3r::intersection_ex(r, q);
            c.boundary = Slic3r::diff_ex(r, q);
            break;
        case 1:
            c.src      = Slic3r::diff_ex(r, q);
            c.boundary = Slic3r::intersection_ex(r, q);
            break;
        case 2:
            c.src      = Slic3r::intersection_ex(r, q);
            c.boundary = Slic3r::diff_ex(Slic3r::offset_ex(r, float(mm(2.))), q);
            break;
        default:
            c.src      = q;
            c.boundary = r;
        }
        const uint64_t k = g.below(10);
        c.expansion      = k < 3 ? float(uniform(g, 3.5e5, 5e5)) : k < 8 ? float(uniform(g, 6e5, 3e6)) : k < 9 ? float(uniform(g, 100., 5e4)) : float(uniform(g, 3e6, 8e6));
        c.step           = g.below(4) ? 100000.f : float(uniform(g, 1e4, 1e6));
        c.max_steps      = g.below(4) ? 5 : 1 + g.below(10);
        out.push_back(std::move(c));
    }
}

template<typename T> bool same_expansions(const std::vector<T> &a, const std::vector<T> &b);
template<> bool same_expansions(const std::vector<A::RegionExpansion> &a, const std::vector<A::RegionExpansion> &b)
{
    if (a.size() != b.size())
        return false;
    for (size_t i = 0; i < a.size(); ++i)
        if (!same(a[i].polygon, b[i].polygon) || a[i].src_id != b[i].src_id || a[i].boundary_id != b[i].boundary_id)
            return false;
    return true;
}
template<> bool same_expansions(const std::vector<A::RegionExpansionEx> &a, const std::vector<A::RegionExpansionEx> &b)
{
    if (a.size() != b.size())
        return false;
    for (size_t i = 0; i < a.size(); ++i)
        if (!same(a[i].expolygon, b[i].expolygon) || a[i].src_id != b[i].src_id || a[i].boundary_id != b[i].boundary_id)
            return false;
    return true;
}

bool same_seeds(const A::WaveSeeds &a, const A::WaveSeeds &b)
{
    if (a.size() != b.size())
        return false;
    for (size_t i = 0; i < a.size(); ++i)
        if (a[i].src != b[i].src || a[i].boundary != b[i].boundary || a[i].path != b[i].path)
            return false;
    return true;
}

Payload sfrx_case(const RxCase &c)
{
    Payload p;
    p.str(c.label);
    put(p, c.src);
    put(p, c.boundary);
    p.f32(c.expansion).f32(c.step).u64(c.max_steps);
    const A::RegionExpansionParameters params = A::RegionExpansionParameters::build(c.expansion, c.step, size_t(c.max_steps));
    put(p, params);
    const A::WaveSeeds unsorted = A::wave_seeds(c.src, c.boundary, params.tiny_expansion, false);
    const A::WaveSeeds seeds    = A::wave_seeds(c.src, c.boundary, params.tiny_expansion, true);
    put(p, unsorted);
    // The sorted seeds as the permutation std::sort makes of the unsorted ones (std::sort of the
    // indices under the same comparison is the same permutation); written whole if they differ.
    std::vector<uint32_t> perm(unsorted.size());
    std::iota(perm.begin(), perm.end(), 0u);
    std::sort(perm.begin(), perm.end(), [&](uint32_t l, uint32_t r) { return A::lower_by_boundary_and_src(unsorted[l], unsorted[r]); });
    A::WaveSeeds permuted;
    for (uint32_t i : perm)
        permuted.push_back(unsorted[i]);
    if (same_seeds(permuted, seeds)) {
        p.u8(0).u32s(perm);
    } else {
        p.u8(1);
        put(p, seeds);
    }
    const std::vector<A::RegionExpansion>   pw  = A::propagate_waves(seeds, c.boundary, params);
    const std::vector<A::RegionExpansionEx> pwx = A::propagate_waves_ex(seeds, c.boundary, params);
    put(p, pw);
    put(p, pwx);
    // expand_expolygons: pw grouped by source id, in order (written whole if it is not).
    const std::vector<Polygons> ee = A::expand_expolygons(c.src, c.boundary, c.expansion, c.step, size_t(c.max_steps));
    std::vector<Polygons>       grouped(c.src.size());
    for (const A::RegionExpansion &r : pw)
        grouped[r.src_id].push_back(r.polygon);
    bool ee_same = ee.size() == grouped.size();
    for (size_t i = 0; ee_same && i < ee.size(); ++i)
        ee_same = same(ee[i], grouped[i]);
    p.boolean(ee_same);
    if (!ee_same) {
        p.u64(ee.size());
        for (const Polygons &x : ee)
            put(p, x);
    }
    const ExPolygons merged = A::merge_expansions_into_expolygons(ExPolygons(c.src), std::vector<A::RegionExpansion>(pw));
    put(p, merged);
    // The overloads that start from the source.
    const auto pw2 = A::propagate_waves(c.src, c.boundary, params);
    p.boolean(same_expansions(pw2, pw));
    if (!same_expansions(pw2, pw))
        put(p, pw2);
    const auto pw3 = A::propagate_waves(c.src, c.boundary, c.expansion, c.step, size_t(c.max_steps));
    p.boolean(same_expansions(pw3, pw));
    if (!same_expansions(pw3, pw))
        put(p, pw3);
    const auto pwx2 = A::propagate_waves_ex(c.src, c.boundary, c.expansion, c.step, size_t(c.max_steps));
    p.boolean(same_expansions(pwx2, pwx));
    if (!same_expansions(pwx2, pwx))
        put(p, pwx2);
    const ExPolygons merged2 = A::expand_merge_expolygons(ExPolygons(c.src), c.boundary, params);
    p.boolean(same(merged2, merged));
    if (!same(merged2, merged))
        put(p, merged2);
    return p;
}

void record_sfrx(Sets &sets, const std::vector<Layer> &layers)
{
    std::vector<RxCase> cases = hand_cases();
    layer_cases(cases, layers);
    random_cases(cases, 500);
    Picker pick{70, 6000};
    for (const RxCase &c : cases)
        add(sets, "SFRX", sfrx_case(c), pick);
}

// ---------------------------------------------------------------- SFME

void sfme_case(Sets &sets, Picker &pick, const ExPolygons &src, const std::vector<A::RegionExpansion> &expanded)
{
    Payload p;
    put(p, src);
    put(p, expanded);
    put(p, A::merge_expansions_into_expolygons(ExPolygons(src), std::vector<A::RegionExpansion>(expanded)));
    add(sets, "SFME", p, pick);
}

void record_sfme(Sets &sets)
{
    SplitMix g(0x5F3E0006u);
    Picker   pick{45, 3000};
    using RE = A::RegionExpansion;
    const ExPolygons two{ExPolygon(rect(0, 0, 10, 10)), ExPolygon(rect(20, 0, 30, 10))};
    sfme_case(sets, pick, two, {});
    sfme_case(sets, pick, two, {RE{rect(10, 0, 12, 10), 0, 0}});
    sfme_case(sets, pick, two, {RE{rect(18, 0, 20, 10), 1, 3}, RE{rect(10, 0, 12, 10), 0, 0}, RE{rect(5, 10, 25, 12), 1, 0}, RE{rect(5, -2, 25, 0), 0, 2}});
    // An expansion apart from its source: the union has two parts; the one with the source's
    // first point is kept.
    sfme_case(sets, pick, two, {RE{rect(0, 20, 10, 30), 0, 0}});
    // A source of no area: the union holds only the expansions, none has the source's first point,
    // so the source is dropped; and one whose only expansion has no area either.
    const ExPolygons flat{ExPolygon(Polygon(Points{Point(0, 0), Point(mm(10), 0), Point(mm(20), 0)})), ExPolygon(rect(20, 0, 30, 10))};
    sfme_case(sets, pick, flat, {RE{rect(0, 20, 10, 30), 0, 0}, RE{rect(40, 20, 50, 30), 0, 1}});
    sfme_case(sets, pick, flat, {RE{Polygon(Points{Point(0, 0), Point(mm(5), 0)}), 0, 0}});
    // A source with a hole, the hole filled by its expansion.
    sfme_case(sets, pick, {with_hole(rect(0, 0, 30, 30), rect(10, 10, 20, 20))}, {RE{rect(10, 10, 20, 20), 0, 0}});
    for (int i = 0; i < 500; ++i) {
        ExPolygons src;
        const int  ns = 1 + int(g.below(5));
        for (int j = 0; j < ns; ++j) {
            const double x = 25. * j, y = uniform(g, 0., 5.);
            if (g.below(8) == 0)
                src.push_back(ExPolygon(Polygon(Points{Point(mm(x), mm(y)), Point(mm(x + 5), mm(y)), Point(mm(x + 10), mm(y))})));
            else if (g.below(2))
                src.push_back(ExPolygon(rect(x, y, x + uniform(g, 2., 15.), y + uniform(g, 2., 15.))));
            else
                src.push_back(ExPolygon(star(g, 3 + int(g.below(9)), x + 8, y + 8, uniform(g, 3., 8.))));
        }
        std::vector<RE> ex;
        const int       ne = int(g.below(9));
        for (int j = 0; j < ne; ++j) {
            const uint32_t sid = uint32_t(g.below(uint64_t(ns)));
            const double   x = 25. * sid + uniform(g, -6., 16.), y = uniform(g, -6., 16.);
            ex.push_back(RE{g.below(2) ? rect(x, y, x + uniform(g, 0.5, 6.), y + uniform(g, 0.5, 6.)) : star(g, 3 + int(g.below(6)), x, y, uniform(g, 0.5, 5.)), sid, uint32_t(g.below(4))});
        }
        sfme_case(sets, pick, src, ex);
    }
}

} // namespace

void record_expansion(Sets &sets, const std::vector<surfaces::Layer> &layers)
{
    record_sfrp(sets);
    record_sfrx(sets, layers);
    record_sfme(sets);
}

} // namespace probe
