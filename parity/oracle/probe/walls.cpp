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
// walls.sop1: PerimeterGenerator, classic and Arachne, BridgeDetector and the principal components
// (parity/SOP1.md, "Probe files", and parity/sop1/walls.md), which orca-perimeter replays for the
// pieces walls-1, walls-2 and walls-3. Recorded by orca-o3-probe-walls (2026-11-13; registered by
// orca-o3-skeleton in scripts/lib/parity/probes/walls.ts). Every kind starts with WL; the
// sources of walls/ record them, in this order:
//
//   walls/basics.cpp  WLMT, WLPC, WLDB, WLBD, WLEA (walls-1)
//   walls/extra.cpp   WLRC, WLPT, WLSX, WLXP (walls-1)
//   walls/order.cpp   WLTP, WLCP (walls-3)
//   walls/jobs.cpp    WLCL (walls-2), WLAR (walls-3)
//
// walls/values.cpp writes the values. The committed set takes the records each recorder marks,
// which must all fit its share of the file's 512 KB, else the probe fails (values.hpp, Writer); the
// full set takes every record. The records run under the oracle's threading: global_control(max_allowed_parallelism,
// 1) inside a task_arena (parity/oracle/README.md).
#include "probe.hpp"

#include <exception>
#include <string>

#include <oneapi/tbb/global_control.h>
#include <oneapi/tbb/task_arena.h>

#include "walls/values.hpp"

namespace probe {

namespace {

bool run_walls(const Options &options, std::string &error)
{
    tbb::global_control gc(tbb::global_control::max_allowed_parallelism, 1);
    tbb::task_arena     arena{8, 8u};
    Sets                sets;
    head(sets, "walls");
    walls::Writer w(sets);
    bool          ok = true;
    arena.execute([&]() {
        try {
            walls::record_moments(w);
            walls::record_principal(w);
            walls::record_bridging_direction(w);
            walls::record_bridge_detector(w);
            walls::record_paths_append(w);
            walls::record_reconnect(w);
            walls::record_touch(w);
            walls::record_sort_extra(w);
            walls::record_extra_perimeters(w);
            walls::record_touching(w);
            walls::record_compatible(w);
            ok = walls::record_jobs(w, error);
        } catch (const std::exception &e) {
            error = std::string("walls: ") + e.what();
            ok    = false;
        }
    });
    if (ok && w.over > 0) {
        error = "walls: " + std::to_string(w.over) + " records marked for the committed set do not fit its " + std::to_string(walls::Writer::BUDGET) + " bytes";
        ok    = false;
    }
    return ok && save(sets, options, "walls.sop1", error);
}

} // namespace

const Target &target()
{
    static const Target t{"walls", false, run_walls};
    return t;
}

} // namespace probe
