// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The static UCRT's `FILE` streams as the engine's model readers use them, written from the C
//! standard with the UCRT's choices: `fopen` in binary mode (through `boost::nowide::fopen`),
//! `fread`, `fgets`, `getc`, `ftell`, `fseek`, `rewind`, `feof` and `fclose`, as admesh's STL
//! reader (`deps_src/admesh/stlinit.cpp`) and the OBJ reader (`src/libslic3r/Format/objparser.cpp`)
//! call them. [`crate::scanf`]'s `fscanf` reads through these streams.
//!
//! The o2-mesh task fills this module, with its records in the `cstdio.sop1` probe file
//! (`parity/SOP1.md`, "Probe files"); until then it is empty.
