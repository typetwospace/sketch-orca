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
// cstdio.sop1: the static UCRT's stdio as admesh reads files with it: fopen, fread, fgets, fscanf
// and sscanf (parity/SOP1.md, "Probe files"), which orca-fmt's stdio and scanf modules replay. The
// o2-mesh task records here; until then this is a stub that writes HEAD and PRUN only, and the
// crate's tests/probes.rs fails as soon as the file holds a record it does not replay. Registered
// 2026-11-07 (scripts/lib/parity/probe.ts, PROBE_REGISTRY).
#include "probe.hpp"

namespace probe {

namespace {

bool run_cstdio(const Options &options, std::string &error) { return write_stub("cstdio", options, error); }

} // namespace

const Target &target()
{
    static const Target t{"cstdio", false, run_cstdio};
    return t;
}

} // namespace probe
