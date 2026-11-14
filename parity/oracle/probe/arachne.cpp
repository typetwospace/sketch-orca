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
// arachne.sop1: the beading strategies, ExtrusionLine, the grids and the stitcher, the skeletal
// trapezoidation and WallToolPaths (parity/SOP1.md, "Probe files", and parity/sop1/arachne.md),
// which orca-arachne replays for the pieces arachne-1 to arachne-4. The sources are in arachne/:
// common (writers, log capture, shapes, settings), beading (ANBS), lines (ANLA, ANXL, ANAE), grids
// (ANSQ, ANSP, ANSL, ANPS), skeletal (ANSC, ANST, ANGR, ANWS, ANIP) and walls (ANWH, ANWT, ANMP).
// Every case goes into the full set; the committed set takes whole cases while each family's
// share of the 512 KB budget holds them (arachne/common.cpp). Registered 2026-11-13 by
// orca-o3-skeleton (scripts/lib/parity/probes/arachne.ts); recorded by orca-o3-probe-arachne,
// 2026-11-13.
#include "probe.hpp"

#include <iostream>

#include "arachne/common.hpp"

namespace probe {

namespace {

bool run_arachne(const Options &options, std::string &error)
{
    Sets sets;
    head(sets, "arachne");
    arachne::log_start();
    arachne::Out out(sets);
    arachne::record_beading(out);
    arachne::record_lines_all(out);
    arachne::record_grids(out);
    const bool ok = arachne::record_skeletal(out, error) && arachne::record_walls(out, error);
    arachne::log_stop();
    if (!ok)
        return false;
    std::clog << "arachne probe: committed bytes by family: " << out.report() << "\n";
    return save(sets, options, "arachne.sop1", error);
}

} // namespace

const Target &target()
{
    static const Target t{"arachne", false, run_arachne};
    return t;
}

} // namespace probe
