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
//! - [`bbs_3mf`]: the 3MF reader (`Format/bbs_3mf.cpp`, on `orca-zip` and `orca-xml`), with
//!   [`semver`] (the bundled `semver.c` and `Semver.hpp`) and [`utils`] (what it takes from
//!   `utils.cpp` and Boost.Filesystem);
//! - [`model`]: `Model`, `ModelObject`, `ModelVolume` and `ModelInstance` with their bounding
//!   boxes, hulls, `ensure_on_bed` and `center_around_origin`, their copies, `clear_` functions,
//!   painted queries and `raw_mesh`, with [`object_id`] (the ids and timestamps PrintApply
//!   compares);
//! - [`compare`]: the comparisons PrintApply makes between two models;
//! - [`transformation`]: `Geometry::Transformation`, an instance's or a part's placement, with
//!   the shrinkage compensation;
//! - [`config`]: `ModelConfig`, an object's or a part's own settings;
//! - [`facets`]: a part's painting as the 3MF reader loads it, and what PrintApply asks of it;
//! - [`build_volume`] and [`circle`]: the print volume and whether an instance fits in it;
//! - [`arrange`]: the arrange polygons, the bed (with its excluded area) and items' inflation, and
//!   the result written back;
//! - [`stop`]: [`stop::NotPorted`], what a call outside domain A returns (painted facets).
//!
//! Every record of the probe file `model.sop1`, and those of `slice.sop1` of the piece front-2
//! (orca-o3-front-2, 2026-11-13), is rebuilt by `tests/probes.rs` from what this crate computes.
//! The 3MF reader is whole, translated in three pieces: 1, the container, its relationships and
//! the root model; 2, the config entries, with [`emboss`]'s text and shape configurations; 3, the
//! sub-model files and the assembly into the model. Only the entries that a load with
//! configuration reads, which the command line's load never asks for, answer
//! [`read::ReadError::NotPorted`].

pub mod arrange;
pub mod bbs_3mf;
pub mod build_volume;
pub mod circle;
pub mod compare;
pub mod config;
pub mod emboss;
pub mod facets;
pub mod model;
pub mod obj;
pub mod object_id;
pub mod read;
pub mod semver;
pub mod stop;
pub mod transformation;
pub mod utils;

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
