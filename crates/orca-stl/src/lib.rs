// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The standard library algorithms and containers of the C++ engine, with their exact order of operations.
//!
//! The algorithms and containers of the C++ standard library as Microsoft's implementation runs
//! them: `sort`, `stable_sort`, `nth_element`, `partial_sort`, the heap functions, `min` and `max`
//! with their NaN behaviour, the unordered containers in MSVC's iteration order, and `mt19937` with
//! its distributions.
//!
//! The order of the comparator calls and the order of the iteration are part of the result of a
//! slice, so they are reproduced, not approximated.

/// The crate's name, as Cargo has it.
pub const NAME: &str = "orca-stl";

#[cfg(test)]
mod tests {
    use super::NAME;

    #[test]
    fn name_is_the_package_name() {
        assert_eq!(NAME, env!("CARGO_PKG_NAME"));
    }
}
