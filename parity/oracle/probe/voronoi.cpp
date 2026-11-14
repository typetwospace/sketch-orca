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
// voronoi.sop1: Boost.Polygon's Voronoi builder, libslic3r's Voronoi utilities and the medial axis
// (parity/SOP1.md, "Probe files", and parity/sop1/voronoi.md), which orca-voronoi replays for the
// pieces voronoi-1, voronoi-2, voronoi-3 and voronoi-4. Its record kinds start with VR. The records
// come from the sources in voronoi/: arith.cpp (voronoi-1's arithmetic and predicates),
// builder.cpp (the builder runs and the predicate calls they make), diagram.cpp (voronoi-3) and
// slic3r.cpp (voronoi-4), on the inputs of common.cpp. Registered 2026-11-13 by orca-o3-skeleton
// (scripts/lib/parity/probes/voronoi.ts); recorded by orca-o3-probe-voronoi, 2026-11-13.
#include "probe.hpp"

#include <cstring>

#include "voronoi/common.hpp"

namespace probe {

namespace {

// extended_exponent_fpt keeps its value and exponent private; put_efpt reads them as the object
// lays them out (a double, then an int). This checks that layout on a known value.
bool efpt_layout(std::string &error)
{
    const vr::Efpt v(1.0);
    double         val;
    int32_t        exp;
    std::memcpy(&val, reinterpret_cast<const char *>(&v), 8);
    std::memcpy(&exp, reinterpret_cast<const char *>(&v) + 8, 4);
    if (val != 0.5 || exp != 1) {
        error = "voronoi: extended_exponent_fpt<double> is not laid out as a double and an int";
        return false;
    }
    return true;
}

bool run_voronoi(const Options &options, std::string &error)
{
    if (!efpt_layout(error) || !vr::inputs_ok(error))
        return false;
    Sets sets;
    head(sets, "voronoi");
    vr::start_log();
    vr::record_arith(sets);
    bool built = vr::record_builder(sets, error);
    if (built) {
        vr::record_diagram(sets);
        built = vr::record_slic3r(sets, error);
    }
    vr::stop_log();
    if (!built)
        return false;
    return save(sets, options, "voronoi.sop1", error);
}

} // namespace

const Target &target()
{
    static const Target t{"voronoi", false, run_voronoi};
    return t;
}

} // namespace probe
