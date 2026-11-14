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
// fill.sop1's FLRP records (piece fill-3: rectilinear, monotonic, monotonicline and crosshatch)
// and FLCP records (piece fill-4: concentric and concentricinternal; parity/sop1/fill.md;
// orca-o3-probe-fill, 2026-11-13). Each case is a surface with a filler and parameters set the
// way Layer::make_fills sets them for one kind of surface (sparse, internal solid, top, bottom,
// bridge, internal bridge), filled twice by a new filler of Fill::new_from_type: once by
// fill_surface (or fill_surface_arachne when the parameters use Arachne), once by
// fill_surface_extrusion, the call make_fills makes. A case whose path reaches multiline_fill past
// its guard or fill_surface_by_multilines is marked out of domain A: a port stops there with
// NotPorted.
#include "fill.hpp"

#include <algorithm>
#include <cmath>
#include <memory>

#include "libslic3r/ClipperUtils.hpp"

namespace probe {
namespace fill {

using namespace Slic3r;

namespace {

// The kinds of surface make_fills fills, as group_fills gives each its role and density.
enum Kind : int { kSparse, kSolid, kTop, kBottom, kBridge, kInternalBridge, kKinds };

const SurfaceType   kind_surface[kKinds] = {stInternal, stInternalSolid, stTop, stBottom, stBottomBridge, stInternalBridge};
const ExtrusionRole kind_role[kKinds]    = {erInternalInfill, erSolidInfill, erTopSolidInfill, erBottomSurface, erBridgeInfill, erInternalBridgeInfill};

struct PatternCase
{
    InfillPattern pattern = ipRectilinear;
    FillerState   s;
    Surface       surface;
    FillParams    params;
    double        nozzle     = 0.4;
    int           gap_target = int(gftEverywhere);
    double        gap_filter = 0.;
};

// Whether a port computes the case: the rectilinear filler past full infill and a single line
// goes to fill_surface_by_multilines, crosshatch and the concentric loops call multiline_fill
// (past its guard when multiline > 1), and the concentric Arachne branch at full density does not.
bool in_domain_a(const PatternCase &c)
{
    const FillParams &q = c.params;
    switch (c.pattern) {
    case ipMonotonic:
    case ipMonotonicLine: return true;
    case ipRectilinear: return q.full_infill() || q.multiline == 1;
    case ipCrossHatch: return q.multiline <= 1;
    case ipConcentric: return (q.use_arachne && q.density > 0.9999f && !q.dont_adjust) || q.multiline <= 1;
    case ipConcentricInternal: return true;
    default: return false;
    }
}

ExPolygons shrunk(const ExPolygon &e, double by_mm)
{
    if (by_mm <= 0.)
        return {e};
    return offset_ex(e, -float(mm(by_mm)));
}

// A case over e of one kind, set as make_fills sets it (group_fills' flow, spacing, anchors and
// density; the filler's layer, z, angle, seam gap and object box).
PatternCase pattern_case(Gen &g, InfillPattern pattern, const ExPolygon &e, int kind)
{
    PatternCase c;
    c.pattern         = pattern;
    c.nozzle          = g.one_in(6) ? 0.6 : g.one_in(8) ? 0.25 : 0.4;
    const double h    = std::min(0.75 * c.nozzle, g.pick(std::vector<double>{0.08, 0.12, 0.16, 0.2, 0.24, 0.28}));
    const bool bridge = kind == kBridge || kind == kInternalBridge;
    c.surface         = Surface(kind_surface[kind], e);
    if (bridge)
        c.surface.bridge_angle = g.one_in(3) ? -1. : g.real(0, PI);
    c.surface.thickness        = g.one_in(3) ? -1. : h;
    c.surface.thickness_layers = (unsigned short) (g.one_in(6) ? 2 : 1);
    FillParams &q              = c.params;
    q.flow                     = bridge ? Flow::bridging_flow(float(c.nozzle * g.real(0.85, 1.)), float(c.nozzle)) :
                                          Flow(float(c.nozzle * g.real(1.0, 1.25)), float(h), float(c.nozzle));
    c.s.spacing                = q.flow.spacing();
    q.extrusion_role           = kind_role[kind];
    q.using_internal_flow      = kind == kSparse;
    q.pattern                  = pattern;
    q.layer_height             = h;
    q.resolution               = g.one_in(3) ? 0.012 : 0.01;
    q.use_arachne              = pattern == ipConcentric || pattern == ipConcentricInternal;
    q.multiline                = 1;
    q.dont_adjust              = false;
    q.anchor_length            = 1000.f;
    q.anchor_length_max        = 1000.f;
    switch (kind) {
    case kSparse: {
        q.density = float(g.pick(std::vector<double>{0.05, 0.1, 0.15, 0.2, 0.25, 0.3, 0.4, 0.5, 0.8}));
        if (g.one_in(4))
            q.density = float(g.real(0.05, 0.95));
        if (g.one_in(3))
            c.s.spacing *= g.real(0.92, 1.08);
        // infill_anchor 400% of the spacing and infill_anchor_max 20 mm by default.
        float anchor_max = 20.f;
        if (g.one_in(4))
            anchor_max = g.one_in(3) ? 0.f : g.one_in(2) ? float(g.real(0.5, 5.)) : 1000.f;
        float anchor = float(4. * c.s.spacing);
        if (g.one_in(4))
            anchor = g.one_in(2) ? 0.f : float(g.real(0.1, 10.));
        q.anchor_length_max = anchor_max;
        q.anchor_length     = std::min(anchor, anchor_max);
        break;
    }
    case kTop:
    case kBottom: q.density = g.one_in(5) ? float(g.real(0.6, 0.99)) : 1.f; break;
    case kBridge:
    case kInternalBridge:
        q.density     = g.one_in(4) ? float(g.real(0.6, 1.)) : 1.f;
        q.dont_adjust = true;
        break;
    default: q.density = 1.f; break;
    }
    c.s.layer_id  = size_t(g.range(1, 200));
    c.s.z         = 0.2 + h * double(c.s.layer_id);
    c.s.angle     = float(g.one_in(4) ? g.real(-PI, PI) : g.pick(std::vector<double>{45., 0., 90., 30., 135.}) * PI / 180.);
    c.s.fixed     = g.one_in(6);
    c.s.link_max  = 0;
    c.s.loop_clip = mm(c.nozzle * g.pick(std::vector<double>{0.1, 0.1, 0.15, 0.}));
    BoundingBox bb = get_extents(e.contour);
    bb.offset(mm(g.real(0, 20)));
    c.s.bbox       = bb;
    c.s.dont_alt   = g.one_in(10);
    c.s.no_overlap = shrunk(e, g.one_in(3) ? 0. : c.s.spacing * g.real(0.05, 0.4));
    c.gap_target   = g.one_in(3) ? int(g.below(3)) : int(gftEverywhere);
    c.gap_filter   = g.one_in(2) ? 0. : g.real(0, 1.5);
    return c;
}

void set_configs(Configs &cfg, const PatternCase &c)
{
    cfg.print.nozzle_diameter.values = {c.nozzle};
    cfg.object.gap_fill_target.value = GapFillTarget(c.gap_target);
    cfg.region.filter_out_gap_fill.value = c.gap_filter;
}

std::unique_ptr<Fill> filler(const PatternCase &c, const Configs &cfg)
{
    std::unique_ptr<Fill> f(Fill::new_from_type(c.pattern));
    c.s.apply(*f, cfg);
    return f;
}

// pattern, in_domain_a, filler, surface, params, configs, whether fill_surface_extrusion follows;
// then fill_surface (or fill_surface_arachne) and, when it follows, fill_surface_extrusion, each
// on a new filler, each with the filler's spacing after it and what the engine logged.
Payload put_pattern(const PatternCase &c, bool concentric, bool extrusion)
{
    Configs cfg;
    set_configs(cfg, c);
    FillParams q = c.params;
    q.config     = &cfg.region;
    Payload p;
    p.u8(uint8_t(c.pattern)).boolean(in_domain_a(c));
    put_filler(p, *filler(c, cfg));
    put(p, c.surface);
    put(p, q);
    put(p, cfg);
    p.boolean(extrusion);
    {
        std::unique_ptr<Fill> f     = filler(c, cfg);
        const bool            thick = concentric && q.use_arachne;
        if (concentric)
            p.u8(thick ? 1 : 0);
        take_errors();
        try {
            if (thick) {
                const ThickPolylines out = f->fill_surface_arachne(&c.surface, q);
                p.u8(0);
                put(p, out);
            } else {
                const Polylines out = f->fill_surface(&c.surface, q);
                p.u8(0);
                put(p, out);
            }
        } catch (const std::exception &e) {
            put_threw(p, e);
        }
        p.f64(f->spacing);
        p.str(take_errors());
    }
    if (extrusion) {
        std::unique_ptr<Fill> f = filler(c, cfg);
        Entities              out;
        take_errors();
        try {
            f->fill_surface_extrusion(&c.surface, q, out.v);
            p.u8(0);
            put(p, out.v);
        } catch (const std::exception &e) {
            put_threw(p, e);
        }
        p.f64(f->spacing);
        p.str(take_errors());
    }
    return p;
}

// The sections small enough for a seeded pattern case.
std::vector<Shape> small(const std::vector<Shape> &sections, double max_mm)
{
    std::vector<Shape> out;
    for (const Shape &s : sections) {
        const BoundingBox bb = get_extents(s.e.contour);
        if (double(bb.size().x()) <= double(mm(max_mm)) && double(bb.size().y()) <= double(mm(max_mm)) && s.e.area() > 0.)
            out.push_back(s);
    }
    return out;
}

} // namespace

void record_patterns(Sets &sets, const std::vector<Shape> &sections)
{
    const std::vector<Shape> shapes = hand_shapes();
    const std::vector<Shape> pieces = small(sections, 40.);
    {
        Recorder                         r(sets, "FLRP", 40, 8000, 160000);
        Gen                              g(0xF1A0009ull);
        const std::vector<InfillPattern> patterns = {ipRectilinear, ipMonotonic, ipMonotonicLine, ipCrossHatch};
        // The branches first, on hand shapes at half their size: anchors off and unlimited, a link
        // limit, the overlap (with fill_surface_extrusion), no line fitting, a full infill not
        // adjusted, multiline 2 and 3 (out of domain A for rectilinear sparse and crosshatch, in it
        // for monotonic) and multiline 2 at full density (in it for rectilinear).
        for (InfillPattern pattern : patterns) {
            {
                PatternCase c = pattern_case(g, pattern, half(shapes[11].e), kSparse);
                c.params.density = 0.2f;
                c.params.anchor_length = c.params.anchor_length_max = 0.f;
                r.edge(put_pattern(c, false, false));
            }
            {
                PatternCase c = pattern_case(g, pattern, half(shapes[3].e), kSparse);
                c.params.density = 0.3f;
                c.params.anchor_length = c.params.anchor_length_max = 1000.f;
                c.s.link_max = mm(3. * c.s.spacing);
                r.edge(put_pattern(c, false, false));
            }
            {
                // Crosshatch's lines at full density hold many points: its extrusion is in the
                // hand shapes' turn.
                PatternCase c = pattern_case(g, pattern, half(shapes[2].e), kSolid);
                c.s.overlap = 0.15;
                r.edge(put_pattern(c, false, pattern != ipCrossHatch));
            }
            {
                PatternCase c = pattern_case(g, pattern, shapes[10].e, kTop);
                r.edge(put_pattern(c, false, true));
            }
            {
                PatternCase c = pattern_case(g, pattern, half(shapes[4].e), kSolid);
                c.params.dont_adjust = true;
                r.edge(put_pattern(c, false, false));
            }
            for (int multiline : {2, 3}) {
                PatternCase c = pattern_case(g, pattern, half(shapes[0].e), kSparse);
                c.params.density   = 0.25f;
                c.params.multiline = multiline;
                r.edge(put_pattern(c, false, false));
            }
            {
                PatternCase c = pattern_case(g, pattern, half(shapes[1].e), kSolid);
                c.params.multiline = 2;
                r.edge(put_pattern(c, false, false));
            }
        }
        // Crosshatch's densities: the repeat ratio below 0.3, the wider lines below 0.999.
        for (double density : {0.1, 0.29, 0.31, 0.998, 0.999, 1.}) {
            PatternCase c = pattern_case(g, ipCrossHatch, half(shapes[7].e), density < 1. ? kSparse : kSolid);
            c.params.density = float(density);
            r.edge(put_pattern(c, false, false));
        }
        // Every hand shape under every pattern, the kinds turning so that each pattern meets each
        // kind; fill_surface_extrusion on every third.
        for (size_t i = 0; i < shapes.size(); ++i)
            for (size_t k = 0; k < patterns.size(); ++k) {
                PatternCase c = pattern_case(g, patterns[k], shapes[i].e, int((i + k) % kKinds));
                r.edge(put_pattern(c, false, (i + k) % 3 == 0));
            }
        for (int i = 0; i < 600; ++i) {
            const InfillPattern pattern = patterns[size_t(i) % patterns.size()];
            const ExPolygon     e       = (!pieces.empty() && g.one_in(5)) ? g.pick(pieces).e : random_shape(g, g.real(1, 25));
            PatternCase         c       = pattern_case(g, pattern, e, int(g.below(kKinds)));
            if (g.one_in(12))
                c.s.overlap = g.real(-0.1, 0.25);
            if (g.one_in(12))
                c.s.link_max = mm(g.real(0.5, 5.));
            const bool extrusion = g.one_in(4);
            r.seeded(put_pattern(c, false, extrusion));
        }
    }
    {
        Recorder r(sets, "FLCP", 30, 6000, 80000);
        Gen      g(0xF1A000Aull);
        // The branches first, on hand shapes at half their size: concentric's classic loops,
        // multiline 2 with and without Arachne and at full density, a full infill not adjusted
        // with and without Arachne, no seam gap; concentricinternal with no
        // no_overlap_expolygons and with two.
        for (size_t shape : {size_t(0), size_t(7), size_t(9), size_t(10)}) {
            PatternCase c = pattern_case(g, ipConcentric, shape == 10 ? shapes[shape].e : half(shapes[shape].e), kSolid);
            c.params.use_arachne = false;
            r.edge(put_pattern(c, true, shape == 0));
        }
        {
            PatternCase c = pattern_case(g, ipConcentric, half(shapes[2].e), kSparse);
            c.params.use_arachne = false;
            c.params.density = 0.3f;
            r.edge(put_pattern(c, true, false));
        }
        for (bool arachne : {false, true}) {
            PatternCase c = pattern_case(g, ipConcentric, half(shapes[0].e), kSparse);
            c.params.use_arachne = arachne;
            c.params.density     = 0.4f;
            c.params.multiline   = 2;
            r.edge(put_pattern(c, true, false));
        }
        {
            PatternCase c = pattern_case(g, ipConcentric, half(shapes[6].e), kSolid);
            c.params.multiline = 2;
            r.edge(put_pattern(c, true, false));
        }
        for (bool arachne : {false, true}) {
            PatternCase c = pattern_case(g, ipConcentric, half(shapes[11].e), kSolid);
            c.params.use_arachne = arachne;
            c.params.dont_adjust = true;
            r.edge(put_pattern(c, true, false));
        }
        {
            PatternCase c = pattern_case(g, ipConcentric, half(shapes[8].e), kTop);
            c.s.loop_clip = 0;
            r.edge(put_pattern(c, true, true));
        }
        {
            PatternCase c = pattern_case(g, ipConcentricInternal, half(shapes[0].e), kSolid);
            c.s.no_overlap.clear();
            r.edge(put_pattern(c, true, true));
        }
        {
            PatternCase c = pattern_case(g, ipConcentricInternal, half(shapes[0].e), kSolid);
            c.s.no_overlap = {half(shapes[0].e), half(shapes[12].e)};
            c.s.loop_clip  = 0;
            r.edge(put_pattern(c, true, true));
        }
        // Every hand shape under both: concentric with Arachne, as make_fills runs it, the kinds
        // turning through sparse, internal solid, top and bottom (fill_surface_extrusion on every
        // third); concentricinternal on internal solid, its loops drawn in
        // no_overlap_expolygons, the surface itself or shrunk (always fill_surface_extrusion, its
        // own).
        for (size_t i = 0; i < shapes.size(); ++i) {
            {
                PatternCase c = pattern_case(g, ipConcentric, shapes[i].e, int(i % 4));
                r.edge(put_pattern(c, true, i % 3 == 0));
            }
            {
                PatternCase c = pattern_case(g, ipConcentricInternal, shapes[i].e, kSolid);
                r.edge(put_pattern(c, true, true));
            }
        }
        for (int i = 0; i < 240; ++i) {
            const InfillPattern pattern = (i & 1) ? ipConcentricInternal : ipConcentric;
            const ExPolygon     e       = (!pieces.empty() && g.one_in(5)) ? g.pick(pieces).e : random_shape(g, g.real(1, 20));
            const int           kind    = pattern == ipConcentricInternal ? kSolid : int(g.below(4));
            PatternCase         c       = pattern_case(g, pattern, e, kind);
            if (pattern == ipConcentric && g.one_in(6))
                c.params.use_arachne = false;
            const bool extrusion = pattern == ipConcentricInternal || g.one_in(4);
            r.seeded(put_pattern(c, true, extrusion));
        }
    }
}

} // namespace fill
} // namespace probe
