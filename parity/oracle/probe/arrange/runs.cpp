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
// arrange.sop1's ARNG records (parity/SOP1.md, "arrange.sop1"), which orca-arrange's
// tests/probes.rs replays: whole runs of Slic3r::arrangement::arrange, the engine's own, on every
// kind of bed (bed points, which the engine turns into a box, a circle, a polygon or an infinite
// bed, and each typed overload), with fixed items (excludes), excluded regions, inflation, the
// print-by-object (sequential) parameters and shrunk bed, cli/run.cpp's parameters and its prime
// tower, a stop condition, and the progress callbacks. Each record holds the bed, the parameters,
// the items and the excludes, then what arrange gave each item (translation, rotation, bed and
// packing order) and every callback. Recorded by the o2-arrange task (2026-11-07).
//
// The generator keeps to the inputs on which the C++ is defined and stops: an item that fits an
// empty plate but scores 1e7 or more on every one makes the engine read past fixed_bins and open
// plates forever (parity/HAZARDS.md), so beds other than boxes get no fixed items or excluded
// regions, polygon beds are convex for sequential printing, and fixed items lie on the bed; a
// triangle alone on a plate ends the process in the final alignment, so a generated triangle is
// always inflated.
#include "../probe.hpp"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <exception>
#include <functional>
#include <string>
#include <vector>

#include "libslic3r/Arrange.hpp"
#include "libslic3r/BoundingBox.hpp"
#include "libslic3r/ExPolygon.hpp"
#include "libslic3r/Point.hpp"
#include "libslic3r/Polygon.hpp"

namespace probe {

namespace {

namespace ar = Slic3r::arrangement;
using Slic3r::Point;
using Slic3r::Points;
using Slic3r::scaled;

constexpr int64_t MM = 1000000; // scaled(1 mm)

void put(Payload &p, const Point &q) { p.i64(q.x()).i64(q.y()); }
void put(Payload &p, const Points &pts)
{
    p.u32(uint32_t(pts.size()));
    for (const Point &q : pts)
        put(p, q);
}

// The fields of an ArrangePolygon that arrange reads.
void put(Payload &p, const ar::ArrangePolygon &a)
{
    put(p, a.poly.contour.points);
    p.i64(a.translation.x()).i64(a.translation.y()).f64(a.rotation).i64(a.inflation);
    p.i32(a.bed_idx).i32(a.priority).boolean(a.is_virt_object).boolean(a.is_wipe_tower);
    p.u32(uint32_t(a.extrude_ids.size()));
    for (int e : a.extrude_ids)
        p.i32(e);
    p.i32(a.filament_temp_type).i32(a.first_bed_temp).i32(a.print_temp).i32(a.vitrify_temp).i32(a.itemid);
    p.f64(a.height).str(a.name);
}

void put(Payload &p, const ar::ArrangePolygons &v)
{
    p.u32(uint32_t(v.size()));
    for (const auto &a : v)
        put(p, a);
}

// One arrange run: the bed (0 points, 1 box, 2 circle, 3 polygon, 4 infinite), the parameters, the
// items and the excludes, whether the progress callbacks are set, and after how many calls the
// stop condition says stop (-1: no stop condition).
struct Run
{
    uint8_t              bed = 0;
    Points               pts;           // bed points (0) or the polygon (3)
    Point                a = Point(0, 0), b = Point(0, 0); // box corners (1), circle or infinite centre (2, 4)
    double               radius = 0.;
    ar::ArrangeParams    params;
    ar::ArrangePolygons  items, excludes;
    bool                 progress   = true;
    int32_t              stop_after = -1;
};

void arng(Payload &p, Run run)
{
    p.u8(run.bed);
    switch (run.bed) {
    case 0:
    case 3: put(p, run.pts); break;
    case 1:
        put(p, run.a);
        put(p, run.b);
        break;
    case 2:
        put(p, run.a);
        p.f64(run.radius);
        break;
    default: put(p, run.a);
    }
    const ar::ArrangeParams &q = run.params;
    p.i64(q.min_obj_distance).f32(q.accuracy).boolean(q.parallel).boolean(q.allow_rotations).boolean(q.do_final_align);
    p.boolean(q.allow_multi_materials_on_same_plate).boolean(q.avoid_extrusion_cali_region).boolean(q.is_seq_print).boolean(q.align_to_y_axis);
    p.f32(q.bed_shrink_x).f32(q.bed_shrink_y).f32(q.brim_skirt_distance).f32(q.clearance_height_to_rod).f32(q.clearance_height_to_lid);
    p.f32(q.clearance_radius).f32(q.object_skirt_offset).f32(q.nozzle_height).f32(q.printable_height);
    p.f64(q.align_center.x()).f64(q.align_center.y());
    put(p, q.excluded_regions);
    put(p, q.nonprefered_regions);
    put(p, run.items);
    put(p, run.excludes);
    p.boolean(run.progress).i32(run.stop_after);

    std::vector<std::pair<uint32_t, std::string>> progress;
    std::vector<std::pair<int32_t, int32_t>>      packed;
    uint32_t                                      stops = 0;
    ar::ArrangeParams                             params = run.params;
    if (run.progress) {
        params.progressind = [&progress](unsigned st, std::string name) { progress.emplace_back(uint32_t(st), name); };
        params.on_packed   = [&packed](const ar::ArrangePolygon &ap) { packed.emplace_back(ap.bed_idx, ap.priority); };
    } else {
        params.progressind = nullptr;
        params.on_packed   = nullptr;
    }
    if (run.stop_after >= 0) {
        const int32_t limit  = run.stop_after;
        params.stopcondition = [&stops, limit]() { return int32_t(++stops) > limit; };
    }

    bool        threw = false;
    std::string what;
    try {
        switch (run.bed) {
        case 0: ar::arrange(run.items, run.excludes, run.pts, params); break;
        case 1: ar::arrange(run.items, run.excludes, Slic3r::BoundingBox(run.a, run.b), params); break;
        case 2: ar::arrange(run.items, run.excludes, ar::CircleBed(run.a, run.radius), params); break;
        case 3: ar::arrange(run.items, run.excludes, Slic3r::Polygon(run.pts), params); break;
        default: ar::arrange(run.items, run.excludes, ar::InfiniteBed(run.a), params);
        }
    } catch (const std::exception &e) {
        threw = true;
        what  = e.what();
    }
    p.boolean(threw);
    if (threw)
        p.str(what);
    for (const auto &it : run.items) {
        p.i64(it.translation.x()).i64(it.translation.y()).f64(it.rotation).i32(it.bed_idx).i32(it.itemid);
    }
    p.u32(uint32_t(progress.size()));
    for (const auto &e : progress)
        p.u32(e.first).str(e.second);
    p.u32(uint32_t(packed.size()));
    for (const auto &e : packed)
        p.i32(e.first).i32(e.second);
    p.u32(stops);
}

// --- shapes ------------------------------------------------------------------------------------

double  unit(SplitMix &g) { return double(g.next() >> 11) * (1.0 / 9007199254740992.0); }
double  span(SplitMix &g, double lo, double hi) { return lo + (hi - lo) * unit(g); }
int64_t ispan(SplitMix &g, int64_t lo, int64_t hi) { return lo + int64_t(g.below(uint64_t(hi - lo) + 1)); }

Points rect(int64_t x0, int64_t y0, int64_t x1, int64_t y1) { return Points{Point(x0, y0), Point(x1, y0), Point(x1, y1), Point(x0, y1)}; }

Points ngon(Point c, double r, size_t n, double a0 = 0.)
{
    Points pts;
    for (size_t i = 0; i < n; ++i) {
        const double a = a0 + 6.283185307179586 * double(i) / double(n);
        pts.emplace_back(c.x() + int64_t(std::llround(r * std::cos(a))), c.y() + int64_t(std::llround(r * std::sin(a))));
    }
    return pts;
}

// An object's footprint about the origin, at most `size` across: a rectangle, a circle, an L, a
// star, a triangle, a convex blob or a long bar; counter-clockwise, or clockwise now and then.
Points rpart(SplitMix &g, int64_t size)
{
    Points        pts;
    const int64_t s = std::max<int64_t>(size, 4 * MM);
    switch (g.below(8)) {
    case 0:
    case 1: pts = rect(0, 0, ispan(g, MM, s), ispan(g, MM, s)); break;
    case 2: pts = ngon(Point(0, 0), double(ispan(g, MM, s / 2)), size_t(8 + g.below(56))); break;
    case 3: {
        const int64_t w = ispan(g, 2 * MM, s), h = ispan(g, 2 * MM, s), t = ispan(g, MM, std::min(w, h) - MM);
        pts = {Point(0, 0), Point(w, 0), Point(w, t), Point(t, t), Point(t, h), Point(0, h)};
        break;
    }
    case 4: {
        const size_t n = size_t(5 + g.below(6));
        const double r = double(ispan(g, 2 * MM, s / 2)), ri = r * span(g, 0.3, 0.7);
        for (size_t i = 0; i < 2 * n; ++i) {
            const double a  = 6.283185307179586 * double(i) / double(2 * n);
            const double rr = i % 2 == 0 ? r : ri;
            pts.emplace_back(int64_t(std::llround(rr * std::cos(a))), int64_t(std::llround(rr * std::sin(a))));
        }
        break;
    }
    case 5: pts = {Point(0, 0), Point(ispan(g, 2 * MM, s), ispan(g, -MM, MM)), Point(ispan(g, 0, s), ispan(g, 2 * MM, s))}; break;
    case 6: {
        const size_t n  = size_t(5 + g.below(12));
        const double rx = double(ispan(g, MM, s / 2)), ry = double(ispan(g, MM, s / 2));
        for (size_t i = 0; i < n; ++i) {
            const double a = 6.283185307179586 * (double(i) + 0.6 * unit(g)) / double(n);
            pts.emplace_back(int64_t(std::llround(rx * std::cos(a))), int64_t(std::llround(ry * std::sin(a))));
        }
        break;
    }
    default: pts = rect(0, 0, ispan(g, s / 2, s), ispan(g, MM, 3 * MM)); break;
    }
    if (g.below(5) == 0)
        std::reverse(pts.begin(), pts.end());
    return pts;
}

ar::ArrangePolygon part(const Points &pts, const std::string &name, int itemid)
{
    ar::ArrangePolygon a;
    a.poly.contour.points = pts;
    a.bed_idx             = 0;
    a.itemid              = itemid;
    a.extrude_ids         = {1};
    a.name                = name;
    a.height              = 10.;
    return a;
}

// cli/run.cpp's excluded area (bed_exclude_area) as a region and as fixed items on 16 plates, and
// its prime tower, for a bed from (x0, y0).
void cli_exclusions(ar::ArrangeParams &params, ar::ArrangePolygons &unselected, int64_t x0, int64_t y0, double exclusion_gap, bool tower)
{
    const Points area = rect(x0, y0, x0 + 18 * MM, y0 + 28 * MM);
    for (int j = 0; j < 16; ++j) {
        ar::ArrangePolygon r;
        r.poly.contour.points = area;
        r.is_virt_object      = true;
        r.bed_idx             = j;
        r.height              = 1;
        r.name                = "ExcludedRegion0";
        r.inflation           = coord_t(scale_(exclusion_gap));
        unselected.push_back(r);
    }
    if (tower) {
        ar::ArrangePolygon t;
        t.poly.contour.points = rect(x0 + 167 * MM, y0 + 197 * MM, x0 + 208 * MM, y0 + 238 * MM);
        t.bed_idx             = 0;
        t.name                = "WipeTower";
        t.is_virt_object      = true;
        t.is_wipe_tower       = true;
        t.inflation           = coord_t(scale_(exclusion_gap));
        unselected.push_back(t);
    }
    ar::ArrangePolygon e;
    e.poly.contour.points = area;
    e.is_virt_object      = true;
    e.bed_idx             = 0;
    e.height              = 1;
    e.name                = "ExcludedRegion0";
    e.inflation           = coord_t(scale_(1));
    params.excluded_regions.push_back(e);
}

// cli/run.cpp's parameters, and get_shrink_bedpts' bed for them.
void cli_params(ar::ArrangeParams &params, bool seq, double skirt)
{
    params.is_seq_print                        = seq;
    params.allow_rotations                     = false;
    params.allow_multi_materials_on_same_plate = true;
    params.avoid_extrusion_cali_region         = false;
    params.clearance_height_to_rod             = 36.f;
    params.clearance_height_to_lid             = 140.f;
    params.clearance_radius                    = 40.f;
    params.printable_height                    = 250.f;
    params.min_obj_distance                    = 0;
    if (seq) {
        params.bed_shrink_x = BED_SHRINK_SEQ_PRINT;
        params.bed_shrink_y = BED_SHRINK_SEQ_PRINT;
    }
    params.align_to_y_axis     = false;
    params.brim_skirt_distance = float(skirt);
    params.bed_shrink_x += params.brim_skirt_distance;
    params.bed_shrink_y += params.brim_skirt_distance;
    if (seq) {
        params.bed_shrink_x -= params.clearance_radius / 2;
        params.bed_shrink_y -= params.clearance_radius / 2;
    }
    if (seq)
        params.min_obj_distance = coord_t(scaled(params.clearance_radius + 0.001));
}

Points shrink(Points bed, double dx, double dy)
{
    auto fun = [](Points &pts, double dist, int dir) {
        const Point c = Slic3r::Polygon(pts).bounding_box().center();
        for (auto &pt : pts)
            pt[dir] += coord_t(dist * ((c[dir] - pt[dir]) >= 0 ? 1 : -1));
    };
    fun(bed, double(scaled(dx)), 0);
    fun(bed, double(scaled(dy)), 1);
    return bed;
}

// The hand-made runs, each in both sets.
std::vector<Run> chosen()
{
    std::vector<Run> out;
    SplitMix         g(0x43484F53u);

    { // cli/run.cpp: five parts on a 256 mm bed with its excluded area and prime tower
        Run r;
        cli_params(r.params, false, 0.);
        r.pts = shrink(rect(0, 0, 256 * MM, 256 * MM), r.params.bed_shrink_x, r.params.bed_shrink_y);
        cli_exclusions(r.params, r.excludes, 0, 0, 1., true);
        const Points shapes[] = {rect(0, 0, 40 * MM, 30 * MM), rect(0, 0, 20 * MM, 20 * MM), ngon(Point(0, 0), 15. * MM, 32),
                                 rect(0, 0, 60 * MM, 10 * MM), rect(0, 0, 25 * MM, 45 * MM)};
        for (int i = 0; i < 5; ++i) {
            r.items.push_back(part(shapes[i], "part" + std::to_string(i), i));
            r.items.back().inflation = coord_t(scaled(1.5));
        }
        out.push_back(r);
    }
    { // the same, printed by object: the bed grown by get_shrink_bedpts, the parts kept apart
        Run r;
        cli_params(r.params, true, 3.);
        r.pts = shrink(rect(0, 0, 256 * MM, 256 * MM), r.params.bed_shrink_x, r.params.bed_shrink_y);
        cli_exclusions(r.params, r.excludes, 0, 0, std::max(1., r.params.clearance_radius / 2 + r.params.bed_shrink_x + 1.), false);
        for (int i = 0; i < 4; ++i) {
            r.items.push_back(part(rect(0, 0, (20 + 10 * i) * MM, 30 * MM), "seq" + std::to_string(i), i));
            r.items.back().inflation = r.params.min_obj_distance / 2;
            r.items.back().height    = 10. + 15. * i;
        }
        out.push_back(r);
    }
    { // a circular bed from points (a delta printer)
        Run r;
        r.pts = ngon(Point(0, 0), 100. * MM, 72);
        for (int i = 0; i < 6; ++i)
            r.items.push_back(part(i % 2 ? ngon(Point(0, 0), 12. * MM, 24) : rect(0, 0, 30 * MM, 18 * MM), "d" + std::to_string(i), i));
        out.push_back(r);
    }
    { // an irregular, convex bed from points, printed by object
        Run r;
        r.params.is_seq_print = true;
        r.pts                 = {Point(0, 0), Point(220 * MM, 0), Point(220 * MM, 180 * MM), Point(180 * MM, 220 * MM), Point(0, 220 * MM)};
        for (int i = 0; i < 4; ++i)
            r.items.push_back(part(rect(0, 0, 50 * MM, 40 * MM), "p" + std::to_string(i), i));
        out.push_back(r);
    }
    { // an L-shaped bed (not convex), the typed overload
        Run r;
        r.bed = 3;
        r.pts = {Point(0, 0), Point(300 * MM, 0), Point(300 * MM, 120 * MM), Point(120 * MM, 120 * MM), Point(120 * MM, 300 * MM), Point(0, 300 * MM)};
        for (int i = 0; i < 5; ++i)
            r.items.push_back(part(rpart(g, 60 * MM), "l" + std::to_string(i), i));
        out.push_back(r);
    }
    { // no bed points: an infinite bed at the origin; one point: an infinite bed there
        Run r;
        for (int i = 0; i < 4; ++i)
            r.items.push_back(part(rect(0, 0, 30 * MM, 30 * MM), "i" + std::to_string(i), i));
        out.push_back(r);
        r.pts = {Point(50 * MM, -20 * MM)};
        out.push_back(r);
        r.bed = 4;
        r.a   = Point(-10 * MM, 5 * MM);
        r.pts.clear();
        out.push_back(r);
    }
    { // the typed box and circle overloads; items too big, items not to arrange, too few points
        Run r;
        r.bed = 1;
        r.a   = Point(-110 * MM, -110 * MM);
        r.b   = Point(110 * MM, 110 * MM);
        r.items.push_back(part(rect(0, 0, 300 * MM, 20 * MM), "toobig", 0));
        r.items.push_back(part(rect(0, 0, 50 * MM, 50 * MM), "fits", 1));
        r.items.push_back(part(rect(0, 0, 10 * MM, 10 * MM), "unarranged", 2));
        r.items.back().bed_idx = -1;
        r.items.push_back(part({Point(0, 0), Point(MM, MM)}, "twopoints", 3));
        r.items.push_back(part(rect(0, 0, 40 * MM, 40 * MM), "plate3", 4));
        r.items.back().bed_idx = 3;
        out.push_back(r);
        r.bed    = 2;
        r.a      = Point(0, 0);
        r.radius = 90. * MM;
        out.push_back(r);
    }
    { // more parts than one plate holds, fixed parts on plates 0 and 1, priorities and filaments
        Run r;
        r.pts = rect(0, 0, 180 * MM, 180 * MM);
        for (int i = 0; i < 9; ++i) {
            r.items.push_back(part(rect(0, 0, 70 * MM, 60 * MM), "m" + std::to_string(i), i));
            r.items.back().priority           = i % 3;
            r.items.back().extrude_ids        = {1 + i % 2};
            r.items.back().filament_temp_type = i % 4 == 0 ? 0 : (i % 4 == 1 ? 1 : -1);
            r.items.back().first_bed_temp     = 55 + 5 * (i % 2);
        }
        ar::ArrangePolygon f = part(rect(10 * MM, 10 * MM, 40 * MM, 50 * MM), "fixed0", 0);
        r.excludes.push_back(f);
        f                    = part(rect(100 * MM, 100 * MM, 170 * MM, 160 * MM), "fixed1", 1);
        f.bed_idx            = 1;
        f.filament_temp_type = 0;
        r.excludes.push_back(f);
        r.params.allow_multi_materials_on_same_plate = false;
        out.push_back(r);
        r.stop_after = 7; // stopped part way
        out.push_back(r);
    }
    { // centred alignment off, a user alignment point, a lower accuracy, serial placement
        Run r;
        r.pts                   = rect(0, 0, 250 * MM, 210 * MM);
        r.params.do_final_align = false;
        r.params.align_center   = Slic3r::Vec2d(0.2, 0.7);
        r.params.accuracy       = 0.5f;
        r.params.parallel       = false;
        r.progress              = false;
        for (int i = 0; i < 6; ++i)
            r.items.push_back(part(rpart(g, 50 * MM), "u" + std::to_string(i), i));
        out.push_back(r);
    }
    return out;
}

// A generated run. Beds other than boxes get no fixed items and no excluded regions.
Run random_run(SplitMix &g)
{
    Run          r;
    const double w = double(ispan(g, 120, 400)), h = double(ispan(g, 120, 400));
    const int64_t x0 = g.below(3) == 0 ? -int64_t(w / 2.) * MM : 0, y0 = g.below(3) == 0 ? -int64_t(h / 2.) * MM : 0;
    const int64_t W = int64_t(w) * MM, H = int64_t(h) * MM;
    const bool    seq = g.below(4) == 0;
    const uint64_t kind = g.below(10);
    bool           box  = false;
    if (kind < 5) {
        box   = true;
        r.bed = g.below(3) == 0 ? 1 : 0;
        r.pts = rect(x0, y0, x0 + W, y0 + H);
        r.a   = Point(x0, y0);
        r.b   = Point(x0 + W, y0 + H);
    } else if (kind < 7) {
        const double rad = double(std::min(W, H)) / 2.;
        r.bed            = g.below(2) == 0 ? 2 : 0;
        r.pts            = ngon(Point(x0 + W / 2, y0 + H / 2), rad, size_t(32 + g.below(64)));
        r.a              = Point(x0 + W / 2, y0 + H / 2);
        r.radius         = rad;
    } else if (kind < 9) {
        r.bed = g.below(2) == 0 ? 3 : 0;
        if (seq || g.below(2) == 0) { // convex: a corner cut off
            const int64_t c = ispan(g, W / 10, W / 3);
            r.pts           = {Point(x0, y0), Point(x0 + W, y0), Point(x0 + W, y0 + H - c), Point(x0 + W - c, y0 + H), Point(x0, y0 + H)};
        } else { // an L
            const int64_t cx = ispan(g, W / 3, 2 * W / 3), cy = ispan(g, H / 3, 2 * H / 3);
            r.pts = {Point(x0, y0), Point(x0 + W, y0), Point(x0 + W, y0 + cy), Point(x0 + cx, y0 + cy), Point(x0 + cx, y0 + H), Point(x0, y0 + H)};
        }
    } else {
        r.bed = g.below(2) == 0 ? 4 : 0;
        r.a   = Point(ispan(g, -50, 50) * MM, ispan(g, -50, 50) * MM);
        r.pts.clear();
        if (r.bed == 0 && g.below(2) == 0)
            r.pts.push_back(r.a);
    }

    ar::ArrangeParams &q = r.params;
    q.is_seq_print       = seq;
    q.do_final_align     = g.below(4) != 0;
    q.parallel           = g.below(2) == 0;
    static const float acc[] = {1.f, 1.f, 0.65f, 0.5f, 0.3f, 0.1f};
    q.accuracy                                  = acc[g.below(6)];
    q.allow_multi_materials_on_same_plate       = g.below(4) != 0;
    q.avoid_extrusion_cali_region               = g.below(2) == 0;
    q.clearance_height_to_rod                   = float(ispan(g, 20, 40));
    q.clearance_height_to_lid                   = float(ispan(g, 80, 160));
    q.clearance_radius                          = float(ispan(g, 20, 60));
    q.printable_height                          = float(ispan(g, 150, 300));
    q.nozzle_height                             = float(span(g, 2., 5.));
    q.min_obj_distance                          = g.below(3) == 0 ? ispan(g, 0, 10 * MM) : 0;
    if (g.below(5) == 0)
        q.align_center = Slic3r::Vec2d(span(g, 0., 1.), span(g, 0., 1.));
    r.progress   = g.below(5) != 0;
    r.stop_after = g.below(8) == 0 ? int32_t(g.below(60)) : -1;

    const int64_t span_mm = std::min(W, H);
    const size_t  n       = size_t(1 + g.below(g.below(4) == 0 ? 16 : 7));
    for (size_t i = 0; i < n; ++i) {
        const int64_t size = g.below(10) == 0 ? span_mm + 10 * MM : ispan(g, 5 * MM, span_mm / 2);
        ar::ArrangePolygon a = part(rpart(g, size), "obj" + std::to_string(i), int(i));
        if (g.below(3) == 0)
            a.translation = Slic3r::Vec2crd(ispan(g, 0, W), ispan(g, 0, H));
        if (g.below(10) == 0)
            a.rotation = span(g, -3.2, 3.2);
        a.inflation = g.below(3) == 0 ? 0 : ispan(g, 0, 5 * MM);
        // libnest2d's hull of a path of three points is empty, so a triangle left alone on a plate
        // ends the process in the final alignment (HAZARDS; the offset by zero that every item
        // gets would take a collinear fourth point off again): an uninflated triangle is inflated
        // by half a millimetre, which draws nothing from the generator.
        if (a.poly.contour.points.size() == 3 && a.inflation == 0)
            a.inflation = MM / 2;
        if (g.below(12) == 0)
            a.bed_idx = g.below(2) == 0 ? -1 : int(1 + g.below(3));
        a.priority           = g.below(4) == 0 ? int(g.below(3)) : 0;
        a.extrude_ids        = {int(g.below(4))};
        if (g.below(4) == 0)
            a.extrude_ids.push_back(int(g.below(4)));
        a.filament_temp_type = int(g.below(4)) - 1;
        a.first_bed_temp     = int(50 + 10 * g.below(3));
        a.print_temp         = int(200 + 10 * g.below(5));
        a.vitrify_temp       = int(g.below(3) * 10);
        a.height             = g.below(5) == 0 ? span(g, 50., 200.) : span(g, 0.2, 40.);
        r.items.push_back(a);
    }

    if (box) {
        if (g.below(3) == 0) { // cli/run.cpp's excluded area, and maybe its prime tower
            const double gap = seq ? std::max(1., double(q.clearance_radius) / 2. + 1.) : 1.;
            if (W >= 220 * MM && H >= 250 * MM)
                cli_exclusions(q, r.excludes, x0, y0, gap, g.below(2) == 0);
        }
        const size_t nf = size_t(g.below(4) == 0 ? 1 + g.below(3) : 0);
        for (size_t i = 0; i < nf; ++i) { // fixed parts within the bed
            const int64_t fw = ispan(g, 5 * MM, W / 4), fh = ispan(g, 5 * MM, H / 4);
            const int64_t fx = x0 + ispan(g, 0, W - fw), fy = y0 + ispan(g, 0, H - fh);
            ar::ArrangePolygon f = part(rect(fx, fy, fx + fw, fy + fh), "fixed" + std::to_string(i), int(i));
            f.bed_idx            = int(g.below(3)) - (g.below(6) == 0 ? 1 : 0);
            f.filament_temp_type = int(g.below(4)) - 1;
            f.height             = span(g, 1., 100.);
            f.inflation          = g.below(2) == 0 ? 0 : ispan(g, 0, 2 * MM);
            // A negative bed leaves the part unfixed, so it is sorted with the others and needs
            // an extruder (the sort reads the first).
            if (f.bed_idx >= 0 && g.below(4) == 0)
                f.extrude_ids.clear();
            r.excludes.push_back(f);
        }
        if (g.below(5) == 0) { // an excluded region of the parameters
            const int64_t ew = ispan(g, 10 * MM, W / 4), eh = ispan(g, 10 * MM, H / 4);
            const int64_t ex = x0 + ispan(g, 0, W - ew), ey = y0 + ispan(g, 0, H - eh);
            ar::ArrangePolygon e = part(rect(ex, ey, ex + ew, ey + eh), "ExcludedRegion1", 0);
            e.is_virt_object     = true;
            e.inflation          = coord_t(scale_(1));
            q.excluded_regions.push_back(e);
        }
        if (g.below(8) == 0) { // a non-preferred region, which arrange keeps only as a box
            ar::ArrangePolygon e = part(rect(x0, y0, x0 + 30 * MM, y0 + 30 * MM), "Nonpreferred", 0);
            e.is_virt_object     = true;
            q.nonprefered_regions.push_back(e);
        }
    }
    return r;
}

} // namespace

void record_runs(Sets &sets)
{
    for (const Run &r : chosen()) {
        Payload p;
        arng(p, r);
        sets.full.add("ARNG", p);
        sets.committed.add("ARNG", p);
    }
    SplitMix g(0x41524E47u);
    size_t   taken = 0;
    for (int i = 0; i < 300; ++i) {
        const Run r = random_run(g);
        Payload   p;
        arng(p, r);
        sets.full.add("ARNG", p);
        if (taken < 16 && r.items.size() <= 5 && r.params.accuracy <= 0.65f && p.bytes.size() <= 6144) {
            sets.committed.add("ARNG", p);
            ++taken;
        }
    }
}

} // namespace probe
