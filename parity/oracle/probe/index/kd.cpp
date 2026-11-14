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
// index.sop1's KDTreeIndirect records (parity/sop1/index.md), which orca-index replays for the piece
// index-1:
//
//   IXKD  KDTreeIndirect<2, double> (as ShortestPath.cpp builds it over the end points of paths) or
//         <3, double>, built from a count or from a list of indices: the order visit() reaches
//         the nodes in when the visitor always goes on (each node's index and dimension), then for
//         each point find_closest_point and find_closest_points<3>, with or without a filter,
//         find_nearby_points within a distance and find_nearby_points in a box.
//
// The points: none, one, two, three, all equal (ties are broken in the order the tree visits
// them), collinear, a grid (many points at the same distance), end points in pairs as
// ShortestPath has them, scaled coordinates and seeded sets. Each record holds its inputs and then
// the engine's results. Recorded by orca-o3-probe-index, 2026-11-13.
#include "index.hpp"

#include <array>
#include <cmath>
#include <cstdint>
#include <functional>
#include <utility>
#include <vector>

#include "libslic3r/KDTreeIndirect.hpp"

namespace probe {

namespace {

using namespace Slic3r;
using namespace probe::ix;

template<size_t D> using VecD = Eigen::Matrix<double, int(D), 1, Eigen::DontAlign>;

template<size_t D> struct KdQuery
{
    VecD<D>  pt;
    uint64_t mod, rem; // the filter keeps idx when idx % mod != rem; mod 0: no filter
    double   radius;
    VecD<D>  lo, hi;
};

template<size_t D> void put_vec(Payload &p, const VecD<D> &v)
{
    for (size_t i = 0; i < D; ++i)
        p.f64(v(int(i)));
}

template<size_t D> void ixkd(Payload &p, const std::vector<VecD<D>> &pts, bool from_indices, const std::vector<size_t> &indices, const std::vector<KdQuery<D>> &queries)
{
    auto coord = [&pts](size_t idx, size_t dim) -> double { return pts[idx](int(dim)); };
    using Tree = KDTreeIndirect<D, double, decltype(coord)>;
    p.u8(uint8_t(D));
    p.u64(pts.size());
    for (const VecD<D> &v : pts)
        put_vec<D>(p, v);
    p.boolean(from_indices);
    if (from_indices)
        put_idx(p, indices);
    std::vector<size_t> ind = indices;
    const Tree          tree = from_indices ? Tree(coord, ind) : Tree(coord, pts.size());

    struct All
    {
        std::vector<std::pair<size_t, size_t>> seen;
        unsigned int operator()(size_t idx, size_t dim)
        {
            seen.emplace_back(idx, dim);
            return (unsigned int) (VisitorReturnMask::CONTINUE_LEFT) | (unsigned int) (VisitorReturnMask::CONTINUE_RIGHT);
        }
    } all;
    tree.visit(all);
    p.u64(all.seen.size());
    for (const auto &s : all.seen)
        p.u64(s.first).u64(s.second);

    p.u64(queries.size());
    for (const KdQuery<D> &q : queries) {
        put_vec<D>(p, q.pt);
        p.u64(q.mod).u64(q.rem).f64(q.radius);
        put_vec<D>(p, q.lo);
        put_vec<D>(p, q.hi);
        const uint64_t mod = q.mod, rem = q.rem;
        auto           filter = [mod, rem](size_t idx) { return mod == 0 || idx % mod != rem; };
        if (q.mod == 0) {
            p.u64(find_closest_point(tree, q.pt));
            const std::array<size_t, 3> c3 = find_closest_points<3>(tree, q.pt);
            p.u64(c3[0]).u64(c3[1]).u64(c3[2]);
            put_idx(p, find_nearby_points(tree, q.pt, q.radius));
        } else {
            p.u64(find_closest_point(tree, q.pt, filter));
            const std::array<size_t, 3> c3 = find_closest_points<3>(tree, q.pt, filter);
            p.u64(c3[0]).u64(c3[1]).u64(c3[2]);
            put_idx(p, find_nearby_points(tree, q.pt, q.radius, filter));
        }
        put_idx(p, find_nearby_points(tree, q.lo, q.hi, filter));
    }
}

template<size_t D> VecD<D> rvec(SplitMix &g, double lo, double hi, bool grid)
{
    VecD<D> v;
    for (size_t i = 0; i < D; ++i)
        v(int(i)) = grid ? double(rnd(g, int64_t(lo), int64_t(hi))) : real(g, lo, hi);
    return v;
}

// Queries: at the points (distance 0), beside them, between them, far off and seeded; filters
// that keep everything, drop every second or third index, or drop all (mod 1); boxes around the
// point.
template<size_t D> std::vector<KdQuery<D>> kd_queries(SplitMix &g, const std::vector<VecD<D>> &pts, double lo, double hi, bool grid, size_t n)
{
    std::vector<KdQuery<D>> out;
    const double            span = hi - lo;
    for (size_t i = 0; i < n; ++i) {
        KdQuery<D> q;
        const int64_t k = pts.empty() ? 3 : rnd(g, 0, 4);
        if (k == 0)
            q.pt = pts[size_t(g.below(pts.size()))];
        else if (k == 1)
            q.pt = pts[size_t(g.below(pts.size()))] + rvec<D>(g, -span * 0.01, span * 0.01, false);
        else if (k == 2)
            q.pt = 0.5 * (pts[size_t(g.below(pts.size()))] + pts[size_t(g.below(pts.size()))]);
        else if (k == 3)
            q.pt = rvec<D>(g, lo - span, hi + span, grid);
        else
            q.pt = rvec<D>(g, lo, hi, grid);
        const int64_t f = rnd(g, 0, 5);
        q.mod           = f <= 2 ? 0 : uint64_t(f - 1);
        q.rem           = q.mod == 0 ? 0 : g.below(q.mod);
        q.radius        = rnd(g, 0, 5) == 0 ? 0. : real(g, 0, span / 2);
        const VecD<D> h = rvec<D>(g, 0, span / 3, false);
        q.lo            = q.pt - h;
        q.hi            = q.pt + h;
        out.push_back(q);
    }
    return out;
}

template<size_t D> void kd_case(std::vector<std::function<void(Payload &)>> &e, const std::vector<VecD<D>> &pts, bool from_indices, const std::vector<size_t> &indices, uint64_t seed, double lo, double hi, bool grid)
{
    e.push_back([pts, from_indices, indices, seed, lo, hi, grid](Payload &p) {
        SplitMix g(seed);
        ixkd<D>(p, pts, from_indices, indices, kd_queries<D>(g, pts, lo, hi, grid, 14));
    });
}

} // namespace

void record_index_kd(Sets &sets)
{
    using Fn  = std::function<void(Payload &)>;
    using V2  = VecD<2>;
    using V3  = VecD<3>;
    std::vector<Fn> e;
    kd_case<2>(e, {}, false, {}, 0x1B0D0001u, -10, 10, true);
    kd_case<2>(e, {V2(1, 2)}, false, {}, 0x1B0D0002u, -10, 10, true);
    kd_case<2>(e, {V2(1, 2), V2(3, -1)}, false, {}, 0x1B0D0003u, -10, 10, true);
    kd_case<2>(e, {V2(1, 2), V2(3, -1), V2(-2, 0)}, false, {}, 0x1B0D0004u, -10, 10, true);
    kd_case<2>(e, std::vector<V2>(9, V2(5, 5)), false, {}, 0x1B0D0005u, 0, 10, true);
    {
        std::vector<V2> line;
        for (int i = 0; i < 12; ++i)
            line.emplace_back(double(i), 0.);
        kd_case<2>(e, line, false, {}, 0x1B0D0006u, -2, 14, true);
        std::vector<V2> grid;
        for (int y = 0; y < 5; ++y)
            for (int x = 0; x < 5; ++x)
                grid.emplace_back(double(x), double(y));
        kd_case<2>(e, grid, false, {}, 0x1B0D0007u, -1, 5, true);
        kd_case<2>(e, grid, true, {0, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 22, 24}, 0x1B0D0008u, -1, 5, true);
        kd_case<2>(e, grid, true, {24, 3, 17, 3}, 0x1B0D0009u, -1, 5, true);
    }
    {
        // End points in pairs, as ShortestPath.cpp's chaining has them: a path's start and end.
        std::vector<V2> ends;
        SplitMix        g(0x1B0D000Au);
        for (int i = 0; i < 10; ++i) {
            const V2 a(real(g, -50, 50), real(g, -50, 50));
            ends.push_back(a);
            ends.push_back(a + V2(real(g, -5, 5), real(g, -5, 5)));
        }
        ends[7] = ends[4];
        kd_case<2>(e, ends, false, {}, 0x1B0D000Bu, -60, 60, false);
    }
    kd_case<2>(e, {V2(-1.2e8, 3.4e7), V2(1.25e8, -1.25e8), V2(0, 0), V2(5e7, 5e7), V2(5e7, 5e7 + 1)}, false, {}, 0x1B0D000Cu, -1.3e8, 1.3e8, false);
    kd_case<3>(e, {V3(0, 0, 0), V3(1, 0, 0), V3(0, 1, 0), V3(0, 0, 1), V3(1, 1, 1), V3(0, 0, 0)}, false, {}, 0x1B0D000Du, -1, 2, true);

    SplitMix        g(0x1B0D1000u);
    std::vector<Fn> random;
    for (int i = 0; i < 400; ++i) {
        const bool   grid = rnd(g, 0, 2) == 0;
        const double r    = grid ? double(rnd(g, 2, 12)) : std::pow(10., double(rnd(g, 0, 8)));
        const size_t n    = size_t(rnd(g, 0, 64));
        if (i % 5 == 4) {
            std::vector<V3> pts;
            for (size_t j = 0; j < n; ++j)
                pts.push_back(rvec<3>(g, -r, r, grid));
            kd_case<3>(random, pts, false, {}, g.next(), -r, r, grid);
            continue;
        }
        std::vector<V2> pts;
        for (size_t j = 0; j < n; ++j)
            pts.push_back(!pts.empty() && rnd(g, 0, 6) == 0 ? pts[size_t(g.below(pts.size()))] : rvec<2>(g, -r, r, grid));
        std::vector<size_t> ind;
        const bool          from_indices = n > 0 && rnd(g, 0, 4) == 0;
        if (from_indices)
            for (size_t j = 0; j < n; ++j)
                if (rnd(g, 0, 2) != 0)
                    ind.push_back(j);
        kd_case<2>(random, pts, from_indices, ind, g.next(), -r, r, grid);
    }
    size_t next = 0;
    record_cases(sets, "IXKD", e, random.size(), 14, 3072, [&](Payload &p) { random[next++](p); });
}

} // namespace probe
