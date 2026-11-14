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
// The slice probe's Slicing and SlicingAdaptive records (parity/sop1/slice.md): SLPR
// SlicingParameters::create_from_config with equal_layering and Slicing's nozzle limits; SLRG
// layer_height_profile_from_ranges; SLAD SlicingAdaptive's faces and next heights; SLAP
// layer_height_profile_adaptive; SLSM smooth_height_profile; SLAJ adjust_layer_height_profile;
// SLOL generate_object_layers (with the precise Z height's alignment) and check_object_layers_fixed.
// Settings range over what domain A sets (layer heights, the first layer, nozzles and their limits,
// shrinkage) and past it (rafts, supports and their gaps), so every branch of create_from_config
// is reached. Inputs avoid what the C++ cannot survive without its asserts: an empty nozzle list, a
// profile height of 0 (generate_object_layers would never end), and an edit band that reaches z 0
// on a profile whose first layer is not fixed (adjust_layer_height_profile's index would wrap).
// Written by orca-o3-probe-slice, 2026-11-13.
#include "common.hpp"

#include <algorithm>
#include <cmath>
#include <iterator>

#include <libslic3r/Model.hpp>
#include <libslic3r/SlicingAdaptive.hpp>

namespace probe {
namespace slice {

namespace {

using namespace Slic3r;

std::string list(SplitMix &g, size_t n, const char *const *values, size_t count)
{
    std::string s;
    for (size_t i = 0; i < n; ++i)
        s += (i ? "," : "") + std::string(values[g.below(count)]);
    return s;
}

Settings random_settings(SplitMix &g, size_t &nozzles)
{
    static const char *const lh[]  = {"0.08", "0.1", "0.12", "0.15", "0.16", "0.2", "0.24", "0.28", "0.3", "0.32"};
    static const char *const ilh[] = {"0.2", "0.25", "0.3", "0.12", "0", "0.35"};
    static const char *const nd[]  = {"0.4", "0.6", "0.2", "0.8", "0.4"};
    static const char *const mn[]  = {"0", "0.005", "0.07", "0.08", "0.1"};
    static const char *const mx[]  = {"0", "0.2", "0.28", "0.32", "0.5"};
    static const char *const zt[]  = {"0", "0.1", "0.2", "0.15", "0.25"};
    static const char *const zb[]  = {"0", "0.1", "0.2"};
    static const char *const rc[]  = {"0", "0.1", "0.2", "0.25"};
    nozzles = 1 + (g.below(4) == 0 ? size_t(g.below(3)) : 0);
    Settings s;
    s.emplace_back("layer_height", pick(g, lh));
    s.emplace_back("initial_layer_print_height", pick(g, ilh));
    s.emplace_back("nozzle_diameter", list(g, nozzles, nd, std::size(nd)));
    s.emplace_back("min_layer_height", list(g, nozzles, mn, std::size(mn)));
    s.emplace_back("max_layer_height", list(g, nozzles, mx, std::size(mx)));
    if (g.below(3) == 0) {
        s.emplace_back("support_filament", std::to_string(g.below(nozzles + 1)));
        s.emplace_back("support_interface_filament", std::to_string(g.below(nozzles + 1)));
        s.emplace_back("enable_support", g.below(2) ? "1" : "0");
        s.emplace_back("enforce_support_layers", std::to_string(g.below(3) == 0 ? 1 + g.below(3) : 0));
        s.emplace_back("support_top_z_distance", pick(g, zt));
        s.emplace_back("support_bottom_z_distance", pick(g, zb));
        s.emplace_back("raft_contact_distance", pick(g, rc));
        s.emplace_back("support_interface_top_layers", std::to_string(g.below(4)));
        s.emplace_back("support_interface_bottom_layers", std::to_string(int(g.below(4)) - 1));
        s.emplace_back("independent_support_layer_height", g.below(2) ? "1" : "0");
    }
    if (g.below(4) == 0)
        s.emplace_back("raft_layers", std::to_string(1 + g.below(5)));
    return s;
}

SlicingParameters params_of(const Settings &s, double height, const std::vector<unsigned int> &extruders, const Vec3d &shrink)
{
    PrintConfig       pc;
    PrintObjectConfig oc;
    apply_settings(pc, oc, s);
    return SlicingParameters::create_from_config(pc, oc, height, extruders, shrink);
}

Vec3d random_shrink(SplitMix &g)
{
    if (g.below(5) < 3)
        return Vec3d(1, 1, 1);
    const double xy = 100. / real(g, 97, 103);
    return Vec3d(xy, xy, 100. / real(g, 97, 103));
}

// A domain-like setting for the profile records: settings, a height and a shrinkage.
SlicingParameters random_params(SplitMix &g, double height)
{
    size_t         n = 0;
    const Settings s = random_settings(g, n);
    return params_of(s, height, {1}, random_shrink(g));
}

void put_ranges(Payload &p, const t_layer_config_ranges &ranges)
{
    p.u64(ranges.size());
    for (const auto &r : ranges)
        p.f64(r.first.first).f64(r.first.second).f64(r.second.opt_float("layer_height"));
}

t_layer_config_ranges random_ranges(SplitMix &g, double height, size_t max_count)
{
    static const double hs[] = {0.1, 0.15, 0.2, 0.3, 0.05, 0.12};
    t_layer_config_ranges out;
    const size_t          n = size_t(g.below(max_count + 1));
    for (size_t i = 0; i < n; ++i) {
        const double lo = g.below(6) == 0 ? 0. : real(g, 0, height);
        double       hi = lo + (g.below(6) == 0 ? real(g, 0, 2e-7) : real(g, 0.1, std::max(0.2, height * 0.6)));
        if (g.below(5) == 0)
            hi = height + real(g, 0, 3);
        out[{lo, hi}].set("layer_height", pick(g, hs));
    }
    return out;
}

// ---- objects -------------------------------------------------------------------------------

// A model object of the table's meshes: volumes (the first a part; others parts or modifiers,
// sometimes moved), one instance (rotated, scaled, sometimes mirrored) lifted so that its lowest
// vertex is at z 0, and layer height ranges.
struct ObjectCase
{
    Model        model;
    ModelObject *object = nullptr;
    double       height = 0;
    std::vector<uint32_t> meshes;
};

void build_object(ObjectCase &c, SplitMix &g, const std::vector<Mesh> &meshes, const std::vector<uint32_t> &pool, double max_scale)
{
    c.object        = c.model.add_object();
    const size_t nv = 1 + (g.below(3) == 0 ? 1 : 0);
    for (size_t i = 0; i < nv; ++i) {
        const uint32_t  id   = pool[g.below(pool.size())];
        ModelVolumeType type = i == 0 || g.below(2) ? ModelVolumeType::MODEL_PART : ModelVolumeType::PARAMETER_MODIFIER;
        ModelVolume    *v    = c.object->add_volume(TriangleMesh(meshes[id].its), type, false);
        c.meshes.push_back(id);
        if (i > 0) {
            Xform x;
            x.offset = Vec3d(real(g, -5, 5), real(g, -5, 5), real(g, 0, 5));
            v->set_transformation(x.make());
        }
    }
    Xform x;
    x.rotation    = Vec3d(g.below(4) == 0 ? real(g, -3.2, 3.2) : 0., g.below(4) == 0 ? real(g, -3.2, 3.2) : 0., real(g, -3.2, 3.2));
    const double s = real(g, 0.3, max_scale);
    x.scale       = Vec3d(s, s, g.below(3) == 0 ? real(g, 0.3, max_scale) : s);
    x.mirror      = Vec3d(g.below(5) == 0 ? -1. : 1., 1., 1.);
    x.offset      = Vec3d(real(g, -50, 50), real(g, -50, 50), 0.);
    ModelInstance *inst = c.object->add_instance();
    inst->set_transformation(x.make());
    TriangleMesh m = c.object->raw_mesh();
    m.transform(inst->get_matrix(), inst->is_left_handed());
    const BoundingBoxf3 bb = m.bounding_box();
    inst->set_offset(Z, -bb.min.z());
    c.height = bb.max.z() - bb.min.z();
}

void put_object(Payload &p, const ObjectCase &c)
{
    p.u64(c.object->volumes.size());
    for (size_t i = 0; i < c.object->volumes.size(); ++i) {
        const ModelVolume *v = c.object->volumes[i];
        p.u32(c.meshes[i]).i32(int(v->type()));
        put_matrix(p, v->get_matrix());
    }
    const ModelInstance *inst = c.object->instances.front();
    put_matrix(p, inst->get_matrix());
    p.boolean(inst->is_left_handed());
    put_ranges(p, c.object->layer_config_ranges);
}

bool small_object(const ObjectCase &c, const std::vector<Mesh> &meshes)
{
    for (uint32_t id : c.meshes)
        if (!meshes[id].small)
            return false;
    return true;
}

std::vector<uint32_t> object_pool(const std::vector<Mesh> &meshes, size_t max_faces)
{
    std::vector<uint32_t> out;
    for (const Mesh &m : meshes)
        if (m.its.indices.size() <= max_faces)
            out.push_back(m.id);
    return out;
}

struct Adaptive : SlicingAdaptive
{
    const std::vector<FaceZ> &faces() const { return m_faces; }
};

// ---- SLPR ----------------------------------------------------------------------------------

void record_params(Sets &sets)
{
    SplitMix g(0x51CE9000u);
    for (int i = 0; i < 160; ++i) {
        size_t   n = 0;
        Settings s = random_settings(g, n);
        double   height = g.below(4) == 0 ? double(1 + g.below(40)) : real(g, 0.05, 80);
        if (i == 0)
            height = 0.1;
        std::vector<unsigned int> ex;
        switch (g.below(4)) {
        case 0: break;
        case 1: ex = {1}; break;
        case 2:
            for (unsigned k = 1; k <= n; ++k)
                ex.push_back(k);
            break;
        default: ex = {0}; break;
        }
        const Vec3d             shrink = random_shrink(g);
        const SlicingParameters a      = params_of(s, height, ex, shrink);
        static const char *const keys[][2] = {{"layer_height", "0.2"},          {"raft_layers", "2"},          {"initial_layer_print_height", "0.3"},
                                              {"support_top_z_distance", "0"}, {"max_layer_height", "0.24"}, {"min_layer_height", "0.1"},
                                              {"enable_support", "1"},         {"layer_height", "0.1"}};
        const auto              &kv      = keys[g.below(std::size(keys))];
        Settings                 variant = s;
        if (std::string(kv[0]) == "max_layer_height" || std::string(kv[0]) == "min_layer_height") {
            std::string v = kv[1];
            for (size_t k = 1; k < n; ++k)
                v += std::string(",") + kv[1];
            variant.emplace_back(kv[0], v);
        } else
            variant.emplace_back(kv[0], kv[1]);
        const SlicingParameters b = params_of(variant, height, ex, shrink);
        Payload p;
        put_settings(p, s);
        p.f64(height).u64(ex.size());
        for (unsigned e : ex)
            p.u32(e);
        p.f64(shrink.x()).f64(shrink.y()).f64(shrink.z());
        put_params(p, a);
        p.str(variant.back().first).str(variant.back().second);
        put_params(p, b);
        p.boolean(equal_layering(a, b));
        DynamicPrintConfig dyn;
        for (const auto &kv2 : s)
            if (kv2.first == "nozzle_diameter" || kv2.first == "min_layer_height" || kv2.first == "max_layer_height")
                dyn.set_deserialize_strict(kv2.first, kv2.second);
        p.u64(n + 2);
        for (int idx = 0; idx < int(n) + 2; ++idx)
            p.f64(Slicing::min_layer_height_from_nozzle(dyn, idx)).f64(Slicing::max_layer_height_from_nozzle(dyn, idx));
        add(sets, "SLPR", p, true);
    }
}

// ---- SLRG ----------------------------------------------------------------------------------

void record_ranges(Sets &sets)
{
    SplitMix g(0x51CEA000u);
    for (int i = 0; i < 100; ++i) {
        const double                height = i < 40 ? real(g, 0.3, 8) : real(g, 0.3, 60);
        const SlicingParameters     sp     = random_params(g, height);
        const t_layer_config_ranges ranges = random_ranges(g, height, 5);
        Payload p;
        put_params(p, sp);
        put_ranges(p, ranges);
        put_profile(p, layer_height_profile_from_ranges(sp, ranges));
        add(sets, "SLRG", p, true);
    }
}

// ---- SLAD, SLAP ----------------------------------------------------------------------------

void record_adaptive(Sets &sets, const std::vector<Mesh> &meshes)
{
    SplitMix                    g(0x51CEB000u);
    const std::vector<uint32_t> pool = object_pool(meshes, 400);
    for (int i = 0; i < 48; ++i) {
        ObjectCase c;
        build_object(c, g, meshes, pool, i < 20 ? 0.6 : 2.);
        const SlicingParameters sp = random_params(g, c.height);
        Adaptive                as;
        as.set_slicing_parameters(sp);
        as.prepare(*c.object);
        Payload p;
        put_object(p, c);
        put_params(p, sp);
        p.u64(as.faces().size());
        for (const SlicingAdaptive::FaceZ &f : as.faces())
            p.f32(f.z_span.first).f32(f.z_span.second).f32(f.n_cos).f32(f.n_sin);
        static const float qs[] = {0.f, 0.25f, 0.5f, 0.75f, 1.f};
        const float        q    = g.below(3) == 0 ? float(real(g, 0, 1)) : pick(g, qs);
        std::vector<std::pair<std::pair<float, size_t>, std::pair<float, size_t>>> calls;
        size_t current = 0;
        double print_z = sp.first_object_layer_height;
        while (print_z + EPSILON < sp.object_print_z_uncompensated_height() && calls.size() < 48) {
            const size_t in = current;
            const float  h  = as.next_layer_height(float(print_z), q, current);
            calls.push_back({{float(print_z), in}, {h, current}});
            print_z += h;
        }
        // Then heights out of order, from the first facet.
        for (int k = 0; k < 4; ++k) {
            size_t      cf = 0;
            const float z  = float(real(g, -0.5, c.height + 0.5));
            const float h  = as.next_layer_height(z, q, cf);
            calls.push_back({{z, 0}, {h, cf}});
        }
        p.f32(q).u64(calls.size());
        for (const auto &cl : calls)
            p.f32(cl.first.first).u64(cl.first.second).f32(cl.second.first).u64(cl.second.second);
        p.u64(6);
        for (int k = 0; k < 6; ++k) {
            const float z = k == 0 ? 0.f : float(real(g, -0.5, c.height + 0.5));
            p.f32(z).f32(as.horizontal_facet_distance(z));
        }
        add(sets, "SLAD", p, small_object(c, meshes));
    }
    for (int i = 0; i < 72; ++i) {
        ObjectCase c;
        build_object(c, g, meshes, pool, i < 30 ? 0.5 : 2.);
        if (g.below(3) == 0)
            c.object->layer_config_ranges = random_ranges(g, c.height, 3);
        const SlicingParameters sp = random_params(g, c.height);
        static const float      qs[] = {0.f, 0.25f, 0.5f, 0.75f, 1.f};
        const float             q    = g.below(3) == 0 ? float(real(g, 0, 1)) : pick(g, qs);
        Payload p;
        put_object(p, c);
        put_params(p, sp);
        p.f32(q);
        put_profile(p, layer_height_profile_adaptive(sp, *c.object, q));
        add(sets, "SLAP", p, small_object(c, meshes));
    }
}

// ---- profiles for SLSM, SLAJ, SLOL ----------------------------------------------------------

struct ProfileCase
{
    SlicingParameters     sp;
    std::vector<double>   profile;
    t_layer_config_ranges ranges;
};

// A profile from ranges (kind 0), from the adaptive algorithm on an object (1), fixed by hand in
// four or eight values (2), or random steps between the limits (3).
ProfileCase profile_case(SplitMix &g, const std::vector<Mesh> &meshes, const std::vector<uint32_t> &pool, int kind, double max_height)
{
    ProfileCase c;
    if (kind == 1) {
        ObjectCase o;
        build_object(o, g, meshes, pool, max_height / 20.);
        if (g.below(3) == 0)
            o.object->layer_config_ranges = random_ranges(g, o.height, 2);
        c.sp      = random_params(g, o.height);
        c.ranges  = o.object->layer_config_ranges;
        c.profile = layer_height_profile_adaptive(c.sp, *o.object, float(real(g, 0, 1)));
        return c;
    }
    const double height = real(g, 0.3, max_height);
    c.sp                = random_params(g, height);
    if (kind == 0) {
        c.ranges  = random_ranges(g, height, 3);
        c.profile = layer_height_profile_from_ranges(c.sp, c.ranges);
    } else if (kind == 2) {
        const double h0 = c.sp.first_object_layer_height, h = c.sp.layer_height;
        const double top = c.sp.object_print_z_uncompensated_height();
        // The z values rise, or generate_object_layers would extrapolate to a height of 0 or less.
        c.profile = g.below(2) || top <= h0 + 1e-6 ? std::vector<double>{0, h, top, h} : std::vector<double>{0, h0, h0, h0, h0, h, top, h};
        if (g.below(4) == 0)
            c.profile[c.profile.size() - 1] *= 1.5;
    } else {
        const double top = c.sp.object_print_z_uncompensated_height();
        double       z   = 0;
        while (z < top - 1e-9) {
            c.profile.push_back(z);
            c.profile.push_back(real(g, c.sp.min_layer_height, std::min(c.sp.max_layer_height, 0.5)));
            z = std::min(top, z + real(g, 0.2, std::max(0.4, top / 3)));
        }
        c.profile.push_back(top);
        c.profile.push_back(c.profile[c.profile.size() - 2]);
        if (c.profile.size() < 4)
            c.profile = {0, c.sp.layer_height, top, c.sp.layer_height};
    }
    return c;
}

void record_smooth(Sets &sets, const std::vector<Mesh> &meshes)
{
    SplitMix                    g(0x51CEC000u);
    const std::vector<uint32_t> pool = object_pool(meshes, 400);
    for (int i = 0; i < 72; ++i) {
        const ProfileCase c      = profile_case(g, meshes, pool, int(g.below(4)), i < 30 ? 4. : 30.);
        static const unsigned rs[] = {0, 1, 2, 5, 10};
        const unsigned        r    = pick(g, rs);
        const bool            keep = g.below(2) != 0;
        Payload p;
        put_params(p, c.sp);
        put_profile(p, c.profile);
        p.u32(r).boolean(keep);
        put_profile(p, smooth_height_profile(c.profile, c.sp, HeightProfileSmoothingParams(r, keep)));
        add(sets, "SLSM", p, true);
    }
}

void record_adjust(Sets &sets, const std::vector<Mesh> &meshes)
{
    SplitMix                    g(0x51CED000u);
    const std::vector<uint32_t> pool = object_pool(meshes, 400);
    Model                       model;
    ModelObject                *object = model.add_object();
    for (int i = 0; i < 80; ++i) {
        const ProfileCase c   = profile_case(g, meshes, pool, int(g.below(2)), i < 32 ? 4. : 30.);
        const double      top = c.sp.object_print_z_uncompensated_height();
        const double      band = real(g, 0.3, 6);
        const double      lo   = c.sp.first_object_layer_height_fixed() ? c.sp.first_object_layer_height : 0.5 * band + 1e-3;
        double            z    = real(g, lo, std::max(lo, top));
        if (g.below(8) == 0)
            z = g.below(2) ? top + 1 : (c.sp.first_object_layer_height_fixed() ? 0.5 * c.sp.first_object_layer_height : -1.);
        const double   delta  = real(g, 0.005, 0.1);
        const uint32_t action = uint32_t(g.below(4));
        object->layer_config_ranges = c.ranges;
        std::vector<double> profile = c.profile;
        adjust_layer_height_profile(*object, c.sp, profile, z, delta, band, LayerHeightEditActionType(action));
        Payload p;
        put_params(p, c.sp);
        put_ranges(p, c.ranges);
        put_profile(p, c.profile);
        p.f64(z).f64(delta).f64(band).u32(action);
        put_profile(p, profile);
        add(sets, "SLAJ", p, true);
    }
}

void record_object_layers(Sets &sets, const std::vector<Mesh> &meshes)
{
    SplitMix                    g(0x51CEE000u);
    const std::vector<uint32_t> pool = object_pool(meshes, 400);
    for (int i = 0; i < 100; ++i) {
        const ProfileCase c       = profile_case(g, meshes, pool, int(g.below(4)), i < 40 ? 4. : 25.);
        const bool        precise = g.below(2) != 0;
        Payload p;
        put_params(p, c.sp);
        put_profile(p, c.profile);
        p.boolean(precise);
        put_profile(p, generate_object_layers(c.sp, c.profile, precise));
        p.boolean(check_object_layers_fixed(c.sp, c.profile));
        add(sets, "SLOL", p, true);
    }
}

} // namespace

void record_layers(Sets &sets, const std::vector<Mesh> &meshes)
{
    record_params(sets);
    record_ranges(sets);
    record_adaptive(sets, meshes);
    record_smooth(sets, meshes);
    record_adjust(sets, meshes);
    record_object_layers(sets, meshes);
}

} // namespace slice
} // namespace probe
