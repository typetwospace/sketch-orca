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
// layer.sop1's records of the piece front-1 (orca-config; parity/sop1/layer.md): the static print
// configs (LYCK their keys, values, hash and DynamicPrintConfig; LYCA get_abs_value; LYCD equals,
// diff and == against a changed copy; LYCO apply_only), DynamicPrintConfig's full_print_config and
// new_from_defaults_keys (LYDY), get_extruder_ams_count (LYAM), compute_filament_override_value
// (LYFO), has_skirt (LYSK) and MaterialType's table (LYMT). Recorded by orca-o3-probe-layer,
// 2026-11-13.
#include "common.hpp"

#include <algorithm>
#include <map>
#include <memory>
#include <set>

#include "libslic3r/Config.hpp"
#include "libslic3r/MaterialType.hpp"
#include "libslic3r/PrintConfig.hpp"

namespace probe {
namespace layer {

namespace {

// Each static class as one ConfigBase: the classes with several parents are cast to their first,
// as DynamicPrintConfig::full_print_config() casts FullPrintConfig to PrintRegionConfig; the
// virtual optptr() and keys() are still the class's own.
const StaticPrintConfig &base(const PrintObjectConfig &c) { return c; }
const StaticPrintConfig &base(const PrintRegionConfig &c) { return c; }
const StaticPrintConfig &base(const MachineEnvelopeConfig &c) { return c; }
const StaticPrintConfig &base(const GCodeConfig &c) { return c; }
const StaticPrintConfig &base(const PrintConfig &c) { return static_cast<const MachineEnvelopeConfig &>(c); }
const StaticPrintConfig &base(const FullPrintConfig &c) { return static_cast<const PrintRegionConfig &>(c); }
const StaticPrintConfig &base(const SLAPrintConfig &c) { return c; }
const StaticPrintConfig &base(const SLAPrintObjectConfig &c) { return c; }
const StaticPrintConfig &base(const SLAMaterialConfig &c) { return c; }
const StaticPrintConfig &base(const SLAPrinterConfig &c) { return c; }
const StaticPrintConfig &base(const SLAFullPrintConfig &c) { return static_cast<const SLAPrinterConfig &>(c); }
template<class T> StaticPrintConfig &base_mut(T &c) { return const_cast<StaticPrintConfig &>(base(static_cast<const T &>(c))); }

bool numeric(ConfigOptionType t) { return t == coFloat || t == coInt || t == coBool || t == coFloatOrPercent || t == coPercent; }

// Whether get_abs_value(key) stays within defined behaviour. It follows the option's ratio_over
// chain (a zero *_line_width goes to line_width): the call reads a missing option as an
// exception and a float, int, bool or absolute float-or-percent as its value, but a percent whose
// chain ends at an option of another type (nozzle_diameter, a vector) dereferences a null pointer
// or casts it to the wrong type, and a chain that loops recurses without end.
bool abs_defined(const ConfigBase &c, const std::string &key, std::set<std::string> &seen)
{
    if (!seen.insert(key).second)
        return false;
    const ConfigOption *o = c.option(key);
    if (o == nullptr)
        return true;
    const ConfigOptionType t = o->type();
    if (t == coFloat || t == coInt || t == coBool)
        return true;
    const ConfigOptionDef *def = c.def() != nullptr ? c.def()->get(key) : nullptr;
    if (t == coFloatOrPercent) {
        const auto *f = static_cast<const ConfigOptionFloatOrPercent *>(o);
        if (f->value == 0 && key.size() >= 11 && key.compare(key.size() - 11, 11, "_line_width") == 0)
            return abs_defined(c, "line_width", seen);
        if (!f->percent)
            return true;
        return def != nullptr && (def->ratio_over.empty() || abs_defined(c, def->ratio_over, seen));
    }
    if (t == coPercent)
        return def != nullptr && def->ratio_over.empty();
    return false;
}

// get_abs_value(key) as `u8` 1 and `f64`, 0 and the exception's text, or 2 when the call is not
// made because it would leave defined behaviour (abs_defined).
void put_abs(Payload &p, const ConfigBase &c, const std::string &key)
{
    std::set<std::string> seen;
    if (!abs_defined(c, key, seen)) {
        p.u8(2);
        return;
    }
    attempt(p, [&](Payload &q) { q.f64(c.get_abs_value(key)); });
}

// A new value for an option, as set_deserialize takes it, unlike its current one where the type
// allows; false for the types it does not change (the groups and the empty enums).
bool new_value(SplitMix &g, const std::string &key, const ConfigOption &cur, std::string &out)
{
    const ConfigOptionDef *def = print_config_def.get(key);
    const std::string      now = cur.serialize();
    auto pick = [&](std::initializer_list<const char *> list) {
        std::vector<std::string> v(list.begin(), list.end());
        out = v[size_t(g.below(v.size()))];
        if (out == now)
            out = v[(size_t(g.below(v.size())) + 1) % v.size()];
    };
    switch (cur.type()) {
    case coFloat: pick({"0", "0.35", "12.5", "-1", "1e3", "0.0001"}); return true;
    case coInt: pick({"0", "1", "7", "-3", "250"}); return true;
    case coBool: out = now == "1" ? "0" : "1"; return true;
    case coPercent: pick({"15%", "100%", "0%", "37.5%"}); return true;
    case coFloatOrPercent: pick({"0.45", "120%", "0", "1.2", "0%"}); return true;
    case coString: pick({"probe", "", "a;b", "{layer_num}"}); return true;
    case coFloats: pick({"0.3,0.5", "1", "0", "2.5,2.5,2.5"}); return true;
    case coInts: pick({"1,2", "0", "5"}); return true;
    case coBools: pick({"1,0", "1", "0"}); return true;
    case coStrings: pick({"\"a\";\"b\"", "\"\"", "\"x\""}); return true;
    case coPercents: pick({"10%,20%", "100%"}); return true;
    case coFloatsOrPercents: pick({"0.4,50%", "0%"}); return true;
    case coPoint: pick({"10x20", "0x0", "-5x2.5"}); return true;
    case coPoints: pick({"0x0,200x0,200x200,0x200", "0x0", "1x1,2x2"}); return true;
    case coPoint3: pick({"1x2x3", "0x0x0"}); return true;
    // coEnums is not changed: set_deserialize of z_hop_types ("Spiral Lift,Auto Lift") on a static
    // GCodeConfig crashed the probe (access violation).
    case coEnum: {
        if (def == nullptr || def->enum_values.empty())
            return false;
        const std::vector<std::string> &ev = def->enum_values;
        std::string a = ev[size_t(g.below(ev.size()))];
        if (a == now && ev.size() > 1)
            a = ev[(size_t(g.below(ev.size() - 1)) + 1 + size_t(std::find(ev.begin(), ev.end(), a) - ev.begin())) % ev.size()];
        out = a;
        return true;
    }
    default: return false;
    }
}

// The changes of one LYCD case: distinct keys of the class with a new value each.
std::vector<std::pair<std::string, std::string>> changes(SplitMix &g, const ConfigBase &c, const std::vector<std::string> &keys, size_t count)
{
    std::vector<std::pair<std::string, std::string>> out;
    std::set<std::string>                            seen;
    for (size_t tries = 0; out.size() < count && tries < count * 8; ++tries) {
        const std::string &key = keys[size_t(g.below(keys.size()))];
        if (!seen.insert(key).second)
            continue;
        std::string v;
        if (new_value(g, key, *c.option(key), v))
            out.emplace_back(key, v);
    }
    return out;
}

template<class T> void record_class(Sets &sets, const char *name, SplitMix &g, size_t cases, size_t committed_cases)
{
    const T                  &defaults = T::defaults();
    const ConfigBase         &d        = base(defaults);
    const std::vector<std::string> keys = d.keys();

    // LYCK: the keys in order with each option's type and serialized value, the hash, and the
    // DynamicPrintConfig made from the class.
    {
        Payload p;
        p.str(name).u64(uint64_t(defaults.hash()));
        p.u64(keys.size());
        for (const std::string &k : keys)
            p.str(k).u32(uint32_t(d.option(k)->type())).str(d.opt_serialize(k));
        DynamicPrintConfig dyn(base(defaults));
        const std::vector<std::string> dk = dyn.keys();
        put_strs(p, dk);
        bool same = dk.size() == keys.size();
        for (const std::string &k : dk)
            same = same && d.option(k) != nullptr && *dyn.option(k) == *d.option(k);
        p.boolean(same);
        add(sets, "LYCK", p, true);
    }
    // LYCA: get_abs_value of every numeric option of the defaults, and of the float-or-percent
    // ones over 0.4 and 2.5.
    {
        Payload p;
        p.str(name);
        std::vector<std::string> nk;
        for (const std::string &k : keys)
            if (numeric(d.option(k)->type()))
                nk.push_back(k);
        p.u64(nk.size());
        for (const std::string &k : nk) {
            p.str(k);
            put_abs(p, d, k);
            if (d.option(k)->type() == coFloatOrPercent) {
                p.u8(1);
                p.f64(d.get_abs_value(k, 0.4)).f64(d.get_abs_value(k, 2.5));
            } else
                p.u8(0);
        }
        add(sets, "LYCA", p, true);
    }
    // LYCD: a copy with some options set by set_deserialize, against the defaults.
    for (size_t ci = 0; ci < cases; ++ci) {
        T           copy(defaults);
        ConfigBase &c = base_mut(copy);
        const auto  ch = changes(g, d, keys, size_t(1 + g.below(12)));
        Payload     p;
        p.str(name).u32(uint32_t(ci));
        p.u64(ch.size());
        for (const auto &kv : ch) {
            p.str(kv.first).str(kv.second);
            attempt(p, [&](Payload &q) { c.set_deserialize_strict(kv.first, kv.second); });
            p.str(c.opt_serialize(kv.first));
            if (numeric(c.option(kv.first)->type())) {
                p.u8(1);
                put_abs(p, c, kv.first);
            } else
                p.u8(0);
        }
        p.u64(uint64_t(copy.hash()));
        p.boolean(copy == defaults);
        p.boolean(d.equals(c));
        put_strs(p, d.diff(c));
        p.u64(d.equal(c).size());
        put_strs(p, c.diff(d));
        add(sets, "LYCD", p, ci < committed_cases);
    }
    // LYCO: apply_only from a DynamicPrintConfig holding some of the keys, changed, with keys the
    // source lacks, keys the class lacks, an indexed key and a made-up one.
    for (size_t ci = 0; ci < cases; ++ci) {
        T           copy(defaults);
        ConfigBase &c       = base_mut(copy);
        const bool  ignore  = g.below(2) == 0;
        const auto  ch      = changes(g, d, keys, size_t(1 + g.below(6)));
        std::vector<std::string> list;
        std::vector<std::string> in_source;
        for (const auto &kv : ch) {
            list.push_back(kv.first);
            in_source.push_back(kv.first);
        }
        // A key of the class the source lacks (left as it is).
        list.push_back(keys[size_t(g.below(keys.size()))]);
        // Keys outside the class, an indexed vector key and a made-up key, now and then.
        const std::vector<std::string> &all = FullPrintConfig::defaults().keys();
        if (g.below(2) == 0) {
            const std::string &k = all[size_t(g.below(all.size()))];
            list.push_back(k);
            in_source.push_back(k);
        }
        // The indexed key. A class without nozzle_diameter is not given a source that holds it:
        // apply_only would then dereference the null option its static config returns.
        if (g.below(3) == 0) {
            list.push_back("nozzle_diameter#0");
            if (d.option("nozzle_diameter") != nullptr)
                in_source.push_back("nozzle_diameter");
            else
                in_source.erase(std::remove(in_source.begin(), in_source.end(), std::string("nozzle_diameter")), in_source.end());
        }
        if (g.below(4) == 0)
            list.push_back("probe_no_such_key");
        // The source holds each key at FullPrintConfig's default, or at the class's own where
        // FullPrintConfig lacks it (the SLA keys, which new_from_defaults_keys would refuse).
        std::unique_ptr<DynamicPrintConfig> src(new DynamicPrintConfig());
        for (const std::string &k : in_source) {
            const ConfigOption *o = FullPrintConfig::defaults().option(k);
            if (o == nullptr)
                o = d.option(k);
            if (o != nullptr)
                src->set_key_value(k, o->clone());
        }
        for (const auto &kv : ch)
            try {
                src->set_deserialize_strict(kv.first, kv.second);
            } catch (const std::exception &) {
            }
        if (src->option("nozzle_diameter") != nullptr)
            src->set_deserialize_strict("nozzle_diameter", "0.6,0.8");
        Payload p;
        p.str(name).u32(uint32_t(ci)).boolean(ignore);
        put_strs(p, list);
        const std::vector<std::string> sk = src->keys();
        p.u64(sk.size());
        for (const std::string &k : sk)
            p.str(k).str(src->opt_serialize(k));
        attempt(p, [&](Payload &q) { c.apply_only(*src, list, ignore); });
        p.u64(list.size());
        for (const std::string &k : list) {
            const ConfigOption *o = c.option(k);
            p.str(k).boolean(o != nullptr).str(o != nullptr ? o->serialize() : std::string());
        }
        p.u64(uint64_t(copy.hash()));
        add(sets, "LYCO", p, ci < committed_cases);
    }
}

void record_classes(Sets &sets)
{
    SplitMix g(0x1A7E5C0Full);
    record_class<PrintObjectConfig>(sets, "PrintObjectConfig", g, 40, 4);
    record_class<PrintRegionConfig>(sets, "PrintRegionConfig", g, 40, 4);
    record_class<MachineEnvelopeConfig>(sets, "MachineEnvelopeConfig", g, 20, 2);
    record_class<GCodeConfig>(sets, "GCodeConfig", g, 40, 4);
    record_class<PrintConfig>(sets, "PrintConfig", g, 40, 4);
    record_class<FullPrintConfig>(sets, "FullPrintConfig", g, 60, 6);
    record_class<SLAPrintConfig>(sets, "SLAPrintConfig", g, 4, 1);
    record_class<SLAPrintObjectConfig>(sets, "SLAPrintObjectConfig", g, 4, 1);
    record_class<SLAMaterialConfig>(sets, "SLAMaterialConfig", g, 4, 1);
    record_class<SLAPrinterConfig>(sets, "SLAPrinterConfig", g, 4, 1);
    record_class<SLAFullPrintConfig>(sets, "SLAFullPrintConfig", g, 4, 1);
}

// LYDY: full_print_config() and new_from_defaults_keys().
void record_dynamic(Sets &sets)
{
    {
        const DynamicPrintConfig  full = DynamicPrintConfig::full_print_config();
        const ConfigBase         &d    = base(FullPrintConfig::defaults());
        Payload                   p;
        p.u8(0);
        const std::vector<std::string> keys = full.keys();
        put_strs(p, keys);
        bool same = keys.size() == d.keys().size();
        for (const std::string &k : keys)
            same = same && d.option(k) != nullptr && *full.option(k) == *d.option(k);
        p.boolean(same);
        add(sets, "LYDY", p, true);
    }
    const std::vector<std::vector<std::string>> lists = {
        {},
        {"layer_height", "wall_loops"},
        {"nozzle_diameter", "line_width", "initial_layer_print_height", "sparse_infill_pattern"},
        {"filament_type", "printable_area", "machine_start_gcode"},
        {"wall_loops", "probe_no_such_key"},
        {"wall_loops", "wall_loops"},
    };
    for (const auto &list : lists) {
        Payload p;
        p.u8(1);
        put_strs(p, list);
        attempt(p, [&](Payload &q) {
            std::unique_ptr<DynamicPrintConfig> c(DynamicPrintConfig::new_from_defaults_keys(list));
            const std::vector<std::string>      keys = c->keys();
            q.u64(keys.size());
            for (const std::string &k : keys)
                q.str(k).str(c->opt_serialize(k));
        });
        add(sets, "LYDY", p, true);
    }
}

// LYAM: get_extruder_ams_count of lists of texts.
void record_ams(Sets &sets)
{
    const std::vector<std::vector<std::string>> inputs = {
        {}, {""}, {"1#2"}, {"1#2|3#4"}, {"0#1|0#5"}, {"", "2#3"}, {"4#1|1#4", "7#0"}, {"a#1"}, {"1#b"}, {"-1#2|10#20|3#3"},
    };
    for (const auto &in : inputs) {
        Payload p;
        put_strs(p, in);
        attempt(p, [&](Payload &q) {
            const std::vector<std::map<int, int>> out = get_extruder_ams_count(in);
            q.u64(out.size());
            for (const auto &m : out) {
                q.u64(m.size());
                for (const auto &kv : m)
                    q.i32(kv.first).i32(kv.second);
            }
        });
        add(sets, "LYAM", p, true);
    }
}

// LYFO: compute_filament_override_value of a machine option and a filament override.
void record_override(Sets &sets)
{
    struct Case
    {
        std::string      key;
        std::string      old_machine, new_machine, filament;
        int              enable;
        std::vector<int> maps;
    };
    const std::vector<Case> cases = {
        {"retraction_length", "0.8", "0.8", "nil", 0, {1}},
        {"retraction_length", "0.8", "0.8", "1.2", 0, {1}},
        {"retraction_length", "0.8", "1", "nil", 0, {1}},
        {"retraction_length", "0.8,0.8", "0.8,0.6", "nil,0.5", 0, {1, 2}},
        {"retraction_length", "0.8,0.6", "0.8,0.6", "nil,nil", 0, {2, 1}},
        {"retraction_length", "0.8", "0.8", "nil,0.4,nil", 0, {1, 1, 1}},
        {"retraction_length", "0.8", "0.8", "nil,nil", 0, {0, 3}},
        {"z_hop", "0.4", "0.4", "0.2", 0, {1}},
        {"long_retractions_when_cut", "0", "0", "1", 2, {1}},
        {"long_retractions_when_cut", "0", "0", "1", 1, {1}},
        {"long_retractions_when_cut", "0", "1", "nil", 0, {1}},
        {"retraction_distances_when_cut", "18", "18", "10", 2, {1}},
        {"retraction_distances_when_cut", "18", "20", "10", 0, {1}},
        {"retraction_distances_when_cut", "18", "18", "nil", 2, {1}},
    };
    for (const Case &c : cases) {
        const bool bools = c.key == "long_retractions_when_cut";
        Payload    p;
        p.str(c.key).str(c.old_machine).str(c.new_machine).str(c.filament).i32(c.enable).i32s(std::vector<int32_t>(c.maps.begin(), c.maps.end()));
        attempt(p, [&](Payload &q) {
            std::unique_ptr<ConfigOption> om, nm, nf;
            if (bools) {
                om.reset(new ConfigOptionBools());
                nm.reset(new ConfigOptionBools());
                nf.reset(new ConfigOptionBoolsNullable());
            } else {
                om.reset(new ConfigOptionFloats());
                nm.reset(new ConfigOptionFloats());
                nf.reset(new ConfigOptionFloatsNullable());
            }
            om->deserialize(c.old_machine);
            nm->deserialize(c.new_machine);
            nf->deserialize(c.filament);
            DynamicPrintConfig full;
            full.set_key_value("enable_long_retraction_when_cut", new ConfigOptionInt(c.enable));
            t_config_option_keys diff;
            DynamicPrintConfig   overrides;
            std::vector<int>     maps = c.maps;
            compute_filament_override_value(c.key, om.get(), nm.get(), nf.get(), full, diff, overrides, maps);
            put_strs(q, diff);
            const std::vector<std::string> ok = overrides.keys();
            q.u64(ok.size());
            for (const std::string &k : ok)
                q.str(k).str(overrides.opt_serialize(k));
        });
        add(sets, "LYFO", p, true);
    }
}

// LYSK: has_skirt of configurations holding some of skirt_height, skirt_loops and draft_shield.
void record_skirt(Sets &sets)
{
    const std::vector<std::vector<std::pair<std::string, std::string>>> inputs = {
        {},
        {{"skirt_height", "1"}, {"skirt_loops", "1"}},
        {{"skirt_height", "0"}, {"skirt_loops", "2"}},
        {{"skirt_height", "3"}, {"skirt_loops", "0"}},
        {{"skirt_height", "0"}, {"skirt_loops", "0"}, {"draft_shield", "enabled"}},
        {{"skirt_height", "0"}, {"skirt_loops", "0"}, {"draft_shield", "disabled"}},
        {{"skirt_loops", "2"}},
        {{"skirt_height", "2"}},
        {{"draft_shield", "enabled"}},
        {{"skirt_height", "-1"}, {"skirt_loops", "4"}},
    };
    for (const auto &in : inputs) {
        Payload p;
        p.u64(in.size());
        for (const auto &kv : in)
            p.str(kv.first).str(kv.second);
        attempt(p, [&](Payload &q) {
            DynamicPrintConfig cfg;
            for (const auto &kv : in)
                cfg.set_deserialize_strict(kv.first, kv.second);
            q.boolean(has_skirt(cfg));
        });
        add(sets, "LYSK", p, true);
    }
}

// LYMT: MaterialType's table and its lookups.
void record_material(Sets &sets)
{
    Payload p;
    const auto &all = MaterialType::all();
    p.u64(all.size());
    for (const MaterialTypeInfo &m : all)
        p.str(m.name).i32(m.min_temp).i32(m.max_temp).i32(m.chamber_min_temp).i32(m.chamber_max_temp).f64(m.adhesion_coefficient).f64(m.yield_strength).f64(m.thermal_length);
    std::vector<std::string> names;
    for (const MaterialTypeInfo &m : all)
        names.push_back(m.name);
    for (const char *n : {"pla", "PLA ", "", "Generic", "PLA+", "TPU-95A", "petg"})
        names.push_back(n);
    p.u64(names.size());
    for (const std::string &n : names) {
        p.str(n);
        const MaterialTypeInfo *f = MaterialType::find(n);
        p.boolean(f != nullptr);
        // Each out value starts at 0 (-1 for x), so a lookup that misses shows what it writes.
        int    a = 0, b = 0;
        double x = -1;
        p.boolean(MaterialType::get_temperature_range(n, a, b)).i32(a).i32(b);
        a = b = 0;
        p.boolean(MaterialType::get_chamber_temperature_range(n, a, b)).i32(a).i32(b);
        x = -1;
        p.boolean(MaterialType::get_adhesion_coefficient(n, x)).f64(x);
        x = -1;
        p.boolean(MaterialType::get_yield_strength(n, x)).f64(x);
        x = -1;
        p.boolean(MaterialType::get_thermal_length(n, x)).f64(x);
    }
    add(sets, "LYMT", p, true);
}

} // namespace

void record_config(Sets &sets)
{
    record_classes(sets);
    record_dynamic(sets);
    record_ams(sets);
    record_override(sets);
    record_skirt(sets);
    record_material(sets);
}

} // namespace layer
} // namespace probe
