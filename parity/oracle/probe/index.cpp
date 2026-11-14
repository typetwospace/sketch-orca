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
// index.sop1: the AABB trees, the KD tree, the mutable priority queue and EdgeGrid (parity/SOP1.md,
// "Probe files", and parity/sop1/index.md), which orca-index replays for the pieces index-1
// (AABBTreeIndirect.hpp, AABBTreeLines.hpp, KDTreeIndirect.hpp, MutablePriorityQueue.hpp) and
// index-2 (EdgeGrid.hpp, EdgeGrid.cpp). The sources of parity/oracle/probe/index/ record each part:
// aabb.cpp IXBX and IXTM, lines.cpp IXLT, kd.cpp IXKD, queue.cpp IXPQ, edgegrid.cpp IXEG and IXEI,
// edges.cpp the IXNP edges. Every record goes into the full set; the committed set takes the edge
// cases and the first seeded cases that are small enough (index/index.hpp, record_cases).
// Registered 2026-11-13 by orca-o3-skeleton (scripts/lib/parity/probes/index.ts); recorded by
// orca-o3-probe-index, 2026-11-13.
#include "probe.hpp"

#include "index/index.hpp"

namespace probe {

namespace {

bool run_index(const Options &options, std::string &error)
{
    Sets sets;
    head(sets, "index");
    if (!record_index_aabb(sets, error))
        return false;
    record_index_lines(sets);
    record_index_kd(sets);
    record_index_queue(sets);
    record_index_edgegrid(sets);
    record_index_edges(sets);
    return save(sets, options, "index.sop1", error);
}

} // namespace

const Target &target()
{
    static const Target t{"index", false, run_index};
    return t;
}

} // namespace probe
