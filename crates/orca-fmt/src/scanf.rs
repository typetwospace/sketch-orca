// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The static UCRT's `fscanf` and `sscanf`, written from the C standard with the UCRT's choices,
//! for the formats the engine's model readers pass: admesh's ASCII STL reader
//! (`deps_src/admesh/stlinit.cpp`: `" solid %[^\n]"`, `" facet normal %31s %31s %31s"`,
//! `" vertex %f %f %f"`, `"%*[^\n]"`, `"%s %s %s"` and `"%f"`).
//!
//! The o2-mesh task fills this module, with its records in the `cstdio.sop1` probe file
//! (`parity/SOP1.md`, "Probe files"); until then it is empty. What a conversion reads, where it
//! stops, what a failed match leaves in its argument and what the call returns are the UCRT's
//! (`parity/HAZARDS.md`, "Formatting and parsing").
