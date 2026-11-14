// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Phase O3 (process() and the layers answer for domain A): libslic3r's plate-wide steps:
//! ToolOrdering for one filament, the wipe tower's data, the simplify steps, skirt and brim, the
//! conflict checker, the fake wipe tower, the layers to print, and the oracle's print dump writer.
//!
//! Set up empty by orca-o3-skeleton (2026-11-13). The pieces of the orca-o3 wave fill it: plate-1
//! (ToolOrdering for one filament, the wipe tower's data and the simplify steps), plate-2 (skirt
//! and brim), plate-3 (the conflict checker, the fake wipe tower and the layers to print), dump-2
//! (the oracle's print dump writer). Each piece names here what it adds and the C++ it carries.
//! README.md lists the C++ files and how the crate is proven.
