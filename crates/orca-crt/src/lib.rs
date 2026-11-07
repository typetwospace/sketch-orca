// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The C runtime the C++ engine links, called as it is: maths, float-to-integer conversions, rand,
//! qsort.
//!
//! The home of every call into the static UCRT that the C++ engine makes:
//!
//! - [`math`]: one `#[inline(never)]` wrapper per maths function (`sin`, `cos`, `pow`, `exp`, and
//!   the rest, for `double` and for `float`), re-exported here, so engine code calls
//!   `orca_crt::sin(x)` where the C++ calls `std::sin(x)`. `sqrt`, `fabs` and `copysignf` stay the
//!   IEEE operations MSVC compiles them to.
//! - [`convert`]: MSVC 14.42's float-to-integer conversions for every integer width, with
//!   `cvttsd2si`'s indefinite value for NaN and out-of-range input and the unsigned sequences.
//! - [`rand`]: `rand` and `srand`, the UCRT's generator with its per-thread state.
//! - [`search`]: `qsort`, the UCRT's own function called on the elements in place, for the C code
//!   that sorts with it (Qhull), whose comparators can tie or contradict themselves.
//!
//! The crate is `#![no_builtins]`, so the compiler cannot replace a call with its own
//! implementation or fold it into a constant: the answer is the one the C++ program gets from the
//! same `libucrt.lib`. The maths itself is never translated, because the UCRT's source is not open;
//! the wrappers call into the library the Rust program links statically (`.cargo/config.toml`).
//! The conversions and the generator are written here from what MSVC's code and the UCRT do,
//! and `tests/crt.rs` holds them to it.

#![no_builtins]

pub mod convert;
pub mod math;
pub mod rand;
pub mod search;

pub use convert::{
    f32_to_i8, f32_to_i16, f32_to_i32, f32_to_i64, f32_to_u8, f32_to_u16, f32_to_u32, f32_to_u64,
    f64_to_i8, f64_to_i16, f64_to_i32, f64_to_i64, f64_to_u8, f64_to_u16, f64_to_u32, f64_to_u64,
};
pub use math::*;
pub use rand::{RAND_MAX, rand, srand};
pub use search::qsort;

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
