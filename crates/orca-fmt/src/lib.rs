// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Number formatting and parsing with the bytes the C++ engine's runtime gives: MSVC 14.42's
//! standard library and the static UCRT, in the "C" locale the engine runs in.
//!
//! - [`printf`]: `sprintf`, `snprintf` and `sprintf_s`, written from the C standard with the
//!   UCRT's choices, compared byte for byte with the UCRT in `tests/fmt.rs`.
//! - [`to_chars`]: microsoft/STL's `std::to_chars` (Ryu for the shortest form, Ryu printf for the
//!   precision forms).
//! - [`ostream`]: microsoft/STL's `std::ostream << double` (`num_put`), and
//!   `std::to_string(double)`.
//! - [`parse`]: microsoft/STL's `std::from_chars`; `strtod`, `strtof`, `strtol` and their family,
//!   written from the C standard around it with the UCRT's choices; and the STL's `std::stod`,
//!   `stof`, `stoi` and the rest.
//!
//! The modules that carry the STL's code say what they carry, and from where, in their `Carries`
//! blocks.
//!
//! Engine crates format and parse numbers only through this crate: Rust's `format!("{}")` and
//! `str::parse` are not the C++ runtime's.

mod charconv_tables;
pub mod ostream;
pub mod parse;
pub mod printf;
pub mod to_chars;

/// The `std::errc` values that `to_chars` and `from_chars` report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Errc {
    /// `errc::invalid_argument`: no number at the start of the input.
    InvalidArgument,
    /// `errc::result_out_of_range`: the number does not fit the type.
    ResultOutOfRange,
    /// `errc::value_too_large`: the output buffer is too short.
    ValueTooLarge,
}

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
