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
// SLMH, the slice probe's mesh table (parity/sop1/slice.md, "SLMH"): shapes the probe builds with
// libslic3r's its_make_* and by hand (nested, hollow, overlapping and open boxes, split walls,
// whole shells flipped, a degenerate face, stacked blocks whose horizontal faces meet a slicing
// plane, a jittered sphere, islands closer than twice the closing radius, a wall thinner than a
// line, and meshes with a face or a wall wound against its neighbours), then OrcaSlicer's test OBJ
// meshes (<exe>/../src/tests/data) as load_obj reads them. A whole flipped shell stays consistent;
// only a face wound against its neighbour makes two intersection lines meet end to end, so that
// make_loops' second rounds (try_connect_reversed) join polylines in reverse, and where the joined
// loop runs clockwise reverse it (the cube with face 4 flipped and its flip; the split boxes with
// their +x wall or the rest flipped). The meshes of 300 faces or fewer are small: the committed set
// takes them and the records that use them. Written by orca-o3-probe-slice, 2026-11-13.
#include "common.hpp"

#include <algorithm>
#include <cmath>
#include <filesystem>
#include <initializer_list>
#include <stdlib.h>
#include <utility>

#include <libslic3r/Format/OBJ.hpp>

namespace probe {
namespace slice {

namespace {

using namespace Slic3r;

indexed_triangle_set moved(indexed_triangle_set its, float dx, float dy, float dz)
{
    for (stl_vertex &v : its.vertices)
        v += stl_vertex(dx, dy, dz);
    return its;
}

indexed_triangle_set merged(indexed_triangle_set a, const indexed_triangle_set &b)
{
    its_merge(a, b);
    return a;
}

indexed_triangle_set flipped(indexed_triangle_set its)
{
    its_flip_triangles(its);
    return its;
}

// The given faces wound the other way (indices 1 and 2 swapped), against their neighbours.
indexed_triangle_set flipped_faces(indexed_triangle_set its, std::initializer_list<int> faces)
{
    for (int f : faces)
        std::swap(its.indices[f](1), its.indices[f](2));
    return its;
}

// The cube's +x wall (faces 4 and 5 of its_make_cube) on vertices of its own, moved by dx.
indexed_triangle_set split_box(float dx)
{
    indexed_triangle_set its = its_make_cube(20, 20, 20);
    const int            n   = int(its.vertices.size());
    for (int k : {0, 4, 7, 1})
        its.vertices.push_back(its.vertices[k] + stl_vertex(dx, 0.f, 0.f));
    auto remap = [n](int k) { return k == 0 ? n : k == 4 ? n + 1 : k == 7 ? n + 2 : k == 1 ? n + 3 : k; };
    for (int f : {4, 5})
        for (int j = 0; j < 3; ++j)
            its.indices[f](j) = remap(its.indices[f](j));
    return its;
}

// The neighbour overload of its_face_edge_ids leaves its slots uninitialised and writes them as it
// walks the faces: a slot whose neighbour comes later is written with the neighbour's slot of the
// same edge, one whose neighbour came earlier is left to that neighbour. This walks the faces as
// it does and is true when every slot is written and every edge is found in its neighbour; then
// that overload and slice_mesh_slabs (which calls it) read no slot they did not write.
bool symmetric_neighbors(const indexed_triangle_set &its)
{
    const std::vector<Vec3i32> nb = its_face_neighbors(its);
    std::vector<uint8_t>       written(nb.size(), 0);
    for (int i = 0; i < int(nb.size()); ++i)
        for (int j = 0; j < 3; ++j) {
            const int n = nb[i](j);
            if (n < -1 || n >= int(nb.size()) || n == i)
                return false;
            if (n == -1)
                written[i] |= uint8_t(1u << j);
            else if (n > i) {
                Vec2i32 edge = its_triangle_edge(its.indices[i], j);
                std::swap(edge(0), edge(1));
                int k = its_triangle_edge_index(its.indices[n], edge);
                if (k < 0) {
                    std::swap(edge(0), edge(1));
                    k = its_triangle_edge_index(its.indices[n], edge);
                }
                if (k < 0)
                    return false;
                written[i] |= uint8_t(1u << j);
                written[n] |= uint8_t(1u << k);
            }
        }
    for (uint8_t w : written)
        if (w != 7)
            return false;
    return true;
}

} // namespace

bool build_meshes(std::vector<Mesh> &out, std::string &error)
{
    const indexed_triangle_set cube = its_make_cube(20, 20, 20);
    std::vector<std::pair<std::string, indexed_triangle_set>> built;
    built.emplace_back("cube 20", cube);
    built.emplace_back("box 30x10x5 centred", moved(its_make_cube(30, 10, 5), -15.f, -5.f, 0.f));
    built.emplace_back("pyramid 20 15", its_make_pyramid(20.f, 15.f));
    built.emplace_back("cylinder 8 12 /24", its_make_cylinder(8, 12, 2. * PI / 24));
    built.emplace_back("cone 10 15 /20", its_make_cone(10, 15, 2. * PI / 20));
    built.emplace_back("sphere 10 /16", its_make_sphere(10, 2. * PI / 16));
    built.emplace_back("frustum 10 12 /16", its_make_frustum(10, 12, 2. * PI / 16));
    built.emplace_back("torus 10 3 /12", its_make_torus(10, 3, 2. * PI / 12));
    built.emplace_back("nested boxes", merged(cube, moved(its_make_cube(10, 10, 10), 5.f, 5.f, 5.f)));
    built.emplace_back("hollow box", merged(cube, flipped(moved(its_make_cube(10, 10, 10), 5.f, 5.f, 5.f))));
    built.emplace_back("overlapping cubes", merged(cube, moved(cube, 10.f, 10.f, 5.f)));
    {
        indexed_triangle_set open = cube;
        open.indices.erase(open.indices.begin() + 4, open.indices.begin() + 6);
        built.emplace_back("open box", open);
    }
    built.emplace_back("split box exact", split_box(0.f));
    built.emplace_back("split box gap", split_box(0.3f));
    built.emplace_back("flipped cube", flipped(cube));
    {
        indexed_triangle_set d = cube;
        const int            n = int(d.vertices.size());
        d.vertices.push_back(stl_vertex(0.f, 0.f, 10.f));
        d.vertices.push_back(stl_vertex(10.f, 0.f, 10.f));
        d.vertices.push_back(stl_vertex(20.f, 0.f, 10.f));
        d.vertices.push_back(d.vertices[3]);
        d.indices.emplace_back(n, n + 1, n + 2);
        built.emplace_back("degenerate", d);
    }
    built.emplace_back("stacked blocks", merged(its_make_cube(20, 20, 10), moved(its_make_cube(10, 20, 10), 0.f, 0.f, 10.f)));
    {
        indexed_triangle_set t;
        t.vertices = {{0, 0, 0}, {15, 0, 5}, {0, 15, 10}, {5, 5, 15}};
        t.indices  = {{0, 2, 1}, {0, 1, 3}, {1, 2, 3}, {0, 3, 2}};
        built.emplace_back("tetrahedron", t);
    }
    {
        indexed_triangle_set s = its_make_sphere(10, 2. * PI / 12);
        SplitMix             g(0x51CE0001u);
        for (stl_vertex &v : s.vertices)
            v += stl_vertex(float(real(g, -0.3, 0.3)), float(real(g, -0.3, 0.3)), float(real(g, -0.3, 0.3)));
        built.emplace_back("jittered sphere", s);
    }
    built.emplace_back("cube on the bed", moved(cube, 150.f, 150.f, 0.f));
    built.emplace_back("tiny cube", its_make_cube(0.5, 0.5, 0.5));
    built.emplace_back("two islands", merged(its_make_cube(10, 10, 10), moved(its_make_cube(10, 10, 10), 10.08f, 0.f, 0.f)));
    built.emplace_back("thin wall", its_make_cube(20, 0.08, 10));
    built.emplace_back("prism", its_make_prism(10.f, 20.f, 8.f));
    {
        // A wedge whose top slopes from z 0.6 to 3.4, its vertices on the heights a 0.2 layer reaches.
        indexed_triangle_set w;
        w.vertices = {{0, 0, 0}, {10, 0, 0}, {10, 10, 0}, {0, 10, 0}, {0, 0, 0.6f}, {10, 0, 3.4f}, {10, 10, 3.4f}, {0, 10, 0.6f}};
        // Put them in its_make_cube's vertex order, so that the cube's faces close the wedge.
        const int map[8] = {2, 1, 0, 3, 6, 7, 4, 5};
        indexed_triangle_set r;
        r.vertices.resize(8);
        for (int k = 0; k < 8; ++k)
            r.vertices[k] = w.vertices[map[k]];
        r.indices = cube.indices;
        built.emplace_back("wedge", r);
    }
    // Faces wound against their neighbours. Face 4 of its_make_cube is a triangle of the +x wall
    // ({0, 4, 7}), which every plane between the cube's ends cuts (face 0, the bottom, is
    // horizontal and gives no line). The cube with it flipped joins its polylines end to end in
    // chain_open_polylines_exact's second round and stays counter-clockwise; flipped whole, the
    // longer polyline is clockwise and that round reverses the joined loop. The split boxes' +x wall
    // has no edge with the rest, so chain_open_polylines_close_gaps joins it, in reverse when it is
    // flipped (touching, then 0.3 mm away), and reverses the loop when the rest is flipped instead.
    {
        const indexed_triangle_set one = flipped_faces(cube, {4});
        built.emplace_back("cube, face 4 flipped", one);
        built.emplace_back("flipped cube, face 4 kept", flipped(one));
        built.emplace_back("split box exact, wall flipped", flipped_faces(split_box(0.f), {4, 5}));
        const indexed_triangle_set gap = flipped_faces(split_box(0.3f), {4, 5});
        built.emplace_back("split box gap, wall flipped", gap);
        built.emplace_back("split box gap, rest flipped", flipped(gap));
    }

    for (auto &b : built) {
        Mesh m;
        m.id    = uint32_t(out.size());
        m.label = b.first;
        m.its   = std::move(b.second);
        m.small = m.its.indices.size() <= 300;
        out.push_back(std::move(m));
    }

    char *pgm = nullptr;
    if (_get_pgmptr(&pgm) != 0 || pgm == nullptr) {
        error = "cannot find the probe's own path";
        return false;
    }
    const std::filesystem::path data = std::filesystem::path(pgm).parent_path().parent_path() / "src" / "tests" / "data";
    const char *const           objs[] = {"20mm_cube",
                                          "2x20x10",
                                          "V",
                                          "V_standing",
                                          "bridge",
                                          "cube_with_concave_hole",
                                          "cube_with_concave_hole_enlarged",
                                          "cube_with_concave_hole_enlarged_standing",
                                          "cube_with_hole",
                                          "ipadstand",
                                          "overhang",
                                          "pyramid",
                                          "simplification",
                                          "sloping_hole",
                                          "small_dorito",
                                          "two_hollow_squares",
                                          "A",
                                          "A_upsidedown",
                                          "extruder_idler",
                                          "extruder_idler_quads",
                                          "frog_legs"};
    for (const char *name : objs) {
        const std::filesystem::path path = data / (std::string(name) + ".obj");
        TriangleMesh                mesh;
        ObjInfo                     info;
        std::string                 message;
        if (!load_obj(path.string().c_str(), &mesh, info, message)) {
            error = "cannot load " + path.string() + ": " + message;
            return false;
        }
        Mesh m;
        m.id    = uint32_t(out.size());
        m.label = std::string("orca:") + name + ".obj";
        m.its   = mesh.its;
        m.small = m.its.indices.size() <= 300;
        out.push_back(std::move(m));
    }

    for (Mesh &m : out) {
        m.symmetric = symmetric_neighbors(m.its);
        m.zmin      = m.its.vertices.front().z();
        m.zmax      = m.zmin;
        for (const stl_vertex &v : m.its.vertices) {
            m.zmin = std::min(m.zmin, v.z());
            m.zmax = std::max(m.zmax, v.z());
        }
    }
    return true;
}

void record_meshes(Sets &sets, const std::vector<Mesh> &meshes)
{
    for (const Mesh &m : meshes) {
        Payload p;
        p.u32(m.id).str(m.label).boolean(m.symmetric);
        put_its(p, m.its);
        // Every small mesh is in the committed set, whatever the share: the records there use them.
        if (m.small)
            sets.committed.add("SLMH", p);
        sets.full.add("SLMH", p);
    }
}

} // namespace slice
} // namespace probe
