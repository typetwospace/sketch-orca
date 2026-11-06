// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! oneTBB 2021.5.0's concurrency and partitioners as fixed split trees, run on rayon.
//!
//! oneTBB 2021.5.0's `max_concurrency` and its partitioners, kept as fixed split trees and join
//! orders, and run on rayon 1.12.0 (Windows: `windows-sys` for the processor count, with processor
//! groups). The split tree depends on the concurrency P and on nothing else, so a result never
//! depends on how threads are scheduled.

/// The crate's name, as Cargo has it.
pub const NAME: &str = "orca-tbb";

#[cfg(test)]
mod tests {
    use super::NAME;

    #[test]
    fn name_is_the_package_name() {
        assert_eq!(NAME, env!("CARGO_PKG_NAME"));
    }
}
