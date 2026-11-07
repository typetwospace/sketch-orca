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
// xml.sop1: expat 2.5.0, the dependency build's, as the 3MF reader parses with it (parity/SOP1.md,
// "Probe files"), which orca-xml replays. The o2-model task records here; until then this is a stub
// that writes HEAD and PRUN only, and the crate's tests/probes.rs fails as soon as the file holds a
// record it does not replay. Registered 2026-11-07 (scripts/lib/parity/probe.ts, PROBE_REGISTRY).
#include "probe.hpp"

namespace probe {

namespace {

bool run_xml(const Options &options, std::string &error) { return write_stub("xml", options, error); }

} // namespace

const Target &target()
{
    static const Target t{"xml", false, run_xml};
    return t;
}

} // namespace probe
