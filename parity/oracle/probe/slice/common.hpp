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
// What the slice probe's sources share (parity/sop1/slice.md, "The values"): the mesh table, the
// writers of the page's compound values (meshes, matrices, polygons, ExPolygons, SlicingParameters,
// profiles, settings), the configs made from settings, and the committed set's share per kind.
// Written by orca-o3-probe-slice, 2026-11-13. Each record function is defined in one file of this
// folder: meshes.cpp (SLMH), front.cpp (SLFE, SLMC, SLTS, SLSH, SLBX), slicer.cpp (SLMS, SLM1,
// SLMX, SLSB) and layers.cpp (SLPR, SLRG, SLAD, SLAP, SLSM, SLAJ, SLOL).
#pragma once

#include "../probe.hpp"

#include <cstdint>
#include <map>
#include <string>
#include <utility>
#include <vector>

#include <libslic3r/ExPolygon.hpp>
#include <libslic3r/Geometry.hpp>
#include <libslic3r/Polygon.hpp>
#include <libslic3r/PrintConfig.hpp>
#include <libslic3r/Slicing.hpp>
#include <libslic3r/TriangleMesh.hpp>

namespace probe {
namespace slice {

// One mesh of the table (SLMH): its id is its index.
struct Mesh
{
    uint32_t             id = 0;
    std::string          label;
    indexed_triangle_set its;
    // In the committed set: the records that use it may go there (meshes.cpp says which).
    bool small = false;
    // its_face_neighbors gives every edge back from both faces (meshes.cpp, symmetric_neighbors), so
    // the neighbour overload of its_face_edge_ids and slice_mesh_slabs fill every slot they make.
    bool symmetric = false;
    // The lowest and highest z of a vertex, untransformed.
    float zmin = 0.f, zmax = 0.f;
};

// The mesh table, in the order of their ids (meshes.cpp); false, with `error`, when a file of
// OrcaSlicer's tests cannot be read.
bool build_meshes(std::vector<Mesh> &out, std::string &error);
void record_meshes(Sets &sets, const std::vector<Mesh> &meshes);

void record_front(Sets &sets, const std::vector<Mesh> &meshes);
void record_slicer(Sets &sets, const std::vector<Mesh> &meshes);
void record_layers(Sets &sets, const std::vector<Mesh> &meshes);

// The committed set's share of each kind, in bytes of payload: an eligible record goes there while
// its kind's share holds (add). The full set takes every record.
struct Shares
{
    std::map<std::string, size_t> used;
    std::map<std::string, size_t> cap;
};
Shares &shares();
void    add(Sets &sets, const char (&kind)[5], const Payload &p, bool eligible);

// A uniform double in [a, b) and a pick from a list, from the seeded generator.
double real(SplitMix &g, double a, double b);
template<typename T, size_t N> T pick(SplitMix &g, const T (&v)[N]) { return v[g.below(N)]; }

// A transformation as the four vectors it is set from, in this order (set_offset, set_rotation,
// set_scaling_factor, set_mirror), as model.sop1's MXFM sets one.
struct Xform
{
    Slic3r::Vec3d offset{0, 0, 0}, rotation{0, 0, 0}, scale{1, 1, 1}, mirror{1, 1, 1};
    Slic3r::Geometry::Transformation make() const;
};
Xform random_xform(SplitMix &g, bool tilt);

using Settings = std::vector<std::pair<std::string, std::string>>;
// Each setting set with set_deserialize_strict on whichever of the two holds its key; a key that
// neither holds throws.
void apply_settings(Slic3r::PrintConfig &pc, Slic3r::PrintObjectConfig &oc, const Settings &s);

// The values of parity/sop1/slice.md.
void put_its(Payload &p, const indexed_triangle_set &its);
void put_matrix(Payload &p, const Slic3r::Transform3d &m);
void put_xform(Payload &p, const Xform &x);
void put_polygon(Payload &p, const Slic3r::Polygon &poly);
void put_polygons(Payload &p, const Slic3r::Polygons &polys);
void put_expolygons(Payload &p, const Slic3r::ExPolygons &ex);
void put_params(Payload &p, const Slic3r::SlicingParameters &s);
void put_profile(Payload &p, const std::vector<double> &v);
void put_floats(Payload &p, const std::vector<float> &v);
void put_settings(Payload &p, const Settings &s);
void put_edges(Payload &p, const std::vector<Slic3r::Vec3i32> &e);

} // namespace slice
} // namespace probe
