// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The C runtime the C++ engine links, called as it is: maths, float-to-integer conversions, rand.
//!
//! The home of every call into the static UCRT that the C++ engine makes: one `#[inline(never)]`
//! wrapper per maths function (`sin`, `cos`, `pow`, `exp`, and the rest, for `double` and for
//! `float`), MSVC's float-to-integer conversions, and `rand` and `srand` with the UCRT's per-thread
//! state.
//!
//! The crate is `#![no_builtins]`, so the compiler cannot replace a call with its own
//! implementation or fold it into a constant: the answer is the one the C++ program gets from the
//! same `libucrt.lib`. The maths itself is never translated, because the UCRT's source is not open;
//! the wrappers call into the library the Rust program links statically (`.cargo/config.toml`).

#![no_builtins]

/// The crate's name, as Cargo has it.
pub const NAME: &str = "orca-crt";

#[cfg(test)]
mod tests {
    use super::NAME;

    #[test]
    fn name_is_the_package_name() {
        assert_eq!(NAME, env!("CARGO_PKG_NAME"));
    }
}
