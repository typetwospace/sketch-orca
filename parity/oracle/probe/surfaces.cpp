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
// surfaces.sop1: MutablePolygon, region expansion and the surface steps' helpers that can be called
// on their own (parity/SOP1.md, "Probe files", and parity/sop1/surfaces.md), which orca-surface
// replays for the pieces surfaces-1, surfaces-2 and surfaces-3. Registered 2026-11-13 by
// orca-o3-skeleton (scripts/lib/parity/probes/surfaces.ts); recorded since 2026-11-13
// (orca-o3-probe-surfaces). Its record kinds start with SF. The sources of the folder surfaces/
// record them: mutable.cpp (SFMP, SFRD, SFSO), expansion.cpp (SFRP, SFRX, SFME), pattern.cpp
// (SFLP) and bridges.cpp (SFBR, SFBF); shapes.cpp makes the inputs, from seeded shapes and the
// layers of OrcaSlicer's test meshes (src/tests/data, never the corpus). It runs under the
// oracle's threading, global_control(max_allowed_parallelism, 1) inside a task_arena, as the
// engine's steps do (parity/oracle/README.md).
#include "probe.hpp"

#include <exception>
#include <string>
#include <vector>

#include <oneapi/tbb/global_control.h>
#include <oneapi/tbb/task_arena.h>

#include "surfaces/shared.hpp"

namespace probe {

void record_mutable(Sets &sets);
void record_expansion(Sets &sets, const std::vector<surfaces::Layer> &layers);
void record_pattern(Sets &sets);
void record_bridges(Sets &sets, const std::vector<surfaces::Layer> &layers);

namespace {

bool run_surfaces(const Options &options, std::string &error)
{
    tbb::global_control gc(tbb::global_control::max_allowed_parallelism, 1);
    tbb::task_arena     arena{8, 8u};
    bool                ok = true;
    Sets                sets;
    head(sets, "surfaces");
    arena.execute([&]() {
        try {
            // Up to six layers of each test mesh with a bottom or a top.
            std::vector<surfaces::Layer> layers;
            if (!(ok = surfaces::mesh_layers(layers, true, 6, error)))
                return;
            record_mutable(sets);
            record_expansion(sets, layers);
            record_pattern(sets);
            record_bridges(sets, layers);
        } catch (const std::exception &e) {
            ok    = false;
            error = e.what();
        }
    });
    if (!ok)
        return false;
    return save(sets, options, "surfaces.sop1", error);
}

} // namespace

const Target &target()
{
    static const Target t{"surfaces", false, run_surfaces};
    return t;
}

} // namespace probe
