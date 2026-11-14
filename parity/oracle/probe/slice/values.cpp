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
// The slice probe's value writers, its committed set's shares and the configs made from settings
// (parity/sop1/slice.md, "The values"; common.hpp declares them). Written by orca-o3-probe-slice,
// 2026-11-13.
#include "common.hpp"

#include <stdexcept>

namespace probe {
namespace slice {

using namespace Slic3r;

Shares &shares()
{
    static Shares s;
    return s;
}

void add(Sets &sets, const char (&kind)[5], const Payload &p, bool eligible)
{
    Shares           &s = shares();
    const std::string k(kind, 4);
    const auto        cap = s.cap.find(k);
    if (eligible && cap != s.cap.end() && s.used[k] + p.bytes.size() <= cap->second) {
        s.used[k] += p.bytes.size();
        sets.committed.add(kind, p);
    }
    sets.full.add(kind, p);
}

double real(SplitMix &g, double a, double b) { return a + (b - a) * (double(g.next() >> 11) * 0x1.0p-53); }

Geometry::Transformation Xform::make() const
{
    Geometry::Transformation t;
    t.set_offset(offset);
    t.set_rotation(rotation);
    t.set_scaling_factor(scale);
    t.set_mirror(mirror);
    return t;
}

Xform random_xform(SplitMix &g, bool tilt)
{
    Xform x;
    x.offset   = Vec3d(real(g, -120, 120), real(g, -120, 120), real(g, -5, 5));
    x.rotation = Vec3d(tilt && g.below(2) ? real(g, -3.2, 3.2) : 0., tilt && g.below(2) ? real(g, -3.2, 3.2) : 0., g.below(4) ? real(g, -3.2, 3.2) : 0.);
    const double s = real(g, 0.5, 2.);
    x.scale        = g.below(2) ? Vec3d(s, s, s) : Vec3d(real(g, 0.5, 2.), real(g, 0.5, 2.), real(g, 0.5, 2.));
    x.mirror       = Vec3d(g.below(5) == 0 ? -1. : 1., g.below(8) == 0 ? -1. : 1., 1.);
    return x;
}

void apply_settings(PrintConfig &pc, PrintObjectConfig &oc, const Settings &s)
{
    for (const auto &kv : s) {
        bool any = false;
        if (pc.optptr(kv.first) != nullptr) {
            pc.set_deserialize_strict(kv.first, kv.second);
            any = true;
        }
        if (oc.optptr(kv.first) != nullptr) {
            oc.set_deserialize_strict(kv.first, kv.second);
            any = true;
        }
        if (!any)
            throw std::runtime_error("slice probe: no config holds the key " + kv.first);
    }
}

void put_its(Payload &p, const indexed_triangle_set &its)
{
    p.u64(its.vertices.size());
    for (const stl_vertex &v : its.vertices)
        p.f32(v.x()).f32(v.y()).f32(v.z());
    p.u64(its.indices.size());
    for (const stl_triangle_vertex_indices &f : its.indices)
        p.i32(f(0)).i32(f(1)).i32(f(2));
}

void put_matrix(Payload &p, const Transform3d &m)
{
    // Column by column, as Eigen stores it (parity/SOP1.md, "Values").
    for (int c = 0; c < 4; ++c)
        for (int r = 0; r < 4; ++r)
            p.f64(m.matrix()(r, c));
}

void put_xform(Payload &p, const Xform &x)
{
    for (const Vec3d *v : {&x.offset, &x.rotation, &x.scale, &x.mirror})
        p.f64(v->x()).f64(v->y()).f64(v->z());
}

void put_polygon(Payload &p, const Polygon &poly)
{
    p.u64(poly.points.size());
    for (const Point &pt : poly.points)
        p.i64(pt.x()).i64(pt.y());
}

void put_polygons(Payload &p, const Polygons &polys)
{
    p.u64(polys.size());
    for (const Polygon &poly : polys)
        put_polygon(p, poly);
}

void put_expolygons(Payload &p, const ExPolygons &ex)
{
    p.u64(ex.size());
    for (const ExPolygon &e : ex) {
        put_polygon(p, e.contour);
        put_polygons(p, e.holes);
    }
}

void put_params(Payload &p, const SlicingParameters &s)
{
    p.boolean(s.valid).u64(s.base_raft_layers).u64(s.interface_raft_layers);
    p.f64(s.base_raft_layer_height).f64(s.interface_raft_layer_height).f64(s.contact_raft_layer_height);
    p.f64(s.layer_height).f64(s.min_layer_height).f64(s.max_layer_height).f64(s.max_suport_layer_height);
    p.f64(s.first_print_layer_height).f64(s.first_object_layer_height).boolean(s.first_object_layer_bridging);
    p.boolean(s.zero_gap_interface_top).boolean(s.zero_gap_interface_bottom).boolean(s.zero_gap_interface_raft);
    p.f64(s.gap_raft_object).f64(s.gap_object_support).f64(s.gap_support_object);
    p.f64(s.raft_base_top_z).f64(s.raft_interface_top_z).f64(s.raft_contact_top_z);
    p.f64(s.object_print_z_min).f64(s.object_print_z_max).f64(s.object_print_z_uncompensated_max).f64(s.object_shrinkage_compensation_z);
}

void put_profile(Payload &p, const std::vector<double> &v)
{
    p.u64(v.size());
    for (double d : v)
        p.f64(d);
}

void put_floats(Payload &p, const std::vector<float> &v)
{
    p.u64(v.size());
    for (float f : v)
        p.f32(f);
}

void put_settings(Payload &p, const Settings &s)
{
    p.u64(s.size());
    for (const auto &kv : s)
        p.str(kv.first).str(kv.second);
}

void put_edges(Payload &p, const std::vector<Vec3i32> &e)
{
    p.u64(e.size());
    for (const Vec3i32 &v : e)
        p.i32(v(0)).i32(v(1)).i32(v(2));
}

} // namespace slice
} // namespace probe
