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
// fill.sop1's records of piece fill-1 (parity/sop1/fill.md; orca-o3-probe-fill, 2026-11-13):
// FLDF (FillParams' and Fill's defaults and Fill's two anchor statics), FLPA (full_infill and
// dont_connect), FLAS (Fill::_adjust_solid_spacing), FLDR (Fill::_infill_direction and
// Fill::_layer_angle, with what _infill_direction prints) and FLSU (Fill::fill_surface and
// Fill::fill_surface_arachne of FillBase.cpp, through SpyFill, a filler of the probe's own whose
// _fill_surface_single only keeps what it is given: the offset regions, the direction and the
// thickness in layers). Each record holds its inputs and then the engine's results.
#include "fill.hpp"

#include <cfloat>
#include <cmath>
#include <limits>

#include "libslic3r/ClipperUtils.hpp"

namespace probe {
namespace fill {

using namespace Slic3r;

namespace {

// A filler of the probe's own: Fill's base fill_surface and fill_surface_arachne call its
// _fill_surface_single, which keeps its arguments and fills nothing.
class SpyFill : public Fill
{
public:
    struct Call
    {
        unsigned int            thickness_layers;
        std::pair<float, Point> direction;
        ExPolygon               expolygon;
    };
    std::vector<Call> calls;

    Fill *clone() const override { return new SpyFill(*this); }
    bool  is_self_crossing() override { return false; }

protected:
    void _fill_surface_single(const FillParams &, unsigned int thickness_layers, const std::pair<float, Point> &direction, ExPolygon expolygon, Polylines &) override
    {
        calls.push_back({thickness_layers, direction, std::move(expolygon)});
    }
    void _fill_surface_single(const FillParams &, unsigned int thickness_layers, const std::pair<float, Point> &direction, ExPolygon expolygon, ThickPolylines &) override
    {
        calls.push_back({thickness_layers, direction, std::move(expolygon)});
    }
};

float next_up(float v) { return std::nextafter(v, std::numeric_limits<float>::infinity()); }
float next_down(float v) { return std::nextafter(v, -std::numeric_limits<float>::infinity()); }

void put_fldf(Sets &sets)
{
    Payload          p;
    const FillParams params{};
    put(p, params);
    p.boolean(params.full_infill()).boolean(params.dont_connect());
    p.f32(Fill::infill_anchor).f32(Fill::infill_anchor_max);
    SpyFill f;
    put_filler(p, f);
    put(p, Surface());
    Recorder(sets, "FLDF", 0, 0).edge(p);
}

void put_flpa(Sets &sets)
{
    Recorder           r(sets, "FLPA", 200, 64);
    auto               one = [](float density, float anchor_max) {
        FillParams q;
        q.density           = density;
        q.anchor_length_max = anchor_max;
        Payload p;
        p.f32(density).f32(anchor_max).boolean(q.full_infill()).boolean(q.dont_connect());
        return p;
    };
    const float        nan = std::numeric_limits<float>::quiet_NaN();
    const float        inf = std::numeric_limits<float>::infinity();
    std::vector<float> densities{0.9999f, next_up(0.9999f), next_down(0.9999f), 1.f, 0.f, -0.f, 1.0001f, 0.5f, 0.15f, nan, inf, -inf, 2.f};
    std::vector<float> anchors{0.05f, next_up(0.05f), next_down(0.05f), 0.f, -0.f, 1000.f, 20.f, 1.8f, nan, inf, -inf, 0.049f, 0.051f};
    for (size_t i = 0; i < densities.size(); ++i)
        r.edge(one(densities[i], anchors[i]));
    Gen g(0xF1A0001ull);
    for (int i = 0; i < 400; ++i) {
        const float d = g.one_in(4) ? float(g.real(0.999, 1.0001)) : float(g.real(0, 1.2));
        const float a = g.one_in(4) ? float(g.real(0.049, 0.051)) : float(g.real(0, 30));
        r.seeded(one(d, a));
    }
}

void put_flas(Sets &sets)
{
    Recorder r(sets, "FLAS", 250, 64);
    auto     one = [](int64_t width, int64_t distance) {
        Payload p;
        p.i64(width).i64(distance).i64(int64_t(Fill::_adjust_solid_spacing(coord_t(width), coord_t(distance))));
        return p;
    };
    const int64_t edges[][2] = {{0, 450000},       {1, 450000},          {450000, 450000},     {450001, 450000},     {899999, 450000},
                                {900000, 450000},  {900001, 450000},     {1349999, 450000},    {1350000, 450000},    {539999, 450000},
                                {540000, 450000},  {20000000, 450000},   {20000000, 1},        {1, 1},               {2, 1},
                                {3, 2},            {100000000, 3333333}, {(int64_t(1) << 53), 1000000}, {(int64_t(1) << 53) + 1, 3},
                                {(int64_t(1) << 62), 450000},            {123456789, 98765}};
    for (const auto &e : edges)
        r.edge(one(e[0], e[1]));
    Gen g(0xF1A0002ull);
    for (int i = 0; i < 600; ++i) {
        const int64_t distance = g.one_in(5) ? g.range(1, 100) : g.range(100000, 3000000);
        const int64_t width    = g.one_in(4) ? distance * g.range(0, 6) + g.range(-2, 2) : g.range(0, 300000000);
        r.seeded(one(width < 0 ? 0 : width, distance));
    }
}

// A filler state with the angle, box and layer of a case.
struct DirectionCase
{
    FillerState s;
    Surface     surface;
};

void put_direction(Payload &p, const DirectionCase &c, std::string &error, bool &ok)
{
    Configs cfg;
    SpyFill f;
    c.s.apply(f, cfg);
    put_filler(p, f);
    put(p, c.surface);
    std::pair<float, Point> d;
    std::string             out;
    if (!capture_stdout([&]() { d = FillAccess::infill_direction(f, &c.surface); }, out, error)) {
        ok = false;
        return;
    }
    p.f32(d.first);
    put(p, d.second);
    p.str(out);
    p.f32(FillAccess::layer_angle(f, f.layer_id)).f32(FillAccess::layer_angle(f, f.layer_id + 1));
}

BoundingBox box_mm(double x0, double y0, double x1, double y1) { return BoundingBox(Point(mm(x0), mm(y0)), Point(mm(x1), mm(y1))); }

} // namespace

void record_base(Sets &sets, const std::vector<Shape> &)
{
    put_fldf(sets);
    put_flpa(sets);
    put_flas(sets);
}

bool record_direction(Sets &sets, std::string &error)
{
    Recorder                   r(sets, "FLDR", 120, 400, 20000);
    const std::vector<Shape>   shapes = hand_shapes();
    bool                       ok     = true;
    auto                       base   = [&]() {
        DirectionCase c;
        c.s.layer_id = 0;
        c.s.angle    = float(PI / 4.);
        c.s.bbox     = box_mm(-10, -10, 10, 10);
        c.surface    = Surface(stInternal, shapes[0].e);
        return c;
    };
    std::vector<DirectionCase> edges;
    for (size_t layer : {size_t(0), size_t(1), size_t(2), size_t(3)}) {
        DirectionCase c = base();
        c.s.layer_id    = layer;
        edges.push_back(c);
    }
    {
        DirectionCase c = base();
        c.s.angle       = FLT_MAX;
        edges.push_back(c);
    }
    {
        DirectionCase c = base();
        c.s.angle       = FLT_MAX;
        c.s.layer_id    = 7;
        c.s.bbox        = BoundingBox(Point(0, 0), Point(-1, -1));
        edges.push_back(c);
    }
    {
        DirectionCase c = base();
        c.s.bbox        = BoundingBox();
        c.surface       = Surface(stInternal, shapes[2].e);
        edges.push_back(c);
    }
    {
        DirectionCase c = base();
        c.s.bbox        = box_mm(5, 5, 5, 9); // empty: no width
        c.surface       = Surface(stTop, shapes[4].e);
        edges.push_back(c);
    }
    for (double bridge : {0., -0., 1.2, 3.1, -1., -0.5}) {
        DirectionCase c         = base();
        c.s.layer_id            = 3;
        c.surface.bridge_angle  = bridge;
        c.surface.surface_type  = bridge >= 0 ? stBottomBridge : stBottom;
        edges.push_back(c);
    }
    {
        DirectionCase c = base();
        c.s.layer_id    = size_t(-1);
        edges.push_back(c);
    }
    for (bool fixed : {true, false}) {
        DirectionCase c = base();
        c.s.layer_id    = 3;
        c.s.fixed       = fixed;
        c.s.dont_alt    = !fixed;
        edges.push_back(c);
    }
    for (unsigned short thickness : {2, 3}) {
        DirectionCase c            = base();
        c.s.layer_id               = 5;
        c.surface.thickness_layers = thickness;
        c.surface.thickness        = 0.4 * thickness;
        edges.push_back(c);
    }
    for (float a : {-1.5f, 0.f, -0.f, 3.14159274f, 1e30f, float(PI / 2.)}) {
        DirectionCase c = base();
        c.s.angle       = a;
        c.s.layer_id    = 1;
        edges.push_back(c);
    }
    for (const DirectionCase &c : edges) {
        Payload p;
        put_direction(p, c, error, ok);
        if (!ok)
            return false;
        r.edge(p);
    }
    Gen g(0xF1A0003ull);
    for (int i = 0; i < 300; ++i) {
        DirectionCase c;
        c.s.layer_id = g.one_in(12) ? size_t(-1) : size_t(g.range(0, 200));
        c.s.angle    = g.one_in(20) ? FLT_MAX : g.one_in(2) ? float(PI / 180. * double(15 * g.range(-12, 12))) : float(g.real(-7, 7));
        const double x = g.real(-80, 80), y = g.real(-80, 80);
        c.s.bbox     = g.one_in(6) ? BoundingBox(Point(0, 0), Point(-1, -1)) : box_mm(x, y, x + g.real(0.5, 150), y + g.real(0.5, 150));
        c.s.fixed    = g.one_in(8);
        c.s.dont_alt = g.one_in(8);
        c.surface    = Surface(SurfaceType(g.range(0, 7)), random_shape(g, g.real(1, 40)));
        if (g.one_in(4))
            c.surface.bridge_angle = g.real(0, PI);
        c.surface.thickness_layers = (unsigned short) g.range(1, 3);
        Payload p;
        put_direction(p, c, error, ok);
        if (!ok)
            return false;
        r.seeded(p);
    }
    return true;
}

bool record_surface(Sets &sets, const std::vector<Shape> &sections, std::string &error)
{
    Recorder r(sets, "FLSU", 40, 3000, 35000);
    // entry 0 is fill_surface, 1 fill_surface_arachne.
    auto one = [&](Payload &p, uint8_t entry, const FillerState &s, const Surface &surface) {
        Configs cfg;
        SpyFill f;
        s.apply(f, cfg);
        FillParams params;
        params.use_arachne = entry == 1;
        p.u8(entry);
        put_filler(p, f);
        put(p, surface);
        put(p, params);
        std::string out;
        size_t      returned = 0;
        if (!capture_stdout(
                [&]() {
                    if (entry == 0)
                        returned = f.fill_surface(&surface, params).size();
                    else
                        returned = f.fill_surface_arachne(&surface, params).size();
                },
                out, error))
            return false;
        p.str(out);
        p.u64(f.calls.size());
        for (const SpyFill::Call &c : f.calls) {
            p.u32(uint32_t(c.thickness_layers)).f32(c.direction.first);
            put(p, c.direction.second);
            put(p, c.expolygon);
        }
        p.u64(returned);
        return true;
    };
    const std::vector<Shape> shapes = hand_shapes();
    // The overlap against half the spacing; a spacing that cuts the dumbbell's neck; no spacing;
    // an undefined angle (printed once for each region).
    const struct
    {
        size_t shape;
        double spacing, overlap;
        float  angle;
    } variants[] = {{2, 0.45, 0.15, 0.f}, {2, 0.45, 0.3, 0.f}, {9, 0.7, 0., 1.f}, {9, 0.5, 0., 1.f}, {0, 0., 0., 0.f}, {9, 0.7, 0., FLT_MAX}, {7, 0.62, -0.1, 2.f}};
    for (const auto &v : variants) {
        FillerState s;
        s.layer_id = 9;
        s.spacing  = v.spacing;
        s.overlap  = v.overlap;
        s.angle    = v.angle;
        Payload p;
        if (!one(p, 0, s, Surface(stInternalSolid, shapes[v.shape].e)))
            return false;
        r.edge(p);
    }
    for (const Shape &sh : shapes)
        for (uint8_t entry = 0; entry < 2; ++entry) {
            FillerState s;
            s.layer_id = 4;
            s.spacing  = 0.45;
            s.angle    = float(PI / 4.);
            s.bbox     = get_extents(sh.e.contour);
            Payload p;
            if (!one(p, entry, s, Surface(stInternal, sh.e)))
                return false;
            r.edge(p);
        }
    Gen g(0xF1A0004ull);
    for (int i = 0; i < 300; ++i) {
        FillerState s;
        s.layer_id = size_t(g.range(0, 60));
        s.spacing  = g.real(0.25, 0.9);
        s.overlap  = g.one_in(3) ? 0. : g.real(-0.1, 0.45);
        s.angle    = float(g.real(-PI, PI));
        const ExPolygon e = (!sections.empty() && g.one_in(5)) ? g.pick(sections).e : random_shape(g, g.real(0.5, 30));
        if (!g.one_in(4))
            s.bbox = get_extents(e.contour);
        Surface surface(SurfaceType(g.range(0, 7)), e);
        surface.thickness_layers = (unsigned short) g.range(1, 2);
        Payload p;
        if (!one(p, uint8_t(g.below(2)), s, surface))
            return false;
        r.seeded(p);
    }
    return true;
}

} // namespace fill
} // namespace probe
