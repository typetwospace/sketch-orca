// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/ClipperZUtils.hpp lines 1-144 whole and the template
// polylines_merge of src/libslic3r/Polyline.hpp (lines 234-246) of OrcaSlicer v2.4.2, translated on
// 2026-11-13 by orca-o3-surfaces-1. Copyright the OrcaSlicer authors, the BambuStudio authors
// (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU Affero General Public
// License, version 3 (AGPL-3.0; LICENSE).

//! `ClipperZUtils`: the conversions between libslic3r's points and ClipperLib_Z's points (whose z
//! carries an index) and the Z fill visitor that records which two contours cross at each new
//! point. Region expansion (`crate::region_expansion`) tracks with them which source and which
//! boundary each seed segment comes from.
//!
//! This belongs in orca-geom, beside its `clipper_z` (ClipperZUtils.hpp is a header of libslic3r
//! that only RegionExpansion.cpp includes in the engine); no piece of phase O3 carries it there, so
//! orca-o3-surfaces-1 translates it here. [`polylines_merge_z`] is `polylines_merge` of
//! Polyline.hpp, a template that orca-geom carries for `Points` only (`polyline::polylines_merge`),
//! here for `ClipperLib_Z::Path`; it belongs in orca-geom's `polyline` as well.
//!
//! The C++ templates' `Open` parameter is a `bool` argument here. The reserves of `to_zpath` and
//! `from_zpath` (`(path.size() + Open) ? 1 : 0` points) change nothing that can be read.

use orca_geom::clipper_z::IntPoint;
use orca_geom::expolygon::ExPolygon;
use orca_geom::point::{Coord, Point, Points};

/// `ZPoint`: `ClipperLib_Z::IntPoint`.
pub type ZPoint = IntPoint;
/// `ZPoints` and `ZPath`: `ClipperLib_Z::Path`.
pub type ZPath = Vec<ZPoint>;
/// `ZPaths`: `ClipperLib_Z::Paths`.
pub type ZPaths = Vec<ZPath>;

/// `zpoint_lower(l, r)`: x, then y, then z.
#[must_use]
pub fn zpoint_lower(l: &ZPoint, r: &ZPoint) -> bool {
    l.x < r.x || (l.x == r.x && (l.y < r.y || (l.y == r.y && l.z < r.z)))
}

/// `to_zpath<Open>(path, z)`: the points with the z `z`; when `open`, the first point repeated at
/// the end.
#[must_use]
pub fn to_zpath(path: &[Point], z: Coord, open: bool) -> ZPath {
    let mut out = ZPath::new();
    if !path.is_empty() {
        out.extend(path.iter().map(|p| ZPoint::new(p.x, p.y, z)));
        if open {
            out.push(out[0]);
        }
    }
    out
}

/// `to_zpaths<Open>(paths, z)`.
#[must_use]
pub fn to_zpaths(paths: &[Points], z: Coord, open: bool) -> ZPaths {
    paths.iter().map(|path| to_zpath(path, z, open)).collect()
}

/// `expolygons_to_zpaths<Open>(src, base_idx)`: each expolygon's contour, then its holes, with the
/// z `base_idx`, which goes up by one for each expolygon.
#[must_use]
pub fn expolygons_to_zpaths(src: &[ExPolygon], base_idx: &mut Coord, open: bool) -> ZPaths {
    let mut out = ZPaths::with_capacity(src.iter().map(ExPolygon::num_contours).sum());
    for expoly in src {
        out.push(to_zpath(&expoly.contour.points, *base_idx, open));
        for hole in &expoly.holes {
            out.push(to_zpath(&hole.points, *base_idx, open));
        }
        *base_idx += 1;
    }
    out
}

/// `from_zpath<Open>(path)`: the points' x and y; when `open`, the first point repeated at the end.
#[must_use]
pub fn from_zpath(path: &[ZPoint], open: bool) -> Points {
    let mut out = Points::new();
    if !path.is_empty() {
        out.extend(path.iter().map(|p| Point::new(p.x, p.y)));
        if open {
            out.push(out[0]);
        }
    }
    out
}

/// `from_zpaths<Open>(paths, out)`: appends each path's points to `out`.
pub fn from_zpaths_into(paths: &[ZPath], out: &mut Vec<Points>, open: bool) {
    out.reserve(paths.len());
    for path in paths {
        out.push(from_zpath(path, open));
    }
}

/// `from_zpaths<Open>(paths)`.
#[must_use]
pub fn from_zpaths(paths: &[ZPath], open: bool) -> Vec<Points> {
    let mut out = Vec::new();
    from_zpaths_into(paths, &mut out, open);
    out
}

/// `ClipperZIntersectionVisitor::Intersection`: the two lowest distinct z of the crossing edges'
/// ends.
pub type Intersection = (Coord, Coord);
/// `ClipperZIntersectionVisitor::Intersections`.
pub type Intersections = Vec<Intersection>;

/// `ClipperZIntersectionVisitor`: the Z fill callback that records, for each point where edges of
/// two contours cross, the two contours' z in `intersections` and gives the point the negative,
/// 1-based index of that record; where the four ends have one z (a contour crossing itself), the
/// point takes it.
#[derive(Debug)]
pub struct ClipperZIntersectionVisitor<'a> {
    /// `m_intersections`.
    intersections: &'a mut Intersections,
}

impl<'a> ClipperZIntersectionVisitor<'a> {
    /// `ClipperZIntersectionVisitor(intersections)`.
    pub fn new(intersections: &'a mut Intersections) -> Self {
        Self { intersections }
    }

    /// `reset()`.
    pub fn reset(&mut self) {
        self.intersections.clear();
    }

    /// `operator()(e1bot, e1top, e2bot, e2top, pt)`: the four z sorted (`std::sort`) and made
    /// unique (`std::unique`); one value is copied to `pt`, otherwise the first two are recorded.
    pub fn visit(
        &mut self,
        e1bot: ZPoint,
        e1top: ZPoint,
        e2bot: ZPoint,
        e2top: ZPoint,
        pt: &mut ZPoint,
    ) {
        let mut srcs: [Coord; 4] = [e1bot.z, e1top.z, e2bot.z, e2top.z];
        orca_stl::sort(&mut srcs);
        let end = orca_stl::unique(&mut srcs);
        if end == 1 {
            // Self intersection may happen on source contour. Just copy the Z value.
            pt.z = srcs[0];
        } else if end >= 2 {
            // Store a -1 based negative index into the "intersections" vector here.
            self.intersections.push((srcs[0], srcs[1]));
            pt.z = -(self.intersections.len() as Coord);
        }
    }

    /// `intersections()`.
    #[must_use]
    pub fn intersections(&self) -> &Intersections {
        self.intersections
    }
}

/// `polylines_merge(dst, dst_first, src, src_first)` of Polyline.hpp for `ClipperLib_Z::Path`:
/// joins `src` to `dst` at the given ends (`dst_first` and `src_first` true for their first
/// points), the result in `dst`.
pub fn polylines_merge_z(dst: &mut ZPath, dst_first: bool, mut src: ZPath, src_first: bool) {
    if dst_first {
        if src_first {
            dst.reverse();
        } else {
            core::mem::swap(dst, &mut src);
        }
    } else if !src_first {
        src.reverse();
    }
    // Merge src into dst.
    dst.append(&mut src);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Four ends of two contours record the two lowest z; one z is copied.
    #[test]
    fn the_visitor_records_two_contours() {
        let mut list = Intersections::new();
        let mut v = ClipperZIntersectionVisitor::new(&mut list);
        let mut pt = ZPoint::new(5, 5, 0);
        let z = |z: Coord| ZPoint::new(0, 0, z);
        v.visit(z(7), z(2), z(7), z(2), &mut pt);
        assert_eq!(pt.z, -1);
        let mut pt2 = ZPoint::new(5, 5, 0);
        v.visit(z(3), z(3), z(3), z(3), &mut pt2);
        assert_eq!(pt2.z, 3);
        assert_eq!(v.intersections(), &vec![(2, 7)]);
    }

    /// Each pair of ends gives the order `polylines_merge` joins them in.
    #[test]
    fn merging_follows_the_ends() {
        let p = |x: Coord| ZPoint::new(x, 0, 0);
        let mut dst = vec![p(1), p(2)];
        polylines_merge_z(&mut dst, true, vec![p(3), p(4)], false);
        assert_eq!(dst, vec![p(3), p(4), p(1), p(2)]);
        let mut dst = vec![p(1), p(2)];
        polylines_merge_z(&mut dst, false, vec![p(3), p(4)], false);
        assert_eq!(dst, vec![p(1), p(2), p(4), p(3)]);
        let mut dst = vec![p(1), p(2)];
        polylines_merge_z(&mut dst, true, vec![p(3), p(4)], true);
        assert_eq!(dst, vec![p(2), p(1), p(3), p(4)]);
    }
}
