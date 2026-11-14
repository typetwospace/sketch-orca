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
// paths.sop1: Clipper with Z, Circle, ArcFitter, the 2D and 3D helpers of orca-geom, and
// ShortestPath's chaining (parity/SOP1.md, "Probe files", and parity/sop1/paths.md), which
// orca-geom and orca-paths replay for the pieces geom-1, geom-2 and paths-1. Registered
// 2026-11-13 by orca-o3-skeleton (scripts/lib/parity/probes/paths.ts) and recorded the same day
// by orca-o3-probe-paths, from the sources of paths/: clipperz.cpp (PSZC), geom1.cpp (geom-1's
// other kinds), geom2.cpp (geom-2's) and chain.cpp (paths-1's). Every kind starts with PS.
#include "probe.hpp"

#include "paths/common.hpp"

namespace probe {

namespace {

bool run_paths(const Options &options, std::string &error)
{
    Sets sets;
    head(sets, "paths");
    paths::record_clipperz(sets);
    paths::record_geom1(sets);
    paths::record_geom2(sets);
    paths::record_chain(sets);
    return save(sets, options, "paths.sop1", error);
}

} // namespace

const Target &target()
{
    static const Target t{"paths", false, run_paths};
    return t;
}

} // namespace probe
