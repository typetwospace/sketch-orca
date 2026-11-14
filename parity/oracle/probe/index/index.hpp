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
// What the index probe's sources share (parity/sop1/index.md): the record functions of each part,
// the case loop that fills both sets, the seeded numbers and the writers of the values every
// record uses. Recorded by orca-o3-probe-index, 2026-11-13.
#pragma once

#include "../probe.hpp"

#include <cstdint>
#include <string>
#include <vector>

#include "libslic3r/Point.hpp"

namespace probe {

// The records, one function per part (index.cpp calls them in this order). IXBX and IXTM
// (index/aabb.cpp): false, with `error`, when one of OrcaSlicer's test meshes cannot be read.
bool record_index_aabb(Sets &sets, std::string &error);
void record_index_lines(Sets &sets);    // IXLT (index/lines.cpp)
void record_index_kd(Sets &sets);       // IXKD (index/kd.cpp)
void record_index_queue(Sets &sets);    // IXPQ (index/queue.cpp)
void record_index_edgegrid(Sets &sets); // IXEG, IXEI (index/edgegrid.cpp)
void record_index_edges(Sets &sets);    // IXNP (index/edges.cpp)

namespace ix {

constexpr uint64_t NPOS = uint64_t(-1);

// The cases of one record kind. Every edge case goes into the full set, and into the committed set
// when its payload has at most `committed_bytes` bytes; then `random` seeded cases go into the full
// set, of which the first `committed_random` of at most `committed_bytes` bytes also go into the
// committed set. So the committed set is a true subset, in the full set's order. A kind's pinned
// cases (IXTM's and IXLT's sort cases) are one call before its others, with no limit
// (std::numeric_limits<size_t>::max()) and no seeded cases.
template<class Edge, class Random>
void record_cases(Sets &sets, const char (&kind)[5], const std::vector<Edge> &edges, size_t random, size_t committed_random, size_t committed_bytes, Random next)
{
    for (const Edge &e : edges) {
        Payload p;
        e(p);
        sets.full.add(kind, p);
        if (p.bytes.size() <= committed_bytes)
            sets.committed.add(kind, p);
    }
    size_t taken = 0;
    for (size_t i = 0; i < random; ++i) {
        Payload p;
        next(p);
        sets.full.add(kind, p);
        if (taken < committed_random && p.bytes.size() <= committed_bytes) {
            sets.committed.add(kind, p);
            ++taken;
        }
    }
}

// Seeded numbers from probe.hpp's splitmix64: an integer in [lo, hi] and a double in [lo, hi).
inline int64_t rnd(SplitMix &g, int64_t lo, int64_t hi) { return lo + int64_t(g.below(uint64_t(hi - lo) + 1)); }
inline double  real(SplitMix &g, double lo, double hi) { return lo + (hi - lo) * (double(g.next() >> 11) * (1.0 / 9007199254740992.0)); }

inline void put(Payload &p, const Slic3r::Point &q) { p.i64(q.x()).i64(q.y()); }
inline void put(Payload &p, const Slic3r::Points &pts)
{
    p.u64(pts.size());
    for (const Slic3r::Point &q : pts)
        put(p, q);
}
inline void put_scalar(Payload &p, long long v) { p.i64(v); }
inline void put_scalar(Payload &p, float v) { p.f32(v); }
inline void put_scalar(Payload &p, double v) { p.f64(v); }
inline void put_idx(Payload &p, const std::vector<size_t> &v)
{
    p.u64(v.size());
    for (size_t x : v)
        p.u64(x);
}

// An AABBTreeIndirect::Tree's nodes: the count, then each node's idx (npos for an unused node,
// inner for an inner node) and its box, the minimum's coordinates then the maximum's, in the tree's
// scalar.
template<class Tree> void put_nodes(Payload &p, const Tree &tree)
{
    p.u64(tree.nodes().size());
    for (const auto &n : tree.nodes()) {
        p.u64(n.idx);
        for (int i = 0; i < Tree::NumDimensions; ++i)
            put_scalar(p, n.bbox.min()(i));
        for (int i = 0; i < Tree::NumDimensions; ++i)
            put_scalar(p, n.bbox.max()(i));
    }
}

} // namespace ix
} // namespace probe
