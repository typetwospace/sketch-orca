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
// fill.sop1's FLNT, FLNM and FLRA records, which piece fill-4 replays (parity/sop1/fill.md;
// orca-o3-probe-fill, 2026-11-13): Fill::new_from_type for every InfillPattern value and one past
// the last, with what the new filler answers (use_bridge_flow, no_sort, is_self_crossing,
// has_consistent_pattern, the layer angle) and the static Fill::use_bridge_flow; the string form
// for every name of the pattern map and for names it lacks; and Fill.cpp's
// calculate_infill_rotation_angle for an empty template and for plain lists of angles, the only
// templates it answers without a PrintObject. That function has external linkage but no header,
// so it is declared here with Fill.cpp's own signature.
#include "fill.hpp"

#include <cfloat>
#include <memory>

#include "libslic3r/Config.hpp"

namespace Slic3r {
class PrintObject;
// Fill.cpp, with external linkage and no header.
double calculate_infill_rotation_angle(const PrintObject *object, size_t layer_id, const double &fixed_infill_angle, const std::string &template_string);
} // namespace Slic3r

namespace probe {
namespace fill {

using namespace Slic3r;

namespace {

bool domain_a(int type)
{
    return type == ipMonotonic || type == ipMonotonicLine || type == ipRectilinear || type == ipCrossHatch || type == ipConcentric ||
           type == ipConcentricInternal;
}

} // namespace

void record_types(Sets &sets)
{
    {
        Recorder                   r(sets, "FLNT", 0, 0);
        const t_config_enum_names &names = ConfigOptionEnum<InfillPattern>::get_enum_names();
        for (int type = 0; type <= int(ipCount); ++type) {
            Payload p;
            p.i32(type).boolean(domain_a(type));
            p.str(size_t(type) < names.size() ? names[size_t(type)] : std::string());
            try {
                std::unique_ptr<Fill> f(Fill::new_from_type(InfillPattern(type)));
                p.u8(0);
                p.boolean(f->use_bridge_flow()).boolean(f->no_sort()).boolean(f->is_self_crossing()).boolean(f->has_consistent_pattern());
                f->fixed_angle = false;
                p.f32(FillAccess::layer_angle(*f, 0)).f32(FillAccess::layer_angle(*f, 1)).f32(FillAccess::layer_angle(*f, 7));
                f->fixed_angle = true;
                p.f32(FillAccess::layer_angle(*f, 0)).f32(FillAccess::layer_angle(*f, 1));
                p.boolean(Fill::use_bridge_flow(InfillPattern(type)));
            } catch (const std::exception &e) {
                put_threw(p, e);
            }
            r.edge(p);
        }
    }
    {
        Recorder r(sets, "FLNM", 0, 0);
        auto     one = [&](const std::string &name) {
            const t_config_enum_values &values = ConfigOptionEnum<InfillPattern>::get_enum_values();
            const auto                  it     = values.find(name);
            Payload                     p;
            p.str(name).boolean(it != values.end()).i32(it != values.end() ? it->second : -1);
            std::unique_ptr<Fill> f(Fill::new_from_type(name));
            p.boolean(f != nullptr);
            return p;
        };
        for (const auto &kv : ConfigOptionEnum<InfillPattern>::get_enum_values())
            r.edge(one(kv.first));
        for (const char *name : {"", "Rectilinear", "rectilinear ", "concentricinternal", "supportbase", "monotonous", "honeycomb2", "3d"})
            r.edge(one(name));
    }
    {
        Recorder r(sets, "FLRA", 0, 0);
        auto     one = [&](size_t layer_id, double fixed, const std::string &tmpl) {
            Payload p;
            p.u64(uint64_t(layer_id)).f64(fixed).str(tmpl);
            try {
                const double angle = calculate_infill_rotation_angle(nullptr, layer_id, fixed, tmpl);
                p.u8(0).f64(angle);
            } catch (const std::exception &e) {
                put_threw(p, e);
            }
            return p;
        };
        for (double fixed : {45., 0., 90., 30., 135., 360., 1e-9, 22.5, 179.99, 1e6, 7.123456789})
            for (size_t layer : {size_t(0), size_t(1), size_t(57)})
                r.edge(one(layer, fixed, ""));
        const char *lists[] = {"0", "45", "45,90", "0,30,60", "90,0", "15.5,100.25,370", "45,135,225,315", "0.1,0.2,0.3,0.4,0.5,0.6,0.7", "720", "30, 60"};
        for (const char *list : lists)
            for (size_t layer : {size_t(0), size_t(1), size_t(2), size_t(3), size_t(6), size_t(101), size_t(1000003)})
                r.edge(one(layer, 45., list));
    }
}

} // namespace fill
} // namespace probe
