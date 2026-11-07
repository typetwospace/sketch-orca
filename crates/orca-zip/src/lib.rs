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
//! ZIP archives as the C++ engine reads and writes them with miniz (`deps_src/miniz`, through
//! `miniz_extension.cpp`): the central directory, the deflate and inflate streams and the entries
//! of a 3MF file, byte for byte.
//!
//! The o2-model task of phase O2 fills this crate; its records are in the probe file `zip.sop1`,
//! which `tests/probes.rs` replays.
//!
//! Until that task runs, the crate holds only its name, and `tests/probes.rs` passes trivially
//! while its probe file is a stub (HEAD and PRUN only).

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
