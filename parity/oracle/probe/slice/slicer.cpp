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
// The slice probe's TriangleMeshSlicer records (parity/sop1/slice.md): SLMS slice_mesh over a list
// of heights, SLM1 slice_mesh at one plane, SLMX slice_mesh_ex, SLSB slice_mesh_slabs. The facet
// slicing, the line chaining (by triangle connectivity, exact, then closing gaps) and
// make_expolygons are file-local in TriangleMeshSlicer.cpp: they are reached through these four
// public functions, as the engine reaches them. Heights are a layer series (the slice_z of
// PrintObjectSlice's layers), the mesh's own vertex heights (planes through vertices, edges and
// horizontal faces), random heights, heights at and past the mesh's ends, one height or none;
// the transformations are an object's (offset, rotation, scale, sometimes mirrored). A mirrored
// case takes the triangles flipped (its_flip_triangles) or not, at random: slice_volume tests
// `params2.trafo.rotation().determinant() < 0.`, and Eigen's rotation() (computeRotationScaling)
// always has the determinant +1, so the engine never flips there, while a caller that does flip is
// recorded too. Written by orca-o3-probe-slice, 2026-11-13.
#include "common.hpp"

#include <algorithm>
#include <cmath>

#include <libslic3r/TriangleMeshSlicer.hpp>

namespace probe {
namespace slice {

namespace {

using namespace Slic3r;
using Mode = MeshSlicingParams::SlicingMode;

enum ZsKind { Layers, Vertices, Random, Ends, Single, Empty };

// The heights a plane through the mesh meets, transformed as the slicer transforms the vertices
// (XY scaled, in floats); only used to choose the heights.
std::vector<float> vertex_heights(const Mesh &m, const Transform3d &t)
{
    const bool         identity = t.matrix() == Transform3d::Identity().matrix();
    Transform3d        t2       = t;
    const double       s        = 1. / SCALING_FACTOR;
    t2.prescale(Vec3d(s, s, 1.));
    const Transform3f  tf = t2.cast<float>();
    std::vector<float> zs;
    for (const stl_vertex &v : m.its.vertices)
        zs.push_back(identity ? v.z() : (tf * v).z());
    std::sort(zs.begin(), zs.end());
    zs.erase(std::unique(zs.begin(), zs.end()), zs.end());
    return zs;
}

std::vector<float> make_zs(SplitMix &g, const Mesh &m, const Transform3d &t, ZsKind kind, size_t cap)
{
    const std::vector<float> vz   = vertex_heights(m, t);
    const double             zmin = vz.front(), zmax = vz.back();
    std::vector<float>       zs;
    switch (kind) {
    case Layers: {
        static const double hs[] = {0.2, 0.28, 0.5, 1.0, 2.0, 0.12};
        const double        h    = pick(g, hs);
        const double        h0   = g.below(3) == 0 ? h : g.below(2) ? 0.2 : 0.3;
        double              lo = zmin, hi = zmin + h0;
        while (lo < zmax && zs.size() < cap) {
            zs.push_back(float(0.5 * (lo + hi)));
            lo = hi;
            hi = lo + h;
        }
        break;
    }
    case Vertices: {
        const size_t step = vz.size() <= cap ? 1 : (vz.size() + cap - 1) / cap;
        for (size_t i = 0; i < vz.size(); i += step)
            zs.push_back(vz[i]);
        break;
    }
    case Random: {
        const size_t n = 1 + size_t(g.below(cap));
        for (size_t i = 0; i < n; ++i)
            zs.push_back(float(real(g, zmin - 0.5, zmax + 0.5)));
        std::sort(zs.begin(), zs.end());
        break;
    }
    case Ends: zs = {float(zmin - 3.), float(zmin), float(zmax), float(zmax + 3.)}; break;
    case Single: zs = {g.below(2) ? vz[g.below(vz.size())] : float(real(g, zmin, zmax))}; break;
    case Empty: break;
    }
    return zs;
}

Mode random_mode(SplitMix &g)
{
    static const Mode modes[] = {Mode::Regular, Mode::EvenOdd, Mode::Positive, Mode::PositiveLargestContour};
    return pick(g, modes);
}

// The object transformation of a case, with whether the triangles are flipped first.
struct Trafo
{
    Transform3d m = Transform3d::Identity();
    bool        flip = false;
};
Trafo random_trafo(SplitMix &g)
{
    Trafo t;
    t.m = random_xform(g, g.below(3) == 0).make().get_matrix();
    if (t.m.linear().determinant() < 0)
        t.flip = g.below(2) != 0;
    return t;
}

indexed_triangle_set prepared(const Mesh &m, const Trafo &t)
{
    indexed_triangle_set its = m.its;
    if (t.flip)
        its_flip_triangles(its);
    return its;
}

// slice_mesh keeps the largest loop of a PositiveLargestContour layer, found by its area, and
// dereferences a null pointer when every loop of the layer has the area 0 (make_loops, an edge the
// engine never reaches: slice_mesh_ex slices PositiveLargestContour as Positive). Where a layer
// that would take that mode is such a layer, the case takes Positive instead.
bool largest_safe(const std::vector<Polygons> &layers, const MeshSlicingParams &params)
{
    for (size_t i = 0; i < layers.size(); ++i) {
        const Mode mode = i < params.slicing_mode_normal_below_layer ? params.mode_below : params.mode;
        if (mode != Mode::PositiveLargestContour || layers[i].empty())
            continue;
        bool any = false;
        for (const Polygon &poly : layers[i])
            any = any || std::abs(poly.area()) > 0.;
        if (!any)
            return false;
    }
    return true;
}
void guard_largest(MeshSlicingParams &params, const std::vector<Polygons> &regular_layers)
{
    if (largest_safe(regular_layers, params))
        return;
    if (params.mode == Mode::PositiveLargestContour)
        params.mode = Mode::Positive;
    if (params.mode_below == Mode::PositiveLargestContour)
        params.mode_below = Mode::Positive;
}
bool uses_largest(const MeshSlicingParams &params)
{
    return params.mode == Mode::PositiveLargestContour ||
           (params.mode_below == Mode::PositiveLargestContour && params.slicing_mode_normal_below_layer > 0);
}
MeshSlicingParams regular(const MeshSlicingParams &params)
{
    MeshSlicingParams r = params;
    r.mode = r.mode_below = Mode::Regular;
    return r;
}

void put_mode(Payload &p, const MeshSlicingParams &params)
{
    p.u32(uint32_t(params.mode)).u64(params.slicing_mode_normal_below_layer).u32(uint32_t(params.mode_below));
    put_matrix(p, params.trafo);
}

// ---- SLMS, SLM1 ----------------------------------------------------------------------------

void record_slice_mesh(Sets &sets, const std::vector<Mesh> &meshes)
{
    SplitMix g(0x51CE6000u);
    for (const Mesh &m : meshes)
        for (int c = 0; c < 7; ++c) {
            MeshSlicingParams params;
            Trafo             t;
            ZsKind            kind = Layers;
            switch (c) {
            case 0: break;
            case 1: kind = Vertices; break;
            case 2:
                params.mode = random_mode(g);
                t           = random_trafo(g);
                break;
            case 3:
                params.mode = Mode::EvenOdd;
                kind        = Random;
                break;
            case 4:
                params.mode = Mode::Positive;
                kind        = Single;
                if (g.below(2))
                    t = random_trafo(g);
                break;
            case 5:
                params.mode                            = Mode::PositiveLargestContour;
                params.mode_below                      = g.below(3) == 0 ? Mode::Positive : Mode::Regular;
                params.slicing_mode_normal_below_layer = size_t(1 + g.below(4));
                if (g.below(2))
                    t = random_trafo(g);
                break;
            case 6: kind = m.id % 2 == 0 ? Ends : Empty; break;
            }
            params.trafo                         = t.m;
            const indexed_triangle_set its       = prepared(m, t);
            const std::vector<float>   zs        = make_zs(g, m, t.m, kind, 24);
            if (uses_largest(params))
                guard_largest(params, slice_mesh(its, zs, regular(params)));
            const std::vector<Polygons> layers   = slice_mesh(its, zs, params);
            Payload p;
            p.u32(m.id).boolean(t.flip);
            put_mode(p, params);
            put_floats(p, zs);
            p.u64(layers.size());
            for (const Polygons &l : layers)
                put_polygons(p, l);
            add(sets, "SLMS", p, m.small && c == int(m.id % 7) && p.bytes.size() <= 6000);
        }
    for (const Mesh &m : meshes)
        for (int c = 0; c < 3; ++c) {
            MeshSlicingParams params;
            Trafo             t;
            params.mode = c == 0 ? Mode::Regular : random_mode(g);
            if (c == 2)
                t = random_trafo(g);
            params.trafo                   = t.m;
            const indexed_triangle_set its = prepared(m, t);
            const std::vector<float>   vz  = vertex_heights(m, t.m);
            const float z = c == 0 ? float(0.5 * (double(vz.front()) + double(vz.back()))) : c == 1 ? vz[g.below(vz.size())] : float(real(g, vz.front(), vz.back()));
            if (uses_largest(params))
                guard_largest(params, {slice_mesh(its, z, regular(params))});
            Payload p;
            p.u32(m.id).boolean(t.flip);
            put_mode(p, params);
            p.f32(z);
            put_polygons(p, slice_mesh(its, z, params));
            add(sets, "SLM1", p, m.small && c == int(m.id % 3));
        }
}

// ---- SLMX ----------------------------------------------------------------------------------

void record_slice_mesh_ex(Sets &sets, const std::vector<Mesh> &meshes)
{
    SplitMix g(0x51CE7000u);
    for (const Mesh &m : meshes)
        for (int c = 0; c < 7; ++c) {
            MeshSlicingParamsEx params;
            Trafo               t;
            ZsKind              kind = Layers;
            params.closing_radius    = 0.049f;
            switch (c) {
            case 0:
                params.resolution = 0.0025;
                t                 = random_trafo(g);
                break;
            case 1: kind = Vertices; break;
            case 2:
                params.mode       = Mode::EvenOdd;
                params.resolution = 0.0025;
                t                 = random_trafo(g);
                break;
            case 3:
                params.mode           = Mode::Positive;
                params.closing_radius = 0.2f;
                params.extra_offset   = 0.1f;
                kind                  = Random;
                break;
            case 4:
                // No closing radius: with no extra offset either (even mesh ids) make_expolygons
                // takes only the union, as PrintObjectSlice does at a slice_closing_radius of 0.
                params.mode           = random_mode(g);
                params.closing_radius = 0.f;
                params.extra_offset   = m.id % 2 == 0 ? 0.f : 0.3f;
                params.resolution     = 0.05;
                break;
            case 5:
                params.mode                            = Mode::PositiveLargestContour;
                params.mode_below                      = Mode::Regular;
                params.slicing_mode_normal_below_layer = size_t(1 + g.below(4));
                params.extra_offset                    = -0.05f;
                params.resolution                      = 0.0025;
                break;
            case 6:
                params.closing_radius = 1.f;
                kind                  = g.below(2) ? Single : Vertices;
                break;
            }
            params.trafo                         = t.m;
            const indexed_triangle_set its       = prepared(m, t);
            const std::vector<float>   zs        = make_zs(g, m, t.m, kind, 16);
            const std::vector<ExPolygons> layers = slice_mesh_ex(its, zs, params);
            Payload p;
            p.u32(m.id).boolean(t.flip);
            put_mode(p, params);
            p.f32(params.closing_radius).f32(params.extra_offset).f64(params.resolution);
            put_floats(p, zs);
            p.u64(layers.size());
            for (const ExPolygons &l : layers)
                put_expolygons(p, l);
            add(sets, "SLMX", p, m.small && c == int(m.id % 7) && p.bytes.size() <= 6000);
        }
}

// ---- SLSB ----------------------------------------------------------------------------------

void record_slabs(Sets &sets, const std::vector<Mesh> &meshes)
{
    SplitMix g(0x51CE8000u);
    for (const Mesh &m : meshes) {
        if (!m.symmetric)
            continue;
        for (int c = 0; c < 2; ++c) {
            Transform3d   trafo = c == 0 ? Transform3d::Identity() : random_xform(g, g.below(3) == 0).make().get_matrix();
            const uint8_t flags = c == 0 ? 7 : uint8_t(1 + g.below(7));
            const std::vector<float>             zs = make_zs(g, m, trafo, c == 0 ? Layers : (g.below(2) ? Layers : Vertices), 16);
            std::vector<Polygons>                top, bottom;
            std::vector<std::pair<Vec3f, Vec3f>> vertical;
            slice_mesh_slabs(m.its, zs, trafo, (flags & 1) ? &top : nullptr, (flags & 2) ? &bottom : nullptr, (flags & 4) ? &vertical : nullptr, []() {});
            Payload p;
            p.u32(m.id);
            put_matrix(p, trafo);
            p.u8(flags);
            put_floats(p, zs);
            if (flags & 1) {
                p.u64(top.size());
                for (const Polygons &l : top)
                    put_polygons(p, l);
            }
            if (flags & 2) {
                p.u64(bottom.size());
                for (const Polygons &l : bottom)
                    put_polygons(p, l);
            }
            if (flags & 4) {
                p.u64(vertical.size());
                for (const auto &v : vertical)
                    p.f32(v.first.x()).f32(v.first.y()).f32(v.first.z()).f32(v.second.x()).f32(v.second.y()).f32(v.second.z());
            }
            add(sets, "SLSB", p, m.small && c == int(m.id % 2) && p.bytes.size() <= 4000);
        }
    }
}

} // namespace

void record_slicer(Sets &sets, const std::vector<Mesh> &meshes)
{
    record_slice_mesh(sets, meshes);
    record_slice_mesh_ex(sets, meshes);
    record_slabs(sets, meshes);
}

} // namespace slice
} // namespace probe
