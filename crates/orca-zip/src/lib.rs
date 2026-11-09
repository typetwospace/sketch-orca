// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! ZIP archives as miniz reads and writes them, for 3MF files.
//!
//! ZIP archives as the C++ engine reads them with miniz 2.1.0 (`deps_src/miniz`, through
//! `miniz_extension.cpp`), translated by the o2-model task (2026-11-07): the read path the 3MF
//! reader takes, bit for bit. [`reader`] opens an archive (from a file, from memory or through a
//! read function), reads and checks its central directory with ZIP64, finds an entry by name,
//! gives its statistics and extra field and extracts it to a buffer, through a callback or to a
//! file; [`tinfl`] is the inflater it extracts with and [`crc`] the CRC-32 it checks with;
//! [`error`] holds miniz's error codes and texts and [`extension`] libslic3r's wrapping of them.
//! The writer (`mz_zip_writer_*`, `tdefl`) is not translated: the command line reads 3MF files
//! and writes G-code.
//!
//! `tests/probes.rs` replays the probe file `zip.sop1`, which the C++ engine's own build writes.

pub mod crc;
pub mod error;
pub mod extension;
pub mod reader;
pub mod tinfl;

/// The crate's name, as Cargo has it.
pub const NAME: &str = "orca-zip";

#[cfg(test)]
mod tests {
    use super::NAME;

    #[test]
    fn name_is_the_package_name() {
        assert_eq!(NAME, env!("CARGO_PKG_NAME"));
    }
}
