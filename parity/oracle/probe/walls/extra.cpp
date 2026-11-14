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
// walls.sop1's records of the extra perimeters over overhangs, which walls-1 carries
// (parity/sop1/walls.md), recorded by orca-o3-probe-walls (2026-11-13). PerimeterGenerator.cpp
// defines these four with external linkage and no header declares them, so the probe declares
// them below and calls them as apply_extra_perimeters does:
//
//   WLRC  reconnect_polylines
//   WLPT  paths_touch
//   WLSX  sort_extra_perimeters
//   WLXP  generate_extra_perimeters_over_overhangs
//
// Carries OrcaSlicer code: the four declarations below repeat the signatures of
// src/libslic3r/PerimeterGenerator.cpp (OrcaSlicer 2.4.2), so their copyright is the holders' of
// that file, the OrcaSlicer contributors with Bambu Lab, Prusa Research and the Slic3r authors,
// under AGPL-3.0; taken 2026-11-13.
#include "values.hpp"

#include <cmath>
#include <exception>
#include <string>
#include <tuple>
#include <vector>

#include "libslic3r/ClipperUtils.hpp"

namespace Slic3r {
bool           paths_touch(const ExtrusionPath &path_one, const ExtrusionPath &path_two, double limit_distance);
Polylines      reconnect_polylines(const Polylines &polylines, double limit_distance);
ExtrusionPaths sort_extra_perimeters(const ExtrusionPaths &extra_perims, int index_of_first_unanchored, double extrusion_spacing);
std::tuple<std::vector<ExtrusionPaths>, Polygons> generate_extra_perimeters_over_overhangs(ExPolygons               infill_area,
                                                                                           const Polygons          &lower_slices_polygons,
                                                                                           int                      perimeter_count,
                                                                                           const Flow              &overhang_flow,
                                                                                           double                   scaled_resolution,
                                                                                           const PrintObjectConfig &object_config,
                                                                                           const PrintConfig       &print_config);
} // namespace Slic3r

namespace probe::walls {

using namespace Slic3r;

namespace {

constexpr double PI_ = 3.14159265358979323846;

coord_t sc(double mm) { return coord_t(std::llround(mm * 1000000.)); }

Polyline line_of(std::initializer_list<std::pair<double, double>> mm)
{
    Polyline out;
    for (const auto &xy : mm)
        out.points.emplace_back(sc(xy.first), sc(xy.second));
    return out;
}

Polyline random_polyline(SplitMix &g, double cx, double cy, int n)
{
    Polyline out;
    double x = cx, y = cy;
    for (int i = 0; i < n; ++i) {
        out.points.emplace_back(sc(x), sc(y));
        x += real(g, -4, 4);
        y += real(g, -4, 4);
    }
    return out;
}

ExtrusionPath path_of(const Polyline &pl, const Flow &flow)
{
    ExtrusionPath path(erOverhangPerimeter, flow.mm3_per_mm(), flow.width(), flow.height());
    path.polyline = Polyline3(pl);
    return path;
}

// ---- WLRC ----------------------------------------------------------------------------------

void reconnect_case(Writer &w, const Polylines &pls, double limit, bool committed)
{
    Payload p;
    put_polylines(p, pls);
    p.f64(limit);
    put_polylines(p, reconnect_polylines(pls, limit));
    w.add("WLRC", p, committed);
}

// ---- WLPT ----------------------------------------------------------------------------------

void touch_case(Writer &w, const ExtrusionPath &a, const ExtrusionPath &b, double limit, bool committed)
{
    Payload p;
    put_entity(p, a);
    put_entity(p, b);
    p.f64(limit);
    p.boolean(paths_touch(a, b, limit));
    w.add("WLPT", p, committed);
}

// ---- WLSX ----------------------------------------------------------------------------------

void sort_case(Writer &w, const ExtrusionPaths &paths, int first_unanchored, double spacing, bool committed)
{
    Payload p;
    put_paths(p, paths);
    p.i32(first_unanchored).f64(spacing);
    put_paths(p, sort_extra_perimeters(paths, first_unanchored, spacing));
    w.add("WLSX", p, committed);
}

// ---- WLXP ----------------------------------------------------------------------------------

void extra_case(Writer &w, const ExPolygons &infill, const Polygons &lower, int count, const Flow &flow, double resolution, bool committed)
{
    Payload p;
    put_expolygons(p, infill);
    put_polygons(p, lower);
    p.i32(count);
    put_flow(p, flow);
    p.f64(resolution);
    try {
        Configs cfg;
        make_configs({}, cfg);
        auto [perims, filled] = generate_extra_perimeters_over_overhangs(infill, lower, count, flow, resolution, cfg.object, cfg.print);
        p.u8(1);
        p.u64(perims.size());
        for (const ExtrusionPaths &region : perims)
            put_paths(p, region);
        put_polygons(p, filled);
    } catch (const std::exception &e) {
        p.u8(2).str(e.what());
    }
    w.add("WLXP", p, committed);
}

// The lower slices as process_classic and process_arachne grow them: by half the nozzle.
Polygons grown(const ExPolygons &lower, double nozzle = 0.4) { return offset(lower, float(scale_(nozzle / 2))); }

} // namespace

void record_reconnect(Writer &w)
{
    const double lim = double(sc(0.45));
    reconnect_case(w, {}, lim, true);
    reconnect_case(w, {line_of({{0, 0}, {5, 0}})}, lim, true);
    // Each of the four joins: last to first, last to last, first to last, first to first.
    reconnect_case(w, {line_of({{0, 0}, {5, 0}}), line_of({{5.1, 0}, {9, 3}})}, lim, true);
    reconnect_case(w, {line_of({{0, 0}, {5, 0}}), line_of({{9, 3}, {5.1, 0.1}})}, lim, true);
    reconnect_case(w, {line_of({{0, 0}, {5, 0}}), line_of({{-4, 2}, {0, 0.2}})}, lim, true);
    reconnect_case(w, {line_of({{0, 0}, {5, 0}}), line_of({{0.2, 0.1}, {-4, 2}})}, lim, true);
    // A chain, a closed ring, an empty polyline among them, nothing close.
    reconnect_case(w, {line_of({{0, 0}, {2, 0}}), line_of({{4, 0}, {6, 0}}), line_of({{2.1, 0}, {3.9, 0}}), line_of({{6.2, 0}, {6, 4}})}, lim, true);
    reconnect_case(w, {line_of({{0, 0}, {4, 0}, {4, 4}}), line_of({{4, 4.1}, {0, 4}, {0, 0.1}})}, lim, true);
    reconnect_case(w, {line_of({{0, 0}, {2, 0}}), Polyline(), line_of({{2, 0.1}, {3, 3}})}, lim, true);
    reconnect_case(w, {line_of({{0, 0}, {2, 0}}), line_of({{10, 10}, {12, 10}}), line_of({{20, 0}, {22, 0}})}, lim, true);
    reconnect_case(w, {line_of({{0, 0}, {2, 0}}), line_of({{2, 0}, {3, 0}})}, 0., true);
    SplitMix g(0x3A11E006ull);
    for (int i = 0; i < 120; ++i) {
        Polylines pls;
        const int n = 1 + int(g.below(7));
        for (int k = 0; k < n; ++k) {
            Polyline pl = random_polyline(g, real(g, -8, 8), real(g, -8, 8), 2 + int(g.below(4)));
            // Now and then start or end it next to an earlier one's end.
            if (!pls.empty() && g.below(2) == 0) {
                const Polyline &o = pls[g.below(pls.size())];
                if (!o.empty()) {
                    const Point near = (g.below(2) == 0 ? o.first_point() : o.last_point()) + Point(coord_t(g.below(300000)), coord_t(g.below(300000)));
                    if (g.below(2) == 0)
                        pl.points.front() = near;
                    else
                        pl.points.back() = near;
                }
            }
            pls.push_back(pl);
        }
        reconnect_case(w, pls, double(sc(real(g, 0.1, 0.6))), i < 20);
    }
}

void record_touch(Writer &w)
{
    const Flow f(0.45f, 0.2f, 0.4f);
    touch_case(w, path_of(line_of({{0, 0}, {5, 0}}), f), path_of(line_of({{0, 0.3}, {5, 0.3}}), f), double(sc(0.45)), true);
    touch_case(w, path_of(line_of({{0, 0}, {5, 0}}), f), path_of(line_of({{0, 1}, {5, 1}}), f), double(sc(0.45)), true);
    touch_case(w, path_of(line_of({{0, 0}, {5, 0}}), f), path_of(line_of({{2, -3}, {2.2, 3}}), f), double(sc(0.1)), true);
    touch_case(w, path_of(line_of({{0, 0}, {10, 0}}), f), path_of(line_of({{4, 0.4}, {6, 0.4}}), f), double(sc(0.45)), true);
    touch_case(w, path_of(line_of({{4, 0.4}, {6, 0.4}}), f), path_of(line_of({{0, 0}, {10, 0}}), f), double(sc(0.45)), true);
    SplitMix g(0x3A11E007ull);
    for (int i = 0; i < 100; ++i) {
        const Polyline a = random_polyline(g, 0, 0, 2 + int(g.below(5)));
        const Polyline b = random_polyline(g, real(g, -3, 3), real(g, -3, 3), 2 + int(g.below(5)));
        touch_case(w, path_of(a, f), path_of(b, f), double(sc(real(g, 0.05, 1.5))), i < 24);
    }
}

void record_sort_extra(Writer &w)
{
    const Flow   f(0.45f, 0.2f, 0.4f);
    const double sp = double(f.scaled_spacing());
    sort_case(w, {}, 0, sp, true);
    {
        // Three parallel lines, the first anchored; then the same, none anchored and all anchored.
        ExtrusionPaths ps = {path_of(line_of({{0, 0}, {10, 0}}), f), path_of(line_of({{0, 0.4}, {10, 0.4}}), f), path_of(line_of({{10, 0.8}, {0, 0.8}}), f)};
        sort_case(w, ps, 1, sp, true);
        sort_case(w, ps, 0, sp, true);
        sort_case(w, ps, 3, sp, true);
    }
    {
        // Short paths that the filter drops, ends close enough to join, a far path past 5 mm.
        ExtrusionPaths ps = {path_of(line_of({{0, 0}, {0.5, 0}}), f), path_of(line_of({{0.6, 0}, {6, 0}}), f), path_of(line_of({{30, 0}, {40, 0}}), f),
                             path_of(line_of({{6, 0.3}, {0, 0.3}}), f)};
        sort_case(w, ps, 1, sp, true);
        sort_case(w, ps, 2, sp, true);
    }
    SplitMix g(0x3A11E008ull);
    for (int i = 0; i < 80; ++i) {
        ExtrusionPaths ps;
        const int n = 1 + int(g.below(8));
        double y = 0;
        for (int k = 0; k < n; ++k) {
            y += real(g, 0.2, 0.9);
            const double x0 = real(g, -2, 2), x1 = real(g, 3, 14);
            ps.push_back(path_of(g.below(2) == 0 ? line_of({{x0, y}, {x1, y}}) : line_of({{x1, y}, {x0, y}, {x0, y + 0.2}}), f));
            if (g.below(6) == 0)
                ps.push_back(path_of(random_polyline(g, real(g, -5, 5), y, 3), f));
        }
        sort_case(w, ps, int(g.below(uint64_t(ps.size()) + 1)), sp, i < 14);
    }
}

void record_extra_perimeters(Writer &w)
{
    const Flow bridge = Flow::bridging_flow(0.4f, 0.4f);
    const Flow normal = Flow(0.45f, 0.2f, 0.4f).with_flow_ratio(1.0);
    const double res  = scale_(0.01);
    const ExPolygons square = {ExPolygon(rect(0, 0, 20, 20))};
    // No overhang; a strip over the air on one side; two pads (a bridge); a corner only.
    extra_case(w, square, grown(square), 2, bridge, res, true);
    extra_case(w, square, grown({ExPolygon(rect(0, 0, 20, 12))}), 2, bridge, res, true);
    extra_case(w, square, grown({ExPolygon(rect(0, 0, 20, 12))}), 1, normal, res, true);
    extra_case(w, square, grown({ExPolygon(rect(0, 0, 20, 12))}), 3, bridge, res, true);
    extra_case(w, square, grown({ExPolygon(rect(-1, -1, 3, 21)), ExPolygon(rect(17, -1, 21, 21))}), 2, bridge, res, true);
    extra_case(w, square, grown({ExPolygon(rect(0, 0, 6, 6))}), 2, bridge, res, true);
    // A narrow overhang (the gap fill of a single width), a round island half over the air.
    extra_case(w, square, grown({ExPolygon(rect(0, 0, 20, 18.8))}), 2, bridge, res, true);
    extra_case(w, {ExPolygon(circle(0, 0, 8, 64))}, grown({ExPolygon(rect(-10, -10, 10, 1))}), 2, bridge, res, true);
    extra_case(w, {with_holes(rect(0, 0, 20, 20), {rect(8, 8, 12, 12)})}, grown({ExPolygon(rect(0, 0, 20, 10))}), 2, bridge, res, true);
    extra_case(w, square, {}, 2, bridge, res, true);
    SplitMix g(0x3A11E009ull);
    for (int i = 0; i < 30; ++i) {
        const double a = real(g, 0, 2 * PI_);
        ExPolygons infill = {rotated(ExPolygon(rect(0, 0, real(g, 5, 20), real(g, 5, 20))), a)};
        ExPolygons lower  = {rotated(ExPolygon(rect(-1, -1, real(g, 2, 15), real(g, 2, 22))), a)};
        if (g.below(3) == 0)
            lower.push_back(rotated(ExPolygon(rect(real(g, 3, 18), -1, 22, 22)), a));
        extra_case(w, infill, grown(lower), 1 + int(g.below(3)), g.below(2) == 0 ? bridge : normal, res, i < 1);
    }
}

} // namespace probe::walls
