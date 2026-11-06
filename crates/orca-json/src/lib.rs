// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! JSON with nlohmann::json's semantics: parsing, ordering, number handling and dump.
//!
//! JSON as the C++ front end's `nlohmann::json` does it: how numbers are read and kept (integers,
//! unsigned, floating point), the ordering of object keys, and `dump` with its indent and its error
//! handlers, so a result file is the same bytes.

/// The crate's name, as Cargo has it.
pub const NAME: &str = "orca-json";

#[cfg(test)]
mod tests {
    use super::NAME;

    #[test]
    fn name_is_the_package_name() {
        assert_eq!(NAME, env!("CARGO_PKG_NAME"));
    }
}
