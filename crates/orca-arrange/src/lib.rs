// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Arrangement: libnest2d, Arrange.cpp, NLopt 2.5.0 and Boost.Geometry's R*-tree.
//!
//! Arrangement as the C++ engine places objects on the bed: libnest2d (`deps_src/libnest2d`, with
//! libslic3r's geometry and NLopt as its optimiser), libslic3r's `Arrange.cpp`, NLopt 2.5.0's
//! subplex and Nelder-Mead (`sbplx`, `nldrmd`, `redblack`, `stop` and the API) and Boost.Geometry's
//! R*-tree (Boost 1.84.0's `geometry/index`), each of OrcaSlicer v2.4.2's build.
//!
//! - [`arrange`]: `Slic3r::arrangement::arrange` and its beds, parameters and object function.
//! - [`firstfit`]: libnest2d's first-fit selection, with Orca's plates and fixed items.
//! - [`placer`]: the no-fit-polygon placer, its edge cache, `finalAlign` and `minimizeCircle`.
//! - [`nfp`]: the convex no-fit polygon over `boost::rational<int128_t>` (`orca-exact`).
//! - [`item`]: libnest2d's item, with its cached transformed shape.
//! - [`geometry`]: libnest2d's geometry traits over libslic3r's types, and their hull.
//! - [`optimizer`]: `TOptimizer<L_SUBPLEX>` through `nlopt::opt`.
//! - [`nlopt`]: NLopt's subplex and Nelder-Mead, its red-black tree, `qsort_r` and API.
//! - [`bg`]: the Boost.Geometry predicates the libslic3r backend calls.
//! - [`rtree`]: `Arrange.cpp`'s R*-tree, for the queries it makes.
//!
//! Every result is the C++ engine's, bit for bit: the probe file `arrange.sop1` records the
//! oracle's (records `NLOP`, `NOPT`, `RTRE`, `NFPC`, `NGEO` and `ARNG`, `parity/SOP1.md`) and
//! `tests/probes.rs` replays them. What the C++ throws is an [`ArrangeError`], or a panic whose
//! payload is a `ClipperException` where Clipper throws. Where the C++ ends the process (an
//! exception from `~_NofitPolyPlacer`'s `finalAlign`) this translation panics; where it reads
//! out of bounds it stops with [`ArrangeError::Undefined`]. `parity/HAZARDS.md` (orca-arrange)
//! lists those paths, the clocks NLopt reads and what is not translated.

pub mod arrange;
pub mod bg;
pub mod firstfit;
pub mod geometry;
pub mod item;
pub mod nfp;
pub mod nlopt;
pub mod optimizer;
pub mod placer;
pub mod rtree;

use std::fmt;

/// The crate's name, as Cargo has it.
pub const NAME: &str = "orca-arrange";

/// An exception an arrangement throws, or a path this translation does not carry.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ArrangeError {
    /// A `boost::rational` or `int128_t` exception of the no-fit polygon's edge sort.
    Exact(orca_exact::ExactError),
    /// An exception `nlopt::opt::optimize` throws.
    Nlopt(nlopt::NloptException),
    /// `allow_rotations`: the rotating-calipers fit (`fit_into_box_rotation`,
    /// `min_area_boundingbox_rotation`) is not translated, as the CLI never rotates.
    RotationsNotTranslated,
    /// A path where the C++ has undefined behaviour (it reads outside a vector), named here.
    Undefined(&'static str),
}

impl fmt::Display for ArrangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArrangeError::Exact(e) => write!(f, "{e}"),
            ArrangeError::Nlopt(e) => write!(f, "nlopt: {e:?}"),
            ArrangeError::RotationsNotTranslated => {
                f.write_str("allow_rotations is not translated (rotating calipers)")
            }
            ArrangeError::Undefined(what) => write!(f, "undefined behaviour in the C++: {what}"),
        }
    }
}

impl std::error::Error for ArrangeError {}

impl From<orca_exact::ExactError> for ArrangeError {
    fn from(e: orca_exact::ExactError) -> ArrangeError {
        ArrangeError::Exact(e)
    }
}

#[cfg(test)]
mod tests {
    use super::NAME;

    #[test]
    fn name_is_the_package_name() {
        assert_eq!(NAME, env!("CARGO_PKG_NAME"));
    }
}
