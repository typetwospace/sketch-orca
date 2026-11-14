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
// Carries no OrcaSlicer code since 2026-11-13 (orca-o3-front-1): the translation that was here
// (DynamicPrintConfig and what cli/presets.cpp calls on one) moved to orca-config's
// src/config.rs, unchanged, with its tests.

//! Orca's configuration as the presets stage handles it: orca-config's
//! [`Config`](orca_config::config::Config) and its functions, re-exported under their old path
//! (`crate::config`) so the rest of orca-cli reads them as before.

pub use orca_config::config::*;
