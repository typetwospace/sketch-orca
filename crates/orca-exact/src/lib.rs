// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Exact arithmetic as Boost 1.84.0 has it: multiprecision's signed-magnitude int128_t and rational.
//!
//! Exact integer and rational arithmetic as the C++ engine gets it from Boost 1.84.0:
//! Boost.Multiprecision's `int128_t` (a signed-magnitude `cpp_int` of 128 bits, which wraps on
//! overflow otherwise than Rust's `i128`) and `boost::rational`, which libnest2d and `Arrange.cpp`
//! compute with (`<work>/deps/OrcaSlicer_dep/usr/local/include/boost-1_84`).
//!
//! The o2-arrange task of phase O2 fills this crate; its records go into the probe file
//! `arrange.sop1` beside those of `orca-arrange`, and `tests/probes.rs` replays them.
//!
//! Until that task runs, the crate holds only its name, and `tests/probes.rs` passes trivially
//! while its probe file is a stub (HEAD and PRUN only).

/// The crate's name, as Cargo has it.
pub const NAME: &str = "orca-exact";

#[cfg(test)]
mod tests {
    use super::NAME;

    #[test]
    fn name_is_the_package_name() {
        assert_eq!(NAME, env!("CARGO_PKG_NAME"));
    }
}
