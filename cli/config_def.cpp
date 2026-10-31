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
// sketch-orca config-def: every print, filament and printer setting of Orca's slicing library, from
// its own definitions (print_config_def), so a client's settings UI and checks follow the engine; and
// the obsolete keys, which Orca drops or renames whenever it reads them, saying which of them Orca
// still defines.
#include <cfloat>
#include <map>
#include <set>
#include <string>

#include "libslic3r/Preset.hpp"
#include "libslic3r/PrintConfig.hpp"
#include "libslic3r_version.h"

#include "common.hpp"
#include "run.hpp"

namespace sketch_orca {

using namespace Slic3r;

namespace {

// The keys PrintConfigDef::handle_legacy names in the Orca source the engine is built from: the
// ones it compares a setting's key with, and its set of ignored keys. CMakeLists.txt reads them from
// src/libslic3r/PrintConfig.cpp into legacy_keys.inc, so they are Orca's own at the locked commit.
const char *const LEGACY_KEYS[] = {
#include "legacy_keys.inc"
};

// Values to read a legacy key with. handle_legacy decides some keys by their value: a percentage
// drops initial_layer_print_height (a setting it keeps otherwise), and prime_tower_rib_wall becomes
// wipe_tower_wall_type when it is 1 and is dropped otherwise.
const char *const PROBE_VALUES[] = {"", "0", "1", "-1", "2", "none", "50%", "nil"};

// The obsolete keys: those Orca's loader never keeps under their own name, because handle_legacy
// drops or renames them whatever their value (it runs on every key of a preset, a project and an
// override, ConfigBase::set_deserialize_nothrow). Each maps to the key Orca reads it as, when it
// renames it for one of the values (for some keys only for some values: prime_tower_rib_wall when
// it is 1, top_one_wall_type unless it is none; dropped otherwise), else to "". Sorted by key.
std::map<std::string, std::string> obsolete_keys()
{
    std::map<std::string, std::string> out;
    for (const char *legacy : LEGACY_KEYS) {
        const std::string key = legacy;
        bool              kept = false;
        std::string       read_as;
        for (const char *probe : PROBE_VALUES) {
            t_config_option_key k     = key;
            std::string         value = probe;
            PrintConfigDef::handle_legacy(k, value);
            if (k == key) {
                kept = true;
                break;
            }
            if (read_as.empty())
                read_as = k;
        }
        if (!kept)
            out.emplace(key, read_as);
    }
    return out;
}

// Orca's type name without its "co" prefix: coFloat -> "float", coFloatOrPercent -> "floatOrPercent".
const char *type_name(ConfigOptionType t)
{
    switch (t) {
    case coFloat: return "float";
    case coFloats: return "floats";
    case coInt: return "int";
    case coInts: return "ints";
    case coString: return "string";
    case coStrings: return "strings";
    case coPercent: return "percent";
    case coPercents: return "percents";
    case coFloatOrPercent: return "floatOrPercent";
    case coFloatsOrPercents: return "floatsOrPercents";
    case coPoint: return "point";
    case coPoints: return "points";
    case coPoint3: return "point3";
    case coBool: return "bool";
    case coBools: return "bools";
    case coEnum: return "enum";
    case coEnums: return "enums";
    case coPointsGroups: return "pointsGroups";
    case coIntsGroups: return "intsGroups";
    default: return "none";
    }
}

const char *mode_name(ConfigOptionMode m)
{
    switch (m) {
    case comSimple: return "simple";
    case comAdvanced: return "advanced";
    case comDevelop: return "develop";
    default: return "expert";
    }
}

const char *gui_type_name(ConfigOptionDef::GUIType t)
{
    switch (t) {
    case ConfigOptionDef::GUIType::i_enum_open: return "i_enum_open";
    case ConfigOptionDef::GUIType::f_enum_open: return "f_enum_open";
    case ConfigOptionDef::GUIType::color: return "color";
    case ConfigOptionDef::GUIType::select_open: return "select_open";
    case ConfigOptionDef::GUIType::slider: return "slider";
    case ConfigOptionDef::GUIType::legend: return "legend";
    case ConfigOptionDef::GUIType::one_string: return "one_string";
    default: return "";
    }
}

} // namespace

json config_def_json()
{
    const std::vector<std::string> &print_keys    = Preset::print_options();
    const std::vector<std::string> &filament_keys = Preset::filament_options();
    const std::vector<std::string> &printer_keys  = Preset::printer_options();
    const std::set<std::string>     process(print_keys.begin(), print_keys.end());
    const std::set<std::string>     filament(filament_keys.begin(), filament_keys.end());
    const std::set<std::string>     machine(printer_keys.begin(), printer_keys.end());
    const std::map<std::string, std::string> obsolete = obsolete_keys();

    json options = json::array();
    for (const auto &entry : print_config_def.options) {
        const std::string     &key = entry.first;
        const ConfigOptionDef &def = entry.second;
        // An obsolete key Orca still defines (silent_mode, tree_support_with_infill) can never be
        // set: it is listed as obsolete (and in stillDefined) only.
        if (def.printer_technology == ptSLA || def.type == coNone || obsolete.count(key))
            continue;
        json o;
        o["key"]      = key;
        o["type"]     = type_name(def.type);
        o["nullable"] = def.nullable;
        json sections = json::array();
        if (process.count(key))
            sections.push_back("process");
        if (filament.count(key))
            sections.push_back("filament");
        if (machine.count(key))
            sections.push_back("machine");
        o["sections"]  = sections;
        o["category"]  = def.category;
        o["label"]     = def.label;
        o["fullLabel"] = def.full_label;
        o["tooltip"]   = def.tooltip;
        o["sidetext"]  = def.sidetext;
        o["mode"]      = mode_name(def.mode);
        o["readonly"]  = def.readonly;
        o["multiline"] = def.multiline;
        o["isCode"]    = def.is_code;
        if (const char *gui = gui_type_name(def.gui_type); *gui != 0)
            o["guiType"] = gui;
        if (def.default_value) {
            try {
                o["default"] = def.default_value->serialize();
            } catch (const std::exception &) {
                o["default"] = nullptr;
            }
        } else {
            o["default"] = nullptr;
        }
        if (def.min > -FLT_MAX)
            o["min"] = double(def.min);
        if (def.max < FLT_MAX)
            o["max"] = double(def.max);
        if (!def.enum_values.empty())
            o["enumValues"] = def.enum_values;
        if (!def.enum_labels.empty())
            o["enumLabels"] = def.enum_labels;
        if (!def.ratio_over.empty())
            o["ratioOver"] = def.ratio_over;
        if (!def.aliases.empty())
            o["aliases"] = def.aliases;
        options.push_back(std::move(o));
    }

    json obsolete_list = json::array();
    json renamed       = json::object();
    json still_defined = json::array();
    for (const auto &kv : obsolete) {
        obsolete_list.push_back(kv.first);
        if (!kv.second.empty())
            renamed[kv.first] = kv.second;
        if (print_config_def.has(kv.first))
            still_defined.push_back(kv.first);
    }

    json out;
    out["format"]        = "sketch-orca-config-def/1";
    out["engine"]        = "sketch-orca";
    out["engineVersion"] = SKETCH_ORCA_VERSION;
    out["orcaVersion"]   = SoftFever_VERSION;
    out["orcaCommit"]    = SKETCH_ORCA_COMMIT;
    out["count"]         = options.size();
    out["options"]       = std::move(options);
    // Keys Orca drops or renames whenever it reads them (none of them is an option above), and the
    // key Orca reads each renamed one as, for the values it renames it for (others it drops); its
    // value may be translated too (wall_infill_order).
    out["obsolete"]      = std::move(obsolete_list);
    out["renamed"]       = std::move(renamed);
    // The obsolete keys Orca still defines, sorted. Orca retires a setting it still defines by
    // ignoring it in handle_legacy, so smoke checks each of these against handle_legacy's ignored
    // keys: a live setting whose value handle_legacy fixes (wall_direction) and which every probe
    // value happened to drop or rename would be listed as obsolete, and would show up here.
    out["stillDefined"]  = std::move(still_defined);
    return out;
}

} // namespace sketch_orca
