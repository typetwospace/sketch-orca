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
// index.sop1's MutablePriorityQueue records (parity/sop1/index.md), which orca-index replays for
// the piece index-1:
//
//   IXPQ  A queue of pointers to items, each with a key and the index the queue's setter writes
//         into it, as ShortestPath.cpp has them (make_mutable_priority_queue<EndPoint*, false> and
//         <EndPoint*, true>, ordered by a key) and as QuadricEdgeCollapse.cpp has the cache
//         friendly one (make_miniheap_mutable_priority_queue<Error, 32, false>), with blocks of 32
//         and of 4: a run of pushes (with a key), pops, removals and updates at a heap address
//         (after a new key) and clears, and after each step the heap's slots and every item's
//         index. Keys tie often, so the order of pops among equal keys is recorded.
//
// The release build (NDEBUG) is the engine's, so the indices of removed items are reset only when
// the queue is made to reset them. Each record holds its operations and then the state after each.
// Recorded by orca-o3-probe-index, 2026-11-13.
#include "index.hpp"

#include <cmath>
#include <cstdint>
#include <functional>
#include <limits>
#include <vector>

#include "libslic3r/MutablePriorityQueue.hpp"

namespace probe {

namespace {

using namespace probe::ix;

struct Item
{
    double   key;
    uint32_t id;
    size_t   qidx;
};

struct Op
{
    uint8_t  code; // 0 push item a with key, 1 pop, 2 remove at address a, 3 update at address a with key, 4 clear
    uint64_t a;
    double   key;
};

// The queues by instance: 0 and 1 MutablePriorityQueue (ResetIndexWhenRemoved false, true), 2 and
// 3 MutableSkipHeapPriorityQueue with blocks of 32, 4 and 5 with blocks of 4.
constexpr size_t block_of(uint8_t instance) { return instance < 2 ? 0 : (instance < 4 ? 32 : 4); }

// The heap address of the logical position j: j itself, or the j-th slot that is not padding.
size_t address(size_t block, size_t j)
{
    if (block == 0)
        return j;
    size_t addr = 0;
    for (size_t n = 0;; ++addr)
        if (addr % block != 0 && n++ == j)
            return addr;
}

// Runs one queue: either the given operations, or `nops` seeded ones, which pick legal steps from
// the queue's state (push an item that is out, pop or change one that is in).
template<class Q>
void run(Payload &p, Q &q, uint8_t instance, std::vector<Item> &items, const std::vector<Op> &script, SplitMix *g, size_t nops, const std::vector<double> &keys)
{
    const size_t block = block_of(instance);
    p.u8(instance);
    p.u64(items.size());
    for (const Item &it : items)
        p.f64(it.key);
    std::vector<bool> in(items.size(), false);
    const size_t      count = g ? nops : script.size();
    p.u64(count);
    for (size_t step = 0; step < count; ++step) {
        Op op{};
        if (g) {
            std::vector<size_t> out;
            for (size_t i = 0; i < items.size(); ++i)
                if (!in[i])
                    out.push_back(i);
            const int64_t k = rnd(*g, 0, 19);
            if (q.size() == 0 || (k < 9 && !out.empty()))
                op = out.empty() ? Op{4, 0, 0.} : Op{0, out[size_t(g->below(out.size()))], keys[size_t(g->below(keys.size()))]};
            else if (k < 13)
                op = Op{1, 0, 0.};
            else if (k < 16)
                op = Op{2, address(block, size_t(g->below(q.size()))), 0.};
            else if (k < 19)
                op = Op{3, address(block, size_t(g->below(q.size()))), keys[size_t(g->below(keys.size()))]};
            else
                op = Op{4, 0, 0.};
        } else
            op = script[step];
        p.u8(op.code).u64(op.a).f64(op.key);
        switch (op.code) {
        case 0:
            items[op.a].key = op.key;
            q.push(&items[op.a]);
            in[op.a] = true;
            break;
        case 1:
            in[q.top()->id] = false;
            q.pop();
            break;
        case 2:
            in[q[op.a]->id] = false;
            q.remove(op.a);
            break;
        case 3:
            q[op.a]->key = op.key;
            q.update(op.a);
            break;
        default:
            q.clear();
            in.assign(items.size(), false);
            break;
        }
        // The state: the size, the heap's slots in address order (an item's id, or 0xFFFFFFFF for
        // a padding slot), then every item's index.
        const size_t n = q.size();
        p.u64(n);
        const size_t raw = n == 0 ? 0 : address(block, n - 1) + 1;
        p.u64(raw);
        for (size_t addr = 0; addr < raw; ++addr)
            p.u32(block != 0 && addr % block == 0 ? 0xFFFFFFFFu : q[addr]->id);
        for (const Item &it : items)
            p.u64(it.qidx);
    }
}

std::vector<Item> make_items(const std::vector<double> &keys)
{
    std::vector<Item> items;
    for (size_t i = 0; i < keys.size(); ++i)
        items.push_back(Item{keys[i], uint32_t(i), NPOS});
    return items;
}

void ixpq(Payload &p, uint8_t instance, std::vector<double> initial, const std::vector<Op> &script, SplitMix *g, size_t nops, const std::vector<double> &keys)
{
    std::vector<Item> items = make_items(initial);
    auto              setter = [](Item *it, size_t idx) { it->qidx = idx; };
    auto              less   = [](const Item *a, const Item *b) { return a->key < b->key; };
    switch (instance) {
    case 0: {
        auto q = make_mutable_priority_queue<Item *, false>(std::move(setter), std::move(less));
        run(p, q, instance, items, script, g, nops, keys);
        break;
    }
    case 1: {
        auto q = make_mutable_priority_queue<Item *, true>(std::move(setter), std::move(less));
        run(p, q, instance, items, script, g, nops, keys);
        break;
    }
    case 2: {
        auto q = make_miniheap_mutable_priority_queue<Item *, 32, false>(std::move(setter), std::move(less));
        run(p, q, instance, items, script, g, nops, keys);
        break;
    }
    case 3: {
        auto q = make_miniheap_mutable_priority_queue<Item *, 32, true>(std::move(setter), std::move(less));
        run(p, q, instance, items, script, g, nops, keys);
        break;
    }
    case 4: {
        auto q = make_miniheap_mutable_priority_queue<Item *, 4, false>(std::move(setter), std::move(less));
        run(p, q, instance, items, script, g, nops, keys);
        break;
    }
    default: {
        auto q = make_miniheap_mutable_priority_queue<Item *, 4, true>(std::move(setter), std::move(less));
        run(p, q, instance, items, script, g, nops, keys);
        break;
    }
    }
}

} // namespace

void record_index_queue(Sets &sets)
{
    using Fn = std::function<void(Payload &)>;
    std::vector<Fn> e;
    const double    nan = std::numeric_limits<double>::quiet_NaN();
    for (uint8_t instance = 0; instance < 6; ++instance) {
        const size_t b = block_of(instance);
        auto         at = [b](size_t j) { return uint64_t(address(b, j)); };
        // Eight equal keys pushed, then popped: the order of pops among ties.
        {
            std::vector<Op> s;
            for (uint64_t i = 0; i < 8; ++i)
                s.push_back(Op{0, i, 1.});
            for (int i = 0; i < 8; ++i)
                s.push_back(Op{1, 0, 0.});
            e.push_back([instance, s](Payload &p) { ixpq(p, instance, std::vector<double>(8, 0.), s, nullptr, 0, {}); });
        }
        // Descending keys in, then a removal at every position from the back, the front and the
        // middle, updates that raise and lower a key, a clear, and items pushed again.
        {
            std::vector<Op> s;
            for (uint64_t i = 0; i < 10; ++i)
                s.push_back(Op{0, i, double(10 - i)});
            s.push_back(Op{2, at(9), 0.});
            s.push_back(Op{2, at(0), 0.});
            s.push_back(Op{2, at(3), 0.});
            s.push_back(Op{3, at(2), 100.});
            s.push_back(Op{3, at(5), -5.});
            s.push_back(Op{3, at(1), -5.});
            s.push_back(Op{1, 0, 0.});
            s.push_back(Op{4, 0, 0.});
            s.push_back(Op{0, 3, 2.});
            s.push_back(Op{0, 0, 2.});
            s.push_back(Op{1, 0, 0.});
            s.push_back(Op{1, 0, 0.});
            e.push_back([instance, s](Payload &p) { ixpq(p, instance, std::vector<double>(10, 0.), s, nullptr, 0, {}); });
        }
        // Keys with -0 and 0 (equal), and a NaN (no item is less than it, nor it than any).
        {
            std::vector<Op> s{Op{0, 0, 0.}, Op{0, 1, -0.}, Op{0, 2, nan}, Op{0, 3, -1.}, Op{0, 4, 0.}, Op{1, 0, 0.}, Op{1, 0, 0.}, Op{1, 0, 0.}, Op{1, 0, 0.}, Op{1, 0, 0.}};
            e.push_back([instance, s](Payload &p) { ixpq(p, instance, std::vector<double>(5, 0.), s, nullptr, 0, {}); });
        }
        // Seeded runs past a block's end: 40 items over few keys.
        e.push_back([instance](Payload &p) {
            SplitMix g(0x1B0E0000u + instance);
            ixpq(p, instance, std::vector<double>(40, 0.), {}, &g, 160, {0., 1., 1., 2., 3., 3., 3., 4.5});
        });
    }

    SplitMix        g(0x1B0E1000u);
    std::vector<Fn> random;
    for (int i = 0; i < 300; ++i) {
        const uint8_t       instance = uint8_t(i % 6);
        const size_t        m        = size_t(rnd(g, 1, i % 4 == 0 ? 70 : 12));
        const size_t        nk       = size_t(rnd(g, 1, 6));
        std::vector<double> keys;
        for (size_t k = 0; k < nk; ++k)
            keys.push_back(rnd(g, 0, 3) == 0 ? real(g, -100, 100) : double(rnd(g, -3, 3)));
        const size_t   nops = size_t(rnd(g, 1, i % 4 == 0 ? 300 : 30));
        const uint64_t seed = g.next();
        random.push_back([instance, m, keys, nops, seed](Payload &p) {
            SplitMix r(seed);
            ixpq(p, instance, std::vector<double>(m, 0.), {}, &r, nops, keys);
        });
    }
    size_t next = 0;
    record_cases(sets, "IXPQ", e, random.size(), 18, 3072, [&](Payload &p) { random[next++](p); });
}

} // namespace probe
