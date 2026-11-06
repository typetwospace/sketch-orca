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
// The recording program of crates/orca-tbb/tests/tbb.rs, kept as it was run on 2026-11-06: oneTBB
// 2021.5.0 (the engine's static tbb12.lib, from the dependency build) with one thread in an arena
// of concurrency P, as the parity oracle runs it (parity/oracle/patches/0003): the leaves of
// parallel_for, the reduce trees, and the orders of the integer loops, parallel_for_each,
// parallel_invoke and task_group. Built in the pinned MSVC 14.42.34433 and Windows SDK
// 10.0.22000.0 environment (README.md here), with <dep> the dependency prefix
// <work>/deps/OrcaSlicer_dep/usr/local:
//
//   cl /nologo /O2 /Ob2 /MT /EHsc /std:c++17 /bigobj /I<dep>/include tbb.cpp /link /LIBPATH:<dep>/lib
//
// parity probe's program (../tbb.cpp) records the same behaviours into parity/probes/tbb.sop1;
// this one stays as the record of where the test's digests came from.
#include <cstdint>
#include <cstdio>
#include <string>
#include <tuple>
#include <vector>

#include <oneapi/tbb/blocked_range.h>
#include <oneapi/tbb/blocked_range2d.h>
#include <oneapi/tbb/global_control.h>
#include <oneapi/tbb/info.h>
#include <oneapi/tbb/parallel_for.h>
#include <oneapi/tbb/parallel_for_each.h>
#include <oneapi/tbb/parallel_invoke.h>
#include <oneapi/tbb/parallel_reduce.h>
#include <oneapi/tbb/partitioner.h>
#include <oneapi/tbb/task_arena.h>
#include <oneapi/tbb/task_group.h>

struct Fnv {
    uint64_t h = 14695981039346656037ull;
    void byte(uint8_t b) { h ^= b; h *= 1099511628211ull; }
    void u64(uint64_t v) { for (int i = 0; i < 8; ++i) byte(uint8_t(v >> (8 * i))); }
    void str(const std::string& s) { for (char c : s) byte(uint8_t(c)); u64(s.size()); }
};

static const int PS[] = {1, 2, 3, 4, 5, 6, 7, 8, 12, 16, 24, 32, 64};
static const long long NS[] = {0, 1, 2, 3, 4, 5, 7, 8, 9, 15, 16, 17, 31, 32, 33, 63, 64, 65, 100, 127, 128, 129, 255, 256, 257, 1000, 1023, 1024, 1025, 4096, 10007, 100000};
static const size_t GS[] = {1, 2, 3, 5, 10, 100};

template <class Part>
static void leaves_for(const char* name, const Part& part, int P, long long n, size_t g) {
    std::vector<std::pair<long long, long long>> leaves;
    tbb::parallel_for(tbb::blocked_range<long long>(0, n, g), [&](const tbb::blocked_range<long long>& r) { leaves.emplace_back(r.begin(), r.end()); }, part);
    Fnv f;
    for (auto& l : leaves) { f.u64(uint64_t(l.first)); f.u64(uint64_t(l.second)); }
    printf("for %s %d %lld %zu count=%zu digest=%016llx", name, P, n, g, leaves.size(), (unsigned long long)f.h);
    if (leaves.size() <= 40 && n <= 100) {
        printf(" |");
        for (auto& l : leaves) printf(" %lld-%lld", l.first, l.second);
    }
    printf("\n");
}

template <class Part>
static void leaves_for_size_t(const char* name, const Part& part, int P, size_t b, size_t e, size_t g) {
    std::vector<std::pair<size_t, size_t>> leaves;
    tbb::parallel_for(tbb::blocked_range<size_t>(b, e, g), [&](const tbb::blocked_range<size_t>& r) { leaves.emplace_back(r.begin(), r.end()); }, part);
    Fnv f;
    for (auto& l : leaves) { f.u64(l.first); f.u64(l.second); }
    printf("for_size_t %s %d %zu %zu %zu count=%zu digest=%016llx\n", name, P, b, e, g, leaves.size(), (unsigned long long)f.h);
}

template <class Part>
static void leaves_for_int(const char* name, const Part& part, int P, int b, int e, size_t g) {
    std::vector<std::pair<int, int>> leaves;
    tbb::parallel_for(tbb::blocked_range<int>(b, e, g), [&](const tbb::blocked_range<int>& r) { leaves.emplace_back(r.begin(), r.end()); }, part);
    Fnv f;
    for (auto& l : leaves) { f.u64(uint64_t(int64_t(l.first))); f.u64(uint64_t(int64_t(l.second))); }
    printf("for_int %s %d %d %d %zu count=%zu digest=%016llx\n", name, P, b, e, g, leaves.size(), (unsigned long long)f.h);
}

template <class Part>
static void leaves_2d(const char* name, const Part& part, int P, int r0, int r1, size_t rg, int c0, int c1, size_t cg) {
    std::vector<int> leaves;
    tbb::parallel_for(tbb::blocked_range2d<int>(r0, r1, rg, c0, c1, cg), [&](const tbb::blocked_range2d<int>& r) {
        leaves.push_back(r.rows().begin()); leaves.push_back(r.rows().end()); leaves.push_back(r.cols().begin()); leaves.push_back(r.cols().end());
    }, part);
    Fnv f;
    for (int x : leaves) f.u64(uint64_t(int64_t(x)));
    printf("for2d %s %d %d %d %zu %d %d %zu count=%zu digest=%016llx", name, P, r0, r1, rg, c0, c1, cg, leaves.size() / 4, (unsigned long long)f.h);
    if (leaves.size() <= 4 * 24) {
        printf(" |");
        for (size_t i = 0; i < leaves.size(); i += 4) printf(" %d-%d:%d-%d", leaves[i], leaves[i + 1], leaves[i + 2], leaves[i + 3]);
    }
    printf("\n");
}

struct Body {
    std::string value;
    Body() = default;
    Body(Body&, tbb::split) {}
    void operator()(const tbb::blocked_range<long long>& r) { value += "[" + std::to_string(r.begin()) + "," + std::to_string(r.end()) + ")"; }
    void join(Body& rhs) { value = "(" + value + "+" + rhs.value + ")"; }
};

static std::string fn_body(const tbb::blocked_range<long long>& r, const std::string& init) {
    return init + "[" + std::to_string(r.begin()) + "," + std::to_string(r.end()) + ")";
}
static std::string fn_join(const std::string& a, const std::string& b) { return "(" + a + "+" + b + ")"; }

static void print_tree(const char* what, const char* name, int P, long long n, size_t g, const std::string& s) {
    Fnv f;
    f.str(s);
    printf("%s %s %d %lld %zu len=%zu digest=%016llx", what, name, P, n, g, s.size(), (unsigned long long)f.h);
    if (s.size() <= 600) printf(" | %s", s.c_str());
    printf("\n");
}

template <class Part>
static void reduces(const char* name, const Part& part, int P, long long n, size_t g) {
    {
        Body b;
        tbb::parallel_reduce(tbb::blocked_range<long long>(0, n, g), b, part);
        print_tree("reduce", name, P, n, g, b.value);
    }
    {
        std::string s = tbb::parallel_reduce(tbb::blocked_range<long long>(0, n, g), std::string("I"), fn_body, fn_join, part);
        print_tree("reduce_fn", name, P, n, g, s);
    }
}

template <class Part>
static void det_reduces(const char* name, const Part& part, int P, long long n, size_t g) {
    {
        Body b;
        tbb::parallel_deterministic_reduce(tbb::blocked_range<long long>(0, n, g), b, part);
        print_tree("det_reduce", name, P, n, g, b.value);
    }
    {
        std::string s = tbb::parallel_deterministic_reduce(tbb::blocked_range<long long>(0, n, g), std::string("I"), fn_body, fn_join, part);
        print_tree("det_reduce_fn", name, P, n, g, s);
    }
}

static void run_in_arena(int P) {
    tbb::task_arena arena{P, static_cast<unsigned>(P)};
    arena.execute([&] {
        printf("arena %d max_concurrency=%d\n", P, tbb::this_task_arena::max_concurrency());
        for (long long n : NS)
            for (size_t g : GS) {
                leaves_for("auto", tbb::auto_partitioner(), P, n, g);
                leaves_for("simple", tbb::simple_partitioner(), P, n, g);
                leaves_for("static", tbb::static_partitioner(), P, n, g);
            }
        for (size_t g : {size_t(1), size_t(7)}) {
            leaves_for_size_t("auto", tbb::auto_partitioner(), P, 5, 1000005, g);
            leaves_for_size_t("static", tbb::static_partitioner(), P, 3, 4000000003ull, g * 1000);
            leaves_for_int("auto", tbb::auto_partitioner(), P, -1000, 777, g);
            leaves_for_int("static", tbb::static_partitioner(), P, -7, 2147483647, g);
            leaves_for_int("simple", tbb::simple_partitioner(), P, -50, 50, g);
        }
        for (int rows : {1, 7, 40, 300})
            for (int cols : {1, 9, 64, 500})
                for (size_t g : {size_t(1), size_t(16)}) {
                    leaves_2d("auto", tbb::auto_partitioner(), P, -3, rows - 3, g, 10, cols + 10, g);
                    leaves_2d("simple", tbb::simple_partitioner(), P, 0, rows, g, 0, cols, g * 2);
                    leaves_2d("static", tbb::static_partitioner(), P, 0, rows, g, 0, cols, g);
                }
        for (long long n : {0LL, 1LL, 2LL, 5LL, 16LL, 17LL, 100LL, 1000LL, 4097LL})
            for (size_t g : {size_t(1), size_t(3), size_t(50)}) {
                reduces("auto", tbb::auto_partitioner(), P, n, g);
                reduces("simple", tbb::simple_partitioner(), P, n, g);
                reduces("static", tbb::static_partitioner(), P, n, g);
                det_reduces("simple", tbb::simple_partitioner(), P, n, g);
                det_reduces("static", tbb::static_partitioner(), P, n, g);
                {
                    Body b;
                    tbb::parallel_reduce(tbb::blocked_range<long long>(0, n, g), b);
                    print_tree("reduce", "default", P, n, g, b.value);
                    Body d;
                    tbb::parallel_deterministic_reduce(tbb::blocked_range<long long>(0, n, g), d);
                    print_tree("det_reduce", "default", P, n, g, d.value);
                }
            }
        // integer forms of parallel_for
        for (auto t : {std::make_tuple(0, 0, 1), std::make_tuple(0, 1, 1), std::make_tuple(0, 10, 1), std::make_tuple(-5, 37, 1), std::make_tuple(3, 100, 7), std::make_tuple(0, 1000, 3), std::make_tuple(10, 3, 1), std::make_tuple(-20, 20, 40), std::make_tuple(0, 100000, 1)}) {
            const int a = std::get<0>(t), b = std::get<1>(t), s = std::get<2>(t);
            std::vector<int> order, order_s, order_auto, order_simple;
            tbb::parallel_for(a, b, s, [&](int i) { order.push_back(i); });
            tbb::parallel_for(a, b, s, [&](int i) { order_s.push_back(i); }, tbb::static_partitioner());
            tbb::parallel_for(a, b, s, [&](int i) { order_simple.push_back(i); }, tbb::simple_partitioner());
            if (s == 1) tbb::parallel_for(a, b, [&](int i) { order_auto.push_back(i); });
            Fnv f, fs, fa, fp;
            for (int i : order) f.u64(uint64_t(int64_t(i)));
            for (int i : order_s) fs.u64(uint64_t(int64_t(i)));
            for (int i : order_auto) fa.u64(uint64_t(int64_t(i)));
            for (int i : order_simple) fp.u64(uint64_t(int64_t(i)));
            printf("for_index %d %d %d %d count=%zu digest=%016llx static=%016llx simple=%016llx nostep=%016llx", P, a, b, s, order.size(), (unsigned long long)f.h, (unsigned long long)fs.h, (unsigned long long)fp.h, (unsigned long long)fa.h);
            if (order.size() <= 40) {
                printf(" |");
                for (int i : order) printf(" %d", i);
            }
            printf("\n");
        }
        for (int n : {0, 1, 2, 5, 33, 100, 1000}) {
            std::vector<int> items(static_cast<size_t>(n));
            for (int i = 0; i < n; ++i) items[size_t(i)] = i;
            std::vector<int> order;
            tbb::parallel_for_each(items.begin(), items.end(), [&](int& x) { order.push_back(x); });
            std::vector<int> order2;
            tbb::parallel_for_each(items, [&](const int& x) { order2.push_back(x); });
            Fnv f, f2;
            for (int i : order) f.u64(uint64_t(i));
            for (int i : order2) f2.u64(uint64_t(i));
            printf("for_each %d %d digest=%016llx range=%016llx", P, n, (unsigned long long)f.h, (unsigned long long)f2.h);
            if (n <= 33) {
                printf(" |");
                for (int i : order) printf(" %d", i);
            }
            printf("\n");
        }
        // parallel_invoke
        {
            std::vector<int> o;
            auto F = [&](int i) { return [&o, i] { o.push_back(i); }; };
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
                case 10: tbb::parallel_invoke(F(0), F(1), F(2), F(3), F(4), F(5), F(6), F(7), F(8), F(9)); break;
                }
                printf("invoke %d %d |", P, n);
                for (int i : o) printf(" %d", i);
                printf("\n");
            }
        }
        // task_group
        for (int n = 1; n <= 6; ++n) {
            std::vector<int> o;
            tbb::task_group tg;
            for (int i = 0; i < n; ++i) tg.run([&o, i] { o.push_back(i); });
            o.push_back(-1);
            tg.wait();
            printf("task_group %d %d |", P, n);
            for (int i : o) printf(" %d", i);
            printf("\n");
        }
        {
            // nested: tasks that run more tasks into the same group, then run_and_wait, then a second wave
            std::vector<int> o;
            tbb::task_group tg;
            tg.run([&] { o.push_back(0); tg.run([&] { o.push_back(10); }); tg.run([&] { o.push_back(11); }); o.push_back(1); });
            tg.run([&] { o.push_back(2); });
            tg.run_and_wait([&] { o.push_back(3); tg.run([&] { o.push_back(30); }); });
            tg.run([&] { o.push_back(4); });
            tg.run([&] { o.push_back(5); tbb::task_group inner; inner.run([&] { o.push_back(50); }); inner.run([&] { o.push_back(51); }); inner.wait(); o.push_back(52); });
            tg.wait();
            printf("task_group_nested %d |", P);
            for (int i : o) printf(" %d", i);
            printf("\n");
        }
        {
            // a parallel_for inside a task_group task, and task_group inside parallel_for leaves
            std::vector<int> o;
            tbb::task_group tg;
            tg.run([&] { o.push_back(100); });
            tg.run([&] { tbb::parallel_for(tbb::blocked_range<int>(0, 8, 1), [&](const tbb::blocked_range<int>& r) { o.push_back(r.begin()); }, tbb::simple_partitioner()); });
            tg.run([&] { o.push_back(200); });
            tg.wait();
            printf("task_group_for %d |", P);
            for (int i : o) printf(" %d", i);
            printf("\n");
        }
    });
}

int main() {
    printf("outside default_concurrency=%d max_concurrency=%d\n", tbb::info::default_concurrency(), tbb::this_task_arena::max_concurrency());
    tbb::global_control one(tbb::global_control::max_allowed_parallelism, 1);
    printf("outside_limited max_concurrency=%d\n", tbb::this_task_arena::max_concurrency());
    for (int P : PS) run_in_arena(P);
    return 0;
}
