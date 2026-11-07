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
// stl.sop1: MSVC 14.42's standard library as the engine compiles it.
//
// - SORT: std::sort, stable_sort (also with each temporary-buffer size, through
//   _Stable_sort_unchecked), nth_element, partial_sort, inplace_merge, the heap functions,
//   priority_queue, lower_bound and upper_bound, and unique, on seeded inputs with ties (integer
//   keys) and with NaNs and signed zeros (double keys): every comparator call, in order, and the
//   result.
// - VALS: std::min, max and clamp on NaNs and signed zeros, the element searches, and the
//   initializer-list forms.
// - HASH: std::hash of the types the engine hashes.
// - UMAP and USET: unordered_map<int, int> through inserts, erases, copies, assignment, reserve,
//   rehash, clear and max_load_factor, and unordered_set of strings, doubles and 64-bit integers:
//   the iteration order and the bucket count at each step.
// - RNGS, SHUF and SSEQ: mt19937 and mt19937_64 with every seeding, uniform_int_distribution for
//   each integer type, uniform_real_distribution, generate_canonical, std::shuffle and seed_seq.
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <limits>
#include <memory>
#include <queue>
#include <random>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <vector>

#include "probe.hpp"

namespace probe {

namespace {

// ---------------------------------------------------------------- sorts

struct E
{
    int key;
    int id;
};

struct D
{
    double key;
    int    id;
};

// Patterns: 0 random in [0, range), 1 ascending, 2 descending, 3 all equal, 4 organ pipe, 5 sawtooth.
std::vector<E> make(int n, int pattern, uint64_t range, uint64_t seed)
{
    SplitMix       g(seed);
    std::vector<E> v(static_cast<size_t>(n));
    for (int i = 0; i < n; ++i) {
        int k = 0;
        switch (pattern) {
        case 0: k = int(g.next() % range); break;
        case 1: k = i; break;
        case 2: k = n - i; break;
        case 3: k = 7; break;
        case 4: k = i < n / 2 ? i : n - i; break;
        default: k = int(uint64_t(i) % range); break;
        }
        v[size_t(i)] = E{k, i};
    }
    return v;
}

// Keys with NaNs, signed zeros and ties.
std::vector<D> make_doubles(int n, uint64_t seed)
{
    SplitMix       g(seed);
    std::vector<D> v(static_cast<size_t>(n));
    const double   nan = std::numeric_limits<double>::quiet_NaN();
    for (int i = 0; i < n; ++i) {
        double k = double(int(g.below(9)) - 4) * 0.5;
        switch (g.below(8)) {
        case 0: k = nan; break;
        case 1: k = -0.0; break;
        case 2: k = 0.0; break;
        default: break;
        }
        v[size_t(i)] = D{k, i};
    }
    return v;
}

struct Log
{
    std::vector<int32_t> calls;
    void                 add(int a, int b)
    {
        calls.push_back(a);
        calls.push_back(b);
    }
};

template<class T> std::vector<int32_t> ids_of(const std::vector<T> &v)
{
    std::vector<int32_t> out;
    for (const T &e : v)
        out.push_back(e.id);
    return out;
}

template<class T> std::vector<uint64_t> keys_of(const std::vector<T> &v)
{
    std::vector<uint64_t> out;
    for (const T &e : v) {
        if constexpr (std::is_same_v<T, D>)
            out.push_back(bits(e.key));
        else
            out.push_back(uint64_t(int64_t(e.key)));
    }
    return out;
}

// One SORT record: the algorithm, the key type (0 int, 1 double), its parameters, the input (keys
// and ids in order), every comparator call (the two ids), the result ids and what else it returns.
template<class T>
void add_sort(File &f, const char *algo, const std::vector<int64_t> &params, const std::vector<T> &input, const Log &log, const std::vector<int32_t> &result,
              const std::vector<int64_t> &extra)
{
    Payload p;
    p.str(algo).u8(std::is_same_v<T, D> ? 1 : 0).i64s(params).u64s(keys_of(input)).i32s(ids_of(input)).i32s(log.calls).i32s(result).i64s(extra);
    f.add("SORT", p);
}

#define CMP(LOG) [&](const auto &a, const auto &b) { LOG.add(a.id, b.id); return a.key < b.key; }

template<class T> bool by_key_then_id(const T &a, const T &b) { return a.key < b.key || (a.key == b.key && a.id < b.id); }

struct SortPlan
{
    std::vector<int>      sizes;
    std::vector<int>      caps;
    bool                  all_patterns;
};

void sorts_into(File &f, const SortPlan &plan)
{
    const uint64_t RANGES[] = {2, 10, 1000, 2147483647};
    for (int n : plan.sizes)
        for (int pat = 0; pat < 6; ++pat)
            for (uint64_t range : RANGES) {
                if (plan.all_patterns) {
                    if (pat != 0 && pat != 5 && range != 2)
                        continue;
                } else {
                    const bool keep = (pat == 0 && (range == 10 || range == 2147483647)) || (pat == 3 && range == 2) || (pat == 5 && range == 10);
                    if (!keep)
                        continue;
                }
                const uint64_t seed = uint64_t(n) * 1000003ull + uint64_t(pat) * 101ull + range;
                const auto     base = make(n, pat, range, seed);
                const std::vector<int64_t> what = {n, pat, int64_t(range)};
                {
                    auto v = base;
                    Log  log;
                    std::sort(v.begin(), v.end(), CMP(log));
                    add_sort(f, "sort", {}, base, log, ids_of(v), {});
                }
                {
                    auto v = base;
                    Log  log;
                    std::stable_sort(v.begin(), v.end(), CMP(log));
                    add_sort(f, "stable_sort", {}, base, log, ids_of(v), {});
                }
                if (n > 32)
                    for (int cap : plan.caps) {
                        auto              v = base;
                        Log               log;
                        std::allocator<E> al;
                        E                *buf = al.allocate(size_t(cap));
                        auto              cmp = CMP(log);
                        std::_Stable_sort_unchecked(v.data(), v.data() + n, ptrdiff_t(n), buf, ptrdiff_t(cap), cmp);
                        al.deallocate(buf, size_t(cap));
                        add_sort(f, "stable_sort_cap", {cap}, base, log, ids_of(v), {});
                    }
                for (int nth : {0, 1, n / 3, n / 2, n - 1, n}) {
                    if (nth < 0 || nth > n || (!plan.all_patterns && nth != n / 2 && nth != 0))
                        continue;
                    auto v = base;
                    Log  log;
                    std::nth_element(v.begin(), v.begin() + nth, v.end(), CMP(log));
                    add_sort(f, "nth_element", {nth}, base, log, ids_of(v), {});
                }
                for (int mid : {0, 1, n / 4, n / 2, n}) {
                    if (mid < 0 || mid > n || (!plan.all_patterns && mid != 1 && mid != n / 2))
                        continue;
                    auto v = base;
                    Log  log;
                    std::partial_sort(v.begin(), v.begin() + mid, v.end(), CMP(log));
                    add_sort(f, "partial_sort", {mid}, base, log, ids_of(v), {});
                }
                {
                    auto      v   = base;
                    const int mid = n / 3;
                    std::sort(v.begin(), v.begin() + mid, by_key_then_id<E>);
                    std::sort(v.begin() + mid, v.end(), by_key_then_id<E>);
                    const auto in = v;
                    Log        log;
                    std::inplace_merge(v.begin(), v.begin() + mid, v.end(), CMP(log));
                    add_sort(f, "inplace_merge", {mid}, in, log, ids_of(v), {});
                }
                {
                    // is_heap_until on the input, make_heap, pop_heap down to one, push_heap up to all,
                    // sort_heap: one log; the result is the ids after make, after the pops and at the end.
                    auto   v = base;
                    Log    log;
                    size_t until = size_t(std::is_heap_until(v.begin(), v.end(), CMP(log)) - v.begin());
                    std::make_heap(v.begin(), v.end(), CMP(log));
                    auto result = ids_of(v);
                    for (int k = n; k > 1; --k)
                        std::pop_heap(v.begin(), v.begin() + k, CMP(log));
                    for (int32_t id : ids_of(v))
                        result.push_back(id);
                    for (int k = 1; k <= n; ++k)
                        std::push_heap(v.begin(), v.begin() + k, CMP(log));
                    std::sort_heap(v.begin(), v.end(), CMP(log));
                    for (int32_t id : ids_of(v))
                        result.push_back(id);
                    add_sort(f, "heap", {}, base, log, result, {int64_t(until)});
                }
                {
                    // priority_queue: push every element, then pop them all; the result is the pop order.
                    Log  log;
                    auto cmp = CMP(log);
                    std::priority_queue<E, std::vector<E>, decltype(cmp)> q(cmp);
                    for (const E &e : base)
                        q.push(e);
                    std::vector<int32_t> order;
                    while (!q.empty()) {
                        order.push_back(q.top().id);
                        q.pop();
                    }
                    add_sort(f, "priority_queue", {}, base, log, order, {});
                }
                {
                    // lower_bound and upper_bound of 40 seeded keys (the probe element's id is -1).
                    auto v = base;
                    std::sort(v.begin(), v.end(), by_key_then_id<E>);
                    Log                  log;
                    std::vector<int64_t> queries, found;
                    SplitMix             g(seed ^ 0x5555);
                    for (int q = 0; q < 40; ++q) {
                        const int key = n == 0 ? 0
                                               : (q < 3 ? (q == 0 ? -1 : q == 1 ? v.front().key : v.back().key + 1)
                                                        : v[size_t(g.next() % uint64_t(n))].key + int(g.next() % 3) - 1);
                        const E probe_e{key, -1};
                        queries.push_back(key);
                        found.push_back(int64_t(std::lower_bound(v.begin(), v.end(), probe_e, CMP(log)) - v.begin()));
                        found.push_back(int64_t(std::upper_bound(v.begin(), v.end(), probe_e, CMP(log)) - v.begin()));
                    }
                    add_sort(f, "bounds", queries, v, log, ids_of(v), found);
                }
                {
                    auto v = base;
                    std::sort(v.begin(), v.end(), by_key_then_id<E>);
                    const auto in = v;
                    Log        log;
                    auto       end = std::unique(v.begin(), v.end(), [&](const E &a, const E &b) {
                        log.add(a.id, b.id);
                        return a.key == b.key;
                    });
                    v.resize(size_t(end - v.begin()));
                    add_sort(f, "unique", {}, in, log, ids_of(v), {int64_t(v.size())});
                }
                (void) what;
            }
    // Double keys with NaNs and signed zeros, compared with <.
    for (int n : plan.sizes)
        for (uint64_t seed : {1ull, 2ull, 3ull}) {
            const auto base = make_doubles(n, uint64_t(n) * 7919ull + seed);
            {
                auto v = base;
                Log  log;
                std::sort(v.begin(), v.end(), CMP(log));
                add_sort(f, "sort", {}, base, log, ids_of(v), {});
            }
            {
                auto v = base;
                Log  log;
                std::stable_sort(v.begin(), v.end(), CMP(log));
                add_sort(f, "stable_sort", {}, base, log, ids_of(v), {});
            }
            if (n > 0) {
                auto v = base;
                Log  log;
                std::nth_element(v.begin(), v.begin() + n / 2, v.end(), CMP(log));
                add_sort(f, "nth_element", {n / 2}, base, log, ids_of(v), {});
                v = base;
                Log log2;
                std::partial_sort(v.begin(), v.begin() + n / 2, v.end(), CMP(log2));
                add_sort(f, "partial_sort", {n / 2}, base, log2, ids_of(v), {});
            }
        }
}

#undef CMP

// ---------------------------------------------------------------- values

void add_vals(Sets &sets, const std::string &name, const std::vector<uint64_t> &in, const std::vector<uint64_t> &out)
{
    Payload p;
    p.str(name).u64s(in).u64s(out);
    sets.committed.add("VALS", p);
    sets.full.add("VALS", p);
}

void record_values(Sets &sets)
{
    const double nan    = std::numeric_limits<double>::quiet_NaN();
    const double inf    = std::numeric_limits<double>::infinity();
    const double vals[] = {nan, -nan, -0.0, 0.0, 1.0, -1.0, inf, -inf, 2.5};
    for (double a : vals)
        for (double b : vals) {
            const bool   lo_first = a < b || a != a || b != b;
            const double lo = lo_first ? a : b, hi = lo_first ? b : a;
            add_vals(sets, "max-min-clamp-f64", {bits(a), bits(b), bits(lo), bits(hi)},
                     {bits((std::max)(a, b)), bits((std::min)(a, b)), bits(std::clamp(2.0, lo, hi))});
            const float fa = float(a), fb = float(b);
            add_vals(sets, "max-min-f32", {uint64_t(bits(fa)), uint64_t(bits(fb))}, {uint64_t(bits((std::max)(fa, fb))), uint64_t(bits((std::min)(fa, fb)))});
        }
    for (int n = 1; n <= 70; ++n)
        for (int variant = 0; variant < 6; ++variant) {
            std::vector<double> v(static_cast<size_t>(n));
            SplitMix            g(uint64_t(n) * 31 + uint64_t(variant));
            for (auto &x : v)
                x = double(int(g.next() % 9)) - 4.0;
            if (variant == 1)
                v[0] = nan;
            if (variant == 2)
                v[size_t(n / 2)] = nan;
            if (variant == 3)
                v[size_t(n - 1)] = nan;
            if (variant == 4)
                for (auto &x : v)
                    x = (g.next() & 1) ? 0.0 : -0.0;
            if (variant == 5)
                for (size_t i = 0; i < v.size(); i += 3)
                    v[i] = nan;
            const double *p  = v.data();
            auto          mm = std::minmax_element(p, p + n);
            std::vector<uint64_t> in;
            for (double x : v)
                in.push_back(bits(x));
            add_vals(sets, "elements-f64", in,
                     {uint64_t(std::max_element(p, p + n) - p), uint64_t(std::min_element(p, p + n) - p), uint64_t(std::max_element(v.begin(), v.end()) - v.begin()),
                      uint64_t(std::min_element(v.begin(), v.end()) - v.begin()), uint64_t(mm.first - p), uint64_t(mm.second - p)});
        }
    add_vals(sets, "initializer-list-f64", {},
             {bits((std::max)({nan, 1.0, 2.0})), bits((std::max)({1.0, nan, 2.0})), bits((std::min)({nan, 1.0, 2.0})), bits((std::min)({-0.0, 0.0, 0.0})),
              bits((std::max)({nan, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0})), bits((std::min)({0.0, -0.0, 1.0, 2.0, 3.0, 4.0, 5.0, -0.0}))});
    add_vals(sets, "initializer-list-int", {},
             {uint64_t(int64_t((std::max)({3, 9, -2, 9}))), uint64_t(int64_t((std::min)({3, 9, -2, -2}))), uint64_t((std::max)({1LL, 5LL, 2LL, 7LL, 7LL, 0LL, -1LL, 6LL, 3LL}))});
}

// ---------------------------------------------------------------- hashes

void record_hashes(Sets &sets)
{
    const auto add = [&](const char *type, const std::vector<std::pair<std::string, uint64_t>> &cases) {
        Payload p;
        p.str(type).u64(cases.size());
        for (const auto &c : cases)
            p.str(c.first).u64(c.second);
        sets.committed.add("HASH", p);
        sets.full.add("HASH", p);
    };
    const auto bytes_of = [](const auto &v) { return std::string(reinterpret_cast<const char *>(&v), sizeof v); };
    std::vector<std::pair<std::string, uint64_t>> i32s, i64s, u64s, f64s, f32s, strs;
    SplitMix                                      g(0x4841534800000001ull);
    for (int i = 0; i < 64; ++i) {
        const int       a = i < 3 ? (i == 0 ? 0 : i == 1 ? 1 : -123456789) : int(uint32_t(g.next()));
        const long long b = (long long) g.next();
        const uint64_t  c = g.next();
        const double    d = i < 5 ? (i == 0 ? 0.0 : i == 1 ? -0.0 : i == 2 ? 1.5 : i == 3 ? -2.25e300 : std::numeric_limits<double>::quiet_NaN())
                                  : from_bits(g.next());
        const float     f = i < 3 ? (i == 0 ? 0.0f : i == 1 ? -0.0f : 3.75f) : from_bits32(uint32_t(g.next()));
        i32s.emplace_back(bytes_of(a), uint64_t(std::hash<int>()(a)));
        i64s.emplace_back(bytes_of(b), uint64_t(std::hash<long long>()(b)));
        u64s.emplace_back(bytes_of(c), uint64_t(std::hash<unsigned long long>()(c)));
        f64s.emplace_back(bytes_of(d), uint64_t(std::hash<double>()(d)));
        f32s.emplace_back(bytes_of(f), uint64_t(std::hash<float>()(f)));
        std::string s;
        const size_t len = size_t(g.below(40));
        for (size_t k = 0; k < len; ++k)
            s += char(g.below(256));
        strs.emplace_back(s, uint64_t(std::hash<std::string>()(s)));
    }
    strs.emplace_back("", uint64_t(std::hash<std::string>()("")));
    strs.emplace_back("Sketch slices with Orca", uint64_t(std::hash<std::string>()("Sketch slices with Orca")));
    add("i32", i32s);
    add("i64", i64s);
    add("u64", u64s);
    add("f64", f64s);
    add("f32", f32s);
    add("string", strs);
}

// ---------------------------------------------------------------- unordered containers

enum UmapOp : uint8_t {
    ADD_ASSIGN = 0, // m[a] += b
    INSERT     = 1, // m.insert({a, b})
    EMPLACE    = 2, // m.emplace(a, b)
    ERASE      = 3, // m.erase(a)
    RESERVE    = 4, // m.reserve(a)
    REHASH     = 5, // m.rehash(a)
    CLEAR      = 6, // m.clear()
    ASSIGN_AT  = 7, // m[a] = b
    COPY       = 8, // m = Map(m), a copy
    ASSIGNED   = 9, // Map t; t[1] = 2; t = m; m = std::move(t)
    MAX_LOAD   = 10, // m.max_load_factor(a / 1000.f)
    BUCKETS    = 11, // m = Map(a)
    SNAPSHOT   = 12,
};

struct UmapScript
{
    std::string                                       name;
    std::vector<std::tuple<uint8_t, int64_t, int64_t>> ops;
};

void run_umap(File &f, const UmapScript &s)
{
    std::unordered_map<int, int>                                            m;
    std::vector<std::tuple<std::vector<int64_t>, uint64_t, uint64_t>> snaps;
    for (const auto &[op, a, b] : s.ops) {
        switch (op) {
        case ADD_ASSIGN: m[int(a)] += int(b); break;
        case INSERT: m.insert({int(a), int(b)}); break;
        case EMPLACE: m.emplace(int(a), int(b)); break;
        case ERASE: m.erase(int(a)); break;
        case RESERVE: m.reserve(size_t(a)); break;
        case REHASH: m.rehash(size_t(a)); break;
        case CLEAR: m.clear(); break;
        case ASSIGN_AT: m[int(a)] = int(b); break;
        case COPY: {
            std::unordered_map<int, int> copy(m);
            m = std::move(copy);
            break;
        }
        case ASSIGNED: {
            std::unordered_map<int, int> t;
            t[1] = 2;
            t    = m;
            m    = std::move(t);
            break;
        }
        case MAX_LOAD: m.max_load_factor(float(a) / 1000.f); break;
        case BUCKETS: m = std::unordered_map<int, int>(size_t(a)); break;
        default: {
            std::vector<int64_t> kv;
            for (const auto &e : m) {
                kv.push_back(e.first);
                kv.push_back(e.second);
            }
            snaps.emplace_back(kv, uint64_t(m.bucket_count()), uint64_t(m.size()));
        }
        }
    }
    Payload p;
    p.str(s.name).u64(s.ops.size());
    for (const auto &[op, a, b] : s.ops)
        p.u8(op).i64(a).i64(b);
    p.u64(snaps.size());
    for (const auto &[kv, buckets, size] : snaps)
        p.i64s(kv).u64(buckets).u64(size);
    f.add("UMAP", p);
}

UmapScript umap_script(int n, uint64_t range)
{
    UmapScript s;
    s.name = "umap " + std::to_string(n) + " " + std::to_string(range);
    SplitMix   g(uint64_t(n) * 7 + range);
    const auto key = [&] { return int64_t(int(g.next() % range) - int(range / 4)); };
    for (int i = 0; i < n; ++i)
        s.ops.emplace_back(uint8_t(i % 3), key(), int64_t(i % 3 == 2 ? -i : i));
    s.ops.emplace_back(SNAPSHOT, 0, 0);
    for (int i = 0; i < n / 3; ++i)
        s.ops.emplace_back(ERASE, key(), 0);
    s.ops.emplace_back(SNAPSHOT, 0, 0);
    s.ops.emplace_back(COPY, 0, 0);
    s.ops.emplace_back(SNAPSHOT, 0, 0);
    s.ops.emplace_back(ASSIGNED, 0, 0);
    s.ops.emplace_back(SNAPSHOT, 0, 0);
    s.ops.emplace_back(RESERVE, int64_t(n) * 3 + 1, 0);
    s.ops.emplace_back(SNAPSHOT, 0, 0);
    s.ops.emplace_back(REHASH, 3, 0);
    s.ops.emplace_back(SNAPSHOT, 0, 0);
    s.ops.emplace_back(CLEAR, 0, 0);
    for (int i = 0; i < n / 2; ++i)
        s.ops.emplace_back(ASSIGN_AT, int64_t(int(g.next() % range)), i);
    s.ops.emplace_back(SNAPSHOT, 0, 0);
    return s;
}

void record_unordered(Sets &sets, const std::vector<int> &committed_sizes, const std::vector<int> &full_sizes)
{
    for (int n : full_sizes)
        for (uint64_t range : {uint64_t(16), uint64_t(1000), uint64_t(1) << 31}) {
            const UmapScript s = umap_script(n, range);
            run_umap(sets.full, s);
            if (std::find(committed_sizes.begin(), committed_sizes.end(), n) != committed_sizes.end())
                run_umap(sets.committed, s);
        }
    {
        UmapScript s{"umap small", {}};
        for (int k : {5, 3, 12, 8, 1, 21, 13, 34, 55, 2, 89, 144, 0, -1, 7})
            s.ops.emplace_back(ASSIGN_AT, k, k * k);
        s.ops.emplace_back(SNAPSHOT, 0, 0);
        s.ops.emplace_back(ERASE, 13, 0);
        s.ops.emplace_back(ERASE, 5, 0);
        s.ops.emplace_back(ASSIGN_AT, 100, 1);
        s.ops.emplace_back(SNAPSHOT, 0, 0);
        s.ops.emplace_back(COPY, 0, 0);
        s.ops.emplace_back(SNAPSHOT, 0, 0);
        run_umap(sets.committed, s);
        run_umap(sets.full, s);
    }
    {
        UmapScript s{"umap max_load_factor", {}};
        s.ops.emplace_back(MAX_LOAD, 500, 0);
        for (int i = 0; i < 300; ++i)
            s.ops.emplace_back(ASSIGN_AT, i * 37 % 1001, i);
        s.ops.emplace_back(SNAPSHOT, 0, 0);
        s.ops.emplace_back(BUCKETS, 100, 0);
        for (int i = 0; i < 300; ++i)
            s.ops.emplace_back(ASSIGN_AT, i * 37 % 1001, i);
        s.ops.emplace_back(SNAPSHOT, 0, 0);
        run_umap(sets.committed, s);
        run_umap(sets.full, s);
    }
    // unordered_set<std::string>, <double> and <uint64_t>: keys as their bytes (numbers little-endian).
    const auto add_set = [&](File &f, const char *type, const std::vector<std::pair<uint8_t, std::string>> &ops, const std::vector<std::string> &order,
                             uint64_t buckets, uint64_t size) {
        Payload p;
        p.str(type).u64(ops.size());
        for (const auto &o : ops)
            p.u8(o.first).str(o.second);
        p.u64(order.size());
        for (const std::string &k : order)
            p.str(k);
        p.u64(buckets).u64(size);
        f.add("USET", p);
    };
    {
        std::vector<std::pair<uint8_t, std::string>> ops;
        std::unordered_set<std::string>              s;
        for (const char *w : {"wall", "infill", "skirt", "brim", "support", "ironing", "wall", "bridge", "gap fill", "top", "bottom", "travel", "retract", "wipe", "seam"}) {
            s.insert(w);
            ops.emplace_back(0, w);
        }
        std::vector<std::string> order(s.begin(), s.end());
        add_set(sets.committed, "string", ops, order, s.bucket_count(), s.size());
        add_set(sets.full, "string", ops, order, s.bucket_count(), s.size());
    }
    {
        std::vector<std::pair<uint8_t, std::string>> ops;
        std::unordered_set<double>                   s;
        for (double x : {0.5, -0.0, 0.0, 1.25, 3.0, -7.5, 1e100, 2.0, 0.1, 0.2, 0.3}) {
            s.insert(x);
            ops.emplace_back(0, std::string(reinterpret_cast<const char *>(&x), 8));
        }
        std::vector<std::string> order;
        for (double x : s)
            order.emplace_back(reinterpret_cast<const char *>(&x), 8);
        add_set(sets.committed, "f64", ops, order, s.bucket_count(), s.size());
        add_set(sets.full, "f64", ops, order, s.bucket_count(), s.size());
    }
    for (int n : {10, 100, 1000, 10000}) {
        SplitMix                                     g(uint64_t(n) * 13);
        std::unordered_set<uint64_t>                 s;
        std::vector<std::pair<uint8_t, std::string>> ops;
        for (int i = 0; i < n; ++i) {
            const uint64_t k = g.next() % (uint64_t(n) * 2);
            s.insert(k);
            ops.emplace_back(0, std::string(reinterpret_cast<const char *>(&k), 8));
        }
        for (int i = 0; i < n / 4; ++i) {
            const uint64_t k = g.next() % (uint64_t(n) * 2);
            s.erase(k);
            ops.emplace_back(1, std::string(reinterpret_cast<const char *>(&k), 8));
        }
        std::vector<std::string> order;
        for (uint64_t x : s)
            order.emplace_back(reinterpret_cast<const char *>(&x), 8);
        add_set(sets.full, "u64", ops, order, s.bucket_count(), s.size());
        if (n <= 100)
            add_set(sets.committed, "u64", ops, order, s.bucket_count(), s.size());
    }
}

// ---------------------------------------------------------------- random numbers

enum Draw : uint8_t {
    NEXT = 0,
    DISCARD,
    INT_I32,
    INT_I64,
    INT_U64,
    INT_SIZE,
    INT_I16,
    INT_U32,
    REAL_F64,
    REAL_F32,
    CANON_F64,
    CANON_F32,
    REAL_DEFAULT,
};

template<class Engine> uint64_t draw(Engine &e, uint8_t kind, uint64_t a, uint64_t b)
{
    switch (kind) {
    case NEXT: return uint64_t(e());
    case DISCARD: e.discard((unsigned long long) a); return 0;
    case INT_I32: return uint64_t(int64_t(std::uniform_int_distribution<int>(int(int64_t(a)), int(int64_t(b)))(e)));
    case INT_I64: return uint64_t(std::uniform_int_distribution<long long>((long long) a, (long long) b)(e));
    case INT_U64: return std::uniform_int_distribution<unsigned long long>(a, b)(e);
    case INT_SIZE: return uint64_t(std::uniform_int_distribution<size_t>(size_t(a), size_t(b))(e));
    case INT_I16: return uint64_t(int64_t(std::uniform_int_distribution<short>(short(int64_t(a)), short(int64_t(b)))(e)));
    case INT_U32: return uint64_t(std::uniform_int_distribution<unsigned>(unsigned(a), unsigned(b))(e));
    case REAL_F64: return bits(std::uniform_real_distribution<double>(from_bits(a), from_bits(b))(e));
    case REAL_F32: return uint64_t(bits(std::uniform_real_distribution<float>(from_bits32(uint32_t(a)), from_bits32(uint32_t(b)))(e)));
    case CANON_F64: return bits(std::generate_canonical<double, 53>(e));
    case CANON_F32: return uint64_t(bits(std::generate_canonical<float, 24>(e)));
    default: return bits(std::uniform_real_distribution<double>()(e));
    }
}

struct DrawPlan
{
    uint8_t  kind;
    uint64_t a, b;
};

std::vector<DrawPlan> draw_plan(size_t rounds)
{
    std::vector<DrawPlan> plan;
    const int             imin = std::numeric_limits<int>::min(), imax = std::numeric_limits<int>::max();
    for (size_t r = 0; r < rounds; ++r) {
        plan.push_back({NEXT, 0, 0});
        plan.push_back({INT_I32, 0, 9});
        plan.push_back({INT_I32, uint64_t(int64_t(-5)), 5});
        plan.push_back({INT_I32, 0, uint64_t(imax)});
        plan.push_back({INT_I32, uint64_t(int64_t(imin)), uint64_t(imax)});
        plan.push_back({INT_I32, 100, 100});
        plan.push_back({INT_I64, uint64_t(int64_t(-3)), 1ull << 40});
        plan.push_back({INT_I64, 0x8000000000000000ull, 0x7FFFFFFFFFFFFFFFull});
        plan.push_back({INT_U64, 0, 6});
        plan.push_back({INT_U64, 1, (1ull << 63) + 5});
        plan.push_back({INT_SIZE, 0, 1000000});
        plan.push_back({INT_I16, uint64_t(int64_t(-300)), 300});
        plan.push_back({INT_U32, 0, 0xFFFFFFFEull});
        plan.push_back({REAL_F64, bits(0.0), bits(1.0)});
        plan.push_back({REAL_F64, bits(-3.5), bits(2.25)});
        plan.push_back({REAL_F32, bits(0.f), bits(1.f)});
        plan.push_back({REAL_F32, bits(-100.f), bits(0.001f)});
        plan.push_back({CANON_F64, 0, 0});
        plan.push_back({CANON_F32, 0, 0});
        plan.push_back({REAL_DEFAULT, 0, 0});
        if (r % 50 == 7)
            plan.push_back({DISCARD, 997, 0});
    }
    return plan;
}

template<class Engine> void add_rng(File &f, const char *engine, const std::vector<uint64_t> &seed, Engine e, const std::vector<DrawPlan> &plan)
{
    Payload p;
    p.str(engine).u64s(seed).u64(plan.size());
    for (const DrawPlan &d : plan)
        p.u8(d.kind).u64(d.a).u64(d.b).u64(draw(e, d.kind, d.a, d.b));
    f.add("RNGS", p);
}

void record_random(Sets &sets)
{
    const auto small = draw_plan(4), big = draw_plan(400);
    for (uint32_t seed : {0u, 1u, 7u, 11u, 42u, 5489u, 0xFFFFFFFFu, 123456789u}) {
        add_rng(sets.committed, "mt19937", {seed}, std::mt19937(seed), small);
        add_rng(sets.full, "mt19937", {seed}, std::mt19937(seed), big);
        add_rng(sets.committed, "mt19937_64", {seed}, std::mt19937_64(seed), small);
        add_rng(sets.full, "mt19937_64", {seed}, std::mt19937_64(seed), big);
    }
    add_rng(sets.committed, "mt19937", {}, std::mt19937(), small);
    add_rng(sets.full, "mt19937", {}, std::mt19937(), big);
    add_rng(sets.committed, "mt19937_64", {}, std::mt19937_64(), small);
    add_rng(sets.full, "mt19937_64", {}, std::mt19937_64(), big);
    {
        // 10000 draws in, the classic checks of both engines.
        std::vector<DrawPlan> plan = {{DISCARD, 9999, 0}, {NEXT, 0, 0}};
        add_rng(sets.committed, "mt19937", {}, std::mt19937(), plan);
        add_rng(sets.full, "mt19937", {}, std::mt19937(), plan);
        add_rng(sets.committed, "mt19937_64", {}, std::mt19937_64(), plan);
        add_rng(sets.full, "mt19937_64", {}, std::mt19937_64(), plan);
    }
    for (const std::vector<uint32_t> &words : {std::vector<uint32_t>{1, 2, 3}, std::vector<uint32_t>{0x12345678u, 0x9abcdef0u}, std::vector<uint32_t>{7, 8, 9, 10, 11, 12, 13, 14}}) {
        std::seed_seq         s1(words.begin(), words.end()), s2(words.begin(), words.end());
        std::vector<uint64_t> seed(words.begin(), words.end());
        seed.insert(seed.begin(), uint64_t(words.size()));
        // A seed_seq: the first word is the count, so a one-word seed_seq is not a plain seed.
        add_rng(sets.committed, "mt19937-seed_seq", seed, std::mt19937(s1), small);
        add_rng(sets.full, "mt19937-seed_seq", seed, std::mt19937(s1), big);
        std::seed_seq s3(words.begin(), words.end()), s4(words.begin(), words.end());
        add_rng(sets.committed, "mt19937_64-seed_seq", seed, std::mt19937_64(s3), small);
        add_rng(sets.full, "mt19937_64-seed_seq", seed, std::mt19937_64(s4), big);
        (void) s2;
        std::seed_seq         gen(words.begin(), words.end());
        std::vector<uint32_t> out(10);
        gen.generate(out.begin(), out.end());
        Payload p;
        p.u32s(words).u32s(out);
        sets.committed.add("SSEQ", p);
        sets.full.add("SSEQ", p);
    }
    for (int n : {0, 1, 2, 3, 10, 100, 1000, 5000}) {
        for (int which = 0; which < 2; ++which) {
            std::vector<int32_t> v(static_cast<size_t>(n));
            for (int i = 0; i < n; ++i)
                v[size_t(i)] = i;
            uint64_t next = 0;
            if (which == 0) {
                std::mt19937 g(uint32_t(n) + 1);
                std::shuffle(v.begin(), v.end(), g);
                next = g();
            } else {
                std::mt19937_64 g(uint64_t(n) + 1);
                std::shuffle(v.begin(), v.end(), g);
                next = g();
            }
            Payload p;
            p.str(which == 0 ? "mt19937" : "mt19937_64").u64(uint64_t(n) + 1).u32(uint32_t(n)).i32s(v).u64(next);
            sets.full.add("SHUF", p);
            if (n <= 100)
                sets.committed.add("SHUF", p);
        }
    }
}

} // namespace

void record_stl(Sets &sets)
{
    head(sets, "stl");
    sorts_into(sets.committed, {{0, 1, 2, 3, 5, 8, 16, 31, 32, 33, 40, 64, 65, 129}, {1, 16, 33}, false});
    sorts_into(sets.full, {{0, 1, 2, 3, 4, 5, 8, 15, 16, 31, 32, 33, 34, 40, 41, 42, 47, 63, 64, 65, 100, 127, 128, 129, 200, 255, 256, 300, 511, 512, 513, 1000, 1025},
                           {1, 2, 3, 5, 16, 31, 32, 33, 100},
                           true});
    record_values(sets);
    record_hashes(sets);
    record_unordered(sets, {0, 1, 5, 9, 64, 65, 100}, {0, 1, 5, 8, 9, 10, 63, 64, 65, 100, 511, 512, 513, 1000, 4097});
    record_random(sets);
}

namespace {

bool run_stl(const Options &options, std::string &error)
{
    Sets sets;
    record_stl(sets);
    return save(sets, options, "stl.sop1", error);
}

} // namespace

const Target &target()
{
    static const Target t{"stl", false, run_stl};
    return t;
}

} // namespace probe
