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
// configdef.json: the engine's own configuration definitions, from libslic3r as the engine links
// it, for the Rust translation's generated definitions (bun scripts/orca.ts parity gen-config
// writes crates/orca-config/src/def_generated.rs from it). Unlike the front end's config-def
// command (cli/config_def.cpp), which lists what a client may set, this lists every option of
// print_config_def with every field the definitions hold (SLA and coNone options and obsolete
// keys included), each float limit as its bits, each enum's map of names to values, the preset
// sections' key lists in their order, and handle_legacy's result for every legacy key and every
// defined key with a set of values. The JSON is nlohmann::ordered_json, dumped as the front end
// dumps its files (indent 1, invalid UTF-8 replaced), so it reads the same everywhere.
#include <cfloat>
#include <cstdint>
#include <cstring>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

#include "libslic3r/Preset.hpp"
#include "libslic3r/PrintConfig.hpp"
#include "libslic3r_version.h"

#include "probe.hpp"

namespace probe {

namespace {

using json = nlohmann::ordered_json;
using namespace Slic3r;

// The keys PrintConfigDef::handle_legacy names in the Orca source the engine is built from, as the
// front end reads them (cli/CMakeLists.txt writes legacy_keys.inc into the oracle's build tree).
const char *const LEGACY_KEYS[] = {
#include "legacy_keys.inc"
};

// The values each key is read with: the front end's probe values (cli/config_def.cpp) and the
// values handle_legacy compares with or rewrites.
const char *const PROBE_VALUES[] = {"",       "0",           "1",           "-1",   "2",      "none",  "50%",  "nil",
                                    "50",     "0.2",         "true",        "false", "auto",  "spiral", "1,0", "inner wall/outer wall",
                                    "outer wall/inner wall", "inner-outer-inner wall", "default", "rectilinear", "normal", "tree"};

uint32_t float_bits(float f)
{
    uint32_t u;
    std::memcpy(&u, &f, 4);
    return u;
}

json string_list(const std::vector<std::string> &v)
{
    json a = json::array();
    for (const std::string &s : v)
        a.push_back(s);
    return a;
}

json option_json(const std::string &key, const ConfigOptionDef &def)
{
    json o;
    o["key"]               = key;
    o["optKey"]            = def.opt_key;
    o["type"]              = int(def.type);
    o["nullable"]          = def.nullable;
    o["printerTechnology"] = int(def.printer_technology);
    o["category"]          = def.category;
    o["label"]             = def.label;
    o["fullLabel"]         = def.full_label;
    o["tooltip"]           = def.tooltip;
    o["sidetext"]          = def.sidetext;
    o["cli"]               = def.cli;
    o["cliParams"]         = def.cli_params;
    o["ratioOver"]         = def.ratio_over;
    o["multiline"]         = def.multiline;
    o["fullWidth"]         = def.full_width;
    o["isCode"]            = def.is_code;
    o["readonly"]          = def.readonly;
    o["height"]            = def.height;
    o["width"]             = def.width;
    o["minBits"]           = float_bits(def.min);
    o["maxBits"]           = float_bits(def.max);
    o["min"]               = double(def.min);
    o["max"]               = double(def.max);
    o["maxLiteral"]        = def.max_literal;
    o["mode"]              = int(def.mode);
    o["guiType"]           = int(def.gui_type);
    o["guiFlags"]          = def.gui_flags;
    o["aliases"]           = string_list(def.aliases);
    o["shortcut"]          = string_list(def.shortcut);
    o["enumValues"]        = string_list(def.enum_values);
    o["enumLabels"]        = string_list(def.enum_labels);
    if (def.enum_keys_map != nullptr) {
        json m = json::object();
        for (const auto &kv : *def.enum_keys_map)
            m[kv.first] = kv.second;
        o["enumKeysMap"] = m;
    } else {
        o["enumKeysMap"] = nullptr;
    }
    o["serializationKeyOrdinal"] = uint64_t(def.serialization_key_ordinal);
    if (def.default_value) {
        o["defaultType"] = int(def.default_value->type());
        try {
            o["default"] = def.default_value->serialize();
        } catch (const std::exception &e) {
            o["default"]      = nullptr;
            o["defaultThrow"] = e.what();
        }
    } else {
        o["defaultType"] = nullptr;
        o["default"]     = nullptr;
    }
    return o;
}

} // namespace

std::string record_configdef()
{
    json options = json::array();
    for (const auto &entry : print_config_def.options)
        options.push_back(option_json(entry.first, entry.second));

    // handle_legacy(key, value) for each legacy key and each defined key, with every probe value:
    // the cases where it changes the key or the value, as [key, value, key after, value after].
    std::vector<std::string> keys(std::begin(LEGACY_KEYS), std::end(LEGACY_KEYS));
    for (const auto &entry : print_config_def.options)
        keys.push_back(entry.first);
    json legacy = json::array();
    for (const std::string &key : keys)
        for (const char *probe : PROBE_VALUES) {
            t_config_option_key k     = key;
            std::string         value = probe;
            PrintConfigDef::handle_legacy(k, value);
            if (k != key || value != probe)
                legacy.push_back(json::array({key, probe, k, value}));
        }

    json out;
    out["format"]          = "sketch-orca-probe-configdef/1";
    out["orcaVersion"]     = SoftFever_VERSION;
    out["count"]           = options.size();
    out["options"]         = std::move(options);
    out["printOptions"]    = string_list(Preset::print_options());
    out["filamentOptions"] = string_list(Preset::filament_options());
    out["printerOptions"]  = string_list(Preset::printer_options());
    out["legacyKeys"]      = string_list(std::vector<std::string>(std::begin(LEGACY_KEYS), std::end(LEGACY_KEYS)));
    json values            = json::array();
    for (const char *probe : PROBE_VALUES)
        values.push_back(probe);
    out["probeValues"] = values;
    out["legacy"]      = std::move(legacy);
    return out.dump(1, ' ', false, json::error_handler_t::replace) + "\n";
}

} // namespace probe
