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
// Carries OrcaSlicer code: the preset merge is ported from src/OrcaSlicer.cpp (load_config_file,
// load_default_gcodes_to_config, update_full_config, the printer safe check with
// cli_config.json, the flush volume computation and the filament settings of the print
// configuration), src/slic3r/GUI/Plater.cpp (get_min_flush_volumes) and
// src/slic3r/GUI/BitmapCache.cpp (BitmapCache::parse_color4), OrcaSlicer v2.4.2, commit 8500fcd.
// Modified for sketch-orca, 2026-10-30, and 2026-11-01.
// Those portions are copyright the OrcaSlicer contributors (NOTICE).

#include "presets.hpp"

#include <algorithm>
#include <cctype>
#include <cmath>
#include <cstdlib>
#include <map>
#include <memory>
#include <set>

#include <boost/algorithm/string/predicate.hpp>
#include <boost/filesystem.hpp>

#include "libslic3r/libslic3r.h"
#include "libslic3r/Config.hpp"
#include "libslic3r/FlushVolCalc.hpp"
#include "libslic3r/Preset.hpp"
#include "libslic3r/PrintConfig.hpp"
#include "libslic3r_version.h"

namespace sketch_orca {

using namespace Slic3r;

namespace {

// Preset files are read with sorted keys, as Orca reads them (ConfigBase::load_from_json).
using sorted_json = nlohmann::json;

// Keys of a preset file that describe the file rather than a setting (BBL_JSON_KEY_* in Preset.hpp).
const char *const META_KEYS[] = {"version", "name", "url", "type", "setting_id", "filament_id", "from", "description", "instantiation", "renamed_from"};

std::string in_quotes(const std::string &s) { return "\"" + s + "\""; }

const ConfigOptionDef *find_def(std::string &key)
{
    if (const ConfigOptionDef *def = print_config_def.get(key))
        return def;
    // Another option may carry this key as an alias (ConfigBase::set_deserialize_raw).
    for (const auto &opt : print_config_def.options)
        for (const t_config_option_key &alias : opt.second.aliases)
            if (alias == key) {
                key = opt.first;
                return &opt.second;
            }
    return nullptr;
}

std::string scalar_text(const sorted_json &v)
{
    if (v.is_string())
        return v.get<std::string>();
    if (v.is_boolean())
        return v.get<bool>() ? "1" : "0";
    return v.dump();
}

// Orca's parse_str_arr (ConfigBase::load_from_json): a list becomes the text Orca deserializes,
// items joined with `single_sep`, nested lists with `array_sep`, strings of a coStrings option
// quoted and escaped. Orca's loader refuses numbers and true/false written without quotes inside a
// list; here they are read as the text they spell.
bool array_text(const sorted_json &arr, char single_sep, char array_sep, bool escape, std::string &out)
{
    bool has_list = false, has_value = false;
    for (const sorted_json &item : arr)
        (item.is_array() ? has_list : has_value) = true;
    if (has_list && has_value)
        return false;
    bool first = true;
    for (const sorted_json &item : arr) {
        if (item.is_array()) {
            if (!first)
                out += array_sep;
            first = false;
            if (!array_text(item, single_sep, array_sep, escape, out))
                return false;
            continue;
        }
        if (!item.is_string() && !item.is_number() && !item.is_boolean())
            return false;
        if (!first)
            out += single_sep;
        first = false;
        const std::string s = scalar_text(item);
        if (escape)
            out += "\"" + escape_string_cstyle(s) + "\"";
        else
            out += s;
    }
    return true;
}

PrinterTechnology printer_technology_of(const DynamicPrintConfig &config)
{
    const auto *opt = config.option<ConfigOptionEnum<PrinterTechnology>>("printer_technology");
    return opt == nullptr ? ptUnknown : opt->value;
}

// One preset file, read like the stock command line reads --load-settings and --load-filaments
// (load_config_file in OrcaSlicer.cpp, which calls ConfigBase::load_from_json with "inherits" kept
// in the configuration). Differences, all reported as warnings: a key Orca does not know is
// skipped instead of failing the file, and a number or true/false written without quotes is read
// instead of being dropped. A key Orca does not have at all is skipped without a word, as Orca's
// own loader skips it (PrintConfigDef::handle_legacy clears it): Orca's bundled profiles carry
// hundreds of them (BambuStudio settings, misspelt keys such as layer_heigth), so a warning would
// come with every job on those printers.
LoadedPreset load_preset(const std::string &path, const std::string &slot, const std::string &field, Warnings &warnings)
{
    if (!is_file(path))
        fail("file-not-found", field + ": there is no file " + path);
    sorted_json j;
    try {
        j = sorted_json::parse(read_file(path));
    } catch (const std::exception &e) {
        fail("preset-invalid", field + ": " + path + " is not JSON (" + e.what() + ")");
    }
    if (!j.is_object())
        fail("preset-invalid", field + ": " + path + " is not a JSON object");

    LoadedPreset p;
    p.path = path;
    DynamicPrintConfig &config = p.config;
    ConfigSubstitutionContext ctx(ForwardCompatibilitySubstitutionRule::Enable);
    std::map<std::string, std::string> meta;
    bool tree_hybrid = false, infill_first = false;

    for (auto it = j.begin(); it != j.end(); ++it) {
        const std::string  key   = it.key();
        const sorted_json &value = it.value();
        if (boost::iequals(key, "is_custom_defined"))
            continue;
        bool is_meta = false;
        for (const char *m : META_KEYS)
            if (boost::iequals(key, m)) {
                meta.emplace(m, value.is_string() ? value.get<std::string>() : value.dump());
                is_meta = true;
                break;
            }
        if (is_meta)
            continue;

        std::string set_key = key, text;
        if (value.is_string() || value.is_number() || value.is_boolean()) {
            text = scalar_text(value);
        } else if (value.is_array()) {
            std::string legacy_value;
            PrintConfigDef::handle_legacy(set_key, legacy_value);
            if (set_key.empty())
                continue; // a setting Orca dropped, or one it never had; its loader skips it too
            const ConfigOptionDef *def        = find_def(set_key);
            char                   single_sep = ',';
            bool                   escape     = false;
            if (def != nullptr && def->type == coStrings) {
                escape     = true;
                single_sep = ';';
            } else if (def != nullptr && def->type == coPointsGroups) {
                single_sep = '#';
            }
            if (!array_text(value, single_sep, '#', escape, text)) {
                warnings.add("preset-value-skipped", field + ": " + key + " is a list Orca cannot read; it keeps Orca's default");
                continue;
            }
        } else {
            warnings.add("preset-value-skipped", field + ": " + key + " is neither text nor a list; it keeps Orca's default");
            continue;
        }

        try {
            if (!config.set_deserialize_nothrow(set_key, text, ctx))
                fail("preset-invalid", field + ": " + key + " has a value Orca cannot read: " + in_quotes(text));
        } catch (const Failure &) {
            throw;
        } catch (const UnknownOptionException &) {
            warnings.add("unknown-setting", field + ": " + key + " is not a setting of Orca " SoftFever_VERSION "; it is ignored");
            continue;
        } catch (const std::exception &e) {
            fail("preset-invalid", field + ": " + key + ": " + e.what());
        }
        if (key == "support_type" && value.is_string() && text == "hybrid(auto)")
            tree_hybrid = true;
        if (key == "wall_infill_order" && value.is_string() && (text == "infill/outer wall/inner wall" || text == "infill/inner wall/outer wall"))
            infill_first = true;
    }
    if (tree_hybrid)
        config.option<ConfigOptionEnum<SupportMaterialStyle>>("support_style", true)->value = smsTreeHybrid;
    if (infill_first)
        config.option<ConfigOptionBool>("is_infill_first", true)->value = true;
    config.handle_legacy_composite();

    p.name = meta["name"];
    p.type = meta["type"];
    p.from = meta["from"];
    p.filament_id = meta["filament_id"];
    if (p.from != "system" && p.from != "User" && p.from != "user")
        fail("preset-invalid", field + ": \"from\" must be system, User or user, got " + in_quotes(p.from));
    if (p.type != slot)
        fail("preset-invalid", field + ": " + path + " is " + (p.type.empty() ? std::string("a file without a type") : "a " + p.type + " preset") +
                                   ", not a " + slot + " preset");
    if (const auto *inherits = config.option<ConfigOptionString>("inherits"))
        p.inherits = inherits->value;

    const PrinterTechnology tech = printer_technology_of(config);
    if (tech != ptUnknown && tech != ptFFF)
        fail("not-fdm", field + ": " + p.name + " is not a filament (FFF) preset");

    try {
        config.normalize_fdm();
    } catch (const std::exception &e) {
        fail("preset-invalid", field + ": " + e.what());
    }
    for (const ConfigSubstitution &s : ctx.substitutions)
        warnings.add("setting-substituted", field + ": " + s.opt_def->opt_key + " = " + in_quotes(s.old_value) + " is not a value Orca " SoftFever_VERSION " knows; it uses " +
                                                in_quotes(s.new_value->serialize()));
    return p;
}

// load_default_gcodes_to_config in OrcaSlicer.cpp: the G-code options a printer or filament
// preset may leave out are created, empty.
void add_default_gcodes(DynamicPrintConfig &config, Preset::Type type)
{
    if (config.size() == 0)
        return;
    if (type == Preset::TYPE_PRINTER) {
        for (const char *key : {"change_filament_gcode", "layer_change_gcode", "machine_end_gcode", "machine_pause_gcode", "machine_start_gcode",
                                "template_custom_gcode", "printing_by_object_gcode", "before_layer_change_gcode", "time_lapse_gcode", "wrapping_detection_gcode"})
            config.option<ConfigOptionString>(key, true);
    } else if (type == Preset::TYPE_FILAMENT) {
        for (const char *key : {"filament_start_gcode", "filament_end_gcode"}) {
            std::vector<std::string> &values = config.option<ConfigOptionStrings>(key, true)->values;
            if (values.empty())
                values.resize(1, std::string());
        }
    }
}

// The lists the merge keeps itself, one entry per preset (load_settings): a preset file that
// carries one (a preset exported from a project may) must not replace them, or the merge's writes
// by index would land outside a shorter list.
bool merge_metadata_key(const std::string &key)
{
    return key == "inherits_group" || key == "different_settings_to_system" || key == "compatible_machine_expression_group" ||
           key == "compatible_process_expression_group" || key == "print_compatible_printers";
}

// Entry `slot` of inherits_group or different_settings_to_system, which hold one entry per preset:
// the process at 0, filament i at i + 1, the machine at n + 1.
void set_preset_slot(DynamicPrintConfig &cfg, const char *key, size_t slot, size_t n, const std::string &value)
{
    std::vector<std::string> &values = cfg.option<ConfigOptionStrings>(key, true)->values;
    if (values.size() < std::max(n + 2, slot + 1))
        values.resize(std::max(n + 2, slot + 1), std::string());
    values[slot] = value;
}

// update_full_config in OrcaSlicer.cpp with update_all: every key of a machine or process preset
// goes into the whole configuration, except the ones that describe the preset.
void copy_preset_keys(DynamicPrintConfig &full, const DynamicPrintConfig &config, const std::string &field)
{
    static const std::set<std::string> skipped = {"compatible_prints", "compatible_printers", "model_id", "inherits", "dev_model_name", "name",
                                                  "from", "type", "version", "setting_id", "instantiation"};
    for (const t_config_option_key &key : config.keys()) {
        if (skipped.count(key) || merge_metadata_key(key))
            continue;
        const ConfigOption *src = config.option(key);
        ConfigOption       *dst = full.option(key, true);
        if (src == nullptr || dst == nullptr)
            fail("preset-invalid", field + ": " + key + " cannot be stored");
        dst->set(src);
    }
}

// The stock command line lowers a Bambu printer's machine limits to the "cli_safe" values of
// profiles/BBL/cli_config.json when it loads a printer (OrcaSlicer.cpp, "printer safe check").
void apply_cli_safe_limits(DynamicPrintConfig &full, const std::string &resources, const std::string &printer_model)
{
    if (printer_model.empty())
        return;
    const std::string file = resources + "/profiles/BBL/cli_config.json";
    if (!is_file(file))
        return;
    try {
        const sorted_json root = sorted_json::parse(read_file(file));
        auto printers = root.find("printer");
        if (printers == root.end() || !printers->is_object())
            return;
        auto model = printers->find(printer_model);
        if (model == printers->end() || !model->is_object())
            return;
        auto limits = model->find("machine_limits");
        if (limits == model->end() || !limits->is_object())
            return;
        for (auto it = limits->begin(); it != limits->end(); ++it) {
            if (!it.value().is_string())
                continue;
            std::string key = it.key();
            if (key.size() < 8)
                continue;
            key.replace(0, 8, "machine_max");
            ConfigOptionFloats *opt = full.option<ConfigOptionFloats>(key);
            if (opt == nullptr)
                continue;
            ConfigOptionFloats safe;
            safe.deserialize(it.value().get<std::string>());
            for (size_t i = 0; i < opt->values.size() && i < safe.values.size(); ++i)
                if (safe.values[i] != 0. && safe.values[i] < opt->values[i])
                    opt->values[i] = safe.values[i];
        }
    } catch (const std::exception &) {
        // The stock command line logs a broken file and goes on.
    }
}

// Every value of a float list option, nullable or not.
bool float_values(const DynamicPrintConfig &cfg, const char *key, std::vector<double> &out)
{
    const ConfigOption *opt = cfg.option(key);
    if (opt == nullptr || opt->type() != coFloats)
        return false;
    out = static_cast<const ConfigOptionVector<double> *>(opt)->values;
    return true;
}

bool bool_values(const DynamicPrintConfig &cfg, const char *key, std::vector<unsigned char> &out)
{
    const ConfigOption *opt = cfg.option(key);
    if (opt == nullptr || opt->type() != coBools)
        return false;
    out = static_cast<const ConfigOptionVector<unsigned char> *>(opt)->values;
    return true;
}

// A volume or length the flush computation takes as a whole number, as Orca's int() does. A value
// past the largest flushing volume Orca computes cannot be meant, and its conversion to int would be
// undefined (sketch-orca, 2026-11-01).
int flush_input(double v, const char *key)
{
    if (!std::isfinite(v) || std::fabs(v) > double(g_max_flush_volume))
        fail("config-invalid", std::string(key) + " cannot be " + format_number(v) + " for the flushing volumes (at most " +
                                   std::to_string(g_max_flush_volume) + " either way)");
    return int(v);
}

// get_min_flush_volumes in slic3r/GUI/Plater.cpp, which the stock command line calls.
std::vector<int> min_flush_volumes(const DynamicPrintConfig &cfg, size_t nozzle_id)
{
    int                 nozzle_volume_val = 0;
    std::vector<double> nozzle_volume;
    if (float_values(cfg, "nozzle_volume", nozzle_volume) && !nozzle_volume.empty()) {
        const double v    = nozzle_id < nozzle_volume.size() ? nozzle_volume[nozzle_id] : nozzle_volume.front();
        nozzle_volume_val = std::isnan(v) ? 0 : flush_input(v, "nozzle_volume");
    }

    int machine_enabled_level = 0;
    if (const auto *opt = cfg.option<ConfigOptionInt>("enable_long_retraction_when_cut"))
        machine_enabled_level = opt->value;
    bool                       machine_activated = false;
    std::vector<unsigned char> machine_long;
    if (bool_values(cfg, "long_retractions_when_cut", machine_long) && nozzle_id < machine_long.size())
        machine_activated = machine_long[nozzle_id] == 1;

    std::vector<double> diameters;
    float_values(cfg, "filament_diameter", diameters);
    const size_t               filament_size = diameters.size();
    std::vector<double>        filament_distance(filament_size, 18.), printer_distance(filament_size, 18.);
    std::vector<unsigned char> filament_long(filament_size, 0);
    float_values(cfg, "filament_retraction_distances_when_cut", filament_distance);
    float_values(cfg, "retraction_distances_when_cut", printer_distance);
    bool_values(cfg, "filament_long_retractions_when_cut", filament_long);

    auto at = [](const auto &v, size_t i, auto fallback) { return i < v.size() ? v[i] : fallback; };
    std::vector<int> out;
    for (size_t idx = 0; idx < filament_size; ++idx) {
        int extra_flush_volume = nozzle_volume_val;
        int retract_length     = machine_enabled_level && machine_activated ? flush_input(at(printer_distance, nozzle_id, 18.), "retraction_distances_when_cut") : 0;

        const unsigned char filament_activated     = at(filament_long, idx, (unsigned char) 0);
        const double        filament_retract_length = at(filament_distance, idx, 18.);
        if (filament_activated == 0)
            retract_length = 0;
        else if (filament_activated == 1 && machine_enabled_level == LongRectrationLevel::EnableFilament) {
            if (!std::isnan(filament_retract_length))
                retract_length = flush_input(filament_retract_length, "filament_retraction_distances_when_cut");
            else
                retract_length = flush_input(at(printer_distance, nozzle_id, 18.), "retraction_distances_when_cut");
        }
        extra_flush_volume -= int(PI * 1.75 * 1.75 / 4 * retract_length);
        out.push_back(extra_flush_volume);
    }
    return out;
}

int hex_digit(char c)
{
    if (c >= '0' && c <= '9')
        return c - '0';
    if (c >= 'a' && c <= 'f')
        return c - 'a' + 10;
    if (c >= 'A' && c <= 'F')
        return c - 'A' + 10;
    return -1;
}

// BitmapCache::parse_color4 in slic3r/GUI/BitmapCache.cpp.
void parse_color4(const std::string &s, unsigned char *rgba)
{
    rgba[0] = rgba[1] = rgba[2] = 0;
    rgba[3]                     = 255;
    if ((s.size() != 7 && s.size() != 9) || s.front() != '#')
        return;
    for (size_t i = 0; i < s.size() / 2; ++i) {
        const int d1 = hex_digit(s[1 + 2 * i]), d2 = hex_digit(s[2 + 2 * i]);
        if (d1 < 0 || d2 < 0)
            return;
        rgba[i] = (unsigned char) (d1 * 16 + d2);
    }
}

bool is_hex_colour(const std::string &s)
{
    return (s.size() == 7 || s.size() == 9) && s.front() == '#' && std::all_of(s.begin() + 1, s.end(), [](char c) { return hex_digit(c) >= 0; });
}

// filament_colour's default (PrintConfig.cpp), which the GUI gives a project's first filament.
const char *const ORCA_FILAMENT_COLOUR = "#F2754E";
// The colours the GUI gives the filaments it adds after the first (Plater::get_next_color_for_filament).
const char *const GUI_FILAMENT_COLOURS[] = {"#00C1AE", "#F4E2C1", "#ED1C24", "#00FF7F", "#F26722", "#FFEB31", "#7841CE", "#115877",
                                            "#ED1E79", "#2EBDEF", "#345B2F", "#800080", "#FA8173", "#800000", "#F7B763", "#A4C41E"};

// Orca counts a print's filaments by filament_colour (ToolOrdering, GCode, Print::extruders), a
// project setting that no filament preset carries (Preset::filament_options leaves it out): the GUI
// keeps one colour per filament in the project. Here each filament gets the colour its preset file
// names (filament_colour, which a preset exported from a project may carry, else
// default_filament_colour), else the colour the GUI gives a filament it adds. The job's
// filament_colour override replaces them by position, as the stock --filament-colour does; an empty
// entry keeps the colour. filament_multi_colour and filament_colour_type follow, as the GUI keeps
// them, and filament_map (which extruder prints each filament) gets 1 for a filament it does not
// name (PresetBundle::set_num_filaments).
void set_filament_colours(DynamicPrintConfig &cfg, DynamicPrintConfig &extra, const std::vector<LoadedPreset> &filaments)
{
    const size_t             n = filaments.size();
    std::vector<std::string> colours(n);
    for (size_t i = 0; i < n; ++i) {
        for (const char *key : {"filament_colour", "default_filament_colour"}) {
            const auto *opt = filaments[i].config.option<ConfigOptionStrings>(key);
            if (colours[i].empty() && opt != nullptr && !opt->values.empty() && is_hex_colour(opt->values.front()))
                colours[i] = opt->values.front();
        }
        if (colours[i].empty())
            colours[i] = i == 0 ? ORCA_FILAMENT_COLOUR : GUI_FILAMENT_COLOURS[(i - 1) % std::size(GUI_FILAMENT_COLOURS)];
    }
    if (const auto *given = extra.option<ConfigOptionStrings>("filament_colour")) {
        if (given->values.size() > n)
            fail("setting-invalid", "overrides: filament_colour gives " + std::to_string(given->values.size()) + " colours for the job's " + std::to_string(n) +
                                        " filament(s)");
        for (size_t i = 0; i < given->values.size(); ++i) {
            const std::string &c = given->values[i];
            if (c.empty())
                continue;
            if (!is_hex_colour(c))
                fail("setting-invalid", "overrides: filament_colour: " + in_quotes(c) + " is not a colour (#rrggbb or #rrggbbaa)");
            colours[i] = c;
        }
        extra.erase("filament_colour");
    }
    cfg.option<ConfigOptionStrings>("filament_colour", true)->values       = colours;
    cfg.option<ConfigOptionStrings>("filament_multi_colour", true)->values = colours;
    cfg.option<ConfigOptionStrings>("filament_colour_type", true)->values.assign(n, "1");
    cfg.option<ConfigOptionInts>("filament_map", true)->values.resize(n, 1);
}

// The flushing volumes between filaments, computed from their colours as the stock command line
// computes them (OrcaSlicer.cpp, "compute the flush volume"), for every job: Orca reads
// flush_volumes_matrix as n x n per nozzle for n filaments, and a matrix from a preset is sized for
// some other set of filaments. `cfg` holds the job's overrides already, so the ones the
// computation reads (filament_is_support, nozzle_volume, the retractions when cut...) count
// (sketch-orca, 2026-11-01). A job's own flush_volumes_matrix (`matrix_given`) is kept as it is,
// and must have n x n values per nozzle.
void compute_flush_volumes(DynamicPrintConfig &cfg, bool matrix_given, size_t extruder_count, size_t printer_variant_count)
{
    const std::vector<std::string> colors = cfg.option<ConfigOptionStrings>("filament_colour", true)->values;
    const size_t                   n      = colors.size();
    if (n == 0)
        return;
    cfg.option<ConfigOptionFloats>("flush_multiplier", true)->values.resize(extruder_count, 1.);
    if (matrix_given) {
        const size_t given = cfg.option<ConfigOptionFloats>("flush_volumes_matrix", true)->values.size();
        if (given != n * n * extruder_count)
            fail("setting-invalid", "overrides: flush_volumes_matrix has " + std::to_string(given) + " values; with " + std::to_string(n) + " filament(s) and " +
                                        std::to_string(extruder_count) + " nozzle(s) it needs " + std::to_string(n * n * extruder_count) + " (one row of " +
                                        std::to_string(n) + " per filament, for each nozzle)");
        return;
    }

    ConfigOptionBools *is_support = cfg.option<ConfigOptionBools>("filament_is_support", true);
    if (is_support->size() != n)
        fail("preset-invalid", "the settings give " + std::to_string(is_support->size()) + " filament_is_support values for " + std::to_string(n) + " filaments");

    std::vector<double> matrix(n * n * extruder_count, 0.);

    std::vector<int> dataset(extruder_count, 0);
    {
        std::vector<int> full = cfg.option<ConfigOptionIntsNullable>("nozzle_flush_dataset", true)->values;
        full.resize(cfg.has("printer_extruder_variant") ? std::max<size_t>(printer_variant_count, 1) : 1, 0);
        std::vector<int> extruders(1, int(etDirectDrive));
        if (const auto *opt = dynamic_cast<const ConfigOptionEnumsGeneric *>(cfg.option("extruder_type")))
            extruders = opt->values;
        std::vector<int> volume_types(1, int(nvtStandard));
        if (const auto *opt = dynamic_cast<const ConfigOptionEnumsGeneric *>(cfg.option("nozzle_volume_type")))
            volume_types = opt->values;
        for (size_t e = 0; e < extruder_count; ++e) {
            int index = 0;
            if (cfg.has("printer_extruder_id") && cfg.has("printer_extruder_variant") && !extruders.empty() && !volume_types.empty())
                index = cfg.get_index_for_extruder(int(e) + 1, "printer_extruder_id", ExtruderType(extruders[std::min(e, extruders.size() - 1)]),
                                                   NozzleVolumeType(volume_types[std::min(e, volume_types.size() - 1)]), "printer_extruder_variant");
            dataset[e] = index >= 0 && size_t(index) < full.size() ? full[index] : full.front();
        }
    }

    for (size_t nozzle = 0; nozzle < extruder_count; ++nozzle) {
        // Each nozzle's own volume and retraction (the stock loop asks for nozzle_id; 2026-11-01).
        const std::vector<int> min_volumes = min_flush_volumes(cfg, nozzle);
        std::vector<double>    m(n * n, 0.);
        for (size_t from = 0; from < n; ++from) {
            unsigned char from_rgb[4];
            parse_color4(colors[from], from_rgb);
            const bool from_support = is_support->get_at(from) != 0;
            for (size_t to = 0; to < n; ++to) {
                if (from == to)
                    continue;
                int volume = 0;
                if (is_support->get_at(to) != 0) {
                    volume = g_flush_volume_to_support;
                } else {
                    unsigned char to_rgb[4];
                    parse_color4(colors[to], to_rgb);
                    FlushVolCalculator calculator(from < min_volumes.size() ? min_volumes[from] : 0, g_max_flush_volume, dataset[nozzle]);
                    volume = calculator.calc_flush_vol(from_rgb[3], from_rgb[0], from_rgb[1], from_rgb[2], to_rgb[3], to_rgb[0], to_rgb[1], to_rgb[2]);
                    if (from_support)
                        volume = std::max(g_min_flush_volume_from_support, volume);
                }
                m[n * from + to] = volume;
            }
        }
        set_flush_volumes_matrix(matrix, m, nozzle, extruder_count);
    }
    cfg.option<ConfigOptionFloats>("flush_volumes_matrix", true)->values = matrix;
}

// Settings of Preset::filament_options that hold one value per filament. The ones with extruder
// variants hold one per filament and variant, laid out by filament_self_index.
bool per_filament_key(const std::string &key)
{
    return key != "compatible_prints" && key != "compatible_printers" && filament_options_with_variant.count(key) == 0;
}

// A filament preset may leave a setting out. Orca reads a filament's value by its index, some of it
// without a range check, and the GUI's filament preset holds every setting (Orca's default, then
// the file), so a filament that does not give a setting gets Orca's default at its index here.
// `given` marks, per setting, the filaments that gave it.
void pad_filament_options(DynamicPrintConfig &cfg, const std::map<std::string, std::vector<bool>> &given, size_t n)
{
    for (const std::string &key : Preset::filament_options()) {
        if (!per_filament_key(key))
            continue;
        ConfigOption *opt = cfg.option(key);
        if (opt == nullptr || !opt->is_vector())
            continue;
        auto                  *vec         = static_cast<ConfigOptionVectorBase *>(opt);
        const ConfigOptionDef *def         = print_config_def.get(key);
        const ConfigOption    *dflt        = def != nullptr ? def->default_value.get() : nullptr;
        const bool             has_default = dflt != nullptr && dflt->is_vector() && static_cast<const ConfigOptionVectorBase *>(dflt)->size() > 0;
        const auto             it          = given.find(key);
        for (size_t i = 0; i < n; ++i) {
            if (it != given.end() && i < it->second.size() && it->second[i] && i < vec->size())
                continue;
            if (has_default) {
                if (vec->size() == 0)
                    vec->resize(1, dflt);
                vec->set_at(dflt, i, 0);
            } else if (vec->size() > 0 && vec->size() <= i) {
                vec->resize(i + 1);
            }
        }
    }
}

// After Orca's defaults fill in the settings no preset gave, each holds one value per filament.
void size_filament_options(DynamicPrintConfig &cfg, size_t n)
{
    for (const std::string &key : Preset::filament_options()) {
        if (!per_filament_key(key))
            continue;
        ConfigOption *opt = cfg.option(key);
        if (opt == nullptr || !opt->is_vector())
            continue;
        auto *vec = static_cast<ConfigOptionVectorBase *>(opt);
        if (vec->size() > 0 && vec->size() < n)
            vec->resize(n);
    }
}

// A job's override of a filament setting gives one value for every filament or one per filament
// (for the settings with extruder variants, one per filament and variant, as the presets lay them
// out); one value is repeated for every filament.
void check_filament_overrides(DynamicPrintConfig &extra, const DynamicPrintConfig &cfg, size_t n)
{
    static const std::set<std::string> project_keys = {"filament_multi_colour", "filament_colour_type", "filament_map"};
    const std::set<std::string>        filament_keys(Preset::filament_options().begin(), Preset::filament_options().end());
    for (const t_config_option_key &key : extra.keys()) {
        if (!project_keys.count(key) && !(filament_keys.count(key) && per_filament_key(key)) && !filament_options_with_variant.count(key))
            continue;
        ConfigOption *opt = extra.option(key);
        if (opt == nullptr || !opt->is_vector())
            continue;
        auto  *vec  = static_cast<ConfigOptionVectorBase *>(opt);
        size_t want = n;
        if (filament_options_with_variant.count(key)) {
            if (const auto *variants = cfg.option<ConfigOptionStrings>("filament_extruder_variant"); variants != nullptr && !variants->values.empty())
                want = variants->values.size();
        }
        if (vec->size() == want)
            continue;
        if (vec->size() == 1) {
            vec->resize(want);
            continue;
        }
        fail("setting-invalid", "overrides: " + key + " gives " + std::to_string(vec->size()) + " values; give one for every filament, or " +
                                    std::to_string(want) + (want == n ? " (one per filament)" : " (one per filament and extruder variant)"));
    }
}

// The model id of a printer model ("BL-P001" for the Bambu Lab X1 Carbon) from the vendor bundles
// in resources/profiles, as Preset::get_printer_type finds it. The stock command line looks in
// profiles/BBL/machine_full, which Orca 2.4.2 does not ship, so it never finds one.
std::string find_printer_model_id(const std::string &resources, const std::string &printer_model)
{
    if (printer_model.empty())
        return "";
    namespace fs = boost::filesystem;
    const fs::path            profiles = fs::path(resources) / "profiles";
    boost::system::error_code ec;
    std::vector<fs::path>     vendors;
    for (fs::directory_iterator it(profiles, ec), end; !ec && it != end; it.increment(ec))
        if (boost::iequals(it->path().extension().string(), ".json") && fs::is_regular_file(it->path(), ec))
            vendors.push_back(it->path());
    std::sort(vendors.begin(), vendors.end());
    for (const fs::path &index : vendors) {
        try {
            const sorted_json j    = sorted_json::parse(read_file(index.string()));
            auto              list = j.find("machine_model_list");
            if (list == j.end() || !list->is_array())
                continue;
            for (const sorted_json &entry : *list) {
                if (!entry.is_object() || entry.value("name", "") != printer_model)
                    continue;
                const fs::path model_file = profiles / index.stem() / entry.value("sub_path", "");
                const sorted_json m       = sorted_json::parse(read_file(model_file.string()));
                return m.value("model_id", "");
            }
        } catch (const std::exception &) {
            continue;
        }
    }
    return "";
}

// Preset::get_default_bed_type: the plate the GUI selects for a printer it has no remembered plate
// for.
BedType default_bed_type(const DynamicPrintConfig &machine, const std::string &model_id)
{
    if (const auto *opt = machine.option<ConfigOptionString>("default_bed_type"); opt != nullptr && !opt->value.empty()) {
        // atoi's number, read without its undefined overflow (sketch-orca, 2026-11-01).
        const long v = std::strtol(opt->value.c_str(), nullptr, 10);
        if (v > 0 && v < long(btCount))
            return BedType(int(v));
        return btPEI;
    }
    if (model_id == "BL-P001" || model_id == "BL-P002" || model_id == "C13")
        return btPC;
    return btPEI;
}

// One number as a setting value holds it: the whole item, with a trailing % where the setting takes
// a percentage.
bool whole_number(std::string item, bool integer, bool percent)
{
    while (!item.empty() && std::isspace((unsigned char) item.front()))
        item.erase(item.begin());
    while (!item.empty() && std::isspace((unsigned char) item.back()))
        item.pop_back();
    if (percent && !item.empty() && item.back() == '%')
        item.pop_back();
    if (item.empty())
        return false;
    char *end = nullptr;
    if (integer)
        (void) std::strtoll(item.c_str(), &end, 10);
    else if (!std::isfinite(std::strtod(item.c_str(), &end)))
        return false;
    return end != nullptr && *end == 0;
}

// Orca's readers of numbers keep what they can read and drop the rest: "220;270" reads as 220, and
// a word in a list as 0. A number setting given in a job is taken only when each of its values
// (comma separated, as Orca writes lists of numbers) is a number, or nil where the setting has it.
bool readable_numbers(const ConfigOptionDef &def, const std::string &value)
{
    bool list = false, integer = false, percent = false;
    switch (def.type) {
    case coFloat: break;
    case coInt: integer = true; break;
    case coPercent:
    case coFloatOrPercent: percent = true; break;
    case coFloats: list = true; break;
    case coInts: list = integer = true; break;
    case coPercents:
    case coFloatsOrPercents: list = percent = true; break;
    default: return true;
    }
    if (!list)
        return whole_number(value, integer, percent);
    size_t start = 0;
    for (;;) {
        const size_t comma = value.find(',', start);
        std::string  item  = value.substr(start, comma == std::string::npos ? std::string::npos : comma - start);
        const auto   first = item.find_first_not_of(" \t");
        const auto   last  = item.find_last_not_of(" \t");
        const bool   nil   = first != std::string::npos && item.substr(first, last - first + 1) == "nil";
        if (!(nil && def.nullable) && !whole_number(item, integer, percent))
            return false;
        if (comma == std::string::npos)
            return true;
        start = comma + 1;
    }
}

std::unique_ptr<Preset> make_preset(Preset::Type type, const LoadedPreset &loaded, const std::vector<std::string> &option_keys)
{
    auto preset       = std::make_unique<Preset>(type, loaded.name);
    preset->is_system = loaded.from == "system";
    std::unique_ptr<DynamicPrintConfig> defaults(DynamicPrintConfig::new_from_defaults_keys(option_keys));
    preset->config = *defaults;
    preset->config.apply(loaded.config, true);
    return preset;
}

} // namespace

std::string parse_setting(DynamicPrintConfig &into, const std::string &key, const std::string &value, const std::string &field)
{
    std::string stored = key, legacy_value = value;
    PrintConfigDef::handle_legacy(stored, legacy_value);
    // handle_legacy renames the old keys Orca still reads, and clears both the keys it dropped and
    // the ones it never had (a misspelt key), which it cannot tell apart from here.
    if (stored.empty())
        fail("setting-invalid", field + ": " + key + " is not a setting of Orca " SoftFever_VERSION);
    const ConfigOptionDef *def = find_def(stored);
    if (def == nullptr)
        fail("setting-invalid", field + ": " + key + " is not a setting of Orca " SoftFever_VERSION);
    if (def->printer_technology == ptSLA)
        fail("setting-invalid", field + ": " + key + " is a resin (SLA) setting");
    ConfigSubstitutionContext ctx(ForwardCompatibilitySubstitutionRule::Disable);
    bool                      ok = readable_numbers(*def, value);
    if (ok) {
        try {
            ok = into.set_deserialize_nothrow(key, value, ctx);
        } catch (const std::exception &) {
            ok = false;
        }
    }
    if (!ok) {
        std::string hint;
        if (!def->enum_values.empty())
            hint = " (one of: " + join(def->enum_values, ", ") + ")";
        else if ((def->type == coFloats || def->type == coInts || def->type == coPercents || def->type == coFloatsOrPercents) && value.find(';') != std::string::npos)
            hint = " (a list of numbers is written with commas, one value per filament or extruder)";
        else if (def->type == coFloats || def->type == coInts || def->type == coPercents || def->type == coFloatsOrPercents)
            hint = " (numbers, separated by commas)";
        else if (def->type == coFloat || def->type == coInt || def->type == coPercent || def->type == coFloatOrPercent)
            hint = " (a number)";
        fail("setting-invalid", field + ": " + key + " cannot be " + in_quotes(value) + hint);
    }
    return stored;
}

Settings load_settings(const Job &job, Warnings &warnings)
{
    Settings s;
    s.machine = load_preset(job.machine, "machine", "presets.machine", warnings);
    s.process = load_preset(job.process, "process", "presets.process", warnings);
    for (size_t i = 0; i < job.filaments.size(); ++i)
        s.filaments.push_back(load_preset(job.filaments[i], "filament", "presets.filaments[" + std::to_string(i) + "]", warnings));
    const size_t n = s.filaments.size();

    // Is the process made for this printer? Orca's own rule (is_compatible_with_printer, which the
    // GUI uses): the process's compatible_printers lists the printer or the printer's parent, or,
    // with no list, its compatible_printers_condition holds for the printer. The stock command line
    // checks the list only, which refuses every process that has a condition and no list.
    {
        std::unique_ptr<Preset> printer = make_preset(Preset::TYPE_PRINTER, s.machine, Preset::printer_options());
        std::unique_ptr<Preset> process = make_preset(Preset::TYPE_PRINT, s.process, Preset::print_options());
        if (!is_compatible_with_printer(PresetWithVendorProfile(*process, nullptr), PresetWithVendorProfile(*printer, nullptr)))
            fail("process-not-compatible", "the process preset " + in_quotes(s.process.name) + " is not made for the printer " + in_quotes(s.machine.name) +
                                               " (its compatible_printers and compatible_printers_condition leave it out)");
        for (size_t i = 0; i < n; ++i) {
            std::unique_ptr<Preset> filament = make_preset(Preset::TYPE_FILAMENT, s.filaments[i], Preset::filament_options());
            if (!is_compatible_with_printer(PresetWithVendorProfile(*filament, nullptr), PresetWithVendorProfile(*printer, nullptr)))
                warnings.add("filament-not-compatible", "presets.filaments[" + std::to_string(i) + "]: " + in_quotes(s.filaments[i].name) +
                                                            " is not made for the printer " + in_quotes(s.machine.name) + "; Orca slices with it anyway");
            else if (!is_compatible_with_print(PresetWithVendorProfile(*filament, nullptr), PresetWithVendorProfile(*process, nullptr),
                                               PresetWithVendorProfile(*printer, nullptr)))
                warnings.add("filament-not-compatible", "presets.filaments[" + std::to_string(i) + "]: " + in_quotes(s.filaments[i].name) +
                                                            " is not made for the process " + in_quotes(s.process.name) + "; Orca slices with it anyway");
        }
    }

    // The merge of OrcaSlicer.cpp for a model that brings no project settings: an empty
    // configuration takes every key of the machine, then of the process, then each filament's
    // values at its index.
    DynamicPrintConfig &cfg = s.config;
    set_preset_slot(cfg, "different_settings_to_system", n + 1, n, "");
    set_preset_slot(cfg, "inherits_group", n + 1, n, s.machine.from == "system" ? "" : s.machine.inherits);

    s.machine.config.set_key_value("printer_settings_id", new ConfigOptionString(s.machine.name));
    add_default_gcodes(s.machine.config, Preset::TYPE_PRINTER);
    copy_preset_keys(cfg, s.machine.config, "presets.machine");
    const std::string printer_model = cfg.has("printer_model") ? cfg.opt_string("printer_model") : std::string();
    apply_cli_safe_limits(cfg, job.resources, printer_model);

    s.process.config.set_key_value("print_settings_id", new ConfigOptionString(s.process.name));
    {
        std::vector<std::string> compatible;
        if (const auto *opt = s.process.config.option<ConfigOptionStrings>("compatible_printers"))
            compatible = opt->values;
        cfg.option<ConfigOptionStrings>("print_compatible_printers", true)->values = compatible;
        set_preset_slot(cfg, "different_settings_to_system", 0, n, "");
        set_preset_slot(cfg, "inherits_group", 0, n, s.process.from == "system" ? "" : s.process.inherits);
    }
    copy_preset_keys(cfg, s.process.config, "presets.process");

    // Filaments: OrcaSlicer.cpp, "set the filament settings into print config", for filaments
    // given on the command line. Options with extruder variants (filament_options_with_variant)
    // keep one value per variant, in blocks that filament_self_index tells apart.
    std::map<std::string, std::vector<bool>> given;
    try {
        std::vector<int> old_start(n, 0), old_counts(n, 1), new_counts;
        ConfigOptionInts *self_index = cfg.option<ConfigOptionInts>("filament_self_index");
        if (self_index == nullptr) {
            self_index = cfg.option<ConfigOptionInts>("filament_self_index", true);
            self_index->values.resize(n);
            for (size_t i = 0; i < n; ++i)
                self_index->values[i] = int(i) + 1;
        }
        {
            const std::vector<int> old_self = self_index->values;
            int                    k = -1, current = 0;
            for (size_t i = 0; i < old_self.size(); ++i) {
                if (old_self[i] > current) {
                    current = old_self[i];
                    if (size_t(k + 1) >= n)
                        break;
                    old_start[++k] = int(i);
                    old_counts[k]  = 1;
                } else if (k >= 0) {
                    old_counts[k] += 1;
                }
            }
        }
        new_counts = old_counts;

        for (size_t index = 0; index < n; ++index) {
            const size_t        filament_index = index + 1;
            DynamicPrintConfig  config         = s.filaments[index].config;
            const std::string  &name           = s.filaments[index].name;
            add_default_gcodes(config, Preset::TYPE_FILAMENT);

            ConfigOptionStrings *settings_ids = static_cast<ConfigOptionStrings *>(cfg.option("filament_settings_id", true));
            {
                ConfigOptionString name_opt(name);
                if (settings_ids->size() < n)
                    settings_ids->resize(n, &name_opt);
                settings_ids->set_at(&name_opt, filament_index - 1, 0);
            }
            config.erase("filament_settings_id");
            set_preset_slot(cfg, "different_settings_to_system", filament_index, n, "");
            set_preset_slot(cfg, "inherits_group", filament_index, n, s.filaments[index].from == "system" ? "" : s.filaments[index].inherits);

            ConfigOptionStrings *ids = static_cast<ConfigOptionStrings *>(cfg.option("filament_ids", true));
            {
                ConfigOptionString id_opt(s.filaments[index].filament_id);
                if (ids->size() < n)
                    ids->resize(n, &id_opt);
                ids->set_at(&id_opt, filament_index - 1, 0);
            }

            ConfigOptionStrings *curr_variants = cfg.option<ConfigOptionStrings>("filament_extruder_variant");
            if (curr_variants == nullptr) {
                curr_variants = cfg.option<ConfigOptionStrings>("filament_extruder_variant", true);
                curr_variants->values.resize(n, get_extruder_variant_string(etDirectDrive, nvtStandard));
            }
            const ConfigOptionStrings *new_variants = dynamic_cast<const ConfigOptionStrings *>(config.option("filament_extruder_variant", true));
            const int new_variant_count = int(new_variants->size());
            const int old_variant_count = old_counts[filament_index - 1];

            for (const t_config_option_key &key : config.keys()) {
                const ConfigOption *src = config.option(key);
                if (src == nullptr)
                    fail("preset-invalid", "presets.filaments[" + std::to_string(index) + "]: " + key + " cannot be read");
                if (src->is_scalar()) {
                    if (key == "compatible_printers_condition") {
                        ConfigOptionStrings *dst = static_cast<ConfigOptionStrings *>(cfg.option("compatible_machine_expression_group", true));
                        if (dst->size() == 0) {
                            ConfigOptionString empty;
                            dst->resize(n + 2, &empty);
                        }
                        dst->set_at(src, filament_index, 0);
                    } else if (key == "compatible_prints_condition") {
                        ConfigOptionStrings *dst = static_cast<ConfigOptionStrings *>(cfg.option("compatible_process_expression_group", true));
                        if (dst->size() == 0) {
                            ConfigOptionString empty;
                            dst->resize(n, &empty);
                        }
                        dst->set_at(src, filament_index - 1, 0);
                    }
                    // Other single values of a filament preset do not reach the print.
                    continue;
                }
                if (key == "compatible_prints" || key == "compatible_printers" || key == "model_id" || key == "dev_model_name" || key == "filament_settings_id" ||
                    merge_metadata_key(key))
                    continue;
                ConfigOption *dst = cfg.option(key, true);
                if (dst == nullptr)
                    fail("preset-invalid", "presets.filaments[" + std::to_string(index) + "]: " + key + " cannot be stored");
                auto       *dst_vec = static_cast<ConfigOptionVectorBase *>(dst);
                const auto *src_vec = static_cast<const ConfigOptionVectorBase *>(src);
                // Orca's set_at and set_with_restore_2 read the first value of both lists unchecked.
                if (src_vec->empty())
                    fail("preset-invalid", "presets.filaments[" + std::to_string(index) + "]: " + key + " is an empty list");
                if (dst_vec->empty())
                    dst_vec->resize(1, src_vec); // the machine or process preset gave it as an empty list
                if (filament_options_with_variant.count(key)) {
                    std::vector<int> restore(new_variant_count, -1);
                    dst_vec->set_with_restore_2(src_vec, restore, old_start[filament_index - 1], old_variant_count, true);
                    if (key == "filament_extruder_variant")
                        new_counts[filament_index - 1] = int(src_vec->size());
                } else {
                    dst_vec->set_at(src_vec, filament_index - 1, 0);
                    std::vector<bool> &by = given[key];
                    by.resize(n, false);
                    by[filament_index - 1] = true;
                }
            }
            if (old_variant_count != new_variant_count)
                for (size_t i = index + 1; i < n; ++i)
                    old_start[i] += new_variant_count - old_variant_count;
        }

        if (const ConfigOptionStrings *variants = cfg.option<ConfigOptionStrings>("filament_extruder_variant")) {
            std::vector<int> &self = cfg.option<ConfigOptionInts>("filament_self_index", true)->values;
            self.resize(variants->size(), 1);
            size_t k = 0;
            for (size_t i = 0; i < n; ++i)
                for (int j = 0; j < new_counts[i] && k < self.size(); ++j)
                    self[k++] = int(i) + 1;
        }
    } catch (const Failure &) {
        throw;
    } catch (const std::exception &e) {
        fail("preset-invalid", std::string("the filament presets cannot be merged: ") + e.what());
    }

    // The job's overrides, read as the stock command line reads its own options (no substitutions).
    DynamicPrintConfig extra;
    for (const auto &kv : job.overrides)
        s.overridden.push_back(parse_setting(extra, kv.first, kv.second, "overrides"));

    pad_filament_options(cfg, given, n);
    set_filament_colours(cfg, extra, s.filaments);
    check_filament_overrides(extra, cfg, n);

    s.printer_model_id = find_printer_model_id(job.resources, printer_model);
    s.is_bbl           = boost::starts_with(printer_model, "Bambu Lab") ||
               (printer_model.empty() && boost::starts_with(s.machine.from == "system" ? s.machine.name : s.machine.inherits, "Bambu Lab"));

    // The plate: the job's curr_bed_type, else the plate the GUI picks for this printer.
    if (!extra.has("curr_bed_type"))
        cfg.set_key_value("curr_bed_type", new ConfigOptionEnum<BedType>(default_bed_type(s.machine.config, s.printer_model_id)));

    // The overrides go in before the flushing volumes are computed, which read some of them, and the
    // nozzles are counted on the settings the print gets (sketch-orca, 2026-11-01).
    cfg.apply(extra, true);
    s.extruder_count = 1;
    if (const auto *nozzles = cfg.option<ConfigOptionFloats>("nozzle_diameter"))
        s.extruder_count = std::max<size_t>(1, nozzles->values.size());
    const size_t printer_variant_count = cfg.option<ConfigOptionStrings>("printer_extruder_variant", true)->values.size();
    compute_flush_volumes(cfg, extra.has("flush_volumes_matrix"), s.extruder_count, printer_variant_count);
    try {
        cfg.normalize_fdm();
    } catch (const std::exception &e) {
        fail("config-invalid", std::string("Orca cannot normalize the settings: ") + e.what());
    }
    cfg.option<ConfigOptionEnum<PrinterTechnology>>("printer_technology", true)->value = ptFFF;
    {
        FullPrintConfig fff;
        fff.apply(cfg, true);
        cfg.apply(fff, true);
    }
    size_filament_options(cfg, n);
    const std::map<std::string, std::string> invalid = cfg.validate(true);
    if (!invalid.empty()) {
        std::vector<std::string> parts;
        for (const auto &kv : invalid)
            parts.push_back(kv.first + ": " + kv.second);
        fail("config-invalid", "Orca refuses these settings: " + join(parts, "; "));
    }
    return s;
}

} // namespace sketch_orca
