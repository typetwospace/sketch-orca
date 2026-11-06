// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! printf, to_chars, iostream double output and strtod, stod and from_chars parsing, as the C++ engine gets them.
//!
//! Number formatting and parsing that gives the digits the C++ engine's runtime gives: `printf`
//! written from the C standard, the standard library's shortest round-trip `to_chars` (Ryu), the
//! default and fixed `iostream` double output, and `strtod`, `stod` and `from_chars` parsing.
//!
//! Each function is tested against the C++ oracle through the probes (`orca-probe`) and against the
//! UCRT itself.

/// The crate's name, as Cargo has it.
pub const NAME: &str = "orca-fmt";

#[cfg(test)]
mod tests {
    use super::NAME;

    #[test]
    fn name_is_the_package_name() {
        assert_eq!(NAME, env!("CARGO_PKG_NAME"));
    }
}
