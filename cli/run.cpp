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
// Carries OrcaSlicer code: the order of a run follows CLI::run in src/OrcaSlicer.cpp, and the plate
// code the stock command line takes from the GUI is ported from src/slic3r/GUI/PartPlate.cpp
// (get_extruders_under_cli, estimate_wipe_tower_size, estimate_wipe_tower_polygon,
// PartPlateList::preprocess_exclude_areas, PartPlateList::store_to_3mf_structure) and
// src/slic3r/GUI/Plater.cpp (the plate boxes of export_3mf), OrcaSlicer v2.4.2, commit 8500fcd;
// the global arrange, the build volume check and the plate's print configuration follow
// src/OrcaSlicer.cpp. Modified for sketch-orca, 2026-10-30.
//
// sketch-orca run: one job, from the job file to the result file. The order follows the stock
// command line (OrcaSlicer.cpp, CLI::run): the resources, the presets, the model, per-object
// settings, layer heights, custom G-code per layer, arrange, the build volume check, then
// Print::apply, validate, process and export, with the stock exit meanings as named failures.
// Where the stock command line needs the GUI's plate code (PartPlate.cpp), the few lines it uses
// are ported here for the one plate a job has.
#include "run.hpp"

#include <algorithm>
#include <atomic>
#include <cctype>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <limits>
#include <map>
#include <mutex>
#include <sstream>
#include <optional>
#include <set>
#include <string>
#include <vector>

#include <boost/filesystem.hpp>

#include "libslic3r/libslic3r.h"
#include "libslic3r/Arrange.hpp"
#include "libslic3r/BoundingBox.hpp"
#include "libslic3r/BuildVolume.hpp"
#include "libslic3r/CustomGCode.hpp"
#include "libslic3r/Exception.hpp"
#include "libslic3r/Format/bbs_3mf.hpp"
#include "libslic3r/GCode/GCodeProcessor.hpp"
#include "libslic3r/GCode/ThumbnailData.hpp"
#include "libslic3r/GCode/ToolOrdering.hpp"
#include "libslic3r/GCode/WipeTower.hpp"
#include "libslic3r/Layer.hpp"
#include "libslic3r/MaterialType.hpp"
#include "libslic3r/Model.hpp"
#include "libslic3r/ModelArrange.hpp"
#include "libslic3r/Orient.hpp"
#include "libslic3r/Preset.hpp"
#include "libslic3r/Print.hpp"
#include "libslic3r/PrintConfig.hpp"
#include "libslic3r/Semver.hpp"
#include "libslic3r/Slicing.hpp"
#include "libslic3r/Utils.hpp"
#include "libslic3r/miniz_extension.hpp"
#include "libslic3r_version.h"

#include "common.hpp"
#include "job.hpp"
#include "presets.hpp"
#include "stats.hpp"

namespace sketch_orca {

using namespace Slic3r;

namespace {

namespace fs = boost::filesystem;

const char *RESULT_FORMAT = "sketch-orca-result/1";

// ---------------------------------------------------------------------------------------------
// Small helpers

double cfg_float(const DynamicPrintConfig &cfg, const std::string &key, double fallback)
{
    const ConfigOption *o = cfg.option(key);
    if (o == nullptr || o->is_vector())
        return fallback;
    return o->getFloat();
}

bool cfg_bool(const DynamicPrintConfig &cfg, const std::string &key, bool fallback = false)
{
    const ConfigOption *o = cfg.option(key);
    if (o == nullptr || o->is_vector())
        return fallback;
    return o->getBool();
}

int cfg_enum(const DynamicPrintConfig &cfg, const std::string &key, int fallback)
{
    const ConfigOption *o = cfg.option(key);
    if (o == nullptr || o->is_vector())
        return fallback;
    return o->getInt();
}

std::string cfg_string(const DynamicPrintConfig &cfg, const std::string &key)
{
    const ConfigOption *o = cfg.option(key);
    return o == nullptr ? std::string() : o->serialize();
}

bool by_object(const DynamicPrintConfig &cfg)
{
    return cfg_enum(cfg, "print_sequence", int(PrintSequence::ByLayer)) == int(PrintSequence::ByObject);
}

bool blank(const std::string &s)
{
    return std::all_of(s.begin(), s.end(), [](unsigned char c) { return std::isspace(c) != 0; });
}

std::string mm(double v)
{
    char buf[64];
    std::snprintf(buf, sizeof(buf), "%.3f", v);
    std::string s = buf;
    while (!s.empty() && s.back() == '0')
        s.pop_back();
    if (!s.empty() && s.back() == '.')
        s.pop_back();
    return s;
}

json point_json(const Vec2d &p) { return json::array({round_um(p.x()), round_um(p.y())}); }
json point3_json(const Vec3d &p) { return json::array({round_um(p.x()), round_um(p.y()), round_um(p.z())}); }

json points_json(const DynamicPrintConfig &cfg, const std::string &key)
{
    json out = json::array();
    if (const auto *opt = cfg.option<ConfigOptionPoints>(key))
        for (const Vec2d &p : opt->values)
            out.push_back(point_json(p));
    return out;
}

// An area of the bed, or none: Orca keeps "no area" as one point at the origin (bed_exclude_area's
// default), and BuildVolume reads fewer than three points as no area.
json area_json(const DynamicPrintConfig &cfg, const std::string &key)
{
    json out = points_json(cfg, key);
    return out.size() < 3 ? json::array() : out;
}

std::string object_label(const Model &model, int index)
{
    if (index < 0 || size_t(index) >= model.objects.size())
        return "an object";
    const std::string &name = model.objects[size_t(index)]->name;
    return "object " + std::to_string(index) + (name.empty() ? std::string() : " (" + name + ")");
}

int index_of_model_object(const Model &model, const ObjectID &id)
{
    for (size_t i = 0; i < model.objects.size(); ++i)
        if (model.objects[i]->id() == id)
            return int(i);
    return -1;
}

// A PrintObject's model object is the Print's copy of ours; copies keep their ObjectID.
int index_of_print_object(const Model &model, const PrintObject *po)
{
    if (po == nullptr || po->model_object() == nullptr)
        return -1;
    return index_of_model_object(model, po->model_object()->id());
}

int index_of_print_object_id(const Print &print, const Model &model, size_t print_object_id)
{
    for (const PrintObject *po : print.objects())
        if (po->id().id == print_object_id)
            return index_of_print_object(model, po);
    return -1;
}

int index_of_object_base(const Print &print, const Model &model, const ObjectBase *o)
{
    if (o == nullptr)
        return -1;
    if (const auto *po = dynamic_cast<const PrintObject *>(o))
        return index_of_print_object(model, po);
    if (const auto *mo = dynamic_cast<const ModelObject *>(o))
        return index_of_model_object(model, mo->id());
    if (const auto *mi = dynamic_cast<const ModelInstance *>(o))
        return mi->get_object() ? index_of_model_object(model, mi->get_object()->id()) : -1;
    return index_of_print_object_id(print, model, o->id().id);
}

json warnings_json(const Warnings &warnings)
{
    json out = json::array();
    for (const Warning &w : warnings.list()) {
        json o;
        o["code"]    = w.code;
        o["message"] = w.message;
        if (w.object_index >= 0)
            o["objectIndex"] = w.object_index;
        out.push_back(std::move(o));
    }
    return out;
}

// ---------------------------------------------------------------------------------------------
// Resources and model

// The folders Orca reads its resources from (CLI::setup), relative to the job's resources folder
// instead of the executable, and the job's work folder for Orca's temporary and data folders.
void set_up_folders(const Job &job)
{
    const fs::path res(job.resources);
    if (!is_directory(job.resources) || !is_directory((res / "profiles").string()))
        fail("environment", "the resources folder " + job.resources + " has no profiles folder; give the engine's resources folder");
    set_resources_dir(res.string());
    set_var_dir((res / "images").string());
    set_local_dir((res / "i18n").string());
    set_sys_shapes_dir((res / "shapes").string());
    set_custom_gcodes_dir((res / "custom_gcodes").string());
    try {
        make_directories(job.work_dir);
        const std::string data = (fs::path(job.work_dir) / "orca-data").string();
        make_directories(data);
        set_temporary_dir(job.work_dir);
        set_data_dir(data);
    } catch (const std::exception &e) {
        fail("output-failed", std::string("cannot use the work folder: ") + e.what());
    }
}

// Model::read_from_file as the stock command line calls it, without the file's own settings: the
// job's presets decide every setting. A 3MF still brings its objects' own settings, height ranges
// and layer height profiles.
Model load_model(const Job &job)
{
    if (!is_file(job.model_path))
        fail("file-not-found", "the model " + job.model_path + " does not exist");
    DynamicPrintConfig        config;
    ConfigSubstitutionContext substitutions(ForwardCompatibilitySubstitutionRule::EnableSilent);
    PlateDataPtrs             plates;
    std::vector<Preset *>     presets;
    bool                      is_bbl_3mf = false;
    Semver                    version;
    Model                     model;
    std::string               error;
    // Orca's loaders name an object after the text behind the last native separator ('\' on
    // Windows), so a path with '/' would name it, and label it in the G-code, by the whole path.
    const std::string path = boost::filesystem::path(job.model_path).make_preferred().string();
    try {
        model = Model::read_from_file(path, &config, &substitutions, LoadStrategy::LoadModel | LoadStrategy::AddDefaultInstances, &plates,
                                      &presets, &is_bbl_3mf, &version);
    } catch (const std::exception &e) {
        error = e.what();
        if (error.empty())
            error = "unknown error";
    }
    release_PlateData_list(plates);
    for (Preset *p : presets)
        delete p;
    if (!error.empty())
        fail("model-unreadable", "cannot read the model " + job.model_path + ": " + error);
    if (model.objects.empty())
        fail("model-unreadable", "the model " + job.model_path + " has no objects");
    // Custom G-code per layer comes from the job only.
    model.plates_custom_gcodes.clear();
    model.curr_plate_index = 0;
    for (ModelObject *mo : model.objects)
        mo->ensure_on_bed();
    return model;
}

// Every part Orca prints must enclose a volume. A part with fewer than two triangles gets no convex
// hull (ModelVolume computes one from two triangles up), which the print volume check and arrange
// read without a check, and a flat part has an empty hull, which the print volume check skips, so
// its object reads as outside the bed. Such a part is refused here, naming its object.
void check_solid_objects(const Model &model)
{
    for (size_t oi = 0; oi < model.objects.size(); ++oi) {
        const ModelObject *mo       = model.objects[oi];
        bool               any_part = false;
        for (const ModelVolume *v : mo->volumes) {
            if (!v->is_model_part())
                continue;
            any_part                = true;
            const size_t      facets = v->mesh().facets_count();
            const Vec3d       size   = v->mesh().bounding_box().size();
            const std::string what   = object_label(model, int(oi)) + " has no volume to print: " +
                                     (mo->volumes.size() > 1 ? "its part \"" + v->name + "\"" : std::string("its mesh"));
            if (facets < 4)
                fail("model-unreadable", what + " has " + std::to_string(facets) + " triangle(s); a closed surface has at least four", int(oi));
            if (!(size.minCoeff() > EPSILON))
                fail("model-unreadable", what + " is flat (" + mm(size.x()) + " x " + mm(size.y()) + " x " + mm(size.z()) + " mm)", int(oi));
        }
        if (!any_part)
            fail("model-unreadable", object_label(model, int(oi)) + " has no part to print, only modifiers", int(oi));
    }
}

void orient_objects(Model &model)
{
    for (size_t i = 0; i < model.objects.size(); ++i) {
        try {
            orientation::orient(model.objects[i]);
        } catch (const std::exception &e) {
            fail("orient-failed", "cannot orient " + object_label(model, int(i)) + ": " + e.what(), int(i));
        }
    }
}

json info_json(const Model &model, const Settings &s)
{
    json objects = json::array();
    for (size_t i = 0; i < model.objects.size(); ++i) {
        const ModelObject  *mo = model.objects[i];
        const BoundingBoxf3 bb = mo->instances.empty() ? mo->raw_bounding_box() : mo->bounding_box_exact();
        json o;
        o["objectIndex"] = i;
        o["name"]        = mo->name;
        o["instances"]   = mo->instances.size();
        o["triangles"]   = mo->facets_count();
        o["min"]         = point3_json(bb.min);
        o["max"]         = point3_json(bb.max);
        o["size"]        = point3_json(bb.size());
        objects.push_back(std::move(o));
    }
    json bed;
    bed["printableArea"]   = points_json(s.config, "printable_area");
    bed["printableHeight"] = round_um(cfg_float(s.config, "printable_height", 0.));
    bed["excludeArea"]     = area_json(s.config, "bed_exclude_area");
    bed["bedType"]         = cfg_string(s.config, "curr_bed_type");
    json filaments = json::array();
    for (const LoadedPreset &f : s.filaments)
        filaments.push_back(f.name);
    json info;
    info["objects"]        = std::move(objects);
    info["bed"]            = std::move(bed);
    info["printer"]        = s.machine.name;
    info["printerModel"]   = cfg_string(s.config, "printer_model");
    info["printerModelId"] = s.printer_model_id;
    info["process"]        = s.process.name;
    info["filaments"]      = std::move(filaments);
    info["extruders"]      = s.extruder_count;
    info["printSequence"]  = cfg_string(s.config, "print_sequence");
    return info;
}

// ---------------------------------------------------------------------------------------------
// Per-object settings, height ranges and custom G-code

std::string object_field(size_t position) { return "objects[" + std::to_string(position) + "]"; }

// Runs `f`; a failure in it that names no object names object `index`.
template<class F> void for_object(int index, F &&f)
{
    try {
        f();
    } catch (const Failure &e) {
        if (e.object_index >= 0)
            throw;
        throw Failure(e.code, e.what(), index, e.exit_code);
    }
}

void check_extruder_setting(const DynamicPrintConfig &c, size_t filaments, const std::string &field)
{
    if (!c.has("extruder"))
        return;
    const int e = c.opt_int("extruder");
    if (e < 0 || size_t(e) > filaments)
        fail("setting-invalid", field + ": extruder " + std::to_string(e) + " is not one of the job's " + std::to_string(filaments) +
                                    " filament(s) (0 keeps the default)");
}

// The job's per-object overrides (object settings and region settings, as the object list of
// Orca's GUI offers them) and height ranges (a modifier over a z range of the object, with its own
// layer height, as ObjectList::get_default_layer_config starts it).
void apply_object_settings(Model &model, const Job &job, const Settings &s)
{
    static const PrintObjectConfig object_defaults;
    static const PrintRegionConfig region_defaults;
    const size_t n = s.filaments.size();
    for (size_t pos = 0; pos < job.objects.size(); ++pos)
        for_object(job.objects[pos].index, [&]() {
        const JobObject   &jo = job.objects[pos];
        const std::string  f  = object_field(pos);
        ModelObject       *mo = model.objects[size_t(jo.index)];
        if (!jo.overrides.empty()) {
            DynamicPrintConfig over;
            for (const auto &kv : jo.overrides) {
                const std::string field = f + ".overrides." + kv.first;
                const std::string key   = parse_setting(over, kv.first, kv.second, field);
                if (key != "extruder" && !object_defaults.has(key) && !region_defaults.has(key))
                    fail("setting-invalid", field + ": " + key + " is a printer, filament or whole-plate setting, not a per-object one; set it in overrides", jo.index);
            }
            check_extruder_setting(over, n, f + ".overrides.extruder");
            mo->config.apply(over);
        }
        if (!jo.height_ranges.empty()) {
            mo->layer_config_ranges.clear();
            const double layer_height = mo->config.has("layer_height") ? mo->config.opt_float("layer_height") : s.config.opt_float("layer_height");
            for (size_t r = 0; r < jo.height_ranges.size(); ++r) {
                const HeightRange &hr = jo.height_ranges[r];
                const std::string  rf = f + ".heightRanges[" + std::to_string(r) + "]";
                DynamicPrintConfig range;
                range.set_key_value("layer_height", new ConfigOptionFloat(layer_height));
                range.set_key_value("extruder", new ConfigOptionInt(0));
                for (const auto &kv : hr.overrides) {
                    const std::string field = rf + ".overrides." + kv.first;
                    const std::string key   = parse_setting(range, kv.first, kv.second, field);
                    if (key != "layer_height" && key != "extruder" && !region_defaults.has(key))
                        fail("setting-invalid", field + ": " + key + " cannot change over a height range; height ranges take layer_height, extruder and region settings (walls, infill, ...)", jo.index);
                }
                if (!(range.opt_float("layer_height") > 0.))
                    fail("setting-invalid", rf + ".overrides.layer_height must be above 0", jo.index);
                check_extruder_setting(range, n, rf + ".overrides.extruder");
                mo->layer_config_ranges[t_layer_height_range(hr.from, hr.to)].assign_config(std::move(range));
            }
        }
        });
}

const char *custom_type_name(const CustomGcode &c) { return c.type.c_str(); }

std::string at_print_z(const CustomGcode &c) { return std::string("the ") + custom_type_name(c) + " at printZ " + mm(c.print_z); }

// The job's custom G-codes as Orca's per-layer custom G-code of plate 1. Orca 2.4.2 compiles out the
// M600 branch of a color change (GCode.cpp, ProcessLayer::emit_custom_gcode_per_print_z, "#if 0"), so
// a color change there would emit nothing useful. So: a pause is Orca's pause (the printer's
// machine_pause_gcode); a color change with one filament is a pause too, for a manual swap; a color
// change with several filaments, and a filament change, switch to the given filament from that
// layer on, as a tool change; custom is the given G-code.
void set_custom_gcodes(Model &model, const Job &job, const Settings &s)
{
    model.plates_custom_gcodes.clear();
    model.curr_plate_index = 0;
    if (job.custom_gcodes.empty())
        return;
    if (by_object(s.config))
        fail("unsupported", "custom G-code per layer is not available with print_sequence \"by object\"; Orca runs it only for prints by layer");
    const size_t      n            = s.filaments.size();
    const std::string pause_gcode  = s.config.opt_string("machine_pause_gcode");
    CustomGCode::Info info;
    bool              tool_changes = false;
    for (const CustomGcode &c : job.custom_gcodes) {
        CustomGCode::Item item;
        item.print_z  = c.print_z;
        item.type     = CustomGCode::Custom;
        item.extruder = c.extruder > 0 ? c.extruder : 1;
        item.color    = c.color;
        item.extra    = "";
        bool pause = false;
        if (c.type == "pause") {
            pause = true;
        } else if (c.type == "color") {
            if (n == 1) {
                pause = true;
            } else {
                if (c.extruder == 0)
                    fail("custom-gcode-invalid", at_print_z(c) + " needs the extruder: the job has " + std::to_string(n) +
                                                     " filaments, so a color change switches to the filament of that color");
                item.type = CustomGCode::ToolChange;
            }
        } else if (c.type == "filament") {
            if (n < 2)
                fail("custom-gcode-invalid", at_print_z(c) + " needs at least two filaments; the job has one");
            item.type = CustomGCode::ToolChange;
        } else {
            item.type  = CustomGCode::Custom;
            item.extra = c.gcode;
        }
        if (pause) {
            if (blank(pause_gcode))
                fail("custom-gcode-invalid", at_print_z(c) + " needs the printer's pause G-code, and machine_pause_gcode is empty in this machine preset");
            item.type  = CustomGCode::PausePrint;
            item.extra = c.note;
            if (item.extra.empty() && c.type == "color")
                item.extra = "Change the filament to " + c.color;
        }
        if (item.type == CustomGCode::ToolChange) {
            if (size_t(c.extruder) > n)
                fail("custom-gcode-invalid", at_print_z(c) + ": filament " + std::to_string(c.extruder) + " is not one of the job's " + std::to_string(n) + " filaments");
            tool_changes = true;
        }
        info.gcodes.push_back(item);
    }
    info.mode = tool_changes ? CustomGCode::MultiAsSingle : (n == 1 ? CustomGCode::SingleExtruder : CustomGCode::MultiExtruder);
    model.plates_custom_gcodes[0] = info;
}

// ---------------------------------------------------------------------------------------------
// Arrange, prime tower and build volume

// The filaments plate 1 prints with, as the stock command line counts them for its prime tower
// (PartPlate::get_extruders_under_cli with custom G-code): the parts' filaments (assigned and
// painted; a part without one prints with filament 1), height ranges, support when it is on, the
// per-feature filaments of the objects and the print, and filament changes at a layer. Orca's
// version counts the per-feature filaments only when support is on; here they always count.
std::set<int> plate_filaments(const Model &model, const DynamicPrintConfig &cfg, size_t n)
{
    std::set<int> used;
    const auto add = [&](int e) {
        if (e >= 1 && size_t(e) <= n)
            used.insert(e);
    };
    const auto object_int = [](const ModelObject *mo, const char *key, int fallback) {
        const ConfigOption *o = mo->config.option(key);
        return o != nullptr ? o->getInt() : fallback;
    };
    const bool global_support = cfg_bool(cfg, "enable_support") || cfg_enum(cfg, "raft_layers", 0) > 0;
    for (const ModelObject *mo : model.objects) {
        if (!mo->printable || std::none_of(mo->instances.begin(), mo->instances.end(), [](const ModelInstance *i) { return i->printable; }))
            continue;
        const int object_extruder = object_int(mo, "extruder", 0);
        for (const ModelVolume *v : mo->volumes) {
            if (!v->is_model_part())
                continue;
            std::vector<int> e = v->get_extruders();
            if (e.empty())
                e.push_back(object_extruder > 0 ? object_extruder : 1);
            for (int x : e)
                add(x);
        }
        for (const auto &range : mo->layer_config_ranges)
            if (const ConfigOption *o = range.second.option("extruder"))
                add(o->getInt());
        bool support = global_support;
        if (mo->config.has("enable_support") || mo->config.has("raft_layers")) {
            const ConfigOption *o = mo->config.option("enable_support");
            support = (o != nullptr && o->getBool()) || object_int(mo, "raft_layers", 0) > 0;
        }
        if (support)
            for (const char *key : {"support_filament", "support_interface_filament"})
                add(object_int(mo, key, 0) != 0 ? object_int(mo, key, 0) : cfg_enum(cfg, key, 0));
        for (const char *key : {"outer_wall_filament_id", "inner_wall_filament_id", "sparse_infill_filament_id", "internal_solid_filament_id",
                                "top_surface_filament_id", "bottom_surface_filament_id"})
            add(object_int(mo, key, 0) != 0 ? object_int(mo, key, 0) : cfg_enum(cfg, key, 0));
    }
    if (const auto it = model.plates_custom_gcodes.find(0); it != model.plates_custom_gcodes.end())
        for (const CustomGCode::Item &item : it->second.gcodes)
            if (item.type == CustomGCode::ToolChange)
                add(item.extruder);
    return used;
}

size_t plate_filament_count(const Model &model, const DynamicPrintConfig &cfg, size_t n) { return plate_filaments(model, cfg, n).size(); }

// PartPlate::estimate_wipe_tower_size.
Vec3d estimate_wipe_tower_size(const DynamicPrintConfig &cfg, const Model &model, double w, double wipe_volume, int extruder_count, int plate_extruder_size, bool enable_wrapping)
{
    Vec3d size = Vec3d::Zero();
    const double layer_height = cfg_float(cfg, "layer_height", 0.08);
    if (plate_extruder_size == 0)
        return size;
    double max_height = 0.;
    for (const ModelObject *mo : model.objects)
        max_height = std::max(mo->bounding_box_exact().size().z(), max_height);
    size(2) = max_height;
    const bool   need_wipe_tower = cfg_enum(cfg, "timelapse_type", int(tlTraditional)) == int(tlSmooth) || enable_wrapping;
    const double extra_spacing   = cfg_float(cfg, "prime_tower_infill_gap", 100.) / 100.;
    const bool   use_rib_wall    = cfg_enum(cfg, "wipe_tower_wall_type", int(wtwRectangle)) == int(wtwRib);
    double       rib_width       = cfg_float(cfg, "wipe_tower_rib_width", 8.);
    double       filament_change_volume = 0.;
    {
        double length = 0., diameter = 1.75;
        if (const auto *opt = cfg.option<ConfigOptionFloats>("filament_change_length"); opt && !opt->values.empty())
            length = *std::max_element(opt->values.begin(), opt->values.end());
        if (const auto *opt = cfg.option<ConfigOptionFloats>("filament_diameter"); opt && !opt->values.empty())
            diameter = *std::max_element(opt->values.begin(), opt->values.end());
        filament_change_volume = length * PI * diameter * diameter / 4.;
    }
    double volume = wipe_volume * (extruder_count == 2 ? plate_extruder_size : (plate_extruder_size - 1));
    if (extruder_count == 2)
        volume += filament_change_volume * int(plate_extruder_size / 2);
    double depth = 0.;
    if (use_rib_wall) {
        depth = std::sqrt(volume / layer_height * extra_spacing);
        if (need_wipe_tower || plate_extruder_size > 1) {
            const float  min_depth    = WipeTower::get_limit_depth_by_height(float(max_height));
            const double volume_depth = depth;
            depth     = std::max(double(min_depth), depth);
            rib_width = std::min(rib_width, depth / 2);
            depth     = rib_width / std::sqrt(2.) + std::max(depth + cfg_float(cfg, "wipe_tower_extra_rib_length", 0.), volume_depth);
            size(0) = size(1) = depth;
        }
    } else {
        depth = volume / (layer_height * w) * extra_spacing;
        if (need_wipe_tower || depth > EPSILON) {
            const float min_depth = WipeTower::get_limit_depth_by_height(float(max_height));
            depth = std::max(double(min_depth), depth);
        }
        size(0) = w;
        size(1) = depth;
    }
    return size;
}

// Where Orca's prime tower goes on plate 1, set in wipe_tower_x/y, and its footprint for arrange.
// The stock command line starts from the GUI's default corner (165, 250; 0, 250 on a bed slinger),
// then PartPlate::estimate_wipe_tower_polygon keeps it on the plate. A job that sets wipe_tower_x or
// wipe_tower_y starts from its own position instead.
std::optional<arrangement::ArrangePolygon> place_prime_tower(DynamicPrintConfig &cfg, const Model &model, const Settings &s)
{
    if (by_object(cfg) || !cfg_bool(cfg, "enable_prime_tower"))
        return std::nullopt;
    const bool wrapping      = cfg_bool(cfg, "enable_wrapping_detection");
    const auto *wrap_area    = cfg.option<ConfigOptionPoints>("wrapping_exclude_area");
    const bool  smooth       = cfg_enum(cfg, "timelapse_type", int(tlTraditional)) == int(tlSmooth);
    const size_t filaments   = plate_filament_count(model, cfg, s.filaments.size());
    if (filaments <= 1 && !smooth && !(wrapping && wrap_area && !wrap_area->values.empty()))
        return std::nullopt;

    auto *xs = cfg.option<ConfigOptionFloats>("wipe_tower_x", true);
    auto *ys = cfg.option<ConfigOptionFloats>("wipe_tower_y", true);
    const bool overridden = std::find(s.overridden.begin(), s.overridden.end(), "wipe_tower_x") != s.overridden.end() ||
                            std::find(s.overridden.begin(), s.overridden.end(), "wipe_tower_y") != s.overridden.end();
    const float w = float(cfg_float(cfg, "prime_tower_width", 35.));
    float       x, y;
    if (overridden) {
        x = float(xs->values.empty() ? 15. : xs->values.front());
        y = float(ys->values.empty() ? 220. : ys->values.front());
    } else {
        const bool i3 = cfg_enum(cfg, "printer_structure", int(psUndefine)) == int(psI3);
        x = i3 ? 0.f : 165.f;
        y = 250.f;
        // OrcaSlicer.cpp names this the tower's brim but reads prime_tower_width; kept as it is.
        const float tower_margin = float(WIPE_TOWER_MARGIN) + w;
        x = std::max(x, tower_margin);
        y = std::max(y, tower_margin);
    }

    const float v                = float(cfg_float(cfg, "prime_volume", 45.));
    const float tower_brim_width = float(cfg_float(cfg, "prime_tower_brim_width", 3.));
    const Vec3d size = estimate_wipe_tower_size(cfg, model, w, v, int(s.extruder_count), int(filaments), wrapping);
    BoundingBoxf bed;
    if (const auto *area = cfg.option<ConfigOptionPoints>("printable_area"))
        for (const Vec2d &p : area->values)
            bed.merge(p);
    const float plate_width = float(bed.defined ? bed.size().x() : 256.);
    const float plate_depth = float(bed.defined ? bed.size().y() : 256.);
    const float depth       = float(size(1));
    const float margin      = float(WIPE_TOWER_MARGIN) + tower_brim_width;
    float       brim        = tower_brim_width;
    if (brim < 0)
        brim = WipeTower::get_auto_brim_by_height(float(size.z()));
    const auto clamp = [](float value, float lo, float hi) { return hi < lo ? lo : std::clamp(value, lo, hi); };
    x = clamp(x, margin, plate_width - w - margin - brim);
    y = clamp(y, margin, plate_depth - depth - margin - brim);
    if (xs->values.empty())
        xs->values.push_back(x);
    else
        xs->values.front() = x;
    if (ys->values.empty())
        ys->values.push_back(y);
    else
        ys->values.front() = y;

    arrangement::ArrangePolygon ap;
    ap.poly.contour = Polygon({{scaled(x - brim), scaled(y - brim)},
                               {scaled(x + w + brim), scaled(y - brim)},
                               {scaled(x + w + brim), scaled(y + depth + brim)},
                               {scaled(x - brim), scaled(y + depth + brim)}});
    ap.bed_idx        = 0;
    ap.setter         = nullptr;
    ap.translation    = Vec2crd(0, 0);
    ap.name           = "WipeTower";
    ap.is_virt_object = true;
    ap.is_wipe_tower  = true;
    return ap;
}

// PartPlateList::preprocess_exclude_areas: the bed's excluded rectangles (bed_exclude_area, four
// points each) and the clumping detection area, as fixed items on plates 0..num_plates-1.
void add_exclusion_regions(arrangement::ArrangePolygons &out, const DynamicPrintConfig &cfg, int num_plates, coord_t inflation)
{
    if (cfg_bool(cfg, "enable_wrapping_detection")) {
        if (const auto *opt = cfg.option<ConfigOptionPoints>("wrapping_exclude_area"); opt && !opt->values.empty()) {
            Polygon poly;
            for (const Vec2d &p : opt->values)
                poly.append(Point::new_scale(p.x(), p.y()));
            for (int j = 0; j < num_plates; ++j) {
                arrangement::ArrangePolygon ret;
                ret.poly.contour   = poly;
                ret.translation    = Vec2crd(0, 0);
                ret.rotation       = 0.;
                ret.is_virt_object = true;
                ret.bed_idx        = j;
                ret.height         = 1;
                ret.name           = "WrappingRegion";
                ret.inflation      = inflation;
                out.push_back(std::move(ret));
            }
        }
    }
    if (const auto *opt = cfg.option<ConfigOptionPoints>("bed_exclude_area")) {
        const Pointfs &pts = opt->values;
        for (size_t i = 0, index = 0; i + 3 < pts.size(); i += 4, ++index) {
            BoundingBoxf bb;
            for (size_t k = 0; k < 4; ++k)
                bb.merge(pts[i + k]);
            const Polygon poly({{scaled(bb.min.x()), scaled(bb.min.y())},
                                {scaled(bb.max.x()), scaled(bb.min.y())},
                                {scaled(bb.max.x()), scaled(bb.max.y())},
                                {scaled(bb.min.x()), scaled(bb.max.y())}});
            for (int j = 0; j < num_plates; ++j) {
                arrangement::ArrangePolygon ret;
                ret.poly.contour   = poly;
                ret.translation    = Vec2crd(0, 0);
                ret.rotation       = 0.;
                ret.is_virt_object = true;
                ret.bed_idx        = j;
                ret.height         = 1;
                ret.name           = "ExcludedRegion" + std::to_string(index);
                ret.inflation      = inflation;
                out.push_back(std::move(ret));
            }
        }
    }
}

// The stock command line's arrange of every object onto plate 1 (OrcaSlicer.cpp, "global arrange")
// with its parameters, except that objects are not rotated, so the parts keep the orientation
// the client gave them: the stock default rotates them, and on a bed slinger (printer_structure i3)
// Orca's arrange also turns each object's long side along Y (align_to_y_axis), which is left off.
// An object that does not fit on plate 1 is a failure: a job has one plate. Orca's arrange keeps
// objects whose filaments are of far apart temperatures (a high- and a low-temperature filament
// type, Print::get_filament_temp_type) off one plate; with one plate that is temperature-difference.
void arrange_objects(Model &model, const DynamicPrintConfig &cfg, const std::optional<arrangement::ArrangePolygon> &tower)
{
    using namespace arrangement;
    ArrangeParams    params;
    ArrangePolygons  selected, unselected, unprintable;
    std::vector<int> owner;
    params.is_seq_print = by_object(cfg);
    for (size_t oi = 0; oi < model.objects.size(); ++oi) {
        ModelObject *mo = model.objects[oi];
        for (ModelInstance *mi : mo->instances) {
            ArrangePolygon ap = get_instance_arrange_poly(mi, cfg);
            if (mi->printable) {
                ap.itemid = int(selected.size());
                selected.push_back(std::move(ap));
                owner.push_back(int(oi));
            } else {
                ap.itemid = int(unprintable.size());
                unprintable.push_back(std::move(ap));
            }
        }
    }
    if (selected.empty() && unprintable.empty())
        return;
    add_exclusion_regions(unselected, cfg, 16, 0);
    if (tower)
        unselected.push_back(*tower);

    params.allow_rotations                     = false;
    params.allow_multi_materials_on_same_plate = true;
    params.avoid_extrusion_cali_region         = false;
    params.clearance_height_to_rod             = float(cfg_float(cfg, "extruder_clearance_height_to_rod", 0.));
    params.clearance_height_to_lid             = float(cfg_float(cfg, "extruder_clearance_height_to_lid", 0.));
    params.clearance_radius                    = float(cfg_float(cfg, "extruder_clearance_radius", 0.));
    params.printable_height                    = float(cfg_float(cfg, "printable_height", 256.));
    params.min_obj_distance                    = 0;
    if (params.is_seq_print) {
        params.bed_shrink_x = BED_SHRINK_SEQ_PRINT;
        params.bed_shrink_y = BED_SHRINK_SEQ_PRINT;
    }
    params.align_to_y_axis = false;

    for (size_t i = 0; i < selected.size(); ++i)
        for (size_t j = i + 1; j < selected.size(); ++j)
            if (owner[i] != owner[j] && !Print::is_filaments_compatible({selected[i].filament_temp_type, selected[j].filament_temp_type}))
                fail("temperature-difference", object_label(model, owner[i]) + " and " + object_label(model, owner[j]) +
                                                   " print with filaments of far apart temperatures (a high-temperature and a low-temperature "
                                                   "filament type), which Orca does not arrange on one plate; slice them as separate jobs");

    update_arrange_params(params, &cfg, selected);
    update_selected_items_inflation(selected, &cfg, params);
    update_unselected_items_inflation(unselected, &cfg, params);
    const Points beds = get_shrink_bedpts(&cfg, params);
    add_exclusion_regions(params.excluded_regions, cfg, 1, coord_t(scale_(1)));
    params.progressind = [](unsigned, std::string) {};

    arrange(selected, unselected, beds, params);
    if (!unprintable.empty())
        arrange(unprintable, {}, beds, params);

    for (size_t i = 0; i < selected.size(); ++i)
        if (selected[i].bed_idx != 0)
            fail("arrange-failed", object_label(model, owner[i]) + " does not fit on the plate next to the other objects", owner[i]);
    for (ArrangePolygon &ap : selected)
        ap.apply();
    for (ArrangePolygon &ap : unprintable)
        if (ap.bed_idx == 0)
            ap.apply();
    for (ModelObject *mo : model.objects)
        mo->invalidate_bounding_box();
}

// The build volume check before slicing (OrcaSlicer.cpp: BuildVolume, update_print_volume_state).
// An instance partly or wholly outside is a failure here; Print::apply would leave it out silently.
void check_build_volume(Model &model, const DynamicPrintConfig &cfg)
{
    PrintConfig pc;
    pc.apply(cfg, true);
    const BuildVolume  volume(pc.printable_area.values, pc.printable_height.value, pc.extruder_printable_area.values, pc.extruder_printable_height.values);
    const unsigned int inside = model.update_print_volume_state(volume);
    const BoundingBoxf3 &bv   = volume.bounding_volume();
    const std::string bed = "the printable volume is x " + mm(bv.min.x()) + ".." + mm(bv.max.x()) + ", y " + mm(bv.min.y()) + ".." + mm(bv.max.y()) +
                            ", height " + mm(pc.printable_height.value) + " mm";
    for (size_t oi = 0; oi < model.objects.size(); ++oi) {
        const ModelObject *mo = model.objects[oi];
        if (!mo->printable)
            continue;
        for (size_t ii = 0; ii < mo->instances.size(); ++ii) {
            const ModelInstance *mi = mo->instances[ii];
            if (!mi->printable || mi->print_volume_state == ModelInstancePVS_Inside)
                continue;
            const BoundingBoxf3 bb    = mo->instance_bounding_box(ii);
            const char         *where = mi->print_volume_state == ModelInstancePVS_Fully_Outside ? "outside" : "partly outside";
            fail("not-printable",
                 object_label(model, int(oi)) + (mo->instances.size() > 1 ? ", instance " + std::to_string(ii) : std::string()) + " is " + where +
                     " the printable volume: it spans x " + mm(bb.min.x()) + ".." + mm(bb.max.x()) + ", y " + mm(bb.min.y()) + ".." + mm(bb.max.y()) +
                     ", z " + mm(bb.min.z()) + ".." + mm(bb.max.z()) + "; " + bed,
                 int(oi), inside == 0 ? -50 : -52);
        }
    }
    if (inside == 0)
        fail("empty-plate", "no object is printable: every object is marked not printable");
}

// ---------------------------------------------------------------------------------------------
// Layer heights

std::string layer_field(size_t position) { return object_field(position) + ".layerHeights"; }

// The slicing parameters Print uses for the object (set by Print::apply), so a profile is checked
// against exactly the values PrintObject::update_layer_height_profile compares it with.
SlicingParameters object_slicing_parameters(const Print &print, const DynamicPrintConfig &cfg, const ModelObject &mo)
{
    for (const PrintObject *po : print.objects())
        if (po->model_object() != nullptr && po->model_object()->id() == mo.id() && po->slicing_parameters().valid)
            return po->slicing_parameters();
    return PrintObject::slicing_parameters(cfg, mo, float(mo.max_z()), print.shrinkage_compensation());
}

// Adaptive and profile layer heights, set after Print::apply so the slicing parameters (and the
// filament shrinkage compensation) are the ones the print uses, as the GUI's layer editing does.
// Orca drops a profile whose first height is not the first layer height, or that does not end at
// the top of the object (PrintObject::update_layer_height_profile); here such a profile is refused.
bool apply_layer_heights(Model &model, const Job &job, const Print &print, const DynamicPrintConfig &cfg, Warnings &warnings)
{
    bool changed = false;
    for (size_t pos = 0; pos < job.objects.size(); ++pos) {
        const JobObject &jo = job.objects[pos];
        ModelObject     *mo = model.objects[size_t(jo.index)];
        switch (jo.layer_heights.mode) {
        case LayerHeights::Mode::Fixed:
            if (!mo->layer_height_profile.empty()) {
                mo->layer_height_profile.clear();
                changed = true;
            }
            break;
        case LayerHeights::Mode::Adaptive: {
            const SlicingParameters   sp      = object_slicing_parameters(print, cfg, *mo);
            const std::vector<double> profile = layer_height_profile_adaptive(sp, *mo, float(jo.layer_heights.quality));
            mo->layer_height_profile.set(profile);
            changed = true;
            break;
        }
        case LayerHeights::Mode::Profile: {
            const std::string       f  = layer_field(pos) + ".profile";
            const SlicingParameters sp = object_slicing_parameters(print, cfg, *mo);
            std::vector<double>     p  = jo.layer_heights.profile;
            if (std::abs(p[1] - sp.first_object_layer_height) > 1e-6)
                fail("layer-profile-invalid",
                     f + ": the first height is " + mm(p[1]) + " mm, but the first layer is " + mm(sp.first_object_layer_height) + " mm (" +
                         (sp.has_raft() ? "layer_height: the object's first layer, above the raft" : "initial_layer_print_height") +
                         "); start the profile with 0, " + mm(sp.first_object_layer_height),
                     jo.index);
            p[1] = sp.first_object_layer_height;
            const double top    = sp.object_print_z_uncompensated_max - sp.object_print_z_min;
            const double last_z = p[p.size() - 2];
            if (std::abs(last_z - top) > 1e-3)
                fail("layer-profile-invalid", f + ": the last z is " + mm(last_z) + " mm, but the object is " + mm(top) + " mm tall; end the profile at its top", jo.index);
            double nozzle = 0.;
            if (const auto *nd = cfg.option<ConfigOptionFloats>("nozzle_diameter"); nd != nullptr && !nd->values.empty())
                nozzle = *std::min_element(nd->values.begin(), nd->values.end());
            for (size_t i = 1; i < p.size(); i += 2)
                if (nozzle > 0. && p[i] > nozzle + 1e-9)
                    fail("layer-profile-invalid",
                         f + ": the height " + mm(p[i]) + " mm at z " + mm(p[i - 1]) + " is above the nozzle, " + mm(nozzle) +
                             " mm (nozzle_diameter); Orca cannot print a layer taller than the nozzle",
                         jo.index);
            for (size_t i = 1; i < p.size(); i += 2) {
                if (p[i] < sp.min_layer_height - 1e-9 || p[i] > sp.max_layer_height + 1e-9) {
                    warnings.add("layer-height-out-of-range",
                                 f + ": the height " + mm(p[i]) + " mm at z " + mm(p[i - 1]) + " is outside the printer's " + mm(sp.min_layer_height) + ".." +
                                     mm(sp.max_layer_height) + " mm (min_layer_height, max_layer_height)",
                                 jo.index);
                    break;
                }
            }
            mo->layer_height_profile.set(p);
            changed = true;
            break;
        }
        }
    }
    return changed;
}

// ---------------------------------------------------------------------------------------------
// Results of processing

// Every print Z of the plate: the objects' layers and support layers, with Z values closer than
// EPSILON merged to their average, as ToolOrdering::initialize_layers and
// GCode::collect_layers_to_print do. One G-code layer per entry for a print by layer.
std::vector<double> plate_print_z(const Print &print)
{
    std::vector<double> zs;
    for (const PrintObject *po : print.objects()) {
        for (const Layer *l : po->layers())
            zs.push_back(l->print_z);
        for (const SupportLayer *l : po->support_layers())
            zs.push_back(l->print_z);
    }
    std::sort(zs.begin(), zs.end());
    zs.erase(std::unique(zs.begin(), zs.end()), zs.end());
    std::vector<double> out;
    for (size_t i = 0; i < zs.size();) {
        size_t       j    = i + 1;
        const double zmax = zs[i] + EPSILON;
        for (; j < zs.size() && zs[j] <= zmax; ++j)
            ;
        out.push_back(0.5 * (zs[i] + zs[j - 1]));
        i = j;
    }
    return out;
}

json layers_json(const Print &print, const Model &model)
{
    json out = json::array();
    for (const PrintObject *po : print.objects()) {
        const int index = index_of_print_object(model, po);
        if (index < 0)
            continue;
        json ranges = json::array();
        for (const Layer *l : po->layers())
            ranges.push_back(json::array({round_um(l->print_z - l->height), round_um(l->print_z)}));
        json o;
        o["objectIndex"] = index;
        o["ranges"]      = std::move(ranges);
        out.push_back(std::move(o));
    }
    return out;
}

json print_z_json(const std::vector<double> &zs)
{
    json out = json::array();
    for (double z : zs)
        out.push_back(round_um(z));
    return out;
}

// Checks the custom G-codes against the layers Orca made: each one lands on exactly the layer Orca
// will run it on (ToolOrdering::assign_custom_gcodes: the layer whose window, half way to the layers
// below and above, holds printZ; a filament change from the first layer at or above printZ,
// ToolOrdering::collect_extruders), and no two land on one layer (Orca keeps one pause or custom
// G-code per layer). A printZ in no layer's window, below the first or above the last, is refused,
// for a filament change too.
void check_custom_gcodes(const Job &job, const Model &model, const Print &print, const std::vector<double> &zs, Warnings &warnings)
{
    if (job.custom_gcodes.empty())
        return;
    const auto it_info = model.plates_custom_gcodes.find(0);
    if (it_info == model.plates_custom_gcodes.end() || zs.empty())
        return;
    const std::vector<CustomGCode::Item> &items = it_info->second.gcodes;
    const bool any_tool_change = std::any_of(items.begin(), items.end(), [](const CustomGCode::Item &i) { return i.type == CustomGCode::ToolChange; });
    if (any_tool_change && print.object_extruders().size() != 1)
        fail("custom-gcode-invalid", "a filament change at a layer needs every object printed with one filament (Orca switches filaments for the whole plate); these objects use " +
                                         std::to_string(print.object_extruders().size()) + " filaments");
    std::vector<int> taken(zs.size(), -1);
    for (size_t i = 0; i < items.size() && i < job.custom_gcodes.size(); ++i) {
        const CustomGcode       &c    = job.custom_gcodes[i];
        const CustomGCode::Item &item = items[i];
        size_t                   layer = 0;
        if (item.type == CustomGCode::ToolChange) {
            if (c.print_z <= 0.5 * zs.front())
                fail("custom-gcode-invalid", at_print_z(c) + " is below the first layer (print Z " + mm(zs.front()) + "); there is no layer to switch before");
            const auto it = std::find_if(zs.begin(), zs.end(), [&](double z) { return z >= c.print_z - EPSILON; });
            if (it == zs.end())
                fail("custom-gcode-invalid", at_print_z(c) + " is above the last layer (print Z " + mm(zs.back()) + ")");
            layer = size_t(it - zs.begin());
        } else {
            bool found = false;
            for (size_t k = 0; k < zs.size(); ++k) {
                const double below = k == 0 ? 0. : zs[k - 1];
                const double lower = 0.5 * (below + zs[k]);
                const double upper = k + 1 < zs.size() ? 0.5 * (zs[k] + zs[k + 1]) : zs[k] + 0.5 * (zs[k] - below);
                if (c.print_z > lower && c.print_z <= upper) {
                    layer = k;
                    found = true;
                    break;
                }
            }
            if (!found) {
                if (c.print_z <= 0.5 * zs.front())
                    fail("custom-gcode-invalid", at_print_z(c) + " is below the first layer (print Z " + mm(zs.front()) + "); there is no layer to run it before");
                fail("custom-gcode-invalid", at_print_z(c) + " is above the last layer (print Z " + mm(zs.back()) + ")");
            }
        }
        if (taken[layer] >= 0)
            fail("custom-gcode-invalid", at_print_z(job.custom_gcodes[size_t(taken[layer])]) + " and " + at_print_z(c) + " land on the same layer (print Z " +
                                             mm(zs[layer]) + "); Orca runs one per layer");
        taken[layer] = int(i);
        if (std::abs(zs[layer] - c.print_z) > 1e-4)
            warnings.add("custom-gcode-moved", at_print_z(c) + " runs before the layer at print Z " + mm(zs[layer]) + ", the layer it falls on");
    }
}

std::string warning_code(PrintStateBase::SlicingNotificationType type)
{
    switch (type) {
    case PrintStateBase::SlicingReplaceInitEmptyLayers: return "empty-first-layers-replaced";
    case PrintStateBase::SlicingNeedSupportOn: return "support-needed";
    case PrintStateBase::SlicingEmptyGcodeLayers: return "empty-layers";
    case PrintStateBase::SlicingGcodeOverlap: return "gcode-overlap";
    default: return "slicing-warning";
    }
}

std::string unprintable_area_message(int error_code)
{
    std::vector<std::string> parts;
    if (error_code & 0b00001)
        parts.push_back("outside the area an extruder can reach");
    if (error_code & 0b00010)
        parts.push_back("above the height an extruder can reach");
    if (error_code & 0b00100)
        parts.push_back("outside the printable area");
    if (error_code & 0b01000)
        parts.push_back("above the printable height");
    if (error_code & 0b10000)
        parts.push_back("in the clumping detection area");
    if (parts.empty())
        parts.push_back("outside the printable space");
    return "the G-code moves " + join(parts, ", ") + " (Orca's G-code check, code " + std::to_string(error_code) + ")";
}

// ---------------------------------------------------------------------------------------------
// The Bambu .gcode.3mf

void drop_missing_relationships(const std::string &path);

// The plate data and first-layer boxes the GUI writes (PartPlateList::store_to_3mf_structure,
// Plater::export_3mf, the stock command line's plate boxes), for one sliced plate, without
// thumbnails (they need OpenGL).
void export_bambu_3mf(const Job &job, Model &model, Print &print, const GCodeProcessorResult &gres, DynamicPrintConfig &cfg, const Settings &s)
{
    const size_t normal = size_t(PrintEstimatedStatistics::ETimeMode::Normal);
    PlateDataPtrs plates;
    PlateData    *pd = new PlateData();
    plates.push_back(pd);
    pd->plate_index = 0;
    pd->locked      = false;
    for (size_t oi = 0; oi < model.objects.size(); ++oi)
        for (size_t ii = 0; ii < model.objects[oi]->instances.size(); ++ii)
            if (model.objects[oi]->instances[ii]->print_volume_state == ModelInstancePVS_Inside)
                pd->objects_and_instances.emplace_back(int(oi), int(ii));
    if (const auto *maps = cfg.option<ConfigOptionInts>("filament_map"))
        pd->filament_maps = maps->values;
    pd->gcode_file               = gres.filename;
    pd->is_sliced_valid          = true;
    pd->gcode_prediction         = std::to_string(int(gres.print_statistics.modes[normal].time));
    pd->toolpath_outside         = gres.toolpath_outside;
    pd->timelapse_warning_code   = gres.timelapse_warning_code;
    pd->is_label_object_enabled  = gres.label_object_enabled;
    pd->limit_filament_maps      = gres.limit_filament_maps;
    pd->layer_filaments          = gres.layer_filaments;
    pd->filament_change_sequence = gres.filament_change_sequence;
    pd->nozzle_change_sequence   = gres.nozzle_change_sequence;
    pd->optimal_assignment       = gres.optimal_assignment;
    pd->first_layer_time         = std::to_string(gres.initial_layer_time);
    const PrintStatistics &ps    = print.print_statistics();
    if (ps.total_weight != 0.) {
        char buf[64];
        std::snprintf(buf, sizeof(buf), "%.2f", ps.total_weight);
        pd->gcode_weight = buf;
    }
    pd->is_support_used = print.is_support_used();
    pd->parse_filament_info(const_cast<GCodeProcessorResult *>(&gres));
    pd->printer_model_id = s.printer_model_id;
    if (const ConfigOption *nd = cfg.option("nozzle_diameter"))
        pd->nozzle_diameters = nd->serialize();
    const auto *colours = cfg.option<ConfigOptionStrings>("filament_colour");
    const auto *ids     = cfg.option<ConfigOptionStrings>("filament_ids");
    for (FilamentInfo &fi : pd->slice_filaments_info) {
        std::string display;
        fi.type        = cfg.get_filament_type(display, fi.id);
        fi.filament_id = ids && !ids->values.empty() ? ids->get_at(size_t(fi.id)) : "";
        fi.color       = colours && !colours->values.empty() ? colours->get_at(size_t(fi.id)) : "#FFFFFF";
    }

    PlateBBoxData *box = new PlateBBoxData();
    box->is_seq_print  = by_object(cfg);
    // With a prime tower the tool ordering is the wipe tower's (Print::process).
    const ToolOrdering &ordering = print.has_wipe_tower() ? print.get_tool_ordering() : print.tool_ordering();
    const unsigned int  first    = ordering.first_extruder();
    box->first_extruder      = first == (unsigned int) -1 ? 0 : int(first);
    box->bed_type            = bed_type_to_gcode_string(BedType(cfg_enum(cfg, "curr_bed_type", int(btPEI))));
    if (const auto *nd = cfg.option<ConfigOptionFloats>("nozzle_diameter"); nd && !nd->values.empty())
        box->nozzle_diameter = float(nd->get_at(size_t(box->first_extruder)));
    BoundingBoxf all;
    for (size_t i = 0; i < print.objects().size(); ++i) {
        PrintObject *po = print.get_object(i);
        BBoxData     data;
        data.area         = 0.f;
        data.layer_height = 0.f;
        const BoundingBox scaled_bb = po->get_first_layer_bbox(data.area, data.layer_height, data.name);
        const auto        bb        = unscaled(scaled_bb);
        all.merge(bb);
        data.area *= float(SCALING_FACTOR * SCALING_FACTOR);
        data.id   = int(po->id().id);
        data.bbox = {bb.min.x(), bb.min.y(), bb.max.x(), bb.max.y()};
        box->bbox_objs.push_back(std::move(data));
    }
    if (print.has_wipe_tower()) {
        const Points corners = print.first_layer_wipe_tower_corners();
        if (corners.size() >= 3) {
            const BoundingBox scaled_bb(corners[0], corners[2]);
            const auto        bb = unscaled(scaled_bb);
            all.merge(bb);
            BBoxData data;
            data.name         = "wipe_tower";
            data.id           = 1000;
            data.area         = 0.f;
            data.layer_height = 0.f;
            data.bbox         = {bb.min.x(), bb.min.y(), bb.max.x(), bb.max.y()};
            box->bbox_objs.push_back(std::move(data));
        }
    }
    if (all.defined)
        box->bbox_all = {all.min.x(), all.min.y(), all.max.x(), all.max.y()};
    for (const FilamentInfo &fi : pd->slice_filaments_info) {
        box->filament_ids.push_back(fi.id);
        box->filament_colors.push_back(fi.color);
    }
    box->first_layer_time = gres.initial_layer_time;

    StoreParams params;
    params.path             = job.out_bambu3mf.c_str();
    params.model            = &model;
    params.plate_data_list  = plates;
    params.export_plate_idx = -1;
    params.config           = &cfg;
    params.id_bboxes        = {box};
    params.strategy = SaveStrategy::Silence | SaveStrategy::WithGcode | SaveStrategy::SplitModel | SaveStrategy::UseLoadedId | SaveStrategy::ShareMesh;
    bool        ok = false;
    std::string error;
    try {
        make_parent_directories(job.out_bambu3mf);
        ok = store_bbs_3mf(params);
        if (ok)
            drop_missing_relationships(job.out_bambu3mf);
    } catch (const std::exception &e) {
        ok    = false;
        error = e.what();
    }
    release_PlateData_list(plates);
    delete box;
    if (!ok)
        fail("output-failed", "cannot write the .gcode.3mf " + job.out_bambu3mf + (error.empty() ? std::string() : ": " + error));
}

// ---------------------------------------------------------------------------------------------
// The run

[[noreturn]] void fail_slicing(const std::exception &e, const Print &print, const Model &model)
{
    if (const auto *errors = dynamic_cast<const SlicingErrors *>(&e)) {
        std::vector<std::string> messages;
        int                      index = -1;
        for (const SlicingError &se : errors->errors_) {
            messages.push_back(se.what());
            if (index < 0)
                index = index_of_print_object_id(print, model, se.objectId());
        }
        fail("slice-failed", messages.empty() ? std::string(e.what()) : join(messages, "; "), index);
    }
    if (const auto *se = dynamic_cast<const SlicingError *>(&e))
        fail("slice-failed", se->what(), index_of_print_object_id(print, model, se->objectId()));
    const std::string what = e.what();
    if (what.find("G-code export to") != std::string::npos && (what.find("Cannot open the file") != std::string::npos || what.find("disk full") != std::string::npos))
        fail("output-failed", what);
    fail("slice-failed", what.empty() ? std::string("slicing failed") : what);
}

// Orca checks that the filaments of a print by layer can share one nozzle (Print::validate,
// check_multi_filament_valid: each filament's nozzle temperature within the recommended range of
// every other), over the objects' and the support's filaments only. A filament change at a layer
// brings its filament onto the plate as well (Print::extruders with the custom G-code), so here
// those count too, by the same rule, and the failure names the two filaments.
void check_filament_temperatures(const Print &print)
{
    const PrintConfig &pc = print.config();
    if (pc.print_sequence == PrintSequence::ByObject || pc.nozzle_diameter.size() >= 2)
        return;
    const std::vector<unsigned int> used = print.extruders(true);
    if (used.size() < 2)
        return;
    struct Filament
    {
        unsigned int id;
        std::string  type;
        int          temperature, low, high;
    };
    std::vector<Filament> filaments;
    for (unsigned int e : used) {
        Filament f{e, pc.filament_type.get_at(e), pc.nozzle_temperature.get_at(e), pc.nozzle_temperature_range_low.get_at(e),
                   pc.nozzle_temperature_range_high.get_at(e)};
        if (f.low == 0 || f.high == 0) {
            int low = f.low, high = f.high;
            MaterialType::get_temperature_range(f.type, low, high);
            if (f.low == 0)
                f.low = low;
            if (f.high == 0)
                f.high = high;
        }
        filaments.push_back(f);
    }
    const auto name = [](const Filament &f) {
        return "filament " + std::to_string(f.id + 1) + " (" + f.type + ", nozzle " + std::to_string(f.temperature) + ", recommended " +
               std::to_string(f.low) + ".." + std::to_string(f.high) + ")";
    };
    for (const Filament &f : filaments)
        if (f.low >= f.high)
            fail("temperature-difference", name(f) + " has an empty recommended nozzle temperature range: the lower bound must be below the upper "
                                                     "(nozzle_temperature_range_low, nozzle_temperature_range_high)");
    for (size_t i = 0; i < filaments.size(); ++i)
        for (size_t j = i + 1; j < filaments.size(); ++j) {
            const Filament &a = filaments[i], &b = filaments[j];
            if (a.temperature < b.low || a.temperature > b.high || b.temperature < a.low || b.temperature > a.high)
                fail("temperature-difference", name(a) + " and " + name(b) +
                                                   " print on this plate but cannot share the nozzle: each filament's nozzle temperature must be "
                                                   "within the recommended range of the other (degrees C)");
        }
}

// Orca's validate message for filaments that cannot share a nozzle ends with the GUI preference that
// lifts the rule, which the engine does not have.
std::string without_gui_hint(std::string text)
{
    static const char *const hints[] = {
        " If you still want to print, you can enable the option in Preferences / Control / Slicing / Remove mixed temperature restriction.",
        "If you still want to print, you can enable the option in Preferences / Control / Slicing / Remove mixed temperature restriction."};
    for (const char *hint : hints)
        if (const size_t at = text.find(hint); at != std::string::npos)
            text.erase(at, std::strlen(hint));
    return text;
}

// The object a path conflict is about: the one object it involves (two of its instances, or the
// object and the prime tower), none for two objects. The conflict names instances by address.
int conflict_object_index(const Print &print, const Model &model, const ConflictResult &c)
{
    const auto index_of = [&](const void *p) {
        if (p == nullptr)
            return -2; // the prime tower
        for (const PrintObject *po : print.objects())
            for (const PrintInstance &pi : po->instances())
                if (static_cast<const void *>(&pi) == p)
                    return index_of_print_object(model, po);
        return -1;
    };
    const int a = index_of(c._obj1), b = index_of(c._obj2);
    if (a == -2)
        return b;
    if (b == -2)
        return a;
    return a == b ? a : -1;
}

// A G-code whose last extrusion is below the top layer stops before the print is done; that is a
// failed slice, not a print. The processor's positions are print Z: the G-code's Z is print Z plus
// z_offset (GCode::change_layer), and the processor takes z_offset back out of every move it stores
// (GCodeProcessor::store_move_vertex, m_end_position[Z] - m_z_offset), so the top is compared with
// the last print Z as it is.
void check_gcode_reaches_top(const GCodeProcessorResult &gres, const std::vector<double> &zs)
{
    if (zs.empty())
        return;
    double top   = -std::numeric_limits<double>::infinity();
    bool   moved = false;
    for (const GCodeProcessorResult::MoveVertex &m : gres.moves)
        if (m.type == EMoveType::Extrude && m.delta_extruder > 0.f) {
            top   = std::max(top, double(m.position.z()));
            moved = true;
        }
    const double last_height = zs.size() > 1 ? zs.back() - zs[zs.size() - 2] : zs.back();
    const double want        = zs.back() - 0.5 * last_height - 0.01;
    if (!moved || top < want)
        fail("slice-failed", "the G-code stops below the top of the print: its last extrusion is at z " + (moved ? mm(top) : std::string("none")) +
                                 ", the last layer at print Z " + mm(zs.back()) + "; Orca left the rest of the print out");
}

std::string xml_unescape(const std::string &s)
{
    static const std::pair<const char *, char> entities[] = {{"&amp;", '&'}, {"&lt;", '<'}, {"&gt;", '>'}, {"&quot;", '"'}, {"&apos;", '\''}};
    std::string out;
    for (size_t i = 0; i < s.size();) {
        bool matched = false;
        if (s[i] == '&')
            for (const auto &e : entities)
                if (s.compare(i, std::strlen(e.first), e.first) == 0) {
                    out += e.second;
                    i += std::strlen(e.first);
                    matched = true;
                    break;
                }
        if (!matched)
            out += s[i++];
    }
    return out;
}

// Orca writes the package's thumbnail relationships whether or not the package has thumbnails
// (_BBS_3MF_Exporter::_add_relationships_file_to_archive), and the engine writes none. Every
// relationship whose target is not in the package is taken out again, so every one names a part.
void drop_missing_relationships(const std::string &path)
{
    mz_zip_archive in;
    mz_zip_zero_struct(&in);
    if (!open_zip_reader(&in, path))
        throw std::runtime_error("cannot read it back");
    std::vector<std::string> names;
    std::map<mz_uint, std::string> rewritten;
    std::string error;
    const mz_uint count = mz_zip_reader_get_num_files(&in);
    for (mz_uint i = 0; i < count && error.empty(); ++i) {
        mz_zip_archive_file_stat st;
        if (!mz_zip_reader_file_stat(&in, i, &st))
            error = "cannot read entry " + std::to_string(i);
        else
            names.push_back(st.m_filename);
    }
    const std::set<std::string> present(names.begin(), names.end());
    for (mz_uint i = 0; i < names.size() && error.empty(); ++i) {
        if (!ends_with_ci(names[i], ".rels"))
            continue;
        size_t size = 0;
        void  *data = mz_zip_reader_extract_to_heap(&in, i, &size, 0);
        if (data == nullptr) {
            error = "cannot read " + names[i];
            break;
        }
        const std::string text(static_cast<const char *>(data), size);
        mz_free(data);
        std::string kept;
        bool        dropped = false;
        size_t      at      = 0;
        while (at < text.size()) {
            size_t end = text.find('\n', at);
            end        = end == std::string::npos ? text.size() : end + 1;
            const std::string line   = text.substr(at, end - at);
            const size_t      target = line.find("Target=\"/");
            if (line.find("<Relationship ") != std::string::npos && target != std::string::npos) {
                const size_t from  = target + 9;
                const size_t close = line.find('"', from);
                if (close != std::string::npos && present.count(xml_unescape(line.substr(from, close - from))) == 0) {
                    dropped = true;
                    at      = end;
                    continue;
                }
            }
            kept += line;
            at = end;
        }
        if (dropped)
            rewritten[i] = kept;
    }
    if (!error.empty() || rewritten.empty()) {
        close_zip_reader(&in);
        if (!error.empty())
            throw std::runtime_error(error);
        return;
    }
    const std::string tmp = path + ".rels.tmp";
    mz_zip_archive    out;
    mz_zip_zero_struct(&out);
    if (!open_zip_writer(&out, tmp)) {
        close_zip_reader(&in);
        throw std::runtime_error("cannot write " + tmp);
    }
    for (mz_uint i = 0; i < names.size() && error.empty(); ++i) {
        const auto it = rewritten.find(i);
        const bool ok = it != rewritten.end() ?
                            mz_zip_writer_add_mem(&out, names[i].c_str(), it->second.data(), it->second.size(), MZ_DEFAULT_COMPRESSION) :
                            mz_zip_writer_add_from_zip_reader(&out, &in, i);
        if (!ok)
            error = "cannot copy " + names[i];
    }
    if (error.empty() && !mz_zip_writer_finalize_archive(&out))
        error = "cannot finish " + tmp;
    close_zip_writer(&out);
    close_zip_reader(&in);
    boost::system::error_code ec;
    if (error.empty()) {
        fs::rename(fs::path(tmp), fs::path(path), ec);
        if (ec)
            error = "cannot replace it: " + ec.message();
    }
    if (!error.empty()) {
        fs::remove(fs::path(tmp), ec);
        throw std::runtime_error(error);
    }
}

struct Critical
{
    std::string text;
    int         object_index = -1;
};

void run(const Job &job, json &result, Warnings &warnings, Progress &progress)
{
    progress.line("presets", 3, "reading the presets");
    set_up_folders(job);
    const Settings s = load_settings(job, warnings);
    const size_t   n = s.filaments.size();

    progress.line("model", 8, "reading the model");
    Model model = load_model(job);
    for (size_t pos = 0; pos < job.objects.size(); ++pos)
        if (size_t(job.objects[pos].index) >= model.objects.size())
            fail("job-invalid", object_field(pos) + ".index is " + std::to_string(job.objects[pos].index) + ", but the model has " +
                                    std::to_string(model.objects.size()) + " object(s)");
    if (job.command != Command::Info || job.orient)
        check_solid_objects(model);
    if (job.orient) {
        progress.line("orient", 10, "orienting the objects");
        orient_objects(model);
    }
    if (job.command == Command::Info) {
        result["info"] = info_json(model, s);
        return;
    }

    DynamicPrintConfig cfg = s.config;
    apply_object_settings(model, job, s);
    set_custom_gcodes(model, job, s);
    const std::optional<arrangement::ArrangePolygon> tower = place_prime_tower(cfg, model, s);
    if (job.arrange) {
        progress.line("arrange", 12, "arranging the objects on the plate");
        arrange_objects(model, cfg, tower);
    }
    check_build_volume(model, cfg);

    // OrcaSlicer.cpp: the plate's print configuration.
    DynamicPrintConfig new_cfg = cfg;
    {
        std::vector<int> &maps = new_cfg.option<ConfigOptionInts>("filament_map", true)->values;
        if (maps.size() < n)
            maps.resize(n, 1);
        if (s.extruder_count == 1)
            for (size_t i = 0; i < n; ++i)
                maps[i] = 1;
        if (!new_cfg.has("nozzle_volume_type"))
            new_cfg.option<ConfigOptionEnumsGeneric>("nozzle_volume_type", true)->values.resize(s.extruder_count, nvtStandard);
    }
    Print print;
    if (s.extruder_count > 1 && cfg_enum(new_cfg, "filament_map_mode", int(fmmAutoForFlush)) < int(fmmManual)) {
        // The stock command line's defaults for the automatic filament-to-nozzle map.
        std::vector<std::string>                     ams_count(s.extruder_count, "");
        std::vector<std::vector<DynamicPrintConfig>> filament_info(s.extruder_count);
        const auto              *type_opt = new_cfg.option<ConfigOptionStrings>("filament_type");
        std::vector<std::string> types    = type_opt ? type_opt->vserialize() : std::vector<std::string>{"PLA"};
        if (types.empty())
            types.push_back("PLA");
        size_t color_count = 0;
        for (size_t e = 0; e < s.extruder_count; ++e) {
            ams_count[e] = "1#0|4#1";
            for (int c = 0; c < 4; ++c) {
                DynamicPrintConfig t;
                t.option<ConfigOptionStrings>("filament_colour", true)->values = {"#FFFFFFFF"};
                t.option<ConfigOptionStrings>("filament_type", true)->values   = {types[color_count % types.size()]};
                t.option<ConfigOptionBools>("filament_is_support", true)->values = {0};
                filament_info[e].push_back(std::move(t));
                ++color_count;
            }
        }
        new_cfg.option<ConfigOptionStrings>("extruder_ams_count", true)->values = ams_count;
        print.set_extruder_filament_info(filament_info);
    }

    progress.line("slice", 14, "preparing the print");
    print.set_plate_origin(Vec3d::Zero());
    print.apply(model, new_cfg);
    if (print.empty())
        fail("empty-plate", "nothing to slice: no object is printable with these settings");
    if (apply_layer_heights(model, job, print, new_cfg, warnings))
        print.apply(model, new_cfg);
    print.set_no_check_flag(false);
    print.is_BBL_printer() = s.is_bbl;
    print.set_check_multi_filaments_compatibility(true);
    check_filament_temperatures(print);

    {
        StringObjectException warning;
        warning.type             = STRING_EXCEPT_NOT_DEFINED;
        StringObjectException err = print.validate(&warning);
        if (!err.string.empty()) {
            std::string code      = "validate-error";
            int         exit_code = 0;
            switch (err.type) {
            case STRING_EXCEPT_FILAMENT_NOT_MATCH_BED_TYPE: code = "filament-bed-mismatch"; break;
            case STRING_EXCEPT_FILAMENTS_DIFFERENT_TEMP: code = "temperature-difference"; err.string = without_gui_hint(err.string); break;
            case STRING_EXCEPT_OBJECT_COLLISION_IN_SEQ_PRINT: code = "collision"; exit_code = -63; break;
            case STRING_EXCEPT_OBJECT_COLLISION_IN_LAYER_PRINT: code = "collision"; exit_code = -64; break;
            default: break;
            }
            fail(code, err.string, index_of_object_base(print, model, err.object), exit_code);
        }
        if (!warning.string.empty())
            warnings.add("validate-warning", warning.string, index_of_object_base(print, model, warning.object));
    }
    if (print.empty())
        fail("empty-plate", "nothing to slice: no object is printable with these settings");

    std::mutex            critical_mutex;
    std::vector<Critical> critical;
    std::atomic<bool>     exporting{false};
    const Print          &cprint = print;
    print.set_status_callback([&](const PrintBase::SlicingStatus &st) {
        if (st.warning_step != -1) {
            if (st.text.empty())
                return;
            int index = -1;
            if (st.flags & PrintBase::SlicingStatus::UPDATE_PRINT_OBJECT_STEP_WARNINGS)
                index = index_of_print_object(model, cprint.get_object(st.warning_object_id));
            warnings.add(warning_code(st.message_type), st.text, index);
            if (st.warning_level == PrintStateBase::WarningLevel::CRITICAL &&
                (st.message_type == PrintStateBase::SlicingEmptyGcodeLayers || st.message_type == PrintStateBase::SlicingGcodeOverlap)) {
                std::lock_guard<std::mutex> lock(critical_mutex);
                critical.push_back({st.text, index});
            }
            return;
        }
        if (st.percent >= 0)
            progress.line(exporting ? "gcode" : "slice", 15. + 0.8 * double(st.percent), st.text);
    });
    const auto check_critical = [&]() {
        std::lock_guard<std::mutex> lock(critical_mutex);
        if (!critical.empty())
            fail("slice-failed", critical.front().text, critical.front().object_index);
    };

    Model::setExtruderParams(new_cfg, int(n));
    Model::setPrintSpeedTable(new_cfg, print.config());
    try {
        print.process();
    } catch (const Failure &) {
        throw;
    } catch (const std::bad_alloc &) {
        fail("out-of-memory", "out of memory while slicing");
    } catch (const std::exception &e) {
        fail_slicing(e, print, model);
    }
    check_critical();
    if (const std::string conflict = print.get_conflict_string(); !conflict.empty()) {
        std::string message = conflict;
        int         index   = -1;
        if (const auto c = print.get_conflict_result(); c) {
            message += " at z " + mm(c->_height);
            index = conflict_object_index(print, model, *c);
        }
        fail("path-conflicts", message, index);
    }

    const std::vector<double> zs = plate_print_z(print);
    if (!by_object(new_cfg))
        check_custom_gcodes(job, model, print, zs, warnings);
    result["layers"] = layers_json(print, model);
    result["printZ"] = print_z_json(zs);

    if (job.command == Command::Layers)
        return;

    progress.line("gcode", 80, "writing the G-code");
    // Orca numbers the objects (PrintObject::set_id) only for printers that take EXCLUDE_OBJECT
    // labels and otherwise leaves the id uninitialised, though the G-code prints it in every
    // "; printing object <name> id:<id>" line. The same numbering for every printer keeps the
    // G-code identical from run to run.
    for (size_t i = 0; i < print.objects().size(); ++i)
        print.get_object(i)->set_id(i);
    try {
        make_parent_directories(job.out_gcode);
    } catch (const std::exception &e) {
        fail("output-failed", "cannot write the G-code " + job.out_gcode + ": " + e.what());
    }
    GCodeProcessorResult gres;
    std::string          gcode_path;
    exporting = true;
    try {
        gcode_path = print.export_gcode(job.out_gcode, &gres, nullptr);
    } catch (const Failure &) {
        throw;
    } catch (const std::bad_alloc &) {
        fail("out-of-memory", "out of memory while writing the G-code");
    } catch (const std::exception &e) {
        fail_slicing(e, print, model);
    }
    exporting = false;
    check_critical();
    if (gres.gcode_check_result.error_code != 0)
        fail("gcode-in-unprintable-area", unprintable_area_message(gres.gcode_check_result.error_code));
    check_gcode_reaches_top(gres, zs);
    add_gcode_warnings(gres, warnings);

    json outputs;
    outputs["gcode"]    = gcode_path;
    outputs["bambu3mf"] = "";
    if (!job.out_bambu3mf.empty()) {
        progress.line("3mf", 96, "writing the .gcode.3mf");
        export_bambu_3mf(job, model, print, gres, new_cfg, s);
        outputs["bambu3mf"] = job.out_bambu3mf;
    }
    result["outputs"] = std::move(outputs);
    if (job.stats) {
        progress.line("stats", 98, "reading the estimates");
        result["stats"] = compute_stats(gres, print.config(), zs.size(), by_object(new_cfg));
    }
}

json ordered_result(bool ok, const std::string &code, const std::string &message, int object_index, int exit_code, const json &body, const Warnings &warnings)
{
    json out;
    out["format"]  = RESULT_FORMAT;
    out["ok"]      = ok;
    out["code"]    = code;
    out["message"] = message;
    if (object_index >= 0)
        out["objectIndex"] = object_index;
    out["exitCode"] = exit_code;
    out["warnings"] = warnings_json(warnings);
    for (const char *key : {"command", "info", "layers", "printZ", "outputs", "stats"})
        if (body.contains(key))
            out[key] = body[key];
    out["engine"] = version_json();
    return out;
}

} // namespace

json version_json()
{
    json j;
    j["engine"]        = "sketch-orca";
    j["engineVersion"] = SKETCH_ORCA_VERSION;
    j["orcaVersion"]   = SoftFever_VERSION;
    j["orcaCommit"]    = SKETCH_ORCA_COMMIT;
    j["orcaTag"]       = SKETCH_ORCA_TAG;
    return j;
}

bool write_failure_result(const std::string &out_path, const std::string &code, const std::string &message, int exit_code)
{
    Warnings none;
    try {
        write_file_atomic(out_path, ordered_result(false, code, message, -1, exit_code, json::object(), none).dump(2, ' ', false, json::error_handler_t::replace) + "\n");
        return true;
    } catch (const std::exception &) {
        return false;
    }
}

// Keeps a failure result ready for a crash in Orca's code (install_crash_handler).
static void arm_crash(const std::string &out_path, const json &body)
{
    static const char *const MARK = "@@sketch-orca-crash-message@@";
    Warnings                 none;
    const std::string        text =
        ordered_result(false, "internal-error", MARK, -1, exit_code_for("internal-error"), body, none).dump(2, ' ', false, json::error_handler_t::replace) +
        "\n";
    const size_t at = text.find(MARK);
    if (at != std::string::npos)
        arm_crash_result(out_path, text.substr(0, at), text.substr(at + std::strlen(MARK)));
}

int run_job(const std::string &job_path, const std::string &out_path, const std::string &progress_path)
{
    Warnings    warnings;
    Progress    progress;
    json        body = json::object();
    bool        ok   = true;
    std::string code = "ok", message;
    int         object_index = -1, exit_code = 0;
    arm_crash(out_path, body);
    if (!progress_path.empty() && !progress.open(progress_path))
        warnings.add("progress-unwritable", "cannot write the progress file " + progress_path);
    try {
        progress.line("job", 1, "reading the job");
        std::string text;
        try {
            text = read_file(job_path);
        } catch (const std::exception &) {
            fail(is_file(job_path) ? "job-invalid" : "file-not-found", "cannot read the job " + job_path);
        }
        const Job job   = parse_job(text);
        body["command"] = job.command_name;
        arm_crash(out_path, body);
        run(job, body, warnings, progress);
    } catch (const Failure &f) {
        ok           = false;
        code         = f.code;
        message      = f.what();
        object_index = f.object_index;
        exit_code    = f.exit_code;
    } catch (const std::bad_alloc &) {
        ok        = false;
        code      = "out-of-memory";
        message   = "out of memory";
        exit_code = exit_code_for(code);
    } catch (const std::exception &e) {
        ok        = false;
        code      = "internal-error";
        message   = e.what();
        exit_code = exit_code_for(code);
    }
    // A failed result holds no partial results: the failure, the warnings and the command only.
    if (!ok)
        for (const char *key : {"info", "layers", "printZ", "outputs", "stats"})
            body.erase(key);
    const json result = ordered_result(ok, code, message, object_index, exit_code, body, warnings);
    disarm_crash_result();
    try {
        write_file_atomic(out_path, result.dump(2, ' ', false, json::error_handler_t::replace) + "\n");
    } catch (const std::exception &e) {
        std::fprintf(stderr, "sketch-orca: cannot write the result %s: %s\n", out_path.c_str(), e.what());
        return ok ? exit_code_for("output-failed") : exit_code;
    }
    progress.finish(ok, ok ? "done" : message);
    return exit_code;
}

} // namespace sketch_orca
