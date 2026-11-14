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
// ANWH, ANWT and ANMP (parity/sop1/arachne.md), piece arachne-4: the outline preparation of
// WallToolPaths::generate (its file-local helpers simplify, fixSelfIntersections,
// removeDegenerateVerts, removeColinearEdges and removeSmallAreas, as a chain and one by one),
// WallToolPaths whole (getToolPaths, getInnerContour and getRegionOrder as PerimeterGenerator calls
// them) and make_paths_params. Also the wall settings the skeletal records share (common.hpp).
// Inputs are the probe's shapes, slices of src/tests/data meshes and splitmix64 generators (seeds
// 0xA7A1C000 to 0xA7A1C400). Recorded by orca-o3-probe-arachne, 2026-11-13.
#include "common.hpp"

#include <algorithm>
#include <cmath>

#include "libslic3r/ClipperUtils.hpp"
#include "libslic3r/Flow.hpp"
#include "libslic3r/Geometry.hpp"
#include "libslic3r/PrintConfig.hpp"

namespace probe::arachne {

using Slic3r::Arachne::WallToolPaths;
using Slic3r::Arachne::WallToolPathsParams;

// ---------------------------------------------------------------- wall settings

std::vector<WallSetup> wall_setups()
{
    std::vector<WallSetup> out;
    // Spacings from Flow (width - height * (1 - pi / 4)) of the default widths.
    {
        WallSetup s;
        s.name         = "nozzle 0.4 walls 3";
        s.bead_width_0 = 377080;
        s.bead_width_x = 407080;
        out.push_back(s);
    }
    {
        WallSetup s;
        s.name         = "nozzle 0.4 walls 2 precise outer wall";
        s.bead_width_0 = 377080;
        s.bead_width_x = 407080;
        s.inset_count  = 2;
        s.wall_0_inset = -21460;
        out.push_back(s);
    }
    {
        WallSetup s;
        s.name          = "nozzle 0.4 walls 1 first layer";
        s.bead_width_0  = 377080;
        s.bead_width_x  = 407080;
        s.inset_count   = 1;
        s.layer_id      = 0;
        s.top_or_bottom = true;
        out.push_back(s);
    }
    {
        WallSetup s;
        s.name         = "nozzle 0.6 walls 4";
        s.nozzle       = 0.6;
        s.layer_height = 0.3;
        s.bead_width_0 = 555619;
        s.bead_width_x = 585619;
        s.inset_count  = 4;
        out.push_back(s);
    }
    {
        WallSetup s;
        s.name               = "nozzle 0.25 walls 3 distribution 2 angle 25";
        s.nozzle             = 0.25;
        s.layer_height       = 0.12;
        s.bead_width_0       = 244248;
        s.bead_width_x       = 244248;
        s.inset_count        = 3;
        s.min_bead_width     = 60;
        s.min_feature_size   = 10;
        s.transition_length  = 150;
        s.filter_deviation   = 40;
        s.transition_angle   = 25;
        s.distribution_count = 2;
        out.push_back(s);
    }
    {
        WallSetup s;
        s.name         = "nozzle 0.4 walls 0";
        s.bead_width_0 = 377080;
        s.bead_width_x = 407080;
        s.inset_count  = 0;
        out.push_back(s);
    }
    return out;
}

namespace {

struct ConfigValues
{
    int                 layer_id = 1;
    double              min_bead_width = 85, initial_layer_min_bead_width = 85, min_feature_size = 25, min_length_factor = 0.5;
    double              filter_deviation = 25, transition_length = 100, transition_angle = 10;
    int                 distribution_count = 1;
    double              max_resolution = 0.5, max_deviation = 0.025;
    std::vector<double> nozzles = {0.4};
};

WallToolPathsParams params_of(const ConfigValues &v)
{
    Slic3r::PrintObjectConfig poc;
    Slic3r::PrintConfig       pc;
    poc.min_bead_width.value                   = v.min_bead_width;
    poc.initial_layer_min_bead_width.value     = v.initial_layer_min_bead_width;
    poc.min_feature_size.value                 = v.min_feature_size;
    poc.min_length_factor.value                = v.min_length_factor;
    poc.wall_transition_filter_deviation.value = v.filter_deviation;
    poc.wall_transition_length.value           = v.transition_length;
    poc.wall_transition_angle.value            = v.transition_angle;
    poc.wall_distribution_count.value          = v.distribution_count;
    poc.wall_maximum_resolution.value          = v.max_resolution;
    poc.wall_maximum_deviation.value           = v.max_deviation;
    pc.nozzle_diameter.values                  = v.nozzles;
    return Slic3r::Arachne::make_paths_params(v.layer_id, poc, pc);
}

ConfigValues values_of(const WallSetup &s)
{
    ConfigValues v;
    v.layer_id = s.layer_id;
    if (s.min_bead_width >= 0)
        v.min_bead_width = v.initial_layer_min_bead_width = s.min_bead_width;
    if (s.min_feature_size >= 0)
        v.min_feature_size = s.min_feature_size;
    if (s.transition_length >= 0)
        v.transition_length = s.transition_length;
    if (s.filter_deviation >= 0)
        v.filter_deviation = s.filter_deviation;
    if (s.transition_angle >= 0)
        v.transition_angle = s.transition_angle;
    if (s.distribution_count >= 0)
        v.distribution_count = s.distribution_count;
    v.nozzles = {s.nozzle};
    return v;
}

} // namespace

WallToolPathsParams wall_params(const WallSetup &s)
{
    WallToolPathsParams p    = params_of(values_of(s));
    p.is_top_or_bottom_layer = s.top_or_bottom;
    return p;
}

void put(Payload &p, const WallSetup &s)
{
    p.str(s.name).f64(s.nozzle).f64(s.layer_height).i64(s.bead_width_0).i64(s.bead_width_x).u64(s.inset_count).i64(s.wall_0_inset);
    p.i32(s.layer_id).boolean(s.top_or_bottom);
}

void put(Payload &p, const WallToolPathsParams &w)
{
    p.f32(w.min_bead_width).f32(w.min_feature_size).f32(w.min_length_factor).f32(w.wall_transition_length).f32(w.wall_transition_angle);
    p.f32(w.wall_transition_filter_deviation).i32(w.wall_distribution_count).boolean(w.is_top_or_bottom_layer);
    p.i64(w.wall_maximum_resolution).i64(w.wall_maximum_deviation);
}

StArgs st_args(const WallSetup &s, const WallToolPathsParams &w)
{
    StArgs a;
    a.smallest_segment                    = w.wall_maximum_resolution;
    a.allowed_distance                    = w.wall_maximum_deviation;
    const double  transitioning_angle     = Slic3r::Geometry::deg2rad(w.wall_transition_angle);
    const coord_t min_bead_width          = scaled<coord_t>(w.min_bead_width);
    const coord_t min_feature_size        = scaled<coord_t>(w.min_feature_size);
    const float   ext_width               = Slic3r::Flow::rounded_rectangle_extrusion_width_from_spacing(Slic3r::unscale<float>(s.bead_width_0), float(s.layer_height));
    const float   width                   = Slic3r::Flow::rounded_rectangle_extrusion_width_from_spacing(Slic3r::unscale<float>(s.bead_width_x), float(s.layer_height));
    const coord_t wall_transition_length  = scaled<coord_t>(w.wall_transition_length);
    a.strat.outer                         = s.bead_width_0;
    a.strat.inner                         = s.bead_width_x;
    a.strat.transition_length             = wall_transition_length;
    a.strat.transitioning_angle           = float(transitioning_angle);
    a.strat.print_thin_walls              = Slic3r::Arachne::fill_outline_gaps;
    a.strat.min_bead_width                = min_bead_width;
    a.strat.min_feature_size              = min_feature_size;
    a.strat.split                         = std::clamp(2. * unscaled<double>(min_bead_width) / ext_width - 1., 0.01, 0.99);
    a.strat.add                           = std::clamp(unscaled<double>(min_bead_width) / width, 0.01, 0.99);
    a.strat.max_bead_count                = coord_t(2 * s.inset_count);
    a.strat.outer_wall_offset             = s.wall_0_inset;
    a.strat.inward_distributed_center_wall_count = w.wall_distribution_count;
    a.strat.minimum_variable_line_ratio   = 0.5;
    a.discretization                      = scaled<coord_t>(0.8);
    a.filter_dist                         = scaled<coord_t>(100.f);
    a.filter_deviation                    = scaled<coord_t>(w.wall_transition_filter_deviation);
    a.propagation_dist                    = wall_transition_length;
    return a;
}

namespace {

int64_t span(SplitMix &g, int64_t r) { return int64_t(g.below(uint64_t(2 * r + 1))) - r; }

// ---------------------------------------------------------------- ANWH

void put_step(Payload &p, const PrepStep &s)
{
    p.u8(s.op).i64(s.a).i64(s.b).f64(s.f).boolean(s.holes);
}

// Form 0: the chain on one outline (the triple offset's result, then each step's output, then the union).
Payload anwh_chain(const std::string &name, const Polygons &outline, coord_t smallest, coord_t allowed, coord_t bead0)
{
    std::vector<PrepStep> steps;
    const Polygons        result = prepare_outline(outline, smallest, allowed, bead0, &steps);
    Payload               p;
    p.u8(0).str(name);
    put(p, outline);
    p.i64(smallest).i64(allowed).i64(bead0);
    put(p, steps.front().in);
    p.u64(steps.size());
    for (const PrepStep &s : steps) {
        put_step(p, s);
        put(p, s.out);
    }
    put(p, result);
    return p;
}

// Form 1: one step on its own.
Payload anwh_step(PrepStep s)
{
    run_step(s);
    Payload p;
    p.u8(1);
    put_step(p, s);
    put(p, s.in);
    put(p, s.out);
    return p;
}

Polygon jitter_circle(SplitMix &g, double r, int n, double amp)
{
    Polygon p;
    for (int i = 0; i < n; ++i) {
        const double a  = 2. * M_PI * double(i) / double(n);
        const double rr = r + amp * (double(g.below(2001)) / 1000. - 1.);
        p.points.emplace_back(coord_t(std::llround(rr * 1e6 * std::cos(a))), coord_t(std::llround(rr * 1e6 * std::sin(a))));
    }
    return p;
}

Polygon pts(std::initializer_list<std::pair<coord_t, coord_t>> l)
{
    Polygon p;
    for (const auto &[x, y] : l)
        p.points.emplace_back(x, y);
    return p;
}

void record_helpers(Out &out, const std::vector<Shape> &own, const std::vector<Shape> &mesh)
{
    const WallSetup           s0 = wall_setups().front();
    const WallToolPathsParams w0 = wall_params(s0);
    size_t                    i  = 0;
    for (const Shape &s : own) {
        Case c;
        c.add("ANWH", anwh_chain(s.name, s.polygons, w0.wall_maximum_resolution, w0.wall_maximum_deviation, s0.bead_width_0));
        out.put("helpers", c, i < 6);
        ++i;
    }
    for (const Shape &s : mesh) {
        Case c;
        c.add("ANWH", anwh_chain(s.name, s.polygons, w0.wall_maximum_resolution, w0.wall_maximum_deviation, s0.bead_width_0));
        out.put("helpers", c, false);
    }
    // One step at a time: hand-made inputs first.
    {
        Case c;
        const Polygons sq = {pts({{0, 0}, {1000000, 0}, {1000000, 1000000}, {0, 1000000}})};
        // simplify: fewer than three points, three points, a near-collinear run, a short segment by a long one.
        c.add("ANWH", anwh_step({0, 500000, 25000, 0, false, {pts({{0, 0}, {5, 5}})}}));
        c.add("ANWH", anwh_step({0, 500000, 25000, 0, false, {pts({{0, 0}, {5, 5}, {0, 9}})}}));
        c.add("ANWH", anwh_step({0, 500000, 25000, 0, false, {pts({{0, 0}, {1000000, 3}, {2000000, 0}, {2000000, 2000000}, {0, 2000000}})}}));
        c.add("ANWH", anwh_step({0, 500000, 25000, 0, false, {pts({{0, 0}, {5000000, 0}, {5100000, 20000}, {5100000, 5000000}, {0, 5000000}})}}));
        c.add("ANWH", anwh_step({0, 10000, 5000, 0, false, sq}));
        // fixSelfIntersections: epsilon below 1 (Clipper's even-odd simplification), a bow tie, a near touch.
        c.add("ANWH", anwh_step({1, 0, 0, 0, false, {pts({{0, 0}, {1000000, 1000000}, {1000000, 0}, {0, 1000000}})}}));
        c.add("ANWH", anwh_step({1, 12499, 0, 0, false, {pts({{0, 0}, {1000000, 1000000}, {1000000, 0}, {0, 1000000}})}}));
        c.add("ANWH", anwh_step({1, 12499, 0, 0, false, {pts({{0, 0}, {3000000, 0}, {3000000, 1000000}, {1500000, 8000}, {0, 1000000}})}}));
        c.add("ANWH", anwh_step({1, 12499, 0, 0, false, {pts({{0, 0}, {1000000, 0}, {1000000, 1000000}, {0, 1000000}}), pts({{1005000, 0}, {2000000, 0}, {2000000, 1000000}, {1005000, 1000000}})}}));
        c.add("ANWH", anwh_step({1, 1, 0, 0, false, sq}));
        // removeDegenerateVerts: a spike, a back-and-forth, a duplicate point.
        c.add("ANWH", anwh_step({2, 0, 0, 0, false, {pts({{0, 0}, {1000000, 0}, {1500000, 0}, {1000000, 0}, {1000000, 1000000}, {0, 1000000}})}}));
        c.add("ANWH", anwh_step({2, 0, 0, 0, false, {pts({{0, 0}, {1000000, 0}, {1000000, 1000000}, {1000000, 1000000}, {0, 1000000}})}}));
        c.add("ANWH", anwh_step({2, 0, 0, 0, false, {pts({{0, 0}, {1000000, 0}, {0, 0}})}}));
        // removeColinearEdges: angles about the default and the engine's 0.005.
        c.add("ANWH", anwh_step({3, 0, 0, 0.005, false, {pts({{0, 0}, {1000000, 1000}, {2000000, 0}, {2000000, 2000000}, {0, 2000000}})}}));
        c.add("ANWH", anwh_step({3, 0, 0, 0.0005, false, {pts({{0, 0}, {1000000, 1000}, {2000000, 0}, {2000000, 2000000}, {0, 2000000}})}}));
        c.add("ANWH", anwh_step({3, 0, 0, 0.005, false, {pts({{0, 0}, {1000000, 0}, {2000000, 1}})}}));
        // removeSmallAreas: islands and holes around the limit, holes kept and removed.
        const Polygons islands = {pts({{0, 0}, {5000000, 0}, {5000000, 5000000}, {0, 5000000}}), pts({{1000000, 1000000}, {1000000, 1100000}, {1100000, 1100000}, {1100000, 1000000}}),
                                  pts({{2000000, 2000000}, {2000000, 3000000}, {3000000, 3000000}, {3000000, 2000000}}), pts({{6000000, 0}, {6150000, 0}, {6150000, 150000}, {6000000, 150000}}),
                                  pts({{7000000, 0}, {7300000, 0}, {7300000, 300000}, {7000000, 300000}})};
        for (bool holes : {false, true})
            for (double area : {188540. * 188540., 1e10, 0.})
                c.add("ANWH", anwh_step({4, 0, 0, area, holes, islands}));
        out.put("helpers", c, true);
    }
    SplitMix g(0xA7A1C000ull);
    for (int block = 0; block < 10; ++block) {
        Case c;
        for (int k = 0; k < 6; ++k) {
            PrepStep s{uint8_t(g.below(5))};
            const double r = 0.3 + double(g.below(8000)) / 1000.;
            Polygons     in = {jitter_circle(g, r, 8 + int(g.below(400)), double(g.below(3)) * 0.01)};
            if (g.below(3) == 0)
                in.push_back(jitter_circle(g, r * 0.5, 6 + int(g.below(100)), 0.005));
            if (s.op == 2 && !in.front().points.empty()) {
                // Spikes and repeats for the degenerate-vertex removal.
                Points &p = in.front().points;
                for (int j = 0; j < 4 && p.size() > 3; ++j) {
                    const size_t at = size_t(g.below(p.size() - 1)) + 1;
                    p.insert(p.begin() + at, {p[at - 1] + (p[at - 1] - p[at]), p[at - 1]});
                }
            }
            if (s.op == 1) {
                // Another loop just beside the first.
                Polygon q = in.front();
                for (Point &pt : q.points)
                    pt += Point(coord_t(2 * r * 1e6) + coord_t(g.below(30000)), coord_t(span(g, 2000)));
                in.push_back(q);
            }
            s.in = in;
            switch (s.op) {
            case 0: s.a = g.below(2) == 0 ? 500000 : 10000 + int64_t(g.below(2000000)); s.b = g.below(2) == 0 ? 25000 : 1000 + int64_t(g.below(100000)); break;
            case 1: s.a = g.below(4) == 0 ? 0 : 12499 + int64_t(g.below(40000)) - 20000; break;
            case 3: s.f = g.below(2) == 0 ? 0.005 : double(g.below(1000)) / 10000.; break;
            case 4: s.f = double(g.below(4)) * 1e10; s.holes = g.below(2) == 0; break;
            default: break;
            }
            c.add("ANWH", anwh_step(s));
        }
        out.put("helpers", c, block < 2);
    }
}

// ---------------------------------------------------------------- ANWT

Payload anwt(const std::string &name, const Polygons &outline, const WallSetup &s)
{
    const WallToolPathsParams w = wall_params(s);
    Payload                   p;
    p.str(name);
    put(p, s);
    put(p, w);
    put(p, outline);
    log_take();
    WallToolPaths wt(outline, s.bead_width_0, s.bead_width_x, s.inset_count, s.wall_0_inset, s.layer_height, w);
    Toolpaths     tp = wt.getToolPaths();
    const Polygons inner = wt.getInnerContour();
    put(p, log_take());
    put(p, tp);
    put(p, inner);
    // getRegionOrder on the lines in PerimeterGenerator's order: inner to outer unless the outer wall goes first.
    for (bool outer_first : {false, true}) {
        std::vector<ExtrusionLine *> all;
        const int start = outer_first ? 0 : int(tp.size()) - 1, end = outer_first ? int(tp.size()) : -1, dir = outer_first ? 1 : -1;
        for (int i = start; i != end; i += dir) {
            if (tp[size_t(i)].empty())
                continue;
            for (ExtrusionLine &l : tp[size_t(i)])
                all.push_back(&l);
        }
        const WallToolPaths::ExtrusionLineSet order = WallToolPaths::getRegionOrder(all, outer_first);
        auto index = [&all](const ExtrusionLine *l) {
            return int32_t(std::find(all.begin(), all.end(), l) - all.begin());
        };
        p.boolean(outer_first).u64(all.size()).u64(order.size());
        for (const auto &[before, after] : order)
            p.i32(index(before)).i32(index(after));
    }
    put(p, log_take());
    return p;
}

void record_walls_whole(Out &out, const std::vector<Shape> &own, const std::vector<Shape> &mesh)
{
    const std::vector<WallSetup> setups = wall_setups();
    size_t                       i      = 0;
    for (const Shape &sh : own) {
        for (size_t k = 0; k < setups.size(); ++k) {
            Case c;
            c.add("ANWT", anwt(sh.name, sh.polygons, setups[k]));
            out.put("walls", c, k != 3 && k != 4); // the shares take the marked ones that fit, in order
        }
        ++i;
    }
    for (const Shape &sh : mesh)
        for (size_t k : {size_t(0), size_t(3)}) {
            Case c;
            c.add("ANWT", anwt(sh.name, sh.polygons, setups[k]));
            out.put("walls", c, false);
        }
}

// ---------------------------------------------------------------- ANMP

Payload anmp(const ConfigValues &v)
{
    Payload p;
    p.i32(v.layer_id).f64(v.min_bead_width).f64(v.initial_layer_min_bead_width).f64(v.min_feature_size).f64(v.min_length_factor);
    p.f64(v.filter_deviation).f64(v.transition_length).f64(v.transition_angle).i32(v.distribution_count).f64(v.max_resolution).f64(v.max_deviation);
    p.u64(v.nozzles.size());
    for (double d : v.nozzles)
        p.f64(d);
    put(p, params_of(v));
    return p;
}

void record_params(Out &out)
{
    Case               c;
    const ConfigValues d;
    c.add("ANMP", anmp(d));
    {
        ConfigValues v = d;
        v.layer_id     = 0;
        v.initial_layer_min_bead_width = 120;
        c.add("ANMP", anmp(v));
    }
    {
        ConfigValues v = d;
        v.nozzles      = {0.6, 0.4, 0.8};
        c.add("ANMP", anmp(v));
    }
    {
        // Options whose zero keeps the default (min_length_factor falls back to 0.5, the resolutions stay).
        ConfigValues v      = d;
        v.min_length_factor = 0;
        v.max_resolution    = 0;
        v.max_deviation     = 0;
        c.add("ANMP", anmp(v));
    }
    {
        ConfigValues v       = d;
        v.min_bead_width     = 50;
        v.min_feature_size   = 10;
        v.filter_deviation   = 40;
        v.transition_length  = 150;
        v.transition_angle   = 25;
        v.distribution_count = 3;
        v.min_length_factor  = 1.2;
        v.max_resolution     = 0.2;
        v.max_deviation      = 0.05;
        v.nozzles            = {0.25};
        c.add("ANMP", anmp(v));
    }
    {
        ConfigValues v = d;
        v.nozzles      = {0.35};
        v.min_bead_width = 33.3;
        c.add("ANMP", anmp(v));
    }
    out.put("params", c, true);
}

} // namespace

bool record_walls(Out &out, std::string &error)
{
    const std::vector<Shape> own  = shapes();
    const std::vector<Shape> mesh = mesh_slices(error);
    if (!error.empty())
        return false;
    record_helpers(out, own, mesh);
    record_walls_whole(out, own, mesh);
    record_params(out);
    return true;
}

} // namespace probe::arachne
