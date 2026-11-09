// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The model and its readers: STL, OBJ and 3MF, Model, BuildVolume and ModelArrange.
//!
//! The model as the C++ engine loads and places it before slicing:
//!
//! - [`read`]: `Model::read_from_file` and the STL reader (`Format/STL.cpp`, on `orca-mesh`);
//! - [`obj`]: the OBJ reader and its material libraries (`Format/OBJ.cpp`, `objparser.cpp`);
//! - [`model`]: `Model`, `ModelObject`, `ModelVolume` and `ModelInstance` with their bounding
//!   boxes, hulls, `ensure_on_bed` and `center_around_origin`;
//! - [`transformation`]: `Geometry::Transformation`, an instance's or a part's placement;
//! - [`config`]: `ModelConfig`, an object's or a part's own settings;
//! - [`facets`]: a part's painting as the 3MF reader loads it;
//! - [`build_volume`] and [`circle`]: the print volume and whether an instance fits in it;
//! - [`arrange`]: the arrange polygons, the bed and items' inflation, and the result written back.
//!
//! Every record of the probe file `model.sop1` is rebuilt by `tests/probes.rs` from what this
//! crate computes. The 3MF reader (`bbs_3mf.cpp`, on `orca-zip` and `orca-xml`) is not translated
//! yet: [`read::read_from_file`] answers it with [`read::ReadError::NotPorted`].

pub mod arrange;
pub mod build_volume;
pub mod circle;
pub mod config;
pub mod facets;
pub mod model;
pub mod obj;
pub mod read;
pub mod transformation;

/// The crate's name, as Cargo has it.
pub const NAME: &str = "orca-model";

#[cfg(test)]
mod tests {
    use super::NAME;

    #[test]
    fn name_is_the_package_name() {
        assert_eq!(NAME, env!("CARGO_PKG_NAME"));
    }
}
