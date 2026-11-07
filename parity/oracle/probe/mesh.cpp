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
// mesh.sop1: admesh and TriangleMesh as the engine reads, repairs and transforms meshes
// (parity/SOP1.md, MCAS and MSTD), recorded by the o2-mesh task (2026-11-07); orca-mesh's
// tests/probes.rs replays it. Each case is an STL file, written to a temporary folder:
//
//   MCAS  stl_open, then the steps of trianglemesh_repair_on_import (TriangleMesh.cpp) one by one,
//         each with a hash of the stl_file after it, and stl_generate_shared_vertices; then
//         TriangleMesh::ReadSTLFile on the same file (the engine's own repair), the mesh's
//         statistics, measures, split, and a seeded chain of transforms, each with its result.
//   MSTD  (full set only, meshes of 24 facets or fewer) the bytes those hashes are taken of.
//
// The files: generated binary and ASCII STL files (closed boxes, rotated and scaled, with facets
// flipped, removed, duplicated, made degenerate or NaN, vertices nudged within the nearby
// tolerance, wrong or mangled normals, wrong sizes, every line end but a lone CR), and the STL files
// of OrcaSlicer's tests/data (from the oracle's source tree beside the probe's build folder) but
// 20mmbox-CR.stl, which overruns admesh's 256-byte buffer (parity/CORPUS.md). A file whose
// first line after "solid" is longer than 255 bytes would overrun it too, so none is generated.
// The tasks run under the oracle's threading: global_control(max_allowed_parallelism, 1) inside
// a task_arena (parity/oracle/README.md, SKETCH_ORCA_ORACLE_THREADS).
#include "probe.hpp"

#include <array>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <stdexcept>
#include <string>
#include <vector>

#include <process.h>

#include <oneapi/tbb/global_control.h>
#include <oneapi/tbb/task_arena.h>

#include <admesh/stl.h>
#include <libslic3r/BoundingBox.hpp>
#include <libslic3r/Point.hpp>
#include <libslic3r/Polygon.hpp>
#include <libslic3r/TriangleMesh.hpp>

namespace probe {

namespace {

using Slic3r::BoundingBoxf3;
using Slic3r::TriangleMesh;

// FNV-1a, 64 bits, of the bytes a snapshot is serialised to.
uint64_t fnv1a(const std::string &s)
{
    uint64_t h = 0xcbf29ce484222325ull;
    for (unsigned char c : s) {
        h ^= c;
        h *= 0x100000001b3ull;
    }
    return h;
}

void put_vec3f(Payload &p, const Slic3r::Vec3f &v)
{
    for (int i = 0; i < 3; ++i)
        p.f32(v(i));
}

// The stl_file: the statistics, then every facet and neighbour entry the arrays hold (a facet's
// two extra bytes only for a binary file: an ASCII facet leaves them unset).
std::string snapshot(const stl_file &stl)
{
    Payload          p;
    const stl_stats &s = stl.stats;
    p.str(std::string(s.header.data(), s.header.size()));
    p.u8(uint8_t(s.type)).u32(s.number_of_facets);
    put_vec3f(p, s.max);
    put_vec3f(p, s.min);
    put_vec3f(p, s.size);
    p.f32(s.bounding_diameter).f32(s.shortest_edge).f32(s.volume);
    for (int v : {s.connected_edges, s.connected_facets_1_edge, s.connected_facets_2_edge, s.connected_facets_3_edge, s.facets_w_1_bad_edge, s.facets_w_2_bad_edge,
                  s.facets_w_3_bad_edge, s.original_num_facets, s.edges_fixed, s.degenerate_facets, s.facets_removed, s.facets_added, s.facets_reversed,
                  s.backwards_edges, s.normals_fixed, s.number_of_parts})
        p.i32(v);
    p.u64(stl.facet_start.size());
    for (const stl_facet &f : stl.facet_start) {
        put_vec3f(p, f.normal);
        for (int k = 0; k < 3; ++k)
            put_vec3f(p, f.vertex[k]);
        if (s.type == binary)
            p.u8(uint8_t(f.extra[0])).u8(uint8_t(f.extra[1]));
    }
    p.u64(stl.neighbors_start.size());
    for (const stl_neighbors &n : stl.neighbors_start) {
        for (int k = 0; k < 3; ++k)
            p.i32(n.neighbor[k]);
        for (int k = 0; k < 3; ++k)
            p.i32(int32_t(n.which_vertex_not[k]));
    }
    return p.bytes;
}

std::string its_bytes(const indexed_triangle_set &its)
{
    Payload p;
    p.u64(its.vertices.size());
    for (const stl_vertex &v : its.vertices)
        put_vec3f(p, v);
    p.u64(its.indices.size());
    for (const stl_triangle_vertex_indices &f : its.indices)
        for (int k = 0; k < 3; ++k)
            p.i32(f(k));
    p.u64(its.properties.size());
    for (const FaceProperty &f : its.properties)
        p.u8(uint8_t(f.type)).f64(f.area);
    return p.bytes;
}

// A TriangleMesh: its statistics and a hash of its indexed triangle set.
void put_mesh(Payload &p, const TriangleMesh &m)
{
    const Slic3r::TriangleMeshStats &s = m.stats();
    p.u32(s.number_of_facets);
    put_vec3f(p, s.max);
    put_vec3f(p, s.min);
    put_vec3f(p, s.size);
    p.f32(s.volume).i32(s.number_of_parts).i32(s.open_edges);
    const Slic3r::RepairedMeshErrors &e = s.repaired_errors;
    p.i32(e.edges_fixed).i32(e.degenerate_facets).i32(e.facets_removed).i32(e.facets_reversed).i32(e.backwards_edges);
    p.u64(fnv1a(its_bytes(m.its)));
}

void put_bbox(Payload &p, const BoundingBoxf3 &b)
{
    p.boolean(b.defined);
    for (int i = 0; i < 3; ++i)
        p.f64(b.min(i));
    for (int i = 0; i < 3; ++i)
        p.f64(b.max(i));
}

void put_polygon(Payload &p, const Slic3r::Polygon &poly)
{
    p.u64(poly.points.size());
    for (const Slic3r::Point &pt : poly.points)
        p.i64(pt.x()).i64(pt.y());
}

// The steps of trianglemesh_repair_on_import, as MCAS numbers them.
enum Step : uint8_t {
    OPEN = 0,
    EXACT = 1,
    NEARBY_1 = 2,
    NEARBY_2 = 3,
    REMOVE_UNCONNECTED = 4,
    NORMAL_DIRECTIONS = 5,
    NORMAL_VALUES = 6,
    VOLUME = 7,
    VERIFY_NEIGHBORS = 8,
    EXACT_AGAIN = 9,
};

// trianglemesh_repair_on_import, a step at a time, calling `after` after each.
template<typename After> void repair_steps(stl_file &stl, After after)
{
    if (stl.stats.number_of_facets == 0)
        return;
    stl_check_facets_exact(&stl);
    stl.stats.facets_w_1_bad_edge = (stl.stats.connected_facets_2_edge - stl.stats.connected_facets_3_edge);
    stl.stats.facets_w_2_bad_edge = (stl.stats.connected_facets_1_edge - stl.stats.connected_facets_2_edge);
    stl.stats.facets_w_3_bad_edge = (stl.stats.number_of_facets - stl.stats.connected_facets_1_edge);
    after(EXACT);
    float tolerance  = (float)stl.stats.shortest_edge;
    float increment  = (float)stl.stats.bounding_diameter / 10000.0f;
    int   iterations = 2;
    if (stl.stats.connected_facets_3_edge < int(stl.stats.number_of_facets)) {
        for (int i = 0; i < iterations; ++i) {
            if (stl.stats.connected_facets_3_edge < int(stl.stats.number_of_facets)) {
                stl_check_facets_nearby(&stl, tolerance);
                tolerance += increment;
                after(i == 0 ? NEARBY_1 : NEARBY_2);
            } else
                break;
        }
    }
    if (stl.stats.connected_facets_3_edge < (int)stl.stats.number_of_facets) {
        stl_remove_unconnected_facets(&stl);
        after(REMOVE_UNCONNECTED);
    }
    stl_fix_normal_directions(&stl);
    after(NORMAL_DIRECTIONS);
    stl_fix_normal_values(&stl);
    after(NORMAL_VALUES);
    stl_calculate_volume(&stl);
    after(VOLUME);
    stl_verify_neighbors(&stl);
    after(VERIFY_NEIGHBORS);
    if (auto nr_degenerated = stl.stats.degenerate_facets; stl.stats.number_of_facets > 0 && nr_degenerated > 0) {
        stl_check_facets_exact(&stl);
        after(EXACT_AGAIN);
    }
}

double real(SplitMix &g, double a, double b) { return a + (b - a) * (double(g.next() >> 11) * 0x1.0p-53); }

struct Tri
{
    std::array<std::array<float, 3>, 3> v;
    std::array<float, 3>                n{};
    uint16_t                            attr = 0;
};

// The unit normal of (b - a) x (c - a) in double, zero when degenerate.
std::array<float, 3> normal_of(const Tri &t)
{
    double e1[3], e2[3], c[3];
    for (int i = 0; i < 3; ++i) {
        e1[i] = double(t.v[1][i]) - double(t.v[0][i]);
        e2[i] = double(t.v[2][i]) - double(t.v[0][i]);
    }
    c[0]            = e1[1] * e2[2] - e1[2] * e2[1];
    c[1]            = e1[2] * e2[0] - e1[0] * e2[2];
    c[2]            = e1[0] * e2[1] - e1[1] * e2[0];
    const double len = std::sqrt(c[0] * c[0] + c[1] * c[1] + c[2] * c[2]);
    if (!(len > 0.))
        return {0.f, 0.f, 0.f};
    return {float(c[0] / len), float(c[1] / len), float(c[2] / len)};
}

// A closed box of k x k quads a face, every face outward, the corner at o and the size s.
std::vector<Tri> box(int k, const double o[3], const double s[3])
{
    std::vector<Tri> out;
    for (int a = 0; a < 3; ++a)
        for (int side = 0; side < 2; ++side) {
            const int b = (a + 1) % 3, c = (a + 2) % 3;
            auto      pt = [&](int i, int j) {
                double q[3];
                q[a] = double(side);
                q[b] = double(i) / double(k);
                q[c] = double(j) / double(k);
                return std::array<float, 3>{float(o[0] + s[0] * q[0]), float(o[1] + s[1] * q[1]), float(o[2] + s[2] * q[2])};
            };
            for (int i = 0; i < k; ++i)
                for (int j = 0; j < k; ++j) {
                    const auto p00 = pt(i, j), p10 = pt(i + 1, j), p11 = pt(i + 1, j + 1), p01 = pt(i, j + 1);
                    Tri        t1, t2;
                    if (side == 1) {
                        t1.v = {p00, p10, p11};
                        t2.v = {p00, p11, p01};
                    } else {
                        t1.v = {p00, p11, p10};
                        t2.v = {p00, p01, p11};
                    }
                    out.push_back(t1);
                    out.push_back(t2);
                }
        }
    return out;
}

// Every vertex rotated about the origin by angles in double, then rounded: equal floats stay equal.
void rotate_all(std::vector<Tri> &tris, double ax, double ay, double az)
{
    const double ca = std::cos(ax), sa = std::sin(ax), cb = std::cos(ay), sb = std::sin(ay), cc = std::cos(az), sc = std::sin(az);
    for (Tri &t : tris)
        for (auto &v : t.v) {
            double x = v[0], y = v[1], z = v[2];
            double y1 = ca * y - sa * z, z1 = sa * y + ca * z;
            double x2 = cb * x + sb * z1, z2 = -sb * x + cb * z1;
            double x3 = cc * x2 - sc * y1, y3 = sc * x2 + cc * y1;
            v = {float(x3), float(y3), float(z2)};
        }
}

void set_normals(SplitMix &g, std::vector<Tri> &tris, int how)
{
    for (Tri &t : tris) {
        const int h = how >= 0 ? how : int(g.below(8));
        if (h <= 3)
            t.n = normal_of(t);
        else if (h == 4)
            t.n = {0.f, 0.f, 0.f};
        else if (h == 5) {
            const auto n = normal_of(t);
            t.n          = {-n[0], -n[1], -n[2]};
        } else if (h == 6)
            t.n = {float(real(g, -1, 1)), float(real(g, -1, 1)), float(real(g, -1, 1))};
        else
            t.n = {NAN, 0.f, 1.f};
    }
}

// Faults, each with its chance in 64 per mesh.
void damage(SplitMix &g, std::vector<Tri> &tris, bool nan_ok)
{
    if (tris.empty())
        return;
    if (g.below(64) < 16) { // flip some facets
        const int n = 1 + int(g.below(3));
        for (int i = 0; i < n; ++i) {
            Tri &t = tris[g.below(tris.size())];
            std::swap(t.v[1], t.v[2]);
        }
    }
    if (g.below(64) < 12 && tris.size() > 4) { // holes
        const int n = 1 + int(g.below(3));
        for (int i = 0; i < n && tris.size() > 4; ++i)
            tris.erase(tris.begin() + ptrdiff_t(g.below(tris.size())));
    }
    if (g.below(64) < 10) { // duplicates
        const int n = 1 + int(g.below(2));
        for (int i = 0; i < n; ++i)
            tris.push_back(tris[g.below(tris.size())]);
    }
    if (g.below(64) < 12) { // degenerate facets
        const int n = 1 + int(g.below(2));
        for (int i = 0; i < n; ++i) {
            Tri t = tris[g.below(tris.size())];
            if (g.below(2))
                t.v[2] = t.v[g.below(2)];
            else
                t.v[1] = t.v[0];
            tris.insert(tris.begin() + ptrdiff_t(g.below(tris.size() + 1)), t);
        }
    }
    if (g.below(64) < 16) { // vertices nudged by a few units in the last place
        const int n = 1 + int(g.below(4));
        for (int i = 0; i < n; ++i) {
            auto &v = tris[g.below(tris.size())].v[g.below(3)];
            float &c = v[g.below(3)];
            const int steps = 1 + int(g.below(8));
            for (int s = 0; s < steps; ++s)
                c = std::nextafter(c, g.below(2) ? INFINITY : -INFINITY);
        }
    }
    if (g.below(64) < 4) { // every facet flipped: a negative volume
        for (Tri &t : tris)
            std::swap(t.v[1], t.v[2]);
    }
    if (nan_ok && g.below(64) < 4)
        tris[g.below(tris.size())].v[g.below(3)][g.below(3)] = NAN;
    if (g.below(64) < 8) { // shuffled
        for (size_t i = tris.size(); i > 1; --i)
            std::swap(tris[i - 1], tris[g.below(i)]);
    }
}

std::string binary_stl(SplitMix &g, const std::vector<Tri> &tris, int header_kind, int64_t count_delta)
{
    std::string out(80, '\0');
    if (header_kind == 1)
        std::memcpy(&out[0], "Sketch", 6);
    else if (header_kind == 2)
        for (char &c : out)
            c = char('a' + g.below(26));
    const uint32_t count = uint32_t(int64_t(tris.size()) + count_delta);
    for (int i = 0; i < 4; ++i)
        out += char(uint8_t(count >> (8 * i)));
    auto put = [&](float f) {
        uint32_t b = bits(f);
        for (int i = 0; i < 4; ++i)
            out += char(uint8_t(b >> (8 * i)));
    };
    for (const Tri &t : tris) {
        for (float f : t.n)
            put(f);
        for (const auto &v : t.v)
            for (float f : v)
                put(f);
        out += char(uint8_t(t.attr));
        out += char(uint8_t(t.attr >> 8));
    }
    return out;
}

std::string ascii_number(SplitMix &g, float f, int style)
{
    char buf[64];
    if (std::isnan(f))
        return g.below(2) ? "nan" : "-nan(ind)";
    switch (style >= 0 ? style : int(g.below(4))) {
    case 0: std::snprintf(buf, sizeof(buf), "%.9g", double(f)); break;
    case 1: std::snprintf(buf, sizeof(buf), "%e", double(f)); break;
    case 2: std::snprintf(buf, sizeof(buf), "%f", double(f)); break;
    default: std::snprintf(buf, sizeof(buf), "%.9E", double(f)); break;
    }
    return buf;
}

// An ASCII STL; `le` the line end (0 LF, 1 CR LF, 2 CR CR LF, 3 LF CR, -1 any of them a line).
std::string ascii_stl(SplitMix &g, const std::vector<Tri> &tris, int style, int le, bool final_newline)
{
    static const char *const ends[] = {"\n", "\r\n", "\r\r\n", "\n\r"};
    auto                     eol    = [&]() { return std::string(ends[le >= 0 ? le : int(g.below(4))]); };
    auto                     sp     = [&]() { return std::string(g.below(8) ? " " : g.below(2) ? "\t" : "   "); };
    std::string              t      = "solid";
    if (g.below(3))
        t += " part" + std::to_string(g.below(100));
    t += eol();
    for (const Tri &tri : tris) {
        t += sp() + "facet normal " + ascii_number(g, tri.n[0], style) + " " + ascii_number(g, tri.n[1], style) + " " + ascii_number(g, tri.n[2], style) + eol();
        t += sp() + sp() + "outer loop" + eol();
        for (const auto &v : tri.v)
            t += sp() + sp() + sp() + "vertex " + ascii_number(g, v[0], style) + sp() + ascii_number(g, v[1], style) + sp() + ascii_number(g, v[2], style) + eol();
        t += sp() + sp() + "endloop" + eol();
        t += sp() + "endfacet" + eol();
    }
    t += "endsolid";
    if (final_newline)
        t += eol();
    return t;
}

struct Case
{
    std::string label;
    std::string bytes;
    int         chl = 80;
};

bool write_file(const std::string &path, const std::string &bytes)
{
    FILE *w = std::fopen(path.c_str(), "wb");
    if (!w)
        return false;
    const bool ok = bytes.empty() || std::fwrite(bytes.data(), 1, bytes.size(), w) == bytes.size();
    std::fclose(w);
    return ok;
}

Slic3r::Transform3d trafo(SplitMix &g)
{
    Slic3r::Transform3d t = Slic3r::Transform3d::Identity();
    t.translate(Slic3r::Vec3d(real(g, -80, 80), real(g, -80, 80), real(g, -20, 20)));
    t.rotate(Eigen::AngleAxisd(real(g, -3.2, 3.2), Slic3r::Vec3d(real(g, -1, 1), real(g, -1, 1), real(g, -1, 1)).normalized()));
    const double s = g.below(4) == 0 ? -1. : 1.;
    t.scale(Slic3r::Vec3d(s * real(g, 0.2, 3), real(g, 0.2, 3), g.below(4) == 0 ? real(g, 0.2, 3) : 1.));
    return t;
}

void put_trafo(Payload &p, const Slic3r::Transform3d &t)
{
    for (int r = 0; r < 4; ++r)
        for (int c = 0; c < 4; ++c)
            p.f64(t.matrix()(r, c));
}

// The transform chain's operations.
enum Op : uint8_t {
    SCALE = 0,
    SCALE_VERSOR = 1,
    TRANSLATE = 2,
    ROTATE_AXIS = 3,
    ROTATE_VEC = 4,
    MIRROR = 5,
    TRANSFORM = 6,
    TRANSFORM_MATRIX = 7,
    FLIP = 8,
    ALIGN = 9,
    MERGE = 10,
    TBB = 11,
    TBB_ABOVE = 12,
    ROTATE_POINT = 13,
    HULL_2D = 14,
    NUM_OPS = 15,
};

void record_case(Sets &sets, const std::string &tmp, const Case &c, SplitMix &g, bool committed)
{
    Payload p;
    p.str(c.label).str(c.bytes).i32(c.chl);
    Payload detail;
    detail.str(c.label);
    bool want_detail = false;
    if (!write_file(tmp, c.bytes))
        throw std::runtime_error("cannot write the temporary file");
    {
        stl_file   stl;
        const bool ok = stl_open(&stl, tmp.c_str(), nullptr, c.chl);
        p.boolean(ok);
        if (ok) {
            want_detail = stl.facet_start.size() <= 24;
            std::vector<std::pair<uint8_t, std::string>> steps;
            steps.emplace_back(uint8_t(OPEN), snapshot(stl));
            repair_steps(stl, [&](Step s) { steps.emplace_back(uint8_t(s), snapshot(stl)); });
            p.u32(uint32_t(steps.size()));
            detail.u32(uint32_t(steps.size()));
            for (const auto &s : steps) {
                p.u8(s.first).u64(fnv1a(s.second));
                detail.u8(s.first).str(s.second);
            }
            indexed_triangle_set its;
            stl_generate_shared_vertices(&stl, its);
            const std::string ib = its_bytes(its);
            p.u64(fnv1a(ib));
            detail.str(ib);
        }
    }
    TriangleMesh mesh;
    const bool   read_ok = mesh.ReadSTLFile(tmp.c_str(), true, nullptr, c.chl);
    p.boolean(read_ok);
    if (read_ok) {
        put_mesh(p, mesh);
        p.u64(mesh.facets_count());
        put_bbox(p, mesh.bounding_box());
        for (int i = 0; i < 3; ++i)
            p.f64(mesh.size()(i));
        for (int i = 0; i < 3; ++i)
            p.f64(mesh.center()(i));
        p.boolean(mesh.empty()).boolean(mesh.is_splittable());
        p.u64(Slic3r::its_num_open_edges(mesh.its)).u64(Slic3r::its_number_of_patches(mesh.its));
        put_polygon(p, mesh.convex_hull());
        const std::vector<TriangleMesh> parts = mesh.split();
        p.u32(uint32_t(parts.size()));
        for (const TriangleMesh &part : parts)
            put_mesh(p, part);
        TriangleMesh       work = mesh;
        const TriangleMesh first = mesh;
        const int          ops   = mesh.empty() ? 0 : 6 + int(g.below(14));
        p.u32(uint32_t(ops));
        for (int i = 0; i < ops; ++i) {
            const uint8_t op = uint8_t(g.below(NUM_OPS));
            p.u8(op);
            switch (op) {
            case SCALE: {
                const float f = float(real(g, -2.5, 2.5));
                p.f32(f);
                work.scale(f);
                put_mesh(p, work);
                break;
            }
            case SCALE_VERSOR: {
                const Slic3r::Vec3f v(float(real(g, -2, 2)), float(real(g, 0.1, 2)), g.below(3) ? float(real(g, 0.1, 2)) : 1.f);
                put_vec3f(p, v);
                work.scale(v);
                put_mesh(p, work);
                break;
            }
            case TRANSLATE: {
                const Slic3r::Vec3f v = g.below(5) == 0 ? Slic3r::Vec3f::Zero().eval() : Slic3r::Vec3f(float(real(g, -100, 100)), float(real(g, -100, 100)), float(real(g, -30, 30)));
                put_vec3f(p, v);
                work.translate(v);
                put_mesh(p, work);
                break;
            }
            case ROTATE_AXIS: {
                const float   a    = g.below(6) == 0 ? 0.f : float(real(g, -7, 7));
                const uint8_t axis = uint8_t(g.below(3));
                p.f32(a).u8(axis);
                work.rotate(a, Slic3r::Axis(axis));
                put_mesh(p, work);
                break;
            }
            case ROTATE_VEC: {
                const float         a = float(real(g, -7, 7));
                const Slic3r::Vec3d axis(real(g, -1, 1), real(g, -1, 1), real(g, -1, 1));
                p.f32(a).f64(axis(0)).f64(axis(1)).f64(axis(2));
                work.rotate(a, axis);
                put_mesh(p, work);
                break;
            }
            case MIRROR: {
                const uint8_t axis = uint8_t(g.below(3));
                p.u8(axis);
                work.mirror(Slic3r::Axis(axis));
                put_mesh(p, work);
                break;
            }
            case TRANSFORM: {
                const Slic3r::Transform3d t   = trafo(g);
                const bool                fix = g.below(2) != 0;
                put_trafo(p, t);
                p.boolean(fix);
                work.transform(t, fix);
                put_mesh(p, work);
                break;
            }
            case TRANSFORM_MATRIX: {
                Slic3r::Matrix3d m;
                for (int r = 0; r < 3; ++r)
                    for (int cc = 0; cc < 3; ++cc)
                        m(r, cc) = (r == cc ? 1. : 0.) + real(g, -0.7, 0.7);
                const bool fix = g.below(2) != 0;
                for (int r = 0; r < 3; ++r)
                    for (int cc = 0; cc < 3; ++cc)
                        p.f64(m(r, cc));
                p.boolean(fix);
                work.transform(m, fix);
                put_mesh(p, work);
                break;
            }
            case FLIP:
                work.flip_triangles();
                put_mesh(p, work);
                break;
            case ALIGN:
                work.align_to_origin();
                put_mesh(p, work);
                break;
            case MERGE: {
                const Slic3r::Vec3f v(float(real(g, -60, 60)), float(real(g, -60, 60)), float(real(g, -10, 10)));
                put_vec3f(p, v);
                TriangleMesh other = first;
                other.translate(v);
                work.merge(other);
                put_mesh(p, work);
                break;
            }
            case TBB: {
                const Slic3r::Transform3d t = trafo(g);
                put_trafo(p, t);
                put_bbox(p, work.transformed_bounding_box(t));
                break;
            }
            case TBB_ABOVE: {
                const Slic3r::Transform3d t = trafo(g);
                const double              z = real(g, -40, 40);
                put_trafo(p, t);
                p.f64(z);
                put_bbox(p, work.transformed_bounding_box(t, z));
                break;
            }
            case ROTATE_POINT: {
                const double  a = g.below(6) == 0 ? 0. : real(g, -400, 400);
                Slic3r::Point center(int64_t(g.below(2000000)) - 1000000, int64_t(g.below(2000000)) - 1000000);
                p.f64(a).i64(center.x()).i64(center.y());
                work.rotate(a, &center);
                put_mesh(p, work);
                break;
            }
            default: put_polygon(p, work.convex_hull()); break;
            }
        }
    }
    sets.full.add("MCAS", p);
    if (committed)
        sets.committed.add("MCAS", p);
    if (want_detail)
        sets.full.add("MSTD", detail);
}

std::vector<Tri> generated_mesh(SplitMix &g, bool nan_ok)
{
    std::vector<Tri> tris;
    const int        parts = g.below(5) == 0 ? 2 + int(g.below(2)) : 1;
    for (int part = 0; part < parts; ++part) {
        const int    k    = 1 + int(g.below(g.below(4) == 0 ? 8 : 3));
        const double o[3] = {real(g, -50, 50), real(g, -50, 50), real(g, -5, 30)};
        const double s[3] = {real(g, 0.5, 40), real(g, 0.5, 40), real(g, 0.5, 40)};
        std::vector<Tri> b = box(k, o, s);
        tris.insert(tris.end(), b.begin(), b.end());
    }
    if (g.below(3))
        rotate_all(tris, real(g, -3.2, 3.2), real(g, -3.2, 3.2), real(g, -3.2, 3.2));
    damage(g, tris, nan_ok);
    set_normals(g, tris, g.below(2) ? 0 : -1);
    for (Tri &t : tris)
        t.attr = g.below(4) == 0 ? uint16_t(g.below(65536)) : 0;
    return tris;
}

// OrcaSlicer's test STL files, from the oracle's source tree (<exe>/../src/tests/data).
bool orca_files(std::vector<Case> &out, std::string &error)
{
    char *pgm = nullptr;
    if (_get_pgmptr(&pgm) != 0 || pgm == nullptr) {
        error = "cannot find the probe's own path";
        return false;
    }
    const std::filesystem::path data = std::filesystem::path(pgm).parent_path().parent_path() / "src" / "tests" / "data";
    const struct
    {
        const char    *label;
        std::wstring   rel;
    } files[] = {
        {"orca:test_stl/ASCII/20mmbox-LF.stl", L"test_stl/ASCII/20mmbox-LF.stl"},
        {"orca:test_stl/ASCII/20mmbox-CRLF.stl", L"test_stl/ASCII/20mmbox-CRLF.stl"},
        {"orca:test_stl/ASCII/20mmbox-nonstandard.stl", L"test_stl/ASCII/20mmbox-nonstandard.stl"},
        {"orca:test_stl/Geraete/20mmbox-unicode.stl", L"test_stl/Geräte/20mmbox-čřšřěá.stl"},
        {"orca:test_3mf/Prusa.stl", L"test_3mf/Prusa.stl"},
    };
    for (const auto &f : files) {
        std::ifstream in(data / f.rel, std::ios::binary);
        if (!in) {
            error = std::string("cannot read ") + f.label + " under " + data.string();
            return false;
        }
        Case c;
        c.label.assign(f.label);
        c.bytes.assign(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
        out.push_back(c);
    }
    return true;
}

std::vector<Case> edge_cases()
{
    std::vector<Case> cases;
    SplitMix          g(0x3E5E0000u);
    const double      o[3] = {0, 0, 0}, s[3] = {20, 20, 20};
    auto              cube = [&]() {
        std::vector<Tri> t = box(1, o, s);
        set_normals(g, t, 0);
        return t;
    };
    auto add = [&](const char *label, std::string bytes, int chl = 80) { cases.push_back(Case{label, std::move(bytes), chl}); };
    add("binary cube", binary_stl(g, cube(), 1, 0));
    {
        auto t = cube();
        std::swap(t[3].v[1], t[3].v[2]);
        add("binary cube, one facet flipped", binary_stl(g, t, 1, 0));
    }
    {
        auto t = cube();
        t.erase(t.begin() + 5);
        add("binary cube, one facet removed", binary_stl(g, t, 0, 0));
    }
    {
        auto t = cube();
        Tri  d = t[2];
        d.v[2] = d.v[0];
        t.insert(t.begin() + 4, d);
        add("binary cube, a degenerate facet", binary_stl(g, t, 0, 0));
    }
    {
        auto t     = cube();
        t[7].v[0][0] = std::nextafter(t[7].v[0][0], INFINITY);
        add("binary cube, a vertex nudged", binary_stl(g, t, 0, 0));
    }
    {
        const double o2[3] = {30, 0, 0};
        auto         t     = cube();
        auto         t2    = box(2, o2, s);
        set_normals(g, t2, 0);
        t.insert(t.end(), t2.begin(), t2.end());
        add("binary, two boxes", binary_stl(g, t, 0, 0));
    }
    {
        auto t = cube();
        for (Tri &tri : t)
            std::swap(tri.v[1], tri.v[2]);
        add("binary cube, inside out", binary_stl(g, t, 0, 0));
    }
    {
        auto t        = cube();
        t[4].v[1][2] = NAN;
        add("binary cube, a NaN vertex", binary_stl(g, t, 0, 0));
    }
    {
        auto t = cube();
        t.push_back(t[0]);
        t.push_back(t[1]);
        add("binary cube, two facets twice", binary_stl(g, t, 0, 0));
    }
    add("binary, no facets", binary_stl(g, {}, 0, 0));
    {
        auto t = cube();
        t.resize(4);
        add("binary, four facets (the smallest size)", binary_stl(g, t, 0, 0));
    }
    {
        std::string b = binary_stl(g, cube(), 0, 0);
        b.resize(b.size() - 7);
        add("binary cube, cut short", b);
    }
    add("binary cube, a wrong count", binary_stl(g, cube(), 2, 3));
    {
        std::string b = binary_stl(g, cube(), 0, 0);
        b.insert(80, 20, 'h');
        add("binary cube, a 100-byte header", b, 100);
    }
    add("binary cube, header length 0 (80)", binary_stl(g, cube(), 0, 0), 0);
    add("ascii cube, LF", ascii_stl(g, cube(), 0, 0, true));
    add("ascii cube, CR LF", ascii_stl(g, cube(), 0, 1, true));
    add("ascii cube, CR CR LF", ascii_stl(g, cube(), 0, 2, true));
    add("ascii cube, LF CR", ascii_stl(g, cube(), 0, 3, true));
    add("ascii cube, no final newline", ascii_stl(g, cube(), 0, 0, false));
    add("ascii cube, %f numbers", ascii_stl(g, cube(), 2, 0, true));
    add("ascii cube, Ctrl-Z at the end", ascii_stl(g, cube(), 0, 0, true) + "\x1a");
    {
        std::string t = ascii_stl(g, cube(), 0, 0, true);
        t.insert(t.size() / 2, "\x1a");
        add("ascii cube, Ctrl-Z in the middle", t);
    }
    {
        std::string  t   = ascii_stl(g, cube(), 0, 0, true);
        const size_t pos = t.find("endloop", t.size() / 2);
        t.erase(pos, 7);
        add("ascii cube, an endloop missing", t);
    }
    {
        auto t = cube();
        for (Tri &tri : t)
            tri.n = {NAN, NAN, NAN};
        add("ascii cube, nan normals", ascii_stl(g, t, 0, 0, true));
    }
    {
        std::string t = ascii_stl(g, cube(), 0, 0, true);
        t += "solid again\nendsolid again\n";
        add("ascii cube, a second empty solid", t);
    }
    add("ascii, too short", "solid x\nendsolid x\n");
    {
        auto t = cube();
        t.resize(2);
        add("ascii, two facets", ascii_stl(g, t, 0, 1, true));
    }
    return cases;
}

bool run_mesh(const Options &options, std::string &error)
{
    tbb::global_control gc(tbb::global_control::max_allowed_parallelism, 1);
    tbb::task_arena     arena{8, 8u};
    bool                ok = true;
    Sets                sets;
    head(sets, "mesh");
    std::error_code             ec;
    const std::filesystem::path dir = std::filesystem::temp_directory_path(ec) / ("sketch-orca-probe-mesh-" + std::to_string(_getpid()));
    if (ec || !std::filesystem::create_directories(dir, ec)) {
        error = "cannot make a temporary folder";
        return false;
    }
    const std::string tmp = (dir / "model.stl").string();
    arena.execute([&]() {
        try {
            std::vector<Case> cases = edge_cases();
            std::vector<Case> orca;
            if (!(ok = orca_files(orca, error)))
                return;
            cases.insert(cases.end(), orca.begin(), orca.end());
            SplitMix ops(0x3E5E0F00u);
            for (const Case &c : cases)
                record_case(sets, tmp, c, ops, true);
            SplitMix g(0x3E5E1000u);
            int      committed = 0;
            for (int i = 0; i < 700; ++i) {
                const bool       ascii = g.below(3) == 0;
                std::vector<Tri> tris  = generated_mesh(g, true);
                Case             c;
                c.label = (ascii ? "ascii #" : "binary #") + std::to_string(i);
                if (ascii)
                    c.bytes = ascii_stl(g, tris, g.below(2) ? 0 : -1, g.below(2) ? int(g.below(4)) : -1, g.below(4) != 0);
                else {
                    const int64_t delta = g.below(10) == 0 ? int64_t(g.below(5)) - 2 : 0;
                    c.bytes             = binary_stl(g, tris, int(g.below(3)), delta);
                    if (g.below(20) == 0)
                        c.bytes.resize(c.bytes.size() - size_t(1 + g.below(49)));
                }
                const bool take = committed < 30 && c.bytes.size() <= 4096;
                committed += take ? 1 : 0;
                record_case(sets, tmp, c, ops, take);
            }
        } catch (const std::exception &e) {
            ok    = false;
            error = e.what();
        }
    });
    std::filesystem::remove_all(dir, ec);
    if (!ok)
        return false;
    return save(sets, options, "mesh.sop1", error);
}

} // namespace

const Target &target()
{
    static const Target t{"mesh", false, run_mesh};
    return t;
}

} // namespace probe
