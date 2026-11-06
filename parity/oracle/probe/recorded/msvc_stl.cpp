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
// The recording program of crates/orca-stl/tests/stl.rs, kept as it was run on 2026-11-06: MSVC
// 14.42's STL algorithms, containers and random numbers, one line per case, as FNV-1a digests of
// the comparator-call log and the result. The test prints the same lines from orca-stl and
// compares the digests and line counts. Built with cl.exe directly (no CMake, no IDE) with the
// engine's flags in the pinned MSVC 14.42.34433 and Windows SDK 10.0.22000.0 environment
// (README.md here):
//
//   cl /nologo /O2 /Ob2 /MT /EHsc /std:c++17 msvc_stl.cpp
//
// parity probe's program (../stl.cpp) records the same behaviours as full records into
// parity/probes/stl.sop1; this one stays as the record of where the test's digests came from.
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <limits>
#include <memory>
#include <queue>
#include <random>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <vector>

static uint64_t sm_state;
static uint64_t splitmix() {
    uint64_t z = (sm_state += 0x9E3779B97F4A7C15ull);
    z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ull;
    z = (z ^ (z >> 27)) * 0x94D049BB133111EBull;
    return z ^ (z >> 31);
}

struct Fnv {
    uint64_t h = 14695981039346656037ull;
    void byte(uint8_t b) { h ^= b; h *= 1099511628211ull; }
    void u32(uint32_t v) { for (int i = 0; i < 4; ++i) byte(uint8_t(v >> (8 * i))); }
    void u64(uint64_t v) { for (int i = 0; i < 8; ++i) byte(uint8_t(v >> (8 * i))); }
};

struct E {
    int key;
    int id;
};

struct Log {
    Fnv f;
    uint64_t calls = 0;
    void add(int a, int b) { f.u32(uint32_t(a)); f.u32(uint32_t(b)); ++calls; }
};

static Fnv ids_of(const std::vector<E>& v) {
    Fnv f;
    for (auto& e : v) f.u32(uint32_t(e.id));
    return f;
}

// Patterns: 0 random in [0, range), 1 ascending, 2 descending, 3 all equal, 4 organ pipe, 5 sawtooth (period range)
static std::vector<E> make(int n, int pattern, uint64_t range, uint64_t seed) {
    sm_state = seed;
    std::vector<E> v(n);
    for (int i = 0; i < n; ++i) {
        int k = 0;
        switch (pattern) {
        case 0: k = int(splitmix() % range); break;
        case 1: k = i; break;
        case 2: k = n - i; break;
        case 3: k = 7; break;
        case 4: k = i < n / 2 ? i : n - i; break;
        case 5: k = int(i % range); break;
        }
        v[i] = E{k, i};
    }
    return v;
}

static const int SIZES[] = {0, 1, 2, 3, 4, 5, 8, 15, 16, 31, 32, 33, 34, 40, 41, 42, 47, 63, 64, 65, 100, 127, 128, 129, 200, 255, 256, 300, 511, 512, 513, 1000, 1023, 1024, 1025, 2000, 4096, 5000, 10007};
static const uint64_t RANGES[] = {2, 10, 1000, 2147483647};

#define CMP(LOG) [&](const E& a, const E& b) { LOG.add(a.id, b.id); return a.key < b.key; }

static void sorts() {
    for (int n : SIZES)
        for (int pat = 0; pat < 6; ++pat)
            for (uint64_t range : RANGES) {
                if (pat != 0 && pat != 5 && range != 2) continue;
                const uint64_t seed = uint64_t(n) * 1000003ull + uint64_t(pat) * 101ull + range;
                {
                    auto v = make(n, pat, range, seed);
                    Log log;
                    std::sort(v.begin(), v.end(), CMP(log));
                    printf("sort %d %d %llu calls=%llu log=%016llx out=%016llx\n", n, pat, (unsigned long long)range, (unsigned long long)log.calls, (unsigned long long)log.f.h, (unsigned long long)ids_of(v).h);
                }
                {
                    auto v = make(n, pat, range, seed);
                    Log log;
                    std::stable_sort(v.begin(), v.end(), CMP(log));
                    printf("stable_sort %d %d %llu calls=%llu log=%016llx out=%016llx\n", n, pat, (unsigned long long)range, (unsigned long long)log.calls, (unsigned long long)log.f.h, (unsigned long long)ids_of(v).h);
                }
                if (n > 32) {
                    for (ptrdiff_t cap : {1, 2, 3, 5, 16, 31, 32, 33, 100}) {
                        auto v = make(n, pat, range, seed);
                        Log log;
                        std::allocator<E> al;
                        E* buf = al.allocate(size_t(cap));
                        auto cmp = CMP(log);
                        std::_Stable_sort_unchecked(v.data(), v.data() + n, ptrdiff_t(n), buf, cap, cmp);
                        al.deallocate(buf, size_t(cap));
                        printf("stable_sort_cap %d %d %llu %lld calls=%llu log=%016llx out=%016llx\n", n, pat, (unsigned long long)range, (long long)cap, (unsigned long long)log.calls, (unsigned long long)log.f.h, (unsigned long long)ids_of(v).h);
                    }
                }
                for (int nth : {0, 1, n / 3, n / 2, n - 1, n}) {
                    if (nth < 0 || nth > n) continue;
                    auto v = make(n, pat, range, seed);
                    Log log;
                    std::nth_element(v.begin(), v.begin() + nth, v.end(), CMP(log));
                    printf("nth_element %d %d %llu %d calls=%llu log=%016llx out=%016llx\n", n, pat, (unsigned long long)range, nth, (unsigned long long)log.calls, (unsigned long long)log.f.h, (unsigned long long)ids_of(v).h);
                }
                for (int mid : {0, 1, n / 4, n / 2, n}) {
                    if (mid < 0 || mid > n) continue;
                    auto v = make(n, pat, range, seed);
                    Log log;
                    std::partial_sort(v.begin(), v.begin() + mid, v.end(), CMP(log));
                    printf("partial_sort %d %d %llu %d calls=%llu log=%016llx out=%016llx\n", n, pat, (unsigned long long)range, mid, (unsigned long long)log.calls, (unsigned long long)log.f.h, (unsigned long long)ids_of(v).h);
                }
                {
                    // inplace_merge of two sorted runs split at n/3
                    auto v = make(n, pat, range, seed);
                    const int mid = n / 3;
                    std::sort(v.begin(), v.begin() + mid, [](const E& a, const E& b) { return a.key < b.key || (a.key == b.key && a.id < b.id); });
                    std::sort(v.begin() + mid, v.end(), [](const E& a, const E& b) { return a.key < b.key || (a.key == b.key && a.id < b.id); });
                    Log log;
                    std::inplace_merge(v.begin(), v.begin() + mid, v.end(), CMP(log));
                    printf("inplace_merge %d %d %llu calls=%llu log=%016llx out=%016llx\n", n, pat, (unsigned long long)range, (unsigned long long)log.calls, (unsigned long long)log.f.h, (unsigned long long)ids_of(v).h);
                }
                {
                    // heaps: make_heap, then pop all, then push all back one by one, sort_heap, is_heap_until on the original
                    auto v = make(n, pat, range, seed);
                    Log log;
                    auto u = v;
                    size_t until = size_t(std::is_heap_until(u.begin(), u.end(), CMP(log)) - u.begin());
                    std::make_heap(v.begin(), v.end(), CMP(log));
                    Fnv mid = ids_of(v);
                    for (int k = n; k > 1; --k) std::pop_heap(v.begin(), v.begin() + k, CMP(log));
                    Fnv popped = ids_of(v);
                    for (int k = 1; k <= n; ++k) std::push_heap(v.begin(), v.begin() + k, CMP(log));
                    std::sort_heap(v.begin(), v.end(), CMP(log));
                    printf("heap %d %d %llu until=%zu calls=%llu log=%016llx made=%016llx popped=%016llx out=%016llx\n", n, pat, (unsigned long long)range, until, (unsigned long long)log.calls, (unsigned long long)log.f.h, (unsigned long long)mid.h, (unsigned long long)popped.h, (unsigned long long)ids_of(v).h);
                }
                {
                    // lower_bound and upper_bound on the sorted keys, for every key value in [-1, max+1] (at most 40 queries)
                    auto v = make(n, pat, range, seed);
                    std::sort(v.begin(), v.end(), [](const E& a, const E& b) { return a.key < b.key || (a.key == b.key && a.id < b.id); });
                    Log log;
                    Fnv res;
                    sm_state = seed ^ 0x5555;
                    for (int q = 0; q < 40; ++q) {
                        int key = n == 0 ? 0 : (q < 3 ? (q == 0 ? -1 : q == 1 ? v.front().key : v.back().key + 1) : v[size_t(splitmix() % uint64_t(n))].key + int(splitmix() % 3) - 1);
                        E probe{key, -1};
                        size_t lb = size_t(std::lower_bound(v.begin(), v.end(), probe, CMP(log)) - v.begin());
                        size_t ub = size_t(std::upper_bound(v.begin(), v.end(), probe, CMP(log)) - v.begin());
                        res.u64(lb);
                        res.u64(ub);
                    }
                    printf("bounds %d %d %llu calls=%llu log=%016llx out=%016llx\n", n, pat, (unsigned long long)range, (unsigned long long)log.calls, (unsigned long long)log.f.h, (unsigned long long)res.h);
                }
                {
                    auto v = make(n, pat, range, seed);
                    std::sort(v.begin(), v.end(), [](const E& a, const E& b) { return a.key < b.key || (a.key == b.key && a.id < b.id); });
                    Log log;
                    auto end = std::unique(v.begin(), v.end(), [&](const E& a, const E& b) { log.add(a.id, b.id); return a.key == b.key; });
                    v.resize(size_t(end - v.begin()));
                    printf("unique %d %d %llu len=%zu calls=%llu log=%016llx out=%016llx\n", n, pat, (unsigned long long)range, v.size(), (unsigned long long)log.calls, (unsigned long long)log.f.h, (unsigned long long)ids_of(v).h);
                }
            }
}

static uint64_t bits(double d) { uint64_t u; memcpy(&u, &d, 8); return u; }

static void minmax() {
    const double nan = std::numeric_limits<double>::quiet_NaN();
    const double inf = std::numeric_limits<double>::infinity();
    const double vals[] = {nan, -nan, -0.0, 0.0, 1.0, -1.0, inf, -inf, 2.5};
    for (double a : vals)
        for (double b : vals)
            printf("minmax %016llx %016llx max=%016llx min=%016llx clamp=%016llx\n", (unsigned long long)bits(a), (unsigned long long)bits(b), (unsigned long long)bits((std::max)(a, b)), (unsigned long long)bits((std::min)(a, b)), (unsigned long long)bits(std::clamp(2.0, a < b || a != a || b != b ? a : b, a < b || a != a || b != b ? b : a)));
    // element-wise forms on contiguous doubles (the forms initializer lists use), NaN and signed zeros placed around
    for (int n = 1; n <= 70; ++n)
        for (int variant = 0; variant < 6; ++variant) {
            std::vector<double> v(static_cast<size_t>(n));
            sm_state = uint64_t(n) * 31 + uint64_t(variant);
            for (auto& x : v) x = double(int(splitmix() % 9)) - 4.0;
            if (variant == 1) v[0] = nan;
            if (variant == 2) v[size_t(n / 2)] = nan;
            if (variant == 3) v[size_t(n - 1)] = nan;
            if (variant == 4) { for (auto& x : v) x = (splitmix() & 1) ? 0.0 : -0.0; }
            if (variant == 5) { for (size_t i = 0; i < v.size(); i += 3) v[i] = nan; }
            const double* p = v.data();
            size_t mxp = size_t(std::max_element(p, p + n) - p);
            size_t mnp = size_t(std::min_element(p, p + n) - p);
            size_t mxi = size_t(std::max_element(v.begin(), v.end()) - v.begin());
            size_t mni = size_t(std::min_element(v.begin(), v.end()) - v.begin());
            auto mm = std::minmax_element(p, p + n);
            printf("elements %d %d max=%zu min=%zu maxi=%zu mini=%zu mm=%zu,%zu\n", n, variant, mxp, mnp, mxi, mni, size_t(mm.first - p), size_t(mm.second - p));
        }
    printf("il3 %016llx %016llx %016llx %016llx\n", (unsigned long long)bits((std::max)({nan, 1.0, 2.0})), (unsigned long long)bits((std::max)({1.0, nan, 2.0})), (unsigned long long)bits((std::min)({nan, 1.0, 2.0})), (unsigned long long)bits((std::min)({-0.0, 0.0, 0.0})));
    printf("il8 %016llx %016llx\n", (unsigned long long)bits((std::max)({nan, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0})), (unsigned long long)bits((std::min)({0.0, -0.0, 1.0, 2.0, 3.0, 4.0, 5.0, -0.0})));
    printf("il16f %08x\n", [] { float r = (std::max)({std::numeric_limits<float>::quiet_NaN(), 1.f, 2.f, 3.f, 4.f, 5.f, 6.f, 7.f, 8.f, 9.f, 10.f, 11.f, 12.f, 13.f, 14.f, 15.f}); uint32_t u; memcpy(&u, &r, 4); return u; }());
    printf("ili %d %d %lld\n", (std::max)({3, 9, -2, 9}), (std::min)({3, 9, -2, -2}), (long long)(std::max)({1LL, 5LL, 2LL, 7LL, 7LL, 0LL, -1LL, 6LL, 3LL}));
}

static void hashes() {
    printf("hash int %zx %zx %zx\n", std::hash<int>()(0), std::hash<int>()(1), std::hash<int>()(-123456789));
    printf("hash i64 %zx %zx\n", std::hash<long long>()(0), std::hash<long long>()(-5000000000LL));
    printf("hash u32 %zx\n", std::hash<unsigned>()(4000000000u));
    printf("hash u64 %zx\n", std::hash<unsigned long long>()(0xFEDCBA9876543210ull));
    printf("hash size_t %zx\n", std::hash<size_t>()(12345));
    printf("hash i16 %zx u8 %zx i8 %zx\n", std::hash<short>()(-2), std::hash<unsigned char>()(200), std::hash<signed char>()(-3));
    printf("hash bool %zx %zx\n", std::hash<bool>()(false), std::hash<bool>()(true));
    printf("hash double %zx %zx %zx %zx %zx\n", std::hash<double>()(0.0), std::hash<double>()(-0.0), std::hash<double>()(1.5), std::hash<double>()(-2.25e300), std::hash<double>()(std::numeric_limits<double>::quiet_NaN()));
    printf("hash float %zx %zx %zx\n", std::hash<float>()(0.0f), std::hash<float>()(-0.0f), std::hash<float>()(3.75f));
    printf("hash string %zx %zx %zx\n", std::hash<std::string>()(""), std::hash<std::string>()("a"), std::hash<std::string>()("Sketch slices with Orca"));
}

template <class C>
static Fnv order_ints(const C& c) {
    Fnv f;
    for (auto& kv : c) { f.u64(uint64_t(int64_t(kv.first))); f.u64(uint64_t(int64_t(kv.second))); }
    return f;
}

static void containers() {
    // unordered_map<int, int>: inserts (operator[], insert, emplace), erases, rehash/reserve/clear, copies
    for (int n : {0, 1, 5, 8, 9, 10, 63, 64, 65, 100, 511, 512, 513, 1000, 4097, 20000})
        for (uint64_t range : {uint64_t(16), uint64_t(1000), uint64_t(1) << 31}) {
            sm_state = uint64_t(n) * 7 + range;
            std::unordered_map<int, int> m;
            for (int i = 0; i < n; ++i) {
                int k = int(splitmix() % range) - int(range / 4);
                switch (i % 3) {
                case 0: m[k] += i; break;
                case 1: m.insert({k, i}); break;
                case 2: m.emplace(k, -i); break;
                }
            }
            Fnv a = order_ints(m);
            size_t b0 = m.bucket_count();
            for (int i = 0; i < n / 3; ++i) m.erase(int(splitmix() % range) - int(range / 4));
            Fnv b = order_ints(m);
            std::unordered_map<int, int> copy(m);
            Fnv c = order_ints(copy);
            size_t bc = copy.bucket_count();
            std::unordered_map<int, int> assigned;
            assigned[1] = 2;
            assigned = m;
            Fnv d = order_ints(assigned);
            m.reserve(size_t(n) * 3 + 1);
            Fnv e = order_ints(m);
            size_t be = m.bucket_count();
            m.rehash(3);
            m.clear();
            for (int i = 0; i < n / 2; ++i) m[int(splitmix() % range)] = i;
            Fnv g = order_ints(m);
            printf("umap %d %llu b0=%zu a=%016llx b=%016llx copy=%016llx/%zu assigned=%016llx/%zu reserved=%016llx/%zu cleared=%016llx/%zu size=%zu\n", n, (unsigned long long)range, b0, (unsigned long long)a.h, (unsigned long long)b.h, (unsigned long long)c.h, bc, (unsigned long long)d.h, assigned.bucket_count(), (unsigned long long)e.h, be, (unsigned long long)g.h, m.bucket_count(), m.size());
        }
    // the full order of a small map, for the tests
    {
        std::unordered_map<int, int> m;
        for (int k : {5, 3, 12, 8, 1, 21, 13, 34, 55, 2, 89, 144, 0, -1, 7}) m[k] = k * k;
        printf("umap_small");
        for (auto& kv : m) printf(" %d", kv.first);
        printf(" | %zu\n", m.bucket_count());
        m.erase(13);
        m.erase(5);
        m[100] = 1;
        printf("umap_small2");
        for (auto& kv : m) printf(" %d", kv.first);
        printf(" | %zu\n", m.bucket_count());
        std::unordered_map<int, int> c(m);
        printf("umap_small_copy");
        for (auto& kv : c) printf(" %d", kv.first);
        printf(" | %zu\n", c.bucket_count());
    }
    // unordered_set<std::string> and unordered_set<double>
    {
        std::unordered_set<std::string> s;
        for (const char* w : {"wall", "infill", "skirt", "brim", "support", "ironing", "wall", "bridge", "gap fill", "top", "bottom", "travel", "retract", "wipe", "seam"}) s.insert(w);
        printf("uset_str");
        for (auto& w : s) printf(" [%s]", w.c_str());
        printf(" | %zu\n", s.bucket_count());
        std::unordered_set<double> d;
        for (double x : {0.5, -0.0, 0.0, 1.25, 3.0, -7.5, 1e100, 2.0, 0.1, 0.2, 0.3}) d.insert(x);
        printf("uset_dbl");
        for (auto& x : d) printf(" %016llx", (unsigned long long)bits(x));
        printf(" | %zu\n", d.bucket_count());
    }
    for (int n : {10, 100, 1000, 10000}) {
        sm_state = uint64_t(n) * 13;
        std::unordered_set<uint64_t> s;
        for (int i = 0; i < n; ++i) s.insert(splitmix() % (uint64_t(n) * 2));
        for (int i = 0; i < n / 4; ++i) s.erase(splitmix() % (uint64_t(n) * 2));
        Fnv f;
        for (auto x : s) f.u64(x);
        printf("uset_u64 %d %016llx %zu %zu\n", n, (unsigned long long)f.h, s.size(), s.bucket_count());
    }
    {
        // max_load_factor changed, then filled
        std::unordered_map<int, int> m;
        m.max_load_factor(0.5f);
        for (int i = 0; i < 300; ++i) m[i * 37 % 1001] = i;
        printf("umap_mlf %016llx %zu\n", (unsigned long long)order_ints(m).h, m.bucket_count());
        std::unordered_map<int, int> m2(100);
        for (int i = 0; i < 300; ++i) m2[i * 37 % 1001] = i;
        printf("umap_ctor100 %016llx %zu\n", (unsigned long long)order_ints(m2).h, m2.bucket_count());
    }
}

static void random_numbers() {
    for (uint32_t seed : {0u, 1u, 42u, 5489u, 0xFFFFFFFFu, 123456789u}) {
        std::mt19937 g(seed);
        printf("mt19937 %u", seed);
        for (int i = 0; i < 5; ++i) printf(" %u", g());
        g.discard(9994);
        printf(" 10000th=%u\n", g());
        std::mt19937_64 h(seed);
        printf("mt19937_64 %u", seed);
        for (int i = 0; i < 3; ++i) printf(" %llu", (unsigned long long)h());
        h.discard(9996);
        printf(" 10000th=%llu\n", (unsigned long long)h());
    }
    {
        std::mt19937 g;
        g.discard(9999);
        std::mt19937_64 h;
        h.discard(9999);
        printf("default 10000th %u %llu\n", g(), (unsigned long long)h());
    }
    {
        std::seed_seq sq{1u, 2u, 3u};
        std::mt19937 g(sq);
        std::seed_seq sq2{0x12345678u, 0x9abcdef0u};
        std::mt19937_64 h(sq2);
        std::seed_seq empty;
        std::mt19937 z(empty);
        printf("seedseq %u %u %llu %llu %u\n", g(), g(), (unsigned long long)h(), (unsigned long long)h(), z());
        unsigned int out[10];
        std::seed_seq sq3{7u, 8u, 9u, 10u, 11u, 12u, 13u, 14u};
        sq3.generate(out, out + 10);
        printf("seedseq_gen");
        for (unsigned x : out) printf(" %u", x);
        printf("\n");
    }
    // uniform_int_distribution
    {
        std::mt19937 g(7);
        Fnv f;
        std::uniform_int_distribution<int> d1(0, 9), d2(-5, 5), d3(0, std::numeric_limits<int>::max()), d4(std::numeric_limits<int>::min(), std::numeric_limits<int>::max()), d5(100, 100);
        for (int i = 0; i < 2000; ++i) { f.u32(uint32_t(d1(g))); f.u32(uint32_t(d2(g))); f.u32(uint32_t(d3(g))); f.u32(uint32_t(d4(g))); f.u32(uint32_t(d5(g))); }
        std::uniform_int_distribution<long long> e1(-3, 1LL << 40), e2(std::numeric_limits<long long>::min(), std::numeric_limits<long long>::max());
        std::uniform_int_distribution<unsigned long long> e3(0, 6), e4(1, (1ull << 63) + 5);
        std::uniform_int_distribution<size_t> e5(0, 1000000);
        std::uniform_int_distribution<short> e6(-300, 300);
        std::uniform_int_distribution<unsigned> e7(0, 0xFFFFFFFEu);
        for (int i = 0; i < 2000; ++i) { f.u64(uint64_t(e1(g))); f.u64(uint64_t(e2(g))); f.u64(e3(g)); f.u64(e4(g)); f.u64(e5(g)); f.u32(uint32_t(int32_t(e6(g)))); f.u32(e7(g)); }
        printf("uniform_int mt19937 %016llx next=%u\n", (unsigned long long)f.h, g());
    }
    {
        std::mt19937_64 g(7);
        Fnv f;
        std::uniform_int_distribution<int> d1(0, 9), d4(std::numeric_limits<int>::min(), std::numeric_limits<int>::max());
        std::uniform_int_distribution<long long> e1(-3, 1LL << 40), e2(std::numeric_limits<long long>::min(), std::numeric_limits<long long>::max());
        std::uniform_int_distribution<unsigned long long> e4(1, (1ull << 63) + 5);
        for (int i = 0; i < 2000; ++i) { f.u32(uint32_t(d1(g))); f.u32(uint32_t(d4(g))); f.u64(uint64_t(e1(g))); f.u64(uint64_t(e2(g))); f.u64(e4(g)); }
        printf("uniform_int mt19937_64 %016llx next=%llu\n", (unsigned long long)f.h, (unsigned long long)g());
    }
    {
        std::mt19937 g(11);
        std::mt19937_64 h(11);
        Fnv f;
        std::uniform_real_distribution<double> a(0.0, 1.0), b(-3.5, 2.25), c;
        std::uniform_real_distribution<float> d(0.f, 1.f), e(-100.f, 0.001f);
        for (int i = 0; i < 3000; ++i) {
            f.u64(bits(a(g))); f.u64(bits(b(g))); f.u64(bits(c(h)));
            float x = d(g), y = e(h);
            uint32_t ux, uy;
            memcpy(&ux, &x, 4); memcpy(&uy, &y, 4);
            f.u32(ux); f.u32(uy);
            f.u64(bits(std::generate_canonical<double, 53>(g)));
            float gc = std::generate_canonical<float, 24>(h);
            uint32_t ug;
            memcpy(&ug, &gc, 4);
            f.u32(ug);
        }
        printf("uniform_real %016llx next=%u %llu\n", (unsigned long long)f.h, g(), (unsigned long long)h());
        std::mt19937 g2(3);
        printf("uniform_real_first");
        std::uniform_real_distribution<double> u01;
        for (int i = 0; i < 4; ++i) printf(" %016llx", (unsigned long long)bits(u01(g2)));
        printf("\n");
    }
    for (int n : {0, 1, 2, 3, 10, 100, 1000, 5000}) {
        std::vector<int> v(static_cast<size_t>(n));
        for (int i = 0; i < n; ++i) v[size_t(i)] = i;
        std::mt19937 g(uint32_t(n) + 1);
        std::shuffle(v.begin(), v.end(), g);
        Fnv f;
        for (int x : v) f.u32(uint32_t(x));
        std::vector<int> w(static_cast<size_t>(n));
        for (int i = 0; i < n; ++i) w[size_t(i)] = i;
        std::mt19937_64 h(uint64_t(n) + 1);
        std::shuffle(w.begin(), w.end(), h);
        Fnv f2;
        for (int x : w) f2.u32(uint32_t(x));
        printf("shuffle %d %016llx %016llx next=%u %llu\n", n, (unsigned long long)f.h, (unsigned long long)f2.h, g(), (unsigned long long)h());
    }
    {
        std::vector<int> v{0, 1, 2, 3, 4, 5, 6, 7, 8, 9};
        std::mt19937 g(2026);
        std::shuffle(v.begin(), v.end(), g);
        printf("shuffle10");
        for (int x : v) printf(" %d", x);
        printf("\n");
    }
}

static void small_orders() {
    // full comparator logs for small inputs, for the tests
    for (int n : {5, 12, 33, 45}) {
        auto v = make(n, 0, 10, 99 + uint64_t(n));
        std::vector<int> calls;
        std::sort(v.begin(), v.end(), [&](const E& a, const E& b) { calls.push_back(a.id); calls.push_back(b.id); return a.key < b.key; });
        printf("sort_small %d keys", n);
        auto k = make(n, 0, 10, 99 + uint64_t(n));
        for (auto& e : k) printf(" %d", e.key);
        printf(" | out");
        for (auto& e : v) printf(" %d", e.id);
        printf(" | calls %zu", calls.size() / 2);
        if (n <= 12) { printf(" |"); for (int c : calls) printf(" %d", c); }
        printf("\n");
    }
}

int main() {
    sorts();
    minmax();
    hashes();
    containers();
    random_numbers();
    small_orders();
    return 0;
}
