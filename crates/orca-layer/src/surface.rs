// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.
//
// Carries OrcaSlicer code: src/libslic3r/Surface.hpp lines 1-312 and Surface.cpp lines 1-95 of
// OrcaSlicer v2.4.2, translated on 2026-11-14 (orca-o3-layer-2). Copyright the OrcaSlicer authors,
// the BambuStudio authors (Bambu Lab), the PrusaSlicer authors (Prusa Research) and the Slic3r
// authors (Alessandro Ranellucci), under the GNU Affero General Public License, version 3
// (AGPL-3.0; LICENSE).

//! [`Surface`]: a typed island of a layer region (top, bottom, bridge, internal, solid, void or
//! perimeter area), its thickness, bridge angle and extra perimeters, and the helpers of
//! Surface.hpp over lists of surfaces ([`Surfaces`]) and of references to them ([`SurfacesPtr`]).
//!
//! The type predicates do not test for `stPerimeter`, as the C++ says. `to_polygons` gives each
//! surface's contour and then its holes; `to_expolygons` the islands in order. `get_extents` of a
//! surface is its contour's box, of a list the first surface's box merged with the others'. An
//! empty contour's box is indeterminate in the C++; it is the oracle build's, (0, 0) to (0, 0)
//! and defined (HAZARDS.md).
//! `surfaces_could_merge` compares the type, the thickness, the thickness in layers and the bridge
//! angle (not the extra perimeters), the doubles by `==`.
//!
//! The C++ forms that move (`Surfaces &&`, `ExPolygons &&`) give the same values as the copying
//! ones; a Rust caller passes the list by value where it no longer needs it.
//!
//! Left out: Surface.cpp's SVG output (`export_surface_type_legend_to_svg`,
//! `export_surface_type_legend_to_svg_box_size` and `export_to_svg`), which only debugging builds
//! call and which needs libslic3r's `SVG` class; nothing in the engine's release path reaches it.

use orca_geom::bounding_box::BoundingBox;
use orca_geom::expolygon::{self, ExPolygon, ExPolygons};
use orca_geom::polygon::{self, Polygons};

/// `enum SurfaceType`, by its value in Surface.hpp.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(i32)]
pub enum SurfaceType {
    /// `stTop`: top horizontal surface, visible from the top.
    Top = 0,
    /// `stBottom`: bottom horizontal surface, printed with a normal extrusion flow.
    Bottom = 1,
    /// `stBottomBridge`: bottom horizontal surface, unsupported, printed with a bridging flow.
    BottomBridge = 2,
    /// `stInternalAfterExternalBridge`: the second bridge surface above a bottom bridge.
    InternalAfterExternalBridge = 3,
    /// `stInternal`: normal sparse infill.
    #[default]
    Internal = 4,
    /// `stInternalSolid`: full infill.
    InternalSolid = 5,
    /// `stInternalBridge`: the first layer of dense infill over sparse infill, bridged.
    InternalBridge = 6,
    /// `stSecondInternalBridge`: the second layer of dense infill over sparse infill, bridged.
    SecondInternalBridge = 7,
    /// `stInternalVoid`: sparse infill turned void.
    InternalVoid = 8,
    /// `stPerimeter`: inner and outer perimeters.
    Perimeter = 9,
    /// `stCount`: the number of surface types.
    Count = 10,
}

/// `stCount` as a count.
pub const ST_COUNT: usize = SurfaceType::Count as usize;

impl SurfaceType {
    /// The type of a value, `None` past `stCount` or below 0.
    #[must_use]
    pub fn from_i32(v: i32) -> Option<Self> {
        Some(match v {
            0 => Self::Top,
            1 => Self::Bottom,
            2 => Self::BottomBridge,
            3 => Self::InternalAfterExternalBridge,
            4 => Self::Internal,
            5 => Self::InternalSolid,
            6 => Self::InternalBridge,
            7 => Self::SecondInternalBridge,
            8 => Self::InternalVoid,
            9 => Self::Perimeter,
            10 => Self::Count,
            _ => return None,
        })
    }
}

/// `class Surface`.
#[derive(Clone, Debug)]
pub struct Surface {
    /// `surface_type`.
    pub surface_type: SurfaceType,
    /// `expolygon`.
    pub expolygon: ExPolygon,
    /// `thickness`: in mm, -1 when not set.
    pub thickness: f64,
    /// `thickness_layers`: in layers.
    pub thickness_layers: u16,
    /// `bridge_angle`: in radians, counter-clockwise, 0 is east; negative means undefined.
    pub bridge_angle: f64,
    /// `extra_perimeters`.
    pub extra_perimeters: u16,
}

impl Default for Surface {
    /// `Surface()`: `stInternal`.
    fn default() -> Self {
        Self::new(SurfaceType::Internal)
    }
}

impl Surface {
    /// `Surface(surface_type)`: no island, thickness -1, one layer, bridge angle -1, no extra
    /// perimeters.
    #[must_use]
    pub fn new(surface_type: SurfaceType) -> Self {
        Self::with_expolygon(surface_type, ExPolygon::default())
    }

    /// `Surface(surface_type, expolygon)`.
    #[must_use]
    pub fn with_expolygon(surface_type: SurfaceType, expolygon: ExPolygon) -> Self {
        Self {
            surface_type,
            expolygon,
            thickness: -1.,
            thickness_layers: 1,
            bridge_angle: -1.,
            extra_perimeters: 0,
        }
    }

    /// `Surface(other, expolygon)`: `other`'s fields with another island.
    #[must_use]
    pub fn from_template(other: &Surface, expolygon: ExPolygon) -> Self {
        Self {
            surface_type: other.surface_type,
            expolygon,
            thickness: other.thickness,
            thickness_layers: other.thickness_layers,
            bridge_angle: other.bridge_angle,
            extra_perimeters: other.extra_perimeters,
        }
    }

    /// `area()`.
    #[must_use]
    pub fn area(&self) -> f64 {
        self.expolygon.area()
    }

    /// `empty()`: the island has no contour points.
    #[must_use]
    pub fn empty(&self) -> bool {
        self.expolygon.is_empty()
    }

    /// `clear()`.
    pub fn clear(&mut self) {
        self.expolygon.clear();
    }

    /// `is_top()`.
    #[must_use]
    pub fn is_top(&self) -> bool {
        self.surface_type == SurfaceType::Top
    }

    /// `is_bottom()`: `stBottom` or `stBottomBridge`.
    #[must_use]
    pub fn is_bottom(&self) -> bool {
        self.surface_type == SurfaceType::Bottom || self.surface_type == SurfaceType::BottomBridge
    }

    /// `is_bridge()`: `stBottomBridge` or `stInternalBridge`.
    #[must_use]
    pub fn is_bridge(&self) -> bool {
        self.surface_type == SurfaceType::BottomBridge
            || self.surface_type == SurfaceType::InternalBridge
    }

    /// `is_internal_bridge()`.
    #[must_use]
    pub fn is_internal_bridge(&self) -> bool {
        self.surface_type == SurfaceType::InternalBridge
    }

    /// `is_external()`: top or bottom.
    #[must_use]
    pub fn is_external(&self) -> bool {
        self.is_top() || self.is_bottom()
    }

    /// `is_internal()`: not external.
    #[must_use]
    pub fn is_internal(&self) -> bool {
        !self.is_external()
    }

    /// `is_solid()`: external, `stInternalSolid` or `stInternalBridge`.
    #[must_use]
    pub fn is_solid(&self) -> bool {
        self.is_external()
            || self.surface_type == SurfaceType::InternalSolid
            || self.surface_type == SurfaceType::InternalBridge
    }

    /// `is_solid_infill()`.
    #[must_use]
    pub fn is_solid_infill(&self) -> bool {
        self.surface_type == SurfaceType::InternalSolid
    }
}

/// `Surfaces`.
pub type Surfaces = Vec<Surface>;

/// `SurfacesPtr`: references to surfaces another list owns.
pub type SurfacesPtr<'a> = Vec<&'a Surface>;

/// `to_polygons(const Surface &)`: the contour, then the holes.
#[must_use]
pub fn to_polygons(surface: &Surface) -> Polygons {
    expolygon::to_polygons(&surface.expolygon)
}

/// `to_polygons(Surface &&)`: the contour, then the holes, moved out.
#[must_use]
pub fn to_polygons_owned(surface: Surface) -> Polygons {
    let ExPolygon { contour, holes } = surface.expolygon;
    let mut polygons = Polygons::with_capacity(holes.len() + 1);
    polygons.push(contour);
    polygons.extend(holes);
    polygons
}

/// `to_polygons(const Surfaces &)` and `to_polygons(const SurfacesPtr &)`: each surface's contour,
/// then its holes, in order.
#[must_use]
pub fn to_polygons_surfaces<'a, I>(src: I) -> Polygons
where
    I: IntoIterator<Item = &'a Surface>,
    I::IntoIter: Clone,
{
    let it = src.into_iter();
    let mut num = 0usize;
    for s in it.clone() {
        num += s.expolygon.holes.len() + 1;
    }
    let mut polygons = Polygons::with_capacity(num);
    for s in it {
        polygons.push(s.expolygon.contour.clone());
        for h in &s.expolygon.holes {
            polygons.push(h.clone());
        }
    }
    polygons
}

/// `to_expolygons(const Surfaces &)` and `to_expolygons(const SurfacesPtr &)`: each island, in
/// order.
#[must_use]
pub fn to_expolygons<'a, I>(src: I) -> ExPolygons
where
    I: IntoIterator<Item = &'a Surface>,
{
    src.into_iter().map(|s| s.expolygon.clone()).collect()
}

/// `to_expolygons(Surfaces &&)`: each island moved out, the list cleared.
#[must_use]
pub fn to_expolygons_owned(src: Surfaces) -> ExPolygons {
    src.into_iter().map(|s| s.expolygon).collect()
}

/// `number_polygons(const Surfaces &)` and `number_polygons(const SurfacesPtr &)`: the contours
/// and holes of the surfaces.
#[must_use]
pub fn number_polygons<'a, I>(surfaces: I) -> usize
where
    I: IntoIterator<Item = &'a Surface>,
{
    let mut n_polygons = 0usize;
    for s in surfaces {
        n_polygons += s.expolygon.holes.len() + 1;
    }
    n_polygons
}

/// `polygons_append(Polygons &dst, const Surfaces &src)` and its `SurfacesPtr` form: each
/// surface's contour, then its holes, after `dst`'s polygons.
pub fn polygons_append<'a, I>(dst: &mut Polygons, src: I)
where
    I: IntoIterator<Item = &'a Surface>,
    I::IntoIter: Clone,
{
    let it = src.into_iter();
    dst.reserve(number_polygons(it.clone()));
    for s in it {
        dst.push(s.expolygon.contour.clone());
        dst.extend(s.expolygon.holes.iter().cloned());
    }
}

/// `polygons_append(Polygons &dst, Surfaces &&src)`: as the copying form, the polygons moved.
pub fn polygons_append_owned(dst: &mut Polygons, src: Surfaces) {
    dst.reserve(number_polygons(&src));
    for s in src {
        let ExPolygon { contour, holes } = s.expolygon;
        dst.push(contour);
        dst.extend(holes);
    }
}

/// `surfaces_append(Surfaces &dst, const ExPolygons &src, SurfaceType)`: a surface of the type
/// for each island.
pub fn surfaces_append_typed(dst: &mut Surfaces, src: &[ExPolygon], surface_type: SurfaceType) {
    dst.reserve(src.len());
    for expoly in src {
        dst.push(Surface::with_expolygon(surface_type, expoly.clone()));
    }
}

/// `surfaces_append(Surfaces &dst, ExPolygons &&src, SurfaceType)`.
pub fn surfaces_append_typed_owned(dst: &mut Surfaces, src: ExPolygons, surface_type: SurfaceType) {
    dst.reserve(src.len());
    for expoly in src {
        dst.push(Surface::with_expolygon(surface_type, expoly));
    }
}

/// `surfaces_append(Surfaces &dst, const ExPolygons &src, const Surface &surfaceTempl)`: a copy of
/// the template for each island.
pub fn surfaces_append_templ(dst: &mut Surfaces, src: &[ExPolygon], surface_templ: &Surface) {
    // The C++ reserves room for number_polygons(src), the contours and holes, not the islands.
    dst.reserve(expolygon::number_polygons(src));
    for expoly in src {
        dst.push(Surface::from_template(surface_templ, expoly.clone()));
    }
}

/// `surfaces_append(Surfaces &dst, ExPolygons &&src, const Surface &surfaceTempl)`.
pub fn surfaces_append_templ_owned(dst: &mut Surfaces, src: ExPolygons, surface_templ: &Surface) {
    dst.reserve(expolygon::number_polygons(&src));
    for expoly in src {
        dst.push(Surface::from_template(surface_templ, expoly));
    }
}

/// `surfaces_append(Surfaces &dst, const Surfaces &src)`.
pub fn surfaces_append(dst: &mut Surfaces, src: &[Surface]) {
    dst.extend_from_slice(src);
}

/// `surfaces_append(Surfaces &dst, Surfaces &&src)`: `dst` takes `src` whole when empty.
pub fn surfaces_append_owned(dst: &mut Surfaces, src: Surfaces) {
    if dst.is_empty() {
        *dst = src;
    } else {
        dst.extend(src);
    }
}

/// `get_extents(const Surface &)`: the contour's box.
///
/// For a contour with no points the C++ box is indeterminate: `BoundingBoxBase(It from, It to)`
/// initialises no member and `construct` sets none for an empty range. The oracle's build leaves
/// min and max (0, 0) and `defined` true (LYSU record 1, an empty `stInternalVoid`), so that is
/// the box given here; orca-geom's `polygon::get_extents` gives an undefined box instead.
#[must_use]
pub fn get_extents(surface: &Surface) -> BoundingBox {
    if surface.expolygon.contour.points.is_empty() {
        let mut bbox = BoundingBox::new();
        bbox.defined = true;
        return bbox;
    }
    polygon::get_extents(&surface.expolygon.contour)
}

/// `get_extents(const Surfaces &)` and `get_extents(const SurfacesPtr &)`: the first surface's box
/// merged with the others'; undefined for no surfaces.
#[must_use]
pub fn get_extents_surfaces<'a, I>(surfaces: I) -> BoundingBox
where
    I: IntoIterator<Item = &'a Surface>,
{
    let mut it = surfaces.into_iter();
    let mut bbox = BoundingBox::new();
    if let Some(first) = it.next() {
        bbox = get_extents(first);
        for s in it {
            bbox.merge_box(&get_extents(s));
        }
    }
    bbox
}

/// `surfaces_could_merge(s1, s2)`: the same type, thickness, thickness in layers and bridge angle.
#[must_use]
#[allow(clippy::float_cmp)] // The C++ compares the doubles with ==.
pub fn surfaces_could_merge(s1: &Surface, s2: &Surface) -> bool {
    s1.surface_type == s2.surface_type
        && s1.thickness == s2.thickness
        && s1.thickness_layers == s2.thickness_layers
        && s1.bridge_angle == s2.bridge_angle
}

/// `surface_type_to_color_name(surface_type)`: the SVG colour of a type.
#[must_use]
pub fn surface_type_to_color_name(surface_type: SurfaceType) -> &'static str {
    surface_type_value_to_color_name(surface_type as i32)
}

/// `surface_type_to_color_name(SurfaceType(value))`, for any value the C++ casts to the enum
/// (Surface.cpp passes `SurfaceType(-1)`): `rgb(64,64,64)` for every type the switch does not
/// name, `stInternalAfterExternalBridge`, `stSecondInternalBridge` and `stCount` among them.
#[must_use]
pub fn surface_type_value_to_color_name(value: i32) -> &'static str {
    match SurfaceType::from_i32(value) {
        Some(SurfaceType::Top) => "rgb(255,0,0)",
        Some(SurfaceType::Bottom) => "rgb(0,255,0)",
        Some(SurfaceType::BottomBridge) => "rgb(0,0,255)",
        Some(SurfaceType::Internal) => "rgb(255,255,128)",
        Some(SurfaceType::InternalSolid) => "rgb(255,0,255)",
        Some(SurfaceType::InternalBridge) => "rgb(0,255,255)",
        Some(SurfaceType::InternalVoid) => "rgb(128,128,128)",
        Some(SurfaceType::Perimeter) => "rgb(128,0,0)",
        _ => "rgb(64,64,64)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orca_geom::point::Point;
    use orca_geom::polygon::Polygon;

    fn square(x: i64, y: i64, s: i64) -> ExPolygon {
        ExPolygon::from_points(vec![
            Point::new(x, y),
            Point::new(x + s, y),
            Point::new(x + s, y + s),
            Point::new(x, y + s),
        ])
    }

    #[test]
    fn a_new_surface_has_the_cpp_defaults() {
        let s = Surface::default();
        assert_eq!(s.surface_type, SurfaceType::Internal);
        assert!(s.empty());
        assert_eq!(s.thickness.to_bits(), (-1f64).to_bits());
        assert_eq!(s.thickness_layers, 1);
        assert_eq!(s.bridge_angle.to_bits(), (-1f64).to_bits());
        assert_eq!(s.extra_perimeters, 0);
    }

    #[test]
    fn the_predicates_ignore_perimeters() {
        let p = Surface::new(SurfaceType::Perimeter);
        assert!(p.is_internal() && !p.is_solid() && !p.is_external());
        let b = Surface::new(SurfaceType::InternalBridge);
        assert!(b.is_bridge() && b.is_internal_bridge() && b.is_solid() && b.is_internal());
        assert!(Surface::new(SurfaceType::BottomBridge).is_bottom());
    }

    #[test]
    fn appending_from_a_template_keeps_its_fields() {
        let mut t = Surface::new(SurfaceType::Top);
        t.thickness = 0.4;
        t.extra_perimeters = 2;
        let mut dst = Surfaces::new();
        surfaces_append_templ(&mut dst, &[square(0, 0, 10), square(20, 0, 5)], &t);
        assert_eq!(dst.len(), 2);
        assert!(dst.iter().all(|s| s.surface_type == SurfaceType::Top));
        assert_eq!(dst[1].extra_perimeters, 2);
        let mut hole = square(0, 0, 100);
        hole.holes.push(Polygon::new(vec![
            Point::new(10, 10),
            Point::new(10, 20),
            Point::new(20, 20),
        ]));
        dst.push(Surface::with_expolygon(SurfaceType::Bottom, hole));
        assert_eq!(number_polygons(&dst), 4);
        assert_eq!(to_polygons_surfaces(&dst).len(), 4);
        let bb = get_extents_surfaces(&dst);
        assert_eq!((bb.min, bb.max), (Point::new(0, 0), Point::new(100, 100)));
    }

    #[test]
    fn colour_names_default_past_the_switch() {
        assert_eq!(surface_type_value_to_color_name(-1), "rgb(64,64,64)");
        assert_eq!(
            surface_type_to_color_name(SurfaceType::Count),
            "rgb(64,64,64)"
        );
        assert_eq!(
            surface_type_to_color_name(SurfaceType::Perimeter),
            "rgb(128,0,0)"
        );
    }
}
