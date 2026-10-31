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
// sketch-orca: its three commands, as protocol/README.md specifies them.
#pragma once

#include <string>

#include "common.hpp"

namespace sketch_orca {

// `sketch-orca version`: engine, engineVersion, orcaVersion, orcaCommit, orcaTag.
json version_json();

// `sketch-orca config-def`: every print, filament and printer setting (config_def.cpp).
json config_def_json();

// `sketch-orca run`: reads the job, does it, writes the result (always, also on failure) and returns
// the exit code: 0 when the result is ok, else the stock Orca command line's code for the failure.
// `progress_path` may be empty.
int run_job(const std::string &job_path, const std::string &out_path, const std::string &progress_path);

// Writes a failure result for a run that could not start (a bad command line). Returns false when
// the result cannot be written.
bool write_failure_result(const std::string &out_path, const std::string &code, const std::string &message, int exit_code);

} // namespace sketch_orca
