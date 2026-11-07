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
// placeholder.sop1: PlaceholderParser::evaluate_boolean_expression, the presets' compatibility
// conditions, as the engine's own build evaluates them (parity/SOP1.md, "Probe files": PHCF and
// PHEV), which orca-placeholder's tests/probes.rs replays. Recorded by the o2-front task,
// 2026-11-07 (the stub registered the same day: scripts/lib/parity/probe.ts, PROBE_REGISTRY).
//
// Both sets: hand-written expressions covering every operator, function, type error, regular
// expression form and regex error, and syntax errors at many positions, against one configuration
// (a printer's and a process's defaults with a few values set), without and with a printer's
// override. The full set adds every condition of the vendor profiles (the oracle's source tree,
// <exe>/../src/resources/profiles, found from the probe's own path as mesh.cpp finds its STL
// files, so a --check run into another folder reads the same profiles) against every printer (compatible_printers_condition, with the override is_compatible_with_printer
// gives) or process (compatible_prints_condition) of its vendor, each preset's configuration built
// as the presets are: its type's defaults, then each file of its inherits chain read with
// load_from_json, the root first.
#include <algorithm>
#include <cctype>
#include <cstdint>
#include <cstdlib>
#include <exception>
#include <filesystem>
#include <fstream>
#include <map>
#include <memory>
#include <set>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

#include "libslic3r/Exception.hpp"
#include "libslic3r/PlaceholderParser.hpp"
#include "libslic3r/Preset.hpp"
#include "libslic3r/PrintConfig.hpp"

#include "probe.hpp"

namespace probe {

namespace {

using Slic3r::DynamicConfig;
using Slic3r::DynamicPrintConfig;
namespace fs = std::filesystem;

// The identifiers a set of expressions could name: every [A-Za-z_][A-Za-z0-9_]* run, strings and
// patterns included (a superset, so a configuration recorded with these keys looks the same to
// each expression).
std::set<std::string> identifiers(const std::vector<std::string> &templates)
{
    std::set<std::string> out;
    for (const std::string &t : templates) {
        size_t i = 0;
        while (i < t.size()) {
            unsigned char c = (unsigned char) t[i];
            if ((c < 0x80 && std::isalpha(c)) || c == '_') {
                size_t j = i + 1;
                while (j < t.size() && (((unsigned char) t[j] < 0x80 && std::isalnum((unsigned char) t[j])) || t[j] == '_'))
                    ++j;
                out.insert(t.substr(i, j - i));
                i = j;
            } else
                ++i;
        }
    }
    // A percent resolves through its "ratio_over" chain (scalar_variable_to_expr), so the chain's
    // keys belong to the configuration too.
    std::vector<std::string> todo(out.begin(), out.end());
    while (! todo.empty()) {
        std::string k = todo.back();
        todo.pop_back();
        const Slic3r::ConfigOptionDef *def = Slic3r::print_config_def.get(k);
        if (def != nullptr && ! def->ratio_over.empty() && out.insert(def->ratio_over).second)
            todo.push_back(def->ratio_over);
    }
    return out;
}

// PHCF: a configuration's id, its name and the keys of `keys` it has, each serialized.
void add_config(File &f, uint32_t id, const std::string &name, const DynamicConfig &config, const std::set<std::string> &keys)
{
    std::vector<std::pair<std::string, std::string>> kv;
    for (const std::string &k : keys)
        if (const Slic3r::ConfigOption *o = config.option(k))
            kv.emplace_back(k, o->serialize());
    Payload p;
    p.u32(id).str(name).u64(kv.size());
    for (const auto &e : kv)
        p.str(e.first).str(e.second);
    f.add("PHCF", p);
}

// PHEV: one evaluation, its outcome (0 a result, 1 PlaceholderParserError, 2 another
// std::exception, 3 anything else), the result and the exception's what().
void add_eval(File &f, uint32_t config_id, int32_t override_id, const std::string &templ, const DynamicConfig &config,
              const DynamicConfig *config_override)
{
    uint8_t     outcome = 0;
    bool        result  = false;
    std::string message;
    try {
        result = Slic3r::PlaceholderParser::evaluate_boolean_expression(templ, config, config_override);
    } catch (const Slic3r::PlaceholderParserError &e) {
        outcome = 1;
        message = e.what();
    } catch (const std::exception &e) {
        outcome = 2;
        message = e.what();
    } catch (...) {
        outcome = 3;
    }
    Payload p;
    p.u32(config_id).i32(override_id).str(templ).u8(outcome).boolean(result).str(message);
    f.add("PHEV", p);
}

// The hand-written expressions.
const std::vector<std::string> &hand_written()
{
    static const std::vector<std::string> t = {
        // values, comparisons and their types
        R"(true)", R"(false)", R"(1 == 1)", R"(1 != 2)", R"(1 <> 2)", R"(1 < 2)", R"(2 <= 2)", R"(3 > 2)", R"(2 >= 3)",
        R"(1.5 == 1.5)", R"(0.1 + 0.2 == 0.3)", R"(1 == 1.000000001)", R"(1 == 1.0001)", R"("a" == "a")", R"("a" < "b")",
        R"("b" > "a")", R"("10" < 9)", R"(true == true)", R"(true != false)", R"(true < false)", R"(1 == true)", R"("x" == 1)",
        R"(true == "true")",
        // arithmetic
        R"(1 + 2 * 3 == 7)", R"((1 + 2) * 3 == 9)", R"(7 / 2 == 3)", R"(-7 / 2 == -3)", R"(7 % 3 == 1)", R"(-7 % 3 == -1)",
        R"(7.5 % 2 == 1.5)", R"(7 / 2.0 == 3.5)", R"(1 / 0 == 0)", R"(1 % 0 == 0)", R"(1 / 0.0 == 0)", R"(1.0 / -0.0 == 0)",
        R"(2147483647 + 1 < 0)", R"(-2147483647 - 1 < 0)", R"(2147483647 * 2 == -2)", R"("a" + 1 == "a1")", R"(1 + "a" == "1a")",
        R"("a" + 0.5 == "a0.5")", R"("v" + 1e20 == "v1e+20")", R"("v" + 1/3.0 == "v0.333333")", R"("v" + 100000.0 == "v100000")",
        R"("v" + 1234567.0 == "v1.23457e+06")", R"(true + "x" == "truex")", R"(true + 1)", R"("a" - 1)", R"("a" * 2)", R"(2 / "a")",
        R"("a" / 2)", R"(1 % true)", R"(2 - true)",
        // unary operators
        R"(-1 < 0)", R"(--1 == 1)", R"(-"a")", R"(+"a" == "a")", R"(!true == false)", R"(not false)", R"(!1)", R"(not "a")",
        R"(- -2147483647 == 2147483647)", R"(!!true)",
        // functions
        R"(min(1, 2) == 1)", R"(max(1, 2.5) == 2.5)", R"(min(2.5, 1) == 1)", R"(min("a", 1))", R"(max(1, "a"))", R"(random(1, 2))",
        R"(int(2.7) == 2)", R"(int(-2.7) == -2)", R"(int("a"))", R"(round(2.5) == 3)", R"(round(-2.5) == -3)", R"(round(true))",
        R"(floor(-1.5) == -2)", R"(ceil(1.2) == 2)", R"(floor("a"))", R"(ceil(true))", R"(int(1e10) < 0)", R"(int(3) == 3)",
        R"(digits(3.14159, 6, 2) == "  3.14")", R"(zdigits(3.14159, 6, 2) == "003.14")", R"(digits(2.5, 3) == "  3")",
        R"(zdigits(-7, 4) == "-007")", R"(digits(1, 2.5))", R"(digits(1, 2, 3.5))", R"(digits("a", 2))", R"(digits(1, -5) == "1")",
        R"(digits(1, 100, 100) == "1")", R"(filament_change(1))", R"(filament_change(1) == 1)", R"(1 + filament_change(0))",
        // variables
        R"(nozzle_diameter[0] == 0.4)", R"(nozzle_diameter[1] == 0.6)", R"(nozzle_diameter[5] == 0.4)", R"(nozzle_diameter[-1] == 0.4)",
        R"(nozzle_diameter == 0.4)", R"(nozzle_diameter[0.5])", R"(nozzle_diameter["a"])", R"(nozzle_diameter[1 + 0] == 0.6)",
        R"(printer_model == "MK3S")", R"(printer_model[0] == "MK3S")", R"(unknown_variable == 1)", R"(printer_technology == "FFF")",
        R"(gcode_flavor == "marlin2")", R"(use_relative_e_distances)", R"(printable_height > 0)", R"(printable_area == 1)",
        R"(retract_length[0] > 0)", R"(extruder_colour[0] == "")", R"(extruder_colour == "")", R"(sparse_infill_density == 15)",
        R"(seam_slope_start_height == 0.1)", R"(small_perimeter_speed > 0)", R"(wipe_speed > 0)", R"(bridge_acceleration > 0)",
        R"(initial_layer_travel_speed > 0)", R"(is_nil(filament_flow_ratio[0]))", R"(is_nil(filament_flow_ratio[1]))",
        R"(filament_flow_ratio[0] == 1)", R"(filament_flow_ratio == 1)", R"(is_nil(printer_model))", R"(is_nil(nozzle_diameter))",
        R"(is_nil(nozzle_diameter[0]))", R"(empty(nozzle_diameter))", R"(size(nozzle_diameter) == 2)", R"(empty(printer_model))",
        R"(size(nozzle_diameter[0]))", R"(size(printer_model))", R"(empty(retract_length) == false)", R"(layer_height == 0.2)",
        R"(wall_loops == 3)", R"(printer_preset == "Test Printer")", R"(num_extruders == 2)",
        // the ternary operator and skipped branches
        R"(true ? true : false)", R"(false ? true : false)", R"(1 ? true : false)", R"(true ? 1 == 1 : unknown_var)",
        R"(false ? unknown_var : true)", R"(true ? (false ? 1 : 2) == 2 : 3)", R"(true ? true : floor(2.5) == 2)",
        R"(false ? ceil(1.5) : true)", R"(true ? true : int(2.5) == 2)", R"(true ? true : -1 == 1)", R"(true ? "a" : "b")",
        R"(true ? 1 : 2)", R"(false ? 1 : true ? true : 3)", R"(true ? true : min(1, 2) == 1)", R"(true ? true : "a" * 2)",
        R"(true ? true : digits(1, 2))", R"(true ? true : one_of(1, "a"))", R"(true ? true : interpolate_table(9, (1, 1)))",
        R"(true ? true : random(1, 2))", R"(true ? true : nozzle_diameter[0.5])", R"(true ? true : x =~ /(/)",
        // logical operators
        R"(true and false)", R"(true && true)", R"(false or true)", R"(false || false)", R"(true and 1)", R"(1 or true)",
        R"(true and not false)", R"(1 == 1 and 2 == 2 or false)", R"(true andx)", R"(true and (1 == 1))",
        // regular expressions
        R"(printer_notes =~ /.*PRINTER_MODEL_MK3S.*/)", R"(printer_notes !~ /.*MK4.*/)", R"(printer_model =~ /MK3S/)",
        R"(printer_model =~ /mk3s/)", R"(printer_model =~ /MK.*/)", R"(printer_model =~ /MK[0-9]S/)", R"(printer_model =~ /MK[^0-9]S/)",
        R"(printer_model =~ /MK\dS/)", R"(printer_model =~ /(MK|XL)3S?/)", R"(printer_model =~ /M{1,2}K3S/)",
        R"(printer_model =~ /^MK3S$/)", R"(printer_notes =~ /.*^PRINTER_MODEL_MK3S$.*/)", R"(printer_notes =~ /^PRINTER_MODEL_MK3S$/)",
        R"(printer_model =~ /MK3S|MK4/)", R"(printer_model =~ /\w+/)", R"(printer_model =~ /\s*MK3S/)", R"(printer_model =~ /\W*MK3S/)",
        R"(printer_model =~ /[\w]+/)", R"(printer_model =~ /[^\d]+3S/)", R"(printer_model =~ /(?:MK)+3S/)", R"(printer_model =~ /MK3S+?/)",
        R"(printer_model =~ /MK3S{1}/)", R"(printer_model =~ /MK3S{1,}/)", R"(printer_model =~ /MK3S{,1}/)", R"(printer_model =~ /x{2/)",
        R"(printer_model =~ /}/)", R"(printer_model =~ /\./)", R"(printer_model =~ /[]a]/)", R"(printer_model =~ /[a-]/)",
        R"(printer_model =~ /a\/b/)", R"(printer_model =~ //)", R"(printer_model =~ /|MK3S/)", R"(printer_model =~ /()MK3S/)",
        R"(1 =~ /1/)", R"(printer_model =~ /*/)", R"(printer_model =~ /(/)", R"(printer_model =~ /)/)", R"(printer_model =~ /[/)",
        R"(printer_model =~ /[a/)", R"(printer_model =~ /[z-a]/)", R"(printer_model =~ /a{3,1}/)", R"(printer_model =~ /a**/)",
        R"(printer_model =~ /?/)", R"(printer_model =~ /+a/)", R"(printer_model =~ /(MK3S/)", R"(printer_model =~ /[^/)",
        R"(printer_model =~ /[a-z/)", R"(printer_model =~ /abcdefghijklmnopqrstuvwxyz(/)", R"(printer_model =~ /a|*/)",
        R"(printer_model =~ /^*/)", R"(printer_model =~ /{2}/)", R"(printer_model =~ /a{2}{3}/)", R"(printer_model =~ /[a-\d]/)",
        R"(printer_model =~ /[\d-z]/)", R"(printer_model =~ /\/)", R"(printer_model =~ 5)", R"(printer_model =~ /x)",
        R"(printer_model =~   /x/)", "printer_model =~ /\xc3\xa9/", "printer_model =~ /[\xc3\xa9]/", "printer_model =~ /\xc3\xa9(/",
        // escapes: classes of one letter, class letters that name none, unescape_character (2026-11-07)
        R"(printer_model =~ /\MK\d\S/)", R"(printer_model =~ /\u\u\d\u/)", R"(printer_model =~ /\l+/)",
        R"(printer_model =~ /[\u\d]+/)", R"(printer_model =~ /[\L]+/)", R"(printer_notes =~ /.*\v.*\v/)",
        R"(printer_notes =~ /\h*.*/)", R"(printer_model =~ /[\I-\O]K3S/)", R"(printer_model =~ /\Y/)",
        R"(printer_model =~ /[\cM]K3S/)", R"(printer_model =~ /\cMK3S/)", R"(printer_model =~ /\c/)", R"(printer_model =~ /[\/)",
        R"(printer_model =~ /\E\e/)", R"(printer_model =~ /[\b\a\e\f\n\r\t\v]/)", R"(printer_model =~ /[\V]+/)",
        R"(printer_model =~ /[\H\W]+/)", "printer_model =~ /\\\xc3\\\xa9/", "printer_model =~ /[\\\xc3]/",
        "\"\x1c\" =~ /\\s/", "\"\x1f\" =~ /\\s/", "\"\x0b\" =~ /\\s/", "\"\x0b\" =~ /\\h/", "\"\x1c\" =~ /\\h/",
        "\"\x7f\" =~ /\\w/", "\"_\" =~ /\\w/",
        // error lines boost::nowide rewrites (2026-11-07)
        "true \xc0\x80", "true \xe0\x80\x80 x", "true \xed\xa0\x80", "true \xf4\x90\x80\x80", "true \xf8\x80\x80\x80",
        "\"\xe2(\" == 1", "\"a\xe2\x82\" == 1 x", "1 == 1 \xc3\xa9\xff",
        // one_of
        R"(one_of(printer_model, "MK3S", "MK4"))", R"(one_of(printer_model, "MK4"))", R"(one_of(printer_model))",
        R"(one_of(printer_model, ))", R"(one_of(printer_model, ~"MK.*"))", R"(one_of(printer_model, /MK.*/))",
        R"(one_of(printer_model, "a", /MK3S/, ~"x"))", R"(one_of(1, "a"))", R"(one_of(printer_model, 1))", R"(one_of(printer_model, ~"("))",
        R"(one_of(printer_model, /(/))", R"(one_of(printer_model "a"))", R"(one_of(printer_model, "a" "MK3S"))",
        R"(one_of(printer_model, "a") + 1)", R"(one_of(1 > 0 ? "MK3S" : "x", "MK3S"))", R"(one_of(printer_model, "MK3S", 1))",
        // interpolate_table
        R"(interpolate_table(1.5, (1,10), (2,20)) == 15)", R"(interpolate_table(1, (1,10), (2,20)) == 10)",
        R"(interpolate_table(0.99995, (1,10), (2,20)) == 10)", R"(interpolate_table(3, (1,10), (2,20)))",
        R"(interpolate_table(1.5, (2,10), (1,20)))", R"(interpolate_table("a", (1,10)))", R"(interpolate_table(1, ("a",10)))",
        R"(interpolate_table(1, (1,"b")))", R"(interpolate_table(1.5, (1,10) (2,20)) == 15)", R"(interpolate_table(1, (1,10)))",
        R"(interpolate_table(1.00001, (1,10), (1.00002,20)) == 15)", R"(interpolate_table(1.5, (1,10), (2,20)) + 1 == 16)",
        R"(interpolate_table(2.00005, (1,10), (2,20)) == 20)", R"(interpolate_table(1.5 (1,10)))",
        // strings and escapes
        R"("a\nb" == "a" + "\n" + "b")", R"("a\"b" == "a\"b")", R"("a\tb")", R"("\\" == "\\")", "\"\xc3\xa9\" == \"\xc3\xa9\"",
        R"("abc)", R"("a\)", R"("a\rb" != "ab")",
        // numbers
        R"(1. == 1)", R"(.5 == 0.5)", R"(1e3 == 1000)", R"(1E-3 == 0.001)", R"(1e == 1)", R"(2147483648 == 0)", R"(0x10)",
        R"(1e400 == 1)", R"(007 == 7)", R"(1.5e+2 == 150)", R"(0.30000000000000004 == 0.3)", R"(123456789012345678901.5 > 1)",
        R"(1e-400 == 0)",
        // syntax errors, at many positions
        "", " ", R"(1 +)", R"((1 == 1)", R"(1 == 1))", R"(min(1))", R"(min(1,)", R"(min 1)", R"(digits(1, 2)", R"(digits(1, 2 3))",
        R"(int 1)", R"(is_nil(1))", R"(is_nil(printer_model)", R"(empty())", R"(1 == == 1)", R"(1 ? true)", R"(true ? true :)",
        R"(true ? : false)", R"())", R"(and)", R"(true and)", R"(true or)", R"(not)", R"(1 = 1)", R"(1 == 1 ; 2)", "\t\n true \r\n",
        "true\n\nfalse", "1 ==\n\n 2 x", "\xc3\xa9", "true \xc3\xa9", "\"x\" == \"x\"\xc3\xa9", "\xff", "true \x80", "\xc3",
        "\"\xc3\" == 1", R"(printer_model[0)", R"(printer_model[])", R"(printer_model[0]x)", R"(_x == 1)", R"(trueish)",
        R"(true_ == 1)", R"(orx)", R"(if)", R"(min)", R"(1 <)", R"(1 < < 2)", R"(1 - - 1 == 2)", R"(- )", R"(1 ==)", R"(true ==)",
        R"((true))", R"(((true)))", R"((1 == 1) == true)", R"(interpolate_table(1))", R"(one_of())", R"(size(1))", R"(1 2)",
        "1 ==\n 1 +\n \"a\" * 2",
    };
    return t;
}

// The hand-written expressions' configuration: a printer's and a process's defaults, with a few
// values set.
DynamicPrintConfig hand_config()
{
    DynamicPrintConfig                  c;
    std::unique_ptr<DynamicPrintConfig> printer(DynamicPrintConfig::new_from_defaults_keys(Slic3r::Preset::printer_options()));
    std::unique_ptr<DynamicPrintConfig> print(DynamicPrintConfig::new_from_defaults_keys(Slic3r::Preset::print_options()));
    c.apply(*printer);
    c.apply(*print);
    Slic3r::ConfigSubstitutionContext                         ctx(Slic3r::ForwardCompatibilitySubstitutionRule::Enable);
    const std::vector<std::pair<std::string, std::string>> set = {
        {"printer_notes", "PRINTER_VENDOR_PRUSA3D\\nPRINTER_MODEL_MK3S\\n"},
        {"printer_model", "MK3S"},
        {"nozzle_diameter", "0.4,0.6"},
        {"retract_length", "0.8,0.8"},
        {"sparse_infill_density", "15%"},
        {"layer_height", "0.2"},
        {"wall_loops", "3"},
        {"seam_slope_start_height", "50%"},
        {"small_perimeter_speed", "50%"},
        {"wipe_speed", "80%"},
        {"bridge_acceleration", "50%"},
        {"initial_layer_travel_speed", "100%"},
        {"filament_flow_ratio", "nil,0.98"},
    };
    for (const auto &kv : set)
        c.set_deserialize(kv.first, kv.second, ctx);
    return c;
}

// The override is_compatible_with_printer(preset, printer) gives: the printer's name and its
// nozzle count.
DynamicConfig printer_override(const std::string &name, const DynamicPrintConfig &printer)
{
    DynamicConfig o;
    o.set_key_value("printer_preset", new Slic3r::ConfigOptionString(name));
    const Slic3r::ConfigOption *nd = printer.option("nozzle_diameter");
    int n = nd ? int(static_cast<const Slic3r::ConfigOptionFloats *>(nd)->values.size()) : 1;
    o.set_key_value("num_extruders", new Slic3r::ConfigOptionInt(n));
    return o;
}

// One vendor's presets: name to file, for each kind.
struct Vendor
{
    std::string                     name;
    std::map<std::string, fs::path> machines, processes, filaments;
};

nlohmann::json read_json(const fs::path &p)
{
    std::ifstream f(p);
    return nlohmann::json::parse(f);
}

std::vector<Vendor> read_vendors(const fs::path &profiles)
{
    std::vector<Vendor>   out;
    std::vector<fs::path> index;
    for (const auto &e : fs::directory_iterator(profiles))
        if (e.is_regular_file() && e.path().extension() == ".json")
            index.push_back(e.path());
    std::sort(index.begin(), index.end());
    for (const fs::path &p : index) {
        try {
            nlohmann::json j = read_json(p);
            Vendor         v;
            v.name    = p.stem().string();
            auto list = [&](const char *key, std::map<std::string, fs::path> &m) {
                if (!j.contains(key))
                    return;
                for (const auto &e : j[key])
                    if (e.contains("name") && e.contains("sub_path"))
                        m[e["name"].get<std::string>()] = profiles / v.name / e["sub_path"].get<std::string>();
            };
            list("machine_list", v.machines);
            list("process_list", v.processes);
            list("filament_list", v.filaments);
            out.push_back(std::move(v));
        } catch (const std::exception &) {
        }
    }
    return out;
}

// A preset's configuration: what `config` holds, then each file of its inherits chain, the root
// first.
bool load_chain(DynamicPrintConfig &config, const std::map<std::string, fs::path> &files, const std::string &name, int depth = 0)
{
    auto it = files.find(name);
    if (it == files.end() || depth > 16)
        return false;
    try {
        nlohmann::json j = read_json(it->second);
        if (j.contains("inherits") && j["inherits"].is_string() && !j["inherits"].get<std::string>().empty())
            load_chain(config, files, j["inherits"].get<std::string>(), depth + 1);
    } catch (const std::exception &) {
        return false;
    }
    Slic3r::ConfigSubstitutionContext  ctx(Slic3r::ForwardCompatibilitySubstitutionRule::Enable);
    std::map<std::string, std::string> key_values;
    std::string                        reason;
    return config.load_from_json(it->second.string(), ctx, false, key_values, reason) == 0;
}

// The distinct conditions `key` of the files.
std::vector<std::string> conditions(const std::map<std::string, fs::path> &files, const char *key)
{
    std::set<std::string> out;
    for (const auto &e : files) {
        try {
            nlohmann::json j = read_json(e.second);
            if (j.contains(key) && j[key].is_string() && !j[key].get<std::string>().empty())
                out.insert(j[key].get<std::string>());
        } catch (const std::exception &) {
        }
    }
    return {out.begin(), out.end()};
}

bool instantiated(const fs::path &p)
{
    try {
        nlohmann::json j = read_json(p);
        return !j.contains("instantiation") || j["instantiation"] == "true" || j["instantiation"] == true;
    } catch (const std::exception &) {
        return false;
    }
}

void record_vendors(File &full, const fs::path &profiles, uint32_t &next_id)
{
    for (const Vendor &v : read_vendors(profiles)) {
        std::vector<std::string> printer_conds = conditions(v.processes, "compatible_printers_condition");
        for (const std::string &c : conditions(v.filaments, "compatible_printers_condition"))
            printer_conds.push_back(c);
        std::sort(printer_conds.begin(), printer_conds.end());
        printer_conds.erase(std::unique(printer_conds.begin(), printer_conds.end()), printer_conds.end());
        std::vector<std::string> print_conds = conditions(v.filaments, "compatible_prints_condition");
        if (!printer_conds.empty()) {
            std::set<std::string> keys = identifiers(printer_conds);
            for (const auto &m : v.machines) {
                if (!instantiated(m.second))
                    continue;
                std::unique_ptr<DynamicPrintConfig> config(DynamicPrintConfig::new_from_defaults_keys(Slic3r::Preset::printer_options()));
                if (!load_chain(*config, v.machines, m.first))
                    continue;
                DynamicConfig o  = printer_override(m.first, *config);
                uint32_t      id = next_id++, oid = next_id++;
                add_config(full, id, v.name + "/machine/" + m.first, *config, keys);
                add_config(full, oid, v.name + "/override/" + m.first, o, keys);
                for (const std::string &c : printer_conds)
                    add_eval(full, id, int32_t(oid), c, *config, &o);
            }
        }
        if (!print_conds.empty()) {
            std::set<std::string> keys = identifiers(print_conds);
            for (const auto &m : v.processes) {
                if (!instantiated(m.second))
                    continue;
                std::unique_ptr<DynamicPrintConfig> config(DynamicPrintConfig::new_from_defaults_keys(Slic3r::Preset::print_options()));
                if (!load_chain(*config, v.processes, m.first))
                    continue;
                uint32_t id = next_id++;
                add_config(full, id, v.name + "/process/" + m.first, *config, keys);
                for (const std::string &c : print_conds)
                    add_eval(full, id, -1, c, *config, nullptr);
            }
        }
    }
}

bool run_placeholder(const Options &options, std::string &error)
{
    Sets sets;
    head(sets, "placeholder");
    const std::vector<std::string> &t      = hand_written();
    DynamicPrintConfig              config = hand_config();
    DynamicConfig                   o      = printer_override("Test Printer", config);
    std::set<std::string>           keys   = identifiers(t);
    for (File *f : {&sets.committed, &sets.full}) {
        add_config(*f, 0, "hand-written", config, keys);
        add_config(*f, 1, "hand-written override", o, keys);
        for (const std::string &templ : t) {
            add_eval(*f, 0, -1, templ, config, nullptr);
            add_eval(*f, 0, 1, templ, config, &o);
        }
    }
    // The profiles from the probe's own path, <exe>/../src (2026-11-07: the full set's folder
    // gave none on a --check run, whose full set goes to <work>/parity/runs/probe-check/full).
    char *pgm = nullptr;
    if (_get_pgmptr(&pgm) != 0 || pgm == nullptr) {
        error = "cannot find the probe's own path";
        return false;
    }
    const fs::path profiles = fs::path(pgm).parent_path().parent_path() / "src" / "resources" / "profiles";
    if (!fs::is_directory(profiles)) {
        error = "no vendor profiles in " + profiles.string();
        return false;
    }
    uint32_t next_id = 2;
    record_vendors(sets.full, profiles, next_id);
    return save(sets, options, "placeholder.sop1", error);
}

} // namespace

const Target &target()
{
    static const Target t{"placeholder", false, run_placeholder};
    return t;
}

} // namespace probe
