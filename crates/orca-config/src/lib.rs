// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Orca configuration: the definition of every setting, legacy keys and serialisation.
//!
//! Orca's configuration: `ConfigDef` (generated from the `config-def` probe, not typed in by hand),
//! the legacy and obsolete keys Orca renames or drops when it reads a configuration, and the
//! serialisation of a configuration.

/// The crate's name, as Cargo has it.
pub const NAME: &str = "orca-config";

#[cfg(test)]
mod tests {
    use super::NAME;

    #[test]
    fn name_is_the_package_name() {
        assert_eq!(NAME, env!("CARGO_PKG_NAME"));
    }
}
