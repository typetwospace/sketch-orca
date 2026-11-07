// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Libnest2D code: libnest2d as OrcaSlicer v2.4.2 carries it in deps_src/libnest2d,
// translated on 2026-11-07 from include/libnest2d/nester.hpp (_Item, BIN_ID_UNSET, BIN_ID_UNFIT,
// LARGE_COST_TO_REJECT) and common.hpp (MAX_NUM_PLATES). Copyright (C) Tamas Meszaros, modified
// by Prusa Research, Bambu Lab and the OrcaSlicer contributors, under the GNU Lesser General
// Public License, version 3 (licenses/LGPL-3.0.txt).

//! libnest2d's `_Item<ExPolygon>`: a shape with its inflation, rotation and translation, and the
//! fields Orca added for arranging (bed temperature, height, extruders, virtual objects).
//!
//! The C++ item caches its inflated and transformed shape, area and bounding box in `mutable`
//! members, invalidated by the setters. Every cache is a pure function of the item's state (the
//! setters invalidate everything a change reaches), so here each is a [`OnceLock`]: computed on
//! first use, from any thread, and reset by the `&mut self` setters. The rightmost-top and
//! leftmost-bottom vertices are found again on each call, as the first maximum and minimum
//! (`std::max_element`, `std::min_element`), which is the cached iterator's vertex.

use crate::geometry::{
    Box, area_expolygon, bounding_box, offset_shape, rotate_shape, translate_shape,
};
use crate::nfp::vsort;
use orca_geom::expolygon::ExPolygon;
use orca_geom::point::{Coord, Point};
use std::sync::OnceLock;

/// `BIN_ID_UNSET`.
pub const BIN_ID_UNSET: i32 = -1;
/// `BIN_ID_UNFIT`.
pub const BIN_ID_UNFIT: i32 = -1;
/// `LARGE_COST_TO_REJECT`.
pub const LARGE_COST_TO_REJECT: f64 = 1e7;
/// `MAX_NUM_PLATES`.
pub const MAX_NUM_PLATES: i32 = 36;

/// `_Item<ExPolygon>`.
#[derive(Clone, Debug, Default)]
pub struct Item {
    sh: ExPolygon,
    translation: Point,
    rotation: f64,
    inflation: Coord,
    has_rotation: bool,
    has_translation: bool,
    has_inflation: bool,
    tr_cache: OnceLock<ExPolygon>,
    area_cache: OnceLock<f64>,
    inflate_cache: OnceLock<ExPolygon>,
    bb_cache: OnceLock<Box>,
    binid: i32,
    priority: i32,
    fixed: bool,
    /// `itemid_`.
    pub itemid: i32,
    /// `extrude_ids`.
    pub extrude_ids: Vec<i32>,
    /// `filament_temp_type`: -1 unset.
    pub filament_temp_type: i32,
    /// `height`.
    pub height: f64,
    /// `print_temp`.
    pub print_temp: f64,
    /// `bed_temp`.
    pub bed_temp: f64,
    /// `vitrify_temp`.
    pub vitrify_temp: f64,
    /// `name`.
    pub name: String,
    /// `is_virt_object`.
    pub is_virt_object: bool,
    /// `is_wipe_tower`.
    pub is_wipe_tower: bool,
    /// `has_tried_with_excluded`.
    pub has_tried_with_excluded: bool,
}

impl Item {
    /// `_Item(sh)`.
    #[must_use]
    pub fn new(sh: ExPolygon) -> Item {
        Item {
            sh,
            binid: BIN_ID_UNSET,
            filament_temp_type: -1,
            ..Item::default()
        }
    }

    /// `isFixed()`.
    #[must_use]
    pub fn is_fixed(&self) -> bool {
        self.fixed
    }

    /// `markAsFixedInBin(binid)`: fixed when the bin is not negative.
    pub fn mark_as_fixed_in_bin(&mut self, binid: i32) {
        self.fixed = binid >= 0;
        self.binid = binid;
    }

    /// `binId(idx)`.
    pub fn set_bin_id(&mut self, idx: i32) {
        self.binid = idx;
    }

    /// `binId()`.
    #[must_use]
    pub fn bin_id(&self) -> i32 {
        self.binid
    }

    /// `priority(p)`.
    pub fn set_priority(&mut self, p: i32) {
        self.priority = p;
    }

    /// `priority()`.
    #[must_use]
    pub fn priority(&self) -> i32 {
        self.priority
    }

    /// `rawShape()`.
    #[must_use]
    pub fn raw_shape(&self) -> &ExPolygon {
        &self.sh
    }

    /// `area()`: the inflated shape's area.
    #[must_use]
    pub fn area(&self) -> f64 {
        *self
            .area_cache
            .get_or_init(|| area_expolygon(self.inflated_shape()))
    }

    /// `translate(d)`.
    pub fn translate(&mut self, d: Point) {
        let t = self.translation + d;
        self.set_translation(t);
    }

    /// `rotate(rads)`.
    pub fn rotate(&mut self, rads: f64) {
        self.set_rotation(self.rotation + rads);
    }

    /// `inflation(distance)`: always marks the item inflated and drops every cache.
    pub fn set_inflation(&mut self, distance: Coord) {
        self.inflation = distance;
        self.has_inflation = true;
        self.invalidate_cache();
    }

    /// `inflation()`.
    #[must_use]
    pub fn inflation(&self) -> Coord {
        self.inflation
    }

    /// `inflate(distance)`.
    pub fn inflate(&mut self, distance: Coord) {
        self.set_inflation(self.inflation.wrapping_add(distance));
    }

    /// `rotation()`.
    #[must_use]
    pub fn rotation(&self) -> f64 {
        self.rotation
    }

    /// `translation()`.
    #[must_use]
    pub fn translation(&self) -> Point {
        self.translation
    }

    /// `rotation(rot)`: a different angle drops the transformed shape and the bounding box.
    pub fn set_rotation(&mut self, rot: f64) {
        #[allow(clippy::float_cmp)]
        if self.rotation != rot {
            self.rotation = rot;
            self.has_rotation = true;
            self.tr_cache = OnceLock::new();
            self.bb_cache = OnceLock::new();
        }
    }

    /// `translation(tr)`: a different offset drops the transformed shape.
    pub fn set_translation(&mut self, tr: Point) {
        if self.translation != tr {
            self.translation = tr;
            self.has_translation = true;
            self.tr_cache = OnceLock::new();
        }
    }

    /// `transformedShape()`: the inflated shape, rotated when a rotation was set, translated when
    /// a translation was.
    #[must_use]
    pub fn transformed_shape(&self) -> &ExPolygon {
        self.tr_cache.get_or_init(|| {
            let mut cpy = self.inflated_shape().clone();
            if self.has_rotation {
                rotate_shape(&mut cpy, self.rotation);
            }
            if self.has_translation {
                translate_shape(&mut cpy, self.translation);
            }
            cpy
        })
    }

    /// `boundingBox()`: the box of the inflated shape (rotated when a rotation was set), moved by
    /// the translation; defined.
    #[must_use]
    pub fn bounding_box(&self) -> Box {
        let bb = self.bb_cache.get_or_init(|| {
            if self.has_rotation {
                let mut rotsh = self.inflated_shape().clone();
                rotate_shape(&mut rotsh, self.rotation);
                bounding_box(&rotsh)
            } else {
                bounding_box(self.inflated_shape())
            }
        });
        Box::new(bb.min + self.translation, bb.max + self.translation)
    }

    /// `referenceVertex()`: the rightmost top vertex.
    #[must_use]
    pub fn reference_vertex(&self) -> Point {
        self.rightmost_top_vertex()
    }

    /// `rightmostTopVertex()`: the first highest point of the transformed contour (by y, then x).
    ///
    /// # Panics
    ///
    /// For an empty contour, which the C++ dereferences.
    #[must_use]
    pub fn rightmost_top_vertex(&self) -> Point {
        let pts = &self.transformed_shape().contour.points;
        assert!(!pts.is_empty(), "rightmostTopVertex of an empty item");
        pts[orca_stl::max_element_by(pts, vsort)]
    }

    /// `leftmostBottomVertex()`: the first lowest point of the transformed contour.
    ///
    /// # Panics
    ///
    /// For an empty contour, which the C++ dereferences.
    #[must_use]
    pub fn leftmost_bottom_vertex(&self) -> Point {
        let pts = &self.transformed_shape().contour.points;
        assert!(!pts.is_empty(), "leftmostBottomVertex of an empty item");
        pts[orca_stl::min_element_by(pts, vsort)]
    }

    /// `infaltedShape()`: the shape offset by the inflation once an inflation was set (even 0).
    fn inflated_shape(&self) -> &ExPolygon {
        if self.has_inflation {
            self.inflate_cache.get_or_init(|| {
                let mut c = self.sh.clone();
                offset_shape(&mut c, self.inflation);
                c
            })
        } else {
            &self.sh
        }
    }

    /// `invalidateCache()`.
    fn invalidate_cache(&mut self) {
        self.tr_cache = OnceLock::new();
        self.area_cache = OnceLock::new();
        self.inflate_cache = OnceLock::new();
        self.bb_cache = OnceLock::new();
    }
}
