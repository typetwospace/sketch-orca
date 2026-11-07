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
// zip.sop1: miniz as the 3MF reader and writer use it through miniz_extension.cpp (parity/SOP1.md,
// "Probe files"), which orca-zip replays. The o2-model task records here; until then this is a stub
// that writes HEAD and PRUN only, and the crate's tests/probes.rs fails as soon as the file holds a
// record it does not replay. Registered 2026-11-07 (scripts/lib/parity/probe.ts, PROBE_REGISTRY).
#include "probe.hpp"

namespace probe {

namespace {

bool run_zip(const Options &options, std::string &error) { return write_stub("zip", options, error); }

} // namespace

const Target &target()
{
    static const Target t{"zip", false, run_zip};
    return t;
}

} // namespace probe
