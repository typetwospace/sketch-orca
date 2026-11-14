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
// surfaces.sop1's bridge records (parity/sop1/surfaces.md, "SFBR" and "SFBF", added 2026-11-13 by
// orca-o3-probe-surfaces), which orca-surface replays for the piece surfaces-3: the helpers of
// LayerRegion::process_external_surfaces that can be called on their own, the bridges' anchors
// and expansions (expand_expolygons of LayerRegion.cpp), their grouping (group_id,
// get_grouped_bridges), their orientation (detect_bridge_directions) and their merging
// (merge_bridges), and the two functions made of them, expand_bridges_detect_orientations and
// expand_merge_surfaces. process_external_surfaces itself, and PrintObject's surface steps, whose
// own helpers are lambdas inside them, are proven by the step dumps of orca-o3-dumps.
//
//   SFBR  one layer's fill surfaces and expansion zones through each step of
//         expand_bridges_detect_orientations in its order, then through it whole, and through
//         expand_merge_surfaces for the bottom bridges at a given angle, the bottoms and the tops;
//   SFBF  get_grouped_bridges, detect_bridge_directions and merge_bridges alone, on inputs made
//         here (overlapping and nested expansions, anchors in several zones or in none, unset
//         angles, no zone at all, where detect_bridge_directions throws).
//
// Carries OrcaSlicer code: the definitions of ExpansionZone, Bridge and ExpansionResult and the
// declarations of group_id, get_grouped_bridges, detect_bridge_directions, merge_bridges,
// expand_expolygons, expand_bridges_detect_orientations and expand_merge_surfaces are copied as
// they are from src/libslic3r/LayerRegion.cpp of OrcaSlicer v2.4.2 (lines 166-179, 182, 192-195,
// 262-266, 310-314, 353-361, 398-402 and 439-445), whose copyright is held by the OrcaSlicer
// contributors, Prusa Research and the Slic3r authors, under the AGPL-3.0; added 2026-11-13.
// LayerRegion.cpp defines these types in no header, so the probe declares them token for token,
// as one definition in every translation unit, and calls the engine's own compiled functions,
// which have external linkage. Nothing of their bodies is here.
#include "shared.hpp"

#include <algorithm>
#include <numeric>
#include <optional>
#include <stdexcept>

#include "libslic3r/BoundingBox.hpp"
#include "libslic3r/ClipperUtils.hpp"

namespace Slic3r {

// As LayerRegion.cpp has them (see the Carries block above).
struct ExpansionZone
{
    ExPolygons                           expolygons;
    Algorithm::RegionExpansionParameters parameters;
    bool                                 expanded_into = false;
};

// Cache for detecting bridge orientation and merging regions with overlapping expansions.
struct Bridge {
    ExPolygon expolygon;
    uint32_t group_id;
    std::vector<Algorithm::RegionExpansionEx>::const_iterator bridge_expansion_begin;
    std::optional<double> angle{std::nullopt};
};

uint32_t group_id(std::vector<Bridge> &bridges, uint32_t src_id);

std::vector<Bridge> get_grouped_bridges(
    ExPolygons&& bridge_expolygons,
    const std::vector<Algorithm::RegionExpansionEx>& bridge_expansions
);

void detect_bridge_directions(
    const Algorithm::WaveSeeds& bridge_anchors,
    std::vector<Bridge>& bridges,
    const std::vector<ExpansionZone>& expansion_zones
);

Surfaces merge_bridges(
    std::vector<Bridge>& bridges,
    const std::vector<Algorithm::RegionExpansionEx>& bridge_expansions,
    const float closing_radius
);

struct ExpansionResult {
    Algorithm::WaveSeeds anchors;
    std::vector<Algorithm::RegionExpansionEx> expansions;
};

ExpansionResult expand_expolygons(
    const ExPolygons& expolygons,
    std::vector<ExpansionZone>& expansion_zones
);

Surfaces expand_bridges_detect_orientations(
    Surfaces &surfaces,
    std::vector<ExpansionZone>& expansion_zones,
    const float closing_radius
);

Surfaces expand_merge_surfaces(
    Surfaces &surfaces,
    SurfaceType surface_type,
    std::vector<ExpansionZone>& expansion_zones,
    const float closing_radius,
    const double bridge_angle
);

} // namespace Slic3r

namespace probe {

void record_bridges(Sets &sets, const std::vector<surfaces::Layer> &layers);

namespace {

using namespace probe::surfaces;
namespace A = Slic3r::Algorithm;
using Slic3r::Bridge;
using Slic3r::ExpansionZone;
using Zones = std::vector<ExpansionZone>;

// Each zone's expolygons and u8 expanded into.
void put_zones_after(Payload &p, const Zones &zones)
{
    p.u64(zones.size());
    for (const ExpansionZone &z : zones) {
        put(p, z.expolygons);
        p.boolean(z.expanded_into);
    }
}

// Each surface's u8: whether its expolygon was moved out (now empty) by the call.
void put_moved(Payload &p, const Surfaces &ss)
{
    p.u64(ss.size());
    for (const Surface &s : ss)
        p.boolean(s.expolygon.contour.points.empty() && s.expolygon.holes.empty());
}

void put_angles(Payload &p, const std::vector<Bridge> &bridges)
{
    p.u64(bridges.size());
    for (const Bridge &b : bridges) {
        p.boolean(b.angle.has_value());
        if (b.angle)
            p.f64(*b.angle);
    }
}

// The raw group ids, then group_id() of each bridge in order on a copy (the chain goes on with
// the bridges as get_grouped_bridges left them).
void put_groups(Payload &p, const std::vector<Bridge> &bridges)
{
    p.u64(bridges.size());
    for (const Bridge &b : bridges)
        p.u32(b.group_id);
    std::vector<Bridge> copy = bridges;
    for (uint32_t i = 0; i < uint32_t(copy.size()); ++i)
        p.u32(Slic3r::group_id(copy, i));
}

template<typename T, typename Less> std::vector<uint32_t> sort_permutation(const std::vector<T> &v, Less less)
{
    std::vector<uint32_t> perm(v.size());
    std::iota(perm.begin(), perm.end(), 0u);
    std::sort(perm.begin(), perm.end(), [&](uint32_t l, uint32_t r) { return less(v[l], v[r]); });
    return perm;
}

bool same_seed(const A::WaveSeed &a, const A::WaveSeed &b) { return a.src == b.src && a.boundary == b.boundary && a.path == b.path; }
bool same_ex(const A::RegionExpansionEx &a, const A::RegionExpansionEx &b) { return same(a.expolygon, b.expolygon) && a.src_id == b.src_id && a.boundary_id == b.boundary_id; }

// ---------------------------------------------------------------- SFBR

struct BrCase
{
    std::string label;
    Surfaces    surfaces;
    Zones       zones; // shells, sparse infill, tops, as process_external_surfaces makes them
    float       closing_radius;
    double      bridge_angle;
};

ExPolygons of_type(const Surfaces &ss, Slic3r::SurfaceType t)
{
    ExPolygons out;
    for (const Surface &s : ss)
        if (s.surface_type == t)
            out.push_back(s.expolygon);
    return out;
}

// The flows of a 0.4 mm nozzle at 0.2 mm layers: the external perimeter, the perimeter and the
// solid infill spacings, as process_external_surfaces turns them into expansions.
struct Flows
{
    float ext_width = 420000.f, ext_spacing = 377080.f, spacing = 407080.f, solid_spacing = 407080.f;
    int   walls     = 2;
};

BrCase make_case(const std::string &label, const Surfaces &ss, const Flows &f, double angle)
{
    float shell_width = 0, expansion_min = 0;
    if (f.walls > 0) {
        shell_width = 0.5f * f.ext_width + f.ext_spacing;
        shell_width += f.spacing * (f.walls - 1);
        expansion_min = f.spacing;
    } else {
        shell_width   = 100.f;
        expansion_min = 100.f;
    }
    const float expansion_bridge = float(shell_width * std::sqrt(2.));
    const float step             = Slic3r::scaled<float>(0.1);
    const auto  into_sparse      = A::RegionExpansionParameters::build(expansion_min, step, 5);
    const auto  into_solid       = A::RegionExpansionParameters::build(expansion_bridge, step, 5);
    auto        un               = [&](Slic3r::SurfaceType t) { return Slic3r::union_ex(of_type(ss, t)); };
    BrCase      c;
    c.label          = label;
    c.surfaces       = ss;
    c.zones          = Zones{ExpansionZone{un(Slic3r::stInternalSolid), into_solid}, ExpansionZone{un(Slic3r::stInternal), into_sparse}, ExpansionZone{un(Slic3r::stTop), into_solid}};
    c.closing_radius = 0.55f * 0.65f * 1.05f * f.solid_spacing;
    c.bridge_angle   = angle;
    return c;
}

Surfaces typed(std::initializer_list<std::pair<Slic3r::SurfaceType, ExPolygon>> parts)
{
    Surfaces out;
    for (const auto &[t, ex] : parts) {
        Surface s(t, ex);
        s.thickness = 0.2;
        out.push_back(s);
    }
    return out;
}

std::vector<BrCase> br_hand()
{
    using namespace Slic3r;
    const Flows         f;
    std::vector<BrCase> out;
    const ExPolygon     hole_ring = [] {
        ExPolygon e(rect(-10, -10, 20, 20));
        e.holes.push_back(reversed(rect(0, 0, 10, 10)));
        return e;
    }();
    out.push_back(make_case("a bridge between two shells", typed({{stTop, ExPolygon(rect(0, -10, 30, 0))}, {stBottomBridge, ExPolygon(rect(10, 0, 20, 20))}, {stInternalSolid, ExPolygon(rect(0, 0, 10, 20))}, {stInternalSolid, ExPolygon(rect(20, 0, 30, 20))}, {stInternal, ExPolygon(rect(0, 20, 30, 30))}}), f, 0.5));
    out.push_back(make_case("two bridges across a narrow shell (one group)", typed({{stBottomBridge, ExPolygon(rect(10, 0, 14, 20))}, {stBottomBridge, ExPolygon(rect(16, 0, 20, 20))}, {stInternalSolid, ExPolygon(rect(14, 0, 16, 20))}, {stInternalSolid, ExPolygon(rect(0, 0, 10, 20))}, {stInternalSolid, ExPolygon(rect(20, 0, 30, 20))}}), f, 1.0));
    out.push_back(make_case("two bridges far apart (two groups)", typed({{stBottomBridge, ExPolygon(rect(10, 0, 14, 20))}, {stBottomBridge, ExPolygon(rect(40, 0, 44, 20))}, {stInternalSolid, ExPolygon(rect(0, 0, 10, 20))}, {stInternalSolid, ExPolygon(rect(44, 0, 54, 20))}, {stInternal, ExPolygon(rect(14, 0, 40, 20))}}), f, 0.));
    out.push_back(make_case("a bridge in a hole of a shell", typed({{stBottomBridge, ExPolygon(rect(0, 0, 10, 10))}, {stInternalSolid, hole_ring}}), f, 2.0));
    out.push_back(make_case("an isolated bridge", typed({{stBottomBridge, ExPolygon(rect(0, 0, 10, 4))}, {stInternalSolid, ExPolygon(rect(30, 0, 40, 10))}}), f, 0.25));
    out.push_back(make_case("an isolated square bridge", typed({{stBottomBridge, ExPolygon(rect(0, 0, 10, 10))}}), f, 0.25));
    out.push_back(make_case("a bridge touching shells, sparse infill and tops", typed({{stTop, ExPolygon(rect(0, 20, 30, 30))}, {stBottomBridge, ExPolygon(rect(10, 10, 20, 20))}, {stInternalSolid, ExPolygon(rect(0, 10, 10, 20))}, {stInternal, ExPolygon(rect(20, 10, 30, 20))}, {stInternal, ExPolygon(rect(0, 0, 30, 10))}}), f, 3.0));
    out.push_back(make_case("a bridge over sparse infill only", typed({{stBottomBridge, ExPolygon(rect(10, 10, 20, 20))}, {stInternal, hole_ring}}), f, 0.));
    out.push_back(make_case("no bridge, bottoms and tops", typed({{stBottom, ExPolygon(rect(0, 0, 10, 10))}, {stTop, ExPolygon(rect(10, 0, 20, 10))}, {stInternalSolid, ExPolygon(rect(0, 10, 20, 15))}, {stInternal, ExPolygon(rect(0, 15, 20, 30))}}), f, 0.));
    out.push_back(make_case("an L-shaped bridge anchored on two sides", typed({{stBottomBridge, ExPolygon(Polygon(Points{Point(mm(10), 0), Point(mm(20), 0), Point(mm(20), mm(20)), Point(0, mm(20)), Point(0, mm(10)), Point(mm(10), mm(10))}))}, {stInternalSolid, ExPolygon(rect(0, 0, 10, 10))}, {stInternalSolid, ExPolygon(rect(20, 0, 30, 30))}, {stInternal, ExPolygon(rect(0, 20, 20, 30))}}), f, 1.5));
    out.push_back(make_case("a bridge with a hole over a shell island", typed({{stBottomBridge, [] { ExPolygon e(rect(0, 0, 30, 30)); e.holes.push_back(reversed(rect(10, 10, 20, 20))); return e; }()}, {stInternalSolid, ExPolygon(rect(10, 10, 20, 20))}, {stInternal, [] { ExPolygon e(rect(-10, -10, 40, 40)); e.holes.push_back(reversed(rect(0, 0, 30, 30))); return e; }()}}), f, 0.75));
    out.push_back(make_case("three bridges chained by overlapping expansions", typed({{stBottomBridge, ExPolygon(rect(0, 0, 5, 20))}, {stBottomBridge, ExPolygon(rect(6, 0, 11, 20))}, {stBottomBridge, ExPolygon(rect(12, 0, 17, 20))}, {stInternalSolid, ExPolygon(rect(5, 0, 6, 20))}, {stInternalSolid, ExPolygon(rect(11, 0, 12, 20))}, {stInternalSolid, ExPolygon(rect(-5, 0, 0, 20))}, {stInternalSolid, ExPolygon(rect(17, 0, 22, 20))}}), f, 0.1));
    out.push_back(make_case("a bottom next to a shell, no walls", typed({{stBottom, ExPolygon(rect(0, 0, 10, 10))}, {stBottomBridge, ExPolygon(rect(10, 0, 20, 10))}, {stInternalSolid, ExPolygon(rect(0, 10, 20, 20))}}), Flows{420000.f, 377080.f, 407080.f, 407080.f, 0}, 0.));
    Flows wide;
    wide.walls = 5;
    out.push_back(make_case("a bridge between two shells, five walls", out[0].surfaces, wide, 0.5));
    return out;
}

void br_random(std::vector<BrCase> &out, int count)
{
    using namespace Slic3r;
    SplitMix g(0x5F3E0008u);
    for (int i = 0; i < count; ++i) {
        const ExPolygons r = region(g, 5, 40.), q1 = region(g, 3, 40.), q2 = region(g, 3, 40.);
        const ExPolygons bridges = intersection_ex(r, q1);
        const ExPolygons rest1   = diff_ex(r, q1);
        const ExPolygons tops    = intersection_ex(rest1, q2);
        const ExPolygons rest2   = diff_ex(rest1, q2);
        Polygons         outer   = to_polygons(bridges);
        polygons_append(outer, to_polygons(tops));
        const Polygons   near   = offset(outer, float(mm(uniform(g, 0.5, 4.))));
        const ExPolygons solid  = intersection_ex(rest2, near);
        const ExPolygons sparse = diff_ex(rest2, near);
        Surfaces         ss;
        auto             app = [&](const ExPolygons &exs, SurfaceType t) {
            for (const ExPolygon &e : exs) {
                Surface s(t, e);
                s.thickness = 0.2;
                ss.push_back(s);
            }
        };
        app(tops, stTop);
        app(bridges, g.below(5) == 0 ? stBottom : stBottomBridge);
        app(solid, stInternalSolid);
        app(sparse, stInternal);
        Flows f;
        f.walls = int(g.below(6));
        if (g.below(3) == 0)
            f.solid_spacing = float(uniform(g, 2e5, 8e5));
        out.push_back(make_case("seeded", ss, f, uniform(g, 0., 6.283185307179586)));
    }
}

Payload sfbr_case(const BrCase &c)
{
    using namespace Slic3r;
    Payload p;
    p.str(c.label);
    put(p, c.surfaces);
    p.u64(c.zones.size());
    for (const ExpansionZone &z : c.zones) {
        put(p, z.expolygons);
        put(p, z.parameters);
    }
    p.f32(c.closing_radius).f64(c.bridge_angle);
    // The steps of expand_bridges_detect_orientations, in its order.
    const ExPolygons bridge_expolygons = of_type(c.surfaces, stBottomBridge);
    p.boolean(!bridge_expolygons.empty());
    Surfaces merged;
    if (!bridge_expolygons.empty()) {
        Zones           zones = c.zones;
        ExpansionResult res   = Slic3r::expand_expolygons(bridge_expolygons, zones);
        put(p, res.anchors);
        put(p, res.expansions);
        for (const ExpansionZone &z : zones)
            p.boolean(z.expanded_into);
        std::vector<Bridge> bridges = get_grouped_bridges(ExPolygons(bridge_expolygons), res.expansions);
        put_groups(p, bridges);
        // The anchors sorted by source and boundary: the permutation, checked against std::sort.
        const std::vector<uint32_t> aperm = sort_permutation(res.anchors, A::lower_by_src_and_boundary);
        A::WaveSeeds                anchors = res.anchors;
        std::sort(anchors.begin(), anchors.end(), A::lower_by_src_and_boundary);
        bool asame = true;
        for (size_t i = 0; i < anchors.size(); ++i)
            asame = asame && same_seed(anchors[i], res.anchors[aperm[i]]);
        if (!asame)
            throw std::runtime_error("SFBR: std::sort of the anchors is not the permutation of their indices");
        p.u32s(aperm);
        detect_bridge_directions(anchors, bridges, zones);
        put_angles(p, bridges);
        // The expansions sorted by source and boundary, in place (the bridges hold their end()).
        auto                        less  = [](const A::RegionExpansionEx &l, const A::RegionExpansionEx &r) { return l.src_id < r.src_id || (l.src_id == r.src_id && l.boundary_id < r.boundary_id); };
        const std::vector<uint32_t> eperm = sort_permutation(res.expansions, less);
        const auto                  before = res.expansions;
        std::sort(res.expansions.begin(), res.expansions.end(), less);
        bool esame = true;
        for (size_t i = 0; i < res.expansions.size(); ++i)
            esame = esame && same_ex(res.expansions[i], before[eperm[i]]);
        if (!esame)
            throw std::runtime_error("SFBR: std::sort of the expansions is not the permutation of their indices");
        p.u32s(eperm);
        merged = merge_bridges(bridges, res.expansions, c.closing_radius);
        put(p, merged);
    }
    {
        Surfaces       ss    = c.surfaces;
        Zones          zones = c.zones;
        const Surfaces out   = expand_bridges_detect_orientations(ss, zones, c.closing_radius);
        p.boolean(same(out, merged));
        if (!same(out, merged))
            put(p, out);
        put_zones_after(p, zones);
        put_moved(p, ss);
    }
    // expand_merge_surfaces: the bottom bridges at the given angle into all the zones, then the
    // bottoms and the tops into the zones but the last (the shells and the sparse infill).
    {
        Surfaces       ss    = c.surfaces;
        Zones          zones = c.zones;
        const Surfaces out   = expand_merge_surfaces(ss, stBottomBridge, zones, c.closing_radius, c.bridge_angle);
        put(p, out);
        put_zones_after(p, zones);
        put_moved(p, ss);
    }
    for (SurfaceType t : {stBottom, stTop}) {
        Surfaces ss    = c.surfaces;
        Zones    zones = c.zones;
        zones.pop_back();
        const Surfaces out = expand_merge_surfaces(ss, t, zones, c.closing_radius, -1);
        put(p, out);
        put_zones_after(p, zones);
        put_moved(p, ss);
    }
    return p;
}

void record_sfbr(Sets &sets, const std::vector<Layer> &layers)
{
    std::vector<BrCase> cases = br_hand();
    for (const Layer &l : layers)
        cases.push_back(make_case(l.name, l.surfaces, Flows{}, 0.5));
    br_random(cases, 300);
    Picker pick{30, 12000};
    for (const BrCase &c : cases)
        add(sets, "SFBR", sfbr_case(c), pick);
}

// ---------------------------------------------------------------- SFBF

// u8 0: get_grouped_bridges. The bridges' expolygons, the expansions (expolygon, u32 src id, u32
// boundary id), then the raw and the resolved group ids.
void bf_group(Sets &sets, Picker &pick, const ExPolygons &bridges, const std::vector<A::RegionExpansionEx> &expansions)
{
    Payload p;
    p.u8(0);
    put(p, bridges);
    put(p, expansions);
    put_groups(p, Slic3r::get_grouped_bridges(ExPolygons(bridges), expansions));
    add(sets, "SFBF", p, pick);
}

// u8 1: detect_bridge_directions. The anchors (wave seeds), the bridges' expolygons, the zones'
// expolygons; then u8 1 and the angles, or u8 2 and the exception's what().
void bf_detect(Sets &sets, Picker &pick, const A::WaveSeeds &anchors, const ExPolygons &bridges, const std::vector<ExPolygons> &zone_exs)
{
    Payload p;
    p.u8(1);
    put(p, anchors);
    put(p, bridges);
    p.u64(zone_exs.size());
    for (const ExPolygons &z : zone_exs)
        put(p, z);
    std::vector<Bridge> bs;
    for (const ExPolygon &e : bridges)
        bs.push_back({e, uint32_t(bs.size()), {}});
    Zones zones;
    for (const ExPolygons &z : zone_exs)
        zones.push_back(ExpansionZone{z, A::RegionExpansionParameters::build(1e6f, 1e5f, 5)});
    try {
        Slic3r::detect_bridge_directions(anchors, bs, zones);
        p.u8(1);
        put_angles(p, bs);
    } catch (const std::exception &e) {
        p.u8(2).str(e.what());
    }
    add(sets, "SFBF", p, pick);
}

// u8 2: merge_bridges. The bridges (expolygon, u32 group id, u8 has an angle and its f64), the
// expansions sorted by source id, f32 closing radius; then the surfaces.
void bf_merge(Sets &sets, Picker &pick, const ExPolygons &bridges, const std::vector<uint32_t> &groups, const std::vector<std::optional<double>> &angles,
              const std::vector<A::RegionExpansionEx> &expansions, float closing)
{
    Payload p;
    p.u8(2);
    p.u64(bridges.size());
    for (size_t i = 0; i < bridges.size(); ++i) {
        put(p, bridges[i]);
        p.u32(groups[i]).boolean(angles[i].has_value());
        if (angles[i])
            p.f64(*angles[i]);
    }
    put(p, expansions);
    p.f32(closing);
    std::vector<Bridge> bs;
    for (size_t i = 0; i < bridges.size(); ++i)
        bs.push_back({bridges[i], groups[i], expansions.end(), angles[i]});
    put(p, Slic3r::merge_bridges(bs, expansions, closing));
    add(sets, "SFBF", p, pick);
}

// A bridge's expansion: a rectangle on one of its sides, `d` mm deep.
A::RegionExpansionEx side(const Polygon &b, int which, double d, uint32_t src, uint32_t boundary)
{
    const Slic3r::BoundingBox bb = b.bounding_box();
    const double              x0 = bb.min.x() * 1e-6, y0 = bb.min.y() * 1e-6, x1 = bb.max.x() * 1e-6, y1 = bb.max.y() * 1e-6;
    Polygon                   r;
    switch (which & 3) {
    case 0: r = rect(x1 - 0.01, y0, x1 + d, y1); break;
    case 1: r = rect(x0 - d, y0, x0 + 0.01, y1); break;
    case 2: r = rect(x0, y1 - 0.01, x1, y1 + d); break;
    default: r = rect(x0, y0 - d, x1, y0 + 0.01); break;
    }
    return {ExPolygon(r), src, boundary};
}

void record_sfbf(Sets &sets)
{
    Picker   pick{70, 2500};
    SplitMix g(0x5F3E0009u);
    using RX = A::RegionExpansionEx;
    const ExPolygons three{ExPolygon(rect(0, 0, 4, 10)), ExPolygon(rect(6, 0, 10, 10)), ExPolygon(rect(30, 0, 34, 10))};
    // Grouping: none, two whose expansions overlap in one boundary, the same in two boundaries
    // (no group), a chain of three, expansions out of order.
    bf_group(sets, pick, three, {});
    bf_group(sets, pick, three, {RX{ExPolygon(rect(3.9, 0, 6.1, 10)), 0, 0}, RX{ExPolygon(rect(4.5, 0, 6.1, 10)), 1, 0}});
    bf_group(sets, pick, three, {RX{ExPolygon(rect(3.9, 0, 6.1, 10)), 0, 0}, RX{ExPolygon(rect(4.5, 0, 6.1, 10)), 1, 1}});
    bf_group(sets, pick, three, {RX{ExPolygon(rect(3.9, 0, 6.1, 10)), 0, 0}, RX{ExPolygon(rect(4, 0, 32, 10)), 1, 0}, RX{ExPolygon(rect(29, 0, 31, 10)), 2, 0}});
    bf_group(sets, pick, three, {RX{ExPolygon(rect(29, 0, 31, 10)), 2, 0}, RX{ExPolygon(rect(3.9, 0, 6.1, 10)), 1, 0}, RX{ExPolygon(rect(3, 0, 5, 10)), 0, 0}});
    bf_group(sets, pick, three, {RX{ExPolygon(rect(3.9, 0, 6.1, 10)), 0, 0}, RX{ExPolygon(rect(3.9, 0, 6.1, 10)), 0, 0}});
    // Bounding boxes that overlap with contours that do not.
    bf_group(sets, pick, three, {RX{ExPolygon(Polygon(Points{Point(0, 0), Point(mm(10), 0), Point(0, mm(10))})), 0, 0}, RX{ExPolygon(Polygon(Points{Point(mm(10), mm(1)), Point(mm(10), mm(10)), Point(mm(1), mm(10))})), 1, 0}});
    // Orientation: no zone (it throws), no anchors (all edges float), anchors on one side, on two
    // opposite sides, in a second zone, with a boundary id past every zone.
    const ExPolygons one{ExPolygon(rect(0, 0, 10, 4))};
    auto             seed = [](uint32_t src, uint32_t boundary, Points path) { return A::WaveSeed{src, boundary, std::move(path)}; };
    bf_detect(sets, pick, {}, one, {});
    bf_detect(sets, pick, {}, one, {ExPolygons{ExPolygon(rect(20, 0, 30, 4))}});
    bf_detect(sets, pick, {}, {ExPolygon(rect(0, 0, 10, 10))}, {ExPolygons{}});
    bf_detect(sets, pick, {}, {ExPolygon(Polygon(Points{Point(0, 0), Point(3, 0), Point(0, 3)}))}, {ExPolygons{}});
    bf_detect(sets, pick, {seed(0, 0, {Point(mm(10), 0), Point(mm(10), mm(4))})}, one, {ExPolygons{ExPolygon(rect(10, 0, 20, 4))}});
    bf_detect(sets, pick, {seed(0, 0, {Point(mm(10), 0), Point(mm(10), mm(4))}), seed(0, 1, {Point(0, 0), Point(0, mm(4))})}, one, {ExPolygons{ExPolygon(rect(10, 0, 20, 4))}, ExPolygons{ExPolygon(rect(-10, 0, 0, 4))}});
    bf_detect(sets, pick, {seed(0, 1, {Point(0, 0), Point(0, mm(4))})}, one, {ExPolygons{ExPolygon(rect(10, 0, 20, 4))}, ExPolygons{ExPolygon(rect(-10, 0, 0, 4))}});
    bf_detect(sets, pick, {seed(0, 7, {Point(0, 0), Point(0, mm(4))})}, one, {ExPolygons{ExPolygon(rect(10, 0, 20, 4))}, ExPolygons{ExPolygon(rect(-10, 0, 0, 4))}});
    bf_detect(sets, pick, {seed(1, 0, {Point(mm(10), 0), Point(mm(10), mm(4))})}, {ExPolygon(rect(0, 0, 10, 4)), ExPolygon(rect(10, 0, 20, 4))}, {ExPolygons{ExPolygon(rect(10, 0, 20, 4))}});
    // Merging: one group of two with an angle, an unset angle (-1), a bridge with no expansion.
    bf_merge(sets, pick, three, {0, 0, 2}, {0.5, 1.0, std::nullopt}, {RX{ExPolygon(rect(3.9, 0, 6.1, 10)), 0, 0}, RX{ExPolygon(rect(4.5, 0, 6.1, 10)), 1, 0}, RX{ExPolygon(rect(29, 0, 31, 10)), 2, 0}}, 140000.f);
    bf_merge(sets, pick, three, {0, 1, 2}, {std::nullopt, 2.0, 3.0}, {RX{ExPolygon(rect(29, 0, 31, 10)), 2, 0}}, 140000.f);
    bf_merge(sets, pick, three, {0, 0, 1}, {0.25, std::nullopt, 0.75}, {RX{ExPolygon(rect(-2, 0, 0.1, 10)), 0, 1}, RX{ExPolygon(rect(3.9, 0, 6.1, 10)), 0, 2}, RX{ExPolygon(rect(9.9, 0, 12, 10)), 1, 0}}, 1.f);
    // Seeded: two to six bridges in a row, expansions on their sides that overlap their
    // neighbours' now and then, anchors from the expansions' sides.
    for (int i = 0; i < 600; ++i) {
        const int  n = 1 + int(g.below(6));
        ExPolygons bridges;
        double     x = 0;
        for (int j = 0; j < n; ++j) {
            const double w = uniform(g, 1., 8.), h = uniform(g, 2., 12.);
            bridges.push_back(g.below(4) == 0 ? ExPolygon(star(g, 3 + int(g.below(8)), x + w / 2, h / 2, std::min(w, h) / 2)) : ExPolygon(rect(x, 0, x + w, h)));
            x += w + uniform(g, -0.5, 3.);
        }
        std::vector<RX> ex;
        for (uint32_t j = 0; j < uint32_t(n); ++j) {
            const int k = int(g.below(4));
            for (int s = 0; s < k; ++s)
                ex.push_back(side(bridges[j].contour, int(g.below(4)), uniform(g, 0.2, 4.), j, uint32_t(g.below(3))));
        }
        switch (g.below(3)) {
        case 0: {
            std::sort(ex.begin(), ex.end(), [](const RX &l, const RX &r) { return l.boundary_id < r.boundary_id || (l.boundary_id == r.boundary_id && l.src_id < r.src_id); });
            bf_group(sets, pick, bridges, ex);
            break;
        }
        case 1: {
            A::WaveSeeds            anchors;
            std::vector<ExPolygons> zones(1 + g.below(3));
            for (const RX &r : ex) {
                const uint32_t zi = uint32_t(g.below(zones.size()));
                zones[zi].push_back(r.expolygon);
                uint32_t boundary = 0;
                for (uint32_t z = 0; z < zi; ++z)
                    boundary += uint32_t(zones[z].size());
                boundary += uint32_t(zones[zi].size() - 1);
                const Points &c = r.expolygon.contour.points;
                anchors.push_back(A::WaveSeed{r.src_id, boundary, Points{c[0], c[1], c[2]}});
            }
            std::sort(anchors.begin(), anchors.end(), A::lower_by_src_and_boundary);
            bf_detect(sets, pick, anchors, bridges, zones);
            break;
        }
        default: {
            std::sort(ex.begin(), ex.end(), [](const RX &l, const RX &r) { return l.src_id < r.src_id || (l.src_id == r.src_id && l.boundary_id < r.boundary_id); });
            std::vector<uint32_t>              groups(static_cast<size_t>(n));
            std::vector<std::optional<double>> angles(static_cast<size_t>(n));
            for (uint32_t j = 0; j < uint32_t(n); ++j) {
                groups[j] = j == 0 || g.below(2) ? j : uint32_t(g.below(j));
                if (g.below(5))
                    angles[j] = uniform(g, 0., 6.3);
            }
            bf_merge(sets, pick, bridges, groups, angles, ex, float(uniform(g, 1., 4e5)));
        }
        }
    }
}

} // namespace

void record_bridges(Sets &sets, const std::vector<surfaces::Layer> &layers)
{
    record_sfbr(sets, layers);
    record_sfbf(sets);
}

} // namespace probe
