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
// index.sop1's IXNP records (parity/sop1/index.md): the inputs where orca-index must stop with
// NotPorted instead of answering, because OrcaSlicer's release build has no answer to copy there.
// It reads past a vector's end, divides by zero, overflows a signed product or reads a member that
// was never set. The engine is never run on these inputs (that would be undefined behaviour); each
// record names the piece, the function, the condition and one input that meets it, laid out as the
// page says. Ties in std::sort are no such edge: orca-index sorts with orca_stl::sort_by, MSVC's
// introsort translated branch for branch, so tied items fall as in the C++ at any count (IXTM and
// IXLT hold cases past 32 items). Recorded by orca-o3-probe-index, 2026-11-13.
#include "index.hpp"

#include <cstdint>
#include <initializer_list>
#include <string>
#include <vector>

namespace probe {

namespace {

void edge(Sets &sets, const char *piece, const char *function, const char *condition, std::initializer_list<int64_t> example)
{
    Payload p;
    p.str(piece).str(function).str(condition).i64s(std::vector<int64_t>(example));
    sets.full.add("IXNP", p);
    sets.committed.add("IXNP", p);
}

} // namespace

void record_index_edges(Sets &sets)
{
    const int64_t big = int64_t(1) << 40, wide = int64_t(1) << 33, cell = int64_t(1) << 31;
    // MutablePriorityQueue.hpp. Examples: the IXPQ instance, the items pushed, then the index.
    edge(sets, "index-1", "MutablePriorityQueue::top, MutablePriorityQueue::pop", "the queue is empty: it reads the heap's first slot, which does not exist", {0, 0});
    edge(sets, "index-1", "MutableSkipHeapPriorityQueue::top, MutableSkipHeapPriorityQueue::pop", "the queue is empty: it reads heap slot 1, which does not exist", {2, 0});
    edge(sets, "index-1", "MutablePriorityQueue::remove, MutablePriorityQueue::update, MutablePriorityQueue::operator[]", "the index is size() or more", {0, 3, 3});
    edge(sets, "index-1", "MutableSkipHeapPriorityQueue::remove, MutableSkipHeapPriorityQueue::update, MutableSkipHeapPriorityQueue::operator[]",
         "the address is a padding slot (a multiple of the block size)", {4, 5, 4});
    edge(sets, "index-1", "MutableSkipHeapPriorityQueue::remove, MutableSkipHeapPriorityQueue::update, MutableSkipHeapPriorityQueue::operator[]",
         "the address is at or past the heap's end", {2, 3, 4});
    // EdgeGrid.hpp and EdgeGrid.cpp. Examples: the resolution, the polygon's points (x then y),
    // then the query's points.
    edge(sets, "index-2", "EdgeGrid::Grid::create", "the resolution is 0: create_from_m_contours divides by it", {0, 0, 0, 1000, 0, 1000, 1000, 0, 1000});
    edge(sets, "index-2", "EdgeGrid::Grid::create", "the resolution is below 0: rows and columns come out negative and the cell vector's size wraps", {-100, 0, 0, 1000, 0, 1000, 1000, 0, 1000});
    edge(sets, "index-2", "EdgeGrid::Grid::create", "rows times columns overflows or cannot be allocated", {1, 0, 0, big, 0, big, big, 0, big});
    edge(sets, "index-2", "EdgeGrid::Grid::create, closest_point_signed_distance, signed_distance_edges, signed_distance, calculate_sdf",
         "a product of a coordinate difference and the resolution, or of two coordinate differences, passes 2^63 (signed overflow)", {cell, 0, 0, wide, wide - 7, 0, wide});
    edge(sets, "index-2", "EdgeGrid::Grid::visit_cells_intersecting_line",
         "an end point lies outside the grid's cells: below the box's minimum, or at or past the minimum plus columns (rows) times the resolution; callers' visitors read that cell",
         {100, 0, 0, 1000, 0, 1000, 1000, 0, 1000, -100, -100, 500, 500});
    edge(sets, "index-2", "EdgeGrid::Grid::signed_distance_bilinear", "calculate_sdf has not run: it reads the empty field", {100, 0, 0, 1000, 0, 1000, 1000, 0, 1000, 500, 500});
    edge(sets, "index-2", "EdgeGrid::Grid queries", "create has not run: the resolution was never set", {});
    // The free intersecting_edges. Example: each polygon's point count, then its points.
    edge(sets, "index-2", "Slic3r::intersecting_edges(const Polygons &)",
         "the mean edge length over the polygons of two or more points is below 1, so the resolution (a double turned into coord_t) is 0", {3, 0, 0, 0, 0, 1, 0});
}

} // namespace probe
