// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! A scaffold: the crate holds only its name until task O1.8a of the translation's plan fills it.
//!
//! It is to read the SOP1 files (`parity/SOP1.md`) that the probe program
//! (`parity/oracle/probe/`) writes, so the foundation crates can be held to the C++ engine's
//! recorded behaviour, on the committed probe subset and on the full set.

/// The crate's name, as Cargo has it.
pub const NAME: &str = "orca-probe";

#[cfg(test)]
mod tests {
    use super::NAME;

    #[test]
    fn name_is_the_package_name() {
        assert_eq!(NAME, env!("CARGO_PKG_NAME"));
    }
}
