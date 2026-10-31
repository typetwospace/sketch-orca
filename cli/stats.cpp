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
// sketch-orca: the "stats" of a slice result, from Orca's G-code processor.
#include "stats.hpp"

#include <array>
#include <cmath>
#include <numeric>
#include <cstdlib>
#include <string>
#include <vector>

#include "libslic3r/ExtrusionEntity.hpp"
#include "libslic3r/libslic3r.h"

namespace sketch_orca {

using namespace Slic3r;

namespace {

double non_negative(double v) { return std::isfinite(v) && v > 0. ? v : 0.; }

} // namespace

json compute_stats(const GCodeProcessorResult &result, const PrintConfig &config, size_t print_z_count, bool by_object)
{
    const size_t normal = size_t(PrintEstimatedStatistics::ETimeMode::Normal);

    // Time per layer and per feature, from every move the processor timed.
    std::vector<double>                    layer_seconds;
    std::array<double, size_t(erCount)>    role_seconds{};
    std::array<double, size_t(erCount)>    role_mm{};
    std::array<bool, size_t(erCount)>      role_seen{};
    double                                 travel_seconds = 0.;
    double                                 travel_mm      = 0.;
    bool                                   travel_seen    = false;
    for (const GCodeProcessorResult::MoveVertex &move : result.moves) {
        const double t = non_negative(move.time[normal]);
        if (move.layer_id >= layer_seconds.size())
            layer_seconds.resize(size_t(move.layer_id) + 1, 0.);
        layer_seconds[move.layer_id] += t;
        // GCodeProcessor::calculate_time adds copies of a move at the points where its speed
        // changes, for the preview, with no time and the whole move's travel_dist; only the move
        // itself counts. (A G2/G3 arc's segments are moves of their own, with their own time.)
        if (move.internal_only && move.time[normal] == 0.f)
            continue;
        if (move.type == EMoveType::Extrude) {
            const size_t role = size_t(move.extrusion_role);
            if (role < size_t(erCount)) {
                role_seconds[role] += t;
                role_mm[role] += non_negative(move.travel_dist);
                role_seen[role] = true;
            }
        } else if (move.type == EMoveType::Travel) {
            travel_seconds += t;
            travel_mm += non_negative(move.travel_dist);
            travel_seen = true;
        }
    }

    const double total = non_negative(result.print_statistics.modes[normal].time);
    if (!by_object && print_z_count > 0) {
        if (layer_seconds.size() > print_z_count) {
            for (size_t i = print_z_count; i < layer_seconds.size(); ++i)
                layer_seconds[print_z_count - 1] += layer_seconds[i];
            layer_seconds.resize(print_z_count);
        } else {
            layer_seconds.resize(print_z_count, 0.);
        }
    }
    if (const double counted = std::accumulate(layer_seconds.begin(), layer_seconds.end(), 0.); !layer_seconds.empty() && total > counted)
        layer_seconds.front() += total - counted;

    json stats;
    stats["timeSeconds"] = round_um(total);
    json layers = json::array();
    for (double s : layer_seconds)
        layers.push_back(round_um(s));
    stats["layerSeconds"] = std::move(layers);

    // Filament per extruder that printed (the processor's ids are 0-based filament indices).
    json filament = json::array();
    for (const auto &entry : result.print_statistics.total_volumes_per_extruder) {
        const size_t id      = entry.first;
        const double volume  = non_negative(entry.second);
        const double d       = config.filament_diameter.values.empty() ? 1.75 : config.filament_diameter.get_at(id);
        const double density = config.filament_density.values.empty() ? 0. : config.filament_density.get_at(id);
        const double cost    = config.filament_cost.values.empty() ? 0. : config.filament_cost.get_at(id);
        const double area    = PI * d * d / 4.;
        const double grams   = volume * non_negative(density) * 0.001;
        json f;
        f["extruder"] = id + 1;
        f["mm"]       = round_um(area > 0. ? volume / area : 0.);
        f["cm3"]      = round_um(volume / 1000.);
        f["g"]        = round_um(grams);
        f["cost"]     = round_um(grams * non_negative(cost) * 0.001);
        filament.push_back(std::move(f));
    }
    stats["filament"] = std::move(filament);

    // Per feature, under Orca's own names ("Outer wall", "Sparse infill", ...), in Orca's order.
    json features = json::object();
    for (size_t role = 0; role < size_t(erCount); ++role) {
        if (!role_seen[role])
            continue;
        json f;
        f["seconds"] = round_um(role_seconds[role]);
        f["mm"]      = round_um(role_mm[role]);
        features[ExtrusionEntity::role_to_string(ExtrusionRole(role))] = std::move(f);
    }
    if (travel_seen) {
        json f;
        f["seconds"] = round_um(travel_seconds);
        f["mm"]      = round_um(travel_mm);
        features["Travel"] = std::move(f);
    }
    stats["features"] = std::move(features);
    return stats;
}

void add_gcode_warnings(const GCodeProcessorResult &result, Warnings &warnings)
{
    for (const GCodeProcessorResult::SliceWarning &w : result.warnings) {
        std::string message = w.msg;
        if (!w.error_code.empty())
            message += " (" + w.error_code + ")";
        if (!w.params.empty()) {
            // The processor names filaments by 0-based index; the result counts from 1.
            std::vector<std::string> names;
            for (const std::string &p : w.params) {
                char *end = nullptr;
                const long n = std::strtol(p.c_str(), &end, 10);
                names.push_back(!p.empty() && end != nullptr && *end == 0 ? std::to_string(n + 1) : p);
            }
            message += ": filaments " + join(names, ", ");
        }
        warnings.add("gcode-warning", message);
    }
}

} // namespace sketch_orca
