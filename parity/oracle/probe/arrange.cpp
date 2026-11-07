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
// arrange.sop1: libnest2d, Arrange.cpp, NLopt 2.5.0, Boost.Geometry's R*-tree and Boost's int128_t
// and rational (parity/SOP1.md, "arrange.sop1"), which orca-arrange and orca-exact replay.
// Registered 2026-11-07 (scripts/lib/parity/probe.ts, PROBE_REGISTRY); recorded by the o2-arrange
// task (2026-11-07) from the engine's own libslic3r, libnest2d and NLopt, in arrange/: exact.cpp
// (XI28, XRAT; orca-exact), nlopt.cpp (NLOP, NOPT), nest.cpp (RTRE, NFPC, NGEO) and runs.cpp
// (ARNG), which orca-arrange replays.
#include "probe.hpp"

namespace probe {

void record_exact(Sets &sets);
void record_nlopt(Sets &sets);
void record_nest(Sets &sets);
void record_runs(Sets &sets);

namespace {

bool run_arrange(const Options &options, std::string &error)
{
    Sets sets;
    head(sets, "arrange");
    record_exact(sets);
    record_nlopt(sets);
    record_nest(sets);
    record_runs(sets);
    return save(sets, options, "arrange.sop1", error);
}

} // namespace

const Target &target()
{
    static const Target t{"arrange", false, run_arrange};
    return t;
}

} // namespace probe
