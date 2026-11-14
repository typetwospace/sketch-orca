// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Phase O3 (process() and the layers answer for domain A): libslic3r's support steps as a print
//! with supports off reaches them: the support check, overhang detection and the curled extrusions.
//!
//! Set up empty by orca-o3-skeleton (2026-11-13). The pieces of the orca-o3 wave fill it: support-1
//! (the support check, overhang detection and curled extrusions (supports off)). Each piece names
//! here what it adds and the C++ it carries. README.md lists the C++ files and how the crate is
//! proven.
