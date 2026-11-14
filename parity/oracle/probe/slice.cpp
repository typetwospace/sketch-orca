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
// slice.sop1: the Model and mesh helpers the slicer calls, TriangleMeshSlicer, SlicingParameters
// and adaptive slicing (parity/SOP1.md, "Probe files", and parity/sop1/slice.md, which specifies
// every record), recorded by orca-o3-probe-slice (2026-11-13) from the engine's own libslic3r.
// orca-model and orca-mesh replay the records of the piece front-2 (SLFE, SLMC, SLTS, SLSH,
// SLBX), orca-slice those of slice-1 (SLPR, SLRG) and slice-2 (SLMS, SLM1, SLMX, SLSB, SLAD, SLAP,
// SLSM, SLAJ, SLOL); SLMH, the mesh table, is their input. The sources are in slice/: meshes.cpp,
// front.cpp, slicer.cpp, layers.cpp, with values.cpp and common.hpp. The records run under the
// oracle's threading (global_control(max_allowed_parallelism, 1) inside a task_arena,
// parity/oracle/README.md), so the order of the intersection lines slice_mesh collects under its
// mutexes is the order of the faces. Registered 2026-11-13 by orca-o3-skeleton
// (scripts/lib/parity/probes/slice.ts).
#include "probe.hpp"

#include <exception>

#include <oneapi/tbb/global_control.h>
#include <oneapi/tbb/task_arena.h>

#include "slice/common.hpp"

namespace probe {

namespace {

bool run_slice(const Options &options, std::string &error)
{
    if (::SCALING_FACTOR != 0.000001) {
        error = "slice probe: SCALING_FACTOR is not 1e-6";
        return false;
    }
    tbb::global_control gc(tbb::global_control::max_allowed_parallelism, 1);
    tbb::task_arena     arena{8, 8u};
    // Each kind's share of the committed file (parity/sop1/slice.md, "The committed set"); the small
    // meshes of SLMH are committed whole, outside these shares.
    slice::Shares &shares = slice::shares();
    shares.cap = {{"SLFE", 24000}, {"SLMC", 64000}, {"SLTS", 20000}, {"SLSH", 12000}, {"SLBX", 12000},
                  {"SLPR", 40000}, {"SLRG", 20000}, {"SLMS", 66500}, {"SLM1", 17500}, {"SLMX", 66500},
                  {"SLSB", 30000}, {"SLAD", 24000}, {"SLAP", 18000}, {"SLSM", 18000}, {"SLAJ", 18000},
                  {"SLOL", 22000}};
    bool ok = true;
    Sets sets;
    head(sets, "slice");
    arena.execute([&]() {
        try {
            std::vector<slice::Mesh> meshes;
            if (!(ok = slice::build_meshes(meshes, error)))
                return;
            slice::record_meshes(sets, meshes);
            slice::record_front(sets, meshes);
            slice::record_slicer(sets, meshes);
            slice::record_layers(sets, meshes);
        } catch (const std::exception &e) {
            ok    = false;
            error = e.what();
        }
    });
    if (!ok)
        return false;
    return save(sets, options, "slice.sop1", error);
}

} // namespace

const Target &target()
{
    static const Target t{"slice", false, run_slice};
    return t;
}

} // namespace probe
