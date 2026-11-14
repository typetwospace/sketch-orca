// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/ClipperUtils.cpp lines 992-1017 of OrcaSlicer v2.4.2,
// translated on 2026-11-13 by paths-1. Copyright the OrcaSlicer authors, the BambuStudio authors
// (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU Affero General Public
// License, version 3 (AGPL-3.0; LICENSE).

//! [`union_pt_chained_outside_in`]: the union of polygons as Clipper's tree (orca-geom's
//! `union_pt`, by even-odd), walked from the outside in: each level's nodes in the order
//! [`chain_clipper_polynodes`] gives their first points, each node's contour followed at once by
//! its children's, holes (clockwise) turned counter-clockwise. Concentric fill and the brim order
//! their loops so.
//!
//! The C++ moves each contour out of the tree (`std::move(node->Contour)`) and passes the children
//! by rvalue reference without moving them; the translation takes the contour and copies the
//! child list. A node's first point is read before any contour of its level is moved. A tree node
//! with an empty contour (Clipper never makes one) stops ([`EMPTY_POLYLINE`]): the C++ reads its
//! `Contour.front()`.

use orca_geom::clipper::{NodeId, PolyTree, ROOT};
use orca_geom::clipper_utils::union_pt;
use orca_geom::point::Point;
use orca_geom::polygon::{Polygon, Polygons};

use crate::points::chain_clipper_polynodes;
use crate::stop::{EMPTY_POLYLINE, stop};

/// `traverse_pt_outside_in(nodes, retval)`.
fn traverse_pt_outside_in(tree: &mut PolyTree, nodes: &[NodeId], retval: &mut Polygons) {
    // collect ordering points
    let mut ordering_points = Vec::with_capacity(nodes.len());
    for &node in nodes {
        match tree.node(node).contour.first() {
            Some(p) => ordering_points.push(Point::new(p.x, p.y)),
            None => stop("traverse_pt_outside_in", EMPTY_POLYLINE),
        }
    }

    // Perform the ordering, push results recursively.
    //FIXME pass the last point to chain_clipper_polynodes?
    for node in chain_clipper_polynodes(&ordering_points, nodes) {
        let contour = core::mem::take(&mut tree.node_mut(node).contour);
        retval.push(Polygon::new(contour));
        if tree.is_hole(node) {
            // Orient a hole, which is clockwise oriented, to CCW.
            let back = retval.len() - 1;
            retval[back].reverse();
        }
        // traverse the next depth
        let childs = tree.node(node).childs.clone();
        traverse_pt_outside_in(tree, &childs, retval);
    }
}

/// `union_pt_chained_outside_in(subject)`.
#[must_use]
pub fn union_pt_chained_outside_in(subject: &[Polygon]) -> Polygons {
    let mut retval = Polygons::new();
    let mut tree = union_pt(subject);
    let childs = tree.node(ROOT).childs.clone();
    traverse_pt_outside_in(&mut tree, &childs, &mut retval);
    retval
}
