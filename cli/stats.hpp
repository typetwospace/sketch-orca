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
// sketch-orca: the estimates of a sliced print, read from Orca's G-code processor (the numbers Orca's
// own preview shows): total and per-layer time, filament per extruder, and time and length per
// feature.
#pragma once

#include "libslic3r/GCode/GCodeProcessor.hpp"
#include "libslic3r/PrintConfig.hpp"

#include "common.hpp"

namespace sketch_orca {

// The result's "stats" object. Times are Orca's normal-mode estimate in seconds, and the entries of
// `layerSeconds` sum to `timeSeconds`. For a print by layer `layerSeconds` has one entry per print Z
// (`print_z_count`, the result's printZ): entry 0 also holds the start G-code and the time Orca
// counts outside any move (a dwell before the first move), the last entry the end G-code and any
// G-code layer past the last print Z (the finishing loop of a spiral vase). For a print by object
// it follows the G-code's layer changes, each object's layers in turn, with the same entry 0.
// Filament is per extruder that printed (1-based), from the volume the processor counted (model,
// support, prime tower and flushes), with the filament's diameter, density and cost per kg.
// `features` gives each feature's time and path length in mm, under Orca's own names.
json compute_stats(const Slic3r::GCodeProcessorResult &result, const Slic3r::PrintConfig &config, size_t print_z_count, bool by_object);

// The processor's own slice warnings, added to `warnings` as "gcode-warning".
void add_gcode_warnings(const Slic3r::GCodeProcessorResult &result, Warnings &warnings);

} // namespace sketch_orca
