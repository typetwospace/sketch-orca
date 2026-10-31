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
// sketch-orca: the job file (`sketch-orca-job/1`), as protocol/README.md and
// protocol/schema/job.schema.json specify it. The checks are the same as the TypeScript side's
// (scripts/lib/job.ts), so a job either side
// refuses, the other refuses too, naming the same field.
#pragma once

#include <string>
#include <utility>
#include <vector>

namespace sketch_orca {

enum class Command { Slice, Layers, Info };

// Setting key -> value as Orca writes it, in the order the job gives them.
using Overrides = std::vector<std::pair<std::string, std::string>>;

struct LayerHeights
{
    enum class Mode { Fixed, Adaptive, Profile };
    Mode                mode    = Mode::Fixed;
    // Adaptive: 0 is the finest, 1 the coarsest (Orca's adaptive layer height quality).
    double              quality = 0.5;
    // Profile: z0, h0, z1, h1, ... with z measured from the bottom of the object.
    std::vector<double> profile;
};

struct HeightRange
{
    // From the bottom of the object, in mm.
    double    from = 0.;
    double    to   = 0.;
    Overrides overrides;
};

struct JobObject
{
    int                      index = 0;
    Overrides                overrides;
    LayerHeights             layer_heights;
    std::vector<HeightRange> height_ranges;
};

struct CustomGcode
{
    // The print Z of the layer this runs before.
    double      print_z = 0.;
    // pause, color, filament or custom.
    std::string type;
    // 1-based; 0 when not given.
    int         extruder = 0;
    // #rrggbb, lower case; empty when not given.
    std::string color;
    std::string gcode;
    std::string note;
};

struct Job
{
    Command                  command = Command::Slice;
    std::string              command_name;
    std::string              resources;
    std::string              work_dir;
    std::string              model_path;
    std::string              machine;
    std::string              process;
    std::vector<std::string> filaments;
    Overrides                overrides;
    std::vector<JobObject>   objects;
    // Sorted by print Z.
    std::vector<CustomGcode> custom_gcodes;
    bool                     arrange = true;
    bool                     orient  = false;
    std::string              out_gcode;
    // Empty when no .gcode.3mf is asked for.
    std::string              out_bambu3mf;
    bool                     stats = true;
};

// Parses and checks a job; throws Failure("job-invalid") naming the field, or the contract's own
// code: custom-gcode-invalid for a printZ at or below 0 or two at one printZ, layer-profile-invalid
// (with the object's index) for a profile that does not start at z 0. An optional field given as
// null is the same as one left out.
Job parse_job(const std::string &text);

} // namespace sketch_orca
