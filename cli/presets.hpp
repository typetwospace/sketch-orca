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
// sketch-orca: the machine, process and filament presets of a job, merged into one print
// configuration the way Orca's own command line merges them (OrcaSlicer.cpp, --load-settings and
// --load-filaments), plus the job's overrides.
#pragma once

#include <string>
#include <vector>

#include "libslic3r/PrintConfig.hpp"

#include "common.hpp"
#include "job.hpp"

namespace sketch_orca {

struct LoadedPreset
{
    std::string                path;
    // machine, process or filament
    std::string                type;
    std::string                name;
    // system, User or user
    std::string                from;
    // The system preset a user preset is based on.
    std::string                inherits;
    std::string                filament_id;
    Slic3r::DynamicPrintConfig config;

    // The name Orca's command line matches compatible_printers against.
    std::string system_name() const { return from == "system" ? name : inherits; }
};

struct Settings
{
    // The whole print configuration, every option present.
    Slic3r::DynamicPrintConfig config;
    LoadedPreset               machine;
    LoadedPreset               process;
    std::vector<LoadedPreset>  filaments;
    size_t                     extruder_count = 1;
    // The printer's model id from its vendor bundle ("BL-P001" for the X1 Carbon), empty when the
    // vendor has none.
    std::string                printer_model_id;
    bool                       is_bbl = false;
    // Keys the job's overrides set, after Orca's renames.
    std::vector<std::string>   overridden;
};

// Reads the presets the job names, merges them, applies the job's overrides and Orca's defaults,
// and checks the result. Throws Failure: file-not-found, preset-invalid, not-fdm,
// process-not-compatible, setting-invalid, config-invalid.
Settings load_settings(const Job &job, Warnings &warnings);

// Parses one setting into `into` (after Orca's renames of old keys) and returns the key it was
// stored under. Throws Failure("setting-invalid") naming `field` for an unknown key or a bad value.
std::string parse_setting(Slic3r::DynamicPrintConfig &into, const std::string &key, const std::string &value, const std::string &field);

} // namespace sketch_orca
