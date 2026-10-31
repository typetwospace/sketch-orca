// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

#include "job.hpp"

#include <algorithm>
#include <cmath>
#include <regex>
#include <set>

#include "common.hpp"

namespace sketch_orca {

namespace {

const char *JOB_FORMAT = "sketch-orca-job/1";

// Two custom G-codes closer than this are at the same print Z (Orca runs one per layer).
const double Z_EPSILON = 1e-6;

[[noreturn]] void bad(const std::string &message) { fail("job-invalid", message); }

std::string quoted(const json &v) { return v.dump(-1, ' ', false, json::error_handler_t::replace); }

const json &record(const json &v, const std::string &field)
{
    if (!v.is_object())
        bad(field + " must be an object");
    return v;
}

std::string text(const json &v, const std::string &field)
{
    if (!v.is_string())
        bad(field + " must be text");
    return v.get<std::string>();
}

// An optional field given as null is the same as one left out, here and in job.ts.
bool boolean(const json &obj, const char *key, const std::string &field, bool fallback)
{
    auto it = obj.find(key);
    if (it == obj.end() || it->is_null())
        return fallback;
    if (!it->is_boolean())
        bad(field + " must be true or false");
    return it->get<bool>();
}

double number(const json &v, const std::string &field)
{
    if (!v.is_number())
        bad(field + " must be a number");
    double d = v.get<double>();
    if (!std::isfinite(d))
        bad(field + " must be a number");
    return d;
}

bool is_whole(double d) { return std::floor(d) == d; }

std::string abs_path(const json &v, const std::string &field, const std::vector<std::string> &extensions = {})
{
    std::string p = text(v, field);
    if (p.empty() || !is_absolute_path(p))
        bad(field + " must be an absolute path, got " + quoted(v));
    if (!extensions.empty()) {
        bool ok = false;
        for (const std::string &e : extensions)
            ok = ok || ends_with_ci(p, e);
        if (!ok)
            bad(field + " must end with " + join(extensions, " or ") + ", got " + quoted(v));
    }
    return p;
}

const json &member(const json &obj, const char *key)
{
    static const json missing;
    auto it = obj.find(key);
    return it == obj.end() ? missing : *it;
}

bool has(const json &obj, const char *key)
{
    auto it = obj.find(key);
    return it != obj.end() && !it->is_null();
}

// Values as Orca writes them; numbers and true/false become that text. A key is a letter, then
// letters, digits and _: most of Orca's keys are lower case, but not all (the filament setting
// required_nozzle_HRC). Whether Orca has the setting is for presets.cpp (parse_setting) to say,
// with setting-invalid. The keys that belong to the preset files are refused in any letter case,
// as Orca's loader reads type, name, from and inherits in any case (ConfigBase::load_from_json).
Overrides overrides(const json &v, const std::string &field)
{
    Overrides out;
    if (v.is_null())
        return out;
    record(v, field);
    static const std::regex key_re("^[A-Za-z][A-Za-z0-9_]*$");
    static const std::set<std::string> reserved = {"type", "name", "from", "inherits", "compatible_printers"};
    for (auto it = v.begin(); it != v.end(); ++it) {
        const std::string k = it.key();
        if (!std::regex_match(k, key_re))
            bad(field + ": \"" + k + "\" is not an Orca setting key");
        if (reserved.count(lower(k)))
            bad(field + ": \"" + k + "\" belongs to the preset files, not to overrides");
        const json &raw = it.value();
        std::string value;
        if (raw.is_string())
            value = raw.get<std::string>();
        else if (raw.is_boolean())
            value = raw.get<bool>() ? "1" : "0";
        else if (raw.is_number() && std::isfinite(raw.get<double>()))
            value = raw.dump();
        else
            bad(field + "." + k + " must be text (the value as Orca writes it)");
        out.emplace_back(k, value);
    }
    return out;
}

LayerHeights layer_heights(const json &v, const std::string &field, int object_index)
{
    LayerHeights lh;
    if (v.is_null())
        return lh;
    record(v, field);
    const json &mode = member(v, "mode");
    if (mode == "fixed")
        return lh;
    if (mode == "adaptive") {
        lh.mode    = LayerHeights::Mode::Adaptive;
        lh.quality = has(v, "quality") ? number(member(v, "quality"), field + ".quality") : 0.5;
        if (lh.quality < 0. || lh.quality > 1.)
            bad(field + ".quality must be between 0 (finest) and 1 (coarsest), got " + format_number(lh.quality));
        return lh;
    }
    if (mode == "profile") {
        lh.mode         = LayerHeights::Mode::Profile;
        const json &arr = member(v, "profile");
        if (!arr.is_array())
            bad(field + ".profile must be a list [z0, h0, z1, h1, ...]");
        for (size_t i = 0; i < arr.size(); ++i)
            lh.profile.push_back(number(arr[i], field + ".profile[" + std::to_string(i) + "]"));
        const std::vector<double> &p = lh.profile;
        if (p.size() < 4 || p.size() % 2 != 0)
            bad(field + ".profile must hold pairs of z and height, at least two");
        if (p[0] != 0.)
            fail("layer-profile-invalid", field + ".profile must start at z 0, the bottom of the object", object_index);
        for (size_t i = 0; i < p.size(); i += 2) {
            if (i > 0 && p[i] < p[i - 2])
                bad(field + ".profile: z " + format_number(p[i]) + " comes after z " + format_number(p[i - 2]));
            if (!(p[i + 1] > 0.))
                bad(field + ".profile: the height at z " + format_number(p[i]) + " must be above 0");
        }
        return lh;
    }
    bad(field + ".mode must be fixed, adaptive or profile, got " + quoted(mode));
}

std::vector<JobObject> objects(const json &v)
{
    std::vector<JobObject> out;
    if (v.is_null())
        return out;
    if (!v.is_array())
        bad("objects must be a list");
    std::set<int> seen;
    for (size_t i = 0; i < v.size(); ++i) {
        const std::string f = "objects[" + std::to_string(i) + "]";
        const json &o = record(v[i], f);
        JobObject jo;
        const double index = number(member(o, "index"), f + ".index");
        if (!is_whole(index) || index < 0 || index > 1e9)
            bad(f + ".index must be a whole number from 0");
        jo.index = int(index);
        if (!seen.insert(jo.index).second)
            bad(f + ".index " + std::to_string(jo.index) + " is listed twice");
        const json &ranges = member(o, "heightRanges");
        if (!ranges.is_null()) {
            if (!ranges.is_array())
                bad(f + ".heightRanges must be a list");
            for (size_t j = 0; j < ranges.size(); ++j) {
                const std::string rf = f + ".heightRanges[" + std::to_string(j) + "]";
                const json &r = record(ranges[j], rf);
                HeightRange hr;
                hr.from = number(member(r, "from"), rf + ".from");
                hr.to   = number(member(r, "to"), rf + ".to");
                if (hr.from < 0. || !(hr.to > hr.from))
                    bad(rf + " must have 0 <= from < to, got " + format_number(hr.from) + ".." + format_number(hr.to));
                hr.overrides = overrides(member(r, "overrides"), rf + ".overrides");
                jo.height_ranges.push_back(std::move(hr));
            }
            for (size_t j = 1; j < jo.height_ranges.size(); ++j)
                if (jo.height_ranges[j].from < jo.height_ranges[j - 1].to)
                    bad(f + ".heightRanges[" + std::to_string(j) + "] starts at " + format_number(jo.height_ranges[j].from) +
                        ", inside the range before it (to " + format_number(jo.height_ranges[j - 1].to) + ")");
        }
        jo.overrides     = overrides(member(o, "overrides"), f + ".overrides");
        jo.layer_heights = layer_heights(member(o, "layerHeights"), f + ".layerHeights", jo.index);
        out.push_back(std::move(jo));
    }
    return out;
}

std::vector<CustomGcode> custom_gcodes(const json &v)
{
    std::vector<CustomGcode> out;
    if (v.is_null())
        return out;
    if (!v.is_array())
        bad("customGcodes must be a list");
    static const std::regex color_re("^#[0-9a-fA-F]{6}$");
    for (size_t i = 0; i < v.size(); ++i) {
        const std::string f = "customGcodes[" + std::to_string(i) + "]";
        const json &o = record(v[i], f);
        CustomGcode c;
        c.print_z = number(member(o, "printZ"), f + ".printZ");
        if (!(c.print_z > 0.))
            fail("custom-gcode-invalid", f + ".printZ must be above 0: there is no layer below the first");
        const json &type = member(o, "type");
        if (!type.is_string() || (type != "pause" && type != "color" && type != "filament" && type != "custom"))
            bad(f + ".type must be pause, color, filament, custom, got " + quoted(type));
        c.type = type.get<std::string>();
        if (has(o, "extruder")) {
            const double e = number(member(o, "extruder"), f + ".extruder");
            if (!is_whole(e) || e < 1 || e > 1e6)
                bad(f + ".extruder must be a whole number from 1");
            c.extruder = int(e);
        } else if (c.type == "filament") {
            bad(f + ": a filament change needs the extruder to switch to");
        }
        if (has(o, "color")) {
            c.color = text(member(o, "color"), f + ".color");
            if (!std::regex_match(c.color, color_re))
                bad(f + ".color must be #rrggbb, got " + quoted(member(o, "color")));
            c.color = lower(c.color);
        } else if (c.type == "color") {
            bad(f + ": a color change needs the color");
        }
        if (has(o, "gcode"))
            c.gcode = text(member(o, "gcode"), f + ".gcode");
        if (c.type == "custom") {
            bool blank = std::all_of(c.gcode.begin(), c.gcode.end(), [](unsigned char ch) { return std::isspace(ch) != 0; });
            if (blank)
                bad(f + ": a custom G-code needs its gcode");
        }
        if (has(o, "note"))
            c.note = text(member(o, "note"), f + ".note");
        out.push_back(std::move(c));
    }
    std::stable_sort(out.begin(), out.end(), [](const CustomGcode &a, const CustomGcode &b) { return a.print_z < b.print_z; });
    for (size_t i = 1; i < out.size(); ++i)
        if (out[i].print_z - out[i - 1].print_z < Z_EPSILON)
            fail("custom-gcode-invalid", "two custom G-codes at print Z " + format_number(out[i].print_z) + "; Orca runs one per layer");
    return out;
}

} // namespace

Job parse_job(const std::string &text_in)
{
    json root;
    try {
        root = json::parse(text_in);
    } catch (const std::exception &e) {
        bad(std::string("the job is not JSON (") + e.what() + ")");
    }
    record(root, "job");
    if (member(root, "format") != JOB_FORMAT)
        bad(std::string("format must be \"") + JOB_FORMAT + "\", got " + quoted(member(root, "format")));

    Job job;
    const json &command = member(root, "command");
    if (command == "slice")
        job.command = Command::Slice;
    else if (command == "layers")
        job.command = Command::Layers;
    else if (command == "info")
        job.command = Command::Info;
    else
        bad("command must be slice, layers, info, got " + quoted(command));
    job.command_name = command.get<std::string>();

    const json &model   = record(member(root, "model"), "model");
    const json &presets = record(member(root, "presets"), "presets");
    const json &fils    = member(presets, "filaments");
    if (!fils.is_array() || fils.empty())
        bad("presets.filaments must list at least one filament preset");

    const json &output = member(root, "output");
    if (output.is_null()) {
        if (job.command == Command::Slice)
            bad("a slice job needs output.gcode");
        job.stats = false;
    } else {
        record(output, "output");
        const json &gcode = member(output, "gcode");
        if (job.command == Command::Slice || (!gcode.is_null() && gcode != ""))
            job.out_gcode = abs_path(gcode, "output.gcode", {".gcode"});
        const json &b3mf = member(output, "bambu3mf");
        if (!b3mf.is_null())
            job.out_bambu3mf = abs_path(b3mf, "output.bambu3mf", {".3mf"});
        job.stats = boolean(output, "stats", "output.stats", true);
    }

    job.resources  = abs_path(member(root, "resources"), "resources");
    job.work_dir   = abs_path(member(root, "workDir"), "workDir");
    job.model_path = abs_path(member(model, "path"), "model.path", {".3mf", ".stl", ".obj"});
    job.machine    = abs_path(member(presets, "machine"), "presets.machine", {".json"});
    job.process    = abs_path(member(presets, "process"), "presets.process", {".json"});
    for (size_t i = 0; i < fils.size(); ++i)
        job.filaments.push_back(abs_path(fils[i], "presets.filaments[" + std::to_string(i) + "]", {".json"}));
    job.overrides     = overrides(member(root, "overrides"), "overrides");
    job.objects       = objects(member(root, "objects"));
    job.custom_gcodes = custom_gcodes(member(root, "customGcodes"));
    job.arrange       = boolean(root, "arrange", "arrange", true);
    job.orient        = boolean(root, "orient", "orient", false);
    return job;
}

} // namespace sketch_orca
