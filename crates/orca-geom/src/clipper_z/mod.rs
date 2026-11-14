// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Clipper code: deps_src/clipper/clipper_z.hpp (1-18), clipper_z.cpp (1-7) and the
// CLIPPERLIB_USE_XYZ parts of clipper.hpp (69-73, 106-110, 133-135) and clipper.cpp (57-61, 81-83
// and Area) of OrcaSlicer v2.4.2 (Clipper 6.4.2 with the PrusaSlicer authors' changes), translated
// on 2026-11-13. Copyright Angus Johnson 2010-2017 and the PrusaSlicer authors, under the Boost
// Software License, version 1.0 (BSL-1.0; licenses/BSL-1.0.txt).

//! ClipperLib_Z: Clipper 6.4.2 as the engine's `clipper.lib` compiles it a second time
//! (`clipper_z.cpp` includes `clipper.cpp` with `CLIPPERLIB_USE_XYZ`), in namespace
//! `ClipperLib_Z`, with an `IntPoint` of three `cInt`s (`Eigen::Matrix<cInt, 3, 1>`, not
//! libslic3r's `Point`) and a Z fill callback. `PerimeterGenerator`'s `clip_extrusion` and
//! `LineSplit` clip extrusion lines with it, the z carrying a width or an index that the callback
//! fills in where two edges cross.
//!
//! - [`Clipper`] (`engine`): the XY [`crate::clipper::Clipper`] translated again for these points,
//!   with `ZFillFunction` and `SetZ`. The XY clipper stays as it is.
//! - [`PolyTree`] (`polytree`): the result tree, with Z contours.
//! - [`area`] and [`poly_tree_to_paths`]: the free functions the engine's callers use.
//!
//! The clip, fill and join types, `IntRect`, the ranges, the `initOptions` flags and
//! [`ClipperException`] are the XY module's: the C++ declares the same enums again in
//! `ClipperLib_Z`, with the same values. Left out, as no code of the engine reaches them in
//! `ClipperLib_Z`: `ClipperOffset`, `Orientation`, `PointInPolygon`, `SimplifyPolygon(s)`,
//! `CleanPolygon(s)`, the Minkowski sums and difference, `ClosedPathsFromPolyTree`,
//! `OpenPathsFromPolyTree` and `ReversePath(s)`.

mod engine;
mod polytree;

pub use engine::Clipper;
pub use polytree::{NodeId, PolyNode, PolyTree, ROOT};

pub(crate) use crate::clipper::round_cint;
pub use crate::clipper::{
    CInt, ClipType, ClipperException, HI_RANGE, IO_PRESERVE_COLLINEAR, IO_REVERSE_SOLUTION,
    IO_STRICTLY_SIMPLE, IntRect, LO_RANGE, PolyFillType, PolyType,
};

/// `ClipperLib_Z::IntPoint` (clipper.hpp 106-110): `Eigen::Matrix<cInt, 3, 1, DontAlign>`. Its
/// `==` compares x, y and z, as Eigen's does for the engine's callers and in `AddPaths`, a
/// template of clipper.hpp; the rest of the engine compares x and y only, as clipper.cpp's own
/// `operator==` does (`engine`'s `pt_eq`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IntPoint {
    /// `x()`.
    pub x: CInt,
    /// `y()`.
    pub y: CInt,
    /// `z()`.
    pub z: CInt,
}

impl IntPoint {
    /// `IntPoint(x, y, z)`.
    #[must_use]
    pub const fn new(x: CInt, y: CInt, z: CInt) -> Self {
        Self { x, y, z }
    }
}

/// `IntPoint2d(x, y)` (clipper.cpp 78-84, the z of 81-83): a point of z 0.
#[must_use]
pub const fn int_point_2d(x: CInt, y: CInt) -> IntPoint {
    IntPoint::new(x, y, 0)
}

/// `Path`: `std::vector<IntPoint>`.
pub type Path = Vec<IntPoint>;
/// `Paths`: `std::vector<Path>`.
pub type Paths = Vec<Path>;

/// `ZFillCallback` (clipper.hpp 133-135): `std::function<void(const IntPoint& e1bot, const
/// IntPoint& e1top, const IntPoint& e2bot, const IntPoint& e2top, IntPoint& pt)>`, called with the
/// ends of the two crossing edges and the crossing point, whose z it sets.
pub type ZFillCallback<'z> =
    Box<dyn FnMut(IntPoint, IntPoint, IntPoint, IntPoint, &mut IntPoint) + 'z>;

/// `Area(const Path&)`: the signed area of x and y, positive for a counter-clockwise path (y up);
/// 0 below three points.
#[must_use]
pub fn area(poly: &[IntPoint]) -> f64 {
    let size = poly.len();
    if size < 3 {
        return 0.0;
    }
    let mut a = 0.0;
    let mut j = size - 1;
    for i in 0..size {
        a += (poly[j].x as f64 + poly[i].x as f64) * (poly[j].y as f64 - poly[i].y as f64);
        j = i;
    }
    -a * 0.5
}

/// `PointInPolygon`'s cross product of `a - pt` and `b - pt` in x and y, in `CrossProductType`
/// (`double`), the differences taken in 64-bit integers first.
pub(crate) fn cross_from(pt: IntPoint, a: IntPoint, b: IntPoint) -> f64 {
    a.x.wrapping_sub(pt.x) as f64 * b.y.wrapping_sub(pt.y) as f64
        - b.x.wrapping_sub(pt.x) as f64 * a.y.wrapping_sub(pt.y) as f64
}

/// `PolyTreeToPaths` (both the copying and the moving form): every non-empty contour, depth first,
/// the tree's own first.
#[must_use]
pub fn poly_tree_to_paths(polytree: &PolyTree) -> Paths {
    let mut paths = Paths::with_capacity(usize::try_from(polytree.total()).unwrap_or(0));
    polytree.add_to_paths(ROOT, false, &mut paths);
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The union of two overlapping squares of z 1 and 2, as paths.sop1's hand-made PSZC case
    /// records it from the C++ engine: three calls of the callback (the second at (500, 1000)
    /// again, whose vertex the clean-up drops), the crossings' z from the first and the third, and
    /// the squares' own vertices keeping theirs.
    #[test]
    fn the_callback_fills_the_crossings() {
        let sq = |x0: i64, y0: i64, x1: i64, y1: i64, z: i64| {
            vec![
                IntPoint::new(x0, y0, z),
                IntPoint::new(x1, y0, z),
                IntPoint::new(x1, y1, z),
                IntPoint::new(x0, y1, z),
            ]
        };
        let mut calls = 0;
        let mut out = Paths::new();
        {
            let mut c = Clipper::new(0);
            c.z_fill_function(|_, _, _, _, pt| {
                calls += 1;
                pt.z = -calls;
            });
            c.add_path(&sq(0, 0, 1000, 1000, 1), PolyType::Subject, true);
            c.add_path(&sq(500, 500, 1500, 1500, 2), PolyType::Clip, true);
            c.execute(
                ClipType::Union,
                &mut out,
                PolyFillType::NonZero,
                PolyFillType::NonZero,
            );
        }
        assert_eq!(calls, 3);
        assert_eq!(out.len(), 1);
        let z_at = |x: i64, y: i64| {
            let p = out[0].iter().find(|p| p.x == x && p.y == y);
            p.map(|p| p.z)
        };
        assert_eq!(out[0].len(), 8);
        assert_eq!(z_at(1000, 500), Some(-3));
        assert_eq!(z_at(500, 1000), Some(-1));
        assert_eq!(z_at(0, 0), Some(1));
        assert_eq!(z_at(1500, 1500), Some(2));
    }

    /// An open path whose last point repeats the one before in x and y but not in z: `AddPath`
    /// (one path) trims the last in x and y and keeps the earlier z; `AddPaths` of two paths trims
    /// by Eigen's `==` of x, y and z, so both stay and `AddPathInternal`'s duplicate pass keeps the
    /// later z, as the full set of paths.sop1's PSZC records (cases 18 and 811).
    #[test]
    fn add_paths_trims_repeated_ends_in_three_coordinates() {
        let walk = vec![
            IntPoint::new(0, 0, 1),
            IntPoint::new(1000, 1000, 2),
            IntPoint::new(1000, 1000, 3),
        ];
        let other = vec![IntPoint::new(0, 500, 1), IntPoint::new(1000, 400, 2)];
        let square = vec![
            IntPoint::new(-100, -100, 0),
            IntPoint::new(2000, -100, 0),
            IntPoint::new(2000, 2000, 0),
            IntPoint::new(-100, 2000, 0),
        ];
        let z_at_corner = |paths: &[&[IntPoint]]| {
            let mut c = Clipper::new(0);
            c.add_paths(paths.iter().copied(), PolyType::Subject, false);
            c.add_path(&square, PolyType::Clip, true);
            let mut t = PolyTree::new();
            c.execute_tree(
                ClipType::Intersection,
                &mut t,
                PolyFillType::NonZero,
                PolyFillType::NonZero,
            );
            let out = poly_tree_to_paths(&t);
            let mut z = Vec::new();
            for p in out.iter().flatten() {
                if p.x == 1000 && p.y == 1000 {
                    z.push(p.z);
                }
            }
            z
        };
        assert_eq!(z_at_corner(&[&walk]), vec![2]);
        assert_eq!(z_at_corner(&[&walk, &other]), vec![3]);
    }
}
