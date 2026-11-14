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
// The slice probe's records for front-2 (parity/sop1/slice.md): SLFE its_face_edge_ids in its three
// forms; SLMC the Model and ModelObject comparisons PrintApply makes, after Model's copy and move
// and a change made through the engine's own Model API, with is_mm_painted, is_fuzzy_skin_painted
// and raw_mesh; SLTS FacetsAnnotation's has_facets (TriangleSelector::has_facets), equals and
// get_facets_strict; SLSH Transformation's shrinkage compensation; SLBX the bed's excluded area.
// Written by orca-o3-probe-slice, 2026-11-13.
#include "common.hpp"

#include <algorithm>
#include <cmath>
#include <stdexcept>

#include <libslic3r/BrimEarsPoint.hpp>
#include <libslic3r/Model.hpp>
#include <libslic3r/TriangleSelector.hpp>

namespace probe {
namespace slice {

namespace {

using namespace Slic3r;

// ---- SLFE ----------------------------------------------------------------------------------

void record_edges(Sets &sets, const std::vector<Mesh> &meshes)
{
    SplitMix g(0x51CE1000u);
    for (const Mesh &m : meshes) {
        Payload p;
        p.u32(m.id);
        put_edges(p, its_face_edge_ids(m.its));
        std::vector<bool> mask(m.its.indices.size());
        const uint64_t    keep = 1 + g.below(4);
        for (size_t i = 0; i < mask.size(); ++i)
            mask[i] = g.below(4) < keep;
        p.u64(mask.size());
        for (bool b : mask)
            p.boolean(b);
        put_edges(p, its_face_edge_ids(m.its, mask));
        p.boolean(m.symmetric);
        if (m.symmetric) {
            std::vector<Vec3i32> nb = its_face_neighbors(m.its);
            put_edges(p, nb);
            for (bool unbound : {false, true}) {
                int                        n = -7;
                const std::vector<Vec3i32> e = its_face_edge_ids(m.its, nb, unbound, &n);
                put_edges(p, e);
                p.i32(n);
            }
        }
        add(sets, "SLFE", p, m.small && m.its.indices.size() <= 120);
    }
}

// ---- SLMC ----------------------------------------------------------------------------------

struct VolSpec
{
    uint32_t        mesh = 0;
    ModelVolumeType type = ModelVolumeType::MODEL_PART;
    Xform           x;
    unsigned        paint = 0;
    Settings        config;
};
struct ObjSpec
{
    std::vector<VolSpec>   vols;
    std::vector<Xform>     insts;
    Settings               config;
    std::vector<BrimPoint> brim;
};

// The volumes' meshes by address (a copied volume shares its mesh), for the descriptions.
using MeshIds = std::map<const TriangleMesh *, uint32_t>;

FacetsAnnotation &annotation(ModelVolume &v, int k)
{
    return k == 0 ? v.supported_facets : k == 1 ? v.seam_facets : k == 2 ? v.mmu_segmentation_facets : v.fuzzy_skin_facets;
}
const FacetsAnnotation &annotation(const ModelVolume &v, int k) { return annotation(const_cast<ModelVolume &>(v), k); }

void set_config(ModelConfig &c, const Settings &s)
{
    ConfigSubstitutionContext ctx(ForwardCompatibilitySubstitutionRule::Disable);
    for (const auto &kv : s)
        c.set_deserialize(kv.first, kv.second, ctx);
}

ModelVolume *add_volume(ModelObject &o, const VolSpec &v, const std::vector<Mesh> &meshes, MeshIds &ids)
{
    ModelVolume *vol = o.add_volume(TriangleMesh(meshes[v.mesh].its), v.type, false);
    ids[&vol->mesh()] = v.mesh;
    vol->set_transformation(v.x.make());
    for (int k = 0; k < 4; ++k)
        if (v.paint & (1u << k))
            annotation(*vol, k).set_triangle_from_string(0, "4");
    set_config(vol->config, v.config);
    return vol;
}

void build_object(Model &model, const ObjSpec &s, const std::vector<Mesh> &meshes, MeshIds &ids)
{
    ModelObject *o = model.add_object();
    for (const VolSpec &v : s.vols)
        add_volume(*o, v, meshes, ids);
    for (const Xform &x : s.insts)
        o->add_instance()->set_transformation(x.make());
    set_config(o->config, s.config);
    o->brim_points = s.brim;
}

// Object IDs and timestamps as small numbers, in the order they are first written.
struct Tokens
{
    std::map<size_t, uint32_t>   ids;
    std::map<uint64_t, uint32_t> stamps;
    uint32_t id(const ObjectID &i) { return ids.emplace(i.id, uint32_t(ids.size())).first->second; }
    uint32_t stamp(uint64_t t) { return stamps.emplace(t, uint32_t(stamps.size())).first->second; }
};

void put_keys(Payload &p, const ModelConfig &c)
{
    const t_config_option_keys keys = c.keys();
    p.u64(keys.size());
    for (const std::string &k : keys)
        p.str(k).str(c.opt_serialize(k));
}

void put_model(Payload &p, const Model &model, Tokens &t, const MeshIds &ids)
{
    p.u64(model.objects.size());
    for (const ModelObject *o : model.objects) {
        p.u32(t.id(o->id()));
        p.u64(o->volumes.size());
        for (const ModelVolume *v : o->volumes) {
            p.u32(t.id(v->id())).i32(int(v->type()));
            const auto it = ids.find(&v->mesh());
            if (it == ids.end())
                throw std::runtime_error("slice probe: a volume's mesh is not in the table");
            p.u32(it->second);
            put_matrix(p, v->get_matrix());
            for (int k = 0; k < 4; ++k)
                p.u32(t.stamp(annotation(*v, k).timestamp())).boolean(annotation(*v, k).empty());
            put_keys(p, v->config);
        }
        p.u64(o->instances.size());
        put_keys(p, o->config);
        p.u64(o->brim_points.size());
        for (const BrimPoint &b : o->brim_points)
            p.f32(b.pos.x()).f32(b.pos.y()).f32(b.pos.z()).f32(b.head_front_radius);
    }
}

std::vector<uint32_t> small_meshes(const std::vector<Mesh> &meshes, size_t max_faces)
{
    std::vector<uint32_t> out;
    for (const Mesh &m : meshes)
        if (m.small && m.its.indices.size() <= max_faces)
            out.push_back(m.id);
    return out;
}

ObjSpec random_object(SplitMix &g, const std::vector<uint32_t> &pool)
{
    static const ModelVolumeType types[] = {ModelVolumeType::MODEL_PART, ModelVolumeType::NEGATIVE_VOLUME, ModelVolumeType::PARAMETER_MODIFIER,
                                            ModelVolumeType::SUPPORT_BLOCKER, ModelVolumeType::SUPPORT_ENFORCER};
    ObjSpec s;
    const size_t nv = 1 + g.below(3);
    for (size_t i = 0; i < nv; ++i) {
        VolSpec v;
        v.mesh = pool[g.below(pool.size())];
        v.type = i == 0 ? ModelVolumeType::MODEL_PART : g.below(2) ? ModelVolumeType::MODEL_PART : types[g.below(5)];
        if (g.below(2))
            v.x = random_xform(g, true);
        v.paint = g.below(4) == 0 ? unsigned(g.below(16)) : 0u;
        if (g.below(6) == 0)
            v.config.emplace_back("extruder", "1");
        if (g.below(8) == 0)
            v.config.emplace_back("wall_loops", "3");
        s.vols.push_back(v);
    }
    const size_t ni = 1 + (g.below(3) == 0 ? 1 : 0);
    for (size_t i = 0; i < ni; ++i)
        s.insts.push_back(random_xform(g, false));
    switch (g.below(4)) {
    case 0: s.config.emplace_back("extruder", "1"); break;
    case 1: s.config.emplace_back("layer_height", "0.12"); break;
    default: break;
    }
    for (uint64_t k = g.below(4) == 0 ? 1 + g.below(2) : 0; k > 0; --k)
        s.brim.emplace_back(float(real(g, -10, 10)), float(real(g, -10, 10)), 0.f, float(real(g, 0.5, 3)));
    return s;
}

enum Change : uint8_t {
    NoChange,
    AppendClone,
    DeleteObject,
    ReplaceWithClone,
    SwapObjects,
    AddVolume,
    ClearVolumes,
    ChangeType,
    MoveVolume,
    NudgeVolume,
    PaintNoTouch,
    ResetSupports,
    ResetSeam,
    ResetMmu,
    ResetFuzzy,
    AddBrimPoint,
    AddInstance,
    ClearInstances,
    AddConfig,
    MovedCopy,
    ClearObjects,
    ClearMaterials,
    ChangeCount
};

void record_comparisons(Sets &sets, const std::vector<Mesh> &meshes)
{
    SplitMix                    g(0x51CE2000u);
    const std::vector<uint32_t> pool = small_meshes(meshes, 40);
    for (int i = 0; i < 132; ++i) {
        MeshIds ids;
        Model   a;
        const size_t no = 1 + g.below(3);
        for (size_t k = 0; k < no; ++k)
            build_object(a, random_object(g, pool), meshes, ids);
        Model         b(a);
        const uint8_t change = i < int(ChangeCount) ? uint8_t(i) : uint8_t(g.below(ChangeCount));
        const size_t  oi     = g.below(b.objects.size());
        ModelObject  *o      = b.objects[oi];
        const size_t  vi     = g.below(o->volumes.size());
        switch (change) {
        case AppendClone: b.add_object(*o); break;
        case DeleteObject: b.delete_object(oi); break;
        case ReplaceWithClone:
            b.add_object(*o);
            std::swap(b.objects[oi], b.objects.back());
            b.delete_object(b.objects.size() - 1);
            break;
        case SwapObjects: std::swap(b.objects[oi], b.objects[g.below(b.objects.size())]); break;
        case AddVolume: {
            VolSpec v;
            v.mesh = pool[g.below(pool.size())];
            v.type = g.below(2) ? ModelVolumeType::MODEL_PART : ModelVolumeType::PARAMETER_MODIFIER;
            add_volume(*o, v, meshes, ids);
            break;
        }
        case ClearVolumes: o->clear_volumes(); break;
        case ChangeType: o->volumes[vi]->set_type(o->volumes[vi]->type() == ModelVolumeType::MODEL_PART ? ModelVolumeType::NEGATIVE_VOLUME : ModelVolumeType::MODEL_PART); break;
        case MoveVolume: o->volumes[vi]->set_offset(o->volumes[vi]->get_offset() + Vec3d(1, 0, 0)); break;
        case NudgeVolume: o->volumes[vi]->set_offset(o->volumes[vi]->get_offset() + Vec3d(1e-13, 0, 0)); break;
        case PaintNoTouch: o->volumes[vi]->supported_facets.set_triangle_from_string(1, "8"); break;
        case ResetSupports: o->volumes[vi]->supported_facets.reset(); break;
        case ResetSeam: o->volumes[vi]->seam_facets.reset(); break;
        case ResetMmu: o->volumes[vi]->mmu_segmentation_facets.reset(); break;
        case ResetFuzzy: o->volumes[vi]->fuzzy_skin_facets.reset(); break;
        case AddBrimPoint: o->brim_points.emplace_back(1.f, 2.f, 0.f, 1.5f); break;
        case AddInstance: o->add_instance()->set_transformation(random_xform(g, false).make()); break;
        case ClearInstances: o->clear_instances(); break;
        case AddConfig: {
            ConfigSubstitutionContext ctx(ForwardCompatibilitySubstitutionRule::Disable);
            o->config.set_deserialize("sparse_infill_density", "35%", ctx);
            break;
        }
        case MovedCopy: {
            Model c(a);
            b = Model(std::move(c));
            break;
        }
        case ClearObjects: b.clear_objects(); break;
        case ClearMaterials: b.clear_materials(); break;
        default: break;
        }
        Payload p;
        Tokens  t;
        p.u8(change);
        put_model(p, a, t, ids);
        put_model(p, b, t, ids);
        p.boolean(model_object_list_equal(a, b)).boolean(model_object_list_extended(a, b));
        const size_t n = std::min(a.objects.size(), b.objects.size());
        p.u64(n);
        for (size_t k = 0; k < n; ++k) {
            const ModelObject &oa = *a.objects[k];
            const ModelObject &ob = *b.objects[k];
            const bool parts = model_volume_list_changed(oa, ob, ModelVolumeType::MODEL_PART);
            p.boolean(parts);
            p.boolean(model_volume_list_changed(oa, ob, {ModelVolumeType::MODEL_PART, ModelVolumeType::NEGATIVE_VOLUME, ModelVolumeType::PARAMETER_MODIFIER}));
            p.boolean(model_volume_list_changed(oa, ob, ModelVolumeType::SUPPORT_BLOCKER));
            p.boolean(model_volume_list_changed(oa, ob, ModelVolumeType::SUPPORT_ENFORCER));
            // PrintApply asks the painting questions only of objects whose parts did not change.
            if (!parts) {
                p.boolean(model_custom_supports_data_changed(oa, ob)).boolean(model_custom_seam_data_changed(oa, ob));
                p.boolean(model_mmu_segmentation_data_changed(oa, ob)).boolean(model_fuzzy_skin_data_changed(oa, ob));
            }
            p.boolean(model_brim_points_data_changed(oa, ob));
        }
        p.boolean(model_has_multi_part_objects(a)).boolean(model_has_multi_part_objects(b));
        p.boolean(model_has_advanced_features(a)).boolean(model_has_advanced_features(b));
        p.boolean(b.is_fdm_support_painted()).boolean(b.is_seam_painted()).boolean(b.is_mm_painted()).boolean(b.is_fuzzy_skin_painted());
        p.u64(b.objects.size());
        for (const ModelObject *ob : b.objects) {
            p.boolean(ob->is_fdm_support_painted()).boolean(ob->is_seam_painted()).boolean(ob->is_mm_painted()).boolean(ob->is_fuzzy_skin_painted());
            put_its(p, ob->raw_mesh().its);
        }
        add(sets, "SLMC", p, true);
    }
}

// ---- SLTS ----------------------------------------------------------------------------------

// One triangle's painting as TriangleSelector serializes it, nibble by nibble (a split node: the
// number of split sides and the special side, then its children; a leaf: its state, past 2 in
// a second nibble), at most two levels deep.
void tree(SplitMix &g, int depth, std::vector<int> &nibbles)
{
    if (depth < 2 && g.below(3) == 0) {
        const int sides = 1 + int(g.below(3));
        nibbles.push_back(sides | (int(g.below(3)) << 2));
        for (int c = 0; c < sides + 1; ++c)
            tree(g, depth + 1, nibbles);
        return;
    }
    const uint64_t r     = g.below(10);
    const int      state = r < 4 ? 0 : r < 8 ? 1 + int(g.below(2)) : 3 + int(g.below(13));
    if (state < 3)
        nibbles.push_back(state << 2);
    else {
        nibbles.push_back(0b1100);
        nibbles.push_back(state - 3);
    }
}

// set_triangle_from_string reads the string from its end: the first nibble is the last character.
std::string tree_string(const std::vector<int> &nibbles)
{
    std::string s;
    for (auto it = nibbles.rbegin(); it != nibbles.rend(); ++it)
        s.push_back("0123456789ABCDEF"[*it]);
    return s;
}

void record_facets(Sets &sets, const std::vector<Mesh> &meshes)
{
    SplitMix                    g(0x51CE3000u);
    const std::vector<uint32_t> pool = small_meshes(meshes, 40);
    Model                       model;
    ModelObject                *object = model.add_object();
    for (int i = 0; i < 60; ++i) {
        const Mesh  &m   = meshes[i < 4 ? std::vector<uint32_t>{0, 15, 17, 2}[i] : pool[g.below(pool.size())]];
        ModelVolume *vol = object->add_volume(TriangleMesh(m.its), ModelVolumeType::MODEL_PART, false);
        std::vector<std::pair<int, std::string>> tris;
        if (i >= 4 && g.below(5) != 0) {
            int id = int(g.below(3));
            while (id < int(m.its.indices.size()) && tris.size() < 4) {
                std::vector<int> nibbles;
                tree(g, 0, nibbles);
                tris.emplace_back(id, tree_string(nibbles));
                id += 1 + int(g.below(4));
            }
        }
        for (const auto &tr : tris)
            vol->supported_facets.set_triangle_from_string(tr.first, tr.second);
        Payload p;
        p.u32(m.id).u64(tris.size());
        for (const auto &tr : tris)
            p.i32(tr.first).str(tr.second);
        const FacetsAnnotation &f = vol->supported_facets;
        p.boolean(f.empty());
        for (int state = 0; state <= int(EnforcerBlockerType::ExtruderMax); ++state)
            p.boolean(f.has_facets(*vol, EnforcerBlockerType(state)));
        for (int type = 1; type <= 3; ++type)
            put_its(p, f.get_facets_strict(*vol, EnforcerBlockerType(type)));
        // equals: the same strings on another annotation, then the last one changed.
        ModelVolume *other = object->add_volume(TriangleMesh(m.its), ModelVolumeType::MODEL_PART, false);
        for (const auto &tr : tris)
            other->seam_facets.set_triangle_from_string(tr.first, tr.second);
        p.boolean(f.equals(other->seam_facets));
        if (!tris.empty()) {
            std::string last = tris.back().second;
            last.back()      = last.back() == '4' ? '8' : '4';
            other->mmu_segmentation_facets.reset();
            for (size_t k = 0; k + 1 < tris.size(); ++k)
                other->mmu_segmentation_facets.set_triangle_from_string(tris[k].first, tris[k].second);
            other->mmu_segmentation_facets.set_triangle_from_string(tris.back().first, last);
            p.str(last).boolean(f.equals(other->mmu_segmentation_facets));
        }
        add(sets, "SLTS", p, true);
        object->clear_volumes();
    }
}

// ---- SLSH ----------------------------------------------------------------------------------

void record_shrinkage(Sets &sets)
{
    SplitMix g(0x51CE4000u);
    for (int i = 0; i < 80; ++i) {
        const Xform x = i == 0 ? Xform{} : random_xform(g, i % 3 == 0);
        Vec3d       s(1, 1, 1);
        if (i % 4 != 1) {
            const double xy = 100. / real(g, 97, 103);
            s               = g.below(2) ? Vec3d(xy, xy, 100. / real(g, 97, 103)) : Vec3d(real(g, 0.95, 1.05), real(g, 0.95, 1.05), real(g, 0.95, 1.05));
        }
        Payload p;
        put_xform(p, x);
        p.f64(s.x()).f64(s.y()).f64(s.z());
        put_matrix(p, x.make().get_matrix_with_applied_shrinkage_compensation(s));
        add(sets, "SLSH", p, true);
    }
}

// ---- SLBX ----------------------------------------------------------------------------------

void put_points(Payload &p, const Pointfs &pts)
{
    p.u64(pts.size());
    for (const Vec2d &v : pts)
        p.f64(v.x()).f64(v.y());
}

void record_bed(Sets &sets)
{
    SplitMix     g(0x51CE5000u);
    const Pointfs square = {{0, 0}, {256, 0}, {256, 256}, {0, 256}};
    for (int i = 0; i < 40; ++i) {
        Pointfs              bed = square, excl;
        std::vector<Pointfs> areas;
        switch (i < 8 ? i : int(g.below(8))) {
        case 0: excl = {{0, 0}, {18, 0}, {18, 28}, {0, 28}}; break;
        case 1: break;
        case 2: excl = {{0, 0}, {0, 28}, {18, 28}, {18, 0}}; break;
        case 3: excl = {{300, 300}, {320, 300}, {320, 320}}; break;
        case 4: excl = {{120, -10}, {136, -10}, {136, 270}, {120, 270}}; break;
        case 5: excl = {{-10, -10}, {300, -10}, {300, 300}, {-10, 300}}; break;
        case 6: {
            bed.clear();
            for (int k = 0; k < 24; ++k)
                bed.emplace_back(100 + 100 * std::cos(2 * PI * k / 24), 100 + 100 * std::sin(2 * PI * k / 24));
            excl = {{90, 90}, {110, 90}, {110, 110}, {90, 110}};
            break;
        }
        case 7: {
            areas = {{{0, 0}, {200, 0}, {200, 256}, {0, 256}}, {{56, 0}, {256, 0}, {256, 256}, {56, 256}}};
            excl  = {{0, 0}, {30, 0}, {30, 30}, {0, 30}};
            break;
        }
        }
        if (i >= 8) {
            const double w = real(g, 150, 400), d = real(g, 150, 400);
            bed            = {{0, 0}, {w, 0}, {w, d}, {0, d}};
            if (g.below(2)) {
                const double x = real(g, -20, w), y = real(g, -20, d);
                excl           = {{x, y}, {x + real(g, 1, 60), y}, {x + real(g, 1, 60), y + real(g, 1, 60)}, {x, y + real(g, 1, 60)}};
                if (g.below(3) == 0)
                    std::reverse(excl.begin(), excl.end());
            }
            if (g.below(4) == 0)
                areas = {{{0, 0}, {w * 0.8, 0}, {w * 0.8, d}, {0, d}}, {{w * 0.2, 0}, {w, 0}, {w, d}, {w * 0.2, d}}};
        }
        PrintConfig pc;
        pc.printable_area.values          = bed;
        pc.bed_exclude_area.values        = excl;
        pc.extruder_printable_area.values = areas;
        Payload p;
        put_points(p, bed);
        put_points(p, excl);
        p.u64(areas.size());
        for (const Pointfs &a : areas)
            put_points(p, a);
        put_polygons(p, get_bed_excluded_area(pc));
        put_polygon(p, get_bed_shape_with_excluded_area(pc, false));
        put_polygon(p, get_bed_shape_with_excluded_area(pc, true));
        add(sets, "SLBX", p, true);
    }
}

} // namespace

void record_front(Sets &sets, const std::vector<Mesh> &meshes)
{
    record_edges(sets, meshes);
    record_comparisons(sets, meshes);
    record_facets(sets, meshes);
    record_shrinkage(sets);
    record_bed(sets);
}

} // namespace slice
} // namespace probe
