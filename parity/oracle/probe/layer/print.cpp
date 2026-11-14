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
// layer.sop1's print records of the piece layer-3 (orca-layer; parity/sop1/layer.md), on prints
// the probe builds (Model of generated boxes and cylinders, Print::apply of a configuration):
// LYPC the print case and what apply leaves, LYPR its PrintRegions, LYSF the support flows, LYPH
// PrintBase's object placeholders and output file name, LYPT Print's filament and extruder
// accessors, LYLA layers added to its object with regions of their own (Layer and LayerRegion).
// The records after an LYPC belong to it (its case number). Recorded by orca-o3-probe-layer,
// 2026-11-13.
#include "common.hpp"

#include <deque>
#include <memory>

#include "libslic3r/ClipperUtils.hpp"
#include "libslic3r/Layer.hpp"
#include "libslic3r/Model.hpp"
#include "libslic3r/Print.hpp"
#include "libslic3r/PrintConfig.hpp"
#include "libslic3r/ShortestPath.hpp"
#include "libslic3r/TriangleMesh.hpp"

namespace probe {
namespace layer {

namespace {

// PrintBase's protected update_object_placeholders and output_filename, reached by making the
// probe's print an Access, a Print with no members of its own (no copy of the engine's code).
struct Access : Print
{
    void placeholders(DynamicConfig &c, const std::string &ext) const { this->update_object_placeholders(c, ext); }
    std::string filename(const std::string &format, const std::string &ext, const std::string &base) const
    {
        return this->PrintBase::output_filename(format, ext, base, nullptr);
    }
};

using KeyValues = std::vector<std::pair<std::string, std::string>>;

struct InstSpec
{
    double ox, oy, sx, sy, sz;
    bool   printable;
};
struct ObjSpec
{
    std::string           name, input_file;
    int                   mesh; // 0 a box x by y by z, 1 a cylinder of radius x and height z
    double                x, y, z;
    std::vector<InstSpec> instances;
    KeyValues             config;
};
struct PrintCase
{
    std::string          label;
    bool                 domain_a;
    KeyValues            config;
    std::vector<ObjSpec> objects;
};

void put_kv(Payload &p, const KeyValues &kv)
{
    p.u64(kv.size());
    for (const auto &e : kv)
        p.str(e.first).str(e.second);
}

std::vector<PrintCase> print_cases()
{
    const InstSpec one{100, 100, 1, 1, 1, true};
    const ObjSpec  cube{"cube", "cube.stl", 0, 20, 20, 10, {one}, {}};
    std::vector<PrintCase> c;
    c.push_back({"defaults", true, {}, {cube}});
    c.push_back({"widths",
                 true,
                 {{"line_width", "0.45"},
                  {"outer_wall_line_width", "0.42"},
                  {"inner_wall_line_width", "0.45"},
                  {"sparse_infill_line_width", "120%"},
                  {"internal_solid_infill_line_width", "0"},
                  {"top_surface_line_width", "0.4"},
                  {"initial_layer_line_width", "0.5"},
                  {"initial_layer_print_height", "0.3"},
                  {"layer_height", "0.2"},
                  {"bridge_flow", "0.9"},
                  {"bridge_line_width", "0"},
                  {"support_line_width", "0"},
                  {"support_filament", "0"},
                  {"support_interface_filament", "0"}},
                 {cube, {"cylinder", "cyl.stl", 1, 8, 8, 15, {{60, 60, 1, 1, 1, true}, {140, 60, 0.5, 0.5, 0.5, true}}, {}}}});
    c.push_back({"percent widths, 0.6 nozzle",
                 true,
                 {{"nozzle_diameter", "0.6"},
                  {"line_width", "110%"},
                  {"outer_wall_line_width", "0"},
                  {"inner_wall_line_width", "105%"},
                  {"sparse_infill_line_width", "0"},
                  {"internal_solid_infill_line_width", "0%"},
                  {"top_surface_line_width", "100%"},
                  {"initial_layer_line_width", "0"},
                  {"bridge_line_width", "120%"},
                  {"bridge_flow", "1.1"},
                  {"layer_height", "0.28"},
                  {"initial_layer_print_height", "0.32"},
                  {"support_line_width", "0.5"},
                  {"support_filament", "1"},
                  {"support_interface_filament", "1"}},
                 {cube}});
    c.push_back({"automatic widths",
                 true,
                 {{"line_width", "0"},
                  {"outer_wall_line_width", "0"},
                  {"inner_wall_line_width", "0"},
                  {"sparse_infill_line_width", "0"},
                  {"internal_solid_infill_line_width", "0"},
                  {"top_surface_line_width", "0"},
                  {"initial_layer_line_width", "0"},
                  {"support_line_width", "0"},
                  {"layer_height", "0.12"}},
                 {cube}});
    c.push_back({"brim and skirt", true, {{"brim_type", "outer_only"}, {"brim_width", "5"}, {"skirt_loops", "2"}, {"skirt_height", "1"}}, {cube}});
    c.push_back({"names and scales",
                 true,
                 {},
                 {{"", "C:/models/part.v2.stl", 0, 10, 30, 5, {{80, 100, 1.5, 1, 2, true}, {120, 100, 1, 1, 1, false}}, {{"wall_loops", "3"}}},
                  {"second", "x.3mf", 1, 6, 6, 6, {{150, 150, 1, 1, 1, true}}, {}},
                  {"third", "", 0, 5, 5, 5, {{30, 30, 0.25, 0.25, 0.25, true}}, {}}}});
    c.push_back({"nothing printable", true, {}, {{"hidden", "hidden.stl", 0, 10, 10, 10, {{100, 100, 1, 1, 1, false}}, {}}}});
    c.push_back({"two filaments", false,
                 {{"nozzle_diameter", "0.4,0.6"},
                  {"filament_diameter", "1.75,1.75"},
                  {"filament_map", "1,2"},
                  {"outer_wall_filament_id", "2"},
                  {"sparse_infill_filament_id", "2"}},
                 {cube}});
    c.push_back({"supports", false, {{"enable_support", "1"}, {"support_line_width", "0.38"}}, {cube}});
    c.push_back({"raft", false, {{"raft_layers", "2"}}, {cube}});
    return c;
}

// The flows of one region on the object, for every role, three heights and both layer kinds.
void put_region(Payload &p, const PrintRegion &r, const PrintObject &obj, const Print &print)
{
    p.u64(uint64_t(r.config_hash()));
    for (int role = 0; role < 8; ++role)
        attempt(p, [&](Payload &q) { q.u32(r.extruder(FlowRole(role))); });
    const double heights[] = {obj.config().layer_height.value, 0.1, 0.};
    for (int role = 0; role < 8; ++role)
        for (double h : heights)
            for (int first = 0; first < 2; ++first)
                attempt(p, [&](Payload &q) { put(q, r.flow(obj, FlowRole(role), h, first != 0)); });
    p.f64(r.nozzle_dmr_avg(print.config())).f64(r.bridging_height_avg(print.config()));
    std::vector<unsigned int> ex;
    r.collect_object_printing_extruders(print, ex);
    p.u32s(std::vector<uint32_t>(ex.begin(), ex.end()));
    for (int brim = 0; brim < 2; ++brim) {
        std::vector<unsigned int> e2;
        PrintRegion::collect_object_printing_extruders(print.config(), r.config(), brim != 0, e2);
        p.u32s(std::vector<uint32_t>(e2.begin(), e2.end()));
    }
}

// The overrides of a layer's region over the object's region configuration.
KeyValues region_overrides(SplitMix &g)
{
    const std::vector<std::string> &patterns = print_config_def.get("sparse_infill_pattern")->enum_values;
    const std::vector<std::pair<std::string, std::vector<std::string>>> choices = {
        {"wall_loops", {"0", "2", "3"}},
        {"sparse_infill_density", {"0%", "15%", "100%"}},
        {"top_shell_layers", {"0", "3"}},
        {"bottom_shell_layers", {"0", "3"}},
        {"bridge_line_width", {"0", "0.5", "120%"}},
        {"bridge_flow", {"0.8", "1", "1.2"}},
        {"sparse_infill_pattern", patterns},
        {"outer_wall_line_width", {"0", "0.42", "110%"}},
        {"inner_wall_line_width", {"0", "0.5"}},
        {"sparse_infill_line_width", {"0", "0.6", "150%"}},
        {"top_surface_line_width", {"0", "0.38"}},
        {"inner_wall_filament_id", {"1", "2"}},
        {"sparse_infill_filament_id", {"1", "2"}},
    };
    KeyValues out;
    for (const auto &c : choices)
        if (g.below(3) == 0)
            out.emplace_back(c.first, c.second[size_t(g.below(c.second.size()))]);
    return out;
}

// LYLA: a layer added to the object with regions of its own, its slices and fill boundaries,
// then what Layer and LayerRegion compute of them.
void record_layers(Sets &sets, uint32_t case_id, PrintObject &obj, SplitMix &g, std::deque<PrintRegion> &regions, int count, int committed)
{
    for (int li = 0; li < count; ++li) {
        const size_t id       = g.below(3) == 0 ? 0 : size_t(rnd(g, 1, 40));
        const double height   = g.below(2) == 0 ? obj.config().layer_height.value : real(g, 0.08, 0.32);
        const double print_z  = id == 0 ? height : real(g, 0.5, 10.0);
        const double slice_z  = print_z - 0.5 * height;
        Layer       *layer    = obj.add_layer(int(id), height, print_z, slice_z);
        const size_t nregions = size_t(rnd(g, 1, 3));
        Payload      p;
        p.u32(case_id).u64(id).f64(height).f64(print_z).f64(slice_z).u64(nregions);
        for (size_t r = 0; r < nregions; ++r) {
            PrintRegionConfig rc = obj.printing_region(0).config();
            const KeyValues   ov = region_overrides(g);
            for (const auto &kv : ov)
                rc.set_deserialize_strict(kv.first, kv.second);
            regions.emplace_back(rc, rc.hash());
            LayerRegion *lr = layer->add_region(&regions.back());
            Surfaces     slices;
            ExPolygons   fill;
            const size_t n = size_t(rnd(g, r == 0 && li == 0 ? 0 : 1, 3));
            for (size_t k = 0; k < n; ++k) {
                const Point   c(rnd(g, -12000000, 12000000), rnd(g, -12000000, 12000000));
                const int64_t rad = rnd(g, 1000000, 8000000);
                Surface       s(SurfaceType(g.below(size_t(stCount))), island(g, c, rad));
                s.thickness = g.below(2) == 0 ? -1. : height;
                slices.push_back(s);
                fill.push_back(island(g, Point(c.x() + rnd(g, -2000000, 2000000), c.y()), rad * 3 / 4));
            }
            lr->slices.surfaces = slices;
            lr->fill_expolygons = fill;
            put_kv(p, ov);
            p.u64(uint64_t(rc.hash()));
            put(p, slices);
            put(p, fill);
        }
        for (LayerRegion *lr : layer->regions()) {
            lr->slices_to_fill_surfaces_clipped();
            put(p, lr->fill_surfaces.surfaces);
        }
        p.boolean(layer->empty());
        layer->make_slices();
        put(p, layer->lslices);
        for (double off : {0., 0.1, 1.0})
            put(p, layer->merged(float(scale_(off))));
        for (LayerRegion *lr : layer->regions())
            put(p, get_extents(*lr));
        put(p, get_extents(layer->regions()));
        for (LayerRegion *lr : layer->regions())
            for (int role = 0; role < 8; ++role) {
                attempt(p, [&](Payload &q) { put(q, lr->flow(FlowRole(role))); });
                attempt(p, [&](Payload &q) { put(q, lr->flow(FlowRole(role), 0.3)); });
                attempt(p, [&](Payload &q) { put(q, lr->bridging_flow(FlowRole(role), false)); });
                attempt(p, [&](Payload &q) { put(q, lr->bridging_flow(FlowRole(role), true)); });
            }
        attempt(p, [&](Payload &q) { q.f64(layer->get_sparse_infill_max_void_area()); });
        layer->backup_untyped_slices();
        for (LayerRegion *lr : layer->regions())
            put(p, lr->raw_slices);
        layer->restore_untyped_slices();
        for (LayerRegion *lr : layer->regions())
            put(p, lr->slices.surfaces);
        layer->restore_untyped_slices_no_extra_perimeters();
        for (LayerRegion *lr : layer->regions())
            put(p, lr->slices.surfaces);
        p.u64(layer->get_extruder_id(0)).u64(layer->get_extruder_id(1));
        add_within(sets, "LYLA", p, li < committed ? 40000 : 0);
    }
}

void record_case(Sets &sets, uint32_t case_id, const PrintCase &pc, SplitMix &g)
{
    Payload p;
    p.u32(case_id).str(pc.label).boolean(pc.domain_a);
    put_kv(p, pc.config);
    p.u64(pc.objects.size());
    for (const ObjSpec &o : pc.objects) {
        p.str(o.name).str(o.input_file).u8(uint8_t(o.mesh)).f64(o.x).f64(o.y).f64(o.z);
        p.u64(o.instances.size());
        for (const InstSpec &i : o.instances)
            p.f64(i.ox).f64(i.oy).f64(i.sx).f64(i.sy).f64(i.sz).boolean(i.printable);
        put_kv(p, o.config);
    }
    // The layers' regions outlive the print, whose layers point at them.
    std::deque<PrintRegion> regions;
    std::unique_ptr<Access> print(new Access());
    attempt(p, [&](Payload &q) {
        DynamicPrintConfig cfg = DynamicPrintConfig::full_print_config();
        for (const auto &kv : pc.config)
            cfg.set_deserialize_strict(kv.first, kv.second);
        Model model;
        for (const ObjSpec &o : pc.objects) {
            ModelObject *mo = model.add_object();
            mo->name        = o.name;
            mo->input_file  = o.input_file;
            mo->add_volume(o.mesh == 0 ? make_cube(o.x, o.y, o.z) : make_cylinder(o.x, o.z));
            for (const InstSpec &i : o.instances) {
                ModelInstance *mi = mo->add_instance();
                mi->set_offset(Vec3d(i.ox, i.oy, 0));
                mi->set_scaling_factor(Vec3d(i.sx, i.sy, i.sz));
                mi->printable = i.printable;
            }
            for (const auto &kv : o.config) {
                ConfigSubstitutionContext ctx(ForwardCompatibilitySubstitutionRule::Disable);
                mo->config.set_deserialize(kv.first, kv.second, ctx);
            }
            mo->ensure_on_bed();
        }
        q.u32(uint32_t(print->apply(model, cfg)));
    });
    const auto objs = print->objects();
    p.u64(objs.size());
    for (size_t oi = 0; oi < objs.size(); ++oi) {
        const PrintObject &o = *objs[oi];
        p.u64(o.instances().size());
        for (const PrintInstance &i : o.instances())
            put(p, i.shift);
        p.i64(o.size().x()).i64(o.size().y()).i64(o.size().z());
        p.u64(uint64_t(o.config().hash()));
        p.u64(o.num_printing_regions());
        for (size_t r = 0; r < o.num_printing_regions(); ++r)
            p.u64(uint64_t(o.printing_region(r).config_hash()));
        const std::vector<unsigned int> ex = o.object_extruders();
        p.u32s(std::vector<uint32_t>(ex.begin(), ex.end()));
    }
    {
        const std::vector<const PrintInstance *> chain = chain_print_object_instances(*print);
        p.u64(chain.size());
        for (const PrintInstance *pi : chain) {
            uint64_t oi = 0, ii = 0;
            for (size_t a = 0; a < objs.size(); ++a)
                for (size_t b = 0; b < objs[a]->instances().size(); ++b)
                    if (&objs[a]->instances()[b] == pi) {
                        oi = a;
                        ii = b;
                    }
            p.u64(oi).u64(ii);
        }
    }
    p.u64(uint64_t(print->config().hash())).boolean(print->has_brim());
    add(sets, "LYPC", p, true);

    // LYPR: the object's regions.
    for (size_t oi = 0; oi < objs.size(); ++oi) {
        const PrintObject &o = *objs[oi];
        for (size_t r = 0; r < o.num_printing_regions(); ++r) {
            Payload q;
            q.u32(case_id).u64(oi).u64(r);
            put_region(q, o.printing_region(r), o, *print);
            add(sets, "LYPR", q, true);
        }
    }
    // LYSF: the support flows of each object.
    for (size_t oi = 0; oi < objs.size(); ++oi) {
        const PrintObject *o = objs[oi];
        Payload            q;
        q.u32(case_id).u64(oi);
        attempt(q, [&](Payload &w) { put(w, support_material_flow(o)); });
        attempt(q, [&](Payload &w) { put(w, support_material_flow(o, 0.15f)); });
        attempt(q, [&](Payload &w) { put(w, support_transition_flow(o)); });
        attempt(q, [&](Payload &w) { put(w, support_material_1st_layer_flow(o)); });
        attempt(q, [&](Payload &w) { put(w, support_material_1st_layer_flow(o, 0.25f)); });
        attempt(q, [&](Payload &w) { put(w, support_material_interface_flow(o)); });
        attempt(q, [&](Payload &w) { put(w, support_material_interface_flow(o, 0.15f)); });
        add(sets, "LYSF", q, true);
    }
    // LYPH: the object placeholders as G-code export sets them (".gcode"), then output_filename.
    {
        Payload       q;
        q.u32(case_id);
        DynamicConfig c;
        print->placeholders(c, ".gcode");
        const std::vector<std::string> keys = c.keys();
        q.u64(keys.size());
        for (const std::string &k : keys)
            q.str(k).str(c.opt_serialize(k));
        bool printable = false;
        for (const ObjSpec &o : pc.objects)
            for (const InstSpec &i : o.instances)
                printable = printable || i.printable;
        const std::vector<std::string> formats = {"", "{input_filename_base}_{num_objects}", "[input_filename_base].gcode", "{first_object_name}", "{scale[0]}", "{bad"};
        const std::vector<std::string> bases   = {"", "base.part"};
        q.u64(formats.size() * bases.size());
        for (const std::string &f : formats)
            for (const std::string &b : bases) {
                q.str(f).str(b);
                // The engine reads input_filename_base unchecked for an empty format: with
                // nothing printable and no base it is not set, so that call is not made.
                if (f.empty() && b.empty() && !printable) {
                    q.u8(2);
                    continue;
                }
                attempt(q, [&](Payload &w) { w.str(print->filename(f, ".gcode", b)); });
            }
        add(sets, "LYPH", q, true);
    }
    // LYPT: Print's filament and extruder accessors.
    {
        Payload q;
        q.u32(case_id);
        const std::vector<int> maps = print->get_filament_maps();
        q.i32s(std::vector<int32_t>(maps.begin(), maps.end()));
        q.u32(uint32_t(print->get_filament_map_mode()));
        const std::vector<double> heights = print->get_extruder_printable_height();
        q.u64(heights.size());
        for (double h : heights)
            q.f64(h);
        const std::vector<Polygons> printable = print->get_extruder_printable_polygons();
        q.u64(printable.size());
        for (const Polygons &ps : printable)
            put(q, ps);
        const std::vector<Polygons> unprintable = print->get_extruder_unprintable_polygons();
        q.u64(unprintable.size());
        for (const Polygons &ps : unprintable)
            put(q, ps);
        for (unsigned f = 0; f < 4; ++f)
            q.u64(print->get_extruder_id(f));
        add(sets, "LYPT", q, true);
    }
    // LYLA: layers on the first object, for the cases of domain A and the two filaments.
    if (!objs.empty() && (pc.domain_a || pc.label == "two filaments"))
        record_layers(sets, case_id, *print->get_object(0), g, regions, case_id < 4 ? 40 : 10, case_id < 4 ? 3 : 1);
}

} // namespace

void record_print(Sets &sets)
{
    SplitMix                     g(0x1A7EB10Cull);
    const std::vector<PrintCase> cases = print_cases();
    for (size_t i = 0; i < cases.size(); ++i)
        record_case(sets, uint32_t(i), cases[i], g);
}

} // namespace layer
} // namespace probe
