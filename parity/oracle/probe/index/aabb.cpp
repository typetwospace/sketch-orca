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
// index.sop1's AABBTreeIndirect records (parity/sop1/index.md), which orca-index replays for the
// piece index-1:
//
//   IXBX  Tree<2, coord_t> built over BoundingBoxWrapper boxes with build_modify_input, as
//         Algorithm/RegionExpansion.cpp builds it: the nodes, then traverse() with a "contains
//         the point" predicate (RegionExpansion's), intersecting() and within(), each stopping
//         after a given number of leaves, and get_candidate_idxs.
//   IXTM  build_aabb_tree_over_indexed_triangle_set over a float mesh (Tree3f, as AABBMesh.cpp and
//         TriangleSelector.cpp build it) and over the same mesh in doubles (Tree3d): the nodes,
//         then rays (intersect_ray_first_hit and
//         intersect_ray_all_hits), points (squared_distance_to_indexed_triangle_set,
//         is_any_triangle_in_radius, all_triangles_in_radius, get_candidate_idxs) and boxes
//         (traverse with intersecting() and within()).
//
// The meshes: OrcaSlicer's test OBJ files (from the oracle's source tree, <exe>/../src/tests/data,
// read with the engine's load_obj), meshes made here (an empty one, one triangle, coplanar squares
// whose diagonal a ray crosses, duplicated faces, and the sort cases: a stack of squares and a book
// of triangles on which a ray finds 34 hits with ties, so intersect_ray_all_hits' std::sort runs
// MSVC's introsort) and seeded triangle soups. Each record holds its
// inputs and then the engine's results, so the replay needs no generator. Recorded by
// orca-o3-probe-index, 2026-11-13.
#include "index.hpp"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <filesystem>
#include <functional>
#include <limits>
#include <string>
#include <type_traits>
#include <vector>

#include "libslic3r/AABBTreeIndirect.hpp"
#include "libslic3r/BoundingBox.hpp"
#include "libslic3r/Format/OBJ.hpp"
#include "libslic3r/TriangleMesh.hpp"

namespace probe {

namespace {

using namespace Slic3r;
using namespace probe::ix;
namespace AT = Slic3r::AABBTreeIndirect;

using TreeBB = AT::Tree<2, coord_t>;
using BoxBB  = Eigen::AlignedBox<coord_t, 2>;
using Faces  = std::vector<stl_triangle_vertex_indices>;

// traverse() with a predicate; the callback keeps each leaf's idx and stops the walk after `stop`
// leaves (0: never).
template<class Tree, class Pred> std::vector<size_t> walk(const Tree &tree, Pred pred, uint64_t stop)
{
    std::vector<size_t> out;
    AT::traverse(tree, pred, [&out, stop](const typename Tree::Node &node) {
        out.push_back(node.idx);
        return stop == 0 || out.size() < stop;
    });
    return out;
}

// ---------------------------------------------------------------- IXBX

struct BxQuery
{
    uint8_t  kind; // 0 contains a, 1 intersecting(a, b), 2 within(a, b), 3 get_candidate_idxs(a)
    Point    a, b;
    uint64_t stop;
};

void ixbx(Payload &p, const std::vector<BoundingBox> &boxes, const std::vector<BxQuery> &queries)
{
    p.f64(SCALING_FACTOR);
    {
        AT::BoundingBoxWrapper w(0, BoundingBox(Point(0, 0), Point(0, 0)));
        p.i64(w.bbox().max().x());
    }
    p.u64(boxes.size());
    for (const BoundingBox &b : boxes) {
        put(p, b.min);
        put(p, b.max);
    }
    std::vector<AT::BoundingBoxWrapper> input;
    input.reserve(boxes.size());
    for (size_t i = 0; i < boxes.size(); ++i)
        input.emplace_back(i, boxes[i]);
    TreeBB tree;
    tree.build_modify_input(input);
    put_nodes(p, tree);
    p.u64(queries.size());
    for (const BxQuery &q : queries) {
        p.u8(q.kind);
        put(p, q.a);
        put(p, q.b);
        p.u64(q.stop);
        std::vector<size_t> out;
        const Point         a = q.a;
        switch (q.kind) {
        case 0: out = walk(tree, [a](const TreeBB::Node &node) { return node.bbox.contains(a); }, q.stop); break;
        case 1: out = walk(tree, AT::intersecting(BoxBB(q.a, q.b)), q.stop); break;
        case 2: out = walk(tree, AT::within(BoxBB(q.a, q.b)), q.stop); break;
        default: AT::get_candidate_idxs(tree, a, out); break;
        }
        put_idx(p, out);
    }
}

// Queries over boxes: points inside, on and off them, boxes around them, each kind with the stops
// 0, 1 and 2 for the walks.
std::vector<BxQuery> bx_queries(SplitMix &g, const std::vector<BoundingBox> &boxes, int64_t lo, int64_t hi, size_t count)
{
    std::vector<BxQuery> q;
    auto                 pick = [&]() -> Point {
        const int64_t k = rnd(g, 0, 5);
        if (!boxes.empty() && k < 4) {
            const BoundingBox &b = boxes[size_t(g.below(boxes.size()))];
            switch (k) {
            case 0: return b.min;
            case 1: return b.max;
            case 2: return Point((b.min.x() + b.max.x()) / 2, (b.min.y() + b.max.y()) / 2);
            default: return Point(b.min.x() - rnd(g, 0, 120), b.max.y() + rnd(g, 0, 120));
            }
        }
        return Point(rnd(g, lo, hi), rnd(g, lo, hi));
    };
    for (size_t i = 0; i < count; ++i) {
        const uint8_t kind = uint8_t(i % 4);
        Point         a = pick(), b = pick();
        if (kind == 1 || kind == 2) {
            if (rnd(g, 0, 4) != 0) {
                const Point mn(std::min(a.x(), b.x()), std::min(a.y(), b.y())), mx(std::max(a.x(), b.x()), std::max(a.y(), b.y()));
                a = mn;
                b = mx;
            }
        } else
            b = Point(0, 0);
        q.push_back(BxQuery{kind, a, b, uint64_t(kind == 3 ? 0 : (i / 4) % 3)});
    }
    return q;
}

void record_ixbx(Sets &sets)
{
    using Fn = std::function<void(Payload &)>;
    std::vector<Fn> e;
    auto            add = [&](std::vector<BoundingBox> boxes, uint64_t seed, int64_t lo, int64_t hi) {
        e.push_back([boxes, seed, lo, hi](Payload &p) {
            SplitMix g(seed);
            ixbx(p, boxes, bx_queries(g, boxes, lo, hi, 16));
        });
    };
    auto box = [](int64_t x0, int64_t y0, int64_t x1, int64_t y1) { return BoundingBox(Point(x0, y0), Point(x1, y1)); };
    // No box; one; the sizes around powers of two; equal boxes (ties in the split); a grid; nested
    // boxes; points as boxes; boxes of slices in scaled coordinates; negative coordinates.
    add({}, 0x1B0A0001u, -1000, 1000);
    add({box(0, 0, 1000, 1000)}, 0x1B0A0002u, -500, 1500);
    add({box(0, 0, 10, 10), box(20, 0, 30, 10)}, 0x1B0A0003u, -200, 300);
    add({box(0, 0, 10, 10), box(20, 0, 30, 10), box(0, 20, 10, 30)}, 0x1B0A0004u, -200, 300);
    add({box(0, 0, 10, 10), box(20, 0, 30, 10), box(0, 20, 10, 30), box(20, 20, 30, 30)}, 0x1B0A0005u, -200, 300);
    add({box(0, 0, 10, 10), box(20, 0, 30, 10), box(0, 20, 10, 30), box(20, 20, 30, 30), box(40, 40, 50, 50)}, 0x1B0A0006u, -200, 300);
    add({box(5, 5, 6, 6), box(5, 5, 6, 6), box(5, 5, 6, 6), box(5, 5, 6, 6), box(5, 5, 6, 6), box(5, 5, 6, 6), box(5, 5, 6, 6)}, 0x1B0A0007u, -200, 300);
    {
        std::vector<BoundingBox> grid;
        for (int64_t y = 0; y < 4; ++y)
            for (int64_t x = 0; x < 5; ++x)
                grid.push_back(box(x * 1000, y * 1000, x * 1000 + 900, y * 1000 + 900));
        add(grid, 0x1B0A0008u, -500, 5500);
    }
    add({box(0, 0, 100000, 100000), box(10000, 10000, 90000, 90000), box(20000, 20000, 80000, 80000), box(30000, 30000, 70000, 70000), box(40000, 40000, 60000, 60000)},
        0x1B0A0009u, -1000, 101000);
    add({box(0, 0, 0, 0), box(7, 7, 7, 7), box(-7, 3, -7, 3), box(0, 0, 0, 0)}, 0x1B0A000Au, -300, 300);
    add({box(-125000000, -125000000, -5000000, 4000000), box(10000000, -20000000, 125000000, 125000000), box(-3000000, -3000000, 3000000, 3000000)}, 0x1B0A000Bu,
        -130000000, 130000000);
    add({box(-30, -40, -10, -20), box(-100, 50, -60, 90), box(15, -15, 25, -5)}, 0x1B0A000Cu, -150, 150);

    SplitMix g(0x1B0A1000u);
    record_cases(sets, "IXBX", e, 400, 24, 2048, [&](Payload &p) {
        const int64_t            r = int64_t(1) << rnd(g, 4, 28);
        const size_t             n = size_t(rnd(g, 0, 40));
        std::vector<BoundingBox> boxes;
        for (size_t i = 0; i < n; ++i) {
            if (!boxes.empty() && rnd(g, 0, 7) == 0) {
                boxes.push_back(boxes[size_t(g.below(boxes.size()))]);
                continue;
            }
            const int64_t x = rnd(g, -r, r), y = rnd(g, -r, r);
            boxes.push_back(box(x, y, x + rnd(g, 0, r / 4), y + rnd(g, 0, r / 4)));
        }
        ixbx(p, boxes, bx_queries(g, boxes, -r - r / 4, r + r / 2, 12));
    });
}

// ---------------------------------------------------------------- IXTM

struct Ray
{
    Vec3d  o, d;
    double eps;
};
struct Probe
{
    Vec3d  pt;
    double r2;
};
struct Box3
{
    Vec3d    lo, hi;
    uint64_t stop;
};

void put_hit(Payload &p, const igl::Hit<float> &h) { p.i32(h.id).i32(h.gid).f32(h.u).f32(h.v).f32(h.t); }

template<class VertexType>
void ixtm(Payload &p, const std::string &label, const std::vector<VertexType> &vertices, const Faces &faces, typename VertexType::Scalar eps, const std::vector<Ray> &rays,
          const std::vector<Probe> &points, const std::vector<Box3> &boxes)
{
    using S    = typename VertexType::Scalar;
    using Tree = AT::Tree<3, S>;
    using Vec  = typename Tree::VectorType;
    p.u8(std::is_same<S, float>::value ? 0 : 1);
    p.str(label);
    p.u64(vertices.size());
    for (const VertexType &v : vertices)
        for (int i = 0; i < 3; ++i)
            put_scalar(p, v(i));
    p.u64(faces.size());
    for (const auto &f : faces)
        p.i32(f(0)).i32(f(1)).i32(f(2));
    put_scalar(p, eps);
    const Tree tree = AT::build_aabb_tree_over_indexed_triangle_set(vertices, faces, eps);
    // detail::intersect_triangle_epsilon is not recorded: its body calls cwiseMax() with no
    // argument, so it does not compile once instantiated, and nothing in the engine calls it.
    put_nodes(p, tree);

    p.u64(rays.size());
    for (const Ray &r : rays) {
        for (int i = 0; i < 3; ++i)
            p.f64(r.o(i));
        for (int i = 0; i < 3; ++i)
            p.f64(r.d(i));
        p.f64(r.eps);
        igl::Hit<float> hit{-7, -7, 0.f, 0.f, 0.f};
        const bool      first = AT::intersect_ray_first_hit(vertices, faces, tree, r.o, r.d, hit, r.eps);
        p.boolean(first);
        if (first)
            put_hit(p, hit);
        std::vector<igl::Hit<float>> hits;
        const bool                   any = AT::intersect_ray_all_hits(vertices, faces, tree, r.o, r.d, hits, r.eps);
        p.boolean(any);
        p.u64(hits.size());
        for (const igl::Hit<float> &h : hits)
            put_hit(p, h);
    }

    p.u64(points.size());
    for (const Probe &q : points) {
        for (int i = 0; i < 3; ++i)
            p.f64(q.pt(i));
        p.f64(q.r2);
        size_t       idx = NPOS;
        Vec3d        c   = Vec3d::Zero();
        const double d   = AT::squared_distance_to_indexed_triangle_set(vertices, faces, tree, q.pt, idx, c);
        p.f64(d).u64(idx).f64(c.x()).f64(c.y()).f64(c.z());
        double r2 = q.r2;
        p.boolean(AT::is_any_triangle_in_radius(vertices, faces, tree, q.pt, r2));
        put_idx(p, AT::all_triangles_in_radius(vertices, faces, tree, q.pt, q.r2));
        std::vector<size_t> cand;
        const Vec           v = q.pt.template cast<S>();
        AT::get_candidate_idxs(tree, v, cand);
        put_idx(p, cand);
    }

    p.u64(boxes.size());
    for (const Box3 &b : boxes) {
        const Vec lo = b.lo.template cast<S>(), hi = b.hi.template cast<S>();
        for (int i = 0; i < 3; ++i)
            put_scalar(p, lo(i));
        for (int i = 0; i < 3; ++i)
            put_scalar(p, hi(i));
        p.u64(b.stop);
        const Eigen::AlignedBox<S, 3> box(lo, hi);
        put_idx(p, walk(tree, AT::intersecting(box), b.stop));
        put_idx(p, walk(tree, AT::within(box), b.stop));
    }
}

// The queries of one mesh, from its vertices and faces: rays along the axes, down onto vertices,
// at edge midpoints and face centroids, from the centre, a zero direction and seeded ones (with
// the default epsilon, AABBMesh's epsilon and intersect_triangle_epsilon's); points at the centre,
// vertices, edge midpoints, centroids, far away and seeded; boxes inside and across the mesh.
struct Queries
{
    std::vector<Ray>   rays;
    std::vector<Probe> points;
    std::vector<Box3>  boxes;
};

Queries mesh_queries(SplitMix &g, const indexed_triangle_set &its, size_t nrays, size_t npoints, size_t nboxes)
{
    Queries q;
    Vec3d   lo(-1, -1, -1), hi(1, 1, 1);
    if (!its.vertices.empty()) {
        lo = hi = its.vertices.front().cast<double>();
        for (const stl_vertex &v : its.vertices) {
            lo = lo.cwiseMin(v.cast<double>());
            hi = hi.cwiseMax(v.cast<double>());
        }
    }
    const Vec3d  c = 0.5 * (lo + hi);
    const double s = std::max(1.0, (hi - lo).maxCoeff());
    const double l = its.indices.empty() ? 0. : double(its_average_edge_length(its));
    auto         vtx = [&](size_t f, int k) { return its.vertices[size_t(its.indices[f](k))].cast<double>(); };
    auto         eps = [&](size_t i) { return i % 3 == 0 ? 0.000001 : (i % 3 == 1 ? 0.000001 * l * l : 1e-9); };
    auto         rdir = [&]() {
        Vec3d d(real(g, -1, 1), real(g, -1, 1), real(g, -1, 1));
        if (rnd(g, 0, 4) == 0)
            d(size_t(g.below(3))) = 0.;
        return d;
    };
    for (int a = 0; a < 3; ++a)
        for (int sgn = -1; sgn <= 1; sgn += 2) {
            Vec3d o = c, d = Vec3d::Zero();
            o(a) -= sgn * 2 * s;
            d(a) = sgn;
            q.rays.push_back(Ray{o, d, eps(q.rays.size())});
        }
    q.rays.push_back(Ray{c, Vec3d::Zero(), 0.000001});
    const size_t nf = its.indices.size();
    while (q.rays.size() < nrays) {
        const size_t i = q.rays.size();
        if (nf == 0) {
            q.rays.push_back(Ray{Vec3d(real(g, -2, 2), real(g, -2, 2), real(g, -2, 2)), rdir(), eps(i)});
            continue;
        }
        const size_t f = size_t(g.below(nf));
        switch (i % 5) {
        case 0: q.rays.push_back(Ray{vtx(f, 0) + Vec3d(0, 0, s + 1), Vec3d(0, 0, -1), eps(i)}); break;
        case 1: {
            const Vec3d m = 0.5 * (vtx(f, 0) + vtx(f, 1));
            const Vec3d d = rdir();
            q.rays.push_back(Ray{m - 2 * s * d, d, eps(i)});
            break;
        }
        case 2: {
            const Vec3d m = (vtx(f, 0) + vtx(f, 1) + vtx(f, 2)) / 3.;
            q.rays.push_back(Ray{c, m - c, eps(i)});
            break;
        }
        case 3: q.rays.push_back(Ray{c + Vec3d(real(g, -s, s), real(g, -s, s), real(g, -s, s)), rdir(), eps(i)}); break;
        default: q.rays.push_back(Ray{lo - Vec3d(1, 1, 1), hi - lo + Vec3d(2, 2, 2), eps(i)}); break;
        }
    }
    auto r2 = [&]() { return rnd(g, 0, 6) == 0 ? 0. : std::pow(real(g, 0, s / 2), 2); };
    q.points.push_back(Probe{c, r2()});
    q.points.push_back(Probe{c + Vec3d(3 * s, -2 * s, s), r2()});
    while (q.points.size() < npoints) {
        const size_t i = q.points.size();
        if (nf == 0) {
            q.points.push_back(Probe{Vec3d(real(g, -2, 2), real(g, -2, 2), real(g, -2, 2)), r2()});
            continue;
        }
        const size_t f = size_t(g.below(nf));
        switch (i % 4) {
        case 0: q.points.push_back(Probe{vtx(f, int(g.below(3))), r2()}); break;
        case 1: q.points.push_back(Probe{0.5 * (vtx(f, 1) + vtx(f, 2)), r2()}); break;
        case 2: q.points.push_back(Probe{(vtx(f, 0) + vtx(f, 1) + vtx(f, 2)) / 3., r2()}); break;
        default: q.points.push_back(Probe{c + Vec3d(real(g, -s, s), real(g, -s, s), real(g, -s, s)), r2()}); break;
        }
    }
    while (q.boxes.size() < nboxes) {
        Vec3d a(real(g, lo.x() - 1, hi.x() + 1), real(g, lo.y() - 1, hi.y() + 1), real(g, lo.z() - 1, hi.z() + 1));
        Vec3d b(real(g, lo.x() - 1, hi.x() + 1), real(g, lo.y() - 1, hi.y() + 1), real(g, lo.z() - 1, hi.z() + 1));
        if (q.boxes.empty()) {
            a = lo - Vec3d(1, 1, 1);
            b = hi + Vec3d(1, 1, 1);
        }
        q.boxes.push_back(Box3{a.cwiseMin(b), a.cwiseMax(b), uint64_t(q.boxes.size() % 3)});
    }
    return q;
}

std::vector<Vec3d> doubles(const indexed_triangle_set &its)
{
    std::vector<Vec3d> out;
    out.reserve(its.vertices.size());
    for (const stl_vertex &v : its.vertices)
        out.push_back(v.cast<double>());
    return out;
}

// One mesh's two records, Tree3f and Tree3d, with the same queries; `extra` rays go before the
// generated ones.
void mesh_records(std::vector<std::function<void(Payload &)>> &e, const std::string &label, const indexed_triangle_set &its, float eps, uint64_t seed, size_t nrays,
                  size_t npoints, size_t nboxes, const std::vector<Ray> &extra = {})
{
    SplitMix g(seed);
    Queries  q = mesh_queries(g, its, nrays, npoints, nboxes);
    q.rays.insert(q.rays.begin(), extra.begin(), extra.end());
    e.push_back([label, its, eps, q](Payload &p) { ixtm(p, label, its.vertices, its.indices, eps, q.rays, q.points, q.boxes); });
    e.push_back([label, its, eps, q](Payload &p) { ixtm(p, label, doubles(its), its.indices, double(eps), q.rays, q.points, q.boxes); });
}

// One mesh's two records with the given queries only and the epsilon 0: the float one into
// `floats`, the double one into `dbl`.
void exact_records(std::vector<std::function<void(Payload &)>> &floats, std::vector<std::function<void(Payload &)>> &dbl, const std::string &label,
                   const indexed_triangle_set &its, const Queries &q)
{
    floats.push_back([label, its, q](Payload &p) { ixtm(p, label, its.vertices, its.indices, 0.f, q.rays, q.points, q.boxes); });
    dbl.push_back([label, its, q](Payload &p) { ixtm(p, label, doubles(its), its.indices, 0., q.rays, q.points, q.boxes); });
}

indexed_triangle_set soup(std::vector<stl_vertex> vertices, Faces faces)
{
    indexed_triangle_set its;
    its.vertices = std::move(vertices);
    its.indices  = std::move(faces);
    return its;
}

// The sort cases' meshes. `stack`: 17 unit squares, two triangles each, at z = 0 to 16; a ray down
// (or up) their shared diagonal meets both triangles of each square at the same t, so
// intersect_ray_all_hits finds 34 hits in 17 tied pairs. `book`: 34 triangles fanned around the
// edge (-1, -1, 0) to (1, 1, 0), which they all share; a ray down through (0.25, 0.25) meets that
// edge, so all 34 hits have one t. The coordinates are small integers, each apex lies 1 or 2 off
// the edge's line (so every determinant is a power of two) and the rays start at binary fractions:
// each t is exact, and the ties are exact.
indexed_triangle_set stack_mesh()
{
    std::vector<stl_vertex> v;
    Faces                   f;
    for (int k = 0; k < 17; ++k) {
        const float z = float(k);
        const int   a = int(v.size());
        v.emplace_back(0.f, 0.f, z);
        v.emplace_back(1.f, 0.f, z);
        v.emplace_back(1.f, 1.f, z);
        v.emplace_back(0.f, 1.f, z);
        f.emplace_back(a, a + 1, a + 2);
        f.emplace_back(a, a + 2, a + 3);
    }
    return soup(v, f);
}

indexed_triangle_set book_mesh()
{
    std::vector<stl_vertex> v{stl_vertex(-1.f, -1.f, 0.f), stl_vertex(1.f, 1.f, 0.f)};
    Faces                   f;
    const float             apex[4][2] = {{0.f, 1.f}, {1.f, 0.f}, {-1.f, 1.f}, {1.f, -1.f}};
    for (int i = 0; i < 34; ++i) {
        v.emplace_back(apex[i % 4][0], apex[i % 4][1], float(i - 17));
        f.emplace_back(0, 1, int(v.size()) - 1);
    }
    return soup(v, f);
}

bool program_data_dir(std::filesystem::path &out, std::string &error)
{
    char *pgm = nullptr;
    if (_get_pgmptr(&pgm) != 0 || pgm == nullptr) {
        error = "cannot find the probe's own path";
        return false;
    }
    out = std::filesystem::path(pgm).parent_path().parent_path() / "src" / "tests" / "data";
    return true;
}

bool record_ixtm(Sets &sets, std::string &error)
{
    using Fn = std::function<void(Payload &)>;
    std::vector<Fn> e;
    // The sort cases (stack_mesh, book_mesh): more than 32 hits with ties, so the std::sort of
    // intersect_ray_all_hits runs MSVC's introsort, not only its insertion sort. Their float records
    // are pinned: they go into both sets whatever their size, first. Their double records and the
    // stack with generated queries are ordinary made-here cases.
    std::vector<Fn> pinned;
    const Ray       down{Vec3d(0.25, 0.25, 20.), Vec3d(0., 0., -1.), 0.000001};
    exact_records(pinned, e, "stack", stack_mesh(), Queries{{down}, {}, {}});
    exact_records(pinned, e, "book", book_mesh(), Queries{{down}, {}, {}});
    mesh_records(e, "stack, more queries", stack_mesh(), 0.f, 0x1B0B0007u, 14, 10, 3,
                 {Ray{Vec3d(0.5, 0.5, 20.), Vec3d(0., 0., -1.), 0.000001}, Ray{Vec3d(0.75, 0.75, -4.), Vec3d(0., 0., 1.), 0.000001}});
    const stl_triangle_vertex_indices f012(0, 1, 2);
    // Made here: an empty mesh, one triangle, a unit square of two triangles (a ray down its
    // diagonal hits both at the same t), a 3 x 3 grid of squares, a face given twice and a
    // degenerate face.
    mesh_records(e, "empty", soup({}, {}), 0.f, 0x1B0B0001u, 8, 4, 2);
    mesh_records(e, "triangle", soup({stl_vertex(0, 0, 0), stl_vertex(1, 0, 0), stl_vertex(0, 1, 0)}, {f012}), 0.f, 0x1B0B0002u, 12, 8, 3);
    mesh_records(e, "square",
                 soup({stl_vertex(0, 0, 0), stl_vertex(1, 0, 0), stl_vertex(1, 1, 0), stl_vertex(0, 1, 0)}, {f012, stl_triangle_vertex_indices(0, 2, 3)}), 0.f,
                 0x1B0B0003u, 12, 8, 3);
    {
        std::vector<stl_vertex> v;
        Faces                   f;
        for (int y = 0; y <= 3; ++y)
            for (int x = 0; x <= 3; ++x)
                v.emplace_back(float(x), float(y), 0.f);
        for (int y = 0; y < 3; ++y)
            for (int x = 0; x < 3; ++x) {
                const int a = y * 4 + x;
                f.emplace_back(a, a + 1, a + 5);
                f.emplace_back(a, a + 5, a + 4);
            }
        mesh_records(e, "grid", soup(v, f), 0.f, 0x1B0B0004u, 16, 10, 4);
        mesh_records(e, "grid eps", soup(v, f), 0.25f, 0x1B0B0005u, 10, 8, 3);
    }
    mesh_records(e, "twice",
                 soup({stl_vertex(0, 0, 0), stl_vertex(2, 0, 0), stl_vertex(0, 2, 1), stl_vertex(2, 2, 1)},
                      {f012, f012, stl_triangle_vertex_indices(1, 3, 2), stl_triangle_vertex_indices(1, 1, 3)}),
                 0.f, 0x1B0B0006u, 12, 8, 3);

    // OrcaSlicer's test meshes.
    std::filesystem::path data;
    if (!program_data_dir(data, error))
        return false;
    const char *files[] = {"20mm_cube",
                           "2x20x10",
                           "pyramid",
                           "small_dorito",
                           "V",
                           "V_standing",
                           "bridge",
                           "cube_with_hole",
                           "simplification",
                           "ipadstand",
                           "cube_with_concave_hole",
                           "two_hollow_squares",
                           "overhang",
                           "sloping_hole",
                           "cube_with_concave_hole_enlarged",
                           "A",
                           "extruder_idler"};
    uint64_t seed = 0x1B0B0100u;
    for (const char *name : files) {
        const std::filesystem::path file = data / (std::string(name) + ".obj");
        TriangleMesh                mesh;
        ObjInfo                     info;
        std::string                 message;
        if (!load_obj(file.string().c_str(), &mesh, info, message)) {
            error = "cannot read " + file.string() + ": " + message;
            return false;
        }
        mesh_records(e, std::string("orca:") + name + ".obj", mesh.its, 0.f, seed++, 14, 10, 3);
    }

    SplitMix        g(0x1B0B1000u);
    std::vector<Fn> soups;
    for (int i = 0; i < 60; ++i) {
        const size_t            n = size_t(rnd(g, 1, 24));
        std::vector<stl_vertex> v;
        Faces                   f;
        const bool              grid = rnd(g, 0, 2) == 0;
        for (size_t j = 0; j < 3 * n; ++j) {
            if (grid)
                v.emplace_back(float(rnd(g, -4, 4)), float(rnd(g, -4, 4)), float(rnd(g, -4, 4)));
            else
                v.emplace_back(float(real(g, -10, 10)), float(real(g, -10, 10)), float(real(g, -10, 10)));
        }
        for (size_t j = 0; j < n; ++j) {
            if (!f.empty() && rnd(g, 0, 7) == 0)
                f.push_back(f[size_t(g.below(f.size()))]);
            else
                f.emplace_back(int(g.below(v.size())), int(g.below(v.size())), int(g.below(v.size())));
        }
        const float eps = rnd(g, 0, 3) == 0 ? float(real(g, 0, 0.5)) : 0.f;
        mesh_records(soups, "soup", soup(v, f), eps, g.next(), 10, 8, 3);
    }
    // The soups are seeded cases: record_cases takes them as its random ones, in order.
    record_cases(sets, "IXTM", pinned, 0, 0, std::numeric_limits<size_t>::max(), [](Payload &) {});
    size_t next = 0;
    record_cases(sets, "IXTM", e, soups.size(), 8, 6144, [&](Payload &p) { soups[next++](p); });
    return true;
}

} // namespace

bool record_index_aabb(Sets &sets, std::string &error)
{
    record_ixbx(sets);
    return record_ixtm(sets, error);
}

} // namespace probe
