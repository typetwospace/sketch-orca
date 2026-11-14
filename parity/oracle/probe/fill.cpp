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
// fill.sop1: the infill of domain A (parity/SOP1.md, "Probe files", and parity/sop1/fill.md,
// which names every record kind, its fields and the piece of orca-fill that replays it: fill-1,
// fill-2, fill-3 or fill-4). Registered 2026-11-13 by orca-o3-skeleton
// (scripts/lib/parity/probes/fill.ts); recorded by orca-o3-probe-fill the same day. The sources
// of fill/ record them, in this order:
//
//   fill/base.cpp       FLDF, FLPA, FLAS, FLDR, FLSU (fill-1)
//   fill/connect.cpp    FLCN, FLEB (fill-2)
//   fill/extrusion.cpp  FLGP, FLEX (fill-2)
//   fill/patterns.cpp   FLRP (fill-3), FLCP (fill-4)
//   fill/types.cpp      FLNT, FLNM, FLRA (fill-4)
//
// fill/common.cpp writes the values and holds what the sources share (fill/fill.hpp). The inputs
// are the probe's own shapes and sections of OrcaSlicer's test meshes (the oracle's
// src/tests/data), never the corpus. group_fills, make_fills and PrintObject::infill take a whole
// Layer and are proven by the posInfill dumps instead. Every kind starts with FL. The records run
// under the oracle's threading: global_control(max_allowed_parallelism, 1) inside a task_arena
// (parity/oracle/README.md).
#include "probe.hpp"

#include <exception>
#include <string>
#include <vector>

#include <oneapi/tbb/global_control.h>
#include <oneapi/tbb/task_arena.h>

#include "fill/fill.hpp"

namespace probe {

namespace {

bool run_fill(const Options &options, std::string &error)
{
    tbb::global_control gc(tbb::global_control::max_allowed_parallelism, 1);
    tbb::task_arena     arena{8, 8u};
    Sets                sets;
    head(sets, "fill");
    fill::ErrorLogScope log;
    bool                ok = true;
    arena.execute([&]() {
        try {
            std::vector<fill::Shape> sections;
            ok = fill::mesh_sections(sections, error);
            if (ok) {
                fill::record_base(sets, sections);
                ok = fill::record_direction(sets, error) && fill::record_surface(sets, sections, error);
            }
            if (ok) {
                fill::record_connect(sets, sections);
                fill::record_extrusion(sets, sections);
                fill::record_patterns(sets, sections);
                fill::record_types(sets);
            }
        } catch (const std::exception &e) {
            error = std::string("fill: ") + e.what();
            ok    = false;
        }
    });
    return ok && save(sets, options, "fill.sop1", error);
}

} // namespace

const Target &target()
{
    static const Target t{"fill", false, run_fill};
    return t;
}

} // namespace probe
