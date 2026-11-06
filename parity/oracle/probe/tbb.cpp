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
// tbb.sop1: oneTBB 2021.5.0, the engine's static tbb12.lib, as the parity oracle runs it with
// SKETCH_ORCA_ORACLE_THREADS=P (parity/oracle/patches/0003): one thread, through
// global_control(max_allowed_parallelism, 1), inside a task_arena of concurrency P. Each P gives
// the split trees the engine's loops take with that many threads' worth of chunking:
//
// - TBBF: the leaves of parallel_for with each partitioner over blocked_range<long long>,
//   blocked_range<size_t>, blocked_range<int> and blocked_range2d<int>, in the order they ran;
// - TBBR: the strings a body builds through parallel_reduce and parallel_deterministic_reduce,
//   which show every leaf, split and join;
// - TBBO: the order of the integer forms of parallel_for, of parallel_for_each, and of the
//   functions parallel_invoke and task_group run, nested groups and a loop inside a task included;
// - TBBA: this_task_arena::max_concurrency() inside each arena and outside under the
//   global_control (TreeSupport3D.cpp:2385 sizes its buffers by it).
//
// Nothing here depends on the machine: the default concurrency outside any arena is not recorded.
#include <cstdint>
#include <string>
#include <tuple>
#include <vector>

#include <oneapi/tbb/blocked_range.h>
#include <oneapi/tbb/blocked_range2d.h>
#include <oneapi/tbb/global_control.h>
#include <oneapi/tbb/parallel_for.h>
#include <oneapi/tbb/parallel_for_each.h>
#include <oneapi/tbb/parallel_invoke.h>
#include <oneapi/tbb/parallel_reduce.h>
#include <oneapi/tbb/partitioner.h>
#include <oneapi/tbb/task_arena.h>
#include <oneapi/tbb/task_group.h>

#include "probe.hpp"

namespace probe {

namespace {

const int       FULL_P[]       = {1, 2, 3, 4, 5, 6, 7, 8, 12, 16, 24, 32, 64};
const int       COMMITTED_P[]  = {1, 2, 4, 8, 16};
const long long FULL_N[]       = {0, 1, 2, 3, 4, 5, 7, 8, 9, 15, 16, 17, 31, 32, 33, 63, 64, 65, 100, 127, 128, 129, 255, 256, 257, 1000, 1023, 1024, 1025, 4096, 10007, 100000};
const long long COMMITTED_N[]  = {0, 1, 2, 3, 5, 8, 16, 17, 33, 100, 1000};
const size_t    FULL_G[]       = {1, 2, 3, 5, 10, 100};
const size_t    COMMITTED_G[]  = {1, 3, 10};

template<class T, size_t K> bool in(const T (&set)[K], T v)
{
    for (const T &x : set)
        if (x == v)
            return true;
    return false;
}

// Where a record goes: the full set always, the committed set when `committed`.
struct Out
{
    Sets &sets;
    bool  committed_p;
    void  add(const char (&kind)[5], const Payload &p, bool committed)
    {
        sets.full.add(kind, p);
        if (committed_p && committed)
            sets.committed.add(kind, p);
    }
};

void add_for(Out &out, const char *range, const char *part, int P, const std::vector<int64_t> &params, const std::vector<int64_t> &leaves, bool committed)
{
    Payload p;
    p.str(range).str(part).i32(P).i64s(params).i64s(leaves);
    out.add("TBBF", p, committed);
}

template<class Part> void leaves_ll(Out &out, const char *name, const Part &part, int P, long long n, size_t g, bool committed)
{
    std::vector<int64_t> leaves;
    tbb::parallel_for(
        tbb::blocked_range<long long>(0, n, g),
        [&](const tbb::blocked_range<long long> &r) {
            leaves.push_back(r.begin());
            leaves.push_back(r.end());
        },
        part);
    add_for(out, "long long", name, P, {0, n, int64_t(g)}, leaves, committed);
}

template<class Part> void leaves_size_t(Out &out, const char *name, const Part &part, int P, size_t b, size_t e, size_t g, bool committed)
{
    std::vector<int64_t> leaves;
    tbb::parallel_for(
        tbb::blocked_range<size_t>(b, e, g),
        [&](const tbb::blocked_range<size_t> &r) {
            leaves.push_back(int64_t(r.begin()));
            leaves.push_back(int64_t(r.end()));
        },
        part);
    add_for(out, "size_t", name, P, {int64_t(b), int64_t(e), int64_t(g)}, leaves, committed);
}

template<class Part> void leaves_int(Out &out, const char *name, const Part &part, int P, int b, int e, size_t g, bool committed)
{
    std::vector<int64_t> leaves;
    tbb::parallel_for(
        tbb::blocked_range<int>(b, e, g),
        [&](const tbb::blocked_range<int> &r) {
            leaves.push_back(r.begin());
            leaves.push_back(r.end());
        },
        part);
    add_for(out, "int", name, P, {b, e, int64_t(g)}, leaves, committed);
}

template<class Part> void leaves_2d(Out &out, const char *name, const Part &part, int P, int r0, int r1, size_t rg, int c0, int c1, size_t cg, bool committed)
{
    std::vector<int64_t> leaves;
    tbb::parallel_for(
        tbb::blocked_range2d<int>(r0, r1, rg, c0, c1, cg),
        [&](const tbb::blocked_range2d<int> &r) {
            leaves.push_back(r.rows().begin());
            leaves.push_back(r.rows().end());
            leaves.push_back(r.cols().begin());
            leaves.push_back(r.cols().end());
        },
        part);
    add_for(out, "2d", name, P, {r0, r1, int64_t(rg), c0, c1, int64_t(cg)}, leaves, committed);
}

struct Body
{
    std::string value;
    Body() = default;
    Body(Body &, tbb::split) {}
    void operator()(const tbb::blocked_range<long long> &r) { value += "[" + std::to_string(r.begin()) + "," + std::to_string(r.end()) + ")"; }
    void join(Body &rhs) { value = "(" + value + "+" + rhs.value + ")"; }
};

std::string fn_body(const tbb::blocked_range<long long> &r, const std::string &init) { return init + "[" + std::to_string(r.begin()) + "," + std::to_string(r.end()) + ")"; }
std::string fn_join(const std::string &a, const std::string &b) { return "(" + a + "+" + b + ")"; }

void add_tree(Out &out, const char *algo, const char *part, int P, long long n, size_t g, const std::string &tree, bool committed)
{
    Payload p;
    p.str(algo).str(part).i32(P).i64(n).u64(g).str(tree);
    out.add("TBBR", p, committed);
}

template<class Part> void reduces(Out &out, const char *name, const Part &part, int P, long long n, size_t g, bool committed)
{
    Body b;
    tbb::parallel_reduce(tbb::blocked_range<long long>(0, n, g), b, part);
    add_tree(out, "reduce", name, P, n, g, b.value, committed);
    add_tree(out, "reduce_fn", name, P, n, g, tbb::parallel_reduce(tbb::blocked_range<long long>(0, n, g), std::string("I"), fn_body, fn_join, part), committed);
}

template<class Part> void det_reduces(Out &out, const char *name, const Part &part, int P, long long n, size_t g, bool committed)
{
    Body b;
    tbb::parallel_deterministic_reduce(tbb::blocked_range<long long>(0, n, g), b, part);
    add_tree(out, "det_reduce", name, P, n, g, b.value, committed);
    add_tree(out, "det_reduce_fn", name, P, n, g,
             tbb::parallel_deterministic_reduce(tbb::blocked_range<long long>(0, n, g), std::string("I"), fn_body, fn_join, part), committed);
}

void add_order(Out &out, const char *what, const char *variant, int P, const std::vector<int64_t> &params, const std::vector<int32_t> &order, bool committed)
{
    Payload p;
    p.str(what).str(variant).i32(P).i64s(params).i32s(order);
    out.add("TBBO", p, committed);
}

void run_in_arena(Sets &sets, int P)
{
    Out             out{sets, in(COMMITTED_P, P)};
    tbb::task_arena arena{P, static_cast<unsigned>(P)};
    arena.execute([&] {
        {
            Payload p;
            p.str("arena").i32(P).i32(tbb::this_task_arena::max_concurrency());
            out.add("TBBA", p, true);
        }
        for (long long n : FULL_N)
            for (size_t g : FULL_G) {
                const bool c = in(COMMITTED_N, n) && in(COMMITTED_G, g);
                leaves_ll(out, "auto", tbb::auto_partitioner(), P, n, g, c);
                // The simple partitioner's 100000 leaves of one element add nothing the smaller
                // cases do not show, and would make the full set ten times larger.
                if (n < 100000 || g >= 100)
                    leaves_ll(out, "simple", tbb::simple_partitioner(), P, n, g, c);
                leaves_ll(out, "static", tbb::static_partitioner(), P, n, g, c);
            }
        for (size_t g : {size_t(1), size_t(7)}) {
            const bool c = g == 7;
            leaves_size_t(out, "auto", tbb::auto_partitioner(), P, 5, 1000005, g, c);
            leaves_size_t(out, "static", tbb::static_partitioner(), P, 3, 4000000003ull, g * 1000, c);
            leaves_int(out, "auto", tbb::auto_partitioner(), P, -1000, 777, g, c);
            leaves_int(out, "static", tbb::static_partitioner(), P, -7, 2147483647, g, c);
            leaves_int(out, "simple", tbb::simple_partitioner(), P, -50, 50, g, c);
        }
        for (int rows : {1, 7, 40, 300})
            for (int cols : {1, 9, 64, 500})
                for (size_t g : {size_t(1), size_t(16)}) {
                    const bool c = rows <= 40 && cols <= 64;
                    leaves_2d(out, "auto", tbb::auto_partitioner(), P, -3, rows - 3, g, 10, cols + 10, g, c);
                    leaves_2d(out, "simple", tbb::simple_partitioner(), P, 0, rows, g, 0, cols, g * 2, c);
                    leaves_2d(out, "static", tbb::static_partitioner(), P, 0, rows, g, 0, cols, g, c);
                }
        for (long long n : {0LL, 1LL, 2LL, 5LL, 16LL, 17LL, 100LL, 1000LL, 4097LL})
            for (size_t g : {size_t(1), size_t(3), size_t(50)}) {
                const bool c = n <= 1000;
                reduces(out, "auto", tbb::auto_partitioner(), P, n, g, c);
                reduces(out, "simple", tbb::simple_partitioner(), P, n, g, c);
                reduces(out, "static", tbb::static_partitioner(), P, n, g, c);
                det_reduces(out, "simple", tbb::simple_partitioner(), P, n, g, c);
                det_reduces(out, "static", tbb::static_partitioner(), P, n, g, c);
                Body b;
                tbb::parallel_reduce(tbb::blocked_range<long long>(0, n, g), b);
                add_tree(out, "reduce", "default", P, n, g, b.value, c);
                Body d;
                tbb::parallel_deterministic_reduce(tbb::blocked_range<long long>(0, n, g), d);
                add_tree(out, "det_reduce", "default", P, n, g, d.value, c);
            }
        // The integer forms of parallel_for: (first, last, step), with each partitioner.
        for (auto t : {std::make_tuple(0, 0, 1), std::make_tuple(0, 1, 1), std::make_tuple(0, 10, 1), std::make_tuple(-5, 37, 1), std::make_tuple(3, 100, 7),
                       std::make_tuple(0, 1000, 3), std::make_tuple(10, 3, 1), std::make_tuple(-20, 20, 40), std::make_tuple(0, 100000, 1)}) {
            const int                  a = std::get<0>(t), b = std::get<1>(t), s = std::get<2>(t);
            const bool                 c = b - a <= 1000;
            const std::vector<int64_t> params = {a, b, s};
            std::vector<int32_t>       order;
            tbb::parallel_for(a, b, s, [&](int i) { order.push_back(i); });
            add_order(out, "for_index", "default", P, params, order, c);
            order.clear();
            tbb::parallel_for(a, b, s, [&](int i) { order.push_back(i); }, tbb::static_partitioner());
            add_order(out, "for_index", "static", P, params, order, c);
            order.clear();
            tbb::parallel_for(a, b, s, [&](int i) { order.push_back(i); }, tbb::simple_partitioner());
            add_order(out, "for_index", "simple", P, params, order, c);
            if (s == 1) {
                order.clear();
                tbb::parallel_for(a, b, [&](int i) { order.push_back(i); });
                add_order(out, "for_index", "nostep", P, params, order, c);
            }
        }
        for (int n : {0, 1, 2, 5, 33, 100, 1000}) {
            std::vector<int> items(static_cast<size_t>(n));
            for (int i = 0; i < n; ++i)
                items[size_t(i)] = i;
            std::vector<int32_t> order;
            tbb::parallel_for_each(items.begin(), items.end(), [&](int &x) { order.push_back(x); });
            add_order(out, "for_each", "iterators", P, {n}, order, true);
            order.clear();
            tbb::parallel_for_each(items, [&](const int &x) { order.push_back(x); });
            add_order(out, "for_each", "range", P, {n}, order, true);
        }
        {
            std::vector<int32_t> o;
            auto                 F = [&](int i) { return [&o, i] { o.push_back(i); }; };
            for (int n = 2; n <= 10; ++n) {
                o.clear();
                switch (n) {
                case 2: tbb::parallel_invoke(F(0), F(1)); break;
                case 3: tbb::parallel_invoke(F(0), F(1), F(2)); break;
                case 4: tbb::parallel_invoke(F(0), F(1), F(2), F(3)); break;
                case 5: tbb::parallel_invoke(F(0), F(1), F(2), F(3), F(4)); break;
                case 6: tbb::parallel_invoke(F(0), F(1), F(2), F(3), F(4), F(5)); break;
                case 7: tbb::parallel_invoke(F(0), F(1), F(2), F(3), F(4), F(5), F(6)); break;
                case 8: tbb::parallel_invoke(F(0), F(1), F(2), F(3), F(4), F(5), F(6), F(7)); break;
                case 9: tbb::parallel_invoke(F(0), F(1), F(2), F(3), F(4), F(5), F(6), F(7), F(8)); break;
                default: tbb::parallel_invoke(F(0), F(1), F(2), F(3), F(4), F(5), F(6), F(7), F(8), F(9)); break;
                }
                add_order(out, "invoke", "", P, {n}, o, true);
            }
        }
        for (int n = 1; n <= 6; ++n) {
            std::vector<int32_t> o;
            tbb::task_group      tg;
            for (int i = 0; i < n; ++i)
                tg.run([&o, i] { o.push_back(i); });
            o.push_back(-1);
            tg.wait();
            add_order(out, "task_group", "", P, {n}, o, true);
        }
        {
            // Tasks that run more tasks into the same group, run_and_wait, then a second wave with
            // a group inside a task.
            std::vector<int32_t> o;
            tbb::task_group      tg;
            tg.run([&] {
                o.push_back(0);
                tg.run([&] { o.push_back(10); });
                tg.run([&] { o.push_back(11); });
                o.push_back(1);
            });
            tg.run([&] { o.push_back(2); });
            tg.run_and_wait([&] {
                o.push_back(3);
                tg.run([&] { o.push_back(30); });
            });
            tg.run([&] { o.push_back(4); });
            tg.run([&] {
                o.push_back(5);
                tbb::task_group inner;
                inner.run([&] { o.push_back(50); });
                inner.run([&] { o.push_back(51); });
                inner.wait();
                o.push_back(52);
            });
            tg.wait();
            add_order(out, "task_group_nested", "", P, {}, o, true);
        }
        {
            // A parallel_for inside a task_group task.
            std::vector<int32_t> o;
            tbb::task_group      tg;
            tg.run([&] { o.push_back(100); });
            tg.run([&] {
                tbb::parallel_for(tbb::blocked_range<int>(0, 8, 1), [&](const tbb::blocked_range<int> &r) { o.push_back(r.begin()); }, tbb::simple_partitioner());
            });
            tg.run([&] { o.push_back(200); });
            tg.wait();
            add_order(out, "task_group_for", "", P, {}, o, true);
        }
    });
}

} // namespace

void record_tbb(Sets &sets)
{
    head(sets, "tbb");
    tbb::global_control one(tbb::global_control::max_allowed_parallelism, 1);
    {
        Payload p;
        p.str("outside").i32(0).i32(tbb::this_task_arena::max_concurrency());
        sets.committed.add("TBBA", p);
        sets.full.add("TBBA", p);
    }
    for (int P : FULL_P)
        run_in_arena(sets, P);
}

} // namespace probe
