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
// What the paths probe's sources share and that needs no libslic3r header (clipperz.cpp includes
// ClipperLib_Z before any of them): the seeded draws and the record sink that keeps the committed
// set a true subset of the full one within paths.sop1's budget (parity/sop1/paths.md). Added
// 2026-11-13 by orca-o3-probe-paths.
#pragma once

#include "../probe.hpp"

#include <cstdint>
#include <cstring>

namespace probe {
namespace paths {

// A double in [a, b) from the top 53 bits of one draw.
inline double real(SplitMix &g, double a, double b) { return a + (b - a) * (double(g.next() >> 11) * 0x1.0p-53); }
// An integer in [lo, hi].
inline int64_t rnd(SplitMix &g, int64_t lo, int64_t hi) { return lo + int64_t(g.below(uint64_t(hi - lo) + 1)); }
// True with odds 1 in n.
inline bool one_in(SplitMix &g, uint64_t n) { return g.below(n) == 0; }

// One record kind's sink: hand-made cases go to both sets, random ones to the full set and, the
// first `committed_random` of at most `committed_bytes` bytes, to the committed set too.
struct Emit
{
    Sets  &sets;
    char   kind[5];
    size_t committed_random;
    size_t committed_bytes;
    size_t taken = 0;
    Emit(Sets &s, const char (&k)[5], size_t random, size_t bytes) : sets(s), committed_random(random), committed_bytes(bytes) { std::memcpy(kind, k, 5); }
    void hand(const Payload &p)
    {
        sets.full.add(kind, p);
        sets.committed.add(kind, p);
    }
    void random(const Payload &p)
    {
        sets.full.add(kind, p);
        if (taken < committed_random && p.bytes.size() <= committed_bytes) {
            sets.committed.add(kind, p);
            ++taken;
        }
    }
};

void record_clipperz(Sets &sets);
void record_geom1(Sets &sets);
void record_geom2(Sets &sets);
void record_chain(Sets &sets);

} // namespace paths
} // namespace probe
