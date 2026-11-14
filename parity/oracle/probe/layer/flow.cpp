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
// layer.sop1's Flow records of the piece layer-1 (orca-layer; parity/sop1/layer.md): LYFL a flow
// made each way and what follows from it, LYFS Flow's static helpers, LYFW
// Flow::extrusion_width of configurations. Recorded by orca-o3-probe-layer, 2026-11-13.
#include "common.hpp"

#include <iterator>

#include "libslic3r/PrintConfig.hpp"

namespace probe {
namespace layer {

namespace {

const float NOZZLES[] = {0.2f, 0.4f, 0.6f, 0.8f};

// A float-or-percent option as `f64` value and `bool` percent.
void put_fop(Payload &p, const ConfigOptionFloatOrPercent &o) { p.f64(o.value).boolean(o.percent); }

void put_mm3(Payload &p, const Flow &f)
{
    attempt(p, [&](Payload &q) { q.f64(f.mm3_per_mm()); });
}

// What follows from one flow: its fields, mm3_per_mm, the scaled values, the with_* flows and the
// comparisons with a reference flow.
void put_flow_ops(Payload &p, const Flow &f, float w2, float h2, float s2, float area, double ratio, const Flow &ref)
{
    put(p, f);
    put_mm3(p, f);
    p.i64(f.scaled_width()).i64(f.scaled_spacing()).i64(f.scaled_elephant_foot_spacing());
    p.f32(w2).f32(h2).f32(s2).f32(area).f64(ratio);
    attempt(p, [&](Payload &q) { put(q, f.with_width(w2)); });
    attempt(p, [&](Payload &q) { put(q, f.with_height(h2)); });
    attempt(p, [&](Payload &q) { put(q, f.with_spacing(s2)); });
    attempt(p, [&](Payload &q) { put(q, f.with_cross_section(area)); });
    attempt(p, [&](Payload &q) { put(q, f.with_flow_ratio(ratio)); });
    put(p, ref);
    p.boolean(f == ref).boolean(f != ref);
    attempt(p, [&](Payload &q) { q.boolean(f < ref); });
}

// LYFL: a flow made by Flow(w, h, nozzle), bridging_flow, new_from_config_width or Flow(), then
// what follows from it.
void record_flows(Sets &sets)
{
    SplitMix g(0x1A7EF10Full);
    struct Made
    {
        int                         how; // 0 Flow(w, h, nozzle), 1 bridging_flow, 2 new_from_config_width, 3 Flow()
        float                       a, b, nozzle;
        int                         role;
        ConfigOptionFloatOrPercent  width;
    };
    std::vector<Made> made = {
        {0, 0.45f, 0.2f, 0.4f, 0, ConfigOptionFloatOrPercent(0, false)},
        {0, 0.42f, 0.2f, 0.4f, 0, ConfigOptionFloatOrPercent(0, false)},
        {0, 0.2f, 0.3f, 0.4f, 0, ConfigOptionFloatOrPercent(0, false)},
        {0, 0.1f, 0.2f, 0.4f, 0, ConfigOptionFloatOrPercent(0, false)},
        {0, 0.f, 0.f, 0.4f, 0, ConfigOptionFloatOrPercent(0, false)},
        {1, 0.4f, 0.f, 0.4f, 0, ConfigOptionFloatOrPercent(0, false)},
        {1, 0.f, 0.f, 0.4f, 0, ConfigOptionFloatOrPercent(0, false)},
        {2, 0.f, 0.2f, 0.4f, 0, ConfigOptionFloatOrPercent(0, false)},
        {2, 0.f, 0.2f, 0.4f, 4, ConfigOptionFloatOrPercent(0, false)},
        {2, 0.f, 0.2f, 0.4f, 5, ConfigOptionFloatOrPercent(-0.1, false)},
        {2, 0.f, 0.2f, 0.4f, 1, ConfigOptionFloatOrPercent(110, true)},
        {2, 0.f, 0.2f, 0.4f, 2, ConfigOptionFloatOrPercent(0, true)},
        {2, 0.f, 0.2f, 0.4f, 3, ConfigOptionFloatOrPercent(0.5, false)},
        {2, 0.f, 0.f, 0.4f, 3, ConfigOptionFloatOrPercent(0.5, false)},
        {2, 0.f, -0.1f, 0.4f, 3, ConfigOptionFloatOrPercent(0.5, false)},
        {2, 0.f, 0.3f, 0.6f, 7, ConfigOptionFloatOrPercent(0, false)},
        {3, 0.f, 0.f, 0.f, 0, ConfigOptionFloatOrPercent(0, false)},
    };
    const size_t hand = made.size();
    for (int i = 0; i < 1500; ++i) {
        Made m{};
        m.how    = int(g.below(3));
        m.a      = float(real(g, 0.05, 1.2));
        m.b      = float(g.below(10) == 0 ? real(g, -0.1, 0.05) : real(g, 0.04, 0.6));
        m.nozzle = NOZZLES[g.below(4)];
        m.role   = int(g.below(8));
        switch (g.below(5)) {
        case 0: m.width = ConfigOptionFloatOrPercent(0, false); break;
        case 1: m.width = ConfigOptionFloatOrPercent(real(g, 50, 250), true); break;
        case 2: m.width = ConfigOptionFloatOrPercent(-real(g, 0, 1), false); break;
        default: m.width = ConfigOptionFloatOrPercent(real(g, 0.1, 1.2), false); break;
        }
        made.push_back(m);
    }
    const Flow ref(0.45f, 0.2f, 0.4f);
    for (size_t i = 0; i < made.size(); ++i) {
        const Made &m = made[i];
        Payload     p;
        p.u8(uint8_t(m.how)).f32(m.a).f32(m.b).f32(m.nozzle).u8(uint8_t(m.role));
        put_fop(p, m.width);
        const float  w2    = float(real(g, 0.05, 1.2));
        const float  h2    = float(real(g, 0.04, 0.6));
        const float  s2    = float(real(g, 0.0, 1.0));
        const double ratio = g.below(4) == 0 ? 1.0 : real(g, 0.3, 2.5);
        const int    ak    = int(g.below(4));
        attempt(p, [&](Payload &q) {
            Flow f;
            switch (m.how) {
            case 0: f = Flow(m.a, m.b, m.nozzle); break;
            case 1: f = Flow::bridging_flow(m.a, m.nozzle); break;
            case 2: f = Flow::new_from_config_width(FlowRole(m.role), m.width, m.nozzle, m.b); break;
            default: break;
            }
            // The cross section: the flow's own (within EPSILON), above it or below it.
            float area = 0.1f;
            try {
                const double mm3 = f.mm3_per_mm();
                area = ak == 0 ? float(mm3) : ak == 1 ? float(mm3 + 5e-5) : ak == 2 ? float(mm3 * real(g, 1.05, 3.0)) : float(mm3 * real(g, 0.1, 0.95));
            } catch (const std::exception &) {
            }
            put_flow_ops(q, f, w2, h2, s2, area, ratio, ref);
        });
        add(sets, "LYFL", p, i < hand + 40);
    }
}

// LYFS: rounded_rectangle_extrusion_spacing, rounded_rectangle_extrusion_width_from_spacing,
// bridge_extrusion_spacing and auto_extrusion_width of every role.
void record_static(Sets &sets)
{
    SplitMix g(0x1A7EF55ull);
    for (int i = 0; i < 400; ++i) {
        const float w = i == 0 ? 0.f : float(real(g, 0.0, 1.2));
        const float h = i == 1 ? 0.f : float(real(g, 0.0, 0.8));
        const float n = NOZZLES[g.below(4)];
        Payload     p;
        p.f32(w).f32(h).f32(n);
        attempt(p, [&](Payload &q) { q.f32(Flow::rounded_rectangle_extrusion_spacing(w, h)); });
        p.f32(Flow::rounded_rectangle_extrusion_width_from_spacing(w, h));
        p.f32(Flow::bridge_extrusion_spacing(w));
        for (int r = 0; r < 8; ++r)
            p.f32(Flow::auto_extrusion_width(FlowRole(r), n));
        add(sets, "LYFS", p, i < 40);
    }
}

// LYFW: Flow::extrusion_width of each line width key, of a configuration of nozzle diameters and
// line widths, for a first printing extruder.
void record_extrusion_width(Sets &sets)
{
    SplitMix                       g(0x1A7EF77ull);
    const std::vector<std::string> keys = {"line_width",           "initial_layer_line_width", "inner_wall_line_width", "outer_wall_line_width",
                                           "sparse_infill_line_width", "internal_solid_infill_line_width", "bridge_line_width",
                                           "top_surface_line_width",   "support_line_width",       "skin_infill_line_width",
                                           "skeleton_infill_line_width"};
    const char *const values[] = {"0", "0.45", "120%", "0%", "0.6", "0.42", "250%", "0.3"};
    for (int i = 0; i < 300; ++i) {
        DynamicPrintConfig cfg;
        std::vector<std::pair<std::string, std::string>> set;
        // The nozzle diameters, absent now and then.
        if (i != 1 && g.below(12) != 0)
            set.emplace_back("nozzle_diameter", g.below(3) == 0 ? "0.4,0.6" : (g.below(2) == 0 ? "0.4" : "0.25"));
        for (const std::string &k : keys) {
            // line_width and internal_solid_infill_line_width are left out now and then, so
            // the fall backs that read them miss them.
            if ((k == "line_width" || k == "internal_solid_infill_line_width") && (i == 2 || g.below(15) == 0))
                continue;
            set.emplace_back(k, i == 0 ? "0" : values[g.below(std::size(values))]);
        }
        for (const auto &kv : set)
            cfg.set_deserialize_strict(kv.first, kv.second);
        const unsigned first = unsigned(g.below(3) == 0 ? g.below(6) : 0);
        Payload        p;
        p.u64(set.size());
        for (const auto &kv : set)
            p.str(kv.first).str(cfg.opt_serialize(kv.first));
        p.u32(first);
        std::vector<std::string> asked;
        for (const std::string &k : keys)
            if (cfg.option(k) != nullptr)
                asked.push_back(k);
        p.u64(asked.size());
        for (const std::string &k : asked) {
            p.str(k);
            attempt(p, [&](Payload &q) { q.f64(Flow::extrusion_width(k, cfg, first)); });
        }
        add_within(sets, "LYFW", p, 10000);
    }
}

} // namespace

void record_flow(Sets &sets)
{
    record_flows(sets);
    record_static(sets);
    record_extrusion_width(sets);
}

} // namespace layer
} // namespace probe
