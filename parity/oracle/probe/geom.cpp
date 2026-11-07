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
// geom.sop1: Clipper 6.4.2, ClipperUtils, libslic3r's geometry and the Eigen operations they reach
// (parity/SOP1.md, "geom.sop1"), recorded by the o2-geom task (2026-11-07) from the engine's own
// libslic3r. orca-eigen's tests/probes.rs replays the Eigen records (geom/eigen.cpp) and
// orca-geom's the rest (geom/clipper.cpp, geom/geometry.cpp).
#include "probe.hpp"

namespace probe {

void record_eigen(Sets &sets);
void record_clipper(Sets &sets);
void record_geometry(Sets &sets);

namespace {

bool run_geom(const Options &options, std::string &error)
{
    Sets sets;
    head(sets, "geom");
    record_eigen(sets);
    record_clipper(sets);
    record_geometry(sets);
    return save(sets, options, "geom.sop1", error);
}

} // namespace

const Target &target()
{
    static const Target t{"geom", false, run_geom};
    return t;
}

} // namespace probe
