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
// fill.sop1's FLGP and FLEX records, which piece fill-2 replays (parity/sop1/fill.md;
// orca-o3-probe-fill, 2026-11-13): Fill::_create_gap_fill (the gap fill with the medial axis and
// variable_width) on a collection that already holds extrusions, and Fill::fill_surface_extrusion
// through StubFill, a filler of the probe's own whose fill_surface and fill_surface_arachne
// return the lines a record gives (or throw InfillFailedException) and set the spacing it gives,
// so that FillBase.cpp 133-191 is recorded apart from any pattern. Each record holds its inputs
// and then the engine's results.
#include "fill.hpp"

#include <algorithm>
#include <cmath>

#include "libslic3r/ClipperUtils.hpp"
#include "libslic3r/ShortestPath.hpp"

namespace probe {
namespace fill {

using namespace Slic3r;

namespace {

class GapFill : public Fill
{
public:
    Fill *clone() const override { return new GapFill(*this); }
    bool  is_self_crossing() override { return false; }
};

// fill_surface and fill_surface_arachne return what the record gives.
class StubFill : public Fill
{
public:
    bool           stub_no_sort = false;
    double         set_spacing  = 0.;
    bool           throws       = false;
    Polylines      give;
    ThickPolylines give_thick;

    Fill *clone() const override { return new StubFill(*this); }
    bool  is_self_crossing() override { return false; }
    bool  no_sort() const override { return stub_no_sort; }
    Polylines fill_surface(const Surface *, const FillParams &) override
    {
        this->spacing = set_spacing;
        if (throws)
            throw InfillFailedException();
        return give;
    }
    ThickPolylines fill_surface_arachne(const Surface *, const FillParams &) override
    {
        this->spacing = set_spacing;
        if (throws)
            throw InfillFailedException();
        return give_thick;
    }
};

// What a gap fill or extrusion case shares: the filler's state, the surface, the parameters and
// the configs.
struct Setup
{
    FillerState s;
    Surface     surface;
    FillParams  params;
    double      nozzle     = 0.4;
    int         gap_target = int(gftEverywhere);
    double      gap_filter = 0.;
    Polylines   lines;
};

void set_configs(Configs &c, const Setup &u)
{
    c.print.nozzle_diameter.values = {u.nozzle};
    c.object.gap_fill_target.value = GapFillTarget(u.gap_target);
    c.region.filter_out_gap_fill.value = u.gap_filter;
}

ExPolygons shrunk(const ExPolygon &e, double by_mm)
{
    if (by_mm <= 0.)
        return {e};
    return offset_ex(e, -float(mm(by_mm)));
}

// A setup over e: a flow, a spacing, lines `factor` spacings apart (so that gaps are left when
// factor > 1) and the region the gap fill looks at.
Setup setup(Gen &g, const ExPolygon &e, bool bridge, double factor)
{
    Setup u;
    u.nozzle         = g.one_in(5) ? 0.6 : 0.4;
    const double h   = std::min(0.75 * u.nozzle, g.pick(std::vector<double>{0.08, 0.12, 0.16, 0.2, 0.24, 0.28}));
    const double w   = u.nozzle * g.real(1.0, 1.25);
    u.params.flow    = bridge ? Flow::bridging_flow(float(u.nozzle), float(u.nozzle)) : Flow(float(w), float(h), float(u.nozzle));
    u.s.spacing      = u.params.flow.spacing();
    u.s.layer_id     = size_t(g.range(1, 80));
    u.s.z            = h * double(u.s.layer_id + 1);
    u.s.angle        = float(PI / 4.);
    u.s.bbox         = get_extents(e.contour);
    u.s.no_overlap   = shrunk(e, g.one_in(3) ? 0. : u.s.spacing * g.real(0.05, 0.3));
    u.surface        = Surface(g.one_in(2) ? stTop : stInternalSolid, e);
    u.params.density = 1.f;
    u.params.dont_adjust     = false;
    u.params.anchor_length   = 1000.f;
    u.params.anchor_length_max = 1000.f;
    u.params.resolution      = 0.012;
    u.params.layer_height    = h;
    u.params.extrusion_role  = bridge ? erBridgeInfill : u.surface.surface_type == stTop ? erTopSolidInfill : erSolidInfill;
    u.params.pattern         = ipRectilinear;
    u.lines                  = clipped_lines(e, g.real(-PI, PI), u.s.spacing * factor, g.real(0, 1));
    u.gap_target             = int(g.below(3));
    u.gap_filter             = g.one_in(3) ? 0. : g.real(0, 1.5);
    return u;
}

Payload put_gap(Setup &u)
{
    Configs cfg;
    set_configs(cfg, u);
    GapFill f;
    u.s.apply(f, cfg);
    FillParams q = u.params;
    q.config     = &cfg.region;
    Payload p;
    put_filler(p, f);
    put(p, u.surface);
    put(p, q);
    put(p, cfg);
    const Flow     &flow = q.flow;
    const double    mm3  = flow.mm3_per_mm();
    put(p, u.lines);
    p.u8(uint8_t(q.extrusion_role)).f64(mm3).f32(flow.width()).f32(flow.height());
    ExtrusionEntityCollection out;
    extrusion_entities_append_paths(out.entities, Polylines(u.lines), q.extrusion_role, mm3, flow.width(), flow.height());
    const size_t before = out.entities.size();
    take_errors();
    try {
        FillAccess::create_gap_fill(f, &u.surface, q, &out);
        // The call only appends: the entities after the paths, and how many paths stayed.
        const ExtrusionEntitiesPtr appended(out.entities.begin() + std::min(before, out.entities.size()), out.entities.end());
        p.u8(0).u64(uint64_t(std::min(before, out.entities.size())));
        put(p, appended);
    } catch (const std::exception &e) {
        put_threw(p, e);
    }
    p.str(take_errors());
    return p;
}

// Thick lines over the lines of a setup: a width (scaled) at each point, each segment its two
// ends' widths, the ends now and then marked.
ThickPolylines thick_lines(Gen &g, const Polylines &lines, double spacing_mm)
{
    ThickPolylines out;
    for (const Polyline &pl : lines) {
        if (pl.points.size() < 2)
            continue;
        ThickPolyline t;
        t.points = pl.points;
        std::vector<double> at;
        for (size_t i = 0; i < pl.points.size(); ++i)
            at.push_back(double(mm(spacing_mm * g.real(0.5, 1.5))));
        for (size_t i = 0; i + 1 < at.size(); ++i) {
            t.width.push_back(at[i]);
            t.width.push_back(at[i + 1]);
        }
        t.endpoints = std::make_pair(g.one_in(2), g.one_in(2));
        out.push_back(std::move(t));
    }
    return out;
}

struct StubCase
{
    Setup          u;
    bool           no_sort = false;
    double         set_spacing = 0.;
    bool           throws  = false;
    ThickPolylines thick;
};

Payload put_stub(StubCase &c)
{
    Configs cfg;
    set_configs(cfg, c.u);
    StubFill f;
    c.u.s.apply(f, cfg);
    f.stub_no_sort = c.no_sort;
    f.set_spacing  = c.set_spacing;
    f.throws       = c.throws;
    f.give         = c.u.lines;
    f.give_thick   = c.thick;
    FillParams q   = c.u.params;
    q.config       = &cfg.region;
    Payload p;
    put_filler(p, f);
    p.boolean(c.no_sort).f64(c.set_spacing).boolean(c.throws);
    put(p, c.u.lines);
    put(p, c.thick);
    put(p, c.u.surface);
    put(p, q);
    put(p, cfg);
    Entities out;
    take_errors();
    try {
        f.fill_surface_extrusion(&c.u.surface, q, out.v);
        p.u8(0);
        put(p, out.v);
    } catch (const std::exception &e) {
        put_threw(p, e);
    }
    p.str(take_errors());
    return p;
}

} // namespace

void record_extrusion(Sets &sets, const std::vector<Shape> &sections)
{
    // The hand-made cases run on the hand shapes at half their size.
    std::vector<Shape> shapes = hand_shapes();
    for (Shape &s : shapes)
        s.e = half(s.e);
    {
        Recorder r(sets, "FLGP", 30, 9000, 40000);
        Gen      g(0xF1A0007ull);
        // Lines 1.4 spacings apart over the square, everywhere, with each way the gap fill returns early or filters.
        auto base = [&](size_t shape) {
            Setup u = setup(g, shapes[shape].e, false, 1.4);
            u.gap_target = int(gftEverywhere);
            u.gap_filter = 0.;
            u.surface.surface_type = stTop;
            u.params.extrusion_role = erTopSolidInfill;
            return u;
        };
        for (size_t shape : {size_t(0), size_t(2), size_t(5), size_t(7), size_t(9)}) {
            Setup u = base(shape);
            r.edge(put_gap(u));
        }
        {
            Setup u = base(0);
            u.gap_target = int(gftNowhere);
            r.edge(put_gap(u));
        }
        for (int target : {int(gftTopBottom), int(gftEverywhere)}) {
            Setup u = base(2);
            u.gap_target = target;
            u.surface.surface_type = stInternalSolid;
            u.params.extrusion_role = erSolidInfill;
            r.edge(put_gap(u));
        }
        {
            Setup u = base(0);
            u.params.density = 0.5f;
            r.edge(put_gap(u));
        }
        {
            Setup u = setup(g, shapes[0].e, true, 1.4);
            u.gap_target = int(gftEverywhere);
            r.edge(put_gap(u));
        }
        {
            Setup u = base(0);
            u.s.configs = false;
            r.edge(put_gap(u));
        }
        {
            Setup u = base(2);
            u.gap_filter = 50.;
            r.edge(put_gap(u));
        }
        {
            Setup u = base(0);
            u.s.no_overlap.clear();
            r.edge(put_gap(u));
        }
        {
            Setup u = base(5);
            u.lines.clear();
            r.edge(put_gap(u));
        }
        for (int i = 0; i < 200; ++i) {
            const ExPolygon e = (!sections.empty() && g.one_in(6)) ? g.pick(sections).e : random_shape(g, g.real(1, 25));
            Setup           u = setup(g, e, g.one_in(8), g.pick(std::vector<double>{1., 1.15, 1.4, 1.8, 2.5}));
            if (g.one_in(5))
                u.params.density = float(g.real(0.5, 1.));
            if (g.one_in(10))
                u.s.configs = false;
            r.seeded(put_gap(u));
        }
    }
    {
        Recorder r(sets, "FLEX", 30, 9000, 55000);
        Gen      g(0xF1A0008ull);
        auto     base = [&](size_t shape, double factor) {
            StubCase c;
            c.u           = setup(g, shapes[shape].e, false, factor);
            c.u.gap_target = int(gftEverywhere);
            c.u.gap_filter = 0.;
            c.u.surface.surface_type = stInternalSolid;
            c.u.params.extrusion_role = erSolidInfill;
            c.set_spacing = c.u.s.spacing;
            return c;
        };
        r.edge([&]() { StubCase c = base(0, 1.); return put_stub(c); }());
        r.edge([&]() {
            StubCase c = base(0, 5.);
            c.u.surface.surface_type = stInternal;
            c.u.params.extrusion_role = erInternalInfill;
            c.u.params.density = 0.2f;
            c.u.params.using_internal_flow = true;
            c.set_spacing = c.u.s.spacing * 1.3;
            return put_stub(c);
        }());
        r.edge([&]() { StubCase c = base(0, 1.); c.u.params.can_reverse = false; return put_stub(c); }());
        r.edge([&]() { StubCase c = base(0, 1.); c.no_sort = true; return put_stub(c); }());
        r.edge([&]() { StubCase c = base(0, 1.); c.throws = true; return put_stub(c); }());
        r.edge([&]() { StubCase c = base(0, 1.); c.u.lines.clear(); return put_stub(c); }());
        r.edge([&]() {
            StubCase c = base(7, 1.2);
            c.u.params.use_arachne = true;
            c.thick = thick_lines(g, c.u.lines, c.u.s.spacing);
            return put_stub(c);
        }());
        r.edge([&]() {
            // with_spacing(0.1) of a 0.45 by 0.3 flow is narrower than its height: it throws.
            StubCase c = base(0, 1.);
            c.u.nozzle      = 0.4;
            c.u.params.flow = Flow(0.45f, 0.3f, 0.4f);
            c.set_spacing   = 0.1;
            return put_stub(c);
        }());
        r.edge([&]() { StubCase c = base(0, 1.); c.set_spacing = c.u.s.spacing * 1.1; return put_stub(c); }());
        r.edge([&]() {
            StubCase c;
            c.u = setup(g, shapes[0].e, true, 1.);
            c.u.surface.surface_type = stBottomBridge;
            c.u.surface.bridge_angle = 0.5;
            c.u.params.dont_adjust = true;
            c.set_spacing = c.u.s.spacing * 1.05;
            return put_stub(c);
        }());
        r.edge([&]() {
            StubCase c = base(2, 1.4);
            c.u.surface.surface_type = stTop;
            c.u.params.extrusion_role = erTopSolidInfill;
            return put_stub(c);
        }());
        r.edge([&]() { StubCase c = base(2, 1.4); c.u.s.configs = false; return put_stub(c); }());
        r.edge([&]() { StubCase c = base(2, 1.4); c.u.gap_target = int(gftTopBottom); return put_stub(c); }());
        r.edge([&]() {
            StubCase c = base(9, 1.4);
            c.u.surface.surface_type = stBottom;
            c.u.params.extrusion_role = erBottomSurface;
            c.u.params.can_reverse = false;
            return put_stub(c);
        }());
        for (int i = 0; i < 250; ++i) {
            const ExPolygon e      = (!sections.empty() && g.one_in(6)) ? g.pick(sections).e : random_shape(g, g.real(1, 25));
            const bool      bridge = g.one_in(8);
            StubCase        c;
            c.u = setup(g, e, bridge, g.pick(std::vector<double>{1., 1.15, 1.4, 2., 4.}));
            if (!bridge && g.one_in(3)) {
                c.u.surface.surface_type     = stInternal;
                c.u.params.extrusion_role    = erInternalInfill;
                c.u.params.density           = float(g.real(0.05, 0.6));
                c.u.params.using_internal_flow = true;
            }
            c.no_sort     = g.one_in(4);
            c.throws      = g.one_in(25);
            c.set_spacing = g.one_in(3) ? c.u.s.spacing * g.real(1., 1.2) : c.u.s.spacing;
            c.u.params.can_reverse = !g.one_in(5);
            if (g.one_in(4)) {
                c.u.params.use_arachne = true;
                c.thick = thick_lines(g, c.u.lines, c.u.s.spacing);
            }
            if (g.one_in(12) && c.u.params.extrusion_role != erTopSolidInfill)
                c.u.s.configs = false;
            r.seeded(put_stub(c));
        }
    }
}

} // namespace fill
} // namespace probe
