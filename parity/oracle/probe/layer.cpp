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
// layer.sop1: the static print configs, extrusion entities, Flow, variable width, Surface, Layer
// and its regions (parity/SOP1.md, "Probe files", and parity/sop1/layer.md), which orca-config and
// orca-layer replay for the pieces front-1, layer-1, layer-2 and layer-3. Registered 2026-11-13 by
// orca-o3-skeleton (scripts/lib/parity/probes/layer.ts); recorded by orca-o3-probe-layer the same
// day. Its record kinds start with LY. The sources of layer/ record them:
//
//   layer/config.cpp     LYCK LYCA LYCD LYCO LYDY LYAM LYFO LYSK LYMT  (front-1)
//   layer/flow.cpp       LYFL LYFS LYFW                                (layer-1)
//   layer/extrusion.cpp  LYRO LYEN LYEM LYEL LYES LYEC LYEA LYVW       (layer-1), LYCH (layer-3)
//   layer/surface.cpp    LYSU LYSC                                     (layer-2)
//   layer/print.cpp      LYPC LYPR LYSF LYPH LYPT LYLA                 (layer-3)
//
// Every input is generated here from fixed seeds (splitmix64, probe.hpp) or written by hand;
// nothing is read from a file. Each record holds its inputs and then the engine's results, so a
// replay needs no generator. The committed set takes the hand-made cases and the first cases of
// each kind, or the records that fit a kind's byte budget (add_within, layer/common.hpp), within
// the file's 512 KB. Where the engine would leave defined behaviour (a null dereference in
// apply_only or get_abs_value, a crash setting a list of enumerations), the probe keeps its inputs
// off that path or records that the call was not made. The run is under the oracle's threading:
// global_control(max_allowed_parallelism, 1) inside a task_arena (parity/oracle/README.md), with
// the engine's log off.
#include "probe.hpp"

#include <exception>

#include <boost/log/core.hpp>

#include <oneapi/tbb/global_control.h>
#include <oneapi/tbb/task_arena.h>

namespace probe {

namespace layer {
void record_config(Sets &sets);
void record_flow(Sets &sets);
void record_extrusion(Sets &sets);
void record_surface(Sets &sets);
void record_print(Sets &sets);
} // namespace layer

namespace {

bool run_layer(const Options &options, std::string &error)
{
    tbb::global_control gc(tbb::global_control::max_allowed_parallelism, 1);
    tbb::task_arena     arena{8, 8u};
    boost::log::core::get()->set_logging_enabled(false);
    Sets sets;
    head(sets, "layer");
    bool ok = true;
    arena.execute([&]() {
        try {
            layer::record_config(sets);
            layer::record_flow(sets);
            layer::record_extrusion(sets);
            layer::record_surface(sets);
            layer::record_print(sets);
        } catch (const std::exception &e) {
            ok    = false;
            error = e.what();
        }
    });
    if (!ok)
        return false;
    return save(sets, options, "layer.sop1", error);
}

} // namespace

const Target &target()
{
    static const Target t{"layer", false, run_layer};
    return t;
}

} // namespace probe
