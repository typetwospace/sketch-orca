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
//! - [`istream`]: microsoft/STL's `std::istringstream >> double`, `>> float`, `>> int` and
//!   `>> unsigned long long` (`num_get`), which Orca's configuration options read their values
//!   with, and Boost's `stream_translator` the 3MF reader's numbers; replayed against the runtime
//!   by `ptree.sop1`'s PTGT records (`orca-xml`'s `tests/ptree.rs`).
//! - [`scanf`]: the UCRT's `fscanf` and `sscanf` for the formats admesh's ASCII STL reader passes,
//!   written from the C standard and black-box tests of the UCRT (`tests/scanf.rs`; o2-mesh,
//!   2026-11-07).
//! - [`stdio`]: the UCRT's `FILE` streams (`fopen`, `fread`, `fgets`, `getc`, `fseek` and the
//!   rest) as the model readers use them, with the text mode's line ends, and C's `stdout`.
//!
//! The modules that carry the STL's code say what they carry, and from where, in their `Carries`
//! blocks.
//!
//! Engine crates format and parse numbers only through this crate: Rust's `format!("{}")` and
//! `str::parse` are not the C++ runtime's.

mod charconv_tables;
pub mod istream;
pub mod ostream;
pub mod parse;
pub mod printf;
pub mod scanf;
pub mod stdio;
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

/// `(std::max)(a, b)` as MSVC defines it, `a < b ? b : a`: the first of equals. This crate has no
/// dependencies, so it does not take orca-stl's `max`; clippy refuses Rust's `Ord::max`.
pub(crate) fn cpp_max<T: PartialOrd>(a: T, b: T) -> T {
    if a < b { b } else { a }
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
